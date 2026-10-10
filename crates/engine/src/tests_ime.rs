//! Input method (IME) composition in the Type tool: marked text previews in place, a commit is
//! one `text.insert`, a cancel leaves nothing behind, and keys wait for the IME.

use designcraft_tools::{Mods, PointerEvent, PointerKind, ToolKey};
use serde_json::json;

use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s
}

/// A text frame holding `text`, with the Type tool's caret at its end; returns the story id.
fn typing_into(s: &mut Session, text: &str) -> u64 {
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
    s.set_tool("type");
    if !text.is_empty() {
        s.execute("text.insert", &json!({"text": text, "raw": true})).unwrap();
    }
    r["story"].as_u64().unwrap()
}

fn text(s: &mut Session, sid: u64) -> String {
    s.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string()
}

fn steps(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

#[test]
fn ime_composition_shows_in_place_and_commits_as_one_step() {
    // Romaji → kana → conversion → commit, as the macOS Japanese IME sends it.
    let mut s = session();
    let v = ViewInfo::default();
    let sid = typing_into(&mut s, "曲:");
    let (before, journal) = (steps(&s), s.journal.len());
    let mut carets = Vec::new();
    for (t, r) in [("g", 1..1), ("が", 1..1), ("がg", 2..2), ("ががく", 3..3)] {
        s.tool_preedit(t, Some(r), v).unwrap();
        assert_eq!(text(&mut s, sid), format!("曲:{t}"), "marked text lays out in place");
        assert!(s.tool_composing());
        carets.push(s.tool_ime_caret(v).expect("the candidate window has a place"));
    }
    // The candidate window follows the IME's cursor along the line.
    assert!(carets.windows(2).all(|w| w[1].0.x >= w[0].0.x), "{carets:?}");
    let (top, bottom) = carets[3];
    assert!((top.x - bottom.x).abs() < 1e-6 && top.y < bottom.y, "an upright caret line in horizontal text: {top:?} {bottom:?}");
    // Conversion: the window goes to the clause being converted, here the whole marked text.
    s.tool_preedit("雅楽", Some(0..2), v).unwrap();
    assert_eq!(text(&mut s, sid), "曲:雅楽");
    assert!(s.tool_ime_caret(v).unwrap().0.x < carets[3].0.x);
    let marks = s.overlays(v);
    assert!(marks.len() >= 2, "the marked text is underlined, the clause twice: {marks:?}");
    assert_eq!((steps(&s), s.journal.len()), (before, journal), "composing records nothing");
    // The commit replaces the marked text: one undo step, one command.
    s.tool_ime_commit("雅楽", v).unwrap();
    assert!(!s.tool_composing());
    assert_eq!(text(&mut s, sid), "曲:雅楽");
    assert_eq!(steps(&s), before + 1);
    assert_eq!(s.journal[journal..], [("text.insert".to_string(), json!({"text": "雅楽"}))]);
    assert_eq!(s.doc().unwrap().selection.text.map(|t| t.focus), Some("曲:雅楽".len()));
    assert!(s.overlays(v).is_empty(), "no underline once typed");
    // macOS and Windows clear the marked text before they commit: the same result.
    s.tool_preedit("おと", Some(2..2), v).unwrap();
    s.tool_preedit("", None, v).unwrap();
    s.tool_ime_commit("音", v).unwrap();
    assert_eq!(text(&mut s, sid), "曲:雅楽音");
    assert_eq!(steps(&s), before + 2);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&mut s, sid), "曲:");
}

#[test]
fn ime_cancel_leaves_no_undo_step_and_keeps_the_text() {
    let mut s = session();
    let v = ViewInfo::default();
    let sid = typing_into(&mut s, "雅楽");
    let before = steps(&s);
    let doc = s.doc().unwrap().doc.clone();
    s.tool_preedit("えんそう", Some(4..4), v).unwrap();
    assert_eq!(text(&mut s, sid), "雅楽えんそう");
    // Escape in the IME: the marked text goes and the document is the one before.
    s.tool_preedit("", None, v).unwrap();
    assert!(!s.tool_composing());
    assert!(s.doc().unwrap().interaction.is_none(), "no gesture left open");
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &doc));
    assert_eq!(steps(&s), before);
    // A stray clear (no composition) never deletes the selected text.
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 3})).unwrap();
    s.tool_preedit("", None, v).unwrap();
    assert_eq!(text(&mut s, sid), "雅楽");
    // A composition over a selection replaces it; cancelling it brings the selection back.
    s.tool_preedit("が", Some(1..1), v).unwrap();
    assert_eq!(text(&mut s, sid), "が楽");
    s.tool_preedit("", None, v).unwrap();
    assert_eq!(text(&mut s, sid), "雅楽");
    assert_eq!(s.doc().unwrap().selection.text.map(|t| t.range()), Some(0..3));
    s.tool_preedit("が", Some(1..1), v).unwrap();
    s.tool_ime_commit("我", v).unwrap();
    assert_eq!(text(&mut s, sid), "我楽");
    assert_eq!(steps(&s), before + 1);
}

#[test]
fn keys_belong_to_the_ime_while_it_composes() {
    let mut s = session();
    let v = ViewInfo::default();
    let sid = typing_into(&mut s, "");
    s.tool_preedit("しょうこ", Some(4..4), v).unwrap();
    // Backspace, arrows, Enter and Escape edit the composition in the IME, not the story.
    for k in [ToolKey::Backspace, ToolKey::Left, ToolKey::Enter, ToolKey::Escape] {
        assert!(s.tool_key(k, Mods::default(), v).unwrap(), "{k:?} is the IME's");
    }
    assert_eq!(text(&mut s, sid), "しょうこ");
    assert!(s.tool_composing());
    assert_eq!(s.tool_id(), "type", "Escape didn't leave the Type tool");
    // An Undo that comes anyway (a script, the control channel) ends the composition first: the
    // marked text becomes typing, which Undo takes back.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(!s.tool_composing());
    assert_eq!(text(&mut s, sid), "");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(text(&mut s, sid), "しょうこ");
    // A click keeps the marked text as typed.
    s.tool_preedit("ひちりき", Some(4..4), v).unwrap();
    let at = s.layout().to_canvas(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Point::new(100.0, 150.0));
    s.pointer(&PointerEvent::new(PointerKind::Down, at.x, at.y), v).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, at.x, at.y), v).unwrap();
    assert!(!s.tool_composing());
    assert_eq!(text(&mut s, sid), "しょうこひちりき");
}

#[test]
fn an_ime_commit_takes_the_format_chosen_at_the_caret() {
    let mut s = session();
    let v = ViewInfo::default();
    let sid = typing_into(&mut s, "A");
    s.execute("type.char", &json!({"attrs": {"size": 24}})).unwrap();
    s.tool_preedit("にほん", Some(3..3), v).unwrap();
    s.tool_ime_commit("日本", v).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 1, "focus": 7})).unwrap();
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 24.0);
}

#[test]
fn a_cancelled_gesture_puts_back_the_revision_and_leaves_a_saved_document_saved() {
    let mut s = session();
    let v = ViewInfo::default();
    let sid = typing_into(&mut s, "雅");
    // As if just saved.
    let st = s.doc_mut().unwrap();
    st.saved_doc = st.doc.clone();
    let before = st.revision;
    let mut seen = vec![before];
    // A cancelled composition…
    for t in ["が", "がく"] {
        s.tool_preedit(t, None, v).unwrap();
        seen.push(s.doc().unwrap().revision);
    }
    s.tool_preedit("", None, v).unwrap();
    assert_eq!(s.doc().unwrap().revision, before);
    assert!(!s.doc().unwrap().is_dirty());
    // …and any other cancelled gesture.
    let preview = |t: &str| designcraft_tools::Action::Preview("text.insert".into(), json!({"text": t, "raw": true}));
    s.run_actions(vec![designcraft_tools::Action::Begin("Type".into()), preview("x")]).unwrap();
    seen.push(s.doc().unwrap().revision);
    s.run_actions(vec![preview("xy"), designcraft_tools::Action::Cancel]).unwrap();
    assert_eq!(s.doc().unwrap().revision, before);
    assert!(!s.doc().unwrap().is_dirty());
    assert_eq!(text(&mut s, sid), "雅");
    // The next change gets a revision no preview had: caches keyed by revision never show a
    // cancelled preview.
    s.execute("text.insert", &json!({"text": "楽"})).unwrap();
    let after = s.doc().unwrap().revision;
    assert!(!seen.contains(&after), "{after} in {seen:?}");
    assert!(s.doc().unwrap().is_dirty());
}
