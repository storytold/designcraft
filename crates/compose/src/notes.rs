//! Footnote layout: numbering, room at the bottom of columns, and placement.
//!
//! When a line holding footnote references is set, each referenced footnote is composed at the
//! column width and the column's text area shrinks by its height (plus the minimum space before
//! the first footnote and the space between footnotes). A line that would collide moves to the
//! next column with its footnotes. After the story is laid out the footnotes of each column are
//! stacked against the column bottom, with the separator rule above the first one.
//!
//! The footnotes of a column read as one text: the first footnote's first baseline sits at the
//! First Baseline offset below the top of the footnote area, and each later footnote's first
//! baseline sits its first line's leading (plus Space Between Footnotes) below the previous
//! footnote's last baseline. Only the last footnote's descent counts toward the area's height.
//!
//! With Span Footnotes Across Columns the notes of a frame share one area across all its columns,
//! composed at the frame's text width: every column of the frame makes room for it.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use designcraft_doc::notes::NoteRestart;
use designcraft_doc::{Document, FOOTNOTE_REF, ItemId, Story, TextFrameOptions};
use designcraft_geom::{Point, Rect};

use crate::shape::{Glyph, SubstCtx};
use crate::{ComposeOptions, ComposedStory, Deco, FrameSpec, PlacedNote};

/// A footnote assigned to a column.
pub(crate) struct Placed {
    k: usize,
    fi: usize,
    col: usize,
    m: NoteMetrics,
    parts: Arc<Vec<NotePart>>,
}

/// One footnote of a placed footnote's block: the footnote itself, then the footnotes nested in
/// it in reading order, each printed under the one before as if after a forced line break.
#[derive(Clone, Debug)]
pub(crate) struct NotePart {
    id: u64,
    label: String,
    text: Arc<ComposedStory>,
    source: Arc<str>,
    /// Top of its composed text below the block's top.
    dy: f64,
    m: NoteMetrics,
}

/// Vertical metrics of a composed footnote, relative to the top of its composed text.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NoteMetrics {
    /// First and last baselines (the first one includes the First Baseline offset).
    first: f64,
    last: f64,
    /// Leading of the first line: its baseline's distance below the previous footnote's last.
    lead: f64,
    /// Descent of the last line.
    descent: f64,
}

#[derive(Clone, Copy)]
pub(crate) struct NoteColumn<'a> {
    pub(crate) frame_index: usize,
    pub(crate) column_index: usize,
    pub(crate) width: f64,
    pub(crate) frame: &'a FrameSpec,
}

/// Height of footnotes stacked baseline to baseline, from the top of the footnote area to the
/// last footnote's descent.
fn stack_height<'a>(notes: impl IntoIterator<Item = &'a NoteMetrics>, space_between: f64) -> f64 {
    let mut h: Option<f64> = None;
    let mut descent = 0.0;
    for m in notes {
        let last = match h {
            None => m.last,
            Some(prev) => prev + space_between + m.lead + (m.last - m.first),
        };
        h = Some(last);
        descent = m.descent;
    }
    h.map_or(0.0, |h| h + descent)
}

pub(crate) struct Notes {
    /// Byte offsets of the reference characters (index = footnote index).
    refs: Vec<usize>,
    /// Number of each footnote (assigned when its paragraph is shaped).
    numbers: Vec<u32>,
    /// Next number and the restart unit (page / spread / section start) it counts in.
    pub num: u32,
    pub key: Option<usize>,
    pub placed: Vec<Placed>,
    /// Composed footnote blocks by (index, number, width, page sides).
    cache: HashMap<(usize, u32, u64, bool, bool), (Arc<Vec<NotePart>>, NoteMetrics)>,
    notes: Vec<Arc<designcraft_doc::Footnote>>,
    /// Numbers of the nested footnotes by id, assigned with the footnote they're nested in.
    nested: Arc<HashMap<u64, u32>>,
    /// Composing a footnote's own text: its references (nested notes) take these numbers, and
    /// nothing is placed (the nested notes print under the footnote, see [`NotePart`]).
    inner: Option<Arc<HashMap<u64, u32>>>,
    /// Footnotes span the columns of their frame.
    span: bool,
    /// Room a frame's earlier columns had to leave for spanning notes placed in a later column;
    /// kept across re-lays so the columns before the reference make room on the next pass.
    floor: HashMap<usize, f64>,
}

impl Notes {
    pub fn new(doc: &Document, story: &Story, opts: &ComposeOptions) -> Notes {
        let refs: Vec<usize> = if story.notes.is_empty() { Vec::new() } else { story.text.match_indices(FOOTNOTE_REF).map(|(i, _)| i).collect() };
        let inner = opts.note_numbers.clone();
        let start = if refs.is_empty() || inner.is_some() { 1 } else { doc.footnote_start(story.id) };
        Notes {
            nested: Arc::default(),
            inner,
            numbers: vec![start; refs.len()],
            refs,
            num: start,
            key: None,
            placed: Vec::new(),
            cache: HashMap::new(),
            notes: story.notes.clone(),
            span: doc.footnote_options.span_columns,
            floor: HashMap::new(),
        }
    }

    /// Whether a note placed in (fi, col) shares the footnote area of column (`fi`, `ci`).
    fn same_area(&self, p: &Placed, fi: usize, ci: usize) -> bool {
        p.fi == fi && (self.span || p.col == ci)
    }

    /// The footnote area of column `col` of a frame with columns `cols`: the column, or all of
    /// them when notes span columns.
    pub fn area(&self, cols: &[Rect], col: usize) -> Option<Rect> {
        if self.span { cols.iter().copied().reduce(|a, b| a.union(b)) } else { cols.get(col.min(cols.len().saturating_sub(1))).copied() }
    }

    pub fn active(&self) -> bool {
        !self.refs.is_empty() && self.refs.len() == self.notes.len()
    }

    /// Number the references in a paragraph (shaped on `page`) and hand their labels to the shaper.
    pub fn number_refs(&mut self, doc: &Document, prange: Range<usize>, page: Option<usize>, sub: &mut SubstCtx) {
        let o = &doc.footnote_options;
        let key = page.and_then(|p| match o.restart {
            NoteRestart::Never => None,
            NoteRestart::Page => Some(p),
            NoteRestart::Spread => doc.page_loc(p).map(|l| l.0),
            NoteRestart::Section => Some(doc.section_of(p).map_or(0, |s| s.start)),
        });
        let a = self.refs.partition_point(|&b| b < prange.start);
        let b = self.refs.partition_point(|&b| b < prange.end);
        sub.note_position = o.ref_position;
        sub.note_style = (o.ref_char_style != designcraft_doc::NO_CHAR_STYLE).then(|| o.ref_char_style.clone());
        if let Some(numbers) = &self.inner {
            // A footnote's own text: its nested notes' numbers were given with the footnote. A
            // note past the depth limit has none and shows no mark.
            for k in a..b {
                let n = self.notes.get(k).and_then(|n| numbers.get(&n.id)).copied();
                sub.notes.insert(self.refs[k], n.map_or_else(String::new, |n| o.label(n, false)));
            }
            return;
        }
        for k in a..b {
            if o.restart != NoteRestart::Never && key != self.key {
                self.key = key;
                self.num = o.start_at;
            }
            self.numbers[k] = self.num;
            self.num = self.num.saturating_add(1);
            sub.notes.insert(self.refs[k], o.label(self.numbers[k], false));
            // The notes nested in it take the next numbers, in reading order: each reference
            // then the notes nested in its note, before the next reference.
            if let Some(note) = self.notes.get(k)
                && !note.text.notes.is_empty()
            {
                let nested = Arc::make_mut(&mut self.nested);
                for n in note.nested() {
                    nested.insert(n.id, self.num);
                    self.num = self.num.saturating_add(1);
                }
            }
        }
    }

    /// Footnote indices referenced by a line's glyphs (none in a footnote's own text: nested
    /// notes are printed with the footnote, not placed).
    pub fn refs_in(&self, glyphs: &[Glyph]) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        if self.inner.is_some() {
            return v;
        }
        for g in glyphs.iter().filter(|g| g.ch == FOOTNOTE_REF && g.len > 0) {
            if let Ok(k) = self.refs.binary_search(&g.byte)
                && !v.contains(&k)
            {
                v.push(k);
            }
        }
        v
    }

    /// Footnote `k` composed at width `w`, with the footnotes nested in it printed under it
    /// ([`NotePart`]), and the block's metrics.
    fn compose(&mut self, doc: &Document, k: usize, w: f64, f: &FrameSpec, opts: &ComposeOptions) -> (Arc<Vec<NotePart>>, NoteMetrics) {
        let o = &doc.footnote_options;
        let number = self.numbers.get(k).copied().unwrap_or(1);
        let mark = o.label(number, true);
        // The page side sets towards/away-from-spine alignment and spine-relative anchored objects.
        let key = (k, number, w.to_bits(), f.left_page, f.right_page);
        if let Some(c) = self.cache.get(&key) {
            return c.clone();
        }
        let spec = FrameSpec {
            id: ItemId(0),
            area: Rect::new(0.0, 0.0, w.max(1.0), 1.0e6),
            opts: TextFrameOptions { first_baseline: o.first_baseline, first_baseline_min: o.first_baseline_min, ..Default::default() },
            vertical: false,
            exclusions: vec![],
            outline: None,
            page_name: f.page_name.clone(),
            section_marker: f.section_marker.clone(),
            page: f.page,
            parent: f.parent,
            grid: None,
            left_page: f.left_page,
            right_page: f.right_page,
            page_rect: None,
        };
        let nested = Arc::clone(&self.nested);
        let compose_part = |story: &Story, label: String| {
            let nopts = ComposeOptions {
                page_name: opts.page_name.clone(),
                section_marker: opts.section_marker.clone(),
                page: opts.page,
                running: opts.running.clone(),
                label: Some(label),
                xrefs: opts.xrefs.clone(),
                placed: Vec::new(),
                note_numbers: Some(Arc::clone(&nested)),
            };
            let cs = crate::compose(doc, story, std::slice::from_ref(&spec), &nopts);
            let lines = cs.frames.first().map(|ft| ft.lines.as_slice()).unwrap_or_default();
            let m = match (lines.first(), lines.last()) {
                (Some(a), Some(z)) => NoteMetrics { first: a.baseline, last: z.baseline, lead: a.leading, descent: z.descent },
                _ => NoteMetrics::default(),
            };
            (Arc::new(cs), m)
        };
        let Some(note) = self.notes.get(k) else { return (Arc::default(), NoteMetrics::default()) };
        let (text, first) = compose_part(&note.text, format!("{mark}{}", o.separator));
        let mut parts = vec![NotePart { id: note.id, label: mark, text, source: Arc::from(note.text.text.as_str()), dy: 0.0, m: first }];
        for n in note.nested() {
            let Some(&num) = self.nested.get(&n.id) else { continue };
            let label = o.label(num, true);
            // Printed as if after a forced line break: no first-line indent and no space before.
            let mut story = n.text.clone();
            if let Some(p) = story.paras.first_mut() {
                p.para.first_line_indent = Some(0.0);
                p.para.space_before = Some(0.0);
            }
            let (text, m) = compose_part(&story, format!("{label}{}", o.separator));
            let Some(prev) = parts.last() else { continue };
            // Its first baseline a line's leading below the previous part's last.
            let dy = prev.dy + prev.m.last + m.lead - m.first;
            parts.push(NotePart { id: n.id, label, text, source: Arc::from(n.text.text.as_str()), dy, m });
        }
        // The block: the footnote's first line to its last nested note's last.
        let m = parts.last().map_or(first, |p| NoteMetrics { first: first.first, lead: first.lead, last: p.dy + p.m.last, descent: p.m.descent });
        let v = (Arc::new(parts), m);
        self.cache.insert(key, v.clone());
        v
    }

    /// Height the footnotes of column (fi, col) take, with `extra` footnotes added.
    pub fn reserve(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], opts: &ComposeOptions) -> f64 {
        let o = &doc.footnote_options;
        let (space_between, space_before) = (o.space_between, o.space_before);
        let mut ms: Vec<NoteMetrics> =
            self.placed.iter().filter(|p| self.same_area(p, column.frame_index, column.column_index)).map(|p| p.m).collect();
        for &k in extra {
            ms.push(self.compose(doc, k, column.width, column.frame, opts).1);
        }
        let r = if ms.is_empty() { 0.0 } else { stack_height(&ms, space_between) + space_before };
        if self.span { r.max(self.floor.get(&column.frame_index).copied().unwrap_or(0.0)) } else { r }
    }

    /// After spanning notes were placed in column `col` of frame `fi`: whether a line of an earlier
    /// column now runs into their area. The area is then remembered for the frame, to re-lay it.
    pub fn earlier_columns_collide(&mut self, lines: &[crate::Line], fi: usize, col: usize, cols: &[Rect], reserve: f64) -> bool {
        if !self.span || col == 0 || reserve <= self.floor.get(&fi).copied().unwrap_or(0.0) + 0.01 {
            return false;
        }
        let hit = lines
            .iter()
            .any(|l| (l.column as usize) < col && cols.get(l.column as usize).is_some_and(|c| l.baseline + l.descent > c.y1 - reserve + 0.01));
        if hit {
            self.floor.insert(fi, reserve);
        }
        hit
    }

    pub fn place(&mut self, doc: &Document, k: usize, column: NoteColumn<'_>, opts: &ComposeOptions) {
        if self.placed.iter().any(|p| p.k == k) {
            return;
        }
        let (parts, m) = self.compose(doc, k, column.width, column.frame, opts);
        self.placed.push(Placed { k, fi: column.frame_index, col: column.column_index, m, parts });
    }

    /// Stack each column's footnotes against its bottom, rule above the first.
    pub fn finish(&mut self, doc: &Document, story: &Story, frames: &[FrameSpec], cols: &[Vec<Rect>], out: &mut ComposedStory) {
        if self.placed.is_empty() {
            return;
        }
        let o = &doc.footnote_options;
        let mut groups: Vec<(usize, usize)> = self.placed.iter().map(|p| (p.fi, if self.span { 0 } else { p.col })).collect();
        groups.dedup();
        groups.sort_unstable();
        groups.dedup();
        for (fi, ci) in groups {
            let (Some(_), Some(col)) = (frames.get(fi), cols.get(fi).filter(|c| ci < c.len()).and_then(|c| self.area(c, ci))) else { continue };
            let here: Vec<&Placed> = self.placed.iter().filter(|p| self.same_area(p, fi, ci)).collect();
            let total = stack_height(here.iter().map(|p| &p.m), o.space_between);
            let top = col.y1 - total;
            let ft = &mut out.frames[fi];
            if o.rule.on && o.rule.weight > 0.0 {
                let x0 = col.x0 + o.rule.left_indent;
                let y = top + o.rule.offset - o.rule.weight;
                ft.decos.push(Deco {
                    rect: Rect::new(x0, y, (x0 + o.rule.width).min(col.x1), y + o.rule.weight),
                    color: o.rule.color.clone(),
                    tint: o.rule.tint,
                    ..Deco::default()
                });
            }
            // Previous footnote's last baseline and bottom.
            let mut prev: Option<(f64, f64)> = None;
            for p in here {
                if story.notes.get(p.k).is_none() {
                    continue;
                }
                let (y, y0) = match prev {
                    None => (top, top),
                    Some((last, bottom)) => {
                        let first = last + o.space_between + p.m.lead;
                        (first - p.m.first, (bottom + o.space_between).min(first))
                    }
                };
                // The footnote, then its nested notes under it, each from where the one before
                // ends (so a click between them finds one).
                let mut part_top = y0;
                for part in p.parts.iter() {
                    let py = y + part.dy;
                    let bottom = py + part.m.last + part.m.descent;
                    ft.notes.push(PlacedNote {
                        id: part.id,
                        index: p.k,
                        column: p.col as u32,
                        label: part.label.clone(),
                        origin: Point::new(col.x0, py),
                        rect: Rect::new(col.x0, part_top, col.x1, bottom.max(part_top)),
                        text: part.text.clone(),
                        source: part.source.clone(),
                    });
                    part_top = bottom;
                }
                let y1 = y + p.m.last + p.m.descent;
                prev = Some((y + p.m.last, y1));
            }
        }
    }
}

/// The placed footnote with id `id`: (frame index, footnote).
pub fn find_note(cs: &ComposedStory, id: u64) -> Option<(usize, &PlacedNote)> {
    cs.frames.iter().enumerate().find_map(|(fi, ft)| ft.notes.iter().find(|n| n.id == id).map(|n| (fi, n)))
}

/// Footnote text hit at frame-inner point `p`: (footnote id, byte in the footnote's story).
pub fn hit_note(cs: &ComposedStory, fi: usize, p: Point) -> Option<(u64, usize)> {
    hit_note_with(cs, fi, p, crate::hit)
}

/// [`hit_note`] with the hit test for the note's text (see [`crate::hit_cell_with`]).
pub fn hit_note_with(cs: &ComposedStory, fi: usize, p: Point, hit: fn(&ComposedStory, usize, Point) -> Option<usize>) -> Option<(u64, usize)> {
    let ft = cs.frames.get(fi)?;
    let n = ft.notes.iter().find(|n| n.rect.contains(p))?;
    let local = Point::new(p.x - n.origin.x, p.y - n.origin.y);
    Some((n.id, hit(&n.text, 0, local).unwrap_or(0)))
}

/// Caret geometry for byte `pos` of a footnote's story, in frame inner space:
/// (frame index, x, baseline, ascent, descent).
pub fn note_caret(cs: &ComposedStory, id: u64, pos: usize) -> Option<(usize, f64, f64, f64, f64)> {
    let (fi, n) = find_note(cs, id)?;
    match crate::caret(&n.text, pos) {
        Some((_, x, bl, a, d)) => Some((fi, x + n.origin.x, bl + n.origin.y, a, d)),
        None => Some((fi, n.origin.x, n.origin.y + 10.0, 9.0, 3.0)),
    }
}
