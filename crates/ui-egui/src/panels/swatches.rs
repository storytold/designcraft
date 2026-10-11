//! Swatches and Color panels.

use designcraft_color::{ColorType, SwatchValue};
use egui::{Sense, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::{DesignApp, icons};

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    let text = st.selection.text.is_some();
    let fill_cur = super::sel_info(app).map(|i| i.fill);
    ui.horizontal(|ui| {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, if text { "Applies to text" } else { "Applies to fill" }))
                .size(11.0)
                .color(t.text_dim),
        );
    });
    // Unnamed colours (mixed in the Color panel) aren't listed until Add to Swatches; swatches in
    // colour groups are listed under their folder.
    let grouped: Vec<&str> = doc.color_groups.iter().flat_map(|g| g.swatches.iter().map(String::as_str)).collect();
    for sw in doc.swatches.iter().filter(|w| !w.hidden && !grouped.contains(&w.name.as_str())) {
        swatch_row(app, ui, &doc, sw, 0.0, fill_cur.as_deref(), text);
    }
    for g in &doc.color_groups {
        let open_id = egui::Id::new(("color_group_open", &g.name));
        let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(true);
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &g.name));
        if resp.hovered() {
            ui.painter().rect_filled(row, 0.0, t.hover);
        }
        icons::paint(
            ui.painter(),
            egui::Rect::from_min_size(row.min + vec2(2.0, 3.0), vec2(16.0, 16.0)),
            if open { "chevron-down" } else { "chevron-right" },
            t.text_dim,
        );
        // A folder.
        let f = egui::Rect::from_min_size(row.min + vec2(20.0, 5.0), vec2(16.0, 12.0));
        ui.painter().rect_stroke(f, 1.5, egui::Stroke::new(1.2, t.text_dim), egui::StrokeKind::Inside);
        ui.painter().line_segment([f.left_top() + vec2(1.0, 3.0), f.right_top() + vec2(-1.0, 3.0)], egui::Stroke::new(1.0, t.text_dim));
        ui.painter().text(row.min + vec2(42.0, 11.0), egui::Align2::LEFT_CENTER, &g.name, egui::FontId::proportional(12.5), t.text);
        // A double click's first click folded the group: it stays as it was.
        if resp.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(open_id, open));
        }
        if resp.double_clicked() {
            let _ = app.run("app.colorGroupOptionsDialog", json!({"name": g.name}));
        }
        resp.context_menu(|ui| {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Color Group Options…"))).clicked() {
                let _ = app.run("app.colorGroupOptionsDialog", json!({"name": g.name}));
                ui.close();
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Ungroup Color Group"))).clicked() {
                let _ = app.run("swatch.ungroupColorGroup", json!({"name": g.name}));
                ui.close();
            }
        });
        if open {
            for n in &g.swatches {
                if let Some(sw) = doc.swatches.iter().find(|w| w.name == *n) {
                    swatch_row(app, ui, &doc, sw, 16.0, fill_cur.as_deref(), text);
                }
            }
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Right-click: stroke, groups")).size(10.5).color(t.text_disabled),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "plus", 20.0, false, crate::i18n::tr(&app.ui.language, "New Swatch")).clicked() {
                let _ = app.run("swatch.create", json!({"color": {"c": 0, "m": 50, "y": 100, "k": 0}}));
            }
            crate::menus::menu_button(ui, "☰", |ui| {
                let current = fill_cur.as_deref().filter(|n| doc.swatch(n).is_some_and(|w| !w.hidden));
                if ui
                    .add_enabled(current.is_some(), egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Swatch Options…"))))
                    .clicked()
                {
                    let _ = app.run("app.swatchOptionsDialog", json!({"name": current}));
                    ui.close();
                }
                crate::menus::menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Ink Manager")), |ui| {
                    let Ok(l) = app.session.execute("ink.list", &json!({})) else { return };
                    let mut all = l["allToProcess"].as_bool().unwrap_or(false);
                    if ui.checkbox(&mut all, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "All Spots to Process"))).changed() {
                        let _ = app.run("ink.options", json!({"allToProcess": all}));
                        ui.close();
                    }
                    ui.separator();
                    let inks = l["inks"].as_array().cloned().unwrap_or_default();
                    if inks.is_empty() {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "No spot inks"));
                    }
                    for ink in inks {
                        let name = ink["name"].as_str().unwrap_or("").to_string();
                        let mut process = ink["process"].as_bool().unwrap_or(false);
                        ui.add_enabled_ui(!all, |ui| {
                            if ui.checkbox(&mut process, format!("{name} → process")).changed() {
                                let _ = app.run("ink.options", json!({"ink": name, "toProcess": process}));
                                ui.close();
                            }
                        });
                    }
                });
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Color Group"))).clicked() {
                    let _ = app.run("swatch.newColorGroup", json!({}));
                    ui.close();
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load Swatches…"))).clicked() {
                    let _ = app.run("app.loadSwatches", json!({}));
                    ui.close();
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Save Swatches for Exchange…"))).clicked() {
                    let _ = app.run("app.saveSwatches", json!({}));
                    ui.close();
                }
            });
        });
    });
}

/// One Swatches panel row: chip, name, type; click applies to fill (or text), the context menu
/// to stroke, colour groups and delete.
fn swatch_row(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    doc: &designcraft_doc::Document,
    sw: &designcraft_color::swatch::Swatch,
    indent: f32,
    fill_cur: Option<&str>,
    text: bool,
) {
    let t = Tokens::get(ui.ctx());
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &sw.name));
    if fill_cur == Some(sw.name.as_str()) {
        ui.painter().rect_filled(row, 0.0, t.row_selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(row, 0.0, t.hover);
    }
    let (c, g) = crate::widgets::swatch_colors(doc, &sw.name, 1.0);
    let chip = egui::Rect::from_min_size(row.min + vec2(4.0 + indent, 3.0), vec2(16.0, 16.0));
    crate::widgets::paint_chip(ui.painter(), chip, c, g);
    ui.painter().text(row.min + vec2(28.0 + indent, 11.0), egui::Align2::LEFT_CENTER, &sw.name, egui::FontId::proportional(12.5), t.text);
    // Type indicators: spot dot / process square, colour mode.
    let kind = match &sw.value {
        SwatchValue::Color { color, color_type } => {
            let m = match color {
                designcraft_color::Color::Cmyk { .. } => "CMYK",
                designcraft_color::Color::Rgb { .. } => "RGB",
                designcraft_color::Color::Gray { .. } => crate::i18n::tr(&app.ui.language, "Gray"),
            };
            format!("{}{}", if *color_type == ColorType::Spot { "● " } else { "" }, m)
        }
        SwatchValue::Gradient { .. } => "Gradient".into(),
        SwatchValue::Tint { tint, .. } => format!("{:.0}%", tint * 100.0),
        _ => String::new(),
    };
    if sw.locked {
        icons::paint(ui.painter(), egui::Rect::from_min_size(egui::pos2(row.max.x - 64.0, row.min.y + 4.0), vec2(13.0, 13.0)), "lock", t.text_dim);
    }
    ui.painter().text(row.right_center() - vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, kind, egui::FontId::proportional(10.5), t.text_dim);
    if resp.clicked() && !resp.double_clicked() {
        let r = if text { app.run("type.char", json!({"attrs": {"fill": sw.name}})) } else { app.run("object.fill", json!({"swatch": sw.name})) };
        let _ = r;
    }
    // Double-click: Swatch Options (the first click has applied the swatch).
    if resp.double_clicked() {
        let _ = app.run("app.swatchOptionsDialog", json!({"name": sw.name}));
    }
    resp.context_menu(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Swatch Options…"))).clicked() {
            let _ = app.run("app.swatchOptionsDialog", json!({"name": sw.name}));
            ui.close();
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Apply to Stroke"))).clicked() {
            let _ = app.run("object.stroke", json!({"swatch": sw.name}));
            ui.close();
        }
        if !sw.locked {
            crate::menus::menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Move to Color Group")), |ui| {
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "(Top Level)"))).clicked() {
                    let _ = app.run("swatch.moveToGroup", json!({"swatches": [sw.name], "group": null}));
                    ui.close();
                }
                for g in &doc.color_groups {
                    if ui.button(&g.name).clicked() {
                        let _ = app.run("swatch.moveToGroup", json!({"swatches": [sw.name], "group": g.name}));
                        ui.close();
                    }
                }
            });
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Color Group with Swatch"))).clicked() {
                let _ = app.run("swatch.newColorGroup", json!({"swatches": [sw.name]}));
                ui.close();
            }
            ui.separator();
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Swatch"))).clicked() {
                let _ = app.run("swatch.delete", json!({"name": sw.name}));
                ui.close();
            }
        }
    });
}

/// Colour models of the Color panel (its panel menu: Lab, CMYK, RGB).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorMode {
    Cmyk,
    Rgb,
    Lab,
}

/// Color panel: fill/stroke proxy, sliders in the chosen model (or a tint slider for a named
/// swatch), the spectrum ramp, Add to Swatches. Edits apply unnamed colours (`object.color`).
pub fn color_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(i) = super::sel_info(app) else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select an object to color its fill or stroke.")).color(t.text_dim),
        ));
        return;
    };
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let stroke_id = egui::Id::new("color_panel_stroke");
    let mode_id = egui::Id::new("color_panel_mode");
    let mut stroke: bool = ui.data(|d| d.get_temp(stroke_id)).unwrap_or(false);
    let default_mode = if doc.settings.intent == designcraft_doc::Intent::Print { ColorMode::Cmyk } else { ColorMode::Rgb };
    let mut mode: ColorMode = ui.data(|d| d.get_temp(mode_id)).unwrap_or(default_mode);
    let target = if stroke { "stroke" } else { "fill" };
    let name = if stroke { i.stroke.clone() } else { i.fill.clone() };
    // Proxy: fill (front) and stroke chips; click to choose, double-click for the Color Picker.
    ui.horizontal(|ui| {
        for (k, label) in [(false, "Fill"), (true, "Stroke")] {
            let n = if k { &i.stroke } else { &i.fill };
            let (c, g) = crate::widgets::swatch_colors(&doc, n, 1.0);
            let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            crate::widgets::paint_chip(ui.painter(), r.shrink(2.0), c, g);
            if k == stroke {
                ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
            let resp = resp.on_hover_text(format!("{label}: {n} (double-click: Color Picker)"));
            if resp.clicked() {
                stroke = k;
            }
            if resp.double_clicked() {
                let rgb = doc.resolve_color(n, 1.0).map(|c| c.to_rgb()).unwrap_or([0.0; 3]);
                let hex =
                    format!("#{:02x}{:02x}{:02x}", (rgb[0] * 255.0).round() as u8, (rgb[1] * 255.0).round() as u8, (rgb[2] * 255.0).round() as u8);
                app.ui.dialog = Some(crate::dialogs::Dialog::new("colorPicker", json!({"target": if k { "stroke" } else { "fill" }, "hex": hex})));
            }
        }
        ui.label(egui::RichText::new(&name).size(11.0).color(t.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt("color_mode")
                .width(64.0)
                .selected_text(match mode {
                    ColorMode::Cmyk => "CMYK",
                    ColorMode::Rgb => "RGB",
                    ColorMode::Lab => "Lab",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mode, ColorMode::Lab, "Lab");
                    ui.selectable_value(&mut mode, ColorMode::Cmyk, "CMYK");
                    ui.selectable_value(&mut mode, ColorMode::Rgb, "RGB");
                });
        });
    });
    ui.data_mut(|d| {
        d.insert_temp(stroke_id, stroke);
        d.insert_temp(mode_id, mode);
    });
    let sw = doc.swatch(&name).cloned();
    let Some(c) = doc.resolve_color(&name, 1.0) else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "[None] — choose a colour on the ramp.")).color(t.text_dim),
        ));
        spectrum(app, ui, target);
        return;
    };
    // A named swatch shows its tint; mixing its values makes an unnamed colour.
    if let Some(sw) = sw.as_ref().filter(|w| !w.hidden && !w.locked) {
        let cur = if stroke { super::sel_tint(app, true) } else { super::sel_tint(app, false) };
        let mut tint = cur * 100.0;
        ui.horizontal(|ui| {
            ui.label("T");
            if ui.add(egui::Slider::new(&mut tint, 0.0..=100.0).suffix("%").integer()).drag_stopped() {
                let cmd = if stroke { "object.stroke" } else { "object.fill" };
                let _ = app.run(cmd, json!({"swatch": sw.name, "tint": tint / 100.0}));
            }
        });
    }
    let mut send: Option<serde_json::Value> = None;
    match mode {
        ColorMode::Cmyk => {
            let mut v = c.to_cmyk();
            let mut done = false;
            for (k, label) in ["C", "M", "Y", "K"].iter().enumerate() {
                done |= channel(ui, label, &mut v[k], 100.0, "%", |x| cmyk_ramp(k, x));
            }
            if done {
                send = Some(json!({"c": v[0] * 100.0, "m": v[1] * 100.0, "y": v[2] * 100.0, "k": v[3] * 100.0}));
            }
        }
        ColorMode::Rgb => {
            let mut v = c.to_rgb();
            let mut done = false;
            for (k, label) in ["R", "G", "B"].iter().enumerate() {
                done |= channel(ui, label, &mut v[k], 255.0, "", |x| {
                    let mut p = [0.0; 3];
                    p[k] = x;
                    egui::Color32::from_rgb((p[0] * 255.0) as u8, (p[1] * 255.0) as u8, (p[2] * 255.0) as u8)
                });
            }
            if done {
                send = Some(json!([(v[0] * 255.0).round(), (v[1] * 255.0).round(), (v[2] * 255.0).round()]));
            }
        }
        ColorMode::Lab => {
            let lab = c.to_lab();
            ui.label(egui::RichText::new(format!("L {:.0}   a {:.0}   b {:.0}", lab.l, lab.a, lab.b)).color(t.text_dim));
        }
    }
    if let Some(color) = send {
        let _ = app.run("object.color", json!({"color": color, "target": target}));
    }
    spectrum(app, ui, target);
    ui.horizontal(|ui| {
        let unnamed = sw.as_ref().is_some_and(|w| w.hidden);
        if ui.add_enabled(unnamed, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add to Swatches")))).clicked() {
            let _ = app.run("swatch.addToSwatches", json!({"target": target}));
        }
    });
}

/// One slider with a gradient track of the channel and a value field. True when it was changed
/// (on release / edit).
fn channel(ui: &mut egui::Ui, label: &str, v: &mut f32, scale: f32, suffix: &str, ramp: impl Fn(f32) -> egui::Color32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).monospace());
        let w = (ui.available_width() - 60.0).max(60.0);
        let (track, resp) = ui.allocate_exact_size(vec2(w, 14.0), Sense::click_and_drag());
        // Gradient track.
        let n = 24;
        for i in 0..n {
            let x0 = track.min.x + track.width() * i as f32 / n as f32;
            let x1 = track.min.x + track.width() * (i + 1) as f32 / n as f32;
            let r = egui::Rect::from_min_max(egui::pos2(x0, track.min.y + 2.0), egui::pos2(x1 + 0.5, track.max.y - 2.0));
            ui.painter().rect_filled(r, 0.0, ramp((i as f32 + 0.5) / n as f32));
        }
        let x = track.min.x + track.width() * v.clamp(0.0, 1.0);
        ui.painter().add(egui::Shape::convex_polygon(
            vec![egui::pos2(x, track.max.y - 3.0), egui::pos2(x - 4.0, track.max.y + 3.0), egui::pos2(x + 4.0, track.max.y + 3.0)],
            ui.visuals().text_color(),
            egui::Stroke::NONE,
        ));
        if let Some(p) = resp.interact_pointer_pos() {
            *v = ((p.x - track.min.x) / track.width()).clamp(0.0, 1.0);
        }
        changed |= resp.drag_stopped() || resp.clicked();
        let mut num = (*v * scale).round();
        let field = ui.add(egui::DragValue::new(&mut num).range(0.0..=scale).suffix(suffix));
        if field.changed() {
            *v = num / scale;
        }
        changed |= field.lost_focus() || field.drag_stopped();
    });
    changed
}

fn cmyk_ramp(k: usize, x: f32) -> egui::Color32 {
    let mut v = [0.0f32; 4];
    v[k] = x;
    let [r, g, b] = designcraft_color::Color::cmyk(v[0], v[1], v[2], v[3]).to_rgb();
    egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// The spectrum ramp: hue across, light to dark down; click to apply.
fn spectrum(app: &mut DesignApp, ui: &mut egui::Ui, target: &str) {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let at = |u: f32, v: f32| -> [f32; 3] {
        let c = egui::ecolor::Hsva::new(u, (v * 2.0).min(1.0), (2.0 - v * 2.0).min(1.0), 1.0).to_rgb();
        [c[0], c[1], c[2]]
    };
    let (nx, ny) = (48, 8);
    for i in 0..nx {
        for j in 0..ny {
            let c = at((i as f32 + 0.5) / nx as f32, (j as f32 + 0.5) / ny as f32);
            let cell = egui::Rect::from_min_max(
                egui::pos2(r.min.x + r.width() * i as f32 / nx as f32, r.min.y + r.height() * j as f32 / ny as f32),
                egui::pos2(r.min.x + r.width() * (i + 1) as f32 / nx as f32 + 0.5, r.min.y + r.height() * (j + 1) as f32 / ny as f32 + 0.5),
            );
            ui.painter().rect_filled(cell, 0.0, egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
        }
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let c = at(((p.x - r.min.x) / r.width()).clamp(0.0, 1.0), ((p.y - r.min.y) / r.height()).clamp(0.0, 1.0));
        let _ = app.run("object.color", json!({"color": [(c[0] * 255.0).round(), (c[1] * 255.0).round(), (c[2] * 255.0).round()], "target": target}));
    }
}

/// Stops in `object.gradient` form.
fn stops_json(g: &designcraft_color::Gradient) -> Vec<serde_json::Value> {
    g.stops
        .iter()
        .map(|s| json!({"location": s.offset as f64 * 100.0, "color": s.color.to_hex(), "opacity": s.opacity as f64 * 100.0, "midpoint": s.midpoint as f64 * 100.0}))
        .collect()
}

/// Gradient panel: type, angle, reverse, and the ramp with its stops (drag to move, click below
/// the ramp to add, drag off to remove; the selected stop's location and colour are editable).
pub fn gradient_panel(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let Some(it) = st.selection.items.first().and_then(|i| st.doc.item(*i)) else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select an object to give its fill a gradient.")).color(t.text_dim),
        ));
        return;
    };
    let fill = it.fill.clone();
    let Some(g) = designcraft_color::swatch::resolve_gradient(&st.doc.swatches, &fill.swatch).cloned() else {
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "The fill isn't a gradient.")).color(t.text_dim));
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Apply Gradient"))).clicked() {
            let _ = app.run("object.gradient", json!({}));
        }
        return;
    };
    let radial = g.kind == designcraft_color::GradientKind::Radial;
    ui.horizontal(|ui| {
        crate::widgets::caption(ui, crate::i18n::tr(&app.ui.language, "Type:"));
        egui::ComboBox::from_id_salt("grad_type")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, g.kind.label())))
            .width(100.0)
            .show_ui(ui, |ui| {
                for (k, v) in [("Linear", "linear"), ("Radial", "radial")] {
                    if ui.selectable_label(g.kind.label() == k, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, k))).clicked() {
                        let _ = app.run("object.gradient", json!({"kind": v}));
                    }
                }
            });
        if ui
            .button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reverse")))
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Reverse the gradient"));
            })
            .clicked()
        {
            let _ = app.run("object.gradient", json!({"reverse": true}));
        }
    });
    let sel_id = egui::Id::new("grad_sel");
    let drag_id = egui::Id::new("grad_drag");
    let mut sel: usize = ui.data(|d| d.get_temp(sel_id)).unwrap_or(0).min(g.stops.len().saturating_sub(1));
    ui.horizontal(|ui| {
        crate::widgets::caption(ui, crate::i18n::tr(&app.ui.language, "Location:"));
        let loc = g.stops.get(sel).map(|s| s.offset as f64 * 100.0);
        if let Some(v) = crate::widgets::number(ui, "grad_loc", loc, " %", 54.0, 1) {
            let mut stops = stops_json(&g);
            stops[sel]["location"] = json!(v.clamp(0.0, 100.0));
            let _ = app.run("object.gradient", json!({"stops": stops}));
        }
        if !radial {
            crate::widgets::caption(ui, crate::i18n::tr(&app.ui.language, "Angle:"));
            let angle = fill.gradient_vector.map(|[x0, y0, x1, y1]| (-(y1 - y0)).atan2(x1 - x0).to_degrees()).or(fill.gradient_angle).unwrap_or(0.0);
            if let Some(v) = crate::widgets::number(ui, "grad_angle", Some(angle), "°", 54.0, 1) {
                let _ = app.run("object.gradient", json!({"angle": v}));
            }
        }
    });
    ui.add_space(4.0);
    // The ramp, with stop markers below it.
    let w = ui.available_width().min(240.0);
    let (area, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click_and_drag());
    let ramp = egui::Rect::from_min_size(area.min + vec2(6.0, 0.0), vec2(w - 12.0, 18.0));
    let painter = ui.painter();
    let n = 64;
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let (c, alpha) = g.sample((a + b) / 2.0);
        let [r, gg, bb] = c.to_rgb();
        let col = egui::Color32::from_rgba_unmultiplied((r * 255.0) as u8, (gg * 255.0) as u8, (bb * 255.0) as u8, (alpha * 255.0) as u8);
        let x0 = ramp.left() + ramp.width() * a;
        let x1 = ramp.left() + ramp.width() * b + 0.5;
        painter.rect_filled(egui::Rect::from_x_y_ranges(x0..=x1, ramp.y_range()), 0.0, col);
    }
    painter.rect_stroke(ramp, 0.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Outside);
    // A stop being dragged: (index, location 0–1, off the ramp).
    let mut dragging: Option<(usize, f32, bool)> = ui.data(|d| d.get_temp(drag_id));
    let x_of = |o: f32| ramp.left() + ramp.width() * o;
    let o_of = |x: f32| ((x - ramp.left()) / ramp.width()).clamp(0.0, 1.0);
    let near = |x: f32| {
        g.stops.iter().enumerate().map(|(i, s)| (i, (x_of(s.offset) - x).abs())).filter(|(_, d)| *d < 7.0).min_by(|a, b| a.1.total_cmp(&b.1))
    };
    if resp.drag_started()
        && let Some(p) = resp.interact_pointer_pos()
        && let Some((i, _)) = near(p.x)
    {
        sel = i;
        dragging = Some((i, g.stops[i].offset, false));
    }
    if resp.dragged()
        && let (Some((i, _, _)), Some(p)) = (dragging, resp.interact_pointer_pos())
    {
        dragging = Some((i, o_of(p.x), p.y > area.bottom() + 16.0 && g.stops.len() > 2));
    }
    if resp.drag_stopped()
        && let Some((i, o, off)) = dragging.take()
    {
        let mut stops = stops_json(&g);
        if off {
            stops.remove(i);
            sel = 0;
        } else {
            stops[i]["location"] = json!(o as f64 * 100.0);
        }
        let _ = app.run("object.gradient", json!({"stops": stops}));
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        match near(p.x) {
            Some((i, _)) => sel = i,
            // Below the ramp, away from the stops: a new stop with the colour there.
            None if p.y > ramp.bottom() => {
                let o = o_of(p.x);
                let (c, a) = g.sample(o);
                let mut stops = stops_json(&g);
                stops.push(json!({"location": o as f64 * 100.0, "color": c.to_hex(), "opacity": a as f64 * 100.0, "midpoint": 50.0}));
                let _ = app.run("object.gradient", json!({"stops": stops}));
                sel = g.stops.iter().filter(|s| s.offset <= o).count();
            }
            None => {}
        }
    }
    for (i, s) in g.stops.iter().enumerate() {
        let (o, off) = match dragging {
            Some((j, o, off)) if j == i => (o, off),
            _ => (s.offset, false),
        };
        if off {
            continue;
        }
        let x = x_of(o);
        let top = ramp.bottom() + 2.0;
        let [r, gg, bb] = s.color.to_rgb();
        let col = egui::Color32::from_rgb((r * 255.0) as u8, (gg * 255.0) as u8, (bb * 255.0) as u8);
        let edge = if i == sel { t.accent } else { t.text_dim };
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, top),
                egui::pos2(x + 6.0, top + 7.0),
                egui::pos2(x + 6.0, top + 13.0),
                egui::pos2(x - 6.0, top + 13.0),
                egui::pos2(x - 6.0, top + 7.0),
            ],
            col,
            egui::Stroke::new(if i == sel { 2.0 } else { 1.0 }, edge),
        ));
    }
    ui.data_mut(|d| {
        d.insert_temp(sel_id, sel);
        match dragging {
            Some(v) => {
                d.insert_temp(drag_id, v);
            }
            None => d.remove::<(usize, f32, bool)>(drag_id),
        }
    });
    // The selected stop's colour.
    if let Some(s) = g.stops.get(sel) {
        ui.horizontal(|ui| {
            crate::widgets::caption(ui, crate::i18n::tr(&app.ui.language, "Stop Color:"));
            let [r, gg, bb] = s.color.to_rgb();
            let mut rgb = [(r * 255.0) as u8, (gg * 255.0) as u8, (bb * 255.0) as u8];
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                let mut stops = stops_json(&g);
                stops[sel]["color"] = json!(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
                let _ = app.run("object.gradient", json!({"stops": stops}));
            }
        });
    }
}
