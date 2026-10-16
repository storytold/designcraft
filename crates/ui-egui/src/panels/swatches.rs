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
    let target = super::color_target(app);
    header(app, ui, &doc, target.as_ref());
    // The row of the colour in front of the proxy is highlighted.
    let fill_cur = target.map(|(fill, _, stroke, _)| if app.ui.stroke_front { stroke } else { fill });
    // Unnamed colours (mixed in the Color panel) aren't listed until Add to Swatches; swatches in
    // colour groups are listed under their folder.
    let grouped: Vec<&str> = doc.color_groups.iter().flat_map(|g| g.swatches.iter().map(String::as_str)).collect();
    for sw in doc.swatches.iter().filter(|w| !w.hidden && !grouped.contains(&w.name.as_str())) {
        swatch_row(app, ui, &doc, sw, 0.0, fill_cur.as_deref());
    }
    for g in &doc.color_groups {
        let open_id = egui::Id::new(("color_group_open", &g.name));
        let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(true);
        let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
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
        if resp.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_temp(open_id, open));
        }
        resp.context_menu(|ui| {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Ungroup Color Group"))).clicked() {
                let _ = app.run("swatch.ungroupColorGroup", json!({"name": g.name}));
                ui.close();
            }
        });
        if open {
            for n in &g.swatches {
                if let Some(sw) = doc.swatches.iter().find(|w| w.name == *n) {
                    swatch_row(app, ui, &doc, sw, 16.0, fill_cur.as_deref());
                }
            }
        }
    }
    ui.add_space(6.0);
    // InDesign's bottom row: New Color Group, New Swatch, Delete Swatch (the one last clicked).
    let picked: Option<String> = ui.data(|d| d.get_temp(egui::Id::new(PICKED))).filter(|n: &String| doc.swatch(n).is_some_and(|w| !w.locked));
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let del = ui
                .add_enabled_ui(picked.is_some(), |ui| icons::button(ui, "trash", 20.0, false, crate::i18n::tr(&app.ui.language, "Delete Swatch")))
                .inner;
            if del.clicked()
                && let Some(name) = &picked
                && app.run("swatch.delete", json!({"name": name})).is_ok()
            {
                ui.data_mut(|d| d.remove::<String>(egui::Id::new(PICKED)));
            }
            if icons::button(ui, "plus", 20.0, false, crate::i18n::tr(&app.ui.language, "New Swatch")).clicked() {
                let _ = app.run("swatch.create", json!({"color": {"c": 0, "m": 50, "y": 100, "k": 0}}));
            }
            if folder_button(ui, crate::i18n::tr(&app.ui.language, "New Color Group")).clicked() {
                let _ = app.run("swatch.newColorGroup", json!({}));
            }
            crate::menus::menu_button(ui, "☰", |ui| {
                crate::menus::menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Ink Manager")), |ui| {
                    let Ok(l) = app.session.execute("ink.list", &json!({})) else { return };
                    let mut all = l["allToProcess"].as_bool().unwrap_or(false);
                    let response = crate::widgets::native_checkbox(
                        ui,
                        &mut all,
                        crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "All Spots to Process")),
                    );
                    if response.changed() {
                        let _ = app.run("ink.options", json!({"allToProcess": all}));
                    }
                    if crate::widgets::checkbox_clicked(ui, &response) {
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
                            let response = ui
                                .push_id(("ink_to_process", &name), |ui| {
                                    crate::widgets::native_checkbox(ui, &mut process, format!("{name} → process"))
                                })
                                .inner;
                            // Aliased rows show resolved output; only write a changed raw flag.
                            if response.changed() && doc.inks.to_process.contains(&name) != process {
                                let _ = app.run("ink.options", json!({"ink": name, "toProcess": process}));
                            }
                            if crate::widgets::checkbox_clicked(ui, &response) {
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

/// Temp-data key of the swatch last clicked (Delete Swatch removes it).
const PICKED: &str = "swatches_picked";

/// The Swatches panel's top row, as InDesign's: a Fill/Stroke proxy (click: the other in front),
/// Formatting Affects Container / Text, and the tint of the colour in front. `target` is
/// [`super::color_target`]: fill and stroke swatches and tints of the text or the first object.
fn header(app: &mut DesignApp, ui: &mut egui::Ui, doc: &designcraft_doc::Document, target: Option<&(String, f32, String, f32)>) {
    let t = Tokens::get(ui.ctx());
    let (fill_sw, stroke_sw) = target.map_or(("[None]", "[Black]"), |(f, _, s, _)| (f.as_str(), s.as_str()));
    let text = super::colors_affect_text(app) || app.ui.formatting_affects_text;
    let stroke_front = app.ui.stroke_front;
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        let (r, resp) = ui.allocate_exact_size(vec2(27.0, 27.0), Sense::click());
        let tip = crate::i18n::tr(&app.ui.language, if stroke_front { "Stroke" } else { "Fill" });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
        let fill_r = egui::Rect::from_min_size(r.min + vec2(1.0, 2.0), vec2(16.0, 16.0));
        let stroke_r = egui::Rect::from_min_size(r.min + vec2(10.0, 10.0), vec2(16.0, 16.0));
        let paint = |stroke: bool| {
            let p = ui.painter();
            if stroke {
                // The stroke: a frame of its colour.
                let (c, g) = crate::widgets::swatch_colors(doc, stroke_sw, 1.0);
                crate::widgets::paint_chip(p, stroke_r, c, g);
                p.rect_filled(stroke_r.shrink(4.0), 0.0, t.panel);
                p.rect_stroke(stroke_r.shrink(4.0), 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
                p.rect_stroke(stroke_r, 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
            } else {
                let (c, g) = crate::widgets::swatch_colors(doc, fill_sw, 1.0);
                crate::widgets::paint_chip(p, fill_r, c, g);
                p.rect_stroke(fill_r, 0.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Outside);
            }
        };
        // The one in front is drawn last.
        paint(!stroke_front);
        paint(stroke_front);
        if resp
            .on_hover_ui(|ui| {
                crate::rtl::label(ui, tip);
            })
            .clicked()
        {
            let _ = app.run("color.toggleFillStroke", json!({}));
        }
        ui.add_space(4.0);
        for (k, (icon, tip)) in
            [("format-container", "Formatting affects container (J)"), ("format-text", "Formatting affects text (J)")].into_iter().enumerate()
        {
            if icons::button(ui, icon, 20.0, text == (k == 1), crate::i18n::tr(&app.ui.language, tip)).clicked() {
                let _ = app.run("app.formattingAffectsText", json!({"on": k == 1}));
            }
        }
        ui.add_space(6.0);
        crate::widgets::caption(ui, crate::i18n::tr(&app.ui.language, "Tint:"));
        let tint = target.map(|(_, ft, _, st)| f64::from(if stroke_front { *st } else { *ft }) * 100.0);
        if let Some(v) = crate::widgets::number(ui, "swatches_tint", tint, "", 44.0, 0) {
            let _ = set_tint(app, v);
        }
        crate::widgets::caption(ui, "%");
    });
}

/// The Tint field: `percent` of the colour in front of the proxy, for the text or each selected
/// object, which keep their own swatches.
fn set_tint(app: &mut DesignApp, percent: f64) -> Result<serde_json::Value, String> {
    let v = percent.clamp(0.0, 100.0) / 100.0;
    let stroke = app.ui.stroke_front;
    if super::colors_affect_text(app) {
        app.run("type.char", json!({"attrs": {if stroke { "strokeTint" } else { "fillTint" }: v}}))
    } else {
        app.run(if stroke { "object.stroke" } else { "object.fill" }, json!({"tint": v}))
    }
}

/// A folder button (New Color Group), drawn like the colour groups' folders.
fn folder_button(ui: &mut egui::Ui, tip: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    let f = egui::Rect::from_center_size(r.center(), vec2(14.0, 10.0));
    ui.painter().rect_stroke(f, 1.5, egui::Stroke::new(1.2, t.icon), egui::StrokeKind::Inside);
    ui.painter().line_segment([f.left_top() + vec2(1.0, 3.0), f.right_top() + vec2(-1.0, 3.0)], egui::Stroke::new(1.0, t.icon));
    resp.on_hover_ui(|ui| {
        crate::rtl::label(ui, tip);
    })
}

/// One Swatches panel row: chip, name, type; click applies it to the fill or stroke in front of
/// the proxy (of the frames or their text, see [`super::colors_affect_text`]), the context menu
/// to the stroke, colour groups and delete.
fn swatch_row(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    doc: &designcraft_doc::Document,
    sw: &designcraft_color::swatch::Swatch,
    indent: f32,
    fill_cur: Option<&str>,
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
    if resp.clicked() {
        ui.data_mut(|d| d.insert_temp(egui::Id::new(PICKED), sw.name.clone()));
        let _ = super::apply_swatch(app, app.ui.stroke_front, &sw.name, None);
    }
    resp.context_menu(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Apply to Stroke"))).clicked() {
            let _ = super::apply_swatch(app, true, &sw.name, None);
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
    let Some((fill, fill_tint, stroke_sw, stroke_tint)) = super::color_target(app) else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Select an object or text to color its fill or stroke.")).color(t.text_dim),
        ));
        return;
    };
    let text = super::colors_affect_text(app);
    let Some(doc) = app.session.active().map(|d| d.doc.clone()) else { return };
    let mode_id = egui::Id::new("color_panel_mode");
    let mut stroke = app.ui.stroke_front;
    let default_mode = if doc.settings.intent == designcraft_doc::Intent::Print { ColorMode::Cmyk } else { ColorMode::Rgb };
    let mut mode: ColorMode = ui.data(|d| d.get_temp(mode_id)).unwrap_or(default_mode);
    let target = if stroke { "stroke" } else { "fill" };
    let name = if stroke { stroke_sw.clone() } else { fill.clone() };
    // Proxy: fill (front) and stroke chips; click to choose, double-click for the Color Picker.
    ui.horizontal(|ui| {
        for (k, label) in [(false, "Fill"), (true, "Stroke")] {
            let n = if k { &stroke_sw } else { &fill };
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
                app.ui.dialog =
                    Some(crate::dialogs::Dialog::new("colorPicker", json!({"target": if k { "stroke" } else { "fill" }, "hex": hex, "text": text})));
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
    app.ui.stroke_front = stroke;
    ui.data_mut(|d| d.insert_temp(mode_id, mode));
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
        let cur = if stroke { stroke_tint } else { fill_tint };
        let mut tint = cur * 100.0;
        let mut commit = None;
        ui.horizontal(|ui| {
            ui.label("T");
            // The slider applies on release. The field keeps typed text while it has focus (the
            // tint is read back from the document every frame) and applies it, clamped, on Enter
            // or when it loses focus.
            ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(60.0);
            if ui.add(egui::Slider::new(&mut tint, 0.0..=100.0).integer().show_value(false)).drag_stopped() {
                commit = Some(tint);
            }
            if let Some(v) =
                crate::widgets::NumField::number("color_tint", Some(f64::from(cur) * 100.0), "%", 0).width(44.0).range(0.0, 100.0).show(ui)
            {
                commit = Some(v as f32);
            }
        });
        if let Some(v) = commit {
            let _ = super::apply_swatch(app, stroke, &sw.name, Some(v / 100.0));
        }
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
        let _ = app.run("object.color", json!({"color": color, "target": target, "text": text}));
    }
    spectrum(app, ui, target);
    ui.horizontal(|ui| {
        let unnamed = sw.as_ref().is_some_and(|w| w.hidden);
        if ui.add_enabled(unnamed, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add to Swatches")))).clicked() {
            let _ = app.run("swatch.addToSwatches", json!({"swatch": name, "target": target}));
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
        let text = super::colors_affect_text(app);
        let _ = app.run(
            "object.color",
            json!({"color": [(c[0] * 255.0).round(), (c[1] * 255.0).round(), (c[2] * 255.0).round()], "target": target, "text": text}),
        );
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
    // The fill's gradient, or the stroke's when the toolbox's Stroke box is in front (as InDesign's
    // Gradient panel follows the Fill/Stroke proxy).
    let target = if app.ui.stroke_front { "stroke" } else { "fill" };
    let fill = if app.ui.stroke_front { it.stroke.as_fill() } else { it.fill.clone() };
    let Some(g) = designcraft_color::swatch::resolve_gradient(&st.doc.swatches, &fill.swatch).cloned() else {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(
                &app.ui.language,
                if app.ui.stroke_front { "The stroke isn't a gradient." } else { "The fill isn't a gradient." },
            ))
            .color(t.text_dim),
        );
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Apply Gradient"))).clicked() {
            let _ = app.run("object.gradient", json!({"target": target, "target": target}));
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
                        let _ = app.run("object.gradient", json!({"target": target, "kind": v}));
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
            let _ = app.run("object.gradient", json!({"target": target, "reverse": true}));
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
            let _ = app.run("object.gradient", json!({"target": target, "stops": stops}));
        }
        if !radial {
            crate::widgets::caption(ui, crate::i18n::tr(&app.ui.language, "Angle:"));
            let angle = fill.gradient_vector.map(|[x0, y0, x1, y1]| (-(y1 - y0)).atan2(x1 - x0).to_degrees()).or(fill.gradient_angle).unwrap_or(0.0);
            if let Some(v) = crate::widgets::number(ui, "grad_angle", Some(angle), "°", 54.0, 1) {
                let _ = app.run("object.gradient", json!({"target": target, "angle": v}));
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
        let _ = app.run("object.gradient", json!({"target": target, "stops": stops}));
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
                let _ = app.run("object.gradient", json!({"target": target, "stops": stops}));
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
                let _ = app.run("object.gradient", json!({"target": target, "stops": stops}));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink_frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 800.0));
        let time = ctx.input(|i| i.time) + 1.0 / 60.0;
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), events, ..Default::default() }, |ui| {
            ui.set_max_width(280.0);
            show(app, ui);
        });
        output.textures_delta.clear();
        fn collect(shape: &egui::Shape, clip: egui::Rect, labels: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => {
                    let rect = text.visual_bounding_rect();
                    if clip.contains_rect(rect) {
                        labels.push((text.galley.job.text.trim().to_owned(), rect));
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, labels);
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, shape.clip_rect.intersect(screen), &mut labels);
        }
        labels
    }

    fn label_center(labels: &[(String, egui::Rect)], label: &str) -> egui::Pos2 {
        labels.iter().find(|(text, _)| text == label).unwrap_or_else(|| panic!("{label} is visibly painted")).1.center()
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn open_ink_manager(app: &mut DesignApp, ctx: &egui::Context) -> Vec<(String, egui::Rect)> {
        for label in ["☰", "Ink Manager"] {
            let labels = ink_frame(app, ctx, vec![]);
            let pos = label_center(&labels, label);
            for pressed in [true, false] {
                ink_frame(app, ctx, vec![egui::Event::PointerMoved(pos), pointer(pos, pressed)]);
            }
            // Settle the real popup's sizing pass before looking up its next row.
            for _ in 0..3 {
                ink_frame(app, ctx, vec![]);
            }
        }
        ink_frame(app, ctx, vec![])
    }

    fn assert_ink_state(
        app: &mut DesignApp,
        baseline: &designcraft_engine::DocState,
        names: &[String; 2],
        individual: [bool; 2],
        all: bool,
        edits: usize,
    ) {
        assert!(baseline.doc.inks.aliases.is_empty());
        assert_resolved_ink_state(app, baseline, names, individual, individual.map(|on| all || on), all, edits);
    }

    fn assert_resolved_ink_state(
        app: &mut DesignApp,
        baseline: &designcraft_engine::DocState,
        names: &[String; 2],
        individual: [bool; 2],
        effective: [bool; 2],
        all: bool,
        edits: usize,
    ) {
        let list = app.session.execute("ink.list", &json!({})).unwrap();
        assert_eq!(list["allToProcess"], all);
        let inks = list["inks"].as_array().unwrap();
        assert_eq!(inks.len(), 2);
        for (name, process) in names.iter().zip(effective) {
            let ink = inks.iter().find(|ink| ink["name"] == *name).unwrap();
            assert_eq!(ink["process"], process);
            let alias = baseline.doc.inks.aliases.iter().find(|(from, _)| from == name).map(|(_, to)| to);
            assert_eq!(ink["alias"], json!(alias));
        }
        let st = app.session.active().unwrap();
        let mut expected = baseline.doc.inks.clone();
        expected.all_to_process = all;
        expected.to_process = names.iter().zip(individual).filter(|(_, on)| *on).map(|(name, _)| name.clone()).collect();
        assert_eq!(st.doc.inks, expected);
        assert_eq!(st.doc.swatches, baseline.doc.swatches);
        assert_eq!(st.revision, baseline.revision + edits as u64);
        assert_eq!(st.history.undo.len(), baseline.history.undo.len() + edits);
        assert_eq!(st.history.redo.len(), baseline.history.redo.len());
        assert!(st.history.undo[baseline.history.undo.len()..].iter().all(|entry| entry.label == "Ink Manager"));
    }

    #[test]
    fn ink_manager_held_origin_cannot_follow_queued_spot_replacement() {
        use designcraft_color::Color;
        use designcraft_engine::{DocState, HistoryEntry, Session};
        use egui_kittest::{
            Harness,
            kittest::{NodeT, Queryable},
        };
        use serde_json::Value;
        use std::sync::Arc;

        fn same(a: &DocState, e: &DocState) {
            macro_rules! fields {
            ($($f:ident),*) => {$(assert_eq!(a.$f, e.$f, stringify!($f));)*}
        }
            // Fail narrowly before the full model comparison if another ink was touched.
            assert_eq!(a.doc.inks.to_process, e.doc.inks.to_process);
            fields!(doc, selection, uid, revision, saved_revision, saved_doc, active_layer, path, editing_parents, preview_stash, preview_record);
            assert_eq!(a.is_dirty(), e.is_dirty());
            assert!(a.interaction.is_none() && e.interaction.is_none());
            assert!(a.fonts.is_none() && e.fonts.is_none());
            assert_eq!((a.history.undo.len(), a.history.redo.len(), a.history.limit), (e.history.undo.len(), e.history.redo.len(), e.history.limit));
            for (x, y) in a.history.undo.iter().chain(&a.history.redo).zip(e.history.undo.iter().chain(&e.history.redo)) {
                assert_eq!((&x.label, x.doc.as_ref(), &x.selection), (&y.label, y.doc.as_ref(), &y.selection));
            }
        }
        fn edited(before: &DocState, document: designcraft_doc::Document, label: &str) -> DocState {
            let mut e = before.clone();
            e.doc = Arc::new(document);
            e.revision += 1;
            e.history.undo.push(HistoryEntry { label: label.into(), doc: before.doc.clone(), selection: before.selection.clone() });
            e.history.redo.clear();
            e
        }
        fn state(app: &DesignApp, expected: &DocState, prefs: &Value, ui: &Value) {
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(app.session.active_index(), Some(0));
            same(app.session.active().unwrap(), expected);
            assert_eq!(&serde_json::to_value(&app.session.prefs).unwrap(), prefs);
            assert_eq!(&serde_json::to_value(&app.ui).unwrap(), ui);
        }
        fn blank_guard(h: &Harness<'_, DesignApp>, point: egui::Pos2, layer: egui::LayerId) {
            assert!(h.ctx.content_rect().contains(point));
            assert_eq!(h.ctx.layer_id_at(point), Some(layer), "blank stays inside Ink Manager's popup layer");
            for n in h.root().children_recursive().filter(|n| n.accesskit_node().role() == egui::accesskit::Role::CheckBox) {
                assert!(!n.rect().expand(0.5).contains(point), "blank never visits a checkbox");
            }
        }

        let mut donor = Session::new();
        donor.execute("file.new", &json!({})).unwrap();
        donor
            .execute(
                "swatch.create",
                &json!({"name":"Held Ink A", "spot":false,
        "color":{"c":0,"m":100,"y":0,"k":0}}),
            )
            .unwrap();
        let payload = donor.execute("swatch.save", &json!({"names":["Held Ink A"]})).unwrap()["base64"].as_str().unwrap().to_owned();
        assert!(!payload.is_empty());
        for replace in [false, true] {
            let mut app = DesignApp::new(Session::new(), crate::Services::default());
            app.ui.language = "en".into();
            app.run("file.new", json!({"facingPages":false,"primaryTextFrame":false})).unwrap();
            for name in ["Held Ink A", "Held Ink B", "Held Ink C"] {
                assert_eq!(
                    app.run(
                        "swatch.create",
                        json!({"name":name,"spot":true,
                "color":{"c":0,"m":100,"y":0,"k":0}})
                    )
                    .unwrap()["name"],
                    json!(name)
                );
            }
            let restored = app.session.active().unwrap().doc.clone();
            app.run("layout.documentSetup", json!({"facingPages":true})).unwrap();
            assert_ne!(app.session.active().unwrap().doc, restored);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.active().unwrap().doc, restored);
            let (tx, rx) = std::sync::mpsc::channel();
            app.control_rx = Some(rx);
            let mut ready = false;
            let mut h = Harness::builder().with_size(vec2(1000.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
                move |ui, app: &mut DesignApp| {
                    if !ready {
                        ready = true;
                        return;
                    }
                    app.logic(ui.ctx());
                    if !app.fonts_ready {
                        return;
                    }
                    ui.set_max_width(280.0);
                    show(app, ui);
                },
                app,
            );
            h.input_mut().max_texture_side = Some(8192);
            h.run_steps(8);
            for label in ["☰", "Ink Manager ⏵"] {
                h.get_by_label(label).click();
                h.run_steps(3);
            }
            let before = h.state().session.active().unwrap().clone();
            assert!(!before.history.undo.is_empty());
            assert_eq!(before.history.redo.len(), 1);
            assert!(!before.doc.inks.all_to_process && before.doc.inks.to_process.is_empty() && before.doc.inks.aliases.is_empty());
            for name in ["Held Ink A", "Held Ink B", "Held Ink C"] {
                let sw = before.doc.swatch(name).unwrap();
                assert!(!sw.locked && !sw.hidden && sw.named);
                assert_eq!(sw.value, SwatchValue::Color { color: Color::Cmyk { c: 0.0, m: 1.0, y: 0.0, k: 0.0 }, color_type: ColorType::Spot });
                assert!(h.query_by_label(&format!("{name} → process")).is_some());
            }
            let prefs = serde_json::to_value(&h.state().session.prefs).unwrap();
            let ui_before = serde_json::to_value(&h.state().ui).unwrap();
            let mut document = (*before.doc).clone();
            document.inks.to_process = vec!["Held Ink A".into()];
            let origin_expected = edited(&before, document, "Ink Manager");
            let mut document = (*origin_expected.doc).clone();
            let sw = document.swatches.iter_mut().find(|s| s.name == "Held Ink A").unwrap();
            sw.value = SwatchValue::Color { color: Color::Cmyk { c: 0.0, m: 1.0, y: 0.0, k: 0.0 }, color_type: ColorType::Process };
            sw.hidden = false;
            let replaced_expected = edited(&origin_expected, document, "Load Swatches…");
            let n = h.get_by_label("Held Ink A → process");
            let local = n.accesskit_node().locate().0;
            let rect = n.rect();
            assert_eq!(n.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            assert!(!n.accesskit_node().is_disabled());
            let p = rect.center();
            h.input_mut().events.push(egui::Event::PointerMoved(p));
            h.run_steps(1);
            assert_eq!(h.get_by_label("Held Ink A → process").rect(), rect);
            assert_eq!(h.get_by_label("Held Ink A → process").accesskit_node().locate().0, local);
            let id = h
                .ctx
                .interaction_snapshot(|s| s.hovered.iter().copied().find(|id| id.accesskit_id() == local))
                .expect("real labelled origin receives hover");
            let origin = h.ctx.read_response(id).unwrap();
            assert!(origin.enabled() && origin.interact_rect.contains(p));
            assert_eq!(h.ctx.layer_id_at(p), Some(origin.layer_id));
            // The popup's left padding is outside every checkbox and stays on the same row.
            let blank = egui::pos2(rect.left() - 2.0, rect.center().y);
            assert!(p.distance(blank) > 6.0 && !origin.interact_rect.contains(blank));
            blank_guard(&h, blank, origin.layer_id);
            state(h.state(), &before, &prefs, &ui_before);
            h.input_mut().events.extend([egui::Event::PointerMoved(p), pointer(p, true)]);
            h.run_steps(1);
            assert!(h.ctx.input(|i| i.pointer.primary_pressed()
                && i.pointer.primary_down()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(p)));
            state(h.state(), &origin_expected, &prefs, &ui_before);
            h.input_mut().events.push(egui::Event::PointerMoved(blank));
            h.run_steps(1);
            assert!(h.ctx.input(|i| !i.pointer.primary_pressed()
                && i.pointer.primary_down()
                && !i.pointer.primary_released()
                && i.pointer.is_decidedly_dragging()));
            assert_eq!(h.ctx.dragged_id(), Some(id));
            assert!(h.ctx.read_response(id).unwrap().is_pointer_button_down_on());
            blank_guard(&h, blank, origin.layer_id);
            state(h.state(), &origin_expected, &prefs, &ui_before);
            assert!(h.input().events.is_empty());
            if replace {
                let (request, reply) = crate::ControlRequest::new(
                    "engine.execute",
                    json!({
                "command":"swatch.load", "params":{"base64":payload,"replace":true}}),
                );
                tx.send(request).unwrap();
                h.run_steps(1);
                let reply = reply.try_recv().unwrap();
                assert_eq!(reply["ok"], json!(true));
                assert_eq!(reply["result"], json!({"added":0,"replaced":1}));
                assert!(h.query_by_label("Held Ink A → process").is_none());
                assert_eq!(
                    h.state().session.active().unwrap().doc.swatches,
                    replaced_expected.doc.swatches,
                    "only A's color type changed; outer names, count, order and other swatch fields are exact"
                );
            } else {
                h.run_steps(1);
            }
            assert!(h.ctx.input(|i| i.pointer.primary_down()
                && !i.pointer.primary_pressed()
                && !i.pointer.primary_released()
                && i.pointer.interact_pos() == Some(blank)
                && !i.events.iter().any(|e| matches!(e, egui::Event::Key { .. }))));
            let label = if replace { "Held Ink B → process" } else { "Held Ink A → process" };
            let n = h.get_by_label(label);
            let current = n.accesskit_node().locate().0;
            let current_rect = n.rect();
            assert_eq!(n.accesskit_node().role(), egui::accesskit::Role::CheckBox);
            assert!(!n.accesskit_node().is_disabled());
            assert_eq!(current_rect.min, rect.min, "replacement occupies the original first ink row");
            assert!(h.ctx.content_rect().contains_rect(current_rect) && !current_rect.contains(blank));
            assert_eq!(h.ctx.layer_id_at(current_rect.center()), Some(origin.layer_id));
            assert!(!h.ctx.interaction_snapshot(|s| s.contains_pointer.iter().any(|id| id.accesskit_id() == current)));
            blank_guard(&h, blank, origin.layer_id);
            let alias = current == id.accesskit_id();
            eprintln!("replace={replace} alias={alias} origin={id:?} current={current:?} blank={blank:?}");
            if alias {
                assert_eq!(h.ctx.dragged_id(), Some(id));
                assert!(h.ctx.read_response(id).unwrap().is_pointer_button_down_on());
            } else {
                assert_ne!(h.ctx.dragged_id().map(|id| id.accesskit_id()), Some(current));
            }
            let expected = if replace { &replaced_expected } else { &origin_expected };
            state(h.state(), expected, &prefs, &ui_before);
            h.run_steps(1);
            state(h.state(), expected, &prefs, &ui_before);
            h.input_mut().events.push(pointer(blank, false));
            h.run_steps(1);
            state(h.state(), expected, &prefs, &ui_before);
            for _ in 0..2 {
                h.run_steps(1);
                assert!(h.query_by_label("Held Ink B → process").is_some());
                assert!(h.query_by_label("Held Ink C → process").is_some());
                state(h.state(), expected, &prefs, &ui_before);
            }
        }
    }

    #[test]
    fn ink_manager_drag_changes_each_ink_once_and_closes_only_on_completed_click() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let names: [String; 2] = [("Drag Ink A", [0, 50, 100, 0]), ("Drag Ink B", [100, 0, 0, 0])].map(|(name, color)| {
            app.run("swatch.create", json!({"name": name, "spot": true, "color": {"c": color[0], "m": color[1], "y": color[2], "k": color[3]}}))
                .unwrap()["name"]
                .as_str()
                .unwrap()
                .to_owned()
        });
        let baseline = app.session.active().unwrap().clone();
        assert_ink_state(&mut app, &baseline, &names, [false; 2], false, 0);
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        crate::theme::apply(&ctx, &Tokens::for_brightness(crate::theme::Brightness::default()));
        for _ in 0..3 {
            ink_frame(&mut app, &ctx, vec![]);
        }
        let labels = open_ink_manager(&mut app, &ctx);
        let row_labels = names.each_ref().map(|name| format!("{name} → process"));
        let mut points = row_labels.each_ref().map(|label| label_center(&labels, label));
        let mut edits = 0;
        for target in [true, false] {
            ink_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(points[0]), pointer(points[0], true)]);
            edits += 1;
            assert_ink_state(&mut app, &baseline, &names, [target, !target], false, edits);
            let labels = ink_frame(&mut app, &ctx, vec![]);
            points = row_labels.each_ref().map(|label| label_center(&labels, label));
            assert_ink_state(&mut app, &baseline, &names, [target, !target], false, edits);
            ink_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(points[1])]);
            edits += 1;
            assert_ink_state(&mut app, &baseline, &names, [target; 2], false, edits);
            for events in [
                vec![egui::Event::PointerMoved(points[0])],
                vec![],
                vec![egui::Event::PointerMoved(points[1])],
                vec![pointer(points[1], false)],
                vec![],
            ] {
                let labels = ink_frame(&mut app, &ctx, events);
                for label in &row_labels {
                    label_center(&labels, label);
                }
                assert_ink_state(&mut app, &baseline, &names, [target; 2], false, edits);
            }
        }
        // An ordinary individual click commits on press and closes only on release.
        ink_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(points[0]), pointer(points[0], true)]);
        edits += 1;
        assert_ink_state(&mut app, &baseline, &names, [true, false], false, edits);
        label_center(&ink_frame(&mut app, &ctx, vec![]), &row_labels[1]);
        ink_frame(&mut app, &ctx, vec![pointer(points[0], false)]);
        let labels = ink_frame(&mut app, &ctx, vec![]);
        assert!(!labels.iter().any(|(text, _)| row_labels.contains(text)), "completed click closes Ink Manager");
        assert_ink_state(&mut app, &baseline, &names, [true, false], false, edits);

        // The global checkbox has the same click lifetime; disabled individual rows do not dispatch.
        let labels = open_ink_manager(&mut app, &ctx);
        let all = label_center(&labels, "All Spots to Process");
        ink_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(all), pointer(all, true)]);
        edits += 1;
        assert_ink_state(&mut app, &baseline, &names, [true, false], true, edits);
        label_center(&ink_frame(&mut app, &ctx, vec![]), &row_labels[1]);
        ink_frame(&mut app, &ctx, vec![pointer(all, false)]);
        let labels = ink_frame(&mut app, &ctx, vec![]);
        assert!(!labels.iter().any(|(text, _)| row_labels.contains(text)));
        assert_ink_state(&mut app, &baseline, &names, [true, false], true, edits);

        let labels = open_ink_manager(&mut app, &ctx);
        points = row_labels.each_ref().map(|label| label_center(&labels, label));
        for events in [
            vec![egui::Event::PointerMoved(points[0]), pointer(points[0], true)],
            vec![egui::Event::PointerMoved(points[1])],
            vec![pointer(points[1], false)],
            vec![],
        ] {
            ink_frame(&mut app, &ctx, events);
            assert_ink_state(&mut app, &baseline, &names, [true, false], true, edits);
        }

        // A resolves to B, but the individual command stores A's raw flag.
        // While B stays opposite to the drag target, repeated held frames must
        // not dispatch the same raw assignment again.
        for target in [false, true] {
            let outside = egui::pos2(900.0, 760.0);
            for pressed in [true, false] {
                ink_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(outside), pointer(outside, pressed)]);
            }
            app.run("ink.options", json!({"allToProcess": false, "ink": names[0], "alias": names[1], "toProcess": !target})).unwrap();
            app.run("ink.options", json!({"ink": names[1], "toProcess": !target})).unwrap();
            let alias_baseline = app.session.active().unwrap().clone();
            assert_resolved_ink_state(&mut app, &alias_baseline, &names, [!target; 2], [!target; 2], false, 0);
            let labels = open_ink_manager(&mut app, &ctx);
            points = row_labels.each_ref().map(|label| label_center(&labels, label));
            ink_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(points[0]), pointer(points[0], true)]);
            assert_resolved_ink_state(&mut app, &alias_baseline, &names, [target, !target], [!target; 2], false, 1);
            for _ in 0..2 {
                let labels = ink_frame(&mut app, &ctx, vec![]);
                label_center(&labels, &row_labels[1]);
                assert_resolved_ink_state(&mut app, &alias_baseline, &names, [target, !target], [!target; 2], false, 1);
            }
            // Crossing B is a second intentional edit; returning to A and
            // releasing adds none. Aliases and allToProcess are never changed.
            ink_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(points[1])]);
            assert_resolved_ink_state(&mut app, &alias_baseline, &names, [target; 2], [target; 2], false, 2);
            for events in [vec![egui::Event::PointerMoved(points[0])], vec![], vec![pointer(points[0], false)], vec![]] {
                ink_frame(&mut app, &ctx, events);
                assert_resolved_ink_state(&mut app, &alias_baseline, &names, [target; 2], [target; 2], false, 2);
            }
        }
    }

    #[test]
    fn ink_manager_completed_short_batches_settle_before_close_and_keep_ordinary_clicks() {
        use egui_kittest::{Harness, kittest::Queryable};
        for reverse in [false, true] {
            for target in [false, true] {
                for neighbor_equal in [false, true] {
                    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
                    app.run("file.new", json!({})).unwrap();
                    let names: [String; 3] =
                        ["Batch Ink A", "Batch Ink B", "Untouched Ink"].map(|name| {
                            app.run("swatch.create", json!({"name": name, "spot": true, "color": {"c": 0, "m": 50, "y": 100, "k": 0}}))
                            .unwrap()["name"].as_str().unwrap().to_owned()
                        });
                    let (origin, neighbor) = if reverse { (1, 0) } else { (0, 1) };
                    let mut initial = [!target; 3];
                    if neighbor_equal {
                        initial[neighbor] = target;
                    }
                    for (name, on) in names.iter().zip(initial) {
                        app.run("ink.options", json!({"ink": name, "toProcess": on})).unwrap();
                    }
                    let baseline = app.session.active().unwrap().clone();
                    let assert_state = |app: &DesignApp, assignments: &[(usize, bool)]| {
                        let st = app.session.active().unwrap();
                        let mut expected = (*baseline.doc).clone();
                        // Literal command order also preserves the raw Vec's insertion order.
                        for &(index, on) in assignments {
                            if on {
                                assert!(!expected.inks.to_process.contains(&names[index]));
                                expected.inks.to_process.push(names[index].clone());
                            } else {
                                let at = expected.inks.to_process.iter().position(|name| name == &names[index]).unwrap();
                                expected.inks.to_process.remove(at);
                            }
                        }
                        let edits = assignments.len();
                        assert_eq!(*st.doc, expected, "aliases, allToProcess, swatches and unrelated content stay exact");
                        assert_eq!(st.selection, baseline.selection);
                        assert_eq!(st.revision, baseline.revision + edits as u64);
                        assert_eq!(st.history.undo.len(), baseline.history.undo.len() + edits);
                        assert_eq!(st.history.redo.len(), baseline.history.redo.len());
                        assert!(st.history.undo[baseline.history.undo.len()..].iter().all(|entry| entry.label == "Ink Manager"));
                    };
                    let mut h = Harness::builder().with_size(vec2(1000.0, 800.0)).build_ui_state(
                        |ui, app: &mut DesignApp| {
                            let key = egui::Id::new("ink-batch-test-fonts");
                            if !ui.data(|d| d.get_temp::<bool>(key).unwrap_or(false)) {
                                crate::theme::install_fonts(ui.ctx(), "en");
                                crate::theme::apply(ui.ctx(), &Tokens::for_brightness(crate::theme::Brightness::default()));
                                ui.data_mut(|d| d.insert_temp(key, true));
                                return;
                            }
                            ui.set_max_width(280.0);
                            show(app, ui);
                        },
                        app,
                    );
                    h.run_steps(3);
                    for label in ["☰", "Ink Manager ⏵"] {
                        h.get_by_label(label).click();
                        h.run_steps(3);
                    }
                    let labels = names.each_ref().map(|name| format!("{name} → process"));
                    // AccessKit bounds are the native Response.rect, not its painted label.
                    let rects = labels.each_ref().map(|label| h.get_by_label(label).rect());
                    let from = rects[origin];
                    let to = rects[neighbor];
                    let x = from.left().max(to.left()) + 1.0;
                    let (p, q) = if reverse {
                        (egui::pos2(x, from.top() + 0.25), egui::pos2(x, to.bottom() - 0.25))
                    } else {
                        (egui::pos2(x, from.bottom() - 0.25), egui::pos2(x, to.top() + 0.25))
                    };
                    assert!(from.contains(p) && to.contains(q) && !from.contains(q));
                    assert!(p.distance(q) > 0.0 && p.distance(q) < 6.0, "real popup edge {p:?} -> {q:?}");
                    // input_mut preserves this exact batch; queued Harness events would
                    // instead split it into separate frames and miss the regression.
                    h.input_mut().events.extend([egui::Event::PointerMoved(p), pointer(p, true), egui::Event::PointerMoved(q), pointer(q, false)]);
                    h.run_steps(1);
                    let mut assignments = vec![(origin, target)];
                    if !reverse && !neighbor_equal {
                        assignments.push((neighbor, target));
                    }
                    assert_state(h.state(), &assignments);
                    for label in &labels {
                        assert!(h.query_by_label(label).is_some());
                    }
                    h.run_steps(1);
                    if reverse && !neighbor_equal {
                        assignments.push((neighbor, target));
                    }
                    assert_eq!(assignments.len(), 1 + usize::from(!neighbor_equal));
                    assert_state(h.state(), &assignments);
                    for label in &labels {
                        assert!(h.query_by_label(label).is_some(), "the endpoint must survive clicked-close through settlement");
                    }
                    h.input_mut().events.push(egui::Event::PointerMoved(rects[2].center()));
                    h.run_steps(1);
                    assert_state(h.state(), &assignments);
                    h.run_steps(2);
                    assert_state(h.state(), &assignments);

                    // Genuine same-origin complete clicks still change once and close.
                    let point = h.get_by_label(&labels[0]).rect().center();
                    h.input_mut().events.extend([egui::Event::PointerMoved(point), pointer(point, true), pointer(point, false)]);
                    h.run_steps(1);
                    assignments.push((0, !target));
                    assert_state(h.state(), &assignments);
                    h.run_steps(1);
                    assert!(h.query_by_label(&labels[0]).is_none());
                    assert_state(h.state(), &assignments);
                    // Keyboard activation is also a completed ordinary click.
                    for label in ["☰", "Ink Manager ⏵"] {
                        h.get_by_label(label).click();
                        h.run_steps(3);
                    }
                    h.get_by_label(&labels[0]).focus();
                    h.run_steps(1);
                    h.key_down(egui::Key::Space);
                    h.run_steps(1);
                    assignments.push((0, target));
                    assert_state(h.state(), &assignments);
                    h.key_up(egui::Key::Space);
                    h.run_steps(1);
                    assert!(h.query_by_label(&labels[0]).is_none());
                    assert_state(h.state(), &assignments);
                }
            }
        }
    }
}

#[cfg(test)]
mod swatches_header_tests {
    use egui_kittest::{Harness, kittest::Queryable};
    use serde_json::json;

    use super::*;

    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        Harness::builder().with_size(vec2(300.0, 520.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        )
    }

    fn items(app: &DesignApp) -> Vec<designcraft_doc::Item> {
        let st = app.session.doc().unwrap();
        st.selection.items.iter().map(|id| st.doc.item(*id).unwrap().clone()).collect()
    }

    #[test]
    fn swatches_go_to_the_fill_or_stroke_in_front_and_the_bottom_row_deletes_the_one_clicked() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "unassigned"})).unwrap();
        app.run("swatch.create", json!({"name": "Brand", "color": "#108080"})).unwrap();
        let mut h = harness(app);
        h.run_steps(3);
        // Fill in front: a swatch is the fill.
        h.get_by_label("C=0 M=100 Y=0 K=0").click();
        h.run_steps(2);
        assert_eq!(items(h.state())[0].fill.swatch, "C=0 M=100 Y=0 K=0");
        // The proxy brings the stroke to the front; the next swatch is the stroke.
        h.get_by_label("Fill").click();
        h.run_steps(2);
        assert!(h.state().ui.stroke_front);
        h.get_by_label("C=100 M=0 Y=0 K=0").click();
        h.run_steps(2);
        let it = &items(h.state())[0];
        assert_eq!((it.fill.swatch.as_str(), it.stroke.swatch.as_str()), ("C=0 M=100 Y=0 K=0", "C=100 M=0 Y=0 K=0"));
        // Formatting affects text, from the panel.
        h.get_by_label("Formatting affects text (J)").click();
        h.run_steps(2);
        assert!(h.state().ui.formatting_affects_text);
        h.get_by_label("Formatting affects container (J)").click();
        h.run_steps(2);
        assert!(!h.state().ui.formatting_affects_text);
        // Delete Swatch removes the swatch last clicked.
        h.get_by_label("Brand").click();
        h.run_steps(2);
        h.get_by_label("Delete Swatch").click();
        h.run_steps(2);
        assert!(h.state().session.doc().unwrap().doc.swatch("Brand").is_none());
    }

    #[test]
    fn the_tint_field_keeps_each_objects_own_swatch() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 100, 100], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        let b = app.run("frame.create", json!({"rect": [136, 36, 200, 100], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        app.run("object.fill", json!({"ids": [a], "swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
        app.run("object.fill", json!({"ids": [b], "swatch": "C=0 M=100 Y=0 K=0"})).unwrap();
        app.run("object.stroke", json!({"ids": [a, b], "swatch": "[Black]"})).unwrap();
        app.run("selection.set", json!({"ids": [a, b]})).unwrap();
        let mut h = harness(app);
        h.run_steps(3);
        // The field shows the first selected object's fill tint.
        assert!(h.query_all_by_value("100").next().is_some());
        set_tint(h.state_mut(), 40.0).unwrap();
        h.run_steps(2);
        assert!(h.query_all_by_value("40").next().is_some(), "the field follows");
        // The stroke in front: the strokes' tints.
        h.state_mut().ui.stroke_front = true;
        set_tint(h.state_mut(), 70.0).unwrap();
        assert!(items(h.state()).iter().all(|i| i.stroke.tint == 0.7));
        let it = items(h.state());
        assert_eq!((it[0].fill.swatch.as_str(), it[0].fill.tint), ("C=100 M=0 Y=0 K=0", 0.4));
        assert_eq!((it[1].fill.swatch.as_str(), it[1].fill.tint), ("C=0 M=100 Y=0 K=0", 0.4));
    }
}
