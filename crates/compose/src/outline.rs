//! The outline of a non-rectangular text frame (ovals, polygons, corner options, Pathfinder
//! results, freeform closed paths): lines are set in the part of each line band the outline covers.

use designcraft_geom::{Affine, BezPath, PathEl, Point};

/// Flattening tolerance (points) for curved frame outlines.
const TOLERANCE: f64 = 0.1;
/// Most edges a flattened outline may have; a larger one is flattened coarser.
const MAX_EDGES: usize = 4096;
/// Scanlines sampled across each inset cap above and below a band.
const CAP_SAMPLES: usize = 6;

/// A closed frame outline flattened to straight edges, in the frame spec's area space.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameOutline {
    edges: Vec<(Point, Point)>,
    /// Distance kept between text and the outline.
    inset: f64,
}

impl FrameOutline {
    /// The outline of `bp` (closed subpaths only), or None when it is its own axis-aligned
    /// rectangle (or has no area), so the frame keeps its rectangular text area.
    pub fn new(bp: &BezPath, inset: f64) -> Option<FrameOutline> {
        let bb = designcraft_geom::Shape::bounding_box(bp);
        let extent = bb.width().max(bb.height());
        if !extent.is_finite() {
            return None;
        }
        // Huge outlines are flattened relative to their size, so the edge count stays bounded.
        let mut tol = TOLERANCE.max(extent * 1e-6);
        let edges = loop {
            let (edges, over) = flatten_closed(bp, tol);
            if !over && edges.len() <= MAX_EDGES {
                break edges;
            }
            tol *= 4.0;
            if tol > 64.0 {
                return None;
            }
        };
        if edges.is_empty() || edges.iter().any(|(a, b)| !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite())) {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (a, _) in &edges {
            x0 = x0.min(a.x);
            y0 = y0.min(a.y);
            x1 = x1.max(a.x);
            y1 = y1.max(a.y);
        }
        let eps = 1e-6 * (x1 - x0).abs().max((y1 - y0).abs()).max(1.0);
        let on_box = |p: Point| (p.x - x0).abs() < eps || (p.x - x1).abs() < eps || (p.y - y0).abs() < eps || (p.y - y1).abs() < eps;
        let rect = edges.iter().all(|&(a, b)| ((a.x - b.x).abs() < eps || (a.y - b.y).abs() < eps) && on_box(a) && on_box(b));
        if rect {
            return None;
        }
        let inset = if inset.is_finite() { inset.max(0.0) } else { 0.0 };
        Some(FrameOutline { edges, inset })
    }

    /// The outline mapped by `xf` (into a vertical frame's turned box).
    pub fn transformed(&self, xf: Affine) -> FrameOutline {
        FrameOutline { edges: self.edges.iter().map(|&(a, b)| (xf * a, xf * b)).collect(), inset: self.inset }
    }

    /// The parts of `[x0, x1]` inside the outline over the whole band `[y0, y1]`, at least the
    /// inset away from it: the inside intervals at every height of the band are intersected, and
    /// the inset is kept by testing the heights within the inset above and below the band, each
    /// shrunk by the half-chord of an inset-radius circle there.
    pub fn intervals(&self, x0: f64, x1: f64, y0: f64, y1: f64) -> Vec<(f64, f64)> {
        let d = self.inset;
        if !(y0.is_finite() && y1.is_finite() && x0.is_finite() && x1.is_finite()) || y1 < y0 {
            return vec![];
        }
        let (lo, hi) = (y0 - d, y1 + d);
        let near: Vec<(Point, Point)> = self.edges.iter().copied().filter(|(a, b)| a.y.max(b.y) >= lo && a.y.min(b.y) <= hi).collect();
        // Heights where an edge ends (the inside intervals change linearly between them).
        let mut ys: Vec<f64> = vec![y0, y1];
        for (a, _) in &near {
            if a.y > lo && a.y < hi {
                ys.push(a.y);
            }
        }
        if d > 0.0 {
            for k in 0..=CAP_SAMPLES {
                let t = k as f64 / CAP_SAMPLES as f64;
                ys.push(lo + t * d);
                ys.push(y1 + t * d);
            }
        }
        ys.sort_by(f64::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        // Midpoints catch spans that close between two sampled heights.
        let mids: Vec<f64> = ys.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
        ys.extend(mids);
        let mut free = vec![(x0, x1)];
        for y in ys {
            let off = y0 - y;
            let off = if off > 0.0 { off } else { (y - y1).max(0.0) };
            let shrink = (d * d - off * off).max(0.0).sqrt();
            let row: Vec<(f64, f64)> = inside_at(&near, y).into_iter().map(|(a, b)| (a + shrink, b - shrink)).filter(|(a, b)| b > a).collect();
            free = intersect(&free, &row);
            if free.is_empty() {
                break;
            }
        }
        free
    }
}

/// Inside intervals of the scanline at `y` through `edges` (non-zero winding, as the frame is
/// filled).
fn inside_at(edges: &[(Point, Point)], y: f64) -> Vec<(f64, f64)> {
    let mut xs: Vec<(f64, i32)> = Vec::new();
    for &(a, b) in edges {
        if (a.y <= y) != (b.y <= y) {
            let t = (y - a.y) / (b.y - a.y);
            let x = a.x + t * (b.x - a.x);
            if x.is_finite() {
                xs.push((x, if b.y > a.y { 1 } else { -1 }));
            }
        }
    }
    xs.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut out = Vec::new();
    let mut wind = 0;
    let mut start = 0.0;
    for (x, w) in xs {
        let was = wind != 0;
        wind += w;
        match (was, wind != 0) {
            (false, true) => start = x,
            (true, false) if x > start => out.push((start, x)),
            _ => {}
        }
    }
    out
}

/// Intersection of two sorted lists of disjoint intervals.
fn intersect(a: &[(f64, f64)], b: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while let (Some(&(a0, a1)), Some(&(b0, b1))) = (a.get(i), b.get(j)) {
        let (s, e) = (a0.max(b0), a1.min(b1));
        if e > s {
            out.push((s, e));
        }
        if a1 < b1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// The closed subpaths of `bp` as straight edges (each subpath closed back to its start), and
/// whether flattening stopped at the edge limit.
fn flatten_closed(bp: &BezPath, tol: f64) -> (Vec<(Point, Point)>, bool) {
    let mut edges = Vec::new();
    let mut sub: Vec<Point> = Vec::new();
    let mut closed = false;
    let finish = |sub: &mut Vec<Point>, closed: bool, edges: &mut Vec<(Point, Point)>| {
        if closed && sub.len() >= 3 {
            for w in sub.windows(2) {
                edges.push((w[0], w[1]));
            }
            if let (Some(&l), Some(&f)) = (sub.last(), sub.first()) {
                edges.push((l, f));
            }
        }
        sub.clear();
    };
    let mut over = false;
    designcraft_geom::kurbo::flatten(bp.iter(), tol, |el| {
        if over {
            return;
        }
        match el {
            PathEl::MoveTo(p) => {
                finish(&mut sub, closed, &mut edges);
                closed = false;
                sub.push(p);
            }
            PathEl::LineTo(p) => sub.push(p),
            PathEl::ClosePath => closed = true,
            _ => {}
        }
        if sub.len() + edges.len() > MAX_EDGES * 4 {
            over = true;
        }
    });
    finish(&mut sub, closed, &mut edges);
    (edges, over)
}
