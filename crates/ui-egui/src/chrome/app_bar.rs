use super::{
    APP_BAR, APP_BAR_BAND, Color32, DesignApp, MAC_APP_BAR, MAC_CONTENT_LEFT, SEPARATOR, Sense, Stroke, Tokens, icons, json, semibold, vec2,
};

pub(super) fn search_id() -> egui::Id {
    egui::Id::unique("app_bar_search")
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
                .align(egui::Align2::LEFT_CENTER)
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

/// Lay out the application bar, native/custom title handling, menu and tool scrolling, drag region, caption controls and search.
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
                let _ = craft_window::title_bar::empty_area(ui, band);
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
                let response = ui.interact(drag, egui::Id::unique("window_title_drag"), Sense::click_and_drag());
                // Only from an empty part of the bar: the lower band, or the gap between the menus
                // and the tools (as they were last frame). A menu title pressed and dragged doesn't
                // move the window, as with native title bars.
                let (gap0, gap1) = ui
                    .data(|d| (d.get_temp::<f32>(egui::Id::unique("app_bar_menus_end")), d.get_temp::<f32>(egui::Id::unique("app_bar_tools_start"))));
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
                let tools_w_id = ui.scope_id().with("tools_width");
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
                        ui.data_mut(|d| d.insert_temp(egui::Id::unique("app_bar_menus_end"), menus_end));
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
                            ui.data_mut(|d| d.insert_temp(egui::Id::unique("app_bar_tools_start"), tools_start));
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
        let response = ui.interact(rect, egui::Id::unique(command), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
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
        let response = ui.interact(region, egui::Id::unique(("window_resize", name)), Sense::drag()).on_hover_cursor(cursor);
        if response.drag_started_by(egui::PointerButton::Primary) {
            let _ = app.run("window.resize", json!({"edge": name}));
            app.flush_window_commands(ui.ctx());
        }
    }
}
