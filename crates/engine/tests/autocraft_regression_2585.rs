//! Find/Change with document scope also searches and changes footnote text.
use designcraft_engine::Session;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let frame = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "The body mentions a heron."})).unwrap();
    let sid = frame["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 25, "focus": 25})).unwrap();
    s.execute("footnote.insert", &json!({"text": "The footnote mentions a zebra and a heron."})).unwrap();
    s
}

fn note_text(s: &mut Session) -> String {
    let notes = s.execute("footnote.list", &json!({})).unwrap();
    notes[0]["text"].as_str().unwrap().to_string()
}

#[test]
fn find_and_change_reach_footnote_text() {
    let mut s = session();

    let zebra = s.execute("find.find", &json!({"find": "zebra", "scope": "document"})).unwrap();
    assert_eq!(zebra.as_array().unwrap().len(), 1, "a word only in a footnote is found: {zebra}");

    let heron = s.execute("find.find", &json!({"find": "heron", "scope": "document"})).unwrap();
    assert_eq!(heron.as_array().unwrap().len(), 2, "a word in body and footnote is found in both: {heron}");

    let r = s.execute("find.change", &json!({"find": "zebra", "change": "okapi", "scope": "document"})).unwrap();
    assert_eq!(r["count"], 1);
    assert!(note_text(&mut s).contains("okapi"), "the footnote was changed: {}", note_text(&mut s));
    assert!(!note_text(&mut s).contains("zebra"));

    let r = s.execute("find.change", &json!({"find": "heron", "change": "egret", "scope": "document"})).unwrap();
    assert_eq!(r["count"], 2);
    let after = s.execute("find.find", &json!({"find": "heron", "scope": "document"})).unwrap();
    assert!(after.as_array().unwrap().is_empty(), "{after}");
}
