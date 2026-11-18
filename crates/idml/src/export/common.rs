use super::{Affine, Color, El, names, num};

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

/// A UI colour property (`LayerColor`, `GuideColor`, ...): the colour's name when it is one of
/// the named UI colours, its RGB as a list of 0–255 values otherwise.
pub(super) fn ui_color_el(name: &str, rgb: [u8; 3]) -> El {
    match names::layer_color_out(rgb) {
        Some(n) => p(name, "enumeration", n),
        None => {
            let mut e = El::new(name).attr("type", "list");
            for c in rgb {
                e.push(p("ListItem", "double", num(f64::from(c))));
            }
            e
        }
    }
}

/// (model, space, value) for a colour.
pub(super) fn color_value(c: &Color) -> (&'static str, String) {
    match *c {
        Color::Cmyk { c, m, y, k } => ("CMYK", format!("{} {} {} {}", pct(c as f64), pct(m as f64), pct(y as f64), pct(k as f64))),
        Color::Rgb { r, g, b } => ("RGB", format!("{} {} {}", num(r as f64 * 255.0), num(g as f64 * 255.0), num(b as f64 * 255.0))),
        Color::Gray { k } => ("CMYK", format!("0 0 0 {}", pct(k as f64))),
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
