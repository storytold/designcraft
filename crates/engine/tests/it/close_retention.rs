//! Engine-only ownership regression: no allocator/RSS assumptions and no cache-budget changes.

use std::sync::Arc;

use designcraft_doc::StoryId;
use designcraft_engine::Session;
use serde_json::json;

fn text_document(s: &mut Session, title: &str) -> StoryId {
    s.execute("file.new", &json!({"title":title})).unwrap();
    let result = s.execute("frame.create", &json!({"rect":[36,36,300,300],"content":"text","text":"A retained story."})).unwrap();
    StoryId(result["story"].as_u64().unwrap())
}

#[test]
fn closing_last_document_releases_current_and_previous_composition_payloads() {
    let mut s = Session::new();
    let sid = text_document(&mut s, "A");
    let original_story = Arc::downgrade(&s.doc().unwrap().doc.stories[&sid]);
    let original_layout = Arc::downgrade(&s.cache.get(&s.doc().unwrap().doc, sid, None));
    s.execute("text.select", &json!({"story":sid.0,"anchor":0,"focus":0})).unwrap();
    s.execute("text.insert", &json!({"text":"é","raw":true})).unwrap();
    let edited_story = Arc::downgrade(&s.doc().unwrap().doc.stories[&sid]);
    let edited_layout = Arc::downgrade(&s.cache.get(&s.doc().unwrap().doc, sid, None));
    assert!(original_layout.upgrade().is_some(), "previous composition is useful before close");
    assert!(edited_layout.upgrade().is_some(), "current composition is cached");
    s.execute("file.close", &json!({})).unwrap();
    assert!(s.documents().is_empty());
    assert!(original_story.upgrade().is_none(), "closed original story payload was released");
    assert!(edited_story.upgrade().is_none(), "closed edited story payload was released");
    assert!(original_layout.upgrade().is_none(), "closed previous composition was released");
    assert!(edited_layout.upgrade().is_none(), "closed current composition was released");
}

#[test]
fn last_close_detaches_in_flight_cache_without_invalidating_its_snapshot() {
    let mut s = Session::new();
    let sid = text_document(&mut s, "A");
    let in_flight_state = s.doc().unwrap().clone();
    let in_flight_cache = s.cache.clone();
    let old_cache = Arc::downgrade(&in_flight_cache);
    s.execute("file.close", &json!({})).unwrap();
    assert!(!Arc::ptr_eq(&s.cache, &in_flight_cache), "late work must not repopulate the idle session cache");
    let late_result = in_flight_cache.get(&in_flight_state.doc, sid, None);
    assert_eq!(late_result.text_len, "A retained story.".len());
    let late_weak = Arc::downgrade(&late_result);
    drop(late_result);
    drop(in_flight_cache);
    drop(in_flight_state);
    assert!(old_cache.upgrade().is_none());
    assert!(late_weak.upgrade().is_none());
}

#[test]
fn closing_another_document_keeps_open_history_and_cache() {
    let mut s = Session::new();
    let sid = text_document(&mut s, "A");
    let before = s.doc().unwrap().doc.clone();
    s.execute("text.select", &json!({"story":sid.0,"anchor":0,"focus":0})).unwrap();
    s.execute("text.insert", &json!({"text":"é","raw":true})).unwrap();
    let edited = s.doc().unwrap().doc.clone();
    let cache = s.cache.clone();
    let cached_a = Arc::downgrade(&s.cache.get(&edited, sid, None));
    text_document(&mut s, "B");
    s.execute("file.close", &json!({})).unwrap();
    assert_eq!(s.documents().len(), 1);
    assert!(Arc::ptr_eq(&s.cache, &cache));
    assert_eq!(s.doc().unwrap().doc, edited);
    assert!(cached_a.upgrade().is_some(), "closing B must not clear A’s populated cache");
    assert!(std::sync::Weak::ptr_eq(&cached_a, &Arc::downgrade(&s.cache.get(&edited, sid, None))));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, edited);
    s.close_document(usize::MAX);
    assert!(Arc::ptr_eq(&s.cache, &cache), "invalid close is a no-op");
    assert!(cached_a.upgrade().is_some(), "invalid close must not clear a populated cache");
    assert_eq!(s.doc().unwrap().doc, edited);
}

#[test]
fn closing_last_document_preserves_explicit_clipboard_ownership() {
    let mut s = Session::new();
    let sid = text_document(&mut s, "A");
    s.clipboard = Some(s.doc().unwrap().doc.clone());
    let story = Arc::downgrade(&s.doc().unwrap().doc.stories[&sid]);
    s.execute("file.close", &json!({})).unwrap();
    assert_eq!(s.clipboard.as_ref().unwrap().story(sid).unwrap().text, "A retained story.");
    assert!(story.upgrade().is_some(), "clipboard payload remains intentionally live");
    s.clipboard = None;
    assert!(story.upgrade().is_none());
}

#[test]
fn invalid_close_on_empty_session_keeps_cache_identity() {
    let mut s = Session::new();
    let cache = s.cache.clone();
    s.close_document(0);
    assert!(Arc::ptr_eq(&s.cache, &cache));
    s.close_document(usize::MAX);
    assert!(Arc::ptr_eq(&s.cache, &cache));
}

#[test]
fn closing_an_inactive_document_preserves_the_active_preview_and_cache() {
    let mut s = Session::new();
    text_document(&mut s, "Inactive");
    let sid = text_document(&mut s, "Preview");
    s.execute("data.source.select", &json!({"bytes":"TmFtZQpBZGEK","name":"people.csv"})).unwrap();
    s.execute("text.select", &json!({"story":sid.0,"anchor":0,"focus":17})).unwrap();
    s.execute("text.insert", &json!({"text":"","raw":true})).unwrap();
    s.execute("data.placeholder.add", &json!({"field":"Name"})).unwrap();
    let template = s.doc().unwrap().doc.clone();
    assert_eq!(template.story(sid).unwrap().text, "<<Name>>");
    s.execute("data.preview", &json!({"record":1})).unwrap();
    let preview = s.doc().unwrap().doc.clone();
    assert_eq!(preview.story(sid).unwrap().text, "Ada");
    let cache = s.cache.clone();
    let preview_layout = Arc::downgrade(&cache.get(&preview, sid, None));
    s.close_document(0);
    assert!(Arc::ptr_eq(&s.cache, &cache));
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &preview));
    assert_eq!(s.doc().unwrap().preview_record, Some(1));
    assert!(Arc::ptr_eq(s.doc().unwrap().preview_stash.as_ref().unwrap(), &template));
    assert!(preview_layout.upgrade().is_some());
    drop(cache);
    drop(template);
    drop(preview);
    s.execute("file.close", &json!({})).unwrap();
    assert!(s.documents().is_empty());
    assert!(preview_layout.upgrade().is_none());
}
