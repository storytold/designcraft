//! `table.*` commands must resolve a table from `story` alone, without a text caret (#1625).
use designcraft_engine::Session;
use serde_json::json;

#[test]
fn table_commands_resolve_a_table_from_the_story_without_a_caret() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [0, 0, 400, 300], "content": "text", "text": ""})).unwrap();
    let story = r["story"].clone();
    s.execute("text.select", &json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
    s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
    // Headless: no caret, no selection.
    s.execute("edit.deselectAll", &json!({})).unwrap();

    let got = s.execute("table.getCellStroke", &json!({"story": story})).unwrap();
    assert!(got["edges"].as_u64().unwrap() > 0, "{got}");

    let sel = s.execute("table.select", &json!({"story": story, "what": "table"})).unwrap();
    assert_eq!(sel["story"], story, "{sel}");

    s.execute("edit.deselectAll", &json!({})).unwrap();
    s.execute("table.setCell", &json!({"story": story, "stroke": {"weight": 0, "edges": "all"}})).unwrap();
    let got = s.execute("table.getCellStroke", &json!({"story": story})).unwrap();
    assert_eq!(got["weight"].as_f64(), Some(0.0), "{got}");
}
