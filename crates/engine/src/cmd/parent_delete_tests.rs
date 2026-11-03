//! Regression coverage for deleting parent spreads through the command boundary.

use std::sync::Arc;

use designcraft_doc::{ItemId, Selection, SpreadId, TextSel};
use serde_json::{Value, json};

use crate::Session;

fn frame(s: &mut Session, parent: usize, text: Option<&str>) -> ItemId {
    let mut p = json!({"spread": {"kind": "parent", "index": parent}, "rect": [36, 36, 200, 100]});
    if let Some(text) = text {
        p["content"] = json!("text");
        p["text"] = json!(text);
    }
    ItemId(s.execute("frame.create", &p).unwrap()["id"].as_u64().unwrap())
}

#[test]
fn parent_delete_rejects_invalid_ids_without_editing_or_adding_history() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("layout.parents.edit", &json!({"on": true})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let history = s.doc().unwrap().history.undo.len();
    for params in [json!({}), json!({"id": null}), json!({"id": "A"}), json!({"id": -1}), json!({"id": 999999})] {
        assert!(s.execute("layout.parents.delete", &params).is_err(), "{params}");
        let st = s.doc().unwrap();
        assert!(Arc::ptr_eq(&st.doc, &before));
        assert_eq!(st.history.undo.len(), history);
        assert!(st.editing_parents);
    }
}

#[test]
fn parent_delete_preserves_local_copies_cleans_references_and_roundtrips_undo() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
    let a = s.doc().unwrap().doc.parents[0].id;
    let text = frame(&mut s, 0, Some("Parent folio"));
    let story = s.doc().unwrap().doc.item(text).unwrap().text_frame().unwrap().story;
    let shape = frame(&mut s, 0, None);
    let group = ItemId(s.execute("object.group", &json!({"ids": [text.0, shape.0]})).unwrap()["id"].as_u64().unwrap());
    let local = ItemId(s.execute("layout.overrideParentItems", &json!({"page": 0})).unwrap()["ids"][0].as_u64().unwrap());
    let local_text = s.doc().unwrap().doc.item(local).unwrap().children().iter().find(|i| i.text_frame().is_some()).unwrap().id;
    let local_story = s.doc().unwrap().doc.item(local_text).unwrap().text_frame().unwrap().story;
    assert_ne!(story, local_story);

    let b = SpreadId(s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap()["id"].as_u64().unwrap());
    let c = SpreadId(s.execute("layout.parents.new", &json!({"prefix": "C"})).unwrap()["id"].as_u64().unwrap());
    let inherited = frame(&mut s, 1, Some("Local parent edit"));
    s.edit(|d, _| {
        // Imported parents can base themselves on another parent and carry local overrides.
        let bp = Arc::make_mut(&mut d.parents[1]);
        bp.parent.as_mut().unwrap().based_on = Some(a);
        bp.pages[0].parent = Some(a);
        bp.pages[0].overridden.push(text);
        Arc::make_mut(&mut d.parents[2]).parent.as_mut().unwrap().based_on = Some(b);
        d.item_mut(inherited).unwrap().overrides = Some(text);
        Ok(())
    })
    .unwrap();
    s.execute("layout.pages.applyParent", &json!({"pages": [1], "parent": "B"})).unwrap();
    let other_local = ItemId(s.execute("layout.overrideParentItems", &json!({"page": 1, "ids": [inherited.0]})).unwrap()["ids"][0].as_u64().unwrap());
    s.execute("layout.parents.edit", &json!({"on": true})).unwrap();
    s.doc_mut().unwrap().selection = Selection::text(TextSel::caret(story, 0));
    let before = s.doc().unwrap().doc.clone();
    let history = s.doc().unwrap().history.undo.len();
    before.check().unwrap();

    s.execute("layout.parents.delete", &json!({"id": a.0})).unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), history + 1);
    assert_eq!(st.selection, Selection::default());
    assert!(st.editing_parents, "other parents remain editable");
    let d = &st.doc;
    assert_eq!(d.parents.iter().map(|p| p.id).collect::<Vec<_>>(), [b, c]);
    assert_eq!(d.page(0).unwrap().parent, None);
    assert_eq!(d.page(1).unwrap().parent, Some(b));
    assert_eq!(d.page(2).unwrap().parent, None);
    assert_eq!(d.parents[0].parent.as_ref().unwrap().based_on, None);
    assert_eq!(d.parents[1].parent.as_ref().unwrap().based_on, Some(b));
    assert_eq!(d.parents[0].pages[0].parent, None);
    assert!(d.parents[0].pages[0].overridden.is_empty());
    assert!([group, text, shape].iter().all(|id| d.item(*id).is_none()));
    assert!(d.story(story).is_none(), "nested text frames leave no dangling story");
    assert_eq!(d.item(local).unwrap().overrides, None);
    assert_eq!(d.story(local_story).unwrap().text, "Parent folio");
    assert!(d.page(0).unwrap().overridden.is_empty());
    assert_eq!(d.item(inherited).unwrap().overrides, None);
    assert_eq!(d.item(other_local).unwrap().overrides, Some(inherited));
    assert_eq!(d.page(1).unwrap().overridden, [inherited]);
    d.check().unwrap();
    let after = d.clone();

    s.execute("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
    s.doc().unwrap().doc.check().unwrap();
    s.execute("edit.redo", &Value::Null).unwrap();
    assert_eq!(s.doc().unwrap().doc, after);
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn parent_delete_last_parent_returns_to_document_pages() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 2})).unwrap();
    let id = s.doc().unwrap().doc.parents[0].id;
    s.execute("layout.parents.edit", &json!({"on": true})).unwrap();
    frame(&mut s, 0, Some("Last parent"));
    let selection_before = s.doc().unwrap().selection.clone();
    assert!(selection_before.text.is_some(), "creating the text frame leaves a caret");
    s.execute("layout.parents.delete", &json!({"id": id.0})).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.parents.is_empty());
    assert!(!st.editing_parents);
    assert_eq!(st.selection, Selection::default());
    assert_eq!(st.doc.page_count(), 2);
    assert!(st.doc.spreads.iter().flat_map(|sp| &sp.pages).all(|page| page.parent.is_none()));
    s.execute("edit.undo", &Value::Null).unwrap();
    assert_eq!(s.doc().unwrap().doc.parents[0].id, id);
    assert_eq!(s.doc().unwrap().selection, selection_before);
    s.execute("edit.redo", &Value::Null).unwrap();
    assert!(s.doc().unwrap().doc.parents.is_empty());
    assert!(!s.doc().unwrap().editing_parents);
    assert_eq!(s.doc().unwrap().selection, Selection::default());
    // A parent-free document still supports creating another parent.
    s.execute("layout.parents.new", &json!({"prefix": "B"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.parents.len(), 1);
    s.doc().unwrap().doc.check().unwrap();
}
