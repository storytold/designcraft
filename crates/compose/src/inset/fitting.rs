//! Page trials keep original stories intact and replay accepted whole-line rectangles.
use super::{Config, MainPosition, Measurement, Stream, layout_with_state};
use crate::{ComposeOptions, ComposedStory, FrameSpec, FrameText, StreamPageInput, StreamState};
use designcraft_doc::{Document, InsetColumns, InsetStream, NoteStream, Story};
use designcraft_geom::Rect;
use std::sync::Arc;

#[derive(Clone)]
struct Cursor {
    areas: Vec<FrameSpec>,
    cut: usize,
    state: StreamState,
    starts_fit: bool,
    warnings: Vec<String>,
    first_line: Option<usize>,
    tail_pending: bool,
    catchword: Option<Rect>,
    main_bottom: f64,
    catchword_space: f64,
}
struct Trial {
    core_bottom: f64,
    first_line: Option<usize>,
    cut: usize,
    areas: Vec<FrameSpec>,
    notes: Vec<crate::PlacedNote>,
    routes: Vec<FrameText>,
    state: StreamState,
    feasible: bool,
    warnings: Vec<String>,
    catchwords: Vec<(Option<u64>, Rect)>,
    route_catchwords: Vec<(FrameSpec, Rect)>,
}

/// A catchword has its own row, so a neighbour cannot reclaim the space beneath its source.
pub(super) const CATCHWORD_SPACE: f64 = 18.0;

fn tail(rect: Rect, height: f64, finished: bool, needed: bool, bottom: bool, space: f64) -> (f64, bool, bool, Option<Rect>) {
    if needed && (finished || bottom) {
        if height + space <= rect.height() + 0.01 {
            let area = Rect::new(rect.x0, rect.y0 + height, rect.x1, rect.y0 + height + space);
            // The footer closes this stream's slice on this page even when its source
            // continues. Keep its carry for the next page, rather than replaying the row.
            (height + space, true, false, Some(area))
        } else {
            (height, false, finished, None)
        }
    } else {
        (height, finished, false, None)
    }
}

fn end(story: &Story, line: &crate::Line) -> usize {
    let at = line.range.end;
    if line.last_in_para && story.text.get(at..).is_some_and(|s| s.starts_with('\n')) { at.saturating_add(1) } else { at }
}

fn main_options(opts: &ComposeOptions, frames: &[FrameSpec]) -> ComposeOptions {
    ComposeOptions {
        inset_measure: true,
        continuations: frames
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| pair.first().and_then(|f| f.page) == pair.get(1).and_then(|f| f.page))
            .map(|(i, _)| (i.saturating_add(1), 0.0))
            .collect(),
        ..opts.clone()
    }
}

fn main_layout(doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions) -> ComposedStory {
    #[cfg(test)]
    MAIN_LAYOUTS.with(|n| n.set(n.get() + 1));
    crate::compose(doc, story, frames, &main_options(opts, frames))
}

/// Local text y -> spread y. Slanted or flipped axes cannot express a horizontal follow edge.
fn frame_y_transform(doc: &Document, id: designcraft_doc::ItemId) -> Option<(f64, f64)> {
    let loc = doc.find(id)?;
    let item = doc.item_at(&loc)?;
    let [sx, b, c, sy, tx, ty] = (doc.parent_xf(&loc) * doc.text_xf(item)).as_coeffs();
    (b == 0.0 && c == 0.0 && sx > 0.0 && sy > 0.0 && [sx, b, c, sy, tx, ty].iter().all(|n| n.is_finite())).then_some((sy, ty))
}

fn route_edge(doc: &Document, host: designcraft_doc::ItemId, route: designcraft_doc::ItemId, bottom: f64, gap: f64) -> Option<f64> {
    let (host_scale, host_y) = frame_y_transform(doc, host)?;
    let (route_scale, route_y) = frame_y_transform(doc, route)?;
    // Add the gap in spread points, then apply the route's axis-aligned inverse.
    let edge = (bottom * host_scale + host_y + gap - route_y) / route_scale;
    edge.is_finite().then_some(edge)
}

pub(super) fn route_dependencies(areas: &[FrameSpec]) -> Vec<Option<Option<usize>>> {
    let indices: std::collections::HashMap<_, _> = areas.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
    let after: Vec<_> = areas.iter().map(|f| f.opts.note_stream.as_ref().and_then(|s| s.follow_inset).and_then(|f| f.after)).collect();
    // 0 = unvisited, 1 = current path, 2 = valid chain, 3 = reaches a cycle.
    let mut status = vec![0_u8; areas.len()];
    for root in 0..areas.len() {
        let mut path = Vec::new();
        let mut next = Some(root);
        let valid = loop {
            let Some(index) = next else { break true };
            match status.get(index).copied() {
                Some(0) => {
                    #[cfg(test)]
                    DEPENDENCY_VISITS.with(|n| n.set(n.get() + 1));
                    if let Some(state) = status.get_mut(index) {
                        *state = 1;
                    }
                    path.push(index);
                    next = after.get(index).copied().flatten().and_then(|id| indices.get(&id).copied());
                }
                Some(2) | None => break true,
                _ => break false,
            }
        };
        for index in path {
            if let Some(state) = status.get_mut(index) {
                *state = if valid { 2 } else { 3 };
            }
        }
    }
    after
        .into_iter()
        .enumerate()
        .map(|(index, after)| after.map(|id| indices.get(&id).copied().filter(|previous| *previous < index && status.get(index) == Some(&2))))
        .collect()
}

struct RouteGeometry<'a> {
    core: Option<(designcraft_doc::ItemId, f64)>,
    catchwords: Option<&'a InsetColumns>,
    routes: &'a [FrameSpec],
    later_ranges: Option<&'a [std::ops::Range<usize>]>,
}

fn route_catchword_rect(frame: &FrameText, reserved: Rect) -> Option<Rect> {
    let bottom = frame
        .notes
        .iter()
        .flat_map(|note| note.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |line| note.origin.y + line.baseline + line.descent)))
        .reduce(f64::max)?;
    let end = bottom + reserved.height();
    (end <= reserved.y1 + 0.01).then_some(Rect::new(reserved.x0, bottom, reserved.x1, end))
}

fn fit_routes(
    doc: &Document,
    story: &Story,
    geometry: RouteGeometry<'_>,
    input: StreamPageInput<'_>,
    before: &StreamState,
    opts: &ComposeOptions,
    ranges: &[std::ops::Range<usize>],
) -> (crate::StreamPageFit, Vec<(FrameSpec, Rect)>) {
    let mut series: Vec<_> = geometry.routes.iter().filter_map(|f| f.opts.note_stream.as_ref().map(|s| s.series)).collect();
    series.sort_unstable();
    series.dedup();
    let dependencies = route_dependencies(input.areas);
    let fit = |rows: &std::collections::HashMap<designcraft_doc::ItemId, f64>| {
        #[cfg(test)]
        ROUTE_AREAS.with(|n| n.set(n.get() + input.areas.len()));
        let mut reserved = std::collections::HashMap::<designcraft_doc::ItemId, Rect>::new();
        let result = crate::notes::fit_stream_page_dynamic(
            doc,
            story,
            StreamPageInput { page: input.page, main: input.main.clone(), areas: input.areas, force_starts: input.force_starts.clone() },
            before,
            opts,
            (Some(&series), ranges),
            |index, areas, fitted, warnings| {
                let apply_follow = |areas: &mut [FrameSpec], warnings: &mut Vec<String>| {
                    let Some(spec) = areas.get(index) else { return };
                    let id = spec.id;
                    let Some(follow) = spec.opts.note_stream.as_ref().and_then(|s| s.follow_inset).filter(|f| f.valid()) else { return };
                    let dependency = dependencies.get(index).copied().flatten();
                    if dependency == Some(None) {
                        warnings.push(format!(
                            "followInset ignored on frame {}: after must name an earlier route of the same story and page without a cycle",
                            id
                        ));
                        return;
                    }
                    let Some((host, bottom)) = geometry.core else { return };
                    let reference = dependency.flatten().and_then(|previous| {
                        let spec = areas.get(previous)?;
                        let frame = fitted.get(previous)?;
                        let bottom = frame.notes.iter().map(|n| n.rect.y1).fold(spec.area.y0, f64::max);
                        let bottom =
                            reserved.get(&spec.id).and_then(|rect| route_catchword_rect(frame, *rect)).map_or(bottom, |rect| bottom.max(rect.y1));
                        route_edge(doc, spec.id, id, bottom, follow.gap)
                    });
                    let Some(edge) = route_edge(doc, host, id, bottom, follow.gap).filter(|_| dependency.is_none() || reference.is_some()) else {
                        warnings.push(format!(
                "followInset ignored on frame {}: host and route transforms must allow finite translation and positive axis-aligned scaling",
                id
            ));
                        return;
                    };
                    let edge = reference.map_or(edge, |r| r.max(edge));
                    let Some(area) = areas.get_mut(index) else { return };
                    match follow.edge {
                        designcraft_doc::InsetEdge::End => area.area.y1 = area.area.y1.min(edge).max(area.area.y0),
                        designcraft_doc::InsetEdge::Start => area.area.y0 = area.area.y0.max(edge).min(area.area.y1),
                    }
                };
                apply_follow(areas, warnings);
                if let Some(area) = areas.get_mut(index)
                    && let Some(height) = rows.get(&area.id)
                {
                    let bottom = area.area.y1;
                    area.area.y1 = (bottom - height).max(area.area.y0);
                    reserved.insert(area.id, Rect::new(area.area.x0, area.area.y1, area.area.x1, bottom));
                }
            },
        );
        (result, reserved)
    };
    let mut rows = std::collections::HashMap::new();
    let (mut result, _) = fit(&rows);
    let mut regions = Vec::new();
    if let Some(options) = geometry.catchwords.filter(|c| c.catchwords.enabled()) {
        let height = super::catchwords::row_height(doc, story, input.main.end, &options.catchwords);
        let refs: Vec<_> = story.text.match_indices(designcraft_doc::FOOTNOTE_REF).map(|(at, _)| at).collect();
        let mut seen = std::collections::HashSet::new();
        for area in input.areas.iter().rev() {
            let Some(series) = area.opts.note_stream.as_ref().filter(|s| s.catchword.enabled()).map(|s| s.series) else { continue };
            let has_line = result.frames.iter().find(|f| f.frame == area.id).is_some_and(|f| super::catchwords::last_line(f, Some(series)).is_some());
            if !has_line || !seen.insert(series) {
                continue;
            }
            let catchwords = super::catchwords::options(doc, area, Some(series), &options.catchwords);
            let future = story.notes.iter().zip(&refs).any(|(note, at)| {
                doc.note_series_id(note.series) == series
                    && !note.text.is_empty()
                    && geometry.later_ranges.map_or(!ranges.is_empty() && *at >= input.main.end, |later| later.iter().any(|r| r.contains(at)))
            });
            let shown = result.frames.iter().flat_map(|f| &f.notes).any(|n| n.series == series);
            if shown && (future || result.state.pending_series().any(|id| id == series)) {
                let height = result
                    .frames
                    .iter()
                    .find(|f| f.frame == area.id)
                    .and_then(|f| f.notes.last())
                    .and_then(|note| {
                        let source = story.notes.get(note.index)?;
                        let line = note.text.frames.iter().rev().find_map(|f| f.lines.last())?;
                        Some(super::catchwords::row_space(doc, &source.text, line.range.end, area, &catchwords, line.descent))
                    })
                    .unwrap_or(height);
                rows.insert(area.id, height);
            }
        }
        if !rows.is_empty() {
            let (fitted, reserved) = fit(&rows);
            result = fitted;
            for spec in input.areas {
                let Some(frame) = result.frames.iter().find(|f| f.frame == spec.id) else { continue };
                let Some(region) = reserved.get(&spec.id).and_then(|rect| route_catchword_rect(frame, *rect)) else { continue };
                let mut spec = spec.clone();
                spec.opts.inset_columns = Some(options.clone());
                regions.push((spec, region));
            }
        }
    }
    (result, regions)
}

fn band_frame(frame: &FrameSpec, area: Rect) -> FrameSpec {
    let mut f = frame.clone();
    f.area = area;
    f.opts.columns = 1;
    f.opts.columns_kind = designcraft_doc::ColumnsKind::FixedNumber;
    f.opts.column_width = 0.0;
    f.opts.balance_columns = false;
    f.opts.vertical_justification = designcraft_doc::VerticalJustification::Top;
    f
}

fn search<T>(
    keys: &[usize],
    budget: usize,
    mut probe: impl FnMut(usize) -> Result<(usize, bool, T), super::Error>,
) -> Result<(Option<T>, usize, bool), super::Error> {
    let (mut lo, mut hi) = (0, keys.len());
    let mut best: Option<(usize, T)> = None;
    let mut used = 0;
    let mut proved = Vec::new();
    // Feasibility can be nonmonotone. Keep a full-page success before a failed
    // midpoint can exclude it from the bounded search.
    if budget > 0
        && let Some(&key) = keys.last()
    {
        let (cut, feasible, trial) = probe(key)?;
        used += 1;
        proved.push(key);
        hi = hi.saturating_sub(1);
        if feasible {
            if cut == key {
                return Ok((Some(trial), used, false));
            }
            best = Some((cut, trial));
        }
    }
    // Reserve the final trial for progress if no cut fits, or verification if one does.
    while lo < hi && used < budget.saturating_sub(1) {
        let mid = lo + (hi - lo) / 2;
        let Some(&key) = keys.get(mid) else { break };
        let (cut, feasible, trial) = probe(key)?;
        used += 1;
        proved.push(key);
        if feasible {
            if best.as_ref().is_none_or(|(previous, _)| cut > *previous) {
                best = Some((cut, trial));
            }
            lo = mid.saturating_add(1);
        } else {
            hi = mid;
        }
    }
    // A failed midpoint cannot rule out larger keys when feasibility is nonmonotone.
    let mut exhausted = false;
    for &key in keys.iter().rev() {
        if best.as_ref().is_some_and(|(cut, _)| key <= *cut) {
            break;
        }
        if proved.contains(&key) {
            continue;
        }
        if used >= budget {
            exhausted = true;
            break;
        }
        let (cut, feasible, trial) = probe(key)?;
        used += 1;
        if feasible && best.as_ref().is_none_or(|(previous, _)| cut > *previous) {
            best = Some((cut, trial));
        }
    }
    Ok((best.map(|(_, trial)| trial), used, exhausted))
}

type CatchwordOwners = std::collections::HashMap<(Option<usize>, Option<u64>), designcraft_doc::ItemId>;

struct Page<'a> {
    doc: &'a Document,
    story: &'a Story,
    frame: &'a FrameSpec,
    options: &'a InsetColumns,
    past: &'a [FrameSpec],
    before: &'a StreamState,
    start: usize,
    opts: &'a ComposeOptions,
    routes: &'a [FrameSpec],
    route_ranges: Vec<std::ops::Range<usize>>,
    deferred_pages: Vec<usize>,
    last_host_on_page: bool,
    prior_core: Option<f64>,
    catchword_owners: Option<&'a CatchwordOwners>,
    max_cut: Option<usize>,
}
impl Page<'_> {
    fn owns_catchword(&self, series: Option<u64>) -> bool {
        self.catchword_owners.is_none_or(|owners| owners.get(&(self.frame.page, series)) == Some(&self.frame.id))
    }
    fn trial(&self, wanted: usize, force: bool) -> Result<Trial, super::Error> {
        let main = self.options.main().ok_or(super::Error::InvalidGeometry)?;
        let space = super::catchwords::row_height(self.doc, self.story, self.start, &self.options.catchwords);
        let pending: Vec<_> = self.before.pending_series().collect();
        let refs: Vec<_> = self.story.text.match_indices(designcraft_doc::FOOTNOTE_REF).map(|(at, _)| at).collect();
        let streams: Vec<Stream<Cursor>> = self
            .options
            .streams
            .iter()
            .map(|s| {
                let finished = match s.series {
                    None => wanted <= self.start,
                    Some(id) => {
                        !pending.contains(&id)
                            && !self
                                .story
                                .notes
                                .iter()
                                .zip(&refs)
                                .any(|(note, at)| self.doc.note_series_id(note.series) == id && (self.start..wanted).contains(at))
                    }
                };
                Stream {
                    cursor: Cursor {
                        areas: Vec::new(),
                        cut: self.start,
                        state: self.before.clone(),
                        starts_fit: true,
                        warnings: Vec::new(),
                        first_line: None,
                        tail_pending: false,
                        catchword: None,
                        main_bottom: self.frame.area.y0,
                        catchword_space: space,
                    },
                    ratio: s.ratio,
                    finished,
                }
            })
            .collect();
        let main_ratio = self.options.streams.get(main).map_or(1.0, |s| s.ratio);
        let main_active = streams.get(main).is_some_and(|s| !s.finished);
        let total: f64 = streams.iter().filter(|s| !s.finished).map(|s| s.ratio).sum();
        let active = streams.iter().filter(|s| !s.finished).count();
        let initial_width = if main_active {
            (self.frame.area.width() - self.options.gutter * active.saturating_sub(1) as f64) * main_ratio / total
        } else {
            self.frame.area.width()
        };
        let default_left =
            streams.iter().take(main).filter(|s| !s.finished).map(|s| initial_width * s.ratio / main_ratio + self.options.gutter).sum::<f64>();
        let left = self.options.main_left_inset.unwrap_or(default_left);
        let right = self.options.main_right_inset.unwrap_or(self.frame.area.width() - default_left - initial_width);
        let positioned = self.options.main_start_baseline.is_some()
            || self.options.main_start_lines > 0
            || self.options.main_left_inset.is_some()
            || self.options.main_right_inset.is_some();
        let metric_width = if positioned { self.frame.area.width() - left - right } else { initial_width };
        if main_active && (!metric_width.is_finite() || metric_width <= 0.0) {
            return Err(super::Error::InvalidGeometry);
        }
        let first_main = if main_active && (self.options.main_start_lines > 0 || self.options.main_start_baseline.is_some()) {
            let metric =
                band_frame(self.frame, Rect::new(self.frame.area.x0, self.frame.area.y0, self.frame.area.x0 + metric_width, self.frame.area.y1));
            let mut metrics = self.past.to_vec();
            metrics.push(metric);
            main_layout(self.doc, self.story, &metrics, self.opts).frames.last().and_then(|f| f.lines.first()).cloned()
        } else {
            None
        };
        let mut top = first_main.as_ref().map_or(0.0, |l| l.leading * f64::from(self.options.main_start_lines));
        if let Some(n) = self.options.main_start_baseline.filter(|_| main_active) {
            let neighbour = streams
                .iter()
                .enumerate()
                .take(main)
                .rev()
                .find(|(_, s)| !s.finished)
                .or_else(|| streams.iter().enumerate().skip(main.saturating_add(1)).find(|(_, s)| !s.finished));
            if let Some((index, _)) = neighbour {
                let series = self.options.streams.get(index).and_then(|s| s.series).ok_or(super::Error::InvalidGeometry)?;
                let live: Vec<_> = streams.iter().enumerate().filter(|(i, s)| *i != main && !s.finished).map(|(i, _)| i).collect();
                let regions = super::split(&streams, &live, self.frame.area.x0, self.frame.area.x1, self.options.gutter)?;
                let region = regions.iter().find(|r| r.stream == index).ok_or(super::Error::InvalidGeometry)?;
                let mut area = band_frame(
                    self.frame,
                    super::rectangle(
                        Config { bounds: self.frame.area, gutter: self.options.gutter, main, mirror: self.frame.left_page, position: None },
                        *region,
                        self.frame.area.y0,
                        self.frame.area.y1,
                    ),
                );
                area.opts.note_stream = Some(NoteStream { story: self.story.id, series, catchword: false.into(), follow_inset: None });
                let fit = crate::notes::fit_stream_page_for(
                    self.doc,
                    self.story,
                    StreamPageInput {
                        page: self.frame.page.unwrap_or(0),
                        main: self.start..wanted,
                        areas: &[area],
                        force_starts: Some(self.start..wanted),
                    },
                    self.before,
                    &ComposeOptions { note_bands: true, ..self.opts.clone() },
                    Some(series),
                );
                let lines: Vec<_> = fit
                    .frames
                    .iter()
                    .flat_map(|f| &f.notes)
                    .flat_map(|note| note.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |l| (note.origin.y + l.baseline, l.leading))))
                    .collect();
                if let Some(&(baseline, leading)) = lines.get(n.saturating_sub(1) as usize).or_else(|| lines.first()) {
                    let baseline = if lines.get(n.saturating_sub(1) as usize).is_some() {
                        baseline
                    } else {
                        baseline + f64::from(n.saturating_sub(1)) * leading
                    };
                    top = (baseline - first_main.as_ref().map_or(self.frame.area.y0, |l| l.baseline)).max(0.0);
                }
            } else {
                top = 0.0;
            }
        }
        let cfg = Config {
            bounds: self.frame.area,
            gutter: self.options.gutter,
            main,
            mirror: self.frame.left_page,
            position: (positioned && main_active).then_some(MainPosition { top, left, right }),
        };
        let mut first_line_seen = None;
        let out = layout_with_state(cfg, streams, |index, cursor, rect, committed| {
            let series = self.options.streams.get(index).and_then(|s| s.series);
            let catchwords = super::catchwords::options(self.doc, self.frame, series, &self.options.catchwords);
            let source =
                series.and_then(|id| self.story.notes.iter().find(|n| self.doc.note_series_id(n.series) == id)).map_or(self.story, |n| &n.text);
            let space = super::catchwords::row_height(self.doc, source, if series.is_some() { 0 } else { cursor.cut }, &catchwords);
            if cursor.tail_pending {
                let (height, finished, tail_pending, catchword) = tail(rect, 0.0, true, true, false, cursor.catchword_space);
                return Measurement {
                    height,
                    finished,
                    cursor: Cursor { tail_pending, catchword, ..cursor.clone() },
                    placement: FrameText::default(),
                };
            }
            let bottom = rect.y1 >= self.frame.area.y1 - space - 0.01;
            let future = self.options.catchwords.enabled()
                && self.owns_catchword(series)
                && match series {
                    None => wanted < self.story.text.len(),
                    Some(id) => self
                        .story
                        .notes
                        .iter()
                        .zip(&refs)
                        .any(|(note, at)| self.doc.note_series_id(note.series) == id && *at >= wanted && !note.text.text.is_empty()),
                };
            let mut areas = cursor.areas.clone();
            let mut f = band_frame(self.frame, rect);
            for (other, stream) in committed.iter().enumerate() {
                if other != index
                    && stream.finished
                    && let Some(row) = stream.cursor.catchword.filter(|r| r.x0 < rect.x1 && r.x1 > rect.x0)
                {
                    f.exclusions.push(crate::Exclusion {
                        rect: row,
                        mode: designcraft_doc::WrapMode::BoundingBox,
                        side: designcraft_doc::WrapSide::BothSides,
                        contour: None,
                        after: None,
                        anchor_baseline: None,
                    });
                }
            }
            if let Some(previous) = cursor.areas.last() {
                // A stream keeps its old measure until a whole line clears the ended neighbour.
                // The band's top includes that neighbour's catchword descent, not just its body.
                // Baselines continue normally; only the line's available horizontal span changes.
                let old = previous.area;
                let top = self.frame.area.y0 - self.frame.area.height();
                for (left, right) in [(rect.x0, old.x0.min(rect.x1)), (old.x1.max(rect.x0), rect.x1)] {
                    if right > left {
                        f.exclusions.push(crate::Exclusion {
                            rect: Rect::new(left, top, right, rect.y0),
                            mode: designcraft_doc::WrapMode::BoundingBox,
                            side: designcraft_doc::WrapSide::BothSides,
                            contour: None,
                            after: None,
                            anchor_baseline: None,
                        });
                    }
                }
            }
            if let Some(series) = series {
                f.opts.note_stream = Some(NoteStream { story: self.story.id, series, catchword: false.into(), follow_inset: None });
            } else if future && bottom {
                f.area.y1 = (f.area.y1 - space).max(f.area.y0);
            }
            areas.push(f);
            if let Some(series) = series {
                let fit_notes = |areas: &[FrameSpec]| {
                    crate::notes::fit_stream_page_for(
                        self.doc,
                        self.story,
                        StreamPageInput {
                            page: self.frame.page.unwrap_or(0),
                            main: self.start..wanted,
                            areas,
                            force_starts: force.then_some(self.start..wanted),
                        },
                        self.before,
                        &ComposeOptions { note_bands: true, ..self.opts.clone() },
                        Some(series),
                    )
                };
                let mut fit = fit_notes(&areas);
                let needed = self.options.catchwords.enabled()
                    && self.owns_catchword(Some(series))
                    && (future || fit.state.pending_series().any(|id| id == series));
                if needed && bottom {
                    if let Some(last) = areas.last_mut() {
                        last.area.y1 = (last.area.y1 - space).max(last.area.y0);
                    }
                    fit = fit_notes(&areas);
                }
                let part = fit.frames.last().cloned().unwrap_or_default();
                let height = part.notes.iter().map(|n| n.rect.y1 - rect.y0).fold(0.0_f64, f64::max).min(rect.height());
                let finished = fit.starts_fit && !fit.state.pending_series().any(|id| id == series);
                let catchword_space = part
                    .notes
                    .last()
                    .and_then(|note| {
                        let source = self.story.notes.get(note.index)?;
                        let line = note.text.frames.iter().rev().find_map(|f| f.lines.last())?;
                        Some(super::catchwords::row_space(self.doc, &source.text, line.range.end, self.frame, &catchwords, line.descent))
                    })
                    .unwrap_or(cursor.catchword_space);
                let (height, finished, tail_pending, catchword) = tail(rect, height, finished, needed, bottom, catchword_space);
                Measurement {
                    height,
                    finished,
                    cursor: Cursor {
                        areas,
                        state: fit.state,
                        starts_fit: fit.starts_fit,
                        cut: wanted,
                        warnings: fit.warnings,
                        first_line: None,
                        tail_pending,
                        catchword,
                        main_bottom: cursor.main_bottom,
                        catchword_space,
                    },
                    placement: part,
                }
            } else {
                let mut all = self.past.to_vec();
                all.extend(areas.iter().cloned());
                let cs = main_layout(self.doc, self.story, &all, self.opts);
                let main_bottom = cs
                    .frames
                    .iter()
                    .filter(|f| f.frame == self.frame.id)
                    .flat_map(|f| &f.lines)
                    .filter(|l| end(self.story, l) <= wanted)
                    .map(|l| l.baseline + l.descent)
                    .fold(self.frame.area.y0, f64::max);
                let mut part = cs.frames.last().cloned().unwrap_or_default();
                let next_line = part.lines.first().map(|l| end(self.story, l)).or_else(|| {
                    if let Some(last) = all.last_mut() {
                        last.area.y1 = self.frame.area.y1;
                    }
                    main_layout(self.doc, self.story, &all, self.opts).frames.last().and_then(|f| f.lines.first()).map(|l| end(self.story, l))
                });
                let first_line = cursor.first_line.or(next_line);
                if first_line.is_some() {
                    first_line_seen = first_line;
                }
                let crossed = part.lines.iter().any(|l| end(self.story, l) > wanted);
                part.lines.retain(|l| end(self.story, l) <= wanted);
                let cut = part.lines.last().map_or(cursor.cut, |l| end(self.story, l));
                let height = part.lines.last().map_or(0.0, |l| (l.baseline + l.descent - rect.y0).max(0.0)).max(
                    part.notes.iter().filter(|n| n.rect.x0 < rect.x1 && n.rect.x1 > rect.x0).map(|n| n.rect.y1 - rect.y0).fold(0.0_f64, f64::max),
                );
                let finished =
                    crossed || cut >= wanted || cut >= self.story.text.len() || (part.lines.is_empty() && next_line.is_some_and(|end| end > wanted));
                if finished {
                    if let Some(last) = areas.last_mut() {
                        last.area.y1 = rect.y0 + height;
                    }
                    if height == 0.0 {
                        areas.pop();
                    }
                }
                let catchword_space = part.lines.last().map_or(cursor.catchword_space, |line| {
                    super::catchwords::row_space(self.doc, self.story, cut, self.frame, &catchwords, line.descent)
                });
                let (height, finished, tail_pending, catchword) = tail(rect, height, finished, future, bottom, catchword_space);
                Measurement {
                    height: height.min(rect.height()),
                    finished,
                    cursor: Cursor { areas, cut, first_line, tail_pending, catchword, main_bottom, catchword_space, ..cursor.clone() },
                    placement: FrameText::default(),
                }
            }
        })?;
        let c = &out.streams.get(main).ok_or(super::Error::InvalidGeometry)?.cursor;
        let mut state = self.before.clone();
        let mut feasible = c.cut == wanted && !c.tail_pending;
        for (s, config) in out.streams.iter().zip(&self.options.streams) {
            if let Some(id) = config.series {
                state.take_series(&s.cursor.state, id);
                feasible &= s.cursor.starts_fit;
            }
            feasible &= !s.cursor.tail_pending;
        }
        let mut warnings: Vec<_> = out.streams.iter().flat_map(|s| s.cursor.warnings.iter().cloned()).collect();
        let follows_core =
            self.routes.iter().any(|f| f.page == self.frame.page && f.opts.note_stream.as_ref().is_some_and(|s| s.follow_inset.is_some()));
        let own_bottom = std::iter::once(c.main_bottom)
            .chain(out.parts.iter().flat_map(|p| &p.placement.notes).flat_map(|note| {
                note.text.frames.iter().flat_map(move |f| f.lines.iter().map(move |line| note.origin.y + line.baseline + line.descent))
            }))
            .chain(out.streams.iter().filter_map(|s| s.cursor.catchword.map(|rect| rect.y1)))
            .fold(self.frame.area.y0, f64::max);
        let core_bottom = (follows_core && (self.frame.opts.inset_columns.is_some() || self.prior_core.is_some()))
            .then_some(self.prior_core.map_or(own_bottom, |b| b.max(own_bottom)));
        let mut routes = Vec::new();
        let mut route_catchwords = Vec::new();
        let page = self.frame.page.unwrap_or(0);
        let previous = self.past.last().and_then(|f| f.page);
        let mut route_pages: Vec<_> =
            self.routes.iter().filter_map(|f| f.page).filter(|p| previous.is_none_or(|prev| *p > prev) && *p <= page).collect();
        route_pages.sort_unstable();
        route_pages.dedup();
        if self.last_host_on_page && !route_pages.contains(&page) && !self.routes.is_empty() {
            route_pages.push(page);
        }
        route_pages.retain(|p| *p != page || self.last_host_on_page);
        route_pages.retain(|p| !self.deferred_pages.contains(p));
        // Route feasibility uses the candidate's ink bottom, before accepting its main cut.
        for route_page in route_pages {
            let mut ranges = self.route_ranges.clone();
            if route_page == page {
                ranges.push(self.start..c.cut);
            } else {
                ranges.clear();
            }
            let areas: Vec<_> = self.routes.iter().filter(|f| f.page == Some(route_page)).cloned().collect();
            let (fit, regions) = fit_routes(
                self.doc,
                self.story,
                RouteGeometry {
                    core: (route_page == page).then_some(core_bottom).flatten().map(|bottom| (self.frame.id, bottom)),
                    catchwords: Some(self.options),
                    routes: self.routes,
                    later_ranges: None,
                },
                StreamPageInput {
                    page: route_page,
                    main: if route_page == page { self.start..c.cut } else { 0..0 },
                    areas: &areas,
                    force_starts: (force && route_page == page).then_some(self.start..c.cut),
                },
                &state,
                self.opts,
                &ranges,
            );
            for area in &areas {
                if let Some(route) = area.opts.note_stream.as_ref() {
                    state.take_series(&fit.state, route.series);
                }
            }
            // Like ordinary routes, the first-line progress fallback may retain an
            // unstarted comment; the final source-range check reports it as overset.
            feasible &= fit.starts_fit || (force && route_page == page);
            warnings.extend(fit.warnings);
            routes.extend(fit.frames);
            route_catchwords.extend(regions);
        }
        let notes = out.parts.into_iter().flat_map(|p| p.placement.notes).collect();
        let catchwords =
            out.streams.iter().zip(&self.options.streams).filter_map(|(s, config)| s.cursor.catchword.map(|area| (config.series, area))).collect();
        Ok(Trial {
            core_bottom: own_bottom,
            first_line: c.first_line.or(first_line_seen),
            cut: c.cut,
            areas: c.areas.clone(),
            notes,
            routes,
            state,
            feasible,
            warnings,
            catchwords,
            route_catchwords,
        })
    }

    fn probe(&self, key: usize, force: bool) -> Result<Trial, super::Error> {
        let trial = self.trial(key, force)?;
        if trial.cut < key { self.trial(trial.cut, force) } else { Ok(trial) }
    }

    fn fit(&self) -> Result<Trial, super::Error> {
        let mut full = self.past.to_vec();
        full.push(band_frame(self.frame, self.frame.area));
        let cs = main_layout(self.doc, self.story, &full, self.opts);
        let mut keys: Vec<_> = cs.frames.last().into_iter().flat_map(|f| &f.lines).map(|l| end(self.story, l)).filter(|k| *k > self.start).collect();
        keys.retain(|key| self.max_cut.is_none_or(|limit| *key <= limit));
        keys.sort_unstable();
        keys.dedup();
        if keys.is_empty() && self.start >= self.story.text.len() && self.before.pending_series().next().is_none() {
            return self.trial(self.start, false);
        }
        let budget = if keys.is_empty() { 2 } else { usize::BITS as usize - keys.len().saturating_sub(1).leading_zeros() as usize + 2 };
        let (best, _, exhausted) = search(&keys, budget, |key| {
            let trial = self.probe(key, false)?;
            Ok((trial.cut, trial.feasible && trial.cut > self.start, trial))
        })?;
        if let Some(mut best) = best {
            if exhausted {
                best.warnings.push(format!(
                    "Inset cut verification exhausted the trial budget on page {}; retaining the best proven cut",
                    self.frame.page.unwrap_or(0).saturating_add(1)
                ));
            }
            return Ok(best);
        }
        self.progress_probe()
    }

    /// Like a key probe, progress retains the exact measured layout and may round its first
    /// whole line once when dropping an empty stream changes the initial width.
    fn progress_probe(&self) -> Result<Trial, super::Error> {
        if self.before.pending_series().next().is_some() {
            let trial = self.probe(self.start, false)?;
            return if trial.state.progress_from(self.before) { Ok(trial) } else { Err(super::Error::InvalidMeasurement) };
        }
        let main = self.options.main().ok_or(super::Error::InvalidGeometry)?;
        let total: f64 = self.options.streams.iter().map(|s| s.ratio).sum();
        let width = if self.options.main_left_inset.is_some() || self.options.main_right_inset.is_some() {
            self.frame.area.width() - self.options.main_left_inset.unwrap_or(0.0) - self.options.main_right_inset.unwrap_or(0.0)
        } else {
            (self.frame.area.width() - self.options.gutter * self.options.streams.len().saturating_sub(1) as f64)
                * self.options.streams.get(main).map_or(1.0, |s| s.ratio)
                / total
        };
        let mut first = self.past.to_vec();
        first.push(band_frame(self.frame, Rect::new(self.frame.area.x0, self.frame.area.y0, self.frame.area.x0 + width, self.frame.area.y1)));
        let mut key = main_layout(self.doc, self.story, &first, self.opts)
            .frames
            .last()
            .and_then(|f| f.lines.first())
            .map_or(self.start, |l| end(self.story, l));
        // Progress may overflow only comments on the actual first main line, never later anchors
        // admitted by a full-width search key. A changed active set can widen that first line.
        for _ in 0..2 {
            let trial = self.trial(key, true)?;
            if trial.feasible && trial.cut > self.start && trial.first_line == Some(trial.cut) {
                return Ok(trial);
            }
            key = trial.first_line.unwrap_or(self.start);
        }
        Err(super::Error::InvalidMeasurement)
    }
}

#[derive(Clone, Debug)]
struct PageRecord {
    core_bottom: f64,
    start: usize,
    cut: usize,
    incoming: Arc<StreamState>,
    state: Arc<StreamState>,
    areas: Vec<FrameSpec>,
    notes: Vec<crate::PlacedNote>,
    routes: Vec<FrameText>,
    warnings: Vec<String>,
    catchwords: Vec<(Option<u64>, Rect)>,
    route_catchwords: Vec<(FrameSpec, Rect)>,
}
#[derive(Debug)]
pub(crate) struct Trace {
    pages: Vec<PageRecord>,
}
#[cfg(test)]
thread_local! {
    pub(crate) static LAST_ROWS: std::cell::RefCell<Vec<(designcraft_doc::ItemId, Option<u64>)>> = const { std::cell::RefCell::new(Vec::new()) };
    pub(crate) static DEPENDENCY_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PAGE_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static MAIN_LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static ROUTE_AREAS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static REFITTED: std::cell::RefCell<Vec<designcraft_doc::ItemId>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn changed_byte(old: &str, new: &str) -> usize {
    old.bytes().zip(new.bytes()).position(|(a, b)| a != b).unwrap_or(old.len().min(new.len()))
}

fn same_format_prefix<'a, T: PartialEq + 'a>(
    mut old: impl Iterator<Item = (std::ops::Range<usize>, &'a T)>,
    mut new: impl Iterator<Item = (std::ops::Range<usize>, &'a T)>,
) -> usize {
    let (mut a, mut b) = (old.next(), new.next());
    let mut at = 0;
    while let (Some((ar, af)), Some((br, bf))) = (&a, &b) {
        if af != bf {
            break;
        }
        at = ar.end.min(br.end);
        if ar.end <= at {
            a = old.next();
        }
        if br.end <= at {
            b = new.next();
        }
    }
    at
}
fn format_prefix(old: &Story, new: &Story) -> usize {
    same_format_prefix(old.runs(), new.runs())
        .min(same_format_prefix(old.para_ranges().into_iter().zip(&old.paras), new.para_ranges().into_iter().zip(&new.paras)))
}

fn same_main_tail(old: &Story, new: &Story, cut: usize) -> bool {
    fn formats<'a, T: PartialEq + 'a>(
        old: impl Iterator<Item = (std::ops::Range<usize>, &'a T)>,
        new: impl Iterator<Item = (std::ops::Range<usize>, &'a T)>,
        cut: usize,
    ) -> bool {
        old.filter(|(r, _)| r.end > cut)
            .map(|(r, f)| (r.start.max(cut)..r.end, f))
            .eq(new.filter(|(r, _)| r.end > cut).map(|(r, f)| (r.start.max(cut)..r.end, f)))
    }
    old.text.get(cut..) == new.text.get(cut..)
        && formats(old.runs(), new.runs(), cut)
        && formats(old.para_ranges().into_iter().zip(&old.paras), new.para_ranges().into_iter().zip(&new.paras), cut)
}

fn resume_page(story: &Story, before: crate::reuse::Reuse<'_>, trace: &Trace) -> (usize, Vec<usize>) {
    let old = before.story;
    let mut first = trace.pages.len();
    if old.text != story.text || old.paras != story.paras || !old.runs().eq(story.runs()) {
        let at = changed_byte(&old.text, &story.text).min(format_prefix(old, story));
        first = trace.pages.iter().position(|p| p.cut > at).unwrap_or(0);
    }
    let mut changed = Vec::new();
    for (k, note) in story.notes.iter().enumerate() {
        let Some(previous) = old.notes.get(k) else {
            first = 0;
            changed.push(k);
            continue;
        };
        if std::sync::Arc::ptr_eq(note, previous) {
            continue;
        }
        changed.push(k);
        let at = changed_byte(&previous.text.text, &note.text.text).min(format_prefix(&previous.text, &note.text));
        let page = trace.pages.iter().position(|p| {
            p.notes
                .iter()
                .chain(p.routes.iter().flat_map(|f| &f.notes))
                .filter(|n| n.index == k)
                .any(|n| n.text.frames.iter().any(|f| f.range.end > at))
        });
        first = first.min(page.unwrap_or(0));
    }
    if old.notes.len() != story.notes.len() {
        first = 0;
        changed.extend(story.notes.len()..old.notes.len());
    }
    (first, changed)
}

fn rejoined(old: &Story, trace: &Trace, index: usize, record: &PageRecord, story: &Story, changed: &[usize], refs: &[usize]) -> bool {
    trace.pages.get(index).is_some_and(|p| {
        p.start == record.start && p.cut == record.cut && p.state.progress() == record.state.progress()
    })
        && same_main_tail(old, story, record.cut)
        && !record.state.pending_indices().any(|k| changed.contains(&k))
        && !changed.iter().any(|k| refs.get(*k).is_some_and(|at| *at >= record.cut))
        // A later, independently changed note may not be pending on this page yet.
        && !trace.pages.iter().skip(index.saturating_add(1)).flat_map(|p| p.notes.iter().chain(p.routes.iter().flat_map(|f| &f.notes))).any(|n| changed.contains(&n.index))
}

fn has_repeated_pages(mut pages: impl Iterator<Item = Option<usize>>) -> bool {
    let mut seen = std::collections::HashSet::new();
    pages.any(|page| {
        #[cfg(test)]
        PAGE_VISITS.with(|n| n.set(n.get() + 1));
        !seen.insert(page)
    })
}

pub(crate) fn compose_frames(
    doc: &Document,
    story: &Story,
    frames: &[FrameSpec],
    opts: &ComposeOptions,
    before: Option<crate::reuse::Reuse<'_>>,
) -> (ComposedStory, Option<crate::reuse::Trace>) {
    let mut result = compose_frames_owned(doc, story, frames, opts, before, None);
    // Only multi-host pages need a second fit: reclaim earlier rows after their stream's
    // last text-bearing host is known, then revalidate the cuts that used that space.
    if !frames.iter().any(|f| f.opts.inset_columns.as_ref().is_some_and(|c| c.catchwords.enabled()))
        || !has_repeated_pages(frames.iter().map(|f| f.page))
    {
        return result;
    }
    let owners = |out: &ComposedStory| {
        let mut owners = CatchwordOwners::new();
        for spec in frames {
            let Some(frame) = out.frame(spec.id) else { continue };
            if !frame.lines.is_empty() {
                owners.insert((spec.page, None), spec.id);
            }
            for note in &frame.notes {
                if note.text.frames.iter().any(|f| !f.lines.is_empty()) {
                    owners.insert((spec.page, Some(note.series)), spec.id);
                }
            }
        }
        owners
    };
    let mut current = owners(&result.0);
    for _ in 0..frames.len().saturating_add(1) {
        result = compose_frames_owned(doc, story, frames, opts, None, Some(&current));
        let next = owners(&result.0);
        if next == current {
            return result;
        }
        current = next;
    }
    result.0.warnings.push("Inset catchword ownership did not stabilize within the host count".into());
    result
}

type HostFit = (Vec<PageRecord>, Vec<FrameSpec>, Arc<StreamState>, usize, Vec<String>);

#[allow(clippy::too_many_arguments)]
fn fit_hosts(
    doc: &Document,
    story: &Story,
    frames: &[FrameSpec],
    opts: &ComposeOptions,
    before: Option<crate::reuse::Reuse<'_>>,
    catchword_owners: Option<&CatchwordOwners>,
    routes: &[FrameSpec],
    limits: &std::collections::HashMap<designcraft_doc::ItemId, usize>,
) -> HostFit {
    let old = before.and_then(|b| b.trace.inset.as_deref().map(|trace| (b, trace)));
    let (resume, changed) = old.map_or((0, Vec::new()), |(b, trace)| resume_page(story, b, trace));
    let mut pages = old.map_or_else(Vec::new, |(_, t)| t.pages.iter().take(resume).cloned().collect::<Vec<_>>());
    let mut past: Vec<_> = pages.iter().flat_map(|p| p.areas.iter().cloned()).collect();
    let mut state = old
        .and_then(|(_, t)| t.pages.get(resume))
        .map(|p| p.incoming.clone())
        .unwrap_or_else(|| pages.last().map_or_else(Arc::default, |p| p.state.clone()));
    let mut start = pages.last().map_or(0, |p| p.cut);
    let refs: Vec<_> = story.text.match_indices(designcraft_doc::FOOTNOTE_REF).map(|(at, _)| at).collect();
    let mut warnings = Vec::new();
    for (index, frame) in frames.iter().enumerate().skip(resume) {
        let default = InsetColumns {
            streams: vec![InsetStream { series: None, ratio: 1.0 }],
            gutter: 0.0,
            main_start_lines: 0,
            main_start_baseline: None,
            main_left_inset: None,
            main_right_inset: None,
            catchwords: false.into(),
        };
        let options = frame.opts.inset_columns.as_ref().unwrap_or(&default);
        if !options.valid() {
            warnings.push("Invalid inset-column options".into());
            break;
        }
        #[cfg(test)]
        REFITTED.with(|v| v.borrow_mut().push(frame.id));
        let route_ranges =
            pages.iter().enumerate().filter(|(i, _)| frames.get(*i).is_some_and(|f| f.page == frame.page)).map(|(_, p)| p.start..p.cut).collect();
        let deferred_pages = frames.iter().skip(index + 1).filter_map(|f| f.page).collect();
        let prior_core = pages
            .iter()
            .enumerate()
            .filter(|(i, _)| frames.get(*i).is_some_and(|f| f.page == frame.page))
            .filter_map(|(i, p)| {
                let host = frames.get(i)?;
                host.opts.inset_columns.as_ref()?;
                route_edge(doc, host.id, frame.id, p.core_bottom, 0.0)
            })
            .reduce(f64::max);
        let last_host_on_page = !frames.iter().skip(index + 1).any(|f| f.page == frame.page);
        let page = Page {
            doc,
            story,
            frame,
            options,
            past: &past,
            before: &state,
            start,
            opts,
            routes,
            route_ranges,
            deferred_pages,
            last_host_on_page,
            prior_core,
            catchword_owners,
            max_cut: limits.get(&frame.id).copied(),
        };
        match page.fit() {
            Ok(mut trial) => {
                if !trial.feasible {
                    trial.warnings.push(format!("Inset stream overflow on page {}", frame.page.unwrap_or(0).saturating_add(1)));
                }
                let record = PageRecord {
                    core_bottom: trial.core_bottom,
                    start,
                    cut: trial.cut,
                    incoming: state.clone(),
                    state: Arc::new(trial.state),
                    areas: trial.areas.clone(),
                    notes: trial.notes,
                    routes: trial.routes,
                    warnings: trial.warnings,
                    catchwords: trial.catchwords,
                    route_catchwords: trial.route_catchwords,
                };
                start = trial.cut;
                past.extend(trial.areas);
                state = record.state.clone();
                // A page's routes still depend on every host's candidate geometry.
                let rejoin =
                    last_host_on_page && old.is_some_and(|(before, trace)| rejoined(before.story, trace, index, &record, story, &changed, &refs));
                pages.push(record);
                if rejoin {
                    if let Some((_, trace)) = old {
                        for p in trace.pages.iter().skip(index.saturating_add(1)) {
                            past.extend(p.areas.iter().cloned());
                            pages.push(p.clone());
                        }
                        start = pages.last().map_or(start, |p| p.cut);
                        state = pages.last().map_or(state.clone(), |p| p.state.clone());
                    }
                    break;
                }
            }
            Err(e) => {
                warnings.push(e.to_string());
                break;
            }
        }
    }
    (pages, past, state, start, warnings)
}

/// Host visits follow the text thread; route carries follow physical pages. Once all visits
/// are measured, reject any earlier host cut whose newly anchored note cannot start there.
fn fit_physical_routes(
    doc: &Document,
    story: &Story,
    frames: &[FrameSpec],
    routes: &[FrameSpec],
    pages: &mut [PageRecord],
    opts: &ComposeOptions,
    state: &mut StreamState,
) -> Option<(designcraft_doc::ItemId, usize)> {
    let mut route_state = StreamState::default();
    let past: Vec<_> = pages.iter().flat_map(|p| p.areas.iter().cloned()).collect();
    let text = main_layout(doc, story, &past, opts);
    let mut numbers: Vec<_> = routes.iter().chain(frames).filter_map(|f| f.page).collect();
    numbers.sort_unstable();
    numbers.dedup();
    for page in numbers {
        let hosts: Vec<_> = frames.iter().zip(pages.iter()).filter(|(f, _)| f.page == Some(page)).collect();
        let Some(host) = hosts
            .last()
            .map(|(f, _)| *f)
            .or_else(|| frames.iter().filter(|f| f.page.is_some_and(|p| p <= page)).max_by_key(|f| f.page))
            .or_else(|| frames.first())
        else {
            continue;
        };
        let ranges: Vec<_> = hosts.iter().map(|(_, p)| p.start..p.cut).collect();
        let core = hosts
            .iter()
            .filter(|(f, _)| f.opts.inset_columns.is_some())
            .filter_map(|(f, p)| route_edge(doc, f.id, host.id, p.core_bottom, 0.0))
            .reduce(f64::max);
        let areas: Vec<_> = routes.iter().filter(|f| f.page == Some(page)).cloned().collect();
        let later_ranges: Vec<_> =
            frames.iter().zip(pages.iter()).filter(|(f, _)| f.page.is_some_and(|p| p > page)).map(|(_, p)| p.start..p.cut).collect();
        let force_starts = hosts.first().and_then(|(spec, _)| text.frame(spec.id).and_then(|f| f.lines.first()).map(|line| line.range.clone()));
        let (fit, regions) = fit_routes(
            doc,
            story,
            RouteGeometry {
                core: core.map(|b| (host.id, b)),
                catchwords: host.opts.inset_columns.as_ref(),
                routes,
                later_ranges: Some(&later_ranges),
            },
            StreamPageInput { page, main: ranges.first().map_or(0, |r| r.start)..ranges.last().map_or(0, |r| r.end), areas: &areas, force_starts },
            &route_state,
            opts,
            &ranges,
        );
        if !fit.starts_fit
            && let Some(at) = fit.failed_anchor
            && let Some((spec, _)) = hosts.iter().find(|(_, p)| (p.start..p.cut).contains(&at))
        {
            return Some((spec.id, at));
        }
        for area in &areas {
            if let Some(route) = &area.opts.note_stream {
                route_state.take_series(&fit.state, route.series);
            }
        }
        // Route-only pages must consume carries before a later page with a host.
        let index = frames.iter().rposition(|f| f.page == Some(page)).or_else(|| pages.len().checked_sub(1))?;
        if let Some(record) = pages.get_mut(index) {
            record.routes.extend(fit.frames);
            record.route_catchwords.extend(regions);
            record.warnings.extend(fit.warnings);
        }
    }
    for series in routes.iter().filter_map(|f| f.opts.note_stream.as_ref().map(|s| s.series)) {
        state.take_series(&route_state, series);
    }
    None
}

fn compose_frames_owned(
    doc: &Document,
    story: &Story,
    frames: &[FrameSpec],
    opts: &ComposeOptions,
    before: Option<crate::reuse::Reuse<'_>>,
    catchword_owners: Option<&CatchwordOwners>,
) -> (ComposedStory, Option<crate::reuse::Trace>) {
    let inset_series: Vec<_> =
        frames.iter().filter_map(|f| f.opts.inset_columns.as_ref()).flat_map(|c| c.streams.iter().filter_map(|s| s.series)).collect();
    let routes: Vec<_> = crate::stream_frame_specs(doc, story.id)
        .into_iter()
        .filter(|f| f.opts.note_stream.as_ref().is_some_and(|s| !inset_series.contains(&s.series)))
        .collect();
    let revisited = frames.windows(2).any(|pair| pair[0].page > pair[1].page);
    let mut limits = std::collections::HashMap::new();
    let mut fitted =
        fit_hosts(doc, story, frames, opts, if revisited { None } else { before }, catchword_owners, if revisited { &[] } else { &routes }, &limits);
    if revisited {
        for _ in 0..story.notes.len().saturating_add(frames.len()).saturating_add(1) {
            let (pages, _, state, _, warnings) = &mut fitted;
            let Some((host, at)) = fit_physical_routes(doc, story, frames, &routes, pages, opts, Arc::make_mut(state)) else { break };
            if limits.insert(host, at) == Some(at) {
                warnings.push("Inset route cannot start its anchored note after revalidating the host cut".into());
                break;
            }
            fitted = fit_hosts(doc, story, frames, opts, None, catchword_owners, &[], &limits);
        }
    }
    let (pages, past, state, start, mut warnings) = fitted;
    let refs: Vec<_> = story.text.match_indices(designcraft_doc::FOOTNOTE_REF).map(|(at, _)| at).collect();
    let mut out = main_layout(doc, story, &past, opts);
    let mut merged: Vec<FrameText> = Vec::new();
    for ft in out.frames {
        if let Some(previous) = merged.iter_mut().find(|f| f.frame == ft.frame) {
            previous.lines.extend(ft.lines);
            previous.columns.extend(ft.columns);
            previous.decos.extend(ft.decos);
            previous.tables.extend(ft.tables);
            previous.notes.extend(ft.notes);
            previous.objects.extend(ft.objects);
            previous.range.end = previous.range.end.max(ft.range.end);
            previous.content_height = previous.content_height.max(ft.content_height);
        } else {
            merged.push(ft);
        }
    }
    for frame in frames {
        if !merged.iter().any(|f| f.frame == frame.id) {
            merged.push(FrameText { frame: frame.id, range: start..start, ..Default::default() });
        }
    }
    for (index, page) in pages.iter().enumerate() {
        let Some(id) = frames.get(index).map(|f| f.id) else { continue };
        for mut note in page.notes.clone() {
            note.reference_baseline =
                refs.get(note.index).and_then(|at| merged.iter().flat_map(|f| &f.lines).find(|l| l.range.contains(at))).map_or(0.0, |l| l.baseline);
            if let Some(source) = story.notes.get(note.index) {
                note.source = std::sync::Arc::from(source.text.text.as_str());
            }
            if let Some(frame) = merged.iter_mut().find(|f| f.frame == id) {
                frame.notes.push(note);
            }
        }
        warnings.extend(page.warnings.iter().cloned());
    }
    for frame in &mut merged {
        let top = frames.iter().find(|f| f.id == frame.frame).map_or(0.0, |f| f.area.y0);
        frame.content_height =
            frame.lines.iter().map(|l| l.baseline + l.descent).chain(frame.notes.iter().map(|n| n.rect.y1)).fold(top, f64::max) - top;
    }
    merged.extend(pages.iter().flat_map(|p| p.routes.iter().cloned()));
    let mut route_catchwords: Vec<_> = pages.iter().flat_map(|p| p.route_catchwords.iter().cloned()).collect();
    // Routes can continue on pages beyond the final main frame.
    let mut state = (*state).clone();
    let fitted_pages: std::collections::HashSet<_> =
        pages.iter().flat_map(|p| &p.routes).filter_map(|f| routes.iter().find(|r| r.id == f.frame).and_then(|r| r.page)).collect();
    let mut trailing: Vec<_> = routes.iter().filter_map(|f| f.page).filter(|p| !fitted_pages.contains(p)).collect();
    trailing.sort_unstable();
    trailing.dedup();
    for page in trailing {
        let areas: Vec<_> = routes.iter().filter(|f| f.page == Some(page)).cloned().collect();
        let (fit, regions) = fit_routes(
            doc,
            story,
            RouteGeometry { core: None, catchwords: frames.last().and_then(|f| f.opts.inset_columns.as_ref()), routes: &routes, later_ranges: None },
            StreamPageInput { page, main: 0..0, areas: &areas, force_starts: None },
            &state,
            opts,
            &[],
        );
        for area in &areas {
            if let Some(route) = area.opts.note_stream.as_ref() {
                state.take_series(&fit.state, route.series);
            }
        }
        warnings.extend(fit.warnings);
        merged.extend(fit.frames);
        route_catchwords.extend(regions);
    }
    let baselines: Vec<_> =
        refs.iter().map(|at| merged.iter().flat_map(|f| &f.lines).find(|l| l.range.contains(at)).map_or(0.0, |l| l.baseline)).collect();
    for note in merged.iter_mut().flat_map(|f| &mut f.notes) {
        note.reference_baseline = baselines.get(note.index).copied().unwrap_or(0.0);
        if let Some(source) = story.notes.get(note.index) {
            note.source = Arc::from(source.text.text.as_str());
        }
    }
    for series in state.pending_series() {
        if let Some(note) = merged.iter_mut().rev().flat_map(|f| f.notes.iter_mut().rev()).find(|n| n.series == series) {
            note.overflow = true;
        }
    }
    let commentary_overset = state.pending_series().next().is_some()
        || story.notes.iter().enumerate().any(|(index, note)| {
            let placed = merged
                .iter()
                .flat_map(|f| &f.notes)
                .filter(|n| n.index == index)
                .flat_map(|n| &n.text.frames)
                .map(|f| f.range.end)
                .max()
                .unwrap_or(0);
            refs.get(index).is_some_and(|at| *at < start)
                && (routes.iter().any(|f| f.opts.note_stream.as_ref().is_some_and(|s| s.series == doc.note_series_id(note.series)))
                    || frames
                        .iter()
                        .filter_map(|f| f.opts.inset_columns.as_ref())
                        .flat_map(|c| &c.streams)
                        .any(|s| s.series == Some(doc.note_series_id(note.series))))
                && placed < note.text.len()
        });
    if commentary_overset {
        warnings.push("Inset commentary remains after the final frame".into());
    }
    out.frames = merged;
    out.overset_at = out.overset_at.or((start < story.text.len() || commentary_overset).then_some(start));
    out.warnings.extend(warnings);
    let mut catchwords: Vec<_> = pages.iter().map(|p| p.catchwords.clone()).collect();
    catchwords.resize_with(frames.len(), Vec::new);
    #[cfg(test)]
    LAST_ROWS.with(|v| {
        *v.borrow_mut() = frames.iter().zip(&catchwords).flat_map(|(f, rows)| rows.iter().map(move |(series, _)| (f.id, *series))).collect()
    });
    let mut catchword_frames = frames.to_vec();
    for (spec, rect) in route_catchwords {
        catchwords.push(vec![(spec.opts.note_stream.as_ref().map(|s| s.series), rect)]);
        catchword_frames.push(spec);
    }
    super::catchwords::place(doc, story, &mut out, &catchword_frames, &catchwords);
    (out, Some(crate::reuse::Trace::inset(Trace { pages })))
}

#[cfg(test)]
mod search_tests {
    use super::*;
    #[test]
    fn repeated_page_detection_visits_each_page_once() {
        PAGE_VISITS.with(|n| n.set(0));
        assert!(!has_repeated_pages((0..256).map(Some)));
        assert!(PAGE_VISITS.with(|n| n.get()) <= 256, "visits {}", PAGE_VISITS.with(|n| n.get()));
        assert!(has_repeated_pages([Some(0), Some(1), Some(0)].into_iter()));
    }
    #[test]
    fn no_success_spends_remaining_budget_on_larger_unprobed_keys() {
        let mut probes = Vec::new();
        let (best, used, _) = search(&[1, 2, 3, 4, 5, 6, 7], 5, |key| {
            probes.push(key);
            Ok((key, key == 6, key))
        })
        .unwrap();
        assert_eq!(best, Some(6), "probed {probes:?}");
        assert_eq!(used, 5);
    }
    #[test]
    fn interior_nonmonotone_fit_checks_larger_unproved_keys_within_budget() {
        let mut probes = Vec::new();
        let (best, used, exhausted) = search(&[1, 2, 3, 4, 5, 6, 7], 5, |key| {
            probes.push(key);
            Ok((key, key == 2 || key == 6, key))
        })
        .unwrap();
        assert!(used <= 5);
        assert_eq!(used, probes.len());
        assert_eq!(best, Some(6), "probed {probes:?}");
        assert!(!exhausted);
    }
    #[test]
    fn verification_budget_exhaustion_keeps_the_best_proven_cut() {
        let mut probes = Vec::new();
        let (best, used, exhausted) = search(&[1, 2, 3, 4, 5, 6, 7], 5, |key| {
            probes.push(key);
            Ok((key, key == 2 || key == 5, key))
        })
        .unwrap();
        assert_eq!(best, Some(2));
        assert_eq!(used, 5);
        assert_eq!(probes, [7, 4, 2, 3, 6]);
        assert!(exhausted);
    }
    #[test]
    fn verification_proves_all_larger_failures_without_repeating_probes() {
        let mut probes = Vec::new();
        let (best, used, exhausted) = search(&[1, 2, 3, 4, 5, 6, 7], 6, |key| {
            probes.push(key);
            Ok((key, key == 2, key))
        })
        .unwrap();
        assert_eq!(best, Some(2));
        assert_eq!(used, 6);
        assert_eq!(probes, [7, 4, 2, 3, 6, 5]);
        assert!(!exhausted);
    }
    #[test]
    fn full_page_fit_survives_nonmonotone_feasibility() {
        let (best, _, exhausted) = search(&[1, 2, 3, 4, 5], 4, |key| Ok((key, key == 1 || key == 5, key))).unwrap();
        assert_eq!(best, Some(5));
        assert!(!exhausted);
    }
    #[test]
    fn catchword_row_is_committed_once_and_can_wait_for_a_wider_band() {
        let short = Rect::new(0.0, 0.0, 100.0, 10.0);
        let (height, finished, pending, caption) = tail(short, 9.0, true, true, false, CATCHWORD_SPACE);
        assert_eq!(height, 9.0);
        assert!(!finished && pending && caption.is_none());
        let next = Rect::new(0.0, 10.0, 200.0, 50.0);
        let (height, finished, pending, caption) = tail(next, 0.0, true, true, false, CATCHWORD_SPACE);
        assert_eq!(height, CATCHWORD_SPACE);
        assert!(finished && !pending);
        assert_eq!(caption, Some(Rect::new(0.0, 10.0, 200.0, 28.0)));
        assert_eq!(tail(next, 9.0, false, true, false, CATCHWORD_SPACE), (9.0, false, false, None));
        assert_eq!(tail(next, 9.0, false, true, true, CATCHWORD_SPACE).0, 9.0 + CATCHWORD_SPACE);
    }
    #[test]
    fn nonmonotone_search_retains_only_a_measured_feasible_layout_within_budget() {
        for count in 1..33_usize {
            let keys: Vec<_> = (1..=count).collect();
            let budget = usize::BITS as usize - count.saturating_sub(1).leading_zeros() as usize + 2;
            let mut measured = Vec::new();
            let (best, used, _) = search(&keys, budget, |key| {
                let feasible = key % 3 != 0;
                measured.push((key, feasible));
                Ok((key, feasible, key))
            })
            .unwrap();
            assert!(used <= budget);
            assert_eq!(used, measured.len());
            assert_eq!(best, measured.iter().filter(|(_, fit)| *fit).map(|(k, _)| *k).max());
        }
    }
}
