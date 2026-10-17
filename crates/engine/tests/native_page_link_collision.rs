//! Native page placement must preserve linked-story identity across document-local ID collisions.
use designcraft_doc::{ItemId, StoryId};
use designcraft_engine::Session;
use serde_json::json;
use std::sync::Arc;

fn frame(s: &mut Session, text: &str, rect: [i32; 4]) -> StoryId {
    let f = s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text, "caret": false})).unwrap();
    StoryId(f["story"].as_u64().unwrap())
}

fn placed_pair(linked: bool) -> (Session, StoryId, StoryId, StoryId) {
    let mut source = Session::new();
    source.execute("file.new", &json!({"pages": 1})).unwrap();
    let source_parent = frame(&mut source, "", [36, 36, 200, 100]);
    let source_child = if linked {
        let f = source.execute("story.placeAndLink", &json!({"story": source_parent.0, "rect": [36, 150, 200, 220]})).unwrap();
        let child = StoryId(f["story"].as_u64().unwrap());
        source.execute("story.setText", &json!({"story": child.0, "text": "LOCAL"})).unwrap();
        child
    } else {
        frame(&mut source, "LOCAL", [36, 150, 200, 220])
    };
    let source_doc = source.doc().unwrap().doc.clone();
    assert_eq!(source_doc.story(source_parent).unwrap().rev, 0);
    assert_eq!(source_doc.story(source_child).unwrap().link, if linked { Some((source_parent, 0)) } else { None });
    assert_eq!(source_doc.story(source_child).unwrap().rev, if linked { 2 } else { 1 });
    source_doc.check().unwrap();

    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    let unrelated = frame(&mut s, "", [36, 36, 200, 100]);
    assert_eq!(unrelated, source_parent, "the collision must be real, not a guessed ID");
    let original = s.doc().unwrap().doc.clone();
    assert_eq!(original.stories.len(), 1);
    assert_eq!(original.story(unrelated).unwrap().rev, 0);
    let original_frame = original.story(unrelated).unwrap().frames[0];
    let saved = source.execute("file.serialize", &json!({})).unwrap();
    let placed = s.execute("file.place", &json!({"base64": saved["base64"], "name": "source.designcraft", "layoutPage": 1, "x": 0, "y": 0})).unwrap();
    assert_eq!(source.doc().unwrap().doc, source_doc);
    assert_eq!(placed["items"], 2);
    let d = &s.doc().unwrap().doc;
    assert_eq!(d.item(ItemId(placed["id"].as_u64().unwrap())).unwrap().children().len(), 2);
    assert_eq!(d.stories.len(), 3);
    assert_eq!(d.story(unrelated), original.story(unrelated));
    assert_eq!(d.item(original_frame), original.item(original_frame));
    let parent = d.stories.values().find(|st| st.id != unrelated && st.text.is_empty()).unwrap().id;
    let child = d.stories.values().find(|st| st.text == "LOCAL").unwrap().id;
    assert_ne!(parent, child);
    assert_ne!(child, unrelated);
    assert_eq!(d.story(parent).unwrap().rev, 0);
    assert_eq!(d.story(child).unwrap().rev, if linked { 2 } else { 1 });
    d.check().unwrap();
    (s, unrelated, parent, child)
}

#[test]
fn native_page_link_collision_cannot_update_from_unrelated_destination_story() {
    let (mut s, unrelated, parent, child) = placed_pair(true);
    s.execute("story.setText", &json!({"story": unrelated.0, "text": "UNRELATED"})).unwrap();
    let before_unrelated_update = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let updated = s.execute("story.updateLink", &json!({})).unwrap();
    // This is the intended negative: current a930 copies UNRELATED into the imported child.
    assert_eq!(s.doc().unwrap().doc.story(child).unwrap().text, "LOCAL", "an unrelated destination edit must not replace imported local text");
    assert_eq!(updated["updated"], 0);
    assert_eq!(s.doc().unwrap().doc, before_unrelated_update);
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(s.doc().unwrap().doc.story(child).unwrap().link, Some((parent, 0)));

    // The link must remain functional; dropping it would hide the collision.
    s.execute("story.setText", &json!({"story": parent.0, "text": "IMPORTED"})).unwrap();
    let links = s.execute("story.links", &json!({})).unwrap();
    assert_eq!(links, json!([{"story": child.0, "parent": parent.0, "outOfDate": true}]));
    let before = s.doc().unwrap().doc.clone();
    assert_eq!(before.story(parent).unwrap().rev, 1);
    assert_eq!(before.story(child).unwrap().rev, 2);
    let mut expected = (*before).clone();
    let st = expected.story_mut(child).unwrap();
    st.text = "IMPORTED".into();
    assert_eq!(st.chars.len(), 1);
    st.chars[0].len = 8;
    st.rev = 3;
    st.link = Some((parent, 1));
    let undo_len = s.doc().unwrap().history.undo.len();
    let saved_doc = s.doc().unwrap().saved_doc.clone();

    assert_eq!(s.execute("story.updateLink", &json!({})).unwrap()["updated"], 1);
    assert_eq!(*s.doc().unwrap().doc, expected, "only the imported child's text, run length, revision and copied token change");
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
    assert!(s.doc().unwrap().history.redo.is_empty());
    assert!(Arc::ptr_eq(&s.doc().unwrap().saved_doc, &saved_doc));
    s.doc().unwrap().doc.check().unwrap();

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_len);
    assert_eq!(s.doc().unwrap().history.redo.len(), 1);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.doc().unwrap().doc, expected);
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1);
    assert!(s.doc().unwrap().history.redo.is_empty());
}

#[test]
fn native_page_unlinked_control_preserves_imported_text_and_destination() {
    let (mut s, unrelated, parent, child) = placed_pair(false);
    s.execute("story.setText", &json!({"story": unrelated.0, "text": "UNRELATED"})).unwrap();
    s.execute("story.setText", &json!({"story": parent.0, "text": "IMPORTED"})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    assert_eq!(s.execute("story.links", &json!({})).unwrap(), json!([]));
    assert_eq!(s.execute("story.updateLink", &json!({})).unwrap()["updated"], 0);
    assert_eq!(s.doc().unwrap().doc, before);
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(s.doc().unwrap().doc.story(child).unwrap().text, "LOCAL");
    assert_eq!(s.doc().unwrap().doc.story(child).unwrap().link, None);
    s.doc().unwrap().doc.check().unwrap();
}
