//! The Tools panel (left): single or double column, tool groups with flyouts, fill/stroke proxy,
//! screen mode buttons.

use designcraft_tools::{SPLIT_IN_TWO_COLUMNS, TOOL_GROUPS};
use egui::{Color32, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{DesignApp, icons};

const BTN: f32 = 24.0;

/// Draws the adaptive Tools panel with localized tool flyouts, fill/stroke controls and
/// screen-mode buttons; flyout rows accommodate translated labels and their shortcuts.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        framework::show(app, ui);
        return;
    }
    let t = Tokens::get(ui.ctx());
    let frame = egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.divider));
    // A single column that doesn't fit the window's height becomes two, without changing the
    // preference; whatever two columns still can't fit scrolls.
    let single_h_id = egui::Id::unique("tools_single_column_height");
    let single_h: Option<f32> = ui.data(|d| d.get_temp(single_h_id));
    let viewport_h = ui.available_height() - frame.total_margin().sum().y;
    let crowded = !app.ui.tools_double_column && single_h.is_some_and(|h| h > viewport_h);
    let cols = if app.ui.tools_double_column || crowded { 2 } else { 1 };
    let width = 8.0 + BTN * cols as f32 + 4.0 * (cols as f32 - 1.0) + 8.0;
    let r = egui::Panel::left("tools").exact_size(width).resizable(false).frame(frame).show(ui, |ui| {
        crate::widgets::overflow_scrolling(ui);
        let scroll = egui::ScrollArea::vertical().id_salt("tools_scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add_space(3.0);
            // Collapse chevrons (nothing to toggle while the window forces two columns).
            ui.horizontal(|ui| {
                ui.add_space(width / 2.0 - 8.0);
                let toggle = ui.add_enabled_ui(!crowded, |ui| {
                    icons::button(
                        ui,
                        if cols == 1 { "double-chevron-right" } else { "double-chevron-left" },
                        16.0,
                        false,
                        crate::i18n::tr(&app.ui.language, "Toggle double column"),
                    )
                });
                if toggle.inner.clicked() {
                    app.ui.tools_double_column = !app.ui.tools_double_column;
                }
            });
            ui.add_space(2.0);
            let current = app.session.tool_id();
            let mut chosen: Option<&'static str> = None;
            let mut opened_now = false;
            // A slot is a group (its current tool shown, the others in its flyout) or, in two
            // columns, one tool of a group InDesign lays out tool by tool.
            let mut row_items: Vec<Option<(usize, Option<usize>)>> = Vec::new();
            for (gi, g) in TOOL_GROUPS.iter().enumerate() {
                if g.is_empty() {
                    row_items.push(None);
                } else if cols == 2 && SPLIT_IN_TWO_COLUMNS.contains(&g[0].id) {
                    row_items.extend((0..g.len()).map(|k| Some((gi, Some(k)))));
                } else {
                    row_items.push(Some((gi, None)));
                }
            }
            let mut i = 0;
            while i < row_items.len() {
                match row_items[i] {
                    None => {
                        let r = ui.available_rect_before_wrap();
                        ui.painter().line_segment(
                            [egui::pos2(r.min.x + 6.0, r.min.y + 2.0), egui::pos2(r.max.x - 6.0, r.min.y + 2.0)],
                            Stroke::new(1.0, t.divider),
                        );
                        ui.add_space(5.0);
                        i += 1;
                    }
                    Some(_) => {
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for _ in 0..cols {
                                let Some(Some((gi, pinned))) = row_items.get(i).copied() else { break };
                                let group = TOOL_GROUPS[gi];
                                let g = match pinned {
                                    Some(k) => &group[k..=k],
                                    None => group,
                                };
                                let shown = g.iter().find(|tl| tl.id == current).unwrap_or(&g[0]);
                                let active = g.iter().any(|tl| tl.id == current);
                                let name = crate::i18n::tr(&app.ui.language, shown.label);
                                let tip = match crate::menus::tool_shortcut_label(app, shown.id) {
                                    Some(s) => format!("{name} ({s})"),
                                    None => name.to_string(),
                                };
                                // The standalone Line slot does not adopt the grouped flyout behavior.
                                let resp = if g.len() == 1 && shown.id == "line" {
                                    line_tool_button(app, ui, shown.icon, active, &tip)
                                } else {
                                    tool_button(ui, shown.icon, active, &tip)
                                };
                                if g.len() > 1 {
                                    // Flyout triangle.
                                    let r = resp.rect;
                                    let p = r.right_bottom() - vec2(3.0, 3.0);
                                    ui.painter().add(egui::Shape::convex_polygon(
                                        vec![p, p - vec2(4.0, 0.0), p - vec2(0.0, 4.0)],
                                        t.icon,
                                        Stroke::NONE,
                                    ));
                                }
                                // The release that ends a hold which opened the flyout isn't a click
                                // on the tool (egui reports releases within 0.8 s as clicks).
                                let hold_id = egui::Id::unique(("flyout_hold", gi));
                                let ended_hold = resp.clicked() && ui.data_mut(|d| d.remove_temp::<bool>(hold_id)).unwrap_or(false);
                                // Plain click selects the shown tool; Alt/Option-click cycles the flyout
                                // group, as the letter-key cycle does. Read the modifiers from the
                                // primary-release event (Alt may already be up by the time this frame
                                // reads i.modifiers), preferring a release on this slot; egui still
                                // counts a click when the pointer drifts a little outside, so a release
                                // whose press started here counts too, and one pressed elsewhere doesn't.
                                let click_mods = ui.input(|i| {
                                    let press_here = i.pointer.press_origin().is_some_and(|o| resp.rect.contains(o));
                                    i.events
                                        .iter()
                                        .rev()
                                        .find_map(|e| match e {
                                            egui::Event::PointerButton {
                                                pos,
                                                button: egui::PointerButton::Primary,
                                                pressed: false,
                                                modifiers,
                                                ..
                                            } if resp.rect.contains(*pos) || press_here => Some(*modifiers),
                                            _ => None,
                                        })
                                        .unwrap_or(i.modifiers)
                                });
                                let alt = click_mods.alt;
                                let modified = click_mods.alt || click_mods.shift || click_mods.ctrl || click_mods.command;
                                // egui's double-click ignores modifiers: remember whether the previous
                                // click was modified so dialogs only open for two plain clicks.
                                let prev_mod_id = egui::Id::unique(("tool-click-mod", gi));
                                let prev_modified = ui.data(|d| d.get_temp::<bool>(prev_mod_id)).unwrap_or(false);
                                let slot_click = resp.clicked() && !ended_hold;
                                if slot_click {
                                    chosen = Some(if g.len() > 1 && alt {
                                        let from = g.iter().position(|tl| tl.id == current).unwrap_or(0);
                                        g[(from + 1) % g.len()].id
                                    } else {
                                        shown.id
                                    });
                                }
                                if !resp.is_pointer_button_down_on() {
                                    ui.data_mut(|d| d.remove::<bool>(hold_id));
                                }
                                // Double-click a polygon tool: Polygon Settings.
                                // Modified clicks only cycle / select: exclude them from dialogs.
                                let plain_double = resp.double_clicked() && !modified && !prev_modified;
                                if plain_double && matches!(shown.id, "polygon" | "polygonFrame") {
                                    let cur = app.session.execute("tool.polygonSettings", &json!({})).unwrap_or_default();
                                    app.ui.dialog = Some(crate::dialogs::Dialog::new("polygonSettings", cur));
                                }
                                // Double-click a transform tool: its Object › Transform dialog.
                                if plain_double && let Some(cmd) = transform_dialog_of(shown.id) {
                                    crate::menus::activate(app, cmd, &Value::Null);
                                }
                                if slot_click {
                                    ui.data_mut(|d| d.insert_temp(prev_mod_id, modified));
                                }
                                let held =
                                    resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.press_start_time().is_some_and(|s| i.time - s > 0.35));
                                if g.len() > 1 && held {
                                    ui.data_mut(|d| d.insert_temp(hold_id, true));
                                }
                                if g.len() > 1 && (resp.secondary_clicked() || resp.long_touched() || held) {
                                    opened_now |= app.ui.flyout != Some(gi);
                                    app.ui.flyout = Some(gi);
                                }
                                if app.ui.flyout == Some(gi) {
                                    let pos = resp.rect.right_top() + vec2(4.0, 0.0);
                                    egui::Area::new(egui::Id::unique(("flyout", gi))).order(egui::Order::Foreground).fixed_pos(pos).show(
                                        ui.ctx(),
                                        |ui| {
                                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                                let row_width = g.iter().fold(210.0_f32, |width, tl| {
                                                    let label = crate::i18n::tr(&app.ui.language, tl.label);
                                                    let label_width = ui
                                                        .painter()
                                                        .layout_no_wrap(label.to_owned(), egui::FontId::proportional(12.5), t.text)
                                                        .size()
                                                        .x;
                                                    let shortcut_width = ui
                                                        .painter()
                                                        .layout_no_wrap(
                                                            crate::menus::tool_shortcut_label(app, tl.id).unwrap_or_default(),
                                                            egui::FontId::proportional(11.5),
                                                            t.text_dim,
                                                        )
                                                        .size()
                                                        .x;
                                                    width.max(label_width + shortcut_width + 46.0)
                                                });
                                                let max_width = (ui.ctx().content_rect().width()
                                                    - egui::Frame::popup(ui.style()).total_margin().sum().x)
                                                    .max(0.0);
                                                let mut scroll =
                                                    egui::ScrollArea::horizontal().id_salt(("tool-flyout-scroll", gi)).max_width(max_width);
                                                if opened_now {
                                                    scroll = scroll.horizontal_scroll_offset(0.0);
                                                }
                                                scroll.show(ui, |ui| {
                                                    for tl in g {
                                                        ui.horizontal(|ui| {
                                                            let (r, row) = ui.allocate_exact_size(vec2(row_width, 24.0), Sense::click());
                                                            if row.hovered() {
                                                                ui.painter().rect_filled(r, 2.0, t.hover);
                                                            }
                                                            icons::paint(
                                                                ui.painter(),
                                                                egui::Rect::from_min_size(r.min + vec2(4.0, 3.0), vec2(18.0, 18.0)),
                                                                tl.icon,
                                                                t.icon,
                                                            );
                                                            if tl.id == current {
                                                                ui.painter().circle_filled(r.min + vec2(-2.0, 12.0), 2.0, t.accent);
                                                            }
                                                            ui.painter().text(
                                                                r.min + vec2(28.0, 12.0),
                                                                egui::Align2::LEFT_CENTER,
                                                                crate::i18n::tr(&app.ui.language, tl.label),
                                                                egui::FontId::proportional(12.5),
                                                                t.text,
                                                            );
                                                            if let Some(sc) = crate::menus::tool_shortcut_label(app, tl.id) {
                                                                ui.painter().text(
                                                                    r.right_center() - vec2(6.0, 0.0),
                                                                    egui::Align2::RIGHT_CENTER,
                                                                    sc,
                                                                    egui::FontId::proportional(11.5),
                                                                    t.text_dim,
                                                                );
                                                            }
                                                            row.widget_info(|| {
                                                                egui::WidgetInfo::labeled(
                                                                    egui::Role::Button,
                                                                    true,
                                                                    crate::i18n::tr(&app.ui.language, tl.label),
                                                                )
                                                            });
                                                            if row.clicked() {
                                                                chosen = Some(tl.id);
                                                            }
                                                        });
                                                    }
                                                });
                                            });
                                        },
                                    );
                                }
                                i += 1;
                                if matches!(row_items.get(i), Some(None) | None) {
                                    break;
                                }
                            }
                        });
                    }
                }
            }
            if let Some(id) = chosen {
                app.select_tool(id);
            }
            // As InDesign's, an open flyout closes on Esc and on a click anywhere outside it (a
            // panel, a swatch, the canvas), not only on the canvas. A new press counts, not a
            // click: the release that ends the hold which opened it is one.
            if let Some(gi) = app.ui.flyout
                && chosen.is_none()
                && !opened_now
            {
                let area = ui.ctx().memory(|m| m.area_rect(egui::Id::unique(("flyout", gi))));
                let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| area.is_none_or(|a| !a.contains(p))));
                let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                if outside || esc {
                    app.ui.flyout = None;
                }
            }
            ui.add_space(8.0);
            fill_stroke_proxy(app, ui, width, cols);
        });
        if cols == 1 {
            let h = scroll.content_size.y;
            ui.data_mut(|d| d.insert_temp(single_h_id, h));
            // Laid out in one column that turned out too tall: lay out again in two.
            if h > viewport_h {
                ui.ctx().request_discard("tools panel: one column does not fit");
            }
        }
    });
    if std::env::var_os("DESIGNCRAFT_DEBUG_LAYOUT").is_some() {
        eprintln!("tools panel rect {:?} (wanted width {width}); remaining {:?}", r.response.rect, ui.available_rect_before_wrap());
    }
}

/// Row 1 of the proxy: the default-colours and swap buttons.
fn proxy_default_swap(app: &mut DesignApp, ui: &mut egui::Ui, t: &Tokens, x0: f32, width: f32) {
    let (row, _) = ui.allocate_exact_size(vec2(width, 11.0), Sense::hover());
    let def = egui::Rect::from_min_size(egui::pos2(x0, row.min.y), vec2(10.0, 10.0));
    icons::paint(ui.painter(), def, "default-colors", t.icon);
    if ui
        .interact(def, ui.scope_id().with("proxy_default"), Sense::click())
        .on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Default Fill and Stroke (D)"));
        })
        .clicked()
    {
        let _ = app.run("color.defaultFillStroke", json!({}));
    }
    let swap = egui::Rect::from_min_size(egui::pos2(x0 + 19.0, row.min.y), vec2(11.0, 11.0));
    icons::paint(ui.painter(), swap, "swap", t.icon);
    if ui
        .interact(swap, ui.scope_id().with("proxy_swap_btn"), Sense::click())
        .on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Swap Fill and Stroke (Shift+X)"));
        })
        .clicked()
    {
        let _ = app.run("color.swapFillStroke", json!({}));
    }
}

/// The area under the tools (InDesign 2026 §3 group 5): mini Default Fill/Stroke and Swap, the
/// overlapping Fill/Stroke proxy, Formatting affects container / text, the Apply None well, a
/// utility button and the Screen Mode well.
fn fill_stroke_proxy(app: &mut DesignApp, ui: &mut egui::Ui, width: f32, cols: usize) {
    let t = Tokens::get(ui.ctx());
    let target = crate::panels::color_target(app);
    let doc = app.session.active().map(|d| d.doc.clone());
    let stroke_front = app.ui.stroke_front;
    let text_mode = crate::panels::colors_affect_text(app) || app.ui.formatting_affects_text;
    let (fill_sw, stroke_sw) = match &target {
        Some((fill, _, stroke, _)) => (fill.clone(), stroke.clone()),
        // Nothing selected: the colours new shapes are drawn with.
        None => doc
            .as_ref()
            .map_or(("[None]".to_string(), "[Black]".to_string()), |d| (d.settings.new_object.fill.clone(), d.settings.new_object.stroke.clone())),
    };
    let x0 = ui.max_rect().min.x + (width - 30.0) / 2.0;
    proxy_default_swap(app, ui, &t, x0, width);
    ui.add_space(2.0);
    // Proxy: 19.5 pt squares, fill top-left, stroke bottom-right.
    let (r, _) = ui.allocate_exact_size(vec2(width, 31.0), Sense::hover());
    let fill_r = egui::Rect::from_min_size(egui::pos2(x0, r.min.y), vec2(19.5, 19.5));
    let stroke_r = egui::Rect::from_min_size(egui::pos2(x0 + 10.0, r.min.y + 10.0), vec2(19.5, 19.5));
    let (fc, fg) = match &doc {
        Some(d) => crate::widgets::swatch_colors(d, &fill_sw, 1.0),
        None => (None, None),
    };
    let sc = doc.as_ref().and_then(|d| crate::widgets::swatch_colors(d, &stroke_sw, 1.0).0);
    let p = ui.painter();
    let paint_stroke = |p: &egui::Painter| {
        match sc {
            Some(c) => {
                p.rect_filled(stroke_r, 0.0, c);
                p.rect_filled(stroke_r.shrink(5.5), 0.0, t.panel);
                p.rect_stroke(stroke_r.shrink(5.5), 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
            }
            None => {
                crate::widgets::paint_chip(p, stroke_r, None, None);
                p.rect_filled(stroke_r.shrink(5.5), 0.0, t.panel);
            }
        }
        p.rect_stroke(stroke_r, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    };
    let paint_fill = |p: &egui::Painter| {
        crate::widgets::paint_chip(p, fill_r, fc, fg);
        p.rect_stroke(fill_r, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    };
    if stroke_front {
        paint_fill(p);
        paint_stroke(p);
    } else {
        paint_stroke(p);
        paint_fill(p);
    }
    let fill_tip = crate::i18n::tr(&app.ui.language, "Fill (X)");
    let fresp = ui.interact(fill_r, ui.scope_id().with("proxy_fill"), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, fill_tip);
    });
    fresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, fill_tip));
    let stroke_tip = crate::i18n::tr(&app.ui.language, "Stroke (X)");
    let sresp = ui.interact(stroke_r.translate(vec2(0.0, 0.0)), ui.scope_id().with("proxy_stroke"), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, stroke_tip);
    });
    sresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, stroke_tip));
    if fresp.clicked() || sresp.clicked() {
        let front =
            if stroke_front { !fresp.clicked() } else { sresp.clicked() && !fill_r.contains(sresp.interact_pointer_pos().unwrap_or_default()) };
        app.ui.stroke_front = front;
    }
    // A double-click opens the Color Picker for that box (as InDesign's), with the colour of what
    // colour edits apply to (see `colors_affect_text`).
    if fresp.double_clicked() || sresp.double_clicked() {
        let on_fill = fresp.double_clicked() && (!stroke_front || !sresp.double_clicked());
        let stroke = !on_fill;
        let current = if stroke { &stroke_sw } else { &fill_sw };
        let mut fields = crate::panels::swatches::picker_fields(doc.as_ref().and_then(|d| d.resolve_color(current, 1.0)), stroke);
        fields["text"] = json!(crate::panels::colors_affect_text(app));
        app.ui.dialog = Some(crate::dialogs::Dialog::new("colorPicker", fields));
    }
    ui.add_space(3.0);
    // Formatting affects container / text.
    let (row, _) = ui.allocate_exact_size(vec2(width, 13.0), Sense::hover());
    for (k, (icon, tip)) in
        [("format-container", "Formatting affects container (J)"), ("format-text", "Formatting affects text (J)")].into_iter().enumerate()
    {
        let r = egui::Rect::from_min_size(egui::pos2(x0 - 1.0 + k as f32 * 18.0, row.min.y), vec2(13.0, 13.0));
        let on = text_mode == (k == 1);
        if on {
            ui.painter().rect(r, 1.5, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Inside);
        }
        icons::paint(ui.painter(), r.shrink(1.5), icon, if on { t.text_strong } else { t.icon });
        if ui
            .interact(r, ui.scope_id().with(("proxy_fmt", k)), Sense::click())
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, tip));
            })
            .clicked()
        {
            let _ = app.run("app.formattingAffectsText", json!({"on": k == 1}));
        }
    }
    ui.add_space(5.0);
    // Apply Color / Apply Gradient / Apply None: three buttons in two columns, as InDesign lays
    // them out; one well with the kind in front (and a flyout of the three) in one column.
    let front_sw = if stroke_front { &stroke_sw } else { &fill_sw };
    let front_kind = match doc.as_ref() {
        Some(_) if front_sw == designcraft_color::swatch::NONE => 2,
        Some(d) if designcraft_color::swatch::resolve_gradient(&d.swatches, front_sw).is_some() => 1,
        _ => 0,
    };
    let applies: [(&str, &str); 3] =
        [("Apply Color (,)", "color.applyColor"), ("Apply Gradient (.)", "color.applyGradient"), ("Apply None (/)", "color.applyNone")];
    // Apply Color shows the colour it applies.
    let last_color = app.ui.last_color.as_deref().and_then(|sw| doc.as_ref().and_then(|d| crate::widgets::swatch_colors(d, sw, 1.0).0));
    let paint_kind = |p: &egui::Painter, r: egui::Rect, k: usize| {
        let chip = egui::Rect::from_center_size(r.center(), vec2(13.0, 13.0));
        match k {
            0 => {
                p.rect_filled(chip, 0.0, last_color.unwrap_or(Color32::BLACK));
            }
            1 => crate::widgets::paint_chip(p, chip, None, Some((Color32::WHITE, Color32::BLACK))),
            _ => crate::widgets::paint_chip(p, chip, None, None),
        }
        p.rect_stroke(chip, 0.0, Stroke::new(0.5, t.border), StrokeKind::Outside);
    };
    let (row, _) = ui.allocate_exact_size(vec2(width, 21.0), Sense::hover());
    if cols == 2 {
        let pitch = 19.0;
        for (k, (tip, id)) in applies.into_iter().enumerate() {
            let c = egui::pos2(row.min.x + width / 2.0 + (k as f32 - 1.0) * pitch, row.center().y);
            let r = egui::Rect::from_center_size(c, vec2(18.0, 18.0));
            let resp = ui.interact(r, ui.scope_id().with(("proxy_apply", k)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, crate::i18n::tr(&app.ui.language, tip)));
            if k == front_kind {
                ui.painter().rect(r, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Outside);
            }
            paint_kind(ui.painter(), r, k);
            if resp
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, tip));
                })
                .clicked()
            {
                let _ = app.run(id, json!({}));
            }
        }
    } else {
        let well = egui::Rect::from_center_size(egui::pos2(row.min.x + width / 2.0, row.center().y), vec2(28.0, 21.0));
        let (tip, id) = applies[front_kind];
        let resp = ui.interact(well, ui.scope_id().with("proxy_apply"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, crate::i18n::tr(&app.ui.language, tip)));
        let resp = resp.on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, tip));
        });
        ui.painter().rect(well, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Outside);
        paint_kind(ui.painter(), well, front_kind);
        flyout_triangle(ui.painter(), well, t.icon);
        if resp.clicked() {
            let _ = app.run(id, json!({}));
        }
        egui::Popup::context_menu(&resp).show(|ui| {
            for (tip, id) in applies {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, tip))).clicked() {
                    let _ = app.run(id, json!({}));
                    ui.close();
                }
            }
        });
    }
    ui.add_space(6.0);
    separator(ui, &t);
    // View Options (InDesign's: Frame Edges, Rulers, Guides, Smart Guides, Baseline Grid, Hidden
    // Characters, each checked when shown).
    ui.vertical_centered(|ui| {
        let resp = tool_button(ui, "view-options", false, crate::i18n::tr(&app.ui.language, "View Options"));
        flyout_triangle(ui.painter(), resp.rect.shrink2(vec2(2.0, 2.0)), t.icon);
        let open = resp.clicked();
        let popup = egui::Popup::menu(&resp).open_memory(open.then_some(egui::SetOpenCommand::Toggle));
        popup.show(|ui| {
            for (k, id) in
                ["view.frameEdges", "view.rulers", "-", "view.guides", "view.smartGuides", "view.baselineGrid", "-", "view.hiddenCharacters"]
                    .into_iter()
                    .enumerate()
            {
                if id == "-" {
                    ui.separator();
                    continue;
                }
                let on = crate::menus::checked(app, id, &Value::Null).unwrap_or(false);
                let label = VIEW_OPTIONS[k];
                let sc = crate::menus::shortcut_label(app, id, &Value::Null).unwrap_or_default();
                let text = format!("{}  {}", if on { "✓" } else { "   " }, crate::i18n::tr(&app.ui.language, label));
                if ui.add(egui::Button::new(crate::rtl::widget(ui, &text)).shortcut_text(sc)).clicked() {
                    let _ = app.run(id, json!({}));
                    ui.close();
                }
            }
        });
    });
    separator(ui, &t);
    // Screen Mode: Normal and Preview (with Bleed, Slug and Presentation) side by side in two
    // columns, as InDesign's; one well with the five in its flyout in one column.
    let (row, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let mode = app.ui.screen_mode;
    let modes = [("Preview", "preview"), ("Bleed", "bleed"), ("Slug", "slug"), ("Presentation", "presentation")];
    if cols == 2 {
        let normal = egui::Rect::from_center_size(egui::pos2(row.min.x + width / 2.0 - 15.0, row.center().y), vec2(28.0, 21.0));
        let other = egui::Rect::from_center_size(egui::pos2(row.min.x + width / 2.0 + 15.0, row.center().y), vec2(28.0, 21.0));
        let is_normal = mode == crate::ScreenMode::Normal;
        for (r, on, icon) in [(normal, is_normal, "screen-normal"), (other, !is_normal, "screen-preview")] {
            if on {
                ui.painter().rect(r, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Outside);
            }
            icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), vec2(15.0, 15.0)), icon, if on { t.text_strong } else { t.icon });
        }
        flyout_triangle(ui.painter(), other, t.icon);
        let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_string();
        let (normal_tip, other_tip) = (tr("Normal"), format!("{} (W)", tr("Preview")));
        let resp = ui.interact(normal, ui.scope_id().with("screen_normal"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &normal_tip));
        if resp.on_hover_text(&normal_tip).clicked() {
            let _ = app.run("view.screenMode", json!({"mode": "normal"}));
        }
        let resp = ui.interact(other, ui.scope_id().with("screen_mode"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &other_tip));
        let resp = resp.on_hover_text(&other_tip);
        if resp.clicked() {
            let _ = app.run("view.screenMode", json!({"mode": "preview"}));
        }
        egui::Popup::context_menu(&resp).show(|ui| {
            for (label, m) in modes {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    let _ = app.run("view.screenMode", json!({"mode": m}));
                    ui.close();
                }
            }
        });
        return;
    }
    let well = egui::Rect::from_center_size(egui::pos2(row.min.x + width / 2.0, row.center().y), vec2(28.0, 21.0));
    let resp = ui.interact(well, ui.scope_id().with("screen_mode"), Sense::click());
    ui.painter().rect(well, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Outside);
    let preview = mode == crate::ScreenMode::Preview;
    icons::paint(
        ui.painter(),
        egui::Rect::from_center_size(well.center(), vec2(15.0, 15.0)),
        if preview { "screen-preview" } else { "screen-normal" },
        t.text_strong,
    );
    flyout_triangle(ui.painter(), well, t.icon);
    let resp = resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, if preview { "Preview (W)" } else { "Normal (W)" }));
    });
    if resp.clicked() {
        let _ = app.run("view.togglePreview", json!({}));
    }
    egui::Popup::context_menu(&resp).show(|ui| {
        for (label, mode) in std::iter::once(("Normal", "normal")).chain(modes) {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run("view.screenMode", json!({"mode": mode}));
                ui.close();
            }
        }
    });
}

/// The View Options flyout's labels, by position (separators keep theirs empty).
const VIEW_OPTIONS: [&str; 8] = ["Frame Edges", "Rulers", "", "Guides", "Smart Guides", "Baseline Grid", "", "Hidden Characters"];

/// The Object › Transform command a double-clicked tool opens (the Selection tool opens Move).
fn transform_dialog_of(tool: &str) -> Option<&'static str> {
    match tool {
        "selection" => Some("transform.move"),
        "rotate" => Some("transform.rotate"),
        "scale" => Some("transform.scale"),
        "shear" => Some("transform.shear"),
        _ => None,
    }
}

fn flyout_triangle(p: &egui::Painter, r: egui::Rect, c: Color32) {
    let q = r.right_bottom() - vec2(2.0, 2.0);
    p.add(egui::Shape::convex_polygon(vec![q, q - vec2(4.0, 0.0), q - vec2(0.0, 4.0)], c, Stroke::NONE));
}

fn separator(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(4.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [egui::pos2(r.min.x + 6.0, r.min.y), egui::pos2(r.max.x - 6.0, r.min.y)],
        Stroke::new(1.0, Color32::from_rgb(0x4c, 0x4b, 0x4b)),
    );
    let _ = t;
    ui.add_space(5.0);
}

/// The Line Tool is a standalone slot: leave every grouped tool's flyout lifecycle alone.
fn line_tool_button(app: &DesignApp, ui: &mut egui::Ui, icon: &str, active: bool, tip: &str) -> egui::Response {
    let target = app.session.active().map(|st| {
        format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells))
    });
    let id = ui.scope_id().with(("tools-line", target));
    ui.push_id(id, |ui| tool_button(ui, icon, active, tip)).inner
}

/// A Tools-panel button: 24 pt pitch; the active tool sits in a 28×20 pt `#303030` well with a rim.
fn tool_button(ui: &mut egui::Ui, icon: &str, active: bool, tip: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(egui::vec2(BTN, BTN), Sense::click());
    if active {
        let well = egui::Rect::from_center_size(r.center(), egui::vec2(28.0, 20.0));
        ui.painter().rect(well, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Outside);
    } else if resp.hovered() {
        ui.painter().rect_filled(egui::Rect::from_center_size(r.center(), egui::vec2(28.0, 20.0)), 2.0, t.hover);
    }
    icons::paint(ui.painter(), egui::Rect::from_center_size(r.center(), egui::vec2(17.0, 17.0)), icon, if active { t.text_strong } else { t.icon });
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    })
}

/// Default Fill and Stroke (D): text gets a black fill and no stroke, objects no fill and a black
/// stroke. Colour keys act on what the proxy shows (see `panels::colors_affect_text`).
pub(crate) fn default_fill_stroke(app: &mut DesignApp) -> Result<Value, String> {
    if crate::panels::colors_affect_text(app) {
        let attrs = json!({"fill": "[Black]", "fillTint": 1.0, "stroke": "[None]", "strokeTint": 1.0});
        return app.run("type.char", json!({ "attrs": attrs }));
    }
    app.run("object.defaultFillStroke", json!({}))
}

/// Swap Fill and Stroke (Shift+X): the text's fill and stroke, or each selected object's, trade
/// places, tints included.
pub(crate) fn swap_fill_stroke(app: &mut DesignApp) -> Result<Value, String> {
    if crate::panels::colors_affect_text(app) { app.run("type.swapFillStroke", json!({})) } else { app.run("object.swapFillStroke", json!({})) }
}

/// Fill/Stroke toggle (X): brings the other of fill and stroke to the front of the proxy.
pub(crate) fn toggle_fill_stroke(app: &mut DesignApp) -> Result<Value, String> {
    app.ui.stroke_front = !app.ui.stroke_front;
    Ok(json!({"strokeFront": app.ui.stroke_front}))
}

/// Apply Color (,): the colour applied last ([Black] at first) to the fill or stroke in front.
pub(crate) fn apply_color(app: &mut DesignApp) -> Result<Value, String> {
    let sw = app.ui.last_color.clone().filter(|s| app.session.active().is_some_and(|d| d.doc.swatch(s).is_some()));
    crate::panels::apply_swatch(app, app.ui.stroke_front, sw.as_deref().unwrap_or("[Black]"), None)
}

/// Apply Gradient (.): the gradient applied last, else the document's first gradient swatch, else
/// a new white-to-black gradient (InDesign's default), on the fill or stroke in front.
pub(crate) fn apply_gradient(app: &mut DesignApp) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let known = app
        .ui
        .last_gradient
        .clone()
        .filter(|s| designcraft_color::swatch::resolve_gradient(&st.doc.swatches, s).is_some())
        .or_else(|| st.doc.swatches.iter().find(|w| matches!(w.value, designcraft_color::SwatchValue::Gradient { .. })).map(|w| w.name.clone()));
    let stroke = app.ui.stroke_front;
    if let Some(sw) = known {
        return crate::panels::apply_swatch(app, stroke, &sw, None);
    }
    if crate::panels::colors_affect_text(app) {
        return Err("no gradient to apply yet: make one in the Gradient panel".into());
    }
    let stops = json!([{"location": 0, "color": "#ffffff"}, {"location": 100, "color": "#000000"}]);
    let r = app.run("object.gradient", json!({"stops": stops, "target": if stroke { "stroke" } else { "fill" }}))?;
    if let Some(sw) = r["swatch"].as_str() {
        app.ui.last_gradient = Some(sw.to_string());
    }
    Ok(r)
}

/// Apply None (/): to the fill or the stroke, whichever is in front of the proxy, of the text or
/// the objects.
pub(crate) fn apply_none(app: &mut DesignApp) -> Result<Value, String> {
    crate::panels::apply_swatch(app, app.ui.stroke_front, "[None]", None)
}

/// The tools as framework commands, for the board in the language `lang`: each tool's own keymap command
/// (`keys::tool`), its translated label and its artwork's name. They carry no default chords: the app's keymap is the
/// one dispatcher of every key, and the board only resolves and fires these for the Tools rail. One catalog per
/// language, so its generation moves only when the language does.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn tool_catalog(lang: &str) -> craft_ui_widgets::runtime::commands::AppCatalog {
    use craft_ui_widgets::runtime::commands::AppCatalog;
    use std::sync::{Mutex, PoisonError};
    static CATALOGS: Mutex<Vec<(String, AppCatalog)>> = Mutex::new(Vec::new());
    let mut catalogs = CATALOGS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, catalog)) = catalogs.iter().find(|(code, _)| code == lang) {
        return catalog.clone();
    }
    let defs: Vec<craft_keymap::CommandDef> = crate::keys::catalog()
        .iter()
        .filter_map(|def| {
            let tool = crate::keys::tool_of(&def.command)?;
            let icon = TOOL_GROUPS.iter().flat_map(|g| g.iter()).find(|t| t.id == tool).map(|t| t.icon);
            let mut def = def.clone();
            def.defaults.clear();
            def.label = crate::i18n::tr(lang, &def.label).to_owned();
            if let Some(icon) = icon {
                def = def.with_icon(icon);
            }
            Some(def)
        })
        .collect();
    let generation = catalogs.len() as u64 + 1;
    let catalog = AppCatalog::new(defs, generation).unwrap_or_else(|error| {
        log::warn!("the tool commands are not on the framework's board: {error}");
        AppCatalog::new(Vec::new(), generation).expect("an empty catalog is valid")
    });
    catalogs.push((lang.to_owned(), catalog.clone()));
    catalog
}

/// The Tools panel on the framework: the tool rail with its slots and flyouts, and under it, in the rail's footer, the
/// fill and stroke pair, Formatting affects container or text, the apply buttons, View Options and the Screen Mode.
/// The same commands, tool state and dialogs as the native panel.
#[cfg(not(target_arch = "wasm32"))]
mod framework {
    use super::{VIEW_OPTIONS, transform_dialog_of};
    use crate::DesignApp;
    use crate::framework_transform::{self as kit, Artwork, MenuRow};
    use craft_behavior::Mixed;
    use craft_color_ui::ui::{PairWell, SwatchPair, Well};
    use craft_theme::{ControlSize, Srgba};
    use craft_ui_panels::docking::{DockGrid, DockSizes};
    use craft_ui_panels::rails::{LastUsed, RailFooter, RailTool, ToolRail, ToolSlot};
    use craft_ui_widgets::buttons::{ButtonOutput, IconButton};
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _, Tracked, Trigger};
    use craft_ui_widgets::display::Divider;
    use craft_ui_widgets::layout::RowAlign;
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::primitives::{Glyph, IconSpec};
    use craft_ui_widgets::style::UiTokens;
    use designcraft_tools::{SPLIT_IN_TWO_COLUMNS, TOOL_GROUPS, ToolInfo};
    use egui::{Color32, Rect, Ui, vec2};
    use serde_json::{Value, json};
    use std::sync::OnceLock;

    /// Every tool in the panel's order: a tool's place here is its id on the rail.
    fn tools() -> &'static [&'static ToolInfo] {
        static ALL: OnceLock<Vec<&'static ToolInfo>> = OnceLock::new();
        ALL.get_or_init(|| TOOL_GROUPS.iter().flat_map(|group| group.iter()).collect())
    }

    /// Each tool's artwork, as the native panel draws it, by the tool's place in [`tools`].
    fn artwork() -> &'static [Artwork] {
        static ART: OnceLock<Vec<Artwork>> = OnceLock::new();
        ART.get_or_init(|| tools().iter().map(|tool| Artwork(tool.icon)).collect())
    }

    /// The rail's slots in `columns` columns: a slot per tool group (its current tool shown, the others in its
    /// flyout), or, in two columns, a slot per tool of a group InDesign lays out tool by tool. An empty group ends the
    /// group of slots before it.
    fn slots(columns: usize) -> Vec<ToolSlot<u8>> {
        let (all, art) = (tools(), artwork());
        let tool = |info: &ToolInfo| {
            let at = all.iter().position(|t| t.id == info.id).unwrap_or(0);
            // The app's own artwork: the icon sets have no InDesign tool drawings. (Framework gap fixed on
            // design-port/framework-gaps: `RailTool::icon`.)
            RailTool::new(at as u8, crate::keys::tool(info.id)).icon(IconSpec::glyph(&art[at]))
        };
        let mut slots: Vec<ToolSlot<u8>> = Vec::new();
        for (gi, group) in TOOL_GROUPS.iter().enumerate() {
            let Some(first) = group.first() else {
                if let Some(last) = slots.last_mut() {
                    last.group_end = true;
                }
                continue;
            };
            let id = |k: usize| (gi * 16 + k) as u32;
            if columns == 2 && SPLIT_IN_TWO_COLUMNS.contains(&first.id) {
                slots.extend(group.iter().enumerate().map(|(k, info)| ToolSlot::new(id(k), vec![tool(info)])));
            } else {
                slots.push(ToolSlot::new(id(0), group.iter().map(tool).collect()));
            }
        }
        slots
    }

    thread_local! {
        /// The slots in one and in two columns, kept so the rail's memoized grid replays while they stay the same.
        static SLOTS: [Tracked<Vec<ToolSlot<u8>>>; 2] = [Tracked::new(slots(1)), Tracked::new(slots(2))];
        /// The tool each slot showed last: none, so a slot shows its current tool, else its first, as the native
        /// panel does.
        static LAST_USED: Tracked<LastUsed<u8>> = Tracked::new(LastUsed::default());
    }

    /// The Tools panel: one column unless the preference or a window too short for one asks for two.
    pub(super) fn show(app: &mut DesignApp, ui: &mut Ui) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let all = tools();
        let current = all.iter().position(|t| t.id == app.session.tool_id()).map_or(u8::MAX, |at| at as u8);
        // A held activation belongs to this document incarnation and ordered selection: a new target is a new rail.
        let target =
            app.session.active().map(|st| format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)));
        // A held activation also belongs to one layout of the rail: a reflow to another column count is a new rail, so a
        // press held across it is retired rather than activating at its slot's new place.
        let rail_id = |cols: usize| egui::Id::new(("tools-rail", &target, cols));
        let toggle_h = 24.0;
        SLOTS.with(|slots| {
            LAST_USED.with(|last| {
                let sizes = DockSizes::of_scope(ui);
                let grid = |cols: usize| DockGrid::rail(slots[cols - 1].len()).resolved(&sizes);
                // A single column that doesn't fit the window's height becomes two, without changing the preference.
                let crowded = !app.ui.tools_double_column && {
                    let width = grid(1).width(1);
                    let tall =
                        ToolRail::new(tr("Tools"), &slots[0], &current, last).id(rail_id(1)).measure_rail(ui, width, |ui, at| footer(app, ui, at)).y;
                    tall > ui.available_height() - toggle_h
                };
                let cols = if app.ui.tools_double_column || crowded { 2 } else { 1 };
                let width = grid(cols).width(cols as u16);
                egui::Panel::left("tools").exact_size(width).resizable(false).frame(egui::Frame::NONE).show(ui, |ui| {
                    Row::new().fill_width().align(RowAlign::Center).show(ui, |ui, _| {
                        let icon = if cols == 1 { "double-chevron-right" } else { "double-chevron-left" };
                        let label = tr("Toggle double column");
                        let toggled = IconButton::new(IconSpec::glyph(&Artwork(icon)))
                            .id(egui::Id::new("tools-double-column"))
                            .label(label.clone())
                            .tooltip(label)
                            .size(ControlSize::Sm)
                            .enabled(!crowded)
                            .show(ui)
                            .clicked();
                        if toggled {
                            app.ui.tools_double_column = !app.ui.tools_double_column;
                        }
                    });
                    let out =
                        ToolRail::new(tr("Tools"), &slots[cols - 1], &current, last).id(rail_id(cols)).show_rail(ui, |ui, at| footer(app, ui, at));
                    if let Some(at) = out.select {
                        app.select_tool(all[usize::from(at)].id);
                    }
                    // A double click: Polygon Settings for a polygon tool, a transform tool's Object › Transform dialog.
                    if let Some(at) = out.double_clicked {
                        let id = all[usize::from(at)].id;
                        if matches!(id, "polygon" | "polygonFrame") {
                            let cur = app.session.execute("tool.polygonSettings", &json!({})).unwrap_or_default();
                            app.ui.dialog = Some(crate::dialogs::Dialog::new("polygonSettings", cur));
                        }
                        if let Some(cmd) = transform_dialog_of(id) {
                            crate::menus::activate(app, cmd, &Value::Null);
                        }
                    }
                });
            });
        });
    }

    /// What an apply button paints: the colour applied last, a gradient, or None.
    #[derive(craft_ui::contract::MemoProps)]
    struct Apply {
        kind: u8,
        /// The colour applied last, `[r, g, b]`, when there is one.
        last: bool,
        r: u8,
        g: u8,
        b: u8,
    }

    impl Glyph for Apply {
        fn paint(&self, painter: &egui::Painter, _: &UiTokens, rect: Rect, _: Color32, _: bool) {
            let chip = Rect::from_center_size(rect.center(), vec2(13.0, 13.0));
            match self.kind {
                0 => {
                    painter.rect_filled(chip, 0.0, if self.last { Color32::from_rgb(self.r, self.g, self.b) } else { Color32::BLACK });
                }
                1 => crate::widgets::paint_chip(painter, chip, None, Some((Color32::WHITE, Color32::BLACK))),
                _ => crate::widgets::paint_chip(painter, chip, None, None),
            }
        }
    }

    /// The menu `label` a right click on `opener` opens, of `rows` (label and key): the key of the row run this pass.
    fn context_menu<K: Clone>(ui: &mut Ui, id: egui::Id, label: &str, opener: &ButtonOutput, rows: &[(String, K)]) -> Option<K> {
        Menu::new(label.to_owned())
            .id(id)
            .opened_by(opener, Trigger::Context)
            .show(ui, |ui, _| {
                let mut ran = None;
                for (k, (label, key)) in rows.iter().enumerate() {
                    if MenuItem::new(label.clone()).id(id.with(k)).show(ui).ran {
                        ran = Some(key.clone());
                    }
                }
                ran
            })
            .inner
            .flatten()
    }

    /// An icon button of the panel's artwork `icon`, named and tipped `label`, on while `on`.
    fn button(ui: &mut Ui, key: &str, icon: &'static str, label: &str, on: bool) -> ButtonOutput {
        IconButton::new(IconSpec::glyph(&Artwork(icon)))
            .id(egui::Id::new(("tools-footer", key)))
            .label(label.to_owned())
            .tooltip(label.to_owned())
            .selected(on)
            .size(ControlSize::Sm)
            .show(ui)
    }

    /// The colour a swatch shows on a well: its colour, a gradient's first stop, or clear for None.
    fn well_color(doc: Option<&designcraft_doc::Document>, name: &str) -> Srgba {
        let (color, gradient) = doc.map_or((None, None), |d| crate::widgets::swatch_colors(d, name, 1.0));
        color
            .or(gradient.map(|(first, _)| first))
            .map_or(Srgba::new(0.0, 0.0, 0.0, 0.0), |c| Srgba::rgb(f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0))
    }

    /// The area under the tools (InDesign 2026 §3 group 5).
    fn footer(app: &mut DesignApp, ui: &mut Ui, at: RailFooter) {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let target = crate::panels::color_target(app);
        let doc = app.session.active().map(|d| d.doc.clone());
        let stroke_front = app.ui.stroke_front;
        let text_mode = crate::panels::colors_affect_text(app) || app.ui.formatting_affects_text;
        let (fill_sw, stroke_sw) = match &target {
            Some((fill, _, stroke, _)) => (fill.clone(), stroke.clone()),
            // Nothing selected: the colours new shapes are drawn with.
            None => doc.as_ref().map_or(("[None]".to_string(), "[Black]".to_string()), |d| {
                (d.settings.new_object.fill.clone(), d.settings.new_object.stroke.clone())
            }),
        };
        Stack::new().fill_width().show(ui, |ui, _| {
            // The Fill and Stroke proxy with Default Fill and Stroke and Swap over it; a click brings a box to the
            // front, a double click opens the Color Picker for it.
            let wells = [
                PairWell::new(tr("Fill (X)"), Mixed::Same(well_color(doc.as_deref(), &fill_sw))),
                PairWell::new(tr("Stroke (X)"), Mixed::Same(well_color(doc.as_deref(), &stroke_sw))).ring(true),
            ];
            let active = if stroke_front { Well::Secondary } else { Well::Primary };
            let pair = SwatchPair::new(tr("Fill and Stroke"), wells, Bind::given(active))
                .id(egui::Id::new("tools-fill-stroke"))
                .default_button(tr("Default Fill and Stroke (D)"))
                .swap_button(tr("Swap Fill and Stroke (Shift+X)"))
                .popover(false)
                .show(ui);
            if pair.defaults {
                let _ = app.run("color.defaultFillStroke", json!({}));
            }
            if pair.swap {
                let _ = app.run("color.swapFillStroke", json!({}));
            }
            if pair.activated {
                app.ui.stroke_front = pair.active == Well::Secondary;
            }
            if let Some(well) = pair.double_clicked {
                let stroke = well == Well::Secondary;
                let current = if stroke { &stroke_sw } else { &fill_sw };
                let mut fields = crate::panels::swatches::picker_fields(doc.as_ref().and_then(|d| d.resolve_color(current, 1.0)), stroke);
                fields["text"] = json!(crate::panels::colors_affect_text(app));
                app.ui.dialog = Some(crate::dialogs::Dialog::new("colorPicker", fields));
            }
            // Formatting affects container / text: side by side in two columns; in one, the two buttons don't fit the
            // rail's width beside each other (the second was clipped off the rail and took no pointer), so they stack.
            let mut formatting = |ui: &mut Ui| {
                for (k, (icon, tip)) in
                    [("format-container", "Formatting affects container (J)"), ("format-text", "Formatting affects text (J)")].into_iter().enumerate()
                {
                    if button(ui, icon, icon, &tr(tip), text_mode == (k == 1)).clicked() {
                        let _ = app.run("app.formattingAffectsText", json!({"on": k == 1}));
                    }
                }
            };
            if at.columns > 1 {
                Row::new().show(ui, |ui, _| formatting(ui));
            } else {
                Stack::new().show(ui, |ui, _| formatting(ui));
            }
            // Apply Color / Apply Gradient / Apply None: three buttons in two columns, as InDesign lays them out; one
            // with the kind in front, and a menu of the three, in one column.
            let front_sw = if stroke_front { &stroke_sw } else { &fill_sw };
            let front_kind = match doc.as_ref() {
                Some(_) if front_sw == designcraft_color::swatch::NONE => 2,
                Some(d) if designcraft_color::swatch::resolve_gradient(&d.swatches, front_sw).is_some() => 1,
                _ => 0,
            };
            let applies: [(&str, &str); 3] =
                [("Apply Color (,)", "color.applyColor"), ("Apply Gradient (.)", "color.applyGradient"), ("Apply None (/)", "color.applyNone")];
            let last_color = app.ui.last_color.as_deref().and_then(|sw| doc.as_ref().and_then(|d| crate::widgets::swatch_colors(d, sw, 1.0).0));
            let glyph = |kind: u8| {
                let c = last_color.unwrap_or(Color32::BLACK);
                Apply { kind, last: last_color.is_some(), r: c.r(), g: c.g(), b: c.b() }
            };
            let mut run = None;
            if at.columns > 1 {
                Row::new().show(ui, |ui, _| {
                    for (k, (tip, id)) in applies.into_iter().enumerate() {
                        let face = glyph(k as u8);
                        let label = tr(tip);
                        let out = IconButton::new(IconSpec::glyph(&face))
                            .id(egui::Id::new(("tools-apply", k)))
                            .label(label.clone())
                            .tooltip(label)
                            .selected(k == front_kind)
                            .size(ControlSize::Sm)
                            .show(ui);
                        if out.clicked() {
                            run = Some(id);
                        }
                    }
                });
            } else {
                let (tip, id) = applies[front_kind];
                let face = glyph(front_kind as u8);
                let label = tr(tip);
                let out = IconButton::new(IconSpec::glyph(&face))
                    .id(egui::Id::new("tools-apply"))
                    .label(label.clone())
                    .tooltip(label.clone())
                    .size(ControlSize::Sm)
                    .show(ui);
                if out.clicked() {
                    run = Some(id);
                }
                let rows: Vec<(String, &str)> = applies.iter().map(|(tip, id)| (tr(tip), *id)).collect();
                run = context_menu(ui, egui::Id::new("tools-apply-menu"), &label, &out, &rows).or(run);
            }
            if let Some(id) = run {
                let _ = app.run(id, json!({}));
            }
            Divider::horizontal().show(ui);
            // View Options: Frame Edges, Rulers, Guides, Smart Guides, Baseline Grid, Hidden Characters, each checked
            // when shown.
            let label = tr("View Options");
            let opener = button(ui, "view-options", "view-options", &label, false);
            let ids = ["view.frameEdges", "view.rulers", "-", "view.guides", "view.smartGuides", "view.baselineGrid", "-", "view.hiddenCharacters"];
            let labels: Vec<String> = VIEW_OPTIONS.iter().map(|l| tr(l)).collect();
            let mut groups: Vec<Vec<MenuRow<'_, &str>>> = vec![Vec::new()];
            for (k, id) in ids.into_iter().enumerate() {
                if id == "-" {
                    groups.push(Vec::new());
                    continue;
                }
                let checked = crate::menus::checked(app, id, &Value::Null).unwrap_or(false);
                let shortcut = crate::menus::shortcut_label(app, id, &Value::Null).filter(|s| !s.is_empty());
                if let Some(group) = groups.last_mut() {
                    group.push(MenuRow { key: id, label: &labels[k], checked: Some(checked), enabled: true, shortcut });
                }
            }
            if let Some(id) = kit::menu(ui, egui::Id::new("tools-view-options"), &label, &opener, &groups) {
                let _ = app.run(id, json!({}));
            }
            Divider::horizontal().show(ui);
            // Screen Mode: Normal and Preview (with Bleed, Slug and Presentation) side by side in two columns, as
            // InDesign's; one button with the five in its menu in one column.
            let mode = app.ui.screen_mode;
            let modes = [("Preview", "preview"), ("Bleed", "bleed"), ("Slug", "slug"), ("Presentation", "presentation")];
            let mut screen = None;
            if at.columns > 1 {
                let normal = mode == crate::ScreenMode::Normal;
                Row::new().show(ui, |ui, _| {
                    if button(ui, "screen-normal", "screen-normal", &tr("Normal"), normal).clicked() {
                        screen = Some("normal");
                    }
                    let other = format!("{} (W)", tr("Preview"));
                    let out = button(ui, "screen-mode", "screen-preview", &other, !normal);
                    if out.clicked() {
                        screen = Some("preview");
                    }
                    let rows: Vec<(String, &str)> = modes.iter().map(|(l, m)| (tr(l), *m)).collect();
                    screen = context_menu(ui, egui::Id::new("tools-screen-menu"), &other, &out, &rows).or(screen);
                });
            } else {
                let preview = mode == crate::ScreenMode::Preview;
                let label = tr(if preview { "Preview (W)" } else { "Normal (W)" });
                let out = button(ui, "screen-mode", if preview { "screen-preview" } else { "screen-normal" }, &label, true);
                if out.clicked() {
                    let _ = app.run("view.togglePreview", json!({}));
                }
                let rows: Vec<(String, &str)> = std::iter::once(("Normal", "normal")).chain(modes).map(|(l, m)| (tr(l), m)).collect();
                screen = context_menu(ui, egui::Id::new("tools-screen-menu"), &label, &out, &rows);
            }
            if let Some(mode) = screen {
                let _ = app.run("view.screenMode", json!({"mode": mode}));
            }
        });
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod text_color_tests {
    use super::*;
    use designcraft_doc::{ItemId, StoryId};

    #[test]
    fn integration_color_swap_preserves_each_run_and_frame() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut frames = vec![];
        let mut stories = vec![];
        for text in ["abcd", "efgh"] {
            let f = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":text})).unwrap();
            frames.push(f["id"].clone());
            stories.push(f["story"].as_u64().unwrap());
        }
        let pairs = [("[Paper]", "[Black]", 0.2, 0.8), ("[Black]", "[Paper]", 0.4, 0.6)];
        for (i, sid) in stories.iter().enumerate() {
            for j in 0..2 {
                let (fill, stroke, ft, st) = pairs[(i + j) % 2];
                app.run("text.select", json!({"story":sid,"anchor":j*2,"focus":j*2+2})).unwrap();
                app.run("type.char", json!({"fill":fill,"stroke":stroke,"fillTint":ft,"strokeTint":st})).unwrap();
            }
        }
        app.run("selection.set", json!({"ids":frames})).unwrap();
        app.run("app.formattingAffectsText", json!({"on":true})).unwrap();
        let before = serde_json::to_value(app.session.doc().unwrap().doc.as_ref()).unwrap();
        let n = app.session.doc().unwrap().history.undo.len();
        swap_fill_stroke(&mut app).unwrap();
        assert_eq!(app.session.doc().unwrap().history.undo.len(), n + 1);
        for (i, sid) in stories.iter().enumerate() {
            let story = app.session.doc().unwrap().doc.story(StoryId(*sid)).unwrap();
            for j in 0..2 {
                let (fill, stroke, ft, st) = pairs[(i + j) % 2];
                let a = &story.format_after(j * 2).over;
                assert_eq!((a.fill.as_deref(), a.stroke.as_deref(), a.fill_tint, a.stroke_tint), (Some(stroke), Some(fill), Some(st), Some(ft)));
            }
        }
        let swapped = serde_json::to_value(app.session.doc().unwrap().doc.as_ref()).unwrap();
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(serde_json::to_value(app.session.doc().unwrap().doc.as_ref()).unwrap(), before);
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(serde_json::to_value(app.session.doc().unwrap().doc.as_ref()).unwrap(), swapped);
    }

    #[test]
    fn integration_color_toolbar_picker_targets_selected_frame_text() {
        use egui_kittest::kittest::Queryable;
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"hello"})).unwrap();
        app.run("selection.set", json!({"ids":[f["id"]]})).unwrap();
        app.run("app.formattingAffectsText", json!({"on":true})).unwrap();
        let before = app.session.doc().unwrap().doc.item(ItemId(f["id"].as_u64().unwrap())).unwrap().fill.clone();
        let mut h = crate::test_window::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(0.001).build_ui_state(
            |ui, app: &mut DesignApp| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(6);
        let at = h.get_by_label("Fill (X)").rect().center();
        h.hover_at(at);
        h.step();
        for pressed in [true, false, true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        }
        h.run_steps(3);
        let d = h.state().ui.dialog.clone().unwrap();
        assert_eq!(d.id, "colorPicker");
        assert_eq!(d.fields["text"], true);
        h.state_mut().run("object.color", json!({"color":"#123456","target":"fill","text":d.fields["text"]})).unwrap();
        let doc = &h.state().session.doc().unwrap().doc;
        assert_eq!(doc.item(ItemId(f["id"].as_u64().unwrap())).unwrap().fill, before);
        assert!(doc.story(StoryId(f["story"].as_u64().unwrap())).unwrap().format_after(0).over.fill.is_some());
    }
}


#[cfg(all(test, not(target_arch = "wasm32")))]
mod line_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use designcraft_doc::{Document, Item, ItemId, Shape};
    use designcraft_geom::Point;
    use egui::{Event, Key, Modifiers, PointerButton, Rect, UiBuilder};
    use egui_kittest::{Harness, kittest::{NodeT, Queryable}};
    use std::{collections::HashSet, sync::Arc};

    struct Rig {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        parent: Option<Rect>,
        enabled: bool,
        sizing: bool,
        anchor: u64,
    }
    type Mark = (Arc<Document>, String, (usize, usize));

    fn step(h: &mut Harness<'_, Rig>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, count: usize) {
        for _ in 0..count { step(h); }
    }
    fn event(h: &mut Harness<'_, Rig>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn key(h: &mut Harness<'_, Rig>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, Rig>, pos: egui::Pos2, pressed: bool) {
        event(h, Event::PointerMoved(pos));
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    fn click(h: &mut Harness<'_, Rig>, pos: egui::Pos2) {
        pointer(h, pos, true);
        pointer(h, pos, false);
        idle(h, 2);
    }
    fn fixture() -> (DesignApp, u64) {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let anchor = session.execute("frame.create", &json!({"rect":[80,80,160,150],"content":"unassigned"})).unwrap()["id"].as_u64().unwrap();
        session.execute("frame.create", &json!({"rect":[10,10,30,30]})).unwrap();
        session.execute("edit.undo", &json!({})).unwrap();
        session.execute("selection.set", &json!({"ids":[anchor]})).unwrap();
        let app = DesignApp::new_for_panel_tests(session, crate::Services::default());
        (app, anchor)
    }
    fn harness(mut app: DesignApp, anchor: u64, installed: bool, size: egui::Vec2) -> Harness<'static, Rig> {
        app.run("window.workspace", json!({"name":"Advanced"})).unwrap();
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        if installed { app.install_framework(&ctx); }
        assert_eq!(app.framework_installed, installed);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut h = Harness::builder().with_context(ctx).with_size(size).build_ui_state(
            |ui, state: &mut Rig| {
                let ctx = ui.ctx().clone();
                state.app.logic(&ctx);
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        let parent_id = ui.scope_id().with("tools");
                        // The full shipping UI owns setup, fonts/theme, shortcuts, toolbar, panels, Canvas and dialogs.
                        let controls = record(ui, egui::Id::unique("line-tool-full-application"), |ui| state.app.ui(ui)).1;
                        state.controls = controls;
                        state.parent = egui::containers::panel::PanelState::load(ui.ctx(), parent_id).map(|panel| panel.outer_rect);
                    });
                });
            },
            Rig { app, controls: None, parent: None, enabled: true, sizing: false, anchor },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 6);
        h
    }
    fn ordinary(installed: bool) -> Harness<'static, Rig> {
        let (app, anchor) = fixture();
        harness(app, anchor, installed, vec2(1800.0, 1000.0))
    }

    fn mark(h: &Harness<'_, Rig>) -> Mark {
        let st = h.state().app.session.active().unwrap();
        (st.doc.clone(), format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)),
            (st.history.undo.len(), st.history.redo.len()))
    }
    /// Two documents (or items) are the same up to floating-point noise in their coordinates (1e-6 pt). The installed and the
    /// legacy window lay the canvas out at slightly different offsets (the framework rail's width is not the native
    /// panel's), so a line drawn by the same screen points lands within a few 1e-13 pt of the other.
    fn same_document<T: serde::Serialize + ?Sized>(a: &T, b: &T) -> bool {
        fn same(a: &Value, b: &Value) -> bool {
            match (a, b) {
                (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
                    (Some(x), Some(y)) => (x - y).abs() <= 1e-6,
                    _ => x == y,
                },
                (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
                (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
                _ => a == b,
            }
        }
        same(&serde_json::to_value(a).unwrap(), &serde_json::to_value(b).unwrap())
    }
    fn unchanged(h: &Harness<'_, Rig>, before: &Mark) {
        assert_eq!(mark(h), *before, "document, incarnation, complete selection and both real history stacks remain unchanged");
    }
    fn item(h: &Harness<'_, Rig>, id: u64) -> Item {
        h.state().app.session.active().unwrap().doc.item(ItemId(id)).unwrap().clone()
    }
    fn label(h: &Harness<'_, Rig>) -> String {
        let app = &h.state().app;
        let name = crate::i18n::tr(&app.ui.language, "Line Tool");
        // The installed Tools rail names a tool by its command's label alone: its chord comes only from the
        // framework's own keymap, which the app's keymap does not feed (a framework gap, display-only chords), so
        // the shortcut the native button names it with is missing there.
        if app.framework_installed {
            return name.to_string();
        }
        crate::menus::tool_shortcut_label(app, "line").map_or_else(|| name.to_string(), |s| format!("{name} ({s})"))
    }
    /// The installed rail's button of the tool named `name`.
    fn rail_elements(h: &Harness<'_, Rig>, name: &str) -> Vec<Element> {
        h.state().controls.as_ref().into_iter().flat_map(|r| &r.elements)
            .filter(|e| e.component.contains("FlyoutButton") && e.part.is_none()
                && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name)))
            .cloned().collect()
    }
    fn line_elements(h: &Harness<'_, Rig>) -> Vec<Element> {
        // Installed, the standalone Line slot is a button of the framework's Tools rail (a FlyoutButton with one tool).
        rail_elements(h, &label(h))
    }
    fn native(h: &Harness<'_, Rig>) -> Element {
        let mut elements = line_elements(h);
        let buttons: Vec<(String, Option<String>)> = h
            .state()
            .controls
            .as_ref()
            .into_iter()
            .flat_map(|r| &r.elements)
            .filter(|e| e.component.contains("FlyoutButton") || e.component.contains("Rail"))
            .map(|e| (e.component.to_string(), e.facts.as_ref().and_then(|f| f.name.clone())))
            .collect();
        assert_eq!(elements.len(), 1, "exactly the standalone Line slot is the installed rail's Line button; recorded rail parts: {buttons:?}");
        let element = elements.remove(0);
        assert_eq!(element.facts.as_ref().unwrap().role, format!("{:?}", egui::accesskit::Role::Button));
        element
    }
    fn line_rect(h: &Harness<'_, Rig>) -> Rect {
        // The rich tooltip has the same text, but it is a Label, never this Button.
        let rect = h.get_by_role_and_label(egui::accesskit::Role::Button, &label(h)).rect();
        if h.state().app.framework_installed {
            let element = native(h);
            assert!(rect.expand(0.5).contains_rect(element.rect) && element.rect.expand(0.5).contains_rect(rect),
                "the unique named Button is the actual recorded Line cell, not tooltip text");
            let controls = h.state().controls.as_ref().unwrap();
            assert!(controls.hits.iter().any(|hit| hit.owner == element.id && hit.part.is_none() && hit.rect.contains(rect.center())),
                "the named Button has the installed Line control's physical hit owner");
        }
        rect
    }
    fn choose_line(h: &mut Harness<'_, Rig>) {
        let start = h.state().app.session.journal.len();
        let before = mark(h);
        let pos = line_rect(h).center();
        click(h, pos);
        assert_eq!(&h.state().app.session.journal[start..], &[("tool.select".to_owned(), json!({"tool":"line"}))]);
        assert_eq!(h.state().app.session.tool_id(), "line");
        assert!(h.state().app.ui.flyout.is_none());
        unchanged(h, &before);
    }
    fn tab_to_line(h: &mut Harness<'_, Rig>) {
        // Tab on the page is the app's: it hides the panels (InDesign), and the canvas holds the keyboard while
        // nothing else does. Traversal starts where a person starts it, from a click in the Control panel's X field.
        if h.state().app.framework_installed {
            let x = h.query_all_by_role_and_label(egui::accesskit::Role::TextInput, "X").next().expect("the Control panel's X field").rect().center();
            click(h, x);
        }
        let mut visited = HashSet::new();
        loop {
            let named = h.query_all_by_role_and_label(egui::accesskit::Role::Button, &label(h)).next().map(|n| n.is_focused());
            let Some(focused_line) = named else {
                let buttons: Vec<String> =
                    h.query_all_by_role(egui::accesskit::Role::Button).filter_map(|n| n.accesskit_node().label()).take(80).collect();
                panic!("no {:?} button; the buttons: {buttons:?}", label(h));
            };
            if focused_line {
                if h.state().app.framework_installed {
                    assert_eq!(h.ctx.memory(|m| m.focused()), Some(native(h).id));
                }
                return;
            }
            let focused = h.ctx.memory(|m| m.focused());
            assert!(visited.insert(focused), "natural Tab traversal reaches the real Line slot without forced focus");
            // The installed rail's buttons are one roving group, as a toolbar's are: Tab reaches the group, and the
            // arrow keys move between its buttons.
            let in_rail = h.state().app.framework_installed
                && h.state()
                    .controls
                    .as_ref()
                    .is_some_and(|r| r.elements.iter().any(|e| e.component.contains("FlyoutButton") && Some(e.id) == focused));
            key(h, if in_rail { Key::ArrowDown } else { Key::Tab });
        }
    }
    fn canvas_point(h: &Harness<'_, Rig>, x: f64, y: f64) -> egui::Pos2 {
        let st = h.state().app.session.active().unwrap();
        let layout = h.state().app.session.layout();
        let (spread_to_canvas, _) = crate::canvas::item_canvas_xf(&st.doc, &layout, ItemId(h.state().anchor)).unwrap();
        let xf = crate::canvas::Xf::new(h.state().app.canvas_rect.unwrap(), h.state().app.view().unwrap());
        let pos = xf.to_screen(spread_to_canvas * Point::new(x, y));
        assert!(h.state().app.canvas_rect.unwrap().shrink(5.0).contains(pos), "the gesture really lands inside the live Canvas");
        pos
    }
    fn begin_line(h: &mut Harness<'_, Rig>, a: [f64; 2], b: [f64; 2]) -> egui::Pos2 {
        let from = canvas_point(h, a[0], a[1]);
        let to = canvas_point(h, b[0], b[1]);
        pointer(h, from, true);
        for fraction in [0.25, 0.5, 0.75, 1.0] {
            event(h, Event::PointerMoved(from + (to - from) * fraction));
        }
        assert!(h.state().app.session.tool_busy(), "a real FrameTool line gesture, not a click-size dialog or direct create command");
        assert!(h.state().app.session.active().unwrap().interaction.is_some());
        to
    }
    fn draw_line(h: &mut Harness<'_, Rig>) -> u64 {
        let before = mark(h);
        let untouched = item(h, h.state().anchor);
        let start = h.state().app.session.journal.len();
        let to = begin_line(h, [250.0,300.0], [370.0,365.0]);
        assert_eq!(mark(h).2, before.2, "previews add no independent Undo entries");
        pointer(h, to, false);
        idle(h, 3);
        let commands = &h.state().app.session.journal[start..];
        assert_eq!(commands.len(), 1, "a genuine multi-pass Canvas drag journals only its committed line");
        assert_eq!(commands[0].0, "line.create");
        for (name, expected) in [("a", [250.0,300.0]), ("b", [370.0,365.0])] {
            for (axis, expected) in expected.into_iter().enumerate() {
                assert!((commands[0].1[name][axis].as_f64().unwrap() - expected).abs() < 1e-3);
            }
        }
        let st = h.state().app.session.active().unwrap();
        assert_eq!(st.selection.items.len(), 1);
        let id = st.selection.items[0].0;
        assert_ne!(id, h.state().anchor);
        let line = item(h, id);
        assert_eq!(line.shape, Shape::GraphicLine);
        assert!((line.bounds().width() - 120.0).abs() < 1e-3 && (line.bounds().height() - 65.0).abs() < 1e-3);
        assert_eq!(item(h, h.state().anchor), untouched);
        assert_eq!(h.state().app.session.tool_id(), "line", "creation retains the chosen tool for subsequent drawing");
        assert!(st.interaction.is_none() && !h.state().app.session.tool_busy());
        assert_eq!(mark(h).2, (before.2.0 + 1, 0));
        id
    }

    #[test]
    fn installed_line_real_canvas_creation_retains_selection_and_has_independent_undo_redo() {
        let mut h = ordinary(true);
        choose_line(&mut h);
        let before = mark(&h);
        let id = draw_line(&mut h);
        let after = mark(&h);
        let created = item(&h, id);
        idle(&mut h, 4);
        unchanged(&h, &after);
        h.state_mut().app.ui.tools_double_column = true;
        idle(&mut h, 3);
        unchanged(&h, &after);
        assert_eq!(h.state().app.session.tool_id(), "line");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 3);
        assert_eq!(mark(&h).0, before.0);
        assert_eq!(mark(&h).1, before.1);
        assert_eq!(mark(&h).2, (before.2.0, 1));
        assert!(h.state().app.session.active().unwrap().doc.item(ItemId(id)).is_none());
        assert_eq!(h.state().app.session.tool_id(), "line", "Undo belongs to creation, not palette selection");
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(&mut h, 3);
        unchanged(&h, &after);
        assert_eq!(item(&h, id), created);
    }

    #[test]
    fn installed_line_genuine_escape_and_second_gesture_release_preserve_actual_legacy_behavior() {
        // Line's current key handler returns no actions. Do not invent a cancellation policy:
        // compare the real app's installed and legacy Canvas traces, including ordinary Escape.
        let (mut saved, anchor) = fixture();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("LineEscapeParity.designcraft");
        saved.run("file.saveAs", json!({"path":path})).unwrap();
        let reopen = |installed| {
            let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.open", json!({"path":path})).unwrap();
            app.run("selection.set", json!({"ids":[anchor]})).unwrap();
            harness(app, anchor, installed, vec2(1800.0,1000.0))
        };
        let mut installed = reopen(true);
        let mut legacy = reopen(false);
        choose_line(&mut installed);
        choose_line(&mut legacy);
        let installed_id = draw_line(&mut installed);
        let legacy_id = draw_line(&mut legacy);
        assert_eq!(installed_id, legacy_id);
        let first = item(&installed, installed_id);
        let committed = mark(&installed);
        let installed_start = installed.state().app.session.journal.len();
        let legacy_start = legacy.state().app.session.journal.len();
        let installed_to = begin_line(&mut installed, [270.0,420.0], [390.0,465.0]);
        let legacy_to = begin_line(&mut legacy, [270.0,420.0], [390.0,465.0]);
        assert_ne!(mark(&installed).0, committed.0, "the second real gesture has a document preview");
        for (a, b) in [(true, false), (false, false)] {
            event(&mut installed, Event::Key { key:Key::Escape, physical_key:None, pressed:a, repeat:b, modifiers:Modifiers::NONE });
            event(&mut legacy, Event::Key { key:Key::Escape, physical_key:None, pressed:a, repeat:b, modifiers:Modifiers::NONE });
            assert!(same_document(&mark(&installed).0, &mark(&legacy).0), "the installed and legacy documents match");
            assert_eq!(mark(&installed).2, mark(&legacy).2);
            let installed_state = installed.state().app.session.active().unwrap();
            let legacy_state = legacy.state().app.session.active().unwrap();
            assert_eq!(format!("{:?}", installed_state.selection), format!("{:?}", legacy_state.selection));
            assert_eq!(installed_state.interaction.is_some(), legacy_state.interaction.is_some());
            assert_eq!(installed.state().app.session.tool_busy(), legacy.state().app.session.tool_busy());
            assert_eq!(installed.state().app.session.tool_id(), legacy.state().app.session.tool_id());
        }
        pointer(&mut installed, installed_to, false);
        pointer(&mut legacy, legacy_to, false);
        idle(&mut installed, 3);
        idle(&mut legacy, 3);
        assert!(same_document(&mark(&installed).0, &mark(&legacy).0), "ordinary release after Escape preserves actual native document behavior");
        assert_eq!(mark(&installed).2, mark(&legacy).2);
        assert!(
            same_document(&installed.state().app.session.journal[installed_start..], &legacy.state().app.session.journal[legacy_start..]),
            "the same commands ran in both windows"
        );
        assert_eq!(item(&installed, installed_id), first, "the first committed line is unchanged");
        assert!(same_document(&item(&legacy, legacy_id), &first), "the legacy window's first line is the same line");
        let after = mark(&installed).0;
        for app in [&mut installed, &mut legacy] {
            app.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(app, 2);
        }
        assert!(same_document(&mark(&installed).0, &mark(&legacy).0), "the installed and legacy documents match");
        assert_eq!(mark(&installed).2, mark(&legacy).2);
        for app in [&mut installed, &mut legacy] {
            app.state_mut().app.run("edit.redo", json!({})).unwrap();
            idle(app, 2);
        }
        assert_eq!(mark(&installed).0, after);
        assert!(same_document(&mark(&installed).0, &mark(&legacy).0), "the installed and legacy documents match");
        assert_eq!(mark(&installed).2, mark(&legacy).2);
    }

    #[test]
    fn installed_line_actual_save_open_preserves_the_created_document_and_line_artwork_data() {
        let mut h = ordinary(true);
        choose_line(&mut h);
        let id = draw_line(&mut h);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("LineWorkflow.designcraft");
        let history = mark(&h).2;
        h.state_mut().app.run("file.saveAs", json!({"path":path})).unwrap();
        let saved = mark(&h).0;
        let line = item(&h, id);
        assert_eq!(mark(&h).2, history, "saving is separate from the creation Undo assertion");
        let mut reopened = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        reopened.run("file.open", json!({"path":path})).unwrap();
        assert_eq!(reopened.session.active().unwrap().doc, saved, "a real file open reads the actual saved document");
        assert_eq!(reopened.session.active().unwrap().doc.item(ItemId(id)), Some(&line));
        assert_eq!(line.shape, Shape::GraphicLine);
    }

    #[test]
    fn installed_line_real_parent_geometry_idle_and_adaptive_columns_are_preserved() {
        for (double, size) in [(false,vec2(1800.0,1000.0)), (true,vec2(1800.0,1000.0)), (false,vec2(1800.0,600.0))] {
            let (mut app, anchor) = fixture();
            app.ui.tools_double_column = double;
            let mut h = harness(app, anchor, true, size);
            let before = mark(&h);
            let status = h.state().app.ui.status.clone();
            let element = native(&h);
            let cell = line_rect(&h);
            // The rail lays out its buttons in the framework's rail size, not the native 24 pt: the Line button is the
            // same square as every other slot's.
            let selection = crate::i18n::tr(&h.state().app.ui.language, "Selection Tool").to_string();
            let other = rail_elements(&h, &selection).pop().expect("the Selection Tool's rail button");
            assert_eq!(element.rect.size(), other.rect.size());
            assert_eq!(element.rect.width(), element.rect.height());
            assert!(cell.expand(0.5).contains_rect(element.rect) && element.rect.expand(0.5).contains_rect(cell));
            let parent = h.state().parent.expect("actual shipping left Tools Panel state");
            assert!(parent.expand(0.5).contains_rect(element.rect));
            let hits: Vec<_> = h.state().controls.as_ref().unwrap().hits.iter().filter(|hit| hit.owner == element.id && hit.part.is_none()).collect();
            assert!(!hits.is_empty() && hits.iter().all(|hit| cell.expand(0.5).contains_rect(hit.rect)));
            assert!(hits.iter().any(|hit| hit.rect.contains(cell.center())));
            idle(&mut h, 4);
            unchanged(&h, &before);
            assert_eq!(native(&h).id, element.id);
            assert_eq!(h.state().app.ui.tools_double_column, double, "crowding never rewrites the saved column preference");
            assert_eq!(h.state().app.ui.status, status);
        }
    }

    #[test]
    fn installed_line_natural_space_enter_and_reselect_keep_the_original_one_command_noop_policy() {
        for key_code in [Key::Space, Key::Enter] {
            let mut h = ordinary(true);
            tab_to_line(&mut h);
            let before = mark(&h);
            for _ in 0..2 {
                let start = h.state().app.session.journal.len();
                key(&mut h, key_code);
                assert_eq!(&h.state().app.session.journal[start..], &[("tool.select".to_owned(), json!({"tool":"line"}))]);
                assert_eq!(h.state().app.session.tool_id(), "line");
                unchanged(&h, &before);
                assert!(h.get_by_role_and_label(egui::accesskit::Role::Button, &label(&h)).is_focused());
                assert_eq!(h.ctx.memory(|m| m.focused()), Some(native(&h).id), "keyboard focus belongs to the recorded physical Line owner");
            }
            choose_line(&mut h);
        }
    }

    #[test]
    fn installed_line_held_pointer_retires_across_selection_targets_and_layout_disappearance() {
        let mut h = ordinary(true);
        let other = h.state_mut().app.run("frame.create", json!({"rect":[430,90,500,160]})).unwrap()["id"].as_u64().unwrap();
        let anchor = h.state().anchor;
        for selected in [vec![anchor], vec![anchor,other], vec![other,anchor], vec![other]] {
            idle(&mut h, 2);
            let old = native(&h);
            pointer(&mut h, old.rect.center(), true);
            h.state_mut().app.run("selection.set", json!({"ids":selected})).unwrap();
            idle(&mut h, 2);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            assert_ne!(native(&h).id, old.id);
            pointer(&mut h, old.rect.center(), false);
            idle(&mut h, 2);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
            assert_eq!(h.state().app.session.tool_id(), "selection");
        }
        let old = native(&h);
        pointer(&mut h, old.rect.center(), true);
        h.state_mut().app.ui.hidden_panels = 1;
        idle(&mut h, 2);
        assert!(line_elements(&h).is_empty(), "only this Line route is absent, not necessarily all SDK controls");
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, old.rect.center(), false);
        h.state_mut().app.ui.hidden_panels = 0;
        idle(&mut h, 3);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        choose_line(&mut h);
    }

    #[test]
    fn installed_line_held_pointer_cannot_cross_real_documents_reusing_item_ids() {
        let mut h = ordinary(true);
        let original = mark(&h);
        let old = native(&h);
        pointer(&mut h, old.rect.center(), true);
        h.state_mut().app.run("file.new", json!({})).unwrap();
        let reused = h.state_mut().app.run("frame.create", json!({"rect":[80,80,160,150]})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(reused, h.state().anchor);
        idle(&mut h, 2);
        assert_ne!(native(&h).id, old.id);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, old.rect.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        assert_eq!(h.state().app.session.tool_id(), "selection");
        h.state_mut().app.session.execute("file.close", &json!({})).unwrap();
        idle(&mut h, 3);
        unchanged(&h, &original);
        choose_line(&mut h);
    }

    #[test]
    fn installed_line_held_pointer_retires_on_real_revert_with_same_uid_and_item_ids() {
        let (mut app, anchor) = fixture();
        let directory = tempfile::tempdir().unwrap();
        app.run("file.saveAs", json!({"path":directory.path().join("LineRevert.designcraft")})).unwrap();
        app.run("transform.move", json!({"dx":7.0,"dy":9.0})).unwrap();
        let mut h = harness(app, anchor, true, vec2(1800.0,1000.0));
        let uid = h.state().app.session.active().unwrap().uid;
        let epoch = h.state().app.session.active().unwrap().identity_epoch;
        let old = native(&h);
        pointer(&mut h, old.rect.center(), true);
        h.state_mut().app.session.execute("file.revert", &json!({})).unwrap();
        h.state_mut().app.run("selection.set", json!({"ids":[anchor]})).unwrap();
        idle(&mut h, 3);
        assert_eq!(h.state().app.session.active().unwrap().uid, uid);
        assert_ne!(h.state().app.session.active().unwrap().identity_epoch, epoch);
        assert_ne!(native(&h).id, old.id);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, old.rect.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        choose_line(&mut h);
    }

    #[test]
    fn installed_line_disabled_or_sizing_parent_cannot_commit_held_input_then_recovers() {
        for sizing in [false, true] {
            let mut h = ordinary(true);
            let old = native(&h);
            pointer(&mut h, old.rect.center(), true);
            h.state_mut().enabled = sizing;
            h.state_mut().sizing = sizing;
            idle(&mut h, 2);
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            pointer(&mut h, old.rect.center(), false);
            h.state_mut().enabled = true;
            h.state_mut().sizing = false;
            idle(&mut h, 3);
            unchanged(&h, &before);
            assert_eq!(h.state().app.session.journal.len(), start);
            choose_line(&mut h);
        }
    }

    #[test]
    fn uninstalled_line_keeps_legacy_pointer_natural_keys_canvas_creation_and_undo() {
        let mut h = ordinary(false);
        assert!(!h.state().app.framework_installed);
        assert!(line_elements(&h).is_empty(), "no SDK Line IconButton; unrelated full-app SDK records are not a fallback oracle");
        // With no SDK runtime, bare Tab is the app's Show/Hide Panels shortcut, not SDK traversal.
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        assert_eq!(h.state().app.ui.hidden_panels, 0);
        assert!(h.state().parent.unwrap().contains_rect(line_rect(&h)));
        key(&mut h, Key::Tab);
        assert_eq!(h.state().app.ui.hidden_panels, 1, "retain the real native Tab policy");
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Button, &label(&h)).is_none(),
            "the exact Line Button disappears with the Tools panel; it does not change role or caption");
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start, "hiding chrome submits no engine command");
        key(&mut h, Key::Tab);
        assert_eq!(h.state().app.ui.hidden_panels, 0);
        assert!(h.state().parent.unwrap().contains_rect(line_rect(&h)), "the same uniquely named native Button returns");
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start, "restoring chrome submits no engine command");

        // The native tool shortcut still selects Line through the shipping menu/keymap path.
        let start = h.state().app.session.journal.len();
        key(&mut h, Key::Backslash);
        assert_eq!(&h.state().app.session.journal[start..], &[("tool.select".to_owned(), json!({"tool":"line"}))]);
        assert_eq!(h.state().app.session.tool_id(), "line");
        unchanged(&h, &before);
        // This custom native Sense::click response does not acquire keyboard focus on a pointer click.
        // Observe its real primary-click owner before idle erases the release-frame interaction snapshot.
        let start = h.state().app.session.journal.len();
        let pos = line_rect(&h).center();
        pointer(&mut h, pos, true);
        pointer(&mut h, pos, false);
        let owner = h.ctx.interaction_snapshot(|snapshot| snapshot.clicked)
            .expect("the ordinary pointer release has an actual clicked egui owner");
        let response = h.ctx.read_response(owner).expect("the pointer-clicked native response is live");
        assert!(response.clicked_by(PointerButton::Primary), "the observed owner received the real primary-pointer click");
        assert_eq!(&h.state().app.session.journal[start..], &[("tool.select".to_owned(), json!({"tool":"line"}))]);
        assert_eq!(h.state().app.session.tool_id(), "line");
        assert!(h.state().app.ui.flyout.is_none());
        unchanged(&h, &before);
        let native_pointer_owner = |h: &Harness<'_, Rig>, owner: egui::Id| {
            assert!(!h.state().app.framework_installed);
            assert!(line_elements(h).is_empty(), "no SDK Line owner substitutes for the native response");
            let caption = label(h);
            let node = h.get_by_role_and_label(egui::accesskit::Role::Button, &caption);
            assert!(!node.is_focused(), "ordinary pointer selection retains the native Button's non-focus policy");
            assert_ne!(h.ctx.memory(|m| m.focused()), Some(owner), "the pointer owner does not acquire egui keyboard focus");
            // The retained consumer's public u128 conversion puts the local AX NodeId in its high half.
            assert_eq!(u128::from(node.accesskit_node().id()) >> 64, u128::from(owner.value()),
                "the unique full-caption AX Button is the actual pointer-clicked physical owner, not tooltip text");
            let response = h.ctx.read_response(owner).expect("the same native pointer response remains live");
            let rect = node.rect();
            assert!(rect.expand(0.5).contains_rect(response.rect) && response.rect.expand(0.5).contains_rect(rect));
            assert!(response.enabled() && response.sense.senses_click() && h.state().parent.unwrap().contains_rect(rect));
        };
        native_pointer_owner(&h, owner);
        idle(&mut h, 2);
        native_pointer_owner(&h, owner);
        assert_eq!(h.state().app.session.tool_id(), "line");
        assert!(h.state().app.ui.flyout.is_none());
        assert_eq!(&h.state().app.session.journal[start..], &[("tool.select".to_owned(), json!({"tool":"line"}))]);
        unchanged(&h, &before);
        // Context synthesizes Space/Enter clicks only for an already-focused response. These real idle
        // keys must not reselect the unfocused native Button; Backslash above is the app's tool shortcut.
        for input in [Key::Space, Key::Enter] {
            let start = h.state().app.session.journal.len();
            key(&mut h, input);
            assert_eq!(h.state().app.session.journal.len(), start, "unfocused native Space/Enter submits no engine command");
            assert_eq!(h.state().app.session.tool_id(), "line");
            native_pointer_owner(&h, owner);
            unchanged(&h, &before);
        }
        choose_line(&mut h);
        let id = draw_line(&mut h);
        let after = mark(&h);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(&mut h, 2);
        assert_eq!(mark(&h).0, before.0);
        assert!(h.state().app.session.active().unwrap().doc.item(ItemId(id)).is_none());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(&mut h, 2);
        unchanged(&h, &after);
    }

    #[test]
    fn uninstalled_line_held_pointer_retires_before_a_replacement_target_receives_it() {
        let mut h = ordinary(false);
        let rect = line_rect(&h);
        pointer(&mut h, rect.center(), true);
        h.state_mut().app.run("selection.set", json!({"ids":[]})).unwrap();
        idle(&mut h, 2);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, rect.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        assert!(line_elements(&h).is_empty());
        choose_line(&mut h);
    }

    #[test]
    fn installed_line_no_document_retains_the_original_enabled_tool_choice_without_editing() {
        let app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        let mut h = harness(app, 0, true, vec2(1800.0,1000.0));
        assert!(h.state().app.session.active().is_none());
        let start = h.state().app.session.journal.len();
        let pos = native(&h).rect.center();
        click(&mut h, pos);
        assert_eq!(h.state().app.session.tool_id(), "line");
        assert!(h.state().app.session.active().is_none());
        assert_eq!(&h.state().app.session.journal[start..], &[("tool.select".to_owned(), json!({"tool":"line"}))]);
    }

    #[test]
    fn installed_line_held_key_release_and_repeat_cannot_follow_a_new_document_target() {
        let mut h = ordinary(true);
        tab_to_line(&mut h);
        let old = native(&h);
        let start = h.state().app.session.journal.len();
        event(&mut h, Event::Key { key:Key::Space, physical_key:None, pressed:true, repeat:false, modifiers:Modifiers::NONE });
        assert_eq!(&h.state().app.session.journal[start..], &[("tool.select".to_owned(), json!({"tool":"line"}))]);
        h.state_mut().app.run("file.new", json!({})).unwrap();
        h.state_mut().app.run("frame.create", json!({"rect":[80,80,160,150]})).unwrap();
        idle(&mut h, 3);
        assert_ne!(native(&h).id, old.id);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        event(&mut h, Event::Key { key:Key::Space, physical_key:None, pressed:true, repeat:true, modifiers:Modifiers::NONE });
        event(&mut h, Event::Key { key:Key::Space, physical_key:None, pressed:false, repeat:false, modifiers:Modifiers::NONE });
        idle(&mut h, 2);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start, "neither a held repeat nor its release activates the replacement slot");
        choose_line(&mut h);
    }

    #[test]
    fn installed_line_held_pointer_cannot_activate_at_its_old_single_column_position_after_reflow() {
        let mut h = ordinary(true);
        let old = native(&h);
        pointer(&mut h, old.rect.center(), true);
        h.state_mut().app.ui.tools_double_column = true;
        idle(&mut h, 3);
        let moved = native(&h);
        assert!(!moved.rect.contains(old.rect.center()), "the real column reflow moved the Line slot");
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        pointer(&mut h, old.rect.center(), false);
        idle(&mut h, 2);
        unchanged(&h, &before);
        let ran = &h.state().app.session.journal[start..];
        assert!(ran.is_empty(), "the release at the old place ran nothing; it ran {ran:?}, tool {:?}", h.state().app.session.tool_id());
        assert_eq!(h.state().app.session.tool_id(), "selection");
        choose_line(&mut h);
    }
}

/// The Tools panel with the framework installed, in the full application: the tool rail and the area under it run the
/// same commands and open the same dialogs as the native panel's tests in `toolbar/tests.rs` check.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_tests {
    use crate::DesignApp;
    use designcraft_tools::TOOL_GROUPS;
    use egui::accesskit::Role;
    use egui::{Event, Modifiers, PointerButton, Rect, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::{Value, json};

    struct Rig {
        app: DesignApp,
        panel: Option<Rect>,
    }

    fn step(h: &mut Harness<'_, Rig>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Rig>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }
    fn event(h: &mut Harness<'_, Rig>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn press(h: &mut Harness<'_, Rig>, pos: egui::Pos2, button: PointerButton, modifiers: Modifiers) {
        // The modifiers are held over the click, as a real Alt-click holds them, not only on its events.
        h.input_mut().events.push(Event::ModifiersChanged(modifiers));
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button, pressed, modifiers });
        }
        event(h, Event::ModifiersChanged(Modifiers::NONE));
    }

    fn harness() -> Harness<'static, Rig> {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let mut app = DesignApp::new_for_panel_tests(session, crate::Services::default());
        app.run("window.workspace", json!({"name": "Advanced"})).unwrap();
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        assert!(app.framework_installed);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(1800.0, 1000.0)).with_step_dt(0.01).build_ui_state(
            |ui, state: &mut Rig| {
                let ctx = ui.ctx().clone();
                state.app.logic(&ctx);
                let parent = ui.scope_id().with("tools");
                state.app.ui(ui);
                state.panel = egui::containers::panel::PanelState::load(ui.ctx(), parent).map(|panel| panel.outer_rect);
            },
            Rig { app, panel: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 6);
        h
    }

    /// The control named `label` with one of `roles` in the Tools panel.
    fn control(h: &Harness<'_, Rig>, label: &str, roles: &[Role]) -> Rect {
        let panel = h.state().panel.expect("the shipping left Tools panel persisted its rect");
        h.query_all_by_label(label)
            .filter(|n| roles.contains(&n.accesskit_node().role()))
            .map(|n| n.rect())
            .find(|r| panel.contains(r.center()))
            .unwrap_or_else(|| panic!("no {label:?} in the Tools panel"))
    }
    fn tr(h: &Harness<'_, Rig>, text: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, text).to_owned()
    }
    fn journal_since(h: &Harness<'_, Rig>, start: usize) -> Vec<(String, Value)> {
        h.state().app.session.journal[start..].to_vec()
    }
    const BUTTONS: &[Role] = &[Role::Button];

    #[test]
    fn installed_tools_rail_selects_cycles_with_alt_and_picks_from_a_flyout() {
        let mut h = harness();
        let group = TOOL_GROUPS.iter().find(|g| g.len() > 2).expect("a group of three tools");
        let first = control(&h, &tr(&h, group[0].label), BUTTONS);
        let start = h.state().app.session.journal.len();
        press(&mut h, first.center(), PointerButton::Primary, Modifiers::NONE);
        idle(&mut h, 3);
        assert_eq!(journal_since(&h, start), [("tool.select".to_owned(), json!({"tool": group[0].id}))]);
        // Alt-click cycles the group, as the letter key does.
        let start = h.state().app.session.journal.len();
        press(&mut h, first.center(), PointerButton::Primary, Modifiers::ALT);
        idle(&mut h, 3);
        assert_eq!(journal_since(&h, start), [("tool.select".to_owned(), json!({"tool": group[1].id}))]);
        // A right click opens the group's flyout; a row picks its tool.
        let slot = control(&h, &tr(&h, group[1].label), BUTTONS);
        press(&mut h, slot.center(), PointerButton::Secondary, Modifiers::NONE);
        idle(&mut h, 3);
        let row = h
            .query_all_by_label(&tr(&h, group[2].label))
            .find(|n| matches!(n.accesskit_node().role(), Role::MenuItem | Role::MenuItemRadio))
            .expect("the flyout lists the group's third tool")
            .rect();
        let start = h.state().app.session.journal.len();
        press(&mut h, row.center(), PointerButton::Primary, Modifiers::NONE);
        idle(&mut h, 3);
        assert_eq!(journal_since(&h, start), [("tool.select".to_owned(), json!({"tool": group[2].id}))]);
        assert_eq!(h.state().app.session.tool_id(), group[2].id);
    }

    #[test]
    fn installed_double_clicking_the_polygon_tool_opens_polygon_settings() {
        let mut h = harness();
        h.state_mut().app.select_tool("polygon");
        idle(&mut h, 3);
        let polygon = control(&h, &tr(&h, "Polygon Tool"), BUTTONS);
        for _ in 0..2 {
            event(&mut h, Event::PointerButton { pos: polygon.center(), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            event(&mut h, Event::PointerButton { pos: polygon.center(), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        }
        idle(&mut h, 3);
        assert_eq!(h.state().app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("polygonSettings"));
    }

    #[test]
    fn installed_fill_and_stroke_pair_swaps_resets_brings_a_box_forward_and_opens_the_picker() {
        let mut h = harness();
        let id = h.state_mut().app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        h.state_mut().app.run("selection.set", json!({"ids": [id]})).unwrap();
        idle(&mut h, 3);
        let paint = |h: &Harness<'_, Rig>| {
            let item = h.state().app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().clone();
            (item.fill.swatch, item.stroke.swatch)
        };
        let before = paint(&h);
        let swap = control(&h, &tr(&h, "Swap Fill and Stroke (Shift+X)"), BUTTONS);
        press(&mut h, swap.center(), PointerButton::Primary, Modifiers::NONE);
        idle(&mut h, 2);
        assert_eq!(paint(&h), (before.1.clone(), before.0.clone()), "the swap button swaps the frame's fill and stroke");
        let reset = control(&h, &tr(&h, "Default Fill and Stroke (D)"), BUTTONS);
        press(&mut h, reset.center(), PointerButton::Primary, Modifiers::NONE);
        idle(&mut h, 2);
        assert_eq!(paint(&h), ("[None]".to_owned(), "[Black]".to_owned()), "the default button gives a frame no fill and a black stroke");
        assert!(!h.state().app.ui.stroke_front);
        // The pair's wells are found by their names, whatever role the framework gives them.
        let well = |h: &Harness<'_, Rig>, label: &str| {
            let panel = h.state().panel.expect("the shipping left Tools panel persisted its rect");
            h.query_all_by_label(label).map(|n| n.rect()).find(|r| panel.contains(r.center())).unwrap_or_else(|| {
                let names: Vec<String> = h
                    .query_all_by_role(Role::Button)
                    .filter(|n| panel.contains(n.rect().center()))
                    .filter_map(|n| n.accesskit_node().label())
                    .collect();
                panic!("no {label:?} in the Tools panel; its buttons: {names:?}")
            })
        };
        let stroke = well(&h, &tr(&h, "Stroke (X)"));
        press(&mut h, stroke.center(), PointerButton::Primary, Modifiers::NONE);
        idle(&mut h, 2);
        assert!(h.state().app.ui.stroke_front, "a click on the stroke box brings it to the front");
        let fill = well(&h, &tr(&h, "Fill (X)"));
        event(&mut h, Event::PointerMoved(fill.center()));
        for pressed in [true, false, true, false] {
            event(&mut h, Event::PointerButton { pos: fill.center(), button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        idle(&mut h, 3);
        let d = h.state().app.ui.dialog.clone().expect("a double click opens the Color Picker");
        assert_eq!((d.id.as_str(), d.fields["target"].as_str()), ("colorPicker", Some("fill")));
    }

    #[test]
    fn installed_formatting_and_screen_mode_buttons_run_their_commands() {
        let mut h = harness();
        // Both are the app's own commands, which the engine's journal does not record: their outcome is the app state.
        assert!(!h.state().app.ui.formatting_affects_text);
        let text = control(&h, &tr(&h, "Formatting affects text (J)"), BUTTONS);
        press(&mut h, text.center(), PointerButton::Primary, Modifiers::NONE);
        idle(&mut h, 2);
        assert!(h.state().app.ui.formatting_affects_text, "Formatting affects text is on after a click at {:?} on {text:?}", text.center());
        let mode = control(&h, &tr(&h, "Normal (W)"), BUTTONS);
        press(&mut h, mode.center(), PointerButton::Primary, Modifiers::NONE);
        idle(&mut h, 2);
        assert_eq!(h.state().app.ui.screen_mode, crate::ScreenMode::Preview);
    }
}
