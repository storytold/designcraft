//! The CJK sections of Paragraph Style Options and Character Style Options, and the Ruby dialog, on the craft
//! framework: Auto Tate-chu-yoko, Tate-chu-yoko, the four ruby sections, Kenten, Kenten Color, Shatai, Japanese
//! Composition, Grid and Warichu settings. They edit `c.<attr>` and `p.<attr>` draft fields like the other sections;
//! the section list and the commands stay `style_cjk`'s and the controller's.

use super::style_kit::{Counts, Extra, Num, captioned, field, heading, line, note, number, pick, swatch_pick, text, toggle};
use crate::DesignApp;
use crate::dialogs::style_cjk::{RUBY_SECTIONS, kinsoku_sets, mojikumi_sets, pick_kenten};
use crate::dialogs::{CharFields, Dialog};
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::contract::{Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::runtime::env::UnitKind;
use designcraft_doc::cjk_settings::KentenKind;
use egui::Ui;
use serde_json::{Value, json};

/// The blank entry of a character style's menu: the attribute is not set.
const UNSET: &str = "\u{1}";

fn tr(app: &DesignApp, s: &str) -> String {
    crate::i18n::tr(&app.ui.language, s).to_string()
}

/// How a number attribute is shown and kept.
struct Spec {
    /// The shown number is the attribute times this (100 for a ratio shown as a percentage).
    scale: f64,
    range: (f64, f64),
    counts: Counts,
    /// Stored as a whole number.
    whole: bool,
    /// Emptying the field unsets the attribute (always for a character style; for an automatic value in either).
    auto: bool,
}

impl Spec {
    fn points(range: (f64, f64)) -> Self {
        Spec { scale: 1.0, range, counts: Counts::Kind(UnitKind::Points), whole: false, auto: false }
    }

    fn percent(range: (f64, f64), scale: f64) -> Self {
        Spec { scale, range, counts: Counts::Kind(UnitKind::Percent), whole: false, auto: false }
    }

    fn degrees(range: (f64, f64)) -> Self {
        Spec { scale: 1.0, range, counts: Counts::Kind(UnitKind::Degrees), whole: false, auto: false }
    }

    fn count(range: (f64, f64)) -> Self {
        Spec { scale: 1.0, range, counts: Counts::Plain, whole: true, auto: false }
    }

    fn automatic(mut self) -> Self {
        self.auto = true;
        self
    }
}

/// A number attribute.
fn cnum(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str, spec: &Spec) {
    let shown = cf.get(d, key).as_f64().map(|x| x * spec.scale);
    let num = Num { range: spec.range, whole: spec.whole, counts: spec.counts.clone() };
    let Some(next) = number(ui, &format!("c.{key}"), label, shown, &num, true) else { return };
    match next.filter(|n| n.is_finite()) {
        Some(n) if spec.whole => cf.set(d, key, json!(n.round().clamp(spec.range.0, spec.range.1) as u64)),
        Some(n) => cf.set(d, key, json!(n / spec.scale)),
        None if cf.sparse || spec.auto => cf.set(d, key, Value::Null),
        None => {}
    }
}

/// An attribute chosen from `options` (value, label); a character style also has a blank entry.
fn cchoice(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str, options: &[(Value, &str)]) {
    let current = cf.get(d, key);
    let mut entries: Vec<(String, String)> = Vec::new();
    if cf.sparse {
        entries.push((UNSET.to_owned(), "\u{2014}".to_owned()));
    }
    entries.extend(options.iter().map(|(v, caption)| (v.to_string(), tr(app, caption))));
    let shown = if current.is_null() && cf.sparse { UNSET.to_owned() } else { current.to_string() };
    if let Some(next) = pick(ui, &format!("c.{key}"), label, &shown, &entries, true) {
        cf.set(d, key, if next == UNSET { Value::Null } else { serde_json::from_str(&next).unwrap_or(Value::Null) });
    }
}

/// An on/off attribute; for a character style a third, mixed state leaves it unset (clicks go unset → on → off →
/// unset).
fn ccheck(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str, enabled: bool) {
    let cur = cf.get(d, key);
    let shown = if cf.sparse && cur.is_null() { None } else { Some(cur.as_bool().unwrap_or(false)) };
    if toggle(ui, &format!("c.{key}"), label, shown, enabled).is_some() {
        let next = match cur.as_bool() {
            _ if !cf.sparse => json!(!cur.as_bool().unwrap_or(false)),
            None => json!(true),
            Some(true) => json!(false),
            Some(false) => Value::Null,
        };
        cf.set(d, key, next);
    }
}

/// A swatch attribute; "" is the text's own colour.
fn cswatch(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document, key: &str, label: &str) {
    let current = cf.get(d, key);
    let text_color = tr(app, "Text Color");
    let blank = "\u{2014}";
    let extra = Extra { text: Some(("", text_color.as_str())), blank: cf.sparse.then_some((UNSET, blank)) };
    let shown = if current.is_null() && cf.sparse { UNSET.to_owned() } else { current.as_str().unwrap_or("").to_owned() };
    if let Some(next) = swatch_pick(ui, &format!("c.{key}"), label, doc, &shown, &extra, true) {
        cf.set(d, key, if next == UNSET { Value::Null } else { json!(next) });
    }
}

/// An adornment's overprint (`auto`, `on` or `off`): checked is `on`; a character style also has the unset state.
fn overprint(ui: &mut Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str) {
    let cur = cf.get(d, key);
    let shown = if cf.sparse && cur.is_null() { None } else { Some(cur.as_str() == Some("on")) };
    if toggle(ui, &format!("c.{key}"), label, shown, true).is_some() {
        let next = match cur.as_str() {
            _ if !cf.sparse => json!(if cur.as_str() == Some("on") { "auto" } else { "on" }),
            None => json!("on"),
            Some("on") => json!("auto"),
            Some(_) => Value::Null,
        };
        cf.set(d, key, next);
    }
}

/// A font family and style pair of attributes; an empty family is the parent text's font.
fn font_fields(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields, keys: (&str, &str), enabled: bool) {
    let fonts = crate::panels::fonts(app);
    let menu = crate::panels::font_menu(app);
    let (fam_key, style_key) = keys;
    let raw = cf.get(d, fam_key);
    let fam = raw.as_str().unwrap_or("").to_owned();
    let mut families: Vec<(String, String)> = Vec::new();
    if cf.sparse {
        families.push((UNSET.to_owned(), "\u{2014}".to_owned()));
    }
    families.push((String::new(), tr(app, "Parent's Font")));
    families.extend(menu.iter().map(|f| (f.family.clone(), f.family.clone())));
    let shown = if raw.is_null() && cf.sparse { UNSET.to_owned() } else { fam.clone() };
    let styles: Vec<(String, String)> = fonts.styles(&fam).into_iter().map(|s| (s.clone(), s)).collect();
    let (family_label, style_label) = (tr(app, "Font Family:"), tr(app, "Font Style:"));
    field(ui, &family_label, fam_key, |ui| {
        if let Some(next) = pick(ui, &format!("cjkfam.{fam_key}"), &family_label, &shown, &families, enabled) {
            if next == UNSET {
                cf.set(d, fam_key, Value::Null);
                cf.set(d, style_key, Value::Null);
            } else if next.is_empty() {
                cf.set(d, fam_key, json!(""));
                cf.set(d, style_key, json!(""));
            } else {
                cf.set(d, fam_key, json!(next));
            }
        }
    });
    field(ui, &style_label, style_key, |ui| {
        let current = cf.get(d, style_key).as_str().unwrap_or("").to_owned();
        if let Some(next) = pick(ui, &format!("cjksty.{style_key}"), &style_label, &current, &styles, enabled && !fam.is_empty()) {
            cf.set(d, style_key, json!(next));
        }
    });
}

/// The CJK section `id`. `para` holds the paragraph style's resolved paragraph attributes; it is `None` in Character
/// Style Options and the Ruby dialog, which have no paragraph-only sections.
pub(crate) fn page(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, id: &str, cf: &CharFields, para: Option<&Value>, doc: &designcraft_doc::Document) {
    Stack::new().id_salt(("cjk_page", id.to_owned())).fill_width().show(ui, |ui, _| match (id, para) {
        ("cjk.tcy", _) => tate_chu_yoko(app, ui, d, cf),
        ("cjk.rubyPlacement", _) => ruby_placement(app, ui, d, cf),
        ("cjk.rubyFont", _) => ruby_font(app, ui, d, cf),
        ("cjk.rubyLonger", _) => ruby_longer(app, ui, d, cf),
        ("cjk.rubyColor", _) => adornment_color(app, ui, d, cf, doc, "ruby"),
        ("cjk.kenten", _) => kenten(app, ui, d, cf),
        ("cjk.kentenColor", _) => adornment_color(app, ui, d, cf, doc, "kenten"),
        ("cjk.shatai", _) => shatai(app, ui, d, cf),
        ("cjk.warichu", _) => warichu(app, ui, d, cf),
        ("cjk.autoTcy", Some(pv)) => auto_tate_chu_yoko(app, ui, d, pv),
        ("cjk.composition", Some(pv)) => japanese_composition(app, ui, d, cf, pv, doc),
        ("cjk.grid", Some(pv)) => grid_settings(app, ui, d, pv),
        _ => {}
    });
}

/// A labelled field over a character attribute number.
fn nfield(ui: &mut Ui, app: &DesignApp, d: &mut Dialog, cf: &CharFields, caption: &str, key: &str, spec: &Spec) {
    let label = tr(app, caption);
    field(ui, &label, key, |ui| cnum(ui, d, cf, key, &label, spec));
}

/// A labelled field over a character attribute chosen from a list.
fn cfield(ui: &mut Ui, app: &DesignApp, d: &mut Dialog, cf: &CharFields, caption: &str, key: &str, options: &[(Value, &str)]) {
    let label = tr(app, caption);
    field(ui, &label, key, |ui| cchoice(app, ui, d, cf, key, &label, options));
}

/// Tate-chu-yoko Settings: the switch and the run's offsets.
fn tate_chu_yoko(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    ccheck(ui, d, cf, "tateChuYoko", &tr(app, "Tate-Chu-Yoko"), true);
    FieldGroup::new(tr(app, "Tate-chu-yoko Settings")).id_salt("cjk_tcy").show(ui, |ui, _| {
        nfield(ui, app, d, cf, "Up/Down Position:", "tateChuYokoYOffset", &Spec::points((-1000.0, 1000.0)));
        nfield(ui, app, d, cf, "Left/Right Position:", "tateChuYokoXOffset", &Spec::points((-1000.0, 1000.0)));
    });
}

/// Ruby Placement and Spacing: type, alignment, position and offsets.
fn ruby_placement(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    FieldGroup::new(tr(app, "Ruby Placement and Spacing")).id_salt("cjk_ruby_place").show(ui, |ui, _| {
        cfield(ui, app, d, cf, "Ruby Type:", "rubyType", &[(json!("perCharacter"), "Per-Character Ruby"), (json!("group"), "Group Ruby")]);
        cfield(
            ui,
            app,
            d,
            cf,
            "Alignment:",
            "rubyAlignment",
            &[
                (json!("left"), "Left/Top"),
                (json!("center"), "Centered"),
                (json!("right"), "Right/Bottom"),
                (json!("fullJustify"), "Full Justify"),
                (json!("jis"), "1-2-1 (JIS) Rule"),
                (json!("equalAki"), "Equal Aki"),
                (json!("oneAki"), "1 Ruby Character Aki"),
            ],
        );
        cfield(ui, app, d, cf, "Position:", "rubyPosition", &[(json!("aboveRight"), "Above/Right"), (json!("belowLeft"), "Below/Left")]);
    });
    GroupBox::new(tr(app, "Offset from Parent"), None).id_salt("cjk_ruby_offset").show(ui, |ui, _| {
        nfield(ui, app, d, cf, "Across the Line:", "rubyYOffset", &Spec::points((-1000.0, 1000.0)));
        nfield(ui, app, d, cf, "Along the Line:", "rubyXOffset", &Spec::points((-1000.0, 1000.0)));
    });
}

/// Ruby Font and Size: font, size (empty: half the parent's), scales, OpenType ruby glyphs and tate-chu-yoko of digits
/// inside vertical ruby.
fn ruby_font(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    FieldGroup::new(tr(app, "Ruby Font and Size")).id_salt("cjk_ruby_font").show(ui, |ui, _| {
        font_fields(app, ui, d, cf, ("rubyFont", "rubyFontStyle"), true);
        nfield(ui, app, d, cf, "Size:", "rubyFontSize", &Spec::points((0.1, 1296.0)).automatic());
        nfield(ui, app, d, cf, "Horizontal Scale:", "rubyXScale", &Spec::percent((1.0, 1000.0), 100.0));
        nfield(ui, app, d, cf, "Vertical Scale:", "rubyYScale", &Spec::percent((1.0, 1000.0), 100.0));
    });
    ccheck(ui, d, cf, "rubyOpenTypePro", &tr(app, "Use OpenType Pro Ruby Glyphs"), true);
    FieldGroup::new(tr(app, "Tate-chu-yoko Digits:")).id_salt("cjk_ruby_tcy").show(ui, |ui, _| {
        nfield(ui, app, d, cf, "Tate-chu-yoko Digits:", "rubyAutoTcyDigits", &Spec::count((0.0, 9.0)));
    });
    ccheck(ui, d, cf, "rubyAutoTcyIncludeRoman", &tr(app, "Include Roman Characters"), true);
    ccheck(ui, d, cf, "rubyAutoTcyAutoScale", &tr(app, "Fit to Width"), true);
}

/// Adjustment When Ruby Is Longer Than Parent: overhang, parent spacing, automatic narrowing and alignment with the
/// line edges.
fn ruby_longer(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    FieldGroup::new(tr(app, "Adjustment When Ruby Is Longer Than Parent")).id_salt("cjk_ruby_longer").show(ui, |ui, _| {
        cfield(
            ui,
            app,
            d,
            cf,
            "Ruby Overhang:",
            "rubyOverhangAmount",
            &[
                (json!("none"), "None"),
                (json!("oneRuby"), "1 Ruby Character"),
                (json!("halfRuby"), "1/2 Ruby Character"),
                (json!("oneChar"), "1 Parent Character"),
                (json!("halfChar"), "1/2 Parent Character"),
                (json!("noLimit"), "Unlimited"),
            ],
        );
        cfield(
            ui,
            app,
            d,
            cf,
            "Parent Character Spacing:",
            "rubyParentSpacing",
            &[
                (json!("noAdjustment"), "No Adjustment"),
                (json!("bothSides"), "Both Sides"),
                (json!("aki121"), "1-2-1 Aki"),
                (json!("equalAki"), "Equal Aki"),
                (json!("fullJustify"), "Full Justify"),
            ],
        );
    });
    ccheck(ui, d, cf, "rubyAutoScaling", &tr(app, "Shrink Ruby Width Automatically"), true);
    let on = cf.get(d, "rubyAutoScaling").as_bool() == Some(true);
    let label = tr(app, "Minimum Scale:");
    field(ui, &label, "rubyScalingMin", |ui| {
        if on {
            cnum(ui, d, cf, "rubyScalingMin", &label, &Spec::percent((10.0, 100.0), 100.0));
        } else {
            // Dimmed while shrinking is off.
            let _ = number(ui, "c.rubyScalingMinOff", &label, cf.get(d, "rubyScalingMin").as_f64().map(|v| v * 100.0), &Num::decimal(10.0, 100.0).counting(Counts::Kind(UnitKind::Percent)), false);
        }
    });
    ccheck(ui, d, cf, "rubyAutoAlign", &tr(app, "Auto Align at Line Start/End"), true);
}

/// Ruby Color / Kenten Color (`prefix` "ruby" or "kenten"): the fill or stroke swatch (Text Color: the text's own),
/// its tint and the stroke weight (empty: the text's), and overprint.
fn adornment_color(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document, prefix: &str) {
    use craft_ui_widgets::choice::{Radio, RadioGroup};
    use craft_ui_widgets::contract::Bind;
    let target = format!("cjk.{prefix}ColorTarget");
    let stroke = d.s(&target) == "stroke";
    let part = if stroke { "Stroke" } else { "Fill" };
    let key = format!("{prefix}{part}");
    let (fill_label, stroke_label, swatch_label) = (tr(app, "Fill"), tr(app, "Stroke"), tr(app, if stroke { "Stroke" } else { "Fill" }));
    Stack::new().id_salt(("cjk_adornment_color", prefix.to_owned())).fill_width().show(ui, |ui, _| {
        let shown = RadioGroup::new(tr(app, "Color Target"), Bind::given(if stroke { "stroke" } else { "fill" }.to_owned())).id_salt(("cjk_color_target", prefix.to_owned())).show(ui, |ui, _| {
            Row::new().show(ui, |ui, _| {
                Radio::new("fill".to_owned()).label(fill_label.clone()).show(ui);
                Radio::new("stroke".to_owned()).label(stroke_label.clone()).show(ui);
            });
        });
        if let Some(next) = shown.output.events.committed().cloned() {
            d.fields.insert(target.clone(), json!(next));
        }
        field(ui, &swatch_label, &key, |ui| cswatch(app, ui, d, cf, doc, &key, &swatch_label));
        FieldGroup::new(tr(app, "Color")).id_salt(("cjk_color_fields", prefix.to_owned())).show(ui, |ui, _| {
            nfield(ui, app, d, cf, "Tint:", &format!("{prefix}{part}Tint"), &Spec::percent((0.0, 100.0), 100.0).automatic());
            nfield(ui, app, d, cf, "Weight:", &format!("{prefix}StrokeWeight"), &Spec::points((0.0, 800.0)).automatic());
        });
        overprint(ui, d, cf, &format!("{prefix}OverprintFill"), &tr(app, "Overprint Fill"));
        overprint(ui, d, cf, &format!("{prefix}OverprintStroke"), &tr(app, "Overprint Stroke"));
    });
}

/// Kenten Type menu labels, in [`KentenKind::ALL`] order.
fn kenten_label(kind: KentenKind) -> &'static str {
    match kind {
        KentenKind::SesameDot => "Sesame Dot",
        KentenKind::WhiteSesameDot => "White Sesame Dot",
        KentenKind::Fisheye => "Fisheye",
        KentenKind::BlackCircle => "Black Circle",
        KentenKind::SmallBlackCircle => "Small Black Circle",
        KentenKind::Bullseye => "Double Circle",
        KentenKind::BlackTriangle => "Black Triangle",
        KentenKind::WhiteTriangle => "White Triangle",
        KentenKind::WhiteCircle => "White Circle",
        KentenKind::SmallWhiteCircle => "Small White Circle",
        KentenKind::Custom => "Custom",
    }
}

/// The kind the menu shows for the stored kind and character. Documents made before kinds existed store a preset as its
/// character with the default kind.
fn shown_kenten_kind(kind: Option<KentenKind>, character: &str) -> KentenKind {
    match kind {
        Some(k) if k != KentenKind::SesameDot => k,
        _ if character.is_empty() || character == "\u{FE45}" => KentenKind::SesameDot,
        _ => KentenKind::ALL.into_iter().find(|k| k.mark() == Some(character)).unwrap_or(KentenKind::Custom),
    }
}

/// Kenten Settings: distance, position, size, alignment, scales, the kind, and for a custom mark its font and
/// characters.
fn kenten(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    use designcraft_doc::cjk_settings::KENTEN_MARK_MAX_CHARS;
    let on = cf.get(d, "kenten").as_bool();
    let stored: Option<KentenKind> = serde_json::from_value(cf.get(d, "kentenKind")).ok();
    let character = cf.get(d, "kentenCharacter").as_str().unwrap_or("").to_owned();
    let kind = shown_kenten_kind(stored, &character);
    let custom = on == Some(true) && kind == KentenKind::Custom;
    let mut types: Vec<(String, String)> = Vec::new();
    if cf.sparse {
        types.push((UNSET.to_owned(), "\u{2014}".to_owned()));
    }
    types.push(("none".to_owned(), tr(app, "None")));
    types.extend(KentenKind::ALL.iter().enumerate().map(|(i, k)| (format!("kind{i}"), tr(app, kenten_label(*k)))));
    let shown = match on {
        None => UNSET.to_owned(),
        Some(false) => "none".to_owned(),
        Some(true) => KentenKind::ALL.iter().position(|k| *k == kind).map_or_else(String::new, |i| format!("kind{i}")),
    };
    let (type_label, mark_label) = (tr(app, "Kenten Type:"), tr(app, "Character:"));
    FieldGroup::new(tr(app, "Kenten Settings")).id_salt("cjk_kenten").show(ui, |ui, _| {
        nfield(ui, app, d, cf, "Distance from Parent:", "kentenDistance", &Spec::points((-1296.0, 1296.0)));
        cfield(ui, app, d, cf, "Position:", "kentenPosition", &[(json!("aboveRight"), "Above/Right"), (json!("belowLeft"), "Below/Left")]);
        nfield(ui, app, d, cf, "Size:", "kentenSize", &Spec::points((0.1, 1296.0)).automatic());
        cfield(ui, app, d, cf, "Alignment:", "kentenAlignment", &[(json!("center"), "Centered"), (json!("start"), "Left/Top")]);
        nfield(ui, app, d, cf, "Horizontal Scale:", "kentenXScale", &Spec::percent((1.0, 1000.0), 100.0));
        nfield(ui, app, d, cf, "Vertical Scale:", "kentenYScale", &Spec::percent((1.0, 1000.0), 100.0));
        field(ui, &type_label, "kentenType", |ui| {
            if let Some(next) = pick(ui, "cjkkententype", &type_label, &shown, &types, true) {
                if next == UNSET {
                    for key in ["kenten", "kentenKind", "kentenCharacter"] {
                        cf.set(d, key, Value::Null);
                    }
                } else if next == "none" {
                    pick_kenten(d, cf, None);
                } else if let Some(k) = next.strip_prefix("kind").and_then(|i| i.parse::<usize>().ok()).and_then(|i| KentenKind::ALL.get(i)) {
                    pick_kenten(d, cf, Some(*k));
                }
            }
        });
        font_fields(app, ui, d, cf, ("kentenFont", "kentenFontStyle"), custom);
        field(ui, &mark_label, "kentenCharacter", |ui| {
            // Typed directly; a mark stored with another input mode (IDML) is kept as it is.
            let current = if custom { character.clone() } else { String::new() };
            let out = craft_ui_widgets::text::TextField::new(mark_label.clone(), craft_ui_widgets::contract::Bind::given(current.clone()))
                .id_salt("cjk_kenten_character")
                .limit(None::<usize>)
                .enabled(custom)
                .show(ui);
            if let Some(next) = out.events.landed().filter(|next| **next != current) {
                let mark: String = next.chars().filter(|c| !c.is_control()).take(KENTEN_MARK_MAX_CHARS).collect();
                cf.set(d, "kentenCharacter", json!(mark));
            }
        });
    });
}

/// Shatai: magnification, angle, and the rotation and tsume adjustments.
fn shatai(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    FieldGroup::new(tr(app, "Shatai")).id_salt("cjk_shatai").show(ui, |ui, _| {
        nfield(ui, app, d, cf, "Magnification:", "shataiMagnification", &Spec::percent((0.0, 90.0), 1.0));
        nfield(ui, app, d, cf, "Angle:", "shataiAngle", &Spec::degrees((0.0, 180.0)).automatic());
    });
    ccheck(ui, d, cf, "shataiAdjustRotation", &tr(app, "Adjust Rotation"), true);
    ccheck(ui, d, cf, "shataiAdjustTsume", &tr(app, "Adjust Tsume"), true);
}

/// A paragraph attribute as shown: the dialog's `p.<key>` edit, else the resolved value.
fn pval(d: &Dialog, pv: &Value, key: &str) -> Value {
    super::para_sections::val(d, "p.", pv, key)
}

/// A paragraph attribute chosen from `options` (value, label); `current` is the value shown.
fn pchoice(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, current: &str, options: &[(&str, &str)]) {
    let entries = captioned(&app.ui.language, options);
    if let Some(next) = pick(ui, &format!("cjkp.{key}"), label, current, &entries, true) {
        d.fields.insert(format!("p.{key}"), json!(next));
    }
}

fn pcheck(ui: &mut Ui, d: &mut Dialog, pv: &Value, key: &str, label: &str) {
    if let Some(on) = toggle(ui, &format!("p.{key}"), label, pval(d, pv, key).as_bool().or(Some(false)), true) {
        d.fields.insert(format!("p.{key}"), json!(on));
    }
}

/// Auto Tate-chu-yoko Settings: the longest digit run set across the line, and whether roman letters count.
fn auto_tate_chu_yoko(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value) {
    let label = tr(app, "Tate-chu-yoko Digits:");
    FieldGroup::new(tr(app, "Auto Tate-chu-yoko Settings")).id_salt("cjk_auto_tcy").show(ui, |ui, _| {
        field(ui, &label, "autoTcy", |ui| {
            let max = f64::from(designcraft_compose::shape::AUTO_TCY_MAX);
            if let Some(Some(n)) = number(ui, "p.autoTcy", &label, pval(d, pv, "autoTcy").as_f64(), &Num::whole(0.0, max), true) {
                d.fields.insert("p.autoTcy".into(), json!(n.round().clamp(0.0, max) as u64));
            }
        });
    });
    pcheck(ui, d, pv, "autoTcyIncludeRoman", &tr(app, "Include Roman Characters"));
}

/// Warichu Settings: the switch, lines, size, spacing, alignment and line break minimums.
fn warichu(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields) {
    ccheck(ui, d, cf, "warichu", &tr(app, "Warichu"), true);
    FieldGroup::new(tr(app, "Warichu Settings")).id_salt("cjk_warichu").show(ui, |ui, _| {
        nfield(ui, app, d, cf, "Lines:", "warichuLines", &Spec::count((2.0, 16.0)));
        nfield(ui, app, d, cf, "Warichu Size:", "warichuSize", &Spec::percent((1.0, 1000.0), 1.0));
        nfield(ui, app, d, cf, "Line Spacing:", "warichuLineSpacing", &Spec::points((-1000.0, 1000.0)));
        cfield(
            ui,
            app,
            d,
            cf,
            "Alignment:",
            "warichuAlignment",
            &[(json!("auto"), "Auto"), (json!("left"), "Left"), (json!("center"), "Center"), (json!("right"), "Right"), (json!("justify"), "Justify")],
        );
    });
    GroupBox::new(tr(app, "Line Break Options"), None).id_salt("cjk_warichu_break").show(ui, |ui, _| {
        nfield(ui, app, d, cf, "Characters Before Break:", "warichuCharsBeforeBreak", &Spec::count((0.0, 100.0)));
        nfield(ui, app, d, cf, "Characters After Break:", "warichuCharsAfterBreak", &Spec::count((0.0, 100.0)));
    });
}

/// IDML's names for "no mojikumi set".
fn mojikumi_value(v: &str) -> &str {
    if matches!(v, "Nothing" | "None") { "" } else { v }
}

/// The built-in kinsoku sets' labels.
fn kinsoku_label(app: &DesignApp, name: &str) -> String {
    const BUILT_IN: &[(&str, &str)] = &[
        ("HardKinsoku", "Hard Kinsoku"),
        ("SoftKinsoku", "Soft Kinsoku"),
        ("SimplifiedChineseKinsoku", "Simplified Chinese Kinsoku"),
        ("TraditionalChineseKinsoku", "Traditional Chinese Kinsoku"),
        ("KoreanKinsoku", "Korean Kinsoku"),
    ];
    if name.is_empty() {
        return tr(app, "None");
    }
    BUILT_IN.iter().find(|k| k.0 == name).map_or_else(|| name.to_owned(), |k| tr(app, k.1))
}

/// Japanese Composition Settings: kinsoku, hanging, bunri-kinshi, mojikumi, the leading model, rensuuji, rotated roman in
/// vertical text, line-end ideographic spaces and roman word breaks.
fn japanese_composition(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, cf: &CharFields, pv: &Value, doc: &designcraft_doc::Document) {
    const DEFAULT_RULES: &str = "\u{2}default";
    let sets = kinsoku_sets(doc);
    // `null`: no set given, so the composer's default rules apply.
    let name = pval(d, pv, "kinsoku").get("name").and_then(Value::as_str).map(str::to_owned);
    let mut kinsoku_entries = vec![(DEFAULT_RULES.to_owned(), tr(app, "Default Rules"))];
    kinsoku_entries.extend(sets.iter().map(|k| (k.name.clone(), kinsoku_label(app, &k.name))));
    let kinsoku_shown = name.clone().unwrap_or_else(|| DEFAULT_RULES.to_owned());
    let kind = pval(d, pv, "kinsokuType");
    let kind = match kind.as_str().unwrap_or("") {
        "KinsokuPushInFirst" => "",
        k => k,
    }
    .to_owned();
    let hang = pval(d, pv, "kinsokuHang").as_str().unwrap_or("none").to_owned();
    let raw = pval(d, pv, "mojikumi");
    let current = mojikumi_value(raw.as_str().unwrap_or("")).to_owned();
    let moji: Vec<(String, String)> =
        mojikumi_sets(doc, &current).into_iter().map(|(v, l)| (v, if l.is_empty() { tr(app, "None") } else { l })).collect();
    // Space below (`akiBelow`) measures the leading from the em box top, space above from its bottom
    // (`cjk_line_reference` in compose).
    let mut leading = vec![
        (json!("akiBelow"), "Em Box Top/Right"),
        (json!("center"), "Em Box Center"),
        (json!("roman"), "Roman Baseline"),
        (json!("akiAbove"), "Em Box Bottom/Left"),
    ];
    // Kept from IDML; not offered otherwise.
    if cf.get(d, "leadingModel") == json!("centerDown") {
        leading.push((json!("centerDown"), "Em Box Center (Down)"));
    }
    let (kinsoku_label_text, type_label, hang_label, moji_label) =
        (tr(app, "Kinsoku Set:"), tr(app, "Kinsoku Type:"), tr(app, "Hanging Type:"), tr(app, "Mojikumi Set:"));
    Stack::new().id_salt("cjk_composition").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "Japanese Composition Settings")).id_salt("cjk_composition_fields").show(ui, |ui, _| {
            field(ui, &kinsoku_label_text, "kinsoku", |ui| {
                if let Some(next) = pick(ui, "cjkkinsoku", &kinsoku_label_text, &kinsoku_shown, &kinsoku_entries, true) {
                    // `style.paragraph.edit` takes `null` as the default rules, also over a Based On set.
                    if next == DEFAULT_RULES {
                        d.fields.insert("p.kinsoku".into(), Value::Null);
                    } else if let Some(v) = sets.iter().find(|k| k.name == next).and_then(|k| serde_json::to_value(k).ok()) {
                        d.fields.insert("p.kinsoku".into(), v);
                    }
                }
            });
            field(ui, &type_label, "kinsokuType", |ui| {
                pchoice(
                    app,
                    ui,
                    d,
                    "kinsokuType",
                    &type_label,
                    &kind,
                    &[
                        ("", "Push In First"),
                        ("KinsokuPushOutFirst", "Push Out First"),
                        ("KinsokuPushOutOnly", "Push Out Only"),
                        ("KinsokuPrioritizeAdjustmentAmount", "Prioritize Adjustment Amount"),
                    ],
                )
            });
            field(ui, &hang_label, "kinsokuHang", |ui| {
                pchoice(app, ui, d, "kinsokuHang", &hang_label, &hang, &[("none", "None"), ("regular", "Standard"), ("force", "Force")])
            });
        });
        pcheck(ui, d, pv, "bunriKinshi", &tr(app, "Bunri-Kinshi"));
        FieldGroup::new(tr(app, "Mojikumi Set:")).id_salt("cjk_composition_more").show(ui, |ui, _| {
            field(ui, &moji_label, "mojikumi", |ui| {
                if let Some(next) = pick(ui, "cjkmojikumi", &moji_label, &current, &moji, true) {
                    d.fields.insert("p.mojikumi".into(), json!(next));
                }
            });
            cfield(ui, app, d, cf, "Leading Model:", "leadingModel", &leading);
        });
        pcheck(ui, d, pv, "rensuuji", &tr(app, "Rensuuji"));
        pcheck(ui, d, pv, "rotateRoman", &tr(app, "Rotate Roman Characters in Vertical Text"));
        pcheck(ui, d, pv, "treatIdeographicSpaceAsSpace", &tr(app, "Absorb Ideographic Space at Line End"));
        pcheck(ui, d, pv, "romanWordBreak", &tr(app, "Roman Word Break"));
    });
}

/// Grid Settings: the point of each line that sits on the baseline grid (or none), only the first line, gyoudori and
/// paragraph gyoudori.
fn grid_settings(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value) {
    use designcraft_doc::GridAlign;
    use designcraft_doc::cjk::CharacterAlignment as R;
    let cur: GridAlign = serde_json::from_value(pval(d, pv, "gridAlign")).unwrap_or_default();
    let reference: R = serde_json::from_value(pval(d, pv, "gridReference")).unwrap_or_default();
    let set = |d: &mut Dialog, g: GridAlign| {
        if let Ok(v) = serde_json::to_value(g) {
            d.fields.insert("p.gridAlign".into(), v);
        }
    };
    const REFERENCES: &[(R, &str)] = &[
        (R::EmTop, "Em Box Top"),
        (R::EmCenter, "Em Box Center"),
        (R::Baseline, "Roman Baseline"),
        (R::EmBottom, "Em Box Bottom"),
        (R::IcfTop, "ICF Top"),
        (R::IcfBottom, "ICF Bottom"),
    ];
    let on = cur != GridAlign::None;
    let mut entries = vec![("none".to_owned(), tr(app, "None"))];
    entries.extend(REFERENCES.iter().filter_map(|(r, label)| Some((serde_json::to_string(r).ok()?, tr(app, label)))));
    let shown = if on { serde_json::to_string(&reference).unwrap_or_default() } else { "none".to_owned() };
    let lines = pval(d, pv, "gridGyoudori").as_u64().unwrap_or(0);
    let (align_label, first_label, gyoudori_label, block_label) =
        (tr(app, "Grid Alignment:"), tr(app, "Align Only First Line to Grid"), tr(app, "Gyoudori:"), tr(app, "Use Paragraph Gyoudori"));
    Stack::new().id_salt("cjk_grid").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "Grid Settings")).id_salt("cjk_grid_fields").show(ui, |ui, _| {
            field(ui, &align_label, "gridAlignment", |ui| {
                if let Some(next) = pick(ui, "cjkgridalign", &align_label, &shown, &entries, true) {
                    if next == "none" {
                        set(d, GridAlign::None);
                    } else {
                        if !on {
                            set(d, GridAlign::AllLines);
                        }
                        if let Ok(v) = serde_json::from_str::<Value>(&next) {
                            d.fields.insert("p.gridReference".into(), v);
                        }
                    }
                }
            });
            field(ui, &gyoudori_label, "gridGyoudori", |ui| {
                Row::new().id_salt("cjk_gyoudori").show(ui, |ui, _| {
                    // Empty or 0: automatic (as many grid lines as the leading needs).
                    let value = (lines > 0).then_some(lines as f64);
                    if let Some(n) = number(ui, "p.gridGyoudori", &gyoudori_label, value, &Num::whole(0.0, 100.0), true) {
                        d.fields.insert("p.gridGyoudori".into(), json!(n.map_or(0, |n| n.round().clamp(0.0, 100.0) as u64)));
                    }
                    if lines == 0 {
                        line(ui, &tr(app, "Auto"));
                    }
                });
            });
        });
        let first = cur == GridAlign::FirstLineOnly;
        if let Some(next) = toggle(ui, "p.gridFirstLine", &first_label, Some(first), on) {
            set(d, if next { GridAlign::FirstLineOnly } else { GridAlign::AllLines });
        }
        // No effect while gyoudori is automatic.
        if let Some(next) = toggle(ui, "p.paragraphGyoudori", &block_label, pval(d, pv, "paragraphGyoudori").as_bool().or(Some(false)), lines > 0) {
            d.fields.insert("p.paragraphGyoudori".into(), json!(next));
        }
    });
}

/// The Ruby dialog: the reading over the selected text and the four ruby panes. `base` holds the selection's resolved
/// character attributes.
pub(crate) fn ruby(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let app: &DesignApp = app;
    let doc = app.session.active().map(|s| s.doc.clone())?;
    let base = d.fields.get("base").cloned().unwrap_or(Value::Null);
    let cf = CharFields { base: &base, sparse: false };
    let (ruby_label, hint, title) = (tr(app, "Ruby:"), tr(app, "Set over the selected text; empty removes it."), tr(app, "Ruby"));
    let section = d.s("section");
    Stack::new().id_salt("ruby_dialog").fill_width().show(ui, |ui, _| {
        field(ui, &ruby_label, "ruby_text", |ui| text(ui, d, "text", "ruby_text", &ruby_label, true));
        note(ui, &hint);
        heading(ui, &title);
        Row::new().id_salt("ruby_layout").fill_width().show(ui, |ui, _| {
            super::section_nav(app, ui, d, RUBY_SECTIONS, &title);
            page(app, ui, d, &section, &cf, None, &doc);
        });
    });
    None
}
