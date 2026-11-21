//! The outline of a non-rectangular text frame (ovals, polygons, corner options, Pathfinder
//! results, freeform closed paths): lines are set in the part of each line band the outline covers.

use designcraft_geom::{Affine, BezPath, PathEl, Point};

/// Flattening tolerance (points) for curved frame outlines.
const TOLERANCE: f64 = 0.1;
/// How far (points) an outline's edges may be from its bounding box and still be a rectangle.
const RECT_TOLERANCE: f64 = TOLERANCE;
/// The same as a fraction of the outline's larger side: a tall frame is a little further off square
/// before anyone could see it (a chart's axis-label frame 239 pt tall is 0.54 pt off).
const RECT_RELATIVE_TOLERANCE: f64 = 0.003;
/// The most an edge may be off, as a fraction of its own length, however large the frame: the
/// frame-wide fraction above would let a 600 pt tall frame 17 pt wide take a 1.8 pt slant (6
/// degrees) on its bottom edge. The one measured frame has 0.54 pt over 16.74 pt (3.2%).
const RECT_EDGE_SLOPE: f64 = 0.04;
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
        // A rectangle is a path whose every edge lies on the bounding box, to within the flattening
        // tolerance (or 0.3% of its size): frames made by scripts and plug-ins are often a hundredth of a point off
        // square, and InDesign composes into such a frame as into its rectangle. Treating it as a
        // shape instead set every line's band against the slanted edge, and a one-line frame as
        // tall as its line lost the part of its width the slant cut off. The limits come from two
        // frames measured against InDesign (0.022 pt off on a 44 pt frame, 0.54 pt off on a 239 pt
        // one), not from a measured changeover: the 4% of an edge's own length only admits the
        // second and keeps a steeper slant on a narrow, tall frame a shape.
        let eps = RECT_TOLERANCE.max(RECT_RELATIVE_TOLERANCE * (x1 - x0).abs().max((y1 - y0).abs()));
        // Both ends of an edge must lie on the same side of the box: an edge with one end on the
        // left side and one on the right is not on the box when it runs through the middle, as the
        // edges of a compound path's inner sub-rectangles do.
        let on_side = |a: Point, b: Point| {
            let near = |v: f64, side: f64| (v - side).abs() < eps;
            (near(a.x, x0) && near(b.x, x0))
                || (near(a.x, x1) && near(b.x, x1))
                || (near(a.y, y0) && near(b.y, y0))
                || (near(a.y, y1) && near(b.y, y1))
        };
        // How far an edge's ends may differ along its short direction: the frame-wide slack, and
        // no more than a fraction of the edge's own length. Only this slant is held to the edge's
        // length: how far the edge lies from the box stays at the frame-wide slack, because a
        // straight side with an extra anchor is stored as shorter edges, which must classify as the
        // side does.
        let slant_limit = |a: Point, b: Point| {
            let length = (a.x - b.x).abs().max((a.y - b.y).abs());
            eps.min(RECT_TOLERANCE.max(RECT_EDGE_SLOPE * length))
        };
        let rect = edges.iter().all(|&(a, b)| {
            let slant = slant_limit(a, b);
            ((a.x - b.x).abs() < slant || (a.y - b.y).abs() < slant) && on_side(a, b)
        });
        if rect {
            return None;
        }
        let inset = if inset.is_finite() { inset.max(0.0) } else { 0.0 };
        Some(FrameOutline { edges, inset })
    }

    /// The outline of `item`'s path with its corner options applied, or None when that is the
    /// item's own rectangle. The composer and the Properties panel's one Inset field both use it.
    pub fn of_item(item: &designcraft_doc::Item, inset: f64) -> Option<FrameOutline> {
        let bp = if item.corners.is_none() { item.path.to_bezpath() } else { designcraft_geom::corners::apply(&item.path, &item.corners) };
        FrameOutline::new(&bp, inset)
    }

    /// The outline mapped by `xf` (into a vertical frame's turned box).
    pub fn transformed(&self, xf: Affine) -> FrameOutline {
        FrameOutline { edges: self.edges.iter().map(|&(a, b)| (xf * a, xf * b)).collect(), inset: self.inset }
    }

    /// The distance kept between text and the outline.
    pub fn inset(&self) -> f64 {
        self.inset
    }

    /// The heights of the outline's corners, sorted and without repeats: the inside intervals of a
    /// scanline change linearly between two of them.
    pub fn corner_heights(&self) -> Vec<f64> {
        let mut ys: Vec<f64> = self.edges.iter().map(|(a, _)| a.y).collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        ys
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A quadrilateral 44 wide and about 10 tall whose bottom edge runs from `left` to `right`.
    fn quad(left: f64, right: f64) -> BezPath {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.line_to((0.0, left));
        bp.line_to((44.0, right));
        bp.line_to((44.0, 0.0));
        bp.close_path();
        bp
    }

    /// Frames made by scripts and plug-ins are often a hundredth of a point off square (a chart's
    /// label frame has a bottom edge 0.022 pt slanted). InDesign composes into such a frame as
    /// into its rectangle; as a shape it cut a one-line frame's slot from 44 pt to 28.
    #[test]
    fn a_frame_a_hundredth_of_a_point_off_square_is_its_rectangle() {
        assert!(FrameOutline::new(&quad(9.766, 9.766), 0.0).is_none(), "an exact rectangle");
        assert!(FrameOutline::new(&quad(9.766, 9.788), 0.0).is_none(), "0.022 pt off square");
        assert!(FrameOutline::new(&quad(9.766, 9.856), 0.0).is_none(), "0.09 pt off square, within the flattening tolerance");
    }

    /// The same slack in a tall frame: a chart's axis-label frame, 239 pt tall and 17 pt wide, has a
    /// bottom edge 0.54 pt slanted. A fraction of the frame's size counts as square too, so the
    /// seven labels it holds are set as in InDesign instead of being left out of the export.
    #[test]
    fn a_tall_frame_a_few_tenths_of_a_point_off_square_is_its_rectangle() {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.line_to((0.0, 238.76));
        bp.line_to((16.74, 239.3));
        bp.line_to((16.74, 0.0));
        bp.close_path();
        assert!(FrameOutline::new(&bp, 0.0).is_none());
        // Three points off over the same height is a visible slant.
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.line_to((0.0, 238.76));
        bp.line_to((16.74, 241.76));
        bp.line_to((16.74, 0.0));
        bp.close_path();
        assert!(FrameOutline::new(&bp, 0.0).is_some());
    }

    /// Two stacked rectangles in one path (a compound path) are not one rectangle, whatever the
    /// slack: the edges between them run through the middle of the bounding box, with each end on
    /// a different side of it.
    #[test]
    fn two_stacked_rectangles_in_one_path_are_a_shape() {
        let stack = |gap_top: f64| {
            let mut bp = BezPath::new();
            for (y0, y1) in [(0.0, 20.0), (gap_top, 238.0)] {
                bp.move_to((0.0, y0));
                bp.line_to((0.0, y1));
                bp.line_to((100.0, y1));
                bp.line_to((100.0, y0));
                bp.close_path();
            }
            bp
        };
        assert!(FrameOutline::new(&stack(218.0), 0.0).is_some(), "198 pt apart");
        // The edge the review named: from (100, 20.54) to (0, 20), within the slack of the box's height.
        let mut bp = BezPath::new();
        bp.move_to((0.0, 20.0));
        bp.line_to((0.0, 238.0));
        bp.line_to((100.0, 238.0));
        bp.line_to((100.0, 20.54));
        bp.close_path();
        bp.move_to((0.0, 0.0));
        bp.line_to((0.0, 20.0));
        bp.line_to((100.0, 20.54));
        bp.line_to((100.0, 0.0));
        bp.close_path();
        assert!(FrameOutline::new(&bp, 0.0).is_some(), "an edge through the middle of the box");
    }

    /// A slant a person can see still makes a shape.
    #[test]
    fn a_slanted_frame_is_a_shape() {
        assert!(FrameOutline::new(&quad(9.0, 10.0), 0.0).is_some(), "1 pt over 44 pt");
        assert!(FrameOutline::new(&quad(9.0, 9.5), 0.0).is_some(), "0.5 pt over 44 pt");
    }

    /// A frame `width` wide and `height` tall whose bottom edge runs `slant` lower on its right.
    fn tall(width: f64, height: f64, slant: f64) -> BezPath {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.line_to((0.0, height));
        bp.line_to((width, height + slant));
        bp.line_to((width, 0.0));
        bp.close_path();
        bp
    }

    /// The frame-wide slack is 0.3% of the larger side: a slant just under it on a wide, tall frame
    /// is square, one just over it is a shape.
    #[test]
    fn the_frame_wide_slack_is_a_third_of_a_percent_of_the_larger_side() {
        assert!(FrameOutline::new(&tall(100.0, 239.0, 0.70), 0.0).is_none(), "0.70 pt, under 0.3% of 239 pt");
        assert!(FrameOutline::new(&tall(100.0, 239.0, 0.75), 0.0).is_some(), "0.75 pt, over 0.3% of 239 pt");
    }

    /// An edge is also held to a fraction of its own length, so a tall frame does not take a visible
    /// slant on its short edge: 1.8 pt over 17 pt is 6 degrees, and 0.70 pt over 16.74 pt is within
    /// the frame-wide slack of a 239 pt frame but steeper than the measured frame's 0.54 pt.
    #[test]
    fn a_tall_narrow_frame_does_not_take_a_visible_slant_on_its_short_edge() {
        assert!(FrameOutline::new(&tall(17.0, 600.0, 1.8), 0.0).is_some(), "1.8 pt over 17 pt on a 600 pt frame");
        assert!(FrameOutline::new(&tall(16.74, 239.0, 0.70), 0.0).is_some(), "0.70 pt over 16.74 pt on a 239 pt frame");
        assert!(FrameOutline::new(&tall(16.74, 239.0, 0.54), 0.0).is_none(), "the measured frame: 0.54 pt over 16.74 pt");
    }

    /// The same frame as [`tall`] with the bottom edge split at `fraction` of its width by an extra
    /// anchor on the line (stored paths keep such anchors).
    fn tall_with_anchor(width: f64, height: f64, slant: f64, fraction: f64) -> BezPath {
        let mut bp = BezPath::new();
        bp.move_to((0.0, 0.0));
        bp.line_to((0.0, height));
        bp.line_to((width * fraction, height + slant * fraction));
        bp.line_to((width, height + slant));
        bp.line_to((width, 0.0));
        bp.close_path();
        bp
    }

    /// A straight side with an extra anchor on it is the same side: its halves are shorter edges,
    /// and an edge's length bounds only its slant, not how far it lies from the box. The measured
    /// frame split at its middle (or anywhere else) is still a rectangle, a steeper slant is still a
    /// shape however it is split.
    #[test]
    fn an_extra_anchor_on_a_side_does_not_change_whether_the_frame_is_a_rectangle() {
        for fraction in [0.5, 0.25, 0.9] {
            assert!(FrameOutline::new(&tall_with_anchor(16.74, 239.0, 0.54, fraction), 0.0).is_none(), "measured frame split at {fraction}");
            assert!(FrameOutline::new(&tall_with_anchor(16.74, 239.0, 0.70, fraction), 0.0).is_some(), "0.70 pt slant split at {fraction}");
            assert!(FrameOutline::new(&tall_with_anchor(17.0, 600.0, 1.8, fraction), 0.0).is_some(), "1.8 pt slant on 600 pt split at {fraction}");
        }
    }
}
