//! White or [Paper] set to overprint (#2401). A paint that lays down no ink knocks out what is
//! beneath it, which is how white shows on press. Set to overprint, it leaves the inks beneath
//! alone instead, so the object is gone from the printed page while the screen, without
//! Overprint Preview, still shows it.
//!
//! What can overprint: a page item's fill, stroke and stroke gap (Attributes panel), and ruby and
//! kenten set to overprint. Text itself has no overprint attribute, so white text knocks out and
//! prints; the frame's Overprint Fill covers the frame's own fill only.

use designcraft_color::{Color, ColorType, SwatchValue, swatch};
use designcraft_doc::cjk_settings::AdornmentOverprint;
use designcraft_doc::{CharProps, Document, Item, StrokeType};

use super::Issue;

/// The issue's `kind`.
pub(super) const KIND: &str = "whiteOverprint";

/// Swatch tints that refer to tints, followed before giving up (as `swatch::resolve` does).
const MAX_TINT_DEPTH: usize = 8;

/// Whether a paint lays down no ink: [Paper] (whatever colour it is shown in), or a colour or
/// tint that comes out white (C=0 M=0 Y=0 K=0, a 0% tint, RGB white). A spot colour prints on
/// its own plate whatever colour stands in for it (a white ink, a varnish), so only a 0% tint of
/// it is white. [None] and gradients are not white.
fn prints_no_ink(d: &Document, name: &str, tint: f32) -> bool {
    const INK: f32 = 0.001;
    let (mut base, mut t) = (name, tint);
    for _ in 0..=MAX_TINT_DEPTH {
        match d.swatches.iter().find(|s| s.name == base).map(|s| &s.value) {
            Some(SwatchValue::Paper { .. }) => return true,
            Some(SwatchValue::Tint { base: b, tint: bt }) => (base, t) = (b, t * bt),
            // A NaN tint compares false: not white.
            Some(SwatchValue::Color { color_type: ColorType::Spot, .. }) => return t <= INK,
            Some(SwatchValue::Color { .. } | SwatchValue::Registration) => break,
            Some(SwatchValue::None | SwatchValue::Gradient { .. }) | None => return false,
        }
    }
    // A NaN component compares false: not white.
    match d.resolve_color(name, tint) {
        Some(Color::Cmyk { c, m, y, k }) => [c, m, y, k].iter().all(|v| *v <= INK),
        Some(Color::Gray { k }) => k <= INK,
        Some(Color::Rgb { r, g, b }) => [r, g, b].iter().all(|v| *v >= 1.0 - INK),
        None => false,
    }
}

/// "fill", "fill and stroke", "fill, stroke and stroke gap".
fn list(parts: &[&str]) -> String {
    match parts.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => (*last).to_string(),
        None => String::new(),
    }
}

/// The warning for `parts` (paints, or ruby and kenten) of one item; none for no parts.
pub(super) fn issue(parts: &[&str], item: Option<u64>, page: Option<usize>) -> Option<Issue> {
    (!parts.is_empty()).then(|| Issue {
        severity: "warning",
        kind: KIND,
        message: format!("White or [Paper] {} set to overprint will not print", list(parts)),
        item,
        page,
    })
}

/// A page item's fill, stroke or stroke gap that is white and set to overprint.
pub(super) fn item_issue(d: &Document, it: &Item, page: Option<usize>) -> Option<Issue> {
    if it.nonprinting {
        return None;
    }
    let mut parts = Vec::new();
    if it.fill.overprint && !it.fill.is_none() && prints_no_ink(d, &it.fill.swatch, it.fill.tint) {
        parts.push("fill");
    }
    let st = &it.stroke;
    if st.overprint && !st.is_none() && prints_no_ink(d, &st.swatch, st.tint) {
        parts.push("stroke");
    }
    // The gap colour shows between dashes, dots and stripes only.
    if st.gap_overprint && !st.is_none() && !matches!(d.stroke_kind(&st.kind), StrokeType::Solid) && prints_no_ink(d, &st.gap_swatch, st.gap_tint) {
        parts.push("stroke gap");
    }
    issue(&parts, Some(it.id.0), page)
}

/// Ruby and kenten of a run that are white and set to overprint, with the colours composition
/// gives them (`kenten::mark_props`, `ruby::ruby_style`): an empty swatch is the text's.
pub(super) fn adornments(d: &Document, p: &CharProps) -> Vec<&'static str> {
    let tint = |t: Option<f32>| t.filter(|t| t.is_finite()).map(|t| t.clamp(0.0, 1.0));
    let white = |on: AdornmentOverprint, name: &str, tint: f32, weight: Option<f64>| {
        on == AdornmentOverprint::On && name != swatch::NONE && weight.is_none_or(|w| w.is_finite() && w > 0.0) && prints_no_ink(d, name, tint)
    };
    let mut out = Vec::new();
    if p.kenten {
        let fill = if p.kenten_fill.is_empty() { &p.fill } else { &p.kenten_fill };
        let stroke = if p.kenten_stroke.is_empty() { &p.stroke } else { &p.kenten_stroke };
        let weight = p.kenten_stroke_weight.filter(|w| w.is_finite()).unwrap_or(p.stroke_weight);
        if white(p.kenten_overprint_fill, fill, tint(p.kenten_fill_tint).unwrap_or(p.fill_tint), None)
            || white(p.kenten_overprint_stroke, stroke, tint(p.kenten_stroke_tint).unwrap_or(p.stroke_tint), Some(weight))
        {
            out.push("kenten");
        }
    }
    if !p.ruby.is_empty() {
        // A ruby colour of its own is at 100% unless it has a tint; without one, the text's.
        let (fill, fill_tint) = if p.ruby_fill.is_empty() {
            (&p.fill, tint(p.ruby_fill_tint).unwrap_or(p.fill_tint))
        } else {
            (&p.ruby_fill, tint(p.ruby_fill_tint).unwrap_or(1.0))
        };
        let (stroke, stroke_tint) = if p.ruby_stroke.is_empty() {
            (&p.stroke, tint(p.ruby_stroke_tint).unwrap_or(p.stroke_tint))
        } else {
            (&p.ruby_stroke, tint(p.ruby_stroke_tint).unwrap_or(1.0))
        };
        let weight = p.ruby_stroke_weight.filter(|w| w.is_finite()).unwrap_or(p.stroke_weight);
        if white(p.ruby_overprint_fill, fill, fill_tint, None) || white(p.ruby_overprint_stroke, stroke, stroke_tint, Some(weight)) {
            out.push("ruby");
        }
    }
    out
}

#[cfg(test)]
mod tests;
