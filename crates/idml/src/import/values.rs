use super::{Color, lab};

/// Parses a whitespace-separated list, rejecting non-finite values as a group.
pub(super) fn nums(s: &str) -> Vec<f64> {
    let values: Vec<f64> = s.split_whitespace().filter_map(|value| value.parse().ok()).collect();
    if values.iter().all(|value| value.is_finite()) { values } else { Vec::new() }
}

/// An XMP date property, written as an element or an attribute.
pub(super) fn xmp_date(xmp: &str, key: &str) -> Option<i64> {
    let value = if let Some(i) = xmp.find(&format!("<{key}>")) {
        let rest = xmp.get(i + key.len() + 2..)?;
        rest.get(..rest.find('<')?)?
    } else {
        let i = xmp.find(&format!("{key}=\""))?;
        let rest = xmp.get(i + key.len() + 2..)?;
        rest.get(..rest.find('"')?)?
    };
    designcraft_doc::vars::parse_iso_date(value)
}

pub(super) fn lab_to_color(l: f32, a: f32, b: f32) -> Color {
    let rgb = lab::xyz_to_srgb(lab::lab_to_xyz(lab::Lab::new(l, a, b)));
    Color::rgb(rgb[0], rgb[1], rgb[2])
}

/// IDML tint (percent; `-1` = the colour's own tint, 100 %) → 0..1.
pub(super) fn tint(v: Option<f64>) -> Option<f32> {
    v.map(|t| if t < 0.0 || t.is_nan() { 1.0 } else { (t / 100.0).min(1.0) as f32 })
}

pub(super) fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite())
}
