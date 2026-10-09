//! Frame and shape tools (Rectangle/Ellipse/Polygon Frame, Rectangle/Ellipse/Polygon, Line).
//! Drag to draw (Shift = square/45°, Alt = from centre); click opens the size dialog.

use designcraft_doc::SpreadRef;
use designcraft_geom::{Point, Rect};
use serde_json::json;

use crate::{Action, Cursor, Gesture, Mods, Overlay, PointerEvent, PointerKind, Snap, SnapRequest, Tool, ToolContext, rect_json, spread_json};

pub struct FrameTool {
    id: &'static str,
    start: Option<Point>,
    cur: Point,
    active: bool,
    guides: Vec<crate::Overlay>,
    /// Gridify: columns and rows (arrow keys while dragging), and the last drawn rect.
    grid: (u32, u32),
    last: Option<(serde_json::Value, Rect)>,
}

impl FrameTool {
    pub fn new(id: &str) -> Self {
        let id = crate::tool_info(id).map(|t| t.id).unwrap_or("rectangleFrame");
        Self { id, start: None, cur: Point::ZERO, active: false, guides: vec![], grid: (1, 1), last: None }
    }

    /// The preview for `r`: one frame, or a grid of them.
    fn preview(&self, spread: serde_json::Value, r: Rect) -> Action {
        let (shape, content) = self.kind();
        let (c, rw) = self.grid;
        if c * rw > 1 {
            Action::Preview(
                "frame.grid".into(),
                json!({"spread": spread, "shape": shape, "content": content, "rect": rect_json(r), "cols": c, "rows": rw}),
            )
        } else {
            Action::Preview("frame.create".into(), json!({"spread": spread, "shape": shape, "content": content, "rect": rect_json(r)}))
        }
    }
    fn kind(&self) -> (&'static str, &'static str) {
        match self.id {
            "rectangleFrame" => ("rectangle", "graphic"),
            "ellipseFrame" => ("ellipse", "graphic"),
            "polygonFrame" => ("polygon", "graphic"),
            "rectangle" => ("rectangle", "unassigned"),
            "ellipse" => ("ellipse", "unassigned"),
            "polygon" => ("polygon", "unassigned"),
            _ => ("line", "unassigned"),
        }
    }
}

/// The rectangle a frame-drawing drag from `sa` to `end` (spread space) makes, snapped when
/// snapping is on, and the snap guides to draw. Shift constrains the shape first; only the
/// driving edge snaps, then the other follows. Alt draws from the centre.
pub(crate) fn snapped_drag_rect(cx: &ToolContext, sr: SpreadRef, sa: Point, end: Point, mods: Mods) -> (Rect, Vec<crate::Overlay>) {
    let r = drag_rect(sa, end, mods);
    if !cx.snap.any() {
        return (r, vec![]);
    }
    let (mut x_edges, mut y_edges) = moving_edges(sa, end, r, mods.alt);
    let driver_x = (end.x - sa.x).abs() >= (end.y - sa.y).abs();
    if mods.shift {
        if driver_x {
            y_edges = [false, false, false];
        } else {
            x_edges = [false, false, false];
        }
    }
    let hit = crate::snap::snap(
        cx,
        SnapRequest {
            spread: sr,
            gesture: Gesture::Create,
            rect: r,
            x_edges,
            y_edges,
            exclude: &[],
            copying: false,
            lengths: [length_on(x_edges, r.width()), length_on(y_edges, r.height())],
            angle: None,
            radius: 0.0,
            pointer: end,
        },
    );
    let mut r = crate::snap::nudge_edges(r, x_edges, y_edges, hit.delta);
    if mods.shift {
        r = restore_shift_frame(sa, end, r, driver_x, mods.alt);
    }
    (r, hit.guides)
}

pub fn drag_rect(a: Point, b: Point, m: Mods) -> Rect {
    let (mut dx, mut dy) = (b.x - a.x, b.y - a.y);
    if m.shift {
        let s = dx.abs().max(dy.abs());
        dx = s * dx.signum();
        dy = s * dy.signum();
    }
    if m.alt {
        Rect::new(a.x - dx.abs(), a.y - dy.abs(), a.x + dx.abs(), a.y + dy.abs())
    } else {
        Rect::from_points(a, Point::new(a.x + dx, a.y + dy))
    }
}

/// Edges the drag is setting. Alt (drawn from the centre) snaps the pointer side.
fn moving_edges(anchor: Point, end: Point, rect: Rect, from_center: bool) -> ([bool; 3], [bool; 3]) {
    if from_center {
        let x = if (end.x - anchor.x).abs() <= 1e-9 {
            [false, false, false]
        } else if end.x >= anchor.x {
            [false, false, true]
        } else {
            [true, false, false]
        };
        let y = if (end.y - anchor.y).abs() <= 1e-9 {
            [false, false, false]
        } else if end.y >= anchor.y {
            [false, false, true]
        } else {
            [true, false, false]
        };
        return (x, y);
    }
    (anchored_edge(anchor.x, rect.x0, rect.x1), anchored_edge(anchor.y, rect.y0, rect.y1))
}

fn anchored_edge(anchor: f64, a0: f64, a1: f64) -> [bool; 3] {
    let on0 = (a0 - anchor).abs() <= 1e-4;
    let on1 = (a1 - anchor).abs() <= 1e-4;
    if on0 && !on1 {
        [false, false, true]
    } else if on1 && !on0 {
        [true, false, false]
    } else if on0 && on1 {
        [false, false, false]
    } else if (a0 - anchor).abs() <= (a1 - anchor).abs() {
        [false, false, true]
    } else {
        [true, false, false]
    }
}

fn length_on(edges: [bool; 3], size: f64) -> Option<f64> {
    if edges == [false, false, false] {
        return None;
    }
    let len = size.abs();
    len.is_finite().then_some(len)
}

fn line_hit(cx: &ToolContext, spread: SpreadRef, point: Point, len: Option<f64>) -> Snap {
    crate::snap::snap(
        cx,
        SnapRequest {
            spread,
            gesture: Gesture::Create,
            rect: Rect::from_points(point, point),
            x_edges: [true, true, true],
            y_edges: [true, true, true],
            exclude: &[],
            copying: false,
            lengths: [len, None],
            angle: None,
            radius: 0.0,
            pointer: point,
        },
    )
}

/// Position corrections only. A length win is not an x shove.
fn apply_position(mut end: Point, hit: &Snap) -> (Point, bool, bool) {
    let x_len = finite_length(hit.length_delta[0]);
    let y_len = finite_length(hit.length_delta[1]);
    if x_len.is_none() && hit.delta.x.is_finite() {
        end.x += hit.delta.x;
    }
    if y_len.is_none() && hit.delta.y.is_finite() {
        end.y += hit.delta.y;
    }
    let x_locked = x_len.is_none() && hit.delta.x.is_finite() && hit.delta.x.abs() > 1e-9;
    let y_locked = y_len.is_none() && hit.delta.y.is_finite() && hit.delta.y.abs() > 1e-9;
    (end, x_locked, y_locked)
}

fn finite_length(v: Option<f64>) -> Option<f64> {
    v.filter(|d| d.is_finite())
}

/// Slide along the segment so its length equals `current + diff`.
fn length_slide(anchor: Point, end: Point, diff: f64) -> Point {
    let dir = end - anchor;
    let len = dir.hypot();
    if !len.is_finite() || len <= 1e-9 || !diff.is_finite() {
        return end;
    }
    let target = len + diff;
    if !target.is_finite() || target < 0.0 {
        return end;
    }
    let scale = target / len;
    if !scale.is_finite() {
        return end;
    }
    Point::new(anchor.x + dir.x * scale, anchor.y + dir.y * scale)
}

/// A length match moves the free endpoint along the segment.
///
/// An axis a position pass already won stays put. The segment is not rescaled about the
/// anchor after that position delta, because the won axis would leave its guide.
fn commit_line_end(anchor: Point, proposed: Point, hit: &Snap) -> Point {
    let len = (proposed - anchor).hypot();
    let (mut end, x_locked, y_locked) = apply_position(proposed, hit);
    let Some(diff) = finite_length(hit.length_delta[0]).or(finite_length(hit.length_delta[1])) else {
        return end;
    };
    if !x_locked && !y_locked {
        return length_slide(anchor, proposed, diff);
    }
    let target = len + diff;
    if !target.is_finite() || target < 0.0 {
        return end;
    }
    if y_locked && !x_locked {
        let dy = end.y - anchor.y;
        let remain = target * target - dy * dy;
        if remain >= 0.0 && remain.is_finite() {
            let vx = proposed.x - anchor.x;
            let sign = if vx.abs() > 1e-9 { vx.signum() } else { 1.0 };
            end.x = anchor.x + sign * remain.sqrt();
        }
        return end;
    }
    if x_locked && !y_locked {
        let dx = end.x - anchor.x;
        let remain = target * target - dx * dx;
        if remain >= 0.0 && remain.is_finite() {
            let vy = proposed.y - anchor.y;
            let sign = if vy.abs() > 1e-9 { vy.signum() } else { 1.0 };
            end.y = anchor.y + sign * remain.sqrt();
        }
    }
    end
}

/// Shift line: slide along the ray. A position hit wins. Otherwise the length does.
fn snap_shift_line(cx: &ToolContext, spread: SpreadRef, anchor: Point, proposed: Point, guides: &mut Vec<Overlay>) -> Point {
    let pos = line_hit(cx, spread, proposed, None);
    if let Some(end) = ray_position(anchor, proposed, &pos) {
        *guides = pos.guides;
        return end;
    }
    let len = (proposed - anchor).hypot();
    let hit = line_hit(cx, spread, proposed, len.is_finite().then_some(len));
    let diff = finite_length(hit.length_delta[0]);
    let end = match diff {
        Some(diff) => length_slide(anchor, proposed, diff),
        None => proposed,
    };
    *guides = hit.guides;
    if let Some(diff) = diff {
        crate::snap::lay_line_dimension(guides, cx.layout.xf(spread), anchor, proposed, end, diff);
    }
    end
}

fn snap_free_line(cx: &ToolContext, spread: SpreadRef, anchor: Point, proposed: Point, guides: &mut Vec<Overlay>) -> Point {
    let len = (proposed - anchor).hypot();
    let hit = line_hit(cx, spread, proposed, len.is_finite().then_some(len));
    let end = commit_line_end(anchor, proposed, &hit);
    let diff = finite_length(hit.length_delta[0]);
    *guides = hit.guides;
    if let Some(diff) = diff {
        crate::snap::lay_line_dimension(guides, cx.layout.xf(spread), anchor, proposed, end, diff);
    }
    end
}

/// Closest position correction, applied along the ray. None when neither axis has one.
fn ray_position(anchor: Point, end: Point, hit: &Snap) -> Option<Point> {
    let dir = end - anchor;
    let len = dir.hypot();
    if !len.is_finite() || len <= 1e-9 {
        return None;
    }
    let ux = dir.x / len;
    let uy = dir.y / len;
    let slide = closer_slide(ux, uy, nonzero_delta(hit.delta.x), nonzero_delta(hit.delta.y))?;
    let t = len + slide;
    t.is_finite().then_some(Point::new(anchor.x + ux * t, anchor.y + uy * t))
}

fn nonzero_delta(delta: f64) -> Option<f64> {
    (delta.is_finite() && delta.abs() > 1e-9).then_some(delta)
}

fn closer_slide(ux: f64, uy: f64, x_pos: Option<f64>, y_pos: Option<f64>) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    if let Some(dx) = x_pos
        && ux.abs() > 1e-9
    {
        let slide = dx / ux;
        if slide.is_finite() {
            best = Some((dx.abs(), slide));
        }
    }
    if let Some(dy) = y_pos
        && uy.abs() > 1e-9
    {
        let slide = dy / uy;
        if slide.is_finite() && best.is_none_or(|(dist, _)| dy.abs() < dist) {
            best = Some((dy.abs(), slide));
        }
    }
    best.map(|(_, slide)| slide)
}

/// Shift already constrained the shape. Snap only the driving edge, then rebuild the other.
fn restore_shift_frame(anchor: Point, end: Point, rect: Rect, driver_x: bool, from_center: bool) -> Rect {
    if from_center {
        let half = if driver_x {
            let edge = if end.x >= anchor.x { rect.x1 } else { rect.x0 };
            (edge - anchor.x).abs()
        } else {
            let edge = if end.y >= anchor.y { rect.y1 } else { rect.y0 };
            (edge - anchor.y).abs()
        };
        if !half.is_finite() {
            return rect;
        }
        return Rect::new(anchor.x - half, anchor.y - half, anchor.x + half, anchor.y + half);
    }
    let span = if driver_x { rect.x1 - rect.x0 } else { rect.y1 - rect.y0 };
    if !span.is_finite() {
        return rect;
    }
    let size = span.abs();
    let sx = (end.x - anchor.x).signum();
    let sy = (end.y - anchor.y).signum();
    if sx == 0.0 || sy == 0.0 || !size.is_finite() {
        return rect;
    }
    if driver_x {
        let y_far = anchor.y + sy * size;
        let (y0, y1) = if sy > 0.0 { (anchor.y, y_far) } else { (y_far, anchor.y) };
        Rect::new(rect.x0, y0, rect.x1, y1)
    } else {
        let x_far = anchor.x + sx * size;
        let (x0, x1) = if sx > 0.0 { (anchor.x, x_far) } else { (x_far, anchor.x) };
        Rect::new(x0, rect.y0, x1, rect.y1)
    }
}

impl Tool for FrameTool {
    fn id(&self) -> &'static str {
        self.id
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let (shape, content) = self.kind();
        match ev.kind {
            PointerKind::Down => {
                let pos = match cx.layout.spread_at(ev.pos) {
                    Some((sr, sp)) if cx.snap.any() => cx.layout.to_canvas(sr, crate::snap::snap_point(cx, sr, sp).0),
                    _ => ev.pos,
                };
                self.start = Some(pos);
                self.cur = ev.pos;
                self.active = false;
                self.grid = (1, 1);
                self.last = None;
                vec![]
            }
            PointerKind::Drag => {
                let Some(a) = self.start else { return vec![] };
                self.cur = ev.pos;
                if !self.active && (ev.pos - a).hypot() < cx.tol(3.0) {
                    return vec![];
                }
                let Some((sr, sa)) = cx.layout.spread_at(a) else { return vec![] };
                self.guides.clear();
                let mut out = vec![];
                if !self.active {
                    self.active = true;
                    out.push(Action::Begin(format!(
                        "Create {}",
                        crate::tool_info(self.id).map(|t| t.label.trim_end_matches(" Tool")).unwrap_or("Frame")
                    )));
                }
                if shape == "line" {
                    // Shift constrains the angle first. The endpoint then snaps along that ray.
                    let mut b = cx.layout.to_spread(sr, ev.pos);
                    if ev.mods.shift {
                        let v = b - sa;
                        b = sa + designcraft_geom::constrain_angle(v, 45.0);
                    }
                    if cx.snap.any() {
                        b = if ev.mods.shift {
                            snap_shift_line(cx, sr, sa, b, &mut self.guides)
                        } else {
                            snap_free_line(cx, sr, sa, b, &mut self.guides)
                        };
                    }
                    out.push(Action::Preview("line.create".into(), json!({"spread": spread_json(sr), "a": [sa.x, sa.y], "b": [b.x, b.y]})));
                } else {
                    let end = cx.layout.to_spread(sr, ev.pos);
                    let (r, guides) = snapped_drag_rect(cx, sr, sa, end, ev.mods);
                    self.guides = guides;
                    self.last = Some((spread_json(sr), r));
                    out.push(self.preview(spread_json(sr), r));
                }
                out
            }
            PointerKind::Up => {
                self.guides.clear();
                let was = self.active;
                self.active = false;
                let start = self.start.take();
                if was {
                    return vec![Action::Commit];
                }
                // Click: size dialog.
                if let Some(a) = start
                    && let Some((sr, sa)) = cx.layout.spread_at(a)
                {
                    return vec![Action::Dialog(
                        "frameSize".into(),
                        json!({"spread": spread_json(sr), "shape": shape, "content": content, "x": sa.x, "y": sa.y, "width": 72.0, "height": 72.0}),
                    )];
                }
                vec![]
            }
            _ => vec![],
        }
    }

    fn key(&mut self, _cx: &ToolContext, key: crate::ToolKey, _mods: Mods) -> Vec<Action> {
        // Gridify while dragging: ←/→ columns, ↑/↓ rows.
        if !self.active || self.kind().0 == "line" {
            return vec![];
        }
        let (c, r) = &mut self.grid;
        match key {
            crate::ToolKey::Right => *c += 1,
            crate::ToolKey::Left => *c = (*c).saturating_sub(1).max(1),
            crate::ToolKey::Up => *r += 1,
            crate::ToolKey::Down => *r = (*r).saturating_sub(1).max(1),
            _ => return vec![],
        }
        match self.last.clone() {
            Some((sp, rect)) => vec![self.preview(sp, rect)],
            None => vec![],
        }
    }

    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        if let (true, Some(a)) = (self.active, self.start) {
            let r = Rect::from_points(a, self.cur);
            let _ = cx;
            let mut v = self.guides.clone();
            v.push(Overlay::Measure { p: self.cur, text: format!("W: {:.0} pt  H: {:.0} pt", r.width(), r.height()) });
            return v;
        }
        vec![]
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Crosshair
    }

    fn busy(&self) -> bool {
        self.active
    }
}
