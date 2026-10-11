//! Selection tool (V) and Direct Selection tool (A).
//!
//! Click selects the frontmost item (Shift toggles), drag moves (Alt duplicates, Shift constrains),
//! the 8 bounding-box handles resize (Shift keeps proportions, Alt from centre, Cmd scales content),
//! empty-canvas drags make a marquee, double-clicking a text frame switches to the Type tool.
//!
//! Content (a placed graphic, or items pasted into a frame) moves inside its frame: the Direct
//! Selection tool selects and drags it, the Selection tool drags it by the content grabber (the
//! circle at the frame's centre), and double-clicking a frame toggles between frame and content.

use designcraft_doc::{Content, Item, ItemId, SpreadRef};
use designcraft_geom::{Affine, Point, Rect, Vec2};
use serde_json::{Value, json};

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
    Rotate {
        center: Point,
        start_angle: f64,
        start_rotation: Option<f64>,
    },
    Marquee {
        start: Point,
        cur: Point,
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
}

impl SelectionTool {
    pub fn new(direct: bool) -> Self {
        Self { direct, drag: Drag::None, hover_handle: None, guides: vec![], shift_release: None }
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

fn handle_at(cx: &ToolContext, p: Point) -> Option<usize> {
    let b = cx.selection_bounds()?;
    let tol = cx.tol(5.0);
    handles(b).iter().position(|h| (h.x - p.x).abs() <= tol && (h.y - p.y).abs() <= tol)
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
    let rect = if m.shift && from.width() > 1e-9 && from.height() > 1e-9 {
        apply_aspect(from, handle, rect, aspect_scale(from, handle, rect))
    } else {
        rect
    };
    if m.alt { apply_alt(from, handle, rect) } else { rect }
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

fn apply_aspect(from: Rect, handle: usize, rect: Rect, s: f64) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
    let (w, h) = (from.width() * s, from.height() * s);
    match handle {
        0 => {
            x0 = x1 - w;
            y0 = y1 - h;
        }
        2 => {
            x1 = x0 + w;
            y0 = y1 - h;
        }
        3..=5 => {
            x1 = x0 + w;
            y1 = y0 + h;
        }
        6 | 7 => {
            x0 = x1 - w;
            y1 = y0 + h;
        }
        _ => {
            x0 = x1 - w;
            y0 = y1 - h;
        }
    }
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
    let c = from.center();
    Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0)
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

fn apply_alt(from: Rect, handle: usize, rect: Rect) -> Rect {
    let (x0, y0, x1, y1) = (rect.x0, rect.y0, rect.x1, rect.y1);
    let c = from.center();
    let (hw, hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let (hw, hh) = match handle {
        1 | 5 => (from.width() / 2.0, (if handle == 1 { c.y - y0 } else { y1 - c.y })),
        3 | 7 => ((if handle == 7 { c.x - x0 } else { x1 - c.x }), from.height() / 2.0),
        _ => (hw.abs().max((c.x - x0).abs()).max((x1 - c.x).abs()), hh.abs().max((c.y - y0).abs()).max((y1 - c.y).abs())),
    };
    Rect::new(c.x - hw, c.y - hh, c.x + hw, c.y + hh)
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
        match ev.kind {
            PointerKind::Move => {
                self.hover_handle = handle_at(cx, p);
                vec![]
            }
            PointerKind::Down => {
                self.shift_release = None;
                if self.direct
                    && let Some((id, si, ai, handle)) = anchor_at(cx, p)
                {
                    let at = anchor_spread_point(cx, id, si, ai);
                    self.drag = Drag::Anchor { id, si, ai, handle, start: p, at };
                    return vec![Action::Begin(if handle.is_some() { "Move Direction Handle".into() } else { "Move Anchor".into() })];
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
                        self.drag = Drag::Pending { start: p, hit: true, content: self.direct };
                        let _ = sr;
                        out
                    }
                    None => {
                        self.drag = Drag::Pending { start: p, hit: false, content: false };
                        if ev.mods.shift { vec![] } else { vec![Action::Exec("selection.set".into(), json!({"ids": []}))] }
                    }
                }
            }
            PointerKind::Drag => match self.drag.clone() {
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
                Drag::Marquee { start, .. } => {
                    self.drag = Drag::Marquee { start, cur: p };
                    vec![]
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
                    Drag::Move { .. } | Drag::Resize { .. } | Drag::Anchor { .. } | Drag::Rotate { .. } => vec![Action::Commit],
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
                    Drag::None => vec![],
                }
            }
            PointerKind::DoubleClick => {
                // A frame with content: toggle between the frame and its content.
                if !self.direct
                    && let Some(id) = cx.hit(p).map(|(_, id)| cx.doc.top_level_of(id).unwrap_or(id))
                    && cx.doc.item(id).is_some_and(holds_content)
                {
                    self.drag = Drag::None;
                    let content = !(cx.selection.content && cx.selection.contains(id));
                    return vec![Action::Exec("selection.set".into(), json!({"ids": [id.0], "content": content}))];
                }
                if let Some((_, id)) = cx.hit(p)
                    && cx.doc.item(id).is_some_and(|i| i.is_text_frame())
                {
                    let (sr, sp) = cx.layout.spread_at(p).unwrap_or((SpreadRef::Doc(0), p));
                    let _ = sr;
                    return vec![
                        Action::SwitchTool("type".into()),
                        Action::Exec("text.placeCaret".into(), json!({"frame": id.0, "point": [sp.x, sp.y]})),
                    ];
                }
                vec![]
            }
        }
    }

    fn key(&mut self, cx: &ToolContext, key: ToolKey, mods: Mods) -> Vec<Action> {
        let inc = cx.doc.settings.keyboard_increment * if mods.shift { 10.0 } else { 1.0 };
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

    fn overlays(&self, _cx: &ToolContext) -> Vec<Overlay> {
        match &self.drag {
            Drag::Marquee { start, cur } => vec![Overlay::Marquee(Rect::from_points(*start, *cur))],
            Drag::Move { content: true, exclude, .. } => {
                let mut v: Vec<Overlay> = exclude.iter().map(|id| Overlay::ContentGhost(*id)).collect();
                v.extend(self.guides.iter().cloned());
                v
            }
            Drag::Move { .. } | Drag::Resize { .. } | Drag::Rotate { .. } | Drag::Anchor { .. } => self.guides.clone(),
            _ => vec![],
        }
    }

    fn cursor(&self, cx: &ToolContext, p: Point, _m: Mods) -> Cursor {
        if let Drag::Move { content: true, .. } = self.drag {
            return Cursor::HandGrab;
        }
        if self.direct {
            let over_content = matches!(self.drag, Drag::None)
                && anchor_at(cx, p).is_none()
                && cx.hit(p).is_some_and(|(_, id)| cx.doc.item(id).is_some_and(holds_content));
            return if over_content { Cursor::Hand } else { Cursor::ArrowHollow };
        }
        match self.drag {
            Drag::Move { .. } => return Cursor::Move,
            Drag::Resize { handle, .. } => return handle_cursor(handle),
            Drag::Rotate { .. } => return Cursor::Rotate,
            _ => {}
        }
        match handle_at(cx, p) {
            Some(h) => handle_cursor(h),
            None if rotate_zone(cx, p).is_some() => Cursor::Rotate,
            None if grabber_at(cx, p).is_some() => Cursor::Hand,
            None => Cursor::Arrow,
        }
    }

    fn busy(&self) -> bool {
        !matches!(self.drag, Drag::None)
    }
}

fn handle_cursor(h: usize) -> Cursor {
    match h {
        0 | 4 => Cursor::ResizeNwSe,
        2 | 6 => Cursor::ResizeNeSw,
        1 | 5 => Cursor::ResizeV,
        _ => Cursor::ResizeH,
    }
}
