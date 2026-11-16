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

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use designcraft_doc::notes::NoteRestart;
use designcraft_doc::{Document, FOOTNOTE_REF, ItemId, Story, TextFrameOptions};
use designcraft_geom::{Point, Rect};

use crate::shape::{Glyph, SubstCtx};
use crate::{ComposeOptions, ComposedStory, Deco, FrameSpec, PlacedNote};

pub(crate) mod streams;
pub(crate) use streams::fit_stream_page_for;
pub use streams::{StreamPageFit, StreamPageInput, StreamState, fit_stream_page};

/// A footnote assigned to a column.
pub(crate) struct Placed {
    k: usize,
    part: usize,
    fi: usize,
    col: usize,
    m: NoteMetrics,
    text: Arc<ComposedStory>,
}

/// Vertical metrics of a composed footnote, relative to the top of its composed text.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NoteMetrics {
    /// First and last baselines (the first one includes the First Baseline offset).
    pub(super) first: f64,
    pub(super) last: f64,
    /// Leading of the first line: its baseline's distance below the previous footnote's last.
    pub(super) lead: f64,
    /// Descent of the last line.
    pub(super) descent: f64,
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct PartPage {
    page_name: Option<String>,
    section_marker: Option<String>,
    page: Option<usize>,
    parent: bool,
    left_page: bool,
    right_page: bool,
}

impl PartPage {
    fn of(f: &FrameSpec) -> PartPage {
        PartPage {
            page_name: f.page_name.clone(),
            section_marker: f.section_marker.clone(),
            page: f.page,
            parent: f.parent,
            left_page: f.left_page,
            right_page: f.right_page,
        }
    }
}

/// Last placed part, including each part's area and page context used to thread its text.
#[derive(Clone, Debug)]
pub(crate) struct Carry {
    k: usize,
    heights: Vec<(f64, f64)>,
    pages: Vec<PartPage>,
    continuations: Vec<(usize, f64)>,
    exclusions: Vec<(usize, Vec<crate::Exclusion>)>,
    fi: usize,
    col: usize,
}

pub(crate) struct SplitPlan {
    exclusions: Vec<(usize, Vec<crate::Exclusion>)>,
    continuations: Vec<(usize, f64)>,
    remainder: bool,
    k: usize,
    heights: Vec<(f64, f64)>,
    pages: Vec<PartPage>,
    text: Arc<ComposedStory>,
    m: NoteMetrics,
}

type SeriesSnapshot = (u32, Option<usize>, usize, Option<Carry>);
type Part = (Arc<ComposedStory>, NoteMetrics);
type CacheKey = (usize, String, u64, PartPage, Vec<(u64, u64, PartPage)>);

pub(super) struct SeriesNotes {
    series: u64,
    band_previous: Option<f64>,
    band_continued: bool,
    band_span: Option<f64>,
    band_spans: HashMap<(usize, usize), f64>,
    band_incoming: Option<(usize, usize)>,
    members: Vec<bool>,
    /// Byte offsets of the reference characters (index = footnote index).
    refs: Vec<usize>,
    /// Number of each footnote (assigned when its paragraph is shaped).
    numbers: Vec<u32>,
    /// Next number and the restart unit (page / spread / section start) it counts in.
    pub num: u32,
    pub key: Option<usize>,
    pub placed: Vec<Placed>,
    /// Composed footnote text by note, label, area dimensions and page context.
    cache: HashMap<CacheKey, Vec<Option<Part>>>,
    pub carry: Option<Carry>,
    notes: Vec<Arc<designcraft_doc::Footnote>>,
    /// Footnotes span the columns of their frame.
    span: bool,
}

impl SeriesNotes {
    pub(super) fn new(doc: &Document, story: &Story, series: u64, nested: bool) -> Self {
        let refs: Vec<usize> = if story.notes.is_empty() { Vec::new() } else { story.text.match_indices(FOOTNOTE_REF).map(|(i, _)| i).collect() };
        let start = if refs.is_empty() { 1 } else { doc.footnote_series_start(story.id, series) };
        Self {
            series,
            band_previous: None,
            band_continued: false,
            band_span: None,
            band_spans: HashMap::new(),
            band_incoming: None,
            members: story.notes.iter().map(|n| nested || doc.note_series_id(n.series) == series).collect(),
            numbers: vec![start; refs.len()],
            refs,
            num: start,
            key: None,
            placed: Vec::new(),
            cache: HashMap::new(),
            carry: None,
            notes: story.notes.clone(),
            span: doc.note_options(series).span_columns,
        }
    }

    fn owns(&self, k: usize) -> bool {
        self.members.get(k).copied().unwrap_or(false)
    }

    fn column<'a>(&self, mut column: NoteColumn<'a>) -> NoteColumn<'a> {
        if self.span {
            column.width = column.frame.columns().into_iter().reduce(|a, b| a.union(b)).map_or(column.width, |r| r.width());
        }
        column
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
        let o = doc.note_options(self.series);
        let key = page.and_then(|p| match o.restart {
            NoteRestart::Never => None,
            NoteRestart::Page => Some(p),
            NoteRestart::Spread => doc.page_loc(p).map(|l| l.0),
            NoteRestart::Section => Some(doc.section_of(p).map_or(0, |s| s.start)),
        });
        let a = self.refs.partition_point(|&b| b < prange.start);
        let b = self.refs.partition_point(|&b| b < prange.end);
        for k in a..b {
            if !self.owns(k) {
                continue;
            }
            if o.restart != NoteRestart::Never && key != self.key {
                self.key = key;
                self.num = o.start_at;
            }
            self.numbers[k] = self.num;
            self.num = self.num.saturating_add(1);
            sub.notes.insert(self.refs[k], if o.ref_hidden { String::new() } else { o.label(self.numbers[k], false) });
            sub.note_formats.insert(self.refs[k], (o.ref_position, o.ref_char_style.clone()));
        }
    }

    /// Footnote indices referenced by a line's glyphs.
    /// In the order they first appear; a set keeps a line with many references linear.
    pub fn refs_in(&self, glyphs: &[Glyph]) -> Vec<usize> {
        let mut v: Vec<usize> = Vec::new();
        let mut seen = HashSet::new();
        for g in glyphs.iter().filter(|g| g.ch == FOOTNOTE_REF && g.len > 0) {
            if let Ok(k) = self.refs.binary_search(&g.byte)
                && self.owns(k)
                && seen.insert(k)
            {
                v.push(k);
            }
        }
        v
    }

    fn compose(&mut self, doc: &Document, k: usize, w: f64, f: &FrameSpec, opts: &ComposeOptions) -> Part {
        self.compose_parts(doc, k, w, f, opts, &[], &[])
            .first()
            .and_then(Clone::clone)
            .unwrap_or_else(|| (Arc::new(ComposedStory::default()), NoteMetrics::default()))
    }

    /// One threaded composition lets the note's own paragraph keeps govern each frame break.
    /// `heights` and `pages` hold each earlier part's area and page context; the last part is `w` wide and unbounded.
    #[allow(clippy::too_many_arguments)] // Each earlier area needs its dimensions and page context.
    pub(super) fn compose_parts(
        &mut self,
        doc: &Document,
        k: usize,
        w: f64,
        f: &FrameSpec,
        opts: &ComposeOptions,
        heights: &[(f64, f64)],
        pages: &[PartPage],
    ) -> Vec<Option<Part>> {
        let o = doc.note_options(self.series);
        let label = format!("{}{}", o.label(self.numbers.get(k).copied().unwrap_or(o.start_at), true), o.separator);
        // Parent frames have no page index: their prefix and section context must also distinguish cached text.
        let current_page = PartPage::of(f);
        // Band replays depend on mutable continuation and contour history. Keep the
        // ordinary route cache; band trials must measure their exact current inputs.
        let key = (!opts.note_bands).then(|| {
            (
                k,
                label.clone(),
                w.to_bits(),
                current_page.clone(),
                heights
                    .iter()
                    .enumerate()
                    .map(|(i, (pw, h))| {
                        let page = pages.get(i).unwrap_or(&current_page);
                        (pw.to_bits(), h.to_bits(), page.clone())
                    })
                    .collect(),
            )
        });
        if let Some(c) = key.as_ref().and_then(|key| self.cache.get(key)) {
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
        let nopts = ComposeOptions {
            page_name: opts.page_name.clone(),
            section_marker: opts.section_marker.clone(),
            page: opts.page,
            running: opts.running.clone(),
            label: Some(label),
            xrefs: opts.xrefs.clone(),
            placed: Vec::new(),
            continuations: self.part_continuations(k, heights, opts.note_bands),
            ..Default::default()
        };
        let Some(note) = self.notes.get(k) else { return Vec::new() };
        let previous = self.carry.as_ref().filter(|c| c.k == k);
        let part = previous.map_or(0, |c| c.heights.len());
        let local_exclusions = || {
            if opts.note_bands {
                f.exclusions
                    .iter()
                    .map(|e| crate::Exclusion {
                        rect: e.rect + designcraft_geom::Vec2::new(-f.area.x0, -f.area.y0),
                        contour: e.contour.as_ref().map(|c| c.clone().transformed(designcraft_geom::Affine::translate((-f.area.x0, -f.area.y0)))),
                        ..e.clone()
                    })
                    .collect()
            } else {
                Vec::new()
            }
        };
        let specs: Vec<FrameSpec> = heights
            .iter()
            .copied()
            .chain(std::iter::once((w, 1.0e6)))
            .enumerate()
            .map(|(i, (pw, h))| {
                let exclusions = if i == part {
                    local_exclusions()
                } else {
                    previous.and_then(|c| c.exclusions.iter().find(|(index, _)| *index == i)).map_or_else(Vec::new, |(_, e)| e.clone())
                };
                let page = pages.get(i).unwrap_or(&current_page);
                FrameSpec {
                    area: Rect::new(0.0, 0.0, pw.max(1.0), h),
                    exclusions,
                    page_name: page.page_name.clone(),
                    section_marker: page.section_marker.clone(),
                    page: page.page,
                    parent: page.parent,
                    left_page: page.left_page,
                    right_page: page.right_page,
                    ..spec.clone()
                }
            })
            .collect();
        let cs = crate::compose(doc, &note.text, &specs, &nopts);
        let parts = cs
            .frames
            .iter()
            .enumerate()
            .map(|(i, ft)| {
                let (Some(a), Some(z)) = (ft.lines.first(), ft.lines.last()) else { return None };
                let m = NoteMetrics { first: a.baseline, last: z.baseline, lead: a.leading, descent: z.descent };
                let mut frame = ft.clone();
                // Body frame ranges exclude paragraph separators; parts own those bytes too.
                frame.range.end = cs.frames.iter().skip(i.saturating_add(1)).find(|f| !f.lines.is_empty()).map_or(note.text.len(), |f| f.range.start);
                let text = ComposedStory { frames: vec![frame], ..cs.clone() };
                Some((Arc::new(text), m))
            })
            .collect::<Vec<_>>();
        if let Some(key) = key {
            self.cache.insert(key, parts.clone());
        }
        parts
    }

    fn part_continuations(&self, k: usize, heights: &[(f64, f64)], bands: bool) -> Vec<(usize, f64)> {
        let mut parts = self.carry.as_ref().filter(|c| c.k == k).map_or_else(Vec::new, |c| c.continuations.clone());
        if bands {
            let first = self.band_incoming.filter(|(index, _)| *index == k).map_or(1, |(_, count)| count.saturating_add(1));
            parts.extend(
                heights
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i.saturating_add(1) >= first)
                    .map(|(i, (_, h))| (i.saturating_add(1), -self.band_spans.get(&(k, i)).copied().unwrap_or(*h))),
            );
        }
        parts.sort_by_key(|(index, _)| *index);
        parts.dedup_by_key(|(index, _)| *index);
        parts
    }

    fn pending(&self, column: NoteColumn<'_>) -> Option<Carry> {
        self.carry
            .as_ref()
            .filter(|c| column.frame_index > c.fi || (column.frame_index == c.fi && !self.span && column.column_index > c.col))
            .cloned()
    }

    fn remainder(&mut self, doc: &Document, c: &Carry, column: NoteColumn<'_>, opts: &ComposeOptions) -> Option<Part> {
        self.compose_parts(doc, c.k, column.width, column.frame, opts, &c.heights, &c.pages).get(c.heights.len()).and_then(Clone::clone)
    }

    fn metrics(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], opts: &ComposeOptions) -> Vec<NoteMetrics> {
        let mut ms: Vec<_> = self.placed.iter().filter(|p| self.same_area(p, column.frame_index, column.column_index)).map(|p| p.m).collect();
        if let Some(c) = self.pending(column)
            && let Some((_, m)) = self.remainder(doc, &c, column, opts)
        {
            ms.push(m);
        }
        for &k in extra {
            ms.push(self.compose(doc, k, column.width, column.frame, opts).1);
        }
        ms
    }

    /// Reserve fixed notes first, then use the largest frame height left for the final note.
    pub fn split_plan(
        &mut self,
        doc: &Document,
        column: NoteColumn<'_>,
        extra: &[usize],
        available: f64,
        opts: &ComposeOptions,
    ) -> Option<SplitPlan> {
        if !doc.note_options(self.series).allow_split || !available.is_finite() || available <= 0.0 {
            return None;
        }
        let pending = self.pending(column);
        let (k, mut heights, mut pages, ms, original) = if let Some((&k, before)) = extra.split_last() {
            // If the carry must split too, move this referencing line on as before.
            if self.carry.is_some() && pending.is_none() {
                return None;
            }
            let ms = self.metrics(doc, column, before, opts);
            if stack_height(&ms, doc.note_options(self.series).space_between) > available {
                return None;
            }
            // A second carry would discard the first note's remainder.
            if pending.as_ref().is_some_and(|c| self.remainder(doc, c, column, opts).is_none()) {
                return None;
            }
            (k, Vec::new(), Vec::new(), ms, self.compose(doc, k, column.width, column.frame, opts))
        } else {
            let c = pending?;
            let original = self.remainder(doc, &c, column, opts)?;
            let ms = self.placed.iter().filter(|p| self.same_area(p, column.frame_index, column.column_index)).map(|p| p.m).collect();
            (c.k, c.heights, c.pages, ms, original)
        };
        let offset = ms.last().map_or(0.0, |last| {
            stack_height(&ms, doc.note_options(self.series).space_between) - last.descent
                + doc.note_options(self.series).space_between
                + original.1.lead
                - original.1.first
        });
        let mut h = available - offset;
        let part = heights.len();
        heights.push((column.width, h));
        pages.push(PartPage::of(column.frame));
        // Ordinary text frames fit baselines, but a footnote stack also includes descent.
        // Each retry drops its last line; the unsplit remainder bounds the attempts.
        let attempts = original.0.frames.first().map_or(0, |f| f.lines.len());
        for _ in 0..attempts {
            if !h.is_finite() || h <= 0.0 {
                return None;
            }
            *heights.get_mut(part)? = (column.width, h);
            let parts = self.compose_parts(doc, k, column.width, column.frame, opts, &heights, &pages);
            let (text, m) = parts.get(part).and_then(Clone::clone)?;
            let total = stack_height(ms.iter().chain(std::iter::once(&m)), doc.note_options(self.series).space_between);
            if m.last + m.descent <= h + 0.01 && total <= available + 0.01 {
                let mut exclusions = self.carry.as_ref().filter(|c| c.k == k).map_or_else(Vec::new, |c| c.exclusions.clone());
                if opts.note_bands {
                    exclusions.push((
                        part,
                        column
                            .frame
                            .exclusions
                            .iter()
                            .map(|e| crate::Exclusion {
                                rect: e.rect + designcraft_geom::Vec2::new(-column.frame.area.x0, -column.frame.area.y0),
                                contour: e.contour.as_ref().map(|c| {
                                    c.clone().transformed(designcraft_geom::Affine::translate((-column.frame.area.x0, -column.frame.area.y0)))
                                }),
                                ..e.clone()
                            })
                            .collect(),
                    ));
                }
                return Some(SplitPlan {
                    exclusions,
                    continuations: self.part_continuations(k, &heights, opts.note_bands).into_iter().filter(|(index, _)| *index <= part).collect(),
                    remainder: parts.get(heights.len()).is_some_and(Option::is_some),
                    k,
                    heights,
                    pages,
                    text,
                    m,
                });
            }
            let smaller = h.min(m.last - 0.02);
            if smaller >= h {
                return None;
            }
            h = smaller;
        }
        None
    }

    /// Height the footnotes of column (fi, col) take, with `extra` footnotes added.
    pub fn reserve(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], opts: &ComposeOptions) -> f64 {
        let o = doc.note_options(self.series);
        let space_between = o.space_between;
        let ms = self.metrics(doc, column, extra, opts);
        if ms.is_empty() { 0.0 } else { stack_height(&ms, space_between) + self.top_space(doc, column, &ms) }
    }

    fn top_space(&self, doc: &Document, column: NoteColumn<'_>, ms: &[NoteMetrics]) -> f64 {
        let o = doc.note_options(self.series);
        match (self.band_previous, ms.first()) {
            (Some(_), _) if self.band_continued => 0.0,
            (Some(previous), Some(m)) => previous + o.space_between + m.lead - m.first - column.frame.area.y0,
            _ => o.space_before,
        }
    }

    fn area_space(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], opts: &ComposeOptions) -> f64 {
        let ms = self.metrics(doc, column, extra, opts);
        self.top_space(doc, column, &ms)
    }

    pub fn place(&mut self, doc: &Document, k: usize, column: NoteColumn<'_>, opts: &ComposeOptions) {
        let (text, m) = self.compose(doc, k, column.width, column.frame, opts);
        self.push(k, 0, text, m, column);
    }

    fn push(&mut self, k: usize, part: usize, text: Arc<ComposedStory>, m: NoteMetrics, column: NoteColumn<'_>) {
        if self.placed.iter().any(|p| p.k == k && p.part == part) {
            return;
        }
        self.placed.push(Placed { k, part, fi: column.frame_index, col: column.column_index, m, text });
    }

    /// Continued text precedes references belonging to the new column's body.
    pub fn place_line(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], plan: Option<SplitPlan>, opts: &ComposeOptions) -> bool {
        let before = self.placed.len();
        if let Some(c) = self.pending(column) {
            if plan.as_ref().is_some_and(|p| p.k == c.k) {
                if let Some(p) = plan {
                    self.place_split(p, column);
                }
                return self.placed.len() != before;
            }
            if let Some((text, m)) = self.remainder(doc, &c, column, opts) {
                self.push(c.k, c.heights.len(), text, m, column);
            }
            self.carry = None;
        }
        for &k in extra {
            if plan.as_ref().is_some_and(|p| p.k == k) {
                continue;
            }
            self.place(doc, k, column, opts);
        }
        if let Some(p) = plan {
            self.place_split(p, column);
        }
        self.placed.len() != before
    }

    fn place_split(&mut self, plan: SplitPlan, column: NoteColumn<'_>) {
        let part = plan.heights.len().saturating_sub(1);
        if let Some(span) = self.band_span {
            self.band_spans.insert((plan.k, part), span);
            self.cache.clear();
        }
        self.push(plan.k, part, plan.text, plan.m, column);
        self.carry = plan.remainder.then_some(Carry {
            k: plan.k,
            heights: plan.heights,
            pages: plan.pages,
            continuations: plan.continuations,
            exclusions: plan.exclusions,
            fi: column.frame_index,
            col: column.column_index,
        });
    }

    /// Stack each column's footnotes against its bottom, rule above the first.
    pub fn finish(&mut self, doc: &Document, story: &Story, frames: &[FrameSpec], cols: &[Vec<Rect>], out: &mut ComposedStory) {
        if self.placed.is_empty() {
            return;
        }
        let o = doc.note_options(self.series);
        let mut groups: Vec<(usize, usize)> = self.placed.iter().map(|p| (p.fi, if self.span { 0 } else { p.col })).collect();
        groups.dedup();
        groups.sort_unstable();
        groups.dedup();
        for (fi, ci) in groups {
            let (Some(_), Some(col)) = (frames.get(fi), cols.get(fi).filter(|c| ci < c.len()).and_then(|c| self.area(c, ci))) else { continue };
            let here: Vec<&Placed> = self.placed.iter().filter(|p| self.same_area(p, fi, ci)).collect();
            let total = stack_height(here.iter().map(|p| &p.m), o.space_between);
            let top = col.y1 - total;
            let Some(ft) = out.frames.get_mut(fi) else { continue };
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
                    series: self.series,
                    reference_baseline: 0.0,
                    overflow: false,
                    index: p.k,
                    column: p.col as u32,
                    label: o.label(self.numbers.get(p.k).copied().unwrap_or(o.start_at), true),
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

mod series;
pub(crate) use series::{NoteSnapshot, Notes, SplitPlan as SeriesSplitPlan};

/// The placed footnote with id `id`: (frame index, footnote).
pub fn find_note(cs: &ComposedStory, id: u64) -> Option<(usize, &PlacedNote)> {
    cs.frames.iter().enumerate().find_map(|(fi, ft)| ft.notes.iter().find(|n| n.id == id).map(|n| (fi, n)))
}

/// The footnote's text with every placed part in placement order.
/// Caret commands use it so line movement crosses parts like body frames;
/// each frame stays in its part's local coordinates.
pub fn note_text(cs: &ComposedStory, id: u64) -> Option<ComposedStory> {
    let mut parts = cs.frames.iter().flat_map(|ft| &ft.notes).filter(|n| n.id == id);
    let mut text = (*parts.next()?.text).clone();
    text.frames = text.frames.first().cloned().into_iter().chain(parts.filter_map(|n| n.text.frames.first().cloned())).collect();
    Some(text)
}

/// Find the footnote part holding byte `pos` for caret movement and selection drawing.
/// The story-end caret belongs to the last part.
pub fn find_note_part(cs: &ComposedStory, id: u64, pos: usize) -> Option<(usize, &PlacedNote)> {
    let mut last = None;
    for (fi, ft) in cs.frames.iter().enumerate() {
        for n in ft.notes.iter().filter(|n| n.id == id) {
            let range = &n.text.frames.first()?.range;
            if range.contains(&pos) {
                return Some((fi, n));
            }
            if pos == range.end {
                last = Some((fi, n));
            }
        }
    }
    last
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
    let (fi, n) = find_note_part(cs, id, pos)?;
    match crate::caret(&n.text, pos) {
        Some((_, x, bl, a, d)) => Some((fi, x + n.origin.x, bl + n.origin.y, a, d)),
        None => Some((fi, n.origin.x, n.origin.y + 10.0, 9.0, 3.0)),
    }
}

mod page_margins;
pub(crate) use page_margins::stack_page_margins;

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::{ParaFormat, SpreadRef, build::NewDocument, story};

    #[test]
    fn note_cache_distinguishes_parent_prefixes_in_current_and_earlier_parts() {
        let mut doc = Document::new(&NewDocument::default());
        let (_, sid) =
            doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 100.0), doc.default_layer(), "Body", ParaFormat::default()).unwrap();
        let text = vec![story::PAGE_NUMBER.to_string(); 8].join("\n");
        doc.story_mut(sid).unwrap().insert_note(0, &text, ParaFormat::default()).unwrap();
        let mut notes = SeriesNotes::new(&doc, doc.story(sid).unwrap(), 0, false);
        let mut frame = crate::frame_specs(&doc, sid).remove(0);
        frame.page = None;
        frame.parent = true;
        frame.page_name = Some("A".into());
        let opts = ComposeOptions::default();
        let visible = |text: &ComposedStory| -> String {
            text.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).filter(|g| g.visible).map(|g| g.rendered_char).collect()
        };
        let first = notes.compose_parts(&doc, 0, 200.0, &frame, &opts, &[], &[]);
        assert!(visible(&first[0].as_ref().unwrap().0).contains('A'));
        let earlier_a = PartPage::of(&frame);
        frame.page_name = Some("B".into());
        let second = notes.compose_parts(&doc, 0, 200.0, &frame, &opts, &[], &[]);
        assert!(visible(&second[0].as_ref().unwrap().0).contains('B'), "parent B must not reuse parent A's markers");
        let earlier_b = PartPage::of(&frame);
        frame.page_name = Some("C".into());
        let heights = [(200.0, 30.0)];
        let first = notes.compose_parts(&doc, 0, 200.0, &frame, &opts, &heights, &[earlier_a]);
        assert!(visible(&first[0].as_ref().unwrap().0).contains('A'));
        let second = notes.compose_parts(&doc, 0, 200.0, &frame, &opts, &heights, &[earlier_b]);
        assert!(visible(&second[0].as_ref().unwrap().0).contains('B'), "earlier parent B must not reuse parent A's markers");
    }
}
