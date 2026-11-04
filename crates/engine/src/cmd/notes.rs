//! Type › Insert Footnote, Document Footnote Options, footnote inspection and editing.

use designcraft_doc::{CellAddr, FootnoteOptions, ParaFormat, StoryId, TextSel};
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
            "{text?} — a footnote reference at the insertion point; the caret moves into the new footnote's text",
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
        cmd!(query "footnote.list", "Footnotes", [], None, "{story?} → [{story, id, index, anchor, text}] in story order", has_doc, list),
        cmd!("footnote.setText", "Set Footnote Text", [], None, "{story, id, text} — replaces the footnote's text", has_doc, set_text),
        cmd!("footnote.delete", "Delete Footnote", [], None, "{story, id} — deletes the reference (and so the footnote)", has_doc, delete),
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
    match s.active().and_then(|d| d.selection.text) {
        Some(t) if t.cell.is_some() => Err("footnotes can't be inserted in table cells or other footnotes".into()),
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
        let st = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let r = t.range();
        let r = r.start.min(st.len())..r.end.min(st.len());
        st.delete(r.clone());
        let id = st.insert_note(r.start, &text, para).ok_or_else(|| bad("footnote.insert", "the story's footnotes don't match their references"))?;
        sel.text = Some(TextSel { story: t.story, anchor: text.len(), focus: text.len(), frame: t.frame, cell: Some(CellAddr::footnote(id)) });
        Ok(json!({"story": t.story.0, "id": id}))
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
    if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell.is_none()) {
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
        for (k, n) in st.notes.iter().enumerate() {
            out.push(json!({"story": st.id.0, "id": n.id, "index": k, "anchor": st.note_anchor(n.id), "text": n.text.text}));
        }
    }
    Ok(Value::Array(out))
}

fn set_text(s: &mut Session, p: &Value) -> Result<Value> {
    let sid = story_id(p, "footnote.setText")?;
    let id = note_id(p, "footnote.setText")?;
    let text = super::text_param(p, "text");
    s.edit(|d, sel| {
        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let n = st.note_mut(id).ok_or_else(|| bad("footnote.setText", format!("no footnote {id}")))?;
        let len = n.text.len();
        n.text.replace(0..len, &text);
        // A caret in the replaced text goes to its end.
        if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell == Some(CellAddr::footnote(id))) {
            (t.anchor, t.focus) = (n.text.len(), n.text.len());
        }
        Ok(Value::Null)
    })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let sid = story_id(p, "footnote.delete")?;
    let id = note_id(p, "footnote.delete")?;
    let moved = s.edit(|d, sel| {
        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let at = st.note_anchor(id).ok_or_else(|| bad("footnote.delete", format!("no footnote {id}")))?;
        follow_delete(sel, sid, at..at + designcraft_doc::FOOTNOTE_REF.len_utf8());
        st.delete(at..at + designcraft_doc::FOOTNOTE_REF.len_utf8());
        let moved = sel.text.is_some_and(|t| t.story == sid && t.cell == Some(CellAddr::footnote(id)));
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
    fn not_in_cells() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("footnote.insert", &json!({"text": "x"})).unwrap();
        // Caret is now in footnote text: no nested footnotes.
        assert!(s.execute("footnote.insert", &json!({})).is_err());
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
}
