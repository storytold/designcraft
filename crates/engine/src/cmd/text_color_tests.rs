//! Cross-command caret color and character-style regressions.
use crate::Session;
use designcraft_doc::StoryId;
use serde_json::{Value, json};

fn setup(text: &str) -> (Session, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect": [36,36,300,200], "content":"text", "text":text})).unwrap();
    let sid = f["story"].as_u64().unwrap();
    s.execute("text.select", &json!({"story":sid,"anchor":text.len()})).unwrap();
    (s, sid)
}
fn attrs(s: &mut Session) -> Value {
    s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"].clone()
}
#[test]
fn integration_color_character_style_captures_pending_and_null() {
    let (mut s, sid) = setup("hello");
    s.execute("type.char", &json!({"size":30,"tracking":40,"fill":"[Paper]"})).unwrap();
    s.execute("style.character.create", &json!({"name":"Pending","fromSelection":true,"chars":{"tracking":null}})).unwrap();
    let a = &s.doc().unwrap().doc.styles.char_style("Pending").unwrap().chars;
    assert_eq!(a.size, Some(30.0));
    assert_eq!(a.fill.as_deref(), Some("[Paper]"));
    assert_eq!(a.tracking, None);
    assert_eq!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().text, "hello");
}
#[test]
fn integration_color_pending_follows_promoted_swatch() {
    for target in ["fill", "stroke"] {
        let (mut s, sid) = setup("hello");
        let old = s.execute("object.color", &json!({"color":"#123456","target":target})).unwrap()["swatch"].clone();
        s.execute("swatch.addToSwatches", &json!({"swatch":old,"name":"Brand"})).unwrap();
        assert_eq!(attrs(&mut s)[target], "Brand");
        s.execute("text.insert", &json!({"text":"X"})).unwrap();
        let st = s.doc().unwrap().doc.story(StoryId(sid)).unwrap();
        let f = &st.format_after(5).over;
        assert_eq!(if target == "fill" { f.fill.as_deref() } else { f.stroke.as_deref() }, Some("Brand"));
    }
}
#[test]
fn integration_color_undo_discards_missing_pending_swatch() {
    let (mut s, sid) = setup("hello");
    let old = s.execute("object.color", &json!({"color":"#123456"})).unwrap()["swatch"].as_str().unwrap().to_string();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.swatch(&old).is_none());
    assert_ne!(attrs(&mut s)["fill"], old);
    s.execute("text.insert", &json!({"text":"X"})).unwrap();
    assert_ne!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().format_after(5).over.fill.as_deref(), Some(old.as_str()));
}
#[test]
fn integration_color_empty_story_uses_one_undo_step() {
    let (mut s, sid) = setup("");
    let before = serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap();
    let n = s.doc().unwrap().history.undo.len();
    s.execute("object.color", &json!({"color":"#123456"})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1);
    let colored = serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap(), before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap(), colored);
    s.execute("text.insert", &json!({"text":"X"})).unwrap();
    assert!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().format_after(0).over.fill.is_some());
}

#[test]
fn integration_color_swap_resolves_each_paragraph_and_keeps_unselected_text() {
    let (mut s, sid) = setup("ab\ncd");
    for (name, fill, stroke, ft, st, start, end) in
        [("First", "[Paper]", "[Black]", 0.2, 0.8, 0, 2), ("Second", "[Black]", "[Paper]", 0.4, 0.6, 3, 5)]
    {
        s.execute("style.paragraph.create", &json!({"name":name,"chars":{"fill":fill,"stroke":stroke,"fillTint":ft,"strokeTint":st}})).unwrap();
        s.execute("text.select", &json!({"story":sid,"anchor":start,"focus":end})).unwrap();
        s.execute("style.paragraph.apply", &json!({"name":name})).unwrap();
    }
    s.execute("text.select", &json!({"story":sid,"anchor":1,"focus":4})).unwrap();
    let before = serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap();
    let n = s.doc().unwrap().history.undo.len();
    s.execute("type.swapFillStroke", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1);
    for (pos, fill, stroke, ft, st) in [
        (0, "[Paper]", "[Black]", 0.2, 0.8),
        (1, "[Black]", "[Paper]", 0.8, 0.2),
        (2, "[Black]", "[Paper]", 0.8, 0.2),
        (3, "[Paper]", "[Black]", 0.6, 0.4),
        (4, "[Black]", "[Paper]", 0.4, 0.6),
    ] {
        s.execute("text.select", &json!({"story":sid,"anchor":pos,"focus":pos+1})).unwrap();
        let a = attrs(&mut s);
        assert_eq!(a["fill"], fill);
        assert_eq!(a["stroke"], stroke);
        assert!((a["fillTint"].as_f64().unwrap() - ft).abs() < 1e-6);
        assert!((a["strokeTint"].as_f64().unwrap() - st).abs() < 1e-6);
    }
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap(), before);
}

#[test]
fn integration_color_swap_caret_applies_only_to_next_typing() {
    let (mut s, sid) = setup("hello");
    s.execute("type.char", &json!({"fill":"[Paper]","stroke":"[Black]","fillTint":0.2,"strokeTint":0.8})).unwrap();
    let before = serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap();
    s.execute("type.swapFillStroke", &json!({})).unwrap();
    assert_eq!(serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap(), before);
    let a = attrs(&mut s);
    assert_eq!(a["fill"], "[Black]");
    assert_eq!(a["stroke"], "[Paper]");
    s.execute("text.insert", &json!({"text":"X"})).unwrap();
    let st = s.doc().unwrap().doc.story(StoryId(sid)).unwrap();
    assert_eq!(st.text, "helloX");
    assert_eq!(st.format_after(0).over.fill, None);
    assert_eq!(st.format_after(5).over.stroke.as_deref(), Some("[Paper]"));
    assert_eq!(st.format_after(5).over.fill_tint, Some(0.8));
}

#[test]
fn integration_color_swap_selected_empty_cells_keeps_each_format() {
    let (mut s, sid) = setup("hello");
    let result = s.execute("table.insert", &json!({"rows":1,"cols":2})).unwrap();
    let tid = result["table"].as_u64().unwrap();
    s.edit(|d, _| {
        let table = std::sync::Arc::make_mut(d.story_mut(StoryId(sid)).unwrap().tables.get_mut(&tid).unwrap());
        for (c, fill, stroke) in [(0, "[Paper]", "[Black]"), (1, "[Black]", "[Paper]")] {
            let attrs = &mut table.cell_mut(0, c).unwrap().text.chars[0].format.over;
            attrs.fill = Some(fill.into());
            attrs.stroke = Some(stroke.into());
        }
        Ok(())
    })
    .unwrap();
    s.execute("table.select", &json!({"story":sid,"table":tid,"what":"table"})).unwrap();
    let before = serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap();
    s.execute("type.swapFillStroke", &json!({})).unwrap();
    let table = &s.doc().unwrap().doc.story(StoryId(sid)).unwrap().tables[&tid];
    assert_eq!(table.cell(0, 0).unwrap().text.chars[0].format.over.fill.as_deref(), Some("[Black]"));
    assert_eq!(table.cell(0, 1).unwrap().text.chars[0].format.over.fill.as_deref(), Some("[Paper]"));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(serde_json::to_value(s.doc().unwrap().doc.as_ref()).unwrap(), before);
}

#[test]
fn integration_color_failed_promotion_preserves_pending_and_history() {
    let (mut s, _) = setup("hello");
    s.execute("swatch.create", &json!({"name":"Brand","color":"#654321"})).unwrap();
    let old = s.execute("object.color", &json!({"color":"#123456"})).unwrap()["swatch"].clone();
    let before = s.doc().unwrap().clone();
    let pending = attrs(&mut s);
    assert!(s.execute("swatch.addToSwatches", &json!({"swatch":old,"name":"Brand"})).is_err());
    assert_eq!(attrs(&mut s), pending);
    assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().doc, &before.doc));
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len());
}

#[test]
fn integration_color_promote_undo_redo_never_reuses_a_missing_pending_name() {
    let (mut s, _) = setup("hello");
    let old = s.execute("object.color", &json!({"color":"#123456"})).unwrap()["swatch"].clone();
    s.execute("swatch.addToSwatches", &json!({"swatch":old,"name":"Brand"})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let fill = attrs(&mut s)["fill"].as_str().unwrap().to_string();
    assert!(s.doc().unwrap().doc.swatch(&fill).is_some());
    s.execute("edit.redo", &json!({})).unwrap();
    let fill = attrs(&mut s)["fill"].as_str().unwrap().to_string();
    assert!(s.doc().unwrap().doc.swatch(&fill).is_some());
}

#[test]
fn integration_color_swap_validates_new_fill_before_any_edit() {
    let (mut s, sid) = setup("hello");
    s.execute("text.select", &json!({"story":sid,"anchor":0,"focus":5})).unwrap();
    s.execute("type.char", &json!({"fill":"[Paper]","stroke":"Missing Ink"})).unwrap();
    let before = s.doc().unwrap().clone();
    let journal = s.journal.clone();
    let error = s.execute("type.swapFillStroke", &json!({})).unwrap_err().to_string();
    assert!(error.contains("Missing Ink"), "{error}");
    assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().doc, &before.doc));
    assert_eq!(s.doc().unwrap().history.undo.len(), before.history.undo.len());
    assert_eq!(s.journal, journal);
}
