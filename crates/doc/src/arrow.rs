//! Arrowheads at the ends of open paths (Stroke panel › Start/End). Each kind is a small shape
//! drawn in the stroke colour, sized by the stroke weight; the path is shortened under heads that
//! end in a point so the stroke doesn't show past the tip.

use designcraft_geom::kurbo::{self, ParamCurve, ParamCurveArclen, ParamCurveDeriv};
use designcraft_geom::{Affine, BezPath, PathSeg, Point, Vec2};

use crate::item::{Arrowhead, Stroke};

/// A head ready to draw: filled, or outlined with `outline` width.
#[derive(Clone, Debug)]
pub struct ArrowShape {
    pub path: BezPath,
    pub outline: Option<f64>,
}

/// Arrowhead outline in a local frame: tip at the origin, pointing along +x, for weight `w`.
/// Returns the shape, whether it is outlined, and how far to pull the path back from the tip.
fn local(kind: Arrowhead, w: f64) -> Option<(BezPath, bool, f64)> {
    // Heads stay legible on hairlines.
    let k = w.max(0.5);
    let poly = |pts: &[(f64, f64)]| {
        let mut p = BezPath::new();
        p.move_to(pts[0]);
        for q in &pts[1..] {
            p.line_to(*q);
        }
        p.close_path();
        p
    };
    let open = |pts: &[(f64, f64)]| {
        let mut p = BezPath::new();
        p.move_to(pts[0]);
        for q in &pts[1..] {
            p.line_to(*q);
        }
        p
    };
    Some(match kind {
        Arrowhead::None => return None,
        Arrowhead::Triangle => (poly(&[(0.0, 0.0), (-4.5 * k, 2.0 * k), (-4.5 * k, -2.0 * k)]), false, 4.0 * k),
        Arrowhead::TriangleWide => (poly(&[(0.0, 0.0), (-4.5 * k, 3.2 * k), (-4.5 * k, -3.2 * k)]), false, 4.0 * k),
        Arrowhead::Simple => (open(&[(-4.0 * k, 2.5 * k), (-0.5 * k, 0.0), (-4.0 * k, -2.5 * k)]), true, 0.5 * k),
        Arrowhead::SimpleWide => (open(&[(-3.0 * k, 3.6 * k), (-0.5 * k, 0.0), (-3.0 * k, -3.6 * k)]), true, 0.5 * k),
        Arrowhead::Barbed => (poly(&[(0.0, 0.0), (-5.0 * k, 2.6 * k), (-3.4 * k, 0.0), (-5.0 * k, -2.6 * k)]), false, 3.4 * k),
        Arrowhead::Curved => {
            let mut p = BezPath::new();
            p.move_to((0.0, 0.0));
            p.quad_to((-2.2 * k, 0.6 * k), (-5.0 * k, 2.8 * k));
            p.quad_to((-3.8 * k, 1.0 * k), (-3.6 * k, 0.0));
            p.quad_to((-3.8 * k, -k), (-5.0 * k, -2.8 * k));
            p.quad_to((-2.2 * k, -0.6 * k), (0.0, 0.0));
            p.close_path();
            (p, false, 3.6 * k)
        }
        // Circles and squares sit centred on the end point.
        Arrowhead::Circle | Arrowhead::CircleSolid => {
            let r = 2.2 * k;
            let p = kurbo::Shape::to_path(&kurbo::Circle::new((0.0, 0.0), r), 0.01);
            let outline = kind == Arrowhead::Circle;
            (p, outline, if outline { r } else { 0.0 })
        }
        Arrowhead::Square | Arrowhead::SquareSolid => {
            let h = 2.0 * k;
            let outline = kind == Arrowhead::Square;
            (poly(&[(-h, -h), (h, -h), (h, h), (-h, h)]), outline, if outline { h } else { 0.0 })
        }
        Arrowhead::Bar => (poly(&[(-0.5 * k, -3.0 * k), (0.5 * k, -3.0 * k), (0.5 * k, 3.0 * k), (-0.5 * k, 3.0 * k)]), false, 0.0),
    })
}

/// The path end: its point and the outward direction (unit).
fn end_of(seg: &PathSeg, at_start: bool) -> (Point, Vec2) {
    let (t, p) = if at_start { (0.0, seg.start()) } else { (1.0, seg.end()) };
    let mut d = match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        PathSeg::Cubic(c) => {
            // A handle sitting on its anchor has no direction: the tangent then points at the next
            // control point of the segment that differs from the end point.
            let pts = if at_start { [c.p1 - c.p0, c.p2 - c.p0, c.p3 - c.p0] } else { [c.p3 - c.p2, c.p3 - c.p1, c.p3 - c.p0] };
            pts.into_iter().find(|v| v.hypot() >= 1e-9).unwrap_or(Vec2::ZERO)
        }
    };
    if d.hypot() < 1e-9 {
        d = seg.end() - seg.start();
    }
    let d = if at_start { -d } else { d };
    (p, if d.hypot() < 1e-12 { Vec2::new(1.0, 0.0) } else { d.normalize() })
}

/// Shorten a segment by `by` (arc length) at its start or end.
fn trim(seg: PathSeg, by: f64, at_start: bool) -> PathSeg {
    let len = seg.arclen(1e-3);
    if by <= 0.0 || len < 1e-9 {
        return seg;
    }
    let by = by.min(len * 0.9);
    if at_start {
        let t = seg.inv_arclen(by, 1e-3);
        seg.subsegment(t..1.0)
    } else {
        let t = seg.inv_arclen(len - by, 1e-3);
        seg.subsegment(0.0..t)
    }
}

/// The path to stroke (shortened under pointed heads) and the heads to draw, for an open path
/// with arrowheads. `None` when there is nothing to change (closed path, no heads).
pub fn apply(bp: &BezPath, st: &Stroke, closed: bool) -> Option<(BezPath, Vec<ArrowShape>)> {
    if closed || (st.start == Arrowhead::None && st.end == Arrowhead::None) {
        return None;
    }
    let mut segs: Vec<PathSeg> = bp.segments().collect();
    if segs.is_empty() {
        return None;
    }
    let mut heads = Vec::new();
    let last = segs.len() - 1;
    for (kind, at_start) in [(st.start, true), (st.end, false)] {
        let Some((shape, outlined, pull)) = local(kind, st.weight) else { continue };
        let i = if at_start { 0 } else { last };
        let (tip, dir) = end_of(&segs[i], at_start);
        let xf = Affine::translate(tip.to_vec2()) * Affine::rotate(dir.y.atan2(dir.x));
        heads.push(ArrowShape { path: xf * shape, outline: outlined.then_some(st.weight.max(0.5) * 0.9) });
        segs[i] = trim(segs[i], pull, at_start);
    }
    let mut out = BezPath::new();
    for (n, s) in segs.iter().enumerate() {
        // A new subpath wherever the original had one (gaps between consecutive segments).
        if n == 0 || (segs[n - 1].end() - s.start()).hypot() > 1e-6 {
            out.move_to(s.start());
        }
        match s {
            PathSeg::Line(l) => out.line_to(l.p1),
            PathSeg::Quad(q) => out.quad_to(q.p1, q.p2),
            PathSeg::Cubic(c) => out.curve_to(c.p1, c.p2, c.p3),
        }
    }
    Some((out, heads))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_point_along_the_path_and_trim_it() {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.line_to((100.0, 0.0));
        let st = Stroke { weight: 2.0, start: Arrowhead::Circle, end: Arrowhead::Triangle, ..Stroke::default() };
        let (line, heads) = apply(&bp, &st, false).unwrap();
        assert_eq!(heads.len(), 2);
        // The triangle's tip is the path end; its body lies before it.
        let tb = kurbo::Shape::bounding_box(&heads[1].path);
        assert!((tb.x1 - 100.0).abs() < 1e-6 && tb.x0 < 95.0, "{tb:?}");
        assert!(heads[1].outline.is_none());
        // The circle is centred on the start, outlined; the line starts at its edge.
        let cb = kurbo::Shape::bounding_box(&heads[0].path);
        assert!((cb.center().x).abs() < 1e-6, "{cb:?}");
        assert!(heads[0].outline.is_some());
        let lb = kurbo::Shape::bounding_box(&line);
        assert!((lb.x0 - 4.4).abs() < 1e-3 && (lb.x1 - 92.0).abs() < 1e-3, "{lb:?}");
        // Closed paths and plain strokes are left alone.
        assert!(apply(&bp, &st, true).is_none());
        assert!(apply(&bp, &Stroke::default(), false).is_none());
    }

    #[test]
    fn head_follows_a_curve_tangent() {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.curve_to((50.0, 0.0), (100.0, 50.0), (100.0, 100.0));
        let st = Stroke { weight: 1.0, end: Arrowhead::Bar, ..Stroke::default() };
        let (_, heads) = apply(&bp, &st, false).unwrap();
        // Ends heading straight down: the bar lies across, horizontally.
        let b = kurbo::Shape::bounding_box(&heads[0].path);
        assert!(b.width() > b.height() * 3.0, "{b:?}");
    }
}
