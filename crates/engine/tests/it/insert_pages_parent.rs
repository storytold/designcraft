//! `layout.pages.insert` without a `parent` gives the new pages the parent of the page they
//! follow, [None] included, instead of always the first parent (#2788).
use designcraft_engine::Session;
use serde_json::json;

fn parents(s: &Session) -> Vec<Option<u64>> {
    s.doc().unwrap().doc.pages().map(|(_, _, p)| p.parent.map(|id| id.0)).collect()
}

#[test]
fn a_page_added_after_a_page_without_a_parent_has_none() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2})).unwrap();
    let a = s.doc().unwrap().doc.parents.first().map(|p| p.id.0).unwrap();
    s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": null})).unwrap();
    assert_eq!(parents(&s), [Some(a), None]);
    // As the Pages panel's Create new page button does: after the current page, no parent.
    s.execute("layout.pages.insert", &json!({"after": 1, "count": 1})).unwrap();
    assert_eq!(parents(&s), [Some(a), None, None], "the new page continues the page it follows");
    s.execute("layout.pages.insert", &json!({"after": 0, "count": 1})).unwrap();
    assert_eq!(parents(&s), [Some(a), Some(a), None, None]);
}

#[test]
fn pages_inserted_at_the_start_take_the_first_page_s_parent() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2})).unwrap();
    let a = s.doc().unwrap().doc.parents.first().map(|p| p.id.0).unwrap();
    s.execute("layout.pages.applyParent", &json!({"pages": [0], "parent": null})).unwrap();
    s.execute("layout.pages.insert", &json!({"before": 0, "count": 1})).unwrap();
    assert_eq!(parents(&s), [None, None, Some(a)]);
}

#[test]
fn a_parent_given_still_wins() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    s.execute("layout.pages.applyParent", &json!({"pages": [0], "parent": null})).unwrap();
    let a = s.doc().unwrap().doc.parents.first().map(|p| p.id.0).unwrap();
    s.execute("layout.pages.insert", &json!({"after": 0, "count": 1, "parent": a})).unwrap();
    assert_eq!(parents(&s), [None, Some(a)]);
}
