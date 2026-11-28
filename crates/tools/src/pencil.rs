//! Pencil tool (N): draw freehand; the stroke becomes a smooth path. Alt while releasing closes
//! it.

use designcraft_doc::SpreadRef;
use designcraft_geom::{BezPath, Point};
use serde_json::json;

use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext};

#[derive(Default)]
pub struct PencilTool {
    /// Spread, its spread → canvas transform, and the canvas points so far.
    stroke: Option<(SpreadRef, designcraft_geom::Affine, Vec<Point>)>,
}

impl Tool for PencilTool {
    fn id(&self) -> &'static str {
        "pencil"
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                let Some((sr, _)) = cx.layout.spread_at(ev.pos) else { return vec![] };
                self.stroke = Some((sr, cx.layout.xf(sr), vec![ev.pos]));
                vec![]
            }
            PointerKind::Drag => {
                if let Some((_, _, pts)) = &mut self.stroke
                    && pts.last().is_none_or(|q| (*q - ev.pos).hypot() >= cx.tol(1.0))
                {
                    pts.push(ev.pos);
                }
                vec![]
            }
            PointerKind::Up => {
                let Some((sr, xf, mut pts)) = self.stroke.take() else { return vec![] };
                pts.push(ev.pos);
                let inv = xf.inverse();
                let spread: Vec<Point> = pts.iter().map(|p| inv * *p).collect();
                // Simplify to about two screen pixels.
                let anchors = designcraft_geom::freehand::fit(&spread, cx.tol(2.0), ev.mods.alt);
                if anchors.len() < 2 {
                    return vec![];
                }
                let a: Vec<_> =
                    anchors.iter().map(|a| json!({"p": [a.p.x, a.p.y], "in": [a.h_in.x, a.h_in.y], "out": [a.h_out.x, a.h_out.y]})).collect();
                vec![Action::Exec("path.create".into(), json!({"spread": sr, "anchors": a, "closed": ev.mods.alt}))]
            }
            _ => vec![],
        }
    }

    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let Some((_, _, pts)) = &self.stroke else { return vec![] };
        let mut path = BezPath::new();
        for (i, p) in pts.iter().enumerate() {
            if i == 0 {
                path.move_to(*p);
            } else {
                path.line_to(*p);
            }
        }
        // In the colour of the layer the path goes on, as InDesign draws it.
        let color = cx.doc.layer(cx.layer).map_or([60, 120, 220], |l| l.color);
        vec![Overlay::Path { path, color, dashed: false }]
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Pen
    }

    fn busy(&self) -> bool {
        self.stroke.is_some()
    }
}

/// Smooth and Erase tools (Pencil group): drag along a selected path.
pub struct PathDragTool {
    erase: bool,
    /// Target path, its spread's spread → canvas transform, and the drag in spread coordinates.
    drag: Option<(u64, designcraft_geom::Affine, Vec<Point>)>,
}

impl PathDragTool {
    pub fn new(erase: bool) -> Self {
        Self { erase, drag: None }
    }
}

impl Tool for PathDragTool {
    fn id(&self) -> &'static str {
        if self.erase { "erase" } else { "smooth" }
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                // The selected path, or the one under the pointer.
                let id = cx.selection.items.first().copied().or_else(|| cx.hit(ev.pos).map(|h| h.1));
                let Some(id) = id else { return vec![] };
                let Some(loc) = cx.doc.find(id) else { return vec![] };
                let xf = cx.layout.xf(loc.spread);
                self.drag = Some((id.0, xf, vec![xf.inverse() * ev.pos]));
                vec![]
            }
            PointerKind::Drag => {
                if let Some((_, xf, pts)) = &mut self.drag {
                    pts.push(xf.inverse() * ev.pos);
                }
                vec![]
            }
            PointerKind::Up => {
                let Some((id, xf, mut pts)) = self.drag.take() else { return vec![] };
                pts.push(xf.inverse() * ev.pos);
                let points: Vec<_> = pts.iter().map(|p| json!([p.x, p.y])).collect();
                let cmd = if self.erase { "path.erase" } else { "path.smooth" };
                vec![Action::Exec(cmd.into(), json!({"id": id, "points": points, "tolerance": cx.tol(6.0)}))]
            }
            _ => vec![],
        }
    }

    fn overlays(&self, _cx: &ToolContext) -> Vec<Overlay> {
        let Some((_, xf, pts)) = &self.drag else { return vec![] };
        let mut path = BezPath::new();
        for (i, p) in pts.iter().enumerate() {
            let p = *xf * *p;
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        vec![Overlay::Path { path, color: if self.erase { [220, 60, 60] } else { [60, 120, 220] }, dashed: true }]
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Pen
    }

    fn busy(&self) -> bool {
        self.drag.is_some()
    }
}
