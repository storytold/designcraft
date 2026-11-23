use designcraft_doc::{ItemId, OBJECT_MARK, StoryId};
use serde_json::json;

use crate::Session;

fn text_frame(s: &mut Session, text: &str) -> StoryId {
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 236, 136], "content": "text", "text": text})).unwrap();
    StoryId(r["story"].as_u64().unwrap())
}

fn copy_formatted_text(s: &mut Session) -> StoryId {
    let source = text_frame(s, "old text");
    let len = s.doc().unwrap().doc.story(source).unwrap().len();
    s.execute("text.select", &json!({"story": source.0, "anchor": 0, "focus": len})).unwrap();
    s.execute("type.bold", &json!({})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    source
}

#[test]
fn object_cut_replaces_old_text_copy_at_a_body_caret_and_survives_undo_redo() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let source = copy_formatted_text(&mut s);
    let target = text_frame(&mut s, "target");
    let cut = s.execute("frame.create", &json!({"rect": [36, 200, 86, 260]})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.set", &json!({"ids": [cut]})).unwrap();
    let before_cut = s.doc().unwrap().doc.clone();
    let original = before_cut.item(ItemId(cut)).unwrap();
    let bounds = original.bounds();
    s.execute("edit.cut", &json!({})).unwrap();
    let after_cut = s.doc().unwrap().doc.clone();
    assert!(after_cut.item(ItemId(cut)).is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before_cut);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after_cut);

    s.execute("text.select", &json!({"story": target.0, "anchor": 0, "focus": 0})).unwrap();
    // Native Paste removes its unchanged U+FFFC clipboard marker before engine execution.
    s.execute("edit.paste", &json!({})).unwrap();
    let after_paste = s.doc().unwrap().doc.clone();
    let list = s.execute("anchored.list", &json!({"story": target.0})).unwrap();
    let anchors = list.as_array().unwrap();
    assert_eq!(anchors.len(), 1, "Cut must paste the shape, not the earlier text copy");
    let pos = anchors[0]["pos"].as_u64().unwrap() as usize;
    let body = after_paste.story(target).unwrap();
    let anchored = body.object_at(pos).unwrap();
    assert_eq!(body.text, format!("{OBJECT_MARK}{}", after_cut.story(target).unwrap().text));
    assert!((anchored.size().0 - bounds.width()).abs() < 1e-6);
    assert!((anchored.size().1 - bounds.height()).abs() < 1e-6);
    assert_eq!(anchored.item.path, original.path);
    assert_eq!(anchored.item.content, original.content);
    assert_ne!(anchored.item.id, original.id, "Paste allocates a fresh anchored identity");
    assert_eq!(after_paste.story(source), before_cut.story(source));
    after_paste.check().unwrap();

    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after_cut);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, after_paste);
}

#[test]
fn rejected_object_cut_keeps_the_previous_formatted_text_copy_and_redo() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let source = copy_formatted_text(&mut s);
    let target = text_frame(&mut s, "target");
    s.execute("frame.create", &json!({"rect": [36, 200, 86, 260]})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.deselectAll", &json!({})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let selection = s.doc().unwrap().selection.clone();
    let history = s.execute("document.history", &json!({})).unwrap();
    assert!(!history["redo"].as_array().unwrap().is_empty());
    assert!(s.execute("edit.cut", &json!({})).is_err());
    assert_eq!(s.doc().unwrap().doc, before);
    assert_eq!(s.doc().unwrap().selection, selection);
    assert_eq!(s.execute("document.history", &json!({})).unwrap(), history);

    s.execute("text.select", &json!({"story": target.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let d = &s.doc().unwrap().doc;
    let copied = before.story(source).unwrap();
    let pasted = d.story(target).unwrap();
    assert_eq!(pasted.text, format!("{}{}", copied.text, before.story(target).unwrap().text));
    assert_eq!(pasted.char_format_at(1), copied.char_format_at(1));
    assert!(pasted.objects.is_empty());
    d.check().unwrap();
}
