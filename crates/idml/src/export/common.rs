use super::{Affine, Color, El, num};

pub(crate) fn uid(n: u64) -> String {
    format!("u{n:x}")
}

pub(super) fn p(name: &str, ty: &str, val: impl Into<String>) -> El {
    crate::xml::from_craft(craft_idml::prop(name, ty, val))
}

/// Insert a `<Properties>` element as the first child when there are any.
pub(super) fn with_props(el: El, props: Vec<El>) -> El {
    crate::xml::from_craft_for_write(craft_idml::with_props(crate::xml::to_craft(&el), props.into_iter().map(|p| crate::xml::to_craft(&p)).collect()))
}

pub(super) fn pkg_root(kind: &str) -> El {
    crate::xml::from_craft(craft_idml::part_root(kind))
}

pub(super) fn bool_s(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

pub(super) fn pct(v: f64) -> String {
    num(v * 100.0)
}

/// Use the coarsest decimal that still imports as the original normalized channel.
fn color_units(value: f32, scale: f64) -> f64 {
    let scaled = f64::from(value) * scale;
    let mut factor = 1.0;
    for _ in 0..=9 {
        let rounded = (scaled * factor).round() / factor;
        if (rounded / scale) as f32 == value {
            return rounded;
        }
        factor *= 10.0;
    }
    scaled
}

pub(super) fn color_percent(value: f32) -> f64 {
    color_units(value, 100.0)
}

pub(super) fn tint_value(value: f32) -> String {
    color_percent(value).to_string()
}

/// (space, value) for a colour.
pub(super) fn color_value(c: &Color) -> (&'static str, String) {
    match *c {
        Color::Cmyk { c, m, y, k } => {
            let [c, m, y, k] = [c, m, y, k].map(color_percent);
            ("CMYK", format!("{c} {m} {y} {k}"))
        }
        Color::Rgb { r, g, b } => {
            let [r, g, b] = [r, g, b].map(|v| color_units(v, 255.0));
            ("RGB", format!("{r} {g} {b}"))
        }
        Color::Gray { k } => ("CMYK", format!("0 0 0 {}", color_percent(k))),
    }
}

pub(super) fn affine_s(a: Affine) -> String {
    a.as_coeffs().iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")
}

pub(super) fn xref_format_self(i: usize) -> String {
    format!("CrossReferenceFormat/f{i}")
}

/// The IDML `ChangeType` of a tracked change.
pub(super) fn change_type(mark: Option<designcraft_doc::ChangeMark>) -> Option<&'static str> {
    match mark {
        Some(designcraft_doc::ChangeMark::Deleted) => Some("DeletedText"),
        Some(designcraft_doc::ChangeMark::Inserted) => Some("InsertedText"),
        _ => None,
    }
}
