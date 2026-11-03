//! Type › Notes: editorial notes anchored in text (never printed or exported).

use std::sync::Arc;

use designcraft_doc::{ANCHOR_MARK, ENDNOTE_REF, EditorialNote, FOOTNOTE_REF, INDEX_MARK, NOTE_MARK, OBJECT_MARK, StoryId, TABLE_ANCHOR, XREF_MARK};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, has_text, ok, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!("note.new", "New Note", ["Type", "Notes"], None, "{text, author?} — at the insertion point → {story, id}", has_text, |s, p| {
            let st = s.doc()?;
            let ts = st.selection.text.ok_or_else(|| bad("note.new", "place the insertion point in text"))?;
            if ts.cell.is_some() {
                return Err(bad("note.new", "notes go in story text"));
            }
            let text = super::text_param(p, "text");
            let author = str_param(p, "author").unwrap_or("").to_string();
            let at = ts.anchor.max(ts.focus);
            s.edit(|d, sel| {
                let story = d.story_mut(ts.story).ok_or_else(|| bad("note.new", "no such story"))?;
                let fmt = story.char_format_at(at).clone();
                story.insert_with(at, &NOTE_MARK.to_string(), fmt);
                // A file may hold fewer notes than anchors; give every anchor one.
                story.fix_marks();
                let k = story.text[..at].matches(NOTE_MARK).count();
                let n = story.editorial.get_mut(k).ok_or_else(|| bad("note.new", "the story's notes don't match their anchors"))?;
                let n = Arc::make_mut(n);
                (n.text, n.author) = (text, author);
                let id = n.id;
                if let Some(t) = &mut sel.text {
                    let c = at + NOTE_MARK.len_utf8();
                    (t.anchor, t.focus) = (c, c);
                }
                Ok(json!({"story": ts.story.0, "id": id}))
            })
        }),
        cmd!(
            "note.convertFromText",
            "Convert to Note",
            ["Type", "Notes"],
            None,
            "{author?} → {story, id}. The selected text moves into a new note anchored where it was.",
            has_text_range,
            convert_from_text
        ),
        cmd!("note.edit", "Edit Note", [], None, "{story, id, text?, author?}", has_doc, |s, p| {
            let (sid, id) = target(p)?;
            let (text, author) = (str_param(p, "text").map(|_| super::text_param(p, "text")), str_param(p, "author").map(str::to_string));
            s.edit(|d, _| {
                let st = d.story_mut(sid).ok_or_else(|| bad("note.edit", "no such story"))?;
                let n = st.editorial.iter_mut().find(|n| n.id == id).ok_or_else(|| bad("note.edit", format!("no note {id}")))?;
                let n = Arc::make_mut(n);
                if let Some(t) = text {
                    n.text = t;
                }
                if let Some(a) = author {
                    n.author = a;
                }
                st.rev = st.rev.wrapping_add(1);
                ok()
            })
        }),
        cmd!("note.delete", "Delete Note", ["Type", "Notes"], None, "{story, id}", has_doc, |s, p| {
            let (sid, id) = target(p)?;
            s.edit(|d, sel| {
                let st = d.story_mut(sid).ok_or_else(|| bad("note.delete", "no such story"))?;
                let at = anchor(st, id).ok_or_else(|| bad("note.delete", format!("no note {id}")))?;
                follow_replace(sel, sid, at..at + NOTE_MARK.len_utf8(), 0);
                st.delete(at..at + NOTE_MARK.len_utf8());
                ok()
            })
        }),
        cmd!(
            "note.convertToText",
            "Convert Note to Text",
            ["Type", "Notes"],
            None,
            "{story, id} — the note's text replaces its anchor",
            has_doc,
            |s, p| {
                let (sid, id) = target(p)?;
                s.edit(|d, sel| {
                    let st = d.story_mut(sid).ok_or_else(|| bad("note.convertToText", "no such story"))?;
                    let at = anchor(st, id).ok_or_else(|| bad("note.convertToText", format!("no note {id}")))?;
                    let text = st.editorial.iter().find(|n| n.id == id).map(|n| n.text.clone()).unwrap_or_default();
                    follow_replace(sel, sid, at..at + NOTE_MARK.len_utf8(), text.len());
                    st.replace(at..at + NOTE_MARK.len_utf8(), &text);
                    ok()
                })
            }
        ),
        cmd!(
            "note.split",
            "Split Note",
            ["Type", "Notes"],
            None,
            "{story, id, at} → {story, id}. The note's text from byte `at` on moves into a new note anchored right after it.",
            has_doc,
            split
        ),
        cmd!(query "note.list", "Notes", [], None, "{} → [{story, id, at, author, text}] in story order", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let mut out = Vec::new();
            for st in d.stories.values() {
                for (k, (at, _)) in st.text.match_indices(NOTE_MARK).enumerate() {
                    if let Some(n) = st.editorial.get(k) {
                        out.push(json!({"story": st.id.0, "id": n.id, "at": at, "author": n.author, "text": n.text}));
                    }
                }
            }
            Ok(Value::Array(out))
        }),
        cmd!(
            noundo "note.previous",
            "Previous Note",
            ["Type", "Notes"],
            None,
            "{} → {story, id, pos}. The caret moves after the anchor of the story's previous note.",
            has_previous,
            |s, _| go_to(s, false)
        ),
        cmd!(
            noundo "note.next",
            "Next Note",
            ["Type", "Notes"],
            None,
            "{} → {story, id, pos}. The caret moves after the anchor of the story's next note.",
            has_next,
            |s, _| go_to(s, true)
        ),
    ]
}

/// Characters whose payload lives outside the story text: a note made from text that holds one
/// would lose it.
const MARKS: [char; 8] = [NOTE_MARK, ENDNOTE_REF, FOOTNOTE_REF, OBJECT_MARK, INDEX_MARK, ANCHOR_MARK, XREF_MARK, TABLE_ANCHOR];

fn has_text_range(s: &Session) -> std::result::Result<(), String> {
    has_text(s)?;
    match s.active().and_then(|d| d.selection.text) {
        Some(t) if t.cell.is_some() => Err("notes go in story text".into()),
        Some(t) if t.is_caret() => Err("select the text to convert".into()),
        _ => Ok(()),
    }
}

/// Type › Notes › Convert to Note: the selected text leaves the story and becomes the text of a
/// new note, whose anchor takes the text's place.
fn convert_from_text(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "note.convertFromText";
    let ts = s.doc()?.selection.text.ok_or_else(|| bad(ID, "select the text to convert"))?;
    if ts.cell.is_some() {
        return Err(bad(ID, "notes go in story text"));
    }
    let r = ts.range();
    if r.is_empty() {
        return Err(bad(ID, "select the text to convert"));
    }
    let author = str_param(p, "author").unwrap_or("").to_string();
    s.edit(|d, sel| {
        let story = d.story_mut(ts.story).ok_or_else(|| bad(ID, "no such story"))?;
        let text = story.text.get(r.clone()).ok_or_else(|| bad(ID, "the selection is outside the story"))?.to_string();
        if text.contains(MARKS) {
            return Err(bad(ID, "the selection holds a table, an anchored object, a note, a footnote, an endnote or a marker"));
        }
        let fmt = story.format_after(r.start).clone();
        story.delete(r.clone());
        story.insert_with(r.start, &NOTE_MARK.to_string(), fmt);
        story.fix_marks();
        let k = story.text[..r.start].matches(NOTE_MARK).count();
        let n = story.editorial.get_mut(k).ok_or_else(|| bad(ID, "the story's notes don't match their anchors"))?;
        let n = Arc::make_mut(n);
        (n.text, n.author) = (text, author);
        let id = n.id;
        if let Some(t) = &mut sel.text {
            let c = r.start + NOTE_MARK.len_utf8();
            (t.anchor, t.focus) = (c, c);
        }
        Ok(json!({"story": ts.story.0, "id": id}))
    })
}

/// Split Note (the Notes panel's Split Note button): the text after the split point leaves the note
/// and becomes the text of a new note by the same author, whose anchor directly follows the note's anchor.
fn split(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "note.split";
    let (sid, id) = target(p)?;
    let at = p.get("at").and_then(Value::as_u64).ok_or_else(|| bad(ID, "`at` required"))?;
    s.edit(|d, sel| {
        let story = d.story_mut(sid).ok_or_else(|| bad(ID, "no such story"))?;
        let pos = anchor(story, id).ok_or_else(|| bad(ID, format!("no note {id}")))?;
        let note = story.editorial.iter().find(|n| n.id == id).cloned().ok_or_else(|| bad(ID, format!("no note {id}")))?;
        let at = usize::try_from(at).ok().filter(|&a| a > 0 && a < note.text.len() && note.text.is_char_boundary(a));
        let at = at.ok_or_else(|| bad(ID, "place the split point inside the note's text"))?;
        let (head, tail) = note.text.split_at(at);
        let (head, tail) = (head.to_string(), tail.to_string());
        let ins = pos + NOTE_MARK.len_utf8();
        let fmt = story.char_format_at(pos).clone();
        story.insert_with(ins, &NOTE_MARK.to_string(), fmt);
        story.fix_marks();
        let k = story.text[..ins].matches(NOTE_MARK).count();
        let first = story.editorial.get_mut(k - 1).ok_or_else(|| bad(ID, "the story's notes don't match their anchors"))?;
        Arc::make_mut(first).text = head;
        let n = story.editorial.get_mut(k).ok_or_else(|| bad(ID, "the story's notes don't match their anchors"))?;
        let n = Arc::make_mut(n);
        (n.text, n.author) = (tail, note.author.clone());
        let new = n.id;
        if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell.is_none()) {
            for c in [&mut t.anchor, &mut t.focus] {
                if *c >= ins {
                    *c += NOTE_MARK.len_utf8();
                }
            }
        }
        Ok(json!({"story": sid.0, "id": new}))
    })
}

fn target(p: &Value) -> Result<(StoryId, u64)> {
    let sid = p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("note", "`story` required"))?;
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("note", "`id` required"))?;
    Ok((StoryId(sid), id))
}

/// Byte offset of note `id`'s anchor.
fn anchor(st: &designcraft_doc::Story, id: u64) -> Option<usize> {
    let k = st.editorial.iter().position(|n: &Arc<EditorialNote>| n.id == id)?;
    st.text.match_indices(NOTE_MARK).nth(k).map(|(i, _)| i)
}

/// Keeps a caret or selection in story `sid`'s own text (not a cell) on the same characters when `range` is replaced
/// by `len` bytes: an end after the range moves by the change in length, and an end inside it moves to its start.
fn follow_replace(sel: &mut designcraft_doc::Selection, sid: StoryId, range: std::ops::Range<usize>, len: usize) {
    if let Some(t) = sel.text.as_mut().filter(|t| t.story == sid && t.cell.is_none()) {
        let follow = |i: usize| if i >= range.end { i - range.len() + len } else { i.min(range.start) };
        t.anchor = follow(t.anchor);
        t.focus = follow(t.focus);
    }
}

/// The anchor offset and id of the note before or after the selection `t` in its story. Going
/// back skips the note whose anchor the caret directly follows, so repeated steps reach every note.
fn neighbor(st: &designcraft_doc::Story, t: &designcraft_doc::TextSel, forward: bool) -> Option<(usize, u64)> {
    let r = t.range();
    let mut anchors = st.text.match_indices(NOTE_MARK).enumerate().map(|(k, (i, _))| (k, i));
    let (k, at) = if forward { anchors.find(|&(_, i)| i >= r.end) } else { anchors.filter(|&(_, i)| i + NOTE_MARK.len_utf8() < r.start).last() }?;
    Some((at, st.editorial.get(k)?.id))
}

fn has_note(s: &crate::Session, forward: bool) -> std::result::Result<(), String> {
    has_text(s)?;
    match s.active().and_then(|d| Some((d, d.selection.text?))) {
        Some((_, t)) if t.cell.is_some() => Err("notes go in story text".into()),
        Some((d, t)) if d.doc.story(t.story).and_then(|st| neighbor(st, &t, forward)).is_some() => Ok(()),
        _ => Err(no_note(forward).into()),
    }
}

fn has_previous(s: &crate::Session) -> std::result::Result<(), String> {
    has_note(s, false)
}

fn has_next(s: &crate::Session) -> std::result::Result<(), String> {
    has_note(s, true)
}

fn no_note(forward: bool) -> &'static str {
    if forward { "no later note in this story" } else { "no earlier note in this story" }
}

/// Type › Notes › Previous Note and Next Note: from the insertion point, the caret moves after
/// the anchor of the story's previous or next note.
fn go_to(s: &mut crate::Session, forward: bool) -> Result<Value> {
    let id = if forward { "note.next" } else { "note.previous" };
    let st = s.doc_mut()?;
    let t = st.selection.text.ok_or_else(|| bad(id, "place the insertion point in text"))?;
    if t.cell.is_some() {
        return Err(bad(id, "notes go in story text"));
    }
    let (at, note) = st.doc.story(t.story).and_then(|x| neighbor(x, &t, forward)).ok_or_else(|| bad(id, no_note(forward)))?;
    let pos = at + NOTE_MARK.len_utf8();
    st.selection.text = Some(designcraft_doc::TextSel::caret(t.story, pos));
    st.bump_revision();
    s.typing_format = None;
    Ok(json!({"story": t.story.0, "id": note, "pos": pos}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn notes_are_anchored_unprinted_and_convertible() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let end_x = |s: &Session| s.cache.get(&s.doc().unwrap().doc, sid, None).frames[0].lines[0].end_x;
        let before = end_x(&s);
        s.execute("text.select", &json!({"story": r["story"], "anchor": 5, "focus": 5})).unwrap();
        let n = s.execute("note.new", &json!({"text": "check this", "author": "Ed"})).unwrap();
        assert!((end_x(&s) - before).abs() < 1e-6, "a note takes no space");
        let list = s.execute("note.list", &json!({})).unwrap();
        assert_eq!(list[0]["text"], "check this");
        assert_eq!(list[0]["at"], 5);
        let txt = s.execute("file.exportText", &json!({"story": r["story"]})).unwrap();
        assert_eq!(txt["text"], "Draft copy.", "not exported");
        s.execute("note.edit", &json!({"story": n["story"], "id": n["id"], "text": " (checked)"})).unwrap();
        s.execute("note.convertToText", &json!({"story": n["story"], "id": n["id"]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.stories[&sid].text, "Draft (checked) copy.");
        assert!(s.doc().unwrap().doc.stories[&sid].editorial.is_empty());
    }

    #[test]
    fn previous_and_next_note_step_through_the_story_s_notes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One two three."})).unwrap();
        let len = designcraft_doc::NOTE_MARK.len_utf8();
        let caret = |s: &mut Session, at: usize| s.execute("text.select", &json!({"story": r["story"], "anchor": at, "focus": at})).unwrap();
        caret(&mut s, 3);
        let first = s.execute("note.new", &json!({"text": "a"})).unwrap();
        caret(&mut s, 7 + len);
        let second = s.execute("note.new", &json!({"text": "b"})).unwrap();
        caret(&mut s, 0);
        let disabled = |s: &mut Session, id: &str| matches!(s.execute(id, &json!({})), Err(crate::EngineError::Disabled(..)));
        assert!(disabled(&mut s, "note.previous"), "no note before the start");
        let n = s.execute("note.next", &json!({})).unwrap();
        assert_eq!((n["id"].clone(), n["pos"].clone()), (first["id"].clone(), json!(3 + len)));
        let n = s.execute("note.next", &json!({})).unwrap();
        assert_eq!((n["id"].clone(), n["pos"].clone()), (second["id"].clone(), json!(7 + 2 * len)));
        let t = s.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.anchor, t.focus), (7 + 2 * len, 7 + 2 * len), "the caret follows the anchor");
        assert!(disabled(&mut s, "note.next"), "no note after the last");
        // From just after the second anchor, Previous Note goes to the first one.
        let n = s.execute("note.previous", &json!({})).unwrap();
        assert_eq!(n["id"], first["id"]);
        assert!(disabled(&mut s, "note.previous"));
        // From the middle of the story, each direction finds its neighbor.
        caret(&mut s, 5 + len);
        assert_eq!(s.execute("note.previous", &json!({})).unwrap()["id"], first["id"]);
        caret(&mut s, 5 + len);
        assert_eq!(s.execute("note.next", &json!({})).unwrap()["id"], second["id"]);
        // Formatting set at the insertion point does not survive the move.
        s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        assert!(s.typing_format.is_some());
        s.execute("note.previous", &json!({})).unwrap();
        assert!(s.typing_format.is_none(), "note.previous");
        s.execute("type.char", &json!({"attrs": {"size": 30}})).unwrap();
        s.execute("note.next", &json!({})).unwrap();
        assert!(s.typing_format.is_none(), "note.next");
    }

    #[test]
    fn notes_insert_into_a_story_whose_markers_lack_payloads() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        // A damaged file: a note anchor, an endnote and a footnote reference without payloads.
        let st = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc).story_mut(sid).unwrap();
        st.insert(0, &format!("{}{}{}", designcraft_doc::NOTE_MARK, designcraft_doc::ENDNOTE_REF, designcraft_doc::FOOTNOTE_REF));
        st.editorial.clear();
        st.endnotes.clear();
        st.notes.clear();
        let end = st.len();
        s.execute("text.select", &json!({"story": r["story"], "anchor": end, "focus": end})).unwrap();
        s.execute("note.new", &json!({"text": "check this"})).unwrap();
        s.execute("endnote.insert", &json!({"text": "An endnote."})).unwrap();
        s.execute("footnote.insert", &json!({"text": "A footnote."})).unwrap();
        let st = &s.doc().unwrap().doc.stories[&sid];
        st.check().unwrap();
        assert_eq!(st.editorial[1].text, "check this");
        assert_eq!(st.endnotes[1].text.text, "An endnote.");
        assert_eq!(st.notes[1].text.text, "A footnote.");
    }

    #[test]
    fn selected_text_converts_to_a_note_at_its_place() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One two\nthree four."})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let mark = designcraft_doc::NOTE_MARK.len_utf8();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 3, "focus": 3})).unwrap();
        s.execute("note.new", &json!({"text": "earlier"})).unwrap();
        // "two\nthree " across the paragraph return, selected backwards.
        let (a, b) = (3 + mark + 1, 3 + mark + 1 + "two\nthree ".len());
        s.execute("text.select", &json!({"story": r["story"], "anchor": b, "focus": a})).unwrap();
        let n = s.execute("note.convertFromText", &json!({"author": "Ed"})).unwrap();
        let st = &s.doc().unwrap().doc.stories[&sid];
        st.check().unwrap();
        assert_eq!(st.text, format!("One{m} {m}four.", m = designcraft_doc::NOTE_MARK));
        assert_eq!(st.paras.len(), 1, "the paragraph return went into the note");
        let list = s.execute("note.list", &json!({})).unwrap();
        assert_eq!(list[0]["text"], "earlier");
        assert_eq!((list[1]["id"].clone(), list[1]["at"].clone()), (n["id"].clone(), json!(a)));
        assert_eq!((list[1]["text"].as_str(), list[1]["author"].as_str()), (Some("two\nthree "), Some("Ed")));
        let caret = s.doc().unwrap().selection.text.map(|t| (t.anchor, t.focus));
        assert_eq!(caret, Some((a + mark, a + mark)), "the insertion point follows the anchor");
        s.execute("note.convertToText", &json!({"story": n["story"], "id": n["id"]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.stories[&sid].text, format!("One{} two\nthree four.", designcraft_doc::NOTE_MARK));
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        let st = &s.doc().unwrap().doc.stories[&sid];
        assert_eq!((st.text.as_str(), st.editorial.len()), (format!("One{} two\nthree four.", designcraft_doc::NOTE_MARK).as_str(), 1));
    }

    #[test]
    fn split_note_moves_the_text_after_the_split_point_into_a_new_note() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let mark = designcraft_doc::NOTE_MARK.len_utf8();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 5, "focus": 5})).unwrap();
        let n = s.execute("note.new", &json!({"text": "check this", "author": "Ed"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 9 + mark, "focus": 9 + mark})).unwrap();
        let later = s.execute("note.new", &json!({"text": "later"})).unwrap();
        // The caret sits after "Draft" and its anchor, before " copy".
        s.execute("text.select", &json!({"story": r["story"], "anchor": 5 + mark, "focus": 5 + mark})).unwrap();
        let before = s.doc().unwrap().doc.stories[&sid].clone();
        let split = |s: &mut Session, at: u64| s.execute("note.split", &json!({"story": n["story"], "id": n["id"], "at": at}));
        for at in [0, 10, 11, 99] {
            assert!(matches!(split(&mut s, at), Err(crate::EngineError::BadParams { .. })), "split point {at}");
        }
        assert_eq!(s.doc().unwrap().doc.stories[&sid], before);
        let m = split(&mut s, 6).unwrap();
        let st = &s.doc().unwrap().doc.stories[&sid];
        st.check().unwrap();
        assert_eq!(st.text, format!("Draft{m}{m} cop{m}y.", m = designcraft_doc::NOTE_MARK));
        let list = s.execute("note.list", &json!({})).unwrap();
        let row = |k: usize| (list[k]["id"].clone(), list[k]["at"].clone(), list[k]["text"].clone(), list[k]["author"].clone());
        assert_eq!(row(0), (n["id"].clone(), json!(5), json!("check "), json!("Ed")));
        assert_eq!(row(1), (m["id"].clone(), json!(5 + mark), json!("this"), json!("Ed")), "the new note follows the first");
        assert_eq!(row(2).0, later["id"], "the later note keeps its text");
        assert_eq!(list[2]["text"], "later");
        let caret = s.doc().unwrap().selection.text.map(|t| (t.anchor, t.focus));
        assert_eq!(caret, Some((5 + 2 * mark, 5 + 2 * mark)), "the caret stays before \" copy\"");
        // Multibyte text splits on character boundaries only.
        let e = s.execute("note.new", &json!({"text": "été"})).unwrap();
        let refused = s.execute("note.split", &json!({"story": e["story"], "id": e["id"], "at": 1}));
        assert!(matches!(refused, Err(crate::EngineError::BadParams { .. })), "inside a character");
        s.execute("note.split", &json!({"story": e["story"], "id": e["id"], "at": 2})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.stories[&sid], before, "undo restores the one note");
    }

    #[test]
    fn convert_to_note_needs_selected_story_text_without_objects() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": r["story"], "anchor": 5, "focus": 5})).unwrap();
        let disabled = |r| matches!(r, Err(crate::EngineError::Disabled(..)));
        assert!(disabled(s.execute("note.convertFromText", &json!({}))), "an insertion point selects no text");
        s.execute("note.new", &json!({"text": "keep"})).unwrap();
        let before = s.doc().unwrap().doc.stories[&sid].clone();
        let all = before.len();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": all})).unwrap();
        let refused = s.execute("note.convertFromText", &json!({}));
        assert!(matches!(refused, Err(crate::EngineError::BadParams { .. })), "a note inside the selection would be lost");
        assert_eq!(s.doc().unwrap().doc.stories[&sid], before);
        s.execute("text.select", &json!({"story": r["story"], "anchor": all, "focus": all})).unwrap();
        s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
        s.execute("text.insert", &json!({"text": "cell", "raw": true})).unwrap();
        s.execute("text.move", &json!({"dir": "left", "extend": true})).unwrap();
        assert!(s.doc().unwrap().selection.text.is_some_and(|t| t.cell.is_some() && !t.is_caret()));
        assert!(disabled(s.execute("note.convertFromText", &json!({}))), "notes go in story text");
    }

    #[test]
    fn deleting_or_converting_a_note_keeps_the_caret_on_its_character() {
        let m = designcraft_doc::NOTE_MARK.len_utf8();
        // (anchor, focus) in "Draft{note} copy." before the command, then (anchor, focus) and the selected text after
        // Delete Note and after Convert Note to Text with the note's text "ab".
        let cases = [
            ((6 + m, 10 + m), ((6, 10), "copy"), ((8, 12), "copy")),
            ((0, 10 + m), ((0, 10), "Draft copy"), ((0, 12), "Draftab copy")),
            ((2, 4), ((2, 4), "af"), ((2, 4), "af")),
            ((5 + m, 5 + m), ((5, 5), ""), ((7, 7), "")),
        ];
        for (before, deleted, converted) in cases {
            for (cmd, (after, selected)) in [("note.delete", deleted), ("note.convertToText", converted)] {
                let mut s = Session::new();
                s.execute("file.new", &json!({})).unwrap();
                let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
                let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
                s.execute("text.select", &json!({"story": r["story"], "anchor": 5, "focus": 5})).unwrap();
                let n = s.execute("note.new", &json!({"text": "ab"})).unwrap();
                s.execute("text.select", &json!({"story": r["story"], "anchor": before.0, "focus": before.1})).unwrap();
                s.execute(cmd, &json!({"story": n["story"], "id": n["id"]})).unwrap();
                let t = s.doc().unwrap().selection.text.unwrap();
                assert_eq!((t.anchor, t.focus), after, "{cmd} with {before:?} selected");
                assert_eq!(&s.doc().unwrap().doc.stories[&sid].text[t.anchor..t.focus], selected, "{cmd} with {before:?} selected");
            }
        }
    }
}
