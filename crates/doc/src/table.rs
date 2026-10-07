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
use crate::item::{StrokeType, VerticalJustification};
use crate::story::{CharFormat, ParaFormat, Story, TABLE_ANCHOR};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowHeightMode {
    /// Grows with the content (the height is a minimum).
    #[default]
    AtLeast,
    Exactly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowKind {
    Header,
    #[default]
    Body,
    Footer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Row {
    pub height: f64,
    pub mode: RowHeightMode,
    pub kind: RowKind,
}

impl Default for Row {
    fn default() -> Self {
        Row { height: 3.0, mode: RowHeightMode::AtLeast, kind: RowKind::Body }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub width: f64,
}

/// One cell edge or the table border.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CellStroke {
    pub weight: f64,
    pub color: String,
    pub tint: f32,
    pub kind: StrokeType,
}

impl Default for CellStroke {
    fn default() -> Self {
        CellStroke { weight: 1.0, color: designcraft_color::swatch::BLACK.into(), tint: 1.0, kind: StrokeType::Solid }
    }
}

impl CellStroke {
    pub fn none() -> Self {
        CellStroke { weight: 0.0, ..Default::default() }
    }
    pub fn is_visible(&self) -> bool {
        self.weight > 0.0 && self.color != designcraft_color::swatch::NONE
    }
}

fn one() -> u32 {
    1
}
fn full() -> f32 {
    1.0
}
fn no_fill() -> String {
    designcraft_color::swatch::NONE.into()
}
fn default_insets() -> [f64; 4] {
    [4.0; 4]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    #[serde(default = "one")]
    pub row_span: u32,
    #[serde(default = "one")]
    pub col_span: u32,
    /// Cell content (a frameless story).
    pub text: Story,
    #[serde(default = "no_fill")]
    pub fill: String,
    #[serde(default = "full")]
    pub fill_tint: f32,
    /// Top, left, bottom, right.
    #[serde(default = "default_insets")]
    pub insets: [f64; 4],
    #[serde(default)]
    pub vj: VerticalJustification,
    /// Text rotation in degrees (only 0 is composed for now).
    #[serde(default)]
    pub rotation: f64,
    /// Edge strokes: top, left, bottom, right.
    #[serde(default)]
    pub strokes: [CellStroke; 4],
    /// Applied cell style ("" = [None]).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// Graphic cell: an image in the cell instead of text (its `xf` maps into the cell's
    /// content box, origin at the box's top left).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphic: Option<crate::Graphic>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            row_span: 1,
            col_span: 1,
            text: Story::new(StoryId(0)),
            fill: no_fill(),
            fill_tint: 1.0,
            insets: default_insets(),
            vj: VerticalJustification::Top,
            rotation: 0.0,
            strokes: Default::default(),
            style: String::new(),
            graphic: None,
        }
    }
}

impl Cell {
    pub fn with_text(text: &str, para: ParaFormat) -> Cell {
        Cell { text: Story::with_text(StoryId(0), text, para), ..Default::default() }
    }
    /// A cell with this cell's formatting and no text (new rows / columns, unmerged cells).
    pub fn blank_like(&self) -> Cell {
        let mut c = Cell { row_span: 1, col_span: 1, ..self.clone() };
        let para = self.text.paras.first().cloned().map(|p| ParaFormat { table: None, ..p }).unwrap_or_default();
        let fmt = self.text.chars.first().map(|r| r.format.clone()).unwrap_or_default();
        c.text = Story::new(StoryId(0));
        c.text.paras[0] = para;
        c.text.chars[0].format = fmt;
        c
    }
    pub fn has_fill(&self) -> bool {
        self.fill != designcraft_color::swatch::NONE
    }
}

/// Table → Table Options → Fills (alternating pattern).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AltFills {
    pub first: u32,
    pub first_color: String,
    pub first_tint: f32,
    pub next: u32,
    pub next_color: String,
    pub next_tint: f32,
    pub skip_first: u32,
    pub skip_last: u32,
}

impl Default for AltFills {
    fn default() -> Self {
        AltFills {
            first: 1,
            first_color: designcraft_color::swatch::BLACK.into(),
            first_tint: 0.2,
            next: 1,
            next_color: designcraft_color::swatch::NONE.into(),
            next_tint: 1.0,
            skip_first: 0,
            skip_last: 0,
        }
    }
}

impl AltFills {
    /// Fill for the `i`-th of `n` body rows/columns, if the pattern paints it.
    pub fn fill_for(&self, i: usize, n: usize) -> Option<(&str, f32)> {
        let (sf, sl) = (self.skip_first as usize, self.skip_last as usize);
        if i < sf || i + sl >= n {
            return None;
        }
        let period = (self.first + self.next).max(1) as usize;
        let k = (i - sf) % period;
        let (c, t) = if k < self.first as usize { (&self.first_color, self.first_tint) } else { (&self.next_color, self.next_tint) };
        (c != designcraft_color::swatch::NONE).then_some((c.as_str(), t))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TableOptions {
    /// Logical column zero is drawn at the right edge for RTL tables.
    pub direction: crate::TextDirection,
    /// Outer border (overrides the outer cell edges).
    pub border: CellStroke,
    pub space_before: f64,
    pub space_after: f64,
    /// Header rows repeat at the top of every text column / frame the table continues into.
    pub repeat_header: bool,
    pub repeat_footer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_rows: Option<AltFills>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_cols: Option<AltFills>,
}

impl Default for TableOptions {
    fn default() -> Self {
        TableOptions {
            direction: crate::TextDirection::LeftToRight,
            border: CellStroke::default(),
            space_before: 4.0,
            space_after: -4.0,
            repeat_header: true,
            repeat_footer: true,
            alt_rows: None,
            alt_cols: None,
        }
    }
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
    pub fn new(id: u64, body_rows: usize, cols: usize, header: usize, footer: usize, width: f64) -> Table {
        let cols = cols.max(1);
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
        for (r, row) in data.iter().enumerate() {
            for (c, s) in row.iter().enumerate() {
                let Some(cell) = t.cell_mut(r, c) else { continue };
                cell.text = Story::with_text(StoryId(0), "", ParaFormat { table: None, ..para.clone() });
                cell.text.chars[0].format = fmt.clone();
                cell.text.insert(0, s);
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

    fn set_regions(&mut self, regions: &[(usize, usize, usize, usize)]) {
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
            let h = self.rows[or].height;
            self.insert_rows(or + 1, 1);
            self.rows[or].height = h / 2.0;
            self.rows[or + 1].height = h / 2.0;
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

    /// Move row `from` to position `to` (indices before the move). Refused when merged cells span
    /// rows (the move would cut them).
    pub fn move_row(&mut self, from: usize, to: usize) -> Result<(), String> {
        let (nr, nc) = (self.nrows(), self.ncols());
        if from >= nr || to >= nr {
            return Err("no such row".into());
        }
        if self.regions().iter().any(|r| r.2 > 1) {
            return Err("unmerge cells that span rows first".into());
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

    /// Cell texts, rows separated by `\n`, cells by `\t` (Convert Table to Text, search, agents).
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for r in 0..self.nrows() {
            if r > 0 {
                out.push('\n');
            }
            for c in 0..self.ncols() {
                if c > 0 {
                    out.push('\t');
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
        for cell in &self.cells {
            cell.text.check()?;
        }
        Ok(())
    }
}

/// Append `other` to `s` as new paragraphs.
fn append_story(s: &mut Story, other: &Story) {
    if s.is_empty() {
        *s = Story { id: s.id, ..other.clone() };
        s.rev += 1;
        return;
    }
    let base = s.len();
    s.insert(base, "\n");
    // Copy other's runs with their formats.
    for (r, f) in other.runs() {
        let at = s.len();
        s.insert_with(at, &other.text[r], f.clone());
    }
    let first_new = s.paras.len() - other.paras.len();
    for (k, p) in other.paras.iter().enumerate() {
        if let Some(dst) = s.paras.get_mut(first_new + k) {
            *dst = p.clone();
        }
    }
}

// ---------- tables in stories ----------

impl crate::Document {
    /// The story text edits go to: a cell's story when `cell` is set, else the story itself.
    pub fn text_story(&self, story: StoryId, cell: Option<crate::CellAddr>) -> Option<&Story> {
        let st = self.story(story)?;
        match cell {
            Some(c) if c.table == crate::FOOTNOTE_TABLE => st.note(c.row as u64).map(|n| &n.text),
            Some(c) => st.tables.get(&c.table)?.cell(c.row, c.col).map(|x| &x.text),
            None => Some(st),
        }
    }

    /// Mutable [`Document::text_story`] (a cell edit bumps the containing story's revision).
    pub fn text_story_mut(&mut self, story: StoryId, cell: Option<crate::CellAddr>) -> Option<&mut Story> {
        let st = self.story_mut(story)?;
        match cell {
            Some(c) if c.table == crate::FOOTNOTE_TABLE => st.note_mut(c.row as u64).map(|n| &mut n.text),
            Some(c) => st.table_mut(c.table)?.cell_mut(c.row, c.col).map(|x| &mut x.text),
            None => Some(st),
        }
    }
}

impl Story {
    /// Visit this story and every text inside it (table cells, footnotes), mutably.
    pub fn for_each_text_mut(&mut self, f: &mut impl FnMut(&mut Story)) {
        f(self);
        for t in self.tables.values_mut() {
            for c in &mut std::sync::Arc::make_mut(t).cells {
                c.text.for_each_text_mut(f);
            }
        }
        for n in &mut self.notes {
            std::sync::Arc::make_mut(n).text.for_each_text_mut(f);
        }
    }

    /// The table whose anchor paragraph is `pi`.
    pub fn para_table(&self, pi: usize) -> Option<&Arc<Table>> {
        self.tables.get(&self.paras.get(pi)?.table?)
    }

    /// Byte offset of a table's anchor character.
    pub fn table_anchor(&self, id: u64) -> Option<usize> {
        let pi = self.paras.iter().position(|p| p.table == Some(id))?;
        let r = self.para_ranges().get(pi)?.clone();
        self.text[r.clone()].find(TABLE_ANCHOR).map(|k| r.start + k)
    }

    /// The table anchored at or right before byte `pos` (a caret next to the anchor).
    pub fn table_at(&self, pos: usize) -> Option<u64> {
        let pi = self.para_at(pos);
        self.paras.get(pi)?.table
    }

    /// Mutable table (copy-on-write); bumps the story revision.
    pub fn table_mut(&mut self, id: u64) -> Option<&mut Table> {
        let t = self.tables.get_mut(&id)?;
        self.rev += 1;
        Some(Arc::make_mut(t))
    }

    /// Insert `table` at `pos`, giving its anchor its own paragraph. Returns the anchor's byte offset.
    pub fn insert_table(&mut self, pos: usize, table: Table) -> usize {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        let pi = self.para_at(pos);
        let pstart = self.para_ranges()[pi].start;
        let mut s = String::new();
        if pos > pstart {
            s.push('\n');
        }
        let anchor = pos + s.len();
        s.push(TABLE_ANCHOR);
        if !self.text[pos..].starts_with('\n') {
            s.push('\n');
        }
        let id = table.id;
        self.tables.insert(id, Arc::new(table));
        // Insert the text first (fix-up would drop an unreferenced table), then point the paragraph at it.
        self.insert_raw(pos, &s);
        let api = self.para_at(anchor);
        self.paras[api].table = Some(id);
        self.fix_tables();
        self.rev += 1;
        anchor
    }

    /// Keep paragraphs and tables consistent after an edit: a paragraph keeps its table only if it
    /// still contains an anchor character, every table is referenced once, and tables nobody
    /// references are dropped.
    pub(crate) fn fix_tables(&mut self) {
        if self.tables.is_empty() && self.paras.iter().all(|p| p.table.is_none()) {
            return;
        }
        let ranges = self.para_ranges();
        let mut used = std::collections::HashSet::new();
        for (p, r) in self.paras.iter_mut().zip(ranges) {
            if let Some(id) = p.table
                && (!self.text[r].contains(TABLE_ANCHOR) || !self.tables.contains_key(&id) || !used.insert(id))
            {
                p.table = None;
            }
        }
        self.tables.retain(|id, _| used.contains(id));
    }

    /// Check table invariants (part of [`Story::check`]).
    pub(crate) fn check_tables(&self) -> Result<(), String> {
        let ranges = self.para_ranges();
        let mut used = std::collections::HashSet::new();
        for (pi, p) in self.paras.iter().enumerate() {
            let Some(id) = p.table else { continue };
            let t = self.tables.get(&id).ok_or_else(|| format!("story {}: paragraph {pi} points at missing table {id}", self.id.0))?;
            if t.id != id {
                return Err(format!("story {}: table key {id} != id {}", self.id.0, t.id));
            }
            if self.text[ranges[pi].clone()].matches(TABLE_ANCHOR).count() != 1 {
                return Err(format!("story {}: table {id} paragraph must hold exactly one anchor", self.id.0));
            }
            if !used.insert(id) {
                return Err(format!("story {}: table {id} anchored twice", self.id.0));
            }
            t.check()?;
        }
        if used.len() != self.tables.len() {
            return Err(format!("story {}: {} tables but {} anchored", self.id.0, self.tables.len(), used.len()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story(t: &str) -> Story {
        Story::with_text(StoryId(1), t, ParaFormat::default())
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
}
