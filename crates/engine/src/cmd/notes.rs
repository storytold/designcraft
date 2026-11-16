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
            "{text?, series?} — a footnote reference at the insertion point; the caret moves into the new footnote's text",
            in_story_text,
            insert
        ),
        cmd!(
            "footnote.options",
            "Document Footnote Options…",
            ["Type"],
            None,
            "{placement?: {kind: columnFoot|margin|stream, side?: inside|outside|left|right (default right), gap: pt, width: pt (gap and width required when first selecting margin)}, style?: arabic|upperRoman|lowerRoman|upperLetters|lowerLetters|symbols|kanji, startAt?, restart?: never|page|spread|section, prefix?, suffix?, affixIn?: none|reference|text|both, refHidden?: bool (zero-width unprinted anchor), refPosition?: superscript|subscript|normal|otSuperscript, refCharStyle?, paraStyle?, separator?, spaceBefore?, spaceBetween?, firstBaseline?: ascent|capHeight|leading|xHeight|emboxHeight|fixed, firstBaselineMin?, spanColumns?, allowSplit? (default true), rule?: {on, weight, color, tint, width, offset, leftIndent}} → the options (no params: just read them)",
            has_doc,
            options
        ),
        cmd!(
            "footnote.series.create",
            "Create Note Series",
            [],
            None,
            "{name, placement?: stream|{kind: columnFoot|margin|stream, …}, refHidden?: bool, …options} → {id}",
            has_doc,
            series_create
        ),
        cmd!(
            "footnote.series.options",
            "Note Series Options",
            [],
            None,
            "{series, …options} → options; placement?: {kind: columnFoot|margin|stream, side?: inside|outside|left|right (default right), gap: pt, width: pt (gap and width required when first selecting margin)}",
            has_doc,
            series_options
        ),
        cmd!(
            "footnote.series.delete",
            "Delete Note Series",
            [],
            None,
            "{series} — moves its notes to series 0; series 0 cannot be deleted",
            has_doc,
            series_delete
        ),
        cmd!(query "footnote.series.list", "Note Series", [], None, "{} → [{id, name, …options}] including series 0", has_doc, series_list),
        cmd!(query "footnote.list", "Footnotes", [], None, "{story?, series?} → [{story, id, series, index, anchor, text}] in story order", has_doc, list),
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
    let series = series_id(p, "footnote.insert", true)?;
    require_series(&s.doc()?.doc, series, "footnote.insert")?;
    let r = s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("footnote.insert", "no insertion point"))?;
        let para = ParaFormat { style: d.note_options(series).para_style.clone(), ..Default::default() };
        let st = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let r = t.range();
        let r = r.start.min(st.len())..r.end.min(st.len());
        st.delete(r.clone());
        let id = st.insert_note(r.start, &text, para).ok_or_else(|| bad("footnote.insert", "the story's footnotes don't match their references"))?;
        if let Some(note) = st.note_mut(id) {
            note.series = series;
        }
        d.adjust_text_ranges(t.story, r, designcraft_doc::FOOTNOTE_REF.len_utf8());
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
    let next = read_options(v, "footnote.options")?;
    validate_options(&next, "footnote.options")?;
    let out = serde_json::to_value(&next).unwrap_or_default();
    if next != cur {
        s.edit(|d, _| {
            d.footnote_options = next;
            Ok(Value::Null)
        })?;
    }
    Ok(out)
}

fn series_id(p: &Value, cmd: &str, default: bool) -> Result<u64> {
    match p.get("series") {
        None if default => Ok(0),
        Some(v) => v.as_u64().ok_or_else(|| bad(cmd, "series must be an unsigned integer")),
        _ => Err(bad(cmd, "missing series")),
    }
}

fn require_series(d: &designcraft_doc::Document, id: u64, cmd: &str) -> Result<()> {
    if id == 0 || d.note_series.iter().any(|s| s.id == id) { Ok(()) } else { Err(bad(cmd, format!("no note series {id}"))) }
}

fn validate_options(o: &FootnoteOptions, cmd: &str) -> Result<()> {
    o.check().map_err(|e| bad(cmd, e))
}

fn read_options(mut value: Value, cmd: &str) -> Result<FootnoteOptions> {
    if let Some(slot) = value.get_mut("placement")
        && let Some(kind) = slot.as_str().map(str::to_owned)
    {
        *slot = json!({"kind": kind});
    }
    // Native files preserve the pre-setting no-split behavior; new API options keep the
    // documented split-enabled default. Serialized current options already contain the flag.
    if let Some(options) = value.as_object_mut() {
        options.entry("allowSplit").or_insert(json!(true));
    }
    serde_json::from_value(value).map_err(|e| bad(cmd, e.to_string()))
}

fn series_create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = p
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| bad("footnote.series.create", "missing name"))?
        .to_owned();
    // New command objects use API defaults; native files may default a missing allowSplit to false.
    let mut value = serde_json::to_value(FootnoteOptions::default()).map_err(|e| bad("footnote.series.create", e.to_string()))?;
    merge(&mut value, p);
    let options = read_options(value, "footnote.series.create")?;
    validate_options(&options, "footnote.series.create")?;
    s.edit(|d, _| {
        let id = d
            .note_series
            .iter()
            .map(|s| s.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| bad("footnote.series.create", "series ids exhausted"))?;
        d.note_series.push(designcraft_doc::NoteSeries { id, name, options });
        Ok(json!({"id": id}))
    })
}

fn series_options(s: &mut Session, p: &Value) -> Result<Value> {
    let id = series_id(p, "footnote.series.options", false)?;
    require_series(&s.doc()?.doc, id, "footnote.series.options")?;
    let cur = s.doc()?.doc.note_options(id).clone();
    let mut value = serde_json::to_value(&cur).map_err(|e| bad("footnote.series.options", e.to_string()))?;
    merge(&mut value, p);
    let next = read_options(value, "footnote.series.options")?;
    validate_options(&next, "footnote.series.options")?;
    let out = serde_json::to_value(&next).map_err(|e| bad("footnote.series.options", e.to_string()))?;
    if next == cur {
        return Ok(out);
    }
    s.edit(|d, _| {
        if id == 0 {
            d.footnote_options = next;
        } else if let Some(series) = d.note_series.iter_mut().find(|s| s.id == id) {
            series.options = next;
        }
        Ok(Value::Null)
    })?;
    Ok(out)
}

fn series_list(s: &mut Session, _: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let mut list = vec![
        serde_json::to_value(designcraft_doc::NoteSeries { id: 0, name: "Footnotes".into(), options: d.footnote_options.clone() })
            .map_err(|e| bad("footnote.series.list", e.to_string()))?,
    ];
    for series in &d.note_series {
        list.push(serde_json::to_value(series).map_err(|e| bad("footnote.series.list", e.to_string()))?);
    }
    Ok(Value::Array(list))
}

fn series_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let id = series_id(p, "footnote.series.delete", false)?;
    if id == 0 {
        return Err(bad("footnote.series.delete", "series 0 cannot be deleted"));
    }
    require_series(&s.doc()?.doc, id, "footnote.series.delete")?;
    s.edit(|d, _| {
        for story in d.stories.values_mut() {
            if story.notes.iter().any(|n| n.series == id) {
                let story = std::sync::Arc::make_mut(story);
                for note in &mut story.notes {
                    if note.series == id {
                        std::sync::Arc::make_mut(note).series = 0;
                    }
                }
                story.rev = story.rev.wrapping_add(1);
            }
        }
        d.update_placed_items(
            |it| it.text_frame().and_then(|tf| tf.options.note_stream).is_some_and(|stream| stream.series == id),
            |it| {
                if let Some(tf) = it.text_frame_mut() {
                    tf.options.note_stream = None;
                }
            },
        );
        d.note_series.retain(|s| s.id != id);
        Ok(Value::Null)
    })
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
        for (k, n) in st.notes.iter().enumerate().filter(|(_, n)| p.get("series").and_then(Value::as_u64).is_none_or(|id| n.series == id)) {
            out.push(json!({"story": st.id.0, "id": n.id, "series": n.series, "index": k, "anchor": st.note_anchor(n.id), "text": n.text.text}));
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
        let len = designcraft_doc::FOOTNOTE_REF.len_utf8();
        follow_delete(sel, sid, at..at + len);
        st.delete(at..at + len);
        d.adjust_text_ranges(sid, at..at + len, 0);
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
    fn footnote_options_omitted_split_preserves_the_document_setting() {
        let (mut s, _) = session_with_text();
        assert_eq!(s.execute("footnote.options", &json!({"spaceBefore": 2.0})).unwrap()["allowSplit"], true);
        s.execute("footnote.options", &json!({"allowSplit": false})).unwrap();
        assert_eq!(s.execute("footnote.options", &json!({"spaceBefore": 3.0})).unwrap()["allowSplit"], false);
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

#[cfg(test)]
mod series_tests {
    use super::*;

    #[test]
    fn new_series_api_defaults_to_splitting_and_honors_explicit_false() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.footnote_options.allow_split);
        for (name, explicit, expected) in [("Default", None, true), ("Unsplit", Some(false), false), ("Split", Some(true), true)] {
            let mut params = json!({"name": name});
            if let Some(value) = explicit {
                params["allowSplit"] = json!(value);
            }
            let id = s.execute("footnote.series.create", &params).unwrap()["id"].as_u64().unwrap();
            let options = s.execute("footnote.series.options", &json!({"series": id, "separator": " "})).unwrap();
            assert_eq!(options["allowSplit"], expected);
        }
    }

    #[test]
    fn series_commands_insert_filter_delete_and_undo() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect":[72,72,400,600],"content":"text","text":"Body"})).unwrap();
        let fid = designcraft_doc::ItemId(frame["id"].as_u64().unwrap());
        let sid = s.doc().unwrap().doc.item(fid).unwrap().text_frame().unwrap().story;
        let series = s.execute("footnote.series.create", &json!({"name":"Sources","style":"lowerLetters"})).unwrap()["id"].as_u64().unwrap();
        s.execute("footnote.series.options", &json!({"series":series,"placement":{"kind":"margin","side":"outside","gap":8,"width":72}})).unwrap();
        s.execute("text.select", &json!({"story":sid.0,"anchor":4,"focus":4})).unwrap();
        s.execute("footnote.insert", &json!({"series":series,"text":"Source"})).unwrap();
        assert_eq!(s.execute("footnote.list", &json!({"series":series})).unwrap()[0]["series"], series);
        assert_eq!(s.execute("footnote.series.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
        assert!(s.execute("footnote.series.delete", &json!({"series":0})).is_err());
        assert!(s.execute("footnote.series.options", &json!({"series":series,"placement":{"width":0}})).is_err());
        s.execute("footnote.series.delete", &json!({"series":series})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes[0].series, 0);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().notes[0].series, series);
        assert!(s.execute("footnote.insert", &json!({"series":999})).is_err());
    }
}

#[cfg(test)]
mod margin_default_tests {
    use super::*;
    #[test]
    fn omitted_margin_side_defaults_to_right() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let o = s.execute("footnote.options", &json!({"placement":{"kind":"margin","gap":8,"width":72}})).unwrap();
        assert_eq!(o["placement"]["side"], "right");
        s.execute("footnote.series.create", &json!({"name":"Sources","placement":{"kind":"margin","gap":8,"width":72}})).unwrap();
        let all = s.execute("footnote.series.list", &json!({})).unwrap();
        assert_eq!(all[1]["placement"]["side"], "right");
    }
}

#[cfg(test)]
mod api_split_default_tests {
    use super::*;

    #[test]
    fn new_api_options_default_to_split_and_honor_explicit_false() {
        assert!(read_options(json!({}), "test").unwrap().allow_split);
        assert!(read_options(json!({"allowSplit": true}), "test").unwrap().allow_split);
        assert!(!read_options(json!({"allowSplit": false}), "test").unwrap().allow_split);
    }
}
