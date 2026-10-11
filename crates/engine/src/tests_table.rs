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
