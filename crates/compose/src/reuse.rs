//! Composing an edited story from the composition it had before the edit.
//!
//! A composition that laid every paragraph once leaves a [`Trace`]: the state it had before each
//! paragraph. Composing the story again after an edit starts at the paragraph before the first
//! changed one, in the state the trace holds for it and with the lines above it copied. It stops
//! at the first paragraph after the edit that it reaches in the state the trace holds for that
//! paragraph, and copies the lines from there on with their story offsets moved by the length of
//! the edit. The result is the composition a full pass over the edited story gives.
//!
//! Only plain stories are traced ([`plain`]): the features left out carry state between
//! paragraphs that the trace does not hold, or move lines after they are set. A composition that
//! goes back over a paragraph (keep options, column balancing, hyphenation across columns)
//! leaves no trace, and one that does so while it reuses a trace is abandoned for a full pass.

use std::rc::Rc;

use designcraft_doc::{CharRun, ListCounter, Story, VerticalJustification};

use crate::{Above, Band, ComposeOptions, ComposedStory, Cursor, FrameSpec, Line, ParaInfo, RunStyle, Snapshot};

#[cfg(test)]
thread_local! {
    /// Paragraphs shaped on this thread (tests count the work an edit causes).
    pub(crate) static SHAPED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Paragraph styles the passes over a whole story resolved on this thread.
    pub(crate) static RESOLVED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The composition's state before a paragraph: the cursor, the list numbering, and how much of
/// the cursor's frame and of the run style table is filled.
#[derive(Clone, Debug)]
struct Mark {
    fi: usize,
    col: usize,
    last_baseline: Option<f64>,
    last_descent: f64,
    last_reference: f64,
    aki_below: Option<f64>,
    pending: f64,
    band: Band,
    grid_next: Option<(f64, f64)>,
    list_counter: ListCounter,
    lines: usize,
    decos: usize,
    styles: usize,
}

fn bits(v: Option<f64>) -> Option<u64> {
    v.map(f64::to_bits)
}

fn above_bits(a: Option<Above>) -> Option<[Option<u64>; 6]> {
    a.map(|a| {
        [
            Some(a.top.to_bits()),
            Some(a.baseline.to_bits()),
            Some(a.descent.to_bits()),
            Some(a.reference.to_bits()),
            bits(a.aki_below),
            Some(a.pending.to_bits()),
        ]
    })
}

impl Mark {
    /// The state `snap` holds with `styles` run styles in the table, or `None` when it has state
    /// a mark does not hold (a split-column block, balanced column bottoms, tables).
    fn of(snap: &Snapshot, styles: usize) -> Option<Mark> {
        let c = &snap.cur;
        if c.split.is_some() || !c.limits.is_empty() || !c.split_limits.is_empty() || snap.tables != 0 {
            return None;
        }
        Some(Mark {
            fi: c.fi,
            col: c.col,
            last_baseline: c.last_baseline,
            last_descent: c.last_descent,
            last_reference: c.last_reference,
            aki_below: c.aki_below,
            pending: c.pending,
            band: c.band,
            grid_next: c.grid_next,
            list_counter: snap.list_counter.clone(),
            lines: snap.lines,
            decos: snap.decos,
            styles,
        })
    }

    fn cursor(&self, pi: usize) -> Cursor {
        Cursor {
            fi: self.fi,
            col: self.col,
            last_baseline: self.last_baseline,
            last_descent: self.last_descent,
            last_reference: self.last_reference,
            aki_below: self.aki_below,
            pending: self.pending,
            pi,
            band: self.band,
            limits: Rc::default(),
            split: None,
            split_limits: Rc::default(),
            grid_next: self.grid_next,
        }
    }

    /// The mark with its paragraph numbers after the edit.
    fn moved(&self, plan: &Plan) -> Mark {
        let band = Band { para: plan.moved(self.band.para), tail: self.band.tail.map(|(p, c)| (plan.moved(p), c)), ..self.band };
        Mark { band, ..self.clone() }
    }

    /// Whether composing on from `self` and from `other` sets the same lines. Numbers are
    /// compared by their bits: a difference too small to show still changes what follows.
    fn same(&self, other: &Mark) -> bool {
        let (a, b) = (&self.band, &other.band);
        self.fi == other.fi
            && self.col == other.col
            && bits(self.last_baseline) == bits(other.last_baseline)
            && self.last_descent.to_bits() == other.last_descent.to_bits()
            && self.last_reference.to_bits() == other.last_reference.to_bits()
            && bits(self.aki_below) == bits(other.aki_below)
            && self.pending.to_bits() == other.pending.to_bits()
            && a.para == b.para
            && a.line0 == b.line0
            && above_bits(a.above) == above_bits(b.above)
            && a.tail == b.tail
            && self.grid_next.map(|(x, y)| (x.to_bits(), y.to_bits())) == other.grid_next.map(|(x, y)| (x.to_bits(), y.to_bits()))
            && self.list_counter == other.list_counter
            && self.lines == other.lines
            && self.decos == other.decos
            && self.styles == other.styles
    }
}

/// What a composition leaves for composing the story again after an edit: its state before each
/// paragraph it laid (and after the last one), and where each paragraph sits.
#[derive(Debug)]
pub(crate) struct Trace {
    marks: Vec<Mark>,
    info: Vec<ParaInfo>,
}

/// A story as it was before an edit, its composition and that composition's trace.
#[derive(Clone, Copy)]
pub(crate) struct Reuse<'a> {
    pub story: &'a Story,
    pub composed: &'a ComposedStory,
    pub trace: &'a Trace,
}

/// How much of the composition before an edit holds for the edited story.
pub(crate) struct Plan {
    /// The paragraph composing starts at.
    pub resume: usize,
    /// The first paragraph (numbered as in the edited story) that may take the lines it had
    /// before the edit: the one before it is unchanged too.
    pub rejoin: usize,
    /// Paragraphs and bytes the edit added.
    paras: isize,
    bytes: isize,
    /// The first paragraph the edit left as it was, numbered as before the edit.
    old_tail: usize,
}

impl Plan {
    /// The number after the edit of paragraph `p` of the story before it.
    fn moved(&self, p: usize) -> usize {
        if p >= self.old_tail { p.wrapping_add_signed(self.paras) } else { p }
    }

    /// The number before the edit of paragraph `pi` of the edited story's unchanged end.
    fn before(&self, pi: usize) -> Option<usize> {
        pi.checked_add_signed(self.paras.checked_neg()?)
    }
}

/// Whether a composition of `story` in `frames` can leave a trace, or use one. Footnotes,
/// endnotes, tables and anchored objects carry state of their own; named lists and tracked
/// deletions reach across paragraphs; vertical justification and column rules change a frame
/// after its lines are set; type on a path sizes its frame from the story; balanced columns are
/// laid more than once.
pub(crate) fn plain(story: &Story, frames: &[FrameSpec], opts: &ComposeOptions, labels: &[Option<String>], deleted: &[bool]) -> bool {
    story.notes.is_empty()
        && story.endnotes.is_empty()
        && story.objects.is_empty()
        && story.tables.is_empty()
        && opts.label.is_none()
        && labels.iter().all(Option::is_none)
        && !deleted.contains(&true)
        && frames.iter().all(|f| {
            let o = &f.opts;
            o.vertical_justification == VerticalJustification::Top && !o.column_rule && o.path.is_none() && !(o.balance_columns && o.columns > 1)
        })
}

/// Whether the paragraphs a composition laid are ones a trace covers: none spans or splits
/// columns (`span`, `split`, by paragraph) or has a border or shading (`boxes`), which are drawn
/// once every line is set.
pub(crate) fn plain_end<T>(span: &[bool], split: &[bool], boxes: &[Option<T>]) -> bool {
    !span.contains(&true) && !split.contains(&true) && boxes.iter().all(Option::is_none)
}

/// Bytes of text from the start of both run lists (in the order given) that have the same
/// character formats.
fn same_formats<'a>(mut old: impl Iterator<Item = &'a CharRun>, mut new: impl Iterator<Item = &'a CharRun>) -> usize {
    let mut same = 0usize;
    let (mut a, mut b): (Option<(&CharRun, usize)>, Option<(&CharRun, usize)>) = (None, None);
    loop {
        if a.is_none_or(|(_, left)| left == 0) {
            a = old.find(|r| r.len > 0).map(|r| (r, r.len));
        }
        if b.is_none_or(|(_, left)| left == 0) {
            b = new.find(|r| r.len > 0).map(|r| (r, r.len));
        }
        let (Some((ra, la)), Some((rb, lb))) = (a, b) else { return same };
        if ra.format != rb.format {
            return same;
        }
        let step = la.min(lb);
        same += step;
        (a, b) = (Some((ra, la - step)), Some((rb, lb - step)));
    }
}

fn returns(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count()
}

impl Reuse<'_> {
    /// Where composing `new` (the story after the edit) starts and may stop, or `None` when the
    /// edit changed something a trace does not cover.
    pub(crate) fn plan(&self, new: &Story) -> Option<Plan> {
        let old = self.story;
        if old.frames != new.frames
            || old.direction != new.direction
            || old.vertical != new.vertical
            || old.optical_margin != new.optical_margin
            || old.anchors != new.anchors
            || old.xrefs != new.xrefs
            || old.index_refs != new.index_refs
            || old.editorial != new.editorial
        {
            return None;
        }
        let (o, n) = (old.text.as_bytes(), new.text.as_bytes());
        if old.paras.len() != returns(&old.text) + 1 || new.paras.len() != returns(&new.text) + 1 {
            return None;
        }
        // The unchanged start: the same text in the same character formats, then whole paragraphs
        // with the same paragraph formats.
        let mut head = o.iter().zip(n).take_while(|(a, b)| a == b).count().min(same_formats(old.chars.iter(), new.chars.iter()));
        while !new.text.is_char_boundary(head) {
            head -= 1;
        }
        let whole = returns(new.text.get(..head)?);
        let first = old.paras.iter().zip(&new.paras).take(whole).take_while(|(a, b)| a == b).count();
        if first < whole {
            // A paragraph format changed above the first changed character.
            head = match first.checked_sub(1) {
                Some(k) => n.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(k).map(|(i, _)| i + 1)?,
                None => 0,
            };
        }
        // The unchanged end, which the unchanged start does not overlap.
        let room = o.len().min(n.len()).checked_sub(head)?;
        let mut tail = o
            .iter()
            .rev()
            .zip(n.iter().rev())
            .take_while(|(a, b)| a == b)
            .count()
            .min(same_formats(old.chars.iter().rev(), new.chars.iter().rev()))
            .min(room);
        while !new.text.is_char_boundary(n.len() - tail) {
            tail -= 1;
        }
        let after = returns(new.text.get(n.len() - tail..)?);
        let after = old.paras.iter().rev().zip(new.paras.iter().rev()).take(after).take_while(|(a, b)| a == b).count();
        let (old_paras, new_paras) = (old.paras.len(), new.paras.len());
        Some(Plan {
            // One paragraph above the first changed one: setting a paragraph reads the one after it.
            resume: first.saturating_sub(1).min(self.trace.marks.len().checked_sub(1)?),
            rejoin: new_paras.checked_sub(after)? + 1,
            paras: isize::try_from(new_paras).ok()?.checked_sub(isize::try_from(old_paras).ok()?)?,
            bytes: isize::try_from(n.len()).ok()?.checked_sub(isize::try_from(o.len()).ok()?)?,
            old_tail: old_paras.checked_sub(after)?,
        })
    }

    /// Put `out` and `styles` back as they were before paragraph `plan.resume`, and give the
    /// cursor, list numbering and paragraph positions of that moment. `None` when the composition
    /// before the edit does not match `out`'s frames.
    pub(crate) fn restore(&self, plan: &Plan, out: &mut ComposedStory, styles: &mut Vec<RunStyle>) -> Option<(Cursor, ListCounter, Vec<ParaInfo>)> {
        let mark = self.trace.marks.get(plan.resume)?;
        let before = self.composed;
        if before.frames.len() != out.frames.len() || before.frames.iter().zip(&out.frames).any(|(a, b)| a.frame != b.frame) {
            return None;
        }
        let info = self.trace.info.get(..plan.resume)?.to_vec();
        *styles = before.styles.get(..mark.styles)?.to_vec();
        for (i, (to, from)) in out.frames.iter_mut().zip(&before.frames).enumerate().take(mark.fi.saturating_add(1)) {
            let (lines, decos) = if i < mark.fi { (from.lines.len(), from.decos.len()) } else { (mark.lines, mark.decos) };
            to.lines = from.lines.get(..lines)?.iter().map(|l| moved_line(l, 0, 0)).collect();
            to.decos = from.decos.get(..decos)?.to_vec();
        }
        Some((mark.cursor(plan.resume), mark.list_counter.clone(), info))
    }

    /// At paragraph `pi` of the edited story, in the state `snap` with `styles` and `info` as
    /// they are: when the composition before the edit was in that state before the same
    /// paragraph, copy what it set from there on into `out`, `styles` and `info`, and say so.
    ///
    /// The copied lines keep their run-style indexes, so this needs `styles` to equal the old
    /// table up to that point. A run style that the edit uses for the first time takes a new
    /// index, and the story then composes on until the old composition added the same style at
    /// the same index, or to its end. The result is the same; only the time differs.
    pub(crate) fn rejoin(
        &self,
        plan: &Plan,
        pi: usize,
        snap: &Snapshot,
        out: &mut ComposedStory,
        styles: &mut Vec<RunStyle>,
        info: &mut Vec<ParaInfo>,
    ) -> bool {
        let before = self.composed;
        let Some(k) = plan.before(pi) else { return false };
        let (Some(old), Some(new)) = (self.trace.marks.get(k), Mark::of(snap, styles.len())) else { return false };
        // The paragraph above is where it was, and no chain of keep-with-next paragraphs reaches
        // back through it into the changed text.
        let last = |v: &[ParaInfo], i: usize| i.checked_sub(1).and_then(|i| v.get(i).cloned());
        let same_above = matches!((last(&self.trace.info, k), last(info, pi)), (Some(a), Some(b)) if a == b && a.keep_with_next == 0);
        if !same_above || info.len() != pi || !old.moved(plan).same(&new) || before.styles.get(..old.styles) != Some(styles.as_slice()) {
            return false;
        }
        for (i, (to, from)) in out.frames.iter_mut().zip(&before.frames).enumerate().skip(old.fi) {
            let (lines, decos) = if i == old.fi { (old.lines, old.decos) } else { (0, 0) };
            to.lines.extend(from.lines.get(lines..).unwrap_or_default().iter().map(|l| moved_line(l, plan.bytes, plan.paras)));
            to.decos.extend(from.decos.get(decos..).unwrap_or_default().iter().cloned());
        }
        out.overset_at = before.overset_at.map(|b| b.wrapping_add_signed(plan.bytes));
        styles.clone_from(&before.styles);
        info.extend(self.trace.info.get(k..).unwrap_or_default().iter().cloned());
        true
    }
}

/// `line` with its story offsets and paragraph number moved, and without the keep-violation
/// flag, which is set again for the whole story once it is composed.
fn moved_line(line: &Line, bytes: isize, paras: isize) -> Line {
    let at = |b: usize| b.wrapping_add_signed(bytes);
    let mut l = Line { keep_violation: false, ..line.clone() };
    l.range = at(l.range.start)..at(l.range.end);
    l.para = l.para.wrapping_add_signed(paras);
    if bytes != 0 {
        for g in &mut l.glyphs {
            g.byte = at(g.byte);
        }
    }
    if let Some(cap) = &mut l.drop_cap {
        cap.end = at(cap.end);
    }
    l
}

/// What one composition keeps for its trace, and what it reuses of the composition before an
/// edit.
pub(crate) struct Pass<'a> {
    before: Option<(&'a Reuse<'a>, Plan)>,
    /// The composition was to reuse the one before an edit and cannot: its result is not used.
    abandoned: bool,
    /// A trace is wanted and nothing rules one out so far.
    traced: bool,
    /// Run styles in the table before each paragraph.
    styles: Vec<usize>,
    /// The paragraph the next step should be at: another one means the layout went back over a
    /// paragraph.
    next: usize,
    /// The paragraph the lines copied from before the edit start at.
    rejoined: Option<usize>,
}

/// Where a composition starts: the paragraph, the cursor, the list numbering and the positions
/// of the paragraphs above.
pub(crate) struct Start {
    pub pi: usize,
    pub cur: Cursor,
    pub list_counter: ListCounter,
    pub info: Vec<ParaInfo>,
}

impl<'a> Pass<'a> {
    /// The bookkeeping for composing `story`, and where to start. `traced`: a trace is wanted and
    /// the story is [`plain`]. `fresh` is the start of the story. With `before`, `out` and
    /// `styles` are filled with what holds from before the edit and the start is the paragraph
    /// above the edit; when `before` cannot be used the pass is abandoned at its first step.
    pub(crate) fn new(
        before: Option<&'a Reuse<'a>>,
        story: &Story,
        traced: bool,
        fresh: Start,
        out: &mut ComposedStory,
        styles: &mut Vec<RunStyle>,
    ) -> (Pass<'a>, Start) {
        let mut pass = Pass { before: None, abandoned: false, traced, styles: Vec::with_capacity(story.paras.len() + 1), next: 0, rejoined: None };
        let Some(b) = before else { return (pass, fresh) };
        let resumed = traced.then(|| b.plan(story)).flatten().and_then(|plan| Some((b.restore(&plan, out, styles)?, plan)));
        let Some(((cur, list_counter, info), plan)) = resumed else {
            pass.abandoned = true;
            return (pass, fresh);
        };
        let pi = plan.resume;
        // The counts for the paragraphs above are never read: their marks are kept as they are.
        pass.styles.resize(pi, 0);
        pass.next = pi;
        pass.before = Some((b, plan));
        (pass, Start { pi, cur, list_counter, info })
    }

    /// Before paragraph `pi` of `count` is laid, in the state `snap`: whether nothing is left to
    /// lay, because the rest of the story is in place (copied from the composition before the
    /// edit into `out`, `styles` and `info`) or because the pass is abandoned.
    pub(crate) fn step(
        &mut self,
        (pi, count): (usize, usize),
        snap: Option<&Snapshot>,
        out: &mut ComposedStory,
        styles: &mut Vec<RunStyle>,
        info: &mut Vec<ParaInfo>,
    ) -> bool {
        self.styles.truncate(pi);
        self.styles.push(styles.len());
        if pi != self.next {
            // The layout went back over a paragraph.
            self.traced = false;
            self.abandoned |= self.before.is_some();
        }
        self.next = pi + 1;
        if self.abandoned {
            return true;
        }
        if pi < count
            && let (Some((before, plan)), Some(snap)) = (&self.before, snap)
            && pi >= plan.rejoin
            && before.rejoin(plan, pi, snap, out, styles, info)
        {
            self.rejoined = Some(pi);
            return true;
        }
        false
    }

    /// The trace of the finished composition, when it laid every paragraph once and `plain_end`
    /// (no paragraph it laid uses something a trace does not cover): `snaps` holds the state
    /// before each paragraph it laid itself, `info` every paragraph's position. The paragraphs
    /// above the one a reusing composition started at keep the marks they had, and those after
    /// the one it rejoined at the marks that followed before the edit. The outer `None` is for an
    /// abandoned pass and for a reusing one that leaves no trace: the story has to be composed
    /// from its start.
    pub(crate) fn trace(self, plain_end: bool, snaps: &[Snapshot], info: Vec<ParaInfo>) -> Option<Option<Trace>> {
        if self.abandoned {
            return None;
        }
        let trace = if self.traced && plain_end { self.marks(snaps).map(|marks| Trace { marks, info }) } else { None };
        if self.before.is_some() && trace.is_none() {
            return None;
        }
        Some(trace)
    }

    fn marks(&self, snaps: &[Snapshot]) -> Option<Vec<Mark>> {
        let reused = self.before.as_ref();
        let start = reused.map_or(0, |(_, plan)| plan.resume);
        let mut marks = Vec::with_capacity(snaps.len());
        if let Some((before, _)) = reused {
            marks.extend(before.trace.marks.get(..start)?.iter().cloned());
        }
        for (snap, styles) in snaps.iter().zip(&self.styles).skip(start) {
            marks.push(Mark::of(snap, *styles)?);
        }
        if let (Some((before, plan)), Some(pi)) = (reused, self.rejoined) {
            let k = plan.before(pi)?;
            marks.extend(before.trace.marks.get(k + 1..)?.iter().map(|m| m.moved(plan)));
        }
        // Lines are only ever added to the cursor's frame, so the frames above it are complete.
        marks.windows(2).all(|w| w[0].fi <= w[1].fi).then_some(marks)
    }
}

#[cfg(test)]
mod tests {
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::{Align, Document, ItemId, ListType, ParaFormat, SpreadRef, StartParagraph, Story, StoryId, VerticalJustification};
    use designcraft_geom::Rect;

    use super::{RESOLVED, SHAPED};
    use crate::Cache;

    const SENTENCE: &str = "The quick brown fox jumps over the lazy dog while the workbook keeps growing page by page.";

    /// A story of `paras` paragraphs of two to five lines, threaded through `frames` frames of 31
    /// lines each.
    fn threaded(paras: usize, frames: usize) -> (Document, StoryId, Vec<ItemId>) {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let text = (0..paras).map(|i| vec![SENTENCE; 2 + i % 4].join(" ")).collect::<Vec<_>>().join("\n");
        let rect = |i: usize| Rect::new(i as f64 * 600.0, 0.0, i as f64 * 600.0 + 540.0, 456.0);
        let (first, sid) = d.add_text_frame(SpreadRef::Doc(0), rect(0), lid, &text, ParaFormat::default()).unwrap();
        let mut ids = vec![first];
        for i in 1..frames {
            let (next, _) = d.add_text_frame(SpreadRef::Doc(0), rect(i), lid, "", ParaFormat::default()).unwrap();
            d.thread(ids[i - 1], next).unwrap();
            ids.push(next);
        }
        (d, sid, ids)
    }

    /// What `f` returns and the paragraphs it shaped.
    fn shaped<T>(f: impl FnOnce() -> T) -> (T, usize) {
        SHAPED.with(|n| n.set(0));
        let out = f();
        (out, SHAPED.with(|n| n.get()))
    }

    /// Compose the edited story through `cache`, which holds its composition before the edit, and
    /// check the result against composing it whole. Gives the paragraphs the cache shaped.
    fn check(cache: &Cache, d: &Document, sid: StoryId, what: &str) -> usize {
        let (cached, n) = shaped(|| cache.get(d, sid, None));
        let whole = Cache::new().get(d, sid, None);
        assert_eq!(cached.frames.len(), whole.frames.len(), "{what}");
        for (fi, (a, b)) in cached.frames.iter().zip(&whole.frames).enumerate() {
            assert_eq!(a.lines.len(), b.lines.len(), "{what}: lines in frame {fi}");
            for (li, (x, y)) in a.lines.iter().zip(&b.lines).enumerate() {
                assert_eq!(format!("{x:?}"), format!("{y:?}"), "{what}: frame {fi} line {li}");
            }
        }
        assert!(format!("{cached:?}") == format!("{whole:?}"), "{what}: the composition differs from composing the story whole");
        n
    }

    #[test]
    fn a_typed_character_composes_the_paragraphs_around_it() {
        for paras in [60, 240] {
            let (mut d, sid, _) = threaded(paras, paras / 4);
            let cache = Cache::new();
            let (first, whole) = shaped(|| cache.get(&d, sid, None));
            assert!(!first.is_overset());
            assert!(first.frames.iter().filter(|f| !f.lines.is_empty()).count() > paras / 12);
            assert_eq!(whole, paras);
            let len = d.story(sid).unwrap().len();
            for at in [10, len / 2, len - 1] {
                d.story_mut(sid).unwrap().insert(at, "x");
                let n = check(&cache, &d, sid, "a typed character");
                assert!(n <= 3, "{n} of {paras} paragraphs composed for a character typed at {at}");
            }
        }
    }

    /// The passes over every paragraph that composing makes besides laying them (named-list
    /// labels, the balancing budgets, keep violations) resolve one style for each run of
    /// paragraphs with the same format.
    #[test]
    fn a_typed_character_resolves_one_style_per_run_of_paragraph_formats() {
        let (mut d, sid, _) = threaded(240, 60);
        // Every tenth paragraph centered: 24 centered paragraphs and 24 runs of nine others.
        let starts: Vec<usize> = d.story(sid).unwrap().para_ranges().iter().step_by(10).map(|r| r.start).collect();
        for at in starts {
            d.story_mut(sid).unwrap().format_paras(at..at, |p| p.para.align = Some(Align::Center));
        }
        let cache = Cache::new();
        assert!(!cache.get(&d, sid, None).is_overset());
        let len = d.story(sid).unwrap().len();
        d.story_mut(sid).unwrap().insert(len / 2, "x");
        RESOLVED.with(|n| n.set(0));
        let ((), laid) = shaped(|| drop(cache.get(&d, sid, None)));
        let resolved = RESOLVED.with(|n| n.get());
        assert!(laid <= 3, "{laid} of 240 paragraphs composed for a typed character");
        assert!(resolved <= 2 * 48, "{resolved} paragraph styles resolved for 48 runs of formats in 240 paragraphs");
        check(&cache, &d, sid, "a typed character");
    }

    /// The preflight cache reads an unchanged composition through its base, while edits
    /// composed in the reader keep their own incremental trace and never replace the base.
    #[test]
    fn a_read_through_cache_keeps_incremental_edits_local() {
        use std::sync::Arc;
        let (mut d, sid, _) = threaded(60, 15);
        let original = d.clone();
        let base = Arc::new(Cache::new());
        let first = base.get(&original, sid, None);
        let reader = Cache::over(Arc::clone(&base));
        assert!(Arc::ptr_eq(&reader.get(&d, sid, None), &first));
        d.story_mut(sid).unwrap().insert(10, "x");
        check(&reader, &d, sid, "first edit in reader");
        d.story_mut(sid).unwrap().insert(11, "y");
        let n = check(&reader, &d, sid, "second edit in reader");
        assert!(n <= 3, "reader shaped {n} paragraphs instead of reusing its trace");
        assert!(Arc::ptr_eq(&base.get(&original, sid, None), &first));
        assert!(base.composed_for(&d, sid).is_none());
    }

    #[test]
    fn characters_typed_one_after_another_each_compose_the_paragraphs_around_them() {
        let (mut d, sid, _) = threaded(80, 20);
        let cache = Cache::new();
        cache.get(&d, sid, None);
        let at = d.story(sid).unwrap().len() / 2;
        for (i, c) in "one two\nthree".chars().enumerate() {
            d.story_mut(sid).unwrap().insert(at + i, &c.to_string());
            let n = check(&cache, &d, sid, "typing");
            // A return adds a line, which moves the paragraphs below it in their frame: they are
            // composed up to the end of the story.
            assert!(c == '\n' || n <= 4, "{n} of 80 paragraphs composed for {c:?}");
        }
    }

    #[test]
    fn an_edit_that_moves_lines_composes_up_to_the_next_frame_the_text_starts_in() {
        let (mut d, sid, _) = threaded(100, 24);
        // Chapters of five paragraphs, each starting in the next frame.
        let starts: Vec<usize> =
            d.story(sid).unwrap().para_ranges().iter().enumerate().filter(|(i, _)| *i > 0 && i % 5 == 0).map(|(_, r)| r.start).collect();
        for at in starts {
            d.story_mut(sid).unwrap().format_paras(at..at, |p| p.para.start_paragraph = Some(StartParagraph::NextFrame));
        }
        let cache = Cache::new();
        let (first, whole) = shaped(|| cache.get(&d, sid, None));
        assert!(!first.is_overset());
        assert_eq!(whole, 100);
        let at = d.story(sid).unwrap().para_ranges()[52].start + 20;
        d.story_mut(sid).unwrap().insert(at, &format!(" {SENTENCE} {SENTENCE}"));
        let n = check(&cache, &d, sid, "two more lines in a chapter");
        assert!(n <= 6, "{n} of 100 paragraphs composed for lines added to one chapter");
    }

    #[test]
    fn edits_give_the_composition_of_the_edited_story() {
        let (mut d, sid, _) = threaded(40, 12);
        let original = d.clone();
        let cache = Cache::new();
        cache.get(&d, sid, None);
        let edits: [(&str, fn(&mut Story)); 15] = [
            ("a character", |s| s.insert(s.len() / 2, "x")),
            ("a return", |s| s.insert(s.len() / 2, "\n")),
            ("a long insertion", |s| s.insert(s.len() / 3, &format!(" {SENTENCE} {SENTENCE} {SENTENCE}"))),
            ("several paragraphs", |s| s.insert(s.len() / 3, &format!("\n{SENTENCE}\n{SENTENCE}\n"))),
            ("a deleted character", |s| s.delete(s.len() / 2..s.len() / 2 + 1)),
            ("a deleted return", |s| {
                let at = s.text[..s.len() / 2].rfind('\n').unwrap();
                s.delete(at..at + 1);
            }),
            ("a deletion across paragraphs", |s| s.delete(s.len() / 4..s.len() / 4 + 900)),
            ("a replacement", |s| s.replace(s.len() / 2..s.len() / 2 + 40, "replaced")),
            ("larger type", |s| s.format_chars(s.len() / 2..s.len() / 2 + 60, |f| f.over.size = Some(18.0))),
            ("a centered paragraph", |s| s.format_paras(s.len() / 2..s.len() / 2, |p| p.para.align = Some(Align::Center))),
            ("text at the start", |s| s.insert(0, "Start. ")),
            ("text at the end", |s| s.insert(s.len(), " End.")),
            ("a two-byte character", |s| s.insert(s.len() / 2, "\u{e9}")),
            ("everything deleted", |s| s.delete(0..s.len())),
            ("text in an empty story", |s| s.insert(0, "Again.")),
        ];
        for (what, edit) in edits {
            edit(d.story_mut(sid).unwrap());
            check(&cache, &d, sid, what);
        }
        // Undo puts an earlier story back.
        check(&cache, &original, sid, "the story before the edits");
    }

    #[test]
    fn edits_in_and_above_overset_text_compose_right() {
        let (mut d, sid, _) = threaded(60, 3);
        let cache = Cache::new();
        assert!(cache.get(&d, sid, None).is_overset());
        let len = d.story(sid).unwrap().len();
        d.story_mut(sid).unwrap().insert(len - 10, "x");
        let n = check(&cache, &d, sid, "a character in overset text");
        assert!(n <= 1, "{n} paragraphs composed for a character typed in overset text");
        d.story_mut(sid).unwrap().insert(200, &format!(" {SENTENCE}"));
        check(&cache, &d, sid, "a line added above overset text");
        d.story_mut(sid).unwrap().delete(100..len / 2);
        check(&cache, &d, sid, "a deletion that brings overset text into the frames");
    }

    /// Stories with features a trace does not cover are composed whole after an edit.
    #[test]
    fn edits_of_stories_a_trace_does_not_cover_compose_right() {
        let setups: [(&str, fn(&mut Document, StoryId, &[ItemId])); 6] = [
            ("keep options", |d, sid, _| {
                let s = d.story_mut(sid).unwrap();
                s.format_paras(0..s.len(), |p| {
                    p.para.keep_with_next = Some(1);
                    p.para.keep_lines_together = Some(true);
                });
            }),
            ("balanced columns", |d, _, frames| {
                for f in frames {
                    let o = &mut d.item_mut(*f).unwrap().text_frame_mut().unwrap().options;
                    (o.columns, o.balance_columns) = (2, true);
                }
            }),
            ("vertical justification", |d, _, frames| {
                for f in frames {
                    d.item_mut(*f).unwrap().text_frame_mut().unwrap().options.vertical_justification = VerticalJustification::Center;
                }
            }),
            ("a numbered list", |d, sid, _| {
                let s = d.story_mut(sid).unwrap();
                s.format_paras(0..s.len(), |p| p.para.list_type = Some(ListType::Numbers));
            }),
            ("a footnote", |d, sid, _| {
                d.story_mut(sid).unwrap().insert_note(300, "A synthetic footnote.", ParaFormat::default());
            }),
            ("a column rule", |d, _, frames| {
                for f in frames {
                    let o = &mut d.item_mut(*f).unwrap().text_frame_mut().unwrap().options;
                    (o.columns, o.column_rule) = (2, true);
                }
            }),
        ];
        for (what, setup) in setups {
            let (mut d, sid, frames) = threaded(30, 10);
            setup(&mut d, sid, &frames);
            let cache = Cache::new();
            cache.get(&d, sid, None);
            let mid = d.story(sid).unwrap().len() / 2;
            d.story_mut(sid).unwrap().insert(mid, "x");
            check(&cache, &d, sid, what);
            d.story_mut(sid).unwrap().insert(mid, &format!("\n{SENTENCE} {SENTENCE}\n"));
            check(&cache, &d, sid, what);
            d.story_mut(sid).unwrap().delete(mid / 2..mid);
            check(&cache, &d, sid, what);
        }
    }

    #[test]
    fn a_run_of_mixed_edits_composes_right_after_each() {
        let (mut d, sid, _) = threaded(40, 14);
        let cache = Cache::new();
        cache.get(&d, sid, None);
        let texts = ["x", " word", "\n", " several words in a row here", "\n\n", "A sentence. "];
        let mut seed = 0x2545_f491_u64;
        let mut next = |below: usize| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize % below.max(1)
        };
        for step in 0..60 {
            let s = d.story_mut(sid).unwrap();
            let at = next(s.len() + 1);
            match next(4) {
                0 | 1 => s.insert(at, texts[next(texts.len())]),
                2 => s.delete(at..(at + next(120)).min(s.len())),
                _ => {
                    let size = 9.0 + next(10) as f64;
                    s.format_chars(at..(at + next(60)).min(s.len()), |f| f.over.size = Some(size));
                }
            }
            check(&cache, &d, sid, &format!("step {step}"));
        }
    }
}
