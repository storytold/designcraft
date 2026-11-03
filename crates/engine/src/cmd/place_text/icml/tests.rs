//! Placing InCopy stories (ICML) and exporting a story as one.

use designcraft_doc::{Content, Document, ItemId, ParaFormat, StoryId};
use serde_json::{Value, json};

use crate::Session;
use crate::cmd::file::base64_encode;

/// A small PNG.
fn png() -> Vec<u8> {
    designcraft_render::Rendered { width: 4, height: 2, pixels: [10, 120, 200, 255].repeat(8) }.to_png()
}

/// A session whose one story holds what an InCopy file carries: a paragraph style with a next
/// style, a character style coloured with a swatch, a footnote, a table, a text anchor with a
/// cross-reference to it and an anchored image. Returns it and the story.
fn source() -> (Session, StoryId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("swatch.create", &json!({"name": "Signal", "color": [200, 30, 20]})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Lead", "chars": {"size": 18}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Text"})).unwrap();
    s.execute("style.character.create", &json!({"name": "Mark", "chars": {"fill": "Signal"}})).unwrap();
    let text = "Opening\nA marked word with a note.\nSee it.";
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 600], "content": "text", "text": text})).unwrap();
    let sid = StoryId(r["story"].as_u64().unwrap());
    let image = s.execute("file.place", &json!({"base64": base64_encode(&png()), "name": "dot.png", "x": 450, "y": 450})).unwrap();
    s.execute("anchored.insert", &json!({"ids": [image["id"]], "story": sid.0, "pos": text.len()})).unwrap();
    s.edit(|d, _| {
        if let Some(lead) = d.styles_mut().para_mut("Lead") {
            lead.next_style = Some("Text".into());
        }
        let story = d.story_mut(sid).unwrap();
        story.format_paras(0..1, |p| p.style = "Lead".into());
        story.format_paras(8..9, |p| p.style = "Text".into());
        let marked = story.text.find("marked").unwrap();
        story.format_chars(marked..marked + 6, |f| f.style = "Mark".into());
        let note = story.text.find(" note").unwrap() + 5;
        story.insert_note(note, "Source.", ParaFormat::default());
        let target = story.insert_anchor(0, "Opening");
        let it = story.text.find(" it").unwrap();
        story.insert_xref(it, designcraft_doc::CrossRef { target, format: "Paragraph Text".into(), ..Default::default() });
        let mut table = designcraft_doc::Table::new(d.alloc(), 1, 2, 0, 0, 200.0);
        table.cell_mut(0, 1).unwrap().text.insert(0, "Right");
        let story = d.story_mut(sid).unwrap();
        let second = story.text.find('\n').unwrap() + 1;
        story.insert_table(second, table);
        Ok(())
    })
    .unwrap();
    (s, sid)
}

/// Story `sid` of the session's document as an ICML file.
fn icml_of(s: &mut Session, sid: StoryId) -> String {
    let r = s.execute("file.exportText", &json!({"story": sid.0, "format": "icml"})).unwrap();
    r["text"].as_str().unwrap().to_string()
}

/// `file.place` of `icml` (as `chapter.icml`) with `extra` parameters.
fn place(s: &mut Session, icml: &str, extra: Value) -> crate::Result<Value> {
    let mut p = json!({"base64": base64_encode(icml.as_bytes()), "name": "chapter.icml"});
    if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    s.execute("file.place", &p)
}

fn doc(s: &Session) -> &Document {
    &s.doc().unwrap().doc
}

/// Every id the document's items have, anchored ones included.
fn item_ids(d: &Document) -> Vec<ItemId> {
    let mut ids: Vec<ItemId> = d.anchored_items().into_iter().map(|(_, i)| i.id).collect();
    for sp in &d.spreads {
        for it in &sp.items {
            it.walk(&mut |i| ids.push(i.id));
        }
    }
    ids
}

#[test]
fn an_exported_story_places_with_what_it_uses() {
    let (mut src, sid) = source();
    let icml = icml_of(&mut src, sid);
    assert!(designcraft_idml::is_icml(icml.as_bytes()), "{icml}");
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = place(&mut s, &icml, json!({"rect": [36, 36, 500, 700]})).unwrap();
    let placed = StoryId(r["story"].as_u64().unwrap());
    let (orig, d) = (src.doc().unwrap().doc.story(sid).unwrap().clone(), doc(&s));
    d.check().unwrap();
    let story = d.story(placed).unwrap();
    assert_eq!(story.text, orig.text);
    let styles: Vec<&str> = story.paras.iter().map(|p| p.style.as_str()).collect();
    assert_eq!(styles, orig.paras.iter().map(|p| p.style.as_str()).collect::<Vec<_>>());
    assert!(styles.contains(&"Lead") && styles.contains(&"Text"), "{styles:?}");
    // The styles, their next style and the swatch they use came along.
    assert_eq!(d.styles.para("Lead").and_then(|p| p.chars.size), Some(18.0));
    assert_eq!(d.styles.para("Lead").and_then(|p| p.next_style.as_deref()), Some("Text"));
    assert_eq!(d.styles.char_style("Mark").and_then(|c| c.chars.fill.as_deref()), Some("Signal"));
    assert_eq!(d.swatch("Signal").map(|w| &w.value), src.doc().unwrap().doc.swatch("Signal").map(|w| &w.value));
    let marked = story.text.find("marked").unwrap();
    let mark: Vec<_> = story.runs().filter(|(_, f)| f.style == "Mark").map(|(r, _)| r).collect();
    assert_eq!(mark.len(), 1);
    assert_eq!(mark[0], marked..marked + 6);
    assert_eq!(story.notes.len(), 1);
    assert_eq!(story.notes[0].text.text, "Source.");
    assert_eq!(story.tables.values().next().and_then(|t| t.cell(0, 1)).map(|c| c.text.text.as_str()), Some("Right"));
    // The cross-reference points at the anchor placed with it, which is the document's.
    assert_eq!(story.anchors.len(), 1);
    assert_eq!(story.xrefs.len(), 1);
    assert_eq!(story.xrefs[0].target, story.anchors[0].id);
    assert!(d.find_anchor(story.anchors[0].id).is_some());
    // The anchored image: its file came along, under an id of this document.
    assert_eq!(story.objects.len(), 1);
    let Content::Graphic(g) = &story.objects[0].item.content else { panic!("an image: {:?}", story.objects[0].item.content) };
    assert_eq!(d.assets.get(&g.asset).map(|a| a.data.as_slice()), Some(png().as_slice()));
    let ids = item_ids(d);
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");
    assert!(d.next_id >= ids.iter().map(|i| i.0).max().unwrap());
}

#[test]
fn placing_a_story_undoes_in_one_step() {
    let (mut src, sid) = source();
    let icml = icml_of(&mut src, sid);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = serde_json::to_value(doc(&s)).unwrap();
    place(&mut s, &icml, json!({})).unwrap();
    assert!(doc(&s).swatch("Signal").is_some() && doc(&s).styles.para("Lead").is_some());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(serde_json::to_value(doc(&s)).unwrap(), before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(doc(&s).swatch("Signal").is_some() && doc(&s).styles.para("Lead").is_some());
}

#[test]
fn at_an_insertion_point_cross_references_keep_their_own_anchors() {
    let (mut src, sid) = source();
    let icml = icml_of(&mut src, sid);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 500, 700], "content": "text", "text": "Before after"})).unwrap();
    let host = StoryId(r["story"].as_u64().unwrap());
    // An anchor of the host story before the insertion point.
    s.edit(|d, _| {
        d.story_mut(host).unwrap().insert_anchor(0, "Host");
        Ok(())
    })
    .unwrap();
    let at = doc(&s).story(host).unwrap().text.find(" after").unwrap() + 1;
    s.execute("text.select", &json!({"story": host.0, "anchor": at, "focus": at})).unwrap();
    place(&mut s, &icml, json!({})).unwrap();
    let story = doc(&s).story(host).unwrap();
    assert!(story.text.starts_with(&format!("{a}Before {a}Opening", a = designcraft_doc::ANCHOR_MARK)), "{:?}", story.text);
    assert!(story.text.ends_with("after"), "{:?}", story.text);
    assert_eq!(story.anchors.len(), 2);
    assert_eq!(story.xrefs.len(), 1);
    assert_eq!(story.xrefs[0].target, story.anchors[1].id, "the placed anchor, not the host's");
    assert_ne!(story.anchors[0].id, story.anchors[1].id);
    doc(&s).check().unwrap();
}

#[test]
fn existing_styles_win_unless_redefined() {
    let (mut src, sid) = source();
    let icml = icml_of(&mut src, sid);
    for (conflicts, size, next) in [("useExisting", 30.0, None), ("redefine", 18.0, Some("Text"))] {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Lead", "chars": {"size": 30}})).unwrap();
        place(&mut s, &icml, json!({"styleConflicts": conflicts})).unwrap();
        let lead = doc(&s).styles.para("Lead").unwrap();
        assert_eq!(lead.chars.size, Some(size), "{conflicts}");
        assert_eq!(lead.next_style.as_deref(), next, "{conflicts}");
        // Styles the document lacked come in either way.
        assert!(doc(&s).styles.para("Text").is_some() && doc(&s).styles.char_style("Mark").is_some(), "{conflicts}");
    }
}

#[test]
fn place_styles_lists_what_an_icml_file_brings() {
    let (mut src, sid) = source();
    let icml = icml_of(&mut src, sid);
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Text"})).unwrap();
    let r = s.execute("place.styles", &json!({"base64": base64_encode(icml.as_bytes()), "name": "chapter.icml"})).unwrap();
    let names = |k: &str| r[k].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(names("paragraph").contains(&"Lead".to_string()) && names("paragraph").contains(&"Text".to_string()), "{r}");
    assert!(names("character").contains(&"Mark".to_string()), "{r}");
    assert_eq!(names("conflicts"), ["Text"]);
}

/// An InCopy file whose story anchors a text frame holding a story of its own.
const ANCHORED_FRAME: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<?aid style="50" type="snippet" readerVersion="6.0" featureSet="513" product="16.0(1)" ?>
<?aid SnippetType="InCopyInterchange"?>
<Document DOMVersion="16.0" Self="d">
<Story Self="inner"><ParagraphStyleRange><CharacterStyleRange><Content>Boxed</Content></CharacterStyleRange></ParagraphStyleRange></Story>
<Story Self="body"><ParagraphStyleRange><CharacterStyleRange><Content>Body </Content>
<TextFrame Self="tf" ParentStory="inner" ItemTransform="1 0 0 1 0 0"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
<PathPointType Anchor="0 0"/><PathPointType Anchor="0 24"/><PathPointType Anchor="80 24"/><PathPointType Anchor="80 0"/>
</PathPointArray></GeometryPathType></PathGeometry></Properties></TextFrame>
<Content> text.</Content></CharacterStyleRange></ParagraphStyleRange></Story>
</Document>"#;

#[test]
fn anchored_text_frames_get_stories_of_their_own() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let stories: Vec<StoryId> = (0..2)
        .map(|k| {
            let frame = s.execute("frame.create", &json!({"rect": [36, 36 + 200 * k, 400, 200 + 200 * k], "content": "text"})).unwrap();
            let r = place(&mut s, ANCHORED_FRAME, json!({"frame": frame["id"]})).unwrap();
            StoryId(r["story"].as_u64().unwrap())
        })
        .collect();
    assert_ne!(stories[0], stories[1]);
    let d = doc(&s);
    d.check().unwrap();
    let mut inner = Vec::new();
    for sid in &stories {
        let story = d.story(*sid).unwrap();
        assert_eq!(story.text, format!("Body {} text.", designcraft_doc::OBJECT_MARK));
        let frame = &story.objects[0].item;
        let tf = frame.text_frame().unwrap();
        let boxed = d.story(tf.story).unwrap();
        assert_eq!(boxed.text, "Boxed");
        assert_eq!(boxed.frames, [frame.id]);
        inner.push((frame.id, tf.story));
    }
    // Each placement is a copy: no id is shared.
    assert_ne!(inner[0].0, inner[1].0);
    assert_ne!(inner[0].1, inner[1].1);
    assert!(!stories.contains(&inner[0].1) && !stories.contains(&inner[1].1));
}

#[test]
fn updating_a_story_drops_the_anchored_frames_it_replaced() {
    let dir = std::env::temp_dir().join(format!("dc-update-icml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("boxed.icml");
    std::fs::write(&path, ANCHORED_FRAME).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true})).unwrap();
    let sid = StoryId(r["story"].as_u64().unwrap());
    let count = doc(&s).stories.len();
    std::fs::write(&path, ANCHORED_FRAME.replace("Boxed", "Boxed again")).unwrap();
    assert_eq!(s.execute("links.update", &json!({"story": sid.0})).unwrap()["updated"], 1);
    let d = doc(&s);
    d.check().unwrap();
    assert_eq!(d.stories.len(), count, "the old frame's story went with it");
    let tf = d.story(sid).unwrap().objects[0].item.text_frame().unwrap().story;
    assert_eq!(d.story(tf).unwrap().text, "Boxed again");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn linked_graphics_are_read_beside_the_file_and_updates_follow_it() {
    let dir = std::env::temp_dir().join(format!("dc-place-icml-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("art")).unwrap();
    std::fs::write(dir.join("art").join("pic.png"), png()).unwrap();
    let story = |words: &str| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><?aid SnippetType="InCopyInterchange"?><Document DOMVersion="16.0" Self="d"><Story Self="s"><ParagraphStyleRange><CharacterStyleRange><Content>{words} </Content>
<Rectangle Self="r" ItemTransform="1 0 0 1 0 0"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
<PathPointType Anchor="0 0"/><PathPointType Anchor="0 20"/><PathPointType Anchor="40 20"/><PathPointType Anchor="40 0"/>
</PathPointArray></GeometryPathType></PathGeometry></Properties><Image Self="i" ItemTransform="10 0 0 10 0 0"><Link Self="l" LinkResourceURI="file:art/pic.png"/></Image></Rectangle>
</CharacterStyleRange></ParagraphStyleRange></Story></Document>"#
        )
    };
    let path = dir.join("chapter.icml");
    std::fs::write(&path, story("First draft")).unwrap();
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true})).unwrap();
    let sid = StoryId(r["story"].as_u64().unwrap());
    let st = doc(&s).story(sid).unwrap();
    assert!(st.text.starts_with("First draft "), "{:?}", st.text);
    let Content::Graphic(g) = &st.objects[0].item.content else { panic!("an image") };
    let asset = doc(&s).assets.get(&g.asset).unwrap();
    assert_eq!(asset.data.as_slice(), png().as_slice());
    // The link, relative in the story, names the file it was read from.
    let link = std::path::Path::new(asset.link.as_deref().unwrap());
    assert!(link.is_absolute() && link.ends_with("art/pic.png"), "{link:?}");
    assert!(doc(&s).story(sid).unwrap().file_link.is_some());
    // The InCopy file changes: Update Link reads it again into the same story.
    std::fs::write(&path, story("Second draft, longer")).unwrap();
    let r = s.execute("links.update", &json!({"story": sid.0})).unwrap();
    assert_eq!(r["updated"], 1, "{r}");
    let st = doc(&s).story(sid).unwrap();
    assert!(st.text.starts_with("Second draft, longer "), "{:?}", st.text);
    assert_eq!(st.objects.len(), 1);
    doc(&s).check().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_is_not_icml_is_refused_and_changes_nothing() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let before = serde_json::to_value(doc(&s)).unwrap();
    for bad in ["plain words", "<Document><Color Self=\"c\"/></Document>", "<Document><Story"] {
        assert!(place(&mut s, bad, json!({})).is_err(), "{bad}");
    }
    assert_eq!(serde_json::to_value(doc(&s)).unwrap(), before);
}

#[test]
fn export_text_writes_icml_for_an_icml_path() {
    let (mut src, sid) = source();
    let dir = std::env::temp_dir().join(format!("dc-export-icml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.icml");
    src.execute("file.exportText", &json!({"story": sid.0, "path": path.to_string_lossy()})).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(designcraft_idml::is_icml(&bytes));
    assert_eq!(String::from_utf8(bytes).unwrap(), icml_of(&mut src, sid));
    let err = src.execute("file.exportText", &json!({"story": sid.0, "format": "incopy"})).unwrap_err();
    assert!(err.to_string().contains("icml"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A style the file bases on nothing doesn't inherit the document's [Basic Paragraph]: it stays
/// based on [No Paragraph Style], as it was in the file.
#[test]
fn a_style_based_on_nothing_stays_so() {
    let icml = r#"<?xml version="1.0" encoding="UTF-8"?><?aid SnippetType="InCopyInterchange"?><Document DOMVersion="16.0" Self="d">
<RootParagraphStyleGroup Self="g"><ParagraphStyle Self="ParagraphStyle/Loose" Name="Loose" LeftIndent="6"/></RootParagraphStyleGroup>
<Story Self="s"><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Loose"><CharacterStyleRange><Content>Free</Content></CharacterStyleRange></ParagraphStyleRange></Story></Document>"#;
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.edit(|d, _| {
        if let Some(basic) = d.styles_mut().para_mut(designcraft_doc::story::BASIC_PARAGRAPH) {
            basic.chars.size = Some(20.0);
        }
        Ok(())
    })
    .unwrap();
    place(&mut s, icml, json!({})).unwrap();
    let st = &doc(&s).styles;
    assert_eq!(st.para("Loose").and_then(|p| p.based_on.as_deref()), Some(designcraft_doc::NO_PARA_STYLE));
    let (para, chars) = st.resolve_para_style("Loose");
    assert_eq!(para.left_indent, 6.0);
    assert_eq!(chars.size, 12.0, "not the document's [Basic Paragraph] size");
}
