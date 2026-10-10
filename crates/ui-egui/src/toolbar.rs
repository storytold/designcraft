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
fn fill_stroke_proxy(app: &mut DesignApp, ui: &mut egui::Ui, width: f32) {
    let t = Tokens::get(ui.ctx());
    let info = crate::panels::sel_info(app);
    let doc = app.session.active().map(|d| d.doc.clone());
    let stroke_front_id = egui::Id::new("proxy_stroke_front");
    let text_id = egui::Id::new("proxy_text");
    let stroke_front: bool = ui.data(|d| d.get_temp(stroke_front_id)).unwrap_or(false);
    let text_mode: bool = ui.data(|d| d.get_temp(text_id)).unwrap_or(false);
    let (fill_sw, stroke_sw) = match &info {
        Some(i) => (i.fill.clone(), i.stroke.clone()),
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
        let _ = app.run("object.fill", json!({"swatch": "[None]"}));
        let _ = app.run("object.stroke", json!({"swatch": "[Black]"}));
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
        let _ = app.run("object.fill", json!({"swatch": stroke_sw}));
        let _ = app.run("object.stroke", json!({"swatch": fill_sw}));
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
            ui.data_mut(|d| d.insert_temp(text_id, k == 1));
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
    let target = if stroke_front { "object.stroke" } else { "object.fill" };
    if resp.clicked() {
        let _ = app.run(target, json!({"swatch": "[None]"}));
    }
    egui::Popup::context_menu(&resp).show(|ui| {
        for (label, sw) in [("Apply Color", "[Black]"), ("Apply None", "[None]")] {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                let _ = app.run(target, json!({"swatch": sw}));
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
    let enabled = ui.is_enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    })
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
    fn a_tall_window_keeps_one_column_and_does_not_scroll() {
        let mut h = test_window::open(app(false), vec2(1400.0, 900.0));
        let panel = test_window::panel_rect(&h, "tools");
        assert_eq!(panel.width(), column_width(1.0));
        let before = (rect(&h, LAST_TOOL), rect(&h, FILL));
        assert!(panel.contains_rect(before.0) && panel.contains_rect(before.1), "{before:?} outside {panel:?}");
        wheel(&mut h, panel.center(), vec2(0.0, -2000.0));
        assert_eq!((rect(&h, LAST_TOOL), rect(&h, FILL)), before);
    }
}
