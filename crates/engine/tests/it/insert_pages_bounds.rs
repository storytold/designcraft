//! `layout.pages.insert` clamps an out-of-range `after` to the end instead of overflowing; a
//! `before` must name a page, and one that doesn't is refused without a change.
use designcraft_engine::Session;
use serde_json::json;

#[test]
fn insert_after_u64_max_appends_at_the_end() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages":2})).unwrap();
    let before: Vec<_> = s.doc().unwrap().doc.pages().map(|(_, _, p)| p.id).collect();
    let out = s.execute("layout.pages.insert", &json!({"after": u64::MAX, "count": 2})).unwrap();
    assert_eq!(out["total"], 4);
    let after: Vec<_> = s.doc().unwrap().doc.pages().map(|(_, _, p)| p.id).collect();
    assert_eq!(after.len(), 4);
    assert_eq!(after[..2], before[..], "the original pages must stay first; new pages go at the end");
    assert!(after[2..].iter().all(|id| !before.contains(id)));
}

#[test]
fn insert_before_u64_max_is_refused_without_overflow() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages":2})).unwrap();
    let undo = s.doc().unwrap().history.undo.len();
    assert!(s.execute("layout.pages.insert", &json!({"before": u64::MAX, "count": 2})).is_err());
    assert_eq!(s.doc().unwrap().doc.page_count(), 2, "no pages added");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo, "no undo step");
}
