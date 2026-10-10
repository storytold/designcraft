//! Type › Insert Endnote and Document Endnote Options. Endnote text lives with its reference;
//! the endnote frame (made on a new last page with the first endnote) lists them all.

use designcraft_doc::{ParaFormat, SpreadRef, StoryId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, has_text, ok, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "endnote.insert",
            "Insert Endnote",
            ["Type"],
            None,
            "{text?} — a reference at the insertion point; the first one makes the endnote frame on a new last page → {story, id, frame}",
            has_text,
            insert
        ),
        cmd!("endnote.edit", "Edit Endnote", [], None, "{story, id, text}", has_doc, |s, p| {
            let (sid, id) = target(p)?;
            let text = str_param(p, "text").unwrap_or("").to_string();
            s.edit(|d, _| {
                let style = d.endnote_options.para_style.clone();
                let st = d.story_mut(sid).ok_or_else(|| bad("endnote.edit", "no such story"))?;
                let n = st.endnote_mut(id).ok_or_else(|| bad("endnote.edit", format!("no endnote {id}")))?;
                n.text = designcraft_doc::Story::with_text(StoryId(0), &text, ParaFormat { style, ..Default::default() });
                ok()
            })
        }),
        cmd!("endnote.delete", "Delete Endnote", [], None, "{story, id} — removes the reference and its text", has_doc, |s, p| {
            let (sid, id) = target(p)?;
            s.edit(|d, _| {
                let st = d.story_mut(sid).ok_or_else(|| bad("endnote.delete", "no such story"))?;
                let k = st.endnotes.iter().position(|n| n.id == id).ok_or_else(|| bad("endnote.delete", format!("no endnote {id}")))?;
                let at = st
                    .text
                    .match_indices(designcraft_doc::ENDNOTE_REF)
                    .nth(k)
                    .map(|(i, _)| i)
                    .ok_or_else(|| bad("endnote.delete", format!("endnote {id} has no reference in the text")))?;
                st.delete(at..at + designcraft_doc::ENDNOTE_REF.len_utf8());
                ok()
            })
        }),
        cmd!(query "endnote.list", "Endnotes", [], None, "{} → [{story, id, number, text}] in numbering order", has_doc, |s, _| {
            let d = &s.doc()?.doc;
            let mut out = Vec::new();
            for st in d.stories.values().filter(|st| !st.endnotes.is_empty()) {
                let first = d.endnote_start(st.id);
                for (k, n) in st.endnotes.iter().enumerate() {
                    out.push(json!({"story": st.id.0, "id": n.id, "number": first + k as u32, "text": n.text.text}));
                }
            }
            out.sort_by_key(|v| v["number"].as_u64());
            Ok(Value::Array(out))
        }),
        cmd!(
            "endnote.options",
            "Document Endnote Options…",
            ["Type"],
            None,
            "{style?: arabic|upperRoman|lowerRoman|upperLetters|lowerLetters|kanji, startAt?, prefix?, suffix?, heading?, headingStyle?, paraStyle?, separator?} → the options",
            has_doc,
            |s, p| {
                let cur = serde_json::to_value(&s.doc()?.doc.endnote_options).map_err(|e| bad("endnote.options", e.to_string()))?;
                let mut new = cur.clone();
                if let (Some(n), Some(o)) = (new.as_object_mut(), p.as_object()) {
                    for (k, v) in o {
                        if !n.contains_key(k) {
                            return Err(bad("endnote.options", format!("unknown option {k}")));
                        }
                        n.insert(k.clone(), v.clone());
                    }
                }
                let opts: designcraft_doc::EndnoteOptions = serde_json::from_value(new.clone()).map_err(|e| bad("endnote.options", e.to_string()))?;
                if new != cur {
                    s.edit(|d, _| {
                        d.endnote_options = opts;
                        ok()
                    })?;
                }
                Ok(new)
            }
        ),
    ]
}

fn target(p: &Value) -> Result<(StoryId, u64)> {
    let sid = p.get("story").and_then(Value::as_u64).ok_or_else(|| bad("endnote", "`story` required"))?;
    let id = p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("endnote", "`id` required"))?;
    Ok((StoryId(sid), id))
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "endnote.insert";
    let st = s.doc()?;
    let ts = st.selection.text.ok_or_else(|| bad(ID, "place the insertion point in text"))?;
    if ts.cell.is_some() {
        return Err(bad(ID, "endnotes go in story text, not tables or footnotes"));
    }
    if st.doc.endnote_story == Some(ts.story) {
        return Err(bad(ID, "the endnote frame lists the endnotes"));
    }
    let text = str_param(p, "text").unwrap_or("").to_string();
    let at = ts.anchor.max(ts.focus);
    s.edit(|d, sel| {
        // The endnote frame: a text frame in the margins of a new last page.
        let mut frame = None;
        if d.endnote_story.is_none() {
            let last = d.page_count().saturating_sub(1);
            let parent = d.page_loc(last).and_then(|(si, pi)| d.spreads[si].pages[pi].parent);
            d.insert_pages(Some(last), 1, parent)?;
            let (si, pi) = d.page_loc(last + 1).ok_or_else(|| bad(ID, "page insert failed"))?;
            let r = d.spreads[si].pages[pi].margin_rect();
            let layer = d.default_layer();
            let (fid, esid) = d.add_text_frame(SpreadRef::Doc(si), r, layer, "", ParaFormat::default())?;
            d.endnote_story = Some(esid);
            frame = Some(fid.0);
        }
        let style = d.endnote_options.para_style.clone();
        let story = d.story_mut(ts.story).ok_or_else(|| bad(ID, "no such story"))?;
        let id = story.insert_endnote(at, &text, ParaFormat { style, ..Default::default() });
        let caret = at + designcraft_doc::ENDNOTE_REF.len_utf8();
        if let Some(t) = &mut sel.text {
            (t.anchor, t.focus) = (caret, caret);
        }
        Ok(json!({"story": ts.story.0, "id": id, "frame": frame}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn endnotes_number_through_the_document_and_fill_the_endnote_frame() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Alpha beta."})).unwrap();
        s.execute("text.select", &json!({"story": a["story"], "anchor": 5, "focus": 5})).unwrap();
        let r1 = s.execute("endnote.insert", &json!({"text": "First source."})).unwrap();
        let pages = s.doc().unwrap().doc.page_count();
        assert_eq!(pages, 3, "a new last page holds the endnote frame");
        assert!(r1["frame"].is_u64());
        s.execute("text.select", &json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
        let r0 = s.execute("endnote.insert", &json!({"text": "Earlier."})).unwrap();
        assert!(r0["frame"].is_null(), "one endnote frame");
        let list = s.execute("endnote.list", &json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 2);
        let d = s.doc().unwrap().doc.clone();
        let esid = d.endnote_story.unwrap();
        assert_eq!(d.stories[&esid].text, "Endnotes\n1\tEarlier.\n2\tFirst source.", "numbered in text order");
        // The references are numbered in the composed text.
        let sid = designcraft_doc::StoryId(a["story"].as_u64().unwrap());
        let cs = s.cache.get(&d, sid, None);
        let shown = cs.frames[0].lines[0].glyphs.len();
        assert!(shown > "Alpha beta.".chars().count(), "reference numbers take glyphs");
        s.execute("endnote.edit", &json!({"story": r1["story"], "id": r1["id"], "text": "Edited."})).unwrap();
        s.execute("endnote.options", &json!({"style": "lowerRoman", "heading": "Notes"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.stories[&esid].text, "Notes\ni\tEarlier.\nii\tEdited.");
        s.execute("endnote.delete", &json!({"story": r0["story"], "id": r0["id"]})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.stories[&esid].text, "Notes\ni\tEdited.");
        assert_eq!(d.stories[&sid].text.matches(designcraft_doc::ENDNOTE_REF).count(), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.stories[&esid].text, "Notes\ni\tEarlier.\nii\tEdited.");
    }
}
