//! Selection tool (V) and Direct Selection tool (A).
//!
//! Click selects the frontmost item (Shift toggles), drag moves (Alt duplicates, Shift constrains),
//! the 8 bounding-box handles resize (Shift keeps proportions, Alt from centre, Cmd scales content),
//! empty-canvas drags make a marquee, double-clicking a text frame switches to the Type tool and
//! double-clicking a handle fits the frame to its content (corner: both sides, edge: that one).
//! A click on a selected text frame's in or out port loads the text cursor (see [`crate::thread`]).
//!
//! Content (a placed graphic, or items pasted into a frame) moves inside its frame: the Direct
//! Selection tool selects and drags it, the Selection tool drags it by the content grabber (the
//! circle at the frame's centre), and double-clicking a frame toggles between frame and content.
//!
//! A selected rectangle frame shows the live-corner widget: clicking it shows a diamond at each
//! corner; dragging a diamond sets the corner size (Shift: that corner only), Alt-clicking one
//! steps its shape (Shift+Alt: every corner). Escape or clicking empty canvas ends corner editing.
//! A selected placed graphic shows its own bounding box: its handles resize the graphic (Shift keeps
//! its proportions, Alt from its centre) and leave the frame as it is.

use designcraft_doc::{Content, Item, ItemId, SpreadRef};
use designcraft_geom::{Affine, Point, Rect, Vec2};
use serde_json::{Value, json};

use crate::corners::{self, LiveFrame};
use crate::thread::Loaded;
use crate::{Action, Cursor, Gesture, Mods, Overlay, PointerEvent, PointerKind, SnapRequest, Tool, ToolContext, ToolKey, rect_json, spread_json};

#[derive(Clone, Debug)]
enum Drag {
    None,
    Pending {
        start: Point,
        hit: bool,
        /// The drag moves the selection's content inside its frames.
        content: bool,
    },
    Move {
        start: Point,
        origin_spread: SpreadRef,
        bounds0: Option<Rect>,
        exclude: Vec<ItemId>,
        content: bool,
    },
    Resize {
        handle: usize,
        start: Point,
        from: Rect,
        spread: SpreadRef,
    },
    /// Resizing a selected placed graphic by its own bounding-box handles (`from`: its bounds on
    /// the spread at pointer down).
    ContentResize {
        id: ItemId,
        handle: usize,
        from: Rect,
        spread: SpreadRef,
    },
    Rotate {
        center: Point,
        start_angle: f64,
        start_rotation: Option<f64>,
    },
    Marquee {
        start: Point,
        cur: Point,
    },
    /// A press with the loaded text cursor.
    Place {
        start: Point,
        cur: Point,
    },
    /// Dragging live-corner diamond `corner`. `frame` is read at pointer down: during the drag
    /// the document is the previous preview.
    Corner {
        frame: LiveFrame,
        corner: usize,
        start: Point,
    },
    /// Dragging a type-on-a-path bracket along its path (`end`: the end bracket).
    Bracket {
        id: ItemId,
        end: bool,
    },
    Anchor {
        id: u64,
        si: usize,
        ai: usize,
        handle: Option<&'static str>,
        start: Point,
        /// Spread and the anchor's spread position at pointer down.
        /// None when that point could not be read. The live document during a drag is the
        /// previous preview, so the snap must not read the anchor from it again.
        at: Option<(SpreadRef, Point)>,
    },
}

/// Half the length of a path-type bracket, in screen pixels.
const BRACKET_PX: f64 = 9.0;

/// A selected type-on-a-path item's path (inner space), its inner → canvas transform and its
/// brackets' arc lengths (start, end).
fn path_type_of(cx: &ToolContext, id: ItemId) -> Option<(designcraft_geom::warp::PathWarp, designcraft_geom::Affine, f64, f64)> {
    let it = cx.doc.item(id)?;
    let pt = it.text_frame()?.options.path.as_ref()?;
    let warp = designcraft_geom::warp::PathWarp::new(&it.path.to_bezpath(), pt.flip);
    let end = pt.end.unwrap_or(warp.length()).min(warp.length());
    Some((warp, cx.item_canvas_xf(id)? * it.xf, pt.start, end))
}

/// The bracket line at arc length `s`: across the path, `BRACKET_PX` either side (canvas).
fn bracket_line(cx: &ToolContext, warp: &designcraft_geom::warp::PathWarp, xf: designcraft_geom::Affine, s: f64) -> Option<(Point, Point)> {
    let (p, a) = warp.at(s)?;
    let c = xf * p;
    let t = xf * (p + designcraft_geom::Vec2::new(a.cos(), a.sin())) - c;
    let n = designcraft_geom::Vec2::new(-t.y, t.x);
    let n = n / n.hypot().max(1e-9) * cx.tol(BRACKET_PX);
    Some((c - n, c + n))
}

/// The bracket of a selected type-on-a-path item under `p` (canvas): (item, whether it is the end
/// bracket).
fn bracket_at(cx: &ToolContext, p: Point) -> Option<(ItemId, bool)> {
    let tol = cx.tol(BRACKET_PX);
    cx.selection.items.iter().find_map(|id| {
        let (warp, xf, start, end) = path_type_of(cx, *id)?;
        if cx.doc.item(*id).is_some_and(|it| it.locked) {
            return None;
        }
        let near = |s: f64| warp.at(s).is_some_and(|(q, _)| (xf * q - p).hypot() <= tol);
        // The end bracket first: with both brackets at one point, it can still be pulled back.
        if near(end) {
            Some((*id, true))
        } else if near(start) {
            Some((*id, false))
        } else {
            None
        }
    })
}

/// Anchor or handle of a selected item under `p` (canvas): (item, subpath, anchor, handle).
pub fn anchor_at(cx: &ToolContext, p: Point) -> Option<(u64, usize, usize, Option<&'static str>)> {
    anchor_at_in(cx, &cx.selection.items, p)
}

/// [`anchor_at`] over the given items.
pub fn anchor_at_in(cx: &ToolContext, ids: &[designcraft_doc::ItemId], p: Point) -> Option<(u64, usize, usize, Option<&'static str>)> {
    let tol = cx.tol(5.0);
    for id in ids {
        let (Some(it), Some(xf)) = (cx.doc.item(*id), cx.item_canvas_xf(*id)) else { continue };
        let m = xf * it.xf;
        for (si, sp) in it.path.subpaths.iter().enumerate() {
            for (ai, a) in sp.anchors.iter().enumerate() {
                if ((m * a.p) - p).hypot() <= tol {
                    return Some((id.0, si, ai, None));
                }
                if a.has_in() && ((m * a.h_in) - p).hypot() <= tol {
                    return Some((id.0, si, ai, Some("in")));
                }
                if a.has_out() && ((m * a.h_out) - p).hypot() <= tol {
                    return Some((id.0, si, ai, Some("out")));
                }
            }
        }
    }
    None
}

/// Spread holding the anchor, and the anchor in that spread's coordinates.
fn anchor_spread_point(cx: &ToolContext, id: u64, si: usize, ai: usize) -> Option<(SpreadRef, Point)> {
    let item_id = ItemId(id);
    let loc = cx.doc.find(item_id)?;
    let it = cx.doc.item(item_id)?;
    let local = it.path.subpaths.get(si)?.anchors.get(ai)?.p;
    Some((loc.spread, (cx.doc.parent_xf(&loc) * it.xf) * local))
}

/// Spread-space correction as a canvas delta. Translation cancels, leaving the linear part of `xf`.
fn spread_delta_to_canvas(xf: Affine, d: Vec2) -> Vec2 {
    (xf * Point::new(d.x, d.y)) - (xf * Point::ORIGIN)
}

pub struct SelectionTool {
    direct: bool,
    drag: Drag,
    hover_handle: Option<usize>,
    guides: Vec<Overlay>,
    /// Selected item to toggle if this Shift press never becomes a drag.
    shift_release: Option<u64>,
    /// The loaded text cursor.
    loaded: Option<Loaded>,
    /// Frame whose live corners are being edited (diamonds shown).
    live: Option<ItemId>,
}

impl SelectionTool {
    pub fn new(direct: bool) -> Self {
        Self { direct, drag: Drag::None, hover_handle: None, guides: vec![], shift_release: None, loaded: None, live: None }
    }

    /// [`bracket_at`] for this tool. With Direct Selection an anchor under the pointer wins: the
    /// default brackets sit on the path's end anchors.
    fn bracket_at(&self, cx: &ToolContext, p: Point) -> Option<(ItemId, bool)> {
        if self.direct && anchor_at(cx, p).is_some() {
            return None;
        }
        bracket_at(cx, p)
    }

    /// The frame in live-corner editing, while it is still the selection.
    fn live_corners(&self, cx: &ToolContext) -> Option<LiveFrame> {
        let id = self.live?;
        corners::live_frame(cx).filter(|f| f.id == id)
    }

    /// The frame whose live-corner widget is under `p` (not while editing its corners).
    fn widget_at(&self, cx: &ToolContext, p: Point) -> Option<LiveFrame> {
        if self.direct || self.live_corners(cx).is_some() {
            return None;
        }
        corners::live_frame(cx).filter(|f| (f.widget(cx.zoom) - p).hypot() <= cx.tol(corners::HIT_PX))
    }

    /// The frame and the index of the live-corner diamond under `p`.
    fn diamond_at(&self, cx: &ToolContext, p: Point) -> Option<(LiveFrame, usize)> {
        if self.direct {
            return None;
        }
        let f = self.live_corners(cx)?;
        let i = f.diamond_at(p, cx.zoom, cx.tol(corners::HIT_PX))?;
        Some((f, i))
    }

    /// Ports and the loaded text cursor. None: not a threading event.
    fn thread_pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Option<Vec<Action>> {
        let p = ev.pos;
        let exec = |c: Option<(&'static str, Value)>| c.map(|(id, params)| Action::Exec(id.into(), params)).into_iter().collect::<Vec<_>>();
        // A double click: sent as such, or the canvas's second press.
        if matches!(ev.kind, PointerKind::Down | PointerKind::DoubleClick)
            && ev.click_count() == 2
            && let Some((frame, out)) = crate::thread::port_at(cx, p)
        {
            self.loaded = None;
            self.drag = Drag::None;
            return Some(exec(crate::thread::unthread_at_port(cx.doc, frame, out)));
        }
        self.loaded = self.loaded.and_then(|l| l.resolve(cx.doc));
        match (ev.kind, self.loaded) {
            (PointerKind::Down | PointerKind::DoubleClick, _) if let Some((frame, out)) = crate::thread::port_at(cx, p) => {
                self.loaded = Some(Loaded::new(frame, out));
                self.drag = Drag::None;
                Some(vec![])
            }
            (PointerKind::Down | PointerKind::DoubleClick, Some(_)) => {
                self.drag = Drag::Place { start: p, cur: p };
                Some(vec![])
            }
            (PointerKind::Drag, Some(_)) => {
                if let Drag::Place { start, .. } = self.drag {
                    self.drag = Drag::Place { start, cur: p };
                }
                Some(vec![])
            }
            (PointerKind::Up, Some(l)) => {
                let Drag::Place { start, .. } = std::mem::replace(&mut self.drag, Drag::None) else { return Some(vec![]) };
                let clicked = l.click(cx, start, p, ev.mods);
                // Alt (semi-autoflow) stays loaded with what is left; an invalid target keeps the load.
                if let Some(c) = &clicked {
                    self.loaded = c.joins.filter(|_| ev.mods.alt && !ev.mods.shift).map(|j| Loaded { advance: Some(j), ..l });
                }
                Some(exec(clicked.map(|c| (c.id, c.params))))
            }
            (PointerKind::Move, Some(_)) => Some(vec![]),
            _ => None,
        }
    }
}

/// A frame whose content moves on its own: a placed graphic or items pasted into it.
pub fn holds_content(it: &Item) -> bool {
    !it.is_group() && (matches!(it.content, Content::Graphic(_)) || it.has_nested_items())
}

/// Grab radius of the content grabber, in screen pixels.
const GRABBER_PX: f64 = 9.0;

/// Canvas centre of the content grabber of `id` (the middle of its frame), when it has one. Frames
/// too small to drag around the grabber have none.
pub fn grabber_center(cx: &ToolContext, id: ItemId) -> Option<Point> {
    let it = cx.doc.item(id)?;
    if it.locked || !holds_content(it) {
        return None;
    }
    let b = cx.item_canvas_xf(id)?.transform_rect_bbox(it.bounds());
    let min = cx.tol(GRABBER_PX * 4.0);
    (b.width() >= min && b.height() >= min && b.center().x.is_finite() && b.center().y.is_finite()).then(|| b.center())
}

/// Content grabbers on show: the single selected frame's and the hovered frame's.
pub fn grabbers(cx: &ToolContext, hover: Option<Point>) -> Vec<(ItemId, Point)> {
    let mut ids = Vec::new();
    if let [one] = cx.selection.items.as_slice()
        && cx.selection.text.is_none()
    {
        ids.push(*one);
    }
    if let Some((_, id)) = hover.and_then(|p| cx.hit(p)) {
        let id = cx.doc.top_level_of(id).unwrap_or(id);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.into_iter().filter_map(|id| grabber_center(cx, id).map(|c| (id, c))).collect()
}

/// The frame whose content grabber is under `p` (canvas).
pub fn grabber_at(cx: &ToolContext, p: Point) -> Option<ItemId> {
    let r = cx.tol(GRABBER_PX);
    grabbers(cx, Some(p)).into_iter().find(|(_, c)| (*c - p).hypot() <= r).map(|(id, _)| id)
}

/// Is `p` on the selected content (a frame double-clicked into, or its grabber pressed)? A press
/// there with the Selection tool moves the content, as in InDesign, where the pointer turns into a
/// hand over it.
fn on_selected_content(cx: &ToolContext, p: Point) -> bool {
    cx.selection.content
        && cx.hit(p).is_some_and(|(_, id)| {
            let id = cx.doc.top_level_of(id).unwrap_or(id);
            cx.selection.contains(id) && cx.doc.item(id).is_some_and(holds_content)
        })
        && content_bounds(cx).is_some_and(|b| b.contains(p))
}

/// Canvas union of the content of the selected frames (an item without content counts whole).
fn content_bounds(cx: &ToolContext) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for id in &cx.selection.items {
        let (Some(it), Some(xf)) = (cx.doc.item(*id), cx.item_canvas_xf(*id)) else { continue };
        let b = match &it.content {
            Content::Graphic(g) => (xf * it.xf * g.xf).transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1)),
            _ if it.has_nested_items() => {
                let Some(r) = it.children().iter().map(|c| (xf * it.xf).transform_rect_bbox(c.bounds())).reduce(|a, b| a.union(b)) else { continue };
                r
            }
            _ => {
                let Some(local) = crate::snap::moving_bounds(it) else { continue };
                xf.transform_rect_bbox(local)
            }
        };
        if !(b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite()) {
            continue;
        }
        acc = Some(acc.map_or(b, |have| have.union(b)));
    }
    acc
}

/// Handle positions (canvas) of a rect: 0 TL, 1 T, 2 TR, 3 R, 4 BR, 5 B, 6 BL, 7 L.
pub fn handles(r: Rect) -> [Point; 8] {
    let c = r.center();
    [
        Point::new(r.x0, r.y0),
        Point::new(c.x, r.y0),
        Point::new(r.x1, r.y0),
        Point::new(r.x1, c.y),
        Point::new(r.x1, r.y1),
        Point::new(c.x, r.y1),
        Point::new(r.x0, r.y1),
        Point::new(r.x0, c.y),
    ]
}

/// The selected placed graphic (content selection of one graphic frame) and its bounds on the canvas.
fn selected_graphic(cx: &ToolContext) -> Option<(ItemId, Rect)> {
    let [id] = cx.selection.items.as_slice() else { return None };
    if !cx.selection.content || !matches!(cx.doc.item(*id)?.content, Content::Graphic(_)) {
        return None;
    }
    Some((*id, content_bounds(cx)?))
}

/// A handle of the selected placed graphic's bounding box under `p` (0 TL … 7 L).
fn content_handle_at(cx: &ToolContext, p: Point) -> Option<(ItemId, usize)> {
    let (id, b) = selected_graphic(cx)?;
    let tol = cx.tol(5.0);
    handles(b).iter().position(|h| (h.x - p.x).abs() <= tol && (h.y - p.y).abs() <= tol).map(|h| (id, h))
}

fn handle_at(cx: &ToolContext, p: Point) -> Option<usize> {
    // With a placed graphic selected, the handles are the graphic's, not the frame's.
    if selected_graphic(cx).is_some() {
        return None;
    }
    let b = cx.selection_bounds()?;
    let tol = cx.tol(5.0);
    handles(b).iter().position(|h| (h.x - p.x).abs() <= tol && (h.y - p.y).abs() <= tol)
}

/// Side handles are drawn on the canvas bounding box; fitting uses the frame's local axes.
fn fitting_axis(cx: &ToolContext, handle: usize) -> &'static str {
    if handle.is_multiple_of(2) {
        return "both";
    }
    let canvas_height = matches!(handle, 1 | 5);
    let local_width_is_vertical = cx
        .selection
        .items
        .first()
        .and_then(|id| {
            let it = cx.doc.item(*id)?;
            let xf = cx.item_canvas_xf(*id)? * it.xf;
            let [a, b, _, _, _, _] = xf.as_coeffs();
            Some(b.abs() > a.abs())
        })
        .unwrap_or(false);
    if canvas_height == local_width_is_vertical { "width" } else { "height" }
}

/// Just outside a corner handle of the selection → rotate.
fn rotate_zone(cx: &ToolContext, p: Point) -> Option<Point> {
    let b = cx.selection_bounds()?;
    let inner = cx.tol(5.0);
    let outer = cx.tol(18.0);
    for c in [Point::new(b.x0, b.y0), Point::new(b.x1, b.y0), Point::new(b.x1, b.y1), Point::new(b.x0, b.y1)] {
        let d = (c - p).hypot();
        let outside = p.x < b.x0 || p.x > b.x1 || p.y < b.y0 || p.y > b.y1;
        if d > inner && d <= outer && outside {
            return Some(b.center());
        }
    }
    None
}

/// New rect when dragging `handle` of `from` to `p`.
pub fn resize_rect(from: Rect, handle: usize, p: Point, m: Mods) -> Rect {
    finish_resize(from, handle, resize_edges_rect(from, handle, p), m)
}

/// Edges the pointer sets, before Shift or Alt.
fn resize_edges_rect(from: Rect, handle: usize, p: Point) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (from.x0, from.y0, from.x1, from.y1);
    match handle {
        0 => {
            x0 = p.x;
            y0 = p.y;
        }
        1 => y0 = p.y,
        2 => {
            x1 = p.x;
            y0 = p.y;
        }
        3 => x1 = p.x,
        4 => {
            x1 = p.x;
            y1 = p.y;
        }
        5 => y1 = p.y,
        6 => {
            x0 = p.x;
            y1 = p.y;
        }
        _ => x0 = p.x,
    }
    Rect::new(x0, y0, x1, y1)
}

fn finish_resize(from: Rect, handle: usize, rect: Rect, m: Mods) -> Rect {
    // Alt first: Shift then measures the drag from the centre and keeps it there.
    let rect = if m.alt { apply_alt(from, handle, rect) } else { rect };
    if !(m.shift && from.width() > 1e-9 && from.height() > 1e-9) {
        return rect;
    }
    let s = aspect_scale(from, handle, rect);
    if m.alt { centered_rect(from, from.width() * s, from.height() * s) } else { apply_aspect(from, handle, rect, s) }
}

fn centered_rect(from: Rect, w: f64, h: f64) -> Rect {
    let c = from.center();
    Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0)
}

fn aspect_scale(from: Rect, handle: usize, rect: Rect) -> f64 {
    let sx = (rect.x1 - rect.x0) / from.width();
    let sy = (rect.y1 - rect.y0) / from.height();
    if matches!(handle, 1 | 5) {
        sy
    } else if matches!(handle, 3 | 7) || sx.abs() > sy.abs() {
        sx
    } else {
        sy
    }
}

/// True when the width, not the height, drives a Shift constraint.
fn drives_x(handle: usize, from: Rect, rect: Rect) -> bool {
    if matches!(handle, 1 | 5) {
        return false;
    }
    if matches!(handle, 3 | 7) {
        return true;
    }
    if from.width() <= 1e-9 || from.height() <= 1e-9 {
        return true;
    }
    let sx = ((rect.x1 - rect.x0) / from.width()).abs();
    let sy = ((rect.y1 - rect.y0) / from.height()).abs();
    sx > sy
}

/// Scales `from` by `s` with the edges opposite the handle fixed. An axis the handle doesn't move stays centred.
fn apply_aspect(from: Rect, handle: usize, rect: Rect, s: f64) -> Rect {
    let (x_edges, y_edges) = resize_edge_flags(handle);
    let span = |edges: [bool; 3], lo: f64, hi: f64, mid: f64, len: f64| match edges {
        [true, _, _] => (hi - len, hi),
        [_, _, true] => (lo, lo + len),
        _ => (mid - len / 2.0, mid + len / 2.0),
    };
    let c = from.center();
    let (x0, x1) = span(x_edges, rect.x0, rect.x1, c.x, from.width() * s);
    let (y0, y1) = span(y_edges, rect.y0, rect.y1, c.y, from.height() * s);
    Rect::new(x0, y0, x1, y1)
}

/// Snap changes the size. Shift then restores the aspect, and Alt places that size on the original center.
///
/// Nudging one edge of an already centered rect walks the center. Mirroring that nudge doubles a length match.
fn finish_snapped_resize(
    from: Rect,
    handle: usize,
    proposed: Rect,
    mods: Mods,
    driver_x: bool,
    x_edges: [bool; 3],
    y_edges: [bool; 3],
    hit: &crate::Snap,
) -> Rect {
    if !mods.alt {
        let mut to = crate::snap::nudge_edges(proposed, x_edges, y_edges, hit.delta);
        if mods.shift && from.width() > 1e-9 && from.height() > 1e-9 {
            let s = if driver_x { (to.x1 - to.x0) / from.width() } else { (to.y1 - to.y0) / from.height() };
            if s.is_finite() {
                to = apply_aspect(from, handle, to, s);
            }
        }
        return to;
    }
    let mut w = proposed.x1 - proposed.x0;
    let mut h = proposed.y1 - proposed.y0;
    w += centered_size_delta(x_edges, hit.delta.x, hit.length_delta[0]);
    h += centered_size_delta(y_edges, hit.delta.y, hit.length_delta[1]);
    if mods.shift && from.width() > 1e-9 && from.height() > 1e-9 {
        if driver_x {
            let s = w / from.width();
            if s.is_finite() {
                h = from.height() * s;
            }
        } else {
            let s = h / from.height();
            if s.is_finite() {
                w = from.width() * s;
            }
        }
    }
    if !(w.is_finite() && h.is_finite()) {
        return proposed;
    }
    centered_rect(from, w, h)
}

/// Length match: add the difference once. Position match: the flagged edge moves and the other side mirrors.
fn centered_size_delta(edges: [bool; 3], delta: f64, length_delta: Option<f64>) -> f64 {
    if let Some(diff) = length_delta.filter(|d| d.is_finite()) {
        return diff;
    }
    let edge = axis_size_delta(edges, delta);
    if edge.is_finite() { edge * 2.0 } else { 0.0 }
}

/// Signed change in `hi - lo` when one flagged edge moves by `delta`.
fn axis_size_delta(edges: [bool; 3], delta: f64) -> f64 {
    if !delta.is_finite() {
        return 0.0;
    }
    let [low_edge, _, high_edge] = edges;
    if high_edge && !low_edge {
        delta
    } else if low_edge && !high_edge {
        -delta
    } else {
        0.0
    }
}

/// Mirrors the dragged sides around the centre of `from`. Shrinking and crossing the centre follow the pointer.
fn apply_alt(from: Rect, handle: usize, rect: Rect) -> Rect {
    let (x_edges, y_edges) = resize_edge_flags(handle);
    let c = from.center();
    let size = |edges: [bool; 3], lo: f64, hi: f64, mid: f64, len: f64| match edges {
        [true, _, _] => 2.0 * (mid - lo),
        [_, _, true] => 2.0 * (hi - mid),
        _ => len,
    };
    let w = size(x_edges, rect.x0, rect.x1, c.x, from.width());
    let h = size(y_edges, rect.y0, rect.y1, c.y, from.height());
    centered_rect(from, w, h)
}

/// Left/center/right and top/center/bottom for a handle. Centers stay off.
fn resize_edge_flags(handle: usize) -> ([bool; 3], [bool; 3]) {
    let x = match handle {
        0 | 6 | 7 => [true, false, false],
        2..=4 => [false, false, true],
        _ => [false, false, false],
    };
    let y = match handle {
        0..=2 => [true, false, false],
        4..=6 => [false, false, true],
        _ => [false, false, false],
    };
    (x, y)
}

fn axis_length(edges: [bool; 3], size: f64, pad: f64) -> Option<f64> {
    if edges == [false, false, false] {
        return None;
    }
    let len = size.abs() + pad;
    len.is_finite().then_some(len)
}

/// Visible size minus path size, for one selected item. Centered stroke then matches the outer edge.
fn single_stroke_pad(cx: &ToolContext) -> (f64, f64) {
    if cx.selection.items.len() != 1 {
        return (0.0, 0.0);
    }
    let Some(id) = cx.selection.items.first() else { return (0.0, 0.0) };
    let Some(it) = cx.doc.item(*id) else { return (0.0, 0.0) };
    let path = it.bounds();
    let vis = it.visible_bounds();
    (positive_pad(vis.width() - path.width()), positive_pad(vis.height() - path.height()))
}

fn positive_pad(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 { v } else { 0.0 }
}

impl Tool for SelectionTool {
    fn id(&self) -> &'static str {
        if self.direct { "directSelection" } else { "selection" }
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let p = ev.pos;
        // Selecting something else ends live-corner editing.
        if let Some(id) = self.live
            && cx.selection.items != [id]
        {
            self.live = None;
        }
        if !self.direct
            && let Some(actions) = self.thread_pointer(cx, ev)
        {
            return actions;
        }
        // The second press of a double click on a selection handle: Fit Frame to Content, along
        // that handle's side (a corner fits both).
        if !self.direct
            && matches!(ev.kind, PointerKind::Down | PointerKind::DoubleClick)
            && ev.click_count() == 2
            && let Some(h) = handle_at(cx, p)
        {
            self.drag = Drag::None;
            return vec![Action::Exec("object.fit".into(), json!({"mode": "fitFrameToContent", "axis": fitting_axis(cx, h)}))];
        }
        // The second press of a double click on a text frame: edit its text.
        if matches!(ev.kind, PointerKind::Down | PointerKind::DoubleClick)
            && ev.click_count() == 2
            && let Some(actions) = edit_text_at(cx, p)
        {
            self.drag = Drag::None;
            return actions;
        }
        // The second press of a double click on a frame with content: toggle between the frame and
        // its content.
        if !self.direct
            && matches!(ev.kind, PointerKind::Down | PointerKind::DoubleClick)
            && ev.click_count() == 2
            && let Some(id) = cx.hit(p).map(|(_, id)| cx.doc.top_level_of(id).unwrap_or(id))
            && cx.doc.item(id).is_some_and(holds_content)
        {
            self.drag = Drag::None;
            let content = !(cx.selection.content && cx.selection.contains(id));
            return vec![Action::Exec("selection.set".into(), json!({"ids": [id.0], "content": content}))];
        }
        match ev.kind {
            PointerKind::Move => {
                self.hover_handle = handle_at(cx, p);
                vec![]
            }
            PointerKind::Down => {
                self.shift_release = None;
                // Type on a path: a bracket moves where the text starts or ends.
                if !ev.mods.shift
                    && let Some((id, end)) = self.bracket_at(cx, p)
                {
                    self.drag = Drag::Bracket { id, end };
                    return vec![Action::Begin("Move Bracket".into())];
                }
                if self.direct
                    && let Some((id, si, ai, handle)) = anchor_at(cx, p)
                {
                    let at = anchor_spread_point(cx, id, si, ai);
                    self.drag = Drag::Anchor { id, si, ai, handle, start: p, at };
                    return vec![Action::Begin(if handle.is_some() { "Move Direction Handle".into() } else { "Move Anchor".into() })];
                }
                // A selected placed graphic's own handles resize it inside its frame (the
                // frame's anchors, under Direct Selection, come first).
                if let Some((id, h)) = content_handle_at(cx, p)
                    && let (Some(loc), Some((_, b))) = (cx.doc.find(id), selected_graphic(cx))
                {
                    let turns = cx.layout.slot(loc.spread).map_or(0, |s| s.rotation as usize);
                    let handle = (h + 8 - 2 * turns) % 8;
                    let from = cx.layout.xf(loc.spread).inverse().transform_rect_bbox(b);
                    self.drag = Drag::ContentResize { id, handle, from, spread: loc.spread };
                    return vec![Action::Begin("Resize Content".into())];
                }
                if let Some((frame, corner)) = self.diamond_at(cx, p) {
                    if ev.mods.alt {
                        self.drag = Drag::None;
                        let opts = frame.cycled(corner, ev.mods.shift);
                        return vec![Action::Exec("object.cornerOptions".into(), corners::params(frame.id, &opts))];
                    }
                    self.drag = Drag::Corner { frame, corner, start: p };
                    return vec![Action::Begin("Corner Options".into())];
                }
                if let Some(frame) = self.widget_at(cx, p) {
                    self.live = Some(frame.id);
                    self.drag = Drag::None;
                    return vec![Action::Handled];
                }
                if !self.direct
                    && handle_at(cx, p).is_none()
                    && let Some(center) = rotate_zone(cx, p)
                {
                    self.drag = Drag::Rotate { center, start_angle: (p - center).atan2(), start_rotation: crate::snap::reference_rotation(cx) };
                    return vec![Action::Begin("Rotate".into())];
                }
                if let Some(h) = handle_at(cx, p).filter(|_| !self.direct)
                    && let (Some(b), Some(first)) = (cx.selection_bounds(), cx.selection.items.first())
                    && let Some(loc) = cx.doc.find(*first)
                {
                    // On a turned spread the canvas handle is another one of the spread's.
                    let turns = cx.layout.slot(loc.spread).map_or(0, |s| s.rotation as usize);
                    let handle = (h + 8 - 2 * turns) % 8;
                    self.drag =
                        Drag::Resize { handle, start: p, from: cx.layout.xf(loc.spread).inverse().transform_rect_bbox(b), spread: loc.spread };
                    return vec![Action::Begin("Resize".into())];
                }
                // The content grabber: select the frame's content; a drag moves it.
                if !self.direct
                    && !ev.mods.shift
                    && let Some(id) = grabber_at(cx, p)
                {
                    self.drag = Drag::Pending { start: p, hit: true, content: true };
                    if cx.selection.items == [id] && cx.selection.content {
                        return vec![];
                    }
                    return vec![Action::Exec("selection.set".into(), json!({"ids": [id.0], "content": true}))];
                }
                // Cmd+Shift-click a parent item on a page: override it there and select the copy.
                if ev.mods.cmd
                    && ev.mods.shift
                    && cx.hit(p).is_none()
                    && let Some((page, id)) = cx.hit_parent_on_page(p)
                {
                    self.drag = Drag::Pending { start: p, hit: true, content: false };
                    return vec![Action::Exec("layout.overrideParentItems".into(), json!({"page": page, "ids": [id.0]}))];
                }
                match cx.hit(p) {
                    Some((sr, id)) => {
                        let id = if self.direct { id } else { cx.doc.top_level_of(id).unwrap_or(id) };
                        let mut out = vec![];
                        if ev.mods.shift {
                            if cx.selection.contains(id) {
                                // Keep it selected so a Shift-drag can constrain. A click still toggles, on release.
                                self.shift_release = Some(id.0);
                            } else {
                                out.push(Action::Exec("selection.toggle".into(), json!({"id": id.0})));
                            }
                        } else if !cx.selection.contains(id) || (self.direct && !cx.selection.content) {
                            out.push(Action::Exec("selection.set".into(), json!({"ids": [id.0], "content": self.direct})));
                        }
                        let content = self.direct || on_selected_content(cx, p);
                        self.drag = Drag::Pending { start: p, hit: true, content };
                        let _ = sr;
                        out
                    }
                    None => {
                        self.live = None;
                        self.drag = Drag::Pending { start: p, hit: false, content: false };
                        if ev.mods.shift { vec![] } else { vec![Action::Exec("selection.set".into(), json!({"ids": []}))] }
                    }
                }
            }
            PointerKind::Drag => match self.drag.clone() {
                Drag::Bracket { id, end } => {
                    let Some((warp, xf, _, _)) = path_type_of(cx, id) else { return vec![] };
                    if xf.determinant().abs() < 1e-12 {
                        return vec![];
                    }
                    let s = warp.nearest(xf.inverse() * p);
                    vec![Action::Preview("type.pathOptions".into(), json!({"id": id.0, if end { "end" } else { "start" }: s}))]
                }
                Drag::Pending { start, hit, content } => {
                    if (p - start).hypot() < cx.tol(3.0) {
                        return vec![];
                    }
                    self.shift_release = None;
                    if hit && !cx.selection.items.is_empty() {
                        // Direct Selection on items without content moves (or duplicates) them whole.
                        let content = content && cx.selection.items.iter().any(|i| cx.doc.item(*i).is_some_and(holds_content));
                        let sr = cx.selection.items.first().and_then(|i| cx.doc.find(*i)).map(|l| l.spread).unwrap_or(SpreadRef::Doc(0));
                        let exclude = cx.selection.items.clone();
                        let bounds0 = if content { content_bounds(cx) } else { cx.selection_visible_bounds() };
                        self.drag = Drag::Move { start, origin_spread: sr, bounds0, exclude: exclude.clone(), content };
                        let mut v = vec![];
                        // Dragging a content selection by its frame moves the frame: select it.
                        if !content && cx.selection.content {
                            let ids: Vec<u64> = exclude.iter().map(|i| i.0).collect();
                            v.push(Action::Exec("selection.set".into(), json!({"ids": ids})));
                        }
                        let label = if content {
                            "Move Content"
                        } else if ev.mods.alt {
                            "Duplicate"
                        } else {
                            "Move"
                        };
                        v.push(Action::Begin(label.into()));
                        v.extend(self.pointer(cx, ev));
                        v
                    } else {
                        self.drag = Drag::Marquee { start, cur: p };
                        vec![]
                    }
                }
                Drag::Move { start, origin_spread, bounds0, exclude, content } => {
                    let mut d: Vec2 = p - start;
                    let mut screen_x_locked = false;
                    let mut screen_y_locked = false;
                    if ev.mods.shift {
                        if d.x.abs() > d.y.abs() {
                            d.y = 0.0;
                            screen_y_locked = true;
                        } else {
                            d.x = 0.0;
                            screen_x_locked = true;
                        }
                    }
                    // Snap on the spread under the pointer. Shift uses that spread's view rotation.
                    // Content stays in its frame, on the frame's spread.
                    let target = if content { origin_spread } else { cx.layout.spread_at(p).map(|(s, _)| s).unwrap_or(origin_spread) };
                    // 180 degrees keeps the screen axis. 90 and 270 swap it into the other spread axis.
                    let odd_turn = cx.layout.slot(target).is_some_and(|slot| slot.rotation % 2 == 1);
                    let spread_x_locked = if odd_turn { screen_y_locked } else { screen_x_locked };
                    let spread_y_locked = if odd_turn { screen_x_locked } else { screen_y_locked };
                    let x_edges = if spread_x_locked { [false; 3] } else { [true; 3] };
                    let y_edges = if spread_y_locked { [false; 3] } else { [true; 3] };
                    self.guides.clear();
                    // The movement in the origin spread's coordinates (its view may be turned).
                    let mut ds = cx.layout.delta_to_spread(origin_spread, d);
                    let b0s = bounds0.map(|b| cx.layout.xf(origin_spread).inverse().transform_rect_bbox(b));
                    let q0 = b0s.map_or(Point::ORIGIN, |b| Point::new(b.x0, b.y0));
                    // Same corner conversion transform.move already uses when the spread changes.
                    let mut landed = cx.layout.to_spread(target, cx.layout.to_canvas(origin_spread, q0 + ds));
                    // Command suspends snapping. A Shift-locked spread axis is not offered.
                    if !ev.mods.cmd
                        && cx.snap.any()
                        && let Some(b0) = b0s
                    {
                        let proposed = if target == origin_spread {
                            b0 + ds
                        } else {
                            let shift = landed - q0;
                            Rect::new(b0.x0 + shift.x, b0.y0 + shift.y, b0.x1 + shift.x, b0.y1 + shift.y)
                        };
                        let hit = crate::snap::snap(
                            cx,
                            SnapRequest {
                                spread: target,
                                gesture: Gesture::Move,
                                rect: proposed,
                                x_edges,
                                y_edges,
                                exclude: &exclude,
                                copying: ev.mods.alt && !content,
                                lengths: [None, None],
                                angle: None,
                                radius: 0.0,
                                pointer: cx.layout.to_spread(target, p),
                            },
                        );
                        if target == origin_spread {
                            if x_edges != [false, false, false] && hit.delta.x.is_finite() {
                                ds.x += hit.delta.x;
                            }
                            if y_edges != [false, false, false] && hit.delta.y.is_finite() {
                                ds.y += hit.delta.y;
                            }
                        } else {
                            if x_edges != [false, false, false] && hit.delta.x.is_finite() {
                                landed.x += hit.delta.x;
                            }
                            if y_edges != [false, false, false] && hit.delta.y.is_finite() {
                                landed.y += hit.delta.y;
                            }
                        }
                        self.guides = hit.guides;
                    }
                    // Same spread: dx, dy are the origin spread translation. Crossing: the corner
                    // conversion, so the reparented item lands on the snapped target point.
                    let (dx, dy) = if target == origin_spread {
                        (ds.x, ds.y)
                    } else {
                        let dt = landed - q0;
                        (dt.x, dt.y)
                    };
                    if content {
                        let ids: Vec<u64> = exclude.iter().map(|i| i.0).collect();
                        return vec![Action::Preview("transform.move".into(), json!({"dx": dx, "dy": dy, "ids": ids, "content": true}))];
                    }
                    let mut params = json!({"dx": dx, "dy": dy, "copy": ev.mods.alt});
                    if target != origin_spread {
                        params = json!({"dx": dx, "dy": dy, "copy": ev.mods.alt, "toSpread": spread_json(target)});
                    }
                    vec![Action::Preview("transform.move".into(), params)]
                }
                Drag::Resize { handle, start, from, spread } => {
                    let _ = start;
                    let pointer = cx.layout.to_spread(spread, p);
                    let raw = resize_edges_rect(from, handle, pointer);
                    let mut to = resize_rect(from, handle, pointer, ev.mods);
                    // Space while dragging: Live Distribute (several objects keep their size).
                    let distribute = ev.mods.space && cx.selection.items.len() > 1;
                    self.guides.clear();
                    // Command still scales content. It does not skip the snap.
                    if !distribute && cx.snap.any() {
                        let (mut x_edges, mut y_edges) = resize_edge_flags(handle);
                        let driver_x = drives_x(handle, from, raw);
                        if ev.mods.shift {
                            if driver_x {
                                y_edges = [false, false, false];
                            } else {
                                x_edges = [false, false, false];
                            }
                        }
                        let exclude = cx.selection.items.clone();
                        let (pad_x, pad_y) = single_stroke_pad(cx);
                        let proposed = to;
                        let mut hit = crate::snap::snap(
                            cx,
                            SnapRequest {
                                spread,
                                gesture: Gesture::Resize,
                                rect: proposed,
                                x_edges,
                                y_edges,
                                exclude: &exclude,
                                copying: false,
                                lengths: [axis_length(x_edges, proposed.width(), pad_x), axis_length(y_edges, proposed.height(), pad_y)],
                                angle: None,
                                radius: 0.0,
                                pointer,
                            },
                        );
                        to = finish_snapped_resize(from, handle, proposed, ev.mods, driver_x, x_edges, y_edges, &hit);
                        crate::snap::lay_dimension_guides(&mut hit.guides, cx.layout.xf(spread), proposed, to, x_edges, y_edges, hit.length_delta);
                        self.guides = hit.guides;
                    }
                    vec![Action::Preview(
                        "transform.resize".into(),
                        json!({"from": rect_json(from), "to": rect_json(to), "content": ev.mods.cmd, "distribute": distribute}),
                    )]
                }
                Drag::ContentResize { id, handle, from, spread } => {
                    let to = resize_rect(from, handle, cx.layout.to_spread(spread, p), ev.mods);
                    vec![Action::Preview("transform.set".into(), json!({"ids": [id.0], "content": true, "bounds": rect_json(to)}))]
                }
                Drag::Marquee { start, .. } => {
                    self.drag = Drag::Marquee { start, cur: p };
                    vec![]
                }
                Drag::Place { .. } => vec![],
                Drag::Corner { frame, corner, start } => {
                    let Some(d) = frame.drag_delta(corner, start, p) else { return vec![] };
                    let opts = frame.dragged(corner, frame.size(corner) + d, ev.mods.shift);
                    vec![Action::Preview("object.cornerOptions".into(), corners::params(frame.id, &opts))]
                }
                Drag::Rotate { center, start_angle, start_rotation } => {
                    let mut a = ((p - center).atan2() - start_angle).to_degrees();
                    self.guides.clear();
                    if ev.mods.shift {
                        // 45 degree steps. The rotation pass does not run while Shift is held.
                        a = (a / 45.0).round() * 45.0;
                    } else if let Some(start) = start_rotation {
                        let raw = -a;
                        if let Some(resulting) = crate::snap::folded_rotation(start + raw) {
                            let spread =
                                cx.selection.items.first().and_then(|id| cx.doc.find(*id)).map(|loc| loc.spread).unwrap_or(SpreadRef::Doc(0));
                            let exclude = cx.selection.items.clone();
                            // The rect is unused by the angle pass. It only has to be finite.
                            let hit = crate::snap::snap(
                                cx,
                                SnapRequest {
                                    spread,
                                    gesture: Gesture::Rotate,
                                    rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                                    x_edges: [false, false, false],
                                    y_edges: [false, false, false],
                                    exclude: &exclude,
                                    copying: false,
                                    lengths: [None, None],
                                    angle: Some(resulting),
                                    radius: (p - center).hypot(),
                                    pointer: cx.layout.to_spread(spread, p),
                                },
                            );
                            if let Some(snapped) = hit.angle
                                && let Some(delta) = crate::snap::rotation_command_delta(start, snapped, raw)
                            {
                                a = -delta;
                            }
                            self.guides = hit.guides;
                        }
                    }
                    // Screen y points down: a positive screen angle is clockwise.
                    vec![Action::Preview("transform.rotate".into(), json!({"angle": -a}))]
                }
                Drag::Anchor { id, si, ai, handle, start, at } => {
                    let d = p - start;
                    let mut dx = d.x;
                    let mut dy = d.y;
                    self.guides.clear();
                    // Direction handles do not snap. An anchor excludes the path it is on.
                    if handle.is_none()
                        && let Some((spread, origin)) = at
                    {
                        let proposed = origin + cx.layout.delta_to_spread(spread, d);
                        let exclude = [ItemId(id)];
                        let hit = crate::snap::snap(
                            cx,
                            SnapRequest {
                                spread,
                                gesture: Gesture::Point,
                                rect: Rect::from_points(proposed, proposed),
                                x_edges: [true, true, true],
                                y_edges: [true, true, true],
                                exclude: &exclude,
                                copying: false,
                                lengths: [None, None],
                                angle: None,
                                radius: 0.0,
                                pointer: proposed,
                            },
                        );
                        // dx and dy stay in canvas space. Snap.delta is spread space.
                        let corr = spread_delta_to_canvas(cx.layout.xf(spread), hit.delta);
                        if corr.x.is_finite() {
                            dx += corr.x;
                        }
                        if corr.y.is_finite() {
                            dy += corr.y;
                        }
                        self.guides = hit.guides;
                    }
                    let mut params = json!({"id": id, "anchors": [[si, ai]], "dx": dx, "dy": dy});
                    if let Some(h) = handle {
                        params["handle"] = json!(h);
                    }
                    vec![Action::Preview("path.moveAnchors".into(), params)]
                }
                Drag::None => vec![],
            },
            PointerKind::Up => {
                self.guides.clear();
                let release = self.shift_release.take();
                let d = std::mem::replace(&mut self.drag, Drag::None);
                match d {
                    Drag::Move { .. }
                    | Drag::Resize { .. }
                    | Drag::ContentResize { .. }
                    | Drag::Anchor { .. }
                    | Drag::Rotate { .. }
                    | Drag::Corner { .. }
                    | Drag::Bracket { .. } => {
                        vec![Action::Commit]
                    }
                    Drag::Pending { .. } => match release {
                        Some(id) => vec![Action::Exec("selection.toggle".into(), json!({"id": id}))],
                        None => vec![],
                    },
                    Drag::Marquee { start, cur } => {
                        let r = Rect::from_points(start, cur);
                        // Items whose bounds intersect the marquee.
                        let mut ids: Vec<Value> = Vec::new();
                        for slot in &cx.layout.slots {
                            let Some(sp) = cx.doc.spread(slot.spread) else { continue };
                            for it in &sp.items {
                                let b = slot.xf.transform_rect_bbox(it.bounds());
                                let locked = it.locked || cx.doc.layer(it.layer).is_some_and(|l| l.locked || !l.visible);
                                if !locked && !it.hidden && b.x0 < r.x1 && b.x1 > r.x0 && b.y0 < r.y1 && b.y1 > r.y0 {
                                    ids.push(Value::from(it.id.0));
                                }
                            }
                        }
                        vec![Action::Exec("selection.set".into(), json!({"ids": ids, "add": ev.mods.shift}))]
                    }
                    Drag::None | Drag::Place { .. } => vec![],
                }
            }
            PointerKind::DoubleClick => vec![],
        }
    }

    fn key(&mut self, cx: &ToolContext, key: ToolKey, mods: Mods) -> Vec<Action> {
        if key == ToolKey::Escape && (self.loaded.take().is_some() || !matches!(&self.drag, Drag::None)) {
            // Restore the interaction snapshot before releasing a drag; a later mouse-up
            // must not commit it. Idle Escape still deselects below.
            self.drag = Drag::None;
            self.guides.clear();
            self.shift_release = None;
            return vec![Action::Cancel];
        }
        let inc = cx.doc.settings.keyboard_increment * if mods.shift { 10.0 } else { 1.0 };
        if key == ToolKey::Escape && self.live.take().is_some_and(|id| cx.selection.items == [id]) {
            return vec![Action::Handled];
        }
        let mv = |dx: f64, dy: f64| vec![Action::Exec("transform.move".into(), json!({"dx": dx, "dy": dy, "copy": mods.alt}))];
        if cx.selection.items.is_empty() {
            return vec![];
        }
        match key {
            ToolKey::Left => mv(-inc, 0.0),
            ToolKey::Right => mv(inc, 0.0),
            ToolKey::Up => mv(0.0, -inc),
            ToolKey::Down => mv(0.0, inc),
            ToolKey::Delete | ToolKey::Backspace => vec![Action::Exec("edit.clear".into(), json!({}))],
            ToolKey::Escape => vec![Action::Exec("selection.set".into(), json!({"ids": []}))],
            _ => vec![],
        }
    }

    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let mut out = match &self.drag {
            Drag::Place { start, cur } if crate::thread::is_drag(cx, *start, *cur) => {
                let Some((sr, r, guides)) = crate::thread::drag_frame(cx, *start, *cur) else { return vec![] };
                let r = Rect::from_points(cx.layout.to_canvas(sr, Point::new(r.x0, r.y0)), cx.layout.to_canvas(sr, Point::new(r.x1, r.y1)));
                std::iter::once(Overlay::Marquee(r)).chain(guides).collect()
            }
            Drag::Marquee { start, cur } => vec![Overlay::Marquee(Rect::from_points(*start, *cur))],
            Drag::Move { content: true, exclude, .. } => {
                let mut v: Vec<Overlay> = exclude.iter().map(|id| Overlay::ContentGhost(*id)).collect();
                v.extend(self.guides.iter().cloned());
                v
            }
            Drag::Move { .. } | Drag::Resize { .. } | Drag::Rotate { .. } | Drag::Anchor { .. } => self.guides.clone(),
            // The whole graphic shows while it's resized, outside the frame too.
            Drag::ContentResize { id, .. } => vec![Overlay::ContentGhost(*id)],
            Drag::None | Drag::Bracket { .. } => brackets(cx),
            _ => vec![],
        };
        // The loaded text cursor places text: the frame's corners aren't edited meanwhile.
        if !self.direct && self.loaded.and_then(|l| l.resolve(cx.doc)).is_none() {
            if let Some(f) = self.live_corners(cx) {
                out.extend(f.diamonds(cx.zoom).into_iter().map(Overlay::LiveCornerDiamond));
            } else if let Some(f) = corners::live_frame(cx) {
                out.push(Overlay::LiveCornerWidget(f.widget(cx.zoom)));
            }
        }
        out
    }

    fn cursor(&self, cx: &ToolContext, p: Point, _m: Mods) -> Cursor {
        if let Drag::Move { content: true, .. } = self.drag {
            return Cursor::HandGrab;
        }
        if matches!(self.drag, Drag::Bracket { .. }) || (matches!(self.drag, Drag::None) && self.bracket_at(cx, p).is_some()) {
            return Cursor::PathBracket;
        }
        if let Drag::ContentResize { handle, .. } = self.drag {
            return handle_cursor(handle);
        }
        if matches!(self.drag, Drag::None)
            && !(self.direct && anchor_at(cx, p).is_some())
            && let Some((_, h)) = content_handle_at(cx, p)
        {
            return handle_cursor(h);
        }
        if self.direct {
            let over_content = matches!(self.drag, Drag::None)
                && anchor_at(cx, p).is_none()
                && cx.hit(p).is_some_and(|(_, id)| cx.doc.item(id).is_some_and(holds_content));
            return if over_content { Cursor::Hand } else { Cursor::ArrowHollow };
        }
        if let Some(l) = self.loaded.and_then(|l| l.resolve(cx.doc)) {
            return if crate::thread::port_at(cx, p).is_some() { Cursor::LoadedText } else { l.cursor(cx, p) };
        }
        match self.drag {
            Drag::Move { .. } => return Cursor::Move,
            Drag::Resize { handle, .. } => return handle_cursor(handle),
            Drag::Rotate { .. } => return Cursor::Rotate,
            Drag::Corner { .. } => return Cursor::Arrow,
            _ => {}
        }
        if self.diamond_at(cx, p).is_some() || self.widget_at(cx, p).is_some() {
            return Cursor::Arrow;
        }
        match handle_at(cx, p) {
            Some(h) => handle_cursor(h),
            None if rotate_zone(cx, p).is_some() => Cursor::Rotate,
            None if grabber_at(cx, p).is_some() || on_selected_content(cx, p) => Cursor::Hand,
            None => Cursor::Arrow,
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.drag, Drag::None)
    }
}

/// The start and end brackets of the selected type-on-a-path items, in the selection colour: a
/// line across the path with a short tick pointing at the text between them.
fn brackets(cx: &ToolContext) -> Vec<Overlay> {
    let mut out = Vec::new();
    for id in &cx.selection.items {
        let Some((warp, xf, start, end)) = path_type_of(cx, *id) else { continue };
        let color = cx.doc.item(*id).and_then(|it| cx.doc.layer(it.layer)).map_or([43, 155, 255], |l| l.color);
        for (s, towards) in [(start, end.max(start + 1.0)), (end, start.min(end - 1.0))] {
            let Some((a, b)) = bracket_line(cx, &warp, xf, s) else { continue };
            out.push(Overlay::Line { a, b, color, dashed: false });
            // The tick at the outer end points along the path towards the text.
            if let (Some((p0, _)), Some((p1, _))) = (warp.at(s), warp.at(towards)) {
                let dir = xf * p1 - xf * p0;
                let dir = dir / dir.hypot().max(1e-9) * cx.tol(BRACKET_PX * 0.6);
                out.push(Overlay::Line { a: b, b: b + dir, color, dashed: false });
            }
        }
    }
    out
}

fn handle_cursor(h: usize) -> Cursor {
    match h {
        0 | 4 => Cursor::ResizeNwSe,
        2 | 6 => Cursor::ResizeNeSw,
        1 | 5 => Cursor::ResizeV,
        _ => Cursor::ResizeH,
    }
}

/// Switch to the Type tool with the caret at `p` when it is over a text frame.
fn edit_text_at(cx: &ToolContext, p: Point) -> Option<Vec<Action>> {
    let (_, id) = cx.hit(p)?;
    if !cx.doc.item(id).is_some_and(|i| i.is_text_frame()) {
        return None;
    }
    let (_, sp) = cx.layout.spread_at(p).unwrap_or((SpreadRef::Doc(0), p));
    Some(vec![Action::SwitchTool("type".into()), Action::Exec("text.placeCaret".into(), json!({"frame": id.0, "point": [sp.x, sp.y]}))])
}
