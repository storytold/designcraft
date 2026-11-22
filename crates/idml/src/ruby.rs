//! IDML ruby settings (`Ruby*` attributes of styles and character ranges). `RubyFlag` and
//! `RubyString` are read and written with the other character attributes.
use designcraft_doc::CharAttrs;
use designcraft_doc::ruby::{RubyAlignment, RubyOverhang, RubyParentSpacing, RubyPosition, RubyType};

use crate::cjk::{overprint_in, overprint_out};
use crate::names::num;
use crate::xml::El;

const TYPES: &[(RubyType, &str)] = &[(RubyType::Group, "GroupRuby"), (RubyType::PerCharacter, "PerCharacterRuby")];

const ALIGNMENTS: &[(RubyAlignment, &str)] = &[
    (RubyAlignment::Left, "RubyLeft"),
    (RubyAlignment::Center, "RubyCenter"),
    (RubyAlignment::Right, "RubyRight"),
    (RubyAlignment::FullJustify, "RubyFullJustify"),
    (RubyAlignment::Jis, "RubyJIS"),
    (RubyAlignment::EqualAki, "RubyEqualAki"),
    (RubyAlignment::OneAki, "Ruby1Aki"),
];

const POSITIONS: &[(RubyPosition, &str)] = &[(RubyPosition::AboveRight, "AboveRight"), (RubyPosition::BelowLeft, "BelowLeft")];

const OVERHANGS: &[(RubyOverhang, &str)] = &[
    (RubyOverhang::None, "None"),
    (RubyOverhang::OneRuby, "RubyOverhangOneRuby"),
    (RubyOverhang::HalfRuby, "RubyOverhangHalfRuby"),
    (RubyOverhang::OneChar, "RubyOverhangOneChar"),
    (RubyOverhang::HalfChar, "RubyOverhangHalfChar"),
    (RubyOverhang::NoLimit, "RubyOverhangNoLimit"),
];

const SPACINGS: &[(RubyParentSpacing, &str)] = &[
    (RubyParentSpacing::NoAdjustment, "RubyParentNoAdjustment"),
    (RubyParentSpacing::BothSides, "RubyParentBothSides"),
    (RubyParentSpacing::Aki121, "RubyParent121Aki"),
    (RubyParentSpacing::EqualAki, "RubyParentEqualAki"),
    (RubyParentSpacing::FullJustify, "RubyParentFullJustify"),
];

/// The text colour as IDML names it in place of a swatch.
const TEXT_COLOR: &str = "Text Color";

fn lookup<T: Copy>(table: &[(T, &str)], v: &str) -> Option<T> {
    let v = v.trim();
    table.iter().find(|(_, n)| *n == v).map(|(t, _)| *t)
}

fn name<T: Copy + PartialEq>(table: &[(T, &'static str)], v: T) -> Option<&'static str> {
    table.iter().find(|(t, _)| *t == v).map(|(_, n)| *n)
}

/// Read neutral Ruby values into `a`. `swatch` maps a swatch reference to the document's swatch name.
pub(crate) fn read_values(values: &craft_idml::model::RubyValues, a: &mut CharAttrs, mut swatch: impl FnMut(&str) -> String) {
    a.ruby_type = values.kind.as_deref().and_then(|value| lookup(TYPES, value));
    a.ruby_alignment = values.alignment.as_deref().and_then(|value| lookup(ALIGNMENTS, value));
    a.ruby_position = values.position.as_deref().and_then(|value| lookup(POSITIONS, value));
    a.ruby_x_offset = values.x_offset.map(|v| v.clamp(-1000.0, 1000.0));
    a.ruby_y_offset = values.y_offset.map(|v| v.clamp(-1000.0, 1000.0));
    if let Some(f) = &values.font {
        let f = f.trim();
        let fam = f.split('\t').next().unwrap_or(f);
        a.ruby_font = Some(if fam == "$ID/" { String::new() } else { fam.to_string() });
    }
    if let Some(s) = &values.font_style {
        let s = s.trim();
        a.ruby_font_style = Some(if s == "Nothing" { String::new() } else { s.to_string() });
    }
    // -1: automatic (half the parent size).
    a.ruby_font_size = values.font_size.map(|v| (v > 0.0).then_some(v.clamp(0.1, 1296.0)));
    a.ruby_x_scale = values.x_scale.map(|v| (v / 100.0).clamp(0.01, 10.0));
    a.ruby_y_scale = values.y_scale.map(|v| (v / 100.0).clamp(0.01, 10.0));
    a.ruby_open_type_pro = values.open_type_pro;
    a.ruby_auto_tcy_digits = values.auto_tcy_digits.map(|v| v.clamp(0.0, 9.0) as u32);
    a.ruby_auto_tcy_include_roman = values.auto_tcy_include_roman;
    a.ruby_auto_tcy_auto_scale = values.auto_tcy_auto_scale;
    a.ruby_overhang = values.overhang;
    a.ruby_overhang_amount = values.parent_overhang.as_deref().and_then(|value| lookup(OVERHANGS, value));
    a.ruby_parent_spacing = values.parent_spacing.as_deref().and_then(|value| lookup(SPACINGS, value));
    a.ruby_auto_align = values.auto_align;
    a.ruby_auto_scaling = values.auto_scaling;
    a.ruby_scaling_min = values.parent_scaling.map(|v| (v / 100.0).clamp(0.1, 1.0));
    let color = |r: &str, swatch: &mut dyn FnMut(&str) -> String| if r.trim() == TEXT_COLOR { String::new() } else { swatch(r.trim()) };
    a.ruby_fill = values.fill.as_deref().map(|r| color(r, &mut swatch));
    a.ruby_stroke = values.stroke.as_deref().map(|r| color(r, &mut swatch));
    // -1: automatic (the text's tint, stroke weight).
    let tint = |v: f64| (v >= 0.0).then_some((v / 100.0).clamp(0.0, 1.0) as f32);
    a.ruby_fill_tint = values.fill_tint.map(tint);
    a.ruby_stroke_tint = values.stroke_tint.map(tint);
    a.ruby_stroke_weight = values.stroke_weight.map(|v| (v >= 0.0).then_some(v.min(800.0)));
    a.ruby_overprint_fill = values.overprint_fill.as_deref().and_then(overprint_in);
    a.ruby_overprint_stroke = values.overprint_stroke.as_deref().and_then(overprint_in);
}

/// Ruby parent-text options (OpenType Pro, auto tate-chu-yoko, overhang, spacing, scaling).
fn write_parent_options(el: &mut El, a: &CharAttrs) {
    let b = |v: bool| if v { "true" } else { "false" };
    if let Some(v) = a.ruby_open_type_pro {
        el.set("RubyOpenTypePro", b(v));
    }
    if let Some(v) = a.ruby_auto_tcy_digits {
        el.set("RubyAutoTcyDigits", v);
    }
    if let Some(v) = a.ruby_auto_tcy_include_roman {
        el.set("RubyAutoTcyIncludeRoman", b(v));
    }
    if let Some(v) = a.ruby_auto_tcy_auto_scale {
        el.set("RubyAutoTcyAutoScale", b(v));
    }
    if let Some(v) = a.ruby_overhang {
        el.set("RubyOverhang", b(v));
    }
    if let Some(v) = a.ruby_overhang_amount.and_then(|v| name(OVERHANGS, v)) {
        el.set("RubyParentOverhangAmount", v);
    }
    if let Some(v) = a.ruby_parent_spacing.and_then(|v| name(SPACINGS, v)) {
        el.set("RubyParentSpacing", v);
    }
    if let Some(v) = a.ruby_auto_align {
        el.set("RubyAutoAlign", b(v));
    }
    if let Some(v) = a.ruby_auto_scaling {
        el.set("RubyAutoScaling", b(v));
    }
    if let Some(v) = a.ruby_scaling_min {
        el.set("RubyParentScalingPercent", num(v * 100.0));
    }
}

/// Write the ruby settings set in `a`: attributes on `el`, fonts and colours as `props`. `sw`
/// maps a swatch name to its IDML reference. Returns the ruby font (family, style) to list in
/// the fonts part.
pub(crate) fn write(el: &mut El, props: &mut Vec<El>, a: &CharAttrs, sw: impl Fn(&str) -> String) -> Option<(String, String)> {
    let prop = |k: &str, ty: &str, v: String| El::new(k).attr("type", ty).text(v);
    if let Some(v) = a.ruby_type.and_then(|v| name(TYPES, v)) {
        el.set("RubyType", v);
    }
    if let Some(v) = a.ruby_alignment.and_then(|v| name(ALIGNMENTS, v)) {
        el.set("RubyAlignment", v);
    }
    if let Some(v) = a.ruby_position.and_then(|v| name(POSITIONS, v)) {
        el.set("RubyPosition", v);
    }
    if let Some(v) = a.ruby_x_offset {
        el.set("RubyXOffset", num(v));
    }
    if let Some(v) = a.ruby_y_offset {
        el.set("RubyYOffset", num(v));
    }
    if let Some(v) = &a.ruby_font {
        props.push(prop("RubyFont", "string", if v.is_empty() { "$ID/".into() } else { v.clone() }));
    }
    if let Some(v) = &a.ruby_font_style {
        props.push(if v.is_empty() { prop("RubyFontStyle", "enumeration", "Nothing".into()) } else { prop("RubyFontStyle", "string", v.clone()) });
    }
    if let Some(v) = a.ruby_font_size {
        el.set("RubyFontSize", num(v.unwrap_or(-1.0)));
    }
    if let Some(v) = a.ruby_x_scale {
        el.set("RubyXScale", num(v * 100.0));
    }
    if let Some(v) = a.ruby_y_scale {
        el.set("RubyYScale", num(v * 100.0));
    }
    write_parent_options(el, a);
    for (k, c) in [("RubyFill", &a.ruby_fill), ("RubyStroke", &a.ruby_stroke)] {
        if let Some(c) = c {
            props.push(if c.is_empty() { prop(k, "string", TEXT_COLOR.into()) } else { prop(k, "object", sw(c.as_str())) });
        }
    }
    if let Some(v) = a.ruby_fill_tint {
        el.set("RubyTint", num(v.map_or(-1.0, |t| t as f64 * 100.0)));
    }
    if let Some(v) = a.ruby_stroke_tint {
        el.set("RubyStrokeTint", num(v.map_or(-1.0, |t| t as f64 * 100.0)));
    }
    if let Some(v) = a.ruby_stroke_weight {
        el.set("RubyWeight", num(v.unwrap_or(-1.0)));
    }
    if let Some(v) = a.ruby_overprint_fill.map(overprint_out) {
        el.set("RubyOverprintFill", v);
    }
    if let Some(v) = a.ruby_overprint_stroke.map(overprint_out) {
        el.set("RubyOverprintStroke", v);
    }
    a.ruby_font.as_ref().filter(|f| !f.is_empty()).map(|f| {
        let style = a.ruby_font_style.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Regular".into());
        (f.clone(), style)
    })
}
