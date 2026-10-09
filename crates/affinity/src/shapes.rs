//! Parametric shapes: Affinity stores the shape's parameters and its box, never the outline.
//! Formulas were fitted to Affinity's own renders (the document thumbnails) of public files.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::model::{Affine, Path, Point, Reader, SubPath};
use crate::stream::{ObjId, Tag, Value};

/// Handle length of a quarter circle of radius 1.
const KAPPA: f64 = 0.552_284_749_830_793_4;
/// Vertices of a polygon or star.
const MAX_SIDES: i64 = 10_000;

/// The last value of a tag: a shape changed from one kind to another keeps a stale first block.
fn last<'a>(r: &'a Reader, id: ObjId, tag: &[u8; 4]) -> Option<&'a Value> {
    let t = Tag::of(tag);
    r.s.object(id)?.fields.iter().rev().find(|(k, _)| *k == t).map(|(_, v)| v)
}

fn num(r: &Reader, id: ObjId, tag: &[u8; 4]) -> Option<f64> {
    match last(r, id, tag)? {
        Value::Float(f) if f.is_finite() => Some(*f),
        Value::UInt(u) => Some(*u as f64),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn sides(r: &Reader, id: ObjId, tag: &[u8; 4], default: f64) -> usize {
    let n = num(r, id, tag).unwrap_or(default).round();
    n.clamp(3.0, MAX_SIDES as f64) as usize
}

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn polygon(pts: impl IntoIterator<Item = Point>) -> Option<SubPath> {
    let mut it = pts.into_iter();
    let start = it.next()?;
    let segments = it.map(|p| [p, p, p]).collect();
    Some(SubPath { start, segments, closed: true })
}

/// Outline of `shape` (a `Shpe` object) in the node box `b` = (x0, y0, x1, y1).
pub(crate) fn shape(r: &mut Reader, shape: ObjId, b: [f64; 4], world: Affine) -> Option<Path> {
    let [x0, y0, x1, y1] = b;
    let (w, h) = (x1 - x0, y1 - y0);
    let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, w / 2.0, h / 2.0);
    let on_ellipse = |a: f64, f: f64| pt(cx + f * rx * a.cos(), cy + f * ry * a.sin());
    let class = r.s.class(shape)?;
    let sub = match &class.0.to_be_bytes() {
        b"ShNR" | b"ShRR" => {
            let rounded = matches!(last(r, shape, b"CTyp"), Some(Value::Array(a)) if !a.is_empty());
            let radius = match last(r, shape, b"ShCR") {
                Some(Value::Floats(v)) => v.first().copied().filter(|f| f.is_finite() && *f > 0.0),
                _ => None,
            };
            if let (true, Some(f)) = (rounded, radius) {
                if let Some(Value::Array(types)) = last(r, shape, b"CTyp")
                    && types.iter().any(|t| !matches!(t, Value::Enum { id: 0 | 4, .. }))
                {
                    r.warn("rectangles with straight, concave or other corner types (imported with round corners)");
                }
                let (crx, cry) = if r.s.bool(shape, b"AbSz") == Some(true) {
                    // An absolute radius is in document pixels, whatever the node's scale.
                    let [a, b2, c, d, ..] = world.0;
                    let (sx, sy) = ((a * a + b2 * b2).sqrt(), (c * c + d * d).sqrt());
                    if sx > 0.0 && sy > 0.0 { (f / sx, f / sy) } else { (0.0, 0.0) }
                } else {
                    (f * w.min(h), f * w.min(h))
                };
                rounded_rect(b, crx.min(w / 2.0), cry.min(h / 2.0))
            } else {
                polygon([pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)])?
            }
        }
        b"ShpE" => ellipse(cx, cy, rx, ry),
        b"ShPy" => {
            let n = sides(r, shape, b"Side", 5.0);
            let a = |i: usize| -FRAC_PI_2 + TAU * i as f64 / n as f64;
            if r.s.object(shape).is_some_and(|_| matches!(last(r, shape, b"Smth"), Some(Value::Bool(true)))) {
                // Smoothed: tangent handles of length curv·4/3·tan(π/2n) along the inscribed ellipse.
                let k = num(r, shape, b"Curv").unwrap_or(0.0) * 4.0 / 3.0 * (PI / (2.0 * n as f64)).tan();
                let v = |i: usize| on_ellipse(a(i), 1.0);
                let t = |i: usize| pt(-rx * a(i).sin(), ry * a(i).cos());
                let segments = (0..n)
                    .map(|i| {
                        let (p, q, tp, tq) = (v(i), v(i + 1), t(i), t(i + 1));
                        [pt(p.x + k * tp.x, p.y + k * tp.y), pt(q.x - k * tq.x, q.y - k * tq.y), q]
                    })
                    .collect();
                SubPath { start: v(0), segments, closed: true }
            } else {
                polygon((0..n).map(|i| on_ellipse(a(i), 1.0)))?
            }
        }
        b"ShSt" => {
            let n = sides(r, shape, b"Pnts", 5.0);
            let q = num(r, shape, b"IRad").unwrap_or(0.382).clamp(0.0, 1.0);
            if [b"CrcI", b"CrcO", b"CrvL", b"CrvR"].iter().any(|t| num(r, shape, t).is_some_and(|v| v.abs() > 1e-9)) {
                r.warn("stars with rounded or curved points (imported with sharp points)");
            }
            polygon((0..2 * n).map(|i| on_ellipse(-FRAC_PI_2 + PI * i as f64 / n as f64, if i % 2 == 0 { 1.0 } else { q })))?
        }
        b"ShSS" => {
            let n = sides(r, shape, b"Side", 5.0);
            let ri = num(r, shape, b"COut").unwrap_or(0.5).clamp(0.0, 1.0);
            let step = PI / n as f64;
            let (l, hw, inner) = (step.cos(), ri * step.sin(), ri * step.cos());
            polygon((0..n).flat_map(|k| {
                let phi = -FRAC_PI_2 + step + TAU * k as f64 / n as f64;
                let (c, s) = (phi.cos(), phi.sin());
                [(inner, -hw), (l, -hw), (l, hw), (inner, hw)].map(|(u, v)| pt(cx + rx * (u * c - v * s), cy + ry * (u * s + v * c)))
            }))?
        }
        b"ShPi" => {
            let raw_start = num(r, shape, b"AngS").unwrap_or(0.0);
            let raw_end = num(r, shape, b"AngE").unwrap_or(TAU);
            // Normalize in constant time. Repeatedly adding TAU can hang on finite values
            // large enough that floating-point addition no longer changes the endpoint.
            let start = raw_start.rem_euclid(TAU);
            let delta = raw_end - raw_start;
            let sweep = if raw_end >= raw_start {
                delta.min(TAU)
            } else if delta.is_finite() {
                delta.rem_euclid(TAU)
            } else {
                (raw_end.rem_euclid(TAU) - start).rem_euclid(TAU)
            };
            let inner = num(r, shape, b"IRad").unwrap_or(0.0).clamp(0.0, 1.0);
            pie(cx, cy, rx, ry, start, start + sweep, inner)
        }
        b"ShpT" => {
            let p = num(r, shape, b"Pos ").unwrap_or(0.5);
            polygon([pt(x0 + p * w, y0), pt(x1, y1), pt(x0, y1)])?
        }
        b"ShTz" => {
            let (l, rr) = (num(r, shape, b"PosL").unwrap_or(0.25), num(r, shape, b"PosR").unwrap_or(0.75));
            polygon([pt(x0 + l * w, y0), pt(x0 + rr * w, y0), pt(x1, y1), pt(x0, y1)])?
        }
        _ => {
            r.warn("cloud, heart, cog, callout, arrow, tear and other special shapes (imported as their bounding ellipse)");
            ellipse(cx, cy, rx, ry)
        }
    };
    Some(Path { subpaths: vec![sub] })
}

pub(crate) fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> SubPath {
    SubPath { start: pt(x0, y0), segments: [pt(x1, y0), pt(x1, y1), pt(x0, y1)].map(|p| [p, p, p]).to_vec(), closed: true }
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> SubPath {
    let (kx, ky) = (KAPPA * rx, KAPPA * ry);
    SubPath {
        start: pt(cx + rx, cy),
        segments: vec![
            [pt(cx + rx, cy + ky), pt(cx + kx, cy + ry), pt(cx, cy + ry)],
            [pt(cx - kx, cy + ry), pt(cx - rx, cy + ky), pt(cx - rx, cy)],
            [pt(cx - rx, cy - ky), pt(cx - kx, cy - ry), pt(cx, cy - ry)],
            [pt(cx + kx, cy - ry), pt(cx + rx, cy - ky), pt(cx + rx, cy)],
        ],
        closed: true,
    }
}

fn rounded_rect([x0, y0, x1, y1]: [f64; 4], rx: f64, ry: f64) -> SubPath {
    let (kx, ky) = ((1.0 - KAPPA) * rx, (1.0 - KAPPA) * ry);
    SubPath {
        start: pt(x0 + rx, y0),
        segments: vec![
            [pt(x1 - rx, y0), pt(x1 - rx, y0), pt(x1 - rx, y0)],
            [pt(x1 - kx, y0), pt(x1, y0 + ky), pt(x1, y0 + ry)],
            [pt(x1, y1 - ry), pt(x1, y1 - ry), pt(x1, y1 - ry)],
            [pt(x1, y1 - ky), pt(x1 - kx, y1), pt(x1 - rx, y1)],
            [pt(x0 + rx, y1), pt(x0 + rx, y1), pt(x0 + rx, y1)],
            [pt(x0 + kx, y1), pt(x0, y1 - ky), pt(x0, y1 - ry)],
            [pt(x0, y0 + ry), pt(x0, y0 + ry), pt(x0, y0 + ry)],
            [pt(x0, y0 + ky), pt(x0 + kx, y0), pt(x0 + rx, y0)],
        ],
        closed: true,
    }
}

/// Elliptical arc from angle `a0` to `a1` (radians, measured y-up as Affinity's pie angles are),
/// as cubic segments of at most a quarter turn.
fn arc(cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64, out: &mut Vec<[Point; 3]>) {
    let n = ((a1 - a0).abs() / FRAC_PI_2).ceil().clamp(1.0, 8.0) as usize;
    let step = (a1 - a0) / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let p = |a: f64| pt(cx + rx * a.cos(), cy - ry * a.sin());
    let d = |a: f64| pt(-rx * a.sin(), -ry * a.cos());
    for i in 0..n {
        let (s, e) = (a0 + step * i as f64, a0 + step * (i + 1) as f64);
        let (ps, pe, ds, de) = (p(s), p(e), d(s), d(e));
        out.push([pt(ps.x + k * ds.x, ps.y + k * ds.y), pt(pe.x - k * de.x, pe.y - k * de.y), pe]);
    }
}

fn pie(cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64, inner: f64) -> SubPath {
    let start = pt(cx + rx * a0.cos(), cy - ry * a0.sin());
    let mut segments = Vec::new();
    arc(cx, cy, rx, ry, a0, a1, &mut segments);
    if inner > 0.0 {
        let p = pt(cx + inner * rx * a1.cos(), cy - inner * ry * a1.sin());
        segments.push([p, p, p]);
        arc(cx, cy, inner * rx, inner * ry, a1, a0, &mut segments);
    } else {
        let c = pt(cx, cy);
        segments.push([c, c, c]);
    }
    SubPath { start, segments, closed: true }
}

/// A polyline with circular corners of the given radii (Affinity's live corners on curves).
pub(crate) fn fillet(pts: &[(Point, f64)], closed: bool) -> SubPath {
    let n = pts.len();
    let get = |i: isize| -> Option<Point> {
        if closed { pts.get(i.rem_euclid(n as isize) as usize).map(|p| p.0) } else { usize::try_from(i).ok().and_then(|i| pts.get(i)).map(|p| p.0) }
    };
    let mut corners: Vec<(Point, Point, Point, Point)> = Vec::with_capacity(n);
    for (i, (p, radius)) in pts.iter().enumerate() {
        let (prev, next) = (get(i as isize - 1), get(i as isize + 1));
        let (Some(a), Some(b)) = (prev, next) else {
            corners.push((*p, *p, *p, *p));
            continue;
        };
        let (u1, l1) = unit(*p, a);
        let (u2, l2) = unit(*p, b);
        let cos = (u1.x * u2.x + u1.y * u2.y).clamp(-1.0, 1.0);
        let phi = cos.acos();
        if *radius <= 0.0 || phi < 1e-6 || (PI - phi) < 1e-6 {
            corners.push((*p, *p, *p, *p));
            continue;
        }
        let t = (radius / (phi / 2.0).tan()).min(l1 / 2.0).min(l2 / 2.0);
        let r = t * (phi / 2.0).tan();
        let k = 4.0 / 3.0 * ((PI - phi) / 4.0).tan() * r;
        let s = pt(p.x + u1.x * t, p.y + u1.y * t);
        let e = pt(p.x + u2.x * t, p.y + u2.y * t);
        corners.push((s, pt(s.x - u1.x * k, s.y - u1.y * k), pt(e.x - u2.x * k, e.y - u2.y * k), e));
    }
    let Some(first) = corners.first().copied() else { return SubPath { start: pt(0.0, 0.0), segments: vec![], closed } };
    let mut segments = Vec::with_capacity(2 * n);
    let mut cur = first.0;
    let start = cur;
    for (i, (s, c1, c2, e)) in corners.iter().enumerate() {
        if i > 0 {
            segments.push([cur, *s, *s]);
        }
        if s != e {
            segments.push([*c1, *c2, *e]);
        }
        cur = *e;
    }
    if closed && cur != start {
        segments.push([cur, start, start]);
    }
    SubPath { start, segments, closed }
}

fn unit(from: Point, to: Point) -> (Point, f64) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let l = (dx * dx + dy * dy).sqrt();
    if l > 0.0 { (pt(dx / l, dy / l), l) } else { (pt(0.0, 0.0), 0.0) }
}
