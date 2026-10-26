//! Object › Effects (and the Effects panel's fx button): InDesign's Effects dialog. Transparency
//! and every effect in one window: the list on the left (a checkbox turns an effect on, a click on
//! its name shows its settings and turns it on), the settings on the right in the document's
//! units, Preview on the page while it is open, and OK as one undo step (`object.effects`).

use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;
use crate::theme::semibold;

/// The dialog's id.
pub const ID: &str = "effects";
/// The params a previewing dialog shows on the page (its interaction is open while it is set).
const PREVIEWED: &str = "_previewed";

/// The dialog's pages: (key in `Effects` JSON, label). Transparency is the object's own.
pub const PAGES: [(&str, &str); 10] = [
    ("transparency", "Transparency"),
    ("dropShadow", "Drop Shadow"),
    ("innerShadow", "Inner Shadow"),
    ("outerGlow", "Outer Glow"),
    ("innerGlow", "Inner Glow"),
    ("bevel", "Bevel and Emboss"),
    ("satin", "Satin"),
    ("feather", "Basic Feather"),
    ("directionalFeather", "Directional Feather"),
    ("gradientFeather", "Gradient Feather"),
];

/// The page an effect command opens (Object › Effects › Drop Shadow…, its shortcut).
pub fn page_of_command(id: &str) -> Option<&'static str> {
    Some(match id {
        "object.dropShadow" => "dropShadow",
        "object.innerShadow" => "innerShadow",
        "object.outerGlow" => "outerGlow",
        "object.innerGlow" => "innerGlow",
        "object.bevel" => "bevel",
        "object.satin" => "satin",
        "object.feather" => "feather",
        "object.directionalFeather" => "directionalFeather",
        "object.gradientFeather" => "gradientFeather",
        _ => return None,
    })
}

/// Open the dialog on `page` for the selected objects, the page's effect turned on (as InDesign
/// does when an effect is chosen from a menu). Nothing selected: a status message.
pub fn open(app: &mut DesignApp, page: &str) {
    let Some(st) = app.session.active() else { return };
    let ids: Vec<u64> = st.selection.items.iter().map(|i| i.0).collect();
    let Some(it) = st.selection.items.first().and_then(|i| st.doc.item(*i)) else {
        app.status(crate::i18n::tr(&app.ui.language, "Select an object.").to_string());
        return;
    };
    let mut fx = effects_json(&it.effects);
    let fields = json!({"ids": ids, "fx": Value::Null, "opacity": it.opacity, "blend": it.blend.label(), "preview": true, "page": "transparency"});
    let mut d = Dialog::new(ID, fields);
    let page = PAGES.iter().find(|p| p.0 == page).map_or("transparency", |p| p.0);
    turn_on(&mut fx, page);
    d.fields.insert("fx".into(), fx);
    d.fields.insert("page".into(), json!(page));
    app.ui.dialog = Some(d);
}

/// Every effect's settings, those still at their defaults included (the document leaves them out).
fn effects_json(e: &designcraft_doc::Effects) -> Value {
    use designcraft_doc::{Bevel, DirectionalFeather, DropShadow, FeatherOptions, GradientFeather, InnerGlow, InnerShadow, OuterGlow, Satin};
    let mut fx = serde_json::to_value(e).unwrap_or_else(|_| json!({}));
    let defaults = [
        ("dropShadow", serde_json::to_value(DropShadow::default())),
        ("innerShadow", serde_json::to_value(InnerShadow::default())),
        ("outerGlow", serde_json::to_value(OuterGlow::default())),
        ("innerGlow", serde_json::to_value(InnerGlow::default())),
        ("bevel", serde_json::to_value(Bevel::default())),
        ("satin", serde_json::to_value(Satin::default())),
        ("directionalFeather", serde_json::to_value(DirectionalFeather::default())),
        ("gradientFeather", serde_json::to_value(GradientFeather::default())),
        ("featherOptions", serde_json::to_value(FeatherOptions::default())),
    ];
    for (k, v) in defaults {
        if !fx[k].is_object()
            && let Ok(v) = v
        {
            fx[k] = v;
        }
    }
    fx
}

/// Turn the effect of `page` on, as choosing it does (Basic Feather: its default width).
fn turn_on(fx: &mut Value, page: &str) {
    match page {
        "transparency" => {}
        "feather" => {
            if fx["feather"].as_f64().unwrap_or(0.0) <= 0.0 {
                fx["feather"] = json!(9.0);
            }
        }
        _ => {
            if fx[page].is_object() {
                fx[page]["on"] = json!(true);
            }
        }
    }
}

fn is_on(fx: &Value, page: &str) -> bool {
    match page {
        "transparency" => true,
        "feather" => fx["feather"].as_f64().unwrap_or(0.0) > 0.0,
        _ => fx[page]["on"].as_bool().unwrap_or(false),
    }
}

/// The `object.effects` params the dialog stands for.
pub fn params(d: &Dialog) -> Value {
    let mut p = json!({"ids": d.fields.get("ids").cloned().unwrap_or(json!([])), "effects": d.fields.get("fx").cloned().unwrap_or(json!({})),
        "opacity": d.fields.get("opacity").and_then(Value::as_f64).unwrap_or(1.0), "blend": d.fields.get("blend").and_then(Value::as_str).unwrap_or("Normal")});
    if let Some(a) = d.fields.get(GLOBAL_LIGHT).and_then(Value::as_f64) {
        p["globalLight"] = json!(a);
    }
    p
}

/// The global light angle as changed in the dialog (an Angle field with Use Global Light on).
const GLOBAL_LIGHT: &str = "globalLight";

/// Apply: the preview goes and the settings are made once, as one undo step.
pub fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    if d.fields.contains_key(PREVIEWED) {
        app.session.cancel_interaction();
    }
    app.run("object.effects", params(d))
}

/// Preview on: the effects show on the page (redone when a value changes); off: they go.
fn preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.fields.get("preview").and_then(Value::as_bool).unwrap_or(false) {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
        }
        return;
    }
    let p = params(d);
    if shown.as_ref() == Some(&p) {
        return;
    }
    if shown.is_none() && app.session.begin_interaction("Effects").is_err() {
        return;
    }
    match app.session.preview("object.effects", p.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), p);
}

/// A number of `fx` at `path` (`["dropShadow", "distance"]`).
fn num(d: &Dialog, path: &[&str]) -> f64 {
    let mut v = d.fields.get("fx").unwrap_or(&Value::Null);
    for k in path {
        v = &v[*k];
    }
    v.as_f64().unwrap_or(0.0)
}

fn set(d: &mut Dialog, path: &[&str], value: Value) {
    let Some((last, parents)) = path.split_last() else { return };
    let Some(mut v) = d.fields.get_mut("fx") else { return };
    for k in parents {
        if !v[*k].is_object() {
            v[*k] = json!({});
        }
        v = &mut v[*k];
    }
    v[*last] = value;
}

/// A distance in the document's unit (stored in points).
fn measure(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str], unit: Unit) {
    let mut v = num(d, path);
    let w = egui::DragValue::new(&mut v)
        .range(0.0..=10_000.0)
        .speed(0.25)
        .custom_formatter(move |n, _| format_measure(n, unit))
        .custom_parser(move |s| parse_measure(s, unit).ok());
    if ui.add_sized([90.0, 20.0], w).changed() {
        set(d, path, json!(v));
    }
}

/// An opacity (0–1, shown as %).
fn percent_of_one(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str]) {
    let mut v = num(d, path) * 100.0;
    if ui.add_sized([70.0, 20.0], egui::DragValue::new(&mut v).range(0.0..=100.0).suffix("%").max_decimals(0)).changed() {
        set(d, path, json!(v / 100.0));
    }
}

/// A percentage (0–100).
fn percent(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str]) {
    let mut v = num(d, path);
    if ui.add_sized([70.0, 20.0], egui::DragValue::new(&mut v).range(0.0..=100.0).suffix("%").max_decimals(0)).changed() {
        set(d, path, json!(v));
    }
}

/// The effect's own angle at `path`, or with Use Global Light (`own` false) the document's
/// global light angle: editing it then changes that one, for every effect that uses it.
fn angle(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str], own: bool, global: f64) -> egui::Response {
    let mut v = if own { num(d, path) } else { global };
    let r = ui.add(egui::DragValue::new(&mut v).range(-180.0..=180.0).suffix("°").max_decimals(1));
    if r.changed() {
        if own {
            set(d, path, json!(v));
        } else {
            d.fields.insert(GLOBAL_LIGHT.into(), json!(v));
        }
    }
    r
}

fn flag(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str], label: &str) {
    let mut v = {
        let mut cur = d.fields.get("fx").unwrap_or(&Value::Null);
        for k in path {
            cur = &cur[*k];
        }
        cur.as_bool().unwrap_or(false)
    };
    if ui.checkbox(&mut v, crate::rtl::widget(ui, label)).changed() {
        set(d, path, json!(v));
    }
}

/// A swatch of the document.
fn swatch(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, path: &[&str]) {
    let cur = {
        let mut v = d.fields.get("fx").unwrap_or(&Value::Null);
        for k in path {
            v = &v[*k];
        }
        v.as_str().unwrap_or("[Black]").to_string()
    };
    let Some(doc) = app.session.active().map(|st| &st.doc) else { return };
    // The colour itself beside its name, and a chip per swatch in the menu, as the swatch menus
    // elsewhere: a list of bare names hid what the shadow was coloured with.
    ui.horizontal(|ui| {
        let (c, g) = crate::widgets::swatch_colors(doc, &cur, 1.0);
        let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
        crate::widgets::paint_chip(ui.painter(), r, c, g);
        if let Some(n) = crate::dialogs::swatch_menu(ui, doc, ("fx_swatch", path.join(".")), &cur, 128.0) {
            set(d, path, json!(n));
        }
    });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(text).font(semibold(12.0)));
}

/// Distance and angle as an X (`y`: false) or Y offset: InDesign's, the shadow falls away from
/// the light. Editing an offset sets the distance and (without Global Light) the angle.
fn offset(ui: &mut egui::Ui, d: &mut Dialog, key: &str, unit: Unit, global: f64, y_axis: bool) {
    let gl = d.fields.get("fx").is_some_and(|f| f[key]["globalLight"].as_bool().unwrap_or(false));
    let a = if gl { global } else { num(d, &[key, "angle"]) }.to_radians();
    let dist = num(d, &[key, "distance"]);
    let (mut x, mut y) = (-dist * a.cos(), dist * a.sin());
    let v = if y_axis { &mut y } else { &mut x };
    let w =
        egui::DragValue::new(v).speed(0.25).custom_formatter(move |n, _| format_measure(n, unit)).custom_parser(move |s| parse_measure(s, unit).ok());
    if ui.add_sized([90.0, 20.0], w).changed() {
        set(d, &[key, "distance"], json!(x.hypot(y)));
        if !gl {
            set(d, &[key, "angle"], json!(y.atan2(-x).to_degrees()));
        }
    }
}

/// The settings of `page`.
fn page_ui(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, page: &str) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let unit = app.session.active().map_or(Unit::Points, |st| st.doc.settings.horizontal_units);
    // The global light angle as the dialog will set it (an edit in it shows at once).
    let global =
        d.fields.get(GLOBAL_LIGHT).and_then(Value::as_f64).unwrap_or_else(|| app.session.active().map_or(120.0, |st| st.doc.settings.global_light));
    let row = |ui: &mut egui::Ui, l: &str| crate::rtl::label(ui, tr(l));
    crate::rtl::label(ui, egui::RichText::new(tr(PAGES.iter().find(|p| p.0 == page).map_or("Transparency", |p| p.1))).font(semibold(14.0)));
    let grid = |id: &str| egui::Grid::new(("fx_grid", id.to_string())).num_columns(4).spacing([8.0, 6.0]);
    match page {
        "transparency" => {
            heading(ui, &tr("Basic Blending"));
            grid("t").show(ui, |ui| {
                row(ui, "Mode:");
                let cur = d.fields.get("blend").and_then(Value::as_str).unwrap_or("Normal").to_string();
                egui::ComboBox::from_id_salt("fx_blend").selected_text(tr(&cur)).width(150.0).show_ui(ui, |ui| {
                    for m in designcraft_color::BlendMode::ALL {
                        if ui.selectable_label(m.label() == cur, tr(m.label())).clicked() {
                            d.fields.insert("blend".into(), json!(m.label()));
                        }
                    }
                });
                row(ui, "Opacity:");
                let mut o = d.fields.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) * 100.0;
                if ui.add_sized([70.0, 20.0], egui::DragValue::new(&mut o).range(0.0..=100.0).suffix("%").max_decimals(0)).changed() {
                    d.fields.insert("opacity".into(), json!(o / 100.0));
                }
                ui.end_row();
            });
        }
        "dropShadow" | "innerShadow" => {
            let key = page;
            heading(ui, &tr("Blending"));
            grid("b").show(ui, |ui| {
                row(ui, "Color:");
                swatch(app, ui, d, &[key, "color"]);
                row(ui, "Opacity:");
                percent_of_one(ui, d, &[key, "opacity"]);
                ui.end_row();
            });
            heading(ui, &tr("Position"));
            grid("p").show(ui, |ui| {
                row(ui, "Distance:");
                measure(ui, d, &[key, "distance"], unit);
                row(ui, "X Offset:");
                offset(ui, d, key, unit, global, false);
                ui.end_row();
                row(ui, "Angle:");
                let gl = d.fields.get("fx").is_some_and(|f| f[key]["globalLight"].as_bool().unwrap_or(false));
                angle(ui, d, &[key, "angle"], !gl, global);
                row(ui, "Y Offset:");
                offset(ui, d, key, unit, global, true);
                ui.end_row();
                crate::rtl::label(ui, "");
                flag(ui, d, &[key, "globalLight"], &tr("Use Global Light"));
                ui.end_row();
            });
            heading(ui, &tr("Options"));
            grid("o").show(ui, |ui| {
                row(ui, "Size:");
                measure(ui, d, &[key, "size"], unit);
                if key == "dropShadow" {
                    row(ui, "Spread:");
                    percent(ui, d, &[key, "spread"]);
                } else {
                    row(ui, "Choke:");
                    percent(ui, d, &[key, "choke"]);
                }
                ui.end_row();
            });
        }
        "outerGlow" | "innerGlow" => {
            let key = page;
            heading(ui, &tr("Blending"));
            grid("b").show(ui, |ui| {
                // Inner Glow: its blend mode (Screen, as InDesign starts it).
                if key == "innerGlow" {
                    row(ui, "Mode:");
                    let cur = d.fields.get("fx").and_then(|f| f["innerGlow"]["mode"].as_str()).unwrap_or("Screen").to_string();
                    let label = |m: &designcraft_color::BlendMode| {
                        serde_json::to_value(m).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
                    };
                    let shown = designcraft_color::BlendMode::ALL.iter().find(|m| label(m) == cur).map_or("Screen", |m| m.label());
                    egui::ComboBox::from_id_salt("fx_glow_mode").selected_text(tr(shown)).width(150.0).show_ui(ui, |ui| {
                        for m in designcraft_color::BlendMode::ALL {
                            if ui.selectable_label(label(&m) == cur, tr(m.label())).clicked() {
                                set(d, &["innerGlow", "mode"], json!(label(&m)));
                            }
                        }
                    });
                    ui.end_row();
                }
                row(ui, "Color:");
                swatch(app, ui, d, &[key, "color"]);
                row(ui, "Opacity:");
                percent_of_one(ui, d, &[key, "opacity"]);
                ui.end_row();
            });
            heading(ui, &tr("Options"));
            grid("o").show(ui, |ui| {
                row(ui, "Size:");
                measure(ui, d, &[key, "size"], unit);
                if key == "outerGlow" {
                    row(ui, "Spread:");
                    percent(ui, d, &[key, "spread"]);
                } else {
                    row(ui, "Choke:");
                    percent(ui, d, &[key, "choke"]);
                    ui.end_row();
                    row(ui, "Technique:");
                    let cur = d.fields.get("fx").and_then(|f| f["innerGlow"]["technique"].as_str()).unwrap_or("softer").to_string();
                    egui::ComboBox::from_id_salt("fx_technique").selected_text(tr(if cur == "precise" { "Precise" } else { "Softer" })).show_ui(
                        ui,
                        |ui| {
                            for (v, l) in [("softer", "Softer"), ("precise", "Precise")] {
                                if ui.selectable_label(cur == v, tr(l)).clicked() {
                                    set(d, &["innerGlow", "technique"], json!(v));
                                }
                            }
                        },
                    );
                    ui.end_row();
                    row(ui, "Source:");
                    let mut center = d.fields.get("fx").is_some_and(|f| f["innerGlow"]["center"].as_bool().unwrap_or(false));
                    let label = |c: bool| tr(if c { "Center" } else { "Edge" });
                    egui::ComboBox::from_id_salt("fx_source").selected_text(label(center)).show_ui(ui, |ui| {
                        for c in [false, true] {
                            if ui.selectable_value(&mut center, c, label(c)).clicked() {
                                set(d, &["innerGlow", "center"], json!(c));
                            }
                        }
                    });
                }
                ui.end_row();
            });
        }
        "bevel" => {
            heading(ui, &tr("Structure"));
            grid("s").show(ui, |ui| {
                row(ui, "Size:");
                measure(ui, d, &["bevel", "size"], unit);
                row(ui, "Depth:");
                let mut v = num(d, &["bevel", "depth"]);
                if ui.add_sized([70.0, 20.0], egui::DragValue::new(&mut v).range(1.0..=1000.0).suffix("%").max_decimals(0)).changed() {
                    set(d, &["bevel", "depth"], json!(v));
                }
                ui.end_row();
            });
            heading(ui, &tr("Shading"));
            grid("sh").show(ui, |ui| {
                row(ui, "Angle:");
                let gl = d.fields.get("fx").is_some_and(|f| f["bevel"]["globalLight"].as_bool().unwrap_or(false));
                angle(ui, d, &["bevel", "angle"], !gl, global);
                flag(ui, d, &["bevel", "globalLight"], &tr("Use Global Light"));
                ui.end_row();
                row(ui, "Highlight:");
                swatch(app, ui, d, &["bevel", "highlight"]);
                row(ui, "Opacity:");
                percent_of_one(ui, d, &["bevel", "highlightOpacity"]);
                ui.end_row();
                row(ui, "Shadow:");
                swatch(app, ui, d, &["bevel", "shadow"]);
                row(ui, "Opacity:");
                percent_of_one(ui, d, &["bevel", "shadowOpacity"]);
                ui.end_row();
            });
        }
        "satin" => {
            heading(ui, &tr("Blending"));
            grid("b").show(ui, |ui| {
                row(ui, "Color:");
                swatch(app, ui, d, &["satin", "color"]);
                row(ui, "Opacity:");
                percent_of_one(ui, d, &["satin", "opacity"]);
                ui.end_row();
            });
            heading(ui, &tr("Options"));
            grid("o").show(ui, |ui| {
                row(ui, "Angle:");
                angle(ui, d, &["satin", "angle"], true, global);
                row(ui, "Distance:");
                measure(ui, d, &["satin", "distance"], unit);
                ui.end_row();
                row(ui, "Size:");
                measure(ui, d, &["satin", "size"], unit);
                crate::rtl::label(ui, "");
                flag(ui, d, &["satin", "invert"], &tr("Invert"));
                ui.end_row();
            });
        }
        "feather" => {
            heading(ui, &tr("Options"));
            grid("o").show(ui, |ui| {
                row(ui, "Feather Width:");
                measure(ui, d, &["feather"], unit);
                ui.end_row();
                row(ui, "Choke:");
                percent(ui, d, &["featherOptions", "choke"]);
                ui.end_row();
                row(ui, "Corners:");
                let cur = d.fields.get("fx").and_then(|f| f["featherOptions"]["corners"].as_str()).unwrap_or("diffused").to_string();
                let shown = match cur.as_str() {
                    "sharp" => "Sharp",
                    "rounded" => "Rounded",
                    _ => "Diffused",
                };
                egui::ComboBox::from_id_salt("fx_corners").selected_text(tr(shown)).show_ui(ui, |ui| {
                    for (v, l) in [("sharp", "Sharp"), ("rounded", "Rounded"), ("diffused", "Diffused")] {
                        if ui.selectable_label(cur == v, tr(l)).clicked() {
                            set(d, &["featherOptions", "corners"], json!(v));
                        }
                    }
                });
                ui.end_row();
            });
        }
        "directionalFeather" => {
            heading(ui, &tr("Feather Widths"));
            grid("w").show(ui, |ui| {
                for (i, l) in ["Top:", "Left:", "Bottom:", "Right:"].into_iter().enumerate() {
                    row(ui, l);
                    let mut v = d.fields.get("fx").and_then(|f| f["directionalFeather"]["widths"][i].as_f64()).unwrap_or(0.0);
                    let w = egui::DragValue::new(&mut v)
                        .range(0.0..=10_000.0)
                        .speed(0.25)
                        .custom_formatter(move |n, _| format_measure(n, unit))
                        .custom_parser(move |s| parse_measure(s, unit).ok());
                    if ui.add_sized([90.0, 20.0], w).changed() {
                        let mut ws: Vec<f64> =
                            (0..4).map(|k| d.fields.get("fx").and_then(|f| f["directionalFeather"]["widths"][k].as_f64()).unwrap_or(0.0)).collect();
                        if let Some(slot) = ws.get_mut(i) {
                            *slot = v;
                        }
                        set(d, &["directionalFeather", "widths"], json!(ws));
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        }
        "gradientFeather" => {
            heading(ui, &tr("Gradient Stops"));
            grid("g").show(ui, |ui| {
                row(ui, "Start Opacity:");
                percent_of_one(ui, d, &["gradientFeather", "start"]);
                row(ui, "End Opacity:");
                percent_of_one(ui, d, &["gradientFeather", "end"]);
                ui.end_row();
            });
            heading(ui, &tr("Options"));
            grid("o").show(ui, |ui| {
                row(ui, "Type:");
                let mut radial = d.fields.get("fx").is_some_and(|f| f["gradientFeather"]["radial"].as_bool().unwrap_or(false));
                let label = |r: bool| tr(if r { "Radial" } else { "Linear" });
                egui::ComboBox::from_id_salt("fx_gtype").selected_text(label(radial)).show_ui(ui, |ui| {
                    for r in [false, true] {
                        if ui.selectable_value(&mut radial, r, label(r)).clicked() {
                            set(d, &["gradientFeather", "radial"], json!(r));
                        }
                    }
                });
                row(ui, "Angle:");
                angle(ui, d, &["gradientFeather", "angle"], true, global);
                ui.end_row();
            });
        }
        _ => {}
    }
}

/// One line per applied level, as in InDesign's summary: "OBJECT: Normal 100%; Drop Shadow".
fn summary(lang: &str, d: &Dialog) -> String {
    let fx = d.fields.get("fx").cloned().unwrap_or(Value::Null);
    let blend = d.fields.get("blend").and_then(Value::as_str).unwrap_or("Normal");
    let opacity = d.fields.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
    let on: Vec<String> = PAGES.iter().skip(1).filter(|p| is_on(&fx, p.0)).map(|p| crate::i18n::tr(lang, p.1).to_string()).collect();
    let effects = if on.is_empty() { crate::i18n::tr(lang, "(no effects)").to_string() } else { on.join(", ") };
    format!("{}: {} {:.0}%; {}", crate::i18n::tr(lang, "OBJECT").to_uppercase(), crate::i18n::tr(lang, blend), opacity * 100.0, effects)
}

/// The dialog's body.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Settings for:"));
        // Stroke, Fill and Text levels aren't in the document model yet: the object only.
        ui.add_enabled_ui(false, |ui| {
            egui::ComboBox::from_id_salt("fx_level").selected_text(tr("Object")).width(110.0).show_ui(ui, |_| {});
        });
    });
    ui.add_space(6.0);
    let page = d.fields.get("page").and_then(Value::as_str).unwrap_or("transparency").to_string();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(180.0);
            for (key, label) in PAGES {
                ui.horizontal(|ui| {
                    if key == "transparency" {
                        ui.add_space(24.0);
                    } else {
                        let fx = d.fields.get("fx").cloned().unwrap_or(Value::Null);
                        let mut on = is_on(&fx, key);
                        if ui.checkbox(&mut on, "").changed() {
                            if on {
                                if let Some(f) = d.fields.get_mut("fx") {
                                    turn_on(f, key);
                                }
                            } else if key == "feather" {
                                set(d, &["feather"], json!(0.0));
                            } else {
                                set(d, &[key, "on"], json!(false));
                            }
                        }
                    }
                    if ui.selectable_label(page == key, tr(label)).clicked() {
                        d.fields.insert("page".into(), json!(key));
                        if let Some(f) = d.fields.get_mut("fx") {
                            turn_on(f, key);
                        }
                    }
                });
            }
            ui.add_space(8.0);
            crate::rtl::label(ui, egui::RichText::new(summary(&lang, d)).size(11.0).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
        });
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.set_min_width(380.0);
            page_ui(app, ui, d, &page);
        });
    });
    ui.add_space(10.0);
    let mut p = d.fields.get("preview").and_then(Value::as_bool).unwrap_or(false);
    if ui.checkbox(&mut p, crate::rtl::widget(ui, tr("Preview"))).changed() {
        d.fields.insert("preview".into(), json!(p));
    }
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
    preview(app, d);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_an_empty_path_changes_nothing() {
        let mut d = Dialog::new(ID, json!({"fx": {"dropShadow": {"distance": 7.0}}}));
        set(&mut d, &[], json!(1));
        assert_eq!(d.fields.get("fx"), Some(&json!({"dropShadow": {"distance": 7.0}})));
        set(&mut d, &["dropShadow", "size"], json!(3.0));
        assert_eq!(num(&d, &["dropShadow", "size"]), 3.0);
    }

    fn app_with_frame() -> (DesignApp, u64) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "inches"})).unwrap();
        let id = app.run("frame.create", json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        app.run("object.fill", json!({"swatch": "[Paper]"})).unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        (app, id)
    }

    fn frame(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..2 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
            out.textures_delta.clear();
            fn walk(s: egui::epaint::Shape, texts: &mut Vec<String>) {
                match s {
                    egui::epaint::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                    egui::epaint::Shape::Vec(v) => v.into_iter().for_each(|s| walk(s, texts)),
                    _ => {}
                }
            }
            texts.clear();
            for c in out.shapes {
                walk(c.shape, &mut texts);
            }
        }
        texts
    }

    /// With Use Global Light on, the Angle is the document's global light and stays editable:
    /// a change there is applied to the document with the effects (it was greyed out).
    #[test]
    fn the_angle_with_global_light_sets_the_global_light() {
        let (mut app, _) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        crate::menus::activate(&mut app, "object.dropShadow", &Value::Null);
        let d = app.ui.dialog.as_mut().unwrap();
        set(d, &["dropShadow", "globalLight"], json!(true));
        frame(&mut app, &ctx);
        // The field shows the global angle and is enabled: a DragValue that isn't disabled.
        let global = app.session.doc().unwrap().doc.settings.global_light;
        let mut d = app.ui.dialog.clone().unwrap();
        let ui_ctx = egui::Context::default();
        let mut enabled = false;
        let mut out = ui_ctx.run_ui(egui::RawInput::default(), |ui| {
            enabled = angle(ui, &mut d, &["dropShadow", "angle"], false, global).enabled();
        });
        out.textures_delta.clear();
        assert!(enabled, "the Angle stays editable with Use Global Light");
        d.fields.insert(GLOBAL_LIGHT.into(), json!(15.0));
        confirm(&mut app, &d).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.settings.global_light, 15.0);
    }

    #[test]
    fn set_makes_the_path_and_ignores_an_empty_one() {
        let mut d = Dialog::new(ID, json!({"fx": {}}));
        set(&mut d, &["featherOptions", "choke"], json!(20));
        assert_eq!(d.fields["fx"]["featherOptions"]["choke"], json!(20));
        let before = d.fields.clone();
        set(&mut d, &[], json!(1));
        assert_eq!(d.fields, before, "an empty path changes nothing (and doesn't panic)");
    }

    #[test]
    fn the_effects_dialog_opens_on_the_chosen_effect_previews_and_applies_once() {
        let (mut app, id) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        let undo = |app: &DesignApp| app.session.doc().unwrap().history.undo.len();
        let fx = |app: &DesignApp| app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone();
        let u0 = undo(&app);
        // Object › Effects › Inner Shadow… (as the menu activates it): the dialog, on that page.
        crate::menus::activate(&mut app, "object.innerShadow", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(ID));
        let texts = frame(&mut app, &ctx);
        for t in [
            "Effects",
            "Settings for:",
            "Transparency",
            "Inner Shadow",
            "Drop Shadow",
            "Blending",
            "Position",
            "Distance:",
            "Angle:",
            "Use Global Light",
            "Options",
            "Size:",
            "Choke:",
            "Preview",
            "OK",
            "Cancel",
        ] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        assert!(texts.iter().any(|x| x.contains("0.0972 in")), "distance in inches: {texts:?}");
        // Preview: the shadow is on the page while the dialog is open, without an undo step.
        frame(&mut app, &ctx);
        assert!(fx(&app).inner_shadow.on);
        assert_eq!(undo(&app), u0);
        // Cancel: as before.
        crate::dialogs::cancel(&mut app);
        assert!(!fx(&app).inner_shadow.on);
        assert_eq!(undo(&app), u0);
        // Again, with a change, then OK: one undo step.
        crate::menus::activate(&mut app, "object.innerShadow", &Value::Null);
        frame(&mut app, &ctx);
        if let Some(d) = app.ui.dialog.as_mut() {
            set(d, &["innerShadow", "size"], json!(9.0));
            d.fields.insert("opacity".into(), json!(0.8));
        }
        frame(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        let e = fx(&app);
        assert!(e.inner_shadow.on && e.inner_shadow.size == 9.0);
        assert_eq!(undo(&app), u0 + 1, "one undo step");
        assert!((app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().opacity - 0.8).abs() < 1e-6);
        app.run("edit.undo", json!({})).unwrap();
        assert!(!fx(&app).inner_shadow.on);
    }

    #[test]
    fn the_shadow_colour_shows_its_swatch_beside_the_name() {
        let (mut app, _) = app_with_frame();
        app.run("swatch.create", json!({"name": "Shade", "color": "#12ab34"})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        let shade = crate::widgets::swatch_colors(&app.session.doc().unwrap().doc, "Shade", 1.0).0.unwrap();
        // Does the dialog, once faded in, paint a chip of the colour `c`?
        let mut time = 0.0;
        let mut painted = |app: &mut DesignApp, c: egui::Color32| {
            let mut found = false;
            for _ in 0..20 {
                time += 0.1;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                    time: Some(time),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
                out.textures_delta.clear();
                fn walk(s: &egui::epaint::Shape, c: egui::Color32) -> bool {
                    match s {
                        egui::epaint::Shape::Rect(r) => r.fill == c,
                        egui::epaint::Shape::Vec(v) => v.iter().any(|s| walk(s, c)),
                        _ => false,
                    }
                }
                found = out.shapes.iter().any(|s| walk(&s.shape, c));
            }
            found
        };
        open(&mut app, "dropShadow");
        assert!(!painted(&mut app, shade), "not the shadow's colour yet");
        set(app.ui.dialog.as_mut().unwrap(), &["dropShadow", "color"], json!("Shade"));
        assert!(painted(&mut app, shade), "the chosen swatch's colour is shown, not only its name");
        assert_eq!(params(app.ui.dialog.as_ref().unwrap())["effects"]["dropShadow"]["color"], "Shade");
    }

    #[test]
    fn offsets_follow_distance_and_angle_as_in_indesign() {
        let (mut app, _) = app_with_frame();
        open(&mut app, "dropShadow");
        let d = app.ui.dialog.as_mut().unwrap();
        set(d, &["dropShadow", "distance"], json!(7.0));
        set(d, &["dropShadow", "angle"], json!(120.0));
        // InDesign shows 7 pt at 120° as X 0.0486 in, Y 0.0842 in.
        let a = 120f64.to_radians();
        assert!(((-7.0 * a.cos()) / 72.0 - 0.0486).abs() < 1e-4 && ((7.0 * a.sin()) / 72.0 - 0.0842).abs() < 1e-4);
        let p = params(d);
        assert_eq!(p["effects"]["dropShadow"]["on"], true, "chosen from the menu: on");
        assert_eq!(summary("en", d), "OBJECT: Normal 100%; Drop Shadow");
    }
}
