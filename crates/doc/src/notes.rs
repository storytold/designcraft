//! Footnotes.
//!
//! A footnote reference is a [`FOOTNOTE_REF`] character in a story; the story's
//! [`Story::notes`] holds one [`Footnote`] per reference, in text order (the k-th reference
//! character owns `notes[k]`). Deleting the reference deletes the footnote; inserting text that
//! contains reference characters inserts empty footnotes. Each footnote's text is a frameless
//! [`Story`] (like a table cell) composed at the bottom of the column its reference lands in.
//!
//! Numbering and layout come from the document-wide [`FootnoteOptions`]
//! (Type › Document Footnote Options).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attrs::{NumberStyle, Position};
use crate::ids::StoryId;
use crate::item::FirstBaseline;
use crate::story::{BASIC_PARAGRAPH, CharFormat, NO_CHAR_STYLE, ParaFormat, Story};

/// Footnote reference character (the number shown in the text).
pub const FOOTNOTE_REF: char = '\u{E00A}';

/// Footnote addresses reuse [`crate::CellAddr`] with this pseudo table id (the row is the
/// footnote id): text selections, typing and formatting reach footnote text like cell text.
pub const FOOTNOTE_TABLE: u64 = u64::MAX;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Footnote {
    /// Stable within the story (selection addresses).
    pub id: u64,
    /// Independent numbering and layout; absent in older documents.
    #[serde(default, skip_serializing_if = "is_default_series")]
    pub series: u64,
    /// The footnote text (no frames).
    pub text: Story,
}

fn is_default_series(series: &u64) -> bool {
    *series == 0
}

/// Extra series only; series 0 keeps the legacy `Document::footnote_options` field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSeries {
    pub id: u64,
    pub name: String,
    #[serde(flatten)]
    pub options: FootnoteOptions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarginSide {
    Inside,
    Outside,
    Left,
    #[default]
    Right,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum NotePlacement {
    #[default]
    ColumnFoot,
    Margin {
        #[serde(default)]
        side: MarginSide,
        gap: f64,
        width: f64,
    },
}

impl NotePlacement {
    pub fn is_column_foot(&self) -> bool {
        matches!(self, Self::ColumnFoot)
    }
}

/// Restart Numbering Every.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteRestart {
    /// Continuous through the document.
    #[default]
    Never,
    Page,
    Spread,
    Section,
}

/// Where the prefix/suffix appear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AffixIn {
    #[default]
    None,
    Reference,
    Text,
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteRule {
    pub on: bool,
    pub weight: f64,
    pub color: String,
    pub tint: f32,
    /// Rule length.
    pub width: f64,
    /// Baseline offset below the top of the footnote area.
    pub offset: f64,
    pub left_indent: f64,
}

impl Default for NoteRule {
    fn default() -> Self {
        NoteRule { on: true, weight: 1.0, color: designcraft_color::swatch::BLACK.into(), tint: 1.0, width: 72.0, offset: 0.0, left_indent: 0.0 }
    }
}

/// Type › Document Footnote Options (Numbering and Formatting, Layout).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FootnoteOptions {
    #[serde(skip_serializing_if = "NotePlacement::is_column_foot")]
    pub placement: NotePlacement,
    pub style: NumberStyle,
    pub start_at: u32,
    pub restart: NoteRestart,
    pub prefix: String,
    pub suffix: String,
    pub affix_in: AffixIn,
    /// Footnote reference number in text.
    pub ref_position: Position,
    pub ref_char_style: String,
    /// Footnote text formatting.
    pub para_style: String,
    /// Between the number and the footnote text.
    pub separator: String,
    /// Minimum space between the text and the first footnote.
    pub space_before: f64,
    pub space_between: f64,
    pub first_baseline: FirstBaseline,
    pub first_baseline_min: f64,
    /// Footnotes span all columns of a multi-column frame (else each column gets its own).
    pub span_columns: bool,
    /// Allow Split Footnotes: continue long notes across columns and frames.
    /// Each part obeys the note paragraphs' widow/orphan and keep options.
    pub allow_split: bool,
    /// Rule above the first footnote in a column.
    pub rule: NoteRule,
}

impl Default for FootnoteOptions {
    fn default() -> Self {
        FootnoteOptions {
            placement: NotePlacement::ColumnFoot,
            style: NumberStyle::Arabic,
            start_at: 1,
            restart: NoteRestart::Never,
            prefix: String::new(),
            suffix: String::new(),
            affix_in: AffixIn::None,
            ref_position: Position::Superscript,
            ref_char_style: NO_CHAR_STYLE.into(),
            para_style: BASIC_PARAGRAPH.into(),
            separator: "\t".into(),
            space_before: 0.0,
            space_between: 0.0,
            first_baseline: FirstBaseline::Leading,
            first_baseline_min: 0.0,
            span_columns: false,
            allow_split: true,
            rule: NoteRule::default(),
        }
    }
}

impl FootnoteOptions {
    /// Validate layout dimensions at command and native-file boundaries.
    pub fn check(&self) -> Result<(), String> {
        if let NotePlacement::Margin { gap, width, .. } = self.placement
            && (!gap.is_finite() || gap < 0.0 || !width.is_finite() || width <= 0.0 || width > 1.0e6)
        {
            return Err("margin gap must be nonnegative and width must be greater than zero and at most 1000000 pt".into());
        }
        if [self.space_before, self.space_between, self.first_baseline_min, self.rule.weight, self.rule.width]
            .iter()
            .any(|n| !n.is_finite() || *n < 0.0)
            || !self.rule.offset.is_finite()
            || !self.rule.left_indent.is_finite()
            || !self.rule.tint.is_finite()
        {
            return Err("note spacing and rule dimensions must be finite, with nonnegative spacing, width and weight".into());
        }
        Ok(())
    }

    /// The number as shown at the reference (`text == false`) or before the footnote text.
    pub fn label(&self, n: u32, text: bool) -> String {
        let num = self.style.format(n);
        let affix = match self.affix_in {
            AffixIn::None => false,
            AffixIn::Both => true,
            AffixIn::Reference => !text,
            AffixIn::Text => text,
        };
        if affix { format!("{}{num}{}", self.prefix, self.suffix) } else { num }
    }
}

impl Story {
    /// Index (in [`Story::notes`]) of the footnote whose reference is the k-th reference before `pos`.
    pub fn notes_before(&self, pos: usize) -> usize {
        let pos = crate::story::floor_char_boundary(&self.text, pos);
        self.text[..pos].matches(FOOTNOTE_REF).count()
    }

    pub fn note(&self, id: u64) -> Option<&Footnote> {
        self.notes.iter().find(|n| n.id == id).map(|n| &**n)
    }

    /// Mutable footnote (copy-on-write); bumps the story revision.
    pub fn note_mut(&mut self, id: u64) -> Option<&mut Footnote> {
        let n = self.notes.iter_mut().find(|n| n.id == id)?;
        self.rev = self.rev.wrapping_add(1);
        Some(Arc::make_mut(n))
    }

    /// Byte offset of a footnote's reference character.
    pub fn note_anchor(&self, id: u64) -> Option<usize> {
        let k = self.notes.iter().position(|n| n.id == id)?;
        self.text.match_indices(FOOTNOTE_REF).nth(k).map(|(i, _)| i)
    }

    /// Insert a footnote reference at `pos` with the given footnote text. Returns the footnote
    /// id, or `None` if the story has no footnote for the new reference.
    pub fn insert_note(&mut self, pos: usize, text: &str, para: ParaFormat) -> Option<u64> {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        let fmt = self.char_format_at(pos).clone();
        self.insert_with(pos, &FOOTNOTE_REF.to_string(), fmt);
        // A file may hold fewer footnotes than references; give every reference one.
        self.fix_notes();
        let k = self.notes_before(pos);
        let note = Arc::make_mut(self.notes.get_mut(k)?);
        note.text = Story::with_text(StoryId(0), text, para);
        Some(note.id)
    }

    pub(crate) fn next_note_id(&self) -> u64 {
        crate::ids::next_local_id(self.notes.iter().map(|n| n.id))
    }

    /// Insert empty footnotes for `n` new reference characters before which `k` references sit.
    pub(crate) fn notes_inserted(&mut self, k: usize, n: usize) {
        // The largest id so far, kept as ids are given out (finding it again for every new note
        // was quadratic in a batch); past the largest id, the free-id search as before.
        let mut max = self.notes.iter().map(|x| x.id).max().unwrap_or(0);
        for i in 0..n {
            let id = max.checked_add(1).unwrap_or_else(|| self.next_note_id());
            max = max.max(id);
            let mut text = Story::new(StoryId(0));
            text.chars[0].format = CharFormat::default();
            self.notes.insert((k + i).min(self.notes.len()), Arc::new(Footnote { id, series: 0, text }));
        }
    }

    /// Make the footnote list match the reference characters (after wholesale text changes).
    pub fn fix_notes(&mut self) {
        let n = self.text.matches(FOOTNOTE_REF).count();
        if n < self.notes.len() {
            self.notes.truncate(n);
        } else if n > self.notes.len() {
            let k = self.notes.len();
            self.notes_inserted(k, n - k);
        }
    }

    pub(crate) fn check_notes(&self) -> Result<(), String> {
        let n = self.text.matches(FOOTNOTE_REF).count();
        if n != self.notes.len() {
            return Err(format!("story {}: {n} footnote references, {} footnotes", self.id.0, self.notes.len()));
        }
        let mut ids = std::collections::HashSet::new();
        for note in &self.notes {
            if !ids.insert(note.id) {
                return Err(format!("story {}: footnote id {} used twice", self.id.0, note.id));
            }
            note.text.check()?;
        }
        Ok(())
    }
}

impl crate::Document {
    /// The numbering of list `name` after the stories before `sid` (page order), so `sid`'s
    /// numbering carries on; a fresh count for lists that don't continue across stories.
    pub fn list_start(&self, sid: StoryId, name: &str) -> crate::ListCounter {
        let mut n = crate::ListCounter::default();
        if !self.settings.lists.iter().any(|l| l.name == name && l.continue_across_stories) {
            return n;
        }
        let key = |st: &Story| -> (usize, u64) {
            let page = st.frames.first().and_then(|f| self.page_of_item(*f)).unwrap_or(usize::MAX);
            (page, st.id.0)
        };
        let Some(me) = self.story(sid) else { return n };
        let mine = key(me);
        let mut before: Vec<&Story> = self.stories.values().map(|s| s.as_ref()).filter(|st| st.id != sid && key(st) < mine).collect();
        before.sort_by_key(|st| key(st));
        for st in before {
            for (p, r) in st.paras.iter().zip(st.para_ranges()) {
                let (pp, _) = self.styles.resolve_para(p);
                if pp.list_type == crate::ListType::Numbers && pp.list_name == name && !r.is_empty() {
                    n.advance(&pp);
                }
            }
        }
        n
    }

    /// Number of the first footnote of story `sid` when numbering runs through the document:
    /// the start number plus the footnotes of stories that start on earlier pages.
    pub fn footnote_start(&self, sid: StoryId) -> u32 {
        self.footnote_series_start(sid, 0)
    }

    /// The effective series of native or pasted notes with an undeclared id.
    pub fn note_series_id(&self, series: u64) -> u64 {
        if self.note_series.iter().any(|s| s.id == series && series != 0) { series } else { 0 }
    }

    /// Adopt additional series by meaning, not by their document-local numeric ids.
    /// Legacy notes still join the destination's series 0, as ordinary footnote paste always did.
    pub fn adopt_note_series(&mut self, story: &mut Story, source: &Self) {
        if !story.notes.iter().any(|note| source.note_series_id(note.series) != 0) {
            for note in &mut story.notes {
                if note.series != 0 {
                    Arc::make_mut(note).series = 0;
                }
            }
            return;
        }
        let mapped = self.adopt_note_series_ids(source);
        for note in &mut story.notes {
            let id = mapped.get(&source.note_series_id(note.series)).copied().unwrap_or(0);
            if note.series != id {
                Arc::make_mut(note).series = id;
            }
        }
    }

    /// Map an imported series library once for notes and stream routes using the same ids.
    pub fn adopt_note_series_ids(&mut self, source: &Self) -> std::collections::HashMap<u64, u64> {
        let mut mapped = std::collections::HashMap::new();
        let mut used = std::collections::HashSet::new();
        // Clipboard snapshots can outlive a deleted equivalent series. Reserve surviving
        // id-and-definition matches before declaration-order matching can consume them.
        for series in source.note_series.iter().filter(|s| s.id != 0) {
            if self.note_series.iter().any(|s| s.id == series.id && s.name == series.name && s.options == series.options) {
                mapped.insert(series.id, series.id);
                used.insert(series.id);
            }
        }
        // Map the source library in declaration order, so separate copied stories reuse the
        // same counters while distinct source series with matching definitions stay distinct.
        for series in source.note_series.iter().filter(|s| s.id != 0) {
            if mapped.contains_key(&series.id) {
                continue;
            }
            let id = if let Some(existing) =
                self.note_series.iter().find(|s| !used.contains(&s.id) && s.name == series.name && s.options == series.options)
            {
                existing.id
            } else {
                // Do not give a newly adopted definition another source definition's id:
                // a later adoption would mistake it for a surviving exact match.
                let id = crate::ids::next_local_id(self.note_series.iter().chain(&source.note_series).map(|s| s.id));
                self.note_series.push(NoteSeries { id, ..series.clone() });
                id
            };
            used.insert(id);
            mapped.insert(series.id, id);
        }
        mapped
    }

    /// Options for a series; unknown ids use the legacy series on imported or pasted text.
    pub fn note_options(&self, series: u64) -> &FootnoteOptions {
        self.note_series.iter().find(|s| s.id == series && series != 0).map_or(&self.footnote_options, |s| &s.options)
    }

    pub fn footnote_series_start(&self, sid: StoryId, series: u64) -> u32 {
        let series = self.note_series_id(series);
        let o = self.note_options(series);
        if o.restart != NoteRestart::Never {
            return o.start_at;
        }
        let key = |st: &Story| -> (usize, u64) {
            let page = st.frames.first().and_then(|f| self.page_of_item(*f)).unwrap_or(usize::MAX);
            (page, st.id.0)
        };
        let Some(me) = self.story(sid) else { return o.start_at };
        let mine = key(me);
        let before: usize = self
            .stories
            .values()
            .filter(|st| st.id != sid && !st.notes.is_empty() && key(st) < mine)
            .map(|st| st.notes.iter().filter(|n| self.note_series_id(n.series) == series).count())
            .sum();
        o.start_at.saturating_add(before as u32)
    }

    /// Absolute page an item sits on (its center), None on parent pages / pasteboard-only spreads.
    pub fn page_of_item(&self, id: crate::ItemId) -> Option<usize> {
        let loc = self.find(id)?;
        let crate::SpreadRef::Doc(si) = loc.spread else { return None };
        let sp = self.spread(loc.spread)?;
        let c = self.item(id)?.bounds().center();
        Some(self.first_page_of_spread(si) + sp.page_at_x(c.x).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(t: &str) -> Story {
        Story::with_text(StoryId(1), t, ParaFormat::default())
    }

    #[test]
    fn references_own_notes_in_order() {
        let mut s = st("alpha beta gamma");
        let b = s.insert_note(10, "second", ParaFormat::default()).unwrap();
        let a = s.insert_note(5, "first", ParaFormat::default()).unwrap();
        s.check().unwrap();
        assert_eq!(s.notes.len(), 2);
        assert_eq!(s.notes[0].id, a);
        assert_eq!(s.notes[0].text.text, "first");
        assert_eq!(s.notes[1].text.text, "second");
        assert_eq!(s.note_anchor(b), Some(10 + FOOTNOTE_REF.len_utf8()));
        // Deleting the first reference deletes its note only.
        s.delete(5..5 + FOOTNOTE_REF.len_utf8());
        s.check().unwrap();
        assert_eq!(s.notes.len(), 1);
        assert_eq!(s.notes[0].id, b);
        // Typing a reference character (paste) creates an empty note.
        s.insert(0, &FOOTNOTE_REF.to_string());
        s.check().unwrap();
        assert_eq!(s.notes.len(), 2);
        assert_eq!(s.notes[1].id, b);
        assert!(s.notes[0].text.text.is_empty());
        // Deleting a range spanning everything clears all notes.
        let n = s.len();
        s.delete(0..n);
        s.check().unwrap();
        assert!(s.notes.is_empty());
    }

    #[test]
    fn labels_and_symbols() {
        let mut o = FootnoteOptions::default();
        assert_eq!(o.label(3, false), "3");
        o.style = NumberStyle::Symbols;
        assert_eq!(o.label(1, false), "*");
        assert_eq!(o.label(2, false), "†");
        assert_eq!(o.label(7, false), "**");
        o.style = NumberStyle::Arabic;
        o.prefix = "[".into();
        o.suffix = "]".into();
        o.affix_in = AffixIn::Reference;
        assert_eq!(o.label(4, false), "[4]");
        assert_eq!(o.label(4, true), "4");
    }
}

#[cfg(test)]
mod series_tests {
    use super::*;

    #[test]
    fn continuous_series_count_only_their_own_earlier_notes() {
        let mut d = crate::Document::new(&crate::build::NewDocument::default());
        d.note_series.push(NoteSeries { id: 1, name: "Sources".into(), options: FootnoteOptions { start_at: 10, ..Default::default() } });
        let mut first = Story::with_text(StoryId(1), "Body", Default::default());
        first.insert_note(0, "ordinary", Default::default());
        let id = first.insert_note(first.len(), "source", Default::default()).unwrap();
        first.note_mut(id).unwrap().series = 1;
        first.insert_note(first.len(), "ordinary", Default::default());
        d.stories.insert(first.id, Arc::new(first));
        d.stories.insert(StoryId(2), Arc::new(Story::new(StoryId(2))));
        assert_eq!(d.footnote_series_start(StoryId(2), 0), 3);
        assert_eq!(d.footnote_series_start(StoryId(2), 1), 11);
    }
}

#[cfg(test)]
mod adoption_tests {
    use super::*;
    #[test]
    fn repeated_adoption_keeps_conflicting_identical_definitions_mapped_consistently() {
        let mut source = crate::Document::new(&Default::default());
        for id in [1, 2] {
            source.note_series.push(NoteSeries { id, name: "Sources".into(), options: Default::default() });
        }
        let mut destination = crate::Document::new(&Default::default());
        destination.note_series.push(NoteSeries { id: 1, name: "Other".into(), options: Default::default() });
        let first = destination.adopt_note_series_ids(&source);
        let again = destination.adopt_note_series_ids(&source);
        assert_eq!(again, first);
        assert_ne!(first[&1], first[&2]);
        assert_eq!(destination.note_series.len(), 3);
    }

    #[test]
    fn adoption_preserves_a_surviving_identical_series_after_copy_then_delete() {
        let mut source = crate::Document::new(&Default::default());
        for id in [1, 2] {
            source.note_series.push(NoteSeries { id, name: "Sources".into(), options: Default::default() });
        }
        let mut copied = Story::new(StoryId(0));
        let note = copied.insert_note(0, "Second series", Default::default()).unwrap();
        copied.note_mut(note).unwrap().series = 2;
        let mut destination = source.clone();
        destination.note_series.retain(|series| series.id != 1);
        destination.adopt_note_series(&mut copied, &source);
        assert_eq!(copied.notes[0].series, 2, "paste stays in the surviving numbering sequence");
        assert_eq!(destination.note_series.iter().filter(|series| series.id == 2).count(), 1);
    }

    #[test]
    fn adoption_keeps_identical_source_series_independent() {
        let mut source = crate::Document::new(&Default::default());
        for id in [1, 2] {
            source.note_series.push(NoteSeries { id, name: "Sources".into(), options: Default::default() });
        }
        let mut dest = source.clone();
        let mut text = Story::new(StoryId(0));
        for series in [1, 2, 1, 2] {
            let id = text.insert_note(text.len(), "Source", Default::default()).unwrap();
            text.note_mut(id).unwrap().series = series;
        }
        dest.adopt_note_series(&mut text, &source);
        let ids: Vec<_> = text.notes.iter().map(|n| n.series).collect();
        assert_ne!(ids[0], ids[1]);
        assert_eq!(ids[0], ids[2]);
        assert_eq!(ids[1], ids[3]);
        assert_eq!(dest.note_series.len(), 2);
        let mut empty = crate::Document::new(&Default::default());
        empty.adopt_note_series(&mut text, &dest);
        assert_ne!(text.notes[0].series, text.notes[1].series);
        assert_eq!(empty.note_series.len(), 2);
    }

    #[test]
    fn adoption_matches_identical_series_across_separate_stories() {
        let mut source = crate::Document::new(&Default::default());
        for id in [1, 2] {
            source.note_series.push(NoteSeries { id, name: "Sources".into(), options: Default::default() });
        }
        // Frame and anchored-story paste adopt stories separately, possibly in reverse order.
        let mut separate = crate::Document::new(&Default::default());
        let mut adopted = Vec::new();
        for series in [2, 1, 2, 1] {
            let mut story = Story::new(StoryId(0));
            let id = story.insert_note(0, "Source", Default::default()).unwrap();
            story.note_mut(id).unwrap().series = series;
            separate.adopt_note_series(&mut story, &source);
            adopted.push(story.notes[0].series);
        }
        assert_ne!(adopted[0], adopted[1]);
        assert_eq!(adopted[0], adopted[2]);
        assert_eq!(adopted[1], adopted[3]);
        assert_eq!(separate.note_series.len(), 2);
    }

    #[test]
    fn note_series_adoption_remaps_conflicting_ids_and_reuses_equivalent_series() {
        let mut source = crate::Document::new(&Default::default());
        source.note_series.push(NoteSeries {
            id: 1,
            name: "Sources".into(),
            options: FootnoteOptions { style: NumberStyle::LowerLetters, ..Default::default() },
        });
        let mut dest = crate::Document::new(&Default::default());
        dest.note_series.push(NoteSeries { id: 1, name: "Other".into(), options: FootnoteOptions { start_at: 20, ..Default::default() } });
        let mut text = Story::with_text(StoryId(0), "Body", Default::default());
        let id = text.insert_note(4, "Source", Default::default()).unwrap();
        text.note_mut(id).unwrap().series = 1;
        let mut copy = text.clone();
        dest.adopt_note_series(&mut copy, &source);
        let adopted = copy.notes[0].series;
        assert_ne!(adopted, 1);
        assert_eq!(dest.note_options(adopted), &source.note_series[0].options);
        assert_eq!(dest.note_options(1).start_at, 20);
        dest.adopt_note_series(&mut text, &source);
        assert_eq!(text.notes[0].series, adopted);
        assert_eq!(dest.note_series.len(), 2);
    }
}
