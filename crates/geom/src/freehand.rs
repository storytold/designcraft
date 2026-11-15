//! Freehand drawing (Pencil tool): a dragged polyline simplified to a few smooth anchors.

use crate::Point;
use crate::path::{Anchor, AnchorKind};

/// Distance tests [`simplify`] may make. Worst-case input splits one point off at a time (quadratic);
/// past this the spans still waiting keep only their end points, a coarser fit rather than a stall.
const MAX_SIMPLIFY_STEPS: usize = 16 * 1024 * 1024;

/// Ramer–Douglas–Peucker: the points that keep the polyline within `tol`.
fn simplify(pts: &[Point], tol: f64) -> Vec<Point> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0, pts.len() - 1)];
    let mut steps = 0usize;
    while let Some((a, b)) = stack.pop() {
        steps = steps.saturating_add(b - a);
        if steps > MAX_SIMPLIFY_STEPS {
            break;
        }
        let (pa, pb) = (pts[a], pts[b]);
        let d = pb - pa;
        let len = d.hypot();
        let mut best = (0.0, 0);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let dist = if len < 1e-9 { (*p - pa).hypot() } else { ((*p - pa).cross(d) / len).abs() };
            if dist > best.0 {
                best = (dist, i);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Smooth anchors through a freehand stroke: simplified within `tol`, with Catmull–Rom handles
/// (a third of the way to the neighbours). Ends are corners unless `closed`.
pub fn fit(points: &[Point], tol: f64, closed: bool) -> Vec<Anchor> {
    // Drop repeated samples first.
    let mut pts: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if pts.last().is_none_or(|q: &Point| (*q - *p).hypot() > 1e-6) {
            pts.push(*p);
        }
    }
    let mut pts = simplify(&pts, tol.max(1e-3));
    if closed && pts.len() > 2 && (pts[0] - pts[pts.len() - 1]).hypot() < tol * 4.0 {
        pts.pop();
    }
    let n = pts.len();
    if n < 2 {
        return pts.into_iter().map(Anchor::corner).collect();
    }
    (0..n)
        .map(|i| {
            let p = pts[i];
            let prev = if i > 0 {
                Some(pts[i - 1])
            } else if closed {
                Some(pts[n - 1])
            } else {
                None
            };
            let next = if i + 1 < n {
                Some(pts[i + 1])
            } else if closed {
                Some(pts[0])
            } else {
                None
            };
            match (prev, next) {
                (Some(a), Some(b)) => {
                    let t = (b - a) / 6.0;
                    // Shorter handles toward a closer neighbour keep curves from overshooting.
                    let (la, lb) = ((p - a).hypot(), (b - p).hypot());
                    let k = |l: f64| if la + lb > 1e-9 { 2.0 * l / (la + lb) } else { 1.0 };
                    Anchor { p, h_in: p - t * k(la), h_out: p + t * k(lb), kind: AnchorKind::Smooth }
                }
                _ => Anchor::corner(p),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freehand_line_and_curve() {
        // A wobbly straight stroke becomes two corners.
        let line: Vec<Point> = (0..=100).map(|i| Point::new(i as f64, if i % 2 == 0 { 0.3 } else { -0.3 })).collect();
        let a = fit(&line, 2.0, false);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].kind, AnchorKind::Corner);
        // A semicircle keeps a handful of smooth anchors.
        let arc: Vec<Point> = (0..=180)
            .map(|d| {
                let t = (d as f64).to_radians();
                Point::new(100.0 * t.cos(), 100.0 * t.sin())
            })
            .collect();
        let a = fit(&arc, 1.0, false);
        assert!(a.len() >= 4 && a.len() <= 20, "{}", a.len());
        assert!(a[1..a.len() - 1].iter().all(|x| x.kind == AnchorKind::Smooth));
        // Closed: the end that meets the start is merged.
        let circle: Vec<Point> = (0..=360)
            .map(|d| {
                let t = (d as f64).to_radians();
                Point::new(50.0 * t.cos(), 50.0 * t.sin())
            })
            .collect();
        let c = fit(&circle, 1.0, true);
        assert!(c.iter().all(|x| x.kind == AnchorKind::Smooth));
        assert!((c[0].p - c[c.len() - 1].p).hypot() > 1.0);
    }

    /// A convex curve splits one point at a time (quadratic): fitting stops at the step budget.
    #[test]
    fn unbalanced_input_stops_at_the_step_budget() {
        let n = 100_000;
        let curve: Vec<Point> = (0..n).map(|i| Point::new(f64::from(i), (f64::from(i) * 1e-3).powi(4))).collect();
        let a = fit(&curve, 1e-9, false);
        assert!(a.len() >= 2 && a.len() < n as usize);
    }
}
