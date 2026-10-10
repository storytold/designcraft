//! Table layout: a table anchored in a story is laid out at its anchor paragraph.
//!
//! Column widths are scaled down to fit the text column; each cell's story is composed into its
//! cell (insets applied) with the regular composer; a row is as tall as its tallest single-row
//! cell (or its fixed height); rows that don't fit continue in the next column / frame, repeating
//! header (and footer) rows. Rows tied together by row spans never split.

use std::sync::Arc;

use designcraft_doc::{Align, Cell, CellStroke, Document, ItemId, ParaProps, RowHeightMode, Table, TextFrameOptions, VerticalJustification};
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
    /// Inset content box. Text baselines fit here; descenders may enter the bottom inset.
    /// Graphic content is clipped to this box.
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
    line_heights: Vec<f64>,
}

/// Cell text fits by its baseline, while its typographic/inline-object bounds must still
/// remain inside the physical cell. The bottom inset can contain ordinary descenders;
/// adding both the inset and the full descent unnecessarily grows every row.
struct CellLineExtent {
    baseline: f64,
    bottom: f64,
}

impl CellLineExtent {
    fn fit_height(&self, bottom_inset: f64) -> f64 {
        self.baseline.max(self.bottom - bottom_inset)
    }
}

/// Compose a cell story at width `w` (content space, top-left at the origin).
fn compose_cell(doc: &Document, cell: &Cell, w: f64, opts: &ComposeOptions, left_page: bool) -> CellLayout {
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
    let cs = crate::compose(doc, &cell.text, std::slice::from_ref(&spec), opts);
    let line_heights: Vec<f64> = cs
        .frames
        .first()
        .map(|f| {
            let mut extents: Vec<_> = f.lines.iter().map(|l| CellLineExtent { baseline: l.baseline, bottom: l.baseline + l.descent }).collect();
            // Line metrics include baseline shifts and super/subscript. Include the final
            // geometry of in-flow objects too, but leave custom anchors out of text flow.
            for object in &f.objects {
                if cell.text.objects.get(object.index).is_some_and(|o| !matches!(o.position, designcraft_doc::AnchorPosition::Custom { .. }))
                    && let Some(extent) = extents.get_mut(object.line)
                {
                    extent.bottom = extent.bottom.max(object.origin.y + object.size.1);
                }
            }
            extents.iter().map(|e| e.fit_height(cell.insets[2])).collect()
        })
        .unwrap_or_default();
    // Check all lines: a large descender or lowered inline object on an earlier line can
    // extend below the final baseline when leading is fixed.
    let content_h = line_heights.iter().copied().fold(0.0, f64::max);
    CellLayout { text: Arc::new(cs), content_h, line_heights }
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
            layouts.push(Some(compose_cell(doc, cell, w, opts, left_page)));
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
            let overset = space < -0.01 || lay.text.is_overset();
            let mut text = lay.text.clone();
            if overset && let Some(f) = text.frames.first() {
                // Use the same baseline/bounds test as auto-growth, and retain a prefix:
                // overset text cannot reappear after a line that did not fit.
                let keep = lay.line_heights.iter().take_while(|h| **h <= clip.height() + 0.01).count();
                let overset_at = f.lines.get(keep).map(|l| l.range.start);
                let mut t = (*text).clone();
                if let Some(frame) = t.frames.first_mut() {
                    frame.lines.truncate(keep);
                    frame.objects.retain(|o| o.line < keep);
                    frame.tables.retain(|table| table.line < keep);
                    frame.range.end = frame.lines.last().map_or(frame.range.start, |l| l.range.end);
                }
                if let Some(at) = overset_at {
                    t.overset_at = Some(at);
                }
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
        }
    }
    let strokes = fragment_strokes(table, rows, heights, colx, x0, bottom, owners);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeSource {
    Cell((usize, usize), usize),
    TablePattern,
}

/// Resolve a shared edge before dropping invisible strokes: a higher-priority None is
/// meaningful and must be allowed to suppress the other cell's visible edge.
fn boundary_stroke<'a>(
    table: &'a Table,
    before: Option<((usize, usize), &'a Cell)>,
    after: Option<((usize, usize), &'a Cell)>,
    before_side: usize,
    after_side: usize,
    pattern: Option<&'a CellStroke>,
) -> Option<(&'a CellStroke, i32, EdgeSource)> {
    let supplied = |cell: &Cell, side: usize| cell.stroke_defined[side] || cell.border_overrides[side] || cell.stroke_priorities[side] != 0;
    let rank =
        |cell: &Cell, side: usize| (cell.stroke_priorities[side], supplied(cell, side), supplied(cell, side) && cell.strokes[side].is_visible());
    let (owner, cell, side, outer) = match (before, after) {
        (Some((ao, a)), Some((bo, b))) => {
            // Table patterns supply missing interior edges; cell styles and local edits
            // retain precedence, including explicit zero/None. Perimeter rules never enter
            // this branch.
            if !supplied(a, before_side)
                && !supplied(b, after_side)
                && let Some(stroke) = pattern
            {
                return Some((stroke, 0, EdgeSource::TablePattern));
            }
            // Adobe IDML specification, Cell attributes (pp. 237-240): higher numeric
            // edge priority means higher drawing priority. Equal-priority conflicts are
            // unspecified: prefer supplied formatting, then a visible supplied rule.
            // Metadata-free native / legacy cells retain lower/right top/left ownership,
            // including explicit None already stored there by older native editing code.
            if rank(a, before_side) > rank(b, after_side) { (ao, a, before_side, false) } else { (bo, b, after_side, false) }
        }
        (Some((owner, a)), None) => (owner, a, before_side, true),
        (None, Some((owner, b))) => (owner, b, after_side, true),
        (None, None) => return None,
    };
    if outer && !cell.border_overrides[side] {
        Some((table.options.border_for(side), 0, EdgeSource::Cell(owner, side)))
    } else {
        Some((&cell.strokes[side], cell.stroke_priorities[side], EdgeSource::Cell(owner, side)))
    }
}

/// Keep an unchanged winning cell edge continuous across different losing neighbors. In
/// particular, splitting a merged dashed edge would restart its dash phase at every cell.
fn push_boundary_stroke(
    strokes: &mut Vec<(StrokeSeg, i32)>,
    previous: &mut Option<EdgeSource>,
    a: Point,
    b: Point,
    edge: Option<(&CellStroke, i32, EdgeSource)>,
) {
    let Some((stroke, priority, source)) = edge.filter(|(s, _, _)| s.is_visible()) else {
        *previous = None;
        return;
    };
    if *previous == Some(source)
        && let Some((last, last_priority)) = strokes.last_mut()
        && last.b == a
        && last.stroke == *stroke
        && *last_priority == priority
    {
        last.b = b;
    } else {
        strokes.push((StrokeSeg { a, b, stroke: stroke.clone() }, priority));
    }
    *previous = Some(source);
}

/// Visit each displayed grid boundary once, including boundaries between repeated headers /
/// footers and nonadjacent source body rows. Interior lines within a merged owner are omitted.
fn fragment_strokes(
    table: &Table,
    rows: &[(usize, f64, bool)],
    heights: &[f64],
    colx: &[f64],
    x0: f64,
    bottom: f64,
    owners: &[(usize, usize)],
) -> Vec<StrokeSeg> {
    let nc = table.ncols();
    let right = x0 + colx.last().copied().unwrap_or(0.0);
    let rtl = table.options.direction == designcraft_doc::TextDirection::RightToLeft;
    let owner = |r: usize, c: usize| {
        let &(r, c) = owners.get(r.checked_mul(nc)?.checked_add(c)?)?;
        Some(((r, c), table.cell(r, c)?))
    };
    let x = |c: usize| colx.get(c).map(|&offset| if rtl { right - offset } else { x0 + offset });
    // Uniform active groups do not depend on which shared edge starts the pattern. Keep
    // nonuniform and skipped patterns intact in the model until their phase is verified.
    let row_stroke = table.options.row_strokes.as_ref().filter(|p| p.skip_first == 0 && p.skip_last == 0).and_then(|p| p.uniform_stroke());
    let column_stroke = table.options.column_strokes.as_ref().filter(|p| p.skip_first == 0 && p.skip_last == 0).and_then(|p| p.uniform_stroke());
    let body = table.body_rows();
    let mut strokes = Vec::new();
    for boundary in 0..=rows.len() {
        let before = boundary.checked_sub(1).and_then(|r| rows.get(r));
        let after = rows.get(boundary);
        let y = after.map_or(bottom, |row| row.1);
        let pattern =
            if before.is_some_and(|row| body.contains(&row.0)) && after.is_some_and(|row| body.contains(&row.0)) { row_stroke } else { None };
        let mut previous = None;
        for physical_col in 0..nc {
            let c = if rtl { nc - physical_col - 1 } else { physical_col };
            let above = before.and_then(|row| owner(row.0, c));
            let below = after.and_then(|row| owner(row.0, c));
            let pair = (above.map(|v| v.0), below.map(|v| v.0));
            if pair.0 == pair.1 {
                previous = None;
                continue;
            }
            let (Some(a), Some(b)) = (x(c), x(c + 1)) else { continue };
            let edge = boundary_stroke(table, above, below, 2, 0, pattern);
            push_boundary_stroke(&mut strokes, &mut previous, Point::new(a.min(b), y), Point::new(a.max(b), y), edge);
        }
    }
    for boundary in 0..=nc {
        let Some(x) = x(boundary) else { continue };
        let mut previous = None;
        for &(r, y, _) in rows {
            let before = boundary.checked_sub(1).and_then(|c| owner(r, c));
            let after = (boundary < nc).then(|| owner(r, boundary)).flatten();
            let (left, right) = if rtl { (after, before) } else { (before, after) };
            let pair = (left.map(|v| v.0), right.map(|v| v.0));
            if pair.0 == pair.1 {
                previous = None;
                continue;
            }
            let Some(&height) = heights.get(r) else { continue };
            let edge = boundary_stroke(table, left, right, 3, 1, if body.contains(&r) { column_stroke } else { None });
            push_boundary_stroke(&mut strokes, &mut previous, Point::new(x, y), Point::new(x, y + height), edge);
        }
    }
    // Shared edges have a single winner; retain the documented priority at crossings too.
    strokes.sort_by_key(|(_, priority)| *priority);
    strokes.into_iter().map(|(stroke, _)| stroke).collect()
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
mod height_tests {
    use super::*;
    use designcraft_doc::{AnchorPosition, AnchoredObject, CellRange, LayerId, ParaFormat, Position, SpreadRef, build::NewDocument};

    fn text_cell(text: &str, bottom: f64) -> Cell {
        let mut cell = Cell::with_text(text, ParaFormat::default());
        cell.insets = [8.5, 4.0, bottom, 4.0];
        cell.text.format_chars(0..cell.text.len(), |f| {
            f.over.font_family = Some("Source Sans 3".into());
            f.over.font_style = Some("Regular".into());
            f.over.size = Some(11.0);
        });
        cell
    }

    fn measure(cell: &Cell, width: f64) -> CellLayout {
        compose_cell(&Document::new(&NewDocument::default()), cell, width, &ComposeOptions::default(), false)
    }

    fn compose_table(table: Table, height: f64) -> ComposedStory {
        let mut doc = Document::new(&NewDocument::default());
        let layer = doc.default_layer();
        let (_, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, height), layer, "", ParaFormat::default()).unwrap();
        doc.story_mut(sid).unwrap().insert_table(0, table);
        crate::compose_story(&doc, sid, &ComposeOptions::default())
    }

    fn single_table(cell: Cell) -> Table {
        let mut table = Table::new(77, 1, 1, 0, 0, 100.0);
        table.cells[0] = cell;
        table
    }

    fn object(position: AnchorPosition) -> AnchoredObject {
        let path = designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 12.0, 30.0));
        AnchoredObject::new(designcraft_doc::Item::new(ItemId(0), LayerId(0), designcraft_doc::Shape::Rectangle, path), position)
    }

    #[test]
    fn cell_bottom_inset_contains_descenders_without_growing_every_row() {
        let cell = text_cell("gyp", 8.5);
        let layout = measure(&cell, 92.0);
        let line = &layout.text.frames[0].lines[0];
        assert!(line.descent > 0.0 && line.descent < cell.insets[2]);
        let expected = cell.insets[0] + line.baseline + cell.insets[2];
        let composed = compose_table(single_table(cell), 200.0);
        let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
        assert!((placed.rect.height() - expected).abs() < 1e-6);
        assert!(!placed.overset);
        assert_eq!(placed.text.frames[0].lines.len(), 1);
        assert!(placed.origin.y + line.baseline + line.descent <= placed.rect.y1);
        // The shared frame composer keeps its full typographic height for non-cell users.
        assert!((layout.text.frames[0].content_height - line.baseline - line.descent).abs() < 1e-6);
    }

    #[test]
    fn baseline_sized_rows_fit_the_frame_without_losing_the_final_rows() {
        let cell = text_cell("gyp", 8.5);
        let layout = measure(&cell, 92.0);
        let row_height = 17.0 + layout.text.frames[0].lines[0].baseline;
        let mut table = Table::new(77, 16, 1, 1, 0, 100.0);
        table.cells.fill(cell);
        let composed = compose_table(table, 17.0 * row_height + 25.0);
        assert!(!composed.is_overset());
        let fragment = &composed.frames[0].tables[0];
        assert_eq!(fragment.cells.len(), 17);
        assert!(fragment.cell(16, 0).is_some());
        assert!((fragment.rect.height() - 17.0 * row_height).abs() < 1e-6);
    }

    #[test]
    fn multiline_cells_keep_leading_and_paragraph_spacing() {
        let mut cell = text_cell("gyp\njqp", 8.5);
        cell.text.format_paras(0..3, |p| p.para.space_after = Some(6.0));
        let layout = measure(&cell, 92.0);
        let lines = &layout.text.frames[0].lines;
        assert_eq!(lines.len(), 2);
        assert!((lines[1].baseline - lines[0].baseline - lines[1].leading - 6.0).abs() < 1e-6);
        let expected = 17.0 + lines[1].baseline;
        let composed = compose_table(single_table(cell), 200.0);
        let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
        assert_eq!(placed.text.frames[0].lines.len(), 2);
        assert!((placed.rect.height() - expected).abs() < 1e-6);
        let wrapped = measure(&text_cell("gyp gyp gyp gyp gyp gyp gyp gyp", 8.5), 30.0);
        assert!(wrapped.text.frames[0].lines.len() > 2);
        assert!((wrapped.content_h - wrapped.text.frames[0].lines.last().unwrap().baseline).abs() < 1e-6);
    }

    #[test]
    fn small_insets_still_contain_descenders_and_shifted_text() {
        for bottom in [0.0, 1.0] {
            for (shift, position) in [(0.0, Position::Normal), (-15.0, Position::Normal), (0.0, Position::Superscript), (0.0, Position::Subscript)] {
                let mut cell = text_cell("gyp", bottom);
                cell.text.format_chars(0..3, |f| {
                    f.over.baseline_shift = Some(shift);
                    f.over.position = Some(position);
                });
                let layout = measure(&cell, 92.0);
                let line = &layout.text.frames[0].lines[0];
                let expected = cell.insets[0] + line.baseline + bottom.max(line.descent);
                let composed = compose_table(single_table(cell), 200.0);
                let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
                assert!((placed.rect.height() - expected).abs() < 1e-6);
                assert!(placed.origin.y + line.baseline + line.descent <= placed.rect.y1 + 1e-6);
                assert!(!placed.overset);
            }
        }
    }

    #[test]
    fn exact_height_uses_the_same_fit_rule_and_keeps_only_a_prefix() {
        let cell = text_cell("gyp\njqp", 8.5);
        let layout = measure(&cell, 92.0);
        let first_height = 17.0 + layout.text.frames[0].lines[0].baseline;
        let full_height = 17.0 + layout.text.frames[0].lines[1].baseline;
        for (height, count, overset) in [(full_height, 2, false), (first_height, 1, true), (first_height - 1.0, 0, true)] {
            let mut table = single_table(cell.clone());
            table.rows[0].mode = RowHeightMode::Exactly;
            table.rows[0].height = height;
            let composed = compose_table(table, 200.0);
            let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
            assert!((placed.rect.height() - height).abs() < 1e-6);
            assert_eq!(placed.overset, overset);
            assert_eq!(placed.text.frames[0].lines.len(), count);
            if overset {
                assert_eq!(placed.text.overset_at, Some(layout.text.frames[0].lines[count].range.start));
            }
        }
    }

    #[test]
    fn exact_height_cannot_fit_a_descender_outside_the_physical_cell() {
        let cell = text_cell("gyp", 0.0);
        let layout = measure(&cell, 92.0);
        let line = &layout.text.frames[0].lines[0];
        let mut table = single_table(cell);
        table.rows[0].mode = RowHeightMode::Exactly;
        table.rows[0].height = 8.5 + line.baseline + line.descent / 2.0;
        let composed = compose_table(table, 200.0);
        let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
        assert!(placed.overset);
        assert!(placed.text.frames[0].lines.is_empty());
    }

    #[test]
    fn earlier_lowered_line_controls_height_and_cannot_be_skipped_on_overset() {
        let mut cell = text_cell("gyp\njqp", 8.5);
        cell.text.format_chars(0..cell.text.len(), |f| f.over.leading = Some(designcraft_doc::Leading::Points(6.0)));
        cell.text.format_chars(0..3, |f| f.over.baseline_shift = Some(-30.0));
        let layout = measure(&cell, 92.0);
        let lines = &layout.text.frames[0].lines;
        assert_eq!(lines.len(), 2);
        let deep_bottom = lines[0].baseline + lines[0].descent;
        assert!(deep_bottom > lines[1].baseline + lines[1].descent + 8.5);
        let composed = compose_table(single_table(cell.clone()), 200.0);
        let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
        assert!((placed.rect.height() - 8.5 - deep_bottom).abs() < 1e-6);
        assert!(!placed.overset);

        let mut table = single_table(cell);
        table.rows[0].mode = RowHeightMode::Exactly;
        table.rows[0].height = 17.0 + lines[1].baseline;
        let composed = compose_table(table, 200.0);
        let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
        assert!(placed.overset);
        assert!(placed.text.frames[0].lines.is_empty(), "a later fitting line cannot reappear after the first line oversets");
    }

    #[test]
    fn empty_and_trailing_empty_paragraphs_keep_their_baseline_height() {
        for text in ["", "gyp\n", "gyp\n\n"] {
            let cell = text_cell(text, 8.5);
            let layout = measure(&cell, 92.0);
            let lines = &layout.text.frames[0].lines;
            assert_eq!(lines.len(), text.matches('\n').count() + 1);
            assert!(lines.last().unwrap().glyphs.is_empty());
            let expected = 17.0 + lines.last().unwrap().baseline;
            let composed = compose_table(single_table(cell), 200.0);
            let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
            assert!((placed.rect.height() - expected).abs() < 1e-6);
            assert_eq!(placed.text.frames[0].lines.len(), lines.len());
            assert!(!placed.overset);
        }
    }

    #[test]
    fn vertical_alignment_uses_baseline_fit_without_losing_descenders() {
        for bottom in [0.0, 8.5] {
            for (vj, expected_shift) in
                [(VerticalJustification::Top, 0.0), (VerticalJustification::Center, 10.0), (VerticalJustification::Bottom, 20.0)]
            {
                let mut cell = text_cell("gyp", bottom);
                cell.vj = vj;
                let layout = measure(&cell, 92.0);
                let required = cell.insets[0] + layout.content_h + bottom;
                let mut table = single_table(cell);
                table.rows[0].mode = RowHeightMode::Exactly;
                table.rows[0].height = required + 20.0;
                let composed = compose_table(table, 200.0);
                let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
                let line = &placed.text.frames[0].lines[0];
                assert!((placed.origin.y - placed.clip.y0 - expected_shift).abs() < 1e-6);
                assert!(placed.origin.y + line.baseline + line.descent <= placed.rect.y1 + 1e-6);
                assert!(!placed.overset);
            }
        }
    }

    #[test]
    fn row_spans_grow_to_the_same_baseline_and_physical_bounds() {
        let cell = text_cell("gyp\njqp\ngyp", 8.5);
        let layout = measure(&cell, 92.0);
        let expected = 17.0 + layout.text.frames[0].lines.last().unwrap().baseline;
        let mut table = Table::new(77, 2, 1, 0, 0, 100.0);
        table.merge(CellRange::new(0, 0, 1, 0)).unwrap();
        table.rows[0].mode = RowHeightMode::Exactly;
        table.rows[0].height = 3.0;
        table.cells[0] = Cell { row_span: 2, ..cell };
        let composed = compose_table(table, 200.0);
        let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
        assert!((placed.rect.height() - expected).abs() < 1e-6);
        assert_eq!(placed.text.frames[0].lines.len(), 3);
        assert!(!placed.overset);
    }

    #[test]
    fn lowered_inline_objects_are_contained_but_custom_anchors_stay_out_of_flow() {
        let mut cell = text_cell("gyp", 8.5);
        cell.text.insert_object(0, object(AnchorPosition::Inline { y_offset: -40.0 }));
        let composed = compose_table(single_table(cell), 200.0);
        let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
        let object = &placed.text.frames[0].objects[0];
        assert!((placed.origin.y + object.origin.y + object.size.1 - placed.rect.y1).abs() < 1e-6);
        assert!(!placed.overset);

        let mut cell = text_cell("gyp", 8.5);
        let original = measure(&cell, 92.0).content_h;
        cell.text.insert_object(
            0,
            self::object(AnchorPosition::Custom {
                x_relative: Default::default(),
                y_relative: Default::default(),
                x_offset: 0.0,
                y_offset: 100.0,
                object_point: 0,
                ref_point: 0,
                keep_within_column: false,
            }),
        );
        assert!((measure(&cell, 92.0).content_h - original).abs() < 1e-6);
    }

    #[test]
    fn overset_lines_cannot_leave_visible_objects_or_nested_tables_behind() {
        let plain = text_cell("gyp", 8.5);
        let first_height = 17.0 + measure(&plain, 92.0).text.frames[0].lines[0].baseline;
        for nested in [false, true] {
            let mut cell = text_cell("gyp\nHidden", 8.5);
            if nested {
                cell.text.insert_table(4, single_table(text_cell("nested", 8.5)));
            } else {
                cell.text.insert_object(4, object(AnchorPosition::Inline { y_offset: 0.0 }));
            }
            let mut table = single_table(cell);
            table.rows[0].mode = RowHeightMode::Exactly;
            table.rows[0].height = first_height;
            let composed = compose_table(table, 200.0);
            let placed = composed.frames[0].tables[0].cell(0, 0).unwrap();
            assert!(placed.overset);
            assert_eq!(placed.text.frames[0].lines.len(), 1);
            assert!(placed.text.frames[0].objects.is_empty());
            assert!(placed.text.frames[0].tables.is_empty());
        }
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
        assert!(
            !fragment.strokes.iter().any(|s| s.a == Point::new(hidden_left.x0, hidden_left.y0) && s.b == Point::new(hidden_left.x0, hidden_left.y1))
        );
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
        let (second, _) =
            doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 200.0, 120.0, 200.0 + height), layer, "", ParaFormat::default()).unwrap();
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
}
