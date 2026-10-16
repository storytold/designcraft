//! InDesign. Clicking an end point of an open path (the cursor shows it) continues that path from
//! there; dragging from it first reshapes its direction lines. Between paths, a click on a selected
//! path's anchor deletes it and a click on its outline adds an anchor; the cursor shows which.

use designcraft_doc::{ItemId, SpreadRef};
use designcraft_geom::{BezPath, Point};
use serde_json::{Value, json};

use crate::anchors::segment_at;
use crate::select::anchor_at_in;
use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext, ToolKey, spread_json};

#[derive(Default)]
pub struct PenTool {
    /// The path being drawn: (item id once created, spread, anchors in spread coords).
    path: Option<u64>,
    spread: Option<SpreadRef>,
    /// Anchors as (p, in, out) in spread coordinates.
    anchors: Vec<(Point, Point, Point)>,
    dragging: bool,
    hover: Point,
    /// Smart guides in canvas coordinates, as `snap_point` returned them.
    guides: Vec<Overlay>,
    /// Continuing an existing path: the subpath the new anchors go on.
    subpath: Option<usize>,
    /// The press was on the continued path's end point: a drag reshapes its direction lines
    /// instead of adding an anchor.
    editing_end: bool,
    /// The press on the end point has been dragged.
    end_moved: bool,
}

/// An end point of an open path under canvas point `p`: (item, subpath, whether it is the last
/// anchor). The selection is checked first, then the frontmost item under the pointer.
fn endpoint_at(cx: &ToolContext, p: Point) -> Option<(ItemId, usize, bool)> {
    let tol = cx.tol(6.0);
    let mut ids = cx.selection.items.clone();
    if let Some((_, id)) = cx.hit(p)
        && !ids.contains(&id)
    {
        ids.push(id);
    }
    for id in ids {
        let Some(it) = cx.doc.item(id) else { continue };
        if it.locked || it.is_group() || !matches!(it.content, designcraft_doc::Content::Unassigned) {
            continue;
        }
        let Some(xf) = cx.item_canvas_xf(id).map(|m| m * it.xf) else { continue };
        for (si, sp) in it.path.subpaths.iter().enumerate() {
            if sp.closed {
                continue;
            }
            let (Some(first), Some(last)) = (sp.anchors.first(), sp.anchors.last()) else { continue };
            if (xf * last.p - p).hypot() <= tol {
                return Some((id, si, true));
            }
            if (xf * first.p - p).hypot() <= tol {
                return Some((id, si, false));
            }
        }
    }
    None
}

fn anchor_json(a: &(Point, Point, Point)) -> Value {
    json!({"p": [a.0.x, a.0.y], "in": [a.1.x, a.1.y], "out": [a.2.x, a.2.y]})
}

/// What a pen click at the pointer does instead of placing an anchor.
#[derive(Debug, PartialEq)]
enum Hover {
    Place,
    /// Close the path being drawn (the pointer is on its first anchor).
    Close,
    /// Delete anchor (subpath, anchor) of a selected path.
    Delete(u64, usize, usize),
    /// Add an anchor on a selected path's outline.
    Add(ItemId),
}

impl PenTool {
    /// What a click at canvas point `p` does.
    fn hover_at(&self, cx: &ToolContext, p: Point) -> Hover {
        if let Some(sr) = self.spread {
            if self.anchors.len() >= 2
                && let Some(first) = self.anchors.first()
                && (first.0 - cx.layout.to_spread(sr, p)).hypot() <= cx.tol(6.0)
            {
                return Hover::Close;
            }
            return Hover::Place;
        }
        if !self.anchors.is_empty() {
            return Hover::Place;
        }
        let ids = &cx.selection.items;
        if let Some((id, si, ai, None)) = anchor_at_in(cx, ids, p) {
            return Hover::Delete(id, si, ai);
        }
        segment_at(cx, ids, p).map_or(Hover::Place, Hover::Add)
    }

    fn finish(&mut self) -> Vec<Action> {
        self.path = None;
        self.spread = None;
        self.anchors.clear();
        self.dragging = false;
        self.guides.clear();
        self.subpath = None;
        self.editing_end = false;
        self.end_moved = false;
        vec![]
    }

    /// Start continuing subpath `si` of `id` from its last (`at_end`) or first anchor: the anchors
    /// so far are its other end and the end point, in spread coordinates.
    fn continue_from(&mut self, cx: &ToolContext, id: ItemId, si: usize, at_end: bool) -> Vec<Action> {
        let (Some(it), Some(loc)) = (cx.doc.item(id), cx.doc.find(id)) else { return vec![] };
        let Some(sp) = it.path.subpaths.get(si) else { return vec![] };
        let (Some(first), Some(last)) = (sp.anchors.first(), sp.anchors.last()) else { return vec![] };
        let xf = cx.doc.parent_xf(&loc) * it.xf;
        let at = |a: &designcraft_geom::Anchor, flip: bool| {
            let (i, o) = if flip { (a.h_out, a.h_in) } else { (a.h_in, a.h_out) };
            (xf * a.p, xf * i, xf * o)
        };
        // From the first anchor the path is reversed first, so the new anchors follow on.
        let (other, end) = if at_end { (at(first, false), at(last, false)) } else { (at(last, true), at(first, true)) };
        self.path = Some(id.0);
        self.subpath = Some(si);
        self.spread = Some(loc.spread);
        self.anchors = vec![other, end];
        self.editing_end = true;
        self.end_moved = false;
        self.dragging = true;
        if at_end { vec![] } else { vec![Action::Exec("path.reverse".into(), json!({"ids": [id.0], "subpath": si}))] }
    }

    /// Shift constrains first. The stored point is the snapped spread point. Guides are already canvas.
    fn snap_click(&mut self, cx: &ToolContext, spread: SpreadRef, mut p: Point, shift: bool) -> Point {
        if shift && let Some(last) = self.anchors.last() {
            p = last.0 + designcraft_geom::constrain_angle(p - last.0, 45.0);
        }
        let (p, guides) = crate::snap::snap_point(cx, spread, p);
        self.guides = guides;
        p
    }

    fn commit_anchor(&mut self) -> Vec<Action> {
        let Some(sr) = self.spread else { return vec![] };
        let Some(last) = self.anchors.last() else { return vec![] };
        match self.path {
            None if self.anchors.len() >= 2 => {
                let anchors: Vec<Value> = self.anchors.iter().map(anchor_json).collect();
                vec![Action::Exec("path.create".into(), json!({"spread": spread_json(sr), "anchors": anchors, "closed": false, "notifyPen": true}))]
            }
            Some(id) => {
                let mut p = json!({"id": id, "anchor": anchor_json(last)});
                if let Some(si) = self.subpath {
                    p["subpath"] = json!(si);
                }
                vec![Action::Exec("path.appendAnchor".into(), p)]
            }
            None => vec![],
        }
    }
}

impl Tool for PenTool {
    fn id(&self) -> &'static str {
        "pen"
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let Some((sr, sp)) = cx.layout.spread_at(ev.pos) else {
            self.guides.clear();
            return vec![];
        };
        self.hover = ev.pos;
        // The pen continues on the spread where the path started.
        let sp = match self.spread {
            Some(s) if s != sr => cx.layout.to_spread(s, ev.pos),
            _ => sp,
        };
        let snap_spread = self.spread.unwrap_or(sr);
        // Pick up the item id created by path.create (the newest selected path).
        if self.path.is_none() && self.anchors.len() >= 2 {
            self.path = cx.selection.items.last().map(|i| i.0);
        }
        match ev.kind {
            PointerKind::Move => {
                let p = self.snap_click(cx, snap_spread, sp, ev.mods.shift);
                self.hover = cx.layout.to_canvas(snap_spread, p);
                vec![]
            }
            PointerKind::Down => {
                match self.hover_at(cx, ev.pos) {
                    Hover::Close => {
                        let out = match self.path {
                            Some(id) => {
                                let mut p = json!({"id": id});
                                if let Some(si) = self.subpath {
                                    p["subpath"] = json!(si);
                                }
                                vec![Action::Exec("path.close".into(), p)]
                            }
                            None => vec![],
                        };
                        self.finish();
                        return out;
                    }
                    Hover::Delete(id, si, ai) => {
                        return vec![Action::Exec("path.deleteAnchor".into(), json!({"id": id, "subpath": si, "anchor": ai}))];
                    }
                    Hover::Add(id) => {
                        return vec![Action::Exec("path.addAnchor".into(), json!({"id": id.0, "at": [sp.x, sp.y], "tolerance": cx.tol(5.0)}))];
                    }
                    Hover::Place => {}
                }
                // On an open path's end point: continue that path.
                if self.anchors.is_empty()
                    && let Some((id, si, at_end)) = endpoint_at(cx, ev.pos)
                {
                    return self.continue_from(cx, id, si, at_end);
                }
                let p = self.snap_click(cx, snap_spread, sp, ev.mods.shift);
                if self.anchors.is_empty() {
                    self.spread = Some(sr);
                }
                self.anchors.push((p, p, p));
                self.dragging = true;
                vec![]
            }
            PointerKind::Drag => {
                self.end_moved |= self.editing_end;
                if let Some(a) = self.anchors.last_mut() {
                    // Smooth point: out handle follows the pointer, in handle mirrors it.
                    a.2 = sp;
                    a.1 = a.0 - (sp - a.0);
                }
                vec![]
            }
            PointerKind::Up => {
                if !self.dragging {
                    return vec![];
                }
                self.dragging = false;
                // The continued path's end point: a drag set its direction lines.
                if std::mem::take(&mut self.editing_end) {
                    let (Some(id), Some(si), Some(&(p, h_in, h_out))) = (self.path, self.subpath, self.anchors.last()) else { return vec![] };
                    if !std::mem::take(&mut self.end_moved) || (h_out - p).hypot() < 1e-9 {
                        return vec![];
                    }
                    return vec![Action::Exec(
                        "path.setAnchor".into(),
                        json!({"id": id, "subpath": si, "anchor": "last", "in": [h_in.x, h_in.y], "out": [h_out.x, h_out.y]}),
                    )];
                }
                self.commit_anchor()
            }
            _ => vec![],
        }
    }

    fn key(&mut self, _cx: &ToolContext, key: ToolKey, _mods: Mods) -> Vec<Action> {
        match key {
            ToolKey::Enter | ToolKey::Escape => {
                let had = !self.anchors.is_empty();
                self.finish();
                if had { vec![Action::Exec("selection.set".into(), json!({"ids": []}))] } else { vec![] }
            }
            _ => vec![],
        }
    }

    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let mut out = Vec::new();
        let Some(sr) = self.spread else {
            out.extend(self.guides.iter().cloned());
            return out;
        };
        let xf = cx.layout.xf(sr);
        if let Some(last) = self.anchors.last()
            && !self.dragging
        {
            // Rubber band to the pointer.
            out.push(Overlay::Line { a: xf * last.0, b: self.hover, color: [79, 153, 255], dashed: true });
        }
        if self.path.is_none() && self.anchors.len() == 1 {
            let a = self.anchors[0];
            out.push(Overlay::Line { a: xf * a.1, b: xf * a.2, color: [79, 153, 255], dashed: false });
        }
        if self.dragging
            && let Some(a) = self.anchors.last()
        {
            out.push(Overlay::Line { a: xf * a.1, b: xf * a.2, color: [79, 153, 255], dashed: false });
            let _ = BezPath::new();
        }
        // Guides are already canvas. Do not run them through the spread transform again.
        out.extend(self.guides.iter().cloned());
        out
    }

    fn cursor(&self, cx: &ToolContext, p: Point, _m: Mods) -> Cursor {
        if self.anchors.is_empty() && endpoint_at(cx, p).is_some() {
            return Cursor::PenContinue;
        }
        match self.hover_at(cx, p) {
            Hover::Place => Cursor::Pen,
            Hover::Close => Cursor::PenClose,
            Hover::Delete(..) => Cursor::PenDelete,
            Hover::Add(_) => Cursor::PenAdd,
        }
    }

    fn busy(&self) -> bool {
        !self.anchors.is_empty()
    }
}
