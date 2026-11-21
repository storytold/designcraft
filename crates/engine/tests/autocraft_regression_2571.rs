//! A numeric `perPage` or `arrange` is refused, not treated as absent.
use designcraft_engine::Session;
use designcraft_engine::cmd::base64_encode;
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"facingPages": false})).unwrap();
    let csv = "Name\nAda\nGrace\nLin\nMay\n";
    s.execute("data.source.select", &json!({"bytes": base64_encode(csv.as_bytes()), "name": "people.csv"})).unwrap();
    s.execute("frame.create", &json!({"rect": [36, 36, 100, 60], "content": "text", "text": "<<Name>>"})).unwrap();
    s
}

#[test]
fn numeric_per_page_is_a_type_error() {
    let mut s = session();
    let err = s.execute("data.merge", &json!({"perPage": 4})).unwrap_err().to_string();
    assert!(err.contains("perPage"), "{err}");
    assert_eq!(s.doc().unwrap().doc.spreads.len(), 1, "a refused merge adds no pages");
}

#[test]
fn numeric_arrange_is_a_type_error() {
    let mut s = session();
    let err = s.execute("data.merge", &json!({"perPage": "multiple", "arrange": 1})).unwrap_err().to_string();
    assert!(err.contains("arrange"), "{err}");
}

#[test]
fn string_values_still_work() {
    let mut s = session();
    assert!(s.execute("data.merge", &json!({"perPage": "bogus"})).is_err());
    s.execute("data.merge", &json!({"perPage": "multiple", "arrange": "columns"})).unwrap();
}
