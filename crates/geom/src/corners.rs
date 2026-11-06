//! Live corner options (Object → Corner Options): rounded, inverse rounded, inset, bevel, fancy.
//!
//! Corners are applied at render time to the corner points of a closed path (where its direction
//! turns), so the frame keeps its simple editable geometry (as in InDesign). Curved sides stay
//! curved: a corner where an arc meets a line trims the arc along its length.

use kurbo::{BezPath, CubicBez, ParamCurve, ParamCurveArclen, Point, Vec2};
use serde::{Deserialize, Serialize};

use crate::path::{PathData, SubPath};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CornerShape {
    #[default]
    None,
    Rounded,
    InverseRounded,
    Inset,
    Bevel,
    Fancy,
}

impl CornerShape {
    pub const ALL: [CornerShape; 6] =
        [CornerShape::None, CornerShape::Fancy, CornerShape::Bevel, CornerShape::Inset, CornerShape::InverseRounded, CornerShape::Rounded];
    pub fn label(self) -> &'static str {
        match self {
            CornerShape::None => "None",
            CornerShape::Rounded => "Rounded",
            CornerShape::InverseRounded => "Inverse Rounded",
            CornerShape::Inset => "Inset",
            CornerShape::Bevel => "Bevel",
            CornerShape::Fancy => "Fancy",
        }
    }
}

/// One corner's setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    pub shape: CornerShape,
    pub size: f64,
}

impl Corner {
    pub fn is_none(&self) -> bool {
        self.shape == CornerShape::None || self.size <= 0.0
    }
}

/// Per-corner options in path order (for rectangles: top-left, top-right, bottom-right, bottom-left).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CornerOptions {
    pub corners: [Corner; 4],
}

impl CornerOptions {
    pub fn uniform(shape: CornerShape, size: f64) -> Self {
        Self { corners: [Corner { shape, size }; 4] }
    }
    pub fn is_none(&self) -> bool {
        self.corners.iter().all(Corner::is_none)
    }
}

/// Apply corner options to `path`. Corners are applied to the corner points of closed subpaths
/// (where the incoming and outgoing directions differ, curved sides included); smooth points are
/// kept. Anchor `i` of every subpath uses `opts.corners[i % 4]`.
pub fn apply(path: &PathData, opts: &CornerOptions) -> BezPath {
    if opts.is_none() {
        return path.to_bezpath();
    }
    let mut out = BezPath::new();
    for sp in &path.subpaths {
        apply_subpath(sp, opts, &mut out);
    }
    out
}

/// Arc length accuracy (points) when trimming curved sides.
const ARC_ACCURACY: f64 = 1e-3;
/// Directions closer than this (cosine, about 1°) are a smooth point, not a corner.
const SMOOTH: f64 = 0.9998;

/// The unit direction a segment leaves its start in (None for a point).
fn start_dir(c: &CubicBez) -> Option<Vec2> {
    [c.p1, c.p2, c.p3].into_iter().map(|q| q - c.p0).find(|d| d.hypot() > 1e-9).map(|d| d / d.hypot())
}

/// The unit direction a segment arrives at its end in (None for a point).
fn end_dir(c: &CubicBez) -> Option<Vec2> {
    [c.p2, c.p1, c.p0].into_iter().map(|q| c.p3 - q).find(|d| d.hypot() > 1e-9).map(|d| d / d.hypot())
}

/// Does the path turn where `prev` ends and `next` starts?
fn is_corner(prev: &CubicBez, next: &CubicBez) -> bool {
    matches!((end_dir(prev), start_dir(next)), (Some(a), Some(b)) if a.dot(b) < SMOOTH)
}

fn apply_subpath(sp: &SubPath, opts: &CornerOptions, out: &mut BezPath) {
    let n = sp.anchors.len();
    if !sp.closed || n < 2 {
        sp.to_bezpath_into(out);
        return;
    }
    // Segment `i` runs from anchor `i` to anchor `i + 1`; the corner at anchor `i` joins segments
    // `i - 1` and `i`, and takes at most half of each.
    let segs: Vec<CubicBez> = (0..n).map(|i| sp.segment(i)).collect();
    let lines: Vec<bool> = (0..n).map(|i| sp.segment_is_line(i)).collect();
    // Straight sides are measured (and trimmed below) exactly; curves by arc length.
    let lens: Vec<f64> = segs.iter().zip(&lines).map(|(c, line)| if *line { (c.p3 - c.p0).hypot() } else { c.arclen(ARC_ACCURACY) }).collect();
    let size: Vec<f64> = (0..n)
        .map(|i| {
            let c = opts.corners[i % 4];
            let prev = (i + n - 1) % n;
            if c.is_none() || !is_corner(&segs[prev], &segs[i]) {
                return 0.0;
            }
            c.size.min(lens[prev] / 2.0).min(lens[i] / 2.0).max(0.0)
        })
        .collect();
    if size.iter().all(|s| *s <= 0.0) {
        sp.to_bezpath_into(out);
        return;
    }
    // Each segment without the parts its corners take.
    let trimmed: Vec<CubicBez> = (0..n)
        .map(|i| {
            let (s0, s1) = (size[i], size[(i + 1) % n]);
            if lines[i] {
                let c = &segs[i];
                let u = if lens[i] > 1e-12 { (c.p3 - c.p0) / lens[i] } else { Vec2::ZERO };
                let (a, b) = (c.p0 + u * s0, c.p3 - u * s1);
                return CubicBez::new(a, a, b, b);
            }
            let t0 = if s0 > 0.0 { segs[i].inv_arclen(s0, ARC_ACCURACY) } else { 0.0 };
            let t1 = if s1 > 0.0 { segs[i].inv_arclen(lens[i] - s1, ARC_ACCURACY) } else { 1.0 };
            segs[i].subsegment(t0..t1.max(t0))
        })
        .collect();
    out.move_to(trimmed[0].p0);
    for i in 0..n {
        let seg = &trimmed[i];
        if lines[i] {
            out.line_to(seg.p3);
        } else {
            out.curve_to(seg.p1, seg.p2, seg.p3);
        }
        // The corner at the segment's end, from where this segment stops to where the next starts.
        let j = (i + 1) % n;
        let next = &trimmed[j];
        if size[j] > 0.0 {
            match (end_dir(seg).or_else(|| end_dir(&segs[i])), start_dir(next).or_else(|| start_dir(&segs[j]))) {
                (Some(ta), Some(tb)) => corner_shape(out, opts.corners[j % 4].shape, sp.anchors[j].p, seg.p3, next.p0, -ta, tb, size[j]),
                _ => out.line_to(next.p0),
            }
        }
    }
    out.close_path();
}

/// The outline of rectangle `r` with the edges in `edges` (top, right, bottom, left) and the
/// corners in `corners` (top left, top right, bottom right, bottom left). A corner gets its shape
/// only where both its edges are drawn; with an edge missing the outline is open there.
pub fn box_outline(r: kurbo::Rect, corners: &[Corner; 4], edges: [bool; 4]) -> BezPath {
    let r = r.abs();
    let v = [Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)];
    let mut out = BezPath::new();
    // Vertex i joins edge i - 1 (in) and edge i (out); edge i runs from v[i] to v[i + 1].
    let corner = |out: &mut BezPath, i: usize, start: bool| {
        let c = corners[i];
        let (p, prev, next) = (v[i], v[(i + 3) % 4], v[(i + 1) % 4]);
        let (din, dout) = (prev - p, next - p);
        let (lin, lout) = (din.hypot(), dout.hypot());
        if c.is_none() || lin < 1e-9 || lout < 1e-9 {
            if start {
                out.move_to(p)
            } else {
                out.line_to(p)
            }
            return;
        }
        let s = c.size.min(lin / 2.0).min(lout / 2.0);
        let (ui, uo) = (din / lin, dout / lout);
        let (a, b) = (p + ui * s, p + uo * s);
        if start {
            out.move_to(a)
        } else {
            out.line_to(a)
        }
        corner_shape(out, c.shape, p, a, b, ui, uo, s);
    };
    if edges.iter().all(|e| *e) {
        for i in 0..4 {
            corner(&mut out, i, i == 0);
        }
        out.close_path();
        return out;
    }
    // Runs of drawn edges, each starting after a missing one.
    let Some(first) = (0..4).find(|&i| edges[i] && !edges[(i + 3) % 4]) else { return out };
    for k in 0..4 {
        let i = (first + k) % 4;
        if !edges[i] {
            continue;
        }
        if !edges[(i + 3) % 4] {
            out.move_to(v[i]);
        }
        let j = (i + 1) % 4;
        if edges[j] {
            corner(&mut out, j, false);
        } else {
            out.line_to(v[j]);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn corner_shape(out: &mut BezPath, shape: CornerShape, p: Point, a: Point, b: Point, ui: Vec2, uo: Vec2, s: f64) {
    const K: f64 = crate::shapes::KAPPA;
    match shape {
        CornerShape::None => out.line_to(b),
        CornerShape::Rounded => out.curve_to(a - ui * (s * K), b - uo * (s * K), b),
        // Quarter arc centred on the corner point.
        CornerShape::InverseRounded => out.curve_to(a + uo * (s * K), b + ui * (s * K), b),
        CornerShape::Inset => {
            let m = p + ui * s + uo * s;
            out.line_to(m);
            out.line_to(b);
        }
        CornerShape::Bevel => out.line_to(b),
        CornerShape::Fancy => {
            // A decorative notch: step in, small inverse arc, step out.
            let q = s / 3.0;
            let a1 = p + ui * (2.0 * q);
            let m = p + ui * (2.0 * q) + uo * (2.0 * q);
            let b1 = p + uo * (2.0 * q);
            out.line_to(a1 + uo * q);
            out.curve_to(a1 + uo * (q + q * K), m - ui * (q * (1.0 - K)), m);
            out.curve_to(m - uo * (q * (1.0 - K)), b1 + ui * (q + q * K), b1 + ui * q);
            out.line_to(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::rectangle;
    use kurbo::{Rect, Shape};

    #[test]
    fn none_is_identity() {
        let r = rectangle(Rect::new(0.0, 0.0, 100.0, 50.0));
        assert_eq!(apply(&r, &CornerOptions::default()).bounding_box(), Rect::new(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn every_shape_stays_inside_bounds_and_reduces_area() {
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let r = rectangle(rect);
        for shape in CornerShape::ALL.into_iter().skip(1) {
            let bp = apply(&r, &CornerOptions::uniform(shape, 12.0));
            let bb = bp.bounding_box();
            assert!(bb.x0 >= -1e-6 && bb.y0 >= -1e-6 && bb.x1 <= 100.0 + 1e-6 && bb.y1 <= 60.0 + 1e-6, "{shape:?} {bb:?}");
            let area = bp.area().abs();
            assert!(area < 6000.0 && area > 5000.0, "{shape:?} area {area}");
        }
    }

    #[test]
    fn rounded_matches_circle_area() {
        let r = rectangle(Rect::new(0.0, 0.0, 20.0, 20.0));
        let bp = apply(&r, &CornerOptions::uniform(CornerShape::Rounded, 10.0));
        let a = bp.area().abs();
        assert!((a - std::f64::consts::PI * 100.0).abs() < 1.0, "{a}");
    }

    /// InDesign draws a 28.35 pt radius on a 51.02 pt tall frame as 25.51 (half the shorter side),
    /// as a circular arc (handles 0.5523 × the radius).
    #[test]
    fn radius_is_clamped_to_half_the_shorter_side() {
        let r = rectangle(Rect::new(0.0, 0.0, 200.0, 51.02));
        let bp = apply(&r, &CornerOptions::uniform(CornerShape::Rounded, 28.35));
        let half = 51.02 / 2.0;
        let mut arcs = 0;
        let mut last = Point::ZERO;
        for el in bp.elements() {
            match *el {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => last = p,
                kurbo::PathEl::CurveTo(c1, c2, p) => {
                    assert!(((p - last).x.abs() - half).abs() < 1e-9 && ((p - last).y.abs() - half).abs() < 1e-9, "{last:?} → {p:?}");
                    assert!(((c1 - last).hypot() - half * crate::shapes::KAPPA).abs() < 1e-9);
                    assert!(((c2 - p).hypot() - half * crate::shapes::KAPPA).abs() < 1e-9);
                    arcs += 1;
                    last = p;
                }
                _ => {}
            }
        }
        assert_eq!(arcs, 4);
        assert!((crate::shapes::KAPPA - 0.5523).abs() < 1e-4);
    }

    #[test]
    fn box_outline_rounds_only_closed_corners() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        let round = [Corner { shape: CornerShape::Rounded, size: 10.0 }; 4];
        let closed = box_outline(r, &round, [true; 4]);
        let curves = |bp: &BezPath| bp.elements().iter().filter(|e| matches!(e, kurbo::PathEl::CurveTo(..))).count();
        assert_eq!(curves(&closed), 4);
        assert!((closed.area().abs() - (5000.0 - (400.0 - std::f64::consts::PI * 100.0))).abs() < 1.0);
        // Open at the top: the two bottom corners are rounded, the top ends are square.
        let open = box_outline(r, &round, [false, true, true, true]);
        assert_eq!(curves(&open), 2);
        assert!(!open.elements().iter().any(|e| matches!(e, kurbo::PathEl::ClosePath)));
        // Open at both ends: two separate sides.
        let sides = box_outline(r, &round, [false, true, false, true]);
        assert_eq!(sides.elements().iter().filter(|e| matches!(e, kurbo::PathEl::MoveTo(_))).count(), 2);
        assert_eq!(curves(&sides), 0);
    }

    fn nearest(bp: &BezPath, p: Point) -> f64 {
        use kurbo::ParamCurveNearest;
        bp.segments().map(|s| s.nearest(p, 1e-6).distance_sq.sqrt()).fold(f64::INFINITY, f64::min)
    }

    /// A quarter disc of radius 100 centred on (0, 100): its arc from (0, 0) to (100, 100).
    fn quarter_disc() -> PathData {
        let k = 100.0 * crate::shapes::KAPPA;
        let mut a0 = crate::path::Anchor::corner(Point::new(0.0, 0.0));
        a0.h_out = Point::new(k, 0.0);
        let mut a1 = crate::path::Anchor::corner(Point::new(100.0, 100.0));
        a1.h_in = Point::new(100.0, 100.0 - k);
        let a2 = crate::path::Anchor::corner(Point::new(0.0, 100.0));
        PathData::new(vec![SubPath { anchors: vec![a0, a1, a2], closed: true }])
    }

    #[test]
    fn rounded_corners_keep_curved_sides_and_round_where_an_arc_meets_a_line() {
        let p = quarter_disc();
        let bp = apply(&p, &CornerOptions::uniform(CornerShape::Rounded, 20.0));
        // Still a quarter disc (area pi/4 * 100^2 = 7854), less three small rounded corners; a
        // triangle would be 5000.
        let area = bp.area().abs();
        assert!(area > 7300.0 && area < 7854.0, "area {area}");
        // The middle of the arc is kept.
        let mid = Point::new(100.0 * std::f64::consts::FRAC_1_SQRT_2, 100.0 - 100.0 * std::f64::consts::FRAC_1_SQRT_2);
        assert!(nearest(&bp, mid) < 0.05, "{}", nearest(&bp, mid));
        // All three corners are rounded off: the line-line one and both where the arc meets a line.
        for corner in [Point::new(0.0, 0.0), Point::new(100.0, 100.0), Point::new(0.0, 100.0)] {
            assert!(nearest(&bp, corner) > 3.0, "{corner:?} is still sharp");
        }
    }

    #[test]
    fn straight_sides_are_trimmed_exactly() {
        let r = rectangle(Rect::new(0.0, 0.0, 100.0, 60.0));
        let bp = apply(&r, &CornerOptions::uniform(CornerShape::Bevel, 10.0));
        // Every point a bevelled rectangle goes through sits exactly 10 pt from a corner.
        let pts: Vec<Point> = bp
            .elements()
            .iter()
            .filter_map(|e| match e {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect();
        assert_eq!(pts.len(), 9, "{pts:?}");
        for p in pts {
            assert!([0.0, 10.0, 90.0, 100.0].contains(&p.x) && [0.0, 10.0, 50.0, 60.0].contains(&p.y), "{p:?}");
        }
    }

    #[test]
    fn smooth_points_are_not_corners() {
        let e = crate::shapes::ellipse(Rect::new(0.0, 0.0, 100.0, 60.0));
        for shape in CornerShape::ALL.into_iter().skip(1) {
            let bp = apply(&e, &CornerOptions::uniform(shape, 20.0));
            assert!((bp.area().abs() - e.to_bezpath().area().abs()).abs() < 1e-6, "{shape:?}");
        }
    }

    #[test]
    fn size_is_clamped_to_half_edge() {
        let r = rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        let bp = apply(&r, &CornerOptions::uniform(CornerShape::Bevel, 100.0));
        // Bevel at half edges gives a diamond: area 50.
        assert!((bp.area().abs() - 50.0).abs() < 1e-6);
    }
}
