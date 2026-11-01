use super::values::{span, tint};
use super::{CellRange, El, Importer, RowHeightMode, StoryId, Table, names, swatch};

/// Cells every table in one import may hold between them. A `Cell` carries a whole story, so
/// it is 1,360 bytes: 400,000 of them is about 520 MB, which is the 512 MiB the package reader
/// already allows one part of the file to decompress to, and the same count the spreadsheet
/// importer collects at most.
const MAX_IMPORT_TABLE_CELLS: usize = 400_000;

/// Clamp warnings one import writes before it says the rest were the same.
const MAX_TABLE_WARNINGS: usize = 20;

/// How large a table of `rows` x `cols` may be read when `reserved` cells have already been
/// taken, and whether the file-wide budget is what cut it down -- or `None` when the budget is
/// spent and the table is not read at all.
///
/// The per-table cap leaves a file free to hold many capped tables, so each is also clamped
/// against what is left. While there is budget, one row and one column are always kept: the table
/// is then still in the document, still says where its text went, and a file pays one cell for a
/// table it really does contain.
///
/// Past that there is nothing left to pay with. Keeping a 1 x 1 table for every further `<Table>`
/// was unbounded in the one direction the budget was meant to close: `<Table></Table>` is fifteen
/// bytes, so the 512 MiB a part may decompress to holds some 36 million of them, and a `Cell`
/// each is tens of gigabytes with every table inside both caps (noted in review).
fn budgeted_size(rows: usize, cols: usize, reserved: usize) -> Option<(usize, usize, bool)> {
    let (mut nr, mut nc) = designcraft_doc::table::clamped_size(rows, cols);
    let left = MAX_IMPORT_TABLE_CELLS.checked_sub(reserved).filter(|left| *left > 0)?;
    let over_budget = nr.saturating_mul(nc) > left;
    if over_budget {
        nc = nc.min(left);
        nr = nr.min((left / nc).max(1));
    }
    Some((nr, nc, over_budget))
}

/// Preserve the legacy uniform field only when all four sides explicitly specify every
/// property. Partial records must stay sparse so they can inherit from a parent style.
pub(super) fn uniform_cell_stroke(edges: &[designcraft_doc::CellStrokeAttrs; 4]) -> Option<designcraft_doc::CellStroke> {
    let first = edges.first()?;
    if edges.iter().any(|edge| edge != first) {
        return None;
    }
    let mut stroke = designcraft_doc::CellStroke {
        weight: first.weight?,
        color: first.color.clone()?,
        tint: first.tint?,
        kind: first.kind.clone()?,
        ..Default::default()
    };
    first.apply_to(&mut stroke);
    Some(stroke)
}

impl<'r> Importer<'r> {
    /// `<Table>`: rows, columns and cells (cell `Name` is `column:row`).
    pub(super) fn table(&mut self, e: &El) -> Option<Table> {
        let rows: Vec<&El> = e.find_all("Row").collect();
        let cols: Vec<&El> = e.find_all("Column").collect();
        // A file's row and column counts are its claim, and a table of them is allocated whole: a
        // story holding one `<Table>` with 5,000 of each asked for 25M cells, around 34 GB, which
        // aborts the process instead of failing (#1583). What is read is clamped, and the rest of
        // this function walks the clamped slices -- indexing `t.rows`/`t.columns` by the file's
        // own counts would be out of bounds.
        //
        // Reserved before `Table::new`, so a cell's own story cannot spend the budget this table
        // is counting on.
        let Some((nr, nc, over_budget)) = budgeted_size(rows.len().max(1), cols.len().max(1), self.table_cells) else {
            // Said once: the file that reaches this is the one with millions of tables in it.
            if !self.table_budget_spent {
                self.table_budget_spent = true;
                self.warnings.push(format!(
                    "the tables in this file claim more than {MAX_IMPORT_TABLE_CELLS} cells between them: the rest of them were not read"
                ));
            }
            return None;
        };
        self.table_cells = self.table_cells.saturating_add(nr.saturating_mul(nc));
        if nr < rows.len() || nc < cols.len() {
            let why = if over_budget { "more than the tables in one file can hold between them" } else { "more than a table can hold" };
            self.table_warnings += 1;
            match self.table_warnings.cmp(&MAX_TABLE_WARNINGS) {
                std::cmp::Ordering::Less => {
                    self.warnings.push(format!("a table of {} rows and {} columns was read as {nr} x {nc}: {why}", rows.len(), cols.len()))
                }
                std::cmp::Ordering::Equal => {
                    self.warnings.push(format!("more than {MAX_TABLE_WARNINGS} tables were read smaller than this file claims"))
                }
                std::cmp::Ordering::Greater => {}
            }
        }
        let id = self.alloc();
        let rows = &rows[..nr.min(rows.len())];
        let cols = &cols[..nc.min(cols.len())];
        let mut t = Table::new(id, nr, nc, 0, 0, 0.0);
        t.options.direction = if e.get("TableDirection") == Some("RightToLeftDirection") {
            designcraft_doc::TextDirection::RightToLeft
        } else {
            designcraft_doc::TextDirection::LeftToRight
        };
        for (r, re) in rows.iter().enumerate() {
            let auto = re.boolean("AutoGrow").unwrap_or(true);
            t.rows[r].mode = if auto { RowHeightMode::AtLeast } else { RowHeightMode::Exactly };
            t.rows[r].height = if auto { re.num("MinimumHeight").unwrap_or(3.0) } else { re.num("SingleRowHeight").unwrap_or(12.0) };
        }
        for (c, ce) in cols.iter().enumerate() {
            t.columns[c].width = ce.num("SingleColumnWidth").unwrap_or(72.0).max(3.0);
        }
        let h = e.num("HeaderRowCount").unwrap_or(0.0) as usize;
        let f = e.num("FooterRowCount").unwrap_or(0.0) as usize;
        if h.saturating_add(f) < nr {
            t.set_header_footer(h, f);
        }
        let style_name = e.get("AppliedTableStyle").map(|r| self.table_style_ref(r)).unwrap_or_else(|| designcraft_doc::BASIC_TABLE.into());
        let style = self.styles.resolve_table_style(&style_name);
        style.apply_to(&mut t, &self.styles.cell);
        // IDML distinguishes an explicit AppliedCellStyle from the effective region style.
        for cell in &mut t.cells {
            cell.style.clear();
        }
        // Table-local options override the named table style. Cell styles are already
        // materialized independently, so their edges retain their higher precedence.
        let mut border_unset = style.border.is_none() && style.borders.iter().all(|b| b.is_empty());
        let mut borders = std::array::from_fn::<_, 4, _>(|i| t.options.border_for(i).clone());
        for (side, border) in ["TopBorder", "LeftBorder", "BottomBorder", "RightBorder"].iter().zip(&mut borders) {
            let attrs = self.cell_stroke_attrs(e, side);
            border_unset &= attrs.is_empty();
            attrs.apply_to(border);
        }
        t.options.border = borders[0].clone();
        t.options.borders = borders.map(Some);
        t.options.space_before = e.num("SpaceBefore").unwrap_or(t.options.space_before);
        t.options.space_after = e.num("SpaceAfter").unwrap_or(t.options.space_after);
        t.options.repeat_header = e.get("HeaderBehavior") != Some("RepeatOnce");
        t.options.repeat_footer = e.get("FooterBehavior") != Some("RepeatOnce");
        // A local count may enable a pattern disabled by its parent style. Merge before
        // materialization so the parent's colors/tints are still available in that case.
        let mut row_fills = style.row_fills.clone();
        row_fills.merge(&self.alt_fill_attrs(e, "Row"));
        row_fills.apply_to(&mut t.options.alt_rows);
        let mut column_fills = style.column_fills.clone();
        column_fills.merge(&self.alt_fill_attrs(e, "Column"));
        column_fills.apply_to(&mut t.options.alt_cols);
        let mut row_strokes = style.row_strokes.clone();
        row_strokes.merge(&self.alt_stroke_attrs(e, "Row"));
        row_strokes.apply_to(&mut t.options.row_strokes);
        let mut column_strokes = style.column_strokes.clone();
        column_strokes.merge(&self.alt_stroke_attrs(e, "Column"));
        column_strokes.apply_to(&mut t.options.column_strokes);
        let mut regions = Vec::new();
        for ce in e.find_all("Cell") {
            let Some((c, r)) =
                ce.get("Name").and_then(|n| n.split_once(':')).and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?)))
            else {
                continue;
            };
            if r >= nr || c >= nc {
                continue;
            }
            let rs = (span(ce, "RowSpan").max(1.0) as usize).min(nr - r);
            let cs = (span(ce, "ColumnSpan").max(1.0) as usize).min(nc - c);
            let mut cell = t.cell(r, c).cloned().unwrap_or_default();
            let cell_style = ce.get("AppliedCellStyle").map(|r| self.cell_style_ref(r)).unwrap_or_else(|| designcraft_doc::NO_CELL_STYLE.into());
            if cell_style != designcraft_doc::NO_CELL_STYLE {
                designcraft_doc::resolve_cell_style(&self.styles.cell, &cell_style).apply_to(&mut cell);
            }
            cell.style = if cell_style == designcraft_doc::NO_CELL_STYLE { String::new() } else { cell_style };
            let default_para = cell.text.paras.first().cloned().unwrap_or_default();
            cell.text = self.story_with_para(StoryId(0), ce, default_para);
            self.read_cell_attrs(&mut cell, ce, border_unset);
            if let Some(slot) = t.cell_mut(r, c) {
                *slot = cell;
            }
            if rs > 1 || cs > 1 {
                regions.push(CellRange { r0: r, c0: c, r1: (r + rs - 1).min(nr - 1), c1: (c + cs - 1).min(nc - 1) });
            }
        }
        for rg in regions {
            let _ = t.merge(rg);
        }
        Some(t)
    }

    /// A cell's local fill, insets, alignment, rotation and edge strokes.
    fn read_cell_attrs(&mut self, cell: &mut designcraft_doc::Cell, ce: &El, border_unset: bool) {
        if let Some(fc) = ce.get("FillColor") {
            cell.fill = self.swatch_ref(fc);
        }
        if let Some(tint) = tint(ce.num("FillTint")) {
            cell.fill_tint = tint;
        }
        for (i, k) in ["TopInset", "LeftInset", "BottomInset", "RightInset"].iter().enumerate() {
            if let Some(v) = ce.num(k) {
                cell.insets[i] = v.max(0.0);
            }
        }
        if let Some(vj) = ce.get("VerticalJustification") {
            cell.vj = names::vj_in(vj);
        }
        if let Some(fb) = ce.get("FirstBaselineOffset") {
            cell.first_baseline = names::first_baseline_in(fb);
        }
        if let Some(min) = ce.num("MinimumFirstBaselineOffset").filter(|v| v.is_finite()) {
            cell.first_baseline_min = min.clamp(0.0, 8640.0);
        }
        cell.rotation = ce.num("RotationAngle").unwrap_or(cell.rotation);
        if cell.rotation != 0.0 {
            self.rotated_cells += 1;
        }
        for (i, side) in ["TopEdge", "LeftEdge", "BottomEdge", "RightEdge"].iter().enumerate() {
            let attrs = self.cell_stroke_attrs(ce, side);
            attrs.apply_to(&mut cell.strokes[i]);
            cell.stroke_defined[i] |= !attrs.is_empty();
            let priority = ce.prop(&format!("{side}StrokePriority")).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
            cell.stroke_priorities[i] = priority;
            // Preserve style-defined edges and #102's positive-priority local overrides.
            cell.border_overrides[i] |= priority > 0;
            // No table or table style border is given anywhere, so the built-in 1 pt default must not
            // draw over an outer edge the cell explicitly hides (weight 0 or the None swatch), as InDesign draws nothing.
            let hidden = attrs.weight == Some(0.0) || attrs.color.as_deref() == Some(swatch::NONE);
            cell.border_overrides[i] |= border_unset && hidden;
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use super::{MAX_IMPORT_TABLE_CELLS, budgeted_size};

    /// One table is capped, but a file chooses how many tables it has: three tables each at the
    /// per-table cap asked for 750,000 cells, about 1 GB of `Cell`, with every table inside its
    /// own limit (noted in review of #1583).
    #[test]
    fn the_tables_in_one_file_are_capped_between_them() {
        let mut reserved = 0;
        let mut sizes = Vec::new();
        for _ in 0..4 {
            let size = budgeted_size(500, 1000, reserved);
            if let Some((nr, nc, _)) = size {
                reserved += nr * nc;
            }
            sizes.push(size);
        }
        assert_eq!(sizes[0], Some((250, 1000, false)), "the first table is held by the per-table cap alone");
        assert_eq!(sizes[1], Some((150, 1000, true)), "the second gets what is left of the file's budget");
        assert_eq!(sizes[2], None, "and the third is not read: there is nothing left to pay for it");
        assert_eq!(sizes[3], None, "nor any after it");
        assert_eq!(reserved, MAX_IMPORT_TABLE_CELLS, "the budget is spent exactly, not overspent");
    }

    /// The budget is a hard limit, not a floor: keeping one cell for every further table was
    /// unbounded in the one direction the budget exists to close, since a `<Table></Table>` is
    /// fifteen bytes and a part may decompress to 512 MiB (noted in review).
    #[test]
    fn a_spent_budget_stops_reading_tables_altogether() {
        let claims = [(0, 0), (1, 1), (7, 3), (usize::MAX, usize::MAX), (1, usize::MAX), (usize::MAX, 1), (10_000, 1_000), (2, 0)];
        let mut reserved = 0usize;
        let mut read = 0usize;
        let mut skipped = 0usize;
        // Every claim, several times over, in an order that exhausts the budget part way through.
        for _ in 0..4 {
            for (rows, cols) in claims {
                match budgeted_size(rows.max(1), cols.max(1), reserved) {
                    Some((nr, nc, _)) => {
                        assert!(nr >= 1 && nc >= 1, "{rows} x {cols} was read as {nr} x {nc}: a table has nowhere to put its text");
                        reserved += nr * nc;
                        read += 1;
                    }
                    None => skipped += 1,
                }
                assert!(reserved <= MAX_IMPORT_TABLE_CELLS, "{read} tables reserved {reserved} cells, past the budget");
            }
        }
        assert_eq!(reserved, MAX_IMPORT_TABLE_CELLS, "this sequence does reach the budget");
        assert!(skipped > 0, "and does go on asking after it: {skipped} tables were not read");
    }

    /// Below the budget nothing is clamped by it, so an ordinary file is read exactly as before.
    #[test]
    fn an_ordinary_file_is_not_touched_by_the_budget() {
        for (rows, cols) in [(1, 1), (2, 3), (50, 20), (250, 1000)] {
            assert_eq!(budgeted_size(rows, cols, 0), Some((rows, cols, false)), "{rows} x {cols}");
        }
    }
}
