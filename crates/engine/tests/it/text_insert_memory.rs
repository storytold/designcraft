//! Text-command fidelity across transformed strings and editor targets.

use designcraft_doc::{ChangeMark, Document, StoryId};
use designcraft_engine::Session;
use designcraft_render::Renderer;
use serde_json::{Value, json};

fn fixture(text: &str) -> (Session, StoryId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect":[36,36,500,700],"content":"text","text":text})).unwrap();
    let sid = StoryId(r["story"].as_u64().unwrap());
    s.execute("text.select", &json!({"story":sid.0,"anchor":text.len(),"focus":text.len()})).unwrap();
    (s, sid)
}

fn fresh_pixels(s: &Session, r: &mut Renderer) -> Vec<u8> {
    let warm = r.render_page(&s.doc().unwrap().doc, &s.cache, 0, 1.0, false, &Default::default()).unwrap().pixels;
    let fresh =
        Renderer::new().render_page(&s.doc().unwrap().doc, &designcraft_compose::Cache::new(), 0, 1.0, false, &Default::default()).unwrap().pixels;
    assert_eq!(warm, fresh);
    warm
}

fn check_edit(s: &mut Session, params: Value, check: impl Fn(&Document)) {
    let before = s.doc().unwrap().doc.clone();
    let undo_len = s.doc().unwrap().history.undo.len();
    let mut renderer = Renderer::new();
    let before_pixels = fresh_pixels(s, &mut renderer);
    s.execute("text.insert", &params).unwrap();
    s.doc().unwrap().doc.check().unwrap();
    check(&s.doc().unwrap().doc);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
    let edited = s.doc().unwrap().doc.clone();
    let edited_pixels = fresh_pixels(s, &mut renderer);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    assert_eq!(fresh_pixels(s, &mut renderer), before_pixels);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, edited);
    assert_eq!(fresh_pixels(s, &mut renderer), edited_pixels);
}

#[test]
fn unicode_replacement_keeps_expected_text_and_character_format() {
    let (mut s, sid) = fixture("Aé🦀e\u{301}Z");
    s.execute("text.select", &json!({"story":sid.0,"anchor":1,"focus":7})).unwrap();
    s.execute("type.char", &json!({"fontStyle":"Bold"})).unwrap();
    check_edit(&mut s, json!({"text":"中","raw":true}), |d| {
        let st = d.story(sid).unwrap();
        assert_eq!(st.text, "A中e\u{301}Z");
        assert_eq!(st.format_after(1).over.font_style.as_deref(), Some("Bold"));
        assert!(st.chars.iter().map(|r| r.len).sum::<usize>() == st.text.len());
    });
}

#[test]
fn smart_quotes_and_expanding_autocorrect_use_final_text() {
    let (mut s, sid) = fixture("");
    check_edit(&mut s, json!({"text":"\"a\""}), |d| assert_eq!(d.story(sid).unwrap().text, "“a”"));
    let (mut s, sid) = fixture("omw");
    s.prefs.autocorrect = true;
    s.prefs.autocorrect_list = vec![("omw".into(), "on my way".into())];
    check_edit(&mut s, json!({"text":" "}), |d| assert_eq!(d.story(sid).unwrap().text, "on my way "));
}

#[test]
fn typing_on_each_side_of_table_anchor_preserves_automatic_newlines() {
    for before in [true, false] {
        let (mut s, sid) = fixture("");
        s.execute("table.insert", &json!({"rows":1,"cols":1})).unwrap();
        let anchor = designcraft_doc::TABLE_ANCHOR;
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, format!("{anchor}\n"));
        let at = if before { 0 } else { anchor.len_utf8() };
        s.execute("text.select", &json!({"story":sid.0,"anchor":at,"focus":at})).unwrap();
        check_edit(&mut s, json!({"text":"Heading","raw":true}), |d| {
            let expected = if before { format!("Heading\n{anchor}\n") } else { format!("{anchor}\nHeading\n") };
            assert_eq!(d.story(sid).unwrap().text, expected);
            assert_eq!(d.story(sid).unwrap().tables.len(), 1);
        });
    }
}

#[test]
fn tracked_replacement_retains_old_text_and_marks_new_text() {
    let (mut s, sid) = fixture("Keep this.");
    s.execute("changes.track", &json!({"on":true})).unwrap();
    s.execute("text.select", &json!({"story":sid.0,"anchor":5,"focus":9})).unwrap();
    check_edit(&mut s, json!({"text":"that","raw":true}), |d| {
        let st = d.story(sid).unwrap();
        assert_eq!(st.text, "Keep thisthat.");
        assert_eq!(st.format_after(5).over.change, Some(ChangeMark::Deleted));
        assert_eq!(st.format_after(9).over.change, Some(ChangeMark::Inserted));
    });
}

#[test]
fn replacing_already_inserted_text_does_not_retain_a_deleted_copy() {
    let (mut s, sid) = fixture("");
    s.execute("changes.track", &json!({"on":true})).unwrap();
    s.execute("text.insert", &json!({"text":"a".repeat(100_000),"raw":true})).unwrap();
    s.execute("text.select", &json!({"story":sid.0,"anchor":0,"focus":100_000})).unwrap();
    let before_capacity = s.doc().unwrap().doc.story(sid).unwrap().text.capacity();
    check_edit(&mut s, json!({"text":"b".repeat(100_000),"raw":true}), |d| {
        let st = d.story(sid).unwrap();
        assert_eq!(st.text, "b".repeat(100_000));
        assert_eq!(st.format_after(0).over.change, Some(ChangeMark::Inserted));
        assert!(st.chars.iter().all(|r| r.format.over.change != Some(ChangeMark::Deleted)));
        assert_eq!(st.text.capacity(), before_capacity, "replacing all Inserted text must reuse the cloned buffer after mark_deleted");
        println!("text_capacity_control {}", json!({"before":before_capacity,"after":st.text.capacity(),"len":st.text.len()}));
    });
}

#[test]
fn cell_and_footnote_insertions_preserve_separate_text_targets() {
    let (mut s, sid) = fixture("Body");
    let table = s.execute("table.insert", &json!({"rows":1,"cols":1})).unwrap()["table"].as_u64().unwrap();
    let root = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
    check_edit(&mut s, json!({"text":"é👩‍💻","raw":true}), |d| {
        let st = d.story(sid).unwrap();
        assert_eq!(st.text, root);
        assert_eq!(st.tables[&table].cells[0].text.text, "é👩‍💻");
    });
    let (mut s, sid) = fixture("Body");
    s.execute("footnote.insert", &json!({"text":"Note"})).unwrap();
    let root = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
    check_edit(&mut s, json!({"text":"é","raw":true}), |d| {
        let st = d.story(sid).unwrap();
        assert_eq!(st.text, root);
        assert_eq!(st.notes[0].text.text, "Noteé");
    });
}

#[test]
fn small_insertions_keep_history_text_capacity_at_the_exact_copy_control() {
    for tracking in [false, true] {
        let text = "a".repeat(16_384);
        let (mut s, sid) = fixture(&text);
        s.execute("changes.track", &json!({"on":tracking})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        let mut expected = text;
        for addition in ["é", "🦀", "x"] {
            s.execute("text.insert", &json!({"text":addition,"raw":true})).unwrap();
            expected.push_str(addition);
            let snapshot = &s.doc().unwrap().doc;
            snapshot.check().unwrap();
            let actual = &snapshot.story(sid).unwrap().text;
            assert_eq!(actual, &expected);
            // Compare with an independent final-text copy on this allocator/toolchain. The
            // regression is geometric String growth retained in every undo snapshot, not RSS.
            assert_eq!(actual.capacity(), expected.as_str().to_owned().capacity());
        }
        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(s.doc().unwrap().doc, before);
        for _ in 0..3 {
            s.execute("edit.redo", &json!({})).unwrap();
            let actual = &s.doc().unwrap().doc.story(sid).unwrap().text;
            assert_eq!(actual.capacity(), actual.as_str().to_owned().capacity());
        }
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, expected);
    }
}

#[test]
fn replacement_reserves_only_net_growth_and_shrinking_text_keeps_existing_room() {
    let original = "a".repeat(16_384);
    for inserted in ["é".repeat(4_097), "é".repeat(4_096), "é".repeat(2_000)] {
        let (mut s, sid) = fixture(&original);
        s.execute("text.select", &json!({"story":sid.0,"anchor":4_096,"focus":12_288})).unwrap();
        let before = s.doc().unwrap().doc.clone();
        s.execute("text.insert", &json!({"text":inserted,"raw":true})).unwrap();
        let expected = ["a".repeat(4_096), inserted, "a".repeat(4_096)].concat();
        let actual = &s.doc().unwrap().doc.story(sid).unwrap().text;
        assert_eq!(actual, &expected);
        let capacity_control = original.capacity().max(expected.as_str().to_owned().capacity());
        assert_eq!(actual.capacity(), capacity_control, "reserve only the replacement's positive net byte growth");
        let after = s.doc().unwrap().doc.clone();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, after);
    }
}

#[test]
fn text_insert_relocates_and_removes_current_data_merge_placeholders() {
    use designcraft_doc::PlaceholderAnchor;

    let (mut s, sid) = fixture("");
    s.execute("data.source.select", &json!({"bytes":"TmFtZQpBZGEK","name":"people.csv"})).unwrap();
    let placeholder = s.execute("data.placeholder.add", &json!({"field":"Name"})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "<<Name>>");
    s.execute("text.select", &json!({"story":sid.0,"anchor":0,"focus":0})).unwrap();
    check_edit(&mut s, json!({"text":"é","raw":true}), |d| {
        assert_eq!(d.story(sid).unwrap().text, "é<<Name>>");
        assert_eq!(d.data_merge.placeholders.len(), 1);
        assert_eq!(d.data_merge.placeholders[0].id, placeholder);
        assert_eq!(d.data_merge.placeholders[0].anchor, PlaceholderAnchor::Text { story: sid, start: 2, end: 10 });
    });
    s.execute("data.preview", &json!({"record":1})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "éAda");
    s.execute("data.preview.stop", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "é<<Name>>");
    s.execute("text.select", &json!({"story":sid.0,"anchor":2,"focus":10})).unwrap();
    check_edit(&mut s, json!({"text":"world","raw":true}), |d| {
        assert_eq!(d.story(sid).unwrap().text, "éworld");
        assert!(d.data_merge.placeholders.is_empty());
    });
}
