//! The CJK sections of Paragraph Style Options and Character Style Options, listed while Show CJK
//! Features is on ([`crate::cjk_features`]): Auto Tate-chu-yoko, Tate-chu-yoko, the four ruby
//! sections, Kenten, Kenten Color, Shatai, Japanese Composition, Grid and Warichu settings. They
//! edit `c.<attr>` and `p.<attr>` fields like the other sections, and OK applies them with the
//! dialog's style edit command. The Ruby dialog (Type › Ruby…) shows the four ruby sections over
//! the selected text and applies them with `type.ruby`.

use designcraft_doc::cjk::Kinsoku;
use designcraft_doc::cjk_settings::KentenKind;
use serde_json::{Value, json};

use super::{CharFields, Dialog, char_check, char_choice, char_number};
use crate::DesignApp;
use crate::i18n::tr;

struct Section {
    id: &'static str,
    label: &'static str,
    /// Absent from Character Style Options.
    para_only: bool,
    /// Listed after Export Tagging.
    after_export: bool,
}

const fn section(id: &'static str, label: &'static str, para_only: bool, after_export: bool) -> Section {
    Section { id, label, para_only, after_export }
}

/// In the order of the reference dialog.
const SECTIONS: &[Section] = &[
    section("cjk.autoTcy", "Auto Tate-chu-yoko Settings", true, false),
    section("cjk.tcy", "Tate-chu-yoko Settings", false, false),
    section("cjk.rubyPlacement", "Ruby Placement and Spacing", false, false),
    section("cjk.rubyFont", "Ruby Font and Size", false, false),
    section("cjk.rubyLonger", "Adjustment When Ruby Is Longer Than Parent", false, false),
    section("cjk.rubyColor", "Ruby Color", false, false),
    section("cjk.kenten", "Kenten Settings", false, false),
    section("cjk.kentenColor", "Kenten Color", false, false),
    section("cjk.shatai", "Shatai", false, false),
    section("cjk.composition", "Japanese Composition Settings", true, false),
    section("cjk.grid", "Grid Settings", true, false),
    section("cjk.warichu", "Warichu Settings", false, true),
];

/// The Ruby dialog's sections, the same panes as in the style dialogs.
pub(super) const RUBY_SECTIONS: &[(&str, &str)] = &[
    ("cjk.rubyPlacement", "Ruby Placement and Spacing"),
    ("cjk.rubyFont", "Ruby Font and Size"),
    ("cjk.rubyLonger", "Adjustment When Ruby Is Longer Than Parent"),
    ("cjk.rubyColor", "Ruby Color"),
];

/// The section list of a style options dialog: `base` with the CJK sections when `cjk` is on.
/// They go before Export Tagging, except Warichu Settings, which follows it; without Export
/// Tagging they all go last.
pub(super) fn sections<'a>(cjk: bool, para: bool, base: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    if !cjk {
        return base.to_vec();
    }
    let shown = move |after: bool| SECTIONS.iter().filter(move |s| (para || !s.para_only) && s.after_export == after).map(|s| (s.id, s.label));
    let mut out = Vec::with_capacity(base.len() + SECTIONS.len());
    let mut placed = false;
    for s in base {
        if s.0 == "export" && !placed {
            out.extend(shown(false));
            out.push(*s);
            out.extend(shown(true));
            placed = true;
        } else {
            out.push(*s);
        }
    }
    if !placed {
        out.extend(shown(false));
        out.extend(shown(true));
    }
    out
}

pub(super) fn is_section(id: &str) -> bool {
    SECTIONS.iter().any(|s| s.id == id)
}

/// Draws CJK section `id`. `para` holds the paragraph style's resolved paragraph attributes; it
/// is `None` in Character Style Options and the Ruby dialog, which have no paragraph-only
/// sections.
pub(super) fn section_ui(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    d: &mut Dialog,
    id: &str,
    cf: &CharFields,
    para: Option<&Value>,
    doc: &designcraft_doc::Document,
) {
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    match (id, para) {
        ("cjk.tcy", _) => tate_chu_yoko(lang, ui, d, cf),
        ("cjk.rubyPlacement", _) => ruby_placement(lang, ui, d, cf),
        ("cjk.rubyFont", _) => ruby_font(app, ui, d, cf),
        ("cjk.rubyLonger", _) => ruby_longer(lang, ui, d, cf),
        ("cjk.rubyColor", _) => adornment_color(lang, ui, d, cf, doc, "ruby"),
        ("cjk.kenten", _) => kenten(app, ui, d, cf),
        ("cjk.kentenColor", _) => adornment_color(lang, ui, d, cf, doc, "kenten"),
        ("cjk.shatai", _) => shatai(lang, ui, d, cf),
        ("cjk.warichu", _) => warichu(lang, ui, d, cf),
        ("cjk.autoTcy", Some(pv)) => auto_tate_chu_yoko(lang, ui, d, pv),
        ("cjk.composition", Some(pv)) => japanese_composition(lang, ui, d, cf, pv, doc),
        ("cjk.grid", Some(pv)) => grid_settings(lang, ui, d, pv),
        _ => {}
    }
}

fn grid(id: &str) -> egui::Grid {
    egui::Grid::new(id).num_columns(2).spacing([8.0, 6.0])
}

/// A number attribute that may be automatic (`null`: half the text size, the text's tint...):
/// emptying the field makes it automatic again.
fn char_auto_number(ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, key: &str, suffix: &str, scale: f64, range: (f64, f64)) {
    let v = cf.get(d, key).as_f64().map(|x| x * scale);
    let field = crate::widgets::NumField::number(key, v, suffix, 2).width(80.0).range(range.0, range.1);
    if let Some(n) = field.show_or_clear(ui) {
        cf.set(d, key, n.map_or(Value::Null, |n| json!(n / scale)));
    }
}

/// A font family and style pair of attributes; an empty family is the parent text's font.
fn font_fields(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, keys: (&str, &str), enabled: bool) {
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    let fonts = crate::panels::fonts(app);
    let menu = crate::panels::font_menu(app);
    let (fam_key, style_key) = keys;
    let raw = cf.get(d, fam_key);
    let fam = raw.as_str().unwrap_or("").to_string();
    let parent = tr(lang, "Parent's Font").to_string();
    let shown = match &raw {
        Value::Null if cf.sparse => String::new(),
        _ if fam.is_empty() => parent.clone(),
        _ => crate::panels::font_label(app, &menu, &fam),
    };
    crate::rtl::label(ui, tr(lang, "Font Family:"));
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(("cjkfam", fam_key))
            .selected_text(shown)
            .width(200.0)
            .height(440.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                if cf.sparse && ui.selectable_label(raw.is_null(), " ").clicked() {
                    cf.set(d, fam_key, Value::Null);
                    cf.set(d, style_key, Value::Null);
                }
                if ui.selectable_label(!raw.is_null() && fam.is_empty(), crate::rtl::widget(ui, parent.as_str())).clicked() {
                    cf.set(d, fam_key, json!(""));
                    cf.set(d, style_key, json!(""));
                }
                if let Some(f) = crate::panels::font_menu_body(app, ui, &menu, &fam, 200.0) {
                    cf.set(d, fam_key, json!(f));
                }
            });
    });
    ui.end_row();
    crate::rtl::label(ui, tr(lang, "Font Style:"));
    let sty = cf.get(d, style_key).as_str().unwrap_or("").to_string();
    ui.add_enabled_ui(enabled && !fam.is_empty(), |ui| {
        egui::ComboBox::from_id_salt(("cjksty", style_key)).selected_text(&sty).width(200.0).show_ui(ui, |ui| {
            for s in fonts.styles(&fam) {
                if ui.selectable_label(s == sty, &s).clicked() {
                    cf.set(d, style_key, json!(s));
                }
            }
        });
    });
    ui.end_row();
}

/// An adornment's overprint (`auto`, `on` or `off`): checked is `on`; a character style also
/// has the unset state.
fn overprint_check(ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str) {
    let cur = cf.get(d, key);
    let mut on = cur.as_str() == Some("on");
    let unset = cf.sparse && cur.is_null();
    if ui.add(egui::Checkbox::new(&mut on, crate::rtl::widget(ui, label)).indeterminate(unset)).clicked() {
        let next = match cur.as_str() {
            _ if !cf.sparse => json!(if on { "on" } else { "auto" }),
            None => json!("on"),
            Some("on") => json!("auto"),
            Some(_) => Value::Null,
        };
        cf.set(d, key, next);
    }
}

/// Tate-chu-yoko Settings: the switch and the run's offsets.
fn tate_chu_yoko(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    char_check(ui, d, cf, "tateChuYoko", crate::i18n::tr(lang, "Tate-Chu-Yoko"));
    ui.add_space(4.0);
    egui::Grid::new("cjktcy").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Up/Down Position:"));
        char_number(ui, d, cf, "tateChuYokoYOffset", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Left/Right Position:"));
        char_number(ui, d, cf, "tateChuYokoXOffset", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
    });
}

/// Ruby Placement and Spacing: type, alignment, position and offsets.
fn ruby_placement(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    grid("cjkrubyplace").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Ruby Type:"));
        char_choice(lang, ui, d, cf, "rubyType", &[(json!("perCharacter"), "Per-Character Ruby"), (json!("group"), "Group Ruby")]);
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Alignment:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
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
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Position:"));
        char_choice(lang, ui, d, cf, "rubyPosition", &[(json!("aboveRight"), "Above/Right"), (json!("belowLeft"), "Below/Left")]);
        ui.end_row();
    });
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Offset from Parent")).font(crate::theme::semibold(12.0)));
    grid("cjkrubyoffset").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Across the Line:"));
        char_number(ui, d, cf, "rubyYOffset", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Along the Line:"));
        char_number(ui, d, cf, "rubyXOffset", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
    });
}

/// Ruby Font and Size: font, size (empty: half the parent's), scales, OpenType ruby glyphs and
/// tate-chu-yoko of digits inside vertical ruby.
fn ruby_font(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    grid("cjkrubyfont").show(ui, |ui| {
        font_fields(app, ui, d, cf, ("rubyFont", "rubyFontStyle"), true);
        crate::rtl::label(ui, tr(lang, "Size:"));
        char_auto_number(ui, d, cf, "rubyFontSize", " pt", 1.0, (0.1, 1296.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Horizontal Scale:"));
        char_number(ui, d, cf, "rubyXScale", "%", 100.0, (1.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Vertical Scale:"));
        char_number(ui, d, cf, "rubyYScale", "%", 100.0, (1.0, 1000.0));
        ui.end_row();
    });
    char_check(ui, d, cf, "rubyOpenTypePro", tr(lang, "Use OpenType Pro Ruby Glyphs"));
    ui.add_space(4.0);
    grid("cjkrubytcy").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Tate-chu-yoko Digits:"));
        char_count(ui, d, cf, "rubyAutoTcyDigits", (0.0, 9.0));
        ui.end_row();
    });
    char_check(ui, d, cf, "rubyAutoTcyIncludeRoman", tr(lang, "Include Roman Characters"));
    char_check(ui, d, cf, "rubyAutoTcyAutoScale", tr(lang, "Fit to Width"));
}

/// Adjustment When Ruby Is Longer Than Parent: overhang, parent spacing, automatic narrowing
/// and alignment with the line edges.
fn ruby_longer(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    grid("cjkrubylonger").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Ruby Overhang:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
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
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Parent Character Spacing:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
            "rubyParentSpacing",
            &[
                (json!("noAdjustment"), "No Adjustment"),
                (json!("bothSides"), "Both Sides"),
                (json!("aki121"), "1-2-1 Aki"),
                (json!("equalAki"), "Equal Aki"),
                (json!("fullJustify"), "Full Justify"),
            ],
        );
        ui.end_row();
    });
    ui.add_space(4.0);
    char_check(ui, d, cf, "rubyAutoScaling", tr(lang, "Shrink Ruby Width Automatically"));
    grid("cjkrubyscale").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Minimum Scale:"));
        let on = cf.get(d, "rubyAutoScaling").as_bool() == Some(true);
        ui.add_enabled_ui(on, |ui| char_number(ui, d, cf, "rubyScalingMin", "%", 100.0, (10.0, 100.0)));
        ui.end_row();
    });
    char_check(ui, d, cf, "rubyAutoAlign", tr(lang, "Auto Align at Line Start/End"));
}

/// Ruby Color / Kenten Color (`prefix` "ruby" or "kenten"): the fill or stroke swatch (Text
/// Color: the text's own), its tint and the stroke weight (empty: the text's), and overprint.
fn adornment_color(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document, prefix: &str) {
    let target = format!("cjk.{prefix}ColorTarget");
    let stroke = d.s(&target) == "stroke";
    ui.horizontal(|ui| {
        for (t, label) in [("fill", "Fill"), ("stroke", "Stroke")] {
            if ui.selectable_label((t == "stroke") == stroke, crate::rtl::widget(ui, tr(lang, label))).clicked() {
                d.fields.insert(target.clone(), json!(t));
            }
        }
    });
    let part = if stroke { "Stroke" } else { "Fill" };
    let key = format!("{prefix}{part}");
    let cur = cf.get(d, &key);
    let cur = cur.as_str();
    let mut pick: Option<Value> = None;
    egui::ScrollArea::vertical().id_salt(("cjkcolor", key.as_str())).max_height(150.0).show(ui, |ui| {
        let entry = |ui: &mut egui::Ui, name: &str, label: &str, pick: &mut Option<Value>| {
            if ui.selectable_label(cur == Some(name), crate::rtl::widget(ui, label)).clicked() {
                // A character style's chosen colour, clicked again, is no longer set.
                *pick = Some(if cf.sparse && cur == Some(name) { Value::Null } else { json!(name) });
            }
        };
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            entry(ui, "", tr(lang, "Text Color"), &mut pick);
        });
        for sw in &doc.swatches {
            let (c, g) = crate::widgets::swatch_colors(doc, &sw.name, 1.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                crate::widgets::paint_chip(ui.painter(), r, c, g);
                entry(ui, &sw.name, &sw.name, &mut pick);
            });
        }
    });
    if let Some(v) = pick {
        cf.set(d, &key, v);
    }
    grid(&format!("cjkcolorg{prefix}")).show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Tint:"));
        char_auto_number(ui, d, cf, &format!("{prefix}{part}Tint"), "%", 100.0, (0.0, 100.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Weight:"));
        char_auto_number(ui, d, cf, &format!("{prefix}StrokeWeight"), " pt", 1.0, (0.0, 800.0));
        ui.end_row();
    });
    overprint_check(ui, d, cf, &format!("{prefix}OverprintFill"), tr(lang, "Overprint Fill"));
    overprint_check(ui, d, cf, &format!("{prefix}OverprintStroke"), tr(lang, "Overprint Stroke"));
}

/// The `type.ruby` parameters for the Ruby dialog's edits: the reading and every `c.ruby*`
/// field, in the command's units (percentages for scales and tints).
pub(super) fn ruby_params(d: &Dialog) -> Value {
    let mut p = serde_json::Map::new();
    p.insert("text".into(), json!(d.s("text")));
    // (attribute, parameter, factor from the model's unit to the parameter's)
    const MAP: &[(&str, &str, f64)] = &[
        ("rubyType", "type", 0.0),
        ("rubyAlignment", "alignment", 0.0),
        ("rubyPosition", "position", 0.0),
        ("rubyXOffset", "xOffset", 1.0),
        ("rubyYOffset", "yOffset", 1.0),
        ("rubyFont", "font", 0.0),
        ("rubyFontStyle", "fontStyle", 0.0),
        ("rubyFontSize", "size", 1.0),
        ("rubyXScale", "xScale", 100.0),
        ("rubyYScale", "yScale", 100.0),
        ("rubyOpenTypePro", "openTypePro", 0.0),
        ("rubyAutoTcyDigits", "autoTcyDigits", 1.0),
        ("rubyAutoTcyIncludeRoman", "autoTcyIncludeRoman", 0.0),
        ("rubyAutoTcyAutoScale", "autoTcyAutoScale", 0.0),
        ("rubyOverhangAmount", "overhang", 0.0),
        ("rubyParentSpacing", "parentSpacing", 0.0),
        ("rubyAutoAlign", "autoAlign", 0.0),
        ("rubyAutoScaling", "autoScaling", 0.0),
        ("rubyScalingMin", "scalingPercent", 100.0),
        ("rubyFill", "fill", 0.0),
        ("rubyStroke", "stroke", 0.0),
        ("rubyFillTint", "fillTint", 100.0),
        ("rubyStrokeTint", "strokeTint", 100.0),
        ("rubyStrokeWeight", "strokeWeight", 1.0),
        ("rubyOverprintFill", "overprintFill", 0.0),
        ("rubyOverprintStroke", "overprintStroke", 0.0),
    ];
    for (attr, param, factor) in MAP {
        let Some(v) = d.fields.get(&format!("c.{attr}")) else { continue };
        let v = match v.as_f64() {
            Some(x) if *factor != 0.0 => json!(x * factor),
            _ => v.clone(),
        };
        p.insert((*param).into(), v);
    }
    Value::Object(p)
}

/// The Ruby dialog: the reading over the selected text and the four ruby panes. `base` holds the
/// selection's resolved character attributes.
pub(super) fn ruby_dialog(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let Some(doc) = app.session.active().map(|s| s.doc.clone()) else { return };
    let base = d.fields.get("base").cloned().unwrap_or(Value::Null);
    let cf = CharFields { base: &base, sparse: false };
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr(&lang, "Ruby:"));
        super::text_field(ui, d, "text", 260.0);
    });
    crate::rtl::label(ui, egui::RichText::new(tr(&lang, "Set over the selected text; empty removes it.")).size(11.0));
    ui.add_space(6.0);
    ui.set_min_width(560.0);
    ui.horizontal_top(|ui| {
        ui.set_min_height(330.0);
        ui.set_max_height(330.0);
        super::section_list(&lang, ui, d, false, RUBY_SECTIONS);
        ui.separator();
        let id = d.s("section");
        ui.vertical(|ui| section_ui(app, ui, d, &id, &cf, None, &doc));
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

/// The kind the menu shows for the stored kind and character. Documents made before kinds
/// existed store a preset as its character with the default kind.
fn shown_kenten_kind(kind: Option<KentenKind>, character: &str) -> KentenKind {
    match kind {
        Some(k) if k != KentenKind::SesameDot => k,
        _ if character.is_empty() || character == "\u{FE45}" => KentenKind::SesameDot,
        _ => KentenKind::ALL.into_iter().find(|k| k.mark() == Some(character)).unwrap_or(KentenKind::Custom),
    }
}

/// Kenten Type picks: `None` switches kenten off (keeping the other settings), `Some(kind)`
/// switches them on with that kind. A preset stores its mark as the character too, as
/// `type.kenten` does.
pub(super) fn pick_kenten(d: &mut Dialog, cf: &CharFields, kind: Option<KentenKind>) {
    let Some(kind) = kind else {
        cf.set(d, "kenten", json!(false));
        return;
    };
    cf.set(d, "kenten", json!(true));
    cf.set(d, "kentenKind", json!(kind));
    if let Some(mark) = kind.mark() {
        cf.set(d, "kentenCharacter", json!(mark));
    }
}

/// Kenten Settings: distance, position, size, alignment, scales, the kind, and for a custom mark
/// its font and characters.
fn kenten(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    use designcraft_doc::cjk_settings::KENTEN_MARK_MAX_CHARS;
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    let on = cf.get(d, "kenten").as_bool();
    let stored: Option<KentenKind> = serde_json::from_value(cf.get(d, "kentenKind")).ok();
    let character = cf.get(d, "kentenCharacter").as_str().unwrap_or("").to_string();
    let kind = shown_kenten_kind(stored, &character);
    let custom = on == Some(true) && kind == KentenKind::Custom;
    grid("cjkkenten").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Distance from Parent:"));
        char_number(ui, d, cf, "kentenDistance", " pt", 1.0, (-1296.0, 1296.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Position:"));
        char_choice(lang, ui, d, cf, "kentenPosition", &[(json!("aboveRight"), "Above/Right"), (json!("belowLeft"), "Below/Left")]);
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Size:"));
        char_auto_number(ui, d, cf, "kentenSize", " pt", 1.0, (0.1, 1296.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Alignment:"));
        char_choice(lang, ui, d, cf, "kentenAlignment", &[(json!("center"), "Centered"), (json!("start"), "Left/Top")]);
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Horizontal Scale:"));
        char_number(ui, d, cf, "kentenXScale", "%", 100.0, (1.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Vertical Scale:"));
        char_number(ui, d, cf, "kentenYScale", "%", 100.0, (1.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Kenten Type:"));
        let shown = match on {
            None => String::new(),
            Some(false) => tr(lang, "None").to_string(),
            Some(true) => tr(lang, kenten_label(kind)).to_string(),
        };
        egui::ComboBox::from_id_salt("cjkkententype").selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
            if cf.sparse && ui.selectable_label(on.is_none(), " ").clicked() {
                for key in ["kenten", "kentenKind", "kentenCharacter"] {
                    cf.set(d, key, Value::Null);
                }
            }
            if ui.selectable_label(on == Some(false), crate::rtl::widget(ui, tr(lang, "None"))).clicked() {
                pick_kenten(d, cf, None);
            }
            for k in KentenKind::ALL {
                if ui.selectable_label(on == Some(true) && kind == k, crate::rtl::widget(ui, tr(lang, kenten_label(k)))).clicked() {
                    pick_kenten(d, cf, Some(k));
                }
            }
        });
        ui.end_row();
        font_fields(app, ui, d, cf, ("kentenFont", "kentenFontStyle"), custom);
        crate::rtl::label(ui, tr(lang, "Character:"));
        // Typed directly; a mark stored with another input mode (IDML) is kept as it is.
        let mut text = if custom { character.clone() } else { String::new() };
        if ui.add_enabled(custom, egui::TextEdit::singleline(&mut text).desired_width(80.0)).changed() {
            let mark: String = text.chars().filter(|c| !c.is_control()).take(KENTEN_MARK_MAX_CHARS).collect();
            cf.set(d, "kentenCharacter", json!(mark));
        }
        ui.end_row();
    });
}

/// Shatai: magnification, angle, and the rotation and tsume adjustments.
fn shatai(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    grid("cjkshatai").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Magnification:"));
        char_number(ui, d, cf, "shataiMagnification", "%", 1.0, (0.0, 90.0));
        ui.end_row();
        crate::rtl::label(ui, tr(lang, "Angle:"));
        let v = cf.get(d, "shataiAngle").as_f64();
        let field = crate::widgets::NumField::number("shataiAngle", v, "°", 2).width(80.0).range(0.0, 180.0).presets(&[30.0, 45.0, 60.0]);
        let edit = if cf.sparse { field.show_or_clear(ui) } else { field.show(ui).map(Some) };
        if let Some(n) = edit {
            cf.set(d, "shataiAngle", n.map_or(Value::Null, |n| json!(n)));
        }
        ui.end_row();
    });
    char_check(ui, d, cf, "shataiAdjustRotation", tr(lang, "Adjust Rotation"));
    char_check(ui, d, cf, "shataiAdjustTsume", tr(lang, "Adjust Tsume"));
}

/// Auto Tate-chu-yoko Settings: the longest digit run set across the line, and whether roman
/// letters count.
fn auto_tate_chu_yoko(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value) {
    grid("cjkautotcy").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Tate-chu-yoko Digits:"));
        para_count(ui, d, pv, "autoTcy", (0.0, f64::from(designcraft_compose::shape::AUTO_TCY_MAX)));
        ui.end_row();
    });
    para_check(ui, d, pv, "autoTcyIncludeRoman", tr(lang, "Include Roman Characters"));
}

/// A whole-number paragraph attribute.
fn para_count(ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, range: (f64, f64)) {
    let v = para_value(d, pv, key).as_f64();
    if let Some(n) = crate::widgets::NumField::number(key, v, "", 0).width(80.0).range(range.0, range.1).show(ui) {
        d.fields.insert(format!("p.{key}"), json!(n.round().max(range.0).min(range.1) as u64));
    }
}

/// A whole-number attribute (stored as an integer: the model's fields are `u32`).
fn char_count(ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, key: &str, range: (f64, f64)) {
    let v = cf.get(d, key).as_f64();
    let field = crate::widgets::NumField::number(key, v, "", 0).width(80.0).range(range.0, range.1);
    let edit = if cf.sparse { field.show_or_clear(ui) } else { field.show(ui).map(Some) };
    if let Some(n) = edit {
        cf.set(d, key, n.map_or(Value::Null, |n| json!(n.round().max(range.0).min(range.1) as u64)));
    }
}

/// Warichu Settings: the switch, lines, size, spacing, alignment and line break minimums.
fn warichu(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    char_check(ui, d, cf, "warichu", crate::i18n::tr(lang, "Warichu"));
    ui.add_space(4.0);
    egui::Grid::new("cjkwarichu").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Lines:"));
        char_count(ui, d, cf, "warichuLines", (2.0, 16.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Warichu Size:"));
        char_number(ui, d, cf, "warichuSize", "%", 1.0, (1.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Line Spacing:"));
        char_number(ui, d, cf, "warichuLineSpacing", " pt", 1.0, (-1000.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Alignment:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
            "warichuAlignment",
            &[
                (json!("auto"), "Auto"),
                (json!("left"), "Left"),
                (json!("center"), "Center"),
                (json!("right"), "Right"),
                (json!("justify"), "Justify"),
            ],
        );
        ui.end_row();
    });
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Line Break Options")).font(crate::theme::semibold(12.0)));
    egui::Grid::new("cjkwarichubreak").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Characters Before Break:"));
        char_count(ui, d, cf, "warichuCharsBeforeBreak", (0.0, 100.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Characters After Break:"));
        char_count(ui, d, cf, "warichuCharsAfterBreak", (0.0, 100.0));
        ui.end_row();
    });
}

/// A paragraph attribute as shown: the dialog's `p.<key>` edit, else the resolved value.
fn para_value(d: &Dialog, pv: &Value, key: &str) -> Value {
    d.fields.get(&format!("p.{key}")).cloned().unwrap_or_else(|| pv.get(key).cloned().unwrap_or(Value::Null))
}

/// A paragraph attribute from `opts` (value, label); `current` is the value shown.
fn para_choice(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, key: &str, current: &str, opts: &[(&str, &str)]) {
    let shown = opts.iter().find(|o| o.0 == current).map_or("", |o| crate::i18n::tr(lang, o.1));
    egui::ComboBox::from_id_salt(("cjkp", key)).selected_text(crate::rtl::widget(ui, shown)).width(220.0).show_ui(ui, |ui| {
        for (v, label) in opts {
            if ui.selectable_label(*v == current, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                d.fields.insert(format!("p.{key}"), json!(v));
            }
        }
    });
}

fn para_check(ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, label: &str) {
    let mut on = para_value(d, pv, key).as_bool().unwrap_or(false);
    if ui.checkbox(&mut on, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(format!("p.{key}"), json!(on));
    }
}

/// The built-in kinsoku sets ([`Kinsoku::named`]) and their labels.
const BUILT_IN_KINSOKU: &[(&str, &str)] = &[
    ("HardKinsoku", "Hard Kinsoku"),
    ("SoftKinsoku", "Soft Kinsoku"),
    ("SimplifiedChineseKinsoku", "Simplified Chinese Kinsoku"),
    ("TraditionalChineseKinsoku", "Traditional Chinese Kinsoku"),
    ("KoreanKinsoku", "Korean Kinsoku"),
];

/// The Kinsoku Set menu: no kinsoku, the built-in sets, then every other named set the document's
/// paragraph styles and paragraphs use (the model keeps each set by value, not in a list).
pub(super) fn kinsoku_sets(doc: &designcraft_doc::Document) -> Vec<Kinsoku> {
    let mut sets = vec![Kinsoku::default()];
    sets.extend(BUILT_IN_KINSOKU.iter().filter_map(|(name, _)| Kinsoku::named(name)));
    let used = doc.styles.paragraph.iter().map(|s| &s.para.kinsoku).chain(doc.stories.values().flat_map(|s| s.paras.iter().map(|p| &p.para.kinsoku)));
    for k in used.flatten().flatten() {
        if !k.name.is_empty() && !sets.iter().any(|s| s.name == k.name) {
            sets.push(k.clone());
        }
    }
    sets
}

fn kinsoku_label(lang: &str, name: &str) -> String {
    if name.is_empty() {
        return crate::i18n::tr(lang, "None").to_string();
    }
    BUILT_IN_KINSOKU.iter().find(|k| k.0 == name).map_or_else(|| name.to_string(), |k| crate::i18n::tr(lang, k.1).to_string())
}

/// IDML's names for "no mojikumi set".
fn mojikumi_value(v: &str) -> &str {
    if matches!(v, "Nothing" | "None") { "" } else { v }
}

/// The Mojikumi Set menu as (value, label): none (empty label), the document's sets, and the
/// `current` value when it names a set the document doesn't define (kept as imported).
pub(super) fn mojikumi_sets(doc: &designcraft_doc::Document, current: &str) -> Vec<(String, String)> {
    let mut out = vec![(String::new(), String::new())];
    for t in &doc.styles.mojikumi_tables {
        let v = format!("MojikumiTable/{}", t.name);
        if !out.iter().any(|o| o.0 == v) {
            out.push((v, t.name.clone()));
        }
    }
    let current = mojikumi_value(current);
    if !current.is_empty() && !out.iter().any(|o| o.0 == current) {
        out.push((current.to_string(), current.trim_start_matches("MojikumiTable/").trim_start_matches("$ID/").to_string()));
    }
    out
}

/// Japanese Composition Settings: kinsoku, hanging, bunri-kinshi, mojikumi, the leading model,
/// rensuuji, rotated roman in vertical text, line-end ideographic spaces and roman word breaks.
fn japanese_composition(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, pv: &Value, doc: &designcraft_doc::Document) {
    egui::Grid::new("cjkcomp").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Kinsoku Set:"));
        let sets = kinsoku_sets(doc);
        // `null`: no set given, so the composer's default rules apply.
        let name = para_value(d, pv, "kinsoku").get("name").and_then(Value::as_str).map(str::to_string);
        let shown = name.as_deref().map_or_else(|| crate::i18n::tr(lang, "Default Rules").to_string(), |n| kinsoku_label(lang, n));
        egui::ComboBox::from_id_salt("cjkkinsoku").selected_text(crate::rtl::widget(ui, shown)).width(220.0).show_ui(ui, |ui| {
            // `style.paragraph.edit` takes `null` as the default rules, also over a Based On set.
            if ui.selectable_label(name.is_none(), crate::rtl::widget(ui, crate::i18n::tr(lang, "Default Rules"))).clicked() {
                d.fields.insert("p.kinsoku".into(), Value::Null);
            }
            for k in &sets {
                if ui.selectable_label(name.as_deref() == Some(k.name.as_str()), crate::rtl::widget(ui, kinsoku_label(lang, &k.name))).clicked()
                    && let Ok(v) = serde_json::to_value(k)
                {
                    d.fields.insert("p.kinsoku".into(), v);
                }
            }
        });
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Kinsoku Type:"));
        let kind = para_value(d, pv, "kinsokuType");
        let kind = match kind.as_str().unwrap_or("") {
            "KinsokuPushInFirst" => "",
            k => k,
        }
        .to_string();
        para_choice(
            lang,
            ui,
            d,
            "kinsokuType",
            &kind,
            &[
                ("", "Push In First"),
                ("KinsokuPushOutFirst", "Push Out First"),
                ("KinsokuPushOutOnly", "Push Out Only"),
                ("KinsokuPrioritizeAdjustmentAmount", "Prioritize Adjustment Amount"),
            ],
        );
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Hanging Type:"));
        let hang = para_value(d, pv, "kinsokuHang").as_str().unwrap_or("none").to_string();
        para_choice(lang, ui, d, "kinsokuHang", &hang, &[("none", "None"), ("regular", "Standard"), ("force", "Force")]);
        ui.end_row();
    });
    para_check(ui, d, pv, "bunriKinshi", crate::i18n::tr(lang, "Bunri-Kinshi"));
    ui.add_space(4.0);
    egui::Grid::new("cjkcomp2").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Mojikumi Set:"));
        let raw = para_value(d, pv, "mojikumi");
        let current = mojikumi_value(raw.as_str().unwrap_or("")).to_string();
        let sets = mojikumi_sets(doc, &current);
        let label = |l: &str| if l.is_empty() { crate::i18n::tr(lang, "None").to_string() } else { l.to_string() };
        let shown = sets.iter().find(|s| s.0 == current).map_or_else(String::new, |s| label(s.1.as_str()));
        egui::ComboBox::from_id_salt("cjkmojikumi").selected_text(crate::rtl::widget(ui, shown)).width(220.0).show_ui(ui, |ui| {
            for (v, l) in &sets {
                if ui.selectable_label(*v == current, crate::rtl::widget(ui, label(l.as_str()))).clicked() {
                    d.fields.insert("p.mojikumi".into(), json!(v));
                }
            }
        });
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Leading Model:"));
        // Space below (`akiBelow`) measures the leading from the em box top, space above from its
        // bottom (`cjk_line_reference` in compose).
        let mut opts = vec![
            (json!("akiBelow"), "Em Box Top/Right"),
            (json!("center"), "Em Box Center"),
            (json!("roman"), "Roman Baseline"),
            (json!("akiAbove"), "Em Box Bottom/Left"),
        ];
        // Kept from IDML; not offered otherwise.
        if cf.get(d, "leadingModel") == json!("centerDown") {
            opts.push((json!("centerDown"), "Em Box Center (Down)"));
        }
        char_choice(lang, ui, d, cf, "leadingModel", &opts);
        ui.end_row();
    });
    para_check(ui, d, pv, "rensuuji", crate::i18n::tr(lang, "Rensuuji"));
    para_check(ui, d, pv, "rotateRoman", crate::i18n::tr(lang, "Rotate Roman Characters in Vertical Text"));
    para_check(ui, d, pv, "treatIdeographicSpaceAsSpace", crate::i18n::tr(lang, "Absorb Ideographic Space at Line End"));
    para_check(ui, d, pv, "romanWordBreak", crate::i18n::tr(lang, "Roman Word Break"));
}

/// Grid Settings: the point of each line that sits on the baseline grid (or none), only the
/// first line, gyoudori and paragraph gyoudori.
fn grid_settings(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value) {
    use designcraft_doc::GridAlign;
    use designcraft_doc::cjk::CharacterAlignment as R;
    let cur: GridAlign = serde_json::from_value(para_value(d, pv, "gridAlign")).unwrap_or_default();
    let reference: R = serde_json::from_value(para_value(d, pv, "gridReference")).unwrap_or_default();
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
    grid("cjkgrid").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Grid Alignment:"));
        let shown = if on { REFERENCES.iter().find(|r| r.0 == reference).map_or("", |r| r.1) } else { "None" };
        egui::ComboBox::from_id_salt("cjkgridalign").selected_text(crate::rtl::widget(ui, tr(lang, shown))).width(220.0).show_ui(ui, |ui| {
            if ui.selectable_label(!on, crate::rtl::widget(ui, tr(lang, "None"))).clicked() {
                set(d, GridAlign::None);
            }
            for (r, label) in REFERENCES {
                if ui.selectable_label(on && *r == reference, crate::rtl::widget(ui, tr(lang, label))).clicked() {
                    if !on {
                        set(d, GridAlign::AllLines);
                    }
                    if let Ok(v) = serde_json::to_value(r) {
                        d.fields.insert("p.gridReference".into(), v);
                    }
                }
            }
        });
        ui.end_row();
    });
    let mut first = cur == GridAlign::FirstLineOnly;
    let check = egui::Checkbox::new(&mut first, crate::rtl::widget(ui, tr(lang, "Align Only First Line to Grid")));
    if ui.add_enabled(on, check).changed() {
        set(d, if first { GridAlign::FirstLineOnly } else { GridAlign::AllLines });
    }
    ui.add_space(4.0);
    let lines = para_value(d, pv, "gridGyoudori").as_u64().unwrap_or(0);
    grid("cjkgyoudori").show(ui, |ui| {
        crate::rtl::label(ui, tr(lang, "Gyoudori:"));
        ui.horizontal(|ui| {
            // Empty or 0: automatic (as many grid lines as the leading needs).
            let field = crate::widgets::NumField::number("gridGyoudori", (lines > 0).then_some(lines as f64), "", 0)
                .width(80.0)
                .range(0.0, 100.0)
                .presets(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
            if let Some(n) = field.show_or_clear(ui) {
                d.fields.insert("p.gridGyoudori".into(), json!(n.map_or(0, |n| n.round().clamp(0.0, 100.0) as u64)));
            }
            if lines == 0 {
                crate::rtl::label(ui, tr(lang, "Auto"));
            }
        });
        ui.end_row();
    });
    // No effect while gyoudori is automatic.
    let mut block = para_value(d, pv, "paragraphGyoudori").as_bool().unwrap_or(false);
    if ui.add_enabled(lines > 0, egui::Checkbox::new(&mut block, crate::rtl::widget(ui, tr(lang, "Use Paragraph Gyoudori")))).changed() {
        d.fields.insert("p.paragraphGyoudori".into(), json!(block));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DesignApp;

    /// A document with paragraph style Body on its text and character style Emphasis.
    fn app_with_text() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body"})).unwrap();
        app.run("style.character.create", json!({"name": "Emphasis", "chars": {"fontStyle": "Italic"}})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "漢字かな"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 2})).unwrap();
        app.run("style.paragraph.apply", json!({"name": "Body"})).unwrap();
        app
    }

    /// The text painted by the open dialog once it has settled.
    fn painted(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.trim().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
        let mut texts = Vec::new();
        for _ in 0..4 {
            let time = ctx.input(|i| i.time) + 1.0 / 60.0;
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), ..Default::default() }, |ui| {
                crate::dialogs::show(app, ui.ctx());
            });
            output.textures_delta.clear();
            texts.clear();
            for s in &output.shapes {
                collect(&s.shape, &mut texts);
            }
        }
        texts
    }

    /// The list model, not painted labels: the section list scrolls, so lower rows may be clipped.
    #[test]
    fn cjk_sections_follow_show_cjk_features() {
        let mut app = app_with_text();
        let base = [("general", "General"), ("export", "Export Tagging")];
        let ids = |app: &DesignApp, para: bool| sections(crate::cjk_features(app), para, &base).into_iter().map(|s| s.0).collect::<Vec<_>>();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        let character =
            ["cjk.tcy", "cjk.rubyPlacement", "cjk.rubyFont", "cjk.rubyLonger", "cjk.rubyColor", "cjk.kenten", "cjk.kentenColor", "cjk.shatai"];
        let mut para = vec!["general", "cjk.autoTcy"];
        para.extend(character);
        para.extend(["cjk.composition", "cjk.grid", "export", "cjk.warichu"]);
        assert_eq!(ids(&app, true), para);
        let mut chars = vec!["general"];
        chars.extend(character);
        chars.extend(["export", "cjk.warichu"]);
        assert_eq!(ids(&app, false), chars);
        let mut last = vec!["general"];
        last.extend(character);
        last.push("cjk.warichu");
        assert_eq!(sections(true, false, &[("general", "General")]).into_iter().map(|s| s.0).collect::<Vec<_>>(), last);
        app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
        assert_eq!(ids(&app, true), ["general", "export"]);
        assert_eq!(ids(&app, false), ["general", "export"]);
    }

    #[test]
    fn cjk_sections_draw() {
        let mut app = app_with_text();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        for lang in ["", "ja", "ar"] {
            app.run("app.language", json!({"lang": lang})).unwrap();
            for s in SECTIONS {
                for fields in [
                    json!({"id": "paragraphStyleOptions", "name": "Body"}),
                    json!({"id": "paragraphStyleOptions", "new": true}),
                    json!({"id": "characterStyleOptions", "name": "Emphasis"}),
                    json!({"id": "characterStyleOptions", "name": "Emphasis", "c.kenten": true, "c.kentenCharacter": "※", "c.warichuLines": null}),
                ] {
                    let id = fields["id"].as_str().unwrap().to_string();
                    let mut f = fields.clone();
                    f["section"] = json!(s.id);
                    app.ui.dialog = Some(Dialog::new(&id, f));
                    painted(&mut app, &ctx);
                    assert!(app.ui.dialog.is_some(), "{lang} {id} {} stays open", s.id);
                }
            }
            for (id, _) in RUBY_SECTIONS {
                app.ui.dialog = Some(Dialog::new("ruby", json!({"text": "かんじ", "base": {}, "section": id})));
                painted(&mut app, &ctx);
                assert!(app.ui.dialog.is_some(), "{lang} ruby {id} stays open");
            }
        }
    }

    #[test]
    fn kenten_and_japanese_composition_reach_the_style_and_its_paragraph() {
        let mut app = app_with_text();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        if let Some(st) = app.session.active_mut() {
            let table = designcraft_doc::cjk::MojikumiTable { name: "Body Text".into(), ..Default::default() };
            std::sync::Arc::make_mut(&mut st.doc).styles_mut().mojikumi_tables.push(table);
        }
        let doc = app.session.active().unwrap().doc.clone();
        let hard = kinsoku_sets(&doc).into_iter().find(|k| k.name == "HardKinsoku").unwrap();
        let moji = mojikumi_sets(&doc, "").into_iter().find(|s| s.1 == "Body Text").unwrap().0;
        assert_eq!(moji, "MojikumiTable/Body Text");

        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.kenten"})));
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            pick_kenten(d, &CharFields { base: &Value::Null, sparse: false }, Some(KentenKind::BlackCircle));
            d.fields.insert("section".into(), json!("cjk.composition"));
            d.fields.insert("p.kinsoku".into(), serde_json::to_value(&hard).unwrap());
            d.fields.insert("p.mojikumi".into(), json!(moji));
            d.fields.insert("p.kinsokuHang".into(), json!("regular"));
        }
        painted(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();

        let doc = app.session.active().unwrap().doc.clone();
        let st = doc.styles.para("Body").unwrap();
        assert_eq!(st.para.kinsoku, Some(Some(hard)));
        assert_eq!(st.para.mojikumi.as_deref(), Some("MojikumiTable/Body Text"));
        assert_eq!(st.para.kinsoku_hang, Some(designcraft_doc::cjk::KinsokuHang::Regular));
        assert_eq!((st.chars.kenten, st.chars.kenten_kind), (Some(true), Some(KentenKind::BlackCircle)));
        assert_eq!(st.chars.kenten_character.as_deref(), Some("\u{25CF}"));
        let a = app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!(a["para"]["kinsoku"]["name"], "HardKinsoku", "the paragraph follows its style");
        assert_eq!(a["para"]["mojikumi"], "MojikumiTable/Body Text");
        assert_eq!(a["chars"]["kenten"], true);
        assert_eq!(a["chars"]["kentenKind"], "blackCircle");

        // Default Rules (`null`) replaces the set.
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.composition", "p.kinsoku": null})));
        painted(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active().unwrap().doc.styles.para("Body").unwrap().para.kinsoku, Some(None));
        assert!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["kinsoku"].is_null());

        // A character style sets only the kenten attributes.
        app.ui.dialog = Some(Dialog::new("characterStyleOptions", json!({"name": "Emphasis", "section": "cjk.kenten"})));
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            let base = json!({});
            pick_kenten(d, &CharFields { base: &base, sparse: true }, Some(KentenKind::SesameDot));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let st = app.session.active().unwrap().doc.styles.char_style("Emphasis").cloned().unwrap();
        assert_eq!((st.chars.kenten, st.chars.kenten_kind), (Some(true), Some(KentenKind::SesameDot)));
        assert!(st.chars.ruby_alignment.is_none() && st.chars.shatai_angle.is_none(), "nothing else is set");
    }

    #[test]
    fn ruby_alignment_and_shatai_angle_reach_the_style_and_its_paragraph() {
        use designcraft_doc::ruby::RubyAlignment;
        let mut app = app_with_text();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.rubyPlacement"})));
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            d.fields.insert("c.rubyAlignment".into(), json!("equalAki"));
            d.fields.insert("section".into(), json!("cjk.shatai"));
            d.fields.insert("c.shataiMagnification".into(), json!(20.0));
            d.fields.insert("c.shataiAngle".into(), json!(60.0));
            d.fields.insert("section".into(), json!("cjk.grid"));
            d.fields.insert("p.gridAlign".into(), json!("allLines"));
            d.fields.insert("p.gridReference".into(), json!("emCenter"));
            d.fields.insert("p.gridGyoudori".into(), json!(2));
        }
        painted(&mut app, &ctx);
        crate::dialogs::confirm(&mut app).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        let st = doc.styles.para("Body").unwrap();
        assert_eq!(st.chars.ruby_alignment, Some(RubyAlignment::EqualAki));
        assert_eq!((st.chars.shatai_magnification, st.chars.shatai_angle), (Some(20.0), Some(60.0)));
        assert_eq!((st.para.grid_reference, st.para.grid_gyoudori), (Some(designcraft_doc::cjk::CharacterAlignment::EmCenter), Some(2)));
        let a = app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!(a["chars"]["rubyAlignment"], "equalAki", "the paragraph follows its style");
        assert_eq!(a["chars"]["shataiAngle"], 60.0);
        assert_eq!(a["para"]["gridGyoudori"], 2);
    }

    #[test]
    fn ruby_dialog_applies_its_settings_with_type_ruby() {
        let mut app = app_with_text();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        let story = app.run("type.selectionAttrs", json!({})).unwrap()["story"].clone();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": "漢字".len()})).unwrap();
        app.run("app.rubyDialog", json!({})).unwrap();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        painted(&mut app, &ctx);
        {
            let d = app.ui.dialog.as_mut().unwrap();
            assert_eq!((d.id.as_str(), d.s("section").as_str()), ("ruby", "cjk.rubyPlacement"));
            assert_eq!(d.fields["base"]["rubyAlignment"], "jis", "the panes show the selection's settings");
            d.fields.insert("text".into(), json!("かんじ"));
            d.fields.insert("c.rubyAlignment".into(), json!("center"));
            d.fields.insert("section".into(), json!("cjk.rubyFont"));
            d.fields.insert("c.rubyFontSize".into(), json!(5.0));
            d.fields.insert("c.rubyXScale".into(), json!(0.8));
        }
        painted(&mut app, &ctx);
        let params = ruby_params(app.ui.dialog.as_ref().unwrap());
        assert_eq!(params, json!({"text": "かんじ", "alignment": "center", "size": 5.0, "xScale": 80.0}));
        crate::dialogs::confirm(&mut app).unwrap();
        let a = app.run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!(a["chars"]["ruby"], "かんじ");
        assert_eq!(a["chars"]["rubyAlignment"], "center");
        assert_eq!(a["chars"]["rubyFontSize"], 5.0);
        assert!((a["chars"]["rubyXScale"].as_f64().unwrap() - 0.8).abs() < 1e-9);
        // Reopened, the dialog shows them.
        app.run("app.rubyDialog", json!({})).unwrap();
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.s("text").as_str(), d.fields["base"]["rubyFontSize"].as_f64()), ("かんじ", Some(5.0)));
    }
}
