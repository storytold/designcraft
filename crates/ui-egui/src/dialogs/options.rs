//! Swatch Options, Color Group Options, Layer Options and Layout › Ruler Guides.

use designcraft_color::{Color, ColorType, SwatchValue};
use serde_json::{Map, Value, json};

use super::{Dialog, check, text_field_named};
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
        Color::Cmyk { c, m, y, k } => json!({"model": "cmyk", "c": c, "m": m, "y": y, "k": k}),
        Color::Rgb { r, g, b } => json!([r, g, b]),
        Color::Gray { k } => json!({"gray": k}),
    }
}

fn caption(ui: &mut egui::Ui, lang: &str, s: &str) {
    crate::rtl::label(ui, tr(lang, s));
}

/// Render a labelled, bounded colour-channel spinner, name it for accessibility and write changed values to dialog fields.
fn channel(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str, max: f64, suffix: &str) {
    ui.label(egui::RichText::new(label).monospace());
    let mut v = d.n(key).unwrap_or(0.0);
    let resp = ui.add(egui::DragValue::new(&mut v).range(0.0..=max).speed(0.5).max_decimals(1).suffix(suffix));
    let changed = resp.changed();
    // The channel name is a painted label beside a bare spinner: name the spinner too.
    let _ = resp.accessible_name(label.to_owned());
    if changed {
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
        ui.add_enabled_ui(!d.b("locked") && !auto, |ui| text_field_named(ui, d, "swatchName", 220.0, tr(lang, "Swatch Name")));
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
                    let combo = craft_ui::ComboBox::from_id_salt("swatch_color_type")
                        .selected_text(crate::rtl::widget(ui, tr(lang, if cur == "spot" { "Spot" } else { "Process" })))
                        .show_ui(ui, |ui| {
                            for (v, l) in [("process", "Process"), ("spot", "Spot")] {
                                if ui.selectable_label(cur == v, crate::rtl::widget(ui, tr(lang, l))).clicked() {
                                    d.fields.insert("colorType".into(), json!(v));
                                }
                            }
                        });
                    let _ = crate::widgets::name_combo(combo, tr(lang, "Color Type"));
                    ui.end_row();
                }
                caption(ui, lang, "Color Mode:");
                let cur = d.s("mode");
                let label = |m: &str| match m {
                    "rgb" => "RGB",
                    "gray" => "Gray",
                    _ => "CMYK",
                };
                let combo = craft_ui::ComboBox::from_id_salt("swatch_color_mode")
                    .selected_text(crate::rtl::widget(ui, tr(lang, label(&cur))))
                    .show_ui(ui, |ui| {
                        for m in ["cmyk", "rgb", "gray"] {
                            if ui.selectable_label(cur == m, crate::rtl::widget(ui, tr(lang, label(m)))).clicked()
                                && cur != m
                                && let Some(c) = dialog_color(&d.fields)
                            {
                                set_color(&mut d.fields, c, Some(m));
                            }
                        }
                    });
                let _ = crate::widgets::name_combo(combo, tr(lang, "Color Mode"));
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
                if ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix("%").integer()).accessible_name(tr(lang, "Tint")).changed() {
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
        text_field_named(ui, d, "groupName", 220.0, tr(&app.ui.language, "Name"));
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

/// Draws Layer Options: name, colour (any of the UI colours or custom) and the layer's flags.
pub fn layer_options(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    egui::Grid::new("layer_options").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        caption(ui, lang, "Name:");
        text_field_named(ui, d, "name", 220.0, tr(lang, "Name"));
        ui.end_row();
        caption(ui, lang, "Color:");
        ui_color_choice(ui, d, lang, "layer_color");
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

/// Applies Layer Options through `layer.set`.
pub fn confirm_layer_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let mut p = json!({"id": d.fields.get("id").cloned().unwrap_or(Value::Null), "name": d.s("name"), "color": d.fields.get("rgb").cloned().unwrap_or(Value::Null)});
    for k in ["visible", "locked", "printable", "showGuides", "suppressWrapWhenHidden"] {
        p[k] = json!(d.b(k));
    }
    app.run("layer.set", p)
}

/// The UI colour list of Layer Options and Ruler Guides, beside a custom colour button: the
/// named colours, each with its swatch, and Custom for any other. The colour is the dialog's
/// `rgb` ([r, g, b]); without one (guides of several colours) the list shows none.
fn ui_color_choice(ui: &mut egui::Ui, d: &mut Dialog, lang: &str, salt: &str) {
    let rgb: Option<[u8; 3]> = d.fields.get("rgb").and_then(|v| serde_json::from_value(v.clone()).ok());
    let named = rgb.and_then(|rgb| designcraft_doc::LAYER_COLORS.iter().find(|(_, c)| *c == rgb).map(|(n, _)| *n));
    let shown = match (rgb, named) {
        (None, _) => "",
        (Some(_), Some(n)) => n,
        (Some(_), None) => "Custom",
    };
    ui.horizontal(|ui| {
        let combo = craft_ui::ComboBox::from_id_salt(salt).selected_text(crate::rtl::widget(ui, tr(lang, shown))).width(140.0).show_ui(ui, |ui| {
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
        let _ = crate::widgets::name_combo(combo, tr(lang, "Color"));
        super::color_field(ui, d, "rgb");
    });
}

/// The Ruler Guides dialog's id.
pub const RULER_GUIDES: &str = "rulerGuides";

/// Layout › Ruler Guides…: the colour and view threshold of the selected guides, or, with none
/// selected, those new guides get. An option the selected guides don't share starts blank and
/// stays as each guide has it unless it is changed.
pub fn open_ruler_guides(app: &mut DesignApp) -> Result<(), String> {
    let st = app.session.active().ok_or("no document open")?;
    let doc = &st.doc;
    let looks: Vec<designcraft_doc::RulerGuideOptions> =
        st.selection.guides.iter().filter_map(|g| doc.spread(g.spread)?.pages.get(g.page)?.guides.get(g.index).map(|g| g.options)).collect();
    let defaults = looks.is_empty();
    let looks = if defaults { vec![doc.settings.ruler_guides] } else { looks };
    // A value all of them share, or null.
    let shared = |f: &dyn Fn(&designcraft_doc::RulerGuideOptions) -> Value| {
        let first = looks.first().map(f).unwrap_or(Value::Null);
        if looks.iter().all(|o| f(o) == first) { first } else { Value::Null }
    };
    let threshold = shared(&|o| json!(threshold_text(o.view_threshold))).as_str().unwrap_or_default().to_string();
    let rgb = shared(&|o| json!(o.rgb()));
    let f = json!({"threshold": threshold, "threshold0": threshold, "rgb": rgb, "rgb0": rgb, "defaults": defaults});
    app.ui.dialog = Some(Dialog::new(RULER_GUIDES, f));
    Ok(())
}

/// A view threshold (1.0 = 100%) as the dialog shows it: "150%".
fn threshold_text(t: f64) -> String {
    format!("{}%", (t * 10_000.0).round() / 100.0)
}

/// Ruler Guides, laid out as InDesign's: View Threshold, then Color.
pub fn ruler_guides(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    egui::Grid::new("ruler_guides").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        caption(ui, lang, "View Threshold:");
        text_field_named(ui, d, "threshold", 80.0, tr(lang, "View Threshold"));
        ui.end_row();
        caption(ui, lang, "Color:");
        ui_color_choice(ui, d, lang, "ruler_guide_color");
        ui.end_row();
    });
    status(ui, d);
}

/// OK: `guide.options` with what the dialog changed. A threshold is a percent, "%" optional.
pub fn confirm_ruler_guides(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let mut p = json!({});
    let threshold = d.s("threshold");
    if threshold != d.s("threshold0") {
        // Checked here, so that a number out of range gets the message in the interface language.
        let in_range = |v: &f64| designcraft_doc::RulerGuideOptions::threshold_of_percent(*v).is_some();
        let percent = threshold.trim().trim_end_matches('%').trim().parse::<f64>().ok().filter(in_range);
        let bad = || tr(&app.ui.language, "View Threshold must be a percentage from 5% to 4000%.").to_string();
        p["viewThreshold"] = json!(percent.ok_or_else(bad)?);
    }
    if d.fields.get("rgb") != d.fields.get("rgb0")
        && let Some(rgb) = d.fields.get("rgb").filter(|v| !v.is_null())
    {
        p["color"] = rgb.clone();
    }
    if d.b("defaults") {
        p["defaults"] = json!(true);
    }
    app.run("guide.options", p)
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
                let id = egui::Id::unique("test_fonts");
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
        // A swatch row's accessible name is its name, then what its marks show.
        let at = h.get_by(|n| n.label().is_some_and(|l| crate::panels::swatches::names_swatch(&l, label))).rect().center();
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

    /// Layout › Ruler Guides… on two selected guides of different colours: the colour starts
    /// blank, a new threshold reaches both, and each keeps its own colour.
    #[test]
    fn ruler_guides_sets_the_threshold_of_the_selected_guides_and_keeps_their_colours() {
        let mut app = app();
        app.run("guide.add", json!({"orientation": "vertical", "position": 100.0, "page": 0})).unwrap();
        app.run("guide.add", json!({"orientation": "horizontal", "position": 200.0, "page": 0})).unwrap();
        app.run("guide.options", json!({"color": "Red"})).unwrap();
        app.run("guide.select", json!({"guides": [{"page": 0, "index": 0}, {"page": 0, "index": 1}]})).unwrap();
        let mut h = harness(app, |_, _| {});
        crate::menus::activate(h.state_mut(), "guide.options", &serde_json::Value::Null);
        h.run_steps(3);
        let d = h.state().ui.dialog.clone().expect("Ruler Guides is open");
        assert_eq!(d.id, super::RULER_GUIDES);
        assert_eq!((d.s("threshold"), d.fields["rgb"].clone(), d.b("defaults")), ("5%".to_string(), serde_json::Value::Null, false));
        retype(&mut h, "5%", "250%");
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        assert!(h.state().ui.dialog.is_none(), "{:?}", h.state().ui.dialog);
        let doc = &h.state().session.active().unwrap().doc;
        let looks: Vec<_> = doc.spreads[0].pages[0].guides.iter().map(|g| g.options).collect();
        assert_eq!(looks.iter().map(|o| o.view_threshold).collect::<Vec<_>>(), [2.5, 2.5]);
        assert_eq!(looks.iter().map(|o| o.color).collect::<Vec<_>>(), [None, Some([255, 0, 0])]);
    }

    /// With no guide selected the dialog shows and sets what new guides get; a threshold out of
    /// range keeps it open with the reason and changes nothing.
    #[test]
    fn ruler_guides_with_no_guide_selected_sets_the_defaults_for_new_guides() {
        let mut h = harness(app(), |_, _| {});
        crate::menus::activate(h.state_mut(), "guide.options", &serde_json::Value::Null);
        h.run_steps(3);
        let d = h.state().ui.dialog.clone().expect("Ruler Guides is open");
        assert_eq!((d.s("threshold"), d.fields["rgb"].clone(), d.b("defaults")), ("5%".to_string(), json!([0, 255, 255]), true));
        retype(&mut h, "5%", "4001");
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        let d = h.state().ui.dialog.clone().expect("a bad threshold keeps the dialog open");
        assert_eq!(d.s("status"), "View Threshold must be a percentage from 5% to 4000%.", "the range message, not the command's error");
        let defaults = |h: &Harness<'static, DesignApp>| h.state().session.active().unwrap().doc.settings.ruler_guides;
        assert!(defaults(&h).is_default());
        // The colour list's own choice, then a threshold in range.
        h.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Color").click();
        h.run_steps(2);
        h.get_by_label("■ Magenta").click();
        h.run_steps(2);
        retype(&mut h, "4001", "75");
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        assert!(h.state().ui.dialog.is_none(), "{:?}", h.state().ui.dialog);
        assert_eq!(defaults(&h), designcraft_doc::RulerGuideOptions { color: Some([255, 79, 255]), view_threshold: 0.75 });
    }

    #[test]
    fn double_clicking_a_layer_opens_layer_options_and_ok_renames_it() {
        let mut app = app();
        app.run("layer.new", json!({"name": "Art"})).unwrap();
        let mut h = harness(app, crate::panels::layers::show);
        double_click(&mut h, "Art");
        let names: Vec<&str> = h.state().session.active().unwrap().doc.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Art", "Layer 1"], "a double-click is not a drag");
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
