use super::cell::{Cell, Column, Row, RowKind, TableOptions};
use super::{CharFormat, Deserialize, ParaFormat, Serialize, Story, StoryId, Table};

/// A rectangular range of cells (inclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellRange {
    pub r0: usize,
    pub c0: usize,
    pub r1: usize,
    pub c1: usize,
}

impl CellRange {
    pub fn cell(r: usize, c: usize) -> Self {
        CellRange { r0: r, c0: c, r1: r, c1: c }
    }
    pub fn new(ra: usize, ca: usize, rb: usize, cb: usize) -> Self {
        CellRange { r0: ra.min(rb), c0: ca.min(cb), r1: ra.max(rb), c1: ca.max(cb) }
    }
    pub fn contains(&self, r: usize, c: usize) -> bool {
        r >= self.r0 && r <= self.r1 && c >= self.c0 && c <= self.c1
    }
}

impl Table {
    /// A `rows × cols` table (`header` + body + `footer` rows) whose columns share `width`.
    ///
    /// The shape is clamped to [`clamped_size`](super::clamped_size): the counts reach this from
    /// IDML and DOCX files, where they are whatever the file says (#1583).
    pub fn new(id: u64, body_rows: usize, cols: usize, header: usize, footer: usize, width: f64) -> Table {
        // Header, body and footer rows share one budget: the total is clamped first, then the
        // header and footer take what they can of it, and the body keeps at least one row.
        let (total, cols) = super::clamped_size(body_rows.max(1).saturating_add(header).saturating_add(footer), cols);
        let header = header.min(total.saturating_sub(1));
        let footer = footer.min(total.saturating_sub(header).saturating_sub(1));
        let body_rows = total.saturating_sub(header).saturating_sub(footer).max(1);
        let mut rows = Vec::new();
        rows.extend((0..header).map(|_| Row { kind: RowKind::Header, ..Default::default() }));
        rows.extend((0..body_rows.max(1)).map(|_| Row::default()));
        rows.extend((0..footer).map(|_| Row { kind: RowKind::Footer, ..Default::default() }));
        let w = (width / cols as f64).max(3.0);
        let n = rows.len() * cols;
        Table {
            id,
            rows,
            columns: vec![Column { width: w }; cols],
            cells: vec![Cell::default(); n],
            options: TableOptions::default(),
            style: String::new(),
        }
    }

    /// A table from rows of cell strings (Convert Text to Table). Short rows are padded.
    pub fn from_strings(id: u64, data: &[Vec<String>], width: f64, para: &ParaFormat, fmt: &CharFormat) -> Table {
        let cols = data.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let mut t = Table::new(id, data.len().max(1), cols, 0, 0, width);
        // Every cell takes the text's format, the padding of a short row too.
        for r in 0..t.nrows() {
            for c in 0..t.ncols() {
                let Some(cell) = t.cell_mut(r, c) else { continue };
                cell.text = Story::with_text(StoryId(0), "", ParaFormat { table: None, ..para.clone() });
                cell.text.chars[0].format = fmt.clone();
                if let Some(s) = data.get(r).and_then(|row| row.get(c)) {
                    cell.text.insert(0, s);
                }
            }
        }
        t
    }

    pub fn nrows(&self) -> usize {
        self.rows.len()
    }

    pub fn ncols(&self) -> usize {
        self.columns.len()
    }

    pub fn width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }

    fn idx(&self, r: usize, c: usize) -> Option<usize> {
        (r < self.nrows() && c < self.ncols()).then(|| r * self.ncols() + c)
    }

    pub fn cell(&self, r: usize, c: usize) -> Option<&Cell> {
        self.cells.get(self.idx(r, c)?)
    }

    pub fn cell_mut(&mut self, r: usize, c: usize) -> Option<&mut Cell> {
        let i = self.idx(r, c)?;
        self.cells.get_mut(i)
    }

    pub fn header_rows(&self) -> usize {
        self.rows.iter().take_while(|r| r.kind == RowKind::Header).count()
    }

    pub fn footer_rows(&self) -> usize {
        self.rows.iter().rev().take_while(|r| r.kind == RowKind::Footer).count()
    }

    pub fn body_rows(&self) -> std::ops::Range<usize> {
        self.header_rows()..self.nrows() - self.footer_rows()
    }

    /// For every grid position, the (row, col) of the cell that owns it.
    pub fn owners(&self) -> Vec<(usize, usize)> {
        let (nr, nc) = (self.nrows(), self.ncols());
        let mut v: Vec<(usize, usize)> = (0..nr * nc).map(|i| (i / nc.max(1), i % nc.max(1))).collect();
        for r in 0..nr {
            for c in 0..nc {
                let cell = &self.cells[r * nc + c];
                if v[r * nc + c] != (r, c) {
                    continue;
                }
                for rr in r..(r + cell.row_span.max(1) as usize).min(nr) {
                    for cc in c..(c + cell.col_span.max(1) as usize).min(nc) {
                        v[rr * nc + cc] = (r, c);
                    }
                }
            }
        }
        v
    }

    /// Owning cells touched by the range, once each in row-major order. Bounds are clipped
    /// to the grid; resolving covered positions does not expand the selection rectangle.
    pub fn selected_owners(&self, range: CellRange) -> Vec<(usize, usize)> {
        let (nr, nc) = (self.nrows(), self.ncols());
        if range.r0 >= nr || range.c0 >= nc || range.r0 > range.r1 || range.c0 > range.c1 {
            return Vec::new();
        }
        let owners = self.owners();
        let mut selected = std::collections::BTreeSet::new();
        for r in range.r0..=range.r1.min(nr - 1) {
            for c in range.c0..=range.c1.min(nc - 1) {
                if let Some(&owner) = owners.get(r * nc + c) {
                    selected.insert(owner);
                }
            }
        }
        selected.into_iter().collect()
    }

    /// The owning (top-left) cell address of grid position (r, c).
    pub fn owner(&self, r: usize, c: usize) -> (usize, usize) {
        self.owners().get(r * self.ncols() + c).copied().unwrap_or((r, c))
    }

    /// Merged regions `(r, c, row_span, col_span)` (span > 1 in some direction).
    pub fn regions(&self) -> Vec<(usize, usize, usize, usize)> {
        let owners = self.owners();
        let mut v = Vec::new();
        for r in 0..self.nrows() {
            for c in 0..self.ncols() {
                let cell = &self.cells[r * self.ncols() + c];
                if owners[r * self.ncols() + c] == (r, c) && (cell.row_span > 1 || cell.col_span > 1) {
                    v.push((r, c, cell.row_span as usize, cell.col_span as usize));
                }
            }
        }
        v
    }

    pub(super) fn set_regions(&mut self, regions: &[(usize, usize, usize, usize)]) {
        for c in &mut self.cells {
            c.row_span = 1;
            c.col_span = 1;
        }
        for &(r, c, rs, cs) in regions {
            if rs == 0 || cs == 0 {
                continue;
            }
            if let Some(cell) = self.cell_mut(r, c) {
                cell.row_span = rs as u32;
                cell.col_span = cs as u32;
            }
        }
    }

    /// Cell texts, rows separated by `\n`, cells by `\t` (Convert Table to Text, search, agents).
    pub fn plain_text(&self) -> String {
        self.text_with("\t", "\n")
    }

    /// The cells' text, columns separated by `col` and rows by `row` (Convert Table to Text).
    pub fn text_with(&self, col: &str, row: &str) -> String {
        let mut out = String::new();
        for r in 0..self.nrows() {
            if r > 0 {
                out.push_str(row);
            }
            for c in 0..self.ncols() {
                if c > 0 {
                    out.push_str(col);
                }
                if let Some(cell) = self.cell(r, c) {
                    out.push_str(&cell.text.text.replace('\n', " "));
                }
            }
        }
        out
    }

    pub fn check(&self) -> Result<(), String> {
        if self.rows.is_empty() || self.columns.is_empty() {
            return Err(format!("table {}: no rows or columns", self.id));
        }
        if self.cells.len() != self.nrows() * self.ncols() {
            return Err(format!("table {}: {} cells for {}×{}", self.id, self.cells.len(), self.nrows(), self.ncols()));
        }
        // Header rows first, footer rows last.
        let (h, f) = (self.header_rows(), self.footer_rows());
        if self.rows[h..self.nrows() - f].iter().any(|r| r.kind != RowKind::Body) {
            return Err(format!("table {}: header/footer rows out of order", self.id));
        }
        self.check_spans()?;
        for cell in &self.cells {
            cell.text.check()?;
        }
        Ok(())
    }

    /// Every merged cell stays inside the grid and no two merged cells overlap.
    fn check_spans(&self) -> Result<(), String> {
        let mut seen = vec![false; self.cells.len()];
        for r in 0..self.nrows() {
            for c in 0..self.ncols() {
                let cell = &self.cells[r * self.ncols() + c];
                if cell.row_span == 0 || cell.col_span == 0 {
                    return Err(format!("table {}: zero span at {r},{c}", self.id));
                }
                if cell.row_span == 1 && cell.col_span == 1 {
                    continue;
                }
                if r + cell.row_span as usize > self.nrows() || c + cell.col_span as usize > self.ncols() {
                    return Err(format!("table {}: span out of bounds at {r},{c}", self.id));
                }
                for rr in r..r + cell.row_span as usize {
                    for cc in c..c + cell.col_span as usize {
                        let i = rr * self.ncols() + cc;
                        if seen[i] {
                            return Err(format!("table {}: overlapping merged cells at {rr},{cc}", self.id));
                        }
                        seen[i] = true;
                    }
                }
            }
        }
        Ok(())
    }
}
