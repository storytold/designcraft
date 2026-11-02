use super::cell::Row;
#[cfg(test)]
use super::cell::{RowHeightMode, RowKind};
use super::grid::CellRange;
use super::story::append_story;
use super::{Story, Table};

impl Table {
    /// The smallest range containing `r` that doesn't cut through a merged region.
    pub fn expand_range(&self, mut r: CellRange) -> CellRange {
        loop {
            let mut grown = r;
            for (rr, cc, rs, cs) in self.regions() {
                let (re, ce) = (rr + rs - 1, cc + cs - 1);
                let overlaps = rr <= r.r1 && re >= r.r0 && cc <= r.c1 && ce >= r.c0;
                if overlaps {
                    grown = CellRange { r0: grown.r0.min(rr), c0: grown.c0.min(cc), r1: grown.r1.max(re), c1: grown.c1.max(ce) };
                }
            }
            if grown == r {
                return r;
            }
            r = grown;
        }
    }

    /// Merge a range into its top-left cell; the other cells' text is appended as paragraphs.
    pub fn merge(&mut self, range: CellRange) -> Result<(), String> {
        if range.r1 >= self.nrows() || range.c1 >= self.ncols() {
            return Err("cell range out of bounds".into());
        }
        let range = self.expand_range(range);
        if range.r0 == range.r1 && range.c0 == range.c1 {
            return Ok(());
        }
        let owners = self.owners();
        let nc = self.ncols();
        let mut regions: Vec<_> = self.regions().into_iter().filter(|&(r, c, _, _)| !range.contains(r, c)).collect();
        let mut extra: Vec<Story> = Vec::new();
        for r in range.r0..=range.r1 {
            for c in range.c0..=range.c1 {
                if (r, c) != (range.r0, range.c0) && owners[r * nc + c] == (r, c) {
                    let t = std::mem::take(&mut self.cells[r * nc + c]);
                    self.cells[r * nc + c] = t.blank_like();
                    if !t.text.is_empty() {
                        extra.push(t.text);
                    }
                }
            }
        }
        let owner = &mut self.cells[range.r0 * nc + range.c0];
        for s in extra {
            append_story(&mut owner.text, &s);
        }
        regions.push((range.r0, range.c0, range.r1 - range.r0 + 1, range.c1 - range.c0 + 1));
        self.set_regions(&regions);
        Ok(())
    }

    /// Unmerge the merged cell covering (r, c).
    pub fn unmerge(&mut self, r: usize, c: usize) {
        let (or, oc) = self.owner(r, c);
        let regions: Vec<_> = self.regions().into_iter().filter(|&(rr, cc, _, _)| (rr, cc) != (or, oc)).collect();
        self.set_regions(&regions);
    }

    /// Split each original owner touched by `range` once. Remember merged-cell cuts in the
    /// original grid: a neighboring split may refine that grid before we reach the owner.
    pub fn split_cells(&mut self, range: CellRange, horizontal: bool) -> Result<(), String> {
        if range.r0 > range.r1 || range.c0 > range.c1 || range.r1 >= self.nrows() || range.c1 >= self.ncols() {
            return Err("cell range out of bounds".into());
        }
        let owners = self.owners();
        let mut selected = std::collections::BTreeSet::new();
        for r in range.r0..=range.r1 {
            for c in range.c0..=range.c1 {
                let owner = owners.get(r * self.ncols() + c).ok_or("missing cell owner")?;
                selected.insert(*owner);
            }
        }
        let mut targets = Vec::new();
        for (r, c) in selected {
            let cell = self.cell(r, c).ok_or("missing split cell")?;
            let span = if horizontal { cell.row_span } else { cell.col_span } as usize;
            let start = if horizontal { r } else { c };
            let cut = if span > 1 { Some(start.checked_add(span.div_ceil(2)).ok_or("invalid split boundary")?) } else { None };
            targets.push((r, c, cut));
        }
        for i in 0..targets.len() {
            let (r, c, cut) = *targets.get(i).ok_or("missing split target")?;
            let cell = self.cell(r, c).ok_or("missing split cell")?;
            let (rs, cs) = (cell.row_span as usize, cell.col_span as usize);
            let mut blank = cell.blank_like();
            // Splitting creates an empty half, not a duplicate of graphic-cell content.
            blank.graphic = None;
            let before = if horizontal { self.nrows() } else { self.ncols() };
            if let Some(cut) = cut {
                let (start, span) = if horizontal { (r, rs) } else { (c, cs) };
                let first = cut.checked_sub(start).filter(|n| *n > 0 && *n < span).ok_or("invalid split boundary")?;
                let mut regions: Vec<_> = self.regions().into_iter().filter(|&(rr, cc, _, _)| (rr, cc) != (r, c)).collect();
                if horizontal {
                    regions.push((r, c, first, cs));
                    regions.push((cut, c, rs - first, cs));
                } else {
                    regions.push((r, c, rs, first));
                    regions.push((r, cut, rs, cs - first));
                }
                self.set_regions(&regions);
            } else {
                // An originally single-track cell may now span two tracks inserted for its
                // neighbor. split_cell divides that span instead of inserting another track.
                self.split_cell(r, c, horizontal);
            }
            let first = self.cell(r, c).ok_or("missing split half")?;
            let second = if horizontal { (r + first.row_span as usize, c) } else { (r, c + first.col_span as usize) };
            let second = self.cell_mut(second.0, second.1).ok_or("missing split half")?;
            blank.row_span = second.row_span;
            blank.col_span = second.col_span;
            *second = blank;
            let after = if horizontal { self.nrows() } else { self.ncols() };
            if after > before {
                let at = if horizontal { r + 1 } else { c + 1 };
                let added = after - before;
                for (row, col, cut) in targets.iter_mut().skip(i + 1) {
                    let axis = if horizontal { row } else { col };
                    if *axis >= at {
                        *axis += added;
                    }
                    if let Some(cut) = cut
                        && *cut >= at
                    {
                        *cut += added;
                    }
                }
            }
        }
        Ok(())
    }

    /// Split the cell covering (r, c) in two: `horizontal` stacks the halves (Split Cell
    /// Horizontally), otherwise side by side. A merged cell splits its span; a single cell gets a
    /// new row (column) that the cells beside it span, the row height (column width) shared.
    pub fn split_cell(&mut self, r: usize, c: usize, horizontal: bool) {
        if r >= self.nrows() || c >= self.ncols() {
            return;
        }
        let (or, oc) = self.owner(r, c);
        let (rs, cs) = self.cell(or, oc).map_or((1, 1), |x| (x.row_span as usize, x.col_span as usize));
        let mut regions: Vec<_> = self.regions().into_iter().filter(|&(rr, cc, _, _)| (rr, cc) != (or, oc)).collect();
        let span = if horizontal { rs } else { cs };
        if span > 1 {
            let a = span.div_ceil(2);
            if horizontal {
                regions.push((or, oc, a, cs));
                regions.push((or + a, oc, rs - a, cs));
            } else {
                regions.push((or, oc, rs, a));
                regions.push((or, oc + a, rs, cs - a));
            }
            self.set_regions(&regions);
            return;
        }
        if horizontal {
            let row = self.rows[or].clone();
            self.insert_rows(or + 1, 1);
            self.rows[or].height = row.height / 2.0;
            // Both halves keep the split row's height mode and header/body/footer role.
            self.rows[or + 1] = Row { height: row.height / 2.0, ..row };
            // insert_rows doesn't grow regions that end at the split row; everything else on it
            // spans the new row now.
            let mut regions: Vec<_> = self.regions();
            let (nr, nc) = (self.nrows(), self.ncols());
            let mut covered = vec![false; nr * nc];
            for &(rr, cc, rrs, ccs) in &regions {
                for y in rr..rr + rrs {
                    for x in cc..cc + ccs {
                        covered[y * nc + x] = true;
                    }
                }
            }
            for reg in regions.iter_mut() {
                let (rr, cc, rrs, ccs) = *reg;
                if rr + rrs == or + 1 && !(cc <= oc && oc < cc + ccs) {
                    *reg = (rr, cc, rrs + 1, ccs);
                }
            }
            for x in 0..nc {
                if (oc..oc + cs).contains(&x) || covered[or * nc + x] {
                    continue;
                }
                regions.push((or, x, 2, 1));
            }
            if cs > 1 {
                regions.push((or + 1, oc, 1, cs));
            }
            self.set_regions(&regions);
        } else {
            let w = self.columns[oc].width;
            self.insert_cols(oc + 1, 1, w / 2.0);
            self.columns[oc].width = w / 2.0;
            let mut regions: Vec<_> = self.regions();
            let (nr, nc) = (self.nrows(), self.ncols());
            let mut covered = vec![false; nr * nc];
            for &(rr, cc, rrs, ccs) in &regions {
                for y in rr..rr + rrs {
                    for x in cc..cc + ccs {
                        covered[y * nc + x] = true;
                    }
                }
            }
            for reg in regions.iter_mut() {
                let (rr, cc, rrs, ccs) = *reg;
                if cc + ccs == oc + 1 && !(rr <= or && or < rr + rrs) {
                    *reg = (rr, cc, rrs, ccs + 1);
                }
            }
            for y in 0..nr {
                if (or..or + rs).contains(&y) || covered[y * nc + oc] {
                    continue;
                }
                regions.push((y, oc, 1, 2));
            }
            if rs > 1 {
                regions.push((or, oc + 1, rs, 1));
            }
            self.set_regions(&regions);
        }
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn split_single_and_merged_cells() {
        let mut t = Table::new(1, 2, 2, 0, 0, 100.0);
        t.split_cell(0, 0, true);
        assert_eq!((t.nrows(), t.ncols()), (3, 2));
        // The cell beside the split one spans both new rows.
        assert_eq!(t.cell(0, 1).unwrap().row_span, 2);
        assert_eq!(t.owner(1, 1), (0, 1));
        assert_eq!(t.owner(1, 0), (1, 0), "the new half is its own cell");
        t.split_cell(2, 1, false);
        assert_eq!(t.ncols(), 3);
        assert_eq!(t.cell(0, 0).unwrap().col_span, 1);
        assert_eq!(t.owner(2, 2), (2, 2));
        assert_eq!(t.owner(0, 2), (0, 1), "the merged cell above widens");
        // A merged cell splits its own span instead of adding rows.
        let mut m = Table::new(1, 4, 1, 0, 0, 100.0);
        m.merge(CellRange { r0: 0, c0: 0, r1: 3, c1: 0 }).unwrap();
        m.split_cell(0, 0, true);
        assert_eq!(m.nrows(), 4);
        assert_eq!((m.cell(0, 0).unwrap().row_span, m.cell(2, 0).unwrap().row_span), (2, 2));
    }

    #[test]
    fn split_range_keeps_original_merged_cut_when_neighbor_tracks_are_refined() {
        for horizontal in [true, false] {
            let mut t = Table::new(1, 4, 4, 0, 0, 280.0);
            for (i, size) in [40.0, 60.0, 80.0, 100.0].into_iter().enumerate() {
                t.rows[i].mode = RowHeightMode::Exactly;
                t.rows[i].height = if horizontal { size } else { 70.0 };
                t.columns[i].width = if horizontal { 70.0 } else { size };
            }
            for r in 0..4 {
                for c in 0..4 {
                    let cell = t.cell_mut(r, c).unwrap();
                    cell.text.insert(0, &format!("{r},{c}"));
                    cell.style = format!("style-{r}-{c}");
                    cell.insets = [r as f64 + c as f64 + 1.0; 4];
                }
            }
            let (merged, selected, owner, lower, neighbor, moved_neighbor) = if horizontal {
                (CellRange::new(0, 1, 2, 1), CellRange::new(0, 0, 2, 1), (0, 1), (4, 1), (3, 3), (6, 3))
            } else {
                (CellRange::new(1, 0, 1, 2), CellRange::new(0, 0, 1, 2), (1, 0), (1, 4), (3, 3), (3, 6))
            };
            t.merge(merged).unwrap();
            let original = t.clone();
            t.split_cells(selected, horizontal).unwrap();
            t.check().unwrap();
            assert_eq!((t.nrows(), t.ncols()), if horizontal { (7, 4) } else { (4, 7) });
            let sizes: Vec<_> = if horizontal { t.rows.iter().map(|r| r.height).collect() } else { t.columns.iter().map(|c| c.width).collect() };
            assert_eq!(sizes, [20.0, 20.0, 30.0, 30.0, 40.0, 40.0, 100.0]);
            let first = t.cell(owner.0, owner.1).unwrap();
            let second = t.cell(lower.0, lower.1).unwrap();
            // The original three-track merge divides after its second original track,
            // regardless of which of those tracks a neighboring selected cell subdivides.
            assert_eq!(if horizontal { first.row_span } else { first.col_span }, 4);
            assert_eq!(if horizontal { second.row_span } else { second.col_span }, 2);
            assert_eq!(first.text, original.cell(owner.0, owner.1).unwrap().text);
            assert!(second.text.is_empty());
            assert_eq!(second.style, first.style);
            assert_eq!(second.insets, first.insets);
            assert_eq!(t.cell(moved_neighbor.0, moved_neighbor.1), original.cell(neighbor.0, neighbor.1));
            let owners = t.owners();
            assert_eq!(owners.iter().copied().collect::<std::collections::BTreeSet<_>>().len(), 18, "four original selected owners split once each");
            let mut before_text: Vec<_> = original.cells.iter().filter(|c| !c.text.is_empty()).map(|c| c.text.text.clone()).collect();
            let mut after_text: Vec<_> = t.cells.iter().filter(|c| !c.text.is_empty()).map(|c| c.text.text.clone()).collect();
            before_text.sort();
            after_text.sort();
            assert_eq!(after_text, before_text, "no original text is copied or lost");
        }
    }

    #[test]
    fn split_range_resolves_covered_positions_without_expanding_selection() {
        for horizontal in [true, false] {
            let mut t = Table::new(1, 3, 3, 0, 0, 180.0);
            t.merge(CellRange::new(0, 0, 1, 1)).unwrap();
            let selected = if horizontal { CellRange::new(1, 1, 1, 2) } else { CellRange::new(1, 1, 2, 1) };
            t.split_cells(selected, horizontal).unwrap();
            t.check().unwrap();
            let owners = t.owners();
            assert_eq!(owners.iter().copied().collect::<std::collections::BTreeSet<_>>().len(), 8, "only the two touched owners split");
            assert_eq!((t.nrows(), t.ncols()), if horizontal { (4, 3) } else { (3, 4) });
            assert_eq!(t.owner(0, 0), (0, 0));
            assert_eq!(if horizontal { t.owner(1, 0) } else { t.owner(0, 1) }, if horizontal { (1, 0) } else { (0, 1) });
        }
    }

    #[test]
    fn split_header_row_keeps_its_height_mode_and_role() {
        let mut t = Table::new(1, 1, 2, 1, 1, 180.0);
        t.rows[0].height = 60.0;
        t.rows[0].mode = RowHeightMode::Exactly;
        let body = t.rows[1].clone();
        let footer = t.rows[2].clone();
        t.split_cells(CellRange::new(0, 0, 0, 1), true).unwrap();
        t.check().unwrap();
        assert_eq!(t.header_rows(), 2);
        for row in &t.rows[..2] {
            assert_eq!(row.height, 30.0);
            assert_eq!(row.mode, RowHeightMode::Exactly);
            assert_eq!(row.kind, RowKind::Header);
        }
        assert_eq!(t.rows[2], body);
        assert_eq!(t.rows[3], footer);
    }
}
