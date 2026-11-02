//! Application-owned panel IDs and commands around the shared docking layout.
//!
//! The window's panels and the canvas are one craft-ui layout: a horizontal split of the
//! protected `canvas` panel and a dock at the logical end edge (the right in left-to-right
//! languages, the left in right-to-left ones).

use craft_ui::docking::{
    Action, Column, DockCommand, DockOperation, FloatSpace, Floating, Group, Layout, Node, Permissions, Placement, Side, Target, UiDock, Workspace,
};
use serde_json::{Value, json};

use crate::DesignApp;

/// The id of the dock area that draws the panels.
fn area_id() -> egui::Id {
    egui::Id::unique("designcraft-panel-docking")
}

/// The dock area a headless app floats into, before it ever drew a frame.
const HEADLESS_AREA: egui::Rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1400.0, 900.0));

/// The egui id of a docking control, for tests that press it or read its response.
#[cfg(test)]
pub(crate) fn hit_id(hit: &craft_ui::docking::Hit<String>) -> egui::Id {
    craft_ui::docking::DockArea::<String>::hit_id(area_id(), hit)
}

/// The egui id of a panel's tab.
#[cfg(test)]
pub(crate) fn tab_id(panel: &str) -> egui::Id {
    hit_id(&craft_ui::docking::Hit::Tab(panel.to_owned()))
}

/// The viewports of the floating windows that may have OS windows of their own. Where egui
/// embeds viewports every floating window stays in the main window.
pub(crate) fn native_viewports(app: &DesignApp, ctx: &egui::Context) -> Vec<egui::ViewportId> {
    if ctx.embed_viewports() {
        return Vec::new();
    }
    layout_of(app).floating.iter().map(|window| craft_ui::docking::DockArea::<String>::window_viewport(area_id(), window.id)).collect()
}

/// The egui layer of the floating window whose first panel is `panel`.
#[cfg(test)]
pub(crate) fn floating_layer(panel: &str) -> egui::LayerId {
    let window = Floating::single(vec![panel.to_owned()], 0, [0.0, 0.0, 1.0, 1.0]);
    craft_ui::docking::DockArea::<String>::floating_layer(area_id(), &window).unwrap_or_else(egui::LayerId::background)
}

pub(crate) fn cancel_gesture(ctx: &egui::Context) {
    craft_ui::docking::cancel_drag::<String>(ctx, area_id());
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
    if id == CANVAS { Permissions::PROTECTED } else { Permissions::default() }
}

pub(crate) fn valid(layout: &Layout<String>) -> bool {
    layout.validate().is_ok()
        && layout.panels().iter().all(|id| registered(id))
        && Layout { root: layout.root.clone(), floating: Vec::new() }.contains(&CANVAS.to_string())
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
    (!groups.is_empty()).then_some(Column { groups, width: COLUMN_WIDTH, iconic: true, iconic_width: Some(width) })
}

/// The canvas beside a dock of `columns` (from the window edge inwards) at the logical end edge.
fn with_canvas(columns: Vec<Column<String>>, floating: Vec<Floating<String>>) -> Layout<String> {
    let width: f32 = columns.iter().map(|c| if c.iconic { c.iconic_width.unwrap_or(ICON_WIDTH) } else { c.width }).sum();
    Layout {
        root: Some(Node::Split {
            axis: craft_ui::layout::SplitAxis::Horizontal,
            size: craft_ui::layout::SplitSize::FixedSecond(width),
            first: Box::new(Node::Tabs { panels: vec![CANVAS.to_string()], active: 0 }),
            second: Box::new(Node::Dock { side: Side::End, columns }),
        }),
        floating,
    }
}

/// The layout of a workspace that has not been customized, from its panel settings: the
/// canvas beside a dock whose outer column is the Properties · Pages · Layers group (with an
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
            floating.push(Floating::single(vec![id], 0, [at[0], at[1], 280.0, 360.0]).with_id(window));
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
            groups.push(Group { height: Some(OPEN_PANEL_HEIGHT), ..Group::new(vec![id], 0) });
        }
        if !groups.is_empty() {
            columns.push(Column { groups, width: COLUMN_WIDTH, iconic: !ui.dock_expanded, iconic_width: Some(ICON_WIDTH) });
        }
    }
    columns.extend(icon_column(ui.icon_groups.as_deref(), &mut placed));
    with_canvas(columns, floating)
}

/// The window id of a default layout's floating `panel`: the same every time, with the top bit
/// set, where craft-ui's fresh ids never reach.
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
    let mut columns: Vec<Column<String>> =
        reading.into_iter().map(|groups| Column { groups, width: COLUMN_WIDTH, iconic: !dock_expanded, iconic_width: Some(ICON_WIDTH) }).collect();
    let mut floating = Vec::new();
    for window in &old.floating {
        if let Some(root) = unplaced_tree(&window.root, 0, &mut placed) {
            floating.push(Floating { minimized: window.minimized, ..Floating::new(root, window.rect) });
        }
    }
    for panel in hidden {
        let _ = placed.take(panel);
    }
    columns.extend(icon_column(icon_groups, &mut placed));
    let layout = with_canvas(columns, floating);
    valid(&layout).then_some(layout)
}

/// A floating window's tree with only the panels not placed yet; a split that loses one side
/// becomes its other side. None when no panel is left.
fn unplaced_tree(node: &Node<String>, depth: usize, placed: &mut Placed) -> Option<Node<String>> {
    if depth > craft_ui::docking::MAX_DEPTH {
        return None;
    }
    match node {
        Node::Tabs { panels, active } => placed.group(panels, *active).map(|g| Node::Tabs { panels: g.panels, active: g.active }),
        Node::Stack { entries } => {
            let entries: Vec<_> =
                entries.iter().filter_map(|e| placed.take(&e.panel).map(|panel| craft_ui::docking::StackEntry { panel, ..e.clone() })).collect();
            (!entries.is_empty()).then_some(Node::Stack { entries })
        }
        Node::Split { axis, size, first, second } => {
            let first = unplaced_tree(first, depth.saturating_add(1), placed);
            let second = unplaced_tree(second, depth.saturating_add(1), placed);
            match (first, second) {
                (Some(first), Some(second)) => Some(Node::Split { axis: *axis, size: *size, first: Box::new(first), second: Box::new(second) }),
                (one, other) => one.or(other),
            }
        }
        _ => None,
    }
}

/// The groups of a floating window, first to last: its tab groups and accordions.
pub(crate) fn window_groups(window: &Floating<String>) -> Vec<Group<String>> {
    let mut out = Vec::new();
    let mut visit = vec![(&window.root, 0usize)];
    while let Some((node, depth)) = visit.pop() {
        if depth > craft_ui::docking::MAX_DEPTH || out.len() > craft_ui::docking::MAX_PANELS {
            break;
        }
        match node {
            Node::Split { first, second, .. } => {
                visit.push((second, depth.saturating_add(1)));
                visit.push((first, depth.saturating_add(1)));
            }
            Node::Tabs { panels, active } => out.push(Group::new(panels.clone(), *active)),
            Node::Stack { entries } => out.push(Group::new(entries.iter().map(|e| e.panel.clone()).collect(), 0)),
            _ => {}
        }
    }
    out
}

/// The groups of a dock-only layout's node, column by column in reading order. A vertical split
/// stacks its parts in one column; a horizontal split puts them side by side.
fn dock_only_columns(node: &Node<String>, depth: usize, placed: &mut Placed) -> Vec<Vec<Group<String>>> {
    if depth > craft_ui::docking::MAX_DEPTH {
        return Vec::new();
    }
    let column = |groups: Vec<Group<String>>| if groups.is_empty() { Vec::new() } else { vec![groups] };
    match node {
        Node::Tabs { panels, active } => column(placed.group(panels, *active).into_iter().collect()),
        Node::Stack { entries } => column(
            entries
                .iter()
                .filter_map(|e| placed.take(&e.panel).map(|id| Group { minimized: !e.open, height: e.height, ..Group::new(vec![id], 0) }))
                .collect(),
        ),
        Node::Split { axis, first, second, .. } => {
            let mut parts = dock_only_columns(first, depth.saturating_add(1), placed);
            parts.extend(dock_only_columns(second, depth.saturating_add(1), placed));
            match axis {
                craft_ui::layout::SplitAxis::Horizontal => parts,
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
    layout.panel_state(&panel.to_string()) == craft_ui::docking::PanelState::Front
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
        if seen > 2 * craft_ui::docking::MAX_PANELS {
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
        if seen > 2 * craft_ui::docking::MAX_PANELS {
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
        if let Some(group) = window_groups(floating).iter().position(has) {
            return Some(Place::Floating { window, group });
        }
    }
    let mut visit: Vec<(&Node<String>, Vec<bool>)> = layout.root.iter().map(|node| (node, Vec::new())).collect();
    let mut seen = 0usize;
    while let Some((node, path)) = visit.pop() {
        seen = seen.saturating_add(1);
        if seen > 2 * craft_ui::docking::MAX_PANELS || path.len() > craft_ui::docking::MAX_DEPTH {
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
            Node::Stack { entries } if entries.iter().any(|e| e.panel == panel) => return Some(Place::Tree { path, column: 0, group: 0 }),
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

/// Is `panel` an open entry of an accordion (which Minimize closes)?
fn open_in_stack(layout: &Layout<String>, panel: &str) -> bool {
    let mut visit: Vec<&Node<String>> = layout.root.iter().collect();
    let mut seen = 0usize;
    while let Some(node) = visit.pop() {
        seen = seen.saturating_add(1);
        if seen > 2 * craft_ui::docking::MAX_PANELS {
            break;
        }
        match node {
            Node::Split { first, second, .. } => {
                visit.push(first);
                visit.push(second);
            }
            Node::Stack { entries } => {
                if let Some(e) = entries.iter().find(|e| e.panel == panel) {
                    return e.open;
                }
            }
            _ => {}
        }
    }
    false
}

/// Where a panel with no other destination goes: a panel of the first expanded dock column,
/// else of any column, else of the root.
fn first_dock_panel(layout: &Layout<String>, except: &str) -> Option<String> {
    let usable = |p: &&String| p.as_str() != except && p.as_str() != CANVAS;
    let columns = dock_columns(layout);
    let in_columns =
        |iconic: bool| columns.iter().filter(|c| c.iconic == iconic).flat_map(|c| &c.groups).flat_map(|g| &g.panels).find(usable).cloned();
    in_columns(false).or_else(|| in_columns(true)).or_else(|| {
        let root = Layout { root: layout.root.clone(), floating: Vec::new() };
        root.panels().into_iter().find(usable).cloned()
    })
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
        let id = if showing(app, "table") { "window.panel.close" } else { "window.panel.activate" };
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
    app.ui.docking_flyout.as_deref() == Some(panel) && in_iconic_column(&layout_of(app), panel)
}

/// Close the open flyout; the layout is unchanged.
fn close_flyout(app: &mut DesignApp) -> Value {
    app.ui.docking_flyout = None;
    if let Some(ctx) = app.egui_ctx.as_ref() {
        craft_ui::docking::close_flyout::<String>(ctx, area_id());
    }
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
/// use, all or nothing. craft-ui's `DockCommand` parses it and makes the layout actions;
/// DesignCraft resolves panel names, shows a hidden panel before moving, floating or docking
/// it, and returns a floated panel to its dock place on Dock Panel.
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
    // a window edge is a `side`, not a frame on the outer edge (craft-ui's `Placement::Edge`).
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
    if let Some(panel) = shows.as_ref().filter(|p| !txn.workspace.layout.contains(p)) {
        let fallback = first_dock_panel(&txn.workspace.layout, panel);
        txn.workspace.set_visible(panel.clone(), true, fallback, model_permissions).map_err(|e| e.to_string())?;
        shown = true;
    }
    let returned = match &command {
        DockCommand::Dock { panel, anchor: None, side: None, group, column: false, .. } if !shown => {
            txn.dock_group_back(panel)? || (!group && txn.dock_back(panel)?)
        }
        _ => false,
    };
    let actions = match &command {
        // Showing it was the whole command (craft-ui's `DockCommand::apply` does the same).
        DockCommand::Dock { anchor: None, side: None, .. } if shown || returned => Vec::new(),
        _ => command.to_actions(&txn.workspace, rtl, &space).map_err(|e| e.to_string())?,
    };
    let reveal = match &command {
        DockCommand::Layout { action } | DockCommand::UiDock(UiDock::Action { action }) => match action {
            Action::Activate { panel } | Action::Open { panel, .. } | Action::OpenAt { panel, .. } => Some(panel.clone()),
            _ => None,
        },
        DockCommand::UiDock(UiDock::Operation(
            DockOperation::Float { panel, .. } | DockOperation::MoveFloating { panel, .. } | DockOperation::Move { panel, .. },
        )) => Some(panel.clone()),
        _ => shows,
    };
    let mut closed = Vec::new();
    for action in actions {
        closed.extend(txn.apply(action)?);
    }
    Ok(txn.commit(app, closed, reveal))
}

/// Where floating windows may go: the window, below the title-bar strip when DesignCraft draws
/// its own title bar under macOS's transparent one. A floating window in that strip would take
/// presses that move the whole app window.
fn float_bounds(app: &DesignApp, ctx: &egui::Context) -> egui::Rect {
    let mut r = ctx.content_rect();
    if app.integrated_titlebar {
        r.min.y = (r.min.y + crate::chrome::MAC_APP_BAR).min(r.max.y);
    }
    r
}

/// Where a command floats a panel, group or column it gives no rect: inside the dock area as
/// last drawn, else inside the window, else (headless, before any frame) inside
/// [`HEADLESS_AREA`].
/// Seeded with every panel's own size, so a panel the area never drew (shown from hidden by the
/// same command, or headless) floats at that size too.
fn float_space(app: &DesignApp) -> FloatSpace<String> {
    let space = match app.egui_ctx.as_ref() {
        Some(ctx) => craft_ui::docking::float_space::<String>(ctx, area_id()).unwrap_or_else(|| FloatSpace::new(float_bounds(app, ctx))),
        None => FloatSpace::new(HEADLESS_AREA),
    };
    crate::dock::DOCK_TABS
        .iter()
        .chain(crate::dock::ICON_PANELS)
        .fold(space, |space, (id, _, _)| space.with_panel_size((*id).to_owned(), floating_size(id)))
}

/// A change to the layout in use, committed only when every step succeeded.
struct Transaction {
    /// The layout, and the closed panels with where each one goes back.
    workspace: Workspace<String>,
    /// Where each floating panel was docked before it floated, for Dock Panel.
    returns: crate::HiddenPanels,
}

impl Transaction {
    fn begin(app: &DesignApp) -> Result<Self, String> {
        if app.ui.docking.as_ref().is_some_and(|layout| !valid(layout)) {
            return Err("saved panel layout is invalid; reset the workspace first".into());
        }
        let layout = layout_of(app);
        let mut workspace = Workspace::new(layout);
        let mut returns = crate::HiddenPanels::default();
        for (panel, location) in
            app.ui.docking_hidden.0.iter().filter(|(id, _)| normalize(id) == Some(id.as_str())).take(craft_ui::docking::MAX_PANELS)
        {
            if !workspace.layout.contains(panel) {
                workspace.hidden.push((panel.clone(), location.clone()));
            } else if workspace.layout.floating.iter().any(|w| w.contains(panel)) {
                returns.insert(panel.clone(), location.clone());
            }
        }
        Ok(Self { workspace, returns })
    }

    /// Dock Panel for a floating panel that floated from the dock: back to that place, while
    /// the panel it was beside is still docked. True when it went back.
    fn dock_back(&mut self, panel: &String) -> Result<bool, String> {
        let layout = &self.workspace.layout;
        if !layout.floating.iter().any(|w| w.contains(panel)) {
            return Ok(false);
        }
        let docked = Layout { root: layout.root.clone(), floating: Vec::new() };
        let Some(location) = self
            .returns
            .get(panel)
            .filter(|location| location.floating.is_none() && location.anchor.as_ref().is_none_or(|anchor| docked.contains(anchor)))
        else {
            return Ok(false);
        };
        let mut candidate = layout.clone();
        apply(&mut candidate, Action::Close { panel: panel.clone() })?;
        if candidate.restore(panel.clone(), location).is_err() {
            return Ok(false);
        }
        self.workspace.layout = candidate;
        self.returns.remove(panel);
        Ok(true)
    }

    /// Dock Panel for a panel of a floating group that floated from the dock as one: the whole
    /// group back to that place, in the same column beside the same neighbour, while that
    /// neighbour is still docked. True when it went back.
    fn dock_group_back(&mut self, panel: &String) -> Result<bool, String> {
        let layout = &self.workspace.layout;
        let Some(group) = layout.floating.iter().flat_map(window_groups).find(|g| g.panels.contains(panel)) else {
            return Ok(false);
        };
        let Some(location) = self.returns.get(panel).cloned() else {
            return Ok(false);
        };
        // A group of one docks back as a panel. Every member keeps the same group place only
        // while the group is the one that floated.
        if group.panels.len() < 2 || !group.panels.iter().all(|p| self.returns.get(p) == Some(&location)) {
            return Ok(false);
        }
        let docked = Layout { root: layout.root.clone(), floating: Vec::new() };
        let Some(anchor) = location.anchor.clone().filter(|anchor| docked.contains(anchor)) else {
            return Ok(false);
        };
        if !matches!(location.placement, Placement::Group { .. } | Placement::Column { .. }) {
            return Ok(false);
        }
        let mut candidate = layout.clone();
        let to = Target::At { anchor, placement: location.placement.clone() };
        if apply(&mut candidate, Action::MoveGroup { panel: panel.clone(), to }).is_err() {
            return Ok(false);
        }
        let mut state = Vec::new();
        if let Some(saved) = &location.group {
            state.push(Action::ResizeGroup { panel: panel.clone(), height: saved.height });
            state.push(Action::SetMinimized { panel: panel.clone(), minimized: saved.minimized });
            // A group that had a column of its own gets that column back as it was.
            if let (Placement::Column { .. }, Some(column)) = (&location.placement, saved.column) {
                state.push(Action::ResizeColumn { panel: panel.clone(), width: column.width });
                state.push(Action::SetIconic { panel: panel.clone(), iconic: column.iconic });
            }
        }
        for action in state {
            apply(&mut candidate, action)?;
        }
        self.workspace.layout = candidate;
        for member in &group.panels {
            self.returns.remove(member);
        }
        Ok(true)
    }

    /// One action from a command or the renderer, for registered panels only. Floating a
    /// docked panel, or its whole group, remembers its place for Dock Panel.
    fn apply(&mut self, action: Action<String>) -> Result<Vec<String>, String> {
        if !known(&action) {
            return Err("docking action contains an unknown panel or an excessive split path".into());
        }
        if let Action::Float { panel, .. } = &action
            && let Ok(location) = self.workspace.layout.location(panel)
            && location.floating.is_none()
        {
            self.returns.insert(panel.clone(), location);
        }
        if let Action::MoveGroup { panel, to: Target::Float { .. } } = &action
            && let Some((members, location)) = docked_group_place(&self.workspace.layout, panel)
        {
            for member in members {
                self.returns.insert(member, location.clone());
            }
        }
        self.workspace.apply(action, model_permissions).map_err(|e| e.to_string())
    }

    /// Store the result. `reveal` is the panel the command showed, raised (or its flyout opened)
    /// on the next frame.
    fn commit(self, app: &mut DesignApp, closed: Vec<String>, reveal: Option<String>) -> Value {
        let Self { workspace, returns } = self;
        let layout = workspace.layout;
        let floating = |p: &String| layout.floating.iter().any(|w| w.contains(p));
        let mut hidden = crate::HiddenPanels(workspace.hidden);
        for (panel, location) in returns.0 {
            if floating(&panel) && !hidden.contains_key(&panel) {
                hidden.0.push((panel, location));
            }
        }
        let reveal = reveal.filter(|panel| layout.contains(panel));
        let flyout = reveal.as_ref().filter(|panel| in_iconic_column(&layout, panel)).cloned();
        // Focus stays on a panel only while it stays where the user left it, as craft-ui's tab
        // focus does: a command that moves it gives the focus up.
        let before = layout_of(app);
        if app.ui.docking_focused.as_deref().is_some_and(|panel| panel != CANVAS && place(&before, panel) != place(&layout, panel)) {
            app.ui.docking_focused = None;
        }
        app.ui.docking = Some(layout);
        app.ui.docking_hidden = hidden;
        release_closed(app, &closed);
        keep_float_only(app);
        if let Some(panel) = reveal {
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

/// The panels of the docked group holding `panel`, and that group's place as a whole: beside
/// the group or column next to it, with its group and column state. None when `panel` is not
/// in a dock.
fn docked_group_place(layout: &Layout<String>, panel: &String) -> Option<(Vec<String>, craft_ui::docking::Location<String>)> {
    let members = dock_columns(layout).into_iter().flat_map(|c| &c.groups).find(|g| g.panels.contains(panel))?.panels.clone();
    // The place of the group's last panel left in it is the group's own place.
    let mut alone = layout.clone();
    for other in members.iter().filter(|p| *p != panel) {
        apply(&mut alone, Action::Close { panel: other.clone() }).ok()?;
    }
    let location = alone.location(panel).ok().filter(|l| l.floating.is_none())?;
    Some((members, location))
}

/// Whether every panel an action names is registered (and a split path is not excessive).
/// The application owns the registry; craft-ui owns tree validation and structural changes.
fn known(action: &Action<String>) -> bool {
    let registered = |id: &String| self::registered(id);
    let tab = |placement: &Placement<String>| match placement {
        Placement::Tab { before: Some(id) } => registered(id),
        _ => true,
    };
    match action {
        Action::Move { panel, anchor, placement } | Action::OpenAt { panel, anchor, placement } => {
            registered(panel) && registered(anchor) && tab(placement)
        }
        Action::Open { panel, anchor } => registered(panel) && anchor.as_ref().is_none_or(registered),
        Action::Float { panel, .. }
        | Action::Close { panel }
        | Action::Activate { panel }
        | Action::MoveFloating { panel, .. }
        | Action::SetStackOpen { panel, .. }
        | Action::ResizeStack { panel, .. } => registered(panel),
        Action::ResizeSplit { path, .. } => path.len() <= craft_ui::docking::MAX_DEPTH,
        Action::ResizeWindowSplit { panel, path, .. } => registered(panel) && path.len() <= craft_ui::docking::MAX_DEPTH,
        Action::MovePanel { panel, to } | Action::MoveGroup { panel, to } | Action::MoveColumn { panel, to } | Action::MoveWindow { panel, to } => {
            registered(panel)
                && match to {
                    Target::At { anchor, placement } => registered(anchor) && tab(placement),
                    _ => true,
                }
        }
        Action::CloseGroup { panel }
        | Action::CloseWindow { panel }
        | Action::CloseOthers { panel }
        | Action::SetMinimized { panel, .. }
        | Action::SetIconic { panel, .. }
        | Action::ResizeColumn { panel, .. }
        | Action::ResizeGroup { panel, .. }
        | Action::ResizeIconic { panel, .. }
        | Action::SetNative { panel, .. }
        | Action::SetMonitor { panel, .. } => registered(panel),
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
        let mut txn = Transaction::begin(app)?;
        let panels: Vec<String> = group.iter().map(|p| (*p).to_string()).collect();
        let active = group.iter().position(|p| *p == panel).unwrap_or(0);
        let mut layout = txn.workspace.layout.clone();
        layout.floating.push(Floating::single(panels, active, [x, y, w, h]));
        layout.validate().map_err(|e| e.to_string())?;
        txn.workspace.layout = layout;
        txn.workspace.hidden.retain(|(p, _)| !group.contains(&p.as_str()));
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
        apply(&mut layout, Action::SetIconic { panel, iconic })?;
    }
    app.ui.docking = Some(layout);
    app.ui.dock_expanded = !iconic;
    app.ui.open_panel = None;
    release_flyout(app);
    Ok(Value::Null)
}

pub(crate) fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let area = area_id();
    let changed_workspace = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<u64>(area.with("generation"));
        data.insert_temp(area.with("generation"), app.ui.docking_generation);
        previous.is_some_and(|generation| generation != app.ui.docking_generation)
    });
    if changed_workspace {
        craft_ui::docking::cancel_drag::<String>(ui.ctx(), area);
    }
    if app.ui.docking.as_ref().is_some_and(|layout| !valid(layout)) {
        app.ui.docking = None;
        forget_panels(app);
        app.status(crate::i18n::tr(&app.ui.language, "Invalid panel layout; restored the default dock"));
    }
    let layout = layout_of(app);
    let raise = app.ui.docking_raise.take();
    let mut blocked = Vec::new();
    let modal = modal_open(app);
    if modal || egui::Popup::is_any_open(ui.ctx()) {
        blocked.push(ui.ctx().content_rect());
    } else if crate::story_editor::visible(app) {
        // The editor is rendered after the dock; block its unmeasured first frame conservatively.
        let rect = ui.ctx().memory(|m| m.area_rect(egui::Id::unique("story_editor"))).unwrap_or_else(|| ui.ctx().content_rect());
        blocked.push(rect);
    }
    let style = dock_style(ui, &app.ui.language);
    let mut dock = craft_ui::docking::DockArea::new(area)
        .float_bounds(float_bounds(app, ui.ctx()))
        .blocked_rects(&blocked)
        .input_blocked(modal)
        .auto_collapse_iconic(app.ui.dock_auto_collapse);
    if let Some(panel) = raise {
        dock = dock.raise_panel(panel);
    }
    let mut output = dock.show_content(ui, &layout, &style, &mut PanelContent { app: &mut *app });
    for event in std::mem::take(&mut output.events) {
        match event {
            craft_ui::docking::DockEvent::ToggleAutoCollapse => app.ui.dock_auto_collapse = !app.ui.dock_auto_collapse,
            craft_ui::docking::DockEvent::FlyoutChanged { panel } => app.ui.docking_flyout = panel,
            _ => {}
        }
    }
    if let Some(panel) = output.focused {
        if panel != CANVAS {
            app.ui.docking_opened = Some(panel.clone());
        }
        app.ui.docking_focused = Some(panel);
    }
    if let Some(error) = output.error {
        app.ui.status = error.to_string();
    }
    // The user's own gesture keeps the focus on the panel it moves, while that panel stays open.
    let focused = app.ui.docking_focused.clone();
    for action in output.actions {
        if let Err(error) = app.run("window.panel.layout", json!({"action": action})) {
            app.ui.status = error;
        }
    }
    if let Some(panel) = focused.filter(|panel| panel == CANVAS || layout_of(app).contains(panel)) {
        app.ui.docking_focused = Some(panel);
    }
    #[cfg(not(target_arch = "wasm32"))]
    open_window_drops(app, &output.window_files);
    // Keys pressed in a floating panel's own OS window run the main window's shortcuts.
    crate::menus::forwarded_shortcuts(app, ui.ctx(), output.forwarded_events, output.forwarded_modifiers.unwrap_or_default());
}

/// Files dropped on a floating panel's own OS window open or place as drops on the main window
/// do. The pointer there is in that window's coordinates, so a placed file gets no canvas point.
#[cfg(not(target_arch = "wasm32"))]
fn open_window_drops(app: &mut DesignApp, windows: &[craft_ui::docking::WindowFiles<String>]) {
    for window in windows {
        app.open_dropped_files(&window.dropped, None);
    }
}

/// Whether a modal (a dialog, About or Quick Apply) is open. Floating panels in their own OS
/// windows take no input then, as the main window behind the modal doesn't.
pub(crate) fn modal_open(app: &DesignApp) -> bool {
    app.ui.dialog.is_some() || app.ui.about || app.ui.palette.is_some()
}

/// What the control channel reports: the layout in use (craft-ui's serialized `Layout`),
/// whether it is the workspace's default, the panel (or `canvas`) last clicked or selected, the
/// panels closed with a remembered place, the panel whose flyout is open, and where each
/// docking gesture starts in the last frame (`hitRects`).
pub(crate) fn inspect(app: &DesignApp, ctx: &egui::Context) -> Value {
    let hits = craft_ui::docking::DockArea::<String>::hit_rects(ctx, area_id());
    let hit_rects: Vec<Value> = hits.iter().map(|(hit, r)| json!({"hit": hit, "rect": [r.left(), r.top(), r.width(), r.height()]})).collect();
    json!({
        "layout": layout_of(app),
        "customized": app.ui.docking.is_some(),
        "focused": app.ui.docking_focused,
        "flyout": app.ui.docking_flyout,
        "autoCollapse": app.ui.dock_auto_collapse,
        "rightToLeft": crate::i18n::is_rtl(&app.ui.language),
        "hidden": app.ui.docking_hidden.keys().collect::<Vec<_>>(),
        "hitRects": hit_rects,
    })
}

fn dock_style(ui: &egui::Ui, language: &str) -> craft_ui::docking::DockStyle {
    let t = crate::theme::Tokens::get(ui.ctx());
    let mut style = craft_ui::docking::DockStyle::from_ui(ui);
    style.tab_height = 27.0;
    style.background = t.panel;
    style.tab_background = t.tab_strip;
    style.active_background = t.panel;
    style.text = t.text_strong;
    style.inactive_text = t.text_dim;
    style.border = egui::Stroke::new(1.0, t.border);
    style.accent = t.accent;
    style.blocked_dim = t.panel.gamma_multiply(0.6);
    style.right_to_left = crate::i18n::is_rtl(language);
    // InDesign's docking: new docks at the docked area's free sides, but no full-height column
    // from the window's left or right edge; a drop on no target (the canvas) floats, and a
    // floating window's title bar docks the whole window. No Ctrl/Cmd undock latch or hint, no
    // Maximize Frame or Close Others items, no wheel or middle-click tab gestures.
    style.side_edge_zones = false;
    style.new_dock_zones = true;
    style.float_on_empty_drop = true;
    style.title_bar_docks = true;
    style.command_press_floats = false;
    style.float_hint = false;
    style.close_others_item = false;
    style.maximize_item = false;
    style.wheel_selects_tabs = false;
    style.middle_click_closes_tabs = false;
    style.localized(|key| DOCK_LABELS.iter().find(|(k, _)| *k == key).map(|(_, english)| crate::i18n::tr(language, english).to_owned()))
}

/// DesignCraft's wording of craft-ui's docking labels, translated through `i18n::tr`.
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

/// The menu behind a panel's three-line button: the panel's own commands, then its docking
/// commands.
fn three_line_menu(app: &mut DesignApp, ui: &mut egui::Ui, panel: &str) {
    if panel == "pages" {
        crate::panels::pages::panel_menu(app, ui);
        ui.separator();
    }
    panel_menu(app, ui, panel);
}

/// The three-line button at the right of a tab strip, for the panel in front.
fn three_line_button(app: &mut DesignApp, ui: &mut egui::Ui, panel: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let area = ui.max_rect();
    let r = egui::Rect::from_center_size(area.center(), egui::vec2(18.0, 18.0));
    let menu = ui.interact(r, ui.scope_id().with("panel-menu"), egui::Sense::click());
    if menu.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    crate::icons::paint(ui.painter(), r, "menu", t.icon);
    egui::Popup::menu(&menu).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| three_line_menu(app, ui, panel));
}

/// The docking commands of a panel: its tab's context menu and the end of its panel menu.
pub(crate) fn panel_menu(app: &mut DesignApp, ui: &mut egui::Ui, panel: &str) {
    let Some(panel) = normalize(panel) else { return };
    tab_commands(app, ui, panel);
    ui.separator();
    let language = app.ui.language.clone();
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, "Float Panel"))).clicked() {
        if let Err(error) = app.run("window.panel.float", json!({"panel": panel})) {
            app.status(error);
        }
        ui.close();
    }
    dock_panel_item(app, ui, panel);
    ui.menu_button(crate::rtl::widget(ui, crate::i18n::tr(&language, "Group with")), |ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (anchor, label, _) in crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS) {
                if *anchor != panel && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, label))).clicked() {
                    // A closed icon panel is made visible before becoming the destination.
                    let result = app
                        .run("window.panel.activate", json!({"panel": anchor}))
                        .and_then(|_| app.run("window.panel.move", json!({"panel":panel,"anchor":anchor})));
                    if let Err(error) = result {
                        app.status(error);
                    }
                    ui.close();
                }
            }
        });
    });
}

/// Dock Panel, for a floating panel: back to where it floated from (`Transaction::dock_back`),
/// else into the dock.
fn dock_panel_item(app: &mut DesignApp, ui: &mut egui::Ui, panel: &str) {
    if !layout_of(app).floating.iter().any(|w| w.contains(&panel.to_owned())) {
        return;
    }
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Dock Panel"))).clicked() {
        if let Err(error) = app.run("window.panel.dock", json!({"panel": panel})) {
            app.status(error);
        }
        ui.close();
    }
}

/// The size a panel floats at (tab strip included), whatever the size of the dock it left.
fn floating_size(panel: &str) -> egui::Vec2 {
    match panel {
        "properties" | "pages" | "layers" => egui::vec2(300.0, 420.0),
        "characterStyles" | "paragraphStyles" => egui::vec2(290.0, 340.0),
        "glyphs" | "library" | "links" | "preflight" | "dataMerge" => egui::vec2(320.0, 400.0),
        _ => egui::vec2(280.0, 360.0),
    }
}

/// The name of a dock panel's icon in [`crate::icons`].
fn panel_icon(panel: &str) -> Option<&'static str> {
    crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS).find(|(id, _, _)| *id == panel).map(|(_, _, icon)| *icon)
}

/// The panels' content and icons.
struct PanelContent<'a> {
    app: &'a mut DesignApp,
}
impl craft_ui::docking::DockContent<String> for PanelContent<'_> {
    fn body(&mut self, ui: &mut egui::Ui, id: &String) {
        if id == CANVAS {
            let fill = crate::theme::Tokens::get(ui.ctx()).pasteboard;
            ui.painter().rect_filled(ui.max_rect(), 0.0, fill);
            self.app.canvas_area(ui, false);
            return;
        }
        egui::Frame::NONE.inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            if crate::dock::scrolls_itself(id) {
                crate::dock::panel_body(self.app, ui, id);
                return;
            }
            // Shrinking to the content's height lets craft-ui fit a float or flyout to it.
            egui::ScrollArea::vertical()
                .id_salt(("shared-panel-body", id))
                .auto_shrink([false, true])
                .show(ui, |ui| crate::dock::panel_body(self.app, ui, id));
        });
    }
    fn logical_label(&self, id: &String) -> String {
        let english = crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS).find(|p| p.0 == id).map_or(id.as_str(), |p| p.1);
        crate::i18n::tr(&self.app.ui.language, english).to_owned()
    }
    fn permissions(&self, id: &String) -> Permissions {
        model_permissions(id)
    }
    fn limits(&self, id: &String) -> craft_ui::docking::PanelLimits {
        let min = if id == CANVAS { egui::vec2(200.0, 120.0) } else { egui::vec2(220.0, 80.0) };
        craft_ui::docking::PanelLimits { min, ..Default::default() }
    }
    fn floating_size(&self, id: &String) -> Option<egui::Vec2> {
        Some(floating_size(id))
    }
    fn paint_icon(&mut self, painter: &egui::Painter, rect: egui::Rect, id: &String, color: egui::Color32) -> bool {
        let Some(icon) = panel_icon(id) else { return false };
        crate::icons::paint(painter, rect, icon, color);
        true
    }
    fn panel_menu(&mut self, ui: &mut egui::Ui, id: &String) {
        if let Some(panel) = normalize(id) {
            dock_panel_item(self.app, ui, panel);
        }
    }
    fn group_header_width(&self, _: &[String], _: &String, _: bool, _: f32) -> f32 {
        24.0
    }
    fn group_header(&mut self, ui: &mut egui::Ui, _: &[String], active: &String, _: bool) {
        if let Some(panel) = normalize(active) {
            three_line_button(self.app, ui, panel);
        }
    }
    fn label(&mut self, ui: &egui::Ui, _: &String, logical: &str, font: &egui::FontId, color: egui::Color32) -> egui::WidgetText {
        egui::WidgetText::Galley(crate::rtl::plain(ui.ctx(), logical, font.clone(), color))
    }
    fn action_label(&mut self, ui: &egui::Ui, logical: &str, font: &egui::FontId, color: egui::Color32) -> egui::WidgetText {
        egui::WidgetText::Galley(crate::rtl::plain(ui.ctx(), logical, font.clone(), color))
    }
}

/// InDesign's panel tab commands at the end of a panel's three-line menu: Close, Close Tab Group,
/// Minimize (an open accordion entry) and Collapse to Icons. A tab's own context menu is
/// craft-ui's, which lists the same commands.
fn tab_commands(app: &mut DesignApp, ui: &mut egui::Ui, panel: &'static str) {
    let language = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&language, s).to_owned();
    let run = |app: &mut DesignApp, ui: &mut egui::Ui, id: &str, p: Value| {
        if let Err(error) = app.run(id, p) {
            app.status(error);
        }
        ui.close();
    };
    if ui.button(crate::rtl::widget(ui, tr("Close"))).clicked() {
        run(app, ui, "window.panel.close", json!({"panel": panel}));
    }
    if ui.button(crate::rtl::widget(ui, tr("Close Tab Group"))).clicked() {
        run(app, ui, "window.panel.closeGroup", json!({"panel": panel}));
    }
    let layout = layout_of(app);
    let minimizable = open_in_stack(&layout, panel);
    if ui.add_enabled(minimizable, egui::Button::new(crate::rtl::widget(ui, tr("Minimize")))).clicked() {
        run(app, ui, "window.panel.layout", json!({"action": {"SetStackOpen": {"panel": panel, "open": false}}}));
    }
    ui.separator();
    let expanded = dock_columns(&layout).iter().any(|c| !c.iconic);
    if ui.add_enabled(expanded, egui::Button::new(crate::rtl::widget(ui, tr("Collapse to Icons")))).clicked() {
        run(app, ui, "window.collapseToIcons", json!({}));
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
