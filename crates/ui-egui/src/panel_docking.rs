//! Application-owned panel IDs and commands around the shared docking layout.

use craft_ui::docking::{Action, Floating, Layout, Node, Placement, Zone};
use serde_json::{Value, json};

use crate::DesignApp;

#[derive(Clone, Default)]
struct LegacyDrag {
    layout: Option<Layout<String>>,
    open_panel: Option<String>,
    floating: Vec<(String, [f32; 2])>,
    generation: u64,
}

pub(crate) fn cancel_gesture(app: &mut DesignApp, ctx: &egui::Context) {
    cancel_legacy_drag(app, ctx);
    craft_ui::docking::cancel_drag::<String>(ctx, egui::Id::new("designcraft-panel-docking"));
}

fn cancel_legacy_drag(app: &mut DesignApp, ctx: &egui::Context) -> bool {
    let key = egui::Id::new("designcraft-panel-docking").with("legacy-origin");
    let previous = ctx.data_mut(|data| data.remove_temp::<LegacyDrag>(key));
    if let Some(previous) = previous {
        craft_ui::docking::cancel_drag::<String>(ctx, egui::Id::new("designcraft-panel-docking"));
        if previous.generation != app.ui.docking_generation {
            return false;
        }
        app.ui.open_panel = previous.open_panel;
        app.ui.floating = previous.floating;
        app.ui.docking = previous.layout;
        true
    } else {
        false
    }
}

fn normalize(raw: &str) -> Option<&'static str> {
    crate::dock::DOCK_TABS
        .iter()
        .chain(crate::dock::ICON_PANELS)
        .find(|(id, label, _)| id.eq_ignore_ascii_case(raw.trim()) || label.eq_ignore_ascii_case(raw.trim()))
        .map(|(id, _, _)| *id)
}

pub(crate) fn valid(layout: &Layout<String>) -> bool {
    layout.validate().is_ok() && layout.panels().iter().all(|id| normalize(id) == Some(id.as_str()))
}

fn legacy(app: &DesignApp) -> Layout<String> {
    let panels: Vec<String> = crate::dock::DOCK_TABS.iter().map(|p| p.0.to_string()).collect();
    let active = panels.iter().position(|p| p == &app.ui.dock_tab).unwrap_or(0);
    let mut layout = Layout { root: Some(Node::Tabs { panels, active }), floating: Vec::new() };
    for (id, at) in &app.ui.floating {
        if normalize(id).is_some() && !layout.contains(id) && at.iter().all(|v| v.is_finite()) {
            layout.floating.push(Floating { panels: vec![id.clone()], active: 0, rect: [at[0], at[1], 280.0, 360.0] });
        }
    }
    if let Some(id) = &app.ui.open_panel
        && normalize(id).is_some()
        && !layout.contains(id)
    {
        // Preserve the visible flyout when the first unrelated docking gesture succeeds.
        if let Some(Node::Tabs { panels, active }) = &mut layout.root {
            panels.push(id.clone());
            *active = panels.len().saturating_sub(1);
        }
    }
    layout
}

fn first_root(layout: &Layout<String>, except: &str) -> Option<String> {
    let root = Layout { root: layout.root.clone(), floating: Vec::new() };
    root.panels().into_iter().find(|p| p.as_str() != except).cloned()
}

fn ensure(layout: &mut Layout<String>, panel: &str) -> Result<(), String> {
    let anchor = first_root(layout, panel);
    layout.apply(Action::Open { panel: panel.into(), anchor }).map_err(|e| e.to_string())
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

fn placement(params: &Value) -> Result<Placement<String>, String> {
    if let Some(before) = params.get("before") {
        let id = before.as_str().and_then(normalize).ok_or("before must name a panel")?;
        return Ok(Placement::Tab { before: Some(id.into()) });
    }
    let zone = match params.get("zone") {
        None => "tab",
        Some(value) => value.as_str().ok_or("zone must be a string")?,
    };
    Ok(match zone {
        "tab" => Placement::Tab { before: None },
        "center" => Placement::Split(Zone::Center),
        "left" => Placement::Split(Zone::Left),
        "right" => Placement::Split(Zone::Right),
        "top" => Placement::Split(Zone::Top),
        "bottom" => Placement::Split(Zone::Bottom),
        _ => return Err("zone must be tab, center, left, right, top or bottom".into()),
    })
}

pub(crate) fn command(app: &mut DesignApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id == "app.tablePanel" && app.ui.docking.is_some() {
        let panel = "table".to_string();
        let visible = app.ui.docking.as_ref().is_some_and(|layout| layout.contains(&panel));
        return Some(change(app, if visible { "close" } else { "activate" }, &json!({"panel": panel})));
    }
    if id == "window.panel.layout" {
        return Some(
            params
                .get("action")
                .ok_or_else(|| "action is required".to_string())
                .and_then(|value| serde_json::from_value::<Action<String>>(value.clone()).map_err(|error| error.to_string()))
                .and_then(|action| apply_action(app, action)),
        );
    }
    let operation = match id {
        "window.panel.move" => "move",
        "window.panel.float" | "window.floatPanel" => "float",
        "window.panel.dock" | "window.dockPanel" => "dock",
        "window.panel.activate" => "activate",
        "window.panel.close" => "close",
        "window.panel" if app.ui.docking.is_some() => "activate",
        _ => return None,
    };
    Some(change(app, operation, params))
}

// UI output and automation share this atomic dispatcher. The application owns the registry
// and saved return locations; craft-ui owns tree validation and structural changes.
fn apply_action(app: &mut DesignApp, action: Action<String>) -> Result<Value, String> {
    let registered = |id: &String| normalize(id) == Some(id.as_str());
    let known = match &action {
        Action::Move { panel, anchor, placement } | Action::OpenAt { panel, anchor, placement } => {
            registered(panel)
                && registered(anchor)
                && match placement {
                    Placement::Tab { before: Some(id) } => registered(id),
                    _ => true,
                }
        }
        Action::Open { panel, anchor } => registered(panel) && anchor.as_ref().is_none_or(registered),
        Action::Float { panel, .. }
        | Action::Close { panel }
        | Action::Activate { panel }
        | Action::MoveFloating { panel, .. }
        | Action::SetStackOpen { panel, .. }
        | Action::ResizeStack { panel, .. } => registered(panel),
        Action::ResizeSplit { path, .. } => path.len() <= craft_ui::docking::MAX_DEPTH,
    };
    if !known {
        return Err("docking action contains an unknown panel or an excessive split path".into());
    }
    if app.ui.docking.as_ref().is_some_and(|layout| !valid(layout)) {
        return Err("saved panel layout is invalid; reset the workspace first".into());
    }
    let mut layout = app.ui.docking.clone().unwrap_or_else(|| legacy(app));
    let returning = match &action {
        Action::Close { panel } => layout.location(panel).ok().map(|location| (panel.clone(), location)),
        Action::Float { panel, .. } => {
            layout.location(panel).ok().filter(|location| location.floating.is_none()).map(|location| (panel.clone(), location))
        }
        _ => None,
    };
    let reveal = match &action {
        Action::Activate { panel } | Action::Open { panel, .. } | Action::OpenAt { panel, .. } => Some(panel.clone()),
        _ => None,
    };
    layout.apply(action).map_err(|error| error.to_string())?;
    if let Some((panel, location)) = returning {
        app.ui.docking_hidden.insert(panel, location);
    }
    app.ui.docking = Some(layout);
    sync_legacy(app);
    if let Some(panel) = reveal {
        app.ui.docking_opened = Some(panel.clone());
        app.ui.docking_raise = Some(panel);
        app.ui.dock_expanded = true;
    } else if app.ui.docking_opened.as_ref().is_some_and(|panel| !app.ui.docking.as_ref().is_some_and(|layout| layout.contains(panel))) {
        app.ui.docking_opened = None;
    }
    app.ui.open_panel = None;
    Ok(json!({"layout": app.ui.docking}))
}

fn change(app: &mut DesignApp, operation: &str, params: &Value) -> Result<Value, String> {
    let panel = params.get("panel").and_then(Value::as_str).and_then(normalize).ok_or("panel must name a registered panel")?;
    if app.ui.docking.as_ref().is_some_and(|layout| !valid(layout)) {
        return Err("saved panel layout is invalid; reset the workspace first".into());
    }
    let mut layout = app.ui.docking.clone().unwrap_or_else(|| legacy(app));
    let mut hidden: std::collections::BTreeMap<_, _> =
        app.ui.docking_hidden.iter().filter(|(id, _)| normalize(id).is_some()).take(64).map(|(id, at)| (id.clone(), at.clone())).collect();
    let mut restored = false;
    if operation == "dock"
        && params.get("anchor").or_else(|| params.get("onto")).is_none()
        && let Some(location) = hidden.get(panel).filter(|location| {
            location.floating.is_none()
                && location.anchor.as_ref().is_none_or(|anchor| Layout { root: layout.root.clone(), floating: Vec::new() }.contains(anchor))
        })
    {
        let mut candidate = layout.clone();
        if candidate.contains(&panel.to_string()) {
            candidate.apply(Action::Close { panel: panel.into() }).map_err(|e| e.to_string())?;
        }
        if candidate.restore(panel.into(), location).is_ok() {
            layout = candidate;
            restored = true;
            hidden.remove(panel);
        }
    }
    match operation {
        "dock" if restored => {}
        "float" => {
            let rect = [number(params, "x", 400.0)?, number(params, "y", 140.0)?, number(params, "width", 300.0)?, number(params, "height", 400.0)?];
            ensure(&mut layout, panel)?;
            if let Ok(location) = layout.location(&panel.to_string())
                && location.floating.is_none()
            {
                hidden.insert(panel.into(), location);
            }
            layout.apply(Action::Float { panel: panel.into(), rect }).map_err(|e| e.to_string())?;
        }
        "move" | "dock" => {
            if operation == "move" && params.get("anchor").or_else(|| params.get("onto")).is_none() {
                return Err("move requires a destination panel".into());
            }
            let anchor = match params.get("anchor").or_else(|| params.get("onto")) {
                Some(v) => Some(v.as_str().and_then(normalize).ok_or("anchor must name a registered panel")?.to_string()),
                None => first_root(&layout, panel),
            };
            let placement = placement(params)?;
            if let Some(anchor) = anchor {
                ensure(&mut layout, panel)?;
                layout.apply(Action::Move { panel: panel.into(), anchor, placement }).map_err(|e| e.to_string())?;
            } else if operation == "dock" {
                if layout.contains(&panel.to_string()) {
                    layout.apply(Action::Close { panel: panel.into() }).map_err(|e| e.to_string())?;
                }
                layout.apply(Action::Open { panel: panel.into(), anchor: None }).map_err(|e| e.to_string())?;
            } else {
                return Err("move requires an existing destination panel".into());
            }
        }
        "close" => {
            hidden.insert(panel.into(), layout.location(&panel.to_string()).map_err(|e| e.to_string())?);
            layout.apply(Action::Close { panel: panel.into() }).map_err(|e| e.to_string())?;
        }
        _ => {
            if !layout.contains(&panel.to_string())
                && let Some(location) = hidden.get(panel)
            {
                // A missing neighbor is allowed: the normal visible dock becomes the fallback.
                let _ = layout.restore(panel.into(), location);
            }
            ensure(&mut layout, panel)?;
            hidden.remove(panel);
        }
    }
    app.ui.docking = Some(layout);
    app.ui.docking_hidden = hidden;
    sync_legacy(app);
    app.ui.open_panel = None;
    app.ui.docking_opened = if operation == "close" { None } else { Some(panel.to_owned()) };
    app.ui.docking_raise = if operation == "close" { None } else { Some(panel.to_owned()) };
    if operation != "close" {
        app.ui.dock_expanded = true;
    }
    Ok(json!({"panel": panel, "floating": app.ui.floating.iter().map(|(id, _)| id).collect::<Vec<_>>(), "layout": app.ui.docking}))
}

pub(crate) fn show(app: &mut DesignApp, ui: &mut egui::Ui) -> bool {
    let area = egui::Id::new("designcraft-panel-docking");
    let changed_workspace = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<u64>(area.with("generation"));
        data.insert_temp(area.with("generation"), app.ui.docking_generation);
        previous.is_some_and(|generation| generation != app.ui.docking_generation)
    });
    if changed_workspace {
        craft_ui::docking::cancel_drag::<String>(ui.ctx(), area);
        ui.ctx().data_mut(|data| data.remove::<LegacyDrag>(area.with("legacy-origin")));
    }

    let cancelled =
        ui.input(|input| !input.focused || input.key_pressed(egui::Key::Escape) || (!input.pointer.primary_down() && !input.pointer.any_released()));
    if cancelled && cancel_legacy_drag(app, ui.ctx()) && app.ui.docking.is_none() {
        return false;
    }
    let Some(layout) = app.ui.docking.as_ref() else { return false };
    if !valid(layout) {
        app.ui.docking = None;
        app.status("Invalid panel layout; restored the default dock");
        return false;
    }
    let mut layout = layout.clone();
    if !app.ui.dock_expanded {
        layout.root = None;
    }
    let raise = app.ui.docking_raise.take();
    let mut blocked = Vec::new();
    if app.ui.dialog.is_some() || app.ui.about || app.ui.palette.is_some() || egui::Popup::is_any_open(ui.ctx()) {
        blocked.push(ui.ctx().content_rect());
    } else if app.story_editor.is_some() {
        // The editor is rendered after the dock; block its unmeasured first frame conservatively.
        let rect = ui.ctx().memory(|m| m.area_rect(egui::Id::new("story_editor"))).unwrap_or_else(|| ui.ctx().content_rect());
        blocked.push(rect);
    }
    let t = crate::theme::Tokens::get(ui.ctx());
    let language = app.ui.language.clone();
    let mut draw = |ui: &mut egui::Ui| {
        let mut style = craft_ui::docking::DockStyle::from_ui(ui);
        style.tab_height = 27.0;
        style.background = t.panel;
        style.tab_background = t.tab_strip;
        style.active_background = t.panel;
        style.text = t.text_strong;
        style.inactive_text = t.text_dim;
        style.border = egui::Stroke::new(1.0, t.border);
        style.accent = t.accent;
        style.float_label = crate::i18n::tr(&language, "Float Panel").to_owned();
        style.close_label = crate::i18n::tr(&language, "Close Panel").to_owned();
        style.move_label = crate::i18n::tr(&language, "Group with").to_owned();
        style.panels_label = crate::i18n::tr(&language, "Panels").to_owned();
        style.resize_label = crate::i18n::tr(&language, "Resize panels").to_owned();
        style.resize_window_label = crate::i18n::tr(&language, "Resize panel window").to_owned();
        let mut area = craft_ui::docking::DockArea::new(egui::Id::new("designcraft-panel-docking")).blocked_rects(&blocked);
        if let Some(panel) = &raise {
            area = area.raise_panel(panel.clone());
        }
        area.show_customized(
            ui,
            &layout,
            &style,
            |id| {
                crate::i18n::tr(
                    &language,
                    crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS).find(|p| p.0 == id).map(|p| p.1).unwrap_or(id),
                )
                .to_owned()
            },
            |_| craft_ui::docking::Permissions::default(),
            |_| craft_ui::docking::PanelLimits { min: egui::vec2(220.0, 80.0), ..Default::default() },
            &mut PanelContent(app),
        )
    };
    let output = if layout.root.is_some() {
        egui::Panel::right("shared-panel-dock")
            .default_size(310.0)
            .size_range(220.0..=720.0)
            .resizable(true)
            .frame(egui::Frame::NONE.fill(t.panel))
            .show(ui, &mut draw)
            .inner
    } else {
        draw(ui)
    };
    if let Some(panel) = output.focused {
        app.ui.docking_opened = Some(panel);
    }
    if let Some(error) = output.error {
        app.ui.status = error.to_string();
    }
    if ui.input(|input| input.pointer.any_released()) {
        if !output.actions.iter().any(|action| matches!(action, Action::Move { .. } | Action::Float { .. })) && cancel_legacy_drag(app, ui.ctx()) {
            return app.ui.docking.is_some();
        }
        ui.ctx().data_mut(|data| data.remove::<LegacyDrag>(egui::Id::new("designcraft-panel-docking").with("legacy-origin")));
    }
    for action in output.actions {
        if let Err(error) = app.run("window.panel.layout", json!({"action": action})) {
            app.ui.status = error;
        }
    }
    icon_rail(app, ui);
    true
}

fn icon_rail(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let column = egui::Panel::right("shared-panel-icons")
        .exact_size(38.0)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(t.panel))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("shared-panel-icons-scroll").show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for &(id, label, icon) in crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS) {
                    if app.ui.docking.as_ref().is_some_and(|layout| {
                        layout.contains(&id.to_string()) && (app.ui.dock_expanded || layout.floating.iter().any(|g| g.panels.iter().any(|p| p == id)))
                    }) {
                        continue;
                    }

                    let response =
                        crate::icons::button(ui, icon, 28.0, false, crate::i18n::tr(&app.ui.language, label)).interact(egui::Sense::click_and_drag());
                    if response.clicked() {
                        let _ = app.run("window.panel.activate", json!({"panel":id}));
                    }
                    legacy_tab(app, ui, id, &response);
                }
            });
        })
        .response
        .rect;
    let _ = column;
}

pub(crate) fn legacy_tab(app: &mut DesignApp, ui: &mut egui::Ui, panel: &str, response: &egui::Response) {
    let Some(panel) = normalize(panel) else { return };
    let language = app.ui.language.clone();
    response.context_menu(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&language, "Float Panel"))).clicked() {
            if let Err(error) = app.run("window.panel.float", json!({"panel": panel})) {
                app.status(error);
            }
            ui.close();
        }
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
    });
    if response.drag_started()
        && let Some(id) = normalize(panel)
    {
        let mut layout = app.ui.docking.clone().unwrap_or_else(|| legacy(app));
        let bounds = ui.max_rect();
        let source = egui::Rect::from_min_size(
            egui::pos2(bounds.left(), response.rect.top()),
            egui::vec2(bounds.width().clamp(240.0, 600.0), bounds.height().clamp(200.0, 600.0)),
        );
        if ensure(&mut layout, id).is_ok()
            && craft_ui::docking::begin_drag(ui.ctx(), egui::Id::new("designcraft-panel-docking"), id.to_string(), source)
        {
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("designcraft-panel-docking").with("legacy-origin"),
                    LegacyDrag {
                        layout: app.ui.docking.clone(),
                        open_panel: app.ui.open_panel.clone(),
                        floating: app.ui.floating.clone(),
                        generation: app.ui.docking_generation,
                    },
                )
            });
            app.ui.docking = Some(layout);
            app.ui.open_panel = None;
        }
    }
}

struct PanelContent<'a>(&'a mut DesignApp);
impl craft_ui::docking::DockContent<String> for PanelContent<'_> {
    fn body(&mut self, ui: &mut egui::Ui, id: &String) {
        egui::Frame::NONE.inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("shared-panel-body", id))
                .auto_shrink([false, false])
                .show(ui, |ui| crate::dock::panel_body(self.0, ui, id));
        });
    }
    fn label(&mut self, ui: &egui::Ui, _: &String, logical: &str, font: &egui::FontId, color: egui::Color32) -> egui::WidgetText {
        egui::WidgetText::Galley(crate::rtl::plain(ui.ctx(), logical, font.clone(), color))
    }
    fn action_label(&mut self, ui: &egui::Ui, logical: &str, font: &egui::FontId, color: egui::Color32) -> egui::WidgetText {
        egui::WidgetText::Galley(crate::rtl::plain(ui.ctx(), logical, font.clone(), color))
    }
}

/// Close the last explicitly opened panel through the same validated layout transaction.
pub(crate) fn close_visible(app: &mut DesignApp) -> Result<(), String> {
    if app.ui.docking.is_some() {
        if let Some(panel) = app.ui.docking_opened.clone() {
            change(app, "close", &json!({"panel": panel}))?;
        }
    } else {
        app.ui.open_panel = None;
    }
    Ok(())
}

// Legacy inspection fields are derived from the shared layout after customization.
fn sync_legacy(app: &mut DesignApp) {
    if let Some(layout) = &app.ui.docking {
        app.ui.floating =
            layout.floating.iter().flat_map(|group| group.panels.iter().map(|id| (id.clone(), [group.rect[0], group.rect[1]]))).collect();
    }
}

#[cfg(test)]
#[path = "panel_docking_tests.rs"]
mod tests;
