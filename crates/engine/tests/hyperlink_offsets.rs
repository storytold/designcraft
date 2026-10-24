//! Hyperlink text source range adjustments across text editing (#1219).

use designcraft_doc::{HyperlinkSource, StoryId};
use designcraft_engine::Session;
use serde_json::json;

fn fixture(text: &str) -> (Session, StoryId, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 500, 700], "content": "text", "text": text})).unwrap();
    let sid = StoryId(r["story"].as_u64().unwrap());
    (s, sid, r["id"].as_u64().unwrap())
}

fn text_hyperlink_range(s: &Session, hid: u64) -> (usize, usize) {
    let doc = &s.doc().unwrap().doc;
    let link = doc.hyperlinks.iter().find(|h| h.id == hid).expect("hyperlink exists");
    match link.source {
        HyperlinkSource::Text { start, end, .. } => (start, end),
        _ => panic!("expected text hyperlink source"),
    }
}

#[test]
fn typing_before_hyperlink_shifts_source_range() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    // "page 2" is at bytes 17..23
    assert_eq!(&initial[17..23], "page 2");
    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();

    assert_eq!(text_hyperlink_range(&s, hid), (17, 23));

    // Type "ABC " at the start of the story (offset 0)
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": "ABC ", "raw": true})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "ABC Hello world. See page 2 for details.");

    // The hyperlink must now be at bytes 21..27, covering "page 2"
    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (21, 27));
    assert_eq!(&text[start..end], "page 2");
}

#[test]
fn multibyte_utf8_inserted_before_hyperlink_shifts_by_byte_length() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();

    // Insert soft hyphen U+00AD (2 UTF-8 bytes) at offset 0
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": "\u{00AD}", "raw": true})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (19, 25));
    assert_eq!(&text[start..end], "page 2");

    // Insert German umlaut word "Größe " (8 UTF-8 bytes: G, r, ö (2B), ß (2B), e, space) at offset 0
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": "Größe ", "raw": true})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (27, 33));
    assert_eq!(&text[start..end], "page 2");
}

#[test]
fn typing_inside_hyperlink_expands_source_range() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();

    // Type "XYZ " inside the link between "page " and "2" (offset 22)
    s.execute("text.select", &json!({"story": sid.0, "anchor": 22, "focus": 22})).unwrap();
    s.execute("text.insert", &json!({"text": "XYZ ", "raw": true})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Hello world. See page XYZ 2 for details.");

    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (17, 27));
    assert_eq!(&text[start..end], "page XYZ 2");
}

#[test]
fn deleting_before_and_across_hyperlink() {
    let initial = "ABC Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    // "page 2" is at bytes 21..27
    assert_eq!(&initial[21..27], "page 2");
    s.execute("text.select", &json!({"story": sid.0, "anchor": 21, "focus": 27})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();

    // Delete "ABC " (4 bytes) before the link
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 4})).unwrap();
    s.execute("text.delete", &json!({})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Hello world. See page 2 for details.");

    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (17, 23));
    assert_eq!(&text[start..end], "page 2");

    // Delete across the link: from offset 13 ("See ") through 22 ("page")
    s.execute("text.select", &json!({"story": sid.0, "anchor": 13, "focus": 22})).unwrap();
    s.execute("text.delete", &json!({})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Hello world. 2 for details.");

    // "2" survived the deletion, starting at offset 13
    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (13, 14));
    assert_eq!(&text[start..end], "2");
}

#[test]
fn deleting_entire_hyperlink_removes_it() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();

    // Select text encompassing the entire link (13..27: "See page 2 for") and delete
    s.execute("text.select", &json!({"story": sid.0, "anchor": 13, "focus": 27})).unwrap();
    s.execute("text.delete", &json!({})).unwrap();

    let doc = &s.doc().unwrap().doc;
    assert!(!doc.hyperlinks.iter().any(|h| h.id == hid), "hyperlink should be pruned when text is deleted");
}

#[test]
fn replacing_hyperlink_text_updates_source_range() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();

    // Select "page 2" and replace with "chapter 5" (9 bytes)
    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    s.execute("text.insert", &json!({"text": "chapter 5", "raw": true})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Hello world. See chapter 5 for details.");

    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (17, 26));
    assert_eq!(&text[start..end], "chapter 5");
}

#[test]
fn undo_and_redo_restore_hyperlink_ranges() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();

    assert_eq!(text_hyperlink_range(&s, hid), (17, 23));

    // Type "ABC " at 0
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": "ABC ", "raw": true})).unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (21, 27));

    // Undo
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (17, 23));

    // Redo
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (21, 27));
}

#[test]
fn track_changes_deleting_inserted_text_reconciles_hyperlink_ranges() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (17, 23));

    // Enable Track Changes
    s.execute("changes.track", &json!({"track": true})).unwrap();

    // Type "Draft " (6 bytes) at offset 0 while tracking
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("text.insert", &json!({"text": "Draft ", "raw": true})).unwrap();

    // Hyperlink shifts to 23..29
    assert_eq!(text_hyperlink_range(&s, hid), (23, 29));

    // Now select and delete the inserted "Draft " (0..6) while tracking is on
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 6})).unwrap();
    s.execute("text.delete", &json!({})).unwrap();

    // The inserted text was physically removed, so the hyperlink must reconcile back to 17..23
    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Hello world. See page 2 for details.");
    assert_eq!(text_hyperlink_range(&s, hid), (17, 23));
}

#[test]
fn change_case_adjusts_hyperlink_offsets_when_text_length_changes() {
    let initial = "Look at \u{FB01} and see page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    // "\u{FB01}" (fi ligature) is at bytes 8..11 (3 bytes).
    // "page 2" is at bytes 20..26.
    s.execute("text.select", &json!({"story": sid.0, "anchor": 20, "focus": 26})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (20, 26));

    // Select "\u{FB01}" (8..11) and convert to uppercase ("FI", 2 bytes)
    s.execute("text.select", &json!({"story": sid.0, "anchor": 8, "focus": 11})).unwrap();
    s.execute("type.changeCase", &json!({"case": "upper"})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Look at FI and see page 2 for details.");

    // Hyperlink should shift backward by 1 byte: 19..25
    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (19, 25));
    assert_eq!(&text[start..end], "page 2");
}

#[test]
fn change_case_preserves_hyperlink_strictly_inside_selection() {
    let initial = "Hello world";
    let (mut s, sid, _) = fixture(initial);

    // Hyperlink on "world" (6..11)
    s.execute("text.select", &json!({"story": sid.0, "anchor": 6, "focus": 11})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com/world"})).unwrap()["ids"][0].as_u64().unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (6, 11));

    // Select entire line (0..11) containing the hyperlink inside it and uppercase
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 11})).unwrap();
    s.execute("type.changeCase", &json!({"case": "upper"})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "HELLO WORLD");

    // Hyperlink must remain intact at 6..11
    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (6, 11));
    assert_eq!(&text[start..end], "WORLD");
}

#[test]
fn formatted_paste_adjusts_hyperlinks_and_placeholders() {
    let initial = "Hello world. See page 2 for details.";
    let (mut s, sid, _) = fixture(initial);

    s.execute("text.select", &json!({"story": sid.0, "anchor": 17, "focus": 23})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com"})).unwrap()["ids"][0].as_u64().unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (17, 23));

    // Select "Hello " (0..6) and copy it
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 6})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();

    // Move caret to start and paste formatted
    s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("edit.paste", &json!({"formatted": true})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Hello Hello world. See page 2 for details.");

    // Hyperlink shifts forward by 6 bytes: 23..29
    let (start, end) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end), (23, 29));
    assert_eq!(&text[start..end], "page 2");
}

#[test]
fn drag_move_preserves_hyperlink_and_adjusts_intervening_ranges() {
    let initial = "Hello brave world.";
    let (mut s, sid, fid) = fixture(initial);

    // Hyperlink on "brave" (6..11)
    s.execute("text.select", &json!({"story": sid.0, "anchor": 6, "focus": 11})).unwrap();
    let hid = s.execute("hyperlink.create", &json!({"url": "https://example.com/brave"})).unwrap()["ids"][0].as_u64().unwrap();
    assert_eq!(text_hyperlink_range(&s, hid), (6, 11));

    let at = |s: &Session, b: usize| {
        let d = &s.doc().unwrap().doc;
        let cs = designcraft_compose::compose_story(d, sid, &Default::default());
        let l = &cs.frames[0].lines[0];
        let g = l.glyphs.iter().find(|g| g.byte == b).unwrap();
        json!([g.x + g.adv * 0.25, l.baseline - 3.0])
    };
    let end = |s: &Session| {
        let d = &s.doc().unwrap().doc;
        let cs = designcraft_compose::compose_story(d, sid, &Default::default());
        let l = &cs.frames[0].lines[0];
        json!([l.end_x + 2.0, l.baseline - 3.0])
    };

    // Drag "brave " (6..12) to the end of the line
    s.execute("text.select", &json!({"story": sid.0, "anchor": 6, "focus": 12})).unwrap();
    s.execute("text.placeCaret", &json!({"frame": fid, "point": at(&s, 8)})).unwrap();
    s.execute("text.release", &json!({"frame": fid, "point": end(&s), "moved": true})).unwrap();

    let text = &s.doc().unwrap().doc.story(sid).unwrap().text;
    assert_eq!(text, "Hello world.brave ");

    // The hyperlink on "brave" must follow the moved text to 12..17
    let (start, end_pos) = text_hyperlink_range(&s, hid);
    assert_eq!((start, end_pos), (12, 17));
    assert_eq!(&text[start..end_pos], "brave");
}
