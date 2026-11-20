//! A page trial is isolated from the accepted route state, so moving an anchoring line back
//! restores both the note numbering and its continuation without consuming any text.

use super::{
    Arc, Carry, ComposeOptions, ComposedStory, Document, FOOTNOTE_REF, FrameSpec, HashMap, NoteColumn, PlacedNote, Point, Range, Rect, SeriesNotes,
    Story, SubstCtx,
};
use crate::FrameText;
use designcraft_doc::NotePlacement;

#[derive(Clone, Default, Debug)]
pub struct StreamState {
    series: HashMap<u64, StreamSeriesState>,
}

impl StreamState {
    pub(crate) fn progress(&self) -> Vec<(u64, usize, usize, u32, Option<usize>)> {
        let mut progress: Vec<_> = self
            .series
            .iter()
            .map(|(id, s)| (*id, s.carry.as_ref().map_or(usize::MAX, |c| c.k), if s.carry.is_some() { s.offset } else { 0 }, s.num, s.key))
            .collect();
        progress.sort_unstable();
        progress
    }
    pub(crate) fn pending_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.series.values().filter_map(|s| s.carry.as_ref().map(|c| c.k))
    }
    pub(crate) fn progress_from(&self, before: &Self) -> bool {
        before.series.iter().any(|(id, old)| {
            old.carry.is_some()
                && self.series.get(id).is_some_and(|new| {
                    new.carry.is_none() || new.carry.as_ref().map(|c| c.k) != old.carry.as_ref().map(|c| c.k) || new.offset > old.offset
                })
        })
    }
    pub(crate) fn take_series(&mut self, other: &Self, id: u64) {
        if let Some(state) = other.series.get(&id) {
            self.series.insert(id, state.clone());
        }
    }

    pub fn pending_series(&self) -> impl Iterator<Item = u64> + '_ {
        self.series.iter().filter_map(|(id, s)| s.carry.is_some().then_some(*id))
    }
}

#[derive(Clone, Debug)]
struct StreamSeriesState {
    num: u32,
    key: Option<usize>,
    carry: Option<Carry>,
    carry_number: u32,
    offset: usize,
}

fn respects_keeps(plan: &super::SplitPlan) -> bool {
    // Ordinary frames may break an impossible keep at their top. A stream can move its
    // anchoring line instead; only the explicit empty-page progress fallback may overflow.
    !plan.text.frames.iter().flat_map(|frame| &frame.lines).any(|line| line.keep_violation)
}

pub struct StreamPageInput<'a> {
    pub page: usize,
    /// Bytes of main text actually placed by the caller's candidate page layout.
    pub main: Range<usize>,
    /// This page's stream areas, each naming its host and series in `note_stream`.
    pub areas: &'a [FrameSpec],
    /// The page's first main line, when progress needs an impossible comment to overflow.
    /// Only anchors in this range may overflow instead of moving through every later page.
    pub force_starts: Option<Range<usize>>,
}

pub struct StreamPageFit {
    pub state: StreamState,
    pub frames: Vec<FrameText>,
    /// Every newly anchored comment placed its first part. The caller separately checks that
    /// its main text candidate fitted; this function never changes the candidate's byte cut.
    pub starts_fit: bool,
    pub failed_anchor: Option<usize>,
    pub had_continuation: bool,
    pub warnings: Vec<String>,
}

/// Fit one page's comments against an already measured main-text cut. Rejected candidates
/// leave `before` untouched. Earlier part dimensions are replayed through the original split
/// composer, which retains source byte ranges and the note paragraphs' keep constraints.
pub fn fit_stream_page(doc: &Document, story: &Story, input: StreamPageInput<'_>, before: &StreamState, opts: &ComposeOptions) -> StreamPageFit {
    fit_stream_page_for(doc, story, input, before, opts, None)
}

pub(crate) fn fit_stream_page_for(
    doc: &Document,
    story: &Story,
    input: StreamPageInput<'_>,
    before: &StreamState,
    opts: &ComposeOptions,
    only: Option<u64>,
) -> StreamPageFit {
    let ranges = [input.main.clone()];
    fit_stream_page_ranges(doc, story, input, before, opts, only, &ranges)
}

/// Fit the union of host visits to one physical page without consuming other pages' anchors.
pub(crate) fn fit_stream_page_ranges(
    doc: &Document,
    story: &Story,
    input: StreamPageInput<'_>,
    before: &StreamState,
    opts: &ComposeOptions,
    only: Option<u64>,
    ranges: &[Range<usize>],
) -> StreamPageFit {
    let selected = only.map(|id| [id]);
    fit_stream_page_dynamic(doc, story, input, before, opts, (selected.as_ref().map(|ids| ids.as_slice()), ranges), |_, _, _, _| {})
}

/// Adjust each route using already fitted predecessors, while keeping each series cursor alive.
pub(crate) fn fit_stream_page_dynamic(
    doc: &Document,
    story: &Story,
    input: StreamPageInput<'_>,
    before: &StreamState,
    opts: &ComposeOptions,
    selection: (Option<&[u64]>, &[Range<usize>]),
    mut adjust: impl FnMut(usize, &mut [FrameSpec], &[FrameText], &mut Vec<String>),
) -> StreamPageFit {
    let (only, ranges) = selection;
    let mut result = StreamPageFit {
        state: before.clone(),
        frames: input.areas.iter().map(|f| FrameText { frame: f.id, columns: f.columns(), ..Default::default() }).collect(),
        starts_fit: true,
        failed_anchor: None,
        had_continuation: before.series.values().any(|s| s.carry.is_some()),
        warnings: Vec::new(),
    };
    let mut ids: Vec<_> = crate::stream_frame_specs(doc, story.id)
        .iter()
        .chain(input.areas)
        .filter_map(|f| f.opts.note_stream.as_ref().map(|s| s.series))
        .chain(before.series.keys().copied())
        .filter(|id| only.is_none_or(|wanted| wanted.contains(id)) && matches!(doc.note_options(*id).placement, NotePlacement::Stream))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut contexts = HashMap::new();
    for series in ids {
        let mut notes = SeriesNotes::new(doc, story, series, false);
        // Stream columns are a route, even when the series spans ordinary column feet.
        notes.span = false;
        if let Some(previous) = before.series.get(&series) {
            notes.num = previous.num;
            notes.key = previous.key;
            // pending() accepts only forward areas. Rebase the incoming part before this page,
            // whose local area indices start at one; keep the original part dimensions intact.
            notes.carry = previous.carry.clone().map(|c| Carry { fi: 0, col: 0, ..c });
            if let Some(c) = &notes.carry
                && let Some(number) = notes.numbers.get_mut(c.k)
            {
                *number = previous.carry_number;
            }
        }
        for range in ranges {
            notes.number_refs(doc, range.clone(), Some(input.page), &mut SubstCtx::default());
        }
        let anchors: Vec<_> =
            notes.refs.iter().enumerate().filter_map(|(k, at)| (notes.owns(k) && ranges.iter().any(|r| r.contains(at))).then_some(k)).collect();
        if opts.note_bands {
            notes.band_incoming = notes.carry.as_ref().map(|c| (c.k, c.heights.len()));
        }
        let had_carry = notes.carry.is_some();
        contexts.insert(series, (notes, anchors, 0usize, had_carry, None));
    }
    let mut sources = HashMap::new();
    let mut areas = input.areas.to_vec();
    for fi in 0..areas.len() {
        adjust(fi, &mut areas, &result.frames, &mut result.warnings);
        let Some(frame) = areas.get(fi) else { continue };
        let Some(series) = frame.opts.note_stream.as_ref().map(|s| s.series) else { continue };
        let Some((notes, anchors, next, had_carry, previous)) = contexts.get_mut(&series) else { continue };
        let allow_split = doc.note_options(series).allow_split;
        if let Some(out) = result.frames.get_mut(fi) {
            out.columns = frame.columns();
        }
        for (ci, area) in frame.columns().iter().enumerate() {
            // An inset-follow edge can close an area; even forced starts must skip it.
            if area.height() <= 0.0 || area.width() <= 0.0 {
                continue;
            }
            let column = NoteColumn { frame_index: fi.saturating_add(1), column_index: ci, width: area.width(), frame };
            if !opts.note_bands {
                *previous = None;
            }
            notes.band_previous = previous.map(|(baseline, _)| baseline);
            notes.band_column = opts.note_bands.then_some((fi.saturating_add(1), ci));
            notes.band_last = None;
            let placed_start = notes.placed.len();
            notes.band_span = opts.note_bands.then_some(area.height());
            notes.band_continued = opts.note_bands && notes.band_previous.is_some() && notes.carry.is_some();
            let room = area.height();
            if notes.carry.is_some() {
                let available = room - notes.area_space(doc, column, &[], opts);
                if notes.reserve(doc, column, &[], opts) <= room + 0.01 {
                    notes.place_line(doc, column, &[], None, opts);
                } else if allow_split && let Some(plan) = notes.split_plan(doc, column, &[], available, opts).filter(respects_keeps) {
                    notes.place_line(doc, column, &[], Some(plan), opts);
                }
            }
            while notes.carry.is_none()
                && let Some(&k) = anchors.get(*next)
            {
                let available = room - notes.area_space(doc, column, &[k], opts);
                if notes.reserve(doc, column, &[k], opts) <= room + 0.01 {
                    notes.place_line(doc, column, &[k], None, opts);
                } else if allow_split && let Some(plan) = notes.split_plan(doc, column, &[k], available, opts).filter(respects_keeps) {
                    notes.place_line(doc, column, &[k], Some(plan), opts);
                } else if !*had_carry && notes.refs.get(k).is_some_and(|at| input.force_starts.as_ref().is_some_and(|range| range.contains(at))) {
                    notes.place(doc, k, column, opts);
                    if let Some(note) = story.notes.get(k) {
                        result.warnings.push(format!(
                            "Stream note {} in series {series} cannot start within its empty area on page {}",
                            note.id,
                            input.page.saturating_add(1)
                        ));
                    }
                } else {
                    break;
                }
                *next += 1;
                if notes.carry.is_some() {
                    break;
                }
            }
            for part in placed_parts(doc, story, notes, &notes.placed[placed_start..], *area, ci, opts.note_bands, previous, &mut sources) {
                if let Some(out) = result.frames.get_mut(fi) {
                    out.content_height = out.content_height.max(part.rect.y1 - frame.area.y0);
                    out.notes.push(part);
                }
            }
        }
    }
    for (series, (notes, anchors, next, _, _)) in contexts {
        if let Some(&k) = anchors.get(next) {
            result.starts_fit = false;
            let anchor = notes.refs.get(k).copied();
            result.failed_anchor = match (result.failed_anchor, anchor) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        let carry_number = notes.carry.as_ref().and_then(|c| notes.numbers.get(c.k)).copied().unwrap_or(0);
        let offset = notes
            .placed
            .last()
            .and_then(|p| p.text.frames.last())
            .map(|f| f.range.end)
            .or_else(|| before.series.get(&series).map(|s| s.offset))
            .unwrap_or(0);
        result.state.series.insert(series, StreamSeriesState { offset, num: notes.num, key: notes.key, carry: notes.carry.clone(), carry_number });
    }
    result
}

#[cfg(test)]
thread_local! {
    pub(crate) static PART_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[allow(clippy::too_many_arguments)]
fn placed_parts(
    doc: &Document,
    story: &Story,
    notes: &SeriesNotes,
    parts: &[super::Placed],
    area: Rect,
    column: usize,
    bands: bool,
    previous: &mut Option<(f64, f64)>,
    sources: &mut HashMap<usize, Arc<str>>,
) -> Vec<PlacedNote> {
    let o = doc.note_options(notes.series);
    let mut out = Vec::new();
    let mut first = true;
    for part in parts {
        #[cfg(test)]
        PART_VISITS.with(|n| n.set(n.get() + 1));
        let Some(note) = story.notes.get(part.k) else { continue };
        let y = if bands && first && part.part > 0 && previous.is_some() {
            area.y0
        } else {
            previous.map_or(area.y0 + o.space_before, |(baseline, _)| baseline + o.space_between + part.m.lead - part.m.first)
        };
        let bottom = y + part.m.last + part.m.descent;
        let top = if first { y } else { previous.map_or(y, |(_, bottom)| bottom + o.space_between) };
        out.push(PlacedNote {
            id: note.id,
            index: part.k,
            series: notes.series,
            reference_baseline: 0.0,
            overflow: bottom > area.y1 + 0.01,
            column: column as u32,
            label: o.label(notes.numbers.get(part.k).copied().unwrap_or(o.start_at), true),
            origin: Point::new(area.x0, y),
            rect: Rect::new(area.x0, top, area.x1, bottom),
            text: part.text.clone(),
            source: sources.entry(part.k).or_insert_with(|| Arc::from(note.text.text.as_str())).clone(),
        });
        *previous = Some((y + part.m.last, bottom));
        first = false;
    }
    out
}

#[derive(Clone)]
struct Anchors {
    page: usize,
    range: Range<usize>,
    forced: bool,
}

/// Paragraph snapshots retain the accepted log length. Replaying the log reconstructs route
/// carries with the shared split composer, including route pages that have no main frame.
pub(super) struct StreamComposer {
    host: designcraft_doc::StoryId,
    routes: Vec<FrameSpec>,
    host_pages: Vec<Option<usize>>,
    refs: Vec<usize>,
    inset_series: Vec<u64>,
    anchors: Vec<Anchors>,
    pending: Option<Anchors>,
}

impl StreamComposer {
    pub(super) fn new(doc: &Document, story: &Story, frames: &[FrameSpec]) -> Self {
        let routes = if frames.iter().any(|f| f.page_rect.is_some()) { crate::stream_frame_specs(doc, story.id) } else { Vec::new() };
        Self {
            inset_series: frames
                .iter()
                .filter_map(|f| f.opts.inset_columns.as_ref())
                .flat_map(|c| c.streams.iter().filter_map(|s| s.series))
                .collect(),
            host: story.id,
            routes,
            host_pages: frames.iter().map(|f| f.page).collect(),
            refs: story.text.match_indices(FOOTNOTE_REF).map(|(at, _)| at).collect(),
            anchors: Vec::new(),
            pending: None,
        }
    }

    pub(super) fn has_route(&self, series: u64) -> bool {
        self.inset_series.contains(&series) || self.routes.iter().any(|f| f.opts.note_stream.as_ref().is_some_and(|s| s.series == series))
    }

    pub(super) fn snapshot(&self) -> usize {
        self.anchors.len()
    }

    pub(super) fn restore(&mut self, len: usize) {
        self.anchors.truncate(len);
        self.pending = None;
    }

    fn replay(&self, doc: &Document, anchors: &[Anchors], until: Option<usize>, opts: &ComposeOptions) -> Vec<StreamPageFit> {
        let Some(story) = doc.story(self.host) else { return Vec::new() };
        let mut pages: Vec<_> = self.routes.iter().filter_map(|f| f.page).chain(anchors.iter().map(|a| a.page)).collect();
        pages.sort_unstable();
        pages.dedup();
        let mut state = StreamState::default();
        let mut fits = Vec::new();
        for page in pages.into_iter().filter(|p| until.is_none_or(|limit| *p <= limit)) {
            let ranges: Vec<_> = anchors.iter().filter(|a| a.page == page).collect();
            let main = ranges.iter().map(|a| a.range.clone()).reduce(|a, b| a.start.min(b.start)..a.end.max(b.end)).unwrap_or(0..0);
            let force_starts = ranges.iter().filter(|a| a.forced).map(|a| a.range.clone()).reduce(|a, b| a.start.min(b.start)..a.end.max(b.end));
            let areas: Vec<_> = self.routes.iter().filter(|f| f.page == Some(page)).cloned().collect();
            let fit = fit_stream_page(doc, story, StreamPageInput { page, main, areas: &areas, force_starts }, &state, opts);
            state = fit.state.clone();
            fits.push(fit);
        }
        fits
    }

    pub(super) fn fits(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], out: &ComposedStory, opts: &ComposeOptions) -> bool {
        self.pending = None;
        let (Some(page), Some(story)) = (column.frame.page, doc.story(self.host)) else { return true };
        let range = extra
            .iter()
            .filter(|k| {
                story.notes.get(**k).is_some_and(|note| {
                    let series = doc.note_series_id(note.series);
                    self.has_route(series) && matches!(doc.note_options(series).placement, NotePlacement::Stream)
                })
            })
            .filter_map(|k| self.refs.get(*k).map(|at| *at..at.saturating_add(FOOTNOTE_REF.len_utf8())))
            .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end));
        let Some(range) = range else { return true };
        let mut anchors = self.anchors.clone();
        anchors.push(Anchors { page, range, forced: false });
        let Some(fit) = self.replay(doc, &anchors, Some(page), opts).pop() else { return true };
        let has_main = out.frames.iter().zip(&self.host_pages).any(|(frame, p)| *p == Some(page) && !frame.lines.is_empty());
        // Missing routes can be reached on a later page; forcing is only for an existing
        // empty area that cannot fit its first comment.
        let routes_present = extra
            .iter()
            .filter_map(|k| story.notes.get(*k))
            .filter(|note| matches!(doc.note_options(note.series).placement, NotePlacement::Stream))
            .all(|note| {
                let series = doc.note_series_id(note.series);
                !self.has_route(series)
                    || self.routes.iter().any(|f| f.page == Some(page) && f.opts.note_stream.as_ref().is_some_and(|s| s.series == series))
            });
        let can_force = !has_main && !fit.had_continuation && routes_present;
        if !fit.starts_fit && !can_force {
            return false;
        }
        if let Some(last) = anchors.last_mut() {
            last.forced = !fit.starts_fit && can_force;
            self.pending = Some(last.clone());
        }
        true
    }

    pub(super) fn place_line(&mut self) {
        if let Some(anchors) = self.pending.take() {
            self.anchors.push(anchors);
        }
    }

    pub(super) fn finish(&self, doc: &Document, story: &Story, opts: &ComposeOptions, out: &mut ComposedStory) {
        let mut fits = self.replay(doc, &self.anchors, None, opts);
        let pending: Vec<_> = fits.last().map(|fit| fit.state.pending_series().collect()).unwrap_or_default();
        let mut frames: Vec<_> = fits.iter_mut().flat_map(|fit| std::mem::take(&mut fit.frames)).collect();
        for series in pending {
            if let Some(note) = frames.iter_mut().rev().flat_map(|frame| frame.notes.iter_mut().rev()).find(|note| note.series == series) {
                note.overflow = true;
            }
            out.warnings.push(format!("Stream overflow in series {series}: commentary remains after the last stream frame"));
        }
        for fit in fits {
            out.warnings.extend(fit.warnings);
            if !fit.starts_fit {
                out.warnings
                    .push(format!("Stream overflow: a comment anchored at byte {} could not start on its page", fit.failed_anchor.unwrap_or(0)));
            }
        }
        for note in frames.iter_mut().flat_map(|frame| &mut frame.notes) {
            if let Some(at) = self.refs.get(note.index) {
                note.reference_baseline =
                    out.frames.iter().flat_map(|frame| &frame.lines).find(|line| line.range.contains(at)).map_or(0.0, |line| line.baseline);
            }
        }
        for note in &story.notes {
            let series = doc.note_series_id(note.series);
            if matches!(doc.note_options(series).placement, NotePlacement::Stream) && !self.has_route(series) {
                let warning = format!("Stream series {series} has no route; its notes use the column foot");
                if !out.warnings.contains(&warning) {
                    out.warnings.push(warning);
                }
            }
        }
        out.frames.extend(frames);
    }
}
