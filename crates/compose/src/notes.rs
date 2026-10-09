//! Footnote layout: numbering, room at the bottom of columns, and placement.
//!
//! When a line holding footnote references is set, each referenced footnote is composed at the
//! column width and the column's text area shrinks by its height (plus the minimum space before
//! the first footnote and the space between footnotes). A line that would collide moves to the
//! next column with its footnotes. After the story is laid out the footnotes of each column are
//! stacked against the column bottom, with the separator rule above the first one.

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
    h: f64,
    text: Arc<ComposedStory>,
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
    cache: HashMap<(usize, String, u64), (Arc<ComposedStory>, f64)>,
    notes: Vec<Arc<designcraft_doc::Footnote>>,
}

impl Notes {
    pub fn new(doc: &Document, story: &Story) -> Notes {
        let refs: Vec<usize> = if story.notes.is_empty() { Vec::new() } else { story.text.match_indices(FOOTNOTE_REF).map(|(i, _)| i).collect() };
        let start = if refs.is_empty() { 1 } else { doc.footnote_start(story.id) };
        Notes { numbers: vec![start; refs.len()], refs, num: start, key: None, placed: Vec::new(), cache: HashMap::new(), notes: story.notes.clone() }
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

    fn compose(&mut self, doc: &Document, k: usize, w: f64, f: &FrameSpec, opts: &ComposeOptions) -> (Arc<ComposedStory>, f64) {
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
            page_name: f.page_name.clone(),
            page: f.page,
            grid: None,
            left_page: f.left_page,
            page_rect: None,
        };
        let nopts = ComposeOptions {
            page_name: opts.page_name.clone(),
            page: opts.page,
            running: opts.running.clone(),
            label: Some(label),
            xrefs: opts.xrefs.clone(),
        };
        let cs = crate::compose(doc, &self.notes[k].text, std::slice::from_ref(&spec), &nopts);
        let h = cs.frames.first().and_then(|ft| ft.lines.last()).map_or(0.0, |l| l.baseline + l.descent);
        let v = (Arc::new(cs), h);
        self.cache.insert(key, v.clone());
        v
    }

    /// Height the footnotes of column (fi, col) take, with `extra` footnotes added.
    #[allow(clippy::too_many_arguments)]
    pub fn reserve(&mut self, doc: &Document, fi: usize, col: usize, extra: &[usize], w: f64, f: &FrameSpec, opts: &ComposeOptions) -> f64 {
        let o = &doc.footnote_options;
        let mut n = 0usize;
        let mut h = 0.0;
        for p in self.placed.iter().filter(|p| p.fi == fi && p.col == col) {
            n += 1;
            h += p.h;
        }
        for &k in extra {
            n += 1;
            h += self.compose(doc, k, w, f, opts).1;
        }
        if n == 0 { 0.0 } else { h + o.space_between * (n - 1) as f64 + o.space_before }
    }

    pub fn place(&mut self, doc: &Document, k: usize, fi: usize, col: usize, w: f64, f: &FrameSpec, opts: &ComposeOptions) {
        if self.placed.iter().any(|p| p.k == k) {
            return;
        }
        let (text, h) = self.compose(doc, k, w, f, opts);
        self.placed.push(Placed { k, fi, col, h, text });
    }

    /// Stack each column's footnotes against its bottom, rule above the first.
    pub fn finish(&mut self, doc: &Document, story: &Story, frames: &[FrameSpec], cols: &[Vec<Rect>], out: &mut ComposedStory) {
        if self.placed.is_empty() {
            return;
        }
        let o = &doc.footnote_options;
        let mut groups: Vec<(usize, usize)> = self.placed.iter().map(|p| (p.fi, p.col)).collect();
        groups.dedup();
        groups.sort_unstable();
        groups.dedup();
        for (fi, ci) in groups {
            let (Some(_), Some(col)) = (frames.get(fi), cols.get(fi).and_then(|c| c.get(ci))) else { continue };
            let here: Vec<&Placed> = self.placed.iter().filter(|p| p.fi == fi && p.col == ci).collect();
            let total: f64 = here.iter().map(|p| p.h).sum::<f64>() + o.space_between * (here.len() - 1) as f64;
            let top = col.y1 - total;
            let ft = &mut out.frames[fi];
            if o.rule.on && o.rule.weight > 0.0 {
                let x0 = col.x0 + o.rule.left_indent;
                let y = top + o.rule.offset - o.rule.weight;
                ft.decos.push(Deco {
                    rect: Rect::new(x0, y, (x0 + o.rule.width).min(col.x1), y + o.rule.weight),
                    color: o.rule.color.clone(),
                    tint: o.rule.tint,
                });
            }
            let mut y = top;
            for p in here {
                let note = &story.notes[p.k];
                ft.notes.push(PlacedNote {
                    id: note.id,
                    index: p.k,
                    column: ci as u32,
                    label: o.label(self.numbers[p.k], true),
                    origin: Point::new(col.x0, y),
                    rect: Rect::new(col.x0, y, col.x1, y + p.h),
                    text: p.text.clone(),
                    source: Arc::from(note.text.text.as_str()),
                });
                y += p.h + o.space_between;
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
