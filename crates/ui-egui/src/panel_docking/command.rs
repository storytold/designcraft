//! The docking command layer: the JSON commands the control channel, the menus and the tests drive the dock with, and
//! the framework dock [`Action`]s they turn into.
//!
//! The framework has typed command sets only ([`DockCommands`]), so the app owns this vocabulary. A command is
//! deserialized from its control params with the id under `"command"`; [`DockCommand::to_actions`] turns it into
//! the actions to apply, all or none, with `Workspace::apply_all`. Placement, the float rect and the moves of a
//! panel, group, window or column come from the framework ([`move_actions`], [`dock_actions`], [`FloatSpace`],
//! [`DockPlacement::for_zone`]); this file only reads the legacy shapes.
//!
//! ```ignore
//! let mut params = params.clone();
//! params["command"] = json!(id);
//! let command: DockCommand<String> = serde_json::from_value(params)?;
//! if !command.is_app_owned() {
//!     let policy = |p: &String| app.permissions(p, locked);
//!     let actions = command.to_actions(&workspace, style.right_to_left, &space, &policy)?;
//!     workspace.apply_all(actions, locked, |p| app.permissions(p, false))?;
//! }
//! ```
//!
//! # Differences from the legacy layer
//!
//! - Zones are the framework's [`DockSide`] (`Start` and `End` in reading order). `left` and `right` stay physical on
//!   the wire: beside a docked anchor `right_to_left` flips them as the legacy `Placement::for_zone` did, and in any
//!   other container they name `Start` and `End` as the legacy `Zone::Left` and `Zone::Right` did.
//! - A floating window is one column. A `left` or `right` zone beside a floating anchor is refused
//!   ([`DockError::InvalidPlacement`]), and `top` and `bottom` there make a new group above or below, where the legacy
//!   model split the window.
//! - `side` is a [`DockEdge`] (`"Start"` or `"End"`, as the legacy `Side`).
//! - `Layout { action }` and `ui.dock { action }` carry a framework [`Action`]. Its JSON matches the legacy one for
//!   `Move`, `Float` (the rect is `[x, y, w, h]`), `Close`, `Activate`, `Open`, `OpenAt`, `MoveFloating` (an optional
//!   `edge` is new), `ResizeSplit`, the stack and group actions and every placement, with the physical `Left` and
//!   `Right` zone names read as `Start` and `End`. It differs for `SetNative` (a `placement`, not a rect) and for
//!   `Target::Float` (the rect is required and there is an optional `edge`).
//! - `window.panel.window` records a [`Placement`] of the window's size and no desktop position, so the window opens
//!   where its group sits in the app window.
//! - Locking is the workspace set's, not the workspace's. `permissions` must already include a lock
//!   ([`Permissions::locked`]) where the workspace is locked, which is what an outer edge dock reads to find the
//!   canvas; apply the actions with the same `locked` flag.

use std::fmt::Debug;
use std::hash::Hash;

use craft_behavior::DockSide;
use craft_behavior::dock::{Action, DockEdge, DockError, DockPlacement, Layout, PanelPlace, Permissions, SplitSize, Target, Workspace};
use craft_behavior::placement::Placement;
use craft_ui_panels::docking::{DockCommands, DockScene, DockTo, DragUnit, FloatSpace, actions as dock_actions, move_actions};
use egui::{Rect, pos2, vec2};
use serde::{Deserialize, Deserializer, Serialize};

/// A docking command, tagged by its control id under `"command"`.
///
/// Ids and params (`?` marks optional ones):
///
/// | Id | Params |
/// |---|---|
/// | `window.panel.layout` | `{action}`: a serialized [`Action`] |
/// | `window.panel.move` | `{panel, anchor, zone?, before?, group?, placement?}` |
/// | `window.panel.float` | `{panel, x?, y?, width?, height?, group?, column?}` |
/// | `window.panel.dock` | `{panel, anchor?, zone?, before?, group?, column?, side?}` |
/// | `window.panel.activate`, `.close`, `.closeGroup`, `.closeOthers` | `{panel}` |
/// | `window.panel.minimize` | `{panel, minimized?}`; without `minimized` it toggles |
/// | `window.panel.iconic` | `{panel, iconic?}`; without `iconic` it toggles |
/// | `window.panel.window` | `{panel, native?}` (desktop) |
/// | `window.workspace`, `.new`, `.delete` | `{name}` |
/// | `window.workspace.reset`, `ui.dock.reset` | `{}` |
/// | `window.workspace.lock` | `{locked?}`; without `locked` it toggles |
/// | `ui.dock` | `{action}`, or `{operation, …}`, see [`DockOperation`] |
///
/// `zone` is `tab` (the default for `move` and `dock`), `center`, `left`, `right`, `top`, `bottom`, `above` or
/// `below` (a new group in a column). The side zones map onto the anchor's container with
/// [`DockPlacement::for_zone`]. `before` names the tab to insert before; `null` appends. `group` moves the panel's
/// whole group, `column` its dock column or floating window. `side` is `Start` or `End`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", bound(serialize = "P: Serialize", deserialize = "P: Deserialize<'de>"))]
pub(crate) enum DockCommand<P> {
    #[serde(rename = "window.panel.layout")]
    Layout { action: Action<P> },
    #[serde(rename = "window.panel.move")]
    Move {
        panel: P,
        anchor: P,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<ZoneName>,
        #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
        before: Option<Option<P>>,
        #[serde(default)]
        group: bool,
        /// A serialized [`DockPlacement`]; it takes precedence over `zone` and `before`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement: Option<DockPlacement<P>>,
    },
    #[serde(rename = "window.panel.float")]
    Float {
        panel: P,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f32>,
        #[serde(default)]
        group: bool,
        #[serde(default)]
        column: bool,
    },
    /// To `anchor`; else to a new column of the dock on `side` (a new dock when it has none); else with a `left`,
    /// `right`, `top` or `bottom` zone to a new frame on that outer edge ([`DockPlacement::Edge`]); else a floating
    /// panel returns to where it was docked, or joins the first dock group (or frame) as a tab, and a docked one
    /// stays. A hidden panel without `anchor` and `side` is shown at its last location (the app's job, by
    /// `Workspace::set_visible`), then moved to the edge a zone names.
    #[serde(rename = "window.panel.dock")]
    Dock {
        panel: P,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<P>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<ZoneName>,
        #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
        before: Option<Option<P>>,
        #[serde(default)]
        group: bool,
        #[serde(default)]
        column: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        side: Option<DockEdge>,
    },
    /// Select a panel; a hidden one is shown at its last location first (the app's job).
    #[serde(rename = "window.panel.activate")]
    Activate { panel: P },
    #[serde(rename = "window.panel.close")]
    Close { panel: P },
    #[serde(rename = "window.panel.closeGroup")]
    CloseGroup { panel: P },
    #[serde(rename = "window.panel.closeOthers")]
    CloseOthers { panel: P },
    #[serde(rename = "window.panel.minimize")]
    Minimize {
        panel: P,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimized: Option<bool>,
    },
    #[serde(rename = "window.panel.iconic")]
    Iconic {
        panel: P,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        iconic: Option<bool>,
    },
    /// Move the panel's floating window to its own OS window (`native`, the default) or back. A docked panel's group
    /// floats first.
    #[serde(rename = "window.panel.window")]
    Window {
        panel: P,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native: Option<bool>,
    },
    /// Switch to a named workspace. The application owns named workspaces.
    #[serde(rename = "window.workspace")]
    Workspace { name: String },
    #[serde(rename = "window.workspace.new")]
    NewWorkspace { name: String },
    #[serde(rename = "window.workspace.delete")]
    DeleteWorkspace { name: String },
    /// Reset the current workspace to its saved or built-in arrangement (application-owned).
    #[serde(rename = "window.workspace.reset", alias = "ui.dock.reset")]
    ResetWorkspace {},
    /// Lock or unlock the workspace set (application-owned).
    #[serde(rename = "window.workspace.lock")]
    Lock {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locked: Option<bool>,
    },
    /// The `ui.dock` shape of LightCraft, PDFCraft, CADCraft and SoundCraft.
    #[serde(rename = "ui.dock")]
    UiDock(UiDock<P>),
}

/// `ui.dock` params: a raw action, or an operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged, bound(serialize = "P: Serialize", deserialize = "P: Deserialize<'de>"))]
pub(crate) enum UiDock<P> {
    Action { action: Action<P> },
    Operation(DockOperation<P>),
}

/// A `ui.dock` operation, tagged by `operation`. `move` takes `target` (the anchor), `zone` (default `center`) and
/// `before`, which only applies to a center move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", bound(serialize = "P: Serialize", deserialize = "P: Deserialize<'de>"))]
pub(crate) enum DockOperation<P> {
    /// Show a panel at its last location and select it.
    Open {
        panel: P,
    },
    Close {
        panel: P,
    },
    Activate {
        panel: P,
    },
    Float {
        panel: P,
        rect: [f32; 4],
    },
    MoveFloating {
        panel: P,
        rect: [f32; 4],
    },
    Move {
        panel: P,
        target: P,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<ZoneName>,
        #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
        before: Option<Option<P>>,
    },
    SetStackOpen {
        panel: P,
        open: bool,
    },
    /// `height` is required; `null` clears it.
    ResizeStack {
        panel: P,
        #[serde(deserialize_with = "required")]
        height: Option<f32>,
    },
    ResizeSplit {
        path: Vec<bool>,
        size: SplitSize,
    },
}

/// A drop zone by name, as control protocols write it. The PascalCase names of the legacy `Zone` (`Left`, `Center`, …)
/// also parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ZoneName {
    Tab,
    #[serde(alias = "Center")]
    Center,
    #[serde(alias = "Left")]
    Left,
    #[serde(alias = "Right")]
    Right,
    #[serde(alias = "Top")]
    Top,
    #[serde(alias = "Bottom")]
    Bottom,
    Above,
    Below,
}

impl ZoneName {
    /// The side the zone names, `None` for `tab`, `above` and `below`. `left` and `right` are physical: `flip`
    /// mirrors them into reading order, as it must beside a docked anchor under right-to-left.
    fn side(self, flip: bool) -> Option<DockSide> {
        match (self, flip) {
            (Self::Center, _) => Some(DockSide::Center),
            (Self::Left, false) | (Self::Right, true) => Some(DockSide::Start),
            (Self::Right, false) | (Self::Left, true) => Some(DockSide::End),
            (Self::Top, _) => Some(DockSide::Top),
            (Self::Bottom, _) => Some(DockSide::Bottom),
            (Self::Tab | Self::Above | Self::Below, _) => None,
        }
    }
}

/// A present field, `null` included: `Some(None)` for `null`, `None` when absent.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// A required field that may be `null`.
fn required<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    T::deserialize(deserializer)
}

/// Where a command puts what it moves relative to `anchor`: `zone` and `before`, with side zones mapped by
/// [`DockPlacement::for_zone`]. `default` is the zone when none is given.
fn zoned<P: Clone + Eq + Hash>(
    layout: &Layout<P>,
    right_to_left: bool,
    anchor: &P,
    zone: Option<ZoneName>,
    before: &Option<Option<P>>,
    default: ZoneName,
) -> Result<DockPlacement<P>, DockError> {
    let zone = zone.unwrap_or(default);
    // Beside a dock column the zone is the physical side the legacy placement took it for; anywhere else it names a
    // side in reading order.
    let flip = right_to_left && matches!(layout.place(anchor), Some(PanelPlace::Dock { .. }));
    Ok(match (zone, before.clone()) {
        (ZoneName::Tab | ZoneName::Center, Some(before)) => DockPlacement::tab(before),
        (ZoneName::Tab, None) => DockPlacement::tab(None),
        (ZoneName::Center, None) => DockPlacement::Split(DockSide::Center),
        (_, Some(_)) => return Err(DockError::InvalidPlacement),
        (ZoneName::Above, None) => DockPlacement::group(false, None),
        (ZoneName::Below, None) => DockPlacement::group(true, None),
        (zone, None) => DockPlacement::for_zone(layout, anchor, zone.side(flip).ok_or(DockError::InvalidPlacement)?)?,
    })
}

/// How much of the layout a command moves.
#[derive(Clone, Copy)]
enum Unit {
    Panel,
    Group,
    /// The floating window of the panel, which is one column.
    Window,
    /// The dock column of the panel.
    Column,
}

impl Unit {
    /// The unit `group` and `column` name for `panel`: a floating panel's column is its whole window.
    fn of<P: Clone + Eq + Hash>(layout: &Layout<P>, panel: &P, group: bool, column: bool) -> Self {
        let floating = layout.floating.iter().any(|window| window.contains(panel));
        match (column, group, floating) {
            (true, _, true) => Self::Window,
            (true, _, false) => Self::Column,
            (false, true, _) => Self::Group,
            (false, false, _) => Self::Panel,
        }
    }

    fn drag<P: Clone>(self, panel: &P) -> DragUnit<P> {
        let panel = panel.clone();
        match self {
            Self::Panel => DragUnit::Panel(panel),
            Self::Group => DragUnit::Group(panel),
            Self::Window => DragUnit::Window(panel),
            Self::Column => DragUnit::Column(panel),
        }
    }

    /// The action that moves this unit of `panel` beside `anchor`.
    fn beside<P: Clone>(self, panel: &P, anchor: P, placement: DockPlacement<P>) -> Action<P> {
        match self {
            Self::Panel => Action::move_beside(panel.clone(), anchor, placement),
            unit => unit.to(panel, Target::at(anchor, placement)),
        }
    }

    /// The action that moves this unit of `panel` to `to`.
    fn to<P: Clone>(self, panel: &P, to: Target<P>) -> Action<P> {
        let panel = panel.clone();
        match self {
            Self::Panel => Action::move_panel(panel, to),
            Self::Group => Action::move_group(panel, to),
            Self::Window => Action::move_window(panel, to),
            Self::Column => Action::move_column(panel, to),
        }
    }
}

/// `base` with the values a command gave (`x`, `y`, `width`, `height`) in place of its own.
fn given_rect(base: Rect, [x, y, width, height]: [Option<f32>; 4]) -> Rect {
    Rect::from_min_size(pos2(x.unwrap_or(base.min.x), y.unwrap_or(base.min.y)), vec2(width.unwrap_or(base.width()), height.unwrap_or(base.height())))
}

/// The rect a drag would float `unit` of `panel` at, inside the area. A panel floats at its own float size or the
/// default; a group or column at the width of the dock column it leaves and the default height.
fn float_rect<P: Clone + Eq + Hash>(layout: &Layout<P>, space: &FloatSpace<P>, panel: &P, unit: Unit) -> Rect {
    let source = match unit {
        Unit::Panel => None,
        _ => {
            let width = layout.place(panel).and_then(|place| layout.column(&place)).and_then(|column| column.width);
            Some(vec2(width.unwrap_or(space.default_size.x), space.content_room()))
        }
    };
    space.place(space.size_of(&unit.drag(panel), source))
}

/// The placement an OS window of `rect`'s size opens with: no desktop position, so it opens where its group sits in
/// the app window.
fn native_placement(rect: Rect) -> Placement {
    Placement::new([rect.width(), rect.height()], 1.0)
}

fn to_rect([x, y, width, height]: [f32; 4]) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(width, height))
}

impl<P> DockCommand<P> {
    /// Every command id, for an application's command registry.
    pub(crate) const IDS: &'static [&'static str] = &[
        "window.panel.layout",
        "window.panel.move",
        "window.panel.float",
        "window.panel.dock",
        "window.panel.activate",
        "window.panel.close",
        "window.panel.closeGroup",
        "window.panel.closeOthers",
        "window.panel.minimize",
        "window.panel.iconic",
        "window.panel.window",
        "window.workspace",
        "window.workspace.new",
        "window.workspace.delete",
        "window.workspace.reset",
        "window.workspace.lock",
        "ui.dock",
        "ui.dock.reset",
    ];

    /// Named workspaces, Reset and Lock belong to the application (its `WorkspaceSet`), which handles these commands
    /// before [`Self::to_actions`]; it returns [`DockError::NotAllowed`] for them.
    pub(crate) fn is_app_owned(&self) -> bool {
        matches!(
            self,
            Self::Workspace { .. } | Self::NewWorkspace { .. } | Self::DeleteWorkspace { .. } | Self::ResetWorkspace {} | Self::Lock { .. }
        )
    }

    /// The outer edge `window.panel.dock` goes to: a side zone without an anchor or side.
    fn edge(&self) -> Option<DockSide> {
        match self {
            Self::Dock { anchor: None, side: None, zone: Some(zone), .. } => zone.side(false).filter(|side| *side != DockSide::Center),
            _ => None,
        }
    }
}

impl<P: Clone + Eq + Hash + Debug> DockCommand<P> {
    /// The layout actions this command makes on `workspace`, to apply all or none with `Workspace::apply_all`.
    ///
    /// Showing a hidden panel (`activate`, `ui.dock` `open`, and `dock` without an anchor or side, which then moves
    /// it to the edge a zone names) and the app-owned commands ([`Self::is_app_owned`]) are not layout actions: the
    /// app handles them first, and here a hidden panel or anchor returns [`DockError::MissingPanel`] or
    /// [`DockError::MissingTarget`] and an app-owned command [`DockError::NotAllowed`].
    ///
    /// `left`, `right`, `top` and `bottom` zones follow [`DockPlacement::for_zone`]: beside a docked anchor they make a
    /// new column or group, and `right_to_left` is the direction the dock is drawn in.
    ///
    /// `space` is where `window.panel.float` floats what it moves when it gives no rect: at the size a drag would
    /// float it at, inside the dock area and clear of the app's menu bar. Given `x`, `y`, `width` and `height`
    /// replace the matching values, and a window that is already a floating window keeps its rect.
    ///
    /// `permissions` are the policy the actions are applied with, a lock included. An outer edge dock wraps what a
    /// drag to that edge would: the canvas column where the area narrows its edge to it, else the part between the
    /// docks.
    pub(crate) fn to_actions(
        &self,
        workspace: &Workspace<P>,
        right_to_left: bool,
        space: &FloatSpace<P>,
        permissions: &dyn Fn(&P) -> Permissions,
    ) -> Result<Vec<Action<P>>, DockError> {
        let layout = workspace.layout();
        let scene = || DockScene::new(layout, space, false, permissions).floated(workspace.floated());
        let shown = |panel: &P| if layout.contains(panel) { Ok(()) } else { Err(DockError::MissingPanel) };
        let one = |action: Action<P>| Ok(vec![action]);
        match self {
            Self::Layout { action } | Self::UiDock(UiDock::Action { action }) => one(action.clone()),
            Self::Move { panel, anchor, zone, before, group, placement: given } => {
                shown(panel)?;
                let placement = match given {
                    Some(placement) => placement.clone(),
                    None => zoned(layout, right_to_left, anchor, *zone, before, ZoneName::Tab)?,
                };
                one(Unit::of(layout, panel, *group, false).beside(panel, anchor.clone(), placement))
            }
            Self::Float { panel, x, y, width, height, group, column } => {
                shown(panel)?;
                let given = |base: Rect| given_rect(base, [*x, *y, *width, *height]);
                let unit = Unit::of(layout, panel, *group, *column);
                let window = layout.floating.iter().find(|window| window.contains(panel));
                let placed = || given(float_rect(layout, space, panel, unit));
                match (unit, window) {
                    // A lone panel, and a window, already float: they keep their rect, moved by what is given.
                    (Unit::Panel, Some(window)) if window.panels().count() == 1 => one(Action::move_floating(panel.clone(), given(window.rect))),
                    (Unit::Window, Some(window)) => one(Action::move_floating(panel.clone(), given(window.rect))),
                    // A window's only group keeps its window's rect.
                    (Unit::Group, Some(window)) if window.groups().len() == 1 => one(unit.to(panel, Target::float(given(window.rect)))),
                    (Unit::Panel, _) => one(Action::float(panel.clone(), placed())),
                    (unit, _) => one(unit.to(panel, Target::float(placed()))),
                }
            }
            Self::Dock { panel, anchor, zone, before, group, column, side } => {
                shown(panel)?;
                let unit = Unit::of(layout, panel, *group, *column);
                if let Some(anchor) = anchor {
                    let placement = zoned(layout, right_to_left, anchor, *zone, before, ZoneName::Tab)?;
                    return one(unit.beside(panel, anchor.clone(), placement));
                }
                if let Some(side) = side {
                    return move_actions(&scene(), &unit.drag(panel), DockTo::Dock(*side));
                }
                if let Some(edge) = self.edge() {
                    if before.is_some() {
                        return Err(DockError::InvalidPlacement);
                    }
                    return move_actions(&scene(), &unit.drag(panel), DockTo::Edge(edge));
                }
                match unit {
                    // Back where the panel was docked, else a tab of the first group that takes it.
                    Unit::Panel => dock_actions(&scene(), DockCommands::DockPanel, panel),
                    unit => move_actions(&scene(), &unit.drag(panel), DockTo::Docked),
                }
            }
            Self::Activate { panel } => {
                shown(panel)?;
                one(Action::activate(panel.clone()))
            }
            Self::Close { panel } => one(Action::close(panel.clone())),
            Self::CloseGroup { panel } => one(Action::close_group(panel.clone())),
            Self::CloseOthers { panel } => one(Action::close_others(panel.clone())),
            Self::Minimize { panel, minimized } => {
                layout.place(panel).ok_or(DockError::MissingPanel)?;
                match minimized {
                    Some(minimized) => one(Action::set_minimized(panel.clone(), *minimized)),
                    // An accordion entry is minimized while it is closed.
                    None => dock_actions(&scene(), DockCommands::ToggleMinimized, panel),
                }
            }
            Self::Iconic { panel, iconic } => {
                layout.place(panel).ok_or(DockError::MissingPanel)?;
                match iconic {
                    Some(iconic) => one(Action::set_iconic(panel.clone(), *iconic)),
                    None => dock_actions(&scene(), DockCommands::CollapseToIcons, panel),
                }
            }
            Self::Window { panel, native } => {
                shown(panel)?;
                if !native.unwrap_or(true) {
                    return one(Action::set_native(panel.clone(), None));
                }
                let mut actions = Vec::new();
                let mut scratch = layout.clone();
                if !scratch.floating.iter().any(|window| window.contains(panel)) {
                    let float = Unit::Group.to(panel, Target::float(float_rect(layout, space, panel, Unit::Group)));
                    scratch.apply(float.clone())?;
                    actions.push(float);
                }
                let rect = scratch.floating.iter().find(|window| window.contains(panel)).map(|window| window.rect).ok_or(DockError::MissingPanel)?;
                actions.push(Action::set_native(panel.clone(), Some(native_placement(rect))));
                Ok(actions)
            }
            Self::UiDock(UiDock::Operation(operation)) => match operation {
                DockOperation::Open { panel } | DockOperation::Activate { panel } => {
                    shown(panel)?;
                    one(Action::activate(panel.clone()))
                }
                DockOperation::Close { panel } => one(Action::close(panel.clone())),
                DockOperation::Float { panel, rect } => one(Action::float(panel.clone(), to_rect(*rect))),
                DockOperation::MoveFloating { panel, rect } => one(Action::move_floating(panel.clone(), to_rect(*rect))),
                DockOperation::Move { panel, target, zone, before } => {
                    let placement = zoned(layout, right_to_left, target, *zone, before, ZoneName::Center)?;
                    one(Action::move_beside(panel.clone(), target.clone(), placement))
                }
                DockOperation::SetStackOpen { panel, open } => one(Action::set_stack_open(panel.clone(), *open)),
                DockOperation::ResizeStack { panel, height } => one(Action::resize_stack(panel.clone(), *height)),
                DockOperation::ResizeSplit { path, size } => one(Action::resize_split(path.clone(), *size)),
            },
            Self::Workspace { .. } | Self::NewWorkspace { .. } | Self::DeleteWorkspace { .. } | Self::ResetWorkspace {} | Self::Lock { .. } => {
                Err(DockError::NotAllowed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use craft_behavior::Axis;
    use craft_behavior::dock::{Column, Floating, Group, Node, PanelState, StackEntry, node_at};
    use craft_ui_panels::docking::DockSizes;
    use egui::{Pos2, Vec2};
    use serde_json::{Value, json};

    use super::*;

    type P = String;

    fn s(panel: &str) -> P {
        panel.to_owned()
    }

    fn rect(rect: [f32; 4]) -> Rect {
        to_rect(rect)
    }

    fn permissions(panel: &P) -> Permissions {
        if panel == "canvas" { Permissions::PROTECTED } else { Permissions::default() }
    }

    /// The permissions a workspace applies with: a lock included, as `to_actions` expects.
    fn policy(locked: bool) -> impl Fn(&P) -> Permissions {
        move |panel| if locked { permissions(panel).locked() } else { permissions(panel) }
    }

    const COLOR: [f32; 4] = [300.0, 200.0, 200.0, 150.0];

    /// A start dock with one column of `[layers, channels]` and `[history]` beside the canvas, a floating `color`, and
    /// `swatches` hidden from history's group.
    fn workspace() -> Workspace<P> {
        let groups = vec![Group::new(vec![s("layers"), s("channels")], 0), Group::new(vec![s("history"), s("swatches")], 0)];
        let dock = Node::dock(DockEdge::Start, vec![Column::new(groups).with_width(240.0)]);
        let canvas = Node::tabs(vec![s("canvas")], 0);
        let root = Node::split(Axis::X, SplitSize::FixedFirst(240.0), dock, canvas);
        let layout = Layout::new(Some(root), vec![Floating::single(1, vec![s("color")], 0, rect(COLOR))]);
        let mut workspace = Workspace::new(layout);
        workspace.apply(Action::close(s("swatches")), false, permissions).unwrap();
        workspace
    }

    fn sizes() -> DockSizes {
        DockSizes::of(&egui::Context::default())
    }

    /// An 800x600 dock area that has drawn nothing.
    fn space() -> FloatSpace<P> {
        FloatSpace::new(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)), &sizes())
    }

    fn parse(params: Value) -> Result<DockCommand<P>, serde_json::Error> {
        serde_json::from_value(params)
    }

    /// What the app does with a command, as the legacy `DockCommand::apply` did: show a hidden panel
    /// (`Workspace::set_visible`, then the edge a zone names), else apply `to_actions` all or none. Lock and the
    /// named workspaces are the app's own, so they are refused here.
    fn apply(command: &DockCommand<P>, workspace: &mut Workspace<P>, locked: bool, space: &FloatSpace<P>) -> Result<Vec<P>, DockError> {
        let policy = policy(locked);
        let show = match command {
            DockCommand::Activate { panel }
            | DockCommand::Dock { panel, anchor: None, side: None, .. }
            | DockCommand::UiDock(UiDock::Operation(DockOperation::Open { panel } | DockOperation::Activate { panel })) => Some(panel),
            _ => None,
        };
        if let Some(panel) = show.filter(|panel| !workspace.layout().contains(panel)) {
            if command.edge().is_none() {
                workspace.set_visible(panel.clone(), true, None, locked, permissions)?;
                return Ok(Vec::new());
            }
            // Shown, then moved to the edge, all or nothing.
            let mut shown = workspace.clone();
            shown.set_visible(panel.clone(), true, None, locked, permissions)?;
            let actions = command.to_actions(&shown, false, space, &policy)?;
            let closed = shown.apply_all(actions, locked, permissions)?;
            *workspace = shown;
            return Ok(closed);
        }
        let actions = command.to_actions(workspace, false, space, &policy)?;
        workspace.apply_all(actions, locked, permissions)
    }

    /// The actions `params` makes on a fresh workspace.
    fn actions(params: Value) -> Vec<Action<P>> {
        let command = parse(params.clone()).unwrap_or_else(|e| panic!("{params}: {e}"));
        command.to_actions(&workspace(), false, &space(), &policy(false)).unwrap_or_else(|e| panic!("{params}: {e}"))
    }

    /// Apply `params` to a fresh workspace; return it and the panels closed.
    fn applied(params: Value) -> (Workspace<P>, Vec<P>) {
        let mut workspace = workspace();
        let command = parse(params.clone()).unwrap_or_else(|e| panic!("{params}: {e}"));
        let closed = apply(&command, &mut workspace, false, &space()).unwrap_or_else(|e| panic!("{params}: {e}"));
        (workspace, closed)
    }

    /// `params` parses but applying it fails with `error` and changes nothing.
    fn rejected(params: Value, error: DockError) {
        let mut workspace = workspace();
        let before = workspace.clone();
        let command = parse(params.clone()).unwrap_or_else(|e| panic!("{params}: {e}"));
        assert_eq!(apply(&command, &mut workspace, false, &space()), Err(error), "{params}");
        assert_eq!(workspace, before, "{params}");
    }

    fn tab(before: Option<&str>) -> DockPlacement<P> {
        DockPlacement::tab(before.map(s))
    }

    fn record_id(seen: &mut BTreeSet<String>, params: Value) -> Value {
        seen.insert(params["command"].as_str().unwrap().to_owned());
        params
    }

    fn ui_dock_operation(mut params: Value) -> Value {
        params["command"] = json!("ui.dock");
        params
    }

    fn exercise_move_commands(seen: &mut BTreeSet<String>) {
        let mut id = |params: Value| record_id(seen, params);

        // The raw action form, in both protocols.
        let raw = json!({"MoveFloating": {"panel": "color", "rect": [44, 66, 360, 450]}});
        let moved = vec![Action::move_floating(s("color"), rect([44.0, 66.0, 360.0, 450.0]))];
        assert_eq!(actions(id(json!({"command": "window.panel.layout", "action": raw}))), moved);
        assert_eq!(actions(id(json!({"command": "ui.dock", "action": raw}))), moved);

        // move: tab by default, before a tab or appended, zones, a serialized placement, a group.
        let mv = |placement| vec![Action::move_beside(s("history"), s("layers"), placement)];
        let base = json!({"command": "window.panel.move", "panel": "history", "anchor": "layers"});
        let with = |extra: Value| {
            let mut params = base.clone();
            params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            params
        };
        assert_eq!(actions(id(base.clone())), mv(tab(None)));
        assert_eq!(actions(with(json!({"before": "channels"}))), mv(tab(Some("channels"))));
        assert_eq!(actions(with(json!({"zone": "center", "before": null}))), mv(tab(None)));
        assert_eq!(actions(with(json!({"zone": "center"}))), mv(DockPlacement::Split(DockSide::Center)));
        assert_eq!(actions(with(json!({"zone": "Left"}))), mv(DockPlacement::column(false)));
        assert_eq!(actions(with(json!({"zone": "below"}))), mv(DockPlacement::group(true, None)));
        assert_eq!(actions(with(json!({"placement": {"Column": {"after": true}}}))), mv(DockPlacement::column(true)));
        assert_eq!(
            actions(with(json!({"group": true, "zone": "above"}))),
            vec![Action::move_group(s("history"), Target::at(s("layers"), DockPlacement::group(false, None)))]
        );
    }

    fn exercise_float_and_dock_commands(seen: &mut BTreeSet<String>) {
        let mut id = |params: Value| record_id(seen, params);
        let space = space();
        // What a drag floats a unit of the docked `layers` at: the default size for a panel, and for a group or a
        // column the width of the dock column and the default height.
        let panel_rect = space.place(space.default_size);
        let column_rect = space.place(space.size_of(&DragUnit::Group(s("layers")), Some(vec2(240.0, space.content_room()))));

        // float: a docked panel, a lone floating panel, a group and a column.
        assert_eq!(
            actions(id(json!({"command": "window.panel.float", "panel": "layers", "x": 10, "y": 20}))),
            vec![Action::float(s("layers"), Rect::from_min_size(pos2(10.0, 20.0), panel_rect.size()))]
        );
        assert_eq!(
            actions(json!({"command": "window.panel.float", "panel": "color", "width": 320})),
            vec![Action::move_floating(s("color"), rect([300.0, 200.0, 320.0, 150.0]))]
        );
        assert_eq!(
            actions(json!({"command": "window.panel.float", "panel": "layers", "group": true})),
            vec![Action::move_group(s("layers"), Target::float(column_rect))]
        );
        assert_eq!(
            actions(json!({"command": "window.panel.float", "panel": "history", "column": true})),
            vec![Action::move_column(s("history"), Target::float(column_rect))]
        );
        // A floating panel's column is its whole window: it keeps its rect and moves as a window.
        assert_eq!(
            actions(json!({"command": "window.panel.float", "panel": "color", "column": true, "x": 5})),
            vec![Action::move_floating(s("color"), rect([5.0, 200.0, 200.0, 150.0]))]
        );

        // dock: to an anchor, to a side, back into the dock, and a hidden panel to its last place.
        assert_eq!(
            actions(id(json!({"command": "window.panel.dock", "panel": "color", "anchor": "history", "zone": "tab"}))),
            vec![Action::move_beside(s("color"), s("history"), tab(None))]
        );
        assert_eq!(
            actions(json!({"command": "window.panel.dock", "panel": "color", "side": "End"})),
            vec![Action::move_group(s("color"), Target::new_dock(DockEdge::End))]
        );
        let new_column = Target::at(s("layers"), DockPlacement::column(true));
        assert_eq!(
            actions(json!({"command": "window.panel.dock", "panel": "color", "column": true, "side": "Start"})),
            vec![Action::move_window(s("color"), new_column.clone())]
        );
        assert_eq!(
            actions(json!({"command": "window.panel.dock", "panel": "history", "group": true, "side": "Start"})),
            vec![Action::move_group(s("history"), new_column)]
        );
        assert_eq!(actions(json!({"command": "window.panel.dock", "panel": "color"})), vec![Action::move_beside(s("color"), s("layers"), tab(None))]);
        assert_eq!(actions(json!({"command": "window.panel.dock", "panel": "layers"})), vec![]);
        let (shown, _) = applied(json!({"command": "window.panel.dock", "panel": "swatches"}));
        assert!(shown.layout().contains(&s("swatches")) && shown.hidden().is_empty());
    }

    fn exercise_visibility_and_state_commands(seen: &mut BTreeSet<String>) {
        let mut id = |params: Value| record_id(seen, params);

        // activate shows a hidden panel; close actions report what they closed.
        let (shown, _) = applied(id(json!({"command": "window.panel.activate", "panel": "swatches"})));
        assert!(shown.layout().contains(&s("swatches")));
        assert_eq!(actions(json!({"command": "window.panel.activate", "panel": "channels"})), vec![Action::activate(s("channels"))]);
        assert_eq!(applied(id(json!({"command": "window.panel.close", "panel": "channels"}))).1, vec![s("channels")]);
        assert_eq!(applied(id(json!({"command": "window.panel.closeGroup", "panel": "channels"}))).1, vec![s("layers"), s("channels")]);
        assert_eq!(applied(id(json!({"command": "window.panel.closeOthers", "panel": "channels"}))).1, vec![s("layers")]);

        // minimize and iconic toggle without a value.
        assert_eq!(actions(id(json!({"command": "window.panel.minimize", "panel": "layers"}))), vec![Action::set_minimized(s("layers"), true)]);
        assert_eq!(
            actions(json!({"command": "window.panel.minimize", "panel": "layers", "minimized": false})),
            vec![Action::set_minimized(s("layers"), false)]
        );
        assert_eq!(actions(id(json!({"command": "window.panel.iconic", "panel": "history"}))), vec![Action::set_iconic(s("history"), true)]);
        assert_eq!(
            actions(json!({"command": "window.panel.iconic", "panel": "history", "iconic": false})),
            vec![Action::set_iconic(s("history"), false)]
        );
    }

    fn exercise_window_and_workspace_commands(seen: &mut BTreeSet<String>) {
        let mut id = |params: Value| record_id(seen, params);

        // window: a floating window goes native at its size; a docked group floats first. The placement has no desktop
        // position, so the window opens where its group sits in the app window.
        let native = actions(id(json!({"command": "window.panel.window", "panel": "color"})));
        let [Action::SetNative { panel, placement: Some(placement), .. }] = native.as_slice() else { panic!("{native:?}") };
        assert_eq!(panel, "color");
        assert_eq!((placement.normal.size, placement.position_px), ([200.0, 150.0], None));
        let docked = actions(json!({"command": "window.panel.window", "panel": "layers"}));
        assert!(
            matches!(docked.as_slice(), [Action::MoveGroup { to: Target::Float { .. }, .. }, Action::SetNative { placement: Some(_), .. }]),
            "{docked:?}"
        );
        assert_eq!(actions(json!({"command": "window.panel.window", "panel": "color", "native": false})), vec![Action::set_native(s("color"), None)]);

        // Named workspaces, Reset and Lock belong to the application.
        for params in [
            json!({"command": "window.workspace", "name": "Painting"}),
            json!({"command": "window.workspace.new", "name": "Mine"}),
            json!({"command": "window.workspace.delete", "name": "Mine"}),
            json!({"command": "window.workspace.reset"}),
            json!({"command": "ui.dock.reset"}),
            json!({"command": "window.workspace.lock"}),
        ] {
            let command = parse(id(params.clone())).unwrap();
            assert!(command.is_app_owned(), "{params}");
            rejected(params, DockError::NotAllowed);
        }
        assert_eq!(parse(json!({"command": "ui.dock.reset"})).unwrap(), DockCommand::ResetWorkspace {});
        // Lock toggles or sets: the app reads `locked`.
        assert_eq!(parse(json!({"command": "window.workspace.lock"})).unwrap(), DockCommand::Lock { locked: None });
        assert_eq!(parse(json!({"command": "window.workspace.lock", "locked": false})).unwrap(), DockCommand::Lock { locked: Some(false) });
    }

    fn exercise_ui_dock_commands() {
        let op = ui_dock_operation;

        // The small apps' ui.dock operations.
        let (shown, _) = applied(op(json!({"operation": "open", "panel": "swatches"})));
        assert!(shown.layout().contains(&s("swatches")));
        assert_eq!(actions(op(json!({"operation": "close", "panel": "layers"}))), vec![Action::close(s("layers"))]);
        assert_eq!(actions(op(json!({"operation": "activate", "panel": "channels"}))), vec![Action::activate(s("channels"))]);
        assert_eq!(
            actions(op(json!({"operation": "float", "panel": "layers", "rect": [1, 2, 300, 200]}))),
            vec![Action::float(s("layers"), rect([1.0, 2.0, 300.0, 200.0]))]
        );
        assert_eq!(
            actions(op(json!({"operation": "moveFloating", "panel": "color", "rect": [1, 2, 300, 200]}))),
            vec![Action::move_floating(s("color"), rect([1.0, 2.0, 300.0, 200.0]))]
        );
        let to_layers = |placement| vec![Action::move_beside(s("color"), s("layers"), placement)];
        assert_eq!(
            actions(op(json!({"operation": "move", "panel": "color", "target": "layers"}))),
            to_layers(DockPlacement::Split(DockSide::Center))
        );
        assert_eq!(
            actions(op(json!({"operation": "move", "panel": "color", "target": "layers", "zone": "left"}))),
            to_layers(DockPlacement::column(false))
        );
        assert_eq!(actions(op(json!({"operation": "move", "panel": "color", "target": "layers", "before": null}))), to_layers(tab(None)));
        assert_eq!(
            actions(op(json!({"operation": "move", "panel": "color", "target": "layers", "before": "channels"}))),
            to_layers(tab(Some("channels")))
        );
        assert_eq!(
            actions(op(json!({"operation": "setStackOpen", "panel": "layers", "open": false}))),
            vec![Action::set_stack_open(s("layers"), false)]
        );
        assert_eq!(
            actions(op(json!({"operation": "resizeStack", "panel": "layers", "height": null}))),
            vec![Action::resize_stack(s("layers"), None)]
        );
        assert_eq!(
            actions(op(json!({"operation": "resizeStack", "panel": "layers", "height": 120}))),
            vec![Action::resize_stack(s("layers"), Some(120.0))]
        );
        assert_eq!(
            actions(op(json!({"operation": "resizeSplit", "path": [], "size": {"Ratio": 0.35}}))),
            vec![Action::resize_split(vec![], SplitSize::Ratio(0.35))]
        );
    }

    fn reject_invalid_commands() {
        let op = ui_dock_operation;

        // Bad params: parse errors, or a DockError with nothing changed.
        for params in [
            json!({"command": "window.panel.unknown", "panel": "layers"}),
            json!({"command": "window.panel.move", "panel": "layers"}),
            json!({"command": "window.panel.move", "panel": "layers", "anchor": "history", "zone": "middle"}),
            json!({"command": "window.panel.minimize", "panel": "layers", "minimized": "yes"}),
            json!({"command": "window.panel.dock", "panel": "layers", "side": "Middle"}),
            op(json!({"operation": "spin", "panel": "layers"})),
            op(json!({"operation": "float", "panel": "layers", "rect": [1, 2, 3]})),
            op(json!({"operation": "resizeStack", "panel": "layers"})),
            op(json!({"operation": "setStackOpen", "panel": "layers"})),
            op(json!({"operation": "move", "panel": "layers"})),
        ] {
            assert!(parse(params.clone()).is_err(), "{params}");
        }
        rejected(json!({"command": "window.panel.move", "panel": "layers", "anchor": "nowhere"}), DockError::MissingTarget);
        rejected(json!({"command": "window.panel.move", "panel": "nowhere", "anchor": "layers"}), DockError::MissingPanel);
        rejected(
            json!({"command": "window.panel.move", "panel": "layers", "anchor": "history", "zone": "left", "before": null}),
            DockError::InvalidPlacement,
        );
        rejected(json!({"command": "window.panel.float", "panel": "layers", "width": 1e30}), DockError::InvalidGeometry);
        rejected(json!({"command": "window.panel.float", "panel": "layers", "width": -10}), DockError::InvalidGeometry);
        rejected(json!({"command": "window.panel.float", "panel": "nowhere"}), DockError::MissingPanel);
        rejected(json!({"command": "window.panel.move", "panel": "canvas", "anchor": "layers"}), DockError::NotAllowed);
        rejected(op(json!({"operation": "moveFloating", "panel": "color", "rect": [0, 0, -5, 10]})), DockError::InvalidGeometry);
        rejected(
            op(json!({"operation": "move", "panel": "color", "target": "layers", "zone": "top", "before": "channels"})),
            DockError::InvalidPlacement,
        );
        rejected(op(json!({"operation": "resizeSplit", "path": [true, true, true], "size": {"Ratio": 0.5}})), DockError::MissingTarget);
    }

    fn reject_locked_native_window() {
        // A docked group cannot float into a native window when the workspace is locked.
        let mut locked = workspace();
        let before = locked.clone();
        let command = parse(json!({"command": "window.panel.window", "panel": "layers"})).unwrap();
        assert!(apply(&command, &mut locked, true, &space()).is_err());
        assert_eq!(locked, before);
    }

    #[test]
    fn dock_commands_parse_every_documented_shape_and_map_to_atomic_actions() {
        let mut seen = BTreeSet::new();
        exercise_move_commands(&mut seen);
        exercise_float_and_dock_commands(&mut seen);
        exercise_visibility_and_state_commands(&mut seen);
        exercise_window_and_workspace_commands(&mut seen);
        exercise_ui_dock_commands();
        let ids: BTreeSet<String> = DockCommand::<P>::IDS.iter().map(|id| (*id).to_owned()).collect();
        assert_eq!(seen, ids, "one case per id");
        reject_invalid_commands();
        reject_locked_native_window();
    }

    #[test]
    fn commands_serialize_back_to_the_json_that_parses_to_them() {
        for params in [
            json!({"command": "window.panel.move", "panel": "history", "anchor": "layers", "zone": "left", "before": null}),
            json!({"command": "window.panel.move", "panel": "history", "anchor": "layers", "group": true, "placement": {"Column": {"after": true}}}),
            json!({"command": "window.panel.float", "panel": "layers", "x": 10, "height": 90, "column": true}),
            json!({"command": "window.panel.dock", "panel": "color", "side": "End", "group": true}),
            json!({"command": "window.panel.minimize", "panel": "layers", "minimized": true}),
            json!({"command": "window.workspace.lock"}),
            json!({"command": "window.panel.layout", "action": {"MoveFloating": {"panel": "color", "rect": [44, 66, 360, 450]}}}),
            json!({"command": "ui.dock", "operation": "float", "panel": "layers", "rect": [1, 2, 300, 200]}),
            json!({"command": "ui.dock", "operation": "resizeStack", "panel": "layers", "height": null}),
        ] {
            let command = parse(params.clone()).unwrap_or_else(|e| panic!("{params}: {e}"));
            let written = serde_json::to_value(&command).unwrap();
            assert_eq!(written["command"], params["command"], "{params}");
            assert_eq!(parse(written.clone()).unwrap_or_else(|e| panic!("{written}: {e}")), command, "{params}");
        }
        // An absent `before` and a null one differ, and survive the round trip.
        let base = json!({"command": "window.panel.move", "panel": "history", "anchor": "layers"});
        assert!(serde_json::to_value(parse(base).unwrap()).unwrap().get("before").is_none());
        let null = json!({"command": "window.panel.move", "panel": "history", "anchor": "layers", "before": null});
        assert_eq!(serde_json::to_value(parse(null).unwrap()).unwrap()["before"], Value::Null);
    }

    #[test]
    fn side_zones_map_to_the_placement_the_anchor_container_takes_in_either_direction() {
        let command =
            |zone: &str, anchor: &str| parse(json!({"command": "window.panel.move", "panel": "history", "anchor": anchor, "zone": zone})).unwrap();
        let placed = |zone: &str, anchor: &str, rtl: bool| match command(zone, anchor).to_actions(&workspace(), rtl, &space(), &policy(false)) {
            Ok(actions) => match actions.as_slice() {
                [Action::Move { placement, .. }] => Ok(placement.clone()),
                other => panic!("{other:?}"),
            },
            Err(error) => Err(error),
        };
        // A start dock is on the left edge left to right, on the right edge right to left.
        assert_eq!(placed("right", "layers", false), Ok(DockPlacement::column(true)));
        assert_eq!(placed("right", "layers", true), Ok(DockPlacement::column(false)));
        assert_eq!(placed("left", "layers", true), Ok(DockPlacement::column(true)));
        assert_eq!(placed("top", "layers", false), Ok(DockPlacement::group(false, None)));
        // The docked tree splits beside a frame; `left` and `right` name its start and end in either direction.
        assert_eq!(placed("bottom", "canvas", false), Ok(DockPlacement::Split(DockSide::Bottom)));
        assert_eq!(placed("left", "canvas", false), Ok(DockPlacement::Split(DockSide::Start)));
        assert_eq!(placed("left", "canvas", true), Ok(DockPlacement::Split(DockSide::Start)));
        assert_eq!(placed("right", "canvas", true), Ok(DockPlacement::Split(DockSide::End)));
        // A floating window is one column, where the legacy model split it as a tree: `top` and `bottom` make a group
        // above or below, and nothing goes beside it.
        assert_eq!(placed("bottom", "color", false), Ok(DockPlacement::group(true, None)));
        assert_eq!(placed("top", "color", false), Ok(DockPlacement::group(false, None)));
        assert_eq!(placed("left", "color", false), Err(DockError::InvalidPlacement));
        assert_eq!(placed("right", "color", true), Err(DockError::InvalidPlacement));
        assert_eq!(placed("left", "nowhere", false), Err(DockError::MissingTarget));

        let (workspace, _) = applied(json!({"command": "window.panel.move", "panel": "history", "anchor": "layers", "zone": "right"}));
        let Some(Node::Split { first, .. }) = &workspace.layout().root else { panic!() };
        let Node::Dock { columns, .. } = &**first else { panic!() };
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[1].groups[0].panels, vec![s("history")]);
        // The same zone under right-to-left lands on the other side of the anchor's column.
        let mut mirrored = self::workspace();
        let command = parse(json!({"command": "window.panel.move", "panel": "history", "anchor": "layers", "zone": "right"})).unwrap();
        mirrored.apply_all(command.to_actions(&mirrored, true, &space(), &policy(false)).unwrap(), false, permissions).unwrap();
        let Some(Node::Split { first, .. }) = &mirrored.layout().root else { panic!() };
        let Node::Dock { columns, .. } = &**first else { panic!() };
        assert_eq!(columns[0].groups[0].panels, vec![s("history")]);
    }

    #[test]
    fn float_without_a_rect_takes_the_drag_size_inside_the_dock_area() {
        // The area starts under a 30 pt menu bar.
        let area = Rect::from_min_max(pos2(0.0, 30.0), pos2(800.0, 600.0));
        let sizes = sizes();
        let space = FloatSpace::new(area, &sizes).with_panel_size(s("layers"), vec2(200.0, 150.0));
        assert_eq!(space.area, area);
        let float = |space: &FloatSpace<P>, params: Value| parse(params).unwrap().to_actions(&workspace(), false, space, &policy(false)).unwrap();
        let inside = area.min + Vec2::splat(space.inset);

        // A panel's own float size, inset in the area.
        let panel = float(&space, json!({"command": "window.panel.float", "panel": "layers"}));
        assert_eq!(panel, vec![Action::float(s("layers"), space.place(vec2(200.0, 150.0)))], "float size, inset in the area");
        assert_eq!(space.place(vec2(200.0, 150.0)).min, inside);
        // A group floats at the width of the dock column it leaves and the default height, capped to the area.
        let [Action::MoveGroup { to: Target::Float { rect: group, .. }, .. }] =
            float(&space, json!({"command": "window.panel.float", "panel": "history", "group": true}))[..]
        else {
            panic!()
        };
        assert_eq!(group.min, inside);
        assert!(group.width() <= 240.01 && group.height() <= space.default_size.y + 0.01 && area.expand(0.01).contains_rect(group), "{group:?}");

        // Given values stay exactly; a lone floating window keeps its rect.
        let given = float(&space, json!({"command": "window.panel.float", "panel": "layers", "x": 5, "y": 6, "width": 100, "height": 90}));
        assert_eq!(given, vec![Action::float(s("layers"), rect([5.0, 6.0, 100.0, 90.0]))]);
        // The legacy `MoveGroup` to a `Float` without a rect now names the window's own rect.
        let color = float(&space, json!({"command": "window.panel.float", "panel": "color", "group": true}));
        assert_eq!(color, vec![Action::move_group(s("color"), Target::float(rect(COLOR)))]);

        // A unit floats shrunk into a small area.
        let small = FloatSpace::new(Rect::from_min_size(pos2(0.0, 30.0), vec2(200.0, 200.0)), &sizes);
        let [Action::MoveGroup { to: Target::Float { rect: shrunk, .. }, .. }] =
            float(&small, json!({"command": "window.panel.float", "panel": "layers", "group": true}))[..]
        else {
            panic!()
        };
        assert!(small.area.expand(0.01).contains_rect(shrunk), "{shrunk:?} in {:?}", small.area);

        // A host seeds the size of a panel the area never drew.
        let seeded = FloatSpace::new(area, &sizes).with_panel_size(s("layers"), vec2(180.0, 240.0));
        assert_eq!(
            float(&seeded, json!({"command": "window.panel.float", "panel": "layers"})),
            vec![Action::float(s("layers"), seeded.place(vec2(180.0, 240.0)))]
        );
    }

    #[test]
    fn dock_to_a_side_without_a_dock_takes_one_panel_out_of_a_shared_group() {
        let params = json!({"command": "window.panel.dock", "panel": "layers", "side": "End"});
        assert_eq!(actions(params.clone()), vec![Action::move_panel(s("layers"), Target::new_dock(DockEdge::End))]);
        let (workspace, _) = applied(params);
        let side = |panel: &str| workspace.layout().location(&s(panel)).unwrap().group.and_then(|g| g.column).map(|c| c.side);
        assert_eq!(side("layers"), Some(Some(DockEdge::End)));
        assert_eq!(side("channels"), Some(Some(DockEdge::Start)));
    }

    #[test]
    fn minimize_without_a_value_toggles_an_accordion_entry_both_ways() {
        let root = Node::stack(vec![StackEntry::new(s("layers")), StackEntry::new(s("history"))]);
        let mut workspace = Workspace::new(Layout::new(Some(root), Vec::new()));
        let toggle = parse(json!({"command": "window.panel.minimize", "panel": "layers"})).unwrap();
        apply(&toggle, &mut workspace, false, &space()).unwrap();
        assert_eq!(workspace.layout().panel_state(&s("layers")), PanelState::Open);
        apply(&toggle, &mut workspace, false, &space()).unwrap();
        assert_eq!(workspace.layout().panel_state(&s("layers")), PanelState::Front);
    }

    #[test]
    fn dock_with_a_side_zone_and_no_anchor_makes_an_outer_edge_frame() {
        let edge = |panel: &str, side| Action::move_beside(s(panel), s("layers"), DockPlacement::Edge(side));
        assert_eq!(actions(json!({"command": "window.panel.dock", "panel": "color", "zone": "left"})), vec![edge("color", DockSide::Start)]);
        assert_eq!(actions(json!({"command": "window.panel.dock", "panel": "history", "zone": "bottom"})), vec![edge("history", DockSide::Bottom)]);
        let (moved, _) = applied(json!({"command": "window.panel.dock", "panel": "color", "zone": "right"}));
        assert!(matches!(moved.layout().place(&s("color")), Some(PanelPlace::Node(_))), "{:?}", moved.layout());
        let (shown, _) = applied(json!({"command": "window.panel.dock", "panel": "swatches", "zone": "top"}));
        assert!(matches!(shown.layout().place(&s("swatches")), Some(PanelPlace::Node(_))) && shown.hidden().is_empty(), "{:?}", shown.layout());
        rejected(json!({"command": "window.panel.dock", "panel": "color", "zone": "left", "before": null}), DockError::InvalidPlacement);
        // A zone that is no side leaves the panel to dock as a tab.
        assert_eq!(
            actions(json!({"command": "window.panel.dock", "panel": "color", "zone": "above"})),
            vec![Action::move_beside(s("color"), s("layers"), tab(None))]
        );
    }

    /// The frame beside `panel`'s own: what an edge split with `panel` wrapped.
    fn wrapped(layout: &Layout<P>, panel: &str) -> Option<Node<P>> {
        let Some(PanelPlace::Node(path)) = layout.place(&s(panel)) else { return None };
        let (last, parent) = path.split_last()?;
        layout.root.as_ref().and_then(|root| node_at(root, &[parent, &[!last]].concat())).cloned()
    }

    #[test]
    fn an_outer_edge_dock_wraps_the_canvas_column_whatever_the_tree_order() {
        let tabs = |panel: &str| Node::tabs(vec![s(panel)], 0);
        let side = |first, second| Node::split(Axis::X, SplitSize::Ratio(0.5), first, second);
        let canvas_first = side(tabs("canvas"), side(tabs("layers"), tabs("history")));
        let canvas_later = side(side(tabs("layers"), tabs("canvas")), tabs("history"));
        for root in [canvas_first, canvas_later] {
            let layout = Layout::new(Some(root), vec![Floating::single(1, vec![s("color")], 0, rect(COLOR))]);
            let command = parse(json!({"command": "window.panel.dock", "panel": "color", "zone": "top"})).unwrap();
            let mut workspace = Workspace::new(layout.clone());
            apply(&command, &mut workspace, false, &space()).unwrap();
            assert_eq!(wrapped(workspace.layout(), "color"), Some(tabs("canvas")), "{:?}", workspace.layout());
            // The framework drops narrow the edge to the canvas column whether or not the area draws side edge zones
            // (`DockTo::Edge` mirrors them), where the legacy command wrapped the whole docked area there.
            let mut workspace = Workspace::new(layout);
            apply(&command, &mut workspace, false, &space().with_side_edge_zones(true)).unwrap();
            assert_eq!(wrapped(workspace.layout(), "color"), Some(tabs("canvas")), "{:?}", workspace.layout());
        }
    }

    #[test]
    fn dock_to_the_side_a_panel_already_leads_the_last_column_of() {
        // Layers leads the dock's only column: it moves out to a new column after it.
        let (workspace, _) = applied(json!({"command": "window.panel.dock", "panel": "layers", "side": "Start"}));
        assert!(matches!(workspace.layout().place(&s("layers")), Some(PanelPlace::Dock { column: 1, .. })));
        assert!(matches!(workspace.layout().place(&s("channels")), Some(PanelPlace::Dock { column: 0, .. })));
        // The whole column already is the last one: nothing to do.
        assert_eq!(actions(json!({"command": "window.panel.dock", "panel": "layers", "column": true, "side": "Start"})), vec![]);
    }

    #[test]
    fn docking_a_floated_panel_returns_it_to_the_group_it_left() {
        let mut workspace = workspace();
        let float = parse(json!({"command": "window.panel.float", "panel": "channels", "x": 50, "y": 70, "width": 330, "height": 410})).unwrap();
        apply(&float, &mut workspace, false, &space()).unwrap();
        assert!(matches!(workspace.layout().place(&s("channels")), Some(PanelPlace::Floating { .. })));
        let dock = parse(json!({"command": "window.panel.dock", "panel": "channels"})).unwrap();
        apply(&dock, &mut workspace, false, &space()).unwrap();
        let layout = workspace.layout();
        assert!(matches!(layout.place(&s("channels")), Some(PanelPlace::Dock { .. })), "{layout:?}");
        assert_eq!(layout.place(&s("channels")), layout.place(&s("layers")), "back in its own group");
    }

    #[test]
    fn showing_a_panel_never_laid_out_opens_it_in_the_first_dock_group_or_at_an_edge() {
        // "brushes" is in neither the layout nor `hidden`.
        for params in [
            json!({"command": "window.panel.activate", "panel": "brushes"}),
            json!({"command": "window.panel.dock", "panel": "brushes"}),
            json!({"command": "ui.dock", "operation": "open", "panel": "brushes"}),
        ] {
            let (shown, _) = applied(params.clone());
            assert!(
                matches!(shown.layout().place(&s("brushes")), Some(PanelPlace::Dock { column: 0, group: 0, .. })),
                "{params}: {:?}",
                shown.layout()
            );
            assert_eq!(shown.layout().panel_state(&s("brushes")), PanelState::Front, "{params}");
        }
        // No dock, and the only frame is a protected canvas: a new frame at the edge, not a tab.
        let mut workspace = Workspace::new(Layout::new(Some(Node::tabs(vec![s("canvas")], 0)), Vec::new()));
        let command = parse(json!({"command": "window.panel.activate", "panel": "brushes"})).unwrap();
        apply(&command, &mut workspace, false, &space()).unwrap();
        assert!(matches!(workspace.layout().place(&s("brushes")), Some(PanelPlace::Node(_))), "{:?}", workspace.layout());
        assert_ne!(workspace.layout().place(&s("brushes")), workspace.layout().place(&s("canvas")));
        // Without the app showing it first, a hidden panel is not a layout action.
        let hidden = parse(json!({"command": "window.panel.activate", "panel": "swatches"})).unwrap();
        assert_eq!(hidden.to_actions(&self::workspace(), false, &space(), &policy(false)), Err(DockError::MissingPanel));
    }
}
