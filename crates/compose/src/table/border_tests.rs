use super::*;
use designcraft_doc::{CellRange, ParaFormat, SpreadRef, build::NewDocument};

fn compose_table(table: Table) -> ComposedStory {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let (_, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 120.0, 200.0), layer, "", ParaFormat::default()).unwrap();
    doc.story_mut(sid).unwrap().insert_table(0, table);
    crate::compose_story(&doc, sid, &ComposeOptions::default())
}

#[test]
fn explicit_cell_edges_override_table_border_without_changing_native_defaults() {
    let mut table = Table::new(77, 1, 1, 0, 0, 100.0);
    table.options.border.weight = 2.0;
    table.cell_mut(0, 0).unwrap().strokes = std::array::from_fn(|_| CellStroke::none());
    let native = compose_table(table.clone());
    let strokes = &native.frames[0].tables[0].strokes;
    assert_eq!(strokes.len(), 4, "default cells still use all four table borders");
    assert!(strokes.iter().all(|s| s.stroke.weight == 2.0));

    let cell = table.cell_mut(0, 0).unwrap();
    cell.border_overrides = [true, true, true, false];
    cell.strokes[2] = CellStroke { weight: 3.0, ..Default::default() };
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2, "top and left None overrides suppress borders");
    let bottom = fragment.strokes.iter().find(|s| s.a.y == fragment.rect.y1 && s.b.y == fragment.rect.y1).unwrap();
    assert_eq!(bottom.stroke.weight, 3.0);
    let right = fragment.strokes.iter().find(|s| s.a.x == fragment.rect.x1 && s.b.x == fragment.rect.x1).unwrap();
    assert_eq!(right.stroke.weight, 2.0, "unoverridden right edge keeps the table border");
}

#[test]
fn merged_cell_keeps_explicit_border_suppression() {
    let mut table = Table::new(77, 2, 2, 0, 0, 100.0);
    table.merge(CellRange::new(0, 0, 1, 1)).unwrap();
    let cell = table.cell_mut(0, 0).unwrap();
    cell.border_overrides = [true; 4];
    cell.strokes = std::array::from_fn(|_| CellStroke::none());
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.cells.len(), 1);
    assert!(fragment.strokes.is_empty());
}

fn rule_table(body: usize, cols: usize, header: usize, footer: usize) -> Table {
    let mut table = Table::new(77, body, cols, header, footer, 100.0);
    table.options.border = CellStroke::none();
    for row in &mut table.rows {
        row.height = 20.0;
        row.mode = RowHeightMode::Exactly;
    }
    for cell in &mut table.cells {
        cell.insets = [0.0; 4];
        cell.strokes = std::array::from_fn(|_| CellStroke::none());
    }
    table
}

fn set_rule(table: &mut Table, row: usize, col: usize, side: usize, weight: f64, priority: i32) {
    let cell = table.cell_mut(row, col).unwrap();
    cell.strokes[side] = CellStroke { weight, ..Default::default() };
    cell.stroke_defined[side] = true;
    cell.stroke_priorities[side] = priority;
}

fn assert_no_overlapping_rules(fragment: &TableFrag) {
    for (i, a) in fragment.strokes.iter().enumerate() {
        for b in &fragment.strokes[i + 1..] {
            let horizontal_overlap = a.a.y == a.b.y && a.a.y == b.a.y && b.a.y == b.b.y && a.a.x.max(b.a.x) < a.b.x.min(b.b.x);
            let vertical_overlap = a.a.x == a.b.x && a.a.x == b.a.x && b.a.x == b.b.x && a.a.y.max(b.a.y) < a.b.y.min(b.b.y);
            assert!(!horizontal_overlap && !vertical_overlap, "duplicate shared edge: {a:?}, {b:?}");
        }
    }
}

#[test]
fn header_bottom_only_style_survives_zero_body_edges() {
    // Synthetic equivalent of a header with a one-point bottom rule and zero on all
    // remaining header/body edges. Both sides supply style formatting at priority zero.
    let mut table = rule_table(2, 3, 1, 0);
    for cell in &mut table.cells {
        cell.stroke_defined = [true; 4];
        cell.border_overrides = [true; 4];
    }
    for col in 0..3 {
        set_rule(&mut table, 0, col, 2, 1.0, 0);
    }
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    let y = fragment.cell(0, 0).unwrap().rect.y1;
    assert_eq!(fragment.strokes.len(), 3);
    assert!(fragment.strokes.iter().all(|s| s.a.y == y && s.b.y == y && s.stroke.weight == 1.0));
    assert_eq!(fragment.strokes.iter().map(|s| s.b.x - s.a.x).sum::<f64>(), 100.0);
    assert_no_overlapping_rules(fragment);
}

#[test]
fn shared_rules_choose_numeric_priority_once_and_preserve_drawing_order() {
    let mut table = rule_table(2, 2, 0, 0);
    set_rule(&mut table, 0, 0, 2, 3.0, 7);
    set_rule(&mut table, 1, 0, 0, 1.0, 4);
    set_rule(&mut table, 0, 0, 3, 4.0, 3);
    set_rule(&mut table, 0, 1, 1, 2.0, 9);
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2);
    let horizontal = &fragment.strokes[0];
    assert_eq!(horizontal.a.y, horizontal.b.y);
    assert_eq!(horizontal.stroke.weight, 3.0);
    let vertical = &fragment.strokes[1];
    assert_eq!(vertical.a.x, vertical.b.x);
    assert_eq!(vertical.stroke.weight, 2.0, "higher priority draws last at a crossing");
    assert_no_overlapping_rules(fragment);
}

#[test]
fn signed_negative_priorities_resolve_and_draw_in_numeric_order() {
    let mut table = rule_table(2, 2, 0, 0);
    set_rule(&mut table, 0, 0, 2, 1.0, -2);
    set_rule(&mut table, 1, 0, 0, 3.0, -1);
    set_rule(&mut table, 0, 0, 3, 2.0, -3);
    set_rule(&mut table, 0, 1, 1, 4.0, -2);
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2);
    assert_eq!(fragment.strokes[0].stroke.weight, 4.0, "priority -2 draws before -1");
    assert_eq!(fragment.strokes[0].a.x, fragment.strokes[0].b.x);
    assert_eq!(fragment.strokes[1].stroke.weight, 3.0, "priority -1 wins over -2");
    assert_eq!(fragment.strokes[1].a.y, fragment.strokes[1].b.y);
    assert_no_overlapping_rules(fragment);
}

#[test]
fn default_zero_priority_beats_a_negative_priority_rule() {
    let mut table = rule_table(2, 1, 0, 0);
    set_rule(&mut table, 0, 0, 2, 3.0, -1);
    assert_eq!(table.cell(1, 0).unwrap().stroke_priorities, [0; 4]);
    let composed = compose_table(table.clone());
    assert!(composed.frames[0].tables[0].strokes.is_empty(), "priority zero None suppresses priority -1");
    table.cell_mut(1, 0).unwrap().strokes[0] = CellStroke::default();
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 1);
    assert_eq!(fragment.strokes[0].stroke.weight, 1.0, "the priority-zero native rule wins over priority -1");
}

#[test]
fn higher_priority_zero_or_none_suppresses_a_visible_shared_rule() {
    for invisible in [CellStroke::none(), CellStroke { color: designcraft_color::swatch::NONE.into(), ..Default::default() }] {
        let mut table = rule_table(2, 2, 0, 0);
        set_rule(&mut table, 0, 0, 2, 3.0, 7);
        set_rule(&mut table, 1, 0, 0, 0.0, 8);
        table.cell_mut(1, 0).unwrap().strokes[0] = invisible.clone();
        set_rule(&mut table, 0, 0, 3, 3.0, 7);
        set_rule(&mut table, 0, 1, 1, 0.0, 8);
        table.cell_mut(0, 1).unwrap().strokes[1] = invisible;
        let composed = compose_table(table);
        assert!(composed.frames[0].tables[0].strokes.is_empty());
    }
}

#[test]
fn supplied_zero_shared_edge_beats_missing_native_default() {
    let mut table = Table::new(77, 2, 1, 0, 0, 100.0);
    table.options.border = CellStroke::none();
    let upper = table.cell_mut(0, 0).unwrap();
    upper.strokes[2] = CellStroke::none();
    upper.stroke_defined[2] = true;
    let composed = compose_table(table.clone());
    assert!(composed.frames[0].tables[0].strokes.is_empty(), "explicit zero beats a missing edge's one-point default");
    table.cell_mut(0, 0).unwrap().stroke_defined[2] = false;
    let composed = compose_table(table);
    assert_eq!(composed.frames[0].tables[0].strokes.len(), 1, "a missing zero cannot suppress the opposite visible default");
}

#[test]
fn specified_zero_priority_outer_edges_keep_per_side_table_borders() {
    for direction in [designcraft_doc::TextDirection::LeftToRight, designcraft_doc::TextDirection::RightToLeft] {
        let mut table = rule_table(1, 1, 0, 0);
        table.options.direction = direction;
        table.options.borders = std::array::from_fn(|i| Some(CellStroke { weight: (i + 1) as f64, ..Default::default() }));
        table.cell_mut(0, 0).unwrap().stroke_defined = [true; 4];
        let composed = compose_table(table);
        let fragment = &composed.frames[0].tables[0];
        assert_eq!(fragment.strokes.len(), 4);
        for rule in &fragment.strokes {
            let expected = if rule.a.y == rule.b.y {
                if rule.a.y == fragment.rect.y0 { 1.0 } else { 3.0 }
            } else if rule.a.x == fragment.rect.x0 {
                2.0
            } else {
                4.0
            };
            assert_eq!(rule.stroke.weight, expected, "priority-zero cell values do not override perimeter formatting");
        }
        assert_no_overlapping_rules(fragment);
    }
}

fn uniform_pattern(weight: f64) -> designcraft_doc::AltStrokes {
    let stroke = CellStroke { weight, ..Default::default() };
    designcraft_doc::AltStrokes { first: 1, next: 1, first_stroke: stroke.clone(), next_stroke: stroke, skip_first: 0, skip_last: 0 }
}

#[test]
fn uniform_zero_table_patterns_suppress_missing_grid_edges_but_not_perimeter() {
    let mut table = Table::new(77, 2, 2, 0, 0, 100.0);
    table.options.row_strokes = Some(uniform_pattern(0.0));
    table.options.column_strokes = Some(uniform_pattern(0.0));
    table.options.border.weight = 2.0;
    let composed = compose_table(table.clone());
    let fragment = &composed.frames[0].tables[0];
    assert!(!fragment.strokes.is_empty(), "table perimeter remains visible");
    assert!(fragment.strokes.iter().all(|s| {
        s.stroke.weight == 2.0
            && ((s.a.x == s.b.x && (s.a.x == fragment.rect.x0 || s.a.x == fragment.rect.x1))
                || (s.a.y == s.b.y && (s.a.y == fragment.rect.y0 || s.a.y == fragment.rect.y1)))
    }));
    assert_no_overlapping_rules(fragment);
    table.options.border = CellStroke::none();
    set_rule(&mut table, 0, 0, 2, 3.0, 0);
    set_rule(&mut table, 0, 1, 1, 4.0, 0);
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2, "supplied cell strokes override the table patterns");
    assert!(fragment.strokes.iter().any(|s| s.stroke.weight == 3.0));
    assert!(fragment.strokes.iter().any(|s| s.stroke.weight == 4.0));
    assert_no_overlapping_rules(fragment);
}

#[test]
fn uniform_visible_table_patterns_supply_continuous_rules_below_explicit_none() {
    let mut table = Table::new(77, 2, 2, 0, 0, 100.0);
    table.options.border = CellStroke::none();
    table.options.row_strokes = Some(uniform_pattern(2.0));
    table.options.column_strokes = Some(uniform_pattern(3.0));
    let composed = compose_table(table.clone());
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2, "a table pattern stays continuous across cell boundaries");
    assert!(fragment.strokes.iter().any(|s| s.stroke.weight == 2.0 && s.b.x - s.a.x == 100.0));
    assert!(fragment.strokes.iter().any(|s| s.stroke.weight == 3.0 && s.b.y - s.a.y == fragment.rect.height()));
    for cell in &mut table.cells {
        cell.strokes = std::array::from_fn(|_| CellStroke::none());
        cell.stroke_defined = [true; 4];
    }
    let composed = compose_table(table);
    assert!(composed.frames[0].tables[0].strokes.is_empty(), "explicit cell None is stronger than table patterns");
}

#[test]
fn uniform_table_patterns_leave_header_and_footer_edges_unchanged() {
    let mut table = Table::new(77, 2, 2, 1, 1, 100.0);
    table.options.border = CellStroke::none();
    table.options.row_strokes = Some(uniform_pattern(0.0));
    table.options.column_strokes = Some(uniform_pattern(0.0));
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    let header = fragment.cell(0, 0).unwrap().rect;
    let body = fragment.cell(1, 0).unwrap().rect;
    let footer = fragment.cell(3, 0).unwrap().rect;
    assert!(fragment.strokes.iter().any(|s| s.a == Point::new(header.x1, header.y0) && s.b == Point::new(header.x1, header.y1)));
    assert!(fragment.strokes.iter().any(|s| s.a == Point::new(footer.x1, footer.y0) && s.b == Point::new(footer.x1, footer.y1)));
    assert!(!fragment.strokes.iter().any(|s| s.a.y == body.y1 && s.b.y == body.y1));
    assert!(!fragment.strokes.iter().any(|s| s.a == Point::new(body.x1, body.y0) && s.b == Point::new(body.x1, body.y1)));
    assert_no_overlapping_rules(fragment);
}

#[test]
fn unverified_nonuniform_or_skipped_pattern_phase_keeps_native_edges() {
    for skipped in [false, true] {
        let mut table = Table::new(77, 2, 2, 0, 0, 100.0);
        table.options.border = CellStroke::none();
        let mut pattern = uniform_pattern(2.0);
        if skipped {
            pattern.skip_first = 1;
        } else {
            pattern.next_stroke.weight = 3.0;
        }
        table.options.row_strokes = Some(pattern.clone());
        table.options.column_strokes = Some(pattern);
        let composed = compose_table(table);
        let fragment = &composed.frames[0].tables[0];
        assert_eq!(fragment.strokes.len(), 4);
        assert!(fragment.strokes.iter().all(|s| s.stroke.weight == 1.0));
    }
}

#[test]
fn metadata_free_native_and_legacy_edges_keep_top_and_left_suppression() {
    let mut table = Table::new(77, 2, 2, 0, 0, 100.0);
    table.options.border = CellStroke::none();
    let composed = compose_table(table.clone());
    assert_eq!(composed.frames[0].tables[0].strokes.len(), 4, "unmodified native grid control");
    // The metadata arrays also default to zero/false when reading legacy JSON. Older
    // native table.setCell code changed these strokes without setting any metadata.
    table.cell_mut(1, 0).unwrap().strokes[0] = CellStroke::none();
    table.cell_mut(0, 1).unwrap().strokes[1].color = designcraft_color::swatch::NONE.into();
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2);
    let hidden_top = fragment.cell(1, 0).unwrap().rect;
    let hidden_left = fragment.cell(0, 1).unwrap().rect;
    assert!(!fragment.strokes.iter().any(|s| s.a == Point::new(hidden_top.x0, hidden_top.y0) && s.b == Point::new(hidden_top.x1, hidden_top.y0)));
    assert!(!fragment.strokes.iter().any(|s| s.a == Point::new(hidden_left.x0, hidden_left.y0) && s.b == Point::new(hidden_left.x0, hidden_left.y1)));
    assert_no_overlapping_rules(fragment);
}

#[test]
fn equal_priority_visible_rules_keep_top_and_left_ownership() {
    let mut table = rule_table(2, 2, 0, 0);
    set_rule(&mut table, 0, 0, 2, 5.0, 0);
    set_rule(&mut table, 1, 0, 0, 2.0, 0);
    set_rule(&mut table, 0, 0, 3, 6.0, 0);
    set_rule(&mut table, 0, 1, 1, 3.0, 0);
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2);
    assert_eq!(fragment.strokes[0].stroke.weight, 2.0);
    assert_eq!(fragment.strokes[1].stroke.weight, 3.0);
    assert_no_overlapping_rules(fragment);
}

#[test]
fn row_spanning_edge_resolves_each_neighbor_without_internal_rules() {
    let mut table = rule_table(2, 2, 0, 0);
    table.merge(CellRange::new(0, 0, 1, 0)).unwrap();
    set_rule(&mut table, 0, 0, 3, 1.0, 5);
    set_rule(&mut table, 0, 1, 1, 2.0, 6);
    set_rule(&mut table, 1, 1, 1, 3.0, 4);
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2);
    let upper = fragment.strokes.iter().find(|s| s.stroke.weight == 2.0).unwrap();
    let lower = fragment.strokes.iter().find(|s| s.stroke.weight == 1.0).unwrap();
    assert_eq!(upper.a, Point::new(50.0, fragment.rect.y0));
    assert_eq!(upper.b, lower.a);
    assert_eq!(lower.b, Point::new(50.0, fragment.rect.y1));
    assert_no_overlapping_rules(fragment);
}

#[test]
fn column_spanning_edge_resolves_each_neighbor_without_internal_rules() {
    let mut table = rule_table(2, 2, 0, 0);
    table.merge(CellRange::new(0, 0, 0, 1)).unwrap();
    set_rule(&mut table, 0, 0, 2, 1.0, 5);
    set_rule(&mut table, 1, 0, 0, 2.0, 6);
    set_rule(&mut table, 1, 1, 0, 3.0, 4);
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 2);
    let left = fragment.strokes.iter().find(|s| s.stroke.weight == 2.0).unwrap();
    let right = fragment.strokes.iter().find(|s| s.stroke.weight == 1.0).unwrap();
    let y = fragment.cell(0, 0).unwrap().rect.y1;
    assert_eq!(left.a, Point::new(fragment.rect.x0, y));
    assert_eq!(left.b, right.a);
    assert_eq!(right.b, Point::new(fragment.rect.x1, y));
    assert_no_overlapping_rules(fragment);
}

#[test]
fn merged_winning_dashed_edge_stays_continuous_across_losing_neighbors() {
    let mut table = rule_table(2, 2, 0, 0);
    table.columns.iter_mut().for_each(|column| column.width = 45.0);
    table.merge(CellRange::new(1, 0, 1, 1)).unwrap();
    set_rule(&mut table, 1, 0, 0, 1.0, 1);
    table.cell_mut(1, 0).unwrap().strokes[0].kind = designcraft_doc::StrokeType::Dashed { pattern: vec![6.0, 4.0] };
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 1, "one winning source edge must retain one dash phase");
    let rule = &fragment.strokes[0];
    let merged = fragment.cell(1, 0).unwrap().rect;
    assert_eq!(rule.a, Point::new(merged.x0, merged.y0));
    assert_eq!(rule.b, Point::new(merged.x1, merged.y0));
    assert_eq!(rule.b.x - rule.a.x, 90.0);
    assert_no_overlapping_rules(fragment);
}

#[test]
fn merged_perimeter_keeps_four_continuous_strokes() {
    let mut table = rule_table(2, 2, 0, 0);
    table.merge(CellRange::new(0, 0, 1, 1)).unwrap();
    table.options.border.weight = 2.0;
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 4);
    assert_eq!(fragment.strokes.iter().filter(|s| s.a.y == s.b.y).count(), 2);
    assert_eq!(fragment.strokes.iter().filter(|s| s.a.x == s.b.x).count(), 2);
    assert_no_overlapping_rules(fragment);
}

#[test]
fn rtl_shared_rules_use_physical_edges_and_outer_borders() {
    let mut table = rule_table(1, 2, 0, 0);
    table.options.direction = designcraft_doc::TextDirection::RightToLeft;
    table.columns[0].width = 30.0;
    table.columns[1].width = 70.0;
    set_rule(&mut table, 0, 0, 1, 2.0, 9);
    set_rule(&mut table, 0, 1, 3, 4.0, 2);
    set_rule(&mut table, 0, 0, 3, 5.0, 3);
    table.cell_mut(0, 0).unwrap().border_overrides[3] = true;
    set_rule(&mut table, 0, 1, 1, 6.0, 4);
    table.cell_mut(0, 1).unwrap().border_overrides[1] = true;
    let composed = compose_table(table);
    let fragment = &composed.frames[0].tables[0];
    assert_eq!(fragment.strokes.len(), 3);
    for (x, weight) in [(0.0, 6.0), (70.0, 2.0), (100.0, 5.0)] {
        let rule = fragment.strokes.iter().find(|s| s.a.x == x && s.b.x == x).unwrap();
        assert_eq!(rule.stroke.weight, weight);
    }
    assert_no_overlapping_rules(fragment);
}

fn compose_two_fragments(table: Table, height: f64) -> ComposedStory {
    let mut doc = Document::new(&NewDocument::default());
    let layer = doc.default_layer();
    let (first, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 120.0, height), layer, "", ParaFormat::default()).unwrap();
    let (second, _) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 200.0, 120.0, 200.0 + height), layer, "", ParaFormat::default()).unwrap();
    doc.thread(first, second).unwrap();
    doc.story_mut(sid).unwrap().insert_table(0, table);
    let composed = crate::compose_story(&doc, sid, &ComposeOptions::default());
    assert!(!composed.is_overset(), "overset at {:?}", composed.overset_at);
    assert_eq!(composed.frames[0].tables.len(), 1);
    assert_eq!(composed.frames[1].tables.len(), 1);
    composed
}

#[test]
fn repeated_header_and_footer_rules_resolve_displayed_fragment_neighbors() {
    let mut table = rule_table(4, 2, 1, 1);
    table.merge(CellRange::new(0, 0, 0, 1)).unwrap();
    table.merge(CellRange::new(5, 0, 5, 1)).unwrap();
    set_rule(&mut table, 0, 0, 2, 1.0, 2);
    set_rule(&mut table, 5, 0, 0, 2.0, 3);
    // Keep 19 points for the anchor's trailing empty paragraph, still below the
    // 100 points needed to fit a third body row plus the header and footer.
    let composed = compose_two_fragments(table, 99.0);
    for (i, frame) in composed.frames.iter().enumerate() {
        let fragment = &frame.tables[0];
        let header = fragment.cell(0, 0).unwrap();
        let footer = fragment.cell(5, 0).unwrap();
        assert_eq!(header.repeated, i == 1);
        assert_eq!(footer.repeated, i == 0);
        assert_eq!(fragment.strokes.len(), 2);
        let header_rules: Vec<_> = fragment.strokes.iter().filter(|s| s.a.y == header.rect.y1 && s.b.y == header.rect.y1).collect();
        let footer_rules: Vec<_> = fragment.strokes.iter().filter(|s| s.a.y == footer.rect.y0 && s.b.y == footer.rect.y0).collect();
        assert_eq!(header_rules.len(), 1);
        assert!(header_rules.iter().all(|s| s.stroke.weight == 1.0));
        assert_eq!(footer_rules.len(), 1);
        assert!(footer_rules.iter().all(|s| s.stroke.weight == 2.0));
        assert_no_overlapping_rules(fragment);
    }
    assert!(composed.frames[0].tables[0].cell(1, 0).is_some());
    assert!(composed.frames[1].tables[0].cell(3, 0).is_some());
}

#[test]
fn fragment_boundary_does_not_resolve_against_an_undisplayed_neighbor() {
    let mut table = rule_table(4, 1, 0, 0);
    set_rule(&mut table, 1, 0, 2, 3.0, 5);
    table.cell_mut(1, 0).unwrap().border_overrides[2] = true;
    set_rule(&mut table, 2, 0, 0, 4.0, 6);
    table.cell_mut(2, 0).unwrap().border_overrides[0] = true;
    // Two rows fit in each frame; leave room for the trailing empty paragraph.
    let composed = compose_two_fragments(table, 59.0);
    let first = &composed.frames[0].tables[0];
    let second = &composed.frames[1].tables[0];
    assert_eq!(first.strokes.len(), 1);
    assert_eq!(first.strokes[0].stroke.weight, 3.0);
    assert_eq!(first.strokes[0].a.y, first.rect.y1);
    assert_eq!(second.strokes.len(), 1);
    assert_eq!(second.strokes[0].stroke.weight, 4.0);
    assert_eq!(second.strokes[0].a.y, second.rect.y0);
}
