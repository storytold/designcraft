//! Gradient Swatch tool (G): drag across the selected objects (or the object under the pointer)
//! to set where their gradient fill starts and ends; Shift constrains the angle to 45° steps.

use designcraft_doc::SpreadRef;
use designcraft_geom::{Point, Vec2};
use serde_json::json;

use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext};

#[derive(Default)]
pub struct GradientTool {
    /// Gradient Feather tool (Shift+G): the drag sets the fade instead of the fill gradient.
    pub feather: bool,
    /// Canvas start point, spread-space start point and the target ids, and the spread.
    drag: Option<(Point, Point, Vec<u64>, SpreadRef)>,
    end: Option<Point>,
}

fn constrain(a: Point, b: Point, m: Mods) -> Point {
    if !m.shift {
        return b;
    }
    let v = b - a;
    let step = std::f64::consts::FRAC_PI_4;
    let ang = (v.y.atan2(v.x) / step).round() * step;
    a + Vec2::new(ang.cos(), ang.sin()) * v.hypot()
}

impl GradientTool {
    pub fn feather() -> Self {
        GradientTool { feather: true, ..Default::default() }
    }
}

impl Tool for GradientTool {
    fn id(&self) -> &'static str {
        if self.feather { "gradientFeather" } else { "gradientSwatch" }
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                let ids: Vec<u64> = if cx.selection.items.is_empty() {
                    cx.hit(ev.pos).map(|(_, id)| vec![cx.doc.top_level_of(id).unwrap_or(id).0]).unwrap_or_default()
                } else {
                    cx.selection.items.iter().map(|i| i.0).collect()
                };
                let Some((spread, sp)) = cx.layout.spread_at(ev.pos) else { return vec![] };
                if ids.is_empty() {
                    return vec![];
                }
                self.drag = Some((ev.pos, sp, ids, spread));
                self.end = None;
                vec![Action::Begin(if self.feather { "Gradient Feather".into() } else { "Gradient".into() })]
            }
            PointerKind::Drag => {
                let Some((start, sp, ids, spread)) = &self.drag else { return vec![] };
                let end = constrain(*start, ev.pos, ev.mods);
                self.end = Some(end);
                if (end - *start).hypot() < cx.tol(2.0) {
                    return vec![];
                }
                let to = *sp + cx.layout.delta_to_spread(*spread, end - *start);
                let cmd = if self.feather { "object.gradientFeather" } else { "object.gradient" };
                vec![Action::Preview(cmd.into(), json!({"ids": ids, "from": [sp.x, sp.y], "to": [to.x, to.y]}))]
            }
            PointerKind::Up => {
                let moved = self.end.zip(self.drag.as_ref()).is_some_and(|(e, (s, ..))| (e - *s).hypot() >= cx.tol(2.0));
                let was = self.drag.take().is_some();
                self.end = None;
                match (was, moved) {
                    (true, true) => vec![Action::Commit],
                    (true, false) => vec![Action::Cancel],
                    _ => vec![],
                }
            }
            _ => vec![],
        }
    }

    fn overlays(&self, _cx: &ToolContext) -> Vec<Overlay> {
        match (&self.drag, self.end) {
            (Some((a, ..)), Some(b)) => vec![Overlay::Line { a: *a, b, color: [0, 0, 0], dashed: false }],
            _ => vec![],
        }
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _m: Mods) -> Cursor {
        Cursor::Crosshair
    }

    fn busy(&self) -> bool {
        self.drag.is_some()
    }
}
