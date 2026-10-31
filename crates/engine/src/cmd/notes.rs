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
            "{text?} — a footnote reference at the insertion point; the caret moves into the new footnote's text. In footnote text, a footnote nested in that note (printed under it, numbered in reading order; at most 5 deep)",
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
        cmd!(query "footnote.list", "Footnotes", [], None, "{story?} → [{story, id, index, anchor, text, parent?, depth?}] in story order, each footnote followed by the notes nested in it (reading order; `parent`: the note whose text holds the reference, `anchor` in that text; `index`: the top-level footnote's)", has_doc, list),
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
    let Some(d) = s.active() else { return Ok(()) };
    let Some((sid, c)) = d.selection.text.and_then(|t| t.cell.map(|c| (t.story, c))) else { return Ok(()) };
    let Some(id) = c.footnote_id() else { return Err("footnotes can't be inserted in table cells".into()) };
    // In footnote text: a nested footnote, down to the depth limit.
    let depth = d.doc.story(sid).and_then(|st| st.note_depth(id)).unwrap_or(usize::MAX);
    if depth >= designcraft_doc::MAX_NOTE_DEPTH {
        return Err(format!("footnotes nest at most {} deep", designcraft_doc::MAX_NOTE_DEPTH));
    }
    Ok(())
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
    in_story_text(s).map_err(|e| bad("footnote.insert", e))?;
    let r = s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("footnote.insert", "no insertion point"))?;
        let para = ParaFormat { style: d.footnote_options.para_style.clone(), ..Default::default() };
        let top = d.story(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        // In footnote text a nested footnote, with an id no note of the story has.
        let fresh = t.cell.and_then(|c| c.footnote_id()).map(|_| top.next_note_id_deep());
        let st = d.text_story_mut(t.story, t.cell).ok_or_else(|| bad("footnote.insert", "no such footnote"))?;
        let r = t.range();
        let r = r.start.min(st.len())..r.end.min(st.len());
        st.delete(r.clone());
        let mut id =
            st.insert_note(r.start, &text, para).ok_or_else(|| bad("footnote.insert", "the story's footnotes don't match their references"))?;
        if let Some(fresh) = fresh
            && let Some(n) = st.notes.iter_mut().find(|n| n.id == id)
        {
            std::sync::Arc::make_mut(n).id = fresh;
            id = fresh;
        }
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
            // Then the notes nested in it, in reading order, with the note each is in.
            for c in n.nested() {
                let parent = st.note_parent(c.id);
                let anchor = st.note_holder(c.id).and_then(|h| h.note_anchor(c.id));
                out.push(json!({"story": st.id.0, "id": c.id, "index": k, "parent": parent, "depth": st.note_depth(c.id), "anchor": anchor, "text": c.text.text}));
            }
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
        let top = d.story(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        // The note itself and the notes nested in it go; a nested note's reference is in the
        // text of the note it's in.
        let note = top.note(id).ok_or_else(|| bad("footnote.delete", format!("no footnote {id}")))?;
        let mut gone: Vec<u64> = vec![id];
        gone.extend(note.nested().iter().map(|n| n.id));
        let holder = top.note_parent(id).map(CellAddr::footnote);
        let st = d.text_story_mut(sid, holder).ok_or_else(|| bad("footnote.delete", "no such footnote"))?;
        let at = st.note_anchor(id).ok_or_else(|| bad("footnote.delete", format!("no footnote {id}")))?;
        let len = designcraft_doc::FOOTNOTE_REF.len_utf8();
        if holder.is_none() {
            follow_delete(sel, sid, at..at + len);
        } else if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell == holder) {
            let back = |i: usize| if i > at { i - (i - at).min(len) } else { i };
            (t.anchor, t.focus) = (back(t.anchor), back(t.focus));
        }
        st.delete(at..at + len);
        // A caret in the deleted note (or one nested in it) goes to where its reference was.
        let moved = sel.text.is_some_and(|t| t.story == sid && t.cell.and_then(|c| c.footnote_id()).is_some_and(|c| gone.contains(&c)));
        if moved {
            sel.text = Some(TextSel { cell: holder, ..TextSel::caret(sid, at) });
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
    // A nested note's reference is in the text of the note it's in.
    let top = st.doc.story(t.story).ok_or_else(|| bad("footnote.goToReference", "footnote not found"))?;
    let holder = top.note_parent(id).map(CellAddr::footnote);
    let at = top.note_holder(id).and_then(|x| x.note_anchor(id)).ok_or_else(|| bad("footnote.goToReference", "footnote not found"))?;
    let pos = at + designcraft_doc::FOOTNOTE_REF.len_utf8();
    st.selection.text = Some(TextSel { anchor: pos, focus: pos, cell: holder, ..t });
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

    /// Labels of the placed footnotes, in order, with their ids.
    fn placed(s: &Session, sid: StoryId) -> Vec<(u64, String)> {
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, sid, None);
        cs.frames.iter().flat_map(|f| f.notes.iter().map(|n| (n.id, n.label.clone()))).collect()
    }

    /// With the caret in footnote text, Insert Footnote nests a footnote in that note: the caret
    /// moves into it, typing reaches it, it's numbered in reading order (your * → † → ‡ chain,
    /// then the next reference), listed under its note, and one undo step takes it away.
    #[test]
    fn footnotes_nest_in_footnote_text() {
        let (mut s, sid) = session_with_text();
        // Footnotes after "Body" and after "note".
        s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 4})).unwrap();
        let a = s.execute("footnote.insert", &json!({"text": "Outer"})).unwrap()["id"].as_u64().unwrap();
        let end = s.doc().unwrap().doc.story(sid).unwrap().text.find("note").unwrap() + 4;
        s.execute("text.select", &json!({"story": sid.0, "anchor": end, "focus": end})).unwrap();
        let b = s.execute("footnote.insert", &json!({"text": "Later"})).unwrap()["id"].as_u64().unwrap();
        // Back into the first note, at its end: a nested note, then one nested in that.
        s.doc_mut().unwrap().selection.text = Some(TextSel { cell: Some(CellAddr::footnote(a)), ..TextSel::caret(sid, 5) });
        let c = s.execute("footnote.insert", &json!({"text": "Inner"})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(s.doc().unwrap().selection.text.unwrap().cell, Some(CellAddr::footnote(c)), "the caret moves into it");
        s.execute("text.insert", &json!({"text": " typed"})).unwrap();
        let e = s.execute("footnote.insert", &json!({"text": "Deepest"})).unwrap()["id"].as_u64().unwrap();
        let ids: std::collections::HashSet<u64> = [a, b, c, e].into_iter().collect();
        assert_eq!(ids.len(), 4, "ids unique across the story's notes");
        let d = s.doc().unwrap().doc.clone();
        let st = d.story(sid).unwrap();
        assert_eq!(st.note(c).unwrap().text.text, format!("Inner typed{}", designcraft_doc::FOOTNOTE_REF));
        assert_eq!((st.note_parent(c), st.note_parent(e)), (Some(a), Some(c)));
        let labels: Vec<(u64, String)> = placed(&s, sid);
        assert_eq!(labels, [(a, "1".into()), (c, "2".into()), (e, "3".into()), (b, "4".into())]);
        // Listed under their note.
        let list = s.execute("footnote.list", &json!({"story": sid.0})).unwrap();
        let rows: Vec<(u64, Option<u64>)> = list.as_array().unwrap().iter().map(|r| (r["id"].as_u64().unwrap(), r["parent"].as_u64())).collect();
        assert_eq!(rows, [(a, None), (c, Some(a)), (e, Some(c)), (b, None)]);
        // Go to Reference from a nested note: the caret goes into the note it's in.
        s.execute("footnote.goToReference", &json!({})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.cell, t.anchor), (Some(CellAddr::footnote(c)), "Inner typed".len() + designcraft_doc::FOOTNOTE_REF.len_utf8()));
        // One undo step per insert.
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.story(sid).unwrap().note(e).is_none());
        assert_eq!(placed(&s, sid).len(), 3);
    }

    /// On the canvas: a click on a nested note's text puts the caret in that note, and typing
    /// goes there.
    #[test]
    fn clicking_a_nested_note_edits_it() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 4})).unwrap();
        s.execute("footnote.insert", &json!({"text": "Outer"})).unwrap();
        let c = s.execute("footnote.insert", &json!({"text": "Inner"})).unwrap()["id"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let fid = d.story(sid).unwrap().frames[0];
        let cs = s.cache.get(&d, sid, None);
        let n = cs.frames[0].notes.iter().find(|n| n.id == c).unwrap();
        let l = &n.text.frames[0].lines[0];
        // Inside the nested note's text, past its mark, in spread coordinates.
        let inner = designcraft_geom::Point::new(n.origin.x + l.end_x - 1.0, n.origin.y + l.baseline - 2.0);
        let pt = d.text_xf(d.item(fid).unwrap()) * inner;
        s.execute("text.placeCaret", &json!({"frame": fid.0, "point": [pt.x, pt.y]})).unwrap();
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!(t.cell, Some(CellAddr::footnote(c)), "{t:?}");
        s.execute("text.insert", &json!({"text": "!"})).unwrap();
        assert!(s.doc().unwrap().doc.story(sid).unwrap().note(c).unwrap().text.text.contains('!'));
    }

    /// Deleting a footnote deletes the notes nested in it; a caret in any of them goes to where
    /// the reference was.
    #[test]
    fn deleting_a_footnote_deletes_its_nested_notes() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 4})).unwrap();
        let a = s.execute("footnote.insert", &json!({"text": "Outer"})).unwrap()["id"].as_u64().unwrap();
        let c = s.execute("footnote.insert", &json!({"text": "Inner"})).unwrap()["id"].as_u64().unwrap();
        let e = s.execute("footnote.insert", &json!({"text": "Deepest"})).unwrap()["id"].as_u64().unwrap();
        // Delete the middle one, with the caret in the deepest: the caret goes to c's reference.
        s.execute("footnote.delete", &json!({"story": sid.0, "id": c})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert!(st.note(c).is_none() && st.note(e).is_none());
        assert_eq!(st.note(a).unwrap().text.text, "Outer");
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.cell, t.anchor), (Some(CellAddr::footnote(a)), 5));
        // Deleting the top-level note takes the rest; the caret goes to the body text.
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("footnote.delete", &json!({"story": sid.0, "id": a})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert!(st.notes.is_empty() && !st.text.contains(designcraft_doc::FOOTNOTE_REF));
        assert_eq!(s.doc().unwrap().selection.text.unwrap().cell, None);
        assert!(placed(&s, sid).is_empty());
    }

    /// Footnotes nest at most MAX_NOTE_DEPTH deep, and not in table cells.
    #[test]
    fn nesting_has_a_limit_and_not_in_cells() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        for _ in 0..designcraft_doc::MAX_NOTE_DEPTH {
            s.execute("footnote.insert", &json!({"text": "x"})).unwrap();
        }
        let e = s.execute("footnote.insert", &json!({})).unwrap_err().to_string();
        assert!(e.contains("at most"), "{e}");
        assert_eq!(placed(&s, sid).len(), designcraft_doc::MAX_NOTE_DEPTH);
    }

    /// A footnote reference pasted into a nested note's text gets a story-wide unique id.
    #[test]
    fn pasted_references_in_nested_notes_get_unique_ids() {
        let (mut s, sid) = session_with_text();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 4, "focus": 4})).unwrap();
        let a = s.execute("footnote.insert", &json!({"text": "Outer"})).unwrap()["id"].as_u64().unwrap();
        let c = s.execute("footnote.insert", &json!({"text": "Inner"})).unwrap()["id"].as_u64().unwrap();
        // Typing a reference character (as a paste brings) into the nested note.
        s.execute("text.insert", &json!({"text": designcraft_doc::FOOTNOTE_REF.to_string(), "raw": true})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        let pasted = st.note(c).unwrap().text.notes[0].id;
        assert!(pasted != a && pasted != c, "{pasted}");
        assert_eq!(st.note(pasted).map(|n| n.text.text.as_str()), Some(""));
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
