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

/// A closed path moved `d` outwards (negative: inwards), whichever way it runs: the stroke line
/// of round dots aligned outside or inside the path, which stay whole rather than being clipped.
/// Each contour moves on its own, away from the shape's filled area (a hole's contour into the
/// hole), and stays a separate closed contour.
pub fn offset_outward(bp: &BezPath, d: f64, tol: f64) -> BezPath {
    use kurbo::Shape;
    let mut out = BezPath::new();
    for contour in contours(bp) {
        // Outside is the side away from the filled area: test a point just left of the
        // contour's travel, at the first place along it where it has a direction (a cubic can
        // stand still at one point, so other points and segments are tried).
        let Some((at, dir)) =
            contour.segments().flat_map(|seg| [0.5, 0.25, 0.75, 0.1, 0.9].map(|t| (seg.eval(t), seg.deriv_at(t)))).find(|(_, d)| d.hypot() > 1e-9)
        else {
            continue;
        };
        let left = Vec2::new(dir.y, -dir.x) / dir.hypot();
        let r = contour.bounding_box();
        let eps = (r.width().min(r.height()) * 1e-3).clamp(1e-6, 0.01);
        let left_inside = bp.winding(at + left * eps) != 0;
        let mut moved = offset_path(&contour, if left_inside { -d } else { d }, tol);
        // Closed, so the dots run round it with no seam or end caps.
        if contour.elements().iter().any(|e| matches!(e, kurbo::PathEl::ClosePath)) {
            moved.close_path();
        }
        for el in moved.elements() {
            out.push(*el);
        }
    }
    out
}

/// The contours (sub-paths) of `bp`, each starting at its MoveTo.
fn contours(bp: &BezPath) -> Vec<BezPath> {
    let mut out: Vec<BezPath> = Vec::new();
    for el in bp.elements() {
        if matches!(el, kurbo::PathEl::MoveTo(_)) || out.is_empty() {
            out.push(BezPath::new());
        }
        if let Some(c) = out.last_mut() {
            c.push(*el);
        }
    }
    out
}

/// The derivative of a segment at `t`.
trait DerivAt {
    fn deriv_at(&self, t: f64) -> Vec2;
}

impl DerivAt for PathSeg {
    fn deriv_at(&self, t: f64) -> Vec2 {
        match self {
            PathSeg::Line(l) => l.p1 - l.p0,
            PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
            PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
        }
    }
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

/// Points along `bp` every `step` of arc length, with the unit tangent there.
fn samples(bp: &BezPath, step: f64) -> Vec<(Point, Vec2)> {
    let mut out = Vec::new();
    let mut carry = 0.0;
    for seg in bp.segments() {
        let len = seg.arclen(1e-3);
        if len < 1e-9 {
            continue;
        }
        let mut s = carry;
        while s <= len {
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

/// A slanted hash: ticks across the path at 45°, leaning back (`lean` −1, Left Slant Hash) or
/// forward (+1, Right Slant Hash), within the weight.
pub fn slant_hash(bp: &BezPath, weight: f64, lean: f64, tol: f64) -> BezPath {
    let mut ticks = BezPath::new();
    let h = weight / 2.0;
    for (p, t) in samples(bp, (weight * 0.6).max(1.0)) {
        let normal = Vec2::new(-t.y, t.x) * h;
        // The tick's top (left of travel) leans back or forward along the path.
        let along = t * (h * lean);
        ticks.move_to(p - normal + along);
        ticks.line_to(p + normal - along);
    }
    let s = Stroke::new((weight * 0.14).max(0.25)).with_caps(kurbo::Cap::Butt);
    kurbo::stroke(ticks.iter(), &s, &kurbo::StrokeOpts::default(), tol)
}

/// White Diamond: hollow diamonds, as tall as the weight and a third longer, tip to tip.
pub fn diamonds(bp: &BezPath, weight: f64, tol: f64) -> BezPath {
    let long = (weight * 1.3).max(1.0);
    let mut out = BezPath::new();
    let h = weight / 2.0;
    for (p, t) in samples(bp, long) {
        let normal = Vec2::new(-t.y, t.x) * h;
        let along = t * (long / 2.0);
        let mut d = BezPath::new();
        d.move_to(p - along);
        d.line_to(p - normal);
        d.line_to(p + along);
        d.line_to(p + normal);
        d.close_path();
        let s = Stroke::new((weight * 0.08).max(0.25)).with_join(kurbo::Join::Miter);
        for el in kurbo::stroke(d.iter(), &s, &kurbo::StrokeOpts::default(), tol).elements() {
            out.push(*el);
        }
    }
    out
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

    #[test]
    fn wavy_and_hash_cover_the_weight() {
        let w = wavy(&line(), 9.0, 0.01).bounding_box();
        assert!(w.height() > 4.0 && w.height() < 9.5, "{w:?}");
        let h = hashed(&line(), 8.0, 0.01).bounding_box();
        assert!((h.height() - 8.0).abs() < 0.5 && h.width() > 90.0, "{h:?}");
    }

    #[test]
    fn offset_outward_moves_a_closed_path_out_or_in_either_way_it_runs() {
        use kurbo::ParamCurveNearest;
        let square = |cw: bool| {
            let mut pts = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
            if !cw {
                pts.reverse();
            }
            let mut b = BezPath::new();
            b.move_to(pts[0]);
            for p in &pts[1..] {
                b.line_to(*p);
            }
            b.close_path();
            b
        };
        // Whether the path passes within 0.5 pt of `p`.
        let passes = |b: &BezPath, p: (f64, f64)| b.segments().any(|s| s.nearest(Point::new(p.0, p.1), 1e-6).distance_sq < 0.25);
        // A closed shape whose first cubic stands still halfway (zero derivative at t = 0.5)
        // keeps its dots: another point gives the direction.
        let mut lens = BezPath::new();
        lens.move_to((0.0, 0.0));
        lens.curve_to((100.0, 0.0), (0.0, 0.0), (100.0, 0.0));
        lens.curve_to((100.0, 60.0), (0.0, 60.0), (0.0, 0.0));
        lens.close_path();
        assert!(!offset_outward(&lens, 2.0, 0.01).elements().is_empty(), "the contour is kept");

        // A square with a square hole (the hole running the other way, or the same way): each
        // contour moves away from the filled ring, the hole's into the hole, and they stay apart.
        for hole_cw in [false, true] {
            let mut ring = square(true);
            let mut hole = [(30.0, 30.0), (70.0, 30.0), (70.0, 70.0), (30.0, 70.0)];
            if !hole_cw {
                hole.reverse();
            }
            ring.move_to(hole[0]);
            for p in &hole[1..] {
                ring.line_to(*p);
            }
            ring.close_path();
            let out = offset_outward(&ring, 5.0, 0.01);
            let moves = out.elements().iter().filter(|e| matches!(e, kurbo::PathEl::MoveTo(_))).count();
            assert_eq!(moves, 2, "{hole_cw}: two contours, not joined");
            assert!(passes(&out, (50.0, -5.0)), "{hole_cw}: the outer contour outside");
            assert!(passes(&out, (50.0, 35.0)), "{hole_cw}: the hole's contour into the hole");
            let inside = offset_outward(&ring, -5.0, 0.01);
            assert!(passes(&inside, (50.0, 5.0)) && passes(&inside, (50.0, 25.0)), "{hole_cw}: both into the ring");
        }
        for cw in [true, false] {
            let closed = |b: &BezPath| matches!(b.elements().last(), Some(kurbo::PathEl::ClosePath));
            assert!(closed(&offset_outward(&square(cw), 5.0, 0.01)) && closed(&offset_outward(&square(cw), -5.0, 0.01)), "{cw}: closed");
            assert!(passes(&offset_outward(&square(cw), 5.0, 0.01), (50.0, -5.0)), "{cw}: outside, above the top edge");
            assert!(passes(&offset_outward(&square(cw), -5.0, 0.01), (50.0, 5.0)), "{cw}: inside, below the top edge");
        }
    }

    #[test]
    fn slant_hashes_lean_their_ways_and_diamonds_are_hollow() {
        // Along y = 0 going right, "left of travel" is −y: a left slant's tick runs from up-left
        // to down-right (\), a right slant's from up-right to down-left (/).
        let left = slant_hash(&line(), 8.0, -1.0, 0.01);
        let right = slant_hash(&line(), 8.0, 1.0, 0.01);
        assert!(left.contains(Point::new(48.0 - 3.0, -3.0)) || left.contains(Point::new(50.4 - 3.0, -3.0)), "\\ top left");
        let lb = left.bounding_box();
        assert!((lb.height() - 8.0).abs() < 1.0, "{lb:?}");
        assert_ne!(format!("{:?}", left.elements().first()), format!("{:?}", right.elements().first()));
        let d = diamonds(&line(), 10.0, 0.01);
        let b = d.bounding_box();
        assert!((b.height() - 10.0).abs() < 1.5 && b.width() > 90.0, "{b:?}");
        // A diamond's centre is empty; its edge is drawn.
        assert!(!d.contains(Point::new(13.0, 0.0)), "hollow");
        assert!(d.contains(Point::new(13.0 + 6.5 / 2.0, -2.5)), "the edge between the side tip and the top");
    }
}
