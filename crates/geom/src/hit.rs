//! Hit testing primitives.

use std::collections::HashSet;

use kurbo::{BezPath, ParamCurve, ParamCurveNearest, Point, Rect, Shape};

use crate::path::{FillRule, PathData};

/// Is `p` inside the filled area of `path` under `rule`? Open subpaths are implicitly closed (as when filled).
pub fn fill_contains(path: &BezPath, rule: FillRule, p: Point) -> bool {
    let w = path.winding(p);
    match rule {
        FillRule::NonZero => w != 0,
        FillRule::EvenOdd => w % 2 != 0,
    }
}

/// Distance from `p` to the nearest point on the path outline.
pub fn distance_to_outline(path: &BezPath, p: Point) -> f64 {
    path.segments().map(|s| s.nearest(p, 1e-6).distance_sq).fold(f64::INFINITY, f64::min).sqrt()
}

/// Is `p` within `tol` of the path's stroke of width `width`?
pub fn stroke_contains(path: &BezPath, width: f64, tol: f64, p: Point) -> bool {
    let quick = path.bounding_box().inflate(width / 2.0 + tol, width / 2.0 + tol);
    quick.contains(p) && distance_to_outline(path, p) <= width / 2.0 + tol
}

/// One flattened edge of a contour, for [`edges_cross`].
struct Edge {
    contour: usize,
    /// Its index in the contour, and the contour's edge count (neighbors wrap around).
    index: usize,
    count: usize,
    line: kurbo::Line,
    /// How far the curve it stands for may be from it (0 for a straight segment).
    slack: f64,
    /// The outline segment it comes from, and the contour's segment count.
    seg: usize,
    segs: usize,
    /// That segment, when it is a curve.
    curve: Option<kurbo::CubicBez>,
}

impl Edge {
    fn lo(&self) -> f64 {
        self.line.p0.x.min(self.line.p1.x) - self.slack
    }
    fn hi(&self) -> f64 {
        self.line.p0.x.max(self.line.p1.x) + self.slack
    }
}

/// Steps one curve is flattened into at most: a larger curve is flattened more coarsely.
const STEPS_PER_CURVE: f64 = 10_000.0;

/// Curves larger than this (width plus height) are not flattened.
const MAX_CURVE_SIZE: f64 = 1e100;

/// Could two edges of `path` cross or touch, other than neighbors in one contour at the point they
/// share? Each subpath counts as filled, closed by a straight edge when it is open; one that
/// encloses no area (its straight edges lie along one line) is left out. A straight edge and a
/// curve are intersected exactly. Two curves are compared flattened to `tolerance` (more coarsely
/// for a curve larger than `STEPS_PER_CURVE` times it), and a flattened edge that comes within that
/// distance of another counts as touching it, so `Some(false)` means the contours are apart. Each
/// edge made takes one from `edges_left` and each comparison one from `tests_left`; `None` when
/// either runs out.
pub fn edges_cross(path: &PathData, tolerance: f64, edges_left: &mut usize, tests_left: &mut usize) -> Option<bool> {
    let mut edges: Vec<Edge> = Vec::new();
    for (contour, sp) in path.subpaths.iter().enumerate() {
        // Each segment that is not a point makes at least one edge: give up before building the
        // outline when its anchors alone would use up what is left.
        if sp.anchors.len() > *edges_left {
            return None;
        }
        let first = edges.len();
        let mut curved = false;
        let outline = sp.to_filled_bezpath();
        // Each segment's edges, leaving out segments that are a single point.
        let mut pieces: Vec<(f64, Vec<kurbo::Line>, Option<kurbo::CubicBez>)> = Vec::new();
        for seg in outline.segments() {
            let (slack, steps, cubic) = match seg {
                kurbo::PathSeg::Line(l) => (0.0, vec![l], None),
                // A curve whose control points lie on one line runs along it: for the fill it is
                // the straight edge between its ends (where it overshoots, it comes back the same way).
                curve if is_flat(curve) => (0.0, vec![kurbo::Line::new(curve.start(), curve.end())], None),
                curve => {
                    curved = true;
                    let b = curve.bounding_box();
                    // Past this size the flattening's error estimate overflows.
                    let size = b.width() + b.height();
                    if !size.is_finite() || size > MAX_CURVE_SIZE {
                        return None;
                    }
                    let tol = tolerance.max(size / STEPS_PER_CURVE);
                    if !tol.is_finite() {
                        return None;
                    }
                    let cubic = match curve {
                        kurbo::PathSeg::Quad(q) => q.raise(),
                        kurbo::PathSeg::Cubic(c) => c,
                        kurbo::PathSeg::Line(l) => kurbo::CubicBez::new(l.p0, l.p0, l.p1, l.p1),
                    };
                    // Its steps may be too few to show a loop, so a curve that may loop is not checked.
                    if may_loop(cubic) {
                        return None;
                    }
                    let mut steps = Vec::new();
                    let mut prev = curve.start();
                    kurbo::flatten([kurbo::PathEl::MoveTo(prev), curve.as_path_el()], tol, |el| {
                        if let kurbo::PathEl::LineTo(p) = el {
                            steps.push(kurbo::Line::new(prev, p));
                            prev = p;
                        }
                    });
                    // The steps made are charged even when the curve is then refused.
                    *edges_left = edges_left.saturating_sub(steps.len());
                    // A small loop back to its start flattens to a point: the edges cannot check it.
                    if steps.iter().all(|l| l.p0 == l.p1) && (b.width() > 0.0 || b.height() > 0.0) {
                        return None;
                    }
                    if *edges_left == 0 {
                        return None;
                    }
                    (tol, steps, Some(cubic))
                }
            };
            if cubic.is_none() {
                *edges_left = edges_left.checked_sub(steps.len())?;
            }
            let steps: Vec<kurbo::Line> = steps.into_iter().filter(|l| l.p0 != l.p1).collect();
            if !steps.is_empty() {
                pieces.push((slack, steps, cubic));
            }
        }
        let segs = pieces.len();
        for (seg, (slack, steps, curve)) in pieces.into_iter().enumerate() {
            for line in steps {
                edges.push(Edge { contour, index: edges.len() - first, count: 0, line, slack, seg, segs, curve });
            }
        }
        let count = edges.len() - first;
        // Fewer than three straight edges enclose no area. A curve flattened that coarsely may
        // still enclose a sliver, which the edges cannot check.
        if count < 3 && curved {
            return None;
        }
        // Straight edges along one line (an open line with more than two anchors) enclose no area
        // either.
        let collinear = !curved
            && edges.get(first).is_some_and(|e0| {
                let d = e0.line.p1 - e0.line.p0;
                edges[first..].iter().all(|e| (e.line.p1 - e0.line.p0).cross(d) == 0.0)
            });
        if count < 3 || collinear {
            edges.truncate(first);
            continue;
        }
        for e in &mut edges[first..] {
            e.count = count;
        }
    }
    edges.sort_by(|a, b| a.lo().total_cmp(&b.lo()));
    // Pairs of curves already found apart when subdivided.
    let mut apart: HashSet<[usize; 4]> = HashSet::new();
    for (k, a) in edges.iter().enumerate() {
        for b in edges.iter().skip(k + 1).take_while(|b| b.lo() <= a.hi()) {
            *tests_left = tests_left.checked_sub(1)?;
            if edges_meet(a, b, &mut apart, tests_left)? {
                return Some(true);
            }
        }
    }
    Some(false)
}

/// Do two edges of [`edges_cross`] cross or touch? `apart` holds the pairs of curves already
/// subdivided and found apart. `None` when `tests_left` runs out.
fn edges_meet(a: &Edge, b: &Edge, apart: &mut HashSet<[usize; 4]>, tests_left: &mut usize) -> Option<bool> {
    let slack = a.slack + b.slack;
    let same_or_next = a.contour == b.contour && (a.seg == b.seg || (a.seg + 1) % a.segs == b.seg || (b.seg + 1) % b.segs == a.seg);
    // Steps of neighboring curves where one heads back the way the other came: they can cross
    // right after the joint, closer than their steps show, so the curves themselves are
    // subdivided without the thousandth next to each joint.
    if let (Some(ca), Some(cb)) = (a.curve, b.curve)
        && same_or_next
        && a.seg != b.seg
        && let Some((pa, pb)) = turned_back(a, ca, b, cb)
    {
        let key = pair_key(a, b);
        if apart.contains(&key) {
            return Some(false);
        }
        if curves_meet(pa, pb, tests_left)? {
            return Some(true);
        }
        apart.insert(key);
        return Some(false);
    }
    Some(match (a.curve, b.curve) {
        // A straight edge and a curve's step: the exact curve against the edge.
        (None, Some(c)) => line_meets_curve(a, c, b),
        (Some(c), None) => line_meets_curve(b, c, a),
        // Steps of two curves that are not neighbors: when the steps come close, the curves
        // themselves are subdivided until they are seen apart or meeting.
        (Some(ca), Some(cb)) if !same_or_next => {
            let key = pair_key(a, b);
            if apart.contains(&key) || !lines_near(a.line, b.line, slack) {
                false
            } else if curves_meet(ca, cb, tests_left)? {
                true
            } else {
                apart.insert(key);
                false
            }
        }
        _ if a.contour == b.contour && (a.index + 1) % a.count == b.index => folds_back(a.line, b.line, slack),
        _ if a.contour == b.contour && (b.index + 1) % b.count == a.index => folds_back(b.line, a.line, slack),
        _ => lines_near(a.line, b.line, slack),
    })
}

/// The segments two edges come from, in a fixed order, for the `apart` cache.
fn pair_key(a: &Edge, b: &Edge) -> [usize; 4] {
    if (a.contour, a.seg) < (b.contour, b.seg) { [a.contour, a.seg, b.contour, b.seg] } else { [b.contour, b.seg, a.contour, a.seg] }
}

/// Neighboring curves `ca` (of edge `a`) and `cb` (of edge `b`), in either order: when the curve
/// after a joint they share heads back the way the one before it came, the two curves without the
/// thousandth next to each joint they share. `None` when neither joint turns back.
fn turned_back(a: &Edge, ca: kurbo::CubicBez, b: &Edge, cb: kurbo::CubicBez) -> Option<(kurbo::CubicBez, kurbo::CubicBez)> {
    let turns = |before: kurbo::CubicBez, after: kurbo::CubicBez| {
        let into = [before.p2, before.p1, before.p0].into_iter().find(|&p| p != before.p3).map(|p| before.p3 - p);
        let out = [after.p1, after.p2, after.p3].into_iter().find(|&p| p != after.p0).map(|p| p - after.p0);
        matches!((into, out), (Some(i), Some(o)) if i.dot(o) < 0.0)
    };
    let a_then_b = (a.seg + 1) % a.segs == b.seg;
    let b_then_a = (b.seg + 1) % b.segs == a.seg;
    if !(a_then_b && turns(ca, cb)) && !(b_then_a && turns(cb, ca)) {
        return None;
    }
    let (mut ra, mut rb) = (0.0..1.0, 0.0..1.0);
    if a_then_b {
        ra.end = 0.999;
        rb.start = 0.001;
    }
    if b_then_a {
        rb.end = 0.999;
        ra.start = 0.001;
    }
    Some((ca.subsegment(ra), cb.subsegment(rb)))
}

/// Could the cubic cross itself? Its last point is placed in the frame that takes the first three
/// to (0, 0), (0, 1) and (1, 1) (Stone and DeRose's canonical form); the curve has a loop only for
/// a last point in the region bounded by the cusp parabola and the two loop curves. The test is
/// widened by a millionth. When the first three points are on one line, the curve is read
/// backward; when the last three are too, it is straight or bends one way only.
fn may_loop(c: kurbo::CubicBez) -> bool {
    // With two control points the same, the other three make a triangle: the curve bends one way.
    if c.p0 == c.p1 || c.p1 == c.p2 || c.p2 == c.p3 {
        return false;
    }
    let frame = |p0: Point, p1: Point, p2: Point| {
        let (a, b) = (p2 - p1, p1 - p0);
        let det = a.cross(b);
        (det != 0.0 && det.is_finite()).then_some((a, b, det))
    };
    let ((a, b, det), r) = match (frame(c.p0, c.p1, c.p2), frame(c.p3, c.p2, c.p1)) {
        (Some(f), _) => (f, c.p3 - c.p0),
        (None, Some(f)) => (f, c.p0 - c.p3),
        (None, None) => return false,
    };
    let x = r.cross(b) / det;
    let y = a.cross(r) / det;
    let margin = 1e-6;
    if x > 1.0 + margin || y > (-x * x + 2.0 * x + 3.0) / 4.0 + margin {
        return false;
    }
    let low = if x <= 0.0 { (-x * x + 3.0 * x) / 3.0 } else { ((3.0 * (4.0 * x - x * x)).max(0.0).sqrt() - x) / 2.0 };
    y >= low - margin
}

/// Are a curve's control points on the line through its ends (or all one point)?
fn is_flat(seg: kurbo::PathSeg) -> bool {
    let pts: Vec<Point> = match seg {
        kurbo::PathSeg::Line(l) => vec![l.p0, l.p1],
        kurbo::PathSeg::Quad(q) => vec![q.p0, q.p1, q.p2],
        kurbo::PathSeg::Cubic(c) => vec![c.p0, c.p1, c.p2, c.p3],
    };
    let p0 = pts[0];
    match pts.iter().find(|&&p| p != p0) {
        Some(&q) => pts.iter().all(|&p| (p - p0).cross(q - p0) == 0.0),
        None => true,
    }
}

/// Does the curve `c` (the segment `curve` was flattened from) cross or touch the straight edge
/// `line`? When the two are neighbors in one contour, the point they share does not count.
fn line_meets_curve(line: &Edge, c: kurbo::CubicBez, curve: &Edge) -> bool {
    let l = line.line;
    let dir = l.p1 - l.p0;
    let len2 = dir.hypot2();
    if len2 == 0.0 {
        return false;
    }
    // The curve's distance to the line (times the line's length), in Bernstein form.
    let [d0, d1, d2, d3] = [c.p0, c.p1, c.p2, c.p3].map(|p| dir.cross(p - l.p0));
    // Neighbors share an end, where the distance is exactly 0: divide that root out, so a tangent
    // joint is not taken for a second meeting, and leave out t at that end.
    let adjacent = |a: &Edge, b: &Edge| a.contour == b.contour && (a.seg + 1) % a.segs == b.seg;
    let at_start = adjacent(line, curve) && d0 == 0.0;
    let at_end = adjacent(curve, line) && d3 == 0.0;
    let e: Vec<f64> = match (at_start, at_end) {
        (false, false) => vec![d0, d1, d2, d3],
        (true, false) => vec![3.0 * d1, 1.5 * d2, d3],
        (false, true) => vec![d0, 1.5 * d1, 3.0 * d2],
        (true, true) => vec![3.0 * d1, 3.0 * d2],
    };
    bernstein_roots(&e).into_iter().filter(|&t| !(at_start && t == 0.0) && !(at_end && t == 1.0)).any(|t| {
        let s = (c.eval(t) - l.p0).dot(dir) / len2;
        (-1e-9..=1.0 + 1e-9).contains(&s)
    })
}

/// Do two curves cross or touch? Pieces whose control points' boxes overlap are halved until
/// they are apart or smaller than a billionth of the curves' size. Each pair of pieces looked at
/// takes one from `tests_left`; `None` when it runs out, or when a piece has been halved
/// `MAX_HALVINGS` times (far from the origin, rounding can stop the pieces from shrinking).
fn curves_meet(a: kurbo::CubicBez, b: kurbo::CubicBez, tests_left: &mut usize) -> Option<bool> {
    const MAX_HALVINGS: u32 = 120;
    let hull = |c: &kurbo::CubicBez| {
        let r = Rect::from_points(c.p0, c.p1);
        r.union_pt(c.p2).union_pt(c.p3)
    };
    let size = |r: Rect| r.width() + r.height();
    let tiny = 1e-9 * (size(hull(&a)) + size(hull(&b)));
    let mut stack = vec![(a, b, 0u32)];
    while let Some((a, b, halvings)) = stack.pop() {
        *tests_left = tests_left.checked_sub(1)?;
        let (ha, hb) = (hull(&a), hull(&b));
        if ha.x0 > hb.x1 || hb.x0 > ha.x1 || ha.y0 > hb.y1 || hb.y0 > ha.y1 {
            continue;
        }
        if size(ha).max(size(hb)) <= tiny {
            return Some(true);
        }
        if halvings >= MAX_HALVINGS {
            return None;
        }
        if size(ha) >= size(hb) {
            let (a0, a1) = a.subdivide();
            stack.push((a0, b, halvings + 1));
            stack.push((a1, b, halvings + 1));
        } else {
            let (b0, b1) = b.subdivide();
            stack.push((a, b0, halvings + 1));
            stack.push((a, b1, halvings + 1));
        }
    }
    Some(false)
}

/// Where in [0, 1] a polynomial given by its Bernstein coefficients (degree 3 at most) is zero,
/// or so close to zero that rounding cannot tell: sign changes are found by bisection on each
/// interval where it runs one way, and touching zeros at the turns between them.
fn bernstein_roots(e: &[f64]) -> Vec<f64> {
    // Power basis a0 + a1 t + a2 t^2 + a3 t^3.
    let a: [f64; 4] = match *e {
        [e0, e1] => [e0, e1 - e0, 0.0, 0.0],
        [e0, e1, e2] => [e0, 2.0 * (e1 - e0), e0 - 2.0 * e1 + e2, 0.0],
        [e0, e1, e2, e3] => [e0, 3.0 * (e1 - e0), 3.0 * (e0 - 2.0 * e1 + e2), e3 - e0 + 3.0 * (e1 - e2)],
        _ => return Vec::new(),
    };
    let f = |t: f64| ((a[3] * t + a[2]) * t + a[1]) * t + a[0];
    let zero = 1e-12 * e.iter().map(|v| v.abs()).sum::<f64>();
    let mut bounds: Vec<f64> = kurbo::common::solve_quadratic(a[1], 2.0 * a[2], 3.0 * a[3]).into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect();
    bounds.push(0.0);
    bounds.push(1.0);
    bounds.sort_by(f64::total_cmp);
    let mut roots = Vec::new();
    for &t in &bounds {
        if f(t).abs() <= zero {
            roots.push(t);
        }
    }
    for w in bounds.windows(2) {
        let (mut lo, mut hi) = (w[0], w[1]);
        let (flo, fhi) = (f(lo), f(hi));
        if flo == 0.0 || fhi == 0.0 || (flo < 0.0) == (fhi < 0.0) {
            continue;
        }
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if (f(mid) < 0.0) == (flo < 0.0) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let t = 0.5 * (lo + hi);
        // Next to an end already taken as zero, a sign change can be rounding: that end stands for it.
        let by_zero_end = |end: f64, value: f64| value.abs() <= zero && (t - end).abs() <= 1e-9;
        if !by_zero_end(w[0], flo) && !by_zero_end(w[1], fhi) {
            roots.push(t);
        }
    }
    roots
}

/// Neighboring edges share the point where `first` ends and `second` starts. Does either come back
/// within `slack` of the other (with 0, onto it) somewhere else? An end within `slack` of the
/// shared point is near the other edge whatever the edges do, so it does not count.
fn folds_back(first: kurbo::Line, second: kurbo::Line, slack: f64) -> bool {
    let joint = first.p1;
    let near = |l: kurbo::Line, p: Point| {
        if slack > 0.0 {
            p.distance(joint) > slack && l.nearest(p, 1e-9).distance_sq.sqrt() <= slack
        } else {
            (l.p1 - l.p0).cross(p - l.p0) == 0.0
                && p.x >= l.p0.x.min(l.p1.x)
                && p.x <= l.p0.x.max(l.p1.x)
                && p.y >= l.p0.y.min(l.p1.y)
                && p.y <= l.p0.y.max(l.p1.y)
        }
    };
    near(first, second.p1) || near(second, first.p0)
}

/// Do the two segments come within `slack` of each other (with 0, share a point)?
fn lines_near(a: kurbo::Line, b: kurbo::Line, slack: f64) -> bool {
    if lines_meet(a, b) {
        return true;
    }
    if slack <= 0.0 {
        return false;
    }
    let to = |l: kurbo::Line, p: Point| l.nearest(p, 1e-9).distance_sq.sqrt();
    to(a, b.p0).min(to(a, b.p1)).min(to(b, a.p0)).min(to(b, a.p1)) <= slack
}

/// Do the two segments share a point (a crossing, a touch or a collinear overlap)?
fn lines_meet(a: kurbo::Line, b: kurbo::Line) -> bool {
    let orient = |p: Point, q: Point, r: Point| (q - p).cross(r - p);
    let within = |p: Point, q: Point, r: Point| r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y);
    let (d1, d2) = (orient(b.p0, b.p1, a.p0), orient(b.p0, b.p1, a.p1));
    let (d3, d4) = (orient(a.p0, a.p1, b.p0), orient(a.p0, a.p1, b.p1));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return true;
    }
    (d1 == 0.0 && within(b.p0, b.p1, a.p0))
        || (d2 == 0.0 && within(b.p0, b.p1, a.p1))
        || (d3 == 0.0 && within(a.p0, a.p1, b.p0))
        || (d4 == 0.0 && within(a.p0, a.p1, b.p1))
}

/// Does the path intersect (or lie inside) the rect? Used for marquee selection.
pub fn intersects_rect(path: &PathData, r: Rect) -> bool {
    let r = r.abs();
    let Some(b) = path.bounds() else { return false };
    let flat = |q: Rect| q.width() == 0.0 || q.height() == 0.0;
    if b.intersect(r).area() <= 0.0 && !flat(b) && !flat(r) {
        return false;
    }
    // Any anchor inside?
    if path.anchors().any(|(_, _, a)| r.contains(a.p)) {
        return true;
    }
    // Any segment crossing the rect edges?
    let edges = [
        kurbo::Line::new((r.x0, r.y0), (r.x1, r.y0)),
        kurbo::Line::new((r.x1, r.y0), (r.x1, r.y1)),
        kurbo::Line::new((r.x1, r.y1), (r.x0, r.y1)),
        kurbo::Line::new((r.x0, r.y1), (r.x0, r.y0)),
    ];
    let bp = path.to_bezpath();
    for seg in bp.segments() {
        for e in &edges {
            if !seg.intersect_line(*e).is_empty() {
                return true;
            }
        }
    }
    // Rect entirely inside a closed shape counts as touching it.
    path.is_closed() && fill_contains(&bp, FillRule::NonZero, r.center())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes;

    #[test]
    fn fill_and_stroke() {
        let p = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)).to_bezpath();
        assert!(fill_contains(&p, FillRule::NonZero, Point::new(5.0, 5.0)));
        assert!(!fill_contains(&p, FillRule::NonZero, Point::new(15.0, 5.0)));
        assert!(stroke_contains(&p, 2.0, 0.0, Point::new(10.5, 5.0)));
        assert!(!stroke_contains(&p, 2.0, 0.0, Point::new(12.0, 5.0)));
    }

    #[test]
    fn evenodd_hole() {
        let mut outer = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        outer.subpaths.extend(shapes::rectangle(Rect::new(3.0, 3.0, 7.0, 7.0)).subpaths);
        let bp = outer.to_bezpath();
        assert!(!fill_contains(&bp, FillRule::EvenOdd, Point::new(5.0, 5.0)));
        assert!(fill_contains(&bp, FillRule::NonZero, Point::new(5.0, 5.0)));
        assert!(fill_contains(&bp, FillRule::EvenOdd, Point::new(1.0, 5.0)));
    }

    #[test]
    fn crossing_edges() {
        let rect = |x0, y0, x1, y1| shapes::rectangle(Rect::new(x0, y0, x1, y1));
        let check = |p: &PathData| edges_cross(p, 0.05, &mut 100_000, &mut 1_000_000);
        let mut ring = rect(0.0, 0.0, 10.0, 10.0);
        ring.subpaths.extend(rect(3.0, 3.0, 7.0, 7.0).subpaths);
        assert_eq!(check(&ring), Some(false), "nested contours");
        assert_eq!(check(&shapes::ellipse(Rect::new(0.0, 0.0, 10.0, 10.0))), Some(false), "one curve");
        let mut plus = rect(0.0, 4.0, 10.0, 6.0);
        plus.subpaths.extend(rect(4.0, 0.0, 6.0, 10.0).subpaths);
        assert_eq!(check(&plus), Some(true), "crossing contours");
        let mut touching = rect(0.0, 0.0, 10.0, 10.0);
        touching.subpaths.extend(rect(0.0, 3.0, 5.0, 7.0).subpaths);
        assert_eq!(check(&touching), Some(true), "a hole on the outline");
        // A huge curve is flattened in a bounded number of steps.
        let mut edges = 100_000;
        assert_eq!(edges_cross(&shapes::ellipse(Rect::new(0.0, 0.0, 1e12, 1e12)), 0.05, &mut edges, &mut 1_000_000), Some(false));
        assert!(edges > 90_000, "{edges}");
        let star = PathData::single(crate::path::SubPath::new(
            [(5.0, 0.0), (8.0, 10.0), (0.0, 4.0), (10.0, 4.0), (2.0, 10.0)]
                .iter()
                .map(|&(x, y)| crate::path::Anchor::corner(Point::new(x, y)))
                .collect(),
            true,
        ));
        assert_eq!(check(&star), Some(true), "a contour crossing itself");
        // An open contour is filled across its gap: the closing edge counts.
        let open = PathData::single(crate::path::SubPath::new(
            [(0.0, 5.0), (0.0, 0.0), (5.0, 0.0), (5.0, 10.0), (10.0, 10.0), (10.0, 5.0)]
                .iter()
                .map(|&(x, y)| crate::path::Anchor::corner(Point::new(x, y)))
                .collect(),
            false,
        ));
        assert_eq!(check(&open), Some(true));
        let mut drawn = open.clone();
        drawn.subpaths[0].anchors.truncate(4);
        assert_eq!(check(&drawn), Some(false), "without the last two anchors nothing crosses");
        // The closing edge is straight: handles left on the open ends are not part of the fill.
        // (Used, they would bow the closing edge below the bottom edge.)
        drawn.subpaths[0].anchors[3].h_out = Point::new(2.0, -10.0);
        drawn.subpaths[0].anchors[0].h_in = Point::new(0.0, -10.0);
        assert_eq!(check(&drawn), Some(false));
    }

    #[test]
    fn bernstein_roots_find_touching_zeros() {
        // 3 (17 t - 13)^2: zero only at t = 13/17, where it touches 0 without changing sign.
        let roots = bernstein_roots(&[507.0, 65.0, -88.0, 48.0]);
        assert!(roots.iter().any(|t| (t - 13.0 / 17.0).abs() < 1e-9), "{roots:?}");
        // t (1 - t): zero at both ends only.
        assert_eq!(bernstein_roots(&[0.0, 0.5, 0.0]), vec![0.0, 1.0]);
        // 1e6 t^2 - 1e-7: the value at t = 0 rounds to zero, and the root just after it still counts.
        let roots = bernstein_roots(&[-1e-7, -1e-7, 1e6 / 3.0 - 1e-7, 1e6 - 1e-7]);
        assert!(roots.iter().any(|t| (t - 1e-13f64.sqrt()).abs() < 1e-12), "{roots:?}");
    }

    #[test]
    fn curve_checks_stay_bounded() {
        // A curve too large to flatten is not checked.
        let huge = shapes::ellipse(Rect::new(0.0, 0.0, 1e155, 1e155));
        assert_eq!(edges_cross(&huge, 0.05, &mut 100_000, &mut 1_000_000), None);
        // Two curves lying on each other far from the origin: the halving ends in a few
        // thousand steps.
        let far = kurbo::CubicBez::new((1e8, 1e8), (1e8 + 1.0, 1e8 + 1.0), (1e8 + 2.0, 1e8), (1e8 + 3.0, 1e8 + 1.0));
        let mut tests = 1_000_000;
        assert_ne!(curves_meet(far, far, &mut tests), Some(false));
        assert!(tests > 990_000, "{tests}");
    }

    #[test]
    fn crossing_edges_where_curves_turn_back() {
        let check = |p: &BezPath| edges_cross(&PathData::from_bezpath(p), 0.05, &mut 100_000, &mut 1_000_000);
        // The second curve heads back and crosses the first just after their joint, closer than
        // their steps show.
        let mut cross = BezPath::new();
        cross.move_to((0.0, 0.0));
        cross.quad_to((5.0, 0.002), (10.0, 0.0));
        cross.quad_to((7.5, -0.01), (5.0, 0.12));
        cross.line_to((5.0, 2.0));
        cross.line_to((12.0, 2.0));
        cross.line_to((12.0, -3.0));
        cross.line_to((0.0, -3.0));
        cross.close_path();
        assert_eq!(check(&cross), Some(true), "curves crossing after a turn back");
        // A crescent: both arcs leave each tip on the same side and meet only there.
        let mut crescent = BezPath::new();
        crescent.move_to((0.0, -10.0));
        crescent.curve_to((13.0, -10.0), (13.0, 10.0), (0.0, 10.0));
        crescent.curve_to((7.0, 10.0), (7.0, -10.0), (0.0, -10.0));
        crescent.close_path();
        assert_eq!(check(&crescent), Some(false), "a crescent");
    }

    #[test]
    fn crossing_edges_with_curves() {
        let rect = |x0, y0, x1, y1| shapes::rectangle(Rect::new(x0, y0, x1, y1));
        let check = |p: &PathData| edges_cross(p, 0.05, &mut 100_000, &mut 1_000_000);
        let mut ring = rect(0.0, 0.0, 10.0, 10.0);
        ring.subpaths.extend(rect(3.0, 3.0, 7.0, 7.0).subpaths);
        // A square corner just inside or just outside a circle's outline: straight edges and
        // curves are intersected exactly, closer than the flattening tolerance.
        let near = |inset: f64| {
            let mut p = shapes::ellipse(Rect::new(0.0, 0.0, 10.0, 10.0));
            let c = 5.0 + (5.0 - inset) / std::f64::consts::SQRT_2;
            p.subpaths.extend(rect(5.0, 5.0, c, c).subpaths);
            check(&p)
        };
        assert_eq!(near(0.02), Some(false));
        assert_eq!(near(-0.02), Some(true));
        assert_eq!(near(0.5), Some(false));
        // A shallow arc closed by its chord flattens to two edges but still encloses a sliver:
        // the edges cannot check it.
        let mut arc = BezPath::new();
        arc.move_to((0.0, 0.0));
        arc.quad_to((5.0, 0.02), (10.0, 0.0));
        arc.close_path();
        assert_eq!(check(&PathData::from_bezpath(&arc)), None, "a sliver");
        // A curve that comes back across the edge before it: the neighbors meet away from their joint.
        let mut hook = BezPath::new();
        hook.move_to((0.0, 0.0));
        hook.line_to((10.0, 0.0));
        hook.quad_to((5.0, -0.02), (0.0, 0.01));
        hook.close_path();
        assert_eq!(check(&PathData::from_bezpath(&hook)), Some(true), "a curve crossing its neighbor");
        // A shallow curve that dips across the edge before it and ends just above it: its chord
        // stays clear, the curve does not.
        let mut dip = BezPath::new();
        dip.move_to((0.0, 0.0));
        dip.line_to((10.0, 0.0));
        dip.quad_to((7.5, -0.01), (5.0, 0.08));
        dip.line_to((5.0, 2.0));
        dip.line_to((12.0, 2.0));
        dip.line_to((12.0, -3.0));
        dip.line_to((0.0, -3.0));
        dip.close_path();
        assert_eq!(check(&PathData::from_bezpath(&dip)), Some(true), "a curve dipping across its neighbor");
        // The same with the first edge written as a straight curve.
        let mut dip_flat = BezPath::new();
        dip_flat.move_to((0.0, 0.0));
        dip_flat.curve_to((3.0, 0.0), (7.0, 0.0), (10.0, 0.0));
        dip_flat.extend(dip.elements().iter().skip(2).copied());
        assert_eq!(check(&PathData::from_bezpath(&dip_flat)), Some(true), "after a straight curve");
        // A straight edge shorter than the flattening tolerance that carries on past a curve.
        let mut short = BezPath::new();
        short.move_to((0.0, 0.0));
        short.quad_to((50.0, -10.0), (100.0, 0.0));
        short.line_to((100.01, 0.0));
        short.line_to((100.01, 100.0));
        short.line_to((0.0, 100.0));
        short.close_path();
        assert_eq!(check(&PathData::from_bezpath(&short)), Some(false), "a short edge after a curve");
        // A repeated anchor between an edge and a curve: they are still neighbors.
        let mut repeated = BezPath::new();
        repeated.move_to((0.0, 0.0));
        repeated.line_to((100.0, 0.0));
        repeated.line_to((100.0, 0.0));
        repeated.quad_to((110.0, 50.0), (100.0, 100.0));
        repeated.line_to((0.0, 100.0));
        repeated.close_path();
        assert_eq!(check(&PathData::from_bezpath(&repeated)), Some(false), "a repeated anchor");
        // A curve crossing back over a long edge close to their joint.
        let mut hairpin = BezPath::new();
        hairpin.move_to((-1e6, 0.0));
        hairpin.line_to((0.0, 0.0));
        hairpin.quad_to((-1.0, -1.0), (-1.0, 3.0));
        hairpin.close_path();
        assert_eq!(check(&PathData::from_bezpath(&hairpin)), Some(true), "a crossing near the joint of a long edge");
        // A curve that only touches an edge of another contour (a double zero).
        let mut kiss = BezPath::new();
        kiss.move_to((0.0, 507.0));
        kiss.curve_to((1.0, 65.0), (2.0, -88.0), (3.0, 48.0));
        kiss.line_to((3.0, 600.0));
        kiss.line_to((0.0, 600.0));
        kiss.close_path();
        kiss.move_to((2.0, 0.0));
        kiss.line_to((3.0, 0.0));
        kiss.line_to((2.5, -5.0));
        kiss.close_path();
        assert_eq!(check(&PathData::from_bezpath(&kiss)), Some(true), "a curve touching an edge");
        // A narrow curved ring: its circles come closer than the flattening tolerance but do not meet.
        let mut thin = shapes::ellipse(Rect::new(-10.0, -10.0, 10.0, 10.0));
        thin.subpaths.extend(shapes::ellipse(Rect::new(-9.98, -9.98, 9.98, 9.98)).subpaths);
        assert_eq!(check(&thin), Some(false), "a narrow ring");
        let mut kissing = shapes::ellipse(Rect::new(-10.0, -10.0, 10.0, 10.0));
        kissing.subpaths.extend(shapes::ellipse(Rect::new(-9.98, -9.98, 10.0, 9.98)).subpaths);
        assert_eq!(check(&kissing), Some(true), "circles touching inside");
        // A loop inside one curve too small to show in its steps.
        let mut tiny_loop = BezPath::new();
        tiny_loop.move_to((0.0, 0.0));
        tiny_loop.curve_to((-0.004, 0.006), (-0.004, -0.003), (0.0, 0.003));
        tiny_loop.line_to((-1.0, 0.003));
        tiny_loop.line_to((-1.0, 0.0));
        tiny_loop.close_path();
        assert_eq!(check(&PathData::from_bezpath(&tiny_loop)), None, "a curve that may loop");
        // A curve with a handle drawn in to its anchor bends one way only.
        let mut retracted = BezPath::new();
        retracted.move_to((0.0, 0.0));
        retracted.curve_to((0.0, 0.0), (100.0, 0.0), (100.0, 100.0));
        retracted.line_to((0.0, 100.0));
        retracted.close_path();
        assert_eq!(check(&PathData::from_bezpath(&retracted)), Some(false), "a retracted handle");
        // A ring whose outer circle has an extra anchor just before the end of one arc: the short
        // arc carries on from the long one.
        let mut split = BezPath::new();
        let circle = shapes::ellipse(Rect::new(-10.0, -10.0, 10.0, 10.0)).to_bezpath();
        for (i, seg) in circle.segments().enumerate() {
            if i == 0 {
                split.move_to(seg.start());
            }
            match seg {
                kurbo::PathSeg::Cubic(c) if i == 0 => {
                    let (long, short) = (c.subsegment(0.0..0.999), c.subsegment(0.999..1.0));
                    split.curve_to(long.p1, long.p2, long.p3);
                    split.curve_to(short.p1, short.p2, short.p3);
                }
                kurbo::PathSeg::Cubic(c) => split.curve_to(c.p1, c.p2, c.p3),
                kurbo::PathSeg::Line(l) => split.line_to(l.p1),
                kurbo::PathSeg::Quad(q) => split.quad_to(q.p1, q.p2),
            }
        }
        split.close_path();
        let mut split_ring = PathData::from_bezpath(&split);
        split_ring.subpaths.extend(shapes::ellipse(Rect::new(-5.0, -5.0, 5.0, 5.0)).subpaths);
        assert_eq!(check(&split_ring), Some(false), "a ring with a short arc");
        // A half disc: its arc meets the diameter at both ends only.
        let mut half = BezPath::new();
        half.move_to((0.0, 0.0));
        half.line_to((10.0, 0.0));
        half.curve_to((10.0, 6.0), (0.0, 6.0), (0.0, 0.0));
        half.close_path();
        assert_eq!(check(&PathData::from_bezpath(&half)), Some(false), "a half disc");
        // A straight line written as a curve is a line: through the ring, it encloses no area.
        let mut flat = BezPath::new();
        flat.move_to((-1.0, 5.0));
        flat.curve_to((2.0, 5.0), (8.0, 5.0), (11.0, 5.0));
        let mut ring_and_flat = ring.clone();
        ring_and_flat.subpaths.extend(PathData::from_bezpath(&flat).subpaths);
        assert_eq!(check(&ring_and_flat), Some(false), "a line written as a curve");
        let fold = PathData::single(crate::path::SubPath::new(
            [(0.0, 0.0), (10.0, 0.0), (5.0, 0.0), (5.0, 5.0)].iter().map(|&(x, y)| crate::path::Anchor::corner(Point::new(x, y))).collect(),
            true,
        ));
        assert_eq!(check(&fold), Some(true), "a straight edge folding back over its neighbor");
        // A loop smaller than the tolerance flattens to a point. (This one stays clear of the
        // edges beside it.)
        let mut looped = BezPath::new();
        looped.move_to((0.0, 0.0));
        looped.line_to((10.0, 0.0));
        looped.curve_to((10.02, 0.01), (10.01, 0.02), (10.0, 0.0));
        looped.line_to((10.0, 10.0));
        looped.line_to((0.0, 10.0));
        looped.close_path();
        assert_eq!(check(&PathData::from_bezpath(&looped)), None, "a loop flattened to a point");
        // An open line through the ring encloses no area, with or without a middle anchor.
        let mut lined = ring.clone();
        lined.subpaths.push(crate::path::SubPath::new(
            [(-10.0, 5.0), (4.0, 5.0), (11.0, 5.0)].iter().map(|&(x, y)| crate::path::Anchor::corner(Point::new(x, y))).collect(),
            false,
        ));
        assert_eq!(check(&lined), Some(false), "a line along one direction");
        assert_eq!(edges_cross(&ring, 0.05, &mut 3, &mut 1000), None, "too many edges");
        assert_eq!(edges_cross(&ring, 0.05, &mut 1000, &mut 1), None, "too many comparisons");
    }

    #[test]
    fn marquee() {
        let p = shapes::ellipse(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(intersects_rect(&p, Rect::new(-1.0, 4.0, 1.0, 6.0)));
        assert!(!intersects_rect(&p, Rect::new(20.0, 20.0, 30.0, 30.0)));
        // corner region of the bbox that misses the ellipse outline
        assert!(!intersects_rect(&p, Rect::new(0.0, 0.0, 0.5, 0.5)));
        // fully inside
        assert!(intersects_rect(&p, Rect::new(4.0, 4.0, 6.0, 6.0)));
        // crossing a line
        let l = shapes::line(Point::new(0.0, 0.0), Point::new(10.0, 10.0));
        assert!(intersects_rect(&l, Rect::new(4.0, 0.0, 6.0, 10.0)));
    }

    #[test]
    fn reversed_marquee() {
        let p = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(intersects_rect(&p, Rect::new(15.0, 15.0, 5.0, 5.0)));
        assert!(intersects_rect(&p, Rect::new(15.0, 5.0, 5.0, 15.0)));
        assert!(intersects_rect(&p, Rect::new(6.0, 6.0, 4.0, 4.0)));
        assert!(!intersects_rect(&p, Rect::new(30.0, 30.0, 20.0, 20.0)));
    }

    #[test]
    fn flat_marquee() {
        let p = shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(intersects_rect(&p, Rect::new(-5.0, 5.0, 15.0, 5.0)));
        assert!(intersects_rect(&p, Rect::new(5.0, -5.0, 5.0, 15.0)));
        assert!(!intersects_rect(&p, Rect::new(-5.0, 20.0, 15.0, 20.0)));
    }
}
