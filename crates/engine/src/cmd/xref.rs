//! Type › Hyperlinks & Cross-References: text anchors, cross-references and their formats.

use designcraft_doc::{CrossRef, StoryId, TextSel, XrefFormat};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "xref.insert",
            "Insert Cross-Reference…",
            ["Type", "Hyperlinks & Cross-References"],
            None,
            "{anchor? | paragraph?: text to find (first paragraph containing it) | story?: with para? | para?: 0-based paragraph of `story`, format?: name (default Full Paragraph & Page Number)} — at the insertion point; the text updates itself as the destination moves",
            in_story_text,
            insert
        ),
        cmd!(
            "anchor.create",
            "New Hyperlink Destination…",
            ["Type", "Hyperlinks & Cross-References"],
            None,
            "{name} — a text anchor at the insertion point",
            in_story_text,
            create_anchor
        ),
        cmd!(query "xref.list", "Cross-References", [], None, "{} → [{story, pos, target, format, text}]", has_doc, list),
        cmd!(query "anchor.list", "Text Anchors", [], None, "{} → [{id, name, story, pos, page}]", has_doc, list_anchors),
        cmd!(query "xref.formats", "Cross-Reference Formats", [], None, "{} → [{name, definition}]", has_doc, |s, _| {
            Ok(serde_json::to_value(&s.doc()?.doc.xref_formats).unwrap_or_default())
        }),
        cmd!(
            "xref.defineFormat",
            "Define Cross-Reference Format…",
            ["Type", "Hyperlinks & Cross-References"],
            None,
            "{name, definition} — building blocks: <fullPara />, <paraText />, <paraNum />, <pageNum />, <txtAnchrName />, <chapNum />, <fileName />, <partialPara delim=\":\" includeDelim=\"false\" />; creates or replaces",
            has_doc,
            define_format
        ),
        cmd!("xref.setFormat", "Set Cross-Reference Format", [], None, "{story, index, format}", has_doc, set_format),
    ]
}

fn in_story_text(s: &Session) -> std::result::Result<(), String> {
    super::has_text(s)?;
    match s.active().and_then(|d| d.selection.text) {
        Some(t) if t.cell.is_some() => Err("cross-references go in story text (not table cells or footnotes yet)".into()),
        _ => Ok(()),
    }
}

/// The destination anchor from params, creating a paragraph anchor when needed.
fn destination(d: &mut designcraft_doc::Document, p: &Value) -> Result<u64> {
    if let Some(a) = p.get("anchor").and_then(Value::as_u64) {
        return d.find_anchor(a).map(|_| a).ok_or_else(|| bad("xref.insert", format!("no anchor {a}")));
    }
    let (sid, pos) = if let Some(text) = str_param(p, "paragraph") {
        let hit = d.stories.values().find_map(|st| st.text.find(text).map(|i| (st.id, i)));
        hit.ok_or_else(|| bad("xref.insert", format!("no paragraph contains `{text}`")))?
    } else if let (Some(sid), Some(pi)) = (p.get("story").and_then(Value::as_u64), p.get("para").and_then(Value::as_u64)) {
        let st = d.story(StoryId(sid)).ok_or_else(|| bad("xref.insert", "no such story"))?;
        let r = st.para_ranges().get(pi as usize).cloned().ok_or_else(|| bad("xref.insert", "no such paragraph"))?;
        (StoryId(sid), r.start)
    } else {
        return Err(bad("xref.insert", "give `anchor`, `paragraph` or `story` + `para`"));
    };
    d.paragraph_anchor(sid, pos).ok_or_else(|| bad("xref.insert", "could not anchor the paragraph"))
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let p = p.clone();
    s.edit(|d, sel| {
        let format = str_param(&p, "format").unwrap_or("Full Paragraph & Page Number").to_string();
        if d.xref_format(&format).is_none() {
            return Err(bad("xref.insert", format!("no cross-reference format `{format}`")));
        }
        let t = sel.text.ok_or_else(|| bad("xref.insert", "no insertion point"))?;
        // A new paragraph anchor before the caret in the same story shifts the caret.
        let len0 = d.story(t.story).map_or(0, |st| st.len());
        let target = destination(d, &p)?;
        let mut t2 = t;
        if let Some((asid, apos)) = d.find_anchor(target)
            && asid == t.story
            && apos <= t.range().start
        {
            let grew = d.story(t.story).map_or(0, |st| st.len()) - len0;
            t2.anchor += grew;
            t2.focus += grew;
        }
        let st = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let r = t2.range();
        let r = r.start.min(st.len())..r.end.min(st.len());
        st.delete(r.clone());
        st.insert_xref(r.start, CrossRef { target, format });
        let pos = r.start + designcraft_doc::XREF_MARK.len_utf8();
        sel.text = Some(TextSel { anchor: pos, focus: pos, ..t2 });
        Ok(json!({"target": target, "pos": r.start}))
    })
}

fn create_anchor(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("anchor.create", "missing name"))?.to_string();
    s.edit(|d, sel| {
        let t = sel.text.ok_or_else(|| bad("anchor.create", "no insertion point"))?;
        let st = d.story_mut(t.story).ok_or(designcraft_doc::DocError::NoStory(t.story))?;
        let at = t.range().start.min(st.len());
        let id = st.insert_anchor(at, &name);
        let pos = t.focus + designcraft_doc::ANCHOR_MARK.len_utf8();
        sel.text = Some(TextSel { anchor: pos, focus: pos, ..t });
        Ok(json!({"id": id}))
    })
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let d = s.doc()?.doc.clone();
    let index = s.cache.xref_index(&d);
    let mut out = Vec::new();
    for st in d.stories.values() {
        for ((pos, _), x) in st.text.match_indices(designcraft_doc::XREF_MARK).zip(&st.xrefs) {
            let text = designcraft_compose::xref::xref_text(&d, x, Some(&index));
            out.push(json!({"story": st.id.0, "pos": pos, "target": x.target, "format": x.format, "text": text}));
        }
    }
    Ok(Value::Array(out))
}

fn list_anchors(s: &mut Session, _: &Value) -> Result<Value> {
    let d = s.doc()?.doc.clone();
    let index = s.cache.xref_index(&d);
    let mut out = Vec::new();
    for st in d.stories.values() {
        for a in &st.anchors {
            out.push(json!({"id": a.id, "name": a.name, "story": st.id.0, "pos": st.anchor_pos(a.id), "page": index.page(a.id)}));
        }
    }
    Ok(Value::Array(out))
}

fn define_format(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("xref.defineFormat", "missing name"))?.to_string();
    let definition = str_param(p, "definition").ok_or_else(|| bad("xref.defineFormat", "missing definition"))?.to_string();
    s.edit(|d, _| {
        match d.xref_formats.iter_mut().find(|f| f.name == name) {
            Some(f) => f.definition = definition.clone(),
            None => d.xref_formats.push(XrefFormat { name: name.clone(), definition: definition.clone() }),
        }
        Ok(Value::Null)
    })
}

fn set_format(s: &mut Session, p: &Value) -> Result<Value> {
    let sid = p.get("story").and_then(Value::as_u64).map(StoryId).ok_or_else(|| bad("xref.setFormat", "missing story"))?;
    let k = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("xref.setFormat", "missing index"))? as usize;
    let format = str_param(p, "format").ok_or_else(|| bad("xref.setFormat", "missing format"))?.to_string();
    s.edit(|d, _| {
        if d.xref_format(&format).is_none() {
            return Err(bad("xref.setFormat", format!("no cross-reference format `{format}`")));
        }
        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let x = st.xrefs.get_mut(k).ok_or_else(|| bad("xref.setFormat", "no such cross-reference"))?;
        std::sync::Arc::make_mut(x).format = format;
        st.rev = st.rev.wrapping_add(1);
        Ok(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_reference_follows_its_destination() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        // A heading on page 3 and a referring frame on page 1.
        let sp = s.doc().unwrap().doc.page_loc(2).unwrap().0;
        let r = s
            .execute("frame.create", &json!({"spread": sp, "rect": [72, 72, 400, 200], "content": "text", "text": "Results: what we found"}))
            .unwrap();
        let head = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "See ."})).unwrap();
        let src = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("text.select", &json!({"story": src.0, "anchor": 4, "focus": 4})).unwrap();
        s.execute("xref.insert", &json!({"paragraph": "Results"})).unwrap();
        let l = s.execute("xref.list", &json!({})).unwrap();
        assert_eq!(l[0]["text"], "\u{201C}Results: what we found\u{201D} on page 3");
        // Edit the heading: the reference follows without an update step.
        // The paragraph now starts with its (zero-width) anchor mark.
        let at = designcraft_doc::ANCHOR_MARK.len_utf8();
        s.execute("text.select", &json!({"story": head.0, "anchor": at, "focus": at})).unwrap();
        s.execute("text.insert", &json!({"text": "Key "})).unwrap();
        let l = s.execute("xref.list", &json!({})).unwrap();
        assert_eq!(l[0]["text"], "\u{201C}Key Results: what we found\u{201D} on page 3");
        // Formats.
        s.execute("xref.setFormat", &json!({"story": src.0, "index": 0, "format": "Partial Paragraph"})).unwrap();
        assert_eq!(s.execute("xref.list", &json!({})).unwrap()[0]["text"], "\u{201C}Key Results\u{201D}");
        s.execute("xref.defineFormat", &json!({"name": "Short", "definition": "(p. <pageNum />)"})).unwrap();
        s.execute("xref.setFormat", &json!({"story": src.0, "index": 0, "format": "Short"})).unwrap();
        assert_eq!(s.execute("xref.list", &json!({})).unwrap()[0]["text"], "(p. 3)");
        // The generated text is composed into the source frame.
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, src, None);
        let glyphs: usize = cs.frames[0].lines.iter().map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum();
        assert!(glyphs >= "See (p. 3).".replace(' ', "").len(), "{glyphs}");
        // Deleting the destination paragraph's anchor leaves an unresolved reference.
        let a = s.execute("anchor.list", &json!({})).unwrap();
        assert_eq!(a[0]["page"], "3");
        s.execute("text.select", &json!({"story": head.0, "anchor": 0, "focus": 3})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        assert_eq!(s.execute("xref.list", &json!({})).unwrap()[0]["text"], "??");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("xref.list", &json!({})).unwrap()[0]["text"], "(p. 3)");
    }

    #[test]
    fn cross_reference_into_a_story_with_a_running_header_composes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "facingPages": false})).unwrap();
        let head = s.execute("frame.create", &json!({"rect": [72, 72, 400, 120], "content": "text", "text": "Chapter"})).unwrap()["story"].clone();
        let body = s.execute("frame.create", &json!({"rect": [72, 200, 400, 300], "content": "text", "text": "See"})).unwrap()["story"].clone();
        s.execute("text.select", &json!({"story": head, "anchor": 0, "focus": 0})).unwrap();
        s.execute("variables.insert", &json!({"name": "Running Header"})).unwrap();
        s.execute("text.select", &json!({"story": body, "anchor": 3, "focus": 3})).unwrap();
        s.execute("xref.insert", &json!({"story": head, "para": 0})).unwrap();
        // The cross-reference index and the running-header index used to build each other without end.
        assert_eq!(s.execute("xref.list", &json!({})).unwrap()[0]["text"], "\u{201C}Chapter\u{201D} on page 1");
        // The header shows the body's paragraph in front of its own text.
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, StoryId(head.as_u64().unwrap()), None);
        let glyphs: usize = cs.frames[0].lines.iter().map(|l| l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0).count()).sum();
        assert_eq!(glyphs, "SeeChapter".len());
    }
}
