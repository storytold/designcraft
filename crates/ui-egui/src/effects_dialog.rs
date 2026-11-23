//! Object › Effects (and the Effects panel's fx button): InDesign's Effects dialog. Transparency
//! and every effect in one window: the list on the left (a checkbox turns an effect on, a click on
//! its name shows its settings and turns it on), the settings on the right in the document's
//! units, Preview on the page while it is open, and OK as one undo step (`object.effects`).

use serde_json::{Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;

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
pub(crate) fn effects_json(e: &designcraft_doc::Effects) -> Value {
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
pub(crate) fn turn_on(fx: &mut Value, page: &str) {
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

pub(crate) fn is_on(fx: &Value, page: &str) -> bool {
    match page {
        "transparency" => true,
        "feather" => fx["feather"].as_f64().unwrap_or(0.0) > 0.0,
        _ => fx[page]["on"].as_bool().unwrap_or(false),
    }
}

/// Turns the effect of `page` on (as choosing it does) or off: Basic Feather off is no width.
pub(crate) fn switch(fx: &mut Value, page: &str, on: bool) {
    if on {
        turn_on(fx, page);
    } else if page == "feather" {
        fx["feather"] = json!(0.0);
    } else {
        fx[page]["on"] = json!(false);
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

/// `deg` turned into the dial's range, −180 (exclusive) to 180°.
fn wrap_angle(deg: f64) -> f64 {
    let a = (deg + 180.0).rem_euclid(360.0) - 180.0;
    if a <= -180.0 { 180.0 } else { a }
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

/// The Effects dialog's form under the framework's dialog host.
pub(crate) const FORM: crate::dialogs::framework::Form = crate::dialogs::framework::Form::new(form::show).width(720.0);

/// The Effects dialog on the framework's components, and the controls its pages share with Object Style Options,
/// which shows the same effect pages. Every control edits the dialog's draft only; OK and Cancel stay the host's.
pub(crate) mod form {
    use super::{GLOBAL_LIGHT, PAGES, is_on, num, preview, set, summary, switch, turn_on};
    use crate::DesignApp;
    use crate::dialogs::Dialog;
    use craft_behavior::{Codec, EditEvents, NotAValue, Value as Bindable};
    use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
    use craft_ui_widgets::contract::{Bind, Entry, LabelKind, Props as _, Show as _, ShowContainer as _, SideLabel, Sizing};
    use craft_ui_widgets::display::{Shade, Text};
    use craft_ui_widgets::form::{Field, FieldGroup};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::runtime::env::UnitKind;
    use craft_ui_widgets::style::TypeRole;
    use craft_ui_widgets::text::{NumberField, TextField};
    use craft_ui_widgets::value_drag::AngleDial;
    use designcraft_geom::{Unit, format_measure, units::parse_measure};
    use egui::{AsIdSalt, Ui};
    use serde_json::{Value, json};
    use std::ops::RangeInclusive;

    /// The value after a control's `events`, when they changed `before`.
    pub(crate) fn edited<T: Bindable>(events: EditEvents<T>, before: T) -> Option<T> {
        let mut next = before.clone();
        for event in events {
            event.apply(&mut next);
        }
        (next != before).then_some(next)
    }

    /// A length in points in the document's own spelling ("1p6", "0.0972 in"). The framework's length fields have no
    /// pica or cicero spelling, so these fields read and write the measure format every other DesignCraft field uses.
    #[derive(Clone, Copy)]
    struct Measure {
        unit: Unit,
        range: Option<(f64, f64)>,
    }

    impl Codec for Measure {
        type Value = f64;

        fn format(&self, v: &f64) -> String {
            format_measure(*v, self.unit)
        }

        fn parse(&self, text: &str, _start: &f64) -> Result<f64, NotAValue> {
            let v = parse_measure(text, self.unit).map_err(|_| NotAValue)?;
            if !v.is_finite() {
                return Err(NotAValue);
            }
            Ok(self.range.map_or(v, |(min, max)| v.clamp(min, max)))
        }
    }

    /// A measure field named `label` over `points`, in the document's `unit`; the new value once it is edited.
    pub(crate) fn measure(ui: &mut Ui, salt: impl AsIdSalt, label: &str, points: f64, unit: Unit, range: Option<(f64, f64)>) -> Option<f64> {
        let out = TextField::with_codec(label.to_owned(), Bind::given(points), Measure { unit, range }).id_salt(salt).show(ui);
        edited(out.events, points)
    }

    /// A number field named `label` over `value`, in `kind`, kept in `range` and written with at most `decimals`.
    pub(crate) fn number(
        ui: &mut Ui,
        salt: impl AsIdSalt,
        label: &str,
        value: f64,
        kind: UnitKind,
        range: RangeInclusive<f64>,
        decimals: usize,
    ) -> Option<f64> {
        let field = NumberField::new(label.to_owned(), Bind::given(Some(value))).id_salt(salt).unit_kind(kind).range(range).step(1.0);
        let field = if decimals == 0 { field.snap_typed(true).fixed_decimals(0).exact_decimals(true) } else { field };
        let out = field.commit_decimals(decimals).show(ui);
        edited(out.edit.events, Some(value)).flatten().filter(|v| v.is_finite())
    }

    /// A check box labelled `label` over `value`; the new value once it is toggled.
    pub(crate) fn check(ui: &mut Ui, salt: impl AsIdSalt, label: &str, value: bool) -> Option<bool> {
        let out = Checkbox::new(Bind::given(value)).label(label.to_owned()).id_salt(salt).show(ui);
        edited(out.events, value)
    }

    /// A select named `label` over `value` among `entries`; the new value once one is picked.
    pub(crate) fn choose<T: Bindable>(ui: &mut Ui, salt: impl AsIdSalt, label: &str, value: T, entries: &[Entry<T>]) -> Option<T> {
        let out = Select::new(label.to_owned(), Bind::given(value.clone()), entries).id_salt(salt).show(ui);
        edited(out.events, value)
    }

    /// The level a style or the Effects dialog sets effects for. Stroke, Fill and Text levels aren't in the document
    /// model yet: the object only, so the select shows it and takes no pick.
    pub(crate) fn level(ui: &mut Ui, salt: impl AsIdSalt, label: &str, object: &str) {
        let entries = [Entry::new(0_u8, object.to_owned())];
        Select::new(label.to_owned(), Bind::given(0_u8), &entries).id_salt(salt).enabled(false).show(ui);
    }

    /// A row of a dialog's page list inside its `RadioGroup` over the page key: the include box (when `check` is
    /// given) and the page's radio. The box's hidden field label names it with the page, so the visible caption is
    /// the radio's alone. The box's new state once it is toggled.
    pub(crate) fn page_row(ui: &mut Ui, scope: &str, page: &str, label: &str, check: Option<bool>) -> Option<bool> {
        let mut result = None;
        Row::new().id_salt((scope.to_owned(), "row", page.to_owned())).show(ui, |ui, _| {
            if let Some(before) = check {
                Field::new(label.to_owned())
                    .side(SideLabel::new().kind(LabelKind::Hidden))
                    .sizing(Sizing::Content)
                    .id_salt((scope.to_owned(), "include", page.to_owned()))
                    .show(ui, |ui, _| {
                        let out = Checkbox::new(Bind::given(before)).show(ui);
                        if let Some(next) = edited(out.events, before) {
                            result = Some(next);
                        }
                    });
            } else {
                ui.add_space(24.0);
            }
            Radio::new(page.to_owned()).label(label.to_owned()).id_salt((scope.to_owned(), "page", page.to_owned())).show(ui);
        });
        result
    }

    /// A heading over a group of fields and the group, named by it.
    fn section(ui: &mut Ui, title: String, mut fields: impl FnMut(&mut Ui)) {
        Text::new(title.clone()).role(TypeRole::BodyStrong).show(ui);
        FieldGroup::new(title).show(ui, |ui, _| fields(ui));
    }

    /// A labelled field holding one control.
    fn field(ui: &mut Ui, label: &str, mut control: impl FnMut(&mut Ui)) {
        Field::new(label.to_owned()).show(ui, |ui, _| control(ui));
    }

    /// The value of `fx` at `path`.
    fn fx<'a>(d: &'a Dialog, path: &[&str]) -> &'a Value {
        let mut v = d.fields.get("fx").unwrap_or(&Value::Null);
        for k in path {
            v = &v[*k];
        }
        v
    }

    /// A distance of `fx` at `path`, in the document's unit (stored in points).
    fn distance(ui: &mut Ui, d: &mut Dialog, path: &[&str], label: &str, unit: Unit) {
        if let Some(v) = measure(ui, ("fx", path.join(".")), label, num(d, path), unit, Some((0.0, 10_000.0))) {
            set(d, path, json!(v));
        }
    }

    /// An opacity of `fx` at `path` (0–1, shown as %).
    fn percent_of_one(ui: &mut Ui, d: &mut Dialog, path: &[&str], label: &str) {
        if let Some(v) = number(ui, ("fx", path.join(".")), label, (num(d, path) * 100.0).round(), UnitKind::Percent, 0.0..=100.0, 0) {
            set(d, path, json!(v / 100.0));
        }
    }

    /// A percentage of `fx` at `path`, kept in `range`.
    fn percent(ui: &mut Ui, d: &mut Dialog, path: &[&str], label: &str, range: RangeInclusive<f64>) {
        if let Some(v) = number(ui, ("fx", path.join(".")), label, num(d, path), UnitKind::Percent, range, 0) {
            set(d, path, json!(v));
        }
    }

    /// A flag of `fx` at `path`.
    fn flag(ui: &mut Ui, d: &mut Dialog, path: &[&str], label: &str) {
        if let Some(v) = check(ui, ("fx", path.join(".")), label, fx(d, path).as_bool().unwrap_or(false)) {
            set(d, path, json!(v));
        }
    }

    /// The effect's own angle at `path`, or with Use Global Light (`own` false) the document's global light angle:
    /// editing it then changes that one, for every effect that uses it. The dial and the field both edit it.
    fn angle(ui: &mut Ui, d: &mut Dialog, path: &[&str], label: &str, own: bool, global: f64) {
        let value = if own { num(d, path) } else { global };
        let salt = path.join(".");
        let mut next = None;
        Row::new().id_salt(("fx_angle", salt.clone())).show(ui, |ui, _| {
            let dial = AngleDial::new(Bind::given(value)).label(label.to_owned()).id_salt(("fx_dial", salt.clone())).show(ui);
            if let Some(v) = edited(dial.events, value) {
                next = Some(v);
            }
            if let Some(v) = number(ui, ("fx_degrees", salt.clone()), label, value, UnitKind::Degrees, -180.0..=180.0, 1) {
                next = Some(v);
            }
        });
        if let Some(v) = next.map(super::wrap_angle) {
            if own {
                set(d, path, json!(v));
            } else {
                d.fields.insert(GLOBAL_LIGHT.into(), json!(v));
            }
        }
    }

    /// Distance and angle as an X (`y_axis` false) or Y offset: InDesign's, the shadow falls away from the light.
    /// Editing an offset sets the distance and (without Global Light) the angle.
    fn offset(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, unit: Unit, global: f64, y_axis: bool) {
        let gl = fx(d, &[key, "globalLight"]).as_bool().unwrap_or(false);
        let a = if gl { global } else { num(d, &[key, "angle"]) }.to_radians();
        let dist = num(d, &[key, "distance"]);
        let (x, y) = (-dist * a.cos(), dist * a.sin());
        let shown = if y_axis { y } else { x };
        let Some(v) = measure(ui, ("fx_offset", key.to_owned(), y_axis), label, shown, unit, None) else { return };
        let (x, y) = if y_axis { (x, v) } else { (v, y) };
        set(d, &[key, "distance"], json!(x.hypot(y)));
        if !gl {
            set(d, &[key, "angle"], json!(y.atan2(-x).to_degrees()));
        }
    }

    /// A swatch of the document at `path` of `fx`, picked by name.
    fn swatch(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, path: &[&str], label: &str) {
        let cur = fx(d, path).as_str().unwrap_or("[Black]").to_owned();
        let entries: Vec<Entry<String>> = app.session.active().map_or_else(Vec::new, |st| {
            st.doc.swatches.iter().filter(|w| !w.hidden || w.name == cur).map(|w| Entry::new(w.name.clone(), w.name.clone())).collect()
        });
        if let Some(name) = choose(ui, ("fx", path.join(".")), label, cur, &entries) {
            set(d, path, json!(name));
        }
    }

    /// A blend mode of `fx` at `key` (shadows start Multiply, glows Screen), by its document name.
    fn mode(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, tr: &dyn Fn(&str) -> String) {
        let fallback = if key.ends_with("Glow") { "Screen" } else { "Multiply" };
        let name = |m: &designcraft_color::BlendMode| serde_json::to_value(m).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        let cur = fx(d, &[key, "mode"]).as_str().unwrap_or(fallback).to_owned();
        let entries: Vec<Entry<String>> = designcraft_color::BlendMode::ALL.iter().map(|m| Entry::new(name(m), tr(m.label()))).collect();
        if let Some(m) = choose(ui, ("fx_mode", key.to_owned()), label, cur, &entries) {
            set(d, &[key, "mode"], json!(m));
        }
    }

    /// A choice of `fx` at `path` among `options` (value, label), shown as `fallback` while unset.
    fn option(ui: &mut Ui, d: &mut Dialog, path: &[&str], label: &str, options: &[(Value, &str)], fallback: Value, tr: &dyn Fn(&str) -> String) {
        let cur = match fx(d, path) {
            Value::Null => fallback,
            v => v.clone(),
        };
        let entries: Vec<Entry<Value>> = options.iter().map(|(v, l)| Entry::new(v.clone(), tr(l))).collect();
        if let Some(v) = choose(ui, ("fx", path.join(".")), label, cur, &entries) {
            set(d, path, v);
        }
    }

    /// The settings of `page`, as the native page shows them: the same fields, in the document's unit.
    pub(crate) fn page(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, page: &str) {
        let lang = app.ui.language.as_str();
        let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
        let unit = app.session.active().map_or(Unit::Points, |st| st.doc.settings.horizontal_units);
        // The global light angle as the dialog will set it (an edit in it shows at once).
        let global = d
            .fields
            .get(GLOBAL_LIGHT)
            .and_then(Value::as_f64)
            .unwrap_or_else(|| app.session.active().map_or(120.0, |st| st.doc.settings.global_light));
        let title = PAGES.iter().find(|p| p.0 == page).map_or("Transparency", |p| p.1);
        Stack::new().id_salt(("fx_page", page.to_owned())).fill_width().show(ui, |ui, _| {
            Text::new(tr(title)).role(TypeRole::Heading).show(ui);
            match page {
                "transparency" => section(ui, tr("Basic Blending"), |ui| {
                    Row::new().show(ui, |ui, _| {
                        field(ui, &tr("Mode:"), |ui| {
                            let cur = d.fields.get("blend").and_then(Value::as_str).unwrap_or("Normal").to_owned();
                            let entries: Vec<Entry<String>> =
                                designcraft_color::BlendMode::ALL.iter().map(|m| Entry::new(m.label().to_owned(), tr(m.label()))).collect();
                            if let Some(m) = choose(ui, "fx_blend", &tr("Mode:"), cur, &entries) {
                                d.fields.insert("blend".into(), json!(m));
                            }
                        });
                        field(ui, &tr("Opacity:"), |ui| {
                            let o = d.fields.get("opacity").and_then(Value::as_f64).unwrap_or(1.0);
                            if let Some(v) = number(ui, "fx_opacity", &tr("Opacity:"), (o * 100.0).round(), UnitKind::Percent, 0.0..=100.0, 0) {
                                d.fields.insert("opacity".into(), json!(v / 100.0));
                            }
                        });
                    });
                }),
                "dropShadow" | "innerShadow" => {
                    let key = page;
                    section(ui, tr("Blending"), |ui| {
                        field(ui, &tr("Mode:"), |ui| mode(ui, d, key, &tr("Mode:"), &tr));
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Color:"), |ui| swatch(app, ui, d, &[key, "color"], &tr("Color:")));
                            field(ui, &tr("Opacity:"), |ui| percent_of_one(ui, d, &[key, "opacity"], &tr("Opacity:")));
                        });
                    });
                    section(ui, tr("Position"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Distance:"), |ui| distance(ui, d, &[key, "distance"], &tr("Distance:"), unit));
                            field(ui, &tr("X Offset:"), |ui| offset(ui, d, key, &tr("X Offset:"), unit, global, false));
                        });
                        Row::new().show(ui, |ui, _| {
                            let gl = fx(d, &[key, "globalLight"]).as_bool().unwrap_or(false);
                            field(ui, &tr("Angle:"), |ui| angle(ui, d, &[key, "angle"], &tr("Angle:"), !gl, global));
                            field(ui, &tr("Y Offset:"), |ui| offset(ui, d, key, &tr("Y Offset:"), unit, global, true));
                        });
                        flag(ui, d, &[key, "globalLight"], &tr("Use Global Light"));
                    });
                    section(ui, tr("Options"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Size:"), |ui| distance(ui, d, &[key, "size"], &tr("Size:"), unit));
                            if key == "dropShadow" {
                                field(ui, &tr("Spread:"), |ui| percent(ui, d, &[key, "spread"], &tr("Spread:"), 0.0..=100.0));
                            } else {
                                field(ui, &tr("Choke:"), |ui| percent(ui, d, &[key, "choke"], &tr("Choke:"), 0.0..=100.0));
                            }
                        });
                    });
                }
                "outerGlow" | "innerGlow" => {
                    let key = page;
                    section(ui, tr("Blending"), |ui| {
                        field(ui, &tr("Mode:"), |ui| mode(ui, d, key, &tr("Mode:"), &tr));
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Color:"), |ui| swatch(app, ui, d, &[key, "color"], &tr("Color:")));
                            field(ui, &tr("Opacity:"), |ui| percent_of_one(ui, d, &[key, "opacity"], &tr("Opacity:")));
                        });
                    });
                    section(ui, tr("Options"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Size:"), |ui| distance(ui, d, &[key, "size"], &tr("Size:"), unit));
                            if key == "outerGlow" {
                                field(ui, &tr("Spread:"), |ui| percent(ui, d, &[key, "spread"], &tr("Spread:"), 0.0..=100.0));
                            } else {
                                field(ui, &tr("Choke:"), |ui| percent(ui, d, &[key, "choke"], &tr("Choke:"), 0.0..=100.0));
                            }
                        });
                        if key == "innerGlow" {
                            Row::new().show(ui, |ui, _| {
                                field(ui, &tr("Technique:"), |ui| {
                                    let options = [(json!("softer"), "Softer"), (json!("precise"), "Precise")];
                                    option(ui, d, &["innerGlow", "technique"], &tr("Technique:"), &options, json!("softer"), &tr);
                                });
                                field(ui, &tr("Source:"), |ui| {
                                    let options = [(json!(false), "Edge"), (json!(true), "Center")];
                                    option(ui, d, &["innerGlow", "center"], &tr("Source:"), &options, json!(false), &tr);
                                });
                            });
                        }
                    });
                }
                "bevel" => {
                    section(ui, tr("Structure"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Size:"), |ui| distance(ui, d, &["bevel", "size"], &tr("Size:"), unit));
                            field(ui, &tr("Depth:"), |ui| percent(ui, d, &["bevel", "depth"], &tr("Depth:"), 1.0..=1000.0));
                        });
                    });
                    section(ui, tr("Shading"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            let gl = fx(d, &["bevel", "globalLight"]).as_bool().unwrap_or(false);
                            field(ui, &tr("Angle:"), |ui| angle(ui, d, &["bevel", "angle"], &tr("Angle:"), !gl, global));
                            flag(ui, d, &["bevel", "globalLight"], &tr("Use Global Light"));
                        });
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Highlight:"), |ui| swatch(app, ui, d, &["bevel", "highlight"], &tr("Highlight:")));
                            field(ui, &tr("Opacity:"), |ui| percent_of_one(ui, d, &["bevel", "highlightOpacity"], &tr("Opacity:")));
                        });
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Shadow:"), |ui| swatch(app, ui, d, &["bevel", "shadow"], &tr("Shadow:")));
                            field(ui, &tr("Opacity:"), |ui| percent_of_one(ui, d, &["bevel", "shadowOpacity"], &tr("Opacity:")));
                        });
                    });
                }
                "satin" => {
                    section(ui, tr("Blending"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Color:"), |ui| swatch(app, ui, d, &["satin", "color"], &tr("Color:")));
                            field(ui, &tr("Opacity:"), |ui| percent_of_one(ui, d, &["satin", "opacity"], &tr("Opacity:")));
                        });
                    });
                    section(ui, tr("Options"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Angle:"), |ui| angle(ui, d, &["satin", "angle"], &tr("Angle:"), true, global));
                            field(ui, &tr("Distance:"), |ui| distance(ui, d, &["satin", "distance"], &tr("Distance:"), unit));
                        });
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Size:"), |ui| distance(ui, d, &["satin", "size"], &tr("Size:"), unit));
                            flag(ui, d, &["satin", "invert"], &tr("Invert"));
                        });
                    });
                }
                "feather" => section(ui, tr("Options"), |ui| {
                    field(ui, &tr("Feather Width:"), |ui| distance(ui, d, &["feather"], &tr("Feather Width:"), unit));
                    field(ui, &tr("Choke:"), |ui| percent(ui, d, &["featherOptions", "choke"], &tr("Choke:"), 0.0..=100.0));
                    field(ui, &tr("Corners:"), |ui| {
                        let options = [(json!("sharp"), "Sharp"), (json!("rounded"), "Rounded"), (json!("diffused"), "Diffused")];
                        option(ui, d, &["featherOptions", "corners"], &tr("Corners:"), &options, json!("diffused"), &tr);
                    });
                }),
                "directionalFeather" => section(ui, tr("Feather Widths"), |ui| {
                    for (row, sides) in [[(0, "Top:"), (1, "Left:")], [(2, "Bottom:"), (3, "Right:")]].into_iter().enumerate() {
                        Row::new().id_salt(("fx_widths", row)).show(ui, |ui, _| {
                            for (i, l) in sides {
                                field(ui, &tr(l), |ui| {
                                    let mut widths: Vec<f64> =
                                        (0..4).map(|k| fx(d, &["directionalFeather", "widths"])[k].as_f64().unwrap_or(0.0)).collect();
                                    if let Some(v) = measure(ui, ("fx_width", i), &tr(l), widths[i], unit, Some((0.0, 10_000.0))) {
                                        widths[i] = v;
                                        set(d, &["directionalFeather", "widths"], json!(widths));
                                    }
                                });
                            }
                        });
                    }
                }),
                "gradientFeather" => {
                    section(ui, tr("Gradient Stops"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Start Opacity:"), |ui| percent_of_one(ui, d, &["gradientFeather", "start"], &tr("Start Opacity:")));
                            field(ui, &tr("End Opacity:"), |ui| percent_of_one(ui, d, &["gradientFeather", "end"], &tr("End Opacity:")));
                        });
                    });
                    section(ui, tr("Options"), |ui| {
                        Row::new().show(ui, |ui, _| {
                            field(ui, &tr("Type:"), |ui| {
                                let options = [(json!(false), "Linear"), (json!(true), "Radial")];
                                option(ui, d, &["gradientFeather", "radial"], &tr("Type:"), &options, json!(false), &tr);
                            });
                            field(ui, &tr("Angle:"), |ui| angle(ui, d, &["gradientFeather", "angle"], &tr("Angle:"), true, global));
                        });
                    });
                }
                _ => {}
            }
        });
    }

    /// The Effects dialog's form: Settings for, the effect list (a box turns an effect on, choosing its name shows its
    /// settings and turns it on), the summary, the settings of the effect shown and Preview.
    pub(crate) fn show(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
        let lang = app.ui.language.clone();
        let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
        let shown = d.fields.get("page").and_then(Value::as_str).unwrap_or("transparency").to_owned();
        let view: &DesignApp = app;
        Stack::new().id_salt("effects_form").fill_width().show(ui, |ui, _| {
            field(ui, &tr("Settings for:"), |ui| level(ui, "fx_level", &tr("Settings for:"), &tr("Object")));
            Row::new().id_salt("effects_columns").show(ui, |ui, _| {
                Stack::new().id_salt("effects_list").show(ui, |ui, _| {
                    let pages = RadioGroup::new(tr("Effects"), Bind::given(shown.clone())).id_salt("effects_pages").show(ui, |ui, _| {
                        Stack::new().show(ui, |ui, _| {
                            for (key, label) in PAGES {
                                let on = (key != "transparency").then(|| is_on(d.fields.get("fx").unwrap_or(&Value::Null), key));
                                if let Some(on) = page_row(ui, "effects", key, &tr(label), on)
                                    && let Some(f) = d.fields.get_mut("fx")
                                {
                                    switch(f, key, on);
                                }
                            }
                        });
                    });
                    if let Some(page) = edited(pages.output.events, shown.clone()) {
                        d.fields.insert("page".into(), json!(page));
                        if let Some(f) = d.fields.get_mut("fx") {
                            turn_on(f, &page);
                        }
                    }
                    Text::new(summary(&lang, d)).role(TypeRole::Caption).shade(Shade::Muted).show(ui);
                });
                Stack::new().id_salt("effects_settings").fill_width().show(ui, |ui, _| page(view, ui, d, &shown));
            });
            if let Some(on) = check(ui, "fx_preview", &tr("Preview"), d.fields.get("preview").and_then(Value::as_bool).unwrap_or(false)) {
                d.fields.insert("preview".into(), json!(on));
            }
            if let Some(status) = d.fields.get("status").and_then(Value::as_str) {
                Text::new(status.to_owned()).shade(Shade::Muted).show(ui);
            }
        });
        // The page shows the draft while Preview is on: once per shown pass, never from a measuring one.
        if !ui.is_sizing_pass() && !craft_ui_widgets::runtime::measure::active(ui.ctx()) {
            preview(app, d);
        }
        None
    }
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

    /// The Effects dialog under the framework's dialog host, driven with real pointer and key events: its
    /// controls edit the draft only, Preview shows it on the page, OK applies it as one undo step and Cancel
    /// takes the preview away.
    mod framework_form {
        use super::*;
        use egui::accesskit::{Role, TreeUpdate};
        use egui::{Event, Key, Modifiers, PointerButton};

        struct Window {
            app: DesignApp,
            ctx: egui::Context,
            ax: Option<TreeUpdate>,
            time: f64,
        }

        impl Window {
            fn new(mut app: DesignApp) -> Self {
                app.ui.language = "en".into();
                let ctx = egui::Context::default();
                ctx.enable_accesskit();
                app.install_framework(&ctx);
                app.raw_input_hook(&ctx, &mut egui::RawInput::default());
                let out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1100.0))),
                        max_texture_side: Some(8192),
                        ..Default::default()
                    },
                    |ui| {
                        crate::theme::install_fonts(ui.ctx(), "");
                    },
                );
                out.drop_without_applying_deltas();
                let mut w = Window { app, ctx, ax: None, time: 1.0 };
                w.idle();
                w
            }

            fn frame(&mut self, events: Vec<Event>) {
                self.time += 0.05;
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1100.0))),
                    max_texture_side: Some(8192),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                };
                self.app.raw_input_hook(&self.ctx, &mut input);
                let ctx = self.ctx.clone();
                let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(&mut self.app, ui.ctx()));
                self.ax = out.platform_output.accesskit_update.take();
                out.drop_without_applying_deltas();
            }

            fn idle(&mut self) {
                for _ in 0..8 {
                    self.frame(Vec::new());
                }
            }

            fn key_with(&mut self, key: Key, modifiers: Modifiers) {
                for pressed in [true, false] {
                    self.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }]);
                }
                self.frame(Vec::new());
            }

            /// The one accessibility node of `role` named `name`, and its bounds.
            fn control(&self, role: Role, name: &str) -> egui::Rect {
                let matches: Vec<_> = self.ax.as_ref().unwrap().nodes.iter().filter(|(_, n)| n.role() == role && n.label() == Some(name)).collect();
                assert_eq!(matches.len(), 1, "exactly one {role:?} named {name}");
                let b = matches[0].1.bounds().unwrap();
                egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
            }

            fn click(&mut self, role: Role, name: &str) {
                let at = self.control(role, name).center();
                self.frame(vec![Event::PointerMoved(at)]);
                for pressed in [true, false] {
                    self.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
                }
                self.idle();
            }

            fn draft(&self, path: &[&str]) -> Value {
                let mut v = self.app.ui.dialog.as_ref().unwrap().fields.get("fx").cloned().unwrap_or(Value::Null);
                for k in path {
                    v = v[*k].clone();
                }
                v
            }
        }

        fn undo(app: &DesignApp) -> usize {
            app.session.doc().unwrap().history.undo.len()
        }

        fn effects(app: &DesignApp, id: u64) -> designcraft_doc::Effects {
            app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone()
        }

        fn window() -> (Window, u64) {
            let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "inches"})).unwrap();
            let id = app.run("frame.create", json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
            app.run("object.fill", json!({"swatch": "[Paper]"})).unwrap();
            app.run("selection.set", json!({"ids": [id]})).unwrap();
            (Window::new(app), id)
        }

        #[test]
        fn the_framework_form_edits_the_draft_previews_and_applies_once() {
            let (mut w, id) = window();
            let u0 = undo(&w.app);
            crate::menus::activate(&mut w.app, "object.dropShadow", &Value::Null);
            w.idle();
            for (role, name) in [
                (Role::ComboBox, "Mode:"),
                (Role::ComboBox, "Color:"),
                (Role::TextInput, "Distance:"),
                (Role::CheckBox, "Use Global Light"),
                (Role::RadioButton, "Drop Shadow"),
                (Role::CheckBox, "Preview"),
                (Role::Button, "OK"),
                (Role::Button, "Cancel"),
            ] {
                w.control(role, name);
            }
            // Preview: the shadow is on the page while the dialog is open, without an undo step.
            assert!(effects(&w.app, id).drop_shadow.on);
            assert_eq!(undo(&w.app), u0);

            // A typed size, in any unit, goes into the draft in points once the field is left.
            w.click(Role::TextInput, "Size:");
            w.key_with(Key::A, Modifiers::COMMAND);
            w.frame(vec![Event::Text("9 pt".into())]);
            w.key_with(Key::Tab, Modifiers::NONE);
            w.idle();
            assert_eq!(w.draft(&["dropShadow", "size"]), json!(9.0));
            let global = w.draft(&["dropShadow", "globalLight"]).as_bool().unwrap_or(false);
            w.click(Role::CheckBox, "Use Global Light");
            assert_eq!(w.draft(&["dropShadow", "globalLight"]), json!(!global));
            assert_eq!(undo(&w.app), u0, "the controls edit the draft and the preview only");
            assert_eq!(effects(&w.app, id).drop_shadow.size, 9.0, "the preview follows the draft");

            // OK: the draft is applied once, as one undo step.
            w.click(Role::Button, "OK");
            assert!(w.app.ui.dialog.is_none());
            let applied = effects(&w.app, id);
            assert!(applied.drop_shadow.on && applied.drop_shadow.size == 9.0 && applied.drop_shadow.global_light == !global);
            assert_eq!(undo(&w.app), u0 + 1, "one undo step");
            w.app.run("edit.undo", json!({})).unwrap();
            assert!(!effects(&w.app, id).drop_shadow.on);

            // Cancel: the preview goes and nothing is applied.
            crate::menus::activate(&mut w.app, "object.innerShadow", &Value::Null);
            w.idle();
            assert!(effects(&w.app, id).inner_shadow.on);
            w.click(Role::Button, "Cancel");
            assert!(w.app.ui.dialog.is_none());
            assert!(!effects(&w.app, id).inner_shadow.on);
            assert_eq!(undo(&w.app), u0);
        }

        /// Choosing an effect's name in the list shows its page and turns it on; its box turns it off again,
        /// in the draft alone.
        #[test]
        fn the_effect_list_shows_a_page_and_its_box_turns_the_effect_off() {
            let (mut w, _) = window();
            crate::menus::activate(&mut w.app, "object.dropShadow", &Value::Null);
            w.idle();
            let u0 = undo(&w.app);
            w.click(Role::RadioButton, "Outer Glow");
            assert_eq!(w.app.ui.dialog.as_ref().unwrap().s("page"), "outerGlow");
            assert_eq!(w.draft(&["outerGlow", "on"]), json!(true));
            w.control(Role::ComboBox, "Mode:");
            w.click(Role::CheckBox, "Outer Glow");
            assert_eq!(w.draft(&["outerGlow", "on"]), json!(false));
            assert_eq!(w.draft(&["dropShadow", "on"]), json!(true), "the other effects stay as they were");
            assert_eq!(undo(&w.app), u0);
            w.click(Role::Button, "Cancel");
            assert!(w.app.ui.dialog.is_none());
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_tests;
