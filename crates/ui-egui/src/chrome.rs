//! Window chrome: application bar (with menus), Control panel, document tabs, status bar, start screen.

use designcraft_doc::{Align, Content};
use designcraft_geom::Unit;
use egui::{Color32, Sense, Stroke, vec2};
use serde_json::json;

use crate::panels;
use crate::theme::{Tokens, semibold};
use crate::widgets::{caption, measure, number};
use crate::{DesignApp, icons};

/// Height of the application bar (InDesign 2026), then a 7 pt lower band and a 1 pt border. The
/// band covers the panel's 1 pt separator line, so the frame's bottom margin is 1 pt short of it.
const APP_BAR: f32 = 36.0;
const APP_BAR_BAND: f32 = 7.0;
const SEPARATOR: f32 = 1.0;
/// Height of the application bar on macOS, where it is the window's title bar (#333). AppKit draws
/// the traffic lights itself, centred in the standard 28 pt title-bar band at the top of the
/// window; moving them would take AppKit calls (`unsafe`, which the workspace forbids). So the bar
/// is that band: the lights sit vertically centred in it, with the bar's controls beside them.
pub const MAC_APP_BAR: f32 = 28.0;
/// The traffic lights as AppKit lays them out: button diameter, centre-to-centre pitch and the
/// first button's left edge (8–10 pt depending on the macOS version; the larger value keeps the
/// gap after them from shrinking).
const MAC_LIGHT: f32 = 12.0;
const MAC_LIGHT_PITCH: f32 = 20.0;
const MAC_LIGHTS_LEFT: f32 = 10.0;
/// Where the bar's content starts to the right of the traffic lights: the end of the three lights
/// plus the same gap the lights keep from the top of the window.
pub const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_APP_BAR - MAC_LIGHT) / 2.0;

fn search_id() -> egui::Id {
    egui::Id::new("app_bar_search")
}

/// Whether the application bar's search field had the keyboard last frame. Escape takes egui's
/// focus away before any widget runs, so that press must not reach the global shortcuts.
pub fn search_had_focus(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.had_focus_last_frame(search_id()))
}

/// The search field's state between frames: the query, the highlighted match, whether the matches
/// are shown, and the widget that had the keyboard before the field took it.
#[derive(Clone, Default)]
struct SearchState {
    query: String,
    highlight: usize,
    open: bool,
    previous_focus: Option<egui::Id>,
}

/// The application bar's search field: finds menu commands by their labels in the interface
/// language. ↑/↓ move through the matches; Return or a click runs one as its menu item would.
/// Running one or pressing Escape clears the field and gives the keyboard back to the widget that
/// had it. While the field has the keyboard, no key reaches the shortcuts or the document.
fn search_field(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let id = search_id();
    let state_id = id.with("state");
    let mut st: SearchState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let (r, _) = ui.allocate_exact_size(vec2(125.0, 18.0), Sense::hover());
    ui.painter().rect(r, 1.0, t.input, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    icons::paint(ui.painter(), egui::Rect::from_min_size(r.min + vec2(3.0, 2.0), vec2(14.0, 14.0)), "search", t.icon);
    let focused = ui.memory(|m| m.has_focus(id));
    let had_focus = search_had_focus(ui.ctx());
    if !focused && !had_focus {
        st.previous_focus = ui.memory(|m| m.focused());
    }
    let none = egui::Modifiers::NONE;
    let (down, up, enter, escape) = if focused || had_focus {
        ui.input_mut(|i| {
            (
                i.consume_key(none, egui::Key::ArrowDown),
                i.consume_key(none, egui::Key::ArrowUp),
                i.consume_key(none, egui::Key::Enter),
                i.consume_key(none, egui::Key::Escape),
            )
        })
    } else {
        (false, false, false, false)
    };
    let lang = app.ui.language.clone();
    let hint = crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&lang, "Search")).color(t.text_dim));
    let field = egui::Rect::from_min_max(r.min + vec2(19.0, 0.0), r.max - vec2(3.0, 0.0));
    let resp = ui
        .put(
            field,
            egui::TextEdit::singleline(&mut st.query)
                .id(id)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(egui::FontId::proportional(11.0))
                .text_color(t.text)
                .hint_text(hint)
                .desired_width(field.width())
                .vertical_align(egui::Align::Center)
                // Return runs the highlighted match instead of leaving the field.
                .return_key(None),
        )
        .on_hover_text(crate::i18n::tr(&lang, "Search commands"));
    if resp.changed() {
        st.highlight = 0;
        st.open = true;
    }
    if resp.gained_focus() {
        st.open = true;
    }
    let items = if st.open { crate::menus::command_search(app, &st.query) } else { Vec::new() };
    let last = items.len().saturating_sub(1);
    if down {
        st.highlight = st.highlight.saturating_add(1);
    }
    if up {
        st.highlight = st.highlight.saturating_sub(1);
    }
    st.highlight = st.highlight.min(last);
    let mut run = None;
    if enter && let Some(it) = items.get(st.highlight).filter(|it| crate::menus::menu_enabled(app, &it.id)) {
        run = Some(it.id.clone());
    }
    let mut over_matches = false;
    if st.open && !st.query.trim().is_empty() {
        let shown = egui::Area::new(id.with("matches"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(r.right_bottom() + vec2(0.0, 2.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    let size = vec2(300.0, 22.0);
                    ui.set_width(size.x);
                    if items.is_empty() {
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "No matching commands")).color(t.text_dim));
                    }
                    for (k, it) in items.iter().enumerate() {
                        let mut b = egui::Button::new(crate::rtl::widget(ui, it.label.as_str())).frame(false).min_size(size);
                        if let Some(sc) = &it.shortcut {
                            b = b.shortcut_text(crate::menus::shortcut_text(sc));
                        }
                        if k == st.highlight {
                            ui.painter().rect_filled(egui::Rect::from_min_size(ui.cursor().min, size), 2.0, t.hover);
                        }
                        let enabled = crate::menus::menu_enabled(app, &it.id);
                        // The button aligns its label by the layout it's in: left, like the menus.
                        let row = ui.allocate_ui_with_layout(
                            size,
                            egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
                            |ui| ui.add_enabled(enabled, b),
                        );
                        if row.inner.clicked() {
                            run = Some(it.id.clone());
                        }
                    }
                });
            });
        over_matches = shown.response.contains_pointer();
    }
    if escape || run.is_some() {
        st.query.clear();
        st.open = false;
        let previous = st.previous_focus.take();
        ui.memory_mut(|m| match previous {
            Some(p) => m.request_focus(p),
            None => m.surrender_focus(id),
        });
    } else if !resp.has_focus() && !over_matches {
        // A click elsewhere or Tab took the keyboard: the matches close, the query stays.
        st.open = false;
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
    if let Some(cmd) = run {
        crate::menus::activate(app, &cmd, &serde_json::Value::Null);
    }
}

/// Width of the app bar's right-hand slot: what is left of the row, minus a pixel of slack so
/// that rounding at fractional scales (115%) never makes the row a hair wider than the scroll
/// area and shows a needless scroll bar. Never narrower than the controls themselves.
fn tools_slot_width(available: f32, tools_w: f32) -> f32 {
    (available - 1.0).max(tools_w)
}

pub fn app_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let mac = app.integrated_titlebar;
    let left = if mac { MAC_CONTENT_LEFT as i8 } else { 8 };
    let bar = if mac { MAC_APP_BAR } else { APP_BAR };
    // The tallest control keeps a margin above and below it in the shorter macOS bar.
    let home = if mac { 22.0 } else { 24.0 };
    let resp = egui::Panel::top("app_bar")
        .exact_size(bar + APP_BAR_BAND)
        .frame(egui::Frame::NONE.fill(t.app_bar).inner_margin(egui::Margin {
            left,
            right: if app.custom_titlebar { 0 } else { 10 },
            top: 0,
            bottom: (APP_BAR_BAND - SEPARATOR) as i8,
        }))
        .show(ui, |ui| {
            let full = ui.max_rect();
            if mac {
                // The whole title-bar band, margins included; the controls drawn below take
                // their own clicks.
                let band = egui::Rect::from_min_max(
                    egui::pos2(full.min.x - f32::from(left), full.min.y),
                    egui::pos2(full.max.x + 10.0, full.min.y + MAC_APP_BAR),
                );
                crate::title_bar::empty_area(ui, band);
            }
            let mut menus_end = full.min.x;
            let mut tools_start = full.max.x;
            // Windows caption buttons stay at the right edge; the rest of the bar scrolls.
            let caption_w = if app.custom_titlebar { CAPTION_W } else { 0.0 };
            // Undecorated window: the bar moves it. The drag area is the whole bar but the caption
            // buttons, lower band included, registered before the menus and tools so they stay on
            // top of it: any gap drags, and the lower band is always there, however narrow the
            // window (the menus and tools scroll).
            if app.custom_titlebar {
                let drag = egui::Rect::from_min_max(
                    egui::pos2(full.min.x - left as f32, full.min.y),
                    egui::pos2(full.max.x - caption_w, full.min.y + bar + APP_BAR_BAND),
                );
                let response = ui.interact(drag, egui::Id::new("window_title_drag"), Sense::click_and_drag());
                // Only from an empty part of the bar: the lower band, or the gap between the menus
                // and the tools (as they were last frame). A menu title pressed and dragged doesn't
                // move the window, as with native title bars.
                let (gap0, gap1) =
                    ui.data(|d| (d.get_temp::<f32>(egui::Id::new("app_bar_menus_end")), d.get_temp::<f32>(egui::Id::new("app_bar_tools_start"))));
                let empty = |p: egui::Pos2| p.y >= full.min.y + bar || matches!((gap0, gap1), (Some(a), Some(b)) if p.x >= a && p.x <= b);
                let from_empty = ui.input(|i| i.pointer.press_origin().or(i.pointer.interact_pos())).is_some_and(empty);
                if response.double_clicked() && from_empty {
                    let on = !ui.input(|i| i.viewport().maximized.unwrap_or(false));
                    let _ = app.run("window.maximize", json!({"on": on}));
                } else if response.drag_started_by(egui::PointerButton::Primary) && from_empty {
                    let _ = app.run("window.drag", json!({}));
                }
                app.flush_window_commands(ui.ctx());
            }
            if app.custom_titlebar {
                let caption = egui::Rect::from_min_max(egui::pos2(full.max.x - caption_w, full.min.y), full.max);
                ui.scope_builder(egui::UiBuilder::new().max_rect(caption).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
                    caption_buttons(app, ui);
                });
            }
            let scrolled = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x - caption_w, full.max.y));
            ui.scope_builder(egui::UiBuilder::new().max_rect(scrolled), |ui| {
                // The right-hand controls' width, measured last frame: in a window too narrow for them
                // and the menus they follow the menus, and the bar scrolls sideways.
                let tools_w_id = ui.id().with("tools_width");
                let tools_w: f32 = ui.data(|d| d.get_temp(tools_w_id)).unwrap_or(0.0);
                crate::widgets::overflow_scrolling(ui);
                // (Drag-scrolling is touch-only by default, so a mouse drag on the bar moves the window.)
                egui::ScrollArea::horizontal().id_salt("app_bar_scroll").auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        if icons::button(ui, "home", home, app.session.active().is_none(), crate::i18n::tr(&app.ui.language, "Home")).clicked() {
                            app.session_home();
                        }
                        ui.add_space(6.0);
                        if !app.native_menu {
                            ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                            ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
                            ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE;
                            ui.style_mut().spacing.button_padding = vec2(7.0, 3.0);
                            crate::menus::menu_bar(app, ui);
                        }
                        menus_end = ui.min_rect().max.x;
                        // Where the menus end on screen (moves as a narrow bar scrolls).
                        ui.data_mut(|d| d.insert_temp(egui::Id::new("app_bar_menus_end"), menus_end));
                        let size = vec2(tools_slot_width(ui.available_width(), tools_w), ui.available_height());
                        let tools = ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);
                            search_field(app, ui);
                            ui.add_space(8.0);
                            let current = app.ui.workspace.clone();
                            let shown = crate::i18n::workspace_name(&app.ui.language, &current);
                            crate::menus::menu_button(
                                ui,
                                crate::rtl::widget(ui, egui::RichText::new(format!("{shown} ▾")).font(semibold(11.5)).color(t.text)),
                                |ui| {
                                    let customs: Vec<String> = app.ui.custom_workspaces.iter().map(|w| w.name.clone()).collect();
                                    for w in crate::menus::WORKSPACES.iter().map(|w| w.to_string()).chain(customs.iter().cloned()) {
                                        if ui
                                            .selectable_label(w == current, crate::rtl::widget(ui, crate::i18n::workspace_name(&app.ui.language, &w)))
                                            .clicked()
                                        {
                                            let _ = app.run("window.workspace", json!({"name": w}));
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    if ui
                                        .button(crate::rtl::widget(
                                            ui,
                                            format!(
                                                "{} {}",
                                                crate::i18n::tr(&app.ui.language, "Reset"),
                                                crate::i18n::workspace_name(&app.ui.language, &current)
                                            ),
                                        ))
                                        .clicked()
                                    {
                                        let _ = app.run("window.resetWorkspace", json!({}));
                                        ui.close();
                                    }
                                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Workspace…"))).clicked() {
                                        let _ = app.run("window.newWorkspace", json!({}));
                                        ui.close();
                                    }
                                    if !customs.is_empty() {
                                        crate::menus::menu_button(
                                            ui,
                                            crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Workspace")),
                                            |ui| {
                                                for w in &customs {
                                                    if ui.button(w).clicked() {
                                                        let _ = app.run("window.deleteWorkspace", json!({"name": w}));
                                                        ui.close();
                                                    }
                                                }
                                            },
                                        );
                                    }
                                },
                            );
                            ui.add_space(6.0);
                            if icons::button(ui, "share", 22.0, false, crate::i18n::tr(&app.ui.language, "Share")).clicked() {
                                app.status(crate::i18n::tr(&app.ui.language, "Export a PDF, IDML or package to share — no cloud account needed."));
                            }
                            ui.add_space(8.0);
                            // Always one click away: the ArtCraft community Discord.
                            if crate::about::discord_button(ui, "Discord", vec2(78.0, 22.0)) {
                                let _ = app.run("help.discord", json!({}));
                            }
                            tools_start = ui.min_rect().min.x;
                            ui.data_mut(|d| d.insert_temp(egui::Id::new("app_bar_tools_start"), tools_start));
                            ui.min_rect().width()
                        });
                        if (tools.inner - tools_w).abs() > 0.5 {
                            ui.data_mut(|d| d.insert_temp(tools_w_id, tools.inner));
                            ui.ctx().request_discard("app bar: the right-hand controls changed width");
                        }
                    });
                });
            });
            // Centred title.
            let title = match app.session.active() {
                Some(d) => format!("DesignCraft — {}", crate::rtl::isolate(&d.title())),
                None => "DesignCraft".to_string(),
            };
            // Centred between the menus and the right-hand controls; left out when it won't fit.
            let galley = crate::rtl::plain(ui.ctx(), &title, egui::FontId::proportional(11.5), t.text);
            let w = galley.size().x;
            let cx = full.center().x;
            if cx - w / 2.0 > menus_end + 12.0 && cx + w / 2.0 < tools_start - 12.0 {
                ui.painter().galley(egui::pos2(cx - w / 2.0, full.center().y - galley.size().y / 2.0), galley, t.text);
            }
        });
    let r = resp.response.rect;
    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(r.min.x, r.max.y - APP_BAR_BAND), r.max), 0.0, t.pasteboard);
    ui.painter().line_segment([egui::pos2(r.min.x, r.max.y), egui::pos2(r.max.x, r.max.y)], Stroke::new(1.0, t.border));
}

const CAPTION_BUTTON_W: f32 = 46.0;
/// Minimize, maximize and close, and the space after them.
const CAPTION_W: f32 = 3.0 * CAPTION_BUTTON_W + 8.0;

/// Windows caption buttons. The macOS traffic lights remain native.
fn caption_buttons(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let spacing = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 0.0;
    for (command, label) in
        [("window.close", "Close"), ("window.maximize", if maximized { "Restore" } else { "Maximize" }), ("window.minimize", "Minimize")]
    {
        let (rect, _) = ui.allocate_exact_size(vec2(CAPTION_BUTTON_W, APP_BAR), Sense::hover());
        let response = ui.interact(rect, egui::Id::new(command), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
        let hovered = response.hovered();
        if hovered {
            ui.painter().rect_filled(rect, 0.0, if command == "window.close" { t.caption_close } else { t.hover });
        }
        let color = if hovered && command == "window.close" { t.caption_close_text } else { t.text };
        let stroke = Stroke::new(1.0, color);
        let icon = egui::Rect::from_center_size(rect.center(), vec2(10.0, 10.0));
        match command {
            "window.close" => {
                ui.painter().line_segment([icon.left_top(), icon.right_bottom()], stroke);
                ui.painter().line_segment([icon.right_top(), icon.left_bottom()], stroke);
            }
            "window.maximize" if maximized => {
                let back = icon.translate(vec2(2.0, -2.0));
                ui.painter().line_segment([back.left_top(), back.right_top()], stroke);
                ui.painter().line_segment([back.right_top(), back.right_bottom()], stroke);
                ui.painter().rect_stroke(icon, 0.0, stroke, egui::StrokeKind::Inside);
            }
            "window.maximize" => {
                ui.painter().rect_stroke(icon, 0.0, stroke, egui::StrokeKind::Inside);
            }
            _ => {
                ui.painter().line_segment([icon.left_center(), icon.right_center()], stroke);
            }
        }
        if response.clicked() {
            let params = if command == "window.maximize" { json!({"on": !maximized}) } else { json!({}) };
            let _ = app.run(command, params);
            app.flush_window_commands(ui.ctx());
        }
        response.on_hover_text(crate::i18n::tr(&app.ui.language, label));
    }
    ui.spacing_mut().item_spacing.x = spacing;
    ui.add_space(8.0);
}

/// Undecorated Windows windows need resize grips as well as caption controls.
pub(crate) fn resize_borders(app: &mut DesignApp, ui: &mut egui::Ui) {
    if ui.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let r = ui.ctx().content_rect();
    let edge = 4.0;
    let corner = 10.0;
    let rect = |x0, y0, x1, y1| egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
    for (name, region, cursor) in [
        ("northWest", rect(r.left(), r.top(), r.left() + corner, r.top() + corner), egui::CursorIcon::ResizeNwSe),
        ("northEast", rect(r.right() - corner, r.top(), r.right(), r.top() + corner), egui::CursorIcon::ResizeNeSw),
        ("southWest", rect(r.left(), r.bottom() - corner, r.left() + corner, r.bottom()), egui::CursorIcon::ResizeNeSw),
        ("southEast", rect(r.right() - corner, r.bottom() - corner, r.right(), r.bottom()), egui::CursorIcon::ResizeNwSe),
        ("north", rect(r.left() + corner, r.top(), r.right() - corner, r.top() + edge), egui::CursorIcon::ResizeVertical),
        ("south", rect(r.left() + corner, r.bottom() - edge, r.right() - corner, r.bottom()), egui::CursorIcon::ResizeVertical),
        ("west", rect(r.left(), r.top() + corner, r.left() + edge, r.bottom() - corner), egui::CursorIcon::ResizeHorizontal),
        ("east", rect(r.right() - edge, r.top() + corner, r.right(), r.bottom() - corner), egui::CursorIcon::ResizeHorizontal),
    ] {
        let response = ui.interact(region, egui::Id::new(("window_resize", name)), Sense::drag()).on_hover_cursor(cursor);
        if response.drag_started_by(egui::PointerButton::Primary) {
            let _ = app.run("window.resize", json!({"edge": name}));
            app.flush_window_commands(ui.ctx());
        }
    }
}

impl DesignApp {
    fn session_home(&mut self) {
        self.ui.status = crate::i18n::tr(&self.ui.language, "Home").into();
    }
}

fn vsep(ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 46.0), Sense::hover());
    ui.painter().line_segment([r.center_top() + vec2(0.0, 4.0), r.center_bottom() - vec2(0.0, 4.0)], Stroke::new(1.0, t.divider));
}

/// Two-row context-sensitive Control panel.
pub fn control_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::top("control_bar")
        .exact_size(59.0)
        .frame(
            egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin { left: 8, right: 8, top: 5, bottom: 3 }).stroke(Stroke::new(1.0, t.divider)),
        )
        .show(ui, |ui| {
            // Wider than the window: the bar scrolls sideways.
            crate::widgets::overflow_scrolling(ui);
            egui::ScrollArea::horizontal().id_salt("control_bar_scroll").auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let text_mode =
                        matches!(app.session.tool_id(), "type" | "verticalType") || app.session.active().is_some_and(|d| d.selection.text.is_some());
                    if app.session.tool_id() == "page" {
                        control_page(app, ui);
                    } else if text_mode {
                        control_text(app, ui);
                    } else {
                        crate::object_control::show(app, ui);
                    }
                });
            });
        });
}

/// The Control panel with the Page tool, as InDesign's: reference point, X/Y, W/H, page-size
/// preset, orientation, Liquid Page Rule and Show Parent Page Overlay, for
/// the page the Page tool selected (else the page in view). W/H keep the reference point in place.
fn control_page(app: &mut DesignApp, ui: &mut egui::Ui) {
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let units = st.doc.settings.horizontal_units;
    let Some(page) = st.selection.page.or_else(|| crate::canvas::current_page(app)) else { return };
    let Some(st) = app.session.active() else { return };
    let d = st.doc.clone();
    let Some((si, pi)) = d.page_loc(page) else { return };
    let pg = d.spreads[si].pages[pi].clone();
    let n = page + 1;
    // Reference point: W/H edits keep it in place.
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 44.0), Sense::hover());
    if let Some(p) = panels::properties::ref_point_proxy(ui, egui::Rect::from_center_size(r.center(), vec2(27.0, 27.0)), app.ui.ref_point) {
        app.ui.ref_point = p;
    }
    let anchor = app.ui.ref_point.min(8);
    let (fx, fy) = ((anchor % 3) as f64 / 2.0, (anchor / 3) as f64 / 2.0);
    // The reference point's position: from the spine across, from the top down.
    let (x, y) = (pg.x - d.spreads[si].spine_x() + fx * pg.width, fy * pg.height);
    egui::Grid::new("ctl_page_xywh").num_columns(4).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&lang, "X:"));
        // Pages don't move within their spread yet.
        ui.add_enabled_ui(false, |ui| measure(ui, "cpx", Some(x), units, 64.0));
        caption(ui, crate::i18n::tr(&lang, "W:"));
        if let Some(v) = measure(ui, "cpw", Some(pg.width), units, 64.0) {
            let _ = app.run("layout.pageSize", json!({"pages": [n], "width": v, "anchor": anchor}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&lang, "Y:"));
        ui.add_enabled_ui(false, |ui| measure(ui, "cpy", Some(y), units, 64.0));
        caption(ui, crate::i18n::tr(&lang, "H:"));
        if let Some(v) = measure(ui, "cph", Some(pg.height), units, 64.0) {
            let _ = app.run("layout.pageSize", json!({"pages": [n], "height": v, "anchor": anchor}));
        }
        ui.end_row();
    });
    vsep(ui);
    // Page-size preset and orientation.
    let landscape = pg.width > pg.height;
    ui.vertical(|ui| {
        let cur = panels::properties::preset_name(pg.width, pg.height);
        egui::ComboBox::from_id_salt("ctl_page_preset").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&lang, cur))).width(130.0).show_ui(
            ui,
            |ui| {
                for (name, w, h) in panels::properties::PAGE_PRESETS {
                    if ui.selectable_label(cur == *name, crate::rtl::widget(ui, crate::i18n::tr(&lang, name))).clicked() {
                        let (w, h) = if landscape { (h, w) } else { (w, h) };
                        let _ = app.run("layout.pageSize", json!({"pages": [n], "width": w, "height": h, "anchor": anchor}));
                    }
                }
            },
        );
        ui.horizontal(|ui| {
            for (icon, tip, want) in [("orient-portrait", "Portrait", false), ("orient-landscape", "Landscape", true)] {
                if crate::widgets::icon_toggle(ui, icon, landscape == want, crate::i18n::tr(&lang, tip)).clicked() && landscape != want {
                    let _ = app.run("layout.pageSize", json!({"pages": [n], "width": pg.height, "height": pg.width, "anchor": anchor}));
                }
            }
        });
    });
    vsep(ui);
    ui.vertical(|ui| {
        caption(ui, crate::i18n::tr(&lang, "Liquid Page Rule:"));
        let label = panels::interactive::liquid_rule_label(pg.liquid);
        egui::ComboBox::from_id_salt("ctl_page_rule").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).width(150.0).show_ui(
            ui,
            |ui| {
                for (r, l) in panels::interactive::LIQUID_RULES {
                    if ui.selectable_label(pg.liquid == r, crate::rtl::widget(ui, crate::i18n::tr(&lang, l))).clicked() && pg.liquid != r {
                        let _ = app.run("liquid.pageRule", json!({"rule": r, "pages": [n]}));
                    }
                }
            },
        );
    });
    vsep(ui);
    ui.vertical(|ui| {
        let mut overlay = pg.show_parent_overlay;
        if ui.checkbox(&mut overlay, crate::rtl::widget(ui, crate::i18n::tr(&lang, "Show Parent Page Overlay"))).changed() {
            let _ = app.run("layout.pageOptions", json!({"pages": [n], "showParentOverlay": overlay}));
        }
    });
}

fn control_text(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let attrs = panels::text_attrs(app);
    let chars = attrs.as_ref().map(|a| a["chars"].clone()).unwrap_or_default();
    let para = attrs.as_ref().map(|a| a["para"].clone()).unwrap_or_default();
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("A").font(semibold(15.0)).color(t.text_strong));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("¶").font(semibold(15.0)).color(t.text_dim));
        });
    });
    vsep(ui);
    ui.vertical(|ui| {
        let fam = chars["fontFamily"].as_str().unwrap_or("").to_string();
        let sty = chars["fontStyle"].as_str().unwrap_or("").to_string();
        panels::font_family_picker(app, ui, &fam, 170.0);
        panels::font_style_picker(app, ui, &fam, &sty, 170.0);
    });
    vsep(ui);
    egui::Grid::new("ctl_char").num_columns(4).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
        caption(ui, "𝐓");
        if let Some(v) = number(ui, "csize", chars["size"].as_f64(), " pt", 54.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"size": v}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "VA"));
        if let Some(v) = number(ui, "ctrack", chars["tracking"].as_f64(), "", 48.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"tracking": v}}));
        }
        ui.end_row();
        caption(ui, "Ā");
        let lead = match chars["leading"]["kind"].as_str() {
            Some("points") => chars["leading"]["value"].as_f64(),
            _ => chars["size"].as_f64().map(|s| s * para["autoLeading"].as_f64().unwrap_or(1.2)),
        };
        if let Some(v) = number(ui, "clead", lead, " pt", 54.0, 2) {
            let _ = app.run("type.char", json!({"attrs": {"leading": {"kind": "points", "value": v}}}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "IT"));
        if let Some(v) = number(ui, "chs", chars["hScale"].as_f64().map(|v| v * 100.0), "%", 48.0, 0) {
            let _ = app.run("type.char", json!({"attrs": {"hScale": v / 100.0}}));
        }
        ui.end_row();
    });
    vsep(ui);
    ui.vertical(|ui| {
        let cur: Align = serde_json::from_value(para["align"].clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            for (a, icon) in [(Align::Left, "align-left"), (Align::Center, "align-center"), (Align::Right, "align-right")] {
                if icons::button(ui, icon, 20.0, cur == a, a.label()).clicked() {
                    let _ = app.run("type.para", json!({"attrs": {"align": a}}));
                }
            }
        });
        ui.horizontal(|ui| {
            for (a, icon) in [(Align::LeftJustified, "align-justify"), (Align::FullyJustified, "align-justify-all")] {
                if icons::button(ui, icon, 20.0, cur == a, a.label()).clicked() {
                    let _ = app.run("type.para", json!({"attrs": {"align": a}}));
                }
            }
        });
    });
    vsep(ui);
    let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    egui::Grid::new("ctl_para").num_columns(4).spacing(vec2(4.0, 3.0)).show(ui, |ui| {
        caption(ui, "→|");
        if let Some(v) = measure(ui, "cli", para["leftIndent"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"leftIndent": v}}));
        }
        caption(ui, "↑¶");
        if let Some(v) = measure(ui, "csb", para["spaceBefore"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"spaceBefore": v}}));
        }
        ui.end_row();
        caption(ui, "→¶");
        if let Some(v) = measure(ui, "cfi", para["firstLineIndent"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"firstLineIndent": v}}));
        }
        caption(ui, "↓¶");
        if let Some(v) = measure(ui, "csa", para["spaceAfter"].as_f64(), units, 54.0) {
            let _ = app.run("type.para", json!({"attrs": {"spaceAfter": v}}));
        }
        ui.end_row();
    });
    vsep(ui);
    ui.vertical(|ui| {
        let ps = attrs.as_ref().and_then(|a| a["paragraphStyle"].as_str()).unwrap_or("").to_string();
        let ov = attrs.as_ref().and_then(|a| a["paraOverrides"].as_u64()).unwrap_or(0)
            + attrs.as_ref().and_then(|a| a["charOverrides"].as_u64()).unwrap_or(0);
        panels::para_style_picker(app, ui, &ps, ov > 0, 170.0);
        let hy = para["hyphenate"].as_bool().unwrap_or(true);
        let mut h = hy;
        if ui.checkbox(&mut h, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hyphenate"))).changed() {
            let _ = app.run("type.para", json!({"attrs": {"hyphenate": h}}));
        }
    });
}

/// The widest a document tab grows: a longer name is shortened so its close button and the
/// tabs after it stay in view.
const DOC_TAB_MAX_W: f32 = 260.0;
/// Close button and padding around a tab's title.
const DOC_TAB_PAD: f32 = 44.0;

pub fn doc_tabs(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let max_w = DOC_TAB_MAX_W.min(bar.width());
    let close_label = crate::i18n::tr(&app.ui.language, "Close");
    ui.painter().rect_filled(bar, 0.0, t.tab_strip);
    ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, t.border));
    let mut x = bar.min.x;
    let active = app.session.active_index();
    let zoom = app.view().map(|v| v.zoom).unwrap_or(1.0);
    let mut activate = None;
    let mut close = None;
    let titles: Vec<(String, bool)> = app.session.documents().iter().map(|d| (d.title(), d.is_dirty())).collect();
    for (i, (title, dirty)) in titles.iter().enumerate() {
        let is_active = Some(i) == active;
        // `*` = unsaved; the view-mode suffix only in Preview (InDesign shows its GPU mode there).
        let suffix = if is_active && app.ui.screen_mode == crate::ScreenMode::Preview { " [Preview]" } else { "" };
        let color = if is_active { t.text_strong } else { t.text_dim };
        // The name gives way first; the dirty marker and zoom stay readable.
        let zoom_label = crate::rtl::plain(ui.ctx(), &format!(" @ {:.0}%{suffix}", zoom * 100.0), semibold(11.5), color);
        let name = format!("{}{}", if *dirty { "*" } else { "" }, crate::rtl::isolate(title));
        let name_max = max_w - DOC_TAB_PAD - zoom_label.size().x;
        let galley = crate::rtl::truncated(ui.ctx(), &name, semibold(11.5), color, name_max);
        let elided = galley.elided;
        let w = (galley.size().x + zoom_label.size().x + DOC_TAB_PAD).max(if is_active { 210.0 } else { 150.0 }).min(max_w);
        let r = egui::Rect::from_min_size(egui::pos2(x, bar.min.y), vec2(w, 28.0));
        let resp = ui.interact(r, ui.id().with(("doctab", i)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
        let resp = if elided {
            resp.on_hover_ui(|ui| {
                let _ = crate::rtl::label(ui, title);
            })
        } else {
            resp
        };
        ui.painter().rect_filled(r, 0.0, if is_active { t.panel } else { t.tab_strip });
        let cr = egui::Rect::from_center_size(egui::pos2(r.min.x + 18.0, r.center().y), vec2(14.0, 14.0));
        let cresp = ui.interact(cr, ui.id().with(("docclose", i)), Sense::click());
        cresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{close_label} {title}")));
        ui.painter().text(
            cr.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(14.0),
            if cresp.hovered() { t.text_strong } else { t.text_dim },
        );
        let name_w = galley.size().x;
        // Clipped too, for a window narrower than the zoom label.
        let text = ui.painter().with_clip_rect(r);
        text.galley(egui::pos2(r.min.x + 32.0, r.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
        text.galley(egui::pos2(r.min.x + 32.0 + name_w, r.center().y - zoom_label.size().y / 2.0), zoom_label, Color32::WHITE);
        ui.painter().line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, t.border));
        if cresp.clicked() {
            close = Some(i);
        } else if resp.clicked() {
            activate = Some(i);
        }
        x += w;
    }
    if let Some(i) = close {
        crate::menus::close_document(app, Some(i));
    } else if let Some(i) = activate {
        let _ = app.run("file.activate", json!({"index": i}));
    }
}

/// Show the Preflight panel. `window.panel` toggles a panel that is already open, so it runs
/// only when Preflight isn't showing.
fn show_preflight(app: &mut DesignApp) {
    if app.ui.open_panel.as_deref() != Some("preflight")
        && let Err(e) = app.run("window.panel", json!({"panel": "preflight"}))
    {
        app.status(e);
    }
}

pub fn status_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let resp = egui::Panel::bottom("status_bar")
        .exact_size(17.0)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(8, 0)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal_centered(|ui| {
                let small = egui::FontId::proportional(10.5);
                if app.session.active().is_none() {
                    ui.label(egui::RichText::new(&app.ui.status).font(small).color(t.text_dim));
                    return;
                }
                let z = app.view().map(|v| v.zoom).unwrap_or(1.0);
                crate::menus::menu_button(ui, egui::RichText::new(format!("{:.2}% ▾", z * 100.0)).font(small.clone()), |ui| {
                    for p in [0.05, 0.125, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0, 8.0, 16.0] {
                        if ui.button(format!("{}%", p * 100.0)).clicked() {
                            let _ = app.run("view.zoom", json!({"zoom": p}));
                            ui.close();
                        }
                    }
                });
                ui.add_space(8.0);
                let n = app.session.active().map(|d| d.doc.page_count()).unwrap_or(1);
                let cur = crate::canvas::current_page(app).unwrap_or(0);
                if icons::button(ui, "first", 14.0, false, crate::i18n::tr(&app.ui.language, "First Spread")).clicked() {
                    crate::canvas::go_to_page(app, 0);
                }
                if icons::button(ui, "prev", 14.0, false, crate::i18n::tr(&app.ui.language, "Previous Spread")).clicked() {
                    crate::canvas::go_to_page(app, cur.saturating_sub(1));
                }
                let name = app.session.page_label(cur);
                let (fr, fresp) = ui.allocate_exact_size(vec2(80.0, 14.0), Sense::click());
                ui.painter().rect(fr, 0.0, t.input, Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                ui.painter().text(fr.left_center() + vec2(5.0, 0.0), egui::Align2::LEFT_CENTER, &name, small.clone(), t.text);
                ui.painter().text(fr.right_center() - vec2(5.0, 0.0), egui::Align2::RIGHT_CENTER, "▾", small.clone(), t.text);
                egui::Popup::menu(&fresp).show(|ui| {
                    let names: Vec<String> = (0..n).map(|i| app.session.page_label(i)).collect();
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (i, nm) in names.iter().enumerate() {
                            if ui.button(nm).clicked() {
                                crate::canvas::go_to_page(app, i);
                                ui.close();
                            }
                        }
                    });
                });
                if icons::button(ui, "next", 14.0, false, crate::i18n::tr(&app.ui.language, "Next Spread")).clicked() {
                    crate::canvas::go_to_page(app, (cur + 1).min(n - 1));
                }
                if icons::button(ui, "last", 14.0, false, crate::i18n::tr(&app.ui.language, "Last Spread")).clicked() {
                    crate::canvas::go_to_page(app, n - 1);
                }
                ui.add_space(12.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "[Basic] (working) ▾")).font(small.clone()));
                ui.add_space(8.0);
                // Preflight: overset text is an error.
                let errors = panels::preflight_errors(app);
                let (r, dot) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::click());
                ui.painter().circle_filled(
                    r.center(),
                    4.0,
                    if errors == 0 { Color32::from_rgb(60, 200, 90) } else { Color32::from_rgb(235, 50, 50) },
                );
                let count = if errors == 0 {
                    format!("{} ▾", crate::i18n::tr(&app.ui.language, "No errors"))
                } else {
                    format!(
                        "{} ▾",
                        crate::i18n::count_label(
                            &app.ui.language,
                            if errors == 1 && !crate::i18n::category_first(&app.ui.language) { "error" } else { "errors" },
                            errors
                        )
                    )
                };
                // As in InDesign, the count opens a menu that leads to the Preflight panel.
                let menu = crate::menus::menu_button(ui, egui::RichText::new(count).font(small.clone()), |ui| {
                    if ui.button(crate::i18n::tr(&app.ui.language, "Preflight Panel…")).clicked() {
                        show_preflight(app);
                        ui.close();
                    }
                });
                // Double-clicking the light or the count shows the panel straight away (the
                // second click has closed the menu the first one opened).
                if dot.double_clicked() || menu.response.double_clicked() {
                    show_preflight(app);
                }
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&app.ui.status).font(small).color(t.text_dim));
            });
        });
    let r = resp.response.rect;
    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.border));
}

pub fn start_screen(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let r = ui.available_rect_before_wrap();
    ui.painter().rect_filled(r, 0.0, t.panel_darker);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(r.shrink(48.0)).layout(egui::Layout::top_down(if crate::i18n::is_rtl(&app.ui.language) {
            egui::Align::Max
        } else {
            egui::Align::Min
        })),
        |ui| {
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Welcome to DesignCraft")).font(semibold(28.0)).color(t.text_strong),
            ));
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page layout for print and screen — fast, open and scriptable."))
                    .size(15.0)
                    .color(t.text_dim),
            ));
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "  New file  ")).size(14.0).color(Color32::WHITE),
                        ))
                        .fill(t.accent_strong)
                        .corner_radius(16.0)
                        .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("app.newDocumentDialog", json!({}));
                }
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open  ")).size(14.0)))
                            .corner_radius(16.0)
                            .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("app.openDialog", json!({}));
                }
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "  Open sample magazine  ")).size(14.0),
                        ))
                        .corner_radius(16.0)
                        .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
                {
                    let _ = app.run("file.newSample", json!({}));
                }
            });
            ui.add_space(20.0);
            community_card(app, ui);
            recent_files(app, ui);
            ui.add_space(24.0);
            ui.label(crate::rtl::widget(
                ui,
                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Start a new document")).font(semibold(15.0)).color(t.text_strong),
            ));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for p in designcraft_doc::build::PRESETS.iter().filter(|p| p.intent == designcraft_doc::Intent::Print) {
                    let (cr, resp) = ui.allocate_exact_size(vec2(120.0, 150.0), Sense::click());
                    let hov = resp.hovered();
                    ui.painter().rect_filled(cr, 6.0, if hov { t.hover } else { t.panel });
                    let k = (80.0 / p.width.max(p.height)) as f32;
                    let pr = egui::Rect::from_center_size(cr.center() - vec2(0.0, 14.0), vec2(p.width as f32 * k, p.height as f32 * k));
                    ui.painter().rect_filled(pr, 0.0, Color32::from_gray(245));
                    ui.painter().text(egui::pos2(cr.center().x, cr.max.y - 26.0), egui::Align2::CENTER_CENTER, p.name, semibold(12.0), t.text_strong);
                    ui.painter().text(
                        egui::pos2(cr.center().x, cr.max.y - 11.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{} × {}", designcraft_geom::format_measure(p.width, p.units), designcraft_geom::format_measure(p.height, p.units)),
                        egui::FontId::proportional(10.0),
                        t.text_dim,
                    );
                    if resp.clicked() {
                        let _ = app.run("file.new", json!({"preset": p.name}));
                    }
                }
            });
        },
    );
    let _ = Content::Unassigned;
}

/// Recent files shown on the start screen (File › Open Recent has them all).
const START_SCREEN_RECENT: usize = 8;

/// Start screen: the most recent files as cards with the first page's picture (saved in the file)
/// and the name; the full path is the tooltip. A file that is gone is dimmed and says so when
/// clicked. Nothing when the list is empty.
fn recent_files(app: &mut DesignApp, ui: &mut egui::Ui) {
    if app.session.prefs.recent_files.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.add_space(24.0);
    ui.label(crate::rtl::widget(
        ui,
        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Recent files")).font(semibold(15.0)).color(t.text_strong),
    ));
    ui.add_space(8.0);
    let rows: Vec<(usize, String)> = app.session.prefs.recent_files.iter().take(START_SCREEN_RECENT).cloned().enumerate().collect();
    let mut open = None;
    ui.horizontal_wrapped(|ui| {
        for (i, path) in rows {
            let stat = recent_file_stat(ui.ctx(), &path);
            let there = stat.exists;
            let modified = there.then_some(stat.modified);
            let (cr, resp) = ui.allocate_exact_size(vec2(140.0, 170.0), Sense::click());
            let resp = resp.on_hover_text(&path);
            ui.painter().rect_filled(cr, 6.0, if resp.hovered() { t.hover } else { t.panel });
            let area = egui::Rect::from_min_size(cr.min + vec2(20.0, 12.0), vec2(100.0, 108.0));
            match modified.and_then(|m| recent_preview_texture(ui.ctx(), &path, m)) {
                Some(tex) => {
                    let size = tex.size_vec2();
                    let k = (area.width() / size.x).min(area.height() / size.y);
                    let pr = egui::Rect::from_center_size(area.center(), size * k);
                    ui.painter().image(tex.id(), pr, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    ui.painter().rect_stroke(pr, 0.0, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
                }
                // No picture (saved before files had one) or the file is gone: a blank page.
                None => {
                    let pr = egui::Rect::from_center_size(area.center(), vec2(62.0, 80.0));
                    ui.painter().rect_filled(pr, 0.0, if there { Color32::from_gray(245) } else { Color32::from_gray(110) });
                }
            }
            let color = if there { t.text_strong } else { t.text_dim };
            // The name without `.designcraft`, on up to two lines (broken after a space or dash).
            let label = crate::menus::recent_label(&path);
            let name = label.strip_suffix(".designcraft").unwrap_or(&label).to_string();
            let mut job = egui::text::LayoutJob::simple(name, semibold(12.0), color, cr.width() - 12.0);
            job.wrap.max_rows = 2;
            job.halign = egui::Align::Center;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(egui::pos2(cr.center().x, area.max.y + 8.0), galley, color);
            if resp.clicked() {
                open = Some(i);
            }
        }
    });
    if let Some(i) = open {
        let _ = app.run("app.openRecent", json!({"index": i}));
    }
}

/// How often a recent file is looked up on disk again, in ms: the start screen and the Open Recent
/// menu draw every frame and must not touch the file system each time.
const RECENT_STAT_MS: f64 = 3000.0;

/// A recent file as last seen on disk.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RecentStat {
    pub exists: bool,
    pub modified: Option<std::time::SystemTime>,
    checked: f64,
}

/// Whether recent file `path` is there and when it last changed, looked up again at most every
/// [`RECENT_STAT_MS`].
pub(crate) fn recent_file_stat(ctx: &egui::Context, path: &str) -> RecentStat {
    recent_file_stat_at(ctx, path, crate::now_ms())
}

fn recent_file_stat_at(ctx: &egui::Context, path: &str, now: f64) -> RecentStat {
    let key = egui::Id::new(("recent_stat", path));
    if let Some(st) = ctx.data(|d| d.get_temp::<RecentStat>(key))
        && (0.0..RECENT_STAT_MS).contains(&(now - st.checked))
    {
        return st;
    }
    let meta = designcraft_engine::io::metadata(path).ok().filter(designcraft_engine::io::Metadata::is_file);
    let st = RecentStat { exists: meta.is_some(), modified: meta.and_then(|m| m.modified().ok()), checked: now };
    ctx.data_mut(|d| d.insert_temp(key, st.clone()));
    st
}

/// The texture of a recent file's saved first-page picture, decoded once per file version.
fn recent_preview_texture(ctx: &egui::Context, path: &str, modified: Option<std::time::SystemTime>) -> Option<egui::TextureHandle> {
    type Cached = (Option<std::time::SystemTime>, Option<egui::TextureHandle>);
    let key = egui::Id::new(("recent_preview", path));
    if let Some((m, tex)) = ctx.data(|d| d.get_temp::<Cached>(key))
        && m == modified
    {
        return tex;
    }
    let tex = designcraft_engine::cmd::recent_preview(path).and_then(|p| {
        let size = [usize::try_from(p.width).ok()?, usize::try_from(p.height).ok()?];
        (size[0].checked_mul(size[1])?.checked_mul(4)? == p.rgba.len()).then(|| {
            ctx.load_texture(format!("recent_preview:{path}"), egui::ColorImage::from_rgba_unmultiplied(size, &p.rgba), egui::TextureOptions::LINEAR)
        })
    });
    ctx.data_mut(|d| d.insert_temp::<Cached>(key, (modified, tex.clone())));
    tex
}

/// Start screen: the ArtCraft community (Discord first) and project links.
fn community_card(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Frame::NONE.fill(t.panel).corner_radius(10.0).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
        // Wide enough for the longer translations of the line under the title, with the button
        // at the right edge; the text wraps before the button's space rather than run under it.
        const BUTTON: egui::Vec2 = vec2(190.0, 38.0);
        ui.set_max_width(760.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            crate::about::paint_mark(ui, r, crate::about::BRAND);
            ui.add_space(8.0);
            let text_width = (ui.available_width() - BUTTON.x - 16.0).max(120.0);
            ui.vertical(|ui| {
                ui.set_max_width(text_width);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Join the ArtCraft community")).font(semibold(14.0)).color(t.text_strong),
                ));
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "Get help, share your layouts and shape what we build next."))
                        .size(12.0)
                        .color(t.text_dim),
                ));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for (text, id) in [("DesignCraft page", "help.appPage"), ("GitHub", "help.github"), ("getartcraft.com", "help.website")] {
                        let url = designcraft_engine::links::get(&id[5..]).unwrap_or_default();
                        let l = ui
                            .add(egui::Label::new(egui::RichText::new(text).size(12.0).color(t.accent).underline()).sense(Sense::click()))
                            .on_hover_text(url)
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if l.clicked() {
                            let _ = app.run(id, json!({}));
                        }
                        ui.add_space(8.0);
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::about::discord_button(ui, crate::i18n::tr(&app.ui.language, "Join our Discord"), BUTTON) {
                    let _ = app.run("help.discord", json!({}));
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2};
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;
    use crate::test_window::{self, wheel};

    /// The Control panel's last control with no text frame selected.
    const LAST_CONTROL: &str = "Align bottom edges";
    const FIRST_CONTROL: &str = "Rotate 90° Counterclockwise";

    /// The start screen and Open Recent menu look a recent file up on disk at most every few
    /// seconds, not every frame.
    #[test]
    fn recent_files_are_looked_up_on_disk_every_few_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.designcraft");
        std::fs::write(&path, b"x").unwrap();
        let path = path.to_string_lossy().to_string();
        let ctx = egui::Context::default();
        let first = super::recent_file_stat_at(&ctx, &path, 10_000.0);
        assert!(first.exists && first.modified.is_some());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(super::recent_file_stat_at(&ctx, &path, 10_000.0 + super::RECENT_STAT_MS - 1.0), first, "cached");
        assert!(!super::recent_file_stat_at(&ctx, &path, 10_000.0 + super::RECENT_STAT_MS).exists, "looked up again");
    }

    fn app() -> crate::DesignApp {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.control_bar = true;
        app
    }

    /// A document with a rectangle selected, so the object controls are enabled.
    fn app_with_selection() -> crate::DesignApp {
        let mut app = app();
        app.run("file.new", serde_json::json!({})).unwrap();
        app.run("frame.create", serde_json::json!({"rect": [72, 72, 300, 200]})).unwrap();
        app.run("edit.selectAll", serde_json::json!({})).unwrap();
        app
    }

    /// The rect of an object Control panel group.
    fn group(h: &egui_kittest::Harness<'static, test_window::Window>, id: &str) -> egui::Rect {
        h.ctx.read_response(egui::Id::new(("object_control", id))).unwrap().rect
    }

    /// The rect of the node with this label, preferring the one in the Control bar's row: the
    /// Properties panel has buttons with the same names.
    fn rect(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> egui::Rect {
        let bar = test_window::panel_rect(h, "control_bar");
        h.query_all_by_label(label).map(|n| n.rect()).find(|r| bar.y_range().contains(r.center().y)).unwrap_or_else(|| h.get_by_label(label).rect())
    }

    /// The application bar's search field (German interface).
    fn search_node<'h>(h: &'h egui_kittest::Harness<'static, test_window::Window>) -> egui_kittest::Node<'h> {
        h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.placeholder() == Some("Suchen"))
    }

    /// A document with two frames, the Selection tool, German menus, and the search field clicked.
    fn searching() -> egui_kittest::Harness<'static, test_window::Window> {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "de".into();
        let mut h = test_window::open(app, vec2(1440.0, 900.0));
        let app = &mut h.state_mut().app;
        app.run("frame.create", serde_json::json!({"rect": [36, 36, 200, 200]})).unwrap();
        app.run("frame.create", serde_json::json!({"rect": [236, 36, 400, 200]})).unwrap();
        app.run("edit.deselectAll", serde_json::json!({})).unwrap();
        h.run_steps(4);
        let field = search_node(&h).rect();
        test_window::click_at(&mut h, field.center());
        assert!(h.ctx.memory(|m| m.has_focus(super::search_id())), "a click gives the field the keyboard");
        h
    }

    /// A match in the search's list, whose accessible label ends in its shortcut.
    fn row<'h>(h: &'h egui_kittest::Harness<'static, test_window::Window>, label: &'static str) -> Option<egui_kittest::Node<'h>> {
        h.query_by(move |n| n.label().is_some_and(|l| l == label || l.starts_with(&format!("{label} "))))
    }

    fn items(h: &egui_kittest::Harness<'static, test_window::Window>) -> (usize, usize) {
        let d = h.state().app.session.active().unwrap();
        (d.doc.spreads.iter().flat_map(|sp| &sp.items).count(), d.selection.items.len())
    }

    #[test]
    fn the_search_field_finds_and_runs_menu_commands() {
        let mut h = searching();
        h.event(egui::Event::Text("gruppier".into()));
        h.run_steps(4);
        assert!(row(&h, "Gruppieren").is_some(), "the German label is listed");
        assert!(row(&h, "Gruppierung aufheben").is_some());
        // Nothing is selected, so Group is off and Return doesn't run it.
        assert!(row(&h, "Gruppieren").is_some_and(|n| n.accesskit_node().is_disabled()));
        h.key_press(egui::Key::Enter);
        h.run_steps(4);
        assert_eq!(items(&h), (2, 0), "a disabled command doesn't run");
        // Select both frames, then ↓ ↑ Return runs the first match, as Object › Group would.
        h.state_mut().app.run("edit.selectAll", serde_json::json!({})).unwrap();
        h.key_press(egui::Key::ArrowDown);
        h.run_steps(2);
        h.key_press(egui::Key::ArrowUp);
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(4);
        assert_eq!(items(&h).0, 1, "the two frames are grouped");
        assert!(row(&h, "Gruppieren").is_none(), "the matches closed");
        assert!(!h.ctx.memory(|m| m.has_focus(super::search_id())), "the field gave the keyboard back");
    }

    #[test]
    fn keys_typed_in_the_search_field_stay_out_of_the_document() {
        let mut h = searching();
        h.state_mut().app.run("edit.selectAll", serde_json::json!({})).unwrap();
        // Escape would switch this tool back to the Selection tool.
        h.state_mut().app.run("tool.select", serde_json::json!({"tool": "rectangleFrame"})).unwrap();
        h.run_steps(2);
        // Tool keys, Delete and Backspace edit the query; the tool and the frames stay.
        for key in [egui::Key::P, egui::Key::T, egui::Key::M, egui::Key::Delete, egui::Key::Backspace] {
            h.key_press(key);
            h.run_steps(2);
        }
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(2);
        assert_eq!(h.state().app.session.tool_id(), "rectangleFrame");
        assert_eq!(items(&h), (2, 2), "nothing was deleted or deselected");
        h.event(egui::Event::Text("zz-no-such-command".into()));
        h.run_steps(4);
        assert!(h.query_by_label("Keine passenden Befehle").is_some());
        // Escape closes the matches and clears the field without touching the document.
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(h.query_by_label("Keine passenden Befehle").is_none());
        assert!(!h.ctx.text_edit_focused());
        assert_eq!(h.state().app.session.tool_id(), "rectangleFrame");
        assert_eq!(items(&h), (2, 2));
        let field = search_node(&h);
        assert_eq!(field.value().unwrap_or_default(), "", "the query is cleared");
    }

    /// The status bar's preflight count is a menu; its Preflight Panel entry shows the panel.
    #[test]
    fn status_bar_error_count_opens_the_preflight_panel() {
        let mut app = app();
        app.run("file.new", serde_json::json!({})).unwrap();
        app.ui.dock_tab = "properties".into();
        let mut h = test_window::open(app, vec2(1200.0, 700.0));
        h.get_by_label("No errors ▾").click();
        h.run_steps(3);
        h.get_by_label("Preflight Panel…").click();
        h.run_steps(3);
        // Preflight is not a dock tab: `window.panel` shows it as its own panel.
        assert_eq!(h.state().app.ui.open_panel.as_deref(), Some("preflight"));
        h.get_by_label("Preflight");
        // With the panel already showing, the entry keeps it open.
        h.get_by_label("No errors ▾").click();
        h.run_steps(3);
        h.get_by_label("Preflight Panel…").click();
        h.run_steps(3);
        assert_eq!(h.state().app.ui.open_panel.as_deref(), Some("preflight"));
    }

    /// As in InDesign, double-clicking the preflight count (or its light) shows the panel.
    #[test]
    fn double_clicking_the_status_bar_error_count_opens_the_preflight_panel() {
        let mut app = app();
        app.run("file.new", serde_json::json!({})).unwrap();
        app.ui.dock_tab = "properties".into();
        let mut h = test_window::open_stepped(app, vec2(1200.0, 700.0), 1.0, 1.0 / 60.0);
        let count = h.get_by_label("No errors ▾");
        count.click();
        count.click();
        h.run_steps(3);
        assert_eq!(h.state().app.ui.open_panel.as_deref(), Some("preflight"));
        h.get_by_label("Preflight");
        assert!(h.query_by_label("Preflight Panel…").is_none(), "the menu isn't left open");
        // A double-click with the panel already showing keeps it open.
        let count = h.get_by_label("No errors ▾");
        count.click();
        count.click();
        h.run_steps(3);
        assert_eq!(h.state().app.ui.open_panel.as_deref(), Some("preflight"));
    }

    /// A very long document name made its tab grow past the window, hiding the close button
    /// and the tabs after it (#603).
    #[test]
    fn long_document_names_keep_tabs_short_and_close_buttons_visible() {
        let latin = "Quarterly report with a very long file name ".repeat(6);
        let hebrew = "דוח רבעוני עם שם קובץ ארוך מאוד ".repeat(6);
        let mut app = app();
        for title in [latin.as_str(), hebrew.as_str(), "Short"] {
            app.run("file.new", serde_json::json!({ "title": title })).unwrap();
        }
        let h = test_window::open(app, vec2(1200.0, 600.0));
        for title in [latin.as_str(), hebrew.as_str()] {
            let tab = rect(&h, title);
            assert!(tab.width() <= super::DOC_TAB_MAX_W, "{title:?} tab is {} wide", tab.width());
            let close = rect(&h, &format!("Close {title}"));
            assert!(tab.contains_rect(close), "close button {close:?} outside its tab {tab:?}");
        }
        let (first, short) = (rect(&h, &latin), rect(&h, "Short"));
        assert!(short.min.x - first.min.x <= 2.0 * super::DOC_TAB_MAX_W, "the last tab {short:?} starts too far right");
    }

    #[test]
    fn the_page_tool_has_its_own_control_panel() {
        // Chris Cicchino's Page Tool and Liquid Layout video (1:02): with the Page tool, the
        // Control panel shows the page's size, preset, orientation and liquid page rule.
        let mut app = app();
        app.run("file.new", serde_json::json!({"width": 252, "height": 144, "facingPages": false})).unwrap();
        app.session.set_tool("page");
        let mut h = test_window::open(app, vec2(1800.0, 600.0));
        let _ = h.get_by_label("Liquid Page Rule:");
        // Pages don't move within their spread, so there is no "Objects Move with Page" option, and
        // no "Controlled by Parent" rule until parent pages have rules of their own.
        assert!(h.query_by_label("Objects Move with Page").is_none());
        assert!(!crate::panels::interactive::LIQUID_RULES.iter().any(|(r, _)| *r == designcraft_doc::LiquidRule::ControlledByParent));
        assert_eq!(crate::panels::properties::preset_name(252.0, 144.0), "US Business Card");
        h.get_by_label("Show Parent Page Overlay").click();
        h.run_steps(4);
        assert!(h.state().app.session.doc().unwrap().doc.spreads[0].pages[0].show_parent_overlay);
    }

    #[test]
    fn a_narrow_window_scrolls_the_control_bar() {
        let w = 700.0;
        let mut h = test_window::open(app_with_selection(), vec2(w, 600.0));
        let bar = test_window::panel_rect(&h, "control_bar");
        assert!(rect(&h, LAST_CONTROL).max.x > w, "the controls are wider than the window");
        // A plain mouse wheel scrolls the bar sideways.
        let first = rect(&h, FIRST_CONTROL).center();
        wheel(&mut h, first, vec2(0.0, -3000.0));
        let last = rect(&h, LAST_CONTROL);
        let window = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 600.0));
        assert!(window.contains_rect(last) && bar.contains_rect(last), "{last:?} outside the bar {bar:?}");
        assert_eq!(test_window::panel_rect(&h, "control_bar").height(), bar.height(), "the bar keeps its height");
        // A popup opened from a scrolled control opens at that control.
        let appearance = group(&h, "appearance");
        let fill = egui::Rect::from_min_size(appearance.min, vec2(30.0, crate::widgets::FIELD_H));
        let chip = pos2(fill.min.x + 9.0, fill.center().y);
        h.hover_at(chip);
        h.drag_at(chip);
        h.drop_at(chip);
        h.run_steps(4);
        let item = rect(&h, "[Black]");
        assert!(item.min.y > fill.max.y && (item.min.x - chip.x).abs() < 120.0, "popup item at {item:?}, control at {chip:?}");
    }

    #[test]
    fn the_mac_app_bar_centres_and_clears_the_traffic_lights() {
        use super::{MAC_APP_BAR, MAC_CONTENT_LEFT, MAC_LIGHT, MAC_LIGHT_PITCH, MAC_LIGHTS_LEFT};
        // Content keeps the same gap after the lights as the lights keep from the top.
        let margin = (MAC_APP_BAR - MAC_LIGHT) / 2.0;
        assert_eq!(MAC_CONTENT_LEFT - (MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT), margin);
        // `egui::Margin` is in `i8`.
        assert!(MAC_CONTENT_LEFT <= f32::from(i8::MAX));
        let mut app = app();
        app.integrated_titlebar = true;
        let h = test_window::open(app, vec2(1400.0, 900.0));
        // The bar's controls share the lights' centre line and start after them (relative to the
        // bar, which the test window insets).
        let bar = test_window::panel_rect(&h, "app_bar").min.to_vec2();
        let home = rect(&h, "Home").translate(-bar);
        assert_eq!(home.center().y, MAC_APP_BAR / 2.0, "{home:?}");
        assert!(home.min.y >= 2.0 && home.min.x >= MAC_CONTENT_LEFT, "{home:?}");
        for label in ["File", "Help", "Share"] {
            let r = rect(&h, label).translate(-bar);
            assert!((r.center().y - MAC_APP_BAR / 2.0).abs() <= 0.5 && r.max.y <= MAC_APP_BAR, "{label} at {r:?}");
        }
    }

    fn press(h: &egui_kittest::Harness<'static, test_window::Window>, pos: egui::Pos2, pressed: bool) {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
    }

    /// The window commands the app sends over the next `steps` frames.
    fn window_commands(h: &mut egui_kittest::Harness<'static, test_window::Window>, steps: usize) -> Vec<egui::ViewportCommand> {
        let mut out = Vec::new();
        for _ in 0..steps {
            h.step();
            if let Some(v) = h.output().viewport_output.get(&egui::ViewportId::ROOT) {
                out.extend(v.commands.iter().cloned());
            }
        }
        out
    }

    fn double_click(h: &mut egui_kittest::Harness<'static, test_window::Window>, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
        h.hover_at(pos);
        h.step();
        // One frame: the harness gives each queued event a frame of its own, longer apart than a
        // double-click allows.
        for pressed in [true, false, true, false] {
            h.input_mut().events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        window_commands(h, 3)
    }

    fn drag(h: &mut egui_kittest::Harness<'static, test_window::Window>, from: egui::Pos2) -> Vec<egui::ViewportCommand> {
        h.hover_at(from);
        h.step();
        press(h, from, true);
        let mut out = window_commands(h, 1);
        for i in 1..=6 {
            h.hover_at(from + vec2(8.0 * i as f32, 3.0));
            out.extend(window_commands(h, 1));
        }
        press(h, from + vec2(48.0, 3.0), false);
        out.extend(window_commands(h, 2));
        out
    }

    #[test]
    fn the_empty_mac_title_bar_zooms_and_moves_the_window() {
        use super::{MAC_APP_BAR, MAC_CONTENT_LEFT};
        use egui::ViewportCommand::{Maximized, Minimized, StartDrag};
        let mut app = app();
        app.integrated_titlebar = true;
        let mut h = test_window::open(app, vec2(1400.0, 900.0));
        let bar = test_window::panel_rect(&h, "app_bar");
        // Under the centred title, and between the traffic lights and the Home button.
        let empty = [pos2(bar.center().x, bar.min.y + MAC_APP_BAR / 2.0), pos2(bar.min.x + MAC_CONTENT_LEFT - 4.0, bar.min.y + 4.0)];
        for at in empty {
            // Tests read the setting as its default, zoom.
            let cmds = double_click(&mut h, at);
            assert_eq!(cmds, vec![Maximized(true)], "double-click at {at:?}");
            let cmds = drag(&mut h, at);
            assert_eq!(cmds, vec![StartDrag], "drag from {at:?}");
        }
        // The bar's controls keep their clicks and drags.
        for label in ["Home", "Share", "File"] {
            let at = rect(&h, label).center();
            let cmds = double_click(&mut h, at);
            assert!(!cmds.iter().any(|c| matches!(c, Maximized(_) | Minimized(_))), "double-click on {label}: {cmds:?}");
            if label == "Home" {
                assert_eq!(h.state().app.ui.status, "Home", "Home took its clicks");
            }
            h.key_press(egui::Key::Escape);
            h.run_steps(2);
            let cmds = drag(&mut h, at);
            assert!(!cmds.contains(&StartDrag), "drag from {label}: {cmds:?}");
            h.key_press(egui::Key::Escape);
            h.run_steps(2);
        }
    }

    #[test]
    fn the_mac_window_is_not_movable_on_the_title_bar_controls() {
        use super::{MAC_APP_BAR, MAC_CONTENT_LEFT};
        let mut app = app();
        app.integrated_titlebar = true;
        let mut h = test_window::open(app, vec2(1400.0, 900.0));
        let bar = test_window::panel_rect(&h, "app_bar");
        let mid = bar.min.y + MAC_APP_BAR / 2.0;
        // The search field is the bar's last control: 125 wide, 4 from the 10-wide right margin.
        let search = pos2(bar.max.x - 10.0 - 4.0 - 60.0, mid);
        let controls = [rect(&h, "Home").center(), rect(&h, "Share").center(), rect(&h, "File").center(), search];
        let mut movable_at = |at: egui::Pos2| {
            h.hover_at(at);
            h.run_steps(2);
            crate::title_bar::window_movable(&h.ctx)
        };
        for at in [pos2(bar.center().x, mid), pos2(bar.min.x + MAC_CONTENT_LEFT - 4.0, bar.min.y + 4.0), pos2(bar.center().x, 500.0)] {
            assert!(movable_at(at), "empty title bar or the page at {at:?}");
        }
        for at in controls {
            assert!(!movable_at(at), "control at {at:?}");
        }
    }

    #[test]
    fn a_narrow_window_scrolls_the_app_bar() {
        let w = 640.0;
        let mut h = test_window::open(app(), vec2(w, 600.0));
        let (help, share) = (rect(&h, "Help"), rect(&h, "Share"));
        assert!(help.max.x <= share.min.x, "the menus ({help:?}) and the workspace controls ({share:?}) overlap");
        wheel(&mut h, help.center(), vec2(0.0, -3000.0));
        let share = rect(&h, "Share");
        let window = egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 600.0));
        assert!(window.contains_rect(share), "{share:?} outside the window");
    }

    #[test]
    fn a_wide_window_does_not_scroll_the_bars() {
        let mut h = test_window::open(app(), vec2(1400.0, 900.0));
        let before = [rect(&h, FIRST_CONTROL), rect(&h, LAST_CONTROL), rect(&h, "Help"), rect(&h, "Share")];
        assert!(before.iter().all(|r| r.max.x <= 1400.0), "{before:?}");
        wheel(&mut h, before[0].center(), vec2(0.0, -3000.0));
        wheel(&mut h, before[2].center(), vec2(0.0, -3000.0));
        let after = [rect(&h, FIRST_CONTROL), rect(&h, LAST_CONTROL), rect(&h, "Help"), rect(&h, "Share")];
        assert_eq!(after, before);
    }

    /// At 115% the app bar fits a wide window exactly: it must not scroll sideways by a rounding error.
    #[test]
    fn the_app_bar_does_not_scroll_at_115_percent() {
        let w = 1400.0;
        let mut h = test_window::open_scaled(app(), vec2(w, 900.0), 1.15);
        assert_eq!(h.ctx.pixels_per_point(), 1.15);
        let before = [rect(&h, "Help"), rect(&h, "Share")];
        assert!(before.iter().all(|r| r.max.x <= w), "{before:?}");
        wheel(&mut h, before[0].center(), vec2(0.0, -3000.0));
        wheel(&mut h, before[0].center(), vec2(-3000.0, 0.0));
        let after = [rect(&h, "Help"), rect(&h, "Share")];
        assert_eq!(after, before, "the app bar scrolled sideways although everything fits");
    }

    #[test]
    fn a_scrolled_bar_keeps_its_captions_on_one_line() {
        let mut app = app();
        app.run("file.new", serde_json::json!({})).unwrap();
        let f = app.run("frame.create", serde_json::json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        app.run("selection.set", serde_json::json!({"ids": [f["id"]]})).unwrap();
        let mut h = test_window::open(app, vec2(700.0, 600.0));
        let bar = test_window::panel_rect(&h, "control_bar").center();
        wheel(&mut h, bar, vec2(0.0, -3000.0));
        // The text frame's last group keeps its full size, not only what is left of the row.
        let columns = group(&h, "columns");
        assert!(columns.width() >= 48.0 && columns.max.x <= 700.0, "Columns group squeezed or off screen: {columns:?}");
    }

    struct WindowHarness {
        ctx: egui::Context,
        app: DesignApp,
        time: f64,
        maximized: bool,
        size: egui::Vec2,
    }

    impl WindowHarness {
        fn new() -> Self {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.custom_titlebar = true;
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx, "");
            let mut harness = Self { ctx, app, time: 0.0, maximized: false, size: vec2(1440.0, 900.0) };
            harness.frame(vec![]);
            harness.frame(vec![]);
            harness
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> Vec<egui::ViewportCommand> {
            self.time += 0.1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            input.viewports.entry(egui::ViewportId::ROOT).or_default().maximized = Some(self.maximized);
            let mut output = self.ctx.run_ui(input, |ui| {
                app_bar(&mut self.app, ui);
                resize_borders(&mut self.app, ui);
            });
            output.textures_delta.clear();
            output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
        }

        fn press(&mut self, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            ])
        }

        fn click(&mut self, pos: egui::Pos2) -> Vec<egui::ViewportCommand> {
            let mut commands = self.press(pos);
            commands.extend(self.frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]));
            commands
        }

        fn caption_center(&self, id: &str) -> egui::Pos2 {
            self.ctx.read_response(egui::Id::new(id)).unwrap().rect.center()
        }
    }

    #[test]
    fn caption_buttons_minimize_maximize_restore_and_close() {
        let mut h = WindowHarness::new();
        for (id, command) in
            [("window.minimize", egui::ViewportCommand::Minimized(true)), ("window.maximize", egui::ViewportCommand::Maximized(true))]
        {
            let p = h.caption_center(id);
            assert!(h.click(p).contains(&command), "{id}");
        }
        h.maximized = true;
        h.frame(vec![]);
        let p = h.caption_center("window.maximize");
        assert!(h.click(p).contains(&egui::ViewportCommand::Maximized(false)));
        let p = h.caption_center("window.close");
        assert!(h.click(p).contains(&egui::ViewportCommand::Close));
    }

    #[test]
    fn title_area_drags_and_double_clicks_but_menus_do_not_drag() {
        let mut h = WindowHarness::new();
        let title = h.ctx.read_response(egui::Id::new("window_title_drag")).unwrap().rect;
        assert!(title.width() > 0.0, "there is a drag region between menus and tools");
        let p = title.center();
        let mut commands = h.press(p);
        commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(20.0, 0.0))]));
        assert!(commands.contains(&egui::ViewportCommand::StartDrag));

        let mut h = WindowHarness::new();
        h.click(p);
        assert!(h.click(p).contains(&egui::ViewportCommand::Maximized(true)));

        let mut h = WindowHarness::new();
        let p = egui::pos2(70.0, 18.0); // File menu, outside the title's drag region.
        let mut commands = h.press(p);
        commands.extend(h.frame(vec![egui::Event::PointerMoved(title.center())]));
        assert!(!commands.contains(&egui::ViewportCommand::StartDrag));
    }

    #[test]
    fn edges_resize_only_when_the_window_is_restored() {
        let mut h = WindowHarness::new();
        let p = egui::pos2(1438.0, 400.0);
        let mut commands = h.press(p);
        commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(-20.0, 0.0))]));
        assert!(commands.contains(&egui::ViewportCommand::BeginResize(egui::ResizeDirection::East)));
        let mut h = WindowHarness::new();
        h.maximized = true;
        h.frame(vec![]);
        let mut commands = h.press(p);
        commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(-20.0, 0.0))]));
        assert!(!commands.iter().any(|c| matches!(c, egui::ViewportCommand::BeginResize(_))));
    }

    #[test]
    fn native_window_actions_are_unavailable_without_custom_chrome() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert!(!crate::menus::enabled(&app, "window.close"));
        assert!(app.run("window.close", json!({})).is_err());
        assert!(app.pending_window_commands.is_empty());
        app.custom_titlebar = true;
        assert!(app.run("window.resize", json!({"edge": "invalid"})).is_err());
        assert!(app.pending_window_commands.is_empty());
    }

    #[test]
    fn caption_buttons_stay_put_while_a_narrow_app_bar_scrolls() {
        let mut h = WindowHarness::new();
        h.size = vec2(640.0, 560.0);
        h.frame(vec![]);
        h.frame(vec![]);
        let rect = |h: &WindowHarness, id: &str| h.ctx.read_response(egui::Id::new(id)).unwrap().rect;
        let menus_end = |h: &WindowHarness| h.ctx.data(|d| d.get_temp::<f32>(egui::Id::new("app_bar_menus_end"))).unwrap();
        let (close, menus0) = (rect(&h, "window.close"), menus_end(&h));
        assert!(close.left() >= 0.0 && close.right() <= h.size.x, "{close:?}");
        let p = egui::pos2(200.0, 18.0);
        h.frame(vec![
            egui::Event::PointerMoved(p),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -3000.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        for _ in 0..10 {
            h.frame(vec![]);
        }
        assert!(menus_end(&h) < menus0, "the menus scrolled");
        assert_eq!(rect(&h, "window.close"), close, "the caption buttons did not");
        assert!(h.click(close.center()).contains(&egui::ViewportCommand::Close));
    }

    #[test]
    fn the_window_drags_from_its_lower_band_at_the_minimum_width() {
        // At 900 px the menus and tools fill the bar's row; its lower band still moves the window,
        // and it isn't drawn over the caption buttons.
        let mut h = WindowHarness::new();
        h.size = vec2(900.0, 560.0);
        h.frame(vec![]);
        h.frame(vec![]);
        let drag = h.ctx.read_response(egui::Id::new("window_title_drag")).unwrap().rect;
        let close = h.ctx.read_response(egui::Id::new("window.close")).unwrap().rect;
        assert!(drag.right() <= close.left() + 0.5, "{drag:?} {close:?}");
        let p = egui::pos2(450.0, APP_BAR + APP_BAR_BAND / 2.0);
        assert!(drag.contains(p), "{drag:?}");
        let mut commands = h.press(p);
        commands.extend(h.frame(vec![egui::Event::PointerMoved(p + vec2(20.0, 0.0))]));
        assert!(commands.contains(&egui::ViewportCommand::StartDrag));
    }

    #[test]
    fn caption_buttons_fit_the_minimum_window_width() {
        let mut h = WindowHarness::new();
        h.size = vec2(900.0, 560.0);
        h.frame(vec![]);
        h.frame(vec![]);
        for id in ["window.minimize", "window.maximize", "window.close"] {
            let rect = h.ctx.read_response(egui::Id::new(id)).unwrap().rect;
            assert!(rect.left() >= 0.0 && rect.right() <= h.size.x, "{id}: {rect:?}");
            let p = rect.center();
            assert!(!h.click(p).is_empty(), "{id} remains clickable");
        }
    }
}
