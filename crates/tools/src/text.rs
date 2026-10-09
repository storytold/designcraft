//! Type tool (T): click in a frame to place the caret, drag in a frame to select text, drag on
//! empty canvas to draw a new text frame. Keys move the caret / delete; typing goes through
//! `text.insert` (sent by the UI as text input).

use designcraft_geom::Point;
use serde_json::json;

use crate::{Action, Cursor, Mods, PointerEvent, PointerKind, Tool, ToolContext, ToolKey, frame::drag_rect, rect_json, spread_json};

#[derive(Default)]
pub struct TypeTool {
    /// Vertical Type Tool: frames it draws set text vertically.
    pub vertical: bool,
    /// Horizontal / Vertical Grid Tool: frames it draws are frame grids (whole cells).
    pub grid: bool,
    start: Option<Point>,
    selecting: Option<u64>,
    drawing: bool,
    /// A press on a table whose whole rows or columns are selected: (frame, press point in
    /// spread space) — a drag moves them, a click places the caret.
    cell_drag: Option<(u64, Point)>,
}

/// Are whole rows or whole columns of a table selected?
fn whole_rows_or_cols(cx: &ToolContext) -> bool {
    let Some(ts) = cx.selection.cells else { return false };
    let Some(t) = cx.doc.story(ts.story).and_then(|s| s.tables.get(&ts.table)) else { return false };
    let r = ts.range;
    (r.c0 == 0 && r.c1 + 1 == t.ncols()) || (r.r0 == 0 && r.r1 + 1 == t.nrows())
}

impl TypeTool {
    pub fn vertical() -> Self {
        TypeTool { vertical: true, ..Default::default() }
    }
    /// Horizontal Grid Tool (`vertical` false) or Vertical Grid Tool.
    pub fn grid(vertical: bool) -> Self {
        TypeTool { vertical, grid: true, ..Default::default() }
    }
}

impl Tool for TypeTool {
    fn id(&self) -> &'static str {
        match (self.grid, self.vertical) {
            (true, false) => "horizontalGrid",
            (true, true) => "verticalGrid",
            (false, true) => "verticalType",
            (false, false) => "type",
        }
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down | PointerKind::DoubleClick => {
                self.start = Some(ev.pos);
                self.drawing = false;
                self.selecting = None;
                self.cell_drag = None;
                if let Some((_, id)) = cx.hit(ev.pos)
                    && let Some(it) = cx.doc.item(id)
                    && !matches!(it.content, designcraft_doc::Content::Graphic(_) | designcraft_doc::Content::Group { .. })
                {
                    let sp = cx.layout.spread_at(ev.pos).map(|(_, p)| p).unwrap_or(ev.pos);
                    // Whole rows/columns selected: wait to see whether this is a drag.
                    if ev.kind == PointerKind::Down && !ev.mods.shift && whole_rows_or_cols(cx) {
                        self.cell_drag = Some((id.0, sp));
                        return vec![];
                    }
                    self.selecting = Some(id.0);
                    let cmd = if ev.kind == PointerKind::DoubleClick {
                        "text.selectWord"
                    } else if ev.mods.shift {
                        "text.extendTo"
                    } else {
                        "text.placeCaret"
                    };
                    return vec![Action::Exec(cmd.into(), json!({"frame": id.0, "point": [sp.x, sp.y]}))];
                }
                vec![]
            }
            PointerKind::Drag => {
                let Some(a) = self.start else { return vec![] };
                if self.cell_drag.is_some() {
                    return vec![];
                }
                if let Some(fid) = self.selecting {
                    let sp = cx.layout.spread_at(ev.pos).map(|(_, p)| p).unwrap_or(ev.pos);
                    return vec![Action::Exec("text.extendTo".into(), json!({"frame": fid, "point": [sp.x, sp.y]}))];
                }
                if (ev.pos - a).hypot() < cx.tol(3.0) && !self.drawing {
                    return vec![];
                }
                let Some((sr, sa)) = cx.layout.spread_at(a) else { return vec![] };
                let r = drag_rect(sa, cx.layout.to_spread(sr, ev.pos), ev.mods);
                let mut out = vec![];
                if !self.drawing {
                    self.drawing = true;
                    out.push(Action::Begin(if self.grid { "Create Frame Grid" } else { "Create Text Frame" }.into()));
                }
                let mut p = json!({"spread": spread_json(sr), "shape": "rectangle", "content": "text", "rect": rect_json(r), "caret": true, "vertical": self.vertical});
                if self.grid {
                    // The engine fits the frame to whole cells of the document's frame grid.
                    p["grid"] = json!(true);
                }
                out.push(Action::Preview("frame.create".into(), p));
                out
            }
            PointerKind::Up => {
                let start = self.start.take();
                if let Some((fid, from)) = self.cell_drag.take() {
                    let sp = cx.layout.spread_at(ev.pos).map(|(_, p)| p).unwrap_or(ev.pos);
                    let moved = start.is_some_and(|a| (ev.pos - a).hypot() >= cx.tol(3.0));
                    return if moved {
                        vec![Action::Exec("table.dropCells".into(), json!({"frame": fid, "from": [from.x, from.y], "to": [sp.x, sp.y]}))]
                    } else {
                        vec![Action::Exec("text.placeCaret".into(), json!({"frame": fid, "point": [from.x, from.y]}))]
                    };
                }
                if std::mem::take(&mut self.drawing) {
                    self.selecting = None;
                    return vec![Action::Commit];
                }
                // Ends a press in text: drops dragged text, or is ignored by the engine.
                if let Some(fid) = self.selecting.take() {
                    let sp = cx.layout.spread_at(ev.pos).map(|(_, p)| p).unwrap_or(ev.pos);
                    let moved = start.is_some_and(|a| (ev.pos - a).hypot() >= cx.tol(3.0));
                    return vec![Action::Exec(
                        "text.release".into(),
                        json!({"frame": fid, "point": [sp.x, sp.y], "moved": moved, "copy": ev.mods.alt}),
                    )];
                }
                vec![]
            }
            PointerKind::Move => vec![],
        }
    }

    fn key(&mut self, cx: &ToolContext, key: ToolKey, mods: Mods) -> Vec<Action> {
        if cx.selection.text.is_none() {
            return vec![];
        }
        let mv = |dir: &str| {
            vec![Action::Exec("text.move".into(), json!({"dir": dir, "extend": mods.shift, "word": mods.alt || mods.ctrl, "far": mods.cmd}))]
        };
        match key {
            ToolKey::Left => mv("left"),
            ToolKey::Right => mv("right"),
            ToolKey::Up => mv("up"),
            ToolKey::Down => mv("down"),
            ToolKey::Home => mv("lineStart"),
            ToolKey::End => mv("lineEnd"),
            ToolKey::Backspace => vec![Action::Exec("text.delete".into(), json!({"forward": false, "word": mods.alt}))],
            ToolKey::Delete => vec![Action::Exec("text.delete".into(), json!({"forward": true, "word": mods.alt}))],
            ToolKey::Enter => vec![Action::Exec("text.insert".into(), json!({"text": if mods.shift { "\u{2028}" } else { "\n" }}))],
            ToolKey::Tab => vec![Action::Exec("text.insert".into(), json!({"text": "\t"}))],
            ToolKey::Escape => vec![Action::SwitchTool("selection".into()), Action::Exec("text.exitToFrame".into(), json!({}))],
            _ => vec![],
        }
    }

    fn cursor(&self, cx: &ToolContext, p: Point, _m: Mods) -> Cursor {
        match cx.hit(p) {
            Some((_, id)) if cx.doc.item(id).is_some_and(|i| i.is_text_frame()) => Cursor::Text,
            _ => Cursor::Text,
        }
    }

    fn busy(&self) -> bool {
        self.drawing
    }

    fn wants_text(&self, cx: &ToolContext) -> bool {
        cx.selection.text.is_some()
    }
}
