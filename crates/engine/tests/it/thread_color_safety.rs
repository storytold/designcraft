//! Linked-story threading and text-color transaction safety.
use designcraft_doc::{ParaFormat, Story, StoryId, table::Table};
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
fn state(s: &Session) -> Value {
    let d = s.doc().unwrap();
    json!({"doc":d.doc,"selection":d.selection,"revision":d.revision,"dirty":d.is_dirty(),"undo":format!("{:?}",d.history),"journal":s.journal})
}
fn link(s: &mut Session, sid: StoryId) -> Value {
    s.execute("story.links", &json!({})).unwrap().as_array().unwrap().iter().find(|l| l["story"] == sid.0).unwrap().clone()
}

#[test]
fn safety_linked_parent_follows_merge_and_updates_after_native_reopen() {
    let mut s = setup();
    let (first, survivor) = frame(&mut s, "PREFIX");
    let (incoming, parent) = frame(&mut s, "PARENT");
    let (_, linked) = child(&mut s, parent);
    // A coincident old parent revision must not hide the change of parent identity/content.
    let future_rev = s.doc().unwrap().doc.story(survivor).unwrap().rev + 2;
    s.edit(|d, _| {
        d.story_mut(linked).unwrap().link = Some((parent, future_rev));
        Ok(())
    })
    .unwrap();
    s.execute("text.thread", &json!({"from":first,"to":incoming})).unwrap();
    assert_eq!(link(&mut s, linked)["parent"], survivor.0);
    assert_eq!(link(&mut s, linked)["outOfDate"], true);
    assert_eq!(s.doc().unwrap().doc.story(linked).unwrap().text, "PARENT");
    let bytes = s.execute("file.serialize", &json!({})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.openBytes", &json!({"name":"links.designcraft","base64":bytes["base64"]})).unwrap();
    assert_eq!(link(&mut reopened, linked)["parent"], survivor.0);
    reopened.execute("story.updateLink", &json!({"story":linked.0})).unwrap();
    assert_eq!(reopened.doc().unwrap().doc.story(linked).unwrap().text, "PREFIX\nPARENT");
    assert_eq!(link(&mut reopened, linked)["outOfDate"], false);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(link(&mut s, linked)["parent"], parent.0);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(link(&mut s, linked)["parent"], survivor.0);
}

#[test]
fn safety_linked_child_cannot_merge_body_text_in_either_direction() {
    for reverse in [false, true] {
        let mut s = setup();
        let (_, parent) = frame(&mut s, "SOURCE");
        let (linked, child) = child(&mut s, parent);
        let (body, _) = frame(&mut s, "BODY");
        let (from, to) = if reverse { (body, linked) } else { (linked, body) };
        let before = state(&s);
        let error = s.execute("text.thread", &json!({"from":from,"to":to})).unwrap_err();
        assert!(error.to_string().contains("linked story"));
        assert_eq!(state(&s), before);
        s.execute("story.updateLink", &json!({"story":child.0})).unwrap();
        assert!(s.doc().unwrap().doc.stories.values().any(|st| st.text == "BODY"));
    }
}

#[test]
fn safety_linked_child_allows_empty_extensions_in_both_directions() {
    for reverse in [false, true] {
        let mut s = setup();
        let (_, parent) = frame(&mut s, "SOURCE");
        let (linked, child) = child(&mut s, parent);
        let (empty, _) = frame(&mut s, "");
        let (from, to) = if reverse { (empty, linked) } else { (linked, empty) };
        s.execute("text.thread", &json!({"from":from,"to":to})).unwrap();
        assert_eq!(link(&mut s, child)["parent"], parent.0);
        s.execute("story.setText", &json!({"story":parent.0,"text":"UPDATED"})).unwrap();
        assert_eq!(link(&mut s, child)["outOfDate"], true);
        s.execute("story.updateLink", &json!({"story":child.0})).unwrap();
        let st = s.doc().unwrap().doc.story(child).unwrap();
        assert_eq!(st.text, "UPDATED");
        assert_eq!(st.frames.len(), 2);
        assert_eq!(link(&mut s, child)["outOfDate"], false);
    }
}

#[test]
fn safety_linked_empty_parent_cannot_be_merged_into_its_child() {
    let mut s = setup();
    let (parent_frame, parent) = frame(&mut s, "");
    let (linked, _) = child(&mut s, parent);
    let before = state(&s);
    assert!(s.execute("text.thread", &json!({"from":parent_frame,"to":linked})).is_err());
    assert_eq!(state(&s), before);
}

#[test]
fn safety_text_color_without_target_is_atomic_and_carets_remain_valid() {
    let mut s = setup();
    let f = s.execute("frame.create", &json!({"rect":[36,36,200,100]})).unwrap();
    s.execute("selection.set", &json!({"ids":[f["id"]]})).unwrap();
    let before = state(&s);
    assert!(s.execute("object.color", &json!({"color":"#123456","text":true})).is_err());
    assert_eq!(state(&s), before);
    for text in ["", "Words"] {
        let (_, sid) = frame(&mut s, text);
        s.execute("text.select", &json!({"story":sid.0,"anchor":0})).unwrap();
        let color = s.execute("object.color", &json!({"color":"#654321","text":true})).unwrap();
        s.execute("text.insert", &json!({"text":"X"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().format_after(0).over.fill.as_deref(), color["swatch"].as_str());
    }
}

fn nested_story(depth: usize, name: &str) -> Story {
    let mut st = Story::with_text(StoryId(0), "leaf", ParaFormat::default());
    st.chars[0].format.over.fill = Some(name.into());
    for _ in 0..depth {
        let mut outer = Story::with_text(StoryId(0), &designcraft_doc::TABLE_ANCHOR.to_string(), ParaFormat::default());
        let mut t = Table::new(1, 1, 1, 0, 0, 100.);
        t.cells[0].text = st;
        outer.tables.insert(1, Arc::new(t));
        outer.paras[0].table = Some(1);
        st = outer;
    }
    st
}

#[test]
fn safety_deep_swatch_rename_rejects_atomically_with_pending_color() {
    // A small boundary probe, never an attempted stack-overflow reproduction.
    for depth in [33, 34] {
        let mut s = setup();
        let (_, sid) = frame(&mut s, "root");
        s.execute("text.select", &json!({"story":sid.0,"anchor":0})).unwrap();
        let old = s.execute("object.color", &json!({"color":"#123456"})).unwrap()["swatch"].as_str().unwrap().to_string();
        s.edit(|d, _| {
            let st = d.story_mut(sid).unwrap();
            let frames = st.frames.clone();
            *st = nested_story(depth, &old);
            st.id = sid;
            st.frames = frames;
            Ok(())
        })
        .unwrap();
        let pending = s.execute("type.selectionAttrs", &json!({})).unwrap();
        let before = state(&s);
        let doc = s.doc().unwrap().doc.clone();
        let error = s.execute("swatch.addToSwatches", &json!({"swatch":old,"name":"Brand"})).unwrap_err();
        assert!(error.to_string().contains("nesting"));
        assert!(Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
        assert_eq!(state(&s), before);
        assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap(), pending);
    }
}

#[test]
fn safety_swatch_rename_reaches_last_supported_depth() {
    let mut s = setup();
    let (_, sid) = frame(&mut s, "root");
    s.execute("text.select", &json!({"story":sid.0,"anchor":0})).unwrap();
    let old = s.execute("object.color", &json!({"color":"#123456"})).unwrap()["swatch"].as_str().unwrap().to_string();
    s.edit(|d, _| {
        let st = d.story_mut(sid).unwrap();
        let frames = st.frames.clone();
        *st = nested_story(32, &old);
        st.id = sid;
        st.frames = frames;
        Ok(())
    })
    .unwrap();
    s.execute("swatch.addToSwatches", &json!({"swatch":old,"name":"Brand"})).unwrap();
    let mut st = s.doc().unwrap().doc.story(sid).unwrap();
    for _ in 0..32 {
        st = &st.tables[&1].cells[0].text;
    }
    assert_eq!(st.format_after(0).over.fill.as_deref(), Some("Brand"));
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["fill"], "Brand");
}

#[test]
fn safety_empty_linked_parent_follows_survivor_only_on_explicit_update() {
    for incoming_empty in [false, true] {
        for alternate_layout in [false, true] {
            let mut s = setup();
            let (empty, parent) = frame(&mut s, "");
            let linked = if alternate_layout {
                s.execute("layout.createAlternate", &json!({"name":"Alternate","width":600,"height":800})).unwrap();
                let links = s.execute("story.links", &json!({})).unwrap();
                StoryId(links.as_array().unwrap().iter().find(|l| l["parent"] == parent.0).unwrap()["story"].as_u64().unwrap())
            } else {
                child(&mut s, parent).1
            };
            let (body, survivor) = frame(&mut s, "BODY");
            let (from, to) = if incoming_empty { (body, empty) } else { (empty, body) };
            s.execute("text.thread", &json!({"from":from,"to":to})).unwrap();
            assert!(s.doc().unwrap().doc.story(parent).is_none());
            assert_eq!(link(&mut s, linked)["parent"], survivor.0);
            assert_eq!(link(&mut s, linked)["outOfDate"], true);
            assert_eq!(s.doc().unwrap().doc.story(linked).unwrap().text, "");
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(link(&mut s, linked)["parent"], parent.0);
            assert_eq!(s.doc().unwrap().doc.story(linked).unwrap().text, "");
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(link(&mut s, linked)["parent"], survivor.0);
            let bytes = s.execute("file.serialize", &json!({})).unwrap();
            let mut reopened = Session::new();
            reopened.execute("file.openBytes", &json!({"name":"empty-parent.designcraft","base64":bytes["base64"]})).unwrap();
            assert_eq!(link(&mut reopened, linked)["outOfDate"], true);
            assert_eq!(reopened.doc().unwrap().doc.story(linked).unwrap().text, "");
            reopened.execute("story.updateLink", &json!({"story":linked.0})).unwrap();
            assert_eq!(reopened.doc().unwrap().doc.story(linked).unwrap().text, "BODY");
            assert_eq!(link(&mut reopened, linked)["outOfDate"], false);
        }
    }
}

fn check_indirect_linked_ancestor_rejection(ancestor_first: bool) {
    let mut s = setup();
    let (ancestor_frame, ancestor) = frame(&mut s, "");
    let (middle_frame, middle) = child(&mut s, ancestor);
    s.execute("story.setText", &json!({"story": middle.0, "text": "MIDDLE"})).unwrap();
    let (leaf_frame, leaf) = child(&mut s, middle);
    s.execute("selection.set", &json!({"ids": [middle_frame]})).unwrap();
    // Retain a real redo entry so rejection cannot silently clear history.
    s.execute("story.setText", &json!({"story": middle.0, "text": "REDO"})).unwrap();
    let redo_doc = s.doc().unwrap().doc.clone();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(middle).unwrap().text, "MIDDLE");
    assert_eq!(s.doc().unwrap().doc.story(middle).unwrap().link, Some((ancestor, 0)));
    assert_eq!(s.doc().unwrap().doc.story(leaf).unwrap().link, Some((middle, 2)));
    assert_eq!(link(&mut s, middle)["outOfDate"], false);
    assert_eq!(link(&mut s, leaf)["outOfDate"], false);
    s.doc().unwrap().doc.check().unwrap();
    assert!(!s.doc().unwrap().history.redo.is_empty());
    let before_doc = s.doc().unwrap().doc.clone();
    let saved_doc = s.doc().unwrap().saved_doc.clone();
    let before = state(&s);
    let (from, to) = if ancestor_first { (ancestor_frame, leaf_frame) } else { (leaf_frame, ancestor_frame) };

    let error = s.execute("text.thread", &json!({"from": from, "to": to})).unwrap_err();

    assert!(error.to_string().contains("linked story"));
    assert_eq!(s.doc().unwrap().doc, before_doc);
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before_doc));
    assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &saved_doc));
    assert_eq!(state(&s), before, "rejection must preserve selection, history, revision, dirty state and journal");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, redo_doc);
}

#[test]
fn safety_thread_rejects_empty_indirect_ancestor_before_descendant() {
    check_indirect_linked_ancestor_rejection(true);
}

#[test]
fn safety_thread_rejects_empty_indirect_ancestor_after_descendant() {
    check_indirect_linked_ancestor_rejection(false);
}

#[test]
fn safety_thread_allows_unrelated_empty_frame_beside_linked_descendant() {
    for empty_first in [true, false] {
        let mut s = setup();
        let (_, ancestor) = frame(&mut s, "");
        let (_, middle) = child(&mut s, ancestor);
        s.execute("story.setText", &json!({"story": middle.0, "text": "MIDDLE"})).unwrap();
        let (leaf_frame, leaf) = child(&mut s, middle);
        let (empty_frame, empty_story) = frame(&mut s, "");
        let before = s.doc().unwrap().doc.clone();
        let undo_len = s.doc().unwrap().history.undo.len();
        assert_eq!(before.story(leaf).unwrap().rev, 1);
        let (from, to) = if empty_first { (empty_frame, leaf_frame) } else { (leaf_frame, empty_frame) };
        let frames = vec![designcraft_doc::ItemId(from), designcraft_doc::ItemId(to)];
        let mut expected = (*before).clone();
        expected.stories.remove(&empty_story);
        let st = expected.story_mut(leaf).unwrap();
        st.frames = frames;
        st.rev = 2;
        expected.item_mut(designcraft_doc::ItemId(empty_frame)).unwrap().text_frame_mut().unwrap().story = leaf;

        s.execute("text.thread", &json!({"from": from, "to": to})).unwrap();

        assert_eq!(*s.doc().unwrap().doc, expected);
        assert_eq!(s.doc().unwrap().selection, designcraft_doc::Selection::items(vec![designcraft_doc::ItemId(to)]));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
        assert_eq!(link(&mut s, middle)["outOfDate"], false);
        assert_eq!(link(&mut s, leaf)["outOfDate"], false);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc, before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(*s.doc().unwrap().doc, expected);
    }
}
