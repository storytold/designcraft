use super::Table;
use super::cell::{Cell, Column, RowKind};

impl Table {
    /// Insert `n` rows before row `at` (formatted like the neighbouring row).
    pub fn insert_rows(&mut self, at: usize, n: usize) {
        let at = at.min(self.nrows());
        if n == 0 {
            return;
        }
        let nc = self.ncols();
        let src = if at < self.nrows() { at } else { at.saturating_sub(1) };
        let mut row = self.rows.get(src).cloned().unwrap_or_default();
        // A row inserted between body rows (or after the last header / before the first footer) is a body row.
        if at > 0 && at < self.nrows() && self.rows[at - 1].kind != self.rows[at].kind {
            row.kind = RowKind::Body;
        }
        if at == self.nrows() && row.kind == RowKind::Header {
            row.kind = RowKind::Body;
        }
        if at == 0 && row.kind == RowKind::Footer {
            row.kind = RowKind::Body;
        }
        let template: Vec<Cell> = (0..nc).map(|c| self.cell(src, c).map(Cell::blank_like).unwrap_or_default()).collect();
        let regions: Vec<_> = self
            .regions()
            .into_iter()
            .map(|(r, c, rs, cs)| {
                if r >= at {
                    (r + n, c, rs, cs)
                } else if r + rs > at {
                    (r, c, rs + n, cs)
                } else {
                    (r, c, rs, cs)
                }
            })
            .collect();
        for k in 0..n {
            self.rows.insert(at + k, row.clone());
            let i = (at + k) * nc;
            for (c, cell) in template.iter().enumerate() {
                self.cells.insert(i + c, cell.clone());
            }
        }
        self.set_regions(&regions);
    }

    /// Delete rows `at..at+n` (at least one row always remains).
    pub fn delete_rows(&mut self, at: usize, n: usize) {
        let nr = self.nrows();
        let at = at.min(nr);
        let n = n.min(nr - at).min(nr.saturating_sub(1));
        if n == 0 {
            return;
        }
        let nc = self.ncols();
        let end = at + n;
        let mut regions = Vec::new();
        for (r, c, rs, cs) in self.regions() {
            let r1 = r + rs;
            if r >= at && r1 <= end {
                continue; // removed entirely
            }
            let new_rs = rs - (r1.min(end).saturating_sub(r.max(at)));
            if r >= at && r < end {
                // The owner row goes away: move its content to the first surviving row.
                let content = self.cells[r * nc + c].clone();
                self.cells[end * nc + c] = content;
                regions.push((at, c, new_rs, cs));
            } else if r < at {
                regions.push((r, c, new_rs, cs));
            } else {
                regions.push((r - n, c, rs, cs));
            }
        }
        self.rows.drain(at..end);
        self.cells.drain(at * nc..end * nc);
        self.set_regions(&regions);
    }

    /// Insert `n` columns of `width` before column `at`.
    pub fn insert_cols(&mut self, at: usize, n: usize, width: f64) {
        let at = at.min(self.ncols());
        if n == 0 {
            return;
        }
        let nc = self.ncols();
        let src = if at < nc { at } else { at.saturating_sub(1) };
        let regions: Vec<_> = self
            .regions()
            .into_iter()
            .map(|(r, c, rs, cs)| {
                if c >= at {
                    (r, c + n, rs, cs)
                } else if c + cs > at {
                    (r, c, rs, cs + n)
                } else {
                    (r, c, rs, cs)
                }
            })
            .collect();
        let mut cells = Vec::with_capacity(self.nrows() * (nc + n));
        for r in 0..self.nrows() {
            for c in 0..nc {
                if c == at {
                    let t = self.cells[r * nc + src].blank_like();
                    cells.extend(std::iter::repeat_n(t, n));
                }
                cells.push(self.cells[r * nc + c].clone());
            }
            if at == nc {
                let t = self.cells[r * nc + src].blank_like();
                cells.extend(std::iter::repeat_n(t, n));
            }
        }
        self.cells = cells;
        for k in 0..n {
            self.columns.insert(at + k, Column { width });
        }
        self.set_regions(&regions);
    }

    /// Delete columns `at..at+n` (at least one column always remains).
    pub fn delete_cols(&mut self, at: usize, n: usize) {
        let nc = self.ncols();
        let at = at.min(nc);
        let n = n.min(nc - at).min(nc.saturating_sub(1));
        if n == 0 {
            return;
        }
        let end = at + n;
        let mut regions = Vec::new();
        for (r, c, rs, cs) in self.regions() {
            let c1 = c + cs;
            if c >= at && c1 <= end {
                continue;
            }
            let new_cs = cs - (c1.min(end).saturating_sub(c.max(at)));
            if c >= at && c < end {
                let content = self.cells[r * nc + c].clone();
                self.cells[r * nc + end] = content;
                regions.push((r, at, rs, new_cs));
            } else if c < at {
                regions.push((r, c, rs, new_cs));
            } else {
                regions.push((r, c - n, rs, cs));
            }
        }
        let mut cells = Vec::with_capacity(self.nrows() * (nc - n));
        for r in 0..self.nrows() {
            for c in 0..nc {
                if c < at || c >= end {
                    cells.push(self.cells[r * nc + c].clone());
                }
            }
        }
        self.cells = cells;
        self.columns.drain(at..end);
        self.set_regions(&regions);
    }

    /// Move row `from` to position `to` (indices before the move). Refused when merged cells span
    /// rows (the move would cut them) or when the move would take a row across a header/footer
    /// boundary (`from` and `to` must be the same kind of row).
    pub fn move_row(&mut self, from: usize, to: usize) -> Result<(), String> {
        let (nr, nc) = (self.nrows(), self.ncols());
        if from >= nr || to >= nr {
            return Err("no such row".into());
        }
        if self.regions().iter().any(|r| r.2 > 1) {
            return Err("unmerge cells that span rows first".into());
        }
        if self.rows[from].kind != self.rows[to].kind {
            return Err("header and footer rows must stay first and last".into());
        }
        let row = self.rows.remove(from);
        self.rows.insert(to, row);
        let cells: Vec<Cell> = self.cells.drain(from * nc..(from + 1) * nc).collect();
        let at = to * nc;
        for (k, c) in cells.into_iter().enumerate() {
            self.cells.insert(at + k, c);
        }
        Ok(())
    }

    /// Move column `from` to position `to`. Refused when merged cells span columns.
    pub fn move_col(&mut self, from: usize, to: usize) -> Result<(), String> {
        let (nr, nc) = (self.nrows(), self.ncols());
        if from >= nc || to >= nc {
            return Err("no such column".into());
        }
        if self.regions().iter().any(|r| r.3 > 1) {
            return Err("unmerge cells that span columns first".into());
        }
        let col = self.columns.remove(from);
        self.columns.insert(to, col);
        for r in 0..nr {
            let c = self.cells.remove(r * nc + from);
            self.cells.insert(r * nc + to, c);
        }
        Ok(())
    }

    /// Mark the first `header` rows as header rows and the last `footer` rows as footer rows.
    pub fn set_header_footer(&mut self, header: usize, footer: usize) {
        let n = self.nrows();
        let header = header.min(n.saturating_sub(1));
        let footer = footer.min(n - header - 1);
        for (i, r) in self.rows.iter_mut().enumerate() {
            r.kind = if i < header {
                RowKind::Header
            } else if i >= n - footer {
                RowKind::Footer
            } else {
                RowKind::Body
            };
        }
    }

    /// Header and footer row counts after Table ▸ Convert Rows turns rows `r0..=r1` into `kind`
    /// rows. Header rows grow from the top and footer rows from the bottom, so the rows must
    /// touch the table's edge or the rows already converted; back to body, they must touch the
    /// body. One body row stays, and no merged cell may straddle a moved boundary.
    pub fn convert_rows_counts(&self, r0: usize, r1: usize, kind: RowKind) -> Result<(usize, usize), String> {
        let (r0, r1) = (r0.min(r1), r0.max(r1));
        let n = self.nrows();
        if r1 >= n {
            return Err("row out of range".into());
        }
        let (h, f) = (self.header_rows(), self.footer_rows());
        let body_end = n - f;
        let (nh, nf) = match kind {
            RowKind::Header if r0 > h => return Err("select rows at the top of the table or right after its header rows".into()),
            RowKind::Header if r1 >= body_end => return Err("footer rows can't become header rows".into()),
            RowKind::Header => (h.max(r1 + 1), f),
            RowKind::Footer if r1 + 1 < body_end => return Err("select rows at the bottom of the table or right before its footer rows".into()),
            RowKind::Footer if r0 < h => return Err("header rows can't become footer rows".into()),
            RowKind::Footer => (h, f.max(n - r0)),
            RowKind::Body => {
                if (r0 < h && r1 + 1 < h) || (r1 >= body_end && r0 > body_end) {
                    return Err("select the header or footer rows next to the body".into());
                }
                (if r0 < h { r0 } else { h }, if r1 >= body_end { n - 1 - r1 } else { f })
            }
        };
        if (nh, nf) == (h, f) {
            return Err("the rows already are of that kind".into());
        }
        if nh + nf >= n {
            return Err("a table keeps at least one body row".into());
        }
        let owners = self.owners();
        let nc = self.ncols();
        let straddled = |b: usize| (0..nc).any(|c| owners.get(b * nc + c).is_some_and(|o| o.0 < b));
        if (nh != h && straddled(nh)) || (nf != f && straddled(n - nf)) {
            return Err("unmerge the cells that span header, body and footer rows first".into());
        }
        Ok((nh, nf))
    }
}
