//! Live corners: the yellow widget on a selected rectangle frame and the four corner diamonds.
//!
//! Geometry works in the frame's local coordinates (the space its path and corner sizes live in)
//! and maps through the frame's canvas transform, so rotated and sheared frames work unchanged.

use designcraft_doc::{ItemId, Shape};
use designcraft_geom::corners::{Corner, CornerOptions, CornerShape};
use designcraft_geom::{Affine, Point, Vec2};
use serde_json::{Value, json};

use crate::ToolContext;

/// Screen distance of the widget below the frame's top-right corner, along the right edge.
const WIDGET_OFFSET_PX: f64 = 11.5;
/// Both sides of the frame must be at least this long on screen to show the widget.
const MIN_SIDE_PX: f64 = 40.0;
/// Diamonds stay at least this far (screen) from their corner, clear of the resize handles.
const DIAMOND_MIN_PX: f64 = 12.0;
/// Hit radius of the widget and the diamonds.
pub(crate) const HIT_PX: f64 = 5.0;
/// Corner size an Alt-click gives a corner that has none.
const DEFAULT_SIZE: f64 = 12.0;
/// Shapes an Alt-click steps through.
const CYCLE: [CornerShape; 5] = [CornerShape::Rounded, CornerShape::InverseRounded, CornerShape::Inset, CornerShape::Bevel, CornerShape::Fancy];

/// A selected rectangle frame that can show live corners.
#[derive(Clone, Debug)]
pub(crate) struct LiveFrame {
    pub id: ItemId,
    /// Local → canvas.
    pub m: Affine,
    /// Path corners in path order (top-left, top-right, bottom-right, bottom-left for a new rectangle).
    pub pts: [Point; 4],
    pub corners: CornerOptions,
}

/// The single selected, unlocked rectangle frame, when it is large enough on screen.
pub(crate) fn live_frame(cx: &ToolContext) -> Option<LiveFrame> {
    let [id] = cx.selection.items.as_slice() else { return None };
    if cx.selection.text.is_some() {
        return None;
    }
    let it = cx.doc.item(*id)?;
    let layer_locked = cx.doc.layer(it.layer).is_some_and(|l| l.locked);
    if !matches!(it.shape, Shape::Rectangle) || !it.children().is_empty() || it.locked || layer_locked {
        return None;
    }
    let [sp] = it.path.subpaths.as_slice() else { return None };
    let [a, b, c, d] = sp.anchors.as_slice() else { return None };
    if !sp.closed || sp.anchors.iter().any(|a| a.has_in() || a.has_out()) {
        return None;
    }
    let m = cx.item_canvas_xf(*id)? * it.xf;
    let det = m.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let pts = [a.p, b.p, c.p, d.p];
    let short = (0..4).map(|i| ((m * pts[i]) - (m * pts[(i + 1) % 4])).hypot()).fold(f64::INFINITY, f64::min);
    if short.is_nan() || short * cx.zoom < MIN_SIDE_PX {
        return None;
    }
    Some(LiveFrame { id: *id, m, pts, corners: it.corners })
}

impl LiveFrame {
    /// The other end of the edge corner `i` slides along: corners 0 and 1 share one edge, 2 and 3 the opposite one.
    fn neighbour(i: usize) -> usize {
        if i.is_multiple_of(2) { (i + 1) % 4 } else { (i + 3) % 4 }
    }

    /// Unit direction (local) from corner `i` along its edge, and that edge's local length.
    fn edge(&self, i: usize) -> Option<(Vec2, f64)> {
        let p = *self.pts.get(i)?;
        let q = *self.pts.get(Self::neighbour(i))?;
        let len = (q - p).hypot();
        (len.is_finite() && len > 1e-9).then(|| ((q - p) / len, len))
    }

    /// Largest corner size: half the shorter side.
    pub fn max_size(&self) -> f64 {
        (0..4).map(|i| (self.pts[(i + 1) % 4] - self.pts[i]).hypot()).fold(f64::INFINITY, f64::min) / 2.0
    }

    /// Corner `i`'s effective size (0 without a shape), clamped to the frame.
    pub fn size(&self, i: usize) -> f64 {
        self.corners.corners.get(i).filter(|c| !c.is_none()).map_or(0.0, |c| c.size.clamp(0.0, self.max_size().max(0.0)))
    }

    /// The widget's canvas position: on the right edge, a little below the top-right corner.
    pub fn widget(&self, zoom: f64) -> Point {
        let (mut x1, mut y0) = (f64::NEG_INFINITY, f64::INFINITY);
        for p in &self.pts {
            x1 = x1.max(p.x);
            y0 = y0.min(p.y);
        }
        let tr = self.m * Point::new(x1, y0);
        let down = (self.m * Point::new(x1, y0 + 1.0)) - tr;
        let len = down.hypot();
        if len.is_nan() || len <= 1e-12 {
            return tr;
        }
        tr + down / len * (WIDGET_OFFSET_PX / zoom.max(1e-9))
    }

    /// Canvas positions of the four diamonds: each at its corner size along its edge, but at least
    /// a few screen pixels from the corner.
    pub fn diamonds(&self, zoom: f64) -> Vec<Point> {
        (0..4)
            .filter_map(|i| {
                let (u, len) = self.edge(i)?;
                let p = self.pts[i];
                let per_unit = ((self.m * (p + u)) - (self.m * p)).hypot();
                let min = if per_unit > 1e-12 { DIAMOND_MIN_PX / zoom.max(1e-9) / per_unit } else { 0.0 };
                let d = self.size(i).max(min).min(len / 2.0);
                Some(self.m * (p + u * d))
            })
            .collect()
    }

    /// Index of the diamond under canvas point `p`.
    pub fn diamond_at(&self, p: Point, zoom: f64, tol: f64) -> Option<usize> {
        let ds = self.diamonds(zoom);
        if ds.len() != 4 {
            return None;
        }
        ds.iter().enumerate().map(|(i, d)| (i, (*d - p).hypot())).filter(|(_, dist)| *dist <= tol).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
    }

    /// Change of corner size when the pointer moves from `start` to `p` (canvas) while dragging
    /// corner `i`: the movement along that corner's edge, in local units.
    pub fn drag_delta(&self, i: usize, start: Point, p: Point) -> Option<f64> {
        let (u, _) = self.edge(i)?;
        let inv = self.m.inverse();
        let d = ((inv * p) - (inv * start)).dot(u);
        d.is_finite().then_some(d)
    }

    /// Options after dragging corner `i` to `size`: every corner takes that size, or only corner
    /// `i` with `only`. A corner without a shape becomes rounded.
    pub fn dragged(&self, i: usize, size: f64, only: bool) -> CornerOptions {
        let size = size.clamp(0.0, self.max_size().max(0.0));
        let mut out = self.corners;
        for (k, c) in out.corners.iter_mut().enumerate() {
            if only && k != i {
                continue;
            }
            if c.shape == CornerShape::None {
                c.shape = CornerShape::Rounded;
            }
            c.size = size;
        }
        out
    }

    /// Options after an Alt-click on corner `i`: that corner (or every corner, with `all`) takes
    /// the shape after corner `i`'s. A corner without a size gets a default one.
    pub fn cycled(&self, i: usize, all: bool) -> CornerOptions {
        let cur = self.corners.corners.get(i).map_or(CornerShape::None, |c| c.shape);
        let next = CYCLE.iter().position(|s| *s == cur).map_or(CornerShape::Rounded, |k| CYCLE[(k + 1) % CYCLE.len()]);
        let fallback = DEFAULT_SIZE.min(self.max_size().max(0.0));
        let mut out = self.corners;
        for (k, c) in out.corners.iter_mut().enumerate() {
            if !all && k != i {
                continue;
            }
            c.shape = next;
            if c.size.is_nan() || c.size <= 0.0 {
                c.size = fallback;
            }
        }
        out
    }
}

fn shape_id(s: CornerShape) -> &'static str {
    match s {
        CornerShape::None => "none",
        CornerShape::Rounded => "rounded",
        CornerShape::InverseRounded => "inverseRounded",
        CornerShape::Inset => "inset",
        CornerShape::Bevel => "bevel",
        CornerShape::Fancy => "fancy",
    }
}

/// `object.cornerOptions` parameters that set every corner of item `id`.
pub(crate) fn params(id: ItemId, opts: &CornerOptions) -> Value {
    let corners: Vec<Value> = opts.corners.iter().map(|c: &Corner| json!({"shape": shape_id(c.shape), "size": c.size})).collect();
    json!({"ids": [id.0], "corners": corners})
}
