use serde_json::json;

use super::*;

fn session_with_frame() -> (Session, u64, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"pages": 1})).unwrap();
    let r = s.execute("frame.create", &json!({"rect": [36, 36, 336, 500], "content": "text"})).unwrap();
    let sid = r["story"].as_u64().unwrap();
    let fid = r["id"].as_u64().unwrap();
    s.execute("text.insert", &json!({"text": "Intro"})).unwrap();
    (s, sid, fid)
}

fn check(s: &Session) {
    s.doc().unwrap().doc.check().unwrap();
}

#[test]
fn table_text_replacement_floors_utf8_selection_and_allows_quote_typing() {
    for (original, (a, b), replacement, (after_a, after_b), quoted) in [
        ("a", (1, 1), "é", (0, 0), Some("“é")), // Exact caret/quote baseline.
        ("abc", (3, 3), "🙂", (0, 0), Some("“🙂")),
        ("ab", (1, 2), "éX", (0, 2), None),
        ("ab", (2, 1), "éX", (2, 0), None),
        ("abcdef", (3, 5), "🙂éX", (0, 4), None),
        ("abcdef", (5, 3), "🙂éX", (4, 0), None),
        ("abc", (1, 2), "🙂", (0, 0), None),     // Both endpoints floor to the same boundary.
        ("abcdef", (2, 6), "é🙂", (2, 6), None), // Valid Unicode endpoints stay fixed.
        ("abcdef", (1, 4), "ABCDEF", (1, 4), None),
        ("abcdef", (4, 1), "ABCDEF", (4, 1), None),
        ("abcdef", (6, 6), "é", (2, 2), None), // Shortening clamps to EOF.
        ("abc", (1, 3), "", (0, 0), None),
    ] {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let frame = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": ""})).unwrap();
        let sid = frame["story"].as_u64().unwrap();
        let fid = frame["id"].as_u64().unwrap();
        let tid = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap()["table"].as_u64().unwrap();
        s.execute("text.insert", &json!({"text": original, "raw": true})).unwrap();
        // Real commands position the child selection; text.select addresses parent stories.
        for _ in a..original.len() {
            s.execute("text.move", &json!({"dir": "left"})).unwrap();
        }
        let (dir, steps) = if b >= a { ("right", b - a) } else { ("left", a - b) };
        for _ in 0..steps {
            s.execute("text.move", &json!({"dir": dir, "extend": true})).unwrap();
        }
        let mut expected = s.doc().unwrap().selection.clone();
        let selected = expected.text.as_mut().unwrap();
        assert_eq!((selected.anchor, selected.focus), (a, b));
        assert_eq!(selected.frame, Some(doc::ItemId(fid)));
        assert_eq!(selected.cell, Some(doc::CellAddr { table: tid, row: 0, col: 0 }));
        (selected.anchor, selected.focus) = (after_a, after_b);
        let parent = s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().text.clone();
        s.execute("table.setCell", &json!({"text": replacement})).unwrap();
        assert_eq!(table_for(&s, sid, tid).cell(0, 0).unwrap().text.text, replacement);
        assert_eq!(s.doc().unwrap().selection, expected, "{original}: {a}->{b}, replacement {replacement}");
        assert!(replacement.is_char_boundary(after_a) && replacement.is_char_boundary(after_b));
        assert_eq!(s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().text, parent);
        if let Some(quoted) = quoted {
            assert!(s.prefs.typographers_quotes);
            s.execute("text.insert", &json!({"text": "\""})).unwrap();
            assert_eq!(table_for(&s, sid, tid).cell(0, 0).unwrap().text.text, quoted);
            let selected = expected.text.as_mut().unwrap();
            (selected.anchor, selected.focus) = (3, 3); // Opening curly quote is three bytes.
            assert_eq!(s.doc().unwrap().selection, expected);
        }
        check(&s);
    }
}

#[test]
fn table_edit_after_anchor_removal_floors_parent_caret() {
    for (original, (a, b), replacement, pos, quoted, typed_pos) in [("ab", (1, 2), "é", 0, "“é\n", 3), ("abcde", (5, 1), "é🙂", 2, "é”🙂\n", 5)]
    {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let other = s.execute("frame.create", &json!({"rect": [320, 36, 580, 200], "content": "text"})).unwrap();
        let other_sid = other["story"].as_u64().unwrap();
        let other_tid = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap()["table"].as_u64().unwrap();
        s.execute("text.insert", &json!({"text": "keep", "raw": true})).unwrap();
        let mut expected_other = table_for(&s, other_sid, other_tid).clone();
        expected_other.cell_mut(0, 0).unwrap().insets = [7.0; 4];

        let frame = s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
        let sid = frame["story"].as_u64().unwrap();
        let tid = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap()["table"].as_u64().unwrap();
        s.execute("text.insert", &json!({"text": original, "raw": true})).unwrap();
        for _ in a..original.len() {
            s.execute("text.move", &json!({"dir": "left"})).unwrap();
        }
        let (dir, steps) = if b >= a { ("right", b - a) } else { ("left", a - b) };
        for _ in 0..steps {
            s.execute("text.move", &json!({"dir": dir, "extend": true})).unwrap();
        }
        let mut expected = s.doc().unwrap().selection.clone();
        let selected = expected.text.unwrap();
        assert_eq!((selected.anchor, selected.focus), (a, b));
        assert_eq!(selected.frame, Some(doc::ItemId(frame["id"].as_u64().unwrap())));
        assert_eq!(selected.cell, Some(doc::CellAddr { table: tid, row: 0, col: 0 }));
        // Explicit parent replacement removes its anchor, leaving the child address stale.
        assert_eq!(
            s.execute("find.change", &json!({"story": sid, "find": doc::TABLE_ANCHOR.to_string(), "change": replacement})).unwrap()["count"],
            1
        );
        let parent = s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap();
        assert_eq!(parent.text, format!("{replacement}\n"));
        assert!(!parent.tables.contains_key(&tid));
        assert_eq!(s.doc().unwrap().selection, expected);
        assert!(!parent.text.is_char_boundary(selected.anchor));
        // An explicit edit to the surviving table reaches the disappeared-table fallback.
        s.execute("table.setCell", &json!({"story": other_sid, "table": other_tid, "insets": 7})).unwrap();
        let selected = expected.text.as_mut().unwrap();
        (selected.anchor, selected.focus, selected.cell) = (pos, pos, None); // Both floor the OLD anchor.
        assert_eq!(s.doc().unwrap().selection, expected);
        assert_eq!(s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().text, format!("{replacement}\n"));
        assert_eq!(table_for(&s, other_sid, other_tid), &expected_other);
        s.execute("text.insert", &json!({"text": "\""})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().text, quoted);
        let selected = expected.text.as_mut().unwrap();
        (selected.anchor, selected.focus) = (typed_pos, typed_pos);
        assert_eq!(s.doc().unwrap().selection, expected);
        assert_eq!(table_for(&s, other_sid, other_tid), &expected_other);
        check(&s);
    }
}

fn selected_merged_table(column: bool) -> (Session, u64, u64, u64) {
    let (mut s, sid, fid) = session_with_frame();
    let tid = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    for (i, text) in ["A", "B", "C", "D"].iter().enumerate() {
        s.execute("text.insert", &json!({"text": text})).unwrap();
        if i < 3 {
            s.execute("table.nextCell", &json!({})).unwrap();
        }
    }
    let merge = if column { json!({"rows": [0, 0], "cols": [0, 1]}) } else { json!({"rows": [0, 1], "cols": [0, 0]}) };
    s.execute("table.select", &merge).unwrap();
    s.execute("table.merge", &json!({})).unwrap();
    s.execute("table.select", &json!({"rows": [1, 1], "cols": [1, 1]})).unwrap();
    s.execute(if column { "table.selectColumn" } else { "table.selectRow" }, &json!({})).unwrap();
    (s, sid, tid, fid)
}

#[test]
fn selected_merged_owners_receive_text_formatting_and_character_styles() {
    for column in [true, false] {
        let (mut s, sid, tid, _) = selected_merged_table(column);
        let selected = s.doc().unwrap().selection.clone();
        assert_eq!(selected.cells.unwrap().range, if column { doc::CellRange::new(0, 1, 1, 1) } else { doc::CellRange::new(1, 0, 1, 1) });
        let before = table_for(&s, sid, tid).clone();
        let neighbor = if column { (1, 0) } else { (0, 1) };
        s.execute("type.char", &json!({"attrs": {"size": 23}})).unwrap();
        for (r, c) in [(0, 0), (1, 1)] {
            let cell = table_for(&s, sid, tid).cell(r, c).unwrap();
            assert!(cell.text.runs().all(|(_, f)| f.over.size == Some(23.0)), "all runs of owner {r},{c}");
            assert_eq!(cell.text.text, before.cell(r, c).unwrap().text.text);
        }
        assert_eq!(table_for(&s, sid, tid).cell(neighbor.0, neighbor.1), before.cell(neighbor.0, neighbor.1));
        assert_eq!(s.doc().unwrap().selection, selected);
        s.execute("style.character.create", &json!({"name": "Selected owners", "chars": {"tracking": 37}})).unwrap();
        s.execute("style.character.apply", &json!({"name": "Selected owners"})).unwrap();
        for (r, c) in [(0, 0), (1, 1)] {
            assert!(table_for(&s, sid, tid).cell(r, c).unwrap().text.runs().all(|(_, f)| f.style == "Selected owners"));
        }
        let after = table_for(&s, sid, tid);
        assert_eq!(after.cell(neighbor.0, neighbor.1), before.cell(neighbor.0, neighbor.1));
        assert_eq!(after.rows, before.rows);
        assert_eq!(after.columns, before.columns);
        assert_eq!(s.doc().unwrap().selection, selected, "formatting preserves the original grid rectangle");
        check(&s);
    }
}

#[test]
fn selected_merged_owners_receive_cell_options_and_unique_style_counts() {
    for column in [true, false] {
        let (mut s, sid, tid, _) = selected_merged_table(column);
        let range = s.doc().unwrap().selection.cells.unwrap().range;
        let before = table_for(&s, sid, tid).clone();
        let neighbor = if column { (1, 0) } else { (0, 1) };
        let covered = if column { (0, 1) } else { (1, 0) };
        s.execute("table.setCell", &json!({"fill": "[Black]", "insets": 7, "text": "REPLACED"})).unwrap();
        for (r, c) in [(0, 0), (1, 1)] {
            let cell = table_for(&s, sid, tid).cell(r, c).unwrap();
            assert_eq!(serde_json::to_value(&cell.fill).unwrap(), json!("[Black]"));
            assert_eq!(cell.insets, [7.0; 4]);
            assert_eq!(cell.text.text, "REPLACED");
        }
        s.execute("style.cell.create", &json!({"name": "Selected cell owners", "insets": 9})).unwrap();
        assert_eq!(s.execute("style.cell.apply", &json!({"name": "Selected cell owners"})).unwrap()["cells"], 2);
        let after = table_for(&s, sid, tid);
        for (r, c) in [(0, 0), (1, 1)] {
            assert_eq!(after.cell(r, c).unwrap().style, "Selected cell owners");
            assert_eq!(after.cell(r, c).unwrap().insets, [9.0; 4]);
        }
        assert_eq!(after.cell(neighbor.0, neighbor.1), before.cell(neighbor.0, neighbor.1));
        assert_eq!(after.cell(covered.0, covered.1), before.cell(covered.0, covered.1));
        assert_eq!(after.rows, before.rows);
        assert_eq!(after.columns, before.columns);
        assert_eq!(s.doc().unwrap().selection.cells.unwrap().range, range);
        let merged = if column { json!({"rows": [0, 0], "cols": [0, 1]}) } else { json!({"rows": [0, 1], "cols": [0, 0]}) };
        s.execute("table.select", &merged).unwrap();
        assert_eq!(s.execute("style.cell.apply", &json!({"name": "Selected cell owners"})).unwrap()["cells"], 1);
        check(&s);
    }
}

#[test]
fn covered_owner_strokes_keep_literal_edge_masks_and_neighbors() {
    for rtl in [false, true] {
        for outer in [false, true] {
            let (mut s, sid, tid, _) = selected_merged_table(true);
            if rtl {
                s.execute("table.options", &json!({"direction": "rightToLeft"})).unwrap();
            }
            s.execute("table.select", &json!({"rows": [0, 0], "cols": [1, 1]})).unwrap();
            let before = table_for(&s, sid, tid).clone();
            s.execute("table.setCell", &json!({"stroke": {"weight": 3, "edges": if outer { "outer" } else { "inner" }}})).unwrap();
            let after = table_for(&s, sid, tid);
            let mask = if rtl { [true, true, true, false] } else { [true, false, true, true] };
            let expected = mask.map(|edge| if outer { edge } else { !edge });
            let owner = after.cell(0, 0).unwrap();
            assert_eq!(owner.border_overrides, expected);
            assert_eq!(owner.stroke_defined, expected);
            for (i, on) in expected.into_iter().enumerate() {
                if on {
                    assert_eq!(owner.strokes[i].weight, 3.0);
                    assert_eq!(owner.stroke_priorities[i], 1);
                } else {
                    assert_eq!(owner.strokes[i], before.cell(0, 0).unwrap().strokes[i]);
                }
            }
            for (r, c) in [(0, 1), (1, 0), (1, 1)] {
                assert_eq!(after.cell(r, c), before.cell(r, c), "strict merged/covered mirror guard preserves {r},{c}");
            }
            assert_eq!(s.doc().unwrap().selection.cells.unwrap().range, doc::CellRange::cell(0, 1));
            check(&s);
        }
    }
}

#[test]
fn cell_style_sampling_resolves_covered_point_but_retains_upper_left_policy() {
    let (mut s, sid, tid, _) = selected_merged_table(true);
    s.execute("table.setCell", &json!({"row": 0, "col": 0, "insets": 7})).unwrap();
    s.execute("table.select", &json!({"rows": [0, 0], "cols": [1, 1]})).unwrap();
    let selection = s.doc().unwrap().selection.clone();
    let before = table_for(&s, sid, tid).clone();
    s.execute("style.cell.create", &json!({"name": "Covered", "fromSelection": true})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.cell.iter().find(|c| c.name == "Covered").unwrap().insets, Some([7.0; 4]));
    assert_eq!(table_for(&s, sid, tid), &before);
    assert_eq!(s.doc().unwrap().selection, selection);

    let (mut s, sid, _) = session_with_frame();
    let tid = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    s.execute("table.select", &json!({"rows": [0, 1], "cols": [1, 1]})).unwrap();
    s.execute("table.merge", &json!({})).unwrap();
    for (row, col, insets, size) in [(0, 1, 7, 17), (1, 0, 11, 29)] {
        s.execute("table.setCell", &json!({"row": row, "col": col, "text": "Text", "insets": insets})).unwrap();
        s.execute("table.select", &json!({"rows": [row, row], "cols": [col, col]})).unwrap();
        s.execute("type.char", &json!({"attrs": {"size": size}})).unwrap();
    }
    s.execute("table.selectRow", &json!({})).unwrap();
    let before = table_for(&s, sid, tid).clone();
    let selection = s.doc().unwrap().selection.clone();
    // Row-major owners are [(0,1),(1,0)], while the rectangle starts at (1,0).
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 17.0);
    s.execute("style.cell.create", &json!({"name": "Upper left", "fromSelection": true})).unwrap();
    assert_eq!(s.doc().unwrap().doc.styles.cell.iter().find(|c| c.name == "Upper left").unwrap().insets, Some([11.0; 4]));
    assert_eq!(table_for(&s, sid, tid), &before);
    assert_eq!(s.doc().unwrap().selection, selection);
}

#[test]
fn selected_merged_owners_convert_graphics_back_to_text() {
    let png = designcraft_render::Rendered { width: 1, height: 1, pixels: vec![255, 0, 0, 255] }.to_png();
    let image = crate::cmd::base64_encode(&png);
    for column in [true, false] {
        let (mut s, sid, tid, _) = selected_merged_table(column);
        let neighbor = if column { (1, 0) } else { (0, 1) };
        let covered = if column { (0, 1) } else { (1, 0) };
        for (r, c) in [covered, (1, 1), neighbor] {
            let placed = s.execute("table.placeGraphic", &json!({"row": r, "col": c, "base64": image, "name": "cell.png"})).unwrap();
            if (r, c) == covered {
                assert_eq!((placed["row"].as_u64(), placed["col"].as_u64()), (Some(0), Some(0)));
            }
        }
        let before = table_for(&s, sid, tid).clone();
        let selected = s.doc().unwrap().selection.cells.unwrap();
        s.execute("table.textCell", &json!({})).unwrap();
        let after = table_for(&s, sid, tid);
        for (r, c) in [(0, 0), (1, 1)] {
            assert!(before.cell(r, c).unwrap().graphic.is_some());
            assert!(after.cell(r, c).unwrap().graphic.is_none());
            assert_eq!(after.cell(r, c).unwrap().text, before.cell(r, c).unwrap().text);
        }
        assert_eq!(after.cell(neighbor.0, neighbor.1), before.cell(neighbor.0, neighbor.1));
        assert_eq!(after.cell(covered.0, covered.1), before.cell(covered.0, covered.1));
        assert_eq!(s.doc().unwrap().selection.cells, Some(selected));
        check(&s);
    }
}

#[test]
fn covered_selected_owner_drag_keeps_merged_move_restrictions_atomic() {
    for column in [true, false] {
        let (mut s, sid, _, fid) = selected_merged_table(column);
        let before = s.doc().unwrap().doc.clone();
        let selection = s.doc().unwrap().selection.clone();
        let revision = s.doc().unwrap().revision;
        let undo = s.doc().unwrap().history.undo.len();
        let redo = s.doc().unwrap().history.redo.len();
        let cs = s.cache.get(&before, doc::StoryId(sid), None);
        let cells = &cs.frames[0].tables[0].cells;
        let point = |row, col| {
            let cell = cells.iter().find(|c| c.row == row && c.col == col).unwrap();
            before.text_xf(before.item(doc::ItemId(fid)).unwrap()) * cell.rect.center()
        };
        let from = point(0, 0);
        let to = if column { point(1, 0) } else { point(0, 1) };
        let err = s.execute("table.dropCells", &json!({"frame": fid, "from": [from.x, from.y], "to": [to.x, to.y]})).unwrap_err();
        assert!(err.to_string().contains(if column { "unmerge cells that span columns" } else { "unmerge cells that span rows" }), "{err}");
        let st = s.doc().unwrap();
        assert_eq!(st.doc, before);
        assert_eq!(st.selection, selection);
        assert_eq!(st.revision, revision);
        assert_eq!((st.history.undo.len(), st.history.redo.len()), (undo, redo));
    }
}

#[test]
fn insert_type_in_cells_and_tab() {
    let (mut s, sid, _) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 2, "cols": 3, "headerRows": 1})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    check(&s);
    // The caret is in the first cell; typing goes there.
    s.execute("text.insert", &json!({"text": "Name"})).unwrap();
    s.execute("text.insert", &json!({"text": "\t"})).unwrap();
    s.execute("text.insert", &json!({"text": "Value"})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!(t["table"], tid);
    assert_eq!(t["headerRows"], 1);
    assert_eq!(t["cells"][0]["text"], "Name");
    assert_eq!(t["cells"][1]["text"], "Value");
    // The story text holds only the anchor.
    let story = s.execute("story.get", &json!({"story": sid})).unwrap();
    assert!(story["text"].as_str().unwrap().contains(designcraft_doc::TABLE_ANCHOR));
    assert!(!story["text"].as_str().unwrap().contains("Name"));
    // Backspace edits the cell.
    s.execute("text.delete", &json!({})).unwrap();
    assert_eq!(s.execute("table.get", &json!({})).unwrap()["cells"][1]["text"], "Valu");
    // Tab in the last cell adds a row.
    for _ in 0..8 {
        s.execute("table.nextCell", &json!({})).unwrap();
    }
    assert_eq!(s.execute("table.get", &json!({})).unwrap()["rows"].as_array().unwrap().len(), 4);
    check(&s);
    // Undo restores the previous shape.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("table.get", &json!({"table": tid})).unwrap()["rows"].as_array().unwrap().len(), 3);
}

#[test]
fn rows_columns_merge_and_options() {
    let (mut s, _, _) = session_with_frame();
    s.execute("table.insert", &json!({"rows": 3, "cols": 3})).unwrap();
    s.execute("table.insertRow", &json!({"where": "below", "count": 2})).unwrap();
    s.execute("table.insertColumnLeft", &json!({})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!((t["rows"].as_array().unwrap().len(), t["columns"].as_array().unwrap().len()), (5, 4));
    // The caret's cell moved right with the inserted column.
    assert_eq!(s.doc().unwrap().selection.text.unwrap().cell.unwrap().col, 1);
    s.execute("table.select", &json!({"rows": [1, 2], "cols": [1, 2]})).unwrap();
    s.execute("table.merge", &json!({})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    let m = t["cells"].as_array().unwrap().iter().find(|c| c["row"] == 1 && c["col"] == 1).unwrap().clone();
    assert_eq!((m["rowSpan"].as_u64(), m["colSpan"].as_u64()), (Some(2), Some(2)));
    s.execute("table.unmerge", &json!({})).unwrap();
    check(&s);
    s.execute("table.select", &json!({"what": "row", "rows": [0, 0]})).unwrap();
    s.execute("table.setCell", &json!({"fill": "[Black]", "tint": 0.3, "insets": 6, "vj": "center", "stroke": {"weight": 2, "edges": "bottom"}}))
        .unwrap();
    s.execute("table.setRowHeight", &json!({"height": 30, "mode": "exactly"})).unwrap();
    s.execute("table.setColumnWidth", &json!({"cols": [0, 0], "width": 40})).unwrap();
    s.execute(
        "table.options",
        &json!({"headerRows": 1, "altRows": {"first": 1, "firstColor": "[Black]", "firstTint": 0.1, "next": 1}, "border": {"weight": 2}}),
    )
    .unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!(t["headerRows"], 1);
    assert_eq!(t["rows"][0]["mode"], "exactly");
    assert_eq!(t["rows"][0]["height"], 30.0);
    assert_eq!(t["columns"][0]["width"], 40.0);
    assert_eq!(t["cells"][0]["fill"], "[Black]");
    assert_eq!(t["options"]["border"]["weight"], 2.0);
    // Composition reflects the fixed row height.
    let st = s.doc().unwrap();
    let sid = st.selection.text.unwrap().story;
    let cs = s.cache.get(&st.doc, sid, None);
    let tf = &cs.frames[0].tables[0];
    assert!((tf.cell(0, 1).unwrap().rect.height() - 30.0).abs() < 1e-6);
    assert!(tf.cell(1, 0).unwrap().fill.is_some());
    // Delete a row and a column, then the whole table.
    s.execute("table.deleteRow", &json!({"rows": [4, 4]})).unwrap();
    s.execute("table.deleteColumn", &json!({"cols": [0, 0]})).unwrap();
    check(&s);
    let t = s.execute("table.get", &json!({})).unwrap();
    assert_eq!((t["rows"].as_array().unwrap().len(), t["columns"].as_array().unwrap().len()), (4, 3));
    s.execute("table.delete", &json!({})).unwrap();
    check(&s);
    let st = s.doc().unwrap();
    assert!(st.doc.stories.values().all(|x| x.tables.is_empty()));
    assert_eq!(st.doc.story(sid).unwrap().text, "Intro");
}

#[test]
fn convert_text_to_table_and_back() {
    let (mut s, sid, _) = session_with_frame();
    s.execute("story.setText", &json!({"story": sid, "text": "Ink\tCoverage\nPlum\t80%\nSunset\t45%\nAfter"})).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 25})).unwrap();
    let r = s.execute("table.convertFromText", &json!({})).unwrap();
    assert_eq!(r["rows"], 3);
    check(&s);
    let t = s.execute("table.get", &json!({"table": r["table"]})).unwrap();
    assert_eq!(t["columns"].as_array().unwrap().len(), 2);
    assert_eq!(t["cells"][3]["text"], "80%");
    let story = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
    assert_eq!(story, format!("{}\nAfter", designcraft_doc::TABLE_ANCHOR));
    s.execute("table.convertToText", &json!({"table": r["table"]})).unwrap();
    check(&s);
    assert_eq!(s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text, "Ink\tCoverage\nPlum\t80%\nSunset\t45%\nAfter");
}

#[test]
fn caret_placement_in_cells_and_cell_formatting() {
    let (mut s, sid, fid) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    s.execute("table.setCell", &json!({"rows": [1, 1], "cols": [1, 1], "text": "Hello"})).unwrap();
    // Click inside cell (1,1): spread coordinates = frame inner space here.
    let (rect, _) = {
        let st = s.doc().unwrap();
        let cs = s.cache.get(&st.doc, designcraft_doc::StoryId(sid), None);
        let c = cs.frames[0].tables[0].cell(1, 1).unwrap().clone();
        (c.rect, c)
    };
    let r = s.execute("text.placeCaret", &json!({"frame": fid, "point": [rect.x1 - 1.0, rect.center().y]})).unwrap();
    assert_eq!(r["cell"]["row"], 1);
    assert_eq!(r["cell"]["col"], 1);
    assert_eq!(r["pos"], 5);
    s.execute("text.insert", &json!({"text": "!"})).unwrap();
    assert_eq!(s.execute("table.get", &json!({"table": tid})).unwrap()["cells"][3]["text"], "Hello!");
    // Formatting at a cell caret/selection goes to the cell story.
    s.execute("edit.selectAll", &json!({})).unwrap();
    s.execute("type.char", &json!({"attrs": {"size": 20}})).unwrap();
    assert_eq!(s.execute("type.selectionAttrs", &json!({})).unwrap()["chars"]["size"], 20.0);
    check(&s);
    // Selected cells format whole cells.
    s.execute("table.select", &json!({"table": tid, "what": "table"})).unwrap();
    s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
    let st = s.doc().unwrap();
    let t = &st.doc.story(designcraft_doc::StoryId(sid)).unwrap().tables[&tid];
    assert!(t.cells.iter().all(|c| c.text.paras[0].para.align == Some(designcraft_doc::Align::Center)));
    // Typing after the table starts a new paragraph instead of hiding text in the anchor paragraph.
    let a = st.doc.story(designcraft_doc::StoryId(sid)).unwrap().table_anchor(tid).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": a + 3, "focus": a + 3})).unwrap();
    s.execute("text.insert", &json!({"text": "Tail"})).unwrap();
    check(&s);
    let text = s.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();
    assert!(text.contains(&format!("{}\nTail", designcraft_doc::TABLE_ANCHOR)), "{text:?}");
}

#[test]
fn file_roundtrip_keeps_tables() {
    let (mut s, sid, _) = session_with_frame();
    s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
    s.execute("text.insert", &json!({"text": "A1"})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let bytes = crate::cmd::file_bytes(&d);
    let back = crate::cmd::file_from(&bytes).unwrap();
    back.check().unwrap();
    let st = back.story(designcraft_doc::StoryId(sid)).unwrap();
    assert_eq!(st.tables.values().next().unwrap().cells[0].text.text, "A1");
}

#[test]
fn backspace_and_paste_between_tables_keep_both_and_the_file_reopens() {
    let n = designcraft_doc::TABLE_ANCHOR.len_utf8();
    let anchor_of = |s: &Session, sid: u64, tid: u64| s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().table_anchor(tid).unwrap();
    let reopens_with = |s: &Session, sid: u64, ids: &[u64]| {
        let back = crate::cmd::file_from(&crate::cmd::file_bytes(&s.doc().unwrap().doc)).unwrap();
        let st = back.story(doc::StoryId(sid)).unwrap();
        assert!(ids.iter().all(|id| st.table_anchor(*id).is_some()), "{:?}", st.tables.keys());
        assert_eq!(st.tables.len(), ids.len());
    };
    // Backspace at the start of the second of two consecutive tables.
    let (mut s, sid, _) = session_with_frame();
    let t1 = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    let len = s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().len();
    s.execute("text.select", &json!({"story": sid, "anchor": len})).unwrap();
    let t2 = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    let a2 = anchor_of(&s, sid, t2);
    s.execute("text.select", &json!({"story": sid, "anchor": a2})).unwrap();
    s.execute("text.delete", &json!({})).unwrap();
    check(&s);
    reopens_with(&s, sid, &[t1, t2]);
    // Pasting a copy of a table right after its anchor.
    let (mut s, sid, _) = session_with_frame();
    let t1 = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
    let a1 = anchor_of(&s, sid, t1);
    s.execute("text.select", &json!({"story": sid, "anchor": a1, "focus": a1 + n})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("text.select", &json!({"story": sid, "anchor": a1 + n})).unwrap();
    let pasted = s.execute("edit.paste", &json!({})).unwrap();
    check(&s);
    assert_eq!(pasted["pos"], a1 + 2 * n + 1);
    let st = s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().clone();
    let copy = st.tables.keys().copied().find(|id| *id != t1).unwrap();
    assert_eq!((st.table_anchor(t1), st.table_anchor(copy)), (Some(a1), Some(a1 + n + 1)));
    reopens_with(&s, sid, &[t1, copy]);
}

fn table_for(s: &Session, sid: u64, tid: u64) -> &doc::Table {
    s.doc().unwrap().doc.story(doc::StoryId(sid)).unwrap().tables.get(&tid).unwrap()
}

#[test]
fn explicit_cell_edges_win_imported_priorities_and_keep_undo_and_file_roundtrip() {
    for (direction, neighbors) in [
        (doc::TextDirection::LeftToRight, [(0, 1, 2), (1, 0, 3), (1, 2, 1), (2, 1, 0)]),
        (doc::TextDirection::RightToLeft, [(0, 1, 2), (1, 0, 1), (1, 2, 3), (2, 1, 0)]),
    ] {
        for imported_priority in [-7, 41, i32::MAX] {
            let (mut s, sid, _) = session_with_frame();
            let r = s.execute("table.insert", &json!({"rows": 3, "cols": 3})).unwrap();
            let tid = r["table"].as_u64().unwrap();
            s.edit(|d, _| {
                let t = d.story_mut(doc::StoryId(sid)).unwrap().table_mut(tid).unwrap();
                t.options.border = doc::CellStroke::none();
                t.options.direction = direction;
                for cell in &mut t.cells {
                    cell.strokes = std::array::from_fn(|_| doc::CellStroke::none());
                }
                t.cell_mut(0, 0).unwrap().stroke_priorities[0] = -9;
                for (row, col, edge) in neighbors {
                    let cell = t.cell_mut(row, col).unwrap();
                    cell.strokes[edge] = doc::CellStroke { weight: 7.0, ..Default::default() };
                    cell.stroke_defined[edge] = true;
                    cell.stroke_priorities[edge] = imported_priority;
                }
                Ok(())
            })
            .unwrap();
            let before = table_for(&s, sid, tid).clone();
            s.execute("table.setCell", &json!({"row": 1, "col": 1, "stroke": {"weight": 0, "edges": "all"}})).unwrap();
            let after = table_for(&s, sid, tid).clone();
            assert_eq!(after.cell(0, 0).unwrap().stroke_priorities[0], -9, "unrelated signed priorities are preserved");
            let edited = after.cell(1, 1).unwrap();
            let priority = if imported_priority == i32::MAX { 2 } else { imported_priority.max(0) + 1 };
            assert_eq!(edited.stroke_defined, [true; 4]);
            assert_eq!(edited.border_overrides, [true; 4]);
            assert_eq!(edited.stroke_priorities, [priority; 4]);
            for (row, col, edge) in neighbors {
                let neighbor = after.cell(row, col).unwrap();
                assert_eq!(neighbor.strokes[edge].weight, 0.0);
                assert!(neighbor.stroke_defined[edge]);
                assert!(neighbor.border_overrides[edge]);
                assert_eq!(neighbor.stroke_priorities[edge], priority);
            }
            let composed = s.cache.get(&s.doc().unwrap().doc, doc::StoryId(sid), None);
            assert!(composed.frames.iter().flat_map(|f| &f.tables).all(|t| t.strokes.is_empty()));
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(table_for(&s, sid, tid), &before);
            s.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(table_for(&s, sid, tid), &after);
            let bytes = crate::cmd::file_bytes(&s.doc().unwrap().doc);
            let back = crate::cmd::file_from(&bytes).unwrap();
            assert_eq!(back.story(doc::StoryId(sid)).unwrap().tables.get(&tid).unwrap().as_ref(), &after);
            check(&s);
        }
    }
}

#[test]
fn perimeter_edits_override_imported_sides_and_uniform_border_clears_side_options() {
    let (mut s, sid, _) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    s.edit(|d, _| {
        let t = d.story_mut(doc::StoryId(sid)).unwrap().table_mut(tid).unwrap();
        t.options.border.weight = 4.0;
        t.options.borders = std::array::from_fn(|_| Some(doc::CellStroke { weight: 9.0, ..Default::default() }));
        Ok(())
    })
    .unwrap();
    s.execute("table.setCell", &json!({"stroke": {"tint": 0.25, "edges": "left"}})).unwrap();
    let t = table_for(&s, sid, tid);
    let cell = t.cell(0, 0).unwrap();
    assert_eq!(cell.stroke_defined, [false, true, false, false]);
    assert_eq!(cell.border_overrides, [false, true, false, false]);
    assert_eq!(cell.strokes[1].weight, 9.0, "partial edits preserve the visible perimeter base");
    assert_eq!(cell.strokes[1].tint, 0.25);
    let before = t.clone();
    s.execute("table.options", &json!({"border": {"weight": 3}})).unwrap();
    let t = table_for(&s, sid, tid);
    assert!(t.options.borders.iter().all(Option::is_none));
    assert_eq!(t.options.border.weight, 3.0);
    let composed = s.cache.get(&s.doc().unwrap().doc, doc::StoryId(sid), None);
    let frag = composed.frames.iter().flat_map(|f| &f.tables).next().unwrap();
    assert_eq!(frag.strokes.iter().filter(|s| s.stroke.weight == 3.0).count(), 3);
    assert_eq!(frag.strokes.iter().filter(|s| s.stroke.weight == 9.0 && s.stroke.tint == 0.25).count(), 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(table_for(&s, sid, tid), &before);
}

#[test]
fn inherited_cell_and_table_styles_apply_and_follow_base_edits() {
    let (mut s, sid, _) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 1, "cols": 2})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    s.execute("style.cell.create", &json!({"name": "Cell Base", "insets": 7, "stroke": {"weight": 2}})).unwrap();
    s.execute("style.cell.create", &json!({"name": "Cell Child", "basedOn": "Cell Base", "fill": "[Paper]"})).unwrap();
    s.execute("style.cell.apply", &json!({"name": "Cell Child", "table": tid})).unwrap();
    let t = table_for(&s, sid, tid);
    assert!(t.cells.iter().all(|cell| cell.style == "Cell Child" && cell.insets == [7.0; 4] && cell.strokes[0].weight == 2.0));
    s.execute("style.cell.edit", &json!({"name": "Cell Base", "insets": 5})).unwrap();
    assert!(table_for(&s, sid, tid).cells.iter().all(|cell| cell.insets == [5.0; 4]));
    s.execute("style.table.create", &json!({"name": "Table Base", "body": "Cell Child", "border": {"weight": 6}, "spaceBefore": 8})).unwrap();
    s.execute("style.table.create", &json!({"name": "Table Child", "basedOn": "Table Base", "spaceAfter": 3})).unwrap();
    s.execute("style.table.apply", &json!({"name": "Table Child"})).unwrap();
    let t = table_for(&s, sid, tid);
    assert_eq!(t.style, "Table Child");
    assert_eq!(t.options.border_for(0).weight, 6.0);
    assert_eq!(t.options.space_before, 8.0);
    assert_eq!(t.options.space_after, 3.0);
    assert!(t.cells.iter().all(|cell| cell.style == "Cell Child" && cell.insets == [5.0; 4]));
    s.execute("style.table.edit", &json!({"name": "Table Base", "border": {"weight": 4}, "spaceBefore": 10})).unwrap();
    let t = table_for(&s, sid, tid);
    assert_eq!(t.options.border_for(0).weight, 4.0);
    assert_eq!(t.options.space_before, 10.0);
    assert_eq!(t.options.space_after, 3.0);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(table_for(&s, sid, tid).options.border_for(0).weight, 6.0);
}

#[test]
fn uniform_style_edits_replace_imported_per_side_attributes() {
    let (mut s, sid, _) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    s.edit(|d, _| {
        d.styles_mut().cell.push(doc::CellStyle {
            name: "Imported Cell".into(),
            strokes: std::array::from_fn(|_| doc::CellStrokeAttrs { weight: Some(9.0), ..Default::default() }),
            inset_overrides: [Some(12.0); 4],
            ..Default::default()
        });
        d.styles_mut().table.push(doc::TableStyle {
            name: "Imported Table".into(),
            borders: std::array::from_fn(|_| doc::CellStrokeAttrs { weight: Some(10.0), ..Default::default() }),
            ..Default::default()
        });
        Ok(())
    })
    .unwrap();
    s.execute("style.cell.apply", &json!({"name": "Imported Cell"})).unwrap();
    s.execute("style.cell.edit", &json!({"name": "Imported Cell", "stroke": {"weight": 2}, "insets": 3})).unwrap();
    let cell = table_for(&s, sid, tid).cell(0, 0).unwrap();
    assert!(cell.strokes.iter().all(|edge| edge.weight == 2.0));
    assert_eq!(cell.insets, [3.0; 4]);
    s.execute("style.table.apply", &json!({"name": "Imported Table"})).unwrap();
    s.execute("style.table.edit", &json!({"name": "Imported Table", "border": {"weight": 4}})).unwrap();
    let t = table_for(&s, sid, tid);
    assert!((0..4).all(|edge| t.options.border_for(edge).weight == 4.0));
}

#[test]
fn rtl_cell_edge_selection_uses_physical_sides() {
    let (mut s, sid, _) = session_with_frame();
    let r = s.execute("table.insert", &json!({"rows": 1, "cols": 3})).unwrap();
    let tid = r["table"].as_u64().unwrap();
    s.execute("table.options", &json!({"direction": "rightToLeft", "border": {"weight": 0}})).unwrap();
    s.execute("table.setCell", &json!({"cols": [0, 2], "stroke": {"weight": 0}})).unwrap();
    s.execute("table.setCell", &json!({"row": 0, "col": 0, "stroke": {"weight": 2, "edges": "left"}})).unwrap();
    let t = table_for(&s, sid, tid);
    assert_eq!(t.cell(0, 0).unwrap().strokes[1].weight, 2.0);
    assert_eq!(t.cell(0, 1).unwrap().strokes[3].weight, 2.0);
    assert_eq!(t.cell(0, 1).unwrap().strokes[1].weight, 0.0);
    s.execute("table.setCell", &json!({"cols": [0, 2], "stroke": {"weight": 5, "edges": "outer"}})).unwrap();
    let t = table_for(&s, sid, tid);
    assert_eq!(t.cell(0, 0).unwrap().strokes[3].weight, 5.0);
    assert_eq!(t.cell(0, 2).unwrap().strokes[1].weight, 5.0);
    assert_eq!(t.cell(0, 0).unwrap().strokes[1].weight, 2.0);
    assert_eq!(t.cell(0, 2).unwrap().strokes[3].weight, 0.0);
    s.execute("table.setCell", &json!({"cols": [0, 2], "stroke": {"weight": 0, "edges": "inner"}})).unwrap();
    let t = table_for(&s, sid, tid);
    assert_eq!(t.cell(0, 0).unwrap().strokes[3].weight, 5.0);
    assert_eq!(t.cell(0, 2).unwrap().strokes[1].weight, 5.0);
    assert_eq!(t.cell(0, 0).unwrap().strokes[1].weight, 0.0);
    assert_eq!(t.cell(0, 1).unwrap().strokes[3].weight, 0.0);
}

#[test]
fn editing_beside_a_merged_cell_only_changes_the_selected_edge_segment() {
    for direction in [doc::TextDirection::LeftToRight, doc::TextDirection::RightToLeft] {
        for imported_priority in [41, i32::MAX] {
            let (mut s, sid, _) = session_with_frame();
            let r = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap();
            let tid = r["table"].as_u64().unwrap();
            s.execute("table.merge", &json!({"row": 0, "cols": [0, 1]})).unwrap();
            s.edit(|d, _| {
                let t = d.story_mut(doc::StoryId(sid)).unwrap().table_mut(tid).unwrap();
                t.options.direction = direction;
                t.options.border = doc::CellStroke::none();
                for cell in &mut t.cells {
                    cell.strokes = std::array::from_fn(|_| doc::CellStroke::none());
                }
                let merged = t.cell_mut(0, 0).unwrap();
                merged.strokes[2] = doc::CellStroke { weight: 7.0, ..Default::default() };
                merged.stroke_defined[2] = true;
                merged.stroke_priorities[2] = imported_priority;
                t.cell_mut(1, 1).unwrap().stroke_priorities[2] = 7;
                Ok(())
            })
            .unwrap();
            let before = table_for(&s, sid, tid).clone();
            s.execute("table.setCell", &json!({"row": 1, "col": 0, "stroke": {"weight": 0, "edges": "top"}})).unwrap();
            let t = table_for(&s, sid, tid);
            assert_eq!(t.cell(0, 0).unwrap().strokes[2].weight, 7.0, "the merged edge itself must not be erased");
            let priorities =
                [t.cell(1, 1).unwrap().stroke_priorities[2], t.cell(0, 0).unwrap().stroke_priorities[2], t.cell(1, 0).unwrap().stroke_priorities[0]];
            assert_eq!(priorities, if imported_priority == i32::MAX { [1, 2, 3] } else { [7, 41, 42] });
            let composed = s.cache.get(&s.doc().unwrap().doc, doc::StoryId(sid), None);
            let frag = composed.frames.iter().flat_map(|f| &f.tables).next().unwrap();
            let unselected = frag.cell(1, 1).unwrap().rect;
            let visible: Vec<_> = frag.strokes.iter().filter(|s| s.stroke.weight == 7.0).collect();
            assert_eq!(visible.len(), 1);
            assert_eq!(visible[0].a, designcraft_geom::Point::new(unselected.x0, unselected.y0));
            assert_eq!(visible[0].b, designcraft_geom::Point::new(unselected.x1, unselected.y0));
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(table_for(&s, sid, tid), &before);
        }
    }
}

#[test]
fn split_selected_row_or_column_splits_both_cells() {
    for horizontal in [true, false] {
        let (mut s, sid, _) = session_with_frame();
        let tid = s.execute("table.insert", &json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        let params = if horizontal { json!({"rows": [0, 0], "cols": [0, 1]}) } else { json!({"rows": [0, 1], "cols": [0, 0]}) };
        s.execute("table.select", &params).unwrap();
        let command = if horizontal { "table.splitHorizontally" } else { "table.splitVertically" };
        s.execute(command, &json!({})).unwrap();
        let table = table_for(&s, sid, tid);
        assert_eq!((table.nrows(), table.ncols()), if horizontal { (3, 2) } else { (2, 3) });
        for r in 0..2 {
            for c in 0..2 {
                assert_eq!(table.owner(r, c), (r, c), "both selected cells have independent halves");
            }
        }
        check(&s);
    }
}

#[test]
fn split_selected_block_and_merged_cells_preserves_geometry_and_history() {
    for horizontal in [true, false] {
        for merged in [false, true] {
            let (mut s, sid, _) = session_with_frame();
            let tid = s.execute("table.insert", &json!({"rows": 4, "cols": 4, "width": 240})).unwrap()["table"].as_u64().unwrap();
            s.execute("table.setRowHeight", &json!({"table": tid, "height": 60, "mode": "exactly"})).unwrap();
            s.execute("style.cell.create", &json!({"name": "Neighbor", "fill": "[Paper]", "insets": 3})).unwrap();
            s.execute("style.cell.create", &json!({"name": "Chosen", "fill": "[Black]", "tint": 0.15, "insets": 2})).unwrap();
            s.execute("style.cell.apply", &json!({"table": tid, "name": "Neighbor"})).unwrap();
            for r in 0..4 {
                for c in 0..4 {
                    s.execute("table.setCell", &json!({"row": r, "col": c, "text": format!("{r},{c}")})).unwrap();
                }
            }
            let range = if !merged {
                doc::CellRange::new(1, 1, 2, 2)
            } else if horizontal {
                doc::CellRange::new(0, 0, 2, 1)
            } else {
                doc::CellRange::new(0, 0, 1, 2)
            };
            let selection = json!({"rows": [range.r0, range.r1], "cols": [range.c0, range.c1]});
            s.execute("table.select", &selection).unwrap();
            s.execute("style.cell.apply", &json!({"name": "Chosen"})).unwrap();
            if merged {
                let merge = if horizontal { json!({"rows": [0, 2], "cols": [1, 1]}) } else { json!({"rows": [1, 1], "cols": [0, 2]}) };
                s.execute("table.merge", &merge).unwrap();
                s.execute("table.select", &selection).unwrap();
            }
            let before_doc = s.doc().unwrap().doc.clone();
            let before_selection = s.doc().unwrap().selection.clone();
            let before = table_for(&s, sid, tid).clone();
            let before_layout = s.cache.get(&before_doc, doc::StoryId(sid), None);
            let before_fragment = before_layout.frames.iter().flat_map(|f| &f.tables).find(|t| t.table == tid).unwrap();
            let undo_len = s.doc().unwrap().history.undo.len();
            let command = if horizontal { "table.splitHorizontally" } else { "table.splitVertically" };
            s.execute(command, &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().history.undo.len(), undo_len + 1, "one split command creates one undo entry");
            let after_doc = s.doc().unwrap().doc.clone();
            let after_selection = s.doc().unwrap().selection.clone();
            let after = table_for(&s, sid, tid).clone();
            let after_layout = s.cache.get(&after_doc, doc::StoryId(sid), None);
            let after_fragment = after_layout.frames.iter().flat_map(|f| &f.tables).find(|t| t.table == tid).unwrap();
            assert_eq!(after_fragment.rect, before_fragment.rect, "splitting does not resize the table");
            assert_eq!(after_fragment.cells.len(), before_fragment.cells.len() + 4, "four original owners split once");
            for placed in &before_fragment.cells {
                let original = before.cell(placed.row, placed.col).unwrap();
                let matches: Vec<_> = after_fragment.cells.iter().filter(|c| c.source == placed.source).collect();
                assert_eq!(matches.len(), 1, "original cell text survives once: {}", placed.source);
                let surviving = matches[0];
                let cell = after.cell(surviving.row, surviving.col).unwrap();
                let mut normalized = cell.clone();
                normalized.row_span = original.row_span;
                normalized.col_span = original.col_span;
                assert_eq!(&normalized, original, "existing cell text and formatting are preserved");
                if !range.contains(placed.row, placed.col) {
                    assert_eq!(surviving.rect, placed.rect, "unselected neighbor geometry is unchanged");
                    continue;
                }
                let second_addr = if horizontal {
                    (surviving.row + cell.row_span as usize, surviving.col)
                } else {
                    (surviving.row, surviving.col + cell.col_span as usize)
                };
                let second = after.cell(second_addr.0, second_addr.1).unwrap();
                assert_eq!(after.owner(second_addr.0, second_addr.1), second_addr);
                assert!(second.text.is_empty());
                assert_eq!(second.style, original.style);
                assert_eq!(second.insets, original.insets);
                assert_eq!(second.fill, original.fill);
                assert_eq!(second.fill_tint, original.fill_tint);
                let second_rect = after_fragment.cell(second_addr.0, second_addr.1).unwrap().rect;
                assert_eq!(surviving.rect.union(second_rect), placed.rect, "the halves exactly cover the original cell");
                if horizontal {
                    assert_eq!(surviving.rect.y1, second_rect.y0);
                } else {
                    assert_eq!(surviving.rect.x1, second_rect.x0);
                }
            }
            let text = s.doc().unwrap().selection.text.unwrap();
            let address = text.cell.unwrap();
            assert_eq!(after.owner(address.row, address.col), (address.row, address.col));
            assert!(text.focus <= after.cell(address.row, address.col).unwrap().text.len());
            let cells = s.doc().unwrap().selection.cells.unwrap();
            assert!(cells.range.r1 < after.nrows() && cells.range.c1 < after.ncols());
            check(&s);
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().doc, &before_doc));
            assert_eq!(s.doc().unwrap().selection, before_selection);
            assert_eq!(s.doc().unwrap().history.undo.len(), undo_len);
            s.execute("edit.redo", &json!({})).unwrap();
            assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().doc, &after_doc));
            assert_eq!(s.doc().unwrap().selection, after_selection);
            let bytes = crate::cmd::file_bytes(&s.doc().unwrap().doc);
            let reopened = crate::cmd::file_from(&bytes).unwrap();
            reopened.check().unwrap();
            assert_eq!(reopened.story(doc::StoryId(sid)).unwrap().tables.get(&tid).unwrap().as_ref(), &after);
        }
    }
}
