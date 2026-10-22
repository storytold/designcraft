//! Type on a path: positions along a path by arc length, with the tangent angle.

use kurbo::{Affine, BezPath, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathSeg, Point};

/// A path measured for placing glyphs along it.
#[derive(Clone, Debug)]
pub struct PathWarp {
    segs: Vec<(PathSeg, f64)>,
    total: f64,
    /// Glyphs run the other way (Type on a Path Options › Flip).
    flip: bool,
}

impl PathWarp {
    pub fn new(bp: &BezPath, flip: bool) -> Self {
        let segs: Vec<(PathSeg, f64)> = bp.segments().map(|s| (s, s.arclen(1e-3))).collect();
        let total = segs.iter().map(|s| s.1).sum();
        PathWarp { segs, total, flip }
    }

    pub fn length(&self) -> f64 {
        self.total
    }

    /// Point and tangent angle (radians) at arc length `s` (clamped to the path).
    pub fn at(&self, s: f64) -> Option<(Point, f64)> {
        let s = if self.flip { self.total - s } else { s };
        let mut s = s.max(0.0).min(self.total);
        let n = self.segs.len();
        for (i, (seg, len)) in self.segs.iter().enumerate() {
            if s <= *len || i + 1 == n {
                let t = if *len > 1e-9 { seg.inv_arclen(s.min(*len), 1e-4) } else { 0.0 };
                let p = seg.eval(t);
                let d = match seg {
                    PathSeg::Line(l) => l.p1 - l.p0,
                    PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
                    PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
                };
                let mut a = d.y.atan2(d.x);
                if self.flip {
                    a += std::f64::consts::PI;
                }
                return Some((p, a));
            }
            s -= len;
        }
        None
    }

    /// The arc length (as `at` measures it, so flipped paths count from their end) of the point
    /// of the path nearest `p`.
    pub fn nearest(&self, p: Point) -> f64 {
        use kurbo::ParamCurveNearest;
        let mut best = (f64::INFINITY, 0.0);
        let mut before = 0.0;
        for (seg, len) in &self.segs {
            let n = seg.nearest(p, 1e-6);
            if n.distance_sq < best.0 {
                best = (n.distance_sq, before + seg.subsegment(0.0..n.t).arclen(1e-4));
            }
            before += len;
        }
        if self.flip { self.total - best.1 } else { best.1 }
    }

    /// Transform for something laid out on a straight line (x along the line, y down from the
    /// line) whose horizontal centre is `mid`: it lands centred at arc length `mid`, turned with
    /// the path, its line at `baseline` on the path.
    pub fn glyph(&self, mid: f64, baseline: f64) -> Option<Affine> {
        let (p, a) = self.at(mid)?;
        Some(Affine::translate(p.to_vec2()) * Affine::rotate(a) * Affine::translate((-mid, -baseline)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_and_angles_along_a_path() {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.line_to((100.0, 0.0));
        bp.line_to((100.0, 100.0));
        let w = PathWarp::new(&bp, false);
        assert!((w.length() - 200.0).abs() < 1e-6);
        let (p, a) = w.at(150.0).unwrap();
        assert!((p - Point::new(100.0, 50.0)).hypot() < 1e-6);
        assert!((a - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        // A glyph's centre (x = 50 on its line, baseline at y = 10) lands on the path at 50.
        let g = w.glyph(50.0, 10.0).unwrap();
        assert!((g * Point::new(50.0, 10.0) - Point::new(50.0, 0.0)).hypot() < 1e-9);
        let flipped = PathWarp::new(&bp, true);
        assert!((flipped.at(0.0).unwrap().0 - Point::new(100.0, 100.0)).hypot() < 1e-9);
        // The nearest point by arc length, off the path and on it; flipped counts from the end.
        assert!((w.nearest(Point::new(120.0, 50.0)) - 150.0).abs() < 1e-6);
        assert!((w.nearest(Point::new(120.0, 150.0)) - 200.0).abs() < 1e-6, "past the end: the end");
        assert!((w.nearest(Point::new(30.0, -8.0)) - 30.0).abs() < 1e-6);
        assert!((flipped.nearest(Point::new(30.0, -8.0)) - 170.0).abs() < 1e-6);
    }
}
