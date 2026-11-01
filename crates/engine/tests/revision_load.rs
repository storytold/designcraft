//! Cold-load compatibility for extreme story revisions and linked-story freshness.
use designcraft_doc::StoryId;
use designcraft_engine::Session;
use serde_json::{Value, json};
use std::sync::Arc;

fn setup() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}
fn frame(s: &mut Session, text: &str) -> (u64, StoryId) {
    let f = s.execute("frame.create", &json!({"rect":[36,36,200,100],"content":"text","text":text,"caret":false})).unwrap();
    (f["id"].as_u64().unwrap(), StoryId(f["story"].as_u64().unwrap()))
}
fn child(s: &mut Session, parent: StoryId) -> (u64, StoryId) {
    let f = s.execute("story.placeAndLink", &json!({"story":parent.0,"rect":[36,150,200,100]})).unwrap();
    (f["id"].as_u64().unwrap(), StoryId(f["story"].as_u64().unwrap()))
}

fn link(s: &mut Session, sid: StoryId) -> Value {
    s.execute("story.links", &json!({})).unwrap().as_array().unwrap().iter().find(|l| l["story"] == sid.0).unwrap().clone()
}

fn revision_load_setup() -> Session {
    let mut s = setup();
    // Make legacy paragraph defaults explicit so whole-model comparisons isolate revision policy.
    let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
    for style in &mut d.styles_mut().paragraph {
        style.para.keep_all_lines = Some(true);
        style.para.drop_cap_align_left = Some(false);
    }
    s
}

fn revision_load_reopen(s: &mut Session, legacy_json: bool) -> Session {
    let encoded = if legacy_json {
        designcraft_engine::cmd::base64_encode(&serde_json::to_vec(s.doc().unwrap().doc.as_ref()).unwrap())
    } else {
        s.execute("file.serialize", &json!({})).unwrap()["base64"].as_str().unwrap().to_string()
    };
    let title = s.doc().unwrap().doc.title.clone();
    let mut reopened = Session::new();
    reopened.execute("file.openBytes", &json!({"name":title,"base64":encoded})).unwrap();
    reopened
}

fn revision_load_parent_edit_case(legacy_json: bool, parent_rev: u64, replace_range: bool) {
    let mut source = revision_load_setup();
    let (_, parent) = frame(&mut source, "OLD");
    Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(parent).unwrap().rev = 0;
    let (_, linked) = child(&mut source, parent);
    source.execute("story.setText", &json!({"story":parent.0,"text":"BEFORE"})).unwrap();
    Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(parent).unwrap().rev = parent_rev;
    assert_eq!(source.doc().unwrap().doc.story(linked).unwrap().link, Some((parent, 0)));
    assert_eq!(link(&mut source, linked)["outOfDate"], true);
    source.doc().unwrap().doc.check().unwrap();

    let mut s = revision_load_reopen(&mut source, legacy_json);
    let opened = s.doc().unwrap().doc.clone();
    assert_eq!(link(&mut s, linked)["outOfDate"], true, "loading must preserve an already-stale link");
    assert_eq!(opened.story(linked).unwrap().text, "OLD");
    assert_eq!(opened.story(linked).unwrap().frames, source.doc().unwrap().doc.story(linked).unwrap().frames);

    if replace_range {
        s.execute("story.replaceRange", &json!({"story":parent.0,"start":0,"end":6,"text":"AFTER"})).unwrap();
    } else {
        s.execute("story.setText", &json!({"story":parent.0,"text":"AFTER"})).unwrap();
    }
    // With overflow checks, an unfixed upstream edit may fail before reaching this assertion.
    // Otherwise require real staleness before the normalization-counter checks below.
    assert_eq!(link(&mut s, linked)["outOfDate"], true, "editing the parent must not make its stale child current");
    assert_eq!(s.doc().unwrap().doc.story(parent).unwrap().text, "AFTER");
    assert_eq!(s.doc().unwrap().doc.story(linked), opened.story(linked));
    let before_update = s.doc().unwrap().doc.clone();
    assert_eq!(s.execute("story.updateLink", &json!({})).unwrap()["updated"], 1);

    // Only the copied text, run length, child revision and copied parent token change.
    let mut expected = (*before_update).clone();
    let parent_after_edit = expected.story(parent).unwrap().rev;
    let expected_child = expected.story_mut(linked).unwrap();
    expected_child.text = "AFTER".into();
    expected_child.chars[0].len = 5;
    expected_child.rev = 2;
    expected_child.link = Some((parent, parent_after_edit));
    assert_eq!(s.doc().unwrap().doc.as_ref(), &expected);
    assert_eq!(link(&mut s, linked)["outOfDate"], false);
    s.doc().unwrap().doc.check().unwrap();

    let after_update = s.doc().unwrap().doc.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before_update);
    assert_eq!(link(&mut s, linked)["outOfDate"], true);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after_update);
    let mut again = revision_load_reopen(&mut s, legacy_json);
    assert_eq!(again.doc().unwrap().doc, after_update);
    assert_eq!(link(&mut again, linked)["outOfDate"], false);

    assert_eq!(opened.story(parent).unwrap().rev, 0, "extreme saved revisions are rebased on load");
    assert_eq!(opened.story(linked).unwrap().link, Some((parent, u64::MAX)));
    assert_eq!(parent_after_edit, if replace_range { 2 } else { 1 });
}

#[test]
fn revision_load_zip_max_parent_stays_stale_and_updates() {
    revision_load_parent_edit_case(false, u64::MAX, false);
}

#[test]
fn revision_load_legacy_max_minus_one_parent_stays_stale_and_updates() {
    revision_load_parent_edit_case(true, u64::MAX - 1, true);
}

#[test]
fn revision_load_fresh_local_child_stays_current() {
    for legacy_json in [false, true] {
        let mut source = revision_load_setup();
        let (_, parent) = frame(&mut source, "SOURCE");
        Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(parent).unwrap().rev = 1_u64 << 63;
        let (_, linked) = child(&mut source, parent);
        source.execute("story.setText", &json!({"story":linked.0,"text":"LOCAL"})).unwrap();
        assert_eq!(link(&mut source, linked)["outOfDate"], false);
        source.doc().unwrap().doc.check().unwrap();

        let mut s = revision_load_reopen(&mut source, legacy_json);
        let opened = s.doc().unwrap().doc.clone();
        assert_eq!(link(&mut s, linked)["outOfDate"], false, "a child's local edits do not change parent freshness");
        assert_eq!(opened.story(linked).unwrap().text, "LOCAL");
        assert_eq!(opened.story(linked).unwrap().frames, source.doc().unwrap().doc.story(linked).unwrap().frames);
        assert_eq!(s.execute("story.updateLink", &json!({})).unwrap()["updated"], 0);
        assert_eq!(s.doc().unwrap().doc, opened, "default Update All must not overwrite a current child's local content");

        s.execute("story.setText", &json!({"story":linked.0,"text":"LOCAL+"})).unwrap();
        let local = s.doc().unwrap().doc.clone();
        assert_eq!(local.story(parent), opened.story(parent));
        assert_eq!(local.story(linked).unwrap().text, "LOCAL+");
        assert_eq!(local.story(linked).unwrap().link, opened.story(linked).unwrap().link);
        assert_eq!(link(&mut s, linked)["outOfDate"], false);
        assert_eq!(s.execute("story.updateLink", &json!({})).unwrap()["updated"], 0);
        assert_eq!(s.doc().unwrap().doc, local);

        // Check the inclusive policy boundary only after proving local-content preservation.
        assert_eq!(opened.story(parent).unwrap().rev, 0, "the normalization threshold includes 2^63");
        assert_eq!(opened.story(linked).unwrap().link, Some((parent, 0)));
    }
}

#[test]
fn revision_load_low_revision_preserves_whole_document() {
    for legacy_json in [false, true] {
        let mut source = revision_load_setup();
        let (_, parent) = frame(&mut source, "SOURCE");
        Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(parent).unwrap().rev = (1_u64 << 63) - 1;
        let (_, linked) = child(&mut source, parent);
        source.execute("story.setText", &json!({"story":linked.0,"text":"LOCAL"})).unwrap();
        let (_, unrelated) = frame(&mut source, "UNRELATED");
        Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(unrelated).unwrap().rev = 7;
        source.doc().unwrap().doc.check().unwrap();
        let before = source.doc().unwrap().doc.clone();

        let mut reopened = revision_load_reopen(&mut source, legacy_json);
        assert_eq!(reopened.doc().unwrap().doc, before, "low revisions and the complete document must survive either loader unchanged");
        assert_eq!(link(&mut reopened, linked)["outOfDate"], false);
        assert_eq!(reopened.execute("story.updateLink", &json!({})).unwrap()["updated"], 0);
        assert_eq!(reopened.doc().unwrap().doc, before);
    }
}

#[test]
fn revision_load_graph_preserves_whole_model_and_original_link_freshness() {
    // One saved graph feeds both readers; the middle is both a locally edited child and a parent.
    let mut source = revision_load_setup();
    let (_, parent) = frame(&mut source, "ROOT");
    Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(parent).unwrap().rev = 1_u64 << 63;
    let (_, middle) = child(&mut source, parent);
    source.execute("story.setText", &json!({"story":middle.0,"text":"MIDDLE BEFORE"})).unwrap();
    Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(middle).unwrap().rev = 0;
    let (_, stale) = child(&mut source, middle);
    source.execute("story.setText", &json!({"story":stale.0,"text":"STALE LOCAL"})).unwrap();
    source.execute("story.setText", &json!({"story":middle.0,"text":"MIDDLE LOCAL"})).unwrap();
    Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(middle).unwrap().rev = u64::MAX;
    let (_, fresh) = child(&mut source, middle);
    source.execute("story.setText", &json!({"story":fresh.0,"text":"FRESH LOCAL"})).unwrap();

    let (_, low_parent) = frame(&mut source, "LOW PARENT");
    Arc::make_mut(&mut source.doc_mut().unwrap().doc).story_mut(low_parent).unwrap().rev = 7;
    let (_, low_child) = child(&mut source, low_parent);
    source.execute("story.setText", &json!({"story":low_child.0,"text":"LOW LOCAL"})).unwrap();
    let (_, missing_child) = frame(&mut source, "MISSING LOCAL");
    let missing_parent = StoryId(u64::MAX);
    {
        let d = Arc::make_mut(&mut source.doc_mut().unwrap().doc);
        assert!(d.story(missing_parent).is_none());
        // High copied tokens alone must not trigger rebasing or invent a missing parent.
        d.story_mut(low_child).unwrap().link = Some((low_parent, u64::MAX));
        d.story_mut(missing_child).unwrap().link = Some((missing_parent, u64::MAX - 1));
        // Nondefault formatting and unrelated metadata are part of the whole-model oracle.
        d.story_mut(middle).unwrap().paras[0].para.keep_all_lines = Some(false);
        d.story_mut(fresh).unwrap().chars[0].format.over.fill = Some("[Black]".into());
        d.settings.keyboard_increment = 3.25;
        d.check().unwrap();
    }
    let check_links = |s: &mut Session, middle_stale: bool, fresh_stale: bool| {
        for (sid, expected) in [(middle, middle_stale), (fresh, fresh_stale), (stale, true), (low_child, true), (missing_child, true)] {
            assert_eq!(link(s, sid)["outOfDate"], expected, "unexpected freshness for story {}", sid.0);
        }
    };
    check_links(&mut source, false, false);
    let original = source.doc().unwrap().doc.clone();
    assert_eq!(original.story(middle).unwrap().link, Some((parent, 1_u64 << 63)));
    assert_eq!(original.story(fresh).unwrap().link, Some((middle, u64::MAX)));
    assert_eq!(original.story(stale).unwrap().link, Some((middle, 0)));

    // Independent allowed-delta oracle: exactly two revisions and three copied tokens change.
    // Do not derive this expectation by iterating revisions or running the normalization helper.
    let mut expected = (*original).clone();
    expected.story_mut(parent).unwrap().rev = 0;
    expected.story_mut(middle).unwrap().rev = 0;
    expected.story_mut(middle).unwrap().link = Some((parent, 0));
    expected.story_mut(fresh).unwrap().link = Some((middle, 0));
    expected.story_mut(stale).unwrap().link = Some((middle, u64::MAX));

    for legacy_json in [false, true] {
        let mut opened = revision_load_reopen(&mut source, legacy_json);
        check_links(&mut opened, false, false);
        assert_eq!(opened.doc().unwrap().doc.as_ref(), &expected, "only the five allowed graph fields may change; legacy={legacy_json}");
        assert_eq!(source.doc().unwrap().doc, original, "both readers must receive the same original graph");
        opened.doc().unwrap().doc.check().unwrap();

        // Reopening immediately must not reinterpret the stale MAX token as a high parent revision.
        let mut again = revision_load_reopen(&mut opened, legacy_json);
        assert_eq!(again.doc().unwrap().doc.as_ref(), &expected, "normalization must be idempotent; legacy={legacy_json}");
        check_links(&mut again, false, false);
        assert_eq!(again.doc().unwrap().doc.story(middle).unwrap().text, "MIDDLE LOCAL");
        assert_eq!(again.doc().unwrap().doc.story(fresh).unwrap().text, "FRESH LOCAL");
        assert_eq!(again.doc().unwrap().doc.story(stale).unwrap().text, "STALE LOCAL");

        // A local middle edit keeps its inbound link current, but invalidates its fresh child.
        again.execute("story.setText", &json!({"story":middle.0,"text":"MIDDLE EDITED"})).unwrap();
        check_links(&mut again, false, true);
        let edited_middle = again.doc().unwrap().doc.story(middle).unwrap();
        assert_eq!(edited_middle.text, "MIDDLE EDITED");
        assert_eq!(edited_middle.rev, 1);
        assert_eq!(edited_middle.link, Some((parent, 0)));
        for sid in [parent, fresh, stale, low_parent, low_child, missing_child] {
            assert_eq!(again.doc().unwrap().doc.story(sid), expected.story(sid), "editing the middle must not rewrite story {}", sid.0);
        }

        // Editing the root now makes the locally edited middle stale without overwriting it.
        let before_parent_edit = again.doc().unwrap().doc.clone();
        again.execute("story.setText", &json!({"story":parent.0,"text":"ROOT EDITED"})).unwrap();
        check_links(&mut again, true, true);
        assert_eq!(again.doc().unwrap().doc.story(parent).unwrap().rev, 1);
        assert_eq!(again.doc().unwrap().doc.story(parent).unwrap().text, "ROOT EDITED");
        for sid in [middle, fresh, stale, low_parent, low_child, missing_child] {
            assert_eq!(again.doc().unwrap().doc.story(sid), before_parent_edit.story(sid), "editing the root must not rewrite story {}", sid.0);
        }
        again.doc().unwrap().doc.check().unwrap();
    }
}
