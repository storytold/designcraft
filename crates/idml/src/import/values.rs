use super::{Affine, Color, El, Point, lab};

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

/// The numbers in a whitespace-separated list; empty when any number is NaN or infinite (JSON
/// can't store them, and dropping just those would shift the rest).
pub(super) fn nums(s: &str) -> Vec<f64> {
    let v: Vec<f64> = s.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    if v.iter().all(|x| x.is_finite()) { v } else { Vec::new() }
}

pub(super) fn parse_affine(s: Option<&str>) -> Affine {
    match s.map(nums) {
        Some(v) if v.len() == 6 => Affine::new([v[0], v[1], v[2], v[3], v[4], v[5]]),
        _ => Affine::IDENTITY,
    }
}

pub(super) fn parse_point(s: Option<&str>) -> Option<Point> {
    let v = nums(s?);
    (v.len() >= 2).then(|| Point::new(v[0], v[1]))
}

/// IDML tint (percent; `-1` = the colour's own tint, 100 %) → 0..1.
pub(super) fn tint(v: Option<f64>) -> Option<f32> {
    v.map(|t| if t < 0.0 || t.is_nan() { 1.0 } else { (t / 100.0).min(1.0) as f32 })
}

/// A cell span count; an infinite span is kept so it clamps to the table edge.
pub(super) fn span(e: &El, k: &str) -> f64 {
    e.prop(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(1.0)
}

pub(super) fn finite(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite())
}
