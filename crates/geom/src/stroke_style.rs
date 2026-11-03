//! Stroke styles drawn as fills: striped strokes (bands across the stroke weight), wavy and
//! straight-hash strokes. Dashes and dots are ordinary stroke dashes and don't come here.

use kurbo::{BezPath, CubicBez, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathSeg, Point, Stroke, Vec2};

/// `bp` offset sideways by `d` (positive = left of the direction of travel, y down).
pub fn offset_path(bp: &BezPath, d: f64, tol: f64) -> BezPath {
    if d.abs() < 1e-9 {
        return bp.clone();
    }
    let mut out = BezPath::new();
    for sub in subpaths(bp) {
        offset_subpath(&sub, d, tol, &mut out);
    }
    out
}

/// `bp` split at each `MoveTo` into its subpaths.
fn subpaths(bp: &BezPath) -> Vec<BezPath> {
    let mut out: Vec<BezPath> = Vec::new();
    for el in bp.elements() {
        if matches!(el, kurbo::PathEl::MoveTo(_)) || out.is_empty() {
            out.push(BezPath::new());
        }
        if let Some(sub) = out.last_mut() {
            sub.push(*el);
        }
    }
    out
}

/// One subpath offset by `d`, its segments joined into one line, appended to `out`.
fn offset_subpath(bp: &BezPath, d: f64, tol: f64, out: &mut BezPath) {
    let mut started = false;
    for seg in bp.segments() {
        let c = match seg {
            PathSeg::Line(l) => CubicBez::new(l.p0, l.p0.lerp(l.p1, 1.0 / 3.0), l.p0.lerp(l.p1, 2.0 / 3.0), l.p1),
            PathSeg::Quad(q) => q.raise(),
            PathSeg::Cubic(c) => c,
        };
        if (c.p3 - c.p0).hypot() < 1e-9 && (c.p1 - c.p0).hypot() < 1e-9 && (c.p2 - c.p0).hypot() < 1e-9 {
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
}

/// Bands (start, width) across a stroke, as fractions of the weight from its left edge.
pub type Bands = [(f64, f64)];

/// Most bands of a striped stroke that are drawn, and most path elements its fill may have:
/// every band strokes the whole path again, so both multiply the path's size.
pub const MAX_STRIPE_BANDS: usize = 64;
const MAX_STRIPE_ELEMENTS: usize = 2_000_000;

/// The filled area of a striped stroke of `weight` along `bp`.
pub fn stripes(bp: &BezPath, weight: f64, bands: &Bands, tol: f64) -> BezPath {
    let mut out = BezPath::new();
    for &(start, width) in bands.iter().take(MAX_STRIPE_BANDS) {
        if out.elements().len() > MAX_STRIPE_ELEMENTS {
            break;
        }
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

/// Points along each subpath of `bp` every `step` of arc length, with the unit tangent there.
/// When that would take more than [`MAX_STROKE_SAMPLES`] points, the step widens so the path
/// still gets them evenly; a path whose length isn't finite gets none.
fn samples(bp: &BezPath, step: f64) -> Vec<Vec<(Point, Vec2)>> {
    let subs = subpaths(bp);
    let total: f64 = subs.iter().flat_map(|s| s.segments()).map(|seg| seg.arclen(1e-3)).sum();
    if !total.is_finite() || !step.is_finite() || step <= 0.0 {
        return Vec::new();
    }
    let step = step.max(total / MAX_STROKE_SAMPLES as f64);
    subs.iter().map(|sub| subpath_samples(sub, step)).collect()
}

/// Points along one subpath every `step` of arc length, with the unit tangent there.
fn subpath_samples(bp: &BezPath, step: f64) -> Vec<(Point, Vec2)> {
    let lens: Vec<f64> = bp.segments().map(|seg| seg.arclen(1e-3)).collect();
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
    let mut line = BezPath::new();
    for pts in samples(bp, wavelength / 16.0) {
        for (i, (p, t)) in pts.iter().enumerate() {
            let normal = Vec2::new(-t.y, t.x);
            let phase = i as f64 / 16.0 * std::f64::consts::TAU;
            let q = *p + normal * (phase.sin() * weight * 0.33);
            if i == 0 { line.move_to(q) } else { line.line_to(q) }
        }
    }
    let s = Stroke::new(weight / 3.0).with_join(kurbo::Join::Round);
    kurbo::stroke(line.iter(), &s, &kurbo::StrokeOpts::default(), tol)
}

/// A straight-hash stroke: short ticks across the path, as wide as the weight.
pub fn hashed(bp: &BezPath, weight: f64, tol: f64) -> BezPath {
    let mut ticks = BezPath::new();
    for (p, t) in samples(bp, (weight * 0.6).max(1.0)).into_iter().flatten() {
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

    #[test]
    fn stripes_draw_at_most_the_band_cap() {
        let mut line = BezPath::new();
        line.move_to((0.0, 0.0));
        line.line_to((100.0, 0.0));
        let one = stripes(&line, 4.0, &[(0.0, 0.1)], 0.1).elements().len();
        let many = vec![(0.0, 0.1); 100_000];
        let all = stripes(&line, 4.0, &many, 0.1).elements().len();
        assert_eq!(all, one * MAX_STRIPE_BANDS);
    }
    use kurbo::Shape;

    fn line() -> BezPath {
        let mut b = BezPath::new();
        b.move_to((0.0, 0.0));
        b.line_to((100.0, 0.0));
        b
    }

    #[test]
    fn stripes_keep_cubic_with_only_second_control_away_from_start() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to((0.0, 0.0), (90.0, 0.0), (0.0, 0.0));
        let offset = offset_path(&path, 2.0, 0.01).bounding_box();
        assert!(offset.x1 > 39.0, "{offset:?}");
        assert!(offset.y0 < -1.9 && offset.y1 > 1.9, "{offset:?}");
        let filled = stripes(&path, 12.0, &[(0.0, 0.5), (0.75, 0.25)], 0.01);
        assert!(filled.contains(Point::new(20.0, -3.0)));
        assert!(filled.contains(Point::new(20.0, 3.0)));
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

    #[test]
    fn styled_strokes_keep_subpaths_apart() {
        // Two separate lines, 50 pt apart, in one compound path.
        let mut b = line();
        b.move_to((0.0, 50.0));
        b.line_to((100.0, 50.0));
        let moves = |p: &BezPath| p.elements().iter().filter(|e| matches!(e, kurbo::PathEl::MoveTo(_))).count();
        assert_eq!(moves(&offset_path(&b, 3.0, 0.01)), 2, "one offset subpath per input subpath");
        let s = stripes(&b, 12.0, &[(0.0, 0.5), (0.75, 0.25)], 0.01);
        assert!(!s.contains(Point::new(50.0, 25.0)), "stripe bridges the subpaths: {:?}", s.bounding_box());
        assert!(s.contains(Point::new(50.0, 46.0)), "second line is still striped");
        let w = wavy(&b, 4.0, 0.01);
        let bridged = (0..=2000).map(|i| f64::from(i) * 0.05).filter(|&x| w.contains(Point::new(x, 25.0))).count();
        assert_eq!(bridged, 0, "wavy bridges the subpaths: {:?}", w.bounding_box());
    }
}
