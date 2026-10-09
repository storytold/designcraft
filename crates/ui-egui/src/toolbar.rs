//! The Tools panel (left): single or double column, tool groups with flyouts, fill/stroke proxy,
//! screen mode buttons.

use designcraft_tools::TOOL_GROUPS;
use egui::{Color32, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{DesignApp, icons};

const BTN: f32 = 24.0;

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let cols = if app.ui.tools_double_column { 2 } else { 1 };
    let width = 8.0 + BTN * cols as f32 + 4.0 * (cols as f32 - 1.0) + 8.0;
    let r = egui::Panel::left("tools")
        .exact_size(width)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.divider)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add_space(3.0);
            // Collapse chevrons.
            ui.horizontal(|ui| {
                ui.add_space(width / 2.0 - 8.0);
                if icons::button(
                    ui,
                    if cols == 1 { "double-chevron-right" } else { "double-chevron-left" },
                    16.0,
                    false,
                    crate::i18n::tr(&app.ui.language, "Toggle double column"),
                )
                .clicked()
                {
                    app.ui.tools_double_column = !app.ui.tools_double_column;
                }
            });
            ui.add_space(2.0);
            let current = app.session.tool_id();
            let mut chosen: Option<&'static str> = None;
            let mut row_items = Vec::new();
            for (gi, g) in TOOL_GROUPS.iter().enumerate() {
                if g.is_empty() {
                    row_items.push(None);
                    continue;
                }
                row_items.push(Some(gi));
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
                                let Some(Some(gi)) = row_items.get(i).copied() else { break };
                                let g = TOOL_GROUPS[gi];
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
            fill_stroke_proxy(app, ui, width);
        });
    if std::env::var_os("DESIGNCRAFT_DEBUG_LAYOUT").is_some() {
        eprintln!("tools panel rect {:?} (wanted width {width}); remaining {:?}", r.response.rect, ui.available_rect_before_wrap());
    }
}

/// The area under the tools (InDesign 2026 §3 group 5): mini Default Fill/Stroke and Swap, the
/// overlapping Fill/Stroke proxy, Formatting affects container / text, the Apply None well, a
/// utility button and the Screen Mode well.
fn fill_stroke_proxy(app: &mut DesignApp, ui: &mut egui::Ui, width: f32) {
    let t = Tokens::get(ui.ctx());
    let target = crate::panels::color_target(app);
    let doc = app.session.active().map(|d| d.doc.clone());
    let stroke_front_id = egui::Id::new("proxy_stroke_front");
    let stroke_front: bool = ui.data(|d| d.get_temp(stroke_front_id)).unwrap_or(false);
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
        // Text defaults to a black fill and no stroke; objects to no fill and a black stroke.
        let (fill, stroke) = if crate::panels::colors_affect_text(app) { ("[Black]", "[None]") } else { ("[None]", "[Black]") };
        let _ = crate::panels::apply_swatch(app, false, fill, None);
        let _ = crate::panels::apply_swatch(app, true, stroke, None);
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
        let _ = crate::panels::apply_swatch(app, false, &stroke_sw, None);
        let _ = crate::panels::apply_swatch(app, true, &fill_sw, None);
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
    let fresp = ui.interact(fill_r, ui.id().with("proxy_fill"), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Fill (X)"));
    });
    let sresp = ui.interact(stroke_r.translate(vec2(0.0, 0.0)), ui.id().with("proxy_stroke"), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Stroke (X)"));
    });
    if fresp.clicked() || sresp.clicked() {
        let front =
            if stroke_front { !fresp.clicked() } else { sresp.clicked() && !fill_r.contains(sresp.interact_pointer_pos().unwrap_or_default()) };
        ui.data_mut(|d| d.insert_temp(stroke_front_id, front));
    }
    if fresp.double_clicked() || sresp.double_clicked() {
        let _ = app.run("window.panel", json!({"panel": "swatches"}));
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
    // Apply None well (flyout: Apply Color / Gradient / None).
    let (row, _) = ui.allocate_exact_size(vec2(width, 21.0), Sense::hover());
    let well = egui::Rect::from_center_size(egui::pos2(row.min.x + width / 2.0, row.center().y), vec2(28.0, 21.0));
    let resp = ui.interact(well, ui.id().with("proxy_apply"), Sense::click()).on_hover_ui(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Apply None (/)"));
    });
    ui.painter().rect(well, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Outside);
    let chip = egui::Rect::from_center_size(well.center(), vec2(14.0, 14.0));
    crate::widgets::paint_chip(ui.painter(), chip, None, None);
    flyout_triangle(ui.painter(), well, t.icon);
    if resp.clicked() {
        let _ = crate::panels::apply_swatch(app, stroke_front, "[None]", None);
    }
    egui::Popup::context_menu(&resp).show(|ui| {
        for (label, sw) in [("Apply Color", "[Black]"), ("Apply None", "[None]")] {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = crate::panels::apply_swatch(app, stroke_front, sw, None);
                ui.close();
            }
        }
    });
    ui.add_space(6.0);
    separator(ui, &t);
    // Utility (frame options).
    ui.vertical_centered(|ui| {
        if tool_button(ui, "frame-options", false, crate::i18n::tr(&app.ui.language, "Text Frame Options")).clicked() {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
        }
    });
    separator(ui, &t);
    // Screen Mode well with flyout.
    let (row, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    let well = egui::Rect::from_center_size(egui::pos2(row.min.x + width / 2.0, row.center().y), vec2(28.0, 21.0));
    let resp = ui.interact(well, ui.id().with("screen_mode"), Sense::click());
    ui.painter().rect(well, 2.0, t.well, Stroke::new(1.0, t.well_rim), StrokeKind::Outside);
    let preview = app.ui.screen_mode == crate::ScreenMode::Preview;
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
        for (label, mode) in [("Normal", "normal"), ("Preview", "preview"), ("Bleed", "bleed"), ("Slug", "slug"), ("Presentation", "presentation")] {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run("view.screenMode", json!({"mode": mode}));
                ui.close();
            }
        }
    });
}

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
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    })
}
