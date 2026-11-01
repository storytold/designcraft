use super::face::FontFace;
use super::{BezPath, DrawSettings, GlyphId, MetadataProvider, Size};

/// Does the face's "l" (else "I") have serifs: much wider at its foot than mid-stem? `None` when
/// it has neither letter.
pub(super) fn has_serifs(face: &FontFace) -> Option<bool> {
    let gid = ['l', 'I'].into_iter().map(|c| face.glyph_for(c)).find(|g| *g != 0)?;
    let path = font_outline(face, gid);
    let bbox = kurbo::Shape::bounding_box(&path);
    // An empty or degenerate outline (NaN included) says nothing.
    if bbox.height().is_nan() || bbox.height() <= 1.0 {
        return None;
    }
    let mut edges = Vec::new();
    let mut last = None;
    let mut start = None;
    kurbo::flatten(&path, 1.0, |el| match el {
        kurbo::PathEl::MoveTo(p) => {
            last = Some(p);
            start = Some(p);
        }
        kurbo::PathEl::LineTo(p) => {
            if let Some(a) = last {
                edges.push((a, p));
            }
            last = Some(p);
        }
        kurbo::PathEl::ClosePath => {
            if let (Some(a), Some(b)) = (last, start) {
                edges.push((a, b));
            }
            last = start;
        }
        _ => {}
    });
    // The outline is y-down: the foot is near bbox.y1 (the baseline).
    let width_at = |y: f64| {
        let xs: Vec<f64> = edges
            .iter()
            .filter(|(a, b)| (a.y <= y && y < b.y) || (b.y <= y && y < a.y))
            .map(|(a, b)| a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y))
            .collect();
        let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
        (hi > lo).then_some(hi - lo)
    };
    let foot = width_at(bbox.y1 - bbox.height() * 0.02)?;
    let stem = width_at(bbox.y0 + bbox.height() * 0.5)?;
    Some(foot > stem * 1.6)
}

/// Glyph `gid`'s outline as the font draws it, in font units, y-down (flipped).
pub(super) fn font_outline(face: &FontFace, gid: u32) -> BezPath {
    let mut pen = FlipPen(BezPath::new());
    if let Some(f) = face.skrifa()
        && let Some(g) = f.outline_glyphs().get(GlyphId::new(gid))
    {
        let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), face.location()), &mut pen);
    }
    pen.0
}

pub(super) struct FlipPen(pub(super) BezPath);
