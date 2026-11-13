//! Type › Insert Footnote, Document Footnote Options, footnote inspection and editing.

use designcraft_doc::{CellAddr, FootnoteOptions, ParaFormat, Story, StoryId, TABLE_ANCHOR, Table, TextSel};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "footnote.insert",
            "Insert Footnote",
            ["Type"],
            None,
            "{text?} → {story, id, table?, row?, col?} (table, row and col are returned for a cell note) — a footnote reference at the insertion point; the caret moves into the new footnote's text (in a table cell it stays in the cell, after its reference)",
            in_story_text,
            insert
        ),
        cmd!(
            "footnote.options",
            "Document Footnote Options…",
            ["Type"],
            None,
            "{style?: arabic|upperRoman|lowerRoman|upperLetters|lowerLetters|symbols|kanji, startAt?, restart?: never|page|spread|section, prefix?, suffix?, affixIn?: none|reference|text|both, refPosition?: superscript|subscript|normal|otSuperscript, refCharStyle?, paraStyle?, separator?, spaceBefore?, spaceBetween?, firstBaseline?: ascent|capHeight|leading|xHeight|emboxHeight|fixed, firstBaselineMin?, spanColumns?, rule?: {on, weight, color, tint, width, offset, leftIndent}} → the options (no params: just read them)",
            has_doc,
            options
        ),
        cmd!(
            query "footnote.list",
            "Footnotes",
            [],
            None,
            "{story?} → [{story, id, index, anchor, text, table?, row?, col?}] in reading order within each story; a table cell's note also names its table and the cell's row and column, and its anchor is in the cell's text",
            has_doc,
            list
        ),
        cmd!(
            "footnote.setText",
            "Set Footnote Text",
            [],
            None,
            "{story, id, text, table?, row?, col?} — replaces the footnote's text (a cell's note names its table and cell)",
            has_doc,
            set_text
        ),
        cmd!(
            "footnote.delete",
            "Delete Footnote",
            [],
            None,
            "{story, id, table?, row?, col?} — deletes the reference (and so the footnote); a cell's note names its table and cell",
            has_doc,
            delete
        ),
        cmd!(
            noundo "footnote.goToReference",
            "Go to Footnote Reference",
            ["Type"],
            None,
            "{} — from footnote text, puts the caret after the footnote's reference",
            in_note,
            go_to_reference
        ),
    ]
}

fn in_story_text(s: &Session) -> std::result::Result<(), String> {
    super::has_text(s)?;
    // A table cell takes a footnote, as InDesign does; footnote text does not (no nested notes).
    match s.active().and_then(|d| d.selection.text) {
        Some(t) if t.cell.is_some_and(|c| c.footnote_id().is_some()) => Err("footnotes can't be inserted in other footnotes".into()),
        _ => Ok(()),
    }
}

fn in_note(s: &Session) -> std::result::Result<(), String> {
    super::has_text(s)?;
    match s.active().and_then(|d| d.selection.text) {
        Some(t) if t.cell.is_some_and(|c| c.footnote_id().is_some()) => Ok(()),
        _ => Err("the insertion point is not in footnote text".into()),
    }
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let text = super::text_param(p, "text");
    let r = s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("footnote.insert", "no insertion point"))?;
        let para = ParaFormat { style: d.footnote_options.para_style.clone(), ..Default::default() };
        let (id, r) = {
            let st = d.text_story_mut(t.story, t.cell).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
            let r = t.range();
            let r = r.start.min(st.len())..r.end.min(st.len());
            st.delete(r.clone());
            let id =
                st.insert_note(r.start, &text, para).ok_or_else(|| bad("footnote.insert", "the story's footnotes don't match their references"))?;
            (id, r)
        };
        let step = designcraft_doc::FOOTNOTE_REF.len_utf8();
        if t.cell.is_none() {
            d.adjust_text_ranges(t.story, r.clone(), step);
        }
        sel.text = Some(match t.cell {
            None => TextSel { story: t.story, anchor: text.len(), focus: text.len(), frame: t.frame, cell: Some(CellAddr::footnote(id)) },
            // A table cell's footnote text has no address of its own yet: the caret stays in the
            // cell, after the reference (the note's text comes from `text`).
            Some(c) => {
                sel.cells = None;
                let at = r.start + step;
                TextSel { story: t.story, anchor: at, focus: at, frame: t.frame, cell: Some(c) }
            }
        });
        Ok(match t.cell {
            Some(c) => json!({"story": t.story.0, "id": id, "table": c.table, "row": c.row, "col": c.col}),
            None => json!({"story": t.story.0, "id": id}),
        })
    })?;
    s.typing_format = None;
    Ok(r)
}

/// Overlay `p` onto `base` (objects merge key by key).
fn merge(base: &mut Value, p: &Value) {
    match (base, p) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge(slot, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, v) => *b = v.clone(),
    }
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = s.doc()?.doc.footnote_options.clone();
    if p.as_object().is_none_or(|o| o.is_empty()) {
        return Ok(serde_json::to_value(&cur).unwrap_or_default());
    }
    let mut v = serde_json::to_value(&cur).unwrap_or_default();
    merge(&mut v, p);
    let next: FootnoteOptions = serde_json::from_value(v).map_err(|e| bad("footnote.options", e.to_string()))?;
    let out = serde_json::to_value(&next).unwrap_or_default();
    if next != cur {
        s.edit(|d, _| {
            d.footnote_options = next;
            Ok(Value::Null)
        })?;
    }
    Ok(out)
}

fn story_id(p: &Value, cmd: &str) -> Result<StoryId> {
    p.get("story").and_then(Value::as_u64).map(StoryId).ok_or_else(|| bad(cmd, "missing story"))
}

fn note_id(p: &Value, cmd: &str) -> Result<u64> {
    p.get("id").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "missing id"))
}

/// Moves a caret or selection in story `sid`'s own text (not a cell or note) back by the length of `range` when it lies
/// after `range.start`, or to `range.start` when it lies inside `range`.
pub(super) fn follow_delete(sel: &mut designcraft_doc::Selection, sid: StoryId, range: std::ops::Range<usize>) {
    follow_delete_in_text(sel, sid, None, range);
}

fn follow_delete_in_text(sel: &mut designcraft_doc::Selection, sid: StoryId, cell: Option<CellAddr>, range: std::ops::Range<usize>) {
    if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell == cell) {
        let back = |i: usize| if i > range.start { i - (i - range.start).min(range.len()) } else { i };
        t.anchor = back(t.anchor);
        t.focus = back(t.focus);
    }
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let only = p.get("story").and_then(Value::as_u64).map(StoryId);
    let mut out = Vec::new();
    for st in d.stories.values().filter(|st| only.is_none_or(|o| o == st.id)) {
        collect_notes(st, st.id, None, &mut out);
    }
    Ok(Value::Array(out))
}

/// Append `st`'s notes, and those of the tables anchored in it, in reading order. `cell` is the
/// table cell `st` is (`None` for a story's own text), and `root` is the document story the notes
/// belong to, so a cell's note names the story a command must open.
fn collect_notes(st: &Story, root: StoryId, cell: Option<(u64, usize, usize)>, out: &mut Vec<Value>) {
    let mut k = 0;
    for (at, ch) in st.text.char_indices() {
        if ch == designcraft_doc::FOOTNOTE_REF {
            if let Some(n) = st.notes.get(k) {
                let mut o = serde_json::Map::new();
                o.insert("story".into(), json!(root.0));
                o.insert("id".into(), json!(n.id));
                o.insert("index".into(), json!(k));
                o.insert("anchor".into(), json!(at));
                o.insert("text".into(), json!(n.text.text));
                if let Some((table, row, col)) = cell {
                    o.insert("table".into(), json!(table));
                    o.insert("row".into(), json!(row));
                    o.insert("col".into(), json!(col));
                }
                out.push(Value::Object(o));
            }
            k += 1;
        } else if ch == TABLE_ANCHOR && cell.is_none() {
            // ponytail: a table nested in a cell (only imports make one) is skipped: the commands
            // resolve `table` in the story's own tables, so its notes need a full cell path first.
            let Some(t) = st.table_at(at).and_then(|id| st.tables.get(&id)) else { continue };
            for (row, col) in cell_reading_order(t) {
                if let Some(c) = t.cell(row, col) {
                    collect_notes(&c.text, root, Some((t.id, row, col)), out);
                }
            }
        }
    }
}

/// The cells a table is read in: row by row, each from its first column (on the right in a
/// right-to-left table, which lays its columns out mirrored).
fn cell_reading_order(t: &Table) -> Vec<(usize, usize)> {
    let (nr, nc) = (t.nrows(), t.ncols());
    let owners = t.owners();
    let mut cells = Vec::new();
    for r in 0..nr {
        for c in 0..nc {
            let Some(at) = r.checked_mul(nc).and_then(|x| x.checked_add(c)) else { continue };
            if owners.get(at) == Some(&(r, c)) {
                cells.push((r, c));
            }
        }
    }
    cells
}

/// The cell a footnote command names, when `table` is given (`row` and `col` then follow it).
fn note_cell(p: &Value, cmd: &str) -> Result<Option<CellAddr>> {
    let Some(table) = p.get("table").filter(|v| !v.is_null()) else {
        if ["row", "col"].iter().any(|key| p.get(key).is_some_and(|v| !v.is_null())) {
            return Err(bad(cmd, "a cell needs `table`"));
        }
        return Ok(None);
    };
    let table = table.as_u64().ok_or_else(|| bad(cmd, "`table` must be a table id"))?;
    let index = |key: &str| -> Result<usize> {
        let v = p.get(key).and_then(Value::as_u64).ok_or_else(|| bad(cmd, format!("a cell needs `{key}`")))?;
        usize::try_from(v).map_err(|_| bad(cmd, format!("`{key}` is too large")))
    };
    Ok(Some(CellAddr { table, row: index("row")?, col: index("col")? }))
}

fn note_story<'a>(d: &'a mut designcraft_doc::Document, sid: StoryId, cell: Option<CellAddr>, cmd: &str) -> Result<&'a mut Story> {
    d.text_story_mut(sid, cell).ok_or_else(|| match cell {
        Some(c) => bad(cmd, format!("no table {} cell {}:{} in story {}", c.table, c.row, c.col, sid.0)),
        None => bad(cmd, format!("no story {}", sid.0)),
    })
}

fn set_text(s: &mut Session, p: &Value) -> Result<Value> {
    let sid = story_id(p, "footnote.setText")?;
    let id = note_id(p, "footnote.setText")?;
    let cell = note_cell(p, "footnote.setText")?;
    let text = super::text_param(p, "text");
    s.edit(|d, sel| {
        let st = note_story(d, sid, cell, "footnote.setText")?;
        let n = st.note_mut(id).ok_or_else(|| bad("footnote.setText", format!("no footnote {id}")))?;
        let len = n.text.len();
        n.text.replace(0..len, &text);
        // A caret in the replaced text goes to its end.
        if let Some(t) = sel.text.as_mut().filter(|t| cell.is_none() && t.story == sid && t.cell == Some(CellAddr::footnote(id))) {
            (t.anchor, t.focus) = (n.text.len(), n.text.len());
        }
        Ok(Value::Null)
    })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let sid = story_id(p, "footnote.delete")?;
    let id = note_id(p, "footnote.delete")?;
    let cell = note_cell(p, "footnote.delete")?;
    let moved = s.edit(|d, sel| {
        let st = note_story(d, sid, cell, "footnote.delete")?;
        let at = st.note_anchor(id).ok_or_else(|| bad("footnote.delete", format!("no footnote {id}")))?;
        let len = designcraft_doc::FOOTNOTE_REF.len_utf8();
        follow_delete_in_text(sel, sid, cell, at..at + len);
        st.delete(at..at + len);
        if cell.is_none() {
            d.adjust_text_ranges(sid, at..at + len, 0);
        }
        let moved = cell.is_none() && sel.text.is_some_and(|t| t.story == sid && t.cell == Some(CellAddr::footnote(id)));
        if moved {
            sel.text = Some(TextSel::caret(sid, at));
        }
        Ok(moved)
    })?;
    if moved {
        s.typing_format = None;
    }
    Ok(Value::Null)
}

fn go_to_reference(s: &mut Session, _: &Value) -> Result<Value> {
    let st = s.doc_mut()?;
    let t = st.selection.text.ok_or_else(|| bad("footnote.goToReference", "no caret"))?;
    let id = t.cell.and_then(|c| c.footnote_id()).ok_or_else(|| bad("footnote.goToReference", "not in footnote text"))?;
    let at = st.doc.story(t.story).and_then(|x| x.note_anchor(id)).ok_or_else(|| bad("footnote.goToReference", "footnote not found"))?;
    let pos = at + designcraft_doc::FOOTNOTE_REF.len_utf8();
    st.selection.text = Some(TextSel { anchor: pos, focus: pos, cell: None, ..t });
    st.bump_revision();
    s.typing_format = None;
    Ok(json!({"story": t.story.0, "pos": pos}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_text() -> (Session, StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 400], "content": "text", "text": "Body text with a note here."})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        (s, sid)
    }

    #[test]
    fn insert_type_and_undo_footnotes() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 16, "focus": 16})).unwrap();
        let r = s.execute("footnote.insert", &json!({})).unwrap();
        let id = r["id"].as_u64().unwrap();
        assert_eq!(r, json!({"story": sid.0, "id": id}));
        // Typing goes into the footnote.
        s.execute("text.insert", &json!({"text": "A source."})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(st.notes.len(), 1);
        assert_eq!(st.notes[0].text.text, "A source.");
        assert_eq!(st.text.matches(designcraft_doc::FOOTNOTE_REF).count(), 1);
        // The footnote is laid out in the frame.
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, sid, None);
        assert_eq!(cs.frames[0].notes.len(), 1);
        // Back to the reference.
        let r = s.execute("footnote.goToReference", &json!({})).unwrap();
        assert_eq!(r["pos"], json!(16 + designcraft_doc::FOOTNOTE_REF.len_utf8()));
        // Listing, editing, options.
        let l = s.execute("footnote.list", &json!({})).unwrap();
        assert_eq!(l[0]["text"], "A source.");
        s.execute("footnote.setText", &json!({"story": sid.0, "id": id, "text": "Changed."})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes[0].text.text, "Changed.");
        let o = s.execute("footnote.options", &json!({"style": "lowerRoman", "rule": {"width": 100.0}})).unwrap();
        assert_eq!(o["style"], "lowerRoman");
        assert_eq!(o["rule"]["width"], 100.0);
        assert_eq!(o["rule"]["on"], true);
        assert!(s.execute("footnote.options", &json!({"restart": "bogus"})).is_err());
        // Deleting removes the reference; undo brings everything back.
        s.execute("footnote.delete", &json!({"story": sid.0, "id": id})).unwrap();
        assert!(s.doc().unwrap().doc.story(sid).unwrap().notes.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes.len(), 1);
    }

    #[test]
    fn footnote_caret_moves_drop_the_pending_typing_format() {
        let (mut s, sid) = session_with_text();
        let pending = |s: &mut Session| {
            s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
            assert!(s.typing_format.is_some());
        };
        // Formatting chosen in footnote text, then away to the reference and back to the same caret (a new
        // footnote takes the deleted one's id): typing there is in the footnote text's own format.
        s.execute("text.select", &json!({"story": sid.0, "anchor": 16, "focus": 16})).unwrap();
        let id = s.execute("footnote.insert", &json!({"text": "A source."})).unwrap()["id"].as_u64().unwrap();
        pending(&mut s);
        s.execute("footnote.goToReference", &json!({})).unwrap();
        s.execute("footnote.delete", &json!({"story": sid.0, "id": id})).unwrap();
        let again = s.execute("footnote.insert", &json!({"text": "A source."})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(again, id);
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes[0].text.char_format_at(10).over.size, None);
        // Each command that moves the caret drops it.
        pending(&mut s);
        s.execute("footnote.goToReference", &json!({})).unwrap();
        assert!(s.typing_format.is_none(), "footnote.goToReference");
        pending(&mut s);
        let id = s.execute("footnote.insert", &json!({"text": "y"})).unwrap()["id"].as_u64().unwrap();
        assert!(s.typing_format.is_none(), "footnote.insert");
        pending(&mut s);
        s.execute("footnote.delete", &json!({"story": sid.0, "id": id})).unwrap();
        assert!(s.typing_format.is_none(), "footnote.delete");
    }

    #[test]
    fn not_in_footnote_text() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("footnote.insert", &json!({"text": "x"})).unwrap();
        // Caret is now in footnote text: no nested footnotes.
        let e = s.execute("footnote.insert", &json!({})).unwrap_err().to_string();
        assert!(e.contains("other footnotes"), "{e}");
    }

    /// A table cell takes a footnote, as InDesign does: the reference and its note live in the
    /// cell's own story, the way an imported cell footnote is stored.
    #[test]
    fn insert_footnote_in_a_table_cell() {
        let (mut s, sid, tid) = session_with_table();
        let r = s.execute("footnote.insert", &json!({"text": "A cell note."})).unwrap();
        let id = r["id"].as_u64().unwrap();
        assert_eq!(r["story"], sid.0);
        assert_eq!(r["table"], tid);
        assert_eq!(r["row"], 0);
        assert_eq!(r["col"], 0);
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let cell = st.tables.get(&tid).unwrap().cell(0, 0).unwrap().clone();
        assert_eq!(cell.text.text, format!("Left{}", designcraft_doc::FOOTNOTE_REF));
        assert_eq!(cell.text.notes.len(), 1);
        assert_eq!(cell.text.notes[0].id, id);
        assert_eq!(cell.text.notes[0].text.text, "A cell note.");
        // The body story takes no part.
        assert!(st.notes.is_empty());
        assert_eq!(st.text.matches(designcraft_doc::FOOTNOTE_REF).count(), 0);
        // The caret stays in the cell, after the reference; typing carries on in the cell and the
        // note keeps its text.
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!(t.cell, Some(CellAddr { table: tid, row: 0, col: 0 }));
        let after_ref = "Left".len() + designcraft_doc::FOOTNOTE_REF.len_utf8();
        assert_eq!((t.anchor, t.focus), (after_ref, after_ref));
        assert!(s.doc().unwrap().selection.cells.is_none(), "the cell is no longer selected as a whole");
        s.execute("text.insert", &json!({"text": "!"})).unwrap();
        let cell = s.doc().unwrap().doc.story(sid).unwrap().tables.get(&tid).unwrap().cell(0, 0).unwrap().clone();
        assert_eq!(cell.text.text, format!("Left{}!", designcraft_doc::FOOTNOTE_REF));
        assert_eq!(cell.text.notes[0].text.text, "A cell note.");
        s.doc().unwrap().doc.check().unwrap();
        // Undo takes the reference and its note away; redo brings them back.
        s.execute("edit.undo", &json!({})).unwrap(); // the typed "!"
        s.execute("edit.undo", &json!({})).unwrap(); // the footnote
        let cell = s.doc().unwrap().doc.story(sid).unwrap().tables.get(&tid).unwrap().cell(0, 0).unwrap().clone();
        assert_eq!(cell.text.text, "Left");
        assert!(cell.text.notes.is_empty());
        s.execute("edit.redo", &json!({})).unwrap();
        let cell = s.doc().unwrap().doc.story(sid).unwrap().tables.get(&tid).unwrap().cell(0, 0).unwrap().clone();
        assert_eq!(cell.text.text, format!("Left{}", designcraft_doc::FOOTNOTE_REF));
        assert_eq!(cell.text.notes.len(), 1);
        assert_eq!(cell.text.notes[0].text.text, "A cell note.");
    }

    /// A cell footnote survives Save and Open.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_cell_footnote_survives_save_and_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell-footnote.designcraft");
        let (mut s, sid, tid) = session_with_table();
        s.execute("footnote.insert", &json!({"text": "A cell note."})).unwrap();
        s.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut back = Session::new();
        back.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        let st = back.doc().unwrap().doc.story(sid).unwrap().clone();
        let cell = st.tables.get(&tid).unwrap().cell(0, 0).unwrap().clone();
        assert_eq!(cell.text.text, format!("Left{}", designcraft_doc::FOOTNOTE_REF));
        assert_eq!(cell.text.notes[0].text.text, "A cell note.");
    }

    /// A text frame whose 2×2 table has "Left" in its first cell and a caret in it.
    fn session_with_table() -> (Session, StoryId, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect": [36, 36, 300, 400], "content": "text"})).unwrap();
        let sid = StoryId(frame["story"].as_u64().unwrap());
        let tid = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        s.execute("table.setCell", &json!({"story": sid.0, "table": tid, "row": 0, "col": 0, "text": "Left"})).unwrap();
        s.execute("table.select", &json!({"story": sid.0, "table": tid, "rows": [0, 0], "cols": [0, 0]})).unwrap();
        s.execute("text.move", &json!({"dir": "storyEnd"})).unwrap();
        (s, sid, tid)
    }

    #[test]
    fn huge_start_at_does_not_overflow() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 16, "focus": 16})).unwrap();
        s.execute("footnote.insert", &json!({"text": "x"})).unwrap();
        s.execute("footnote.options", &json!({"startAt": 4294967295u32})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, sid, None);
        assert_eq!(cs.frames[0].notes.len(), 1);
    }

    #[test]
    fn deleting_a_footnote_keeps_the_caret_on_its_character() {
        let n = designcraft_doc::FOOTNOTE_REF.len_utf8();
        // (anchor, focus) in the story text before the delete, then after it.
        for (before, after) in [((17 + n, 21 + n), (17, 21)), ((5, 21 + n), (5, 21)), ((5, 5), (5, 5)), ((16 + n, 16 + n), (16, 16))] {
            let (mut s, sid) = session_with_text();
            s.execute("text.select", &json!({"story": sid.0, "anchor": 16, "focus": 16})).unwrap();
            let id = s.execute("footnote.insert", &json!({"text": "x"})).unwrap()["id"].as_u64().unwrap();
            s.execute("text.select", &json!({"story": sid.0, "anchor": before.0, "focus": before.1})).unwrap();
            s.execute("footnote.delete", &json!({"story": sid.0, "id": id})).unwrap();
            let t = s.doc().unwrap().selection.text.unwrap();
            assert_eq!((t.anchor, t.focus), after, "selected {before:?}");
        }
    }

    #[test]
    fn replacing_footnote_text_puts_its_caret_at_the_end() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 16, "focus": 16})).unwrap();
        let id = s.execute("footnote.insert", &json!({"text": "abcd"})).unwrap()["id"].as_u64().unwrap();
        // The caret between "c" and "d" is inside the second "é" of the new text.
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!(t.cell, Some(CellAddr::footnote(id)));
        s.doc_mut().unwrap().selection.text = Some(TextSel { anchor: 3, focus: 3, ..t });
        s.execute("footnote.setText", &json!({"story": sid.0, "id": id, "text": "éé"})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (4, 4));
        s.execute("text.delete", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes[0].text.text, "é");
    }

    /// A story with a note before a 2 × 2 table, notes in three cells (row 1 left, row 1 right,
    /// row 2 right) and a note after it — the shape of the shared `cell-footnotes.idml` repro.
    fn session_with_cell_notes() -> (Session, StoryId, u64) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 400], "content": "text", "text": "Before table. After table."})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let at = "Before table.".len();
        s.execute("text.select", &json!({"story": sid.0, "anchor": at, "focus": at})).unwrap();
        s.execute("footnote.insert", &json!({"text": "Note 1 (body)."})).unwrap();
        let after_ref = at + designcraft_doc::FOOTNOTE_REF.len_utf8();
        s.execute("text.select", &json!({"story": sid.0, "anchor": after_ref, "focus": after_ref})).unwrap();
        let tid = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        {
            let doc = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            for (row, col, text, note) in [
                (0, 0, "Left 1", "Note A (left cell, row 1)."),
                (0, 1, "Right 1", "Note B (right cell, row 1)."),
                (1, 1, "Right 2", "Note C (right cell, row 2)."),
            ] {
                let st = doc.text_story_mut(sid, Some(CellAddr { table: tid, row, col })).unwrap();
                st.insert(st.len(), text);
                st.insert_note(st.len(), note, ParaFormat::default());
            }
        }
        let after = s.doc().unwrap().doc.story(sid).unwrap().text.find("After table").unwrap() + "After table".len();
        s.execute("text.select", &json!({"story": sid.0, "anchor": after, "focus": after})).unwrap();
        s.execute("footnote.insert", &json!({"text": "Note 2 (body)."})).unwrap();
        (s, sid, tid)
    }

    #[test]
    fn list_includes_table_cell_notes_in_reading_order() {
        let (mut s, sid, tid) = session_with_cell_notes();
        let l = s.execute("footnote.list", &json!({})).unwrap();
        let notes = l.as_array().unwrap();
        let texts: Vec<&str> = notes.iter().filter_map(|n| n["text"].as_str()).collect();
        assert_eq!(
            texts,
            ["Note 1 (body).", "Note A (left cell, row 1).", "Note B (right cell, row 1).", "Note C (right cell, row 2).", "Note 2 (body)."]
        );
        // Body notes keep the shape they had: no cell fields.
        assert!(notes[0].get("table").is_none());
        assert!(notes[0].get("row").is_none());
        assert_eq!(notes[0]["index"], 0);
        assert_eq!(notes[4]["index"], 1);
        // A cell's note names the document story, its table and the cell to open and edit it.
        let at = |n: &Value| (n["story"].as_u64(), n["table"].as_u64(), n["row"].as_u64(), n["col"].as_u64());
        assert_eq!(at(&notes[1]), (Some(sid.0), Some(tid), Some(0), Some(0)));
        assert_eq!(at(&notes[2]), (Some(sid.0), Some(tid), Some(0), Some(1)));
        assert_eq!(at(&notes[3]), (Some(sid.0), Some(tid), Some(1), Some(1)));
        assert_eq!(notes[1]["anchor"], 6, "the reference offset within the cell's text");
    }

    #[test]
    fn list_leaves_out_notes_in_nested_tables() {
        // An imported cell can hold a nested table whose id matches the outer one; the footnote
        // commands resolve a listed cell in the story's own tables, so listing it would point there.
        let (mut s, sid, tid) = session_with_cell_notes();
        {
            let doc = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let mut nested = Table::new(tid, 1, 1, 0, 0, 100.0);
            nested.cell_mut(0, 0).unwrap().text.insert_note(0, "Nested note.", ParaFormat::default());
            let outer = doc.text_story_mut(sid, Some(CellAddr { table: tid, row: 0, col: 0 })).unwrap();
            let end = outer.len();
            outer.insert_table(end, nested);
        }
        let l = s.execute("footnote.list", &json!({})).unwrap();
        let texts: Vec<&str> = l.as_array().unwrap().iter().filter_map(|n| n["text"].as_str()).collect();
        assert!(!texts.contains(&"Nested note."), "{texts:?}");
        assert!(texts.contains(&"Note A (left cell, row 1)."), "{texts:?}");
    }

    #[test]
    fn table_cells_are_read_from_each_rows_first_column() {
        let mut t = Table::new(1, 2, 2, 0, 0, 100.0);
        assert_eq!(cell_reading_order(&t), [(0, 0), (0, 1), (1, 0), (1, 1)]);
        // A right-to-left table lays column 0 out on the right, so it is still read first.
        t.options.direction = designcraft_doc::TextDirection::RightToLeft;
        assert_eq!(cell_reading_order(&t), [(0, 0), (0, 1), (1, 0), (1, 1)]);
    }

    #[test]
    fn list_finds_cell_notes_when_the_story_has_no_body_notes() {
        let (mut s, sid, tid) = session_with_cell_notes();
        let ids: Vec<u64> = s.doc().unwrap().doc.story(sid).unwrap().notes.iter().map(|n| n.id).collect();
        for id in ids {
            s.execute("footnote.delete", &json!({"story": sid.0, "id": id})).unwrap();
        }
        let l = s.execute("footnote.list", &json!({})).unwrap();
        let notes = l.as_array().unwrap();
        assert_eq!(notes.len(), 3, "the three cell notes remain, with the empty cell contributing none");
        assert!(notes.iter().all(|n| n["table"] == tid));
    }

    #[test]
    fn a_table_cell_footnote_can_be_edited_and_deleted_by_its_listed_cell() {
        let (mut s, sid, tid) = session_with_cell_notes();
        let notes = s.execute("footnote.list", &json!({})).unwrap();
        let id = notes[3]["id"].as_u64().unwrap();
        let cell = CellAddr { table: tid, row: 1, col: 1 };
        // A cell note's id is only unique within its story: the cell fields pick it out.
        assert!(s.execute("footnote.setText", &json!({"story": sid.0, "id": id, "table": tid, "text": "x"})).is_err());
        s.execute("footnote.setText", &json!({"story": sid.0, "id": id, "table": tid, "row": 1, "col": 1, "text": "Changed."})).unwrap();
        assert_eq!(s.doc().unwrap().doc.text_story(sid, Some(cell)).unwrap().notes[0].text.text, "Changed.");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes[0].text.text, "Note 1 (body).");
        s.execute("footnote.delete", &json!({"story": sid.0, "id": id, "table": tid, "row": 1, "col": 1})).unwrap();
        let d = s.doc().unwrap();
        let cell_story = d.doc.text_story(sid, Some(cell)).unwrap();
        assert!(cell_story.notes.is_empty());
        assert!(!cell_story.text.contains(designcraft_doc::FOOTNOTE_REF));
        assert_eq!(d.doc.story(sid).unwrap().notes.len(), 2, "the body notes stay");
        assert_eq!(d.doc.story(sid).unwrap().text.matches(designcraft_doc::FOOTNOTE_REF).count(), 2);
    }

    #[test]
    fn cell_note_edits_leave_a_body_note_caret_unchanged() {
        let (mut s, sid, tid) = session_with_cell_notes();
        let id = s.doc().unwrap().doc.story(sid).unwrap().notes[0].id;
        let caret = TextSel { cell: Some(CellAddr::footnote(id)), ..TextSel::caret(sid, 4) };
        s.doc_mut().unwrap().selection.text = Some(caret);
        s.execute("footnote.setText", &json!({"story": sid.0, "id": id, "table": tid, "row": 0, "col": 0, "text": "x"})).unwrap();
        assert_eq!(s.doc().unwrap().selection.text, Some(caret));
        s.execute("footnote.delete", &json!({"story": sid.0, "id": id, "table": tid, "row": 0, "col": 0})).unwrap();
        assert_eq!(s.doc().unwrap().selection.text, Some(caret));
    }

    #[test]
    fn deleting_a_cell_note_adjusts_only_a_selection_in_that_cell() {
        for same_cell in [true, false] {
            let (mut s, sid, tid) = session_with_cell_notes();
            let cell = CellAddr { table: tid, row: 0, col: 0 };
            let st = s.doc().unwrap().doc.text_story(sid, Some(cell)).unwrap();
            let id = st.notes[0].id;
            let end = st.len();
            let selected = CellAddr { col: usize::from(!same_cell), ..cell };
            s.doc_mut().unwrap().selection.text = Some(TextSel { cell: Some(selected), ..TextSel::caret(sid, end) });
            s.execute("footnote.delete", &json!({"story": sid.0, "id": id, "table": tid, "row": 0, "col": 0})).unwrap();
            let t = s.doc().unwrap().selection.text.unwrap();
            let expected = if same_cell { end - designcraft_doc::FOOTNOTE_REF.len_utf8() } else { end };
            assert_eq!((t.anchor, t.focus, t.cell), (expected, expected, Some(selected)));
        }
    }

    #[test]
    fn partial_cell_addresses_cannot_change_a_body_note() {
        let (mut s, sid, _) = session_with_cell_notes();
        let id = s.doc().unwrap().doc.story(sid).unwrap().notes[0].id;
        for fields in [json!({"row": 0}), json!({"col": 0}), json!({"table": null, "row": 0, "col": 0})] {
            let mut p = json!({"story": sid.0, "id": id, "text": "changed"});
            merge(&mut p, &fields);
            assert!(s.execute("footnote.setText", &p).is_err());
            assert!(s.execute("footnote.delete", &p).is_err());
        }
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes[0].text.text, "Note 1 (body).");
    }
}
