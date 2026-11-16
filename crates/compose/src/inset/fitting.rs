//! Page trials keep original stories intact and replay accepted whole-line rectangles.
use super::{Config, MainPosition, Measurement, Stream, layout};
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
}
struct Trial {
    first_line: Option<usize>,
    cut: usize,
    areas: Vec<FrameSpec>,
    notes: Vec<crate::PlacedNote>,
    state: StreamState,
    feasible: bool,
    warnings: Vec<String>,
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
    crate::compose(doc, story, frames, &main_options(opts, frames))
}

fn band_frame(frame: &FrameSpec, area: Rect) -> FrameSpec {
    let mut f = frame.clone();
    f.area = area;
    f.opts.columns = 1;
    f.opts.balance_columns = false;
    f.opts.vertical_justification = designcraft_doc::VerticalJustification::Top;
    f
}

fn search<T>(
    keys: &[usize],
    budget: usize,
    mut probe: impl FnMut(usize) -> Result<(usize, bool, T), super::Error>,
) -> Result<(Option<T>, usize), super::Error> {
    let (mut lo, mut hi) = (0, keys.len());
    let mut best: Option<(usize, T)> = None;
    let mut used = 0;
    while lo < hi && used < budget {
        let mid = lo + (hi - lo) / 2;
        let Some(&key) = keys.get(mid) else { break };
        let (cut, feasible, trial) = probe(key)?;
        used += 1;
        if feasible {
            if best.as_ref().is_none_or(|(previous, _)| cut > *previous) {
                best = Some((cut, trial));
            }
            lo = mid.saturating_add(1);
        } else {
            hi = mid;
        }
    }
    Ok((best.map(|(_, trial)| trial), used))
}

struct Page<'a> {
    doc: &'a Document,
    story: &'a Story,
    frame: &'a FrameSpec,
    options: &'a InsetColumns,
    past: &'a [FrameSpec],
    before: &'a StreamState,
    start: usize,
    opts: &'a ComposeOptions,
}
impl Page<'_> {
    fn trial(&self, wanted: usize, force: bool) -> Result<Trial, super::Error> {
        let main = self.options.main().ok_or(super::Error::InvalidGeometry)?;
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
        let positioned = self.options.main_start_lines > 0 || self.options.main_left_inset.is_some() || self.options.main_right_inset.is_some();
        let metric_width = if positioned { self.frame.area.width() - left - right } else { initial_width };
        if main_active && (!metric_width.is_finite() || metric_width <= 0.0) {
            return Err(super::Error::InvalidGeometry);
        }
        let leading = if main_active && self.options.main_start_lines > 0 {
            let metric =
                band_frame(self.frame, Rect::new(self.frame.area.x0, self.frame.area.y0, self.frame.area.x0 + metric_width, self.frame.area.y1));
            let mut metrics = self.past.to_vec();
            metrics.push(metric);
            main_layout(self.doc, self.story, &metrics, self.opts).frames.last().and_then(|f| f.lines.first()).map_or(0.0, |l| l.leading)
        } else {
            0.0
        };
        let cfg = Config {
            bounds: self.frame.area,
            gutter: self.options.gutter,
            main,
            mirror: self.frame.left_page,
            position: (positioned && main_active).then_some(MainPosition { top: leading * f64::from(self.options.main_start_lines), left, right }),
        };
        let mut first_line_seen = None;
        let out = layout(cfg, streams, |index, cursor, rect| {
            let series = self.options.streams.get(index).and_then(|s| s.series);
            let mut areas = cursor.areas.clone();
            let mut f = band_frame(self.frame, rect);
            if let Some(previous) = cursor.areas.last() {
                // A stream keeps its old measure until a whole line clears the ended neighbour.
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
                f.opts.note_stream = Some(NoteStream { story: self.story.id, series });
            }
            areas.push(f);
            if let Some(series) = series {
                let fit = crate::notes::fit_stream_page_for(
                    self.doc,
                    self.story,
                    StreamPageInput {
                        page: self.frame.page.unwrap_or(0),
                        main: self.start..wanted,
                        areas: &areas,
                        force_starts: force.then_some(self.start..wanted),
                    },
                    self.before,
                    &ComposeOptions { note_bands: true, ..self.opts.clone() },
                    Some(series),
                );
                let part = fit.frames.last().cloned().unwrap_or_default();
                let height = part.notes.iter().map(|n| n.rect.y1 - rect.y0).fold(0.0_f64, f64::max).min(rect.height());
                let finished = fit.starts_fit && !fit.state.pending_series().any(|id| id == series);
                Measurement {
                    height,
                    finished,
                    cursor: Cursor { areas, state: fit.state, starts_fit: fit.starts_fit, cut: wanted, warnings: fit.warnings, first_line: None },
                    placement: part,
                }
            } else {
                let mut all = self.past.to_vec();
                all.extend(areas.iter().cloned());
                let cs = main_layout(self.doc, self.story, &all, self.opts);
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
                let height = part
                    .lines
                    .last()
                    .map_or(0.0, |l| (l.baseline + l.descent - rect.y0).max(0.0))
                    .max(part.notes.iter().map(|n| n.rect.y1 - rect.y0).fold(0.0_f64, f64::max));
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
                Measurement {
                    height: height.min(rect.height()),
                    finished,
                    cursor: Cursor { areas, cut, first_line, ..cursor.clone() },
                    placement: FrameText::default(),
                }
            }
        })?;
        let c = &out.streams.get(main).ok_or(super::Error::InvalidGeometry)?.cursor;
        let mut state = self.before.clone();
        let mut feasible = c.cut == wanted;
        for (s, config) in out.streams.iter().zip(&self.options.streams) {
            if let Some(id) = config.series {
                state.take_series(&s.cursor.state, id);
                feasible &= s.cursor.starts_fit;
            }
        }
        let warnings = out.streams.iter().flat_map(|s| s.cursor.warnings.iter().cloned()).collect();
        let notes = out.parts.into_iter().flat_map(|p| p.placement.notes).collect();
        Ok(Trial { first_line: c.first_line.or(first_line_seen), cut: c.cut, areas: c.areas.clone(), notes, state, feasible, warnings })
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
        keys.sort_unstable();
        keys.dedup();
        if keys.is_empty() && self.start >= self.story.text.len() && self.before.pending_series().next().is_none() {
            return self.trial(self.start, false);
        }
        let budget = if keys.is_empty() { 2 } else { usize::BITS as usize - keys.len().saturating_sub(1).leading_zeros() as usize + 2 };
        let (best, _) = search(&keys, budget.saturating_sub(1), |key| {
            let trial = self.probe(key, false)?;
            Ok((trial.cut, trial.feasible && trial.cut > self.start, trial))
        })?;
        if let Some(best) = best {
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
    start: usize,
    cut: usize,
    incoming: Arc<StreamState>,
    state: Arc<StreamState>,
    areas: Vec<FrameSpec>,
    notes: Vec<crate::PlacedNote>,
    warnings: Vec<String>,
}
#[derive(Debug)]
pub(crate) struct Trace {
    pages: Vec<PageRecord>,
}
#[cfg(test)]
thread_local! {
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
        let page = trace.pages.iter().position(|p| p.notes.iter().filter(|n| n.index == k).any(|n| n.text.frames.iter().any(|f| f.range.end > at)));
        first = first.min(page.unwrap_or(0));
    }
    if old.notes.len() != story.notes.len() {
        first = 0;
    }
    (first, changed)
}

pub(crate) fn compose_frames(
    doc: &Document,
    story: &Story,
    frames: &[FrameSpec],
    opts: &ComposeOptions,
    before: Option<crate::reuse::Reuse<'_>>,
) -> (ComposedStory, Option<crate::reuse::Trace>) {
    let old = before.and_then(|b| b.trace.inset.as_deref().map(|trace| (b, trace)));
    let (resume, changed) = old.map_or((0, Vec::new()), |(b, trace)| resume_page(story, b, trace));
    let mut pages = old.map_or_else(Vec::new, |(_, t)| t.pages.iter().take(resume).cloned().collect::<Vec<_>>());
    let mut past: Vec<_> = pages.iter().flat_map(|p| p.areas.iter().cloned()).collect();
    let mut state = old
        .and_then(|(_, t)| t.pages.get(resume))
        .map(|p| p.incoming.clone())
        .unwrap_or_else(|| pages.last().map_or_else(Arc::default, |p| p.state.clone()));
    let mut start = pages.last().map_or(0, |p| p.cut);
    let mut warnings = Vec::new();
    for (index, frame) in frames.iter().enumerate().skip(resume) {
        let default = InsetColumns {
            streams: vec![InsetStream { series: None, ratio: 1.0 }],
            gutter: 0.0,
            main_start_lines: 0,
            main_left_inset: None,
            main_right_inset: None,
        };
        let options = frame.opts.inset_columns.as_ref().unwrap_or(&default);
        if !options.valid() {
            warnings.push("Invalid inset-column options".into());
            break;
        }
        #[cfg(test)]
        REFITTED.with(|v| v.borrow_mut().push(frame.id));
        let page = Page { doc, story, frame, options, past: &past, before: &state, start, opts };
        match page.fit() {
            Ok(mut trial) => {
                if !trial.feasible {
                    trial.warnings.push(format!("Inset stream overflow on page {}", frame.page.unwrap_or(0).saturating_add(1)));
                }
                let record = PageRecord {
                    start,
                    cut: trial.cut,
                    incoming: state.clone(),
                    state: Arc::new(trial.state),
                    areas: trial.areas.clone(),
                    notes: trial.notes,
                    warnings: trial.warnings,
                };
                start = trial.cut;
                past.extend(trial.areas);
                state = record.state.clone();
                let rejoin = old.and_then(|(before, trace)| trace.pages.get(index).map(|p| (before, p))).is_some_and(|(before, p)| {
                    p.start == record.start
                            && p.cut == record.cut
                            && p.state.progress() == record.state.progress()
                            // Matching this page cannot conceal another edit farther down the story.
                            && same_main_tail(before.story, story, record.cut)
                }) && !state.pending_indices().any(|k| changed.contains(&k));
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
    let refs: Vec<_> = story.text.match_indices(designcraft_doc::FOOTNOTE_REF).map(|(at, _)| at).collect();
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
    if state.pending_series().next().is_some() {
        warnings.push("Inset commentary remains after the final frame".into());
    }
    out.frames = merged;
    out.overset_at = (start < story.text.len()).then_some(start);
    out.warnings.extend(warnings);
    (out, Some(crate::reuse::Trace::inset(Trace { pages })))
}

#[cfg(test)]
mod search_tests {
    use super::*;
    #[test]
    fn nonmonotone_search_retains_only_a_measured_feasible_layout_within_budget() {
        for count in 1..33_usize {
            let keys: Vec<_> = (1..=count).collect();
            let budget = usize::BITS as usize - count.saturating_sub(1).leading_zeros() as usize + 2;
            let mut measured = Vec::new();
            let (best, used) = search(&keys, budget, |key| {
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
