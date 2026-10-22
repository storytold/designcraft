//! Own a real native Job across last close. This does not run the asynchronous worker.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use designcraft_doc::StoryId;
use designcraft_engine::Session;
use designcraft_geom::Affine;
use designcraft_render::RenderOptions;
use designcraft_ui_egui::render_worker::Job;
use serde_json::json;

#[test]
fn real_job_keeps_retired_cache_until_its_last_owner_drops() {
    let mut session = Session::new();
    session.execute("file.new", &json!({"pages": 1})).unwrap();
    let frame = session.execute("frame.create", &json!({"rect": [36, 36, 220, 320], "content": "text", "text": "Job source A."})).unwrap();
    let sid = StoryId(frame["story"].as_u64().unwrap());
    let initial = session.cache.get(&session.doc().unwrap().doc, sid, None);
    drop(initial);
    let end = session.doc().unwrap().doc.story(sid).unwrap().text.len();
    session.execute("text.select", &json!({"story": sid.0, "anchor": end, "focus": end})).unwrap();
    session.execute("text.insert", &json!({"text": " Edited B."})).unwrap();
    let current = session.cache.get(&session.doc().unwrap().doc, sid, None);
    drop(current);

    let doc = session.doc().unwrap().doc.clone();
    let source = Arc::downgrade(doc.stories.get(&sid).unwrap());
    let document = Arc::downgrade(&doc);
    let styles = Arc::downgrade(&doc.styles);
    let old_cache = Arc::downgrade(&session.cache);
    let job = Job { token: 1, doc, cache: session.cache.clone(), placed: vec![], w: 1, h: 1, view: Affine::IDENTITY, opts: RenderOptions::default() };
    session.execute("file.close", &json!({})).unwrap();
    assert!(session.documents().is_empty());
    assert!(!Arc::ptr_eq(&session.cache, &job.cache), "memory prerequisite: idle session must retire the old cache");
    assert!(source.upgrade().is_some() && document.upgrade().is_some() && old_cache.upgrade().is_some());

    // A late job may continue to write its retired cache. It cannot refill the idle one.
    job.cache.clear();
    let late = job.cache.get(&job.doc, sid, None);
    assert!(session.cache.composed_for(&job.doc, sid).is_none());
    drop(late);
    drop(job);
    assert!(old_cache.upgrade().is_none(), "no other owner was manufactured by this control");
    assert!(source.upgrade().is_none() && document.upgrade().is_none() && styles.upgrade().is_none());
}
