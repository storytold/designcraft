//! A page trial is isolated from the accepted route state, so moving an anchoring line back
//! restores both the note numbering and its continuation without consuming any text.

use super::{
    Arc, Carry, ComposeOptions, ComposedStory, Document, FOOTNOTE_REF, FrameSpec, HashMap, NoteColumn, PlacedNote, Point, Range, Rect, SeriesNotes,
    Story, SubstCtx,
};
use crate::FrameText;
use designcraft_doc::NotePlacement;

#[derive(Clone, Default)]
pub struct StreamState {
    series: HashMap<u64, StreamSeriesState>,
}

impl StreamState {
    pub fn pending_series(&self) -> impl Iterator<Item = u64> + '_ {
        self.series.iter().filter_map(|(id, s)| s.carry.is_some().then_some(*id))
    }
}

#[derive(Clone)]
struct StreamSeriesState {
    num: u32,
    key: Option<usize>,
    carry: Option<Carry>,
    carry_number: u32,
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
        .filter_map(|f| f.opts.note_stream.map(|s| s.series))
        .chain(before.series.keys().copied())
        .filter(|id| matches!(doc.note_options(*id).placement, NotePlacement::Stream))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    for series in ids {
        let mut notes = SeriesNotes::new(doc, story, series, false);
        let allow_split = doc.note_options(series).allow_split;
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
        notes.number_refs(doc, input.main.clone(), Some(input.page), &mut SubstCtx::default());
        let anchors: Vec<_> = notes.refs.iter().enumerate().filter_map(|(k, at)| (notes.owns(k) && input.main.contains(at)).then_some(k)).collect();
        let had_carry = notes.carry.is_some();
        let mut next = 0;
        for (fi, frame) in input.areas.iter().enumerate().filter(|(_, f)| f.opts.note_stream.is_some_and(|s| s.series == series)) {
            for (ci, area) in frame.columns().iter().enumerate() {
                let column = NoteColumn { frame_index: fi.saturating_add(1), column_index: ci, width: area.width(), frame };
                let room = area.height();
                if notes.carry.is_some() {
                    if notes.reserve(doc, column, &[], opts) <= room + 0.01 {
                        notes.place_line(doc, column, &[], None, opts);
                    } else if allow_split
                        && let Some(plan) =
                            notes.split_plan(doc, column, &[], room - doc.note_options(series).space_before, opts).filter(respects_keeps)
                    {
                        notes.place_line(doc, column, &[], Some(plan), opts);
                    }
                    if notes.carry.is_some() {
                        continue;
                    }
                }
                while let Some(&k) = anchors.get(next) {
                    if notes.reserve(doc, column, &[k], opts) <= room + 0.01 {
                        notes.place_line(doc, column, &[k], None, opts);
                    } else if allow_split
                        && let Some(plan) =
                            notes.split_plan(doc, column, &[k], room - doc.note_options(series).space_before, opts).filter(respects_keeps)
                    {
                        notes.place_line(doc, column, &[k], Some(plan), opts);
                    } else if !had_carry && notes.refs.get(k).is_some_and(|at| input.force_starts.as_ref().is_some_and(|range| range.contains(at))) {
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
                    next += 1;
                    if notes.carry.is_some() {
                        break;
                    }
                }
            }
        }
        if let Some(&k) = anchors.get(next) {
            result.starts_fit = false;
            let anchor = notes.refs.get(k).copied();
            result.failed_anchor = match (result.failed_anchor, anchor) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        let carry_number = notes.carry.as_ref().and_then(|c| notes.numbers.get(c.k)).copied().unwrap_or(0);
        result.state.series.insert(series, StreamSeriesState { num: notes.num, key: notes.key, carry: notes.carry.clone(), carry_number });
        let o = doc.note_options(series);
        for (fi, frame) in input.areas.iter().enumerate() {
            for (ci, area) in frame.columns().iter().enumerate() {
                let mut previous: Option<(f64, f64)> = None;
                for part in notes.placed.iter().filter(|p| p.fi == fi.saturating_add(1) && p.col == ci) {
                    let Some(note) = story.notes.get(part.k) else { continue };
                    let y = previous.map_or(area.y0 + o.space_before, |(baseline, _)| baseline + o.space_between + part.m.lead - part.m.first);
                    let bottom = y + part.m.last + part.m.descent;
                    let overflow = bottom > area.y1 + 0.01;
                    let top = previous.map_or(y, |(_, bottom)| bottom + o.space_between);
                    if let Some(ft) = result.frames.get_mut(fi) {
                        ft.notes.push(PlacedNote {
                            id: note.id,
                            index: part.k,
                            series,
                            reference_baseline: 0.0,
                            overflow,
                            column: ci as u32,
                            label: o.label(notes.numbers.get(part.k).copied().unwrap_or(o.start_at), true),
                            origin: Point::new(area.x0, y),
                            rect: Rect::new(area.x0, top, area.x1, bottom),
                            text: part.text.clone(),
                            source: Arc::from(note.text.text.as_str()),
                        });
                        ft.content_height = ft.content_height.max(bottom - frame.area.y0);
                    }
                    previous = Some((y + part.m.last, bottom));
                }
            }
        }
    }
    result
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
    anchors: Vec<Anchors>,
    pending: Option<Anchors>,
}

impl StreamComposer {
    pub(super) fn new(doc: &Document, story: &Story, frames: &[FrameSpec]) -> Self {
        let routes = if frames.iter().any(|f| f.page_rect.is_some()) { crate::stream_frame_specs(doc, story.id) } else { Vec::new() };
        Self {
            host: story.id,
            routes,
            host_pages: frames.iter().map(|f| f.page).collect(),
            refs: story.text.match_indices(FOOTNOTE_REF).map(|(at, _)| at).collect(),
            anchors: Vec::new(),
            pending: None,
        }
    }

    pub(super) fn has_route(&self, series: u64) -> bool {
        self.routes.iter().any(|f| f.opts.note_stream.is_some_and(|s| s.series == series))
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
        let can_force = !has_main && !fit.had_continuation;
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
