//! Add Anchor Point (=), Delete Anchor Point (-) and Convert Direction Point (Shift+C): they act on
//! the selected paths, or the path under the pointer when nothing is selected.

use designcraft_doc::ItemId;
use designcraft_geom::Point;
use serde_json::json;

use crate::select::anchor_at_in;
use crate::{Action, Cursor, Mods, PointerEvent, PointerKind, Tool, ToolContext};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Add,
    /// Scissors: cut the path at the click.
    Scissors,
    Delete,
    Convert,
}

pub struct AnchorTool {
    kind: Kind,
    /// Convert: the anchor pressed on (id, subpath, anchor), the press point, whether it dragged.
    press: Option<(u64, usize, usize, Point, bool)>,
}

impl AnchorTool {
    pub fn new(kind: Kind) -> Self {
        Self { kind, press: None }
    }
}

/// The paths the tool works on.
fn targets(cx: &ToolContext, p: Point) -> Vec<ItemId> {
    if cx.selection.items.is_empty() { cx.hit(p).map(|(_, id)| vec![id]).unwrap_or_default() } else { cx.selection.items.clone() }
}

/// The first of `ids` whose outline passes within 5 screen pixels of canvas point `p`.
pub(crate) fn segment_at(cx: &ToolContext, ids: &[ItemId], p: Point) -> Option<ItemId> {
    ids.iter().copied().find(|&id| {
        let (Some(it), Some(xf)) = (cx.doc.item(id), cx.item_canvas_xf(id)) else { return false };
        let inner = (xf * it.xf).inverse() * p;
        let scale = (xf * it.xf).inverse().determinant().abs().sqrt();
        it.path.nearest(inner).is_some_and(|n| n.4 <= cx.tol(5.0) * scale)
    })
}

impl Tool for AnchorTool {
    fn id(&self) -> &'static str {
        match self.kind {
            Kind::Add => "addAnchor",
            Kind::Scissors => "scissors",
            Kind::Delete => "deleteAnchor",
            Kind::Convert => "convertDirection",
        }
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        let p = ev.pos;
        match (self.kind, ev.kind) {
            (Kind::Add | Kind::Scissors, PointerKind::Down) => {
                let Some((_, sp)) = cx.layout.spread_at(p) else { return vec![] };
                let Some(id) = segment_at(cx, &targets(cx, p), p) else { return vec![] };
                vec![if self.kind == Kind::Scissors {
                    Action::Exec("path.split".into(), json!({"id": id.0, "at": [sp.x, sp.y]}))
                } else {
                    Action::Exec("path.addAnchor".into(), json!({"id": id.0, "at": [sp.x, sp.y], "tolerance": cx.tol(5.0)}))
                }]
            }
            (Kind::Delete, PointerKind::Down) => match anchor_at_in(cx, &targets(cx, p), p) {
                Some((id, si, ai, None)) => vec![Action::Exec("path.deleteAnchor".into(), json!({"id": id, "subpath": si, "anchor": ai}))],
                _ => vec![],
            },
            (Kind::Convert, PointerKind::Down) => {
                if let Some((id, si, ai, None)) = anchor_at_in(cx, &targets(cx, p), p) {
                    self.press = Some((id, si, ai, p, false));
                }
                vec![]
            }
            (Kind::Convert, PointerKind::Drag) => {
                let Some((id, si, ai, start, dragged)) = self.press else { return vec![] };
                if !dragged && (p - start).hypot() < cx.tol(3.0) {
                    return vec![];
                }
                let Some((_, sp)) = cx.layout.spread_at(p) else { return vec![] };
                let mut out = vec![];
                if !dragged {
                    out.push(Action::Begin("Convert Direction Point".into()));
                    self.press = Some((id, si, ai, start, true));
                }
                out.push(Action::Preview("path.convertAnchor".into(), json!({"id": id, "subpath": si, "anchor": ai, "to": [sp.x, sp.y]})));
                out
            }
            (Kind::Convert, PointerKind::Up) => match self.press.take() {
                Some((_, _, _, _, true)) => vec![Action::Commit],
                // A click toggles smooth ↔ corner.
                Some((id, si, ai, _, false)) => vec![Action::Exec("path.convertAnchor".into(), json!({"id": id, "subpath": si, "anchor": ai}))],
                None => vec![],
            },
            _ => vec![],
        }
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        match self.kind {
            Kind::Add => Cursor::PenAdd,
            Kind::Scissors => Cursor::Crosshair,
            Kind::Delete => Cursor::PenDelete,
            Kind::Convert => Cursor::PenConvert,
        }
    }

    fn busy(&self) -> bool {
        self.press.is_some_and(|p| p.4)
    }
}
