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

/// A part of a hidden stroke style (below): its area and its colour (`None`: the stroke's own).
pub type Part = (BezPath, Option<[u8; 3]>);

const BLACK: [u8; 3] = [0, 0, 0];

/// The Rainbow stroke style's bands, outside (left of travel) to inside.
pub const RAINBOW: [[u8; 3]; 7] = [[229, 73, 47], [241, 152, 61], [255, 230, 72], [0, 140, 88], [0, 151, 231], [47, 55, 145], [225, 70, 137]];

/// The Rasta stroke style's bands.
pub const RASTA: [[u8; 3]; 3] = [[244, 5, 0], [254, 255, 0], [8, 251, 1]];

/// The bulb colours of the Lights stroke style, in turn.
pub const LIGHTS: [[u8; 3]; 5] = [[226, 52, 44], [250, 222, 40], [45, 70, 200], [240, 140, 40], [20, 160, 80]];

/// Equal bands across the weight, each in its colour (Rainbow, Rasta). Neighbouring bands overlap
/// a hair so no seam shows between them.
pub fn colour_bands(bp: &BezPath, weight: f64, colours: &[[u8; 3]], tol: f64) -> Vec<Part> {
    let w = 1.0 / colours.len() as f64;
    colours
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let start = (i as f64 * w - 0.002).max(0.0);
            let width = (w + 0.004).min(1.0 - start);
            (stripes(bp, weight, &[(start, width)], tol), Some(*c))
        })
        .collect()
}

/// `shape` drawn in a local frame (x along the path, y across it towards the right of travel)
/// placed at `p` with the path's unit tangent `t`.
fn placed(shape: &BezPath, p: Point, t: Vec2) -> BezPath {
    kurbo::Affine::new([t.x, t.y, -t.y, t.x, p.x, p.y]) * shape.clone()
}

/// An ellipse centred at (`cx`, `cy`) with radii `rx` along and `ry` across.
fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> BezPath {
    use kurbo::Shape;
    kurbo::Ellipse::new((cx, cy), (rx.max(0.01), ry.max(0.01)), 0.0).to_path(0.01)
}

fn extend(out: &mut BezPath, b: &BezPath) {
    for el in b.elements() {
        out.push(*el);
    }
}

/// The Feet stroke style: footprints walking along the path, left and right in turn, toes
/// forward, in the stroke colour.
pub fn feet(bp: &BezPath, weight: f64) -> Vec<Part> {
    let len = weight * 0.34;
    let mut out = BezPath::new();
    for (i, (p, t)) in samples(bp, (weight * 0.5).max(1.0)).into_iter().enumerate() {
        // The left foot on the left of travel, the right foot on the right.
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let mut foot = BezPath::new();
        // Heel and ball, then five toes across the front, the big toe on the inside.
        extend(&mut foot, &ellipse(-len * 0.2, 0.0, len * 0.2, len * 0.15));
        extend(&mut foot, &ellipse(len * 0.1, 0.0, len * 0.24, len * 0.19));
        for k in 0..5 {
            let f = k as f64 / 4.0;
            let r = len * (0.085 - 0.035 * f);
            let across = (f - 0.5) * len * 0.42 * -side;
            extend(&mut foot, &ellipse(len * (0.44 - 0.07 * (2.0 * f - 1.0).abs()), across, r, r));
        }
        let at = p + Vec2::new(-t.y, t.x) * (side * weight * 0.2);
        extend(&mut out, &placed(&foot, at, t));
    }
    vec![(out, None)]
}

/// The Woof stroke style: paw prints in two staggered rows, toes to the left of travel, in the
/// stroke colour.
pub fn woof(bp: &BezPath, weight: f64) -> Vec<Part> {
    let size = weight * 0.36;
    let mut out = BezPath::new();
    for (i, (p, t)) in samples(bp, (weight * 0.42).max(1.0)).into_iter().enumerate() {
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let mut paw = BezPath::new();
        // The pad, and four toes in an arc beyond it.
        extend(&mut paw, &ellipse(0.0, size * 0.14, size * 0.3, size * 0.24));
        for (dx, dy) in [(-0.36, -0.12), (-0.13, -0.32), (0.13, -0.32), (0.36, -0.12)] {
            extend(&mut paw, &ellipse(size * dx, size * dy, size * 0.1, size * 0.12));
        }
        let at = p + Vec2::new(-t.y, t.x) * (side * weight * 0.22);
        extend(&mut out, &placed(&paw, at, t));
    }
    vec![(out, None)]
}

/// The Happy stroke style: yellow smiling faces as wide as the weight, touching, looking along
/// the path, outlined in black.
pub fn happy(bp: &BezPath, weight: f64, tol: f64) -> Vec<Part> {
    let r = weight * 0.47;
    let line = Stroke::new((weight * 0.05).max(0.25)).with_caps(kurbo::Cap::Round);
    let (mut faces, mut ink) = (BezPath::new(), BezPath::new());
    for (p, t) in samples(bp, weight.max(1.0)) {
        let disc = ellipse(0.0, 0.0, r, r);
        extend(&mut faces, &placed(&disc, p, t));
        let mut lines = disc.clone();
        // The smile: an arc on the far side of the face from the eyes.
        let mut smile = BezPath::new();
        for k in 0..=12 {
            let a = std::f64::consts::PI * (0.62 + 0.76 * k as f64 / 12.0);
            let q = Point::new(r * 0.08 + r * 0.55 * a.cos(), r * 0.55 * a.sin());
            if k == 0 { smile.move_to(q) } else { smile.line_to(q) }
        }
        extend(&mut lines, &smile);
        let stroked = kurbo::stroke(lines.iter(), &line, &kurbo::StrokeOpts::default(), tol);
        extend(&mut ink, &placed(&stroked, p, t));
        // The eyes, towards the direction of travel.
        for y in [-0.28, 0.28] {
            extend(&mut ink, &placed(&ellipse(r * 0.3, r * y, r * 0.14, r * 0.09), p, t));
        }
    }
    vec![(faces, Some([255, 232, 0])), (ink, Some(BLACK))]
}

/// The Lights stroke style: a black wire swinging along the path with a bulb hanging from each
/// swing, on alternating sides, in turn red, yellow, blue, orange and green.
pub fn lights(bp: &BezPath, weight: f64, tol: f64) -> Vec<Part> {
    let wavelength = (weight * 1.6).max(2.0);
    let amp = weight * 0.16;
    let mut wire = BezPath::new();
    let mut sockets = BezPath::new();
    let mut bulbs: Vec<Part> = Vec::new();
    for (i, (p, t)) in samples(bp, wavelength / 16.0).into_iter().enumerate() {
        let normal = Vec2::new(-t.y, t.x);
        let phase = i as f64 / 16.0 * std::f64::consts::TAU;
        let q = p + normal * (phase.sin() * amp);
        if i == 0 {
            wire.move_to(q)
        } else {
            wire.line_to(q)
        }
        // A bulb at each crest, hanging outwards from it on its socket.
        if i % 8 == 4 {
            let k = i / 8;
            let out = normal * if k % 2 == 0 { 1.0 } else { -1.0 };
            let dir = (t * 0.4 + out).normalize();
            let socket_at = q + dir * (weight * 0.05);
            extend(&mut sockets, &placed(&ellipse(0.0, 0.0, weight * 0.05, weight * 0.035), socket_at, dir));
            let bulb_at = socket_at + dir * (weight * 0.16);
            bulbs.push((placed(&ellipse(0.0, 0.0, weight * 0.13, weight * 0.07), bulb_at, dir), Some(LIGHTS[k % LIGHTS.len()])));
        }
    }
    let s = Stroke::new((weight * 0.035).max(0.25)).with_join(kurbo::Join::Round);
    let mut black = kurbo::stroke(wire.iter(), &s, &kurbo::StrokeOpts::default(), tol);
    extend(&mut black, &sockets);
    let mut parts = vec![(black, Some(BLACK))];
    parts.extend(bulbs);
    parts
}

/// The Bounce stroke style: dashes across the weight that are longest at the path's anchor points
/// and shrink to slivers halfway between them, in the stroke colour.
pub fn bounce(bp: &BezPath, weight: f64) -> Vec<Part> {
    let h = weight / 2.0;
    let gap = (weight * 0.22).max(0.5);
    let mut out = BezPath::new();
    let at = |seg: &PathSeg, s: f64| {
        let t = seg.inv_arclen(s, 1e-3);
        let d = match seg {
            PathSeg::Line(l) => l.p1 - l.p0,
            PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
            PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
        };
        let n = d.hypot();
        let tan = if n > 1e-12 { d / n } else { Vec2::new(1.0, 0.0) };
        (seg.eval(t), Vec2::new(-tan.y, tan.x) * h)
    };
    for seg in bp.segments() {
        let len = seg.arclen(1e-3);
        if len < 1e-6 {
            continue;
        }
        let mut s = 0.0;
        let mut n = 0;
        while s < len && n < 100_000 {
            let f = s / len;
            let dash = weight * (0.15 + 0.95 * (1.0 - (std::f64::consts::PI * f).sin())).max(0.15);
            let e = (s + dash).min(len);
            let ((p0, n0), (p1, n1)) = (at(&seg, s), at(&seg, e));
            out.move_to(p0 - n0);
            out.line_to(p1 - n1);
            out.line_to(p1 + n1);
            out.line_to(p0 + n0);
            out.close_path();
            s = e + gap;
            n += 1;
        }
    }
    vec![(out, None)]
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
