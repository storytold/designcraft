//! Application-owned panel ids, workspace defaults and JSON commands around the craft framework's docking.
//!
//! The window's panels and the canvas are one dock layout (`craft_behavior::dock`): a horizontal split of the
//! protected `canvas` panel and a dock at the logical end edge (the right in left-to-right languages, the left in
//! right-to-left ones). The framework's `DockArea` draws it. DesignCraft owns the panel registry, the workspace
//! defaults, its JSON docking commands ([`command`]) and what it saves: the layout in `UiState::docking` and the closed
//! and floated panels' places in `UiState::docking_hidden`, in the shapes earlier versions saved.

mod command;

use craft_behavior::Axis;
use craft_behavior::dock::{
    Action, Column, DockEdge, DockPlacement, Floating, Group, Layout, Location, MAX_DEPTH, MAX_PANELS, Node, PanelState, Permissions, SplitSize,
    Target, Workspace,
};
use craft_ui::contract::{Bind, BodyCx, PlainData, Tracked};
use craft_ui::docking::{DockConfig, DockEvent, DockPanels, DockSizes, FloatSpace, GroupAt, PanelSpec, PanelSpecs};
use craft_ui::patterns::dock::DockArea;
use egui::{Rect, pos2, vec2};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::sync::Arc;

use crate::DesignApp;
use command::{DockCommand, DockOperation, UiDock};

/// The name of the dock area that draws the panels; its root id comes from it.
const AREA: &str = "Panels";

/// The dock area a headless app floats into, before it ever drew a frame.
const HEADLESS_AREA: Rect = Rect::from_min_max(egui::Pos2::ZERO, pos2(1400.0, 900.0));

/// Where the context records that the craft UI runtime is installed on it.
fn runtime_key() -> egui::Id {
    egui::Id::new("designcraft-craft-ui-runtime")
}

/// Records that the craft UI runtime is installed on `ctx`, which the dock needs to draw.
pub(crate) fn note_runtime(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(runtime_key(), true));
}

/// Whether the craft UI runtime is installed on `ctx`.
pub(crate) fn runtime_on(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(runtime_key())).unwrap_or(false)
}

/// Where the last pass recorded what the area drew: its float space and the OS windows of its floating windows.
fn drawn_key() -> egui::Id {
    egui::Id::new("designcraft-dock-drawn")
}

/// What the dock drew in the last pass.
#[derive(Clone, Default)]
struct Drawn {
    space: Option<FloatSpace<String>>,
    viewports: Vec<egui::ViewportId>,
}

/// The viewports of the floating windows in OS windows of their own in the last pass. Where egui embeds viewports
/// every floating window stays in the main window.
pub(crate) fn native_viewports(_app: &DesignApp, ctx: &egui::Context) -> Vec<egui::ViewportId> {
    if ctx.embed_viewports() {
        return Vec::new();
    }
    ctx.data(|data| data.get_temp::<Drawn>(drawn_key())).map(|drawn| drawn.viewports).unwrap_or_default()
}

/// Ends a docking drag that is under way, without a drop.
pub(crate) fn cancel_gesture(ctx: &egui::Context) {
    craft_ui::runtime::drag::cancel(ctx, craft_behavior::CancelReason::Dismissed);
}

fn normalize(raw: &str) -> Option<&'static str> {
    crate::dock::DOCK_TABS
        .iter()
        .chain(crate::dock::ICON_PANELS)
        .find(|(id, label, _)| id.eq_ignore_ascii_case(raw.trim()) || label.eq_ignore_ascii_case(raw.trim()))
        .map(|(id, _, _)| *id)
}

/// The panel that holds the document area. It never moves, floats, closes or takes tabs.
pub(crate) const CANVAS: &str = "canvas";
/// Width of an expanded dock column.
const COLUMN_WIDTH: f32 = 260.0;
/// Width of an iconic column of bare icons, and of one whose groups show their names.
const ICON_WIDTH: f32 = 37.0;
const NAMED_ICON_WIDTH: f32 = 150.0;
/// Height of the group a workspace opens below Properties · Pages · Layers.
const OPEN_PANEL_HEIGHT: f32 = 260.0;

fn registered(id: &str) -> bool {
    id == CANVAS || normalize(id) == Some(id)
}

/// The policy commands and the renderer apply: the canvas is protected.
fn model_permissions(id: &String) -> Permissions {
    if id == CANVAS { Permissions::PROTECTED } else { Permissions::ALL }
}

/// The docked tree of `layout`, without its floating windows.
fn docked(layout: &Layout<String>) -> Layout<String> {
    Layout::new(layout.root.clone(), Vec::new())
}

pub(crate) fn valid(layout: &Layout<String>) -> bool {
    layout.validate().is_ok() && layout.panels().iter().all(|id| registered(id)) && docked(layout).contains(&CANVAS.to_string())
}

/// Panels already given a place, so each appears once.
#[derive(Default)]
struct Placed(Vec<String>);

impl Placed {
    /// The registered id of `raw` if no other place holds it yet.
    fn take(&mut self, raw: &str) -> Option<String> {
        let id = normalize(raw)?;
        if self.0.iter().any(|p| p == id) {
            return None;
        }
        self.0.push(id.to_string());
        Some(id.to_string())
    }

    /// A group of the unplaced registered panels of `panels`, with the one at `active` in front
    /// when it is among them.
    fn group(&mut self, panels: &[impl AsRef<str>], active: usize) -> Option<Group<String>> {
        let front = panels.get(active).and_then(|p| normalize(p.as_ref()));
        let kept: Vec<String> = panels.iter().filter_map(|p| self.take(p.as_ref())).collect();
        let active = front.and_then(|f| kept.iter().position(|p| p == f)).unwrap_or(0);
        (!kept.is_empty()).then(|| Group::new(kept, active))
    }
}

/// The iconic column's groups, top to bottom, in a workspace without its own `icon_groups`:
/// InDesign's panel families, with a separator only between families.
const ICON_GROUPS: &[&[&str]] = &[
    &["stroke", "color", "swatches", "gradient"],
    &["effects"],
    &["characterStyles", "paragraphStyles"],
    &["objectStyles"],
    &["character", "paragraph", "typography"],
    &["glyphs", "story"],
    &["table"],
    &["textWrap"],
    &["align", "pathfinder"],
    &["flexLayout", "liquid"],
    &["links", "library"],
    &["info"],
    &["attributes", "preflight", "separations"],
    &["notes", "trackChanges", "conditions"],
    &["hyperlinks", "bookmarks"],
    &["buttons", "states"],
    &["media", "transitions"],
    &["articles", "tags"],
    &["book", "dataMerge", "scripts"],
];

/// A dock column of `groups`, `COLUMN_WIDTH` wide when expanded, collapsed to icons `iconic_width` wide when `iconic`.
fn column(groups: Vec<Group<String>>, iconic: bool, iconic_width: f32) -> Column<String> {
    let mut column = Column::new(groups).with_width(COLUMN_WIDTH).with_iconic(iconic);
    column.iconic_width = Some(iconic_width);
    column
}

/// A workspace's iconic column of the panels not placed yet: its named `icon_groups`
/// (Essentials Classic, Typography), or [`ICON_GROUPS`] followed by any other dock panel as a
/// group of its own.
fn icon_column(icon_groups: Option<&[Vec<String>]>, placed: &mut Placed) -> Option<Column<String>> {
    let (groups, width): (Vec<Group<String>>, f32) = match icon_groups {
        Some(groups) => (groups.iter().filter_map(|g| placed.group(g, 0)).collect(), NAMED_ICON_WIDTH),
        None => {
            let mut groups: Vec<Group<String>> = ICON_GROUPS.iter().filter_map(|g| placed.group(g, 0)).collect();
            groups.extend(
                crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS).filter_map(|p| placed.take(p.0)).map(|id| Group::new(vec![id], 0)),
            );
            (groups, ICON_WIDTH)
        }
    };
    (!groups.is_empty()).then(|| column(groups, true, width))
}

/// The width a dock column takes beside the canvas.
fn column_width(c: &Column<String>) -> f32 {
    if c.iconic { c.iconic_width.unwrap_or(ICON_WIDTH) } else { c.width.unwrap_or(COLUMN_WIDTH) }
}

/// The canvas beside a dock of `columns` (from the window edge inwards) at the logical end edge.
fn with_canvas(columns: Vec<Column<String>>, floating: Vec<Floating<String>>) -> Layout<String> {
    let width: f32 = columns.iter().map(column_width).sum();
    let root = Node::split(Axis::X, SplitSize::FixedSecond(width), Node::tabs(vec![CANVAS.to_string()], 0), Node::dock(DockEdge::End, columns));
    Layout::new(Some(root), floating)
}

/// The layout of a workspace that has not been customized, from its panel settings: the
/// canvas beside a dock whose outer column is the Properties · Pages · Layers group (with
/// the panel the workspace opens stacked below it) and whose inner column is iconic. Essentials Classic
/// and Typography (`icon_groups`) have the iconic column only.
pub(crate) fn default_layout(ui: &crate::UiState) -> Layout<String> {
    let mut placed = Placed::default();
    let mut floating = Vec::new();
    for (id, at) in &ui.floating {
        if at.iter().all(|v| v.is_finite())
            && let Some(id) = placed.take(id)
        {
            // The layout is rebuilt every frame, so each window keeps an id of its panel's (a
            // native window lives as long as its id).
            let window = default_window_id(&id);
            floating.push(Floating::single(window, vec![id], 0, Rect::from_min_size(pos2(at[0], at[1]), vec2(280.0, 360.0))));
        }
    }
    let mut columns = Vec::new();
    if ui.icon_groups.is_none() {
        let panels: Vec<String> = crate::dock::DOCK_TABS.iter().filter_map(|p| placed.take(p.0)).collect();
        let active = panels.iter().position(|p| p == &ui.dock_tab).unwrap_or(0);
        let mut groups = Vec::new();
        if !panels.is_empty() {
            groups.push(Group::new(panels, active));
        }
        // The panel a workspace opens (Printing and Proofing: Swatches) goes below at a fixed
        // height, so Properties keeps most of the column.
        if let Some(id) = ui.open_panel.as_deref().and_then(|id| placed.take(id)) {
            let mut group = Group::new(vec![id], 0);
            group.height = Some(OPEN_PANEL_HEIGHT);
            groups.push(group);
        }
        if !groups.is_empty() {
            columns.push(column(groups, !ui.dock_expanded, ICON_WIDTH));
        }
    }
    columns.extend(icon_column(ui.icon_groups.as_deref(), &mut placed));
    with_canvas(columns, floating)
}

/// The window id of a default layout's floating `panel`: the same every time, with the top bit
/// set, where fresh window ids never reach.
fn default_window_id(panel: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ("designcraft-default-float", panel).hash(&mut h);
    h.finish() | 1 << 63
}

/// Is `layout` from before the canvas joined the panel layout? Those layouts held only the
/// right dock's panels and floating windows; every later one holds the canvas.
pub(crate) fn dock_only(layout: &Layout<String>) -> bool {
    !layout.panels().iter().any(|p| p.as_str() == CANVAS)
}

/// A [`dock_only`] layout in the current shape: its tab groups, accordions and splits become
/// the dock's expanded columns (collapsed to icons when the dock was), the panels it does not
/// hold form the iconic column, and its floating windows stay. Panels closed with a remembered
/// place (`hidden`) stay closed. None when the layout is invalid.
pub(crate) fn from_dock_only<'a>(
    old: &Layout<String>,
    hidden: impl IntoIterator<Item = &'a String>,
    dock_expanded: bool,
    icon_groups: Option<&[Vec<String>]>,
) -> Option<Layout<String>> {
    old.validate().ok()?;
    let mut placed = Placed::default();
    // Reading order (start to end); a dock lists its columns from the window edge inwards.
    let mut reading = old.root.as_ref().map(|root| dock_only_columns(root, 0, &mut placed)).unwrap_or_default();
    reading.reverse();
    let mut columns: Vec<Column<String>> = reading.into_iter().map(|groups| column(groups, !dock_expanded, ICON_WIDTH)).collect();
    let mut floating = Vec::new();
    for window in &old.floating {
        let groups: Vec<Group<String>> = window
            .groups()
            .iter()
            .filter_map(|g| {
                placed.group(&g.panels, g.active).map(|mut kept| {
                    kept.minimized = g.minimized;
                    kept.height = g.height;
                    kept
                })
            })
            .collect();
        if !groups.is_empty() {
            let mut kept = window.column.clone();
            kept.groups = groups;
            floating.push(Floating::new(window.id, kept, window.rect));
        }
    }
    for panel in hidden {
        let _ = placed.take(panel);
    }
    columns.extend(icon_column(icon_groups, &mut placed));
    let layout = with_canvas(columns, floating);
    valid(&layout).then_some(layout)
}

/// The groups of a floating window, first to last.
pub(crate) fn window_groups(window: &Floating<String>) -> Vec<Group<String>> {
    window.groups().to_vec()
}

/// The groups of a dock-only layout's node, column by column in reading order. A vertical split
/// stacks its parts in one column; a horizontal split puts them side by side.
fn dock_only_columns(node: &Node<String>, depth: usize, placed: &mut Placed) -> Vec<Vec<Group<String>>> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    let column = |groups: Vec<Group<String>>| if groups.is_empty() { Vec::new() } else { vec![groups] };
    match node {
        Node::Tabs { panels, active, .. } => column(placed.group(panels, *active).into_iter().collect()),
        Node::Stack { entries, .. } => column(
            entries
                .iter()
                .filter_map(|e| {
                    placed.take(&e.panel).map(|id| {
                        let mut group = Group::new(vec![id], 0);
                        group.minimized = !e.open;
                        group.height = e.height;
                        group
                    })
                })
                .collect(),
        ),
        Node::Split { axis, first, second, .. } => {
            let mut parts = dock_only_columns(first, depth.saturating_add(1), placed);
            parts.extend(dock_only_columns(second, depth.saturating_add(1), placed));
            match axis {
                Axis::X => parts,
                _ => column(parts.into_iter().flatten().collect()),
            }
        }
        Node::Dock { columns, .. } => columns
            .iter()
            .rev()
            .filter_map(|c| {
                let groups: Vec<Group<String>> = c.groups.iter().filter_map(|g| placed.group(&g.panels, g.active)).collect();
                (!groups.is_empty()).then_some(groups)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The layout in use: the customized one, or the workspace's default.
fn layout_of(app: &DesignApp) -> Layout<String> {
    app.ui.docking.clone().unwrap_or_else(|| default_layout(&app.ui))
}

/// Is `panel` showing: in front of its group and not in a column collapsed to icons, or in
/// the open flyout of an iconic column?
pub(crate) fn showing(app: &DesignApp, panel: &str) -> bool {
    let layout = layout_of(app);
    layout.panel_state(&panel.to_string()) == PanelState::Front
        || (app.ui.docking_flyout.as_deref() == Some(panel) && in_iconic_column(&layout, panel))
}

/// Is `panel` in a dock column collapsed to icons?
fn in_iconic_column(layout: &Layout<String>, panel: &str) -> bool {
    dock_columns(layout).iter().any(|c| c.iconic && c.groups.iter().any(|g| g.panels.iter().any(|p| p == panel)))
}

/// Every dock column, from each dock's window edge inwards.
fn dock_columns(layout: &Layout<String>) -> Vec<&Column<String>> {
    let mut out = Vec::new();
    let mut visit: Vec<&Node<String>> = layout.root.iter().collect();
    let mut seen = 0usize;
    while let Some(node) = visit.pop() {
        seen = seen.saturating_add(1);
        if seen > 2 * MAX_PANELS {
            break;
        }
        match node {
            Node::Split { first, second, .. } => {
                visit.push(second);
                visit.push(first);
            }
            Node::Dock { columns, .. } => out.extend(columns),
            _ => {}
        }
    }
    out
}

/// The panels tabbed with `panel`: its floating group, or its tab group in the dock. A panel in
/// an accordion, or not in the layout, is a group of its own.
#[cfg(test)]
pub(crate) fn tab_group(layout: &Layout<String>, panel: &str) -> Vec<String> {
    if let Some(group) = layout.floating.iter().flat_map(window_groups).find(|g| g.panels.iter().any(|p| p == panel)) {
        return group.panels.clone();
    }
    let mut visit: Vec<&Node<String>> = layout.root.iter().collect();
    let mut seen = 0usize;
    while let Some(node) = visit.pop() {
        seen = seen.saturating_add(1);
        if seen > 2 * MAX_PANELS {
            break;
        }
        match node {
            Node::Split { first, second, .. } => {
                visit.push(first);
                visit.push(second);
            }
            Node::Tabs { panels, .. } if panels.iter().any(|p| p == panel) => return panels.clone(),
            Node::Dock { columns, .. } => {
                if let Some(group) = columns.iter().flat_map(|c| &c.groups).find(|g| g.panels.iter().any(|p| p == panel)) {
                    return group.panels.clone();
                }
            }
            _ => {}
        }
    }
    vec![panel.to_string()]
}

/// Where a panel is: its floating window and group, or the path to its tab group, accordion or
/// dock in the tree with the dock column and group. A different place means the panel moved.
#[derive(Debug, PartialEq)]
enum Place {
    Floating { window: usize, group: usize },
    Tree { path: Vec<bool>, column: usize, group: usize },
}

fn place(layout: &Layout<String>, panel: &str) -> Option<Place> {
    let has = |group: &Group<String>| group.panels.iter().any(|p| p == panel);
    for (window, floating) in layout.floating.iter().enumerate() {
        if let Some(group) = floating.groups().iter().position(has) {
            return Some(Place::Floating { window, group });
        }
    }
    let mut visit: Vec<(&Node<String>, Vec<bool>)> = layout.root.iter().map(|node| (node, Vec::new())).collect();
    let mut seen = 0usize;
    while let Some((node, path)) = visit.pop() {
        seen = seen.saturating_add(1);
        if seen > 2 * MAX_PANELS || path.len() > MAX_DEPTH {
            return None;
        }
        match node {
            Node::Split { first, second, .. } => {
                let mut second_path = path.clone();
                second_path.push(true);
                let mut first_path = path;
                first_path.push(false);
                visit.push((second, second_path));
                visit.push((first, first_path));
            }
            Node::Tabs { panels, .. } if panels.iter().any(|p| p == panel) => return Some(Place::Tree { path, column: 0, group: 0 }),
            Node::Stack { entries, .. } if entries.iter().any(|e| e.panel == panel) => return Some(Place::Tree { path, column: 0, group: 0 }),
            Node::Dock { columns, .. } => {
                for (column, c) in columns.iter().enumerate() {
                    if let Some(group) = c.groups.iter().position(has) {
                        return Some(Place::Tree { path, column, group });
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Where a panel with no other destination goes: a panel of the first expanded dock column,
/// else of any column, else of the root.
fn first_dock_panel(layout: &Layout<String>, except: &str) -> Option<String> {
    let usable = |p: &&String| p.as_str() != except && p.as_str() != CANVAS;
    let columns = dock_columns(layout);
    let in_columns =
        |iconic: bool| columns.iter().filter(|c| c.iconic == iconic).flat_map(|c| &c.groups).flat_map(|g| &g.panels).find(usable).cloned();
    in_columns(false).or_else(|| in_columns(true)).or_else(|| docked(layout).panels().into_iter().find(usable).cloned())
}

fn apply(layout: &mut Layout<String>, action: Action<String>) -> Result<Vec<String>, String> {
    layout.apply_with_permissions(action, model_permissions).map_err(|e| e.to_string())
}

fn number(params: &Value, key: &str, default: f32) -> Result<f32, String> {
    match params.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|v| v.is_finite() && v.abs() <= 1_000_000.0)
            .map(|v| v as f32)
            .ok_or_else(|| format!("{key} must be a finite coordinate")),
    }
}

/// The shared docking command a DesignCraft command id names. `window.floatPanel` and
/// `window.dockPanel` are DesignCraft's names for Float Panel and Dock Panel. `window.workspace`
/// stays DesignCraft's own command (same id and params), with the other workspace commands.
fn shared_id(id: &str) -> Option<&'static str> {
    match id {
        "window.floatPanel" => Some("window.panel.float"),
        "window.dockPanel" => Some("window.panel.dock"),
        "window.workspace" => None,
        _ => DockCommand::<String>::IDS.iter().copied().find(|known| *known == id),
    }
}

pub(crate) fn command(app: &mut DesignApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id == "app.tablePanel" {
        // A Table flyout closes; the panel stays in its icon group.
        if flyout_open(app, "table") {
            return Some(Ok(close_flyout(app)));
        }
        let id = if app.ui.hidden_panels == 0 && showing(app, "table") { "window.panel.close" } else { "window.panel.activate" };
        return Some(shared_command(app, id, &json!({"panel": "table"})));
    }
    if id == "window.collapseToIcons" {
        return Some(set_iconic(app, true));
    }
    if matches!(id, "window.floatPanel" | "window.dockPanel" | "window.panel.float" | "window.panel.dock")
        && let Some(panel) = params.get("panel").and_then(Value::as_str).filter(|p| crate::dock::float_only(p))
    {
        return Some(float_only_panel(app, id, panel, params));
    }
    if id == "window.panel" {
        if let Some(result) = open_floating_group(app, params) {
            return Some(result);
        }
        // Float-only panels (Type › Tabs) are not in the layout.
        let panel = params.get("panel").and_then(Value::as_str).and_then(normalize)?;
        // Choosing a panel whose flyout is open closes the flyout, as the panel's icon does.
        if flyout_open(app, panel) {
            return Some(Ok(close_flyout(app)));
        }
        return Some(shared_command(app, "window.panel.activate", params));
    }
    let id = shared_id(id)?;
    Some(shared_command(app, id, params))
}

/// Is `panel` showing in the open flyout of an iconic column?
fn flyout_open(app: &DesignApp, panel: &str) -> bool {
    app.ui.hidden_panels == 0 && app.ui.docking_flyout.as_deref() == Some(panel) && in_iconic_column(&layout_of(app), panel)
}

/// Close the open flyout; the layout is unchanged. The dock area shows the flyout the app holds.
fn close_flyout(app: &mut DesignApp) -> Value {
    app.ui.docking_flyout = None;
    json!({"layout": layout_of(app), "closed": [], "floating": floating_panels(app)})
}

/// Command params with DesignCraft's panel names resolved (a panel id or English name, in any
/// case) and `onto` read as `anchor`.
fn resolve_panels(params: &Value) -> Result<Value, String> {
    let mut params = if params.is_null() { json!({}) } else { params.clone() };
    let fields = params.as_object_mut().ok_or("params must be an object")?;
    if let Some(onto) = fields.remove("onto") {
        fields.entry("anchor").or_insert(onto);
    }
    for key in ["panel", "anchor", "target", "before"] {
        let Some(value) = fields.get_mut(key) else { continue };
        if key == "before" && value.is_null() {
            continue;
        }
        let id = value.as_str().and_then(normalize).ok_or_else(|| format!("{key} must name a registered panel"))?;
        *value = Value::String(id.to_owned());
    }
    Ok(params)
}

/// A shared docking command (`window.panel.*`, `window.workspace.*`, `ui.dock`) on the layout in
/// use, all or nothing. [`command::DockCommand`] parses it and makes the layout actions;
/// DesignCraft resolves panel names, shows a hidden panel before moving, floating or docking
/// it.
fn shared_command(app: &mut DesignApp, id: &str, params: &Value) -> Result<Value, String> {
    let mut params = resolve_panels(params)?;
    if let Some(fields) = params.as_object_mut() {
        fields.insert("command".into(), Value::String(id.to_owned()));
    }
    let command: DockCommand<String> = serde_json::from_value(params).map_err(|e| e.to_string())?;
    match &command {
        DockCommand::Workspace { name } => return app.run("window.workspace", json!({"name": name})),
        DockCommand::NewWorkspace { name } => return app.run("window.newWorkspace", json!({"name": name})),
        DockCommand::DeleteWorkspace { name } => return app.run("window.deleteWorkspace", json!({"name": name})),
        DockCommand::ResetWorkspace {} => return app.run("window.resetWorkspace", json!({})),
        DockCommand::Lock { .. } => return Err("Lock Workspace is not available in DesignCraft".into()),
        DockCommand::Window { .. } => {
            return Err(
                "Move Panel to New Window is not available: a floating panel has its own window on the desktop and stays in the page on the web"
                    .into(),
            );
        }
        _ => {}
    }
    // A zone is a side of `anchor`. DesignCraft's docks are columns beside the canvas, so a dock at
    // a window edge is a `side`, not a frame on the outer edge (`DockPlacement::Edge`).
    if let DockCommand::Dock { anchor: None, side: None, zone: Some(_), .. } = &command {
        return Err("zone needs an anchor: dock beside a panel with anchor, or in a new dock column with side (Start or End)".into());
    }
    let rtl = crate::i18n::is_rtl(&app.ui.language);
    let space = float_space(app);
    let mut txn = Transaction::begin(app)?;
    let shows = match &command {
        DockCommand::Activate { panel }
        | DockCommand::Move { panel, .. }
        | DockCommand::Float { panel, .. }
        | DockCommand::Dock { panel, .. }
        | DockCommand::UiDock(UiDock::Operation(DockOperation::Open { panel } | DockOperation::Activate { panel })) => Some(panel.clone()),
        _ => None,
    };
    let mut shown = false;
    if let Some(panel) = shows.as_ref().filter(|p| !txn.workspace.layout().contains(p)) {
        let fallback = first_dock_panel(txn.workspace.layout(), panel);
        txn.workspace.set_visible(panel.clone(), true, fallback, false, model_permissions).map_err(|e| e.to_string())?;
        shown = true;
    }
    // Dock Panel with no destination returns a group that floated as one to its dock place as a whole; a floating
    // panel goes back to the place it left (the command layer reads the workspace's floated records), else into the
    // first dock group.
    let returned = match &command {
        DockCommand::Dock { panel, anchor: None, side: None, column: false, .. } if !shown => txn.dock_group_back(panel)?,
        _ => false,
    };
    let actions = match &command {
        // Showing it, or returning its group, was the whole command.
        DockCommand::Dock { anchor: None, side: None, .. } if shown || returned => Vec::new(),
        _ => command.to_actions(&txn.workspace, rtl, &space, &model_permissions).map_err(|e| e.to_string())?,
    };
    let reveal = match &command {
        DockCommand::Layout { action } | DockCommand::UiDock(UiDock::Action { action }) => match action {
            Action::Activate { panel, .. } | Action::Open { panel, .. } | Action::OpenAt { panel, .. } => Some(panel.clone()),
            _ => None,
        },
        DockCommand::UiDock(UiDock::Operation(
            DockOperation::Float { panel, .. } | DockOperation::MoveFloating { panel, .. } | DockOperation::Move { panel, .. },
        )) => Some(panel.clone()),
        _ => shows,
    };
    // A panel or group floated out of a column collapsed to icons floats expanded, as InDesign's do; the framework
    // keeps the column's collapsed state on the window it makes.
    let from_icons = match &command {
        DockCommand::Float { panel, .. } => in_iconic_column(txn.workspace.layout(), panel).then(|| panel.clone()),
        _ => None,
    };
    let mut closed = Vec::new();
    for action in actions {
        closed.extend(txn.apply(action)?);
    }
    if let Some(panel) = from_icons
        && txn.workspace.layout().floating.iter().any(|w| w.contains(&panel) && w.column.iconic)
    {
        closed.extend(txn.apply(Action::set_iconic(panel, false))?);
    }
    Ok(txn.commit(app, closed, reveal))
}

/// Where floating windows may go: the window, below the title-bar strip when DesignCraft draws
/// its own title bar under macOS's transparent one. A floating window in that strip would take
/// presses that move the whole app window.
fn float_bounds(app: &DesignApp, ctx: &egui::Context) -> Rect {
    let mut r = ctx.content_rect();
    if app.integrated_titlebar {
        r.min.y = (r.min.y + crate::chrome::MAC_APP_BAR).min(r.max.y);
    }
    r
}

/// Where a command floats a panel, group or column it gives no rect: inside the dock area as
/// last drawn, else inside the window, else (headless, before any frame) inside
/// [`HEADLESS_AREA`]. Seeded with every panel's own size, so a panel the area never drew (shown
/// from hidden by the same command, or headless) floats at that size too.
fn float_space(app: &DesignApp) -> FloatSpace<String> {
    let space = match app.egui_ctx.as_ref() {
        Some(ctx) => ctx
            .data(|data| data.get_temp::<Drawn>(drawn_key()))
            .and_then(|drawn| drawn.space)
            .unwrap_or_else(|| FloatSpace::new(float_bounds(app, ctx), &DockSizes::of(ctx))),
        None => FloatSpace::new(HEADLESS_AREA, &DockSizes::of(&egui::Context::default())),
    };
    crate::dock::DOCK_TABS
        .iter()
        .chain(crate::dock::ICON_PANELS)
        .fold(space, |space, (id, _, _)| space.with_panel_size((*id).to_owned(), floating_size(id)))
}

/// A change to the layout in use, committed only when every step succeeded.
struct Transaction {
    /// The layout, the closed panels with where each one goes back, and the floating panels with the dock place each
    /// one left, for Dock Panel.
    workspace: Workspace<String>,
}

impl Transaction {
    fn begin(app: &DesignApp) -> Result<Self, String> {
        if app.ui.docking.as_ref().is_some_and(|layout| !valid(layout)) {
            return Err("saved panel layout is invalid; reset the workspace first".into());
        }
        let layout = layout_of(app);
        let (mut hidden, mut floated) = (Vec::new(), Vec::new());
        for (panel, location) in app.ui.docking_hidden.0.iter().filter(|(id, _)| normalize(id) == Some(id.as_str())).take(MAX_PANELS) {
            if !layout.contains(panel) {
                hidden.push((panel.clone(), location.clone()));
            } else if layout.floating.iter().any(|w| w.contains(panel)) {
                floated.push((panel.clone(), location.clone()));
            }
        }
        Ok(Self { workspace: Workspace::from_parts(layout, hidden, floated) })
    }

    /// One action from a command or the dock area, for registered panels only. Floating a docked group as a whole
    /// records the group's own place for each of its panels, so Dock Panel brings the group back as one.
    fn apply(&mut self, action: Action<String>) -> Result<Vec<String>, String> {
        if !known(&action) {
            return Err("docking action contains an unknown panel or an excessive split path".into());
        }
        let group_place = match &action {
            Action::MoveGroup { panel, to: Target::Float { .. }, .. } => docked_group_place(self.workspace.layout(), panel),
            _ => None,
        };
        let closed = self.workspace.apply(action, false, model_permissions).map_err(|e| e.to_string())?;
        if let Some((members, location)) = group_place {
            let mut floated: Vec<(String, Location<String>)> =
                self.workspace.floated().iter().filter(|(p, _)| !members.contains(p)).cloned().collect();
            floated.extend(members.into_iter().map(|member| (member, location.clone())));
            let (layout, hidden) = (self.workspace.layout().clone(), self.workspace.hidden().to_vec());
            self.workspace = Workspace::from_parts(layout, hidden, floated);
        }
        Ok(closed)
    }

    /// Dock Panel for a panel of a floating group that floated from the dock as one: the whole group back to that
    /// place, in the same column beside the same neighbour, with its group and column state, while that neighbour is
    /// still docked. True when it went back.
    fn dock_group_back(&mut self, panel: &String) -> Result<bool, String> {
        let layout = self.workspace.layout();
        let Some(group) = layout.floating.iter().flat_map(window_groups).find(|g| g.panels.contains(panel)) else {
            return Ok(false);
        };
        let place = |p: &String| self.workspace.floated().iter().find(|(q, _)| q == p).map(|(_, location)| location.clone());
        let Some(location) = place(panel) else { return Ok(false) };
        // A group of one docks back as a panel. Every member keeps the same group place only while the group is the
        // one that floated.
        if group.panels.len() < 2 || !group.panels.iter().all(|p| place(p).as_ref() == Some(&location)) {
            return Ok(false);
        }
        let Some(anchor) = location.anchor.clone().filter(|anchor| docked(layout).contains(anchor)) else {
            return Ok(false);
        };
        if !matches!(location.placement, DockPlacement::Group { .. } | DockPlacement::Column { .. }) {
            return Ok(false);
        }
        let mut actions = vec![Action::move_group(panel.clone(), Target::at(anchor, location.placement.clone()))];
        if let Some(saved) = &location.group {
            actions.push(Action::resize_group(panel.clone(), saved.height));
            actions.push(Action::set_minimized(panel.clone(), saved.minimized));
            actions.push(Action::resize_flyout_width(panel.clone(), saved.flyout_width));
            actions.push(Action::move_flyout_top(panel.clone(), saved.flyout_offset));
            // A group that had a column of its own gets that column back as it was.
            if let (DockPlacement::Column { .. }, Some(column)) = (&location.placement, saved.column) {
                if let Some(width) = column.width {
                    actions.push(Action::resize_column(panel.clone(), width));
                }
                actions.push(Action::set_iconic(panel.clone(), column.iconic));
            }
        }
        let mut candidate = self.workspace.clone();
        if candidate.apply_all(actions, false, model_permissions).is_err() {
            return Ok(false);
        }
        self.workspace = candidate;
        Ok(true)
    }

    /// Store the result. `reveal` is the panel the command showed, raised (or its flyout opened)
    /// on the next frame.
    fn commit(self, app: &mut DesignApp, closed: Vec<String>, reveal: Option<String>) -> Value {
        let layout = self.workspace.layout().clone();
        let floating = |p: &String| layout.floating.iter().any(|w| w.contains(p));
        let mut hidden = crate::HiddenPanels(self.workspace.hidden().to_vec());
        for (panel, location) in self.workspace.floated() {
            if floating(panel) && !hidden.contains_key(panel) {
                hidden.0.push((panel.clone(), location.clone()));
            }
        }
        let reveal = reveal.filter(|panel| layout.contains(panel));
        let flyout = reveal.as_ref().filter(|panel| in_iconic_column(&layout, panel)).cloned();
        // Focus stays on a panel only while it stays where the user left it: a command that moves it gives the focus
        // up.
        let before = layout_of(app);
        if app.ui.docking_focused.as_deref().is_some_and(|panel| panel != CANVAS && place(&before, panel) != place(&layout, panel)) {
            app.ui.docking_focused = None;
        }
        app.ui.docking = Some(layout);
        app.ui.docking_hidden = hidden;
        release_closed(app, &closed);
        keep_float_only(app);
        if let Some(panel) = reveal {
            app.ui.hidden_panels = 0;
            app.ui.docking_opened = Some(panel.clone());
            app.ui.docking_raise = Some(panel);
            app.ui.docking_flyout = flyout;
        } else if app.ui.docking_opened.as_ref().is_some_and(|panel| !app.ui.docking.as_ref().is_some_and(|layout| layout.contains(panel))) {
            app.ui.docking_opened = None;
        }
        release_flyout(app);
        app.ui.open_panel = None;
        json!({"layout": app.ui.docking, "closed": closed, "floating": floating_panels(app)})
    }
}

/// The panels of the docked group holding `panel`, and that group's place as a whole: beside the group or column
/// next to it, with its group and column state. None when `panel` is not in a dock column.
fn docked_group_place(layout: &Layout<String>, panel: &String) -> Option<(Vec<String>, Location<String>)> {
    let members = dock_columns(layout).into_iter().flat_map(|c| &c.groups).find(|g| g.panels.contains(panel))?.panels.clone();
    // The place of the group's last panel left in it is the group's own place.
    let mut alone = layout.clone();
    for other in members.iter().filter(|p| *p != panel) {
        apply(&mut alone, Action::close(other.clone())).ok()?;
    }
    let location = alone.location(panel).ok().filter(|l| l.floating.is_none())?;
    Some((members, location))
}

/// Whether every panel an action names is registered (and a split path is not excessive).
/// The application owns the registry; the framework owns tree validation and structural changes.
fn known(action: &Action<String>) -> bool {
    let registered = |id: &String| self::registered(id);
    let tab = |placement: &DockPlacement<String>| match placement {
        DockPlacement::Tab { before: Some(id), .. } => registered(id),
        _ => true,
    };
    let target = |to: &Target<String>| match to {
        Target::At { anchor, placement, .. } => registered(anchor) && tab(placement),
        _ => true,
    };
    match action {
        Action::Move { panel, anchor, placement, .. } | Action::OpenAt { panel, anchor, placement, .. } => {
            registered(panel) && registered(anchor) && tab(placement)
        }
        Action::Open { panel, anchor, .. } => registered(panel) && anchor.as_ref().is_none_or(registered),
        Action::ResizeSplit { path, .. } => path.len() <= MAX_DEPTH,
        Action::MovePanel { panel, to, .. }
        | Action::MoveGroup { panel, to, .. }
        | Action::MoveColumn { panel, to, .. }
        | Action::MoveWindow { panel, to, .. } => registered(panel) && target(to),
        Action::Float { panel, .. }
        | Action::Close { panel, .. }
        | Action::Activate { panel, .. }
        | Action::MoveFloating { panel, .. }
        | Action::SetStackOpen { panel, .. }
        | Action::ResizeStack { panel, .. }
        | Action::StepGroup { panel, .. }
        | Action::CloseGroup { panel, .. }
        | Action::CloseWindow { panel, .. }
        | Action::CloseOthers { panel, .. }
        | Action::SetMinimized { panel, .. }
        | Action::SetIconic { panel, .. }
        | Action::ResizeColumn { panel, .. }
        | Action::SetColumnSteps { panel, .. }
        | Action::ResizeGroup { panel, .. }
        | Action::ResizeIconic { panel, .. }
        | Action::ResizeFlyoutWidth { panel, .. }
        | Action::MoveFlyoutTop { panel, .. }
        | Action::SetNative { panel, .. } => registered(panel),
        // Actions newer than this list are rejected until DesignCraft checks them.
        _ => false,
    }
}

/// Type › Tabs and the other float-only panels: their own windows outside the docking layout,
/// listed in `UiState::floating` with their top-left positions.
fn float_only_panel(app: &mut DesignApp, id: &str, panel: &str, params: &Value) -> Result<Value, String> {
    let at = if matches!(id, "window.floatPanel" | "window.panel.float") {
        Some([number(params, "x", 400.0)?, number(params, "y", 160.0)?])
    } else {
        None
    };
    app.ui.floating.retain(|(p, _)| p != panel);
    if let Some(at) = at {
        app.ui.hidden_panels = 0;
        app.ui.floating.push((panel.to_string(), at));
    }
    Ok(json!({"panel": panel, "floating": floating_panels(app)}))
}

/// Once the layout is customized it holds every floating dock panel, so `UiState::floating`
/// keeps only the float-only panels.
fn keep_float_only(app: &mut DesignApp) {
    if app.ui.docking.is_some() {
        app.ui.floating.retain(|(id, _)| crate::dock::float_only(id));
    }
}

/// Every floating panel: the layout's floating windows, then the float-only panels.
pub(crate) fn floating_panels(app: &DesignApp) -> Vec<String> {
    let layout = layout_of(app);
    let docked = layout.floating.iter().flat_map(|w| w.panels().cloned());
    docked.chain(app.ui.floating.iter().filter(|(id, _)| crate::dock::float_only(id)).map(|(id, _)| id.clone())).collect()
}

/// Panels InDesign opens together as one floating group from the Window menu, in tab order:
/// Window › Styles › Paragraph Styles (or Character Styles) brings both.
const FLOATING_GROUPS: &[&[&str]] = &[&["characterStyles", "paragraphStyles"]];

/// Window › a panel of a [`FLOATING_GROUPS`] group none of whose panels is showing: the group
/// opens as one floating window over the right of the canvas, with that panel in front. A group
/// that is an icon group of the default workspaces floats out of the iconic column. None
/// otherwise (the panel opens as before).
fn open_floating_group(app: &mut DesignApp, params: &Value) -> Option<Result<Value, String>> {
    let panel = params.get("panel").and_then(Value::as_str).and_then(normalize)?;
    let group = FLOATING_GROUPS.iter().find(|g| g.contains(&panel))?;
    // Panels in the workspace's icon groups open from there.
    if app.ui.icon_groups.as_ref().is_some_and(|g| g.iter().flatten().any(|p| group.contains(&p.as_str()))) {
        return None;
    }
    // A panel closed from a place in the dock comes back there.
    if group.iter().any(|p| app.ui.docking_hidden.get(p).is_some_and(|l| l.floating.is_none())) {
        return None;
    }
    let layout = layout_of(app);
    let (w, h) = (290.0, 340.0);
    let (x, y) = app.canvas_rect.map_or((400.0, 140.0), |r| ((r.right() - w - 24.0).max(r.left()), r.top() + 96.0));
    let icon_group = dock_columns(&layout)
        .iter()
        .filter(|c| c.iconic)
        .flat_map(|c| &c.groups)
        .any(|g| g.panels.len() == group.len() && g.panels.iter().all(|p| group.contains(&p.as_str())));
    if icon_group && !group.iter().any(|p| showing(app, p)) {
        let float = json!({"panel": panel, "group": true, "x": x, "y": y, "width": w, "height": h});
        return Some(
            shared_command(app, "window.panel.float", &float).and_then(|_| shared_command(app, "window.panel.activate", &json!({"panel": panel}))),
        );
    }
    if group.iter().any(|p| layout.contains(&p.to_string())) {
        return None;
    }
    let open = |app: &mut DesignApp| -> Result<Value, String> {
        let txn = Transaction::begin(app)?;
        let panels: Vec<String> = group.iter().map(|p| (*p).to_string()).collect();
        let active = group.iter().position(|p| *p == panel).unwrap_or(0);
        let mut layout = txn.workspace.layout().clone();
        let id = layout.fresh_window_id();
        layout.floating.push(Floating::single(id, panels, active, Rect::from_min_size(pos2(x, y), vec2(w, h))));
        layout.validate().map_err(|e| e.to_string())?;
        let hidden: Vec<(String, Location<String>)> = txn.workspace.hidden().iter().filter(|(p, _)| !group.contains(&p.as_str())).cloned().collect();
        let floated = txn.workspace.floated().to_vec();
        let txn = Transaction { workspace: Workspace::from_parts(layout, hidden, floated) };
        Ok(txn.commit(app, Vec::new(), Some(panel.to_string())))
    };
    Some(open(app))
}

/// Forget what DesignCraft keeps about panels that just closed: the focus, the panel last
/// opened (for `ui.set closePanel`) and a pending raise.
fn release_closed(app: &mut DesignApp, closed: &[String]) {
    for slot in [&mut app.ui.docking_focused, &mut app.ui.docking_opened, &mut app.ui.docking_raise, &mut app.ui.docking_flyout] {
        if slot.as_ref().is_some_and(|panel| closed.contains(panel)) {
            *slot = None;
        }
    }
}

/// Forget the panel focused, opened, raised or showing as a flyout: the workspace was replaced.
pub(crate) fn forget_panels(app: &mut DesignApp) {
    app.ui.docking_focused = None;
    app.ui.docking_opened = None;
    app.ui.docking_raise = None;
    app.ui.docking_flyout = None;
}

/// A flyout opens only from an iconic column; forget it once its panel is no longer in one.
fn release_flyout(app: &mut DesignApp) {
    let layout = layout_of(app);
    if app.ui.docking_flyout.as_deref().is_some_and(|panel| !in_iconic_column(&layout, panel)) {
        app.ui.docking_flyout = None;
    }
}

/// Window › Collapse to Icons collapses every dock column; `iconic: false` (the control
/// channel's `dockExpanded`) expands the outer column.
pub(crate) fn set_iconic(app: &mut DesignApp, iconic: bool) -> Result<Value, String> {
    if app.ui.docking.as_ref().is_some_and(|layout| !valid(layout)) {
        return Err("saved panel layout is invalid; reset the workspace first".into());
    }
    let mut layout = layout_of(app);
    let columns = dock_columns(&layout);
    let firsts = |c: &&Column<String>| c.groups.first().and_then(|g| g.panels.first()).cloned();
    let targets: Vec<String> = if iconic {
        columns.iter().filter(|c| !c.iconic).filter_map(firsts).collect()
    } else {
        columns.first().filter(|c| c.iconic).and_then(firsts).into_iter().collect()
    };
    for panel in targets {
        apply(&mut layout, Action::set_iconic(panel, iconic))?;
    }
    app.ui.docking = Some(layout);
    app.ui.dock_expanded = !iconic;
    app.ui.open_panel = None;
    release_flyout(app);
    Ok(Value::Null)
}

/// The panels' names, icons, sizes and rights, in the interface language: the spec model the dock reads, kept in a
/// `Tracked` so the dock reads it again only when the language changes.
#[derive(Clone, Debug, PartialEq, PlainData)]
struct Specs {
    language: String,
}

impl PanelSpecs<String> for Specs {
    fn spec(&self, panel: &String) -> PanelSpec {
        let english = crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS).find(|p| p.0 == panel).map_or(panel.as_str(), |p| p.1);
        let spec = PanelSpec::new(crate::i18n::tr(&self.language, english).to_owned());
        if panel == CANVAS {
            return spec.canvas().limits(craft_ui::contract::SizeLimits::at_least(vec2(200.0, 120.0)));
        }
        let spec = spec.limits(craft_ui::contract::SizeLimits::at_least(vec2(220.0, 80.0))).float_size(floating_size(panel));
        match panel_artwork(panel) {
            Some(artwork) => spec.icon(craft_ui::primitives::IconSpec::glyph(artwork)),
            None => spec,
        }
    }

    fn permissions(&self, panel: &String) -> Permissions {
        model_permissions(panel)
    }
}

/// A panel's icon from [`crate::icons`], as a framework glyph: the dock's icon strips keep the app's drawings.
#[derive(craft_ui::contract::MemoProps)]
struct PanelArtwork(&'static str);

impl craft_ui::primitives::Glyph for PanelArtwork {
    fn paint(&self, painter: &egui::Painter, _: &craft_ui::style::UiTokens, rect: Rect, color: egui::Color32, _: bool) {
        crate::icons::paint(painter, rect, self.0, color);
    }
}

/// The artwork of a dock panel's icon.
fn panel_artwork(panel: &str) -> Option<&'static PanelArtwork> {
    static ART: std::sync::OnceLock<Vec<(&'static str, PanelArtwork)>> = std::sync::OnceLock::new();
    let art =
        ART.get_or_init(|| crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS).map(|(id, _, icon)| (*id, PanelArtwork(icon))).collect());
    art.iter().find(|(id, _)| *id == panel).map(|(_, artwork)| artwork)
}

thread_local! {
    /// The workspace the dock shows, kept while the layout and the closed and floated places stay the same, so the
    /// dock's derived values follow its revision; and the spec model in the language it was built for.
    static SHOWN: RefCell<Option<(Layout<String>, crate::HiddenPanels, Arc<Workspace<String>>)>> = const { RefCell::new(None) };
    static SPECS: RefCell<Option<Arc<Tracked<Specs>>>> = const { RefCell::new(None) };
}

/// The workspace of the layout in use with its closed and floated panels' places, the same `Arc` while they stay the
/// same.
fn shown_workspace(app: &DesignApp) -> Result<Arc<Workspace<String>>, String> {
    let layout = layout_of(app);
    let hidden = app.ui.docking_hidden.clone();
    if let Some(kept) = SHOWN.with(|shown| shown.borrow().as_ref().filter(|(l, h, _)| *l == layout && *h == hidden).map(|(_, _, w)| Arc::clone(w))) {
        return Ok(kept);
    }
    let workspace = Arc::new(Transaction::begin(app)?.workspace);
    SHOWN.with(|shown| *shown.borrow_mut() = Some((layout, hidden, Arc::clone(&workspace))));
    Ok(workspace)
}

/// The spec model in `language`, the same one while the language stays.
fn specs(language: &str) -> Arc<Tracked<Specs>> {
    SPECS.with(|specs| {
        let mut specs = specs.borrow_mut();
        match specs.as_ref().filter(|kept| kept.language == language) {
            Some(kept) => Arc::clone(kept),
            None => {
                let made = Arc::new(Tracked::new(Specs { language: language.to_owned() }));
                *specs = Some(Arc::clone(&made));
                made
            }
        }
    })
}

/// How DesignCraft's dock behaves, as InDesign's: new docks at the docked area's free sides but no full-height column
/// from the window's edges, a drop on no target (the canvas) floats, a floating window's title bar docks the whole
/// window, a panel menu button on every group (as InDesign's panel menus) with Minimize and the group order in it,
/// flyouts that stay open on canvas work unless Auto-Collapse Iconic Panels is on, no Maximize Frame or Close Others
/// items and no wheel or tab close gestures. Floating windows get OS windows of their own on the desktop.
fn dock_config() -> DockConfig {
    DockConfig::default()
        .side_edge_zones(false)
        .new_dock_zones(true)
        .float_on_empty_drop(true)
        .title_bar_docks(true)
        .close_others_item(false)
        .maximize_item(false)
        .wheel_selects_tabs(false)
        .tab_close_buttons(false)
        .group_order_items(true)
        .menu_button(true)
        .flyout_outside_closes(false)
        .native_windows(!cfg!(target_arch = "wasm32"))
}

/// Draws the panels and the canvas: the framework's dock area over the layout in use. The area writes nothing
/// itself; what the user did comes back as actions, applied through `window.panel.layout`, as a command is.
pub(crate) fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    if app.ui.docking.as_ref().is_some_and(|layout| !valid(layout)) {
        app.ui.docking = None;
        forget_panels(app);
        app.status(crate::i18n::tr(&app.ui.language, "Invalid panel layout; restored the default dock"));
    }
    if !runtime_on(ui.ctx()) {
        // The dock needs the craft UI runtime on the context; without it the window shows the canvas alone.
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| log::warn!("the craft UI runtime is not installed on this context: the panels are not shown"));
        let fill = crate::theme::Tokens::get(ui.ctx()).pasteboard;
        ui.painter().rect_filled(ui.max_rect(), 0.0, fill);
        app.canvas_area(ui, false);
        return;
    }
    let workspace = match shown_workspace(app) {
        Ok(workspace) => workspace,
        Err(error) => {
            app.status(error);
            return;
        }
    };
    // Under a modal (a dialog, About, Quick Apply) or an open popup the dock takes no gesture, and a release over the
    // story editor, which draws after the dock (its first, unmeasured frame: over the whole window), drops nothing.
    let ctx = ui.ctx().clone();
    if modal_open(app) || egui::Popup::is_any_open(&ctx) {
        cancel_gesture(&ctx);
    } else if crate::story_editor::visible(app) {
        let editor = ctx.memory(|m| m.area_rect(egui::Id::unique("story_editor"))).unwrap_or_else(|| ctx.content_rect());
        if ctx.input(|i| i.pointer.any_released() && i.pointer.interact_pos().is_some_and(|at| editor.contains(at))) {
            cancel_gesture(&ctx);
        }
    }
    let specs = specs(&app.ui.language);
    let mut flyout = app.ui.docking_flyout.clone().filter(|panel| in_iconic_column(workspace.layout(), panel));
    app.ui.docking_raise = None;
    let auto_collapse = app.ui.dock_auto_collapse;
    // The dock lays out in the interface language's direction (the dock at the end edge: the left in right-to-left
    // languages), with its words in that language.
    let rtl = crate::i18n::is_rtl(&app.ui.language);
    #[cfg(not(target_arch = "wasm32"))]
    let words = crate::framework_host::words(&app.ui.language);
    let output = craft_ui::runtime::env::with_env(
        ui,
        |env| {
            env.direction = Some(if rtl { craft_ui::runtime::env::Direction::Rtl } else { craft_ui::runtime::env::Direction::Ltr });
            #[cfg(not(target_arch = "wasm32"))]
            {
                env.words = Some(craft_ui::runtime::env::WordsOverride::Replace(words));
            }
        },
        |ui| {
            let mut content = PanelContent { app: &mut *app };
            let mut area =
                DockArea::new(AREA, Bind::given(Arc::clone(&workspace)), &*specs, &mut content, dock_config()).flyout(Bind::bound(&mut flyout));
            if auto_collapse {
                area = area.auto_collapse();
            }
            area.show(ui)
        },
    );
    app.ui.docking_flyout = flyout;
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            drawn_key(),
            Drawn { space: output.float_space.clone(), viewports: output.windows.iter().map(|(_, viewport)| *viewport).collect() },
        )
    });
    for event in output.events {
        match event {
            DockEvent::ToggleAutoCollapse => app.ui.dock_auto_collapse = !app.ui.dock_auto_collapse,
            DockEvent::FlyoutChanged { panel } => app.ui.docking_flyout = panel,
            _ => {}
        }
    }
    if let Some(panel) = output.focused.or(output.picked) {
        if panel != CANVAS {
            app.ui.docking_opened = Some(panel.clone());
        }
        app.ui.docking_focused = Some(panel);
    }
    if let Some(error) = output.error {
        app.ui.status = error.error.to_string();
    }
    // The user's own gesture keeps the focus on the panel it moves, while that panel stays open.
    let focused = app.ui.docking_focused.clone();
    for action in output.actions.into_iter().chain(output.requested) {
        if let Err(error) = app.run("window.panel.layout", json!({"action": action})) {
            app.ui.status = error;
        }
    }
    if let Some(panel) = focused.filter(|panel| panel == CANVAS || layout_of(app).contains(panel)) {
        app.ui.docking_focused = Some(panel);
    }
    #[cfg(not(target_arch = "wasm32"))]
    open_window_drops(app, &output.window_files);
    // Keys pressed in a floating panel's own OS window that nothing there took run the main window's shortcuts.
    crate::menus::forwarded_shortcuts(app, ui.ctx(), output.forwarded, output.forwarded_modifiers.unwrap_or_default());
}

/// Files dropped on a floating panel's own OS window open or place as drops on the main window
/// do. The pointer there is in that window's coordinates, so a placed file gets no canvas point.
#[cfg(not(target_arch = "wasm32"))]
fn open_window_drops(app: &mut DesignApp, files: &[craft_ui::runtime::drag::OsFile]) {
    let drops: Vec<(String, Option<egui::Pos2>)> = files
        .iter()
        .filter_map(|file| file.path.as_ref())
        .map(|path| path.to_string_lossy().to_string())
        .filter(|p| !p.is_empty())
        .map(|p| (p, None))
        .collect();
    if !drops.is_empty() {
        app.open_dropped_batch(drops);
    }
}

/// Whether a modal (a dialog, About or Quick Apply) is open. Floating panels in their own OS
/// windows take no input then, as the main window behind the modal doesn't.
pub(crate) fn modal_open(app: &DesignApp) -> bool {
    app.ui.dialog.is_some() || app.ui.about || app.ui.palette.is_some()
}

/// What the control channel reports: the layout in use (the framework's serialized `Layout`),
/// whether it is the workspace's default, the panel (or `canvas`) last clicked or selected, the
/// panels closed with a remembered place, and the panel whose flyout is open.
pub(crate) fn inspect(app: &DesignApp, _ctx: &egui::Context) -> Value {
    json!({
        "layout": layout_of(app),
        "customized": app.ui.docking.is_some(),
        "focused": app.ui.docking_focused,
        "flyout": app.ui.docking_flyout,
        "autoCollapse": app.ui.dock_auto_collapse,
        "rightToLeft": crate::i18n::is_rtl(&app.ui.language),
        "hidden": app.ui.docking_hidden.keys().collect::<Vec<_>>(),
    })
}

/// The wording legacy craft-ui's docking labels had in DesignCraft, translated through `i18n::tr`. The framework's
/// dock shows its own words now; the translation tables keep these, which the i18n tests check.
#[cfg(test)]
pub(crate) const DOCK_LABELS: &[(&str, &str)] = &[
    ("dock.float", "Float Panel"),
    ("dock.close", "Close"),
    ("dock.movePanelTo", "Group with"),
    ("dock.panels", "Panels"),
    ("dock.resize", "Resize panels"),
    ("dock.resizeWindow", "Resize panel window"),
    ("dock.moveGroup", "Move tab group"),
    ("dock.closeGroup", "Close Tab Group"),
    ("dock.minimize", "Minimize"),
    ("dock.expand", "Expand"),
    ("dock.floatGroup", "Float Tab Group"),
    ("dock.moveGroupTo", "Group Tab Group with"),
    ("dock.moveColumn", "Move panel column"),
    ("dock.collapseToIcons", "Collapse to Icons"),
    ("dock.expandPanels", "Expand Panels"),
    ("dock.collapseColumn", "Collapse to Icons"),
    ("dock.expandColumn", "Expand Panels"),
    ("dock.moveWindow", "Move panel window"),
    ("dock.closeWindow", "Close panel window"),
    ("dock.closeOthers", "Close Other Panels in Group"),
    ("dock.maximize", "Maximize Frame"),
    ("dock.restoreFrame", "Restore Frame Size"),
    ("dock.autoCollapse", "Auto-Collapse Iconic Panels"),
];

/// The size a panel floats at (tab strip included), whatever the size of the dock it left.
fn floating_size(panel: &str) -> egui::Vec2 {
    match panel {
        "properties" | "pages" | "layers" => egui::vec2(300.0, 420.0),
        "characterStyles" | "paragraphStyles" => egui::vec2(290.0, 340.0),
        "glyphs" | "library" | "links" | "preflight" | "dataMerge" => egui::vec2(320.0, 400.0),
        _ => egui::vec2(280.0, 360.0),
    }
}

/// The panels' content.
struct PanelContent<'a> {
    app: &'a mut DesignApp,
}

impl DockPanels<String> for PanelContent<'_> {
    /// A panel's body: content taller than the group scrolls in a framework scrolling stack (an egui scroll area
    /// would take the pointer from the framework controls inside it). The Pages panel scrolls its own parts.
    fn body(&mut self, panel: &String, ui: &mut egui::Ui, _: &mut BodyCx<'_>) {
        use craft_ui::contract::{Props as _, ShowContainer as _};
        if crate::dock::scrolls_itself(panel) {
            crate::dock::panel_body(self.app, ui, panel);
            return;
        }
        let app = &mut *self.app;
        craft_ui::layout_components::Stack::new()
            .id_salt(("shared-panel-body", panel.as_str()))
            .fill_width()
            .scroll(craft_ui::contract::Scroll::vertical())
            .show(ui, |ui, _| crate::dock::panel_body(app, ui, panel));
    }

    fn group_view(&mut self, panels: &[String], _: usize, ui: &mut egui::Ui, at: GroupAt, _: &mut BodyCx<'_>) -> Option<f32> {
        if !panels.iter().any(|panel| panel == CANVAS) {
            return None;
        }
        // The canvas fills the rect it is shown in; protected, it never sits in a flyout, which is what measures.
        let GroupAt::Show(rect) = at else { return Some(0.0) };
        let fill = crate::theme::Tokens::get(ui.ctx()).pasteboard;
        ui.painter().rect_filled(rect, 0.0, fill);
        self.app.canvas_area(ui, false);
        Some(rect.height())
    }

    /// The Pages panel's own menu, before its tab strip: its page commands. The dock's own rows (Float, Dock Panel,
    /// Move Panel To, Close, Collapse to Icons) are in the group's menu at the end of the strip.
    fn group_header(&mut self, panels: &[String], active: usize, ui: &mut egui::Ui, _: &mut BodyCx<'_>) {
        use craft_ui::contract::{Props as _, Show as _, ShowContainer as _};
        if panels.get(active).map(String::as_str) != Some("pages") {
            return;
        }
        static MENU: PanelArtwork = PanelArtwork("menu");
        let label = crate::i18n::tr(&self.app.ui.language, "Pages Panel Menu").to_owned();
        let opener = craft_ui::buttons::IconButton::new(craft_ui::primitives::IconSpec::glyph(&MENU))
            .id(egui::Id::new("designcraft-pages-panel-menu"))
            .label(label.clone())
            .tooltip(label.clone())
            .show(ui);
        let app = &mut *self.app;
        // The menu's open state is the dock's: a row that ran (it closes its egui menu) closes the framework menu too.
        let open_id = egui::Id::new("designcraft-pages-panel-menu-open");
        let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        let mut ran = false;
        // The page commands' rows are the Pages panel's own; they come to the framework with that panel.
        let _ =
            craft_ui::menu::Menu::new(label).opened_by(&opener, craft_ui::contract::Trigger::Click).open(Bind::bound(&mut open)).show(ui, |ui, _| {
                crate::panels::pages::panel_menu(app, ui);
                ran = ui.should_close();
            });
        if ran {
            open = false;
        }
        ui.data_mut(|d| d.insert_temp(open_id, open));
    }
}

/// Close the last explicitly opened panel through the same validated layout transaction.
pub(crate) fn close_visible(app: &mut DesignApp) -> Result<(), String> {
    if let Some(panel) = app.ui.docking_opened.clone() {
        shared_command(app, "window.panel.close", &json!({"panel": panel}))?;
    }
    app.ui.open_panel = None;
    Ok(())
}

#[cfg(test)]
mod tests;
