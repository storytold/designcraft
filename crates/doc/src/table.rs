//! Tables.
//!
//! A table lives in a story: a [`TABLE_ANCHOR`](crate::story::TABLE_ANCHOR) character occupies its
//! own paragraph and that paragraph's [`ParaFormat::table`](crate::story::ParaFormat) names the
//! table in [`Story::tables`]. Deleting the anchor character deletes the table.
//!
//! The grid is `rows × columns` cells stored row-major. A merged cell is the top-left cell of its
//! region with `row_span`/`col_span` > 1; the cells it covers stay in the grid (empty, ignored).
//! Each cell holds a [`Story`] (no frames) composed into the cell rectangle.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ids::StoryId;
use crate::item::{FirstBaseline, StrokeType, VerticalJustification};
use crate::story::{CharFormat, ParaFormat, Story, TABLE_ANCHOR};

mod cell;
mod edit;
mod grid;
mod merge;
mod story;

pub use cell::{AltFills, AltStrokes, Cell, CellStroke, Column, Row, RowHeightMode, RowKind, TableOptions};
pub use grid::CellRange;

/// Most rows a table holds, as InDesign allows.
pub const MAX_TABLE_ROWS: usize = 10_000;
/// Most columns a table holds, as InDesign allows.
pub const MAX_TABLE_COLS: usize = 1_000;
/// Most cells a table holds, whatever its shape.
///
/// A [`Cell`] is about 1.4 KB (it carries a [`Story`]), so this is roughly 340 MB of cells at the
/// limit. The row and column limits alone are not enough: 10,000 × 1,000 would be 13.6 GB, and an
/// IDML story holding one `<Table>` with 5,000 `<Row/>` and 5,000 `<Column/>` -- about 1 KB
/// zipped -- asked for 25M cells, some 34 GB. That allocation does not fail politely: it aborts
/// the process through `handle_alloc_error`, which no `catch_unwind` can save, so every open
/// document's unsaved work goes with it (#1583).
pub const MAX_TABLE_CELLS: usize = 250_000;

/// The rows and columns a table of `rows × cols` is actually given, inside the limits above.
///
/// Columns are clamped first and rows take what is left of the cell budget, because a table is
/// far more often long than wide: truncating a 5,000-column table to 1,000 columns and then its
/// rows loses less of it than the other way round. Callers that read a file report the
/// difference; the counts are a document's claim, not something to trust.
pub fn clamped_size(rows: usize, cols: usize) -> (usize, usize) {
    let cols = cols.clamp(1, MAX_TABLE_COLS);
    let rows = rows.clamp(1, MAX_TABLE_ROWS.min(MAX_TABLE_CELLS / cols).max(1));
    (rows, cols)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub id: u64,
    pub rows: Vec<Row>,
    pub columns: Vec<Column>,
    /// Row-major, `rows.len() * columns.len()`.
    pub cells: Vec<Cell>,
    #[serde(default)]
    pub options: TableOptions,
    /// Applied table style ("" = [Basic Table]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story(t: &str) -> Story {
        Story::with_text(StoryId(1), t, ParaFormat::default())
    }

    /// A table's shape comes out of IDML and DOCX files unchecked, and `Cell` is about 1.4 KB, so
    /// a file claiming 5,000 x 5,000 asked for 25M cells -- some 34 GB. That allocation aborts the
    /// process rather than failing, taking every open document's unsaved work with it (#1583).
    #[test]
    fn a_table_larger_than_the_limits_is_clamped_not_allocated() {
        // The reported shape: 5,000 rows and 5,000 columns.
        let t = Table::new(1, 5_000, 5_000, 0, 0, 500.0);
        assert_eq!(t.columns.len(), MAX_TABLE_COLS, "columns are clamped to InDesign's limit");
        assert!(t.cells.len() <= MAX_TABLE_CELLS, "{} cells is within the budget", t.cells.len());
        assert_eq!(t.rows.len() * t.columns.len(), t.cells.len(), "the grid stays rectangular");
        // What it would have been: 25,000,000 cells at 1,360 bytes each.
        assert!(25_000_000 * std::mem::size_of::<Cell>() > 30_000_000_000, "the size this test is about");
        // Freed before the next large table, so the peak stays at one of them.
        drop(t);

        // A long table keeps its rows while it fits the budget (the budget itself is checked by
        // `clamped_size`, which allocates nothing).
        let long = Table::new(2, 10_000, 5, 0, 0, 500.0);
        assert_eq!((long.rows.len(), long.columns.len()), (10_000, 5), "a long thin table is untouched");
        assert_eq!(clamped_size(10_000, 25), (10_000, 25), "10,000 x 25 is inside the budget");
        drop(long);

        // Past the row limit the rows are clamped too.
        let longer = Table::new(3, 50_000, 2, 0, 0, 100.0);
        assert_eq!(longer.rows.len(), MAX_TABLE_ROWS, "rows are clamped to InDesign's limit");
        drop(longer);

        // Header, body and footer share the budget, and the table still has a body.
        let with_parts = Table::new(4, 5_000, 5_000, 3, 2, 500.0);
        assert!(with_parts.rows.len() <= MAX_TABLE_ROWS, "{} rows", with_parts.rows.len());
        assert!(with_parts.cells.len() <= MAX_TABLE_CELLS, "{} cells", with_parts.cells.len());
        assert!(with_parts.rows.iter().any(|r| r.kind == RowKind::Header), "the header survives");
        assert!(with_parts.rows.iter().any(|r| r.kind == RowKind::Footer), "the footer survives");
        assert!(with_parts.rows.iter().any(|r| r.kind == RowKind::Body), "and so does a body row");
        drop(with_parts);

        // A header and footer bigger than the whole budget still leave a body row.
        let all_header = Table::new(6, 1, 1, MAX_TABLE_ROWS * 2, MAX_TABLE_ROWS * 2, 100.0);
        assert!(all_header.rows.len() <= MAX_TABLE_ROWS, "{} rows", all_header.rows.len());
        assert!(all_header.rows.iter().any(|r| r.kind == RowKind::Body), "a body row survives");
    }

    #[test]
    fn move_row_keeps_header_and_footer_rows_in_place() {
        let mut t = Table::new(1, 3, 2, 1, 1, 120.0);
        for (from, to) in [(0, 2), (0, 4), (2, 0), (2, 4), (4, 2), (4, 0)] {
            assert!(t.move_row(from, to).is_err(), "move_row({from}, {to}) should be refused");
            t.check().unwrap();
        }
        t.move_row(1, 3).unwrap();
        t.check().unwrap();
    }

    #[test]
    fn selected_owners_include_covered_cells_without_expanding_range() {
        for column in [true, false] {
            let mut t = Table::new(1, 2, 2, 0, 0, 120.0);
            let range = if column { CellRange::new(0, 1, 1, 1) } else { CellRange::new(1, 0, 1, 1) };
            assert_eq!(t.selected_owners(range), if column { vec![(0, 1), (1, 1)] } else { vec![(1, 0), (1, 1)] });
            t.merge(if column { CellRange::new(0, 0, 0, 1) } else { CellRange::new(0, 0, 1, 0) }).unwrap();
            assert_eq!(t.selected_owners(range), vec![(0, 0), (1, 1)]);
        }
    }

    #[test]
    fn selected_owners_are_bounded_sorted_and_unique() {
        let mut t = Table::new(1, 3, 3, 0, 0, 180.0);
        t.merge(CellRange::new(0, 0, 1, 1)).unwrap();
        assert_eq!(t.selected_owners(CellRange::new(1, 1, usize::MAX, usize::MAX)), vec![(0, 0), (1, 2), (2, 1), (2, 2)]);
        assert_eq!(t.selected_owners(CellRange::new(0, 0, 1, 1)), vec![(0, 0)], "covered positions do not duplicate an owner");
        assert!(t.selected_owners(CellRange::new(3, 0, usize::MAX, usize::MAX)).is_empty());
        assert!(t.selected_owners(CellRange::new(0, 3, usize::MAX, usize::MAX)).is_empty());
        assert!(t.selected_owners(CellRange { r0: 1, c0: 0, r1: 0, c1: 1 }).is_empty());
        t.rows.clear();
        t.cells.clear();
        assert!(t.selected_owners(CellRange::cell(0, 0)).is_empty());

        let mut right = Table::new(2, 2, 2, 0, 0, 120.0);
        right.merge(CellRange::new(0, 1, 1, 1)).unwrap();
        assert_eq!(right.selected_owners(CellRange::new(1, 0, 1, 1)), vec![(0, 1), (1, 0)], "owner order, not encounter order");
    }

    #[test]
    fn insert_table_gets_own_paragraph() {
        let mut s = story("before after");
        let a = s.insert_table(6, Table::new(7, 3, 2, 1, 0, 200.0));
        s.check().unwrap();
        assert_eq!(s.text, format!("before\n{TABLE_ANCHOR}\n after"));
        assert_eq!(s.table_anchor(7), Some(a));
        assert_eq!(s.para_table(1).map(|t| t.id), Some(7));
        assert_eq!(s.tables[&7].nrows(), 4);
        // At the start of a paragraph / end of the story.
        let mut s = story("x");
        s.insert_table(1, Table::new(1, 1, 1, 0, 0, 10.0));
        s.check().unwrap();
        assert_eq!(s.text, format!("x\n{TABLE_ANCHOR}\n"));
        let mut s = story("");
        s.insert_table(0, Table::new(1, 1, 1, 0, 0, 10.0));
        s.check().unwrap();
        assert_eq!(s.text, format!("{TABLE_ANCHOR}\n"));
    }

    #[test]
    fn deleting_the_anchor_removes_the_table() {
        let mut s = story("ab");
        let a = s.insert_table(2, Table::new(9, 2, 2, 0, 0, 100.0));
        s.delete(a..a + TABLE_ANCHOR.len_utf8());
        s.check().unwrap();
        assert!(s.tables.is_empty());
        // Deleting across the paragraph keeps the story consistent too.
        let mut s = story("ab\ncd");
        s.insert_table(2, Table::new(9, 2, 2, 0, 0, 100.0));
        s.delete(1..s.len() - 1);
        s.check().unwrap();
        assert!(s.tables.is_empty());
    }

    #[test]
    fn deleting_the_return_between_two_tables_keeps_both() {
        // Backspace at the start of the second table's paragraph.
        let mut s = story("x");
        let a1 = s.insert_table(1, Table::new(1, 1, 1, 0, 0, 10.0));
        let a2 = s.insert_table(s.len(), Table::new(2, 1, 1, 0, 0, 10.0));
        let before = s.text.clone();
        s.delete(a2 - 1..a2);
        s.check().unwrap();
        assert_eq!(s.text, before);
        assert_eq!((s.table_anchor(1), s.table_anchor(2)), (Some(a1), Some(a2)));
        // Deleting the table between two others keeps the outer ones on their own anchors.
        let mut s = story("x");
        for id in 1..=3 {
            s.insert_table(s.len(), Table::new(id, 1, 1, 0, 0, 10.0));
        }
        let (a1, a3) = (s.table_anchor(1).unwrap(), s.table_anchor(3).unwrap());
        s.delete(a1 + TABLE_ANCHOR.len_utf8()..a3);
        s.check().unwrap();
        assert_eq!(s.text, format!("x\n{TABLE_ANCHOR}\n{TABLE_ANCHOR}\n"));
        assert_eq!(s.tables.keys().copied().collect::<Vec<_>>(), [1, 3]);
        assert_eq!(s.table_anchor(3), Some(a1 + TABLE_ANCHOR.len_utf8() + 1));
    }

    #[test]
    fn edits_around_the_anchor_keep_the_table() {
        let mut s = story("ab\ncd");
        let a = s.insert_table(2, Table::new(3, 1, 1, 0, 0, 10.0));
        // New paragraphs before and after the anchor.
        s.insert(a, "\n");
        s.check().unwrap();
        let a = s.table_anchor(3).unwrap();
        s.insert(a + 3, "\nzz");
        s.check().unwrap();
        assert!(s.tables.contains_key(&3));
        // Merging the anchor paragraph with the previous one keeps the table.
        let a = s.table_anchor(3).unwrap();
        s.delete(a - 1..a);
        s.check().unwrap();
        assert!(s.tables.contains_key(&3), "{:?}", s.text);
        // Restyling the paragraph keeps the table.
        let a = s.table_anchor(3).unwrap();
        s.format_paras(a..a, |p| *p = ParaFormat { style: "X".into(), ..Default::default() });
        s.check().unwrap();
    }

    #[test]
    fn rows_columns_and_merges() {
        let mut t = Table::new(1, 3, 3, 1, 0, 300.0);
        t.cell_mut(1, 1).unwrap().text.insert(0, "a");
        t.cell_mut(1, 2).unwrap().text.insert(0, "b");
        t.merge(CellRange::new(1, 1, 2, 2)).unwrap();
        t.check().unwrap();
        assert_eq!(t.cell(1, 1).unwrap().text.text, "a\nb");
        assert_eq!(t.owner(2, 2), (1, 1));
        // Inserting a row inside the merged region extends it.
        t.insert_rows(2, 1);
        t.check().unwrap();
        assert_eq!(t.cell(1, 1).unwrap().row_span, 3);
        assert_eq!(t.nrows(), 5);
        // Header stays first.
        t.insert_rows(0, 1);
        t.check().unwrap();
        assert_eq!(t.rows[0].kind, RowKind::Header);
        assert_eq!(t.header_rows(), 2);
        // Deleting the owner row moves the content down.
        let (r, c) = (2, 1);
        assert_eq!(t.owner(r, c), (2, 1));
        t.delete_rows(2, 1);
        t.check().unwrap();
        assert_eq!(t.cell(2, 1).unwrap().text.text, "a\nb");
        assert_eq!(t.cell(2, 1).unwrap().row_span, 2);
        // Columns.
        t.insert_cols(1, 2, 50.0);
        t.check().unwrap();
        assert_eq!(t.ncols(), 5);
        assert_eq!(t.cell(2, 3).unwrap().col_span, 2);
        t.delete_cols(3, 1);
        t.check().unwrap();
        assert_eq!(t.cell(2, 3).unwrap().text.text, "a\nb");
        assert_eq!(t.cell(2, 3).unwrap().col_span, 1);
        t.unmerge(3, 3);
        t.check().unwrap();
        assert!(t.regions().is_empty());
        // Never below one row / column.
        t.delete_rows(0, 99);
        t.delete_cols(0, 99);
        t.check().unwrap();
        assert_eq!((t.nrows(), t.ncols()), (1, 1));
    }

    #[test]
    fn alternating_fills() {
        let a = AltFills { first: 1, next: 1, ..Default::default() };
        assert!(a.fill_for(0, 4).is_some());
        assert!(a.fill_for(1, 4).is_none());
        assert!(a.fill_for(2, 4).is_some());
        let b = AltFills { first: 2, next: 1, skip_first: 1, ..Default::default() };
        assert!(b.fill_for(0, 9).is_none());
        assert!(b.fill_for(1, 9).is_some());
        assert!(b.fill_for(2, 9).is_some());
        assert!(b.fill_for(3, 9).is_none());
    }

    #[test]
    fn alternating_fill_counts_cannot_overflow() {
        let fills = AltFills { first: u32::MAX, next: 1, ..Default::default() };
        assert!(fills.fill_for(0, 2).is_some());
        assert!(fills.fill_for(1, 2).is_some());
        let skipped = AltFills { skip_last: u32::MAX, ..fills };
        assert!(skipped.fill_for(1, 2).is_none());
        assert!(skipped.fill_for(usize::MAX, usize::MAX).is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let mut s = story("x");
        s.insert_table(1, Table::new(4, 2, 2, 1, 0, 100.0));
        let j = serde_json::to_string(&s).unwrap();
        let back: Story = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        back.check().unwrap();
        // Old files without tables still load.
        let old = r#"{"id":1,"text":"a","paras":[{"style":"[Basic Paragraph]"}],"chars":[{"len":1,"style":"[None]"}],"frames":[]}"#;
        let st: Story = serde_json::from_str(old).unwrap();
        assert!(st.tables.is_empty());
    }

    #[test]
    fn cell_border_overrides_default_for_old_documents_and_roundtrip() {
        let cell = Cell::default();
        let mut old = serde_json::to_value(&cell).unwrap();
        old.as_object_mut().unwrap().remove("borderOverrides");
        let back: Cell = serde_json::from_value(old).unwrap();
        assert_eq!(back.border_overrides, [false; 4]);
        assert_eq!(back.stroke_defined, [false; 4]);
        assert_eq!(back.stroke_priorities, [0; 4]);
        let legacy_options: TableOptions = serde_json::from_str(r#"{"border":{"weight":3}}"#).unwrap();
        assert!(legacy_options.borders.iter().all(Option::is_none));
        assert!(legacy_options.row_strokes.is_none());
        assert!(legacy_options.column_strokes.is_none());
        for side in 0..4 {
            assert_eq!(legacy_options.border_for(side).weight, 3.0);
        }

        let explicit = Cell {
            border_overrides: [true, false, true, false],
            stroke_defined: [true, true, false, false],
            stroke_priorities: [3, 0, 7, 0],
            ..cell
        };
        let back: Cell = serde_json::from_str(&serde_json::to_string(&explicit).unwrap()).unwrap();
        assert_eq!(back, explicit);
        assert_eq!(back.blank_like().border_overrides, explicit.border_overrides);
    }
}
