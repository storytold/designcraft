//! Type tool (T): click in a frame to place the caret, drag in a frame to select text, drag on
//! empty canvas to draw a new text frame. Keys move the caret / delete; typing goes through
//! `text.insert` (sent by the UI as text input).
//!
//! IME: the marked text of a composition is a preview of `text.insert` at the caret (so it lays
//! out in place like typed text), underlined, until the IME commits it (one `text.insert`, one
//! undo step) or clears it (the preview goes, leaving no undo step).

use std::ops::Range;

use designcraft_compose as compose;
use designcraft_doc::{ItemId, TextSel};
use designcraft_geom::{Affine, Point, Vec2};
use serde_json::json;

use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext, ToolKey, frame::snapped_drag_rect, rect_json, spread_json};

/// IME marked text shown at the caret through a preview of `text.insert`, not yet typed.
#[derive(Clone, Debug, PartialEq)]
struct Preedit {
    text: String,
    /// The clause being converted (empty: the IME's cursor), in bytes of `text`.
    active: Option<Range<usize>>,
}

#[derive(Default)]
pub struct TypeTool {
    /// Vertical Type Tool: frames it draws set text vertically.
    pub vertical: bool,
    start: Option<Point>,
    selecting: Option<u64>,
    drawing: bool,
    /// A press on a table whose whole rows or columns are selected: (frame, press point in
    /// spread space) — a drag moves them, a click places the caret.
    cell_drag: Option<(u64, Point)>,
    /// Snap guides while drawing a frame.
    guides: Vec<Overlay>,
    /// The press selected a word, line, paragraph or story (several clicks): drags keep it.
    unit_selected: bool,
    preedit: Option<Preedit>,
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
}

/// Byte range in `s` of the characters `r` (IME ranges count characters; carets count bytes).
/// `None` when `r` runs past the end of `s` or backwards.
fn char_range_to_bytes(s: &str, r: Range<usize>) -> Option<Range<usize>> {
    let n = s.chars().count();
    let byte = |c: usize| if c == n { Some(s.len()) } else { s.char_indices().nth(c).map(|(i, _)| i) };
    let (a, b) = (byte(r.start)?, byte(r.end)?);
    (a <= b).then_some(a..b)
}

/// Text space → canvas for frame `fid` (with the quarter turn of a vertical frame).
fn text_canvas_xf(cx: &ToolContext, fid: ItemId) -> Option<Affine> {
    Some(cx.item_canvas_xf(fid)? * cx.doc.text_xf(cx.doc.item(fid)?))
}

/// Where the marked text starts: the preview typed it just before the caret.
fn marked_start(cx: &ToolContext, ts: TextSel, p: &Preedit) -> usize {
    let text = cx.doc.text_story(ts.story, ts.cell).map_or("", |s| s.text.as_str());
    designcraft_doc::story::floor_char_boundary(text, ts.focus.saturating_sub(p.text.len()))
}

/// The caret line (top, bottom) at byte `pos` of the selection's text, in canvas space; `None`
/// when `pos` isn't composed (overset).
fn caret_line(cx: &ToolContext, ts: TextSel, pos: usize) -> Option<(Point, Point)> {
    let cs = cx.cache.get(cx.doc, ts.story, None);
    let (fi, x, bl, asc, desc) = match ts.cell {
        Some(c) => match c.footnote_id() {
            Some(id) => compose::note_caret(&cs, id, pos)?,
            None => compose::cell_caret(&cs, c.table, c.row, c.col, pos)?,
        },
        None => compose::caret(&cs, pos)?,
    };
    let m = text_canvas_xf(cx, cs.frames.get(fi)?.frame)?;
    Some((m * Point::new(x, bl - asc), m * Point::new(x, bl + desc)))
}

/// Underlines (start, end in canvas space) under bytes `r` of the selection's text.
fn underlines(cx: &ToolContext, ts: TextSel, r: Range<usize>) -> Vec<(Point, Point)> {
    let mut out = Vec::new();
    let mut add = |lines: &[compose::Line], m: Affine| {
        for l in lines {
            let (s, e) = (r.start.max(l.range.start), r.end.min(l.range.end));
            if s < e {
                out.extend(compose::highlight_quads(l, s, e, false).into_iter().map(|q| (m * q[3], m * q[2])));
            }
        }
    };
    let cs = cx.cache.get(cx.doc, ts.story, None);
    match ts.cell {
        // A cell's or footnote's text is composed on its own and placed in the frame.
        Some(c) => {
            let found = match c.footnote_id() {
                Some(id) => compose::find_note(&cs, id).map(|(fi, n)| (fi, n.origin, n.text.clone())),
                None => compose::find_cell(&cs, c.table, c.row, c.col).map(|(fi, _, pc)| (fi, pc.origin, pc.text.clone())),
            };
            if let Some((fi, origin, text)) = found
                && let Some(m) = cs.frames.get(fi).and_then(|ft| text_canvas_xf(cx, ft.frame))
                && let Some(ft) = text.frames.first()
            {
                add(&ft.lines, m * Affine::translate(origin.to_vec2()));
            }
        }
        None => {
            for ft in &cs.frames {
                if let Some(m) = text_canvas_xf(cx, ft.frame) {
                    add(&ft.lines, m);
                }
            }
        }
    }
    out
}

impl Tool for TypeTool {
    fn id(&self) -> &'static str {
        if self.vertical { "verticalType" } else { "type" }
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Down | PointerKind::DoubleClick => {
                self.start = Some(ev.pos);
                self.drawing = false;
                self.selecting = None;
                self.cell_drag = None;
                self.unit_selected = false;
                if let Some((_, id)) = cx.hit(ev.pos)
                    && let Some(it) = cx.doc.item(id)
                    && !matches!(it.content, designcraft_doc::Content::Graphic(_) | designcraft_doc::Content::Group { .. })
                {
                    let sp = cx.layout.spread_at(ev.pos).map(|(_, p)| p).unwrap_or(ev.pos);
                    let clicks = ev.click_count();
                    // Whole rows/columns selected: wait to see whether this is a drag.
                    if clicks == 1 && !ev.mods.shift && whole_rows_or_cols(cx) {
                        self.cell_drag = Some((id.0, sp));
                        return vec![];
                    }
                    self.selecting = Some(id.0);
                    // Two presses select a word (or a run of spaces), three a line, four a
                    // paragraph, five the story.
                    if clicks >= 2 {
                        self.unit_selected = true;
                        let unit = match clicks {
                            2 => "word",
                            3 => "line",
                            4 => "paragraph",
                            _ => "story",
                        };
                        return vec![Action::Exec("text.selectAt".into(), json!({"frame": id.0, "point": [sp.x, sp.y], "unit": unit}))];
                    }
                    let cmd = if ev.mods.shift { "text.extendTo" } else { "text.placeCaret" };
                    return vec![Action::Exec(cmd.into(), json!({"frame": id.0, "point": [sp.x, sp.y]}))];
                }
                vec![]
            }
            PointerKind::Drag => {
                let Some(a) = self.start else { return vec![] };
                if self.cell_drag.is_some() || self.unit_selected {
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
                // Drawn like the frame tools: the press point and the moving edges snap.
                let sa = if cx.snap.any() { crate::snap::snap_point(cx, sr, sa).0 } else { sa };
                let (r, guides) = snapped_drag_rect(cx, sr, sa, cx.layout.to_spread(sr, ev.pos), ev.mods);
                self.guides = guides;
                let mut out = vec![];
                if !self.drawing {
                    self.drawing = true;
                    out.push(Action::Begin("Create Text Frame".into()));
                }
                out.push(Action::Preview(
                    "frame.create".into(),
                    json!({"spread": spread_json(sr), "shape": "rectangle", "content": "text", "rect": rect_json(r), "caret": true, "vertical": self.vertical}),
                ));
                out
            }
            PointerKind::Up => {
                self.guides.clear();
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
        // The IME owns the keys while it composes (the UI holds them back; this is a guard).
        if cx.selection.text.is_none() || self.preedit.is_some() {
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

    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        if self.drawing {
            return self.guides.clone();
        }
        let (Some(p), Some(ts)) = (&self.preedit, cx.selection.text) else { return vec![] };
        // Marked text: a thin underline, a thick one under the clause being converted.
        let start = marked_start(cx, ts, p);
        let line = |(a, b): (Point, Point)| Overlay::Line { a, b, color: [0, 0, 0], dashed: false };
        let mut o: Vec<Overlay> = underlines(cx, ts, start..start + p.text.len()).into_iter().map(line).collect();
        match &p.active {
            Some(r) if !r.is_empty() => {
                for (a, b) in underlines(cx, ts, start + r.start..start + r.end) {
                    // A second line a pixel further from the text.
                    let d = b - a;
                    let n = Vec2::new(-d.y, d.x) * (cx.tol(1.0) / d.hypot().max(1e-9));
                    o.push(line((a + n, b + n)));
                }
            }
            // The IME's cursor inside the marked text (the text caret stays at its end).
            Some(r) if r.start < p.text.len() => o.extend(caret_line(cx, ts, start + r.start).map(line)),
            _ => {}
        }
        o
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

    fn ime_preedit(&mut self, cx: &ToolContext, text: &str, active_chars: Option<Range<usize>>) -> Vec<Action> {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        // A cancelled composition: the preview goes, leaving no undo step. A clear with nothing
        // marked (some integrations send one when the IME is enabled) changes nothing, so it never
        // deletes the selected text.
        if text.is_empty() || cx.selection.text.is_none() {
            return if self.preedit.take().is_some() { vec![Action::Cancel] } else { vec![] };
        }
        let mut out = Vec::new();
        if self.preedit.is_none() {
            out.push(Action::Begin("Type".into()));
        }
        // Each update replaces the previous marked text (or the selected text). Marked text shows
        // as it is: typographer's quotes and autocorrect wait for the commit.
        out.push(Action::Preview("text.insert".into(), json!({"text": text, "raw": true})));
        self.preedit = Some(Preedit { active: active_chars.and_then(|r| char_range_to_bytes(&text, r)), text });
        out
    }

    fn ime_commit(&mut self, cx: &ToolContext, text: &str) -> Vec<Action> {
        let text: String = text.chars().map(|c| if c == '\r' { '\n' } else { c }).filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
        // The marked text goes; the result is typed in its place like any typing: one command,
        // one undo step.
        let mut out = if self.preedit.take().is_some() { vec![Action::Cancel] } else { vec![] };
        if !text.is_empty() && cx.selection.text.is_some() {
            out.push(Action::Exec("text.insert".into(), json!({"text": text})));
        }
        out
    }

    fn ime_end(&mut self, cx: &ToolContext) -> Vec<Action> {
        match self.preedit.as_ref().map(|p| p.text.clone()) {
            Some(text) => self.ime_commit(cx, &text),
            None => vec![],
        }
    }

    fn composing(&self) -> bool {
        self.preedit.is_some()
    }

    fn ime_caret(&self, cx: &ToolContext) -> Option<(Point, Point)> {
        let ts = cx.selection.text?;
        // The candidate window follows the clause being converted (or the IME's cursor), else the
        // caret.
        let at = match &self.preedit {
            Some(p) => marked_start(cx, ts, p) + p.active.as_ref().map_or(0, |r| r.start),
            None => ts.range().start,
        };
        caret_line(cx, ts, at).or_else(|| {
            // Overset text has no caret on a page: the window goes to the out port of the story's
            // last frame.
            let fid = cx.doc.story(ts.story).and_then(|s| s.frames.last().copied()).or(ts.frame)?;
            let it = cx.doc.item(fid)?;
            let m = cx.item_canvas_xf(fid)? * it.xf;
            let r = it.inner_bounds();
            Some((m * Point::new(r.x1, r.y1 - 12.0), m * Point::new(r.x1, r.y1)))
        })
    }
}

#[cfg(test)]
mod ime_tests {
    use super::char_range_to_bytes;

    #[test]
    fn ime_character_ranges_become_byte_ranges() {
        assert_eq!(char_range_to_bytes("雅楽", 0..2), Some(0..6));
        assert_eq!(char_range_to_bytes("がg", 1..2), Some(3..4));
        assert_eq!(char_range_to_bytes("がg", 2..2), Some(4..4));
        // Past the end or backwards: no clause.
        assert_eq!(char_range_to_bytes("が", 1..3), None);
        #[allow(clippy::reversed_empty_ranges)]
        let backwards = 1..0;
        assert_eq!(char_range_to_bytes("がg", backwards), None);
    }
}
