use designcraft_engine::Session;
use serde_json::json;

#[test]
fn choosing_a_baseline_explicitly_adopts_current_metrics_and_undo_restores_legacy() {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let result = session.execute("frame.create", &json!({"rect": [10, 10, 200, 200], "content": "text"})).unwrap();
    let id = designcraft_doc::ItemId(result["id"].as_u64().unwrap());
    session.execute("object.textFrameOptions", &json!({"ids":[id.0], "legacyFirstBaseline":true})).unwrap();
    session.execute("object.textFrameOptions", &json!({"ids": [id.0], "inset": 2, "firstBaseline": "ascent"})).unwrap();
    assert!(
        session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.legacy_first_baseline,
        "unrelated frame edits preserve layout compatibility"
    );
    session.execute("object.textFrameOptions", &json!({"ids": [id.0], "firstBaseline": "capHeight"})).unwrap();
    assert!(!session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.legacy_first_baseline);
    session.execute("edit.undo", &json!({})).unwrap();
    assert!(session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.legacy_first_baseline);
}

#[test]
fn legacy_recovery_is_explicit_reversible_and_rejects_non_boolean_flags() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("frame.create", &json!({"rect":[10,10,200,200],"content":"text"})).unwrap()["id"].as_u64().unwrap();
    let flag = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().text_frame().unwrap().options.legacy_first_baseline;
    assert!(!flag(&s));
    assert!(s.execute("object.textFrameOptions", &json!({"ids":[id],"legacyFirstBaseline":"true"})).is_err());
    assert!(!flag(&s));
    s.execute("object.textFrameOptions", &json!({"ids":[id],"legacyFirstBaseline":true})).unwrap();
    assert!(flag(&s));
    assert_eq!(s.execute("document.inspect", &json!({})).unwrap()["spreads"][0]["items"][0]["legacyFirstBaseline"], true);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!flag(&s));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(flag(&s));
    s.execute("object.textFrameOptions", &json!({"ids":[id],"legacyFirstBaseline":false})).unwrap();
    assert!(!flag(&s));
}
