//! Live corner options (Object → Corner Options): rounded, inverse rounded, inset, bevel, fancy.
//!
//! Corners are applied at render time to the straight-segment corners of a closed path, so the
//! frame keeps its simple editable geometry (as in InDesign).

use kurbo::{BezPath, Point, Vec2};
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

/// Apply corner options to `path`. Corners are applied to anchors joining two straight segments of
/// closed subpaths; other anchors are kept. Anchor `i` of every subpath uses `opts.corners[i % 4]`.
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

fn apply_subpath(sp: &SubPath, opts: &CornerOptions, out: &mut BezPath) {
    let n = sp.anchors.len();
    let straight_corner = |i: usize| {
        let a = &sp.anchors[i];
        sp.closed && !a.has_in() && !a.has_out()
    };
    if !sp.closed || n < 3 || !(0..n).any(straight_corner) {
        sp.to_bezpath_into(out);
        return;
    }
    let pts: Vec<Point> = sp.anchors.iter().map(|a| a.p).collect();
    let mut first = true;
    for i in 0..n {
        let c = opts.corners[i % 4];
        let p = pts[i];
        let prev = pts[(i + n - 1) % n];
        let next = pts[(i + 1) % n];
        if c.is_none() || !straight_corner(i) || !straight_corner((i + n - 1) % n) && sp.anchors[(i + n - 1) % n].has_out() {
            if first {
                out.move_to(p);
                first = false;
            } else {
                out.line_to(p);
            }
            continue;
        }
        let din = prev - p;
        let dout = next - p;
        let (lin, lout) = (din.hypot(), dout.hypot());
        if lin < 1e-9 || lout < 1e-9 {
            continue;
        }
        let s = c.size.min(lin / 2.0).min(lout / 2.0);
        let ui = din / lin;
        let uo = dout / lout;
        let a = p + ui * s; // entry point on incoming edge
        let b = p + uo * s; // exit point on outgoing edge
        if first {
            out.move_to(a);
            first = false;
        } else {
            out.line_to(a);
        }
        corner_shape(out, c.shape, p, a, b, ui, uo, s);
    }
    out.close_path();
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
    fn size_is_clamped_to_half_edge() {
        let r = rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        let bp = apply(&r, &CornerOptions::uniform(CornerShape::Bevel, 100.0));
        // Bevel at half edges gives a diamond: area 50.
        assert!((bp.area().abs() - 50.0).abs() < 1e-6);
    }
}
