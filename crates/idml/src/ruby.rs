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

fn finite(e: &El, k: &str) -> Option<f64> {
    e.num(k).filter(|v| v.is_finite())
}

/// Read the ruby settings of a style or range element into `a`. `swatch` maps a swatch
/// reference to the document's swatch name.
pub(crate) fn read(e: &El, a: &mut CharAttrs, mut swatch: impl FnMut(&str) -> String) {
    a.ruby_type = e.prop("RubyType").and_then(|v| lookup(TYPES, &v));
    a.ruby_alignment = e.prop("RubyAlignment").and_then(|v| lookup(ALIGNMENTS, &v));
    a.ruby_position = e.prop("RubyPosition").and_then(|v| lookup(POSITIONS, &v));
    a.ruby_x_offset = finite(e, "RubyXOffset").map(|v| v.clamp(-1000.0, 1000.0));
    a.ruby_y_offset = finite(e, "RubyYOffset").map(|v| v.clamp(-1000.0, 1000.0));
    if let Some(f) = e.prop("RubyFont") {
        let f = f.trim();
        let fam = f.split('\t').next().unwrap_or(f);
        a.ruby_font = Some(if fam == "$ID/" { String::new() } else { fam.to_string() });
    }
    if let Some(s) = e.prop("RubyFontStyle") {
        let s = s.trim();
        a.ruby_font_style = Some(if s == "Nothing" { String::new() } else { s.to_string() });
    }
    // -1: automatic (half the parent size).
    a.ruby_font_size = finite(e, "RubyFontSize").map(|v| (v > 0.0).then_some(v.clamp(0.1, 1296.0)));
    a.ruby_x_scale = finite(e, "RubyXScale").map(|v| (v / 100.0).clamp(0.01, 10.0));
    a.ruby_y_scale = finite(e, "RubyYScale").map(|v| (v / 100.0).clamp(0.01, 10.0));
    a.ruby_open_type_pro = e.boolean("RubyOpenTypePro");
    a.ruby_auto_tcy_digits = finite(e, "RubyAutoTcyDigits").map(|v| v.clamp(0.0, 9.0) as u32);
    a.ruby_auto_tcy_include_roman = e.boolean("RubyAutoTcyIncludeRoman");
    a.ruby_auto_tcy_auto_scale = e.boolean("RubyAutoTcyAutoScale");
    a.ruby_overhang = e.boolean("RubyOverhang");
    a.ruby_overhang_amount = e.prop("RubyParentOverhangAmount").and_then(|v| lookup(OVERHANGS, &v));
    a.ruby_parent_spacing = e.prop("RubyParentSpacing").and_then(|v| lookup(SPACINGS, &v));
    a.ruby_auto_align = e.boolean("RubyAutoAlign");
    a.ruby_auto_scaling = e.boolean("RubyAutoScaling");
    a.ruby_scaling_min = finite(e, "RubyParentScalingPercent").map(|v| (v / 100.0).clamp(0.1, 1.0));
    let color = |r: String, swatch: &mut dyn FnMut(&str) -> String| if r.trim() == TEXT_COLOR { String::new() } else { swatch(r.trim()) };
    a.ruby_fill = e.prop("RubyFill").map(|r| color(r, &mut swatch));
    a.ruby_stroke = e.prop("RubyStroke").map(|r| color(r, &mut swatch));
    // -1: automatic (the text's tint, stroke weight).
    let tint = |v: f64| (v >= 0.0).then_some((v / 100.0).clamp(0.0, 1.0) as f32);
    a.ruby_fill_tint = finite(e, "RubyTint").map(tint);
    a.ruby_stroke_tint = finite(e, "RubyStrokeTint").map(tint);
    a.ruby_stroke_weight = finite(e, "RubyWeight").map(|v| (v >= 0.0).then_some(v.min(800.0)));
    let overprint = |k: &str| e.prop(k).and_then(|v| overprint_in(&v));
    a.ruby_overprint_fill = overprint("RubyOverprintFill");
    a.ruby_overprint_stroke = overprint("RubyOverprintStroke");
}

/// Write the ruby settings set in `a`: attributes on `el`, fonts and colours as `props`. `sw`
/// maps a swatch name to its IDML reference. Returns the ruby font (family, style) to list in
/// the fonts part.
pub(crate) fn write(el: &mut El, props: &mut Vec<El>, a: &CharAttrs, sw: impl Fn(&str) -> String) -> Option<(String, String)> {
    let b = |v: bool| if v { "true" } else { "false" };
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
