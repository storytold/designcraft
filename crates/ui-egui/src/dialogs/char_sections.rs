//! The character sections of the Find/Change format settings and of the style options dialogs on the framework: Basic and
//! Advanced Character Formats, Character Color, OpenType Features and Underline/Strikethrough Options. Each draws the body
//! of one page over the dialog's `c.<attr>` draft fields (`CharFields`). With `sparse` (a character style, a Find or Change
//! Format) a field can be blank: a blank entry, an emptied number field, or the third state of a check box leaves the
//! attribute unset (`Null`).

use super::frame_forms::{field, message};
use super::{CharFields, Dialog};
use crate::DesignApp;
use craft_behavior::Mixed;
use craft_ui_widgets::buttons::ToggleButton;
use craft_ui_widgets::choice::{Checkbox, Segmented};
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::text::{NumberField, TextField};
use egui::Ui;
use serde_json::{Value, json};

/// The entry of a sparse picker that unsets the attribute.
const BLANK: &str = "\u{2014}";

fn tr(lang: &str, s: &str) -> String {
    crate::i18n::tr(lang, s).to_string()
}

/// A number attribute field: what it shows and how it reads.
struct Spec<'a> {
    label: &'a str,
    shown: Option<f64>,
    unit: Option<&'static str>,
    range: (f64, f64),
}

/// A number attribute; `make` is the value a number gives the attribute. An emptied field of a sparse form unsets it.
fn number(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, spec: Spec<'_>, make: impl Fn(f64) -> Value) {
    let Spec { label, shown, unit, range } = spec;
    let mut field = NumberField::new(label.to_owned(), Bind::given(shown)).id_salt(("char_number", key)).range(range.0..=range.1);
    if let Some(unit) = unit {
        field = field.unit(unit);
    }
    match field.show(ui).edit.events.landed().copied() {
        Some(Some(n)) if n.is_finite() && Some(n) != shown => cf.set(d, key, make(n)),
        Some(None) if cf.sparse && shown.is_some() => cf.set(d, key, Value::Null),
        _ => {}
    }
}

/// A number attribute shown multiplied by `scale` (100 for a fraction shown as a percentage), with its unit.
fn scaled(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str, unit: (&'static str, f64), range: (f64, f64)) {
    let (unit, scale) = unit;
    let shown = cf.get(d, key).as_f64().map(|x| x * scale);
    number(ui, d, cf, key, Spec { label, shown, unit: Some(unit), range }, |n| json!(n / scale));
}

/// An attribute chosen from `options` (value, caption); a sparse form also has a blank entry.
pub(super) fn choice(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str, options: &[(Value, String)]) {
    let current = cf.get(d, key);
    let mut entries: Vec<Entry<i32>> = Vec::new();
    if cf.sparse {
        entries.push(Entry::new(-1, BLANK));
    }
    entries.extend(options.iter().enumerate().map(|(at, (_, caption))| Entry::new(at as i32, caption.clone())));
    let at = options.iter().position(|(value, _)| *value == current).map(|at| at as i32).or_else(|| (cf.sparse && current.is_null()).then_some(-1));
    let out = Select::new(label.to_owned(), Bind::given(at.unwrap_or(-2)), &entries).keeps_name(true).id_salt(("char_choice", key)).show(ui);
    if let Some(picked) = out.events.committed().copied().filter(|picked| Some(*picked) != at) {
        let value = usize::try_from(picked).ok().and_then(|at| options.get(at)).map_or(Value::Null, |(value, _)| value.clone());
        cf.set(d, key, value);
    }
}

/// An on/off attribute; in a sparse form a third, unset state: clicks go unset, on, off, unset.
fn toggle(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str) {
    let current = cf.get(d, key);
    let bind = if cf.sparse && current.is_null() { Bind::given(Mixed::<bool>::Mixed) } else { Bind::given(current.as_bool().unwrap_or(false)) };
    let out = Checkbox::new(bind).label(label.to_owned()).id_salt(("char_check", key)).show(ui);
    if let Some(on) = out.events.committed().copied() {
        let next = match current.as_bool() {
            _ if !cf.sparse => json!(on),
            None => json!(true),
            Some(true) => json!(false),
            Some(false) => Value::Null,
        };
        cf.set(d, key, next);
    }
}

/// A swatch attribute; `text_color` names the "" entry (the text's own colour).
fn swatch(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document, key: &str, label: &str, text_color: Option<String>) {
    let mut options: Vec<(Value, String)> = text_color.map(|caption| (json!(""), caption)).into_iter().collect();
    options.extend(doc.swatches.iter().map(|w| (json!(w.name), w.name.clone())));
    choice(ui, d, cf, key, label, &options);
}

/// Basic Character Formats: font, size, leading, kerning, tracking, case, position and the underline, strikethrough,
/// ligatures and no-break switches.
pub(crate) fn basic(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    let lang = app.ui.language.as_str();
    let fonts = crate::panels::fonts(app);
    let english = app.session.prefs.show_font_names_in_english;
    let menu = crate::panels::font_menu(app);
    let family = cf.get(d, "fontFamily").as_str().unwrap_or("").to_owned();
    let style = cf.get(d, "fontStyle").as_str().unwrap_or("").to_owned();
    let kerning = [(json!({"kind": "metrics"}), "Metrics"), (json!({"kind": "optical"}), "Optical"), (json!({"kind": "none"}), "0")]
        .map(|(value, caption)| (value, tr(lang, caption)));
    let cases = [("normal", "Normal"), ("allCaps", "All Caps"), ("smallCaps", "Small Caps"), ("openTypeAllSmallCaps", "OpenType All Small Caps")]
        .map(|(value, caption)| (json!(value), tr(lang, caption)));
    let positions = [
        ("normal", "Normal"),
        ("superscript", "Superscript"),
        ("subscript", "Subscript"),
        ("otSuperscript", "OpenType Superscript"),
        ("otSubscript", "OpenType Subscript"),
        ("otNumerator", "OpenType Numerator"),
        ("otDenominator", "OpenType Denominator"),
    ]
    .map(|(value, caption)| (json!(value), tr(lang, caption)));
    Stack::new().id_salt("char_basic").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(lang, "Basic Character Formats")).id_salt("char_basic_fields").show(ui, |ui, _| {
            field(ui, &tr(lang, "Font Family:"), "font_family", |ui| {
                let mut entries: Vec<Entry<String>> = Vec::new();
                if cf.sparse {
                    entries.push(Entry::new(String::new(), BLANK));
                }
                entries.extend(menu.iter().map(|f| Entry::new(f.family.clone(), f.label(english).to_owned())));
                let out = Select::new(tr(lang, "Font Family:"), Bind::given(family.clone()), &entries)
                    .searchable()
                    .keeps_name(true)
                    .id_salt("char_font_family")
                    .show(ui);
                if let Some(picked) = out.events.committed().cloned().filter(|picked| *picked != family) {
                    cf.set(d, "fontFamily", if picked.is_empty() { Value::Null } else { json!(picked) });
                }
            });
            field(ui, &tr(lang, "Font Style:"), "font_style", |ui| {
                if cf.sparse && family.is_empty() {
                    // No family to list styles of: type the style ("Italic" in whatever font the text has).
                    let out = TextField::new(tr(lang, "Font Style:"), Bind::given(style.clone())).id_salt("char_font_style_text").show(ui);
                    if let Some(typed) = out.events.landed().filter(|typed| **typed != style) {
                        cf.set(d, "fontStyle", if typed.trim().is_empty() { Value::Null } else { json!(typed) });
                    }
                } else {
                    let _ = fonts.face(&family, &style);
                    let mut entries: Vec<Entry<String>> = Vec::new();
                    if cf.sparse {
                        entries.push(Entry::new(String::new(), BLANK));
                    }
                    entries.extend(fonts.styles(&family).into_iter().map(|s| Entry::new(s.clone(), s)));
                    let out = Select::new(tr(lang, "Font Style:"), Bind::given(style.clone()), &entries)
                        .keeps_name(true)
                        .id_salt("char_font_style")
                        .show(ui);
                    if let Some(picked) = out.events.committed().cloned().filter(|picked| *picked != style) {
                        cf.set(d, "fontStyle", if picked.is_empty() { Value::Null } else { json!(picked) });
                    }
                }
            });
            field(ui, &tr(lang, "Size:"), "size", |ui| scaled(ui, d, cf, "size", &tr(lang, "Size:"), ("pt", 1.0), (0.1, 1296.0)));
            field(ui, &tr(lang, "Leading:"), "leading", |ui| {
                let shown = match cf.get(d, "leading") {
                    v if v["kind"] == "points" => v["value"].as_f64(),
                    _ => None,
                };
                number(
                    ui,
                    d,
                    cf,
                    "leading",
                    Spec { label: &tr(lang, "Leading:"), shown, unit: Some("pt"), range: (0.0, 5000.0) },
                    |n| json!({"kind": "points", "value": n}),
                );
            });
            field(ui, &tr(lang, "Kerning:"), "kerning", |ui| choice(ui, d, cf, "kerning", &tr(lang, "Kerning:"), &kerning));
            field(ui, &tr(lang, "Tracking:"), "tracking", |ui| {
                let shown = cf.get(d, "tracking").as_f64();
                number(ui, d, cf, "tracking", Spec { label: &tr(lang, "Tracking:"), shown, unit: None, range: (-1000.0, 10000.0) }, |n| json!(n));
            });
            field(ui, &tr(lang, "Case:"), "case", |ui| choice(ui, d, cf, "capitalization", &tr(lang, "Case:"), &cases));
            field(ui, &tr(lang, "Position:"), "position", |ui| choice(ui, d, cf, "position", &tr(lang, "Position:"), &positions));
        });
        for (key, caption) in [("underline", "Underline"), ("ligatures", "Ligatures"), ("strikethrough", "Strikethrough"), ("noBreak", "No Break")] {
            toggle(ui, d, cf, key, &tr(lang, caption));
        }
    });
}

/// Advanced Character Formats: scaling, baseline shift, skew, language and digits.
pub(crate) fn advanced(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    let lang = app.ui.language.as_str();
    let languages: Vec<(Value, String)> = crate::panels::properties::LANGUAGES.iter().map(|l| (json!(l), (*l).to_owned())).collect();
    let digits: Vec<(Value, String)> =
        crate::panels::properties::DIGIT_OPTIONS.iter().map(|o| (json!(o.0), crate::panels::properties::digit_label(lang, *o))).collect();
    Stack::new().id_salt("char_advanced").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(lang, "Advanced Character Formats")).id_salt("char_advanced_fields").show(ui, |ui, _| {
            field(ui, &tr(lang, "Horizontal Scale:"), "h_scale", |ui| {
                scaled(ui, d, cf, "hScale", &tr(lang, "Horizontal Scale:"), ("%", 100.0), (1.0, 1000.0))
            });
            field(ui, &tr(lang, "Vertical Scale:"), "v_scale", |ui| {
                scaled(ui, d, cf, "vScale", &tr(lang, "Vertical Scale:"), ("%", 100.0), (1.0, 1000.0))
            });
            field(ui, &tr(lang, "Baseline Shift:"), "baseline_shift", |ui| {
                scaled(ui, d, cf, "baselineShift", &tr(lang, "Baseline Shift:"), ("pt", 1.0), (-5000.0, 5000.0))
            });
            field(ui, &tr(lang, "Skew:"), "skew", |ui| scaled(ui, d, cf, "skew", &tr(lang, "Skew:"), ("\u{b0}", 1.0), (-85.0, 85.0)));
            field(ui, &tr(lang, "Language:"), "language", |ui| choice(ui, d, cf, "language", &tr(lang, "Language:"), &languages));
            // Digits (World-Ready): Farsi draws 0-9 as Persian digits.
            field(ui, &tr(lang, "Digits:"), "digits", |ui| choice(ui, d, cf, "digits", &tr(lang, "Digits:"), &digits));
        });
    });
}

/// Character Color: the fill or stroke swatch, its tint and the stroke weight.
pub(crate) fn color(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document) {
    let lang = app.ui.language.as_str();
    let targets = [Entry::new("fill".to_owned(), tr(lang, "Fill")), Entry::new("stroke".to_owned(), tr(lang, "Stroke"))];
    Stack::new().id_salt("char_color").fill_width().show(ui, |ui, _| {
        let target = if d.s("colorTarget") == "stroke" { "stroke" } else { "fill" };
        let out = Segmented::new(tr(lang, "Character Color"), Bind::given(target.to_owned()), &targets).id_salt("char_color_target").show(ui);
        if let Some(next) = out.events.committed().cloned().filter(|next| next != target) {
            d.fields.insert("colorTarget".into(), json!(next));
        }
        let stroke = d.s("colorTarget") == "stroke";
        let key = if stroke { "stroke" } else { "fill" };
        FieldGroup::new(tr(lang, "Character Color")).id_salt(("char_color_fields", key)).show(ui, |ui, _| {
            field(ui, &tr(lang, "Color:"), "swatch", |ui| swatch(ui, d, cf, doc, key, &tr(lang, "Color:"), None));
            field(ui, &tr(lang, "Tint:"), "tint", |ui| {
                scaled(ui, d, cf, if stroke { "strokeTint" } else { "fillTint" }, &tr(lang, "Tint:"), ("%", 100.0), (0.0, 100.0))
            });
            field(ui, &tr(lang, "Weight:"), "weight", |ui| scaled(ui, d, cf, "strokeWeight", &tr(lang, "Weight:"), ("pt", 1.0), (0.0, 800.0)));
        });
        toggle(ui, d, cf, "strokeBehindFill", &tr(lang, "Stroke Behind Fill"));
    });
}

/// OpenType Features: the feature switches, figure style and stylistic sets (one `otfFeatures` list: changing any feature
/// sets the whole list).
pub(crate) fn open_type(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    use designcraft_doc::otf;
    let lang = app.ui.language.as_str();
    let mut list: Vec<String> =
        cf.get(d, "otfFeatures").as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let mut changed = false;
    let figures: Vec<Entry<String>> = otf::FIGURES.iter().map(|(id, label, ..)| Entry::new((*id).to_owned(), tr(lang, label))).collect();
    Stack::new().id_salt("char_open_type").fill_width().show(ui, |ui, _| {
        for (tag, label, _) in otf::TOGGLES {
            let on = otf::is_on(&list, tag);
            let out = Checkbox::new(Bind::given(on)).label(tr(lang, label)).id_salt(("char_otf", *tag)).show(ui);
            if let Some(next) = out.events.committed().copied().filter(|next| *next != on) {
                otf::set(&mut list, tag, next);
                changed = true;
            }
        }
        let figure = otf::figures(&list).to_owned();
        field(ui, &tr(lang, "Figure Style:"), "figure_style", |ui| {
            let out = Select::new(tr(lang, "Figure Style:"), Bind::given(figure.clone()), &figures).keeps_name(true).id_salt("char_figures").show(ui);
            if let Some(picked) = out.events.committed().cloned().filter(|picked| *picked != figure)
                && otf::set_figures(&mut list, &picked)
            {
                changed = true;
            }
        });
        message(ui, &tr(lang, "Stylistic Sets"));
        let mask = otf::stylistic_sets(&list);
        Row::new().id_salt("char_stylistic_sets").wrap(true).show(ui, |ui, _| {
            for n in 1..=20u32 {
                let bit = 1u32 << (n - 1);
                let out = ToggleButton::new(n.to_string(), Bind::given(mask & bit != 0)).id_salt(("char_stylistic_set", n)).show(ui);
                if out.events.committed().is_some() {
                    otf::set_stylistic_sets(&mut list, mask ^ bit);
                    changed = true;
                }
            }
        });
    });
    if changed {
        cf.set(d, "otfFeatures", json!(list));
    }
}

/// Underline Options / Strikethrough Options (`key` is `underline` or `strikethrough`).
pub(crate) fn line(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document, key: &str) {
    let lang = app.ui.language.as_str();
    Stack::new().id_salt(("char_line", key.to_owned())).fill_width().show(ui, |ui, _| {
        toggle(ui, d, cf, key, &tr(lang, if key == "underline" { "Underline On" } else { "Strikethrough On" }));
        FieldGroup::new(tr(lang, if key == "underline" { "Underline Options" } else { "Strikethrough Options" }))
            .id_salt(("char_line_fields", key.to_owned()))
            .show(ui, |ui, _| {
                field(ui, &tr(lang, "Weight:"), "weight", |ui| {
                    scaled(ui, d, cf, &format!("{key}Weight"), &tr(lang, "Weight:"), ("pt", 1.0), (0.0, 800.0))
                });
                field(ui, &tr(lang, "Offset:"), "offset", |ui| {
                    scaled(ui, d, cf, &format!("{key}Offset"), &tr(lang, "Offset:"), ("pt", 1.0), (-800.0, 800.0))
                });
                field(ui, &tr(lang, "Color:"), "color", |ui| {
                    swatch(ui, d, cf, doc, &format!("{key}Color"), &tr(lang, "Color:"), Some(tr(lang, "(Text Color)")))
                });
                field(ui, &tr(lang, "Tint:"), "tint", |ui| scaled(ui, d, cf, &format!("{key}Tint"), &tr(lang, "Tint:"), ("%", 100.0), (0.0, 100.0)));
            });
    });
}
