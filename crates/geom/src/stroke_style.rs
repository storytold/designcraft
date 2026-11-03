//! Stroke styles drawn as fills: striped strokes (bands across the stroke weight), wavy and
//! straight-hash strokes. Dashes and dots are ordinary stroke dashes and don't come here.

use kurbo::{BezPath, CubicBez, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathSeg, Point, Stroke, Vec2};

/// `bp` offset sideways by `d` (positive = left of the direction of travel, y down).
pub fn offset_path(bp: &BezPath, d: f64, tol: f64) -> BezPath {
    if d.abs() < 1e-9 {
        return bp.clone();
    }
    let mut out = BezPath::new();
    let mut started = false;
    for seg in bp.segments() {
        let c = match seg {
            PathSeg::Line(l) => CubicBez::new(l.p0, l.p0.lerp(l.p1, 1.0 / 3.0), l.p0.lerp(l.p1, 2.0 / 3.0), l.p1),
            PathSeg::Quad(q) => q.raise(),
            PathSeg::Cubic(c) => c,
        };
        if (c.p3 - c.p0).hypot() < 1e-9 && (c.p1 - c.p0).hypot() < 1e-9 {
            continue;
        }
        let mut part = BezPath::new();
        kurbo::offset::offset_cubic(c, -d, tol, &mut part);
        for (i, el) in part.elements().iter().enumerate() {
            match (*el, i, started) {
                (kurbo::PathEl::MoveTo(p), 0, true) => out.line_to(p),
                (kurbo::PathEl::MoveTo(p), _, _) => out.move_to(p),
                (e, _, _) => out.push(e),
            }
        }
        started = true;
    }
    out
}

/// Bands (start, width) across a stroke, as fractions of the weight from its left edge.
pub type Bands = [(f64, f64)];

/// The filled area of a striped stroke of `weight` along `bp`.
pub fn stripes(bp: &BezPath, weight: f64, bands: &Bands, tol: f64) -> BezPath {
    let mut out = BezPath::new();
    for &(start, width) in bands {
        if width <= 0.0 {
            continue;
        }
        // The band's centre line, measured leftwards from the stroke's centre.
        let centre = (0.5 - start - width / 2.0) * weight;
        let line = offset_path(bp, centre, tol);
        let s = Stroke::new(width * weight).with_caps(kurbo::Cap::Butt).with_join(kurbo::Join::Miter);
        for el in kurbo::stroke(line.iter(), &s, &kurbo::StrokeOpts::default(), tol).elements() {
            out.push(*el);
        }
    }
    out
}

/// Most points [`samples`] places along one path. A wavy or hashed stroke places one every
/// few points of length, so a long path from a document (a segment thousands of kilometres
/// long) would otherwise take millions; past this the points spread out to fit the budget.
pub const MAX_STROKE_SAMPLES: usize = 100_000;

/// Points along `bp` every `step` of arc length, with the unit tangent there. When that would
/// take more than [`MAX_STROKE_SAMPLES`] points, the step widens so the path still gets them
/// evenly; a path whose length isn't finite gets none.
fn samples(bp: &BezPath, step: f64) -> Vec<(Point, Vec2)> {
    let lens: Vec<f64> = bp.segments().map(|seg| seg.arclen(1e-3)).collect();
    let total: f64 = lens.iter().sum();
    if !total.is_finite() || !step.is_finite() || step <= 0.0 {
        return Vec::new();
    }
    let step = step.max(total / MAX_STROKE_SAMPLES as f64);
    let mut out = Vec::new();
    let mut carry = 0.0;
    for (seg, &len) in bp.segments().zip(&lens) {
        if len < 1e-9 {
            continue;
        }
        let mut s = carry;
        while s <= len && out.len() < MAX_STROKE_SAMPLES {
            let t = seg.inv_arclen(s, 1e-3);
            let d = match seg {
                PathSeg::Line(l) => l.p1 - l.p0,
                PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
                PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
            };
            let n = d.hypot();
            if n > 1e-12 {
                out.push((seg.eval(t), d / n));
            }
            s += step;
        }
        carry = s - len;
    }
    out
}

/// A wavy stroke: the line swings `weight` wide, drawn a third of the weight thick.
pub fn wavy(bp: &BezPath, weight: f64, tol: f64) -> BezPath {
    let wavelength = (weight * 4.0).max(2.0);
    let pts = samples(bp, wavelength / 16.0);
    let mut line = BezPath::new();
    for (i, (p, t)) in pts.iter().enumerate() {
        let normal = Vec2::new(-t.y, t.x);
        let phase = i as f64 / 16.0 * std::f64::consts::TAU;
        let q = *p + normal * (phase.sin() * weight * 0.33);
        if i == 0 { line.move_to(q) } else { line.line_to(q) }
    }
    let s = Stroke::new(weight / 3.0).with_join(kurbo::Join::Round);
    kurbo::stroke(line.iter(), &s, &kurbo::StrokeOpts::default(), tol)
}

/// A straight-hash stroke: short ticks across the path, as wide as the weight.
pub fn hashed(bp: &BezPath, weight: f64, tol: f64) -> BezPath {
    let mut ticks = BezPath::new();
    for (p, t) in samples(bp, (weight * 0.6).max(1.0)) {
        let normal = Vec2::new(-t.y, t.x) * (weight / 2.0);
        ticks.move_to(p - normal);
        ticks.line_to(p + normal);
    }
    let s = Stroke::new((weight * 0.18).max(0.25)).with_caps(kurbo::Cap::Butt);
    kurbo::stroke(ticks.iter(), &s, &kurbo::StrokeOpts::default(), tol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape;

    fn line() -> BezPath {
        let mut b = BezPath::new();
        b.move_to((0.0, 0.0));
        b.line_to((100.0, 0.0));
        b
    }

    #[test]
    fn stripes_leave_gaps_across_the_weight() {
        // Thick (0–50%) and thin (75–100%) bands of a 12 pt stroke along y = 0.
        let s = stripes(&line(), 12.0, &[(0.0, 0.5), (0.75, 0.25)], 0.01);
        let inside = |y: f64| s.contains(Point::new(50.0, y));
        assert!(inside(-4.0), "thick band (left edge is −6)");
        assert!(!inside(1.5), "the gap");
        assert!(inside(4.5), "thin band");
        assert!(!inside(7.0), "outside the weight");
        let off = offset_path(&line(), 3.0, 0.01);
        assert!((off.bounding_box().y0 + 3.0).abs() < 1e-6, "left of travel is −y");
    }

    /// A path kilometres long gets at most the sample budget, not one point per few points of
    /// length; a path of infinite length gets none.
    #[test]
    fn long_paths_stay_within_the_sample_budget() {
        let mut long = BezPath::new();
        long.move_to((0.0, 0.0));
        long.line_to((1e12, 0.0));
        long.line_to((1e12, 1e12));
        assert!(samples(&long, 1.0).len() <= MAX_STROKE_SAMPLES);
        let h = hashed(&long, 1.0, 0.01).bounding_box();
        assert!(h.width() > 1e11, "{h:?}");
        assert!(!wavy(&long, 1.0, 0.01).is_empty());
        let mut huge = BezPath::new();
        huge.move_to((-f64::MAX, 0.0));
        huge.line_to((f64::MAX, 0.0));
        assert!(samples(&huge, 1.0).is_empty());
        assert_eq!(samples(&line(), 10.0).len(), 11, "short paths sample as before");
    }

    #[test]
    fn wavy_and_hash_cover_the_weight() {
        let w = wavy(&line(), 9.0, 0.01).bounding_box();
        assert!(w.height() > 4.0 && w.height() < 9.5, "{w:?}");
        let h = hashed(&line(), 8.0, 0.01).bounding_box();
        assert!((h.height() - 8.0).abs() < 0.5 && h.width() > 90.0, "{h:?}");
    }
}
