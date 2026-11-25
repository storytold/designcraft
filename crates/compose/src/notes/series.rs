//! Body-facing note state. Each series retains its own numbering and split continuation.
use super::{
    Arc, ComposeOptions, ComposedStory, Document, FrameSpec, Glyph, HashMap, HashSet, NoteColumn, PlacedNote, Point, Range, Rect, SeriesNotes,
    SeriesSnapshot, Story, SubstCtx, stack_height,
};
use designcraft_doc::{MarginSide, MarginStack, NotePlacement};

pub(crate) type NoteSnapshot = Vec<SeriesSnapshot>;
pub(crate) struct SplitPlan(Vec<(usize, super::SplitPlan)>);

pub(crate) struct Notes {
    series: Vec<SeriesNotes>,
    nested: bool,
    floor: HashMap<usize, f64>,
}

impl Notes {
    pub fn new(doc: &Document, story: &Story, frames: &[FrameSpec]) -> Self {
        // Cell and note compositions have no page rectangle and never run a margin pass.
        let nested = frames.iter().all(|f| f.page_rect.is_none());
        let mut ids = vec![0];
        if !nested {
            ids.extend(doc.note_series.iter().map(|s| s.id).filter(|id| *id != 0));
        }
        Self { series: ids.into_iter().map(|id| SeriesNotes::new(doc, story, id, nested)).collect(), nested, floor: HashMap::new() }
    }

    fn at_foot(&self, doc: &Document, i: usize) -> bool {
        self.nested || matches!(doc.note_options(self.series[i].series).placement, NotePlacement::ColumnFoot)
    }

    pub fn snapshot(&self) -> NoteSnapshot {
        self.series.iter().map(|s| (s.num, s.key, s.placed.len(), s.carry.clone())).collect()
    }

    pub fn restore(&mut self, snapshot: &NoteSnapshot) {
        for (s, (num, key, placed, carry)) in self.series.iter_mut().zip(snapshot) {
            (s.num, s.key) = (*num, *key);
            s.placed.truncate(*placed);
            s.carry = carry.clone();
        }
    }

    pub fn active(&self) -> bool {
        self.series.iter().any(SeriesNotes::active)
    }

    pub fn number_refs(&mut self, doc: &Document, range: Range<usize>, page: Option<usize>, sub: &mut SubstCtx) {
        for s in &mut self.series {
            s.number_refs(doc, range.clone(), page, sub);
        }
    }

    pub fn refs_in(&self, glyphs: &[Glyph]) -> Vec<usize> {
        // Return all references. The placement methods select column-foot series only.
        let mut refs: Vec<_> = self.series.iter().flat_map(|s| s.refs_in(glyphs)).collect();
        refs.sort_unstable();
        refs
    }

    pub fn area(&self, cols: &[Rect], col: usize) -> Option<Rect> {
        cols.get(col).copied()
    }

    /// Replay the same per-series areas as finish: a spanning group starts above the
    /// deepest preceding local stack, so every column reserves that displacement too.
    fn reservations(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], opts: &ComposeOptions) -> (f64, Vec<f64>) {
        let count = column.frame.columns().len();
        let mut heights = vec![0.0_f64; count];
        let mut own_heights = vec![0.0; self.series.len()];
        for (i, own_height) in own_heights.iter_mut().enumerate() {
            if !self.at_foot(doc, i) {
                continue;
            }
            let s = &mut self.series[i];
            let own: Vec<_> = extra.iter().copied().filter(|k| s.owns(*k)).collect();
            let height = s.reserve(doc, s.column(column), &own, opts);
            *own_height = height;
            if s.span {
                if height > 0.0 {
                    let offset = heights.iter().copied().fold(0.0, f64::max);
                    heights.fill(offset + height);
                }
            } else {
                for (ci, total) in heights.iter_mut().enumerate() {
                    let o = doc.note_options(s.series);
                    let ms: Vec<_> = s.placed.iter().filter(|p| s.same_area(p, column.frame_index, ci)).map(|p| p.m).collect();
                    *total += if ci == column.column_index {
                        height
                    } else if ms.is_empty() {
                        0.0
                    } else {
                        stack_height(&ms, o.space_between) + o.space_before
                    };
                }
            }
        }
        (heights.get(column.column_index).copied().unwrap_or(0.0), own_heights)
    }

    pub fn reserve(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], opts: &ComposeOptions) -> f64 {
        self.reservations(doc, column, extra, opts).0.max(self.floor.get(&column.frame_index).copied().unwrap_or(0.0))
    }

    pub fn split_plan(
        &mut self,
        doc: &Document,
        column: NoteColumn<'_>,
        extra: &[usize],
        available: f64,
        opts: &ComposeOptions,
    ) -> Option<SplitPlan> {
        // A rewind preserves the space needed by a later spanning stack. Splitting a local
        // note must not admit a body line into that space before the stack is replayed.
        if available + 0.01 < self.floor.get(&column.frame_index).copied().unwrap_or(0.0) {
            return None;
        }
        let (total, reserves) = self.reservations(doc, column, extra, opts);
        // Split the last series needing room. Others must fit intact before it.
        for i in (0..self.series.len()).rev() {
            if reserves[i] <= 0.0 {
                continue;
            }
            let s = &mut self.series[i];
            let own: Vec<_> = extra.iter().copied().filter(|k| s.owns(*k)).collect();
            let room = available - (total - reserves[i]) - doc.note_options(s.series).space_before;
            if let Some(plan) = s.split_plan(doc, s.column(column), &own, room, opts) {
                return Some(SplitPlan(vec![(i, plan)]));
            }
        }
        None
    }

    pub fn earlier_columns_collide(&mut self, lines: &[crate::Line], fi: usize, col: usize, cols: &[Rect], reserve: f64) -> bool {
        if col == 0
            || !self.series.iter().any(|s| s.span && s.placed.iter().any(|p| p.fi == fi))
            || reserve <= self.floor.get(&fi).copied().unwrap_or(0.0) + 0.01
        {
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

    pub fn place_line(&mut self, doc: &Document, column: NoteColumn<'_>, extra: &[usize], plan: Option<SplitPlan>, opts: &ComposeOptions) -> bool {
        let mut plans = plan.map_or_else(Vec::new, |p| p.0);
        let mut placed = false;
        for i in 0..self.series.len() {
            if !self.at_foot(doc, i) {
                continue;
            }
            let s = &mut self.series[i];
            let own: Vec<_> = extra.iter().copied().filter(|k| s.owns(*k)).collect();
            let plan = plans.iter().position(|(si, _)| *si == i).map(|at| plans.remove(at).1);
            placed |= s.place_line(doc, s.column(column), &own, plan, opts);
        }
        placed
    }

    pub fn finish_carry(
        &mut self,
        doc: &Document,
        frames: &[FrameSpec],
        cols: &[Vec<Rect>],
        opts: &ComposeOptions,
        out: &mut ComposedStory,
        end: usize,
    ) {
        for (fi, (frame, columns)) in frames.iter().zip(cols).enumerate() {
            for (ci, area) in columns.iter().enumerate() {
                let column = NoteColumn { frame_index: fi, column_index: ci, width: area.width(), frame };
                if !self.series.iter().enumerate().any(|(i, s)| self.at_foot(doc, i) && s.pending(column).is_some()) {
                    continue;
                }
                if self.reserve(doc, column, &[], opts) <= area.height() + 0.01 {
                    self.place_line(doc, column, &[], None, opts);
                } else if let Some(plan) = self.split_plan(doc, column, &[], area.height(), opts) {
                    self.place_line(doc, column, &[], Some(plan), opts);
                }
            }
        }
        if self.series.iter().any(|s| s.carry.is_some()) && out.overset_at.is_none() {
            out.overset_at = Some(end);
        }
    }

    pub fn finish(&mut self, doc: &Document, story: &Story, frames: &[FrameSpec], cols: &[Vec<Rect>], out: &mut ComposedStory) {
        let mut used: HashMap<(usize, usize), f64> = HashMap::new();
        for i in 0..self.series.len() {
            if !self.at_foot(doc, i) {
                continue;
            }
            let s = &mut self.series[i];
            let mut shifted = cols.to_vec();
            for (fi, columns) in shifted.iter_mut().enumerate() {
                let span_offset = columns.iter().enumerate().map(|(ci, _)| used.get(&(fi, ci)).copied().unwrap_or(0.0)).fold(0.0, f64::max);
                for (ci, col) in columns.iter_mut().enumerate() {
                    col.y1 -= if s.span { span_offset } else { used.get(&(fi, ci)).copied().unwrap_or(0.0) };
                }
            }
            s.finish(doc, story, frames, &shifted, out);
            for (fi, ft) in out.frames.iter().enumerate() {
                for note in ft.notes.iter().filter(|n| n.series == s.series) {
                    for (ci, col) in cols.get(fi).into_iter().flatten().enumerate() {
                        if s.span || note.column as usize == ci {
                            let height = col.y1 - note.rect.y0 + doc.note_options(s.series).space_before;
                            used.entry((fi, ci)).and_modify(|v| *v = v.max(height)).or_insert(height);
                        }
                    }
                }
            }
        }
    }

    pub fn finish_margins(&mut self, doc: &Document, story: &Story, frames: &[FrameSpec], opts: &ComposeOptions, out: &mut ComposedStory) {
        if self.nested {
            return;
        }
        // Sort in page coordinates, so frames on the same page share one physical margin stack.
        let mut refs = Vec::new();
        for (fi, (ft, f)) in out.frames.iter().zip(frames).enumerate() {
            let Some((page, _)) = f.page_rect else { continue };
            for (li, line) in ft.lines.iter().enumerate() {
                for (si, s) in self.series.iter().enumerate() {
                    for k in line.glyphs.iter().filter(|g| g.len > 0).filter_map(|g| s.refs.binary_search(&g.byte).ok()).filter(|k| s.owns(*k)) {
                        // refs_in checks exact marker bytes; no nested stories are scanned.
                        refs.push((f.page.unwrap_or(fi), line.baseline - page.y0, fi, li, si, k));
                    }
                }
            }
        }
        // Top stacks keep source order even when a later column has an earlier baseline.
        let top_order = |si: usize, k: usize| {
            matches!(doc.note_options(self.series[si].series).placement, NotePlacement::Margin { stack: MarginStack::Top, .. }).then_some((si, k))
        };
        refs.sort_by(|a, b| {
            let (at, bt) = (top_order(a.4, a.5), top_order(b.4, b.5));
            a.0.cmp(&b.0).then(at.is_none().cmp(&bt.is_none())).then(at.cmp(&bt)).then(a.1.total_cmp(&b.1)).then(a.5.cmp(&b.5))
        });
        let mut bottoms: HashMap<(usize, bool), f64> = HashMap::new();
        let mut seen = HashSet::new();
        for (page_index, _, fi, li, si, k) in refs {
            if !seen.insert((si, k)) {
                continue;
            }
            let s = &mut self.series[si];
            let o = doc.note_options(s.series);
            let f = &frames[fi];
            let baseline = out.frames[fi].lines[li].baseline;
            if let NotePlacement::Margin { side, gap, width, stack } = o.placement {
                let Some((page, margin)) = f.page_rect else { continue };
                if !width.is_finite() || width <= 0.0 || !gap.is_finite() || gap < 0.0 {
                    continue;
                }
                let left = match side {
                    MarginSide::Left => true,
                    MarginSide::Right => false,
                    MarginSide::Inside => !f.left_page,
                    MarginSide::Outside => f.left_page,
                };
                let (text, m) = s.compose(doc, k, width, f, opts);
                let requested = if stack == MarginStack::Top { margin.y0 } else { baseline - m.first };
                let y = bottoms.get(&(page_index, left)).map_or(requested, |bottom| requested.max(bottom + page.y0 + o.space_between));
                let bottom = y + m.last + m.descent;
                bottoms.insert((page_index, left), bottom - page.y0);
                let edge = doc.item(f.id).and_then(|item| item.path.bounds()).unwrap_or(f.area);
                let x = if left { edge.x0 - gap - width } else { edge.x1 + gap };
                let overflow = bottom > margin.y1 + 0.01;
                let Some(note) = story.notes.get(k) else { continue };
                if overflow {
                    out.warnings.push(format!(
                        "Margin note {} in series {} runs below the bottom margin on page {}",
                        note.id,
                        s.series,
                        page_index + 1
                    ));
                }
                let column = out.frames[fi].lines[li].column;
                out.frames[fi].notes.push(PlacedNote {
                    id: note.id,
                    index: k,
                    series: s.series,
                    reference_baseline: baseline,
                    overflow,
                    column,
                    label: o.label(s.numbers[k], true),
                    origin: Point::new(x, y),
                    rect: Rect::new(x, y, x + width, bottom),
                    text,
                    source: Arc::from(note.text.text.as_str()),
                });
            } else {
                for ft in &mut out.frames {
                    for n in ft.notes.iter_mut().filter(|n| n.id == story.notes[k].id && n.series == s.series) {
                        n.reference_baseline = baseline;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_plan_keeps_the_spanning_floor_after_a_rewind() {
        let mut doc = Document::new(&Default::default());
        let (fid, sid) = doc
            .add_text_frame(designcraft_doc::SpreadRef::Doc(0), Rect::new(36.0, 36.0, 436.0, 436.0), doc.default_layer(), "Body", Default::default())
            .unwrap();
        doc.item_mut(fid).unwrap().text_frame_mut().unwrap().options.columns = 2;
        doc.note_series.push(designcraft_doc::NoteSeries {
            id: 1,
            name: "Spanning".into(),
            options: designcraft_doc::FootnoteOptions { span_columns: true, space_before: 70.0, allow_split: true, ..Default::default() },
        });
        let story = doc.story_mut(sid).unwrap();
        let span = story.insert_note(0, "Spanning", Default::default()).unwrap();
        story.note_mut(span).unwrap().series = 1;
        story.insert_note(story.len(), "Local\nLocal\nLocal\nLocal", Default::default());
        let story = doc.story(sid).unwrap();
        let frames = crate::frame_specs(&doc, sid);
        let frame = &frames[0];
        let cols = frame.columns();
        let opts = ComposeOptions::default();
        let mut notes = Notes::new(&doc, story, &frames);
        let snapshot = notes.snapshot();
        let column = NoteColumn { frame_index: 0, column_index: 1, width: cols[1].width(), frame };
        assert!(notes.place_line(&doc, column, &[0], None, &opts));
        let floor = notes.reserve(&doc, column, &[], &opts);
        let local = NoteColumn { column_index: 0, ..column };
        // A late local reference fits its own stack, but not the later spanning reservation.
        let available = floor - 10.0;
        let mut line = crate::compose_story(&doc, sid, &opts).frames[0].lines[0].clone();
        line.column = 0;
        line.baseline = cols[0].y1 - available - line.descent;
        assert!(notes.earlier_columns_collide(&[line], 0, 1, &cols, floor));
        notes.restore(&snapshot);
        assert!(notes.reserve(&doc, local, &[1], &opts) >= floor);
        assert!(notes.split_plan(&doc, local, &[1], available, &opts).is_none());
        assert!(notes.split_plan(&doc, local, &[1], floor + 10.0, &opts).is_some());
    }
}
