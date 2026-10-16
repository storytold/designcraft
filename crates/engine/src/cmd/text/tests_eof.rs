//! Native blank-EOF typing regressions; commands drive edits, queries, save/reopen and history.
use std::sync::Arc;

use designcraft_doc::{Leading, StoryId};
use serde_json::{Value, json};

use crate::Session;

fn fixture(text: &str) -> (Session, StoryId) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [40, 40, 440, 500], "content": "text", "text": text})).unwrap();
    let sid = StoryId(r["story"].as_u64().unwrap());
    select(&mut s, sid, text.len(), text.len());
    (s, sid)
}

fn select(s: &mut Session, sid: StoryId, a: usize, b: usize) {
    s.execute("text.select", &json!({"story": sid.0, "anchor": a, "focus": b})).unwrap();
}

fn format(s: &mut Session, attrs: Value) {
    s.execute("type.char", &json!({"attrs": attrs})).unwrap();
}

fn attrs(s: &mut Session) -> Value {
    s.execute("type.selectionAttrs", &json!({})).unwrap()
}

fn open(bytes: &[u8]) -> Session {
    let mut s = Session::new();
    s.execute("file.openBytes", &json!({"base64": crate::cmd::file::base64_encode(bytes)})).unwrap();
    s
}

fn reopen(s: &mut Session) -> Session {
    let native = s.execute("file.serialize", &json!({})).unwrap();
    let bytes = crate::cmd::file::base64_decode(native["base64"].as_str().unwrap());
    assert!(bytes.starts_with(b"PK"));
    let back = open(&bytes);
    assert_eq!(serde_json::to_value(&*back.doc().unwrap().doc).unwrap(), serde_json::to_value(&*s.doc().unwrap().doc).unwrap());
    back
}

#[test]
fn partial_eof_edits_preserve_neighbor_runs_and_continue_typing() {
    let (mut s, sid) = fixture("Café\n");
    select(&mut s, sid, 0, 6);
    format(&mut s, json!({"size": 15, "leading": {"kind": "points", "value": 18}}));
    select(&mut s, sid, 6, 6);
    let prior = s.doc().unwrap().doc.story(sid).unwrap().clone();
    format(&mut s, json!({"size": 20}));
    assert_eq!(attrs(&mut s)["chars"]["leading"], json!({"kind": "points", "value": 18.0}));
    format(&mut s, json!({"leading": {"kind": "points", "value": 36}}));
    let after = s.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(after.text, prior.text);
    assert_eq!(after.chars, prior.chars);
    assert_eq!(after.paras[0], prior.paras[0]);
    assert!(after.chars.iter().all(|r| r.len > 0));
    let composed = s.cache.get(&s.doc().unwrap().doc, sid, None);
    let blank = composed.frames[0].lines.last().unwrap();
    assert!(blank.range.is_empty());
    assert!((blank.leading - 36.0).abs() < 1e-6);
    assert_eq!(attrs(&mut s)["chars"]["size"], 20.0);
    s.execute("text.insert", &json!({"text": "Z", "raw": true})).unwrap();
    s.execute("text.insert", &json!({"text": "\n", "raw": true})).unwrap();
    let composed = s.cache.get(&s.doc().unwrap().doc, sid, None);
    assert!((composed.frames[0].lines.last().unwrap().leading - 36.0).abs() < 1e-6);
    assert_eq!(attrs(&mut s)["chars"]["size"], 20.0);
    s.execute("text.insert", &json!({"text": "é", "raw": true})).unwrap();
    let st = s.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(st.text, "Café\nZ\né");
    assert_eq!(st.format_after(6).over.size, Some(20.0));
    assert_eq!(st.format_after(8).over.leading, Some(Leading::Points(36.0)));
    assert!(st.paras.iter().all(|p| p.eof_inherited_chars.is_none()));
    select(&mut s, sid, 6, 10);
    format(&mut s, json!({"size": null, "leading": null}));
    assert_eq!(attrs(&mut s)["chars"]["size"], 12.0, "consumed typing attrs must not become paragraph defaults");
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn explicit_empty_eof_stage_survives_native_reopen_and_differs_from_unstaged() {
    let (mut s, sid) = fixture("A\n");
    select(&mut s, sid, 0, 2);
    format(&mut s, json!({"size": 15}));
    select(&mut s, sid, 2, 2);
    let old = serde_json::to_vec(&*s.doc().unwrap().doc).unwrap();
    assert!(!String::from_utf8(old.clone()).unwrap().contains("eofInheritedChars"));
    let mut unstaged = open(&old);
    select(&mut unstaged, sid, 2, 2);
    assert_eq!(attrs(&mut unstaged)["chars"]["size"], 15.0);
    format(&mut s, json!({"size": null}));
    let pf = s.doc().unwrap().doc.story(sid).unwrap().paras.last().unwrap();
    assert!(pf.chars.is_empty() && pf.eof_inherited_chars.as_ref().unwrap().is_empty());
    let mut back = reopen(&mut s);
    select(&mut back, sid, 2, 2);
    assert_eq!(attrs(&mut back)["chars"]["size"], 12.0);
    back.execute("text.insert", &json!({"text": "X"})).unwrap();
    let st = back.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(st.format_after(0).over.size, Some(15.0));
    assert_eq!(st.format_after(2).over.size, None);
    assert!(st.paras[1].eof_inherited_chars.is_none());
    back.doc().unwrap().doc.check().unwrap();
}

#[test]
fn eof_keeps_live_character_style_above_genuine_paragraph_defaults() {
    let (mut s, sid) = fixture("A\n");
    s.execute("style.character.create", &json!({"name": "Live", "chars": {"size": 30}})).unwrap();
    select(&mut s, sid, 0, 2);
    s.execute("style.character.apply", &json!({"name": "Live"})).unwrap();
    // A legacy/native paragraph can carry real direct defaults below its character style.
    let mut legacy = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
    legacy["stories"][sid.0.to_string()]["paras"][1]["chars"] = json!({"size": 24});
    let mut s = open(&serde_json::to_vec(&legacy).unwrap());
    select(&mut s, sid, 2, 2);
    format(&mut s, json!({"leading": {"kind": "points", "value": 36}}));
    assert_eq!(attrs(&mut s)["chars"]["size"], 30.0);
    s.execute("style.character.edit", &json!({"name": "Live", "chars": {"size": 32}})).unwrap();
    assert_eq!(attrs(&mut s)["chars"]["size"], 32.0);
    let mut s = reopen(&mut s);
    select(&mut s, sid, 2, 2);
    assert_eq!(attrs(&mut s)["chars"]["size"], 32.0);
    s.execute("text.insert", &json!({"text": "é", "raw": true})).unwrap();
    let st = s.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(st.paras[1].chars.size, Some(24.0));
    assert_eq!(st.format_after(2).style, "Live");
    assert_eq!(st.format_after(2).over.size, None);
    select(&mut s, sid, 2, 4);
    format(&mut s, json!({"leading": null}));
    assert_eq!(attrs(&mut s)["chars"]["size"], 32.0);
    s.execute("style.character.edit", &json!({"name": "Live", "chars": {"size": null}})).unwrap();
    assert_eq!(attrs(&mut s)["chars"]["size"], 24.0);
}

#[test]
fn eof_format_and_typing_have_exact_document_undo_redo_after_native_reopen() {
    let (mut s, sid) = fixture("A\n");
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();
    format(&mut s, json!({"size": 20}));
    let formatted = s.doc().unwrap().doc.clone();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &formatted));
    let mut back = reopen(&mut s);
    select(&mut back, sid, 2, 2);
    let before = back.doc().unwrap().doc.clone();
    back.execute("text.insert", &json!({"text": "é", "raw": true})).unwrap();
    let typed = back.doc().unwrap().doc.clone();
    assert_eq!(typed.story(sid).unwrap().text, "A\né");
    back.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&back.doc().unwrap().doc, &before));
    back.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&back.doc().unwrap().doc, &typed));
    back.doc().unwrap().doc.check().unwrap();
}

#[test]
fn ordinary_carets_empty_stories_and_cell_eof_keep_their_existing_policy() {
    // A nonempty paragraph start is an ordinary caret. Upstream's hidden-mark regression
    // covers the distinct blank-paragraph caret, whose LF now takes formatting immediately.
    for (text, at) in [("AB", 1), ("A\nB", 2)] {
        let (mut s, sid) = fixture(text);
        select(&mut s, sid, at, at);
        let before = s.doc().unwrap().doc.story(sid).unwrap().clone();
        format(&mut s, json!({"size": 20}));
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap(), &before);
    }
    let (mut range, sid) = fixture("A\n");
    format(&mut range, json!({"tateChuYoko": true}));
    select(&mut range, sid, 1, 2);
    range.execute("type.tateChuYoko", &json!({})).unwrap();
    assert_eq!(attrs(&mut range)["chars"]["tateChuYoko"], true, "a selected final LF reads its own run, not the staged EOF caret");
    let (mut s, sid) = fixture("");
    format(&mut s, json!({"size": 20}));
    assert_eq!(attrs(&mut s)["chars"]["size"], 20.0);
    assert!(s.doc().unwrap().doc.story(sid).unwrap().paras[0].eof_inherited_chars.is_none());
    s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
    s.execute("table.setCell", &json!({"row": 0, "col": 0, "text": "A\n"})).unwrap();
    let table = *s.doc().unwrap().doc.story(sid).unwrap().tables.keys().next().unwrap();
    s.execute("text.move", &json!({"dir": "storyEnd"})).unwrap();
    let caret = s.doc().unwrap().selection.text.unwrap();
    assert_eq!(caret.cell, Some(designcraft_doc::CellAddr { table, row: 0, col: 0 }));
    assert_eq!((caret.story, caret.anchor, caret.focus), (sid, 2, 2));
    assert!(s.doc().unwrap().selection.text.unwrap().cell.is_some());
    let before = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
    format(&mut s, json!({"size": 42}));
    assert_eq!(serde_json::to_value(&*s.doc().unwrap().doc).unwrap(), before);
    assert_eq!(attrs(&mut s)["chars"]["size"], 42.0);
    let pending = super::typing_format(&s).unwrap();
    assert_eq!(pending.at, caret);
    assert_eq!(pending.attrs.size, Some(42.0));
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn eof_character_style_capture_uses_staged_format_and_preserves_selected_lf() {
    for (clear, selected_lf, captured_size, resolved_size) in
        [(false, false, Some(20.0), 20.0), (true, false, None, 12.0), (false, true, Some(15.0), 15.0)]
    {
        let (mut s, sid) = fixture("A\n");
        s.execute("style.character.create", &json!({"name": "Named", "chars": {"underline": true}})).unwrap();
        select(&mut s, sid, 0, 2);
        s.execute("style.character.apply", &json!({"name": "Named"})).unwrap();
        format(&mut s, json!({"size": 15}));
        select(&mut s, sid, 2, 2);
        format(&mut s, json!({"size": 20}));
        if clear {
            format(&mut s, json!({"size": null}));
        }
        if selected_lf {
            select(&mut s, sid, 1, 2);
        }
        assert_eq!(attrs(&mut s)["chars"]["size"], resolved_size);
        let source = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(source.text, "A\n");
        assert_eq!(source.format_after(1).over.size, Some(15.0));
        let selection = s.doc().unwrap().selection.clone();
        let created = s.execute("style.character.create", &json!({"name": "Captured", "fromSelection": true})).unwrap();
        let name = created["name"].as_str().unwrap();
        let captured = &s.doc().unwrap().doc.styles.char_style(name).unwrap().chars;
        assert_eq!(captured.size, captured_size, "clear={clear}, selected_lf={selected_lf}");
        assert_eq!(captured.underline, Some(true), "the named character style remains part of sparse capture");
        assert_eq!(s.doc().unwrap().selection, selection);
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap(), &source);

        // Reuse proves that an explicitly cleared EOF size inherits its new paragraph's size,
        // while a staged size and an actual selected LF retain their distinct literal values.
        let frame = s.execute("frame.create", &json!({"rect": [40, 520, 440, 700], "content": "text", "text": "Reuse"})).unwrap();
        let other = StoryId(frame["story"].as_u64().unwrap());
        s.execute("style.paragraph.create", &json!({"name": "Reuse paragraph", "chars": {"size": 24}})).unwrap();
        select(&mut s, other, 0, 5);
        s.execute("style.paragraph.apply", &json!({"name": "Reuse paragraph"})).unwrap();
        let reuse_selection = s.doc().unwrap().selection.clone();
        s.execute("style.character.apply", &json!({"name": name})).unwrap();
        assert_eq!(s.doc().unwrap().selection, reuse_selection);
        let reused = attrs(&mut s);
        assert_eq!(reused["chars"]["size"], captured_size.unwrap_or(24.0));
        assert_eq!(reused["chars"]["underline"], true);
        let target = s.doc().unwrap().doc.story(other).unwrap();
        assert_eq!(target.text, "Reuse");
        assert!(target.chars.iter().all(|r| r.format.style == name && r.format.over.size.is_none()));
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap(), &source);
        s.doc().unwrap().doc.check().unwrap();
    }
}

#[test]
fn eof_note_references_preserve_staged_and_cleared_typing_formats() {
    for (command, marker) in [("note.new", '\u{E010}'), ("endnote.insert", '\u{E00F}'), ("footnote.insert", '\u{E00A}')] {
        for (edit, selected_lf, stored_size, queried_size) in [
            (json!({"size": 20}), false, Some(20.0), 20.0),
            (json!({"size": null}), false, None, 12.0),
            (json!({}), false, Some(15.0), 15.0),
            (json!({"size": 20}), true, Some(15.0), 15.0),
        ] {
            let (mut s, sid) = fixture("A\n");
            select(&mut s, sid, 0, 2);
            format(&mut s, json!({"size": 15}));
            let previous = s.doc().unwrap().doc.story(sid).unwrap().format_after(0).clone();
            select(&mut s, sid, 2, 2);
            format(&mut s, edit);
            assert_eq!(attrs(&mut s)["chars"]["size"], if selected_lf { 20.0 } else { queried_size });
            if selected_lf {
                select(&mut s, sid, 1, 2);
            }
            let result = s.execute(command, &json!({"text": "check"})).unwrap();
            assert_eq!(result["story"], sid.0);
            if command == "footnote.insert" {
                assert!(s.doc().unwrap().selection.text.unwrap().cell.unwrap().footnote_id().is_some());
                s.execute("footnote.goToReference", &json!({})).unwrap();
            }
            // Footnote replaces a range; editorial/endnote insert after its upper endpoint.
            let prefix = if selected_lf && command == "footnote.insert" { "A" } else { "A\n" };
            let marker_at = prefix.len();
            let caret = marker_at + 3;
            assert_eq!(s.doc().unwrap().selection, designcraft_doc::Selection::text(designcraft_doc::TextSel::caret(sid, caret)));
            assert_eq!(attrs(&mut s)["chars"]["size"], queried_size, "{command}, selected_lf={selected_lf}");
            let st = s.doc().unwrap().doc.story(sid).unwrap();
            assert_eq!(st.text, format!("{prefix}{marker}"));
            assert_eq!(st.format_after(marker_at).over.size, stored_size);
            match command {
                "note.new" => assert_eq!(st.editorial[0].text, "check"),
                "endnote.insert" => assert_eq!(st.endnotes[0].text.text, "check"),
                _ => {
                    assert_eq!(command, "footnote.insert");
                    assert_eq!(st.notes[0].text.text, "check");
                }
            }
            s.execute("text.insert", &json!({"text": "Z", "raw": true})).unwrap();
            assert_eq!(attrs(&mut s)["chars"]["size"], queried_size);
            assert_eq!(s.doc().unwrap().selection, designcraft_doc::Selection::text(designcraft_doc::TextSel::caret(sid, caret + 1)));
            let st = s.doc().unwrap().doc.story(sid).unwrap();
            assert_eq!(st.text, format!("{prefix}{marker}Z"));
            assert_eq!(st.format_after(0), &previous);
            if prefix == "A\n" {
                assert_eq!(st.format_after(1), &previous);
            }
            assert_eq!(st.format_after(marker_at).over.size, stored_size);
            assert_eq!(st.format_after(caret).over.size, stored_size);
            assert!(st.paras.iter().all(|p| p.eof_inherited_chars.is_none()));
            s.doc().unwrap().doc.check().unwrap();
        }
    }
}

#[test]
fn eof_table_cells_consume_typing_format_without_baking_in_paragraph_overrides() {
    for (edit, stored_size, queried_size) in
        [(json!({"size": 20}), Some(20.0), 20.0), (json!({"size": null}), None, 12.0), (json!({}), Some(15.0), 15.0)]
    {
        let (mut s, sid) = fixture("A\n");
        select(&mut s, sid, 0, 2);
        format(&mut s, json!({"size": 15}));
        select(&mut s, sid, 2, 2);
        format(&mut s, edit);
        assert_eq!(attrs(&mut s)["chars"]["size"], queried_size);
        let result = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
        assert_eq!(result["story"], sid.0);
        let table = result["table"].as_u64().unwrap();
        let caret = designcraft_doc::TextSel {
            story: sid,
            anchor: 0,
            focus: 0,
            frame: None,
            cell: Some(designcraft_doc::CellAddr { table, row: 0, col: 0 }),
        };
        assert_eq!(s.doc().unwrap().selection, designcraft_doc::Selection::text(caret));
        assert_eq!(attrs(&mut s)["chars"]["size"], queried_size, "query the cell selected by table.insert, before typing");
        let st = s.doc().unwrap().doc.story(sid).unwrap();
        assert_eq!(st.text, "A\n\u{E009}\n");
        assert_eq!(st.format_after(0).over.size, Some(15.0));
        assert_eq!(st.format_after(1).over.size, Some(15.0));
        let parent = (st.text.clone(), st.chars.clone(), st.paras.clone());
        let cells = &st.tables[&table].cells;
        assert_eq!(cells.len(), 2);
        for cell in cells {
            assert!(cell.text.text.is_empty());
            assert_eq!(cell.text.chars.len(), 1);
            assert_eq!(cell.text.chars[0].len, 0);
            assert_eq!(cell.text.chars[0].format.over.size, stored_size);
            assert_eq!(cell.text.paras.len(), 1);
            assert!(cell.text.paras[0].chars.is_empty());
            assert!(cell.text.paras[0].eof_inherited_chars.is_none());
            assert!(cell.text.paras[0].table.is_none());
        }
        let neighbor = cells[1].clone();
        let back = reopen(&mut s);
        assert_eq!(back.doc().unwrap().doc.story(sid).unwrap().tables[&table].cells[0].text.chars[0].format.over.size, stored_size);
        assert_eq!(s.doc().unwrap().selection, designcraft_doc::Selection::text(caret));
        s.execute("text.insert", &json!({"text": "Z", "raw": true})).unwrap();
        assert_eq!(attrs(&mut s)["chars"]["size"], queried_size);
        assert_eq!(s.doc().unwrap().selection, designcraft_doc::Selection::text(designcraft_doc::TextSel { anchor: 1, focus: 1, ..caret }));
        let cell = &s.doc().unwrap().doc.story(sid).unwrap().tables[&table].cells[0];
        assert_eq!(cell.text.text, "Z");
        assert_eq!(cell.text.format_after(0).over.size, stored_size);
        assert!(cell.text.paras[0].chars.is_empty());
        assert!(cell.text.paras[0].eof_inherited_chars.is_none());
        s.execute("text.move", &json!({"dir": "storyStart", "extend": true})).unwrap();
        assert_eq!(s.doc().unwrap().selection, designcraft_doc::Selection::text(designcraft_doc::TextSel { anchor: 1, focus: 0, ..caret }));
        format(&mut s, json!({"size": null}));
        assert_eq!(attrs(&mut s)["chars"]["size"], 12.0, "the staged size was not installed as a permanent paragraph default");
        let st = s.doc().unwrap().doc.story(sid).unwrap();
        assert_eq!((st.text.clone(), st.chars.clone(), st.paras.clone()), parent);
        assert_eq!(st.tables[&table].cells[1], neighbor);
        let cell = &st.tables[&table].cells[0];
        assert_eq!(cell.text.text, "Z");
        assert_eq!(cell.text.format_after(0).over.size, None);
        assert!(cell.text.paras[0].chars.is_empty());
        assert!(cell.text.paras[0].eof_inherited_chars.is_none());
        let back = reopen(&mut s);
        assert_eq!(back.doc().unwrap().doc.story(sid).unwrap().tables[&table].cells[0].text.format_after(0).over.size, None);
        s.doc().unwrap().doc.check().unwrap();
    }
    // A selected LF keeps the existing capture at the range's lower boundary (preceding A7),
    // rather than the LF15 or the unrelated staged EOF20.
    let (mut s, sid) = fixture("A\n");
    select(&mut s, sid, 0, 2);
    format(&mut s, json!({"size": 15}));
    select(&mut s, sid, 0, 1);
    format(&mut s, json!({"size": 7}));
    select(&mut s, sid, 2, 2);
    format(&mut s, json!({"size": 20}));
    assert_eq!(attrs(&mut s)["chars"]["size"], 20.0);
    select(&mut s, sid, 1, 2);
    assert_eq!(attrs(&mut s)["chars"]["size"], 15.0);
    let result = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
    assert_eq!(attrs(&mut s)["chars"]["size"], 7.0);
    let st = s.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(st.text, "A\n\u{E009}\n");
    assert_eq!(st.format_after(0).over.size, Some(7.0));
    let cell = &st.tables[&result["table"].as_u64().unwrap()].cells[0];
    assert_eq!(cell.text.chars[0].format.over.size, Some(7.0));
    assert!(cell.text.paras[0].eof_inherited_chars.is_none());
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn eof_tracked_replacement_preserves_selected_lf_format_and_caret_staging() {
    use designcraft_doc::{ChangeMark, Selection, TextSel};

    for (stage, caret_size, caret_query) in
        [(json!({"size": 20}), Some(20.0), 20.0), (json!({"size": null}), None, 12.0), (json!({}), Some(15.0), 15.0)]
    {
        for (anchor, focus) in [(1, 2), (2, 1), (2, 2)] {
            let selected_lf = anchor != focus;
            let (mut s, sid) = fixture("A\n");
            select(&mut s, sid, 0, 2);
            format(&mut s, json!({"size": 15}));
            let prior = s.doc().unwrap().doc.story(sid).unwrap().format_after(0).clone();
            select(&mut s, sid, 2, 2);
            if !stage.as_object().unwrap().is_empty() {
                format(&mut s, stage.clone());
            }
            assert_eq!(attrs(&mut s)["chars"]["size"], caret_query);
            select(&mut s, sid, anchor, focus);
            assert_eq!(attrs(&mut s)["chars"]["size"], if selected_lf { 15.0 } else { caret_query });
            s.execute("changes.track", &json!({"on": true})).unwrap();
            let result = s.execute("text.insert", &json!({"text": "X", "raw": true})).unwrap();
            assert_eq!(result, json!({"pos": 3}));
            assert_eq!(s.doc().unwrap().selection, Selection::text(TextSel::caret(sid, 3)));
            let expected_size = if selected_lf { Some(15.0) } else { caret_size };
            let expected_query = if selected_lf { 15.0 } else { caret_query };
            assert_eq!(attrs(&mut s)["chars"]["size"], expected_query, "stage={stage}, selection={anchor}..{focus}");
            let mut inserted = prior.clone();
            inserted.over.size = expected_size;
            inserted.over.change = Some(ChangeMark::Inserted);
            let mut lf = prior.clone();
            if selected_lf {
                lf.over.change = Some(ChangeMark::Deleted);
            }
            let st = s.doc().unwrap().doc.story(sid).unwrap();
            assert_eq!(st.text, "A\nX");
            assert_eq!(st.format_after(0), &prior);
            assert_eq!(st.format_after(1), &lf);
            assert_eq!(st.format_after(2), &inserted);
            assert!(st.paras.iter().all(|p| p.eof_inherited_chars.is_none()));
            let mut changes = Vec::new();
            if selected_lf {
                changes.push(json!({"story": sid.0, "start": 1, "end": 2, "kind": "deleted", "text": "\n"}));
            }
            changes.push(json!({"story": sid.0, "start": 2, "end": 3, "kind": "inserted", "text": "X"}));
            assert_eq!(s.execute("changes.list", &json!({})).unwrap(), json!(changes));
            s.doc().unwrap().doc.check().unwrap();

            let result = s.execute("changes.acceptAll", &json!({"story": sid.0})).unwrap();
            assert_eq!(result, json!({"changes": if selected_lf { 2 } else { 1 }}));
            let (text, at, caret) = if selected_lf { ("AX", 1, 2) } else { ("A\nX", 2, 3) };
            assert_eq!(s.doc().unwrap().selection, Selection::text(TextSel::caret(sid, caret)));
            assert_eq!(attrs(&mut s)["chars"]["size"], expected_query);
            inserted.over.change = None;
            let st = s.doc().unwrap().doc.story(sid).unwrap();
            assert_eq!(st.text, text);
            assert_eq!(st.format_after(0), &prior);
            if !selected_lf {
                assert_eq!(st.format_after(1), &prior);
            }
            assert_eq!(st.format_after(at), &inserted);
            assert!(st.chars.iter().all(|r| r.format.over.change.is_none()));
            assert!(st.paras.iter().all(|p| p.eof_inherited_chars.is_none()));
            assert_eq!(s.execute("changes.list", &json!({})).unwrap(), json!([]));
            s.doc().unwrap().doc.check().unwrap();
        }
    }
}

fn assert_pending_size_continues_at_new_eof(pending_edit: Value, stored_size: Option<f64>, resolved_size: f64) {
    let (mut s, sid) = fixture("A");
    assert_eq!(attrs(&mut s)["chars"]["size"], 12.0);
    select(&mut s, sid, 0, 1);
    format(&mut s, json!({"size": 15}));
    select(&mut s, sid, 1, 1);
    let before = s.doc().unwrap().doc.clone();
    let original = before.story(sid).unwrap().format_after(0).clone();
    let undo = s.doc().unwrap().history.undo.len();
    format(&mut s, pending_edit.clone());
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before), "pending formatting must leave the entire document unchanged");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo);
    assert_eq!(attrs(&mut s)["chars"]["size"], json!(resolved_size));
    assert!(super::typing_format(&s).is_some());

    assert_eq!(s.execute("text.insert", &json!({"text": "\n", "raw": true})).unwrap(), json!({"pos": 2}));
    assert!(super::typing_format(&s).is_none());
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    let after_lf = s.doc().unwrap().doc.clone();
    let story = after_lf.story(sid).unwrap();
    assert_eq!(story.text, "A\n");
    assert_eq!(story.format_after(0), &original);
    assert_eq!(story.format_after(1).over.size, stored_size, "the inserted LF already has the pending edit");
    let eof_size = attrs(&mut s)["chars"]["size"].clone();
    let mut back = reopen(&mut s);
    select(&mut back, sid, 2, 2);
    let reopened_eof_size = attrs(&mut back)["chars"]["size"].clone();

    assert_eq!(s.execute("text.insert", &json!({"text": "X", "raw": true})).unwrap(), json!({"pos": 3}));
    assert_eq!(back.execute("text.insert", &json!({"text": "X", "raw": true})).unwrap(), json!({"pos": 3}));
    let typed = s.doc().unwrap().doc.clone();
    let story = typed.story(sid).unwrap();
    assert_eq!(story.text, "A\nX");
    assert_eq!(story.format_after(0), &original);
    assert_eq!(story.format_after(1).over.size, stored_size);
    let inserted_size = story.format_after(2).over.size;
    let native_story = back.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(native_story.text, "A\nX");
    assert_eq!(native_story.format_after(0), &original);
    assert_eq!(native_story.format_after(1).over.size, stored_size);
    let reopened_inserted_size = native_story.format_after(2).over.size;
    assert_eq!(
        (eof_size, inserted_size, reopened_eof_size, reopened_inserted_size),
        (json!(resolved_size), stored_size, json!(resolved_size), stored_size),
        "pending edit {pending_edit} must continue after Enter, including native reopen"
    );
    assert_eq!(attrs(&mut s)["chars"]["size"], json!(resolved_size));
    assert_eq!(attrs(&mut back)["chars"]["size"], json!(resolved_size));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after_lf));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &typed));
    s.doc().unwrap().doc.check().unwrap();
    back.doc().unwrap().doc.check().unwrap();
}

#[test]
fn eof_pending_size_survives_newline_and_native_continuation() {
    assert_pending_size_continues_at_new_eof(json!({"size": 20}), Some(20.0), 20.0);
}

#[test]
fn eof_pending_size_clear_survives_newline_and_native_continuation() {
    assert_pending_size_continues_at_new_eof(json!({"size": null}), None, 12.0);
}

fn assert_eof_color_follows_promotion(target: &str) {
    let (mut s, sid) = fixture("A\n");
    let prior = s.doc().unwrap().doc.story(sid).unwrap().clone();
    let old = s.execute("object.color", &json!({"color": "#123456", "target": target})).unwrap()["swatch"].as_str().unwrap().to_string();
    assert!(s.doc().unwrap().doc.swatch(&old).is_some());
    assert!(super::typing_format(&s).is_none(), "outer blank EOF color is document-resident");
    let story = s.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(story.text, prior.text);
    assert_eq!(story.chars, prior.chars);
    assert_eq!(story.paras[0], prior.paras[0]);
    assert_eq!(serde_json::to_value(&story.paras[1].chars).unwrap()[target], json!(old));
    assert_eq!(attrs(&mut s)["chars"][target], json!(old));
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();

    assert_eq!(s.execute("swatch.addToSwatches", &json!({"swatch": old, "name": "Brand"})).unwrap(), json!({"name": "Brand"}));
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    assert!(s.doc().unwrap().doc.swatch(&old).is_none());
    assert!(s.doc().unwrap().doc.swatch("Brand").is_some());
    let promoted = s.doc().unwrap().doc.clone();
    let eof_color = attrs(&mut s)["chars"][target].clone();
    let mut back = reopen(&mut s);
    select(&mut back, sid, 2, 2);
    let reopened_eof_color = attrs(&mut back)["chars"][target].clone();

    s.execute("text.insert", &json!({"text": "X", "raw": true})).unwrap();
    back.execute("text.insert", &json!({"text": "X", "raw": true})).unwrap();
    let story = s.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(story.text, "A\nX");
    assert_eq!(story.format_after(0), prior.format_after(0));
    assert_eq!(story.format_after(1), prior.format_after(1));
    let inserted_color = serde_json::to_value(&story.format_after(2).over).unwrap()[target].clone();
    let native_story = back.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(native_story.text, "A\nX");
    assert_eq!(native_story.format_after(0), prior.format_after(0));
    assert_eq!(native_story.format_after(1), prior.format_after(1));
    let reopened_inserted_color = serde_json::to_value(&native_story.format_after(2).over).unwrap()[target].clone();
    assert_eq!(
        (eof_color, inserted_color, reopened_eof_color, reopened_inserted_color),
        (json!("Brand"), json!("Brand"), json!("Brand"), json!("Brand")),
        "promoted {target} must update persistent EOF typing and subsequent insertion"
    );
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &promoted));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &promoted));
    select(&mut s, sid, 2, 2);
    assert_eq!(attrs(&mut s)["chars"][target], "Brand");
    s.doc().unwrap().doc.check().unwrap();
    back.doc().unwrap().doc.check().unwrap();
}

#[test]
fn eof_promoted_fill_survives_native_reopen_and_typing() {
    assert_eof_color_follows_promotion("fill");
}

#[test]
fn eof_promoted_stroke_survives_native_reopen_and_typing() {
    assert_eof_color_follows_promotion("stroke");
}

#[test]
fn eof_promoted_swatch_updates_inherited_character_defaults() {
    let (mut s, sid) = fixture("A\n");
    let old = s.execute("object.color", &json!({"color": "#123456"})).unwrap()["swatch"].as_str().unwrap().to_string();
    // Like the existing legacy-defaults fixture, load genuine paragraph character defaults.
    // Staging a size then moves these references into eof_inherited_chars.
    let mut legacy = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
    let paragraph = &mut legacy["stories"][sid.0.to_string()]["paras"][1];
    paragraph["chars"] = json!({"fill": old, "stroke": old});
    paragraph.as_object_mut().unwrap().remove("eofInheritedChars");
    let mut s = open(&serde_json::to_vec(&legacy).unwrap());
    select(&mut s, sid, 2, 2);
    format(&mut s, json!({"size": 20}));
    let story = s.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(story.paras[1].chars.size, Some(20.0));
    assert_eq!(story.paras[1].chars.fill, None);
    assert_eq!(story.paras[1].chars.stroke, None);
    assert_eq!(serde_json::to_value(story.paras[1].eof_inherited_chars.as_ref().unwrap()).unwrap(), json!({"fill": old, "stroke": old}));
    assert!(story.chars.iter().all(|run| run.format.over.fill.is_none() && run.format.over.stroke.is_none()));
    let before = s.doc().unwrap().doc.clone();
    let undo = s.doc().unwrap().history.undo.len();

    s.execute("swatch.addToSwatches", &json!({"swatch": old, "name": "Brand"})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    assert!(s.doc().unwrap().doc.swatch(&old).is_none());
    assert!(s.doc().unwrap().doc.swatch("Brand").is_some());
    let promoted = s.doc().unwrap().doc.clone();
    let eof_attrs = attrs(&mut s);
    let inherited = serde_json::to_value(&promoted.story(sid).unwrap().paras[1].eof_inherited_chars).unwrap();
    let mut back = reopen(&mut s);
    select(&mut back, sid, 2, 2);
    back.execute("text.insert", &json!({"text": "X", "raw": true})).unwrap();
    let after_attrs = attrs(&mut back);
    let story = back.doc().unwrap().doc.story(sid).unwrap();
    assert_eq!(story.text, "A\nX");
    assert_eq!(story.format_after(0), before.story(sid).unwrap().format_after(0));
    assert_eq!(story.format_after(1), before.story(sid).unwrap().format_after(1));
    assert_eq!(story.format_after(2).over.size, Some(20.0));
    assert_eq!(story.format_after(2).over.fill, None);
    assert_eq!(story.format_after(2).over.stroke, None);
    assert!(story.paras[1].eof_inherited_chars.is_none());
    let restored = serde_json::to_value(&story.paras[1].chars).unwrap();
    assert_eq!(
        (eof_attrs["chars"]["fill"].clone(), eof_attrs["chars"]["stroke"].clone(), inherited),
        (json!("Brand"), json!("Brand"), json!({"fill": "Brand", "stroke": "Brand"})),
        "promotion must update references held only in the EOF inheritance carrier"
    );
    assert_eq!(restored, json!({"fill": "Brand", "stroke": "Brand"}));
    assert_eq!(after_attrs["chars"]["fill"], "Brand");
    assert_eq!(after_attrs["chars"]["stroke"], "Brand");
    assert_eq!(after_attrs["chars"]["size"], 20.0);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &promoted));
    s.doc().unwrap().doc.check().unwrap();
    back.doc().unwrap().doc.check().unwrap();
}

#[test]
fn eof_enter_preserves_promoted_empty_paragraph_leading() {
    let (mut s, sid) = fixture("A\n");
    select(&mut s, sid, 0, 2);
    format(&mut s, json!({"leading": {"kind": "points", "value": 18}}));
    select(&mut s, sid, 2, 2);
    format(&mut s, json!({"leading": {"kind": "points", "value": 36}}));
    let metrics = |s: &Session| {
        s.cache
            .get(&s.doc().unwrap().doc, sid, None)
            .frames
            .iter()
            .flat_map(|f| &f.lines)
            .map(|line| (line.para, line.range.clone(), line.leading))
            .collect::<Vec<_>>()
    };
    let mut native = reopen(&mut s);
    for s in [&mut s, &mut native] {
        select(s, sid, 2, 2);
        let before = s.doc().unwrap().doc.clone();
        assert_eq!(metrics(s), vec![(0, 0..1, 18.0), (1, 2..2, 36.0)]);
        s.execute("text.insert", &json!({"text": "\n", "raw": true})).unwrap();
        let after = s.doc().unwrap().doc.clone();
        let story = after.story(sid).unwrap();
        assert_eq!(story.text, "A\n\n");
        assert_eq!(story.format_after(0), before.story(sid).unwrap().format_after(0));
        assert_eq!(story.format_after(1), before.story(sid).unwrap().format_after(1));
        assert_eq!(
            metrics(s),
            vec![(0, 0..1, 18.0), (1, 2..2, 36.0), (2, 3..3, 36.0)],
            "Enter must preserve the former EOF's composed leading, not only the new EOF"
        );
        assert!(story.paras[1].eof_inherited_chars.is_none());
        assert!(story.paras[1].chars.is_empty());
        assert!(story.paras[2].eof_inherited_chars.is_some());
        let back = reopen(s);
        assert_eq!(metrics(&back), vec![(0, 0..1, 18.0), (1, 2..2, 36.0), (2, 3..3, 36.0)]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        assert_eq!(metrics(s), vec![(0, 0..1, 18.0), (1, 2..2, 36.0)]);
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
        assert_eq!(metrics(s), vec![(0, 0..1, 18.0), (1, 2..2, 36.0), (2, 3..3, 36.0)]);
        select(s, sid, 3, 3);
        s.execute("text.insert", &json!({"text": "\n", "raw": true})).unwrap();
        assert_eq!(metrics(s), vec![(0, 0..1, 18.0), (1, 2..2, 36.0), (2, 3..3, 36.0), (3, 4..4, 36.0)]);
        s.doc().unwrap().doc.check().unwrap();
        back.doc().unwrap().doc.check().unwrap();
    }
}

#[test]
fn eof_append_and_thread_preserve_both_stories_blank_leading() {
    let metrics = |s: &Session, sid: StoryId| {
        s.cache
            .get(&s.doc().unwrap().doc, sid, None)
            .frames
            .iter()
            .flat_map(|f| &f.lines)
            .map(|line| (line.para, line.range.clone(), line.leading))
            .collect::<Vec<_>>()
    };
    for direct_append in [true, false] {
        let (mut s, sid) = fixture("A\n");
        let from = s.doc().unwrap().doc.story(sid).unwrap().frames[0];
        select(&mut s, sid, 0, 2);
        format(&mut s, json!({"leading": {"kind": "points", "value": 18}}));
        select(&mut s, sid, 2, 2);
        format(&mut s, json!({"leading": {"kind": "points", "value": 36}}));
        let frame = s.execute("frame.create", &json!({"rect": [40, 520, 440, 740], "content": "text", "text": "B\n"})).unwrap();
        let other_sid = StoryId(frame["story"].as_u64().unwrap());
        select(&mut s, other_sid, 0, 2);
        format(&mut s, json!({"leading": {"kind": "points", "value": 22}}));
        select(&mut s, other_sid, 2, 2);
        format(&mut s, json!({"leading": {"kind": "points", "value": 42}}));
        let incoming = s.doc().unwrap().doc.story(other_sid).unwrap().clone();
        let original = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert_eq!(metrics(&s, sid), vec![(0, 0..1, 18.0), (1, 2..2, 36.0)]);
        assert_eq!(metrics(&s, other_sid), vec![(0, 0..1, 22.0), (1, 2..2, 42.0)]);

        // Empty appends must neither consume a stage nor invent a separator.
        let mut unchanged = original.clone();
        unchanged.append_story(designcraft_doc::Story::with_text(StoryId(0), "", Default::default()));
        assert_eq!(unchanged, original);
        let (mut empty, empty_sid) = fixture("");
        empty
            .edit(|d, _| {
                d.story_mut(empty_sid).unwrap().append_story(incoming.clone());
                Ok(())
            })
            .unwrap();
        assert_eq!(empty.doc().unwrap().doc.story(empty_sid).unwrap().text, "B\n");
        assert_eq!(empty.doc().unwrap().doc.story(empty_sid).unwrap().paras, incoming.paras);
        assert_eq!(metrics(&empty, empty_sid), vec![(0, 0..1, 22.0), (1, 2..2, 42.0)]);
        empty.doc().unwrap().doc.check().unwrap();

        let before = s.doc().unwrap().doc.clone();
        if direct_append {
            s.edit(|d, _| {
                d.story_mut(sid).unwrap().append_story(incoming.clone());
                Ok(())
            })
            .unwrap();
        } else {
            s.execute("text.thread", &json!({"from": from.0, "to": frame["id"]})).unwrap();
            assert!(s.doc().unwrap().doc.story(other_sid).is_none());
        }
        let after = s.doc().unwrap().doc.clone();
        let story = after.story(sid).unwrap();
        assert_eq!(story.text, "A\n\nB\n");
        assert_eq!(story.format_after(0), original.format_after(0));
        assert_eq!(story.format_after(1), original.format_after(1));
        assert_eq!(story.format_after(3), incoming.format_after(0));
        let expected = vec![(0, 0..1, 18.0), (1, 2..2, 36.0), (2, 3..4, 22.0), (3, 5..5, 42.0)];
        assert_eq!(metrics(&s, sid), expected, "direct_append={direct_append}: both blank paragraphs must keep their own leading");
        assert!(story.paras[1].eof_inherited_chars.is_none());
        assert!(story.paras[1].chars.is_empty());
        assert!(story.paras[3].eof_inherited_chars.is_some());
        let back = reopen(&mut s);
        assert_eq!(metrics(&back, sid), expected);
        if !direct_append {
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
            assert_eq!(metrics(&s, sid), vec![(0, 0..1, 18.0), (1, 2..2, 36.0)]);
            assert_eq!(metrics(&s, other_sid), vec![(0, 0..1, 22.0), (1, 2..2, 42.0)]);
            s.execute("edit.redo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
            assert_eq!(metrics(&s, sid), expected);
        }
        s.doc().unwrap().doc.check().unwrap();
        back.doc().unwrap().doc.check().unwrap();
    }
}

#[test]
fn eof_promoted_empty_leading_keeps_live_style_and_explicit_clear() {
    for direct_append in [false, true] {
        for clear_before in [false, true] {
            let (mut s, sid) = fixture("A\n");
            select(&mut s, sid, 0, 2);
            if clear_before {
                format(&mut s, json!({"leading": {"kind": "points", "value": 36}}));
            } else {
                s.execute("style.character.create", &json!({"name": "Live", "chars": {"leading": {"kind": "points", "value": 30}}})).unwrap();
                s.execute("style.character.apply", &json!({"name": "Live"})).unwrap();
            }
            // Genuine paragraph defaults stay below the live character style and direct LF format.
            let mut legacy = serde_json::to_value(&*s.doc().unwrap().doc).unwrap();
            legacy["stories"][sid.0.to_string()]["paras"][1]["chars"] = json!({"leading": {"kind": "points", "value": 24}});
            let mut s = open(&serde_json::to_vec(&legacy).unwrap());
            let leading = |s: &Session| {
                let composed = s.cache.get(&s.doc().unwrap().doc, sid, None);
                let mut lines = composed.frames.iter().flat_map(|f| &f.lines).filter(|line| line.para == 1 && line.range == (2..2));
                let leading = lines.next().unwrap().leading;
                assert!(lines.next().is_none(), "the promoted paragraph must compose exactly once");
                leading
            };
            select(&mut s, sid, 2, 2);
            if clear_before {
                format(&mut s, json!({"leading": null}));
                assert!(s.doc().unwrap().doc.story(sid).unwrap().paras[1].chars.is_empty());
            } else {
                format(&mut s, json!({"leading": {"kind": "points", "value": 36}}));
            }
            let expected = if clear_before { 24.0 } else { 36.0 };
            assert_eq!(leading(&s), expected);
            if direct_append {
                s.edit(|d, _| {
                    d.story_mut(sid).unwrap().append_story(designcraft_doc::Story::with_text(StoryId(0), "B", Default::default()));
                    Ok(())
                })
                .unwrap();
            } else {
                s.execute("text.insert", &json!({"text": "\n", "raw": true})).unwrap();
            }
            assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, if direct_append { "A\n\nB" } else { "A\n\n" });
            assert_eq!(leading(&s), expected, "direct_append={direct_append}, clear_before={clear_before}: the LF owns the blank's metrics");
            let story = s.doc().unwrap().doc.story(sid).unwrap();
            assert!(story.paras[1].eof_inherited_chars.is_none());
            assert_eq!(story.paras[1].chars.leading, Some(Leading::Points(24.0)));
            if clear_before {
                assert_eq!(story.format_after(1).over.leading, Some(Leading::Points(36.0)));
                assert_eq!(story.format_after(2).over.leading, None);
            } else {
                assert_eq!(story.format_after(2).style, "Live");
                select(&mut s, sid, 2, 3);
                format(&mut s, json!({"leading": null}));
                assert_eq!(leading(&s), 30.0, "clearing direct leading must expose the live character style");
                s.execute("style.character.edit", &json!({"name": "Live", "chars": {"leading": {"kind": "points", "value": 32}}})).unwrap();
                assert_eq!(leading(&s), 32.0, "the separator's character style must remain live");
                s.execute("style.character.edit", &json!({"name": "Live", "chars": {"leading": null}})).unwrap();
                assert_eq!(leading(&s), 24.0, "removing style leading must expose genuine paragraph defaults");
            }
            let back = reopen(&mut s);
            assert_eq!(leading(&back), 24.0);
            s.doc().unwrap().doc.check().unwrap();
            back.doc().unwrap().doc.check().unwrap();
        }
    }
}
