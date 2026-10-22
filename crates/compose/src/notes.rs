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
    text: Arc<ComposedStory>,
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
    /// Composed footnote text by (index, label, width).
    cache: HashMap<(usize, String, u64), (Arc<ComposedStory>, NoteMetrics)>,
    notes: Vec<Arc<designcraft_doc::Footnote>>,
    /// Footnotes span the columns of their frame.
    span: bool,
    /// Room a frame's earlier columns had to leave for spanning notes placed in a later column;
    /// kept across re-lays so the columns before the reference make room on the next pass.
    floor: HashMap<usize, f64>,
}

impl Notes {
    pub fn new(doc: &Document, story: &Story) -> Notes {
        let refs: Vec<usize> = if story.notes.is_empty() { Vec::new() } else { story.text.match_indices(FOOTNOTE_REF).map(|(i, _)| i).collect() };
        let start = if refs.is_empty() { 1 } else { doc.footnote_start(story.id) };
        Notes {
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
        for k in a..b {
            if o.restart != NoteRestart::Never && key != self.key {
                self.key = key;
                self.num = o.start_at;
            }
            self.numbers[k] = self.num;
            self.num += 1;
            sub.notes.insert(self.refs[k], o.label(self.numbers[k], false));
        }
        sub.note_position = o.ref_position;
        sub.note_style = (o.ref_char_style != designcraft_doc::NO_CHAR_STYLE).then(|| o.ref_char_style.clone());
    }

    /// Footnote indices referenced by a line's glyphs.
    pub fn refs_in(&self, glyphs: &[Glyph]) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        for g in glyphs.iter().filter(|g| g.ch == FOOTNOTE_REF && g.len > 0) {
            if let Ok(k) = self.refs.binary_search(&g.byte)
                && !v.contains(&k)
            {
                v.push(k);
            }
        }
        v
    }

    fn compose(&mut self, doc: &Document, k: usize, w: f64, f: &FrameSpec, opts: &ComposeOptions) -> (Arc<ComposedStory>, NoteMetrics) {
        let o = &doc.footnote_options;
        let label = format!("{}{}", o.label(self.numbers[k], true), o.separator);
        let key = (k, label.clone(), w.to_bits());
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
            grid: None,
            left_page: f.left_page,
            page_rect: None,
        };
        let nopts = ComposeOptions {
            page_name: opts.page_name.clone(),
            section_marker: opts.section_marker.clone(),
            page: opts.page,
            running: opts.running.clone(),
            label: Some(label),
            xrefs: opts.xrefs.clone(),
        };
        let cs = crate::compose(doc, &self.notes[k].text, std::slice::from_ref(&spec), &nopts);
        let lines = cs.frames.first().map(|ft| ft.lines.as_slice()).unwrap_or_default();
        let m = match (lines.first(), lines.last()) {
            (Some(a), Some(z)) => NoteMetrics { first: a.baseline, last: z.baseline, lead: a.leading, descent: z.descent },
            _ => NoteMetrics::default(),
        };
        let v = (Arc::new(cs), m);
        self.cache.insert(key, v.clone());
        v
    }

    /// Height the footnotes of column (fi, col) take, with `extra` footnotes added.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(&mut self, doc: &Document, fi: usize, col: usize, extra: &[usize], w: f64, f: &FrameSpec, opts: &ComposeOptions) -> f64 {
        let o = &doc.footnote_options;
        let (space_between, space_before) = (o.space_between, o.space_before);
        let mut ms: Vec<NoteMetrics> = self.placed.iter().filter(|p| self.same_area(p, fi, col)).map(|p| p.m).collect();
        for &k in extra {
            ms.push(self.compose(doc, k, w, f, opts).1);
        }
        let r = if ms.is_empty() { 0.0 } else { stack_height(&ms, space_between) + space_before };
        if self.span { r.max(self.floor.get(&fi).copied().unwrap_or(0.0)) } else { r }
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

    pub fn place(&mut self, doc: &Document, k: usize, fi: usize, col: usize, w: f64, f: &FrameSpec, opts: &ComposeOptions) {
        if self.placed.iter().any(|p| p.k == k) {
            return;
        }
        let (text, m) = self.compose(doc, k, w, f, opts);
        self.placed.push(Placed { k, fi, col, m, text });
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
                let Some(note) = story.notes.get(p.k) else { continue };
                let (y, y0) = match prev {
                    None => (top, top),
                    Some((last, bottom)) => {
                        let first = last + o.space_between + p.m.lead;
                        (first - p.m.first, (bottom + o.space_between).min(first))
                    }
                };
                let y1 = y + p.m.last + p.m.descent;
                ft.notes.push(PlacedNote {
                    id: note.id,
                    index: p.k,
                    column: p.col as u32,
                    label: o.label(self.numbers[p.k], true),
                    origin: Point::new(col.x0, y),
                    rect: Rect::new(col.x0, y0, col.x1, y1),
                    text: p.text.clone(),
                    source: Arc::from(note.text.text.as_str()),
                });
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
