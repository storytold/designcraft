//! Character Styles panel's existing `fromSelection` command contract.
use designcraft_doc::{CharAttrs, StoryId};
use designcraft_engine::Session;
use serde_json::{Value, json};

fn story(text: &str) -> (Session, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let frame = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": text})).unwrap();
    (s, frame["story"].as_u64().unwrap())
}

fn select(s: &mut Session, sid: u64, anchor: usize, focus: usize) {
    s.execute("text.select", &json!({"story": sid, "anchor": anchor, "focus": focus})).unwrap();
}

fn chars(s: &Session, name: &str) -> CharAttrs {
    s.doc().unwrap().doc.styles.char_style(name).unwrap().chars.clone()
}

fn document(s: &Session) -> Value {
    serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap()
}

#[test]
fn character_capture_overlays_style_chain_local_and_explicit_attrs_but_inherits_paragraph() {
    let (mut s, sid) = story("alpha\nbeta");
    s.execute("style.paragraph.create", &json!({"name": "Source paragraph", "chars": {"size": 18}})).unwrap();
    s.execute("style.paragraph.create", &json!({"name": "Destination", "chars": {"size": 30, "underline": true}})).unwrap();
    s.execute("style.character.create", &json!({"name": "Base", "chars": {"underline": true, "tracking": 10, "strikethrough": true}})).unwrap();
    s.execute("style.character.create", &json!({"name": "Emphasis", "basedOn": "Base", "chars": {"fontStyle": "Italic"}})).unwrap();
    select(&mut s, sid, 0, 5);
    s.execute("style.paragraph.apply", &json!({"name": "Source paragraph"})).unwrap();
    s.execute("style.character.apply", &json!({"name": "Emphasis"})).unwrap();
    s.execute("type.char", &json!({"tracking": 20, "underline": false})).unwrap();
    s.execute("style.character.create", &json!({"name": "Captured", "fromSelection": true, "chars": {"tracking": 7}})).unwrap();
    assert_eq!(
        chars(&s, "Captured"),
        CharAttrs { font_style: Some("Italic".into()), underline: Some(false), tracking: Some(7.0), strikethrough: Some(true), ..Default::default() }
    );
    select(&mut s, sid, 6, 10);
    s.execute("style.paragraph.apply", &json!({"name": "Destination"})).unwrap();
    s.execute("style.character.apply", &json!({"name": "Captured"})).unwrap();
    let resolved = s.execute("type.selectionAttrs", &json!({})).unwrap();
    assert_eq!(resolved["chars"]["fontStyle"], "Italic");
    assert_eq!(resolved["chars"]["strikethrough"], true);
    assert_eq!(resolved["chars"]["underline"], false);
    assert_eq!(resolved["chars"]["tracking"], 7.0);
    assert_eq!(resolved["chars"]["size"], 30.0);
    s.execute("style.paragraph.edit", &json!({"name": "Destination", "chars": {"size": 34}})).unwrap();
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 34.0);
    // The captured attributes are a snapshot, independent of future source-style edits.
    s.execute("style.character.edit", &json!({"name": "Emphasis", "chars": {"fontStyle": "Bold"}})).unwrap();
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fontStyle"], "Italic");
}

#[test]
fn character_capture_reproducer_is_one_undo_step_and_survives_native_reopen() {
    let (mut s, sid) = story("alpha beta");
    select(&mut s, sid, 0, 5);
    s.execute("type.char", &json!({"underline": true})).unwrap();
    let before = document(&s);
    let selection = s.doc().unwrap().selection.clone();
    let undo_len = s.doc().unwrap().history.undo.len();
    let source = serde_json::to_value(s.doc().unwrap().doc.story(StoryId(sid)).unwrap()).unwrap();
    s.execute("style.character.create", &json!({"name": "Captured", "fromSelection": true})).unwrap();
    assert_eq!(chars(&s, "Captured"), CharAttrs { underline: Some(true), ..Default::default() });
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(serde_json::to_value(s.doc().unwrap().doc.story(StoryId(sid)).unwrap()).unwrap(), source);
    let after = document(&s);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(document(&s), before);
    assert_eq!(s.doc().unwrap().selection, selection);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(document(&s), after);
    assert_eq!(s.doc().unwrap().selection, selection);

    // Both commands use the native ZIP format, without a filesystem fixture.
    let saved = s.execute("file.serialize", &json!({})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.openBytes", &json!({"name": "capture.designcraft", "base64": saved["base64"]})).unwrap();
    assert_eq!(chars(&reopened, "Captured"), chars(&s, "Captured"));
    select(&mut reopened, sid, 6, 10);
    assert_eq!(reopened.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["underline"], false);
    let before_apply = document(&reopened);
    reopened.execute("style.character.apply", &json!({"name": "Captured"})).unwrap();
    assert_eq!(reopened.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["underline"], true);
    assert_eq!(reopened.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, "alpha beta");
    let after_apply = document(&reopened);
    reopened.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(document(&reopened), before_apply);
    reopened.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(document(&reopened), after_apply);
}

#[test]
fn character_capture_matches_paragraph_selection_and_caret_boundary_policy() {
    let (mut s, sid) = story("éX");
    select(&mut s, sid, 0, 2);
    s.execute("type.char", &json!({"underline": true})).unwrap();
    select(&mut s, sid, 2, 3);
    s.execute("type.char", &json!({"underline": false})).unwrap();
    // A selected character takes the run on the right; a caret takes the typing run on the left.
    // Reversed and mixed selections start at the lowest byte offset, just like paragraph capture.
    for (anchor, focus, underlined) in [(3, 2, false), (2, 2, true), (3, 0, true)] {
        select(&mut s, sid, anchor, focus);
        let p = s.execute("style.paragraph.create", &json!({"name": "Paragraph", "fromSelection": true})).unwrap();
        let c = s.execute("style.character.create", &json!({"name": "Character", "fromSelection": true})).unwrap();
        let p = s.doc().unwrap().doc.styles.para(p["name"].as_str().unwrap()).unwrap();
        let c = chars(&s, c["name"].as_str().unwrap());
        assert_eq!(c.underline, Some(underlined));
        assert_eq!(c.underline, p.chars.underline);
        assert_eq!(c.size, None, "unlike paragraph capture, character capture must not freeze paragraph defaults");
    }
}

#[test]
fn character_capture_is_opt_in_and_no_text_uses_only_explicit_chars() {
    let (mut s, sid) = story("alpha");
    select(&mut s, sid, 0, 5);
    s.execute("type.char", &json!({"underline": true})).unwrap();
    for options in [json!({"name": "Plain"}), json!({"name": "Plain", "fromSelection": false})] {
        let created = s.execute("style.character.create", &options).unwrap();
        assert!(chars(&s, created["name"].as_str().unwrap()).is_empty());
    }
    assert!(s.doc().unwrap().doc.styles.char_style("Plain 2").is_some());
    s.execute("selection.set", &json!({"ids": []})).unwrap();
    s.execute("style.character.create", &json!({"name": "No target", "fromSelection": true, "basedOn": "Plain", "chars": {"underline": false}}))
        .unwrap();
    assert_eq!(chars(&s, "No target"), CharAttrs { underline: Some(false), ..Default::default() });
    assert_eq!(s.doc().unwrap().doc.styles.char_style("No target").unwrap().based_on.as_deref(), Some("Plain"));
    let (mut empty, empty_sid) = story("");
    select(&mut empty, empty_sid, 0, 0);
    empty.execute("style.character.create", &json!({"name": "Empty", "fromSelection": true})).unwrap();
    assert!(chars(&empty, "Empty").is_empty());
}

#[test]
fn character_capture_retains_deleted_fill_and_reuses_it_after_native_reopen() {
    for named_source in [false, true] {
        let (mut s, sid) = story("alpha beta gamma");
        s.execute("swatch.create", &json!({"name": "Ink", "color": "#ff0000"})).unwrap();
        select(&mut s, sid, 0, 5);
        if named_source {
            s.execute("style.character.create", &json!({"name": "Base", "chars": {"fill": "Ink"}})).unwrap();
            s.execute("style.character.create", &json!({"name": "Source", "basedOn": "Base"})).unwrap();
            s.execute("style.character.apply", &json!({"name": "Source"})).unwrap();
        } else {
            s.execute("type.char", &json!({"fill": "Ink"})).unwrap();
        }
        s.execute("swatch.delete", &json!({"name": "Ink"})).unwrap();
        s.doc().unwrap().doc.check().unwrap();
        let before = document(&s);
        let selection = s.doc().unwrap().selection.clone();
        let undo_len = s.doc().unwrap().history.undo.len();
        s.execute("style.character.create", &json!({"name": "Captured", "fromSelection": true})).unwrap();
        assert_eq!(chars(&s, "Captured"), CharAttrs { fill: Some("Ink".into()), ..Default::default() });
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
        assert_eq!(s.doc().unwrap().selection, selection);
        let after = document(&s);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(document(&s), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(document(&s), after);
        // Non-fill overrides do not turn an existing captured reference into explicit input.
        s.execute("style.character.create", &json!({"name": "Tweaked", "fromSelection": true, "chars": {"underline": false, "tracking": 0}}))
            .unwrap();
        assert_eq!(chars(&s, "Tweaked"), CharAttrs { fill: Some("Ink".into()), underline: Some(false), tracking: Some(0.0), ..Default::default() });
        s.execute("style.character.create", &json!({"name": "Paper", "fromSelection": true, "chars": {"fill": "[Paper]"}})).unwrap();
        assert_eq!(chars(&s, "Paper"), CharAttrs { fill: Some("[Paper]".into()), ..Default::default() });
        s.execute("style.character.create", &json!({"name": "Cleared fill", "fromSelection": true, "chars": {"fill": null}})).unwrap();
        assert!(chars(&s, "Cleared fill").is_empty());
        select(&mut s, sid, 6, 10);
        s.execute("style.character.apply", &json!({"name": "Captured"})).unwrap();
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "Ink");
        s.doc().unwrap().doc.check().unwrap();

        let saved = s.execute("file.serialize", &json!({})).unwrap();
        let mut reopened = Session::new();
        reopened.execute("file.openBytes", &json!({"name": "deleted-fill.designcraft", "base64": saved["base64"]})).unwrap();
        reopened.doc().unwrap().doc.check().unwrap();
        for name in ["Captured", "Tweaked", "Paper", "Cleared fill"] {
            assert_eq!(chars(&reopened, name), chars(&s, name));
        }
        select(&mut reopened, sid, 6, 10);
        assert_eq!(reopened.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "Ink");
        select(&mut reopened, sid, 11, 16);
        assert_eq!(reopened.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "[Black]");
        reopened.execute("style.character.apply", &json!({"name": "Captured"})).unwrap();
        assert_eq!(reopened.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "Ink");
        reopened.execute("style.character.apply", &json!({"name": "Cleared fill"})).unwrap();
        assert_eq!(reopened.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "[Black]");
        assert_eq!(reopened.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, "alpha beta gamma");
    }
}

#[test]
fn character_capture_rejects_explicit_missing_fills_without_losing_redo() {
    let (mut s, sid) = story("alpha");
    s.execute("swatch.create", &json!({"name": "Ink", "color": "#ff0000"})).unwrap();
    select(&mut s, sid, 0, 5);
    s.execute("type.char", &json!({"fill": "Ink"})).unwrap();
    s.execute("swatch.delete", &json!({"name": "Ink"})).unwrap();
    s.execute("style.character.create", &json!({"name": "Redo sentinel"})).unwrap();
    let redo_doc = s.doc().unwrap().doc.clone();
    let redo_selection = s.doc().unwrap().selection.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    let before = s.doc().unwrap().clone();
    let journal = s.journal.clone();
    for (capture, fill) in [(true, "Missing"), (true, "Ink"), (false, "Missing")] {
        let error = s.execute("style.character.create", &json!({"name": "Invalid", "fromSelection": capture, "chars": {"fill": fill}})).unwrap_err();
        assert!(error.to_string().contains(fill));
        assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().doc, &before.doc));
        assert_eq!(s.doc().unwrap().selection, before.selection);
        assert_eq!(s.doc().unwrap().revision, before.revision);
        assert_eq!(format!("{:?}", s.doc().unwrap().history), format!("{:?}", before.history));
        assert_eq!(s.journal, journal);
    }
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().doc, &redo_doc));
    assert_eq!(s.doc().unwrap().selection, redo_selection);
    assert!(s.doc().unwrap().doc.styles.char_style("Redo sentinel").is_some());
}

#[test]
fn character_capture_explicit_null_restores_live_paragraph_inheritance() {
    for named_source in [false, true] {
        let (mut s, sid) = story("alpha\nbeta");
        select(&mut s, sid, 0, 5);
        let source = json!({"tracking": 20, "underline": true, "underlineWeight": 1.5, "fontStyle": "Italic"});
        if named_source {
            s.execute("style.character.create", &json!({"name": "Base", "chars": source})).unwrap();
            s.execute("style.character.create", &json!({"name": "Source", "basedOn": "Base"})).unwrap();
            s.execute("style.character.apply", &json!({"name": "Source"})).unwrap();
        } else {
            s.execute("type.char", &source).unwrap();
        }
        // Omitting chars still captures values, including a nullable attribute's numeric value.
        s.execute("style.character.create", &json!({"name": "Unchanged", "fromSelection": true})).unwrap();
        assert_eq!(chars(&s, "Unchanged").tracking, Some(20.0));
        assert_eq!(chars(&s, "Unchanged").underline_weight, Some(Some(1.5)));
        let before = document(&s);
        let undo_len = s.doc().unwrap().history.undo.len();
        s.execute(
            "style.character.create",
            &json!({"name": "Cleared", "fromSelection": true,
                "chars": {"tracking": null, "underlineWeight": null, "underline": false, "baselineShift": 0}}),
        )
        .unwrap();
        assert_eq!(
            chars(&s, "Cleared"),
            CharAttrs { font_style: Some("Italic".into()), underline: Some(false), baseline_shift: Some(0.0), ..Default::default() }
        );
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
        let after = document(&s);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(document(&s), before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(document(&s), after);
        s.execute("style.paragraph.create", &json!({"name": "Destination", "chars": {"tracking": 30, "underlineWeight": 2}})).unwrap();
        select(&mut s, sid, 6, 10);
        s.execute("style.paragraph.apply", &json!({"name": "Destination"})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Cleared"})).unwrap();
        let effective = s.execute("type.selectionAttrs", &json!({})).unwrap();
        assert_eq!(effective["chars"]["tracking"], 30.0);
        assert_eq!(effective["chars"]["underlineWeight"], 2.0);
        assert_eq!(effective["chars"]["underline"], false);
        s.execute("style.paragraph.edit", &json!({"name": "Destination", "chars": {"tracking": 40, "underlineWeight": 3}})).unwrap();
        let effective = s.execute("type.selectionAttrs", &json!({})).unwrap();
        assert_eq!(effective["chars"]["tracking"], 40.0);
        assert_eq!(effective["chars"]["underlineWeight"], 3.0);
    }
}

#[test]
fn character_capture_at_a_bare_caret_takes_the_typing_format() {
    let (mut s, sid) = story("alpha beta");
    select(&mut s, sid, 5, 5);
    s.execute("type.char", &json!({"attrs": {"fontStyle": "Bold"}})).unwrap();
    s.execute("style.character.create", &json!({"name": "Typed", "fromSelection": true})).unwrap();
    assert_eq!(chars(&s, "Typed").font_style.as_deref(), Some("Bold"));
}
