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
    json!({"ids": d.fields.get("ids").cloned().unwrap_or(json!([])), "effects": d.fields.get("fx").cloned().unwrap_or(json!({})),
        "opacity": d.fields.get("opacity").and_then(Value::as_f64).unwrap_or(1.0), "blend": d.fields.get("blend").and_then(Value::as_str).unwrap_or("Normal")})
}

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
/// The field id for a value of the dialog (`fx_dropShadow_size`).
fn field_id(path: &[&str]) -> String {
    format!("fx_{}", path.join("_"))
}

/// A length in the document's units, with spinner arrows as InDesign's.
fn measure(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str], unit: Unit) {
    let id = field_id(path);
    if let Some(v) = crate::widgets::NumField::measure(&id, Some(num(d, path)), unit).width(96.0).spinner().range(0.0, 10_000.0).show(ui) {
        set(d, path, json!(v));
    }
}

/// An opacity (0–1, shown as %).
fn percent_of_one(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str]) {
    let id = field_id(path);
    if let Some(v) =
        crate::widgets::NumField::number(&id, Some((num(d, path) * 100.0).round()), "%", 0).width(96.0).range(0.0, 100.0).slider().show(ui)
    {
        set(d, path, json!(v / 100.0));
    }
}

/// A percentage (0–100).
fn percent(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str]) {
    let id = field_id(path);
    if let Some(v) = crate::widgets::NumField::number(&id, Some(num(d, path)), "%", 0).width(96.0).range(0.0, 100.0).slider().show(ui) {
        set(d, path, json!(v));
    }
}

fn angle(ui: &mut egui::Ui, d: &mut Dialog, path: &[&str], enabled: bool, global: f64) {
    let mut v = if enabled { num(d, path) } else { global };
    ui.horizontal(|ui| {
        if let Some(a) = angle_dial(ui, v, enabled) {
            v = a;
            set(d, path, json!(v));
        }
        let id = field_id(path);
        if let Some(n) = crate::widgets::NumField::number(&id, Some(v), "°", 1).width(70.0).range(-180.0, 180.0).enabled(enabled).show(ui) {
            set(d, path, json!(n));
        }
    });
}

/// The angle dial's size.
const DIAL: f32 = 26.0;

/// InDesign's angle dial: a circle with a radius pointing where the light comes from (0° to the
/// right, counter-clockwise). A click or a drag on it points it at the pointer.
fn angle_dial(ui: &mut egui::Ui, deg: f64, enabled: bool) -> Option<f64> {
    let sense = if enabled { egui::Sense::click_and_drag() } else { egui::Sense::hover() };
    let (r, resp) = ui.allocate_exact_size(egui::vec2(DIAL, DIAL), sense);
    resp.widget_info(|| egui::WidgetInfo::slider(enabled, deg, "Angle"));
    let col = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    let (c, rad) = (r.center(), 11.0);
    ui.painter().circle_stroke(c, rad, egui::Stroke::new(1.2, col));
    let a = deg.to_radians() as f32;
    ui.painter().line_segment([c, c + egui::vec2(a.cos(), -a.sin()) * rad], egui::Stroke::new(1.4, col));
    if enabled && (resp.dragged() || resp.clicked()) {
        return resp.interact_pointer_pos().and_then(|p| dial_angle(c, p));
    }
    None
}

/// The angle a dial at `c` points at for the pointer at `p`, in whole degrees (−180 to 180,
/// counter-clockwise from the right); none too close to the centre to tell.
fn dial_angle(c: egui::Pos2, p: egui::Pos2) -> Option<f64> {
    let d = p - c;
    // Straight left is 180° (not −180°, which a negative zero would give).
    (d.length() > 2.0).then(|| f64::from((-d.y).atan2(d.x)).to_degrees().round()).map(|a| if a <= -180.0 { 180.0 } else { a })
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
    let names: Vec<String> = app
        .session
        .active()
        .map_or_else(Vec::new, |st| st.doc.swatches.iter().filter(|w| !w.hidden || w.name == cur).map(|w| w.name.clone()).collect());
    egui::ComboBox::from_id_salt(("fx_swatch", path.join("."))).selected_text(&cur).width(150.0).show_ui(ui, |ui| {
        for n in names {
            if ui.selectable_label(n == cur, &n).clicked() {
                set(d, path, json!(n));
            }
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
    let id = field_id(&[key, if y_axis { "yOffset" } else { "xOffset" }]);
    let cur = if y_axis { y } else { x };
    if let Some(v) = crate::widgets::NumField::measure(&id, Some(cur), unit).width(96.0).spinner().range(-10_000.0, 10_000.0).show(ui) {
        if y_axis {
            y = v;
        } else {
            x = v;
        }
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
    let global = app.session.active().map_or(120.0, |st| st.doc.settings.global_light);
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
        "dropShadow" | "innerShadow" | "outerGlow" | "innerGlow" => shadow_or_glow(app, ui, d, page, unit, global, &tr),
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

/// A titled group as InDesign's Effects dialog draws it: a thin frame with the title inside.
fn group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.add_space(8.0);
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).inner_margin(egui::Margin::symmetric(10, 8)).corner_radius(2.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        crate::rtl::label(ui, egui::RichText::new(title).font(semibold(12.0)));
        ui.add_space(2.0);
        ui.spacing_mut().item_spacing.y = 7.0;
        add(ui);
    });
}

/// The widths of a page's two columns: the left captions, its fields, the right captions (both
/// captions right-aligned before their fields, as InDesign's).
#[derive(Clone, Copy)]
struct Cols {
    left: f32,
    fields: f32,
    right: f32,
}

impl Cols {
    fn new(ui: &egui::Ui, left: &[String], right: &[String]) -> Self {
        let w = |l: &[String]| {
            l.iter()
                .map(|s| {
                    egui::WidgetText::from(s.as_str())
                        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body)
                        .size()
                        .x
                })
                .fold(0.0, f32::max)
        };
        Cols { left: w(left) + 2.0, fields: 196.0, right: w(right) + 2.0 }
    }
}

/// A caption right-aligned in `w`, then `add` in a cell `fields` wide (0: as wide as it is).
fn cell(ui: &mut egui::Ui, caption: &str, w: f32, fields: f32, add: impl FnOnce(&mut egui::Ui)) {
    let h = crate::widgets::FIELD_H;
    ui.allocate_ui_with_layout(egui::vec2(w, h), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.set_min_width(w);
        if !caption.is_empty() {
            ui.add(egui::Label::new(crate::rtl::widget(ui, caption)).wrap_mode(egui::TextWrapMode::Extend));
        }
    });
    if fields > 0.0 {
        ui.allocate_ui_with_layout(egui::vec2(fields, h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(fields);
            add(ui);
        });
    } else {
        add(ui);
    }
}

/// The colour of a shadow or glow, as InDesign's: a chip beside the mode that lists the swatches.
fn color_chip(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, path: &[&str], tr: &dyn Fn(&str) -> String) {
    let cur = {
        let mut v = d.fields.get("fx").unwrap_or(&Value::Null);
        for k in path {
            v = &v[*k];
        }
        v.as_str().unwrap_or("[Black]").to_string()
    };
    let doc = app.session.active().map(|st| &st.doc);
    let chip = |name: &str| doc.map_or((None, None), |doc| crate::widgets::swatch_colors(doc, name, 1.0));
    let (c, g) = chip(&cur);
    let resp = crate::widgets::swatch_chip(ui, crate::widgets::FIELD_H, c, g).on_hover_text(&cur);
    let label = tr("Color:").trim_end_matches([':', ' ']).to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let names: Vec<String> =
        doc.map_or_else(Vec::new, |doc| doc.swatches.iter().filter(|w| !w.hidden || w.name == cur).map(|w| w.name.clone()).collect());
    egui::Popup::menu(&resp).show(|ui| {
        for n in names {
            let picked = ui
                .horizontal(|ui| {
                    let (c, g) = chip(&n);
                    let r = crate::widgets::swatch_chip(ui, 14.0, c, g);
                    ui.selectable_label(n == cur, &n).clicked() || r.clicked()
                })
                .inner;
            if picked {
                set(d, path, json!(n));
                ui.close();
            }
        }
    });
}

/// Softer or Precise, for a glow.
fn technique(ui: &mut egui::Ui, d: &mut Dialog, key: &str, tr: &dyn Fn(&str) -> String) {
    let cur = d.fields.get("fx").and_then(|f| f[key]["technique"].as_str()).unwrap_or("softer").to_string();
    egui::ComboBox::from_id_salt(("fx_technique", key.to_string()))
        .selected_text(tr(if cur == "precise" { "Precise" } else { "Softer" }))
        .width(130.0)
        .show_ui(ui, |ui| {
            for (v, l) in [("softer", "Softer"), ("precise", "Precise")] {
                if ui.selectable_label(cur == v, tr(l)).clicked() {
                    set(d, &[key, "technique"], json!(v));
                }
            }
        });
}

/// Drop Shadow, Inner Shadow, Outer Glow and Inner Glow, laid out as InDesign's: Blending (Mode,
/// colour, Opacity), Position for the shadows (Distance and Angle beside the X and Y Offsets),
/// and Options in two columns.
fn shadow_or_glow(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, key: &str, unit: Unit, global: f64, tr: &dyn Fn(&str) -> String) {
    let shadow = key.ends_with("Shadow");
    let (left, right): (&[&str], &[&str]) = match key {
        "dropShadow" => (&["Mode:", "Distance:", "Angle:", "Size:", "Spread:", "Noise:"], &["Opacity:", "X Offset:", "Y Offset:"]),
        "innerShadow" => (&["Mode:", "Distance:", "Angle:", "Size:", "Choke:", "Noise:"], &["Opacity:", "X Offset:", "Y Offset:"]),
        "outerGlow" => (&["Mode:", "Technique:", "Size:"], &["Opacity:", "Noise:", "Spread:"]),
        _ => (&["Mode:", "Technique:", "Size:", "Source:"], &["Opacity:", "Noise:", "Choke:"]),
    };
    let c = Cols::new(ui, &left.iter().map(|l| tr(l)).collect::<Vec<_>>(), &right.iter().map(|l| tr(l)).collect::<Vec<_>>());
    group(ui, &tr("Blending"), |ui| {
        ui.horizontal(|ui| {
            cell(ui, &tr("Mode:"), c.left, c.fields, |ui| {
                mode_combo(ui, d, key, tr);
                color_chip(app, ui, d, &[key, "color"], tr);
            });
            cell(ui, &tr("Opacity:"), c.right, 0.0, |ui| percent_of_one(ui, d, &[key, "opacity"]));
        });
    });
    if shadow {
        group(ui, &tr("Position"), |ui| {
            let gl = d.fields.get("fx").is_some_and(|f| f[key]["globalLight"].as_bool().unwrap_or(false));
            ui.horizontal(|ui| {
                cell(ui, &tr("Distance:"), c.left, c.fields, |ui| measure(ui, d, &[key, "distance"], unit));
                cell(ui, &tr("X Offset:"), c.right, 0.0, |ui| offset(ui, d, key, unit, global, false));
            });
            ui.horizontal(|ui| {
                cell(ui, &tr("Angle:"), c.left, c.fields, |ui| angle(ui, d, &[key, "angle"], !gl, global));
                cell(ui, &tr("Y Offset:"), c.right, 0.0, |ui| offset(ui, d, key, unit, global, true));
            });
            ui.horizontal(|ui| {
                // Under the angle's field, past the dial.
                let past = c.left + DIAL + 2.0 * ui.spacing().item_spacing.x;
                cell(ui, "", past, 0.0, |ui| flag(ui, d, &[key, "globalLight"], &tr("Use Global Light")));
            });
        });
    }
    group(ui, &tr("Options"), |ui| match key {
        "dropShadow" => {
            ui.horizontal(|ui| {
                cell(ui, &tr("Size:"), c.left, c.fields, |ui| measure(ui, d, &[key, "size"], unit));
                flag(ui, d, &[key, "knockout"], &tr("Object Knocks Out Shadow"));
            });
            ui.horizontal(|ui| {
                cell(ui, &tr("Spread:"), c.left, c.fields, |ui| percent(ui, d, &[key, "spread"]));
                flag(ui, d, &[key, "honorsEffects"], &tr("Shadow Honors Other Effects"));
            });
            ui.horizontal(|ui| cell(ui, &tr("Noise:"), c.left, c.fields, |ui| percent(ui, d, &[key, "noise"])));
        }
        "innerShadow" => {
            ui.horizontal(|ui| cell(ui, &tr("Size:"), c.left, c.fields, |ui| measure(ui, d, &[key, "size"], unit)));
            ui.horizontal(|ui| cell(ui, &tr("Choke:"), c.left, c.fields, |ui| percent(ui, d, &[key, "choke"])));
            ui.horizontal(|ui| cell(ui, &tr("Noise:"), c.left, c.fields, |ui| percent(ui, d, &[key, "noise"])));
        }
        _ => {
            let outer = key == "outerGlow";
            ui.horizontal(|ui| {
                cell(ui, &tr("Technique:"), c.left, c.fields, |ui| technique(ui, d, key, tr));
                cell(ui, &tr("Noise:"), c.right, 0.0, |ui| percent(ui, d, &[key, "noise"]));
            });
            ui.horizontal(|ui| {
                cell(ui, &tr("Size:"), c.left, c.fields, |ui| measure(ui, d, &[key, "size"], unit));
                let (l, k) = if outer { ("Spread:", "spread") } else { ("Choke:", "choke") };
                cell(ui, &tr(l), c.right, 0.0, |ui| percent(ui, d, &[key, k]));
            });
            if !outer {
                ui.horizontal(|ui| {
                    cell(ui, &tr("Source:"), c.left, c.fields, |ui| {
                        let mut center = d.fields.get("fx").is_some_and(|f| f["innerGlow"]["center"].as_bool().unwrap_or(false));
                        let label = |c: bool| tr(if c { "Center" } else { "Edge" });
                        egui::ComboBox::from_id_salt("fx_source").selected_text(label(center)).width(130.0).show_ui(ui, |ui| {
                            for c in [false, true] {
                                if ui.selectable_value(&mut center, c, label(c)).clicked() {
                                    set(d, &["innerGlow", "center"], json!(c));
                                }
                            }
                        });
                    })
                });
            }
        }
    });
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

/// The Mode of a shadow's or glow's Blending: its blend mode, as InDesign's (shadows start
/// Multiply, glows Screen).
fn mode_combo(ui: &mut egui::Ui, d: &mut Dialog, key: &str, tr: &dyn Fn(&str) -> String) {
    let fallback = if key.ends_with("Glow") { "Screen" } else { "Multiply" };
    let cur = d.fields.get("fx").and_then(|f| f[key]["mode"].as_str()).unwrap_or(fallback).to_string();
    let name = |m: &designcraft_color::BlendMode| serde_json::to_value(m).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let shown = designcraft_color::BlendMode::ALL.iter().find(|m| name(m) == cur).map_or(fallback, |m| m.label());
    let mut picked = None;
    egui::ComboBox::from_id_salt(("fx_mode", key.to_string())).selected_text(tr(shown)).width(140.0).show_ui(ui, |ui| {
        for m in designcraft_color::BlendMode::ALL {
            if ui.selectable_label(name(&m) == cur, tr(m.label())).clicked() {
                picked = Some(name(&m));
            }
        }
    });
    if let Some(m) = picked {
        set(d, &[key, "mode"], json!(m));
    }
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
            ui.set_min_width(470.0);
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
        for _ in 0..4 {
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

    /// Drop Shadow and Outer Glow have InDesign's Mode under Blending (Multiply and Screen to
    /// start); a mode picked there is what OK applies.
    #[test]
    fn shadows_and_glows_have_a_blend_mode() {
        let (mut app, id) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        open(&mut app, "dropShadow");
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.trim() == "Mode:") && texts.iter().any(|x| x.trim() == "Multiply"), "{texts:?}");
        crate::dialogs::cancel(&mut app);
        open(&mut app, "outerGlow");
        let texts = frame(&mut app, &ctx);
        assert!(texts.iter().any(|x| x.trim() == "Screen"), "{texts:?}");
        if let Some(d) = app.ui.dialog.as_mut() {
            set(d, &["outerGlow", "mode"], json!("Normal"));
        }
        frame(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        let fx = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone();
        assert!(fx.outer_glow.on);
        assert_eq!(fx.outer_glow.mode, designcraft_color::BlendMode::Normal);
    }

    /// The angle dial points where the pointer is, as InDesign's: right is 0°, up 90°, left 180°,
    /// down −90°, upper left 135°.
    /// The shadow and glow pages have InDesign's options: Noise, Object Knocks Out Shadow (on
    /// to start) and Shadow Honors Other Effects for the drop shadow, a Technique for the outer
    /// glow; what they're set to is what OK applies.
    #[test]
    fn shadows_and_glows_have_indesigns_options() {
        let (mut app, id) = app_with_frame();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        open(&mut app, "dropShadow");
        let texts = frame(&mut app, &ctx);
        for t in ["Object Knocks Out Shadow", "Shadow Honors Other Effects", "Noise:", "X Offset:", "Use Global Light"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields["fx"]["dropShadow"]["knockout"], json!(true));
        if let Some(d) = app.ui.dialog.as_mut() {
            set(d, &["dropShadow", "knockout"], json!(false));
            set(d, &["dropShadow", "honorsEffects"], json!(true));
            set(d, &["dropShadow", "noise"], json!(20.0));
            d.fields.insert("page".into(), json!("outerGlow"));
            turn_on(d.fields.get_mut("fx").unwrap(), "outerGlow");
        }
        let texts = frame(&mut app, &ctx);
        for t in ["Technique:", "Softer", "Noise:", "Spread:"] {
            assert!(texts.iter().any(|x| x.trim() == t), "{t}: {texts:?}");
        }
        if let Some(d) = app.ui.dialog.as_mut() {
            set(d, &["outerGlow", "technique"], json!("precise"));
        }
        frame(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        let fx = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone();
        assert!(!fx.drop_shadow.knockout && fx.drop_shadow.honors_effects);
        assert_eq!(fx.drop_shadow.noise, 20.0);
        assert_eq!(fx.outer_glow.technique, designcraft_doc::GlowTechnique::Precise);
    }

    #[test]
    fn the_angle_dial_points_at_the_pointer() {
        let c = egui::pos2(100.0, 100.0);
        assert_eq!(dial_angle(c, egui::pos2(120.0, 100.0)), Some(0.0));
        assert_eq!(dial_angle(c, egui::pos2(100.0, 80.0)), Some(90.0));
        assert_eq!(dial_angle(c, egui::pos2(80.0, 100.0)), Some(180.0));
        assert_eq!(dial_angle(c, egui::pos2(100.0, 120.0)), Some(-90.0));
        assert_eq!(dial_angle(c, egui::pos2(90.0, 90.0)), Some(135.0));
        assert_eq!(dial_angle(c, egui::pos2(101.0, 100.0)), None, "too close to the centre");
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
