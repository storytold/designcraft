//! Swatch Options, Color Group Options and Layer Options.

use designcraft_color::{Color, ColorType, SwatchValue};
use serde_json::{Map, Value, json};

use super::{Dialog, check, text_field};
use crate::DesignApp;
use crate::i18n::tr;

/// Swatch Options for `name`: its name, and what its kind lets the user change.
pub fn open_swatch_options(app: &mut DesignApp, name: &str) -> Result<(), String> {
    let doc = &app.session.active().ok_or("no document open")?.doc;
    let w = doc.swatch(name).ok_or_else(|| format!("no swatch `{name}`"))?;
    let mut f = Map::new();
    f.insert("name".into(), json!(name));
    f.insert("swatchName".into(), json!(name));
    f.insert("locked".into(), json!(w.locked));
    let kind = match &w.value {
        SwatchValue::Color { color, color_type } if !w.locked => {
            f.insert("nameWithValue".into(), json!(!w.named));
            f.insert("colorType".into(), json!(if *color_type == ColorType::Spot { "spot" } else { "process" }));
            set_color(&mut f, *color, None);
            "color"
        }
        SwatchValue::Paper { color } => {
            set_color(&mut f, *color, None);
            "paper"
        }
        SwatchValue::Tint { base, tint } => {
            f.insert("base".into(), json!(base));
            f.insert("tint".into(), json!((f64::from(*tint) * 100.0).round()));
            "tint"
        }
        SwatchValue::Gradient { .. } => "gradient",
        _ => "fixed",
    };
    f.insert("kind".into(), json!(kind));
    if let Some(c) = dialog_color(&f) {
        f.insert("origColor".into(), color_json(c));
    }
    app.ui.dialog = Some(Dialog::new("swatchOptions", Value::Object(f)));
    Ok(())
}

/// Write `c`'s channels in `mode` (default: the colour's own model).
fn set_color(f: &mut Map<String, Value>, c: Color, mode: Option<&str>) {
    let mode = mode.unwrap_or(match c {
        Color::Cmyk { .. } => "cmyk",
        Color::Rgb { .. } => "rgb",
        Color::Gray { .. } => "gray",
    });
    f.insert("mode".into(), json!(mode));
    match mode {
        "rgb" => {
            for (k, v) in ["r", "g", "b"].iter().zip(c.to_rgb()) {
                f.insert((*k).into(), json!((f64::from(v) * 255.0).round()));
            }
        }
        "gray" => {
            let k = match c {
                Color::Gray { k } => k,
                other => other.to_cmyk()[3],
            };
            f.insert("gray".into(), json!((f64::from(k) * 1000.0).round() / 10.0));
        }
        _ => {
            for (k, v) in ["c", "m", "y", "k"].iter().zip(c.to_cmyk()) {
                f.insert((*k).into(), json!((f64::from(v) * 1000.0).round() / 10.0));
            }
        }
    }
}

/// The colour the dialog's channels describe.
fn dialog_color(f: &Map<String, Value>) -> Option<Color> {
    let n = |k: &str, max: f64| f.get(k).and_then(Value::as_f64).map(|v| (v.clamp(0.0, max) / max) as f32);
    match f.get("mode").and_then(Value::as_str)? {
        "rgb" => Some(Color::rgb(n("r", 255.0)?, n("g", 255.0)?, n("b", 255.0)?)),
        "gray" => Some(Color::gray(n("gray", 100.0)?)),
        _ => Some(Color::cmyk(n("c", 100.0)?, n("m", 100.0)?, n("y", 100.0)?, n("k", 100.0)?)),
    }
}

/// A colour as `swatch.options` takes it (fractions, so 1% isn't read as 100%).
fn color_json(c: Color) -> Value {
    match c {
        Color::Cmyk { c, m, y, k } => json!({"c": c, "m": m, "y": y, "k": k}),
        Color::Rgb { r, g, b } => json!([r, g, b]),
        Color::Gray { k } => json!({"gray": k}),
    }
}

fn caption(ui: &mut egui::Ui, lang: &str, s: &str) {
    crate::rtl::label(ui, tr(lang, s));
}

fn channel(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str, max: f64, suffix: &str) {
    ui.label(egui::RichText::new(label).monospace());
    let mut v = d.n(key).unwrap_or(0.0);
    if ui.add(egui::DragValue::new(&mut v).range(0.0..=max).speed(0.5).max_decimals(1).suffix(suffix)).changed() {
        d.fields.insert(key.into(), json!(v));
    }
    ui.end_row();
}

pub fn swatch_options(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let kind = d.s("kind");
    let auto = kind == "color" && d.b("nameWithValue");
    if auto && let Some(c) = dialog_color(&d.fields) {
        d.fields.insert("swatchName".into(), json!(designcraft_color::swatch::value_name(c)));
    }
    egui::Grid::new("swatch_options").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        caption(ui, lang, "Swatch Name:");
        ui.add_enabled_ui(!d.b("locked") && !auto, |ui| text_field(ui, d, "swatchName", 220.0));
        ui.end_row();
    });
    if kind == "color" {
        check(ui, d, "nameWithValue", tr(lang, "Name with Color Value"));
    }
    ui.add_space(6.0);
    match kind.as_str() {
        "color" | "paper" => {
            egui::Grid::new("swatch_color").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                if kind == "color" {
                    caption(ui, lang, "Color Type:");
                    let cur = d.s("colorType");
                    egui::ComboBox::from_id_salt("swatch_color_type")
                        .selected_text(crate::rtl::widget(ui, tr(lang, if cur == "spot" { "Spot" } else { "Process" })))
                        .show_ui(ui, |ui| {
                            for (v, l) in [("process", "Process"), ("spot", "Spot")] {
                                if ui.selectable_label(cur == v, crate::rtl::widget(ui, tr(lang, l))).clicked() {
                                    d.fields.insert("colorType".into(), json!(v));
                                }
                            }
                        });
                    ui.end_row();
                }
                caption(ui, lang, "Color Mode:");
                let cur = d.s("mode");
                let label = |m: &str| match m {
                    "rgb" => "RGB",
                    "gray" => "Gray",
                    _ => "CMYK",
                };
                egui::ComboBox::from_id_salt("swatch_color_mode").selected_text(crate::rtl::widget(ui, tr(lang, label(&cur)))).show_ui(ui, |ui| {
                    for m in ["cmyk", "rgb", "gray"] {
                        if ui.selectable_label(cur == m, crate::rtl::widget(ui, tr(lang, label(m)))).clicked()
                            && cur != m
                            && let Some(c) = dialog_color(&d.fields)
                        {
                            set_color(&mut d.fields, c, Some(m));
                        }
                    }
                });
                ui.end_row();
                match d.s("mode").as_str() {
                    "rgb" => {
                        for k in ["r", "g", "b"] {
                            channel(ui, d, k, &k.to_uppercase(), 255.0, "");
                        }
                    }
                    "gray" => channel(ui, d, "gray", "K", 100.0, "%"),
                    _ => {
                        for k in ["c", "m", "y", "k"] {
                            channel(ui, d, k, &k.to_uppercase(), 100.0, "%");
                        }
                    }
                }
            });
            if let Some(c) = dialog_color(&d.fields) {
                let [r, g, b] = c.to_rgb();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 24.0), egui::Sense::hover());
                let col = egui::Color32::from_rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8);
                ui.painter().rect_filled(rect, 2.0, col);
            }
        }
        "tint" => {
            egui::Grid::new("swatch_tint").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                caption(ui, lang, "Base Color:");
                ui.label(d.s("base"));
                ui.end_row();
                caption(ui, lang, "Tint:");
                let mut v = d.n("tint").unwrap_or(100.0);
                if ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix("%").integer()).changed() {
                    d.fields.insert("tint".into(), json!(v));
                }
                ui.end_row();
            });
        }
        "gradient" => caption(ui, lang, "Edit the gradient's stops in the Gradient panel."),
        _ => caption(ui, lang, "This swatch can't be edited."),
    }
    status(ui, d);
}

/// A failed OK's reason.
fn status(ui: &mut egui::Ui, d: &Dialog) {
    let s = d.s("status");
    if !s.is_empty() {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(s).color(crate::theme::Tokens::get(ui.ctx()).text_strong));
    }
}

pub fn confirm_swatch_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let kind = d.s("kind");
    let mut p = json!({"name": d.s("name")});
    if !d.b("locked") {
        p["to"] = json!(d.s("swatchName").trim());
    }
    if let Some(c) = dialog_color(&d.fields).map(color_json)
        && d.fields.get("origColor") != Some(&c)
    {
        p["color"] = c;
    }
    match kind.as_str() {
        "color" => {
            p["nameWithValue"] = json!(d.b("nameWithValue"));
            p["spot"] = json!(d.s("colorType") == "spot");
        }
        "tint" => p["tint"] = json!((d.n("tint").unwrap_or(100.0) / 100.0).clamp(0.0, 1.0)),
        "paper" => {}
        "gradient" => {}
        _ => return Ok(Value::Null),
    }
    app.run("swatch.options", p)
}

pub fn open_color_group_options(app: &mut DesignApp, name: &str) -> Result<(), String> {
    let doc = &app.session.active().ok_or("no document open")?.doc;
    if !doc.color_groups.iter().any(|g| g.name == name) {
        return Err(format!("no color group `{name}`"));
    }
    app.ui.dialog = Some(Dialog::new("colorGroupOptions", json!({"name": name, "groupName": name})));
    Ok(())
}

pub fn color_group_options(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        caption(ui, &app.ui.language, "Name:");
        text_field(ui, d, "groupName", 220.0);
    });
    status(ui, d);
}

pub fn confirm_color_group_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let (name, to) = (d.s("name"), d.s("groupName").trim().to_string());
    if to == name {
        return Ok(Value::Null);
    }
    app.run("swatch.renameColorGroup", json!({"name": name, "to": to}))
}

/// Layer Options for layer `id` (default: the active layer).
pub fn open_layer_options(app: &mut DesignApp, id: Option<u64>) -> Result<(), String> {
    let st = app.session.active().ok_or("no document open")?;
    let id = id.map_or(st.active_layer, designcraft_doc::LayerId);
    let l = st.doc.layer(id).ok_or("no such layer")?;
    let f = json!({"id": l.id.0, "name": l.name, "rgb": l.color, "visible": l.visible, "locked": l.locked, "printable": l.printable,
        "showGuides": l.show_guides, "suppressWrapWhenHidden": l.suppress_wrap_when_hidden});
    app.ui.dialog = Some(Dialog::new("layerSettings", f));
    Ok(())
}

pub fn layer_options(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let rgb: [u8; 3] = d.fields.get("rgb").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or([0, 0, 0]);
    let named = designcraft_doc::LAYER_COLORS.iter().find(|(_, c)| *c == rgb).map(|(n, _)| *n);
    egui::Grid::new("layer_options").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        caption(ui, lang, "Name:");
        text_field(ui, d, "name", 220.0);
        ui.end_row();
        caption(ui, lang, "Color:");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("layer_color")
                .selected_text(crate::rtl::widget(ui, tr(lang, named.unwrap_or("Custom"))))
                .width(140.0)
                .show_ui(ui, |ui| {
                    for (n, c) in designcraft_doc::LAYER_COLORS {
                        let swatch = egui::RichText::new("■ ").color(egui::Color32::from_rgb(c[0], c[1], c[2]));
                        let mut job = egui::text::LayoutJob::default();
                        swatch.append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        egui::RichText::new(tr(lang, n)).append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        if ui.selectable_label(named == Some(*n), job).clicked() {
                            d.fields.insert("rgb".into(), json!(c));
                        }
                    }
                });
            super::color_field(ui, d, "rgb");
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    egui::Grid::new("layer_flags").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
        check(ui, d, "visible", tr(lang, "Show Layer"));
        check(ui, d, "locked", tr(lang, "Lock Layer"));
        ui.end_row();
        check(ui, d, "showGuides", tr(lang, "Show Guides"));
        check(ui, d, "printable", tr(lang, "Print Layer"));
        ui.end_row();
    });
    check(ui, d, "suppressWrapWhenHidden", tr(lang, "Suppress Text Wrap When Layer is Hidden"));
    status(ui, d);
}

pub fn confirm_layer_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let mut p = json!({"id": d.fields.get("id").cloned().unwrap_or(Value::Null), "name": d.s("name"), "color": d.fields.get("rgb").cloned().unwrap_or(Value::Null)});
    for k in ["visible", "locked", "printable", "showGuides", "suppressWrapWhenHidden"] {
        p[k] = json!(d.b(k));
    }
    app.run("layer.set", p)
}

#[cfg(test)]
mod tests {
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::json;

    use crate::DesignApp;

    /// `panel` and the dialogs, in a small window.
    fn harness(app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui)) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(vec2(360.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                // The fonts are installed before the first frame that draws.
                let id = egui::Id::new("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                panel(app, ui);
                crate::dialogs::show(app, &ui.ctx().clone());
            },
            app,
        );
        h.run_steps(3);
        h
    }

    fn double_click(h: &mut Harness<'static, DesignApp>, label: &str) {
        let at = h.get_by_label(label).rect().center();
        h.hover_at(at);
        // Two clicks without the pointer leaving (`drop_at` would end the double click).
        for pressed in [true, false, true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.run_steps(3);
    }

    /// Replace the text of the dialog field showing `old` with `new`.
    fn retype(h: &mut Harness<'static, DesignApp>, old: &str, new: &str) {
        let field = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some(old)).rect().center();
        h.hover_at(field);
        h.drag_at(field);
        h.drop_at(field);
        h.run_steps(2);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text(new.into()));
        h.run_steps(2);
    }

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app
    }

    #[test]
    fn double_clicking_a_swatch_opens_swatch_options_and_ok_renames_it() {
        let mut app = app();
        app.run("swatch.create", json!({"name": "Brand", "color": {"c": 0, "m": 80, "y": 100, "k": 0}})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 72, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("object.fill", json!({"swatch": "Brand", "ids": [id]})).unwrap();
        let mut h = harness(app, crate::panels::swatches::show);
        double_click(&mut h, "Brand");
        let d = h.state().ui.dialog.clone().expect("Swatch Options is open");
        assert_eq!(d.id, "swatchOptions");
        assert_eq!(d.s("swatchName"), "Brand");
        retype(&mut h, "Brand", "Signal Orange");
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        assert!(h.state().ui.dialog.is_none(), "{:?}", h.state().ui.dialog);
        let doc = &h.state().session.active().unwrap().doc;
        assert!(doc.swatch("Signal Orange").is_some() && doc.swatch("Brand").is_none());
        assert_eq!(doc.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, "Signal Orange");
    }

    #[test]
    fn a_system_swatch_name_cant_be_edited() {
        let mut h = harness(app(), crate::panels::swatches::show);
        double_click(&mut h, "[Black]");
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("swatchOptions"));
        let field = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("[Black]"));
        assert!(field.accesskit_node().is_disabled(), "the name field is disabled");
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        assert!(h.state().session.active().unwrap().doc.swatch("[Black]").is_some());
    }

    #[test]
    fn double_clicking_a_layer_opens_layer_options_and_ok_renames_it() {
        let mut app = app();
        app.run("layer.new", json!({"name": "Art"})).unwrap();
        let mut h = harness(app, crate::panels::layers::show);
        double_click(&mut h, "Art");
        let d = h.state().ui.dialog.clone().expect("Layer Options is open");
        assert_eq!(d.id, "layerSettings");
        assert_eq!(d.s("name"), "Art");
        retype(&mut h, "Art", "Artwork");
        h.get_by_label("Print Layer").click();
        h.run_steps(2);
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        assert!(h.state().ui.dialog.is_none(), "{:?}", h.state().ui.dialog);
        let doc = &h.state().session.active().unwrap().doc;
        let l = doc.layers.iter().find(|l| l.name == "Artwork").expect("renamed");
        assert!(!l.printable);
    }
}
