use designcraft_doc::{ParaFormat, Story, StoryId};
use designcraft_engine::Session;
use serde_json::{Value, json};
use std::sync::Arc;
fn setup() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}
fn frame(s: &mut Session, text: &str) -> (u64, u64) {
    let r = s.execute("frame.create", &json!({"rect":[72,72,300,300],"content":"text","text":text,"caret":false})).unwrap();
    (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap())
}
fn snapshot(s: &Session) -> Value {
    let d = s.doc().unwrap();
    json!({"doc":d.doc,"selection":d.selection,"revision":d.revision,"dirty":d.is_dirty(),"undo":d.history.undo.len(),"redo":d.history.redo.len(),"journal":s.journal})
}
#[test]
fn rejected_thread_is_atomic_with_new_frame_and_redo() {
    let mut s = setup();
    let (a, _) = frame(&mut s, "hello");
    frame(&mut s, "undo");
    s.execute("edit.undo", &json!({})).unwrap();
    let before = snapshot(&s);
    assert!(s.execute("text.thread", &json!({"from":999999,"rect":[0,0,100,100]})).is_err());
    assert_eq!(snapshot(&s), before);
    assert!(s.execute("text.thread", &json!({"from":a,"rect":[0,0,100,100],"spread":999})).is_err());
    assert_eq!(snapshot(&s), before);
}
#[test]
fn locked_frame_cannot_be_unthreaded() {
    let mut s = setup();
    let (a, _) = frame(&mut s, "hello");
    let (b, _) = frame(&mut s, "");
    s.execute("text.thread", &json!({"from":a,"to":b})).unwrap();
    s.execute("object.lock", &json!({"ids":[a]})).unwrap();
    let before = snapshot(&s);
    let r = s.execute("text.unthread", &json!({"frame":a}));
    assert!(r.is_err(), "unthread accepted locked frame: {r:?}");
    assert_eq!(snapshot(&s), before);
}
#[test]
fn locked_frame_cannot_be_removed_from_thread() {
    let mut s = setup();
    let (a, _) = frame(&mut s, "hello");
    let (b, _) = frame(&mut s, "");
    s.execute("text.thread", &json!({"from":a,"to":b})).unwrap();
    s.execute("object.lock", &json!({"ids":[a]})).unwrap();
    let before = snapshot(&s);
    let r = s.execute("text.removeFromThread", &json!({"frame":a}));
    assert!(r.is_err(), "remove accepted locked frame: {r:?}");
    assert_eq!(snapshot(&s), before);
}
#[test]
fn rejected_selection_preserves_pending_typing() {
    let mut s = setup();
    let (_, sid) = frame(&mut s, "hello");
    s.execute("text.select", &json!({"story":sid,"anchor":3})).unwrap();
    s.execute("type.char", &json!({"attrs":{"size":30}})).unwrap();
    let before = s.execute("type.selectionAttrs", &json!({})).unwrap();
    assert!(s.execute("text.select", &json!({"story":999999,"anchor":0})).is_err());
    let after = s.execute("type.selectionAttrs", &json!({})).unwrap();
    assert_eq!(after, before, "rejected selection reset pending typing");
}
#[test]
fn append_max_id_preserves_both_tables() {
    use designcraft_doc::table::Table;
    let mut a = Story::with_text(StoryId(1), &designcraft_doc::TABLE_ANCHOR.to_string(), ParaFormat::default());
    let mut b = Story::with_text(StoryId(2), &designcraft_doc::TABLE_ANCHOR.to_string(), ParaFormat::default());
    let mut ta = Table::new(u64::MAX, 1, 1, 0, 0, 100.);
    ta.cells[0].text = Story::with_text(StoryId(0), "ORIGINAL", ParaFormat::default());
    a.tables.insert(u64::MAX, Arc::new(ta));
    let mut tb = Table::new(1, 1, 1, 0, 0, 100.);
    tb.cells[0].text = Story::with_text(StoryId(0), "INCOMING", ParaFormat::default());
    b.tables.insert(1, Arc::new(tb));
    a.paras[0].table = Some(u64::MAX);
    b.paras[0].table = Some(1);
    a.check().unwrap();
    b.check().unwrap();
    a.append_story(b);
    let contents: Vec<_> = a.tables.values().map(|t| t.cells[0].text.text.clone()).collect();
    assert!(contents.contains(&"ORIGINAL".into()) && contents.contains(&"INCOMING".into()), "table loss: {contents:?}");
}
#[test]
fn idml_dropcap_detail_preserves_inherited_other_bit() {
    use designcraft_doc::{Document, ParaAttrs, ParagraphStyle, build::NewDocument};
    let mut d = Document::new(&NewDocument::default());
    let parent = ParagraphStyle {
        name: "Parent".into(),
        based_on: None,
        next_style: None,
        para: ParaAttrs { drop_cap_align_left: Some(false), drop_cap_scale_descenders: Some(true), ..Default::default() },
        chars: Default::default(),
        shortcut: String::new(),
    };
    let child = ParagraphStyle {
        name: "Child".into(),
        based_on: Some("Parent".into()),
        next_style: None,
        para: ParaAttrs { drop_cap_scale_descenders: Some(false), ..Default::default() },
        chars: Default::default(),
        shortcut: String::new(),
    };
    d.styles_mut().paragraph.extend([parent, child]);
    let before = d.styles.resolve_para_style("Child").0;
    let bytes = designcraft_idml::export_idml(&d);
    let back = designcraft_idml::import_idml(&bytes).unwrap();
    let after = back.styles.resolve_para_style("Child").0;
    assert_eq!(
        (after.drop_cap_align_left, after.drop_cap_scale_descenders),
        (before.drop_cap_align_left, before.drop_cap_scale_descenders),
        "IDML export resets inherited align-left bit"
    );
}
#[test]
fn threading_preserves_incoming_text_hyperlink() {
    let mut s = setup();
    let (a, sa) = frame(&mut s, "First");
    let (b, sb) = frame(&mut s, "Second");
    s.execute("text.select", &json!({"story":sb,"anchor":0,"focus":6})).unwrap();
    s.execute("hyperlink.create", &json!({"url":"https://example.com"})).unwrap();
    s.execute("text.thread", &json!({"from":a,"to":b})).unwrap();
    let d = &s.doc().unwrap().doc;
    let back = designcraft_format::load(&designcraft_format::save(d).unwrap()).unwrap();
    match &back.hyperlinks[0].source {
        designcraft_doc::HyperlinkSource::Text { story, start, end } => {
            assert_eq!(story.0, sa, "hyperlink points to deleted incoming story");
            assert_eq!((*start, *end), (6, 12));
        }
        _ => panic!("wrong source"),
    };
}
#[test]
fn thread_cannot_destroy_body_by_joining_generated_endnotes() {
    let mut s = setup();
    let (body, sid) = frame(&mut s, "KEEP THIS BODY");
    s.execute("text.select", &json!({"story":sid,"anchor":14})).unwrap();
    let e = s.execute("endnote.insert", &json!({"text":"a note"})).unwrap();
    let ef = e["frame"].as_u64().unwrap();
    let before = snapshot(&s);
    let r = s.execute("text.thread", &json!({"from":ef,"to":body}));
    assert!(
        r.is_err(),
        "thread into generated endnotes accepted: {r:?}; stories: {:?}",
        s.doc().unwrap().doc.stories.values().map(|s| &s.text).collect::<Vec<_>>()
    );
    assert_eq!(snapshot(&s), before);
}
#[test]
fn generated_endnotes_accept_empty_extensions_but_reject_reverse_body_merge() {
    let mut s = setup();
    let (body, sid) = frame(&mut s, "Body");
    s.execute("text.select", &json!({"story":sid,"anchor":4})).unwrap();
    let r = s.execute("endnote.insert", &json!({"text":"note"})).unwrap();
    let ef = r["frame"].as_u64().unwrap();
    let esid = s.doc().unwrap().doc.endnote_story.unwrap();
    let before = snapshot(&s);
    assert!(s.execute("text.thread", &json!({"from":body,"to":ef})).is_err());
    assert_eq!(snapshot(&s), before);
    s.execute("text.thread", &json!({"from":ef,"rect":[72,72,300,300]})).unwrap();
    assert_eq!(s.doc().unwrap().doc.endnote_story, Some(esid));
    s.execute("text.thread", &json!({"to":ef,"rect":[72,72,300,300]})).unwrap();
    assert_eq!(s.doc().unwrap().doc.endnote_story, Some(esid));
    assert_eq!(s.doc().unwrap().doc.story(esid).unwrap().frames.len(), 3);
    assert!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text.starts_with("Body"));
    s.doc().unwrap().doc.check().unwrap();
}
#[test]
fn threading_moves_data_placeholders_and_primary_story_with_text() {
    use designcraft_doc::{Placeholder, PlaceholderAnchor, PlaceholderRole};
    let mut s = setup();
    let (a, sa) = frame(&mut s, "First");
    let (b, sb) = frame(&mut s, "<<name>>");
    s.edit(|d, _| {
        d.settings.primary_story = Some(StoryId(sb));
        d.data_merge.placeholders.push(Placeholder {
            id: 123,
            source_id: 0,
            field: "name".into(),
            role: PlaceholderRole::Text,
            anchor: PlaceholderAnchor::Text { story: StoryId(sb), start: 0, end: 8 },
        });
        Ok(())
    })
    .unwrap();
    let before = s.doc().unwrap().doc.clone();
    s.execute("text.thread", &json!({"from":a,"to":b})).unwrap();
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.settings.primary_story, Some(StoryId(sa)));
    assert_eq!(d.data_merge.placeholders.len(), 1);
    assert_eq!(d.data_merge.placeholders[0].anchor, PlaceholderAnchor::Text { story: StoryId(sa), start: 6, end: 14 });
    let after = d.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after);
}
#[test]
fn append_max_ids_preserves_addressable_footnotes_and_endnotes() {
    let mut a = Story::with_text(StoryId(1), "A", ParaFormat::default());
    let mut b = Story::with_text(StoryId(2), "B", ParaFormat::default());
    a.insert_note(1, "original footnote", ParaFormat::default());
    b.insert_note(1, "incoming footnote", ParaFormat::default());
    Arc::make_mut(&mut a.notes[0]).id = u64::MAX;
    a.insert_endnote(1, "original endnote", ParaFormat::default());
    b.insert_endnote(1, "incoming endnote", ParaFormat::default());
    Arc::make_mut(&mut a.endnotes[0]).id = u64::MAX;
    a.check().unwrap();
    b.check().unwrap();
    a.append_story(b);
    a.check().unwrap();
    assert_ne!(a.notes[0].id, a.notes[1].id);
    assert_ne!(a.endnotes[0].id, a.endnotes[1].id);
    assert_eq!(a.notes[0].text.text, "original footnote");
    assert_eq!(a.endnotes[1].text.text, "incoming endnote");
}
#[test]
fn locked_layer_and_neighbour_reject_unthread_atomically() {
    for lock_layer in [false, true] {
        let mut s = setup();
        let (a, _) = frame(&mut s, "A");
        let (b, _) = frame(&mut s, "B");
        s.execute("text.thread", &json!({"from":a,"to":b})).unwrap();
        if lock_layer {
            let lid = s.doc().unwrap().active_layer.0;
            s.execute("layer.set", &json!({"id":lid,"locked":true})).unwrap();
        } else {
            s.execute("object.lock", &json!({"ids":[b]})).unwrap();
        }
        let before = snapshot(&s);
        assert!(s.execute("text.unthread", &json!({"frame":a})).is_err());
        assert_eq!(snapshot(&s), before);
    }
}
#[test]
fn idml_partial_dropcap_bits_resolve_both_style_and_paragraph_inheritance() {
    use designcraft_doc::{Document, ParaAttrs, ParagraphStyle, SpreadRef, build::NewDocument};
    use designcraft_geom::Rect;
    for parent_align in [false, true] {
        for parent_scale in [false, true] {
            for local_align in [false, true] {
                let mut d = Document::new(&NewDocument::default());
                d.styles_mut().paragraph.push(ParagraphStyle {
                    name: "Parent".into(),
                    based_on: None,
                    next_style: None,
                    para: ParaAttrs { drop_cap_align_left: Some(parent_align), drop_cap_scale_descenders: Some(parent_scale), ..Default::default() },
                    chars: Default::default(),
                    shortcut: String::new(),
                });
                let layer = d.default_layer();
                let (_, sid) = d
                    .add_text_frame(
                        SpreadRef::Doc(0),
                        Rect::new(10., 10., 200., 200.),
                        layer,
                        "Body",
                        ParaFormat {
                            style: "Parent".into(),
                            para: ParaAttrs { drop_cap_align_left: Some(local_align), ..Default::default() },
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let bytes = designcraft_idml::export_idml(&d);
                let back = designcraft_idml::import_idml(&bytes).unwrap();
                let st = back.stories.values().find(|s| s.text == "Body").unwrap();
                let actual = back.styles.resolve_para(&st.paras[0]).0;
                let expected = d.styles.resolve_para(&d.story(sid).unwrap().paras[0]).0;
                assert_eq!(
                    (actual.drop_cap_align_left, actual.drop_cap_scale_descenders),
                    (expected.drop_cap_align_left, expected.drop_cap_scale_descenders)
                );
            }
        }
    }
}
