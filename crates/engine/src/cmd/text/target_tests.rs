//! What `story.get`, `story.setDirection`, `type.fillWithPlaceholder` and `file.exportText` act on
//! when they are given a `story` or `frame` (#1809). An explicit id the document doesn't have is an
//! error: it used to fall back to the selection, so an edit landed on one story and an export wrote
//! another story's text.

use designcraft_doc::{ItemId, StoryId};
use serde_json::{Value, json};

use crate::Session;

/// The commands that take an explicit story target.
const COMMANDS: [&str; 4] = ["file.exportText", "story.get", "story.setDirection", "type.fillWithPlaceholder"];

/// `p` with what a command needs to act on a story rather than only to report it.
fn acting(command: &str, mut p: Value) -> Value {
    if command == "story.setDirection" {
        p["direction"] = json!("rightToLeft");
    }
    p
}

/// Two stories on one page. `selected` (its text is "Selected") is selected, `other` ("Explicit")
/// is not, so a target that falls back to the selection shows.
struct Stories {
    s: Session,
    selected: Value,
    other: Value,
}

fn two_stories() -> Stories {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let selected = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Selected"})).unwrap();
    let other = s.execute("frame.create", &json!({"rect": [0, 300, 300, 500], "content": "text", "text": "Explicit"})).unwrap();
    s.execute("selection.set", &json!({"ids": [selected["id"]]})).unwrap();
    Stories { s, selected, other }
}

/// The first few characters of `text`, for an assertion message that a whole filler text would swamp.
fn start(text: &str) -> String {
    text.chars().take(40).collect()
}

/// The text of the story `frame` names.
fn story_text(s: &Session, frame: &Value) -> String {
    let sid = StoryId(frame["story"].as_u64().unwrap());
    s.doc().unwrap().doc.story(sid).unwrap().text.clone()
}

#[test]
fn a_target_the_document_does_not_have_is_an_error_not_the_selection() {
    for command in COMMANDS {
        let d = two_stories();
        let cases = [
            (json!({"story": 999999u64}), "no such story 999999"),
            (json!({"frame": 999999u64}), "no such frame 999999"),
            (json!({"story": d.selected["story"].clone(), "frame": 999999u64}), "no such frame 999999"),
            (json!({"story": 999999u64, "frame": d.selected["id"].clone()}), "no such story 999999"),
        ];
        let mut s = d.s;
        for (p, message) in cases {
            let before = s.doc().unwrap().doc.clone();
            let steps = s.doc().unwrap().history.undo.len();
            let error = s.execute(command, &acting(command, p.clone())).unwrap_err();
            assert!(error.to_string().contains(message), "{command} {p}: {error}");
            assert_eq!(s.doc().unwrap().doc, before, "{command} {p} acted on a story anyway");
            assert_eq!(s.doc().unwrap().history.undo.len(), steps, "{command} {p} left an undo step");
        }
    }
}

#[test]
fn a_target_that_is_not_an_unsigned_integer_is_an_error_not_the_selection() {
    let not_ids = [json!("1"), json!("invalid"), json!(-1), json!(1.0), json!(1.5), json!(true), json!({}), json!([])];
    for command in COMMANDS {
        for key in ["story", "frame"] {
            let d = two_stories();
            let mut s = d.s;
            for not_id in &not_ids {
                // Alone, and beside the other target the command could quietly use instead.
                let usable = json!({"story": d.selected["story"].clone(), "frame": d.other["id"].clone()});
                for mut p in [json!({}), usable] {
                    p[key] = not_id.clone();
                    let before = s.doc().unwrap().doc.clone();
                    let steps = s.doc().unwrap().history.undo.len();
                    let error = s.execute(command, &acting(command, p.clone())).unwrap_err();
                    assert!(matches!(error, crate::EngineError::BadParams { .. }), "{command} {p}: {error}");
                    let message = format!("`{key}` must be an unsigned integer");
                    assert!(error.to_string().contains(&message), "{command} {p}: {error}");
                    assert_eq!(s.doc().unwrap().doc, before, "{command} {p} acted on a story anyway");
                    assert_eq!(s.doc().unwrap().history.undo.len(), steps, "{command} {p} left an undo step");
                }
            }
        }
    }
}

#[test]
fn an_explicit_target_wins_and_a_null_or_absent_one_uses_the_selection() {
    for command in COMMANDS {
        for case in ["story", "frame", "both", "null-story", "null-frame", "nulls", "absent"] {
            let d = two_stories();
            let mut s = d.s;
            let p = match case {
                "story" => json!({"story": d.other["story"]}),
                "frame" => json!({"frame": d.other["id"]}),
                "both" => json!({"story": d.other["story"], "frame": d.selected["id"]}),
                "null-story" => json!({"story": null, "frame": d.other["id"]}),
                "null-frame" => json!({"story": d.other["story"], "frame": null}),
                "nulls" => json!({"story": null, "frame": null}),
                _ => json!({}),
            };
            let explicit = !matches!(case, "nulls" | "absent");
            let (target, untouched) = if explicit { (&d.other, &d.selected) } else { (&d.selected, &d.other) };
            let untouched_before = s.doc().unwrap().doc.story(StoryId(untouched["story"].as_u64().unwrap())).unwrap().clone();
            let before = s.doc().unwrap().doc.clone();
            let steps = s.doc().unwrap().history.undo.len();
            let result = s.execute(command, &acting(command, p)).unwrap_or_else(|e| panic!("{command} {case}: {e}"));
            match command {
                "story.get" | "file.exportText" => {
                    let expected = if explicit { "Explicit" } else { "Selected" };
                    assert_eq!(result["text"], expected, "{command} {case}");
                }
                "story.setDirection" => {
                    let direction = s.execute("story.get", &json!({"story": target["story"]})).unwrap()["direction"].clone();
                    assert_eq!(direction, "rightToLeft", "{command} {case}");
                }
                _ => {
                    let filled = story_text(&s, target);
                    assert!(filled.len() > 500 && filled != "Selected" && filled != "Explicit", "{command} {case}: {}", start(&filled));
                }
            }
            assert_eq!(
                *s.doc().unwrap().doc.story(StoryId(untouched["story"].as_u64().unwrap())).unwrap(),
                untouched_before,
                "{command} {case} acted on the other story"
            );
            if matches!(command, "story.get" | "file.exportText") {
                assert_eq!(s.doc().unwrap().doc, before, "{command} {case} changed the document");
                assert_eq!(s.doc().unwrap().history.undo.len(), steps, "{command} {case} left an undo step");
            } else {
                assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1, "{command} {case} is one undo step");
                s.execute("edit.undo", &json!({})).unwrap();
                assert_eq!(s.doc().unwrap().doc, before, "undo takes {command} {case} back");
            }
        }
    }
}

#[test]
fn a_frame_that_holds_no_text_is_refused_except_by_placeholder_text() {
    for command in ["file.exportText", "story.get", "story.setDirection"] {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Selected"})).unwrap();
        let graphic = s.execute("frame.create", &json!({"rect": [0, 300, 300, 500], "content": "graphic"})).unwrap();
        s.execute("selection.set", &json!({"ids": [text["id"]]})).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("refused.txt");
        for mut p in [json!({"frame": graphic["id"]}), json!({"story": text["story"], "frame": graphic["id"]})] {
            if command == "file.exportText" {
                p["path"] = json!(path.to_string_lossy());
            }
            let before = s.doc().unwrap().doc.clone();
            let error = s.execute(command, &acting(command, p.clone())).unwrap_err();
            // The message names the case, and nothing was read, edited or written.
            assert!(error.to_string().contains(&format!("frame {} holds no text", graphic["id"])), "{command} {p}: {error}");
            assert_eq!(s.doc().unwrap().doc, before, "{command} {p} changed the document");
            assert!(!path.exists(), "{command} {p} wrote a file");
        }
    }
}

#[test]
fn export_with_a_story_and_a_frame_without_text_writes_nothing() {
    // The case that exports another story's text on pre-fix behaviour: a valid story makes the call
    // look fine, so a frame that holds no text was quietly ignored and the export went ahead.
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let text = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Selected"})).unwrap();
    let graphic = s.execute("frame.create", &json!({"rect": [0, 300, 300, 500], "content": "graphic"})).unwrap();
    s.execute("selection.set", &json!({"ids": [text["id"]]})).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wrong-story.txt");
    let result = s.execute("file.exportText", &json!({"path": path.to_string_lossy(), "story": text["story"], "frame": graphic["id"]}));
    assert!(result.is_err(), "the frame that holds no text must be refused: {result:?}");
    assert!(!path.exists(), "and nothing may be written");
}

#[test]
fn placeholder_text_converts_an_explicit_frame_that_holds_no_text() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let text = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Selected"})).unwrap();
    let graphic = s.execute("frame.create", &json!({"rect": [0, 300, 300, 900], "content": "graphic"})).unwrap();
    s.execute("selection.set", &json!({"ids": [text["id"]]})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let steps = s.doc().unwrap().history.undo.len();
    s.execute("type.fillWithPlaceholder", &json!({"frame": graphic["id"]})).unwrap();
    let converted = ItemId(graphic["id"].as_u64().unwrap());
    let sid = s.doc().unwrap().doc.item(converted).and_then(|i| i.text_frame().map(|t| t.story));
    let filled = sid.and_then(|sid| s.doc().unwrap().doc.story(sid).cloned());
    let filled = filled.expect("the frame became a text frame");
    assert!(filled.text.len() > 500 && filled.text != "Selected", "the converted frame is filled: {}", start(&filled.text));
    assert_eq!(story_text(&s, &text), "Selected", "the selection is untouched");
    assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1, "converting and filling is one undo step");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc, before);
}

#[test]
fn placeholder_text_fills_an_explicit_story_and_leaves_a_textless_frame_alone() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let selected = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Selected"})).unwrap();
    let other = s.execute("frame.create", &json!({"rect": [0, 300, 300, 900], "content": "text", "text": "Explicit"})).unwrap();
    let graphic = s.execute("frame.create", &json!({"rect": [400, 0, 700, 200], "content": "graphic"})).unwrap();
    s.execute("selection.set", &json!({"ids": [selected["id"]]})).unwrap();
    s.execute("type.fillWithPlaceholder", &json!({"story": other["story"], "frame": graphic["id"]})).unwrap();
    let filled = story_text(&s, &other);
    assert!(filled.len() > 500 && filled != "Explicit", "the story the call names is filled: {}", start(&filled));
    assert_eq!(story_text(&s, &selected), "Selected", "the selection is untouched");
    let left = ItemId(graphic["id"].as_u64().unwrap());
    assert!(
        s.doc().unwrap().doc.item(left).and_then(|i| i.text_frame()).is_none(),
        "a frame that holds no text stays as it is when a story is named"
    );
}

#[test]
fn placeholder_text_fills_the_story_of_a_caret_with_no_frame() {
    // `text.select` leaves `Selection::text` with `frame: None`, so a caret names no frame at all.
    for p in [json!({}), json!({"story": null, "frame": null}), json!({"story": null}), json!({"frame": null})] {
        let d = two_stories();
        let mut s = d.s;
        s.execute("text.select", &json!({"story": d.selected["story"], "anchor": 0})).unwrap();
        let (items, frame) = {
            let st = s.doc().unwrap();
            (st.selection.items.len(), st.selection.text.and_then(|t| t.frame))
        };
        assert_eq!(items, 0, "the caret replaces the object selection");
        assert!(frame.is_none(), "and names no frame");
        let other = story_text(&s, &d.other);
        let steps = s.doc().unwrap().history.undo.len();
        s.execute("type.fillWithPlaceholder", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let filled = story_text(&s, &d.selected);
        assert!(filled.len() > 500 && filled != "Selected", "{p}: the caret's story is filled: {}", start(&filled));
        assert_eq!(story_text(&s, &d.other), other, "{p}: the other story is untouched");
        assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1, "{p}: one undo step");
    }
}

#[test]
fn placeholder_text_checks_its_target_before_converting_a_frame() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let text = s.execute("frame.create", &json!({"rect": [0, 0, 300, 200], "content": "text", "text": "Selected"})).unwrap();
    let empty = s.execute("frame.create", &json!({"rect": [0, 300, 300, 500], "content": "unassigned"})).unwrap();
    s.execute("selection.set", &json!({"ids": [text["id"]]})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let steps = s.doc().unwrap().history.undo.len();
    // The frame could become a text frame, but the story the call names does not exist: refusing
    // leaves the frame unassigned rather than converting it and then failing.
    let error = s.execute("type.fillWithPlaceholder", &json!({"frame": empty["id"], "story": 999999u64})).unwrap_err();
    assert_eq!(s.doc().unwrap().doc, before, "the frame must not be converted");
    assert_eq!(s.doc().unwrap().history.undo.len(), steps, "and no step may be left behind");
    assert!(error.to_string().contains("no such story 999999"), "{error}");
}
