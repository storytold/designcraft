use super::{
    CellAddr, Content, ItemId, Point, Result, Selection, Session, StoryId, TextSel, Value, bad, bool_or, compose, floor_char_boundary, id_param,
    json, ok, point_param, str_param,
};

pub(crate) fn story_of(s: &Session, p: &Value) -> Option<StoryId> {
    let st = s.active()?;
    if let Some(v) = p.get("story").and_then(Value::as_u64) {
        return Some(StoryId(v));
    }
    let frame = id_param(p, "frame").or_else(|| st.selection.items.first().copied());
    if let Some(f) = frame {
        return st.doc.item(f).and_then(|i| i.text_frame()).map(|t| t.story);
    }
    st.selection.text.map(|t| t.story)
}

/// Story byte at spread point `pt` in text frame `frame` (in a table cell's story when the point is in a cell).
fn hit_byte(s: &Session, frame: ItemId, pt: Point) -> Option<(StoryId, usize, Option<CellAddr>)> {
    hit_byte_with(s, frame, pt, compose::hit)
}

/// [`hit_byte`] with the hit test: [`compose::hit`] (caret position) or [`compose::hit_char`]
/// (the character under the point).
pub(super) fn hit_byte_with(
    s: &Session,
    frame: ItemId,
    pt: Point,
    hit: fn(&compose::ComposedStory, usize, Point) -> Option<usize>,
) -> Option<(StoryId, usize, Option<CellAddr>)> {
    let st = s.active()?;
    let loc = st.doc.find(frame)?;
    let it = st.doc.item_at(&loc)?;
    let sid = it.text_frame()?.story;
    let xf = st.doc.parent_xf(&loc) * st.doc.text_xf(it);
    let inner = xf.inverse() * pt;
    let cs = s.cache.get(&st.doc, sid, None);
    let fi = cs.frames.iter().position(|f| f.frame == frame)?;
    // Map path-text hits along the curve before choosing the character unit.
    let inner = cs.frames.get(fi).and_then(|ft| compose::PathTextMap::new(it, ft)).map_or(inner, |m| m.to_line(inner));
    if let Some((table, row, col, b)) = compose::hit_cell_with(&cs, fi, inner, hit) {
        return Some((sid, b, Some(CellAddr { table, row, col })));
    }
    if let Some((id, b)) = compose::hit_note_with(&cs, fi, inner, hit) {
        return Some((sid, b, Some(CellAddr::footnote(id))));
    }
    let b = hit(&cs, fi, inner).unwrap_or(0);
    Some((sid, b, None))
}

/// Select the word, line, paragraph or story at `point` in `frame` (multiple clicks).
pub(super) fn select_at(s: &mut Session, p: &Value, unit: &str) -> Result<Value> {
    const ID: &str = "text.selectAt";
    let frame = id_param(p, "frame").ok_or_else(|| bad(ID, "missing frame"))?;
    let pt = point_param(p, "point").ok_or_else(|| bad(ID, "missing point"))?;
    let (sid, b, cell) = hit_byte_with(s, frame, pt, compose::hit_char).ok_or_else(|| bad(ID, "not a text frame"))?;
    let (a, e) = unit_at(s, sid, cell, b, unit, ID)?;
    s.text_drag = false;
    s.typing_format = None;
    let sel = TextSel { story: sid, anchor: a, focus: e, frame: Some(frame), cell };
    s.text_unit = Some(sel);
    let st = s.doc_mut()?;
    st.selection = Selection::text(sel);
    // Explicit reselection invalidates the Find scope even when the range is unchanged.
    st.bump_revision();
    Ok(json!({"story": sid.0, "anchor": a, "focus": e}))
}

/// The `unit` (word, line, paragraph or story) around byte `b` of story `sid` (a cell's story
/// with `cell`), as the multiple clicks select it.
fn unit_at(s: &Session, sid: StoryId, cell: Option<CellAddr>, b: usize, unit: &str, cmd: &str) -> Result<(usize, usize)> {
    let st = s.doc()?;
    let story = st.doc.text_story(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let text = story.text.as_str();
    let b = floor_char_boundary(text, b);
    Ok(match unit {
        "word" => word_or_spaces_at(text, b),
        "line" => {
            let cs = s.cache.get(&st.doc, sid, None);
            line_at(&text_composition(cs, cell), b).unwrap_or((b, b))
        }
        // The paragraph with its return (the last paragraph has none).
        "paragraph" => story.para_ranges().get(story.para_at(b)).map_or((b, b), |r| (r.start, (r.end + 1).min(text.len()))),
        "story" => (0, text.len()),
        other => return Err(bad(cmd, format!("unknown unit `{other}` (word, line, paragraph or story)"))),
    })
}

/// The word at byte `i`; on spaces the run of spaces; on anything else that one character
/// (grapheme cluster). At the end of a paragraph, what ends it. Grapheme clusters are never split:
/// combining marks, viramas, variation selectors and emoji ZWJ sequences stay with their base.
fn word_or_spaces_at(s: &str, i: usize) -> (usize, usize) {
    use unicode_segmentation::UnicodeSegmentation;
    let i = floor_char_boundary(s, i.min(s.len()));
    // Clusters never span a paragraph return, so the paragraph is enough.
    let ps = s[..i].rfind('\n').map_or(0, |k| k + 1);
    let pe = s[i..].find('\n').map_or(s.len(), |k| i + k);
    let g: Vec<(usize, &str)> = s[ps..pe].grapheme_indices(true).collect();
    let Some(k) = g.iter().rposition(|(o, _)| ps + o <= i) else { return (i, i) };
    let kind = |c: &str| match c.chars().next() {
        Some(c) if is_word(c) => 1,
        Some(c) if c.is_whitespace() => 2,
        _ => 0,
    };
    let span = |a: usize, e: usize| g.get(a).zip(g.get(e)).map_or((i, i), |((oa, _), (oe, ge))| (ps + oa, ps + oe + ge.len()));
    let kd = g.get(k).map_or(0, |(_, c)| kind(c));
    if kd == 0 {
        return span(k, k);
    }
    let mut a = k;
    while a > 0 && g.get(a - 1).is_some_and(|(_, c)| kind(c) == kd) {
        a -= 1;
    }
    let mut e = k;
    while g.get(e + 1).is_some_and(|(_, c)| kind(c) == kd) {
        e += 1;
    }
    span(a, e)
}

/// The composition whose lines hold a story's text: the story's own, a table cell's or a footnote's.
fn text_composition(cs: std::sync::Arc<compose::ComposedStory>, cell: Option<CellAddr>) -> std::sync::Arc<compose::ComposedStory> {
    match cell {
        Some(c) if c.footnote_id().is_some() => compose::find_note(&cs, c.row as u64).map(|(_, n)| n.text.clone()).unwrap_or(cs),
        Some(c) => compose::find_cell(&cs, c.table, c.row, c.col).map(|(_, _, pc)| pc.text.clone()).unwrap_or(cs),
        None => cs,
    }
}

/// The composed line holding the character at byte `pos`.
fn line_at(cs: &compose::ComposedStory, pos: usize) -> Option<(usize, usize)> {
    let lines = || cs.frames.iter().flat_map(|f| f.lines.iter());
    lines().find(|l| l.range.contains(&pos)).map(|l| (l.range.start, l.range.end)).or_else(|| line_of(cs, pos))
}

pub(super) fn place(s: &mut Session, p: &Value, extend: bool) -> Result<Value> {
    let frame = id_param(p, "frame").ok_or_else(|| bad("text.placeCaret", "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    // Empty frames become text frames; a locked one stays as it is.
    let st = s.doc()?;
    let locked = st.doc.item(frame).is_some_and(|i| i.locked || st.doc.layer(i.layer).is_some_and(|l| l.locked));
    let needs_convert = st.doc.item(frame).is_some_and(|i| matches!(i.content, Content::Unassigned));
    if needs_convert && locked {
        return Err(bad("text.placeCaret", "the frame is locked"));
    }
    if needs_convert {
        s.execute("object.content", &json!({"ids": [frame.0], "type": "text"}))?;
    }
    let (sid, b, cell) = hit_byte(s, frame, pt).ok_or_else(|| bad("text.placeCaret", "not a text frame"))?;
    // A drag after several clicks: whole units from the one first selected, in either direction.
    if extend
        && let Some(unit) = str_param(p, "unit")
        && let Some(u) = s.text_unit.filter(|u| u.story == sid && u.cell == cell)
    {
        // The remembered unit may predate edits that shortened the text.
        let text = s.doc()?.doc.text_story(sid, cell).map_or("", |t| t.text.as_str());
        let (ua, ue) = (floor_char_boundary(text, u.anchor.min(u.focus)), floor_char_boundary(text, u.anchor.max(u.focus)));
        let (na, ne) = unit_at(s, sid, cell, b, unit, "text.extendTo")?;
        let (anchor, focus) = if na < ua { (ue, na) } else { (ua, ne.max(ue)) };
        s.typing_format = None;
        let st = s.doc_mut()?;
        st.selection = Selection::text(TextSel { story: sid, anchor, focus, frame: Some(frame), cell });
        st.bump_revision();
        return Ok(json!({"story": sid.0, "anchor": anchor, "focus": focus}));
    }
    // Drag and drop: a press inside the selected text keeps it (to be dragged).
    if !extend {
        s.text_drag = s.doc()?.selection.text.is_some_and(|t| t.story == sid && t.cell == cell && !t.range().is_empty() && t.range().contains(&b));
        if s.text_drag {
            return Ok(json!({"story": sid.0, "dragging": true}));
        }
    } else if s.text_drag {
        return Ok(json!({"story": sid.0, "dragging": true}));
    }
    s.typing_format = None;
    let st = s.doc_mut()?;
    // Dragging from one cell into another selects cells.
    if extend
        && let (Some(t), Some(c)) = (st.selection.text, cell)
        && let Some(a) = t.cell
        && t.story == sid
        && a.table == c.table
        && (a.row, a.col) != (c.row, c.col)
    {
        let range = designcraft_doc::CellRange::new(a.row, a.col, c.row, c.col);
        st.selection.cells = Some(designcraft_doc::TableSel { story: sid, table: c.table, range });
        st.bump_revision();
        return Ok(json!({"story": sid.0, "cells": range}));
    }
    let t = match (extend, st.selection.text) {
        (true, Some(t)) if t.story == sid && t.cell == cell => TextSel { focus: b, frame: Some(frame), ..t },
        _ => TextSel { story: sid, anchor: b, focus: b, frame: Some(frame), cell },
    };
    st.selection = Selection::text(t);
    st.bump_revision();
    Ok(json!({"story": sid.0, "pos": b, "cell": cell}))
}

pub(super) fn release(s: &mut Session, p: &Value) -> Result<Value> {
    if !std::mem::take(&mut s.text_drag) {
        return ok();
    }
    let frame = id_param(p, "frame").ok_or_else(|| bad("text.release", "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    if !p.get("moved").and_then(Value::as_bool).unwrap_or(false) {
        // A click in the selection: the caret goes there.
        s.doc_mut()?.selection.text = None;
        return place(s, &json!({"frame": frame.0, "point": [pt.x, pt.y]}), false);
    }
    let copy = p.get("copy").and_then(Value::as_bool).unwrap_or(false);
    let Some(t) = s.doc()?.selection.text else { return ok() };
    let Some((sid, b, cell)) = hit_byte(s, frame, pt) else { return ok() };
    let r = t.range();
    if sid != t.story || cell != t.cell || (r.contains(&b) && !copy) {
        return ok();
    }
    s.edit(|d, sel| {
        let st = d.text_story_mut(sid, cell).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let piece = st.extract(r.clone());
        let mut at = b;
        if !copy {
            st.delete(r.clone());
            if at > r.start {
                at -= r.len().min(at - r.start);
            }
        }
        let end = st.insert_story(at, &piece);
        sel.text = Some(TextSel { story: sid, anchor: at, focus: end, frame: Some(frame), cell });
        Ok(json!({"moved": piece.text.len(), "at": at}))
    })
}

pub fn next_char(s: &str, i: usize) -> usize {
    s[i.min(s.len())..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(s.len())
}

pub fn prev_char(s: &str, i: usize) -> usize {
    s[..i.min(s.len())].chars().next_back().map(|c| i - c.len_utf8()).unwrap_or(0)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’'
}

pub fn next_word(s: &str, i: usize) -> usize {
    let mut j = i;
    let mut seen = false;
    for (k, c) in s[i..].char_indices() {
        if is_word(c) {
            seen = true;
        } else if seen {
            return i + k;
        }
        j = i + k + c.len_utf8();
    }
    j
}

pub fn prev_word(s: &str, i: usize) -> usize {
    let mut seen = false;
    for (k, c) in s[..i].char_indices().rev() {
        if is_word(c) {
            seen = true;
        } else if seen {
            return k + c.len_utf8();
        }
    }
    0
}

/// Selects `bounds(text, byte)` around the byte under `point` in `frame`. Unlike a caret
/// placement, a press inside the current selection is no start of a text drag here: it is the
/// third or later click of a row, which lands in the word or sentence just selected.
pub(super) fn select_around(s: &mut Session, p: &Value, id: &str, bounds: fn(&str, usize) -> (usize, usize)) -> Result<Value> {
    let frame = id_param(p, "frame").ok_or_else(|| bad(id, "missing frame"))?;
    let pt = point_param(p, "point").unwrap_or(Point::ZERO);
    let (sid, b, cell) = hit_byte(s, frame, pt).ok_or_else(|| bad(id, "not a text frame"))?;
    let st = s.doc_mut()?;
    let text = st.doc.text_story(sid, cell).map(|x| x.text.clone()).unwrap_or_default();
    let (anchor, focus) = bounds(&text, floor_char_boundary(&text, b));
    s.text_drag = false;
    s.typing_format = None;
    let st = s.doc_mut()?;
    st.selection = Selection::text(TextSel { story: sid, anchor, focus, frame: Some(frame), cell });
    st.bump_revision();
    Ok(json!({"story": sid.0, "anchor": anchor, "focus": focus}))
}

/// The paragraph around byte `i`, without its `\n`.
pub(super) fn para_bounds(s: &str, i: usize) -> (usize, usize) {
    let i = i.min(s.len());
    let a = s.get(..i).and_then(|t| t.rfind('\n')).map_or(0, |k| k + 1);
    let b = s.get(i..).and_then(|t| t.find('\n')).map_or(s.len(), |k| i + k);
    (a, b)
}

/// The sentence around byte `i`, within its paragraph: it ends after `.`, `!`, `?` or `…` (and
/// any closing quotes or brackets) followed by a space or the paragraph end, or after a CJK full
/// stop, and takes the spaces after it, as deleting a sentence should leave none behind.
pub(super) fn sentence_bounds(s: &str, i: usize) -> (usize, usize) {
    let (pa, pb) = para_bounds(s, i);
    let para = s.get(pa..pb).unwrap_or("");
    let mut start = 0;
    let mut it = para.char_indices().peekable();
    while let Some((_, c)) = it.next() {
        let cjk = matches!(c, '。' | '！' | '？');
        if !cjk && !matches!(c, '.' | '!' | '?' | '…') {
            continue;
        }
        while it.peek().is_some_and(|&(_, c)| {
            matches!(c, '.' | '!' | '?' | '…' | '。' | '！' | '？' | '"' | '\'' | '”' | '’' | '»' | '›' | ')' | ']' | '」' | '』' | '）')
        }) {
            it.next();
        }
        if !cjk && it.peek().is_some_and(|&(_, c)| !c.is_whitespace()) {
            continue;
        }
        while it.peek().is_some_and(|&(_, c)| c.is_whitespace()) {
            it.next();
        }
        let end = it.peek().map_or(para.len(), |&(k, _)| k);
        if pa + end > i || end == para.len() {
            return (pa + start, pa + end);
        }
        start = end;
    }
    (pa + start, pb)
}

pub(super) fn move_caret(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").unwrap_or("right").to_string();
    let extend = bool_or(p, "extend", false);
    let word = bool_or(p, "word", false);
    let st = s.doc()?;
    let t = st.selection.text.ok_or_else(|| bad("text.move", "no caret"))?;
    let story = st.doc.text_story(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
    let text = story.text.clone();
    // In a cell, lines come from the cell's own composition.
    let cs = text_composition(s.cache.get(&st.doc, t.story, None), t.cell);
    let pos = t.focus.min(text.len());
    // Vertical lines run top to bottom and follow each other right to left: Down and Up move along
    // the line (Right and Left of horizontal text), Left and Right to the next and previous line.
    let vertical_text = compose::caret(&cs, pos).and_then(|(fi, ..)| cs.frames.get(fi)).is_some_and(|f| f.vertical);
    let dir = match (vertical_text, dir.as_str()) {
        (true, "down") => "right",
        (true, "up") => "left",
        (true, "left") => "down",
        (true, "right") => "up",
        (_, d) => d,
    };
    let collapse_to = |left: bool| if left { t.range().start } else { t.range().end };
    let new = match dir {
        "left" if !extend && !t.is_caret() => collapse_to(true),
        "right" if !extend && !t.is_caret() => collapse_to(false),
        "left" | "right" => {
            // Lines with right-to-left text move as drawn; at a line's edge, or by words, a
            // right-to-left line's "left" is forward in the text.
            let left = dir == "left";
            let visual = if word { None } else { compose::visual_step(&cs, pos, left) };
            match visual {
                Some(p) => p,
                None => {
                    let forward = left == compose::line_rtl(&cs, pos);
                    match (forward, word) {
                        (true, true) => next_word(&text, pos),
                        (true, false) => next_char(&text, pos),
                        (false, true) => prev_word(&text, pos),
                        (false, false) => prev_char(&text, pos),
                    }
                }
            }
        }
        "up" | "down" => vertical(&cs, pos, dir == "up").unwrap_or(pos),
        "lineStart" => line_of(&cs, pos).map(|l| l.0).unwrap_or(0),
        "lineEnd" => line_of(&cs, pos).map(|l| l.1).unwrap_or(text.len()),
        "storyStart" => 0,
        "storyEnd" => text.len(),
        other => return Err(bad("text.move", format!("unknown dir `{other}`"))),
    };
    s.typing_format = None;
    let stm = s.doc_mut()?;
    stm.selection.text = Some(if extend { TextSel { focus: new, ..t } } else { TextSel { anchor: new, focus: new, ..t } });
    // Moving the caret leaves the selected cells, so typing inserts at the caret.
    stm.selection.cells = None;
    stm.bump_revision();
    Ok(json!({"pos": new}))
}

fn line_of(cs: &compose::ComposedStory, pos: usize) -> Option<(usize, usize)> {
    cs.frames.iter().flat_map(|f| f.lines.iter()).find(|l| pos >= l.range.start && pos <= l.range.end).map(|l| (l.range.start, l.range.end))
}

fn vertical(cs: &compose::ComposedStory, pos: usize, up: bool) -> Option<usize> {
    let (fi, x, caret_y, _, _) = compose::caret(cs, pos)?;
    let lines: Vec<(usize, &compose::Line)> = cs.frames.iter().enumerate().flat_map(|(i, f)| f.lines.iter().map(move |l| (i, l))).collect();
    // Match the line by the bytes it owns. A warichu caret sits on a small-line baseline, so it
    // does not equal the parent line's baseline.
    let cur = lines.iter().position(|(i, l)| *i == fi && pos >= l.range.start && pos <= l.range.end && (pos < l.range.end || l.last_in_para))?;
    let (_, l) = lines.get(cur)?;
    if let Some(y) = compose::adjacent_row(l, x, caret_y, up)
        && let Some(p) = compose::hit(cs, fi, Point::new(x, y))
        && p != pos
    {
        return Some(p);
    }
    // The next row: segments of the same line (beside a wrap object) share its baseline.
    let other_row = |(i, t): &&(usize, &compose::Line)| *i != fi || (t.baseline - l.baseline).abs() > 1e-6;
    let (tf, tl) = if up { lines.get(..cur)?.iter().rfind(other_row)? } else { lines.get(cur + 1..)?.iter().find(other_row)? };
    compose::hit(cs, *tf, Point::new(x, tl.baseline - 1.0))
}

#[cfg(test)]
mod multi_click_tests {
    use serde_json::{Value, json};

    use crate::Session;

    #[test]
    fn moving_the_caret_drops_the_pending_typing_format() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Hello world"})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        let size_at = |s: &Session, b: usize| s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().char_format_at(b).over.size;
        let pending = |s: &mut Session| {
            s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
            s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        };
        // Away and back by arrow keys, by a click, by selecting and by double-clicking: typing at the
        // same caret is in the text's own format.
        let moves: Vec<Box<dyn Fn(&mut Session)>> = vec![
            Box::new(|s| {
                s.execute("text.move", &json!({"dir": "right"})).unwrap();
                s.execute("text.move", &json!({"dir": "left"})).unwrap();
            }),
            Box::new(|s| {
                s.execute("text.placeCaret", &json!({"frame": fid, "point": [80, 80]})).unwrap();
                s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
            }),
            Box::new(|s| {
                s.execute("text.selectAt", &json!({"frame": fid, "point": [80, 80], "unit": "story"})).unwrap();
                s.execute("text.select", &json!({"story": sid, "anchor": 5})).unwrap();
            }),
        ];
        for m in &moves {
            pending(&mut s);
            m(&mut s);
            s.execute("text.insert", &json!({"text": "x"})).unwrap();
            assert_eq!(size_at(&s, 6), None);
            s.execute("edit.undo", &json!({})).unwrap();
        }
        // Without a move it applies.
        pending(&mut s);
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        assert_eq!(size_at(&s, 6), Some(30.0));
        // Selecting a sentence or paragraph around a point drops it too.
        for id in ["text.selectSentence", "text.selectParagraph"] {
            pending(&mut s);
            assert!(s.typing_format.is_some());
            s.execute(id, &json!({"frame": fid, "point": [80, 80]})).unwrap();
            assert!(s.typing_format.is_none(), "{id}");
        }
    }

    #[test]
    fn word_selection_keeps_grapheme_clusters_whole() {
        use super::word_or_spaces_at as word;
        // Decomposed "café": the combining acute belongs to the word, wherever the click lands.
        let s = "un cafe\u{301} noir";
        let cafe = (3, 3 + "cafe\u{301}".len());
        assert_eq!(word(s, 4), cafe);
        assert_eq!(word(s, 7), cafe, "on the combining mark");
        // Devanagari with a virama (U+094D) and a dependent vowel sign.
        let s = "\u{928}\u{92E}\u{938}\u{94D}\u{924}\u{947} x";
        let w = "\u{928}\u{92E}\u{938}\u{94D}\u{924}\u{947}".len();
        assert_eq!(word(s, 0), (0, w));
        assert_eq!(word(s, 9), (0, w), "on the virama");
        // An emoji ZWJ sequence and a variation selector stay one character.
        let s = "a \u{1F469}\u{200D}\u{1F4BB} b \u{263A}\u{FE0F}";
        let e = (2, 2 + "\u{1F469}\u{200D}\u{1F4BB}".len());
        assert_eq!(word(s, 2), e);
        assert_eq!(word(s, 6), e, "on the joiner");
        assert_eq!(word(s, s.len() - 1), (s.len() - 6, s.len()), "at the end: the last character whole");
        // Empty paragraphs select nothing.
        assert_eq!(word("a\n\nb", 2), (2, 2));
    }

    /// Issue #981: holding the button after a double, triple or quadruple click and moving extends
    /// the selection a word, line or paragraph at a time, from the unit first selected, either way.
    #[test]
    fn a_drag_after_several_clicks_extends_the_selection_by_units() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "Hello  brave world.\nSecond paragraph here.";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": text})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        let at = |s: &Session, b: usize| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let (l, g) = cs.frames[0].lines.iter().find_map(|l| l.glyphs.iter().find(|g| g.byte == b && g.len > 0).map(|g| (l, g))).unwrap();
            json!([g.x + g.adv * 0.75, l.baseline - 3.0])
        };
        let sel = |s: &Session| s.doc().unwrap().selection.text.map(|t| (t.anchor, t.focus)).unwrap();
        let extend = |s: &mut Session, b: usize, unit: &str| {
            let p = at(s, b);
            s.execute("text.extendTo", &json!({"frame": fid, "point": p, "unit": unit})).unwrap();
            sel(s)
        };
        // Double-click "brave", drag into "world.": whole words, anchored at the start of "brave".
        s.execute("text.selectAt", &json!({"frame": fid, "point": at(&s, 8), "unit": "word"})).unwrap();
        assert_eq!(sel(&s), (7, 12));
        assert_eq!(extend(&mut s, 14, "word"), (7, 18), "forward: through \"world\"");
        assert_eq!(extend(&mut s, 9, "word"), (7, 12), "back inside the word: just the word");
        assert_eq!(extend(&mut s, 1, "word"), (12, 0), "backward: from \"Hello\" to the end of \"brave\"");
        assert_eq!(&text[0..12], "Hello  brave");
        // Triple-click the first line, drag into the second: both lines.
        s.execute("text.selectAt", &json!({"frame": fid, "point": at(&s, 8), "unit": "line"})).unwrap();
        assert_eq!(sel(&s), (0, 19));
        assert_eq!(extend(&mut s, 22, "line"), (0, text.len()));
        // Without `unit` a drag still moves the focus a character at a time.
        s.execute("text.selectAt", &json!({"frame": fid, "point": at(&s, 8), "unit": "word"})).unwrap();
        s.execute("text.extendTo", &json!({"frame": fid, "point": at(&s, 14)})).unwrap();
        assert_eq!(sel(&s).0, 7);
        assert!(sel(&s).1 > 12 && sel(&s).1 < 18, "{:?}", sel(&s));
        assert!(s.execute("text.extendTo", &json!({"frame": fid, "point": at(&s, 14), "unit": "sentence"})).is_err());
        // A unit remembered before the text was deleted stays inside what is left.
        s.execute("text.selectAt", &json!({"frame": fid, "point": at(&s, 14), "unit": "word"})).unwrap();
        s.execute("story.setText", &json!({"story": sid, "text": ""})).unwrap();
        s.execute("text.extendTo", &json!({"frame": fid, "point": [100, 90], "unit": "word"})).unwrap();
        assert_eq!(sel(&s), (0, 0));
    }

    #[test]
    fn clicks_select_words_spaces_lines_paragraphs_and_the_story() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "Hello  brave world.\nSecond paragraph here.";
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": text})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        // Spread point over the right half of the glyph at byte `b`.
        let at = |s: &Session, b: usize| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let (l, g) = cs.frames[0].lines.iter().find_map(|l| l.glyphs.iter().find(|g| g.byte == b && g.len > 0).map(|g| (l, g))).unwrap();
            json!([g.x + g.adv * 0.75, l.baseline - 3.0])
        };
        let mut select = |unit: &str, b: usize| {
            let p = at(&s, b);
            let r: Value = s.execute("text.selectAt", &json!({"frame": fid, "point": p, "unit": unit})).unwrap();
            (r["anchor"].as_u64().unwrap() as usize, r["focus"].as_u64().unwrap() as usize)
        };
        assert_eq!(select("word", 1), (0, 5), "inside a word");
        assert_eq!(select("word", 4), (0, 5), "its last letter");
        assert_eq!(select("word", 5), (5, 7), "the run of spaces after it");
        assert_eq!(select("word", 18), (18, 19), "punctuation alone");
        assert_eq!(select("line", 9), (0, 19));
        assert_eq!(select("paragraph", 9), (0, 20), "with its return");
        assert_eq!(select("paragraph", 22), (20, text.len()));
        assert_eq!(select("story", 9), (0, text.len()));
        // A second click inside the selected word selects again rather than starting a text drag.
        s.execute("text.select", &json!({"story": sid, "anchor": 7, "focus": 12})).unwrap();
        let p = at(&s, 9);
        s.execute("text.selectAt", &json!({"frame": fid, "point": p, "unit": "line"})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 19));
        s.execute("text.release", &json!({"frame": fid, "point": p, "moved": false})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (0, 19), "releasing keeps it");
        // The older word command agrees.
        let p = at(&s, 8);
        s.execute("text.selectWord", &json!({"frame": fid, "point": p})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (7, 12));
        assert!(s.execute("text.selectAt", &json!({"frame": fid, "point": p, "unit": "sentence"})).is_err());
        assert!(s.execute("text.selectAt", &json!({"frame": fid, "unit": "word"})).is_err(), "no point");
    }
}

#[cfg(test)]
mod drag_text_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn drag_selected_text_to_move_or_copy_it() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "Hello brave world"})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        // Spread point of the glyph at byte `b`.
        let at = |s: &Session, b: usize| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let l = &cs.frames[0].lines[0];
            let g = l.glyphs.iter().find(|g| g.byte == b).unwrap();
            json!([g.x + g.adv * 0.25, l.baseline - 3.0])
        };
        let end = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let cs = designcraft_compose::compose_story(d, designcraft_doc::StoryId(sid), &Default::default());
            let l = &cs.frames[0].lines[0];
            json!([l.end_x + 2.0, l.baseline - 3.0])
        };
        s.execute("text.select", &json!({"story": sid, "anchor": 6, "focus": 12})).unwrap();
        let r = s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 8)})).unwrap();
        assert_eq!(r["dragging"], true);
        s.execute("text.extendTo", &json!({"frame": fid, "point": end(&s)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": end(&s), "moved": true})).unwrap();
        let text = |s: &Session| s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
        assert_eq!(text(&s), "Hello worldbrave ");
        // Alt-drag copies (the moved text is still selected).
        s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 12)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": at(&s, 0), "moved": true, "copy": true})).unwrap();
        assert_eq!(text(&s), "brave Hello worldbrave ");
        // A click inside the selection just places the caret.
        s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 2)})).unwrap();
        s.execute("text.release", &json!({"frame": fid, "point": at(&s, 2), "moved": false})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!(t.anchor, t.focus);
    }
}

#[cfg(test)]
mod bidi_caret_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn arrow_keys_in_a_right_to_left_paragraph() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "سلام عليكم"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("type.para", &json!({"attrs": {"direction": "rightToLeft"}})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        // Left moves forward through right-to-left text, Right back.
        let p = s.execute("text.move", &json!({"dir": "left"})).unwrap()["pos"].as_u64().unwrap();
        assert_eq!(p, 'س'.len_utf8() as u64);
        let p = s.execute("text.move", &json!({"dir": "right"})).unwrap()["pos"].as_u64().unwrap();
        assert_eq!(p, 0);
        // By words too.
        let p = s.execute("text.move", &json!({"dir": "left", "word": true})).unwrap()["pos"].as_u64().unwrap();
        assert!(p >= "سلام".len() as u64, "{p}");
    }
}

#[cfg(test)]
mod vertical_caret_tests {
    use designcraft_doc::StoryId;
    use serde_json::json;

    use crate::Session;

    /// A story set in three or more lines; returns its id.
    fn story(s: &mut Session, vertical: bool) -> StoryId {
        s.execute("file.new", &json!({})).unwrap();
        let text = "一二三四五六七八九十".repeat(8);
        let rect = if vertical { [72, 72, 200, 400] } else { [72, 72, 400, 200] };
        let r = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text, "vertical": vertical})).unwrap();
        StoryId(r["story"].as_u64().unwrap())
    }

    fn mv(s: &mut Session, dir: &str, extend: bool, word: bool) -> usize {
        s.execute("text.move", &json!({"dir": dir, "extend": extend, "word": word})).unwrap()["pos"].as_u64().unwrap() as usize
    }

    /// (line index, position along the line) of the caret at `pos`.
    fn place(s: &Session, sid: StoryId, pos: usize) -> (usize, f64) {
        let cs = s.cache.get(&s.doc().unwrap().doc, sid, None);
        let (_, x, baseline, _, _) = crate::compose::caret(&cs, pos).unwrap();
        let line = cs.frames[0].lines.iter().position(|l| (l.baseline - baseline).abs() < 0.01).unwrap();
        (line, x)
    }

    #[test]
    fn arrow_keys_follow_vertical_lines() {
        let mut s = Session::new();
        let sid = story(&mut s, true);
        assert!(s.cache.get(&s.doc().unwrap().doc, sid, None).frames[0].lines.len() >= 3);
        let c = '一'.len_utf8();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 3 * c, "focus": 3 * c})).unwrap();
        // Down and Up move along the line, a character at a time.
        assert_eq!(mv(&mut s, "down", false, false), 4 * c);
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
        // Left moves to the next line (lines follow each other right to left), keeping the
        // position along the line; Right comes back.
        let (line, x) = place(&s, sid, 3 * c);
        let next = mv(&mut s, "left", false, false);
        let (nline, nx) = place(&s, sid, next);
        assert_eq!(nline, line + 1);
        assert!((nx - x).abs() < 0.5, "{x} {nx}");
        assert_eq!(mv(&mut s, "right", false, false), 3 * c);
        // Shift extends.
        assert_eq!(mv(&mut s, "down", true, false), 4 * c);
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (3 * c, 4 * c));
        let p = mv(&mut s, "left", true, false);
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (3 * c, p));
        assert_eq!(place(&s, sid, p).0, line + 1);
        // With a selection, Up collapses it to its start.
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
        // By words: Alt+Down is the vertical Alt+Right.
        let text = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
        assert_eq!(mv(&mut s, "down", false, true), super::next_word(&text, 3 * c));
    }

    #[test]
    fn arrow_keys_in_horizontal_lines() {
        let mut s = Session::new();
        let sid = story(&mut s, false);
        let c = '一'.len_utf8();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 3 * c, "focus": 3 * c})).unwrap();
        assert_eq!(mv(&mut s, "right", false, false), 4 * c);
        assert_eq!(mv(&mut s, "left", false, false), 3 * c);
        let (line, x) = place(&s, sid, 3 * c);
        let next = mv(&mut s, "down", false, false);
        let (nline, nx) = place(&s, sid, next);
        assert_eq!(nline, line + 1);
        assert!((nx - x).abs() < 0.5, "{x} {nx}");
        assert_eq!(mv(&mut s, "up", false, false), 3 * c);
    }
}

#[cfg(test)]
mod sentence_selection_tests {
    use super::{para_bounds, sentence_bounds};

    fn sentence(s: &str, at: &str) -> String {
        let i = s.find(at).unwrap();
        let (a, b) = sentence_bounds(s, i);
        s[a..b].to_string()
    }

    #[test]
    fn sentences_end_at_a_stop_before_a_space_or_the_paragraph_end() {
        let s = "It costs 3.5 euros. \u{201C}Really?\u{201D} (Twice!) Yes\u{2026} fine.\nNext one. Last";
        assert_eq!(sentence(s, "costs"), "It costs 3.5 euros. ");
        assert_eq!(sentence(s, "Really"), "\u{201C}Really?\u{201D} ");
        assert_eq!(sentence(s, "Twice"), "(Twice!) ");
        assert_eq!(sentence(s, "Yes"), "Yes\u{2026} ");
        assert_eq!(sentence(s, "fine"), "fine.");
        assert_eq!(sentence(s, "one"), "Next one. ");
        assert_eq!(sentence(s, "Last"), "Last");
        // The caret at the very end of a paragraph is in its last sentence.
        let end = s.find('\n').unwrap();
        assert_eq!(sentence_bounds(s, end), (s.find("fine").unwrap(), end));
    }

    #[test]
    fn cjk_full_stops_end_sentences_without_a_space() {
        let s = "これは文です。次の文！最後";
        assert_eq!(sentence(s, "次"), "次の文！");
        assert_eq!(sentence(s, "最後"), "最後");
    }

    #[test]
    fn paragraphs_stop_at_returns() {
        let s = "One.\nTwo two.\n";
        assert_eq!(para_bounds(s, 6), (5, 13));
        assert_eq!(para_bounds(s, 0), (0, 4));
        assert_eq!(para_bounds(s, s.len()), (s.len(), s.len()));
    }
}
