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
    /// The footnote text (no frames).
    pub text: Story,
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
    /// Rule above the first footnote in a column.
    pub rule: NoteRule,
}

impl Default for FootnoteOptions {
    fn default() -> Self {
        FootnoteOptions {
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
            rule: NoteRule::default(),
        }
    }
}

impl FootnoteOptions {
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
        self.rev += 1;
        Some(Arc::make_mut(n))
    }

    /// Byte offset of a footnote's reference character.
    pub fn note_anchor(&self, id: u64) -> Option<usize> {
        let k = self.notes.iter().position(|n| n.id == id)?;
        self.text.match_indices(FOOTNOTE_REF).nth(k).map(|(i, _)| i)
    }

    /// Insert a footnote reference at `pos` with the given footnote text. Returns the footnote id.
    pub fn insert_note(&mut self, pos: usize, text: &str, para: ParaFormat) -> u64 {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        let fmt = self.char_format_at(pos).clone();
        self.insert_with(pos, &FOOTNOTE_REF.to_string(), fmt);
        let k = self.notes_before(pos);
        let note = Arc::make_mut(&mut self.notes[k]);
        note.text = Story::with_text(StoryId(0), text, para);
        note.id
    }

    pub(crate) fn next_note_id(&self) -> u64 {
        self.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1
    }

    /// Insert empty footnotes for `n` new reference characters before which `k` references sit.
    pub(crate) fn notes_inserted(&mut self, k: usize, n: usize) {
        for i in 0..n {
            let id = self.next_note_id();
            let mut text = Story::new(StoryId(0));
            text.chars[0].format = CharFormat::default();
            self.notes.insert((k + i).min(self.notes.len()), Arc::new(Footnote { id, text }));
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
        let o = &self.footnote_options;
        if o.restart != NoteRestart::Never {
            return o.start_at;
        }
        let key = |st: &Story| -> (usize, u64) {
            let page = st.frames.first().and_then(|f| self.page_of_item(*f)).unwrap_or(usize::MAX);
            (page, st.id.0)
        };
        let Some(me) = self.story(sid) else { return o.start_at };
        let mine = key(me);
        let before: usize = self.stories.values().filter(|st| st.id != sid && !st.notes.is_empty() && key(st) < mine).map(|st| st.notes.len()).sum();
        o.start_at + before as u32
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
        let b = s.insert_note(10, "second", ParaFormat::default());
        let a = s.insert_note(5, "first", ParaFormat::default());
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
