//! The Tools panel (left): single or double column, tool groups with flyouts, fill/stroke proxy,
//! screen mode buttons.

use designcraft_tools::{SPLIT_IN_TWO_COLUMNS, TOOL_GROUPS};
use egui::{Color32, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{DesignApp, icons};

const BTN: f32 = 24.0;

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let frame = egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.divider));
    // A single column that doesn't fit the window's height becomes two, without changing the
    // preference; whatever two columns still can't fit scrolls.
    let single_h_id = egui::Id::new("tools_single_column_height");
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
                                let tip = match shown.shortcut {
                                    Some(s) => format!("{} ({s})", shown.label),
                                    None => shown.label.to_string(),
                                };
                                let resp = tool_button(ui, shown.icon, active, &tip);
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
                                if resp.clicked() {
                                    chosen = Some(shown.id);
                                }
                                // Double-click a polygon tool: Polygon Settings.
                                if resp.double_clicked() && matches!(shown.id, "polygon" | "polygonFrame") {
                                    let cur = app.session.execute("tool.polygonSettings", &json!({})).unwrap_or_default();
                                    app.ui.dialog = Some(crate::dialogs::Dialog::new("polygonSettings", cur));
                                }
                                // Double-click a transform tool: its Object › Transform dialog.
                                if resp.double_clicked()
                                    && let Some(cmd) = transform_dialog_of(shown.id)
                                {
                                    crate::menus::activate(app, cmd, &Value::Null);
                                }
                                if g.len() > 1
                                    && (resp.secondary_clicked()
                                        || resp.long_touched()
                                        || (resp.is_pointer_button_down_on()
                                            && ui.input(|i| i.pointer.press_start_time().is_some_and(|s| i.time - s > 0.35))))
                                {
                                    app.ui.flyout = Some(gi);
                                }
                                if app.ui.flyout == Some(gi) {
                                    let pos = resp.rect.right_top() + vec2(4.0, 0.0);
                                    egui::Area::new(egui::Id::new(("flyout", gi))).order(egui::Order::Foreground).fixed_pos(pos).show(
                                        ui.ctx(),
                                        |ui| {
                                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                                for tl in g {
                                                    ui.horizontal(|ui| {
                                                        let (r, row) = ui.allocate_exact_size(vec2(210.0, 24.0), Sense::click());
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
                                                            tl.label,
                                                            egui::FontId::proportional(12.5),
                                                            t.text,
                                                        );
                                                        if let Some(sc) = tl.shortcut {
                                                            ui.painter().text(
                                                                r.right_center() - vec2(6.0, 0.0),
                                                                egui::Align2::RIGHT_CENTER,
                                                                sc,
                                                                egui::FontId::proportional(11.5),
                                                                t.text_dim,
                                                            );
                                                        }
                                                        if row.clicked() {
                                                            chosen = Some(tl.id);
                                                        }
                                                    });
                                                }
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
            if app.ui.flyout.is_some() && ui.input(|i| i.pointer.any_click()) && chosen.is_none() && !ui.ctx().is_pointer_over_egui() {
                app.ui.flyout = None;
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
        None => ("[None]".to_string(), "[Black]".to_string()),
    };
    let x0 = ui.max_rect().min.x + (width - 30.0) / 2.0;
    // Row 1: default colours + swap.
    let (row, _) = ui.allocate_exact_size(vec2(width, 11.0), Sense::hover());
    let def = egui::Rect::from_min_size(egui::pos2(x0, row.min.y), vec2(10.0, 10.0));
    icons::paint(ui.painter(), def, "default-colors", t.icon);
    if ui
        .interact(def, ui.id().with("proxy_default"), Sense::click())
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
        .interact(swap, ui.id().with("proxy_swap_btn"), Sense::click())
        .on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Swap Fill and Stroke (Shift+X)"));
        })
        .clicked()
    {
        let _ = app.run("color.swapFillStroke", json!({}));
    }
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
    let fresp = ui.interact(fill_r, ui.id().with("proxy_fill"), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, fill_tip);
    });
    fresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, fill_tip));
    let stroke_tip = crate::i18n::tr(&app.ui.language, "Stroke (X)");
    let sresp = ui.interact(stroke_r.translate(vec2(0.0, 0.0)), ui.id().with("proxy_stroke"), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, stroke_tip);
    });
    sresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, stroke_tip));
    if fresp.clicked() || sresp.clicked() {
        let front =
            if stroke_front { !fresp.clicked() } else { sresp.clicked() && !fill_r.contains(sresp.interact_pointer_pos().unwrap_or_default()) };
        app.ui.stroke_front = front;
    }
    // A double-click opens the Color Picker for that box (as InDesign's), with the selection's
    // colour: the selected text's when text is selected.
    if fresp.double_clicked() || sresp.double_clicked() {
        let on_fill = fresp.double_clicked() && (!stroke_front || !sresp.double_clicked());
        let stroke = !on_fill;
        let text = crate::panels::text_attrs(app).filter(|_| app.session.active().is_some_and(|d| d.selection.text.is_some()));
        let current = match text {
            Some(a) => a["chars"][if stroke { "stroke" } else { "fill" }].as_str().unwrap_or("[None]").to_string(),
            None if stroke => stroke_sw.clone(),
            None => fill_sw.clone(),
        };
        let rgb = doc.as_ref().and_then(|d| d.resolve_color(&current, 1.0)).map_or([0.0; 3], |c| c.to_rgb());
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let hex = format!("#{:02x}{:02x}{:02x}", byte(rgb[0]), byte(rgb[1]), byte(rgb[2]));
        app.ui.dialog = Some(crate::dialogs::Dialog::new(
            "colorPicker",
            json!({"target": if stroke { "stroke" } else { "fill" }, "hex": hex, "text": crate::panels::colors_affect_text(app)}),
        ));
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
            .interact(r, ui.id().with(("proxy_fmt", k)), Sense::click())
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
            let resp = ui.interact(r, ui.id().with(("proxy_apply", k)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::i18n::tr(&app.ui.language, tip)));
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
        let resp = ui.interact(well, ui.id().with("proxy_apply"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, crate::i18n::tr(&app.ui.language, tip)));
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
                let sc = crate::menus::ui_label(id).and_then(|c| c.1).unwrap_or("");
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
        let resp = ui.interact(normal, ui.id().with("screen_normal"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &normal_tip));
        if resp.on_hover_text(&normal_tip).clicked() {
            let _ = app.run("view.screenMode", json!({"mode": "normal"}));
        }
        let resp = ui.interact(other, ui.id().with("screen_mode"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &other_tip));
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
    let resp = ui.interact(well, ui.id().with("screen_mode"), Sense::click());
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
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    })
}

/// Default Fill and Stroke (D): no fill, a black stroke.
pub(crate) fn default_fill_stroke(app: &mut DesignApp) -> Result<Value, String> {
    if crate::panels::colors_affect_text(app) {
        app.run("type.char", json!({"fill": "[Black]", "fillTint": 1.0, "stroke": "[None]", "strokeTint": 1.0}))
    } else {
        app.run("object.defaultFillStroke", json!({}))
    }
}

/// Swap Fill and Stroke (Shift+X): each selected object's fill and stroke trade places.
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
    let fill = crate::panels::color_target(app).map(|t| t.0);
    let stops = json!([{"location": 0, "color": "#ffffff"}, {"location": 100, "color": "#000000"}]);
    let r = app.run("object.gradient", json!({"stops": stops}))?;
    let sw = r["swatch"].as_str().unwrap_or_default().to_string();
    app.ui.last_gradient = Some(sw.clone());
    if stroke {
        // The gradient was made on the fill: it goes to the stroke, the fill gets its swatch back.
        crate::panels::apply_swatch(app, true, &sw, None)?;
        if let Some(f) = fill {
            app.run("object.fill", json!({"swatch": f}))?;
        }
    }
    Ok(r)
}

/// Apply None (/): to the fill or the stroke, whichever is in front of the proxy.
pub(crate) fn apply_none(app: &mut DesignApp) -> Result<Value, String> {
    crate::panels::apply_swatch(app, app.ui.stroke_front, "[None]", None)
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2};
    use egui_kittest::kittest::Queryable;

    use crate::test_window::{self, wheel};

    /// The last tool group's button and the Fill proxy, near the bottom of the panel.
    const LAST_TOOL: &str = "Zoom Tool (Z)";
    const FILL: &str = "Fill (X)";
    /// Status bar height: the Tools panel ends above it.
    const STATUS_H: f32 = 17.0;

    fn app(double: bool) -> crate::DesignApp {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.tools_double_column = double;
        app
    }

    fn column_width(cols: f32) -> f32 {
        8.0 + super::BTN * cols + 4.0 * (cols - 1.0) + 8.0
    }

    fn rect(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> egui::Rect {
        h.get_by_label(label).rect()
    }

    #[test]
    fn a_short_window_reaches_every_tool() {
        for double in [false, true] {
            let (w, ht) = (1000.0, 500.0);
            let mut h = test_window::open(app(double), vec2(w, ht));
            let panel = test_window::panel_rect(&h, "tools");
            wheel(&mut h, panel.center(), vec2(0.0, -2000.0));
            let visible = egui::Rect::from_min_max(pos2(0.0, panel.min.y), pos2(w, ht - STATUS_H));
            for label in [LAST_TOOL, FILL] {
                let r = rect(&h, label);
                assert!(visible.contains_rect(r), "double column {double}: {label} at {r:?} is outside {visible:?}");
            }
            assert_eq!(h.state().app.ui.tools_double_column, double, "the preference is the user's");
        }
    }

    #[test]
    fn tools_take_two_columns_while_one_does_not_fit() {
        let (w, ht) = (1086.0, 612.0);
        let mut h = test_window::open(app(false), vec2(w, ht));
        assert_eq!(test_window::panel_rect(&h, "tools").width(), column_width(2.0));
        let visible = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(w, ht - STATUS_H));
        for label in [LAST_TOOL, FILL] {
            let r = rect(&h, label);
            assert!(visible.contains_rect(r), "{label} at {r:?} is outside {visible:?} without scrolling");
        }
        assert!(!h.state().app.ui.tools_double_column);
        // A taller window brings the single column back.
        h.set_size(vec2(1400.0, 900.0));
        h.run_steps(4);
        assert_eq!(test_window::panel_rect(&h, "tools").width(), column_width(1.0));
        assert!(!h.state().app.ui.tools_double_column);
    }

    #[test]
    fn double_clicking_the_fill_box_opens_the_color_picker_with_the_selections_colour() {
        let mut app = app(false);
        app.run("file.new", serde_json::json!({})).unwrap();
        let id = app.run("frame.create", serde_json::json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].clone();
        app.run("object.color", serde_json::json!({"color": "#456daf", "ids": [id]})).unwrap();
        app.run("selection.set", serde_json::json!({"ids": [id]})).unwrap();
        // Frames 1 ms apart, so two clicks make a double-click.
        let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(0.001).build_ui_state(
            |ui, app: &mut crate::DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(6);
        let at = h.get_by_label(FILL).rect().center();
        h.hover_at(at);
        h.step();
        for pressed in [true, false, true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        }
        h.run_steps(3);
        let d = h.state().ui.dialog.clone().expect("the Color Picker opened");
        assert_eq!(d.id, "colorPicker");
        assert_eq!((d.fields["target"].as_str(), d.fields["hex"].as_str()), (Some("fill"), Some("#456daf")), "{:?}", d.fields);
    }

    #[test]
    fn a_tall_window_keeps_one_column_and_does_not_scroll() {
        let mut h = test_window::open(app(false), vec2(1400.0, 900.0));
        let panel = test_window::panel_rect(&h, "tools");
        assert_eq!(panel.width(), column_width(1.0));
        let before = (rect(&h, LAST_TOOL), rect(&h, FILL));
        assert!(panel.contains_rect(before.0) && panel.contains_rect(before.1), "{before:?} outside {panel:?}");
        wheel(&mut h, panel.center(), vec2(0.0, -2000.0));
        assert_eq!((rect(&h, LAST_TOOL), rect(&h, FILL)), before);
    }
    #[test]
    fn the_tools_panel_lays_out_like_indesigns() {
        // Chris Nielsen's "The Adobe InDesign Toolbox" (InDesign 2020): Gradient Swatch and Gradient
        // Feather are tools of their own; in two columns Content Collector and Placer, Apply
        // Color / Gradient / None and Normal / Preview each get a button.
        for double in [false, true] {
            let h = test_window::open(app(double), vec2(1400.0, 900.0));
            let _ = rect(&h, "Gradient Swatch Tool (G)");
            let _ = rect(&h, "Gradient Feather Tool (Shift+G)");
            let _ = rect(&h, "View Options");
            let split = ["Content Placer Tool (B)", "Apply Color (,)", "Apply Gradient (.)", "Normal"];
            for label in split {
                assert_eq!(h.query_by_label(label).is_some(), double, "double column {double}: {label}");
            }
            let _ = rect(&h, "Content Collector Tool (B)");
            // One column: the Apply well names what is in front (no fill: Apply None).
            let _ = rect(&h, "Apply None (/)");
            if double {
                let (a, b) = (rect(&h, "Content Collector Tool (B)"), rect(&h, "Content Placer Tool (B)"));
                assert!((a.center().y - b.center().y).abs() < 1.0 && a.center().x < b.center().x, "side by side: {a:?} {b:?}");
                let (n, p) = (rect(&h, "Normal"), rect(&h, "Preview (W)"));
                assert!((n.center().y - p.center().y).abs() < 1.0, "Normal and Preview side by side");
            }
        }
    }

    #[test]
    fn apply_color_and_gradient_reuse_the_last_ones() {
        use serde_json::json;
        let mut app = app(true);
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200], "content": "unassigned"})).unwrap()["id"].clone();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let fill = |app: &crate::DesignApp| {
            let st = app.session.active().unwrap();
            let it = st.doc.item(st.selection.items[0]).unwrap();
            (it.fill.swatch.clone(), it.stroke.swatch.clone())
        };
        // Apply Color: [Black] at first, then the colour last applied.
        app.run("color.applyColor", json!({})).unwrap();
        assert_eq!(fill(&app).0, "[Black]");
        crate::panels::apply_swatch(&mut app, false, "C=100 M=0 Y=0 K=0", None).unwrap();
        app.run("color.applyNone", json!({})).unwrap();
        assert_eq!(fill(&app).0, "[None]");
        app.run("color.applyColor", json!({})).unwrap();
        assert_eq!(fill(&app).0, "C=100 M=0 Y=0 K=0");
        // Apply Gradient: white to black when there's none yet, then that one again.
        app.run("color.applyGradient", json!({})).unwrap();
        let g = fill(&app).0;
        let doc = &app.session.active().unwrap().doc;
        let grad = designcraft_color::swatch::resolve_gradient(&doc.swatches, &g).expect("a gradient");
        assert_eq!((grad.stops[0].color.to_rgb(), grad.stops[1].color.to_rgb()), ([1.0; 3], [0.0; 3]));
        app.run("color.applyColor", json!({})).unwrap();
        // With the stroke in front, it goes to the stroke; the fill keeps its colour.
        app.ui.stroke_front = true;
        app.run("color.applyGradient", json!({})).unwrap();
        assert_eq!(fill(&app), ("C=100 M=0 Y=0 K=0".to_string(), g));
        // The shortcuts: , and .
        for (id, sc) in [("color.applyColor", ","), ("color.applyGradient", ".")] {
            assert_eq!(crate::menus::ui_label(id).and_then(|c| c.1), Some(sc));
        }
    }

    /// The Fill/Stroke proxy's keys run on the selection and stay out of text being typed.
    #[test]
    fn fill_stroke_proxy_keys() {
        use egui::{Event, Key, Modifiers};
        use serde_json::json;
        fn press(app: &mut crate::DesignApp, ctx: &egui::Context, key: Key, modifiers: Modifiers) {
            let events = [true, false].map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).to_vec();
            ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| crate::menus::shortcuts(app, ui.ctx())).textures_delta.clear();
        }
        fn colors(app: &crate::DesignApp) -> (String, f32, String, f32) {
            let st = app.session.active().unwrap();
            let it = st.doc.item(st.selection.items[0]).unwrap();
            (it.fill.swatch.clone(), it.fill.tint, it.stroke.swatch.clone(), it.stroke.tint)
        }
        let mut app = app(false);
        let ctx = egui::Context::default();
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        app.run("selection.set", json!({"ids": [f["id"]]})).unwrap();
        app.run("object.fill", json!({"swatch": "[Paper]", "tint": 0.5})).unwrap();
        app.run("object.stroke", json!({"swatch": "[Black]", "tint": 1.0})).unwrap();

        press(&mut app, &ctx, Key::X, Modifiers::SHIFT);
        assert_eq!(colors(&app), ("[Black]".into(), 1.0, "[Paper]".into(), 0.5), "Shift+X swaps, tints included");
        press(&mut app, &ctx, Key::D, Modifiers::NONE);
        assert_eq!(colors(&app), ("[None]".into(), 1.0, "[Black]".into(), 1.0), "D: no fill, black stroke");
        press(&mut app, &ctx, Key::X, Modifiers::NONE);
        assert!(app.ui.stroke_front, "X brings the stroke to the front");
        press(&mut app, &ctx, Key::Slash, Modifiers::NONE);
        assert_eq!(colors(&app).2, "[None]", "/ applies None to the stroke in front");

        // Typing in a story: the keys are text, not shortcuts.
        app.session.execute("text.select", &json!({"story": f["story"], "anchor": 5})).unwrap();
        app.select_tool("type");
        assert!(app.session.wants_text());
        press(&mut app, &ctx, Key::X, Modifiers::NONE);
        assert!(app.ui.stroke_front, "X while typing leaves the proxy alone");
    }
}

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
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(0.001).build_ui_state(
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
