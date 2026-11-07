//! Text Frame Options (Cmd+B) work while typing in a text frame, not only with a frame selected.

use designcraft_engine::Session;
use designcraft_engine::doc::ItemId;
use serde_json::json;

#[test]
fn text_frame_options_run_with_a_text_caret() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 300], "content": "text", "text": "Hello"})).unwrap();
    let id = ItemId(r["id"].as_u64().unwrap());
    {
        let st = s.doc().unwrap();
        assert!(st.selection.items.is_empty() && st.selection.text.is_some(), "a text caret and no frame selected");
    }
    let columns = |s: &Session| s.doc().unwrap().doc.item(id).and_then(|i| i.text_frame().map(|t| t.options.columns));
    let before = columns(&s);
    // An empty `ids` names nothing, so it does not reach the caret's frames.
    let e = s.execute("object.textFrameOptions", &json!({"ids": [], "columns": 3})).unwrap_err().to_string();
    assert!(e.contains("nothing selected"), "{e}");
    assert_eq!(columns(&s), before);
    s.execute("object.textFrameOptions", &json!({"columns": 2})).unwrap();
    assert_eq!(columns(&s), Some(2));
    assert_ne!(before, Some(2));
}
