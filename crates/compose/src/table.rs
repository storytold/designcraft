//! Table layout: a table anchored in a story is laid out at its anchor paragraph.
//!
//! Column widths are scaled down to fit the text column; each cell's story is composed into its
//! cell (insets applied) with the regular composer; a row is as tall as its tallest single-row
//! cell (or its fixed height); rows that don't fit continue in the next column / frame, repeating
//! header (and footer) rows. Rows tied together by row spans never split.

use std::sync::Arc;

use designcraft_doc::{Align, CellStroke, Document, ItemId, ParaProps, RowHeightMode, Story, Table, TextFrameOptions, VerticalJustification};
use designcraft_geom::{Point, Rect};

use crate::{ComposeOptions, ComposedStory, Cursor, FrameSpec, Line};

/// A cell placed in a frame (frame inner space).
#[derive(Clone, Debug)]
pub struct PlacedCell {
    pub row: usize,
    pub col: usize,
    pub rect: Rect,
    /// Fill swatch and tint (cell fill or alternating fill).
    pub fill: Option<(String, f32)>,
    /// Where the cell's composed text (its frame 0, in content space) sits in the frame.
    pub origin: Point,
    pub text: Arc<ComposedStory>,
    /// The cell's story text (exporters map glyphs back to text).
    pub source: Arc<str>,
    /// Content height available in the cell (for clipping).
    pub clip: Rect,
    pub overset: bool,
    /// A header/footer row repeated in a continuation fragment.
    pub repeated: bool,
    /// Graphic cell content (drawn at `clip`'s top left, clipped to it).
    pub graphic: Option<designcraft_doc::Graphic>,
}

/// One cell edge (or the table border) to stroke.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeSeg {
    pub a: Point,
    pub b: Point,
    pub stroke: CellStroke,
}

/// The part of a table shown in one text column.
#[derive(Clone, Debug)]
pub struct TableFrag {
    pub table: u64,
    /// Story byte of the anchor character.
    pub anchor: usize,
    pub column: u32,
    pub rect: Rect,
    pub cells: Vec<PlacedCell>,
    pub strokes: Vec<StrokeSeg>,
    /// Index of the fragment's (glyph-less) line in the frame's lines.
    pub line: usize,
    pub first: bool,
    pub last: bool,
}

impl TableFrag {
    pub(crate) fn shift(&mut self, dy: f64) {
        if dy == 0.0 {
            return;
        }
        let v = designcraft_geom::Vec2::new(0.0, dy);
        self.rect = self.rect + v;
        for c in &mut self.cells {
            c.rect = c.rect + v;
            c.clip = c.clip + v;
            c.origin += v;
        }
        for s in &mut self.strokes {
            s.a += v;
            s.b += v;
        }
    }

    /// The placed cell owning grid position (row, col), if it is in this fragment.
    pub fn cell(&self, row: usize, col: usize) -> Option<&PlacedCell> {
        self.cells.iter().find(|c| c.row == row && c.col == col)
    }
}

struct CellLayout {
    text: Arc<ComposedStory>,
    content_h: f64,
}

/// Compose a cell story at width `w` (content space, top-left at the origin).
fn compose_cell(doc: &Document, story: &Story, w: f64, opts: &ComposeOptions, left_page: bool) -> CellLayout {
    let spec = FrameSpec {
        id: ItemId(0),
        area: Rect::new(0.0, 0.0, w.max(1.0), 1.0e6),
        opts: TextFrameOptions::default(),
        vertical: false,
        exclusions: vec![],
        page_name: opts.page_name.clone(),
        page: opts.page,
        grid: None,
        left_page,
        page_rect: None,
    };
    let cs = crate::compose(doc, story, std::slice::from_ref(&spec), opts);
    let content_h = cs.frames.first().map(|f| if f.lines.is_empty() { 0.0 } else { f.content_height }).unwrap_or(0.0);
    CellLayout { text: Arc::new(cs), content_h }
}

/// Lay out `table` (anchored at byte `anchor` of paragraph `pi`) from the cursor. Returns false
/// when it doesn't fit the remaining frames (overset).
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_table(
    doc: &Document,
    table: &Table,
    anchor: usize,
    pi: usize,
    pp: &ParaProps,
    frames: &[FrameSpec],
    cols: &[Vec<Rect>],
    cur: &mut Cursor,
    out: &mut ComposedStory,
    opts: &ComposeOptions,
) -> bool {
    if cur.fi >= frames.len() || table.nrows() == 0 || table.ncols() == 0 {
        return false;
    }
    let col_rect = |cur: &Cursor| cur.clip(cols[cur.fi][cur.col.min(cols[cur.fi].len() - 1)]);
    let mut col = col_rect(cur);
    // Column widths: scaled to fit the measure.
    let avail = (col.width() - pp.left_indent - pp.right_indent).max(1.0);
    let natural = table.width();
    let k = if natural > avail && natural > 0.0 { avail / natural } else { 1.0 };
    let widths: Vec<f64> = table.columns.iter().map(|c| c.width * k).collect();
    let total_w: f64 = widths.iter().sum();
    let x_off = pp.left_indent
        + match pp.align {
            Align::Center | Align::CenterJustified => (avail - total_w) / 2.0,
            Align::Right | Align::RightJustified => avail - total_w,
            _ => 0.0,
        }
        .max(0.0);
    let mut colx = vec![0.0; widths.len() + 1];
    for (i, w) in widths.iter().enumerate() {
        colx[i + 1] = colx[i] + w;
    }
    let (nr, nc) = (table.nrows(), table.ncols());
    let owners = table.owners();
    let is_owner = |r: usize, c: usize| owners[r * nc + c] == (r, c);
    let left_page = frames[cur.fi].left_page;
    // Compose every owner cell.
    let mut layouts: Vec<Option<CellLayout>> = Vec::with_capacity(nr * nc);
    for r in 0..nr {
        for c in 0..nc {
            if !is_owner(r, c) {
                layouts.push(None);
                continue;
            }
            let cell = &table.cells[r * nc + c];
            let cs = (cell.col_span.max(1) as usize).min(nc - c);
            let w = colx[c + cs] - colx[c] - cell.insets[1] - cell.insets[3];
            layouts.push(Some(compose_cell(doc, &cell.text, w, opts, left_page)));
        }
    }
    // Row heights.
    let need = |r: usize, c: usize| -> f64 {
        let cell = &table.cells[r * nc + c];
        layouts[r * nc + c].as_ref().map_or(0.0, |l| l.content_h) + cell.insets[0] + cell.insets[2]
    };
    let mut heights: Vec<f64> = table
        .rows
        .iter()
        .enumerate()
        .map(|(r, row)| match row.mode {
            RowHeightMode::Exactly => row.height.max(0.0),
            RowHeightMode::AtLeast => {
                let content = (0..nc).filter(|&c| is_owner(r, c) && table.cells[r * nc + c].row_span <= 1).map(|c| need(r, c)).fold(0.0, f64::max);
                content.max(row.height)
            }
        })
        .collect();
    // Row-spanning cells grow the last row they span (unless it is fixed).
    for r in 0..nr {
        for c in 0..nc {
            let cell = &table.cells[r * nc + c];
            if !is_owner(r, c) || cell.row_span <= 1 {
                continue;
            }
            let end = (r + cell.row_span as usize).min(nr);
            let have: f64 = heights[r..end].iter().sum();
            let n = need(r, c);
            if n > have
                && let Some(i) = (r..end).rev().find(|&i| table.rows[i].mode == RowHeightMode::AtLeast)
            {
                heights[i] += n - have;
            }
        }
    }
    // Row groups that must stay together: end (exclusive) of the group starting at each row.
    let mut group_end: Vec<usize> = (0..nr).map(|r| r + 1).collect();
    {
        let mut r = 0;
        while r < nr {
            let mut e = r + 1;
            let mut i = r;
            while i < e {
                for c in 0..nc {
                    if is_owner(i, c) {
                        e = e.max((i + table.cells[i * nc + c].row_span.max(1) as usize).min(nr));
                    }
                }
                i += 1;
            }
            for g in group_end.iter_mut().take(e).skip(r) {
                *g = e;
            }
            r = e;
        }
    }
    let header = table.header_rows();
    let footer = table.footer_rows();
    let body_end = nr - footer;
    let sum = |a: usize, b: usize| heights[a..b].iter().sum::<f64>();
    let footer_h = sum(body_end, nr);
    let o = &table.options;
    let mut y = match cur.last_baseline {
        Some(b) => b + cur.last_descent + cur.pending + o.space_before,
        None => col.y0,
    };
    let mut frag_rows: Vec<(usize, f64, bool)> = Vec::new(); // (row, y, repeated)
    let mut first_frag = true;
    let mut body_placed = 0usize;
    let mut top = y;
    let place_rows = |rows: std::ops::Range<usize>, y: &mut f64, repeated: bool, frag_rows: &mut Vec<(usize, f64, bool)>| {
        for r in rows {
            frag_rows.push((r, *y, repeated));
            *y += heights[r];
        }
    };
    place_rows(0..header, &mut y, false, &mut frag_rows);
    let mut r = header;
    loop {
        if r >= body_end {
            break;
        }
        let ge = group_end[r].min(body_end).max(r + 1);
        let gh = sum(r, ge);
        let at_col_top = cur.last_baseline.is_none() && (top - col.y0).abs() < 1e-6;
        if y + gh + footer_h <= col.y1 + 0.01 || (body_placed == 0 && at_col_top) {
            if y + gh + footer_h > col.y1 + 0.01 && body_placed == 0 && at_col_top && y + gh > col.y1 + 0.01 {
                // A row group taller than an empty column can never fit.
                return false;
            }
            place_rows(r..ge, &mut y, false, &mut frag_rows);
            body_placed += 1;
            r = ge;
            continue;
        }
        // Continue in the next column / frame.
        if body_placed > 0 {
            if o.repeat_footer {
                place_rows(body_end..nr, &mut y, true, &mut frag_rows);
            }
            emit(table, anchor, pi, &frag_rows, &heights, &colx, col.x0 + x_off, top, y, first_frag, false, &layouts, &owners, cur, col, out);
            first_frag = false;
        }
        let was_first = first_frag;
        cur.next_column(cols);
        if cur.fi >= frames.len() {
            return false;
        }
        col = col_rect(cur);
        y = col.y0;
        top = y;
        frag_rows.clear();
        body_placed = 0;
        // Headers: the first fragment's header moves along; later ones repeat it.
        if was_first {
            place_rows(0..header, &mut y, false, &mut frag_rows);
        } else if o.repeat_header {
            place_rows(0..header, &mut y, true, &mut frag_rows);
        }
    }
    place_rows(body_end..nr, &mut y, false, &mut frag_rows);
    emit(table, anchor, pi, &frag_rows, &heights, &colx, col.x0 + x_off, top, y, first_frag, true, &layouts, &owners, cur, col, out);
    cur.last_baseline = Some(y);
    cur.last_descent = 0.0;
    cur.pending = o.space_after;
    true
}

/// Build one fragment from its rows and push it (with its glyph-less line) to the current frame.
#[allow(clippy::too_many_arguments)]
fn emit(
    table: &Table,
    anchor: usize,
    pi: usize,
    rows: &[(usize, f64, bool)],
    heights: &[f64],
    colx: &[f64],
    x0: f64,
    top: f64,
    bottom: f64,
    first: bool,
    last: bool,
    layouts: &[Option<CellLayout>],
    owners: &[(usize, usize)],
    cur: &Cursor,
    col: Rect,
    out: &mut ComposedStory,
) {
    let nc = table.ncols();
    let ry = |r: usize| rows.iter().find(|x| x.0 == r).map(|x| x.1);
    let header = table.header_rows();
    let body = table.body_rows();
    let nbody = body.len();
    let mut cells = Vec::new();
    let mut strokes = Vec::new();
    let o = &table.options;
    let right = x0 + colx[nc];
    for &(r, y, repeated) in rows {
        for c in 0..nc {
            if owners[r * nc + c] != (r, c) {
                continue;
            }
            let cell = &table.cells[r * nc + c];
            let cs = (cell.col_span.max(1) as usize).min(nc - c);
            let rs = (cell.row_span.max(1) as usize).min(table.nrows() - r);
            let last_row = r + rs - 1;
            let y1 = ry(last_row).map_or(y + heights[r], |ly| ly + heights[last_row]);
            let rect = if table.options.direction == designcraft_doc::TextDirection::RightToLeft {
                Rect::new(right - colx[c + cs], y, right - colx[c], y1)
            } else {
                Rect::new(x0 + colx[c], y, x0 + colx[c + cs], y1)
            };
            let fill = if cell.has_fill() {
                Some((cell.fill.clone(), cell.fill_tint))
            } else if body.contains(&r) {
                let i = r - header;
                o.alt_rows
                    .as_ref()
                    .and_then(|a| a.fill_for(i, nbody))
                    .or_else(|| o.alt_cols.as_ref().and_then(|a| a.fill_for(c, nc)))
                    .map(|(s, t)| (s.to_string(), t))
            } else {
                None
            };
            let Some(lay) = layouts.get(r * nc + c).and_then(Option::as_ref) else { continue };
            let clip = Rect::new(rect.x0 + cell.insets[1], rect.y0 + cell.insets[0], rect.x1 - cell.insets[3], rect.y1 - cell.insets[2]);
            let space = clip.height() - lay.content_h;
            let dy = match cell.vj {
                VerticalJustification::Center if space > 0.0 => space / 2.0,
                VerticalJustification::Bottom if space > 0.0 => space,
                _ => 0.0,
            };
            let overset = space < -0.01;
            let mut text = lay.text.clone();
            if overset && let Some(f) = text.frames.first() {
                // Drop the lines that don't fit the cell.
                let keep: Vec<Line> = f.lines.iter().filter(|l| l.baseline + l.descent <= clip.height() + 0.01).cloned().collect();
                let mut t = (*text).clone();
                t.frames[0].lines = keep;
                text = Arc::new(t);
            }
            cells.push(PlacedCell {
                row: r,
                col: c,
                rect,
                fill,
                origin: Point::new(clip.x0, clip.y0 + dy),
                text,
                source: Arc::from(cell.text.text.as_str()),
                clip,
                overset,
                repeated,
                graphic: cell.graphic.clone(),
            });
            // Edges: every cell draws its top and left; the fragment's bottom/right cells their bottom/right.
            // The table border replaces outer edges unless the cell explicitly overrides it.
            let at_top = (y - top).abs() < 1e-6;
            let at_bottom = (y1 - bottom).abs() < 1e-6;
            let edge = |i: usize, outer: bool| {
                if outer && !cell.border_overrides[i] { o.border.clone() } else { cell.strokes[i].clone() }
            };
            let mut push = |a: Point, b: Point, s: CellStroke| {
                if s.is_visible() {
                    strokes.push(StrokeSeg { a, b, stroke: s });
                }
            };
            push(Point::new(rect.x0, rect.y0), Point::new(rect.x1, rect.y0), edge(0, at_top));
            push(Point::new(rect.x0, rect.y0), Point::new(rect.x0, rect.y1), edge(1, (rect.x0 - x0).abs() < 1e-6));
            if at_bottom {
                push(Point::new(rect.x0, rect.y1), Point::new(rect.x1, rect.y1), edge(2, true));
            }
            if (rect.x1 - right).abs() < 1e-6 {
                push(Point::new(rect.x1, rect.y0), Point::new(rect.x1, rect.y1), edge(3, true));
            }
        }
    }
    let ft = &mut out.frames[cur.fi];
    let line = ft.lines.len();
    let h = bottom - top;
    ft.lines.push(Line {
        column: cur.col as u32,
        baseline: bottom,
        x0: col.x0,
        x1: col.x1,
        ascent: h,
        descent: 0.0,
        leading: h,
        range: anchor..anchor + designcraft_doc::TABLE_ANCHOR.len_utf8(),
        para: pi,
        glyphs: vec![],
        hyphenated: false,
        first_in_para: first,
        last_in_para: last,
        end_x: right,
        spacing: 1.0,
        hj: 0,
        keep_violation: false,
    });
    ft.tables.push(TableFrag {
        table: table.id,
        anchor,
        column: cur.col as u32,
        rect: Rect::new(x0, top, right, bottom),
        cells,
        strokes,
        line,
        first,
        last,
    });
}

// ---------- hit testing and carets in cells ----------

/// The cell under `p` (frame inner space) in frame `fi`: (table, row, col, story byte in the cell).
pub fn hit_cell(cs: &ComposedStory, fi: usize, p: Point) -> Option<(u64, usize, usize, usize)> {
    let ft = cs.frames.get(fi)?;
    for t in &ft.tables {
        if !t.rect.contains(p) {
            continue;
        }
        for c in &t.cells {
            if c.rect.contains(p) {
                let local = Point::new(p.x - c.origin.x, p.y - c.origin.y);
                let b = crate::hit(&c.text, 0, local).unwrap_or(0);
                return Some((t.table, c.row, c.col, b));
            }
        }
    }
    None
}

/// Frame index and placed cell for a cell address (the first, non-repeated occurrence).
pub fn find_cell(cs: &ComposedStory, table: u64, row: usize, col: usize) -> Option<(usize, &TableFrag, &PlacedCell)> {
    let mut found = None;
    for (fi, ft) in cs.frames.iter().enumerate() {
        for t in ft.tables.iter().filter(|t| t.table == table) {
            if let Some(c) = t.cell(row, col) {
                if !c.repeated {
                    return Some((fi, t, c));
                }
                found = found.or(Some((fi, t, c)));
            }
        }
    }
    found
}

/// Caret geometry for byte `pos` of a cell's story, in frame inner space:
/// (frame index, x, baseline, ascent, descent).
pub fn cell_caret(cs: &ComposedStory, table: u64, row: usize, col: usize, pos: usize) -> Option<(usize, f64, f64, f64, f64)> {
    let (fi, _, c) = find_cell(cs, table, row, col)?;
    match crate::caret(&c.text, pos) {
        Some((_, x, bl, a, d)) => Some((fi, x + c.origin.x, bl + c.origin.y, a, d)),
        None => Some((fi, c.origin.x, c.origin.y + 10.0, 9.0, 3.0)),
    }
}

#[cfg(test)]
mod border_tests {
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
}
