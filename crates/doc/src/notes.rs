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
//!
//! Footnotes nest: a footnote's text can hold footnote references of its own, down to
//! [`MAX_NOTE_DEPTH`] levels (InDesign can't do this). A nested note is printed under the note it
//! comes from and numbered in reading order. Footnote ids are unique across a story and all its
//! nested notes, so an id alone ([`crate::CellAddr::footnote`]) finds a note at any depth.

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

/// How deep footnotes nest: a footnote (depth 1) can hold notes down to this depth. Deeper notes
/// in a file are kept but not shown or numbered.
pub const MAX_NOTE_DEPTH: usize = 5;

impl Footnote {
    /// This footnote's text with its nested notes written out as InDesign can hold them, and as
    /// GDT-style layouts do by hand: each nested reference becomes its mark as superscript text,
    /// and each nested note follows the note it comes from after a forced line break: its mark
    /// (superscript), `separator`, then its text (its first paragraph joins the paragraph it
    /// follows; later ones stay paragraphs). `mark` gives a nested note's mark by id.
    pub fn flattened(&self, mark: &dyn Fn(u64) -> String, separator: &str) -> Story {
        fn superscript(st: &Story, at: usize) -> CharFormat {
            let mut f = st.char_format_at(at).clone();
            f.over.position = Some(Position::Superscript);
            f
        }
        /// `st` with each footnote reference replaced by its mark (its notes go with it).
        fn marked(st: &Story, mark: &dyn Fn(u64) -> String) -> Story {
            let mut out = st.clone();
            let refs: Vec<(usize, u64)> = st.text.match_indices(FOOTNOTE_REF).zip(&st.notes).map(|((at, _), n)| (at, n.id)).collect();
            for (at, id) in refs.into_iter().rev() {
                let f = superscript(&out, at);
                out.delete(at..at + FOOTNOTE_REF.len_utf8());
                out.insert_with(at, &mark(id), f);
            }
            out
        }
        fn append(out: &mut Story, st: &Story, depth: usize, mark: &dyn Fn(u64) -> String, separator: &str) {
            if depth > MAX_NOTE_DEPTH {
                return;
            }
            for n in &st.notes {
                let end = out.len();
                let f = out.char_format_at(end).clone();
                out.insert_with(end, &crate::story::FORCED_LINE_BREAK.to_string(), f.clone());
                let end = out.len();
                out.insert_with(end, &mark(n.id), superscript(out, end));
                let end = out.len();
                out.insert_with(end, separator, f);
                let end = out.len();
                out.insert_story(end, &marked(&n.text, mark));
                append(out, &n.text, depth + 1, mark, separator);
            }
        }
        let mut out = marked(&self.text, mark);
        append(&mut out, &self.text, 2, mark, separator);
        out
    }

    /// The footnotes nested in this (top-level) footnote, in reading order: each note, then the
    /// notes nested in it, before its next sibling, down to [`MAX_NOTE_DEPTH`]. Read in this order
    /// they are numbered and printed under it.
    pub fn nested(&self) -> Vec<&Footnote> {
        fn walk<'a>(st: &'a Story, depth: usize, out: &mut Vec<&'a Footnote>) {
            if depth > MAX_NOTE_DEPTH {
                return;
            }
            for n in &st.notes {
                out.push(n);
                walk(&n.text, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.text, 2, &mut out);
        out
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

    /// Footnote `id`, at any depth (nested notes included).
    pub fn note(&self, id: u64) -> Option<&Footnote> {
        let path = self.note_path(id)?;
        let mut st = self;
        let mut found = None;
        for k in path {
            let n = st.notes.get(k)?;
            found = Some(&**n);
            st = &n.text;
        }
        found
    }

    /// Indices from this story down to footnote `id`: `notes[a]`, then that note's `notes[b]`,
    /// and so on (at most [`MAX_NOTE_DEPTH`] + 1 steps).
    pub fn note_path(&self, id: u64) -> Option<Vec<usize>> {
        fn walk(st: &Story, id: u64, depth: usize, path: &mut Vec<usize>) -> bool {
            for (k, n) in st.notes.iter().enumerate() {
                path.push(k);
                if n.id == id || (depth < MAX_NOTE_DEPTH && walk(&n.text, id, depth + 1, path)) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        walk(self, id, 1, &mut path).then_some(path)
    }

    /// Mutable footnote at any depth (copy-on-write along its path); bumps the revision of every
    /// story on the way, so the compositions that show it are made again.
    pub fn note_mut(&mut self, id: u64) -> Option<&mut Footnote> {
        let path = self.note_path(id)?;
        let (last, steps) = path.split_last()?;
        let mut st = self;
        for &k in steps {
            st.rev = st.rev.wrapping_add(1);
            st = &mut Arc::make_mut(st.notes.get_mut(k)?).text;
        }
        st.rev = st.rev.wrapping_add(1);
        Some(Arc::make_mut(st.notes.get_mut(*last)?))
    }

    /// The story that holds footnote `id`'s reference: this story, or the text of the note it's
    /// nested in.
    pub fn note_holder(&self, id: u64) -> Option<&Story> {
        let path = self.note_path(id)?;
        let mut st = self;
        for &k in &path[..path.len() - 1] {
            st = &st.notes.get(k)?.text;
        }
        Some(st)
    }

    /// The id of the footnote whose text holds footnote `id`'s reference (`None` at the top level
    /// or when there's no such note).
    pub fn note_parent(&self, id: u64) -> Option<u64> {
        let path = self.note_path(id)?;
        let (_, up) = path.split_last()?;
        let (&last, steps) = up.split_last()?;
        let mut st = self;
        for &k in steps {
            st = &st.notes.get(k)?.text;
        }
        st.notes.get(last).map(|n| n.id)
    }

    /// How deep footnote `id` sits: 1 for a footnote of this story, 2 for one nested in it, …
    pub fn note_depth(&self, id: u64) -> Option<usize> {
        self.note_path(id).map(|p| p.len())
    }

    /// The footnotes nested in this story's footnotes, in reading order (each followed by its own
    /// nested notes, before its next sibling), down to [`MAX_NOTE_DEPTH`].
    pub fn nested_notes(&self) -> Vec<&Footnote> {
        self.notes.iter().flat_map(|n| n.nested()).collect()
    }

    /// Every footnote id in this story and its nested notes.
    fn note_ids_deep(&self) -> Vec<u64> {
        fn walk(st: &Story, depth: usize, out: &mut Vec<u64>) {
            for n in &st.notes {
                out.push(n.id);
                if depth < MAX_NOTE_DEPTH {
                    walk(&n.text, depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, 1, &mut out);
        out
    }

    /// A footnote id no note of this story or its nested notes uses.
    pub fn next_note_id_deep(&self) -> u64 {
        crate::ids::next_local_id(self.note_ids_deep().into_iter())
    }

    /// Give each footnote a story-wide unique id again: a footnote pasted into a nested note
    /// gets an id from that note's own text, which another note may already use. Of two notes
    /// with one id, the later in reading order takes a new one. Returns whether any changed.
    pub fn fix_note_ids(&mut self) -> bool {
        fn walk(st: &mut Story, depth: usize, seen: &mut std::collections::HashSet<u64>, next: &mut u64) -> bool {
            let mut changed = false;
            for n in &mut st.notes {
                if !seen.insert(n.id) {
                    let n = Arc::make_mut(n);
                    n.id = *next;
                    *next = next.saturating_add(1);
                    seen.insert(n.id);
                    changed = true;
                }
                if depth < MAX_NOTE_DEPTH && !n.text.notes.is_empty() && walk(&mut Arc::make_mut(n).text, depth + 1, seen, next) {
                    changed = true;
                }
            }
            if changed {
                st.rev = st.rev.wrapping_add(1);
            }
            changed
        }
        if !self.notes.iter().any(|n| !n.text.notes.is_empty()) {
            return false;
        }
        let mut next = self.next_note_id_deep();
        walk(self, 1, &mut std::collections::HashSet::new(), &mut next)
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
    /// [`Story::fix_note_ids`] for every story with nested footnotes (after an edit or a load).
    pub fn fix_note_ids(&mut self) {
        let ids: Vec<StoryId> = self.stories.iter().filter(|(_, st)| st.notes.iter().any(|n| !n.text.notes.is_empty())).map(|(id, _)| *id).collect();
        for id in ids {
            if let Some(st) = self.stories.get_mut(&id) {
                Arc::make_mut(st).fix_note_ids();
            }
        }
    }

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
        // Nested notes take numbers too (see `Footnote::nested`).
        let before: usize = self
            .stories
            .values()
            .filter(|st| st.id != sid && !st.notes.is_empty() && key(st) < mine)
            .map(|st| st.notes.len() + st.nested_notes().len())
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

    /// A story with footnotes at "a" and "b"; the first holds a nested note at "c", which holds
    /// one at "d" (reading order: a, c, d, b).
    fn nested() -> (Story, [u64; 4]) {
        let mut s = Story::with_text(StoryId(1), "a b", ParaFormat::default());
        let a = s.insert_note(1, "c text", ParaFormat::default()).unwrap();
        let b = s.insert_note(s.len(), "b text", ParaFormat::default()).unwrap();
        let c_id = s.next_note_id_deep();
        let c = {
            let n = s.note_mut(a).unwrap();
            let local = n.text.insert_note(1, "d text", ParaFormat::default()).unwrap();
            Arc::make_mut(n.text.notes.iter_mut().find(|x| x.id == local).unwrap()).id = c_id;
            c_id
        };
        let d_id = s.next_note_id_deep();
        {
            let n = s.note_mut(c).unwrap();
            let local = n.text.insert_note(1, "deepest", ParaFormat::default()).unwrap();
            Arc::make_mut(n.text.notes.iter_mut().find(|x| x.id == local).unwrap()).id = d_id;
        }
        (s, [a, b, c, d_id])
    }

    /// Nested footnotes are found by id at any depth, with their holder, parent and depth, and
    /// listed in reading order.
    #[test]
    fn nested_notes_are_found_by_id() {
        let (s, [a, b, c, d]) = nested();
        assert_eq!(s.note(d).unwrap().text.text, "deepest");
        assert_eq!(s.note_depth(a), Some(1));
        assert_eq!(s.note_depth(c), Some(2));
        assert_eq!(s.note_depth(d), Some(3));
        assert_eq!(s.note_parent(a), None);
        assert_eq!(s.note_parent(d), Some(c));
        assert_eq!(s.note_holder(d).unwrap().text, s.note(c).unwrap().text.text);
        assert_eq!(s.nested_notes().iter().map(|n| n.id).collect::<Vec<_>>(), [c, d]);
        assert!(s.note(b).unwrap().text.notes.is_empty());
        assert_eq!(s.note(999), None);
    }

    /// Editing a nested note marks every story above it changed (copy-on-write), so the
    /// compositions that show it are made again.
    #[test]
    fn editing_a_nested_note_bumps_its_ancestors() {
        let (mut s, [a, _, c, d]) = nested();
        let before = (s.rev, s.note(a).unwrap().text.rev, s.note(c).unwrap().text.rev);
        s.note_mut(d).unwrap().text.insert(0, "x");
        assert_ne!(s.rev, before.0);
        assert_ne!(s.note(a).unwrap().text.rev, before.1);
        assert_ne!(s.note(c).unwrap().text.rev, before.2);
        assert_eq!(s.note(d).unwrap().text.text, "xdeepest");
    }

    /// Ids stay unique across nested notes: a note pasted into a nested note's text takes an id
    /// that note's text had free, which the repair replaces; the earlier note keeps its id.
    #[test]
    fn duplicate_nested_ids_are_repaired() {
        let (mut s, [a, b, c, d]) = nested();
        // Pasting a reference into note b's text gives the new note b's text's first free id: 1.
        s.note_mut(b).unwrap().text.insert(0, &FOOTNOTE_REF.to_string());
        let pasted = s.note(b).unwrap().text.notes[0].id;
        assert_eq!(pasted, 1, "the same id as note a");
        assert!(s.fix_note_ids());
        let ids: Vec<u64> = [a, b, c, d].into_iter().chain([s.note(b).unwrap().text.notes[0].id]).collect();
        let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
        assert_eq!(unique.len(), 5, "{ids:?}");
        assert_eq!(s.note(a).unwrap().text.text, "c\u{E00A} text", "the earlier note keeps its id");
        assert!(!s.fix_note_ids(), "nothing more to repair");
    }

    /// Notes nested past the depth limit are kept but not reached.
    #[test]
    fn nesting_is_bounded() {
        let mut s = Story::with_text(StoryId(1), "x", ParaFormat::default());
        let mut id = s.insert_note(1, "x", ParaFormat::default()).unwrap();
        for _ in 0..MAX_NOTE_DEPTH + 2 {
            let next = s.next_note_id_deep();
            let n = s.note_mut(id).expect("reachable");
            let local = n.text.insert_note(1, "x", ParaFormat::default()).unwrap();
            Arc::make_mut(n.text.notes.iter_mut().find(|x| x.id == local).unwrap()).id = next;
            if s.note(next).is_none() {
                break;
            }
            id = next;
        }
        assert_eq!(s.note_depth(id), Some(MAX_NOTE_DEPTH), "the deepest reachable note");
        assert_eq!(s.nested_notes().len(), MAX_NOTE_DEPTH - 1);
    }

    /// Written out for InDesign: nested references as superscript marks, each nested note after
    /// a forced line break under the note it comes from (mark, separator, text).
    #[test]
    fn nested_notes_flatten_as_marks_and_line_breaks() {
        let (s, [a, _, c, d]) = nested();
        let mark = |id: u64| {
            if id == c {
                "2".to_string()
            } else if id == d {
                "3".to_string()
            } else {
                "?".to_string()
            }
        };
        let flat = s.note(a).unwrap().flattened(&mark, "\t");
        assert_eq!(flat.text, "c2 text\u{2028}2\td3 text\u{2028}3\tdeepest");
        assert!(flat.notes.is_empty() && !flat.text.contains(FOOTNOTE_REF));
        flat.check().unwrap();
        // The format of the character at `at` (char_format_at gives the one before a position).
        let sup = |at: usize| flat.char_format_at(at + flat.text[at..].chars().next().unwrap().len_utf8()).over.position;
        let at = |needle: &str| flat.text.find(needle).unwrap();
        assert_eq!(sup(at("2 text")), Some(Position::Superscript), "the reference's mark");
        assert_eq!(sup(at("2\td")), Some(Position::Superscript), "the nested note's mark");
        assert_eq!(sup(at("\td")), None, "the separator");
        assert_eq!(sup(at("deepest")), None, "the text");
        assert_eq!(flat.paras.len(), 1, "one paragraph: forced line breaks");
    }

    /// A story's footnotes are numbered after every footnote of the stories before it, nested
    /// ones included.
    #[test]
    fn footnote_start_counts_nested_notes() {
        let mut d = crate::Document::new(&crate::build::NewDocument::default());
        let lid = d.default_layer();
        let (_, first) =
            d.add_text_frame(crate::SpreadRef::Doc(0), crate::geom::Rect::new(36.0, 36.0, 200.0, 100.0), lid, "a", ParaFormat::default()).unwrap();
        let (_, second) =
            d.add_text_frame(crate::SpreadRef::Doc(0), crate::geom::Rect::new(36.0, 300.0, 200.0, 400.0), lid, "b", ParaFormat::default()).unwrap();
        let (st, _) = nested();
        let first_story = d.story_mut(first).unwrap();
        first_story.notes = st.notes.clone();
        first_story.text = format!("a{FOOTNOTE_REF}b{FOOTNOTE_REF}");
        first_story.chars[0].len = first_story.text.len();
        assert_eq!(d.footnote_start(second), 1 + 2 + 2, "two footnotes and two nested in the first");
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
