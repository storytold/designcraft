//! Data merge grid tool. A drag on a document page calls `data.grid.create` with that rectangle
//! and no other fields, so the grid uses the same defaults as the Create Grid button. The tool
//! does not snap. A click does not create a grid. A parent spread is left alone.

use designcraft_doc::SpreadRef;
use designcraft_geom::{Point, Rect};

use crate::{Action, Cursor, Mods, PointerEvent, PointerKind, Tool, ToolContext, rect_json};

#[derive(Default)]
pub struct DataGridTool {
    start: Option<Point>,
    active: bool,
}

fn dragged(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

impl Tool for DataGridTool {
    fn id(&self) -> &'static str {
        "dataGrid"
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down => {
                self.start = Some(ev.pos);
                self.active = false;
                vec![]
            }
            PointerKind::Drag => {
                let Some(a) = self.start else { return vec![] };
                if !self.active && (ev.pos - a).hypot() < cx.tol(3.0) {
                    return vec![];
                }
                let Some((sr, origin)) = cx.layout.spread_at(a) else { return vec![] };
                let SpreadRef::Doc(index) = sr else {
                    if self.active {
                        self.active = false;
                        return vec![Action::Cancel];
                    }
                    return vec![];
                };
                let end = cx.layout.to_spread(sr, ev.pos);
                let rect = dragged(origin, end);
                if rect.width() < 1.0 || rect.height() < 1.0 {
                    if self.active {
                        self.active = false;
                        return vec![Action::Cancel];
                    }
                    return vec![];
                }
                let mut out = Vec::new();
                if !self.active {
                    self.active = true;
                    out.push(Action::Begin("Create Grid".into()));
                }
                out.push(Action::Preview("data.grid.create".into(), serde_json::json!({"rect": rect_json(rect), "spread": index})));
                out
            }
            PointerKind::Up => {
                let was = self.active;
                self.active = false;
                self.start = None;
                if was { vec![Action::Commit] } else { vec![] }
            }
            _ => vec![],
        }
    }

    fn cursor(&self, _cx: &ToolContext, _p: Point, _mods: Mods) -> Cursor {
        Cursor::Crosshair
    }

    fn busy(&self) -> bool {
        self.active
    }
}
