use super::*;

fn frame(s: &mut Session, text: &str) -> designcraft_doc::StoryId {
    let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
    s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story
}

#[test]
fn paste_text_with_no_insertion_point_makes_a_text_frame() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let items = |s: &Session| s.doc().unwrap().doc.spreads[0].items.len();
    // Text from another app, nothing copied here: a new, selected text frame.
    let r = s.execute("edit.paste", &json!({"text": "From elsewhere\r\nSecond line"})).unwrap();
    let id = ItemId(r["id"].as_u64().unwrap());
    let d = s.doc().unwrap();
    assert_eq!(d.selection.items, vec![id]);
    assert!(d.selection.text.is_none(), "selected as an object");
    let it = d.doc.item(id).unwrap();
    assert_eq!(d.doc.story(it.text_frame().unwrap().story).unwrap().text, "From elsewhere\nSecond line");
    let page = &d.doc.spreads[0].pages[0];
    let (b, m) = (it.bounds(), page.margin_rect());
    assert!((b.width() - page.column_rects()[0].width()).abs() < 1e-6, "{b:?}");
    assert!(b.height() < m.height() / 2.0, "fits two lines: {b:?}");
    assert!((b.center().y - m.center().y).abs() < 1e-6, "{b:?}");
    assert!(!s.cache.get(&d.doc, it.text_frame().unwrap().story, None).is_overset());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(items(&s), 0, "one undo removes it");
    // Centred on `at`; hostile numbers are clamped.
    let r = s.execute("edit.paste", &json!({"text": "Here", "at": [200.0, 300.0]})).unwrap();
    let b = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().bounds();
    assert!((b.center().x - 200.0).abs() < 1e-6 && (b.center().y - 300.0).abs() < 1e-6, "{b:?}");
    assert!(s.execute("edit.paste", &json!({"text": "Far", "at": [1e300, -1e300], "spread": 99})).is_err());
    s.execute("edit.paste", &json!({"text": "Far", "at": [1e300, -1e300]})).unwrap();
    // Copied objects paste when no text is given.
    s.execute("edit.deselectAll", &json!({})).unwrap();
    let rect = s.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
    s.execute("selection.set", &json!({"ids": [rect]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    let n = items(&s);
    let r = s.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(r["ids"].as_array().unwrap().len(), 1);
    assert_eq!(items(&s), n + 1);
    assert!(s.doc().unwrap().doc.item(ItemId(r["ids"][0].as_u64().unwrap())).unwrap().text_frame().is_none());
    s.doc().unwrap().doc.check().unwrap();
}

/// Copying a rectangle, then text in another app, pasted the rectangle into the text (#212).
#[test]
fn paste_at_a_caret_prefers_other_apps_text_to_copied_objects() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let story = frame(&mut s, "ab");
    let rect = s.execute("frame.create", &json!({"rect": [72, 400, 200, 480]})).unwrap()["id"].clone();
    s.execute("selection.set", &json!({"ids": [rect]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    let marker = copied_objects_text(&s);
    s.execute("text.select", &json!({"story": story.0, "anchor": 2, "focus": 2})).unwrap();
    let r = s.execute("edit.paste", &json!({"text": "From elsewhere"})).unwrap();
    assert!(r.get("anchored").is_none(), "{r}");
    assert_eq!(s.doc().unwrap().doc.story(story).unwrap().text, "abFrom elsewhere");
    // What the object copy put on the system clipboard still pastes the objects.
    assert_eq!(s.execute("edit.paste", &json!({"text": marker})).unwrap()["anchored"], 1);
    // With no insertion point too: the rectangle, not a text frame holding the marker.
    s.execute("edit.deselectAll", &json!({})).unwrap();
    let r = s.execute("edit.paste", &json!({"text": marker})).unwrap();
    let id = ItemId(r["ids"][0].as_u64().unwrap());
    assert!(s.doc().unwrap().doc.item(id).unwrap().text_frame().is_none());
    s.doc().unwrap().doc.check().unwrap();
    // Paste without Formatting inserts the text given; empty text is no text.
    s.execute("text.select", &json!({"story": story.0, "anchor": 0, "focus": 0})).unwrap();
    assert!(s.execute("edit.pasteWithoutFormatting", &json!({"text": ""})).is_err());
}

/// Text frames, and groups holding one, can't be anchored: at a caret their copy pastes as text.
#[test]
fn copied_text_frames_paste_at_a_caret_as_their_text() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let target = frame(&mut s, "ab");
    let copy_and_paste = |s: &mut Session, ids: Value, text: Value| {
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s.execute("text.select", &json!({"story": target.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("edit.paste", &json!({"text": text})).unwrap()
    };
    let chapter = s.execute("frame.create", &json!({"rect": [72, 400, 300, 480], "content": "text", "text": "Chapter 1"})).unwrap()["id"].clone();
    // The same text copied in another app, and the system clipboard read back as absent.
    let r = copy_and_paste(&mut s, json!([chapter]), json!("Chapter 1"));
    assert!(r.get("anchored").is_none(), "{r}");
    s.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(target).unwrap().text, "Chapter 1Chapter 1ab");
    // A group with a text frame inside, its own placeholder on the system clipboard.
    let rect = s.execute("frame.create", &json!({"rect": [72, 500, 200, 560]})).unwrap()["id"].clone();
    let group = s.execute("object.group", &json!({"ids": [chapter, rect]})).unwrap()["id"].clone();
    let r = copy_and_paste(&mut s, json!([group]), Value::Null);
    assert!(r.get("anchored").is_none(), "{r}");
    assert_eq!(s.doc().unwrap().doc.story(target).unwrap().text, "Chapter 1Chapter 1Chapter 1ab");
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn outside_text_never_pastes_markers() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    // Private-use characters from another app would read as footnote, table or variable markers.
    let dirty = "a\u{E00A}b\u{E009}c\u{E1FF}d\u{E200}";
    let r = s.execute("edit.paste", &json!({"text": dirty})).unwrap();
    let st = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
    assert_eq!(s.doc().unwrap().doc.story(st).unwrap().text, "abcd\u{E200}");
    s.doc().unwrap().doc.check().unwrap();
    let b = frame(&mut s, "Target");
    s.execute("text.select", &json!({"story": b.0, "anchor": 6, "focus": 6})).unwrap();
    s.execute("edit.paste", &json!({"text": dirty})).unwrap();
    s.execute("edit.pasteWithoutFormatting", &json!({"text": dirty})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(b).unwrap().text, "Targetabcd\u{E200}abcd\u{E200}");
    assert!(s.execute("edit.paste", &json!({"text": "\u{E00A}"})).is_err());
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn copied_text_pasted_with_no_insertion_point_keeps_its_formatting() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let a = frame(&mut s, "Bold words\nSecond");
    s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 4})).unwrap();
    s.execute("type.bold", &json!({})).unwrap();
    s.execute("text.select", &json!({"story": a.0, "anchor": 11, "focus": 17})).unwrap();
    s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
    s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 17})).unwrap();
    let copied = s.execute("edit.copy", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
    s.execute("edit.deselectAll", &json!({})).unwrap();
    let r = s.execute("edit.paste", &json!({"text": copied})).unwrap();
    let d = s.doc().unwrap();
    let st = d.doc.story(d.doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story).unwrap();
    assert_eq!(st.text, "Bold words\nSecond");
    assert_eq!(st.char_format_at(2).over.font_style.as_deref(), Some("Bold"));
    assert_eq!(st.char_format_at(7).over.font_style, None);
    assert_eq!(st.paras[1], d.doc.story(a).unwrap().paras[1]);
    d.doc.check().unwrap();
}

#[test]
fn formatted_copy_paste_and_plain_paste() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let a = frame(&mut s, "Bold words here.");
    let b = frame(&mut s, "Target: .");
    s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 4})).unwrap();
    s.execute("type.bold", &json!({})).unwrap();
    // A footnote inside the copied range travels with it.
    s.execute("text.select", &json!({"story": a.0, "anchor": 4, "focus": 4})).unwrap();
    s.execute("footnote.insert", &json!({"text": "n"})).unwrap();
    let fl = designcraft_doc::FOOTNOTE_REF.len_utf8();
    s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": 10 + fl})).unwrap();
    let r = s.execute("edit.copy", &json!({})).unwrap();
    assert_eq!(r["text"], "Bold words");
    // Formatted paste when the system clipboard matches.
    s.execute("text.select", &json!({"story": b.0, "anchor": 8, "focus": 8})).unwrap();
    s.execute("edit.paste", &json!({"text": "Bold words"})).unwrap();
    let st = s.doc().unwrap().doc.story(b).unwrap().clone();
    assert_eq!(st.text, format!("Target: Bold{} words.", designcraft_doc::FOOTNOTE_REF));
    assert_eq!(st.char_format_at(9).over.font_style.as_deref(), Some("Bold"));
    assert_eq!(st.notes.len(), 1);
    assert_eq!(st.notes[0].text.text, "n");
    // Other text on the system clipboard pastes as plain text.
    s.execute("edit.paste", &json!({"text": "\"x\""})).unwrap();
    assert!(s.doc().unwrap().doc.story(b).unwrap().text.contains("\"x\""), "no smart quotes on paste");
    // Paste without formatting takes the insertion format.
    s.execute("text.select", &json!({"story": b.0, "anchor": 0, "focus": 0})).unwrap();
    s.execute("edit.pasteWithoutFormatting", &json!({})).unwrap();
    let st = s.doc().unwrap().doc.story(b).unwrap().clone();
    assert!(st.text.starts_with("Bold wordsTarget"), "{:?}", st.text);
    assert_eq!(st.char_format_at(1).over.font_style, None);
    // Cut removes and copies.
    s.execute("text.select", &json!({"story": b.0, "anchor": 0, "focus": 4})).unwrap();
    assert_eq!(s.execute("edit.cut", &json!({})).unwrap()["text"], "Bold");
    assert!(s.doc().unwrap().doc.story(b).unwrap().text.starts_with(" wordsTarget"));
    s.doc().unwrap().doc.check().unwrap();
}

fn note_clipboard_session() -> (Session, designcraft_doc::StoryId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let source = frame(&mut s, "aéz");
    s.execute("text.select", &json!({"story": source.0, "anchor": 1, "focus": 1})).unwrap();
    s.execute("endnote.insert", &json!({"text": "Citation A"})).unwrap();
    s.execute("note.new", &json!({"text": "Memo A", "author": "Ed"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(source).unwrap().text, "a\u{E00F}\u{E010}éz");
    s.execute("text.select", &json!({"story": source.0, "anchor": 0, "focus": 10})).unwrap();
    s.execute("type.char", &json!({"attrs": {"fontStyle": "Bold"}})).unwrap();
    assert_eq!(s.execute("edit.copy", &json!({})).unwrap(), json!({"text": "aéz"}));
    (s, source)
}

fn assert_child_note_paste(
    s: &mut Session,
    source: designcraft_doc::StoryId,
    parent: designcraft_doc::StoryId,
    cell: designcraft_doc::CellAddr,
    endpoints: (usize, usize),
) {
    let before = s.doc().unwrap().doc.clone();
    let selected = s.doc().unwrap().selection.text.unwrap();
    assert_eq!((selected.story, selected.cell, selected.anchor, selected.focus), (parent, Some(cell), endpoints.0, endpoints.1));
    assert_eq!(selected.frame, None);
    let before_child = before.text_story(parent, Some(cell)).unwrap();
    assert_eq!(before_child.text, "L----R");
    let clipboard = s.text_clipboard.clone();
    let history = s.execute("document.history", &json!({})).unwrap();
    assert_eq!(history["redo"], json!([]));

    let result = s.execute("edit.paste", &json!({})).unwrap();
    let after = s.doc().unwrap().doc.clone();
    let child = after.text_story(parent, Some(cell)).unwrap();
    assert_eq!(child.text, "LaézR", "root-only note markers must not enter child text");
    assert_eq!(child.len(), 6);
    assert_eq!(result, json!({"pos": 5, "formatted": true}));
    assert_eq!(s.doc().unwrap().selection, Selection::text(designcraft_doc::TextSel { anchor: 5, focus: 5, ..selected }));
    assert!(child.endnotes.is_empty() && child.editorial.is_empty());
    for pos in [1, 2, 4] {
        let (_, format) = child.runs().find(|(range, _)| range.contains(&pos)).unwrap();
        assert_eq!(format.over.font_style.as_deref(), Some("Bold"));
    }
    // Inspect actual run coverage: char_format_at at a run boundary means the preceding run.
    for pos in [0, 5] {
        let (_, old_format) = before_child.runs().find(|(range, _)| range.contains(&pos)).unwrap();
        let (_, new_format) = child.runs().find(|(range, _)| range.contains(&pos)).unwrap();
        assert_eq!(new_format, old_format);
    }
    assert_eq!(after.story(parent).unwrap().text, before.story(parent).unwrap().text);
    assert_eq!(after.story(parent).unwrap().chars, before.story(parent).unwrap().chars);
    assert_eq!(after.story(source), before.story(source));
    assert_eq!(s.text_clipboard, clipboard);

    let mut pasted_history = history.clone();
    pasted_history["undo"].as_array_mut().unwrap().push(json!("Paste"));
    assert_eq!(s.execute("document.history", &json!({})).unwrap(), pasted_history);
    assert_eq!(s.execute("edit.undo", &json!({})).unwrap(), json!({"undone": "Paste"}));
    assert_eq!(s.doc().unwrap().doc, before);
    let mut undone_history = history;
    undone_history["redo"] = json!(["Paste"]);
    assert_eq!(s.execute("document.history", &json!({})).unwrap(), undone_history);
    // This filter does not change which selection Undo restores.
    assert_eq!(s.execute("edit.redo", &json!({})).unwrap(), json!({"redone": "Paste"}));
    assert_eq!(s.doc().unwrap().doc, after);
    assert_eq!(s.execute("document.history", &json!({})).unwrap(), pasted_history);
}

#[test]
fn formatted_child_paste_filters_endnotes_and_editorial_in_cells() {
    let (mut s, source) = note_clipboard_session();
    let parent = frame(&mut s, "PZ");
    s.execute("text.select", &json!({"story": parent.0, "anchor": 1, "focus": 1})).unwrap();
    let table_id = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    s.execute("text.insert", &json!({"text": "L----R"})).unwrap();
    s.execute("table.nextCell", &json!({})).unwrap();
    s.execute("text.insert", &json!({"text": "NEIGHBOR"})).unwrap();
    s.execute("table.prevCell", &json!({})).unwrap();
    s.execute("text.move", &json!({"dir": "storyStart"})).unwrap();
    s.execute("text.move", &json!({"dir": "right"})).unwrap();
    for _ in 0..4 {
        s.execute("text.move", &json!({"dir": "right", "extend": true})).unwrap();
    }
    let before_table = s.doc().unwrap().doc.story(parent).unwrap().tables[&table_id].clone();
    let cell = designcraft_doc::CellAddr { table: table_id, row: 0, col: 0 };
    assert_child_note_paste(&mut s, source, parent, cell, (1, 5));
    let table = &s.doc().unwrap().doc.story(parent).unwrap().tables[&table_id];
    assert_eq!(table.cell(0, 1).unwrap().text.text, "NEIGHBOR");
    let mut expected_table = (*before_table).clone();
    expected_table.cells[0].text = table.cells[0].text.clone();
    assert_eq!(table.as_ref(), &expected_table);

    // The same clipboard must still retain both markers when pasted into an ordinary root story.
    let root = frame(&mut s, "L----R");
    s.execute("text.select", &json!({"story": root.0, "anchor": 1, "focus": 5})).unwrap();
    assert_eq!(s.execute("edit.paste", &json!({})).unwrap(), json!({"pos": 11, "formatted": true}));
    assert_eq!(s.doc().unwrap().doc.story(root).unwrap().text, "La\u{E00F}\u{E010}ézR");
}

#[test]
fn formatted_child_paste_filters_endnotes_and_editorial_in_footnotes() {
    let (mut s, source) = note_clipboard_session();
    let parent = frame(&mut s, "PZ");
    let neighbor = s.execute("footnote.insert", &json!({"text": "NEIGHBOR"})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.select", &json!({"story": parent.0, "anchor": 1, "focus": 1})).unwrap();
    let target = s.execute("footnote.insert", &json!({"text": "L----R"})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.move", &json!({"dir": "left"})).unwrap();
    for _ in 0..4 {
        s.execute("text.move", &json!({"dir": "left", "extend": true})).unwrap();
    }
    let before = s.doc().unwrap().doc.story(parent).unwrap().clone();
    assert_child_note_paste(&mut s, source, parent, designcraft_doc::CellAddr::footnote(target), (5, 1));
    let after = s.doc().unwrap().doc.story(parent).unwrap();
    assert_eq!(after.note(neighbor).unwrap().text.text, "NEIGHBOR");
    assert_eq!(after.note(neighbor), before.note(neighbor));
    assert_eq!(after.notes.iter().map(|note| note.id).collect::<Vec<_>>(), before.notes.iter().map(|note| note.id).collect::<Vec<_>>());
}

#[test]
fn change_case_keeps_runs() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let a = frame(&mut s, "the quick brown fox. jumps over straße");
    s.execute("text.select", &json!({"story": a.0, "anchor": 4, "focus": 9})).unwrap();
    s.execute("type.italic", &json!({})).unwrap();
    let len = s.doc().unwrap().doc.story(a).unwrap().len();
    s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": len})).unwrap();
    s.execute("type.changeCase", &json!({"case": "title"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "The Quick Brown Fox. Jumps Over Straße");
    s.execute("type.changeCase", &json!({"case": "sentence"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "The quick brown fox. Jumps over straße");
    s.execute("type.changeCase", &json!({"case": "upper"})).unwrap();
    let st = s.doc().unwrap().doc.story(a).unwrap().clone();
    assert_eq!(st.text, "THE QUICK BROWN FOX. JUMPS OVER STRASSE");
    assert_eq!(st.char_format_at(5).over.font_style.as_deref(), Some("Italic"));
    assert_eq!(s.doc().unwrap().selection.text.unwrap().range(), 0..st.len());
    st.check().unwrap();
}

#[test]
fn change_case_leaves_georgian_without_initial_capitals() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let a = frame(&mut s, "ᲒᲐᲛᲐᲠᲯᲝᲑᲐ ᲛᲡᲝᲤᲚᲘᲝ. ᲐᲮᲐᲚᲘ word");
    let len = s.doc().unwrap().doc.story(a).unwrap().len();
    s.execute("text.select", &json!({"story": a.0, "anchor": 0, "focus": len})).unwrap();
    // Georgian has no title or sentence capitals: Mtavruli is only for all-caps text.
    s.execute("type.changeCase", &json!({"case": "title"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "გამარჯობა მსოფლიო. ახალი Word");
    s.execute("type.changeCase", &json!({"case": "sentence"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "გამარჯობა მსოფლიო. ახალი word");
    s.execute("type.changeCase", &json!({"case": "upper"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.story(a).unwrap().text, "ᲒᲐᲛᲐᲠᲯᲝᲑᲐ ᲛᲡᲝᲤᲚᲘᲝ. ᲐᲮᲐᲚᲘ WORD");
}
