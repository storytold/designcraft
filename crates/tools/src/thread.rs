//! Text threads with the Selection tool: the in and out ports of selected text frames, and the
//! loaded text cursor a click on a port gives.
//!
//! Loaded from an out port, a click threads the clicked frame after the port's frame; loaded
//! from an in port, in front of it. A click on empty space makes a frame the width of the column
//! there, a drag makes one of the dragged size. Clicking the frame on the other side of an
//! existing link breaks the thread there. Shift autoflows, Alt stays loaded with the rest.

use designcraft_doc::{Document, Item, ItemId, SpreadRef};
use designcraft_geom::{Point, Rect};
use serde_json::{Value, json};

use crate::{CanvasLayout, Cursor, Mods, Overlay, ToolContext, rect_json, spread_json};

/// Port square size, in screen pixels.
pub const PORT_SIZE_PX: f64 = 8.5;
/// The in port sits this far below the top-left corner, the out port this far above the
/// bottom-right corner (screen pixels along the frame edge).
const IN_PORT_PX: f64 = 14.5;
const OUT_PORT_PX: f64 = 12.0;
/// Screen pixels around a port that still hit it.
const PORT_SLOP_PX: f64 = 2.0;
/// A click frame shorter than this (the click was at or below the bottom margin) gets
/// [`CLICK_FRAME_HEIGHT`] instead.
const MIN_CLICK_FRAME_HEIGHT: f64 = 12.0;
const CLICK_FRAME_HEIGHT: f64 = 72.0;

/// Canvas centre of a text frame's in port (on its left edge below the top-left corner) or out
/// port (on its right edge above the bottom-right corner), at `zoom` screen pixels per point.
pub fn port_center(doc: &Document, layout: &CanvasLayout, id: ItemId, out: bool, zoom: f64) -> Option<Point> {
    let it = doc.item(id)?;
    let loc = doc.find(id)?;
    let m = layout.xf(loc.spread) * doc.parent_xf(&loc) * it.xf;
    let r = it.inner_bounds();
    let (corner, toward) = if out { (Point::new(r.x1, r.y1), Point::new(r.x1, r.y0)) } else { (Point::new(r.x0, r.y0), Point::new(r.x0, r.y1)) };
    let c = m * corner;
    let edge = (m * toward) - c;
    let len = edge.hypot();
    let step = (if out { OUT_PORT_PX } else { IN_PORT_PX }) / zoom.max(1e-9);
    if !(len.is_finite() && len > 1e-9 && step.is_finite()) {
        return Some(c);
    }
    // Along the edge, never past its middle on a tiny frame.
    Some(c + edge * (step.min(len / 2.0) / len))
}

/// The port of a selected text frame under `p` (canvas): (frame, out port?).
pub fn port_at(cx: &ToolContext, p: Point) -> Option<(ItemId, bool)> {
    let half = cx.tol(PORT_SIZE_PX / 2.0 + PORT_SLOP_PX);
    for id in &cx.selection.items {
        if !cx.doc.item(*id).is_some_and(Item::is_text_frame) {
            continue;
        }
        for out in [true, false] {
            if let Some(c) = port_center(cx.doc, cx.layout, *id, out, cx.zoom)
                && (c.x - p.x).abs() <= half
                && (c.y - p.y).abs() <= half
            {
                return Some((*id, out));
            }
        }
    }
    None
}

/// The frame a click with the loaded cursor makes on empty space (spread coordinates): the width
/// of the column under the click (the nearest column of the nearest page), from the click down
/// to the bottom margin. Off the page's columns it starts at the click, a column wide.
pub fn click_frame_rect(doc: &Document, sr: SpreadRef, p: Point) -> Option<Rect> {
    let sp = doc.spread(sr)?;
    let page = sp.pages.get(sp.page_at_x(p.x)?)?;
    let cols = page.column_rects();
    let dist = |c: &Rect| {
        if p.x < c.x0 {
            c.x0 - p.x
        } else if p.x > c.x1 {
            p.x - c.x1
        } else {
            0.0
        }
    };
    let col = cols.iter().min_by(|a, b| dist(a).total_cmp(&dist(b)))?;
    let on_page = p.x >= page.x && p.x <= page.x + page.width;
    let (x0, x1) = if on_page { (col.x0, col.x1) } else { (p.x, p.x + col.width()) };
    let y1 = if col.y1 - p.y >= MIN_CLICK_FRAME_HEIGHT { col.y1 } else { p.y + CLICK_FRAME_HEIGHT };
    let r = Rect::new(x0, p.y, x1.max(x0 + 1.0), y1);
    [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()).then_some(r)
}

/// Is a press at `start` released at `p` (canvas) a drag rather than a click?
pub(crate) fn is_drag(cx: &ToolContext, start: Point, p: Point) -> bool {
    (p - start).hypot() >= cx.tol(3.0)
}

/// The frame a drag with the loaded cursor from `start` to `p` (canvas) makes, in spread
/// coordinates, and the snap guides to draw. It is drawn like the frame tools (the press point
/// and the moving edges snap) without their modifiers: Shift and Alt autoflow here.
pub(crate) fn drag_frame(cx: &ToolContext, start: Point, p: Point) -> Option<(SpreadRef, Rect, Vec<Overlay>)> {
    let (sr, sa) = cx.layout.spread_at(start)?;
    let sa = if cx.snap.any() { crate::snap::snap_point(cx, sr, sa).0 } else { sa };
    let (r, guides) = crate::frame::snapped_drag_rect(cx, sr, sa, cx.layout.to_spread(sr, p), Mods::default());
    Some((sr, r, guides))
}

/// The loaded text cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Loaded {
    /// The frame whose port was clicked, or the frame semi-autoflow moved on to.
    pub frame: ItemId,
    /// Loaded from the out port (thread after `frame`) or the in port (in front of it).
    pub out: bool,
    /// Semi-autoflow: the frame expected to join on this side once the click's command has run.
    /// The load moves on to it only if the document has it there.
    pub advance: Option<Joins>,
    /// The frame semi-autoflow moved on from: loaded again once `frame` is no longer its
    /// neighbour on this side (undo).
    pub back: Option<ItemId>,
}

/// The frame a `text.thread` from the loaded cursor adds on the loaded side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Joins {
    /// This existing frame.
    Frame(ItemId),
    /// A new frame: any frame other than the neighbour there before the click (`None`: none).
    NewFrameReplacing(Option<ItemId>),
}

/// A click with the loaded cursor: the command, and for `text.thread` the frame it adds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clicked {
    pub id: &'static str,
    pub params: Value,
    pub joins: Option<Joins>,
}

/// What a click with the loaded cursor does at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// Nothing there: a new frame.
    Empty,
    /// A frame that can join the thread.
    Thread(ItemId),
    /// The frame on the other side of the port's link: break the thread there.
    Unthread,
    /// A frame that can't join (the loaded frame itself, a picture, a frame text flows into…).
    Invalid,
}

impl Loaded {
    pub fn new(frame: ItemId, out: bool) -> Self {
        Self { frame, out, advance: None, back: None }
    }

    /// The loaded frame now, or None when that frame is gone (undo, delete). A semi-autoflow load
    /// moves on to the frame its click added once the document has it there, and goes back to
    /// the frame it came from once an undo takes that frame out again.
    pub fn resolve(self, doc: &Document) -> Option<Loaded> {
        let usable = |id: ItemId| doc.item(id).is_some_and(Item::is_text_frame);
        let mut l = self;
        if let Some(b) = l.back
            && usable(b)
            && Loaded::new(b, l.out).neighbour(doc) != Some(l.frame)
        {
            l = Loaded::new(b, l.out);
        }
        if let Some(joins) = l.advance.take()
            && let Some(n) = l.neighbour(doc)
            && usable(n)
        {
            let arrived = match joins {
                Joins::Frame(f) => n == f,
                Joins::NewFrameReplacing(before) => Some(n) != before,
            };
            if arrived {
                l = Loaded { frame: n, out: l.out, advance: None, back: Some(l.frame) };
            }
        }
        usable(l.frame).then_some(l)
    }

    /// The next frame from an out port, the previous one from an in port.
    fn neighbour(self, doc: &Document) -> Option<ItemId> {
        if self.out { doc.next_frame(self.frame) } else { doc.prev_frame(self.frame) }
    }

    /// `text.thread` `from`/`to` for joining frame `t` (None: a new frame) on this side.
    fn pair(self, doc: &Document, t: Option<ItemId>) -> (Option<ItemId>, Option<ItemId>) {
        match (self.out, doc.prev_frame(self.frame)) {
            (true, _) => (Some(self.frame), t),
            (false, Some(prev)) => (Some(prev), t),
            (false, None) => (t, Some(self.frame)),
        }
    }

    pub fn target(self, cx: &ToolContext, p: Point) -> Target {
        // A text frame inside a group is the frame the click means, not the group (gaps 27).
        let Some((_, id)) = cx.hit_text_frame(p).or_else(|| cx.hit(p)) else { return Target::Empty };
        if Some(id) == self.neighbour(cx.doc) {
            return Target::Unthread;
        }
        match self.pair(cx.doc, Some(id)) {
            (Some(from), Some(to)) if cx.doc.check_thread(from, to).is_ok() => Target::Thread(id),
            _ => Target::Invalid,
        }
    }

    pub fn cursor(self, cx: &ToolContext, p: Point) -> Cursor {
        match self.target(cx, p) {
            Target::Empty => Cursor::LoadedText,
            Target::Thread(_) => Cursor::ThreadLink,
            Target::Unthread => Cursor::Unthread,
            Target::Invalid if cx.hit(p).is_some_and(|(_, id)| id == self.frame) => Cursor::LoadedText,
            Target::Invalid => Cursor::NotAllowed,
        }
    }

    /// The command for a click at `p`, or a drag from `start` to `p` (canvas). None: nothing to do.
    pub fn click(self, cx: &ToolContext, start: Point, p: Point, mods: Mods) -> Option<Clicked> {
        let new_frame = Joins::NewFrameReplacing(self.neighbour(cx.doc));
        let (pair, new_rect, joins) = if is_drag(cx, start, p) {
            let (sr, r, _) = drag_frame(cx, start, p)?;
            (self.pair(cx.doc, None), Some((sr, r)), new_frame)
        } else {
            match self.target(cx, p) {
                Target::Unthread => {
                    let side = if self.out { "after" } else { "before" };
                    return Some(Clicked { id: "text.unthread", params: json!({"frame": self.frame.0, "side": side}), joins: None });
                }
                Target::Invalid => return None,
                Target::Thread(t) => (self.pair(cx.doc, Some(t)), None, Joins::Frame(t)),
                Target::Empty => {
                    let (sr, sp) = cx.layout.spread_at(p)?;
                    (self.pair(cx.doc, None), Some((sr, click_frame_rect(cx.doc, sr, sp)?)), new_frame)
                }
            }
        };
        let mut params = serde_json::Map::new();
        if let Some(f) = pair.0 {
            params.insert("from".into(), json!(f.0));
        }
        if let Some(t) = pair.1 {
            params.insert("to".into(), json!(t.0));
        }
        if let Some((sr, r)) = new_rect {
            params.insert("rect".into(), rect_json(r));
            params.insert("spread".into(), spread_json(sr));
        }
        if mods.shift {
            params.insert("autoflow".into(), json!(true));
        }
        Some(Clicked { id: "text.thread", params: Value::Object(params), joins: Some(joins) })
    }
}

/// Double-click on a port: break the link it shows, if it has one.
pub(crate) fn unthread_at_port(doc: &Document, frame: ItemId, out: bool) -> Option<(&'static str, Value)> {
    let linked = if out { doc.next_frame(frame).is_some() } else { doc.prev_frame(frame).is_some() };
    linked.then(|| ("text.unthread", json!({"frame": frame.0, "side": if out { "after" } else { "before" }})))
}
