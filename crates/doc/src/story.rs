//! Stories: the text that flows through a chain of threaded text frames.
//!
//! A story is one `String` plus formatting:
//! - `paras`: one [`ParaFormat`] per paragraph (paragraphs are separated by `\n`; the last
//!   paragraph has no separator), so `paras.len() == text.matches('\n').count() + 1`;
//! - `chars`: character-format runs `(byte_len, CharFormat)` covering exactly `text.len()` bytes.
//!   Runs are never empty unless the text is empty (then there is exactly one empty run that holds
//!   the "typing" format).
//!
//! Special characters: `\t` tab, `\u{2028}` forced line break, [`PAGE_NUMBER`], [`SECTION_MARKER`],
//! [`COLUMN_BREAK`], [`FRAME_BREAK`], [`PAGE_BREAK`], `\u{AD}` discretionary hyphen,
//! `\u{2011}` non-breaking hyphen, `\u{A0}` non-breaking space, [`INDENT_HERE`], [`RIGHT_INDENT_TAB`],
//! [`TABLE_ANCHOR`] (a table, see [`crate::table`]), [`crate::notes::FOOTNOTE_REF`] (a footnote),
//! [`crate::xref::ANCHOR_MARK`] / [`crate::xref::XREF_MARK`] (text anchor / cross-reference).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attrs::{CharAttrs, ParaAttrs};
use crate::ids::{ItemId, StoryId};
use crate::table::Table;

pub const PAGE_NUMBER: char = '\u{E000}';
pub const SECTION_MARKER: char = '\u{E001}';
pub const COLUMN_BREAK: char = '\u{E002}';
pub const FRAME_BREAK: char = '\u{E003}';
pub const PAGE_BREAK: char = '\u{E004}';
pub const INDENT_HERE: char = '\u{E005}';
pub const RIGHT_INDENT_TAB: char = '\u{E006}';
pub const NEXT_PAGE_NUMBER: char = '\u{E007}';
pub const PREV_PAGE_NUMBER: char = '\u{E008}';
/// Anchor of a table: alone in its paragraph, whose [`ParaFormat::table`] names the table.
pub const TABLE_ANCHOR: char = '\u{E009}';
pub const FORCED_LINE_BREAK: char = '\u{2028}';

pub const BASIC_PARAGRAPH: &str = "[Basic Paragraph]";
pub const NO_CHAR_STYLE: &str = "[None]";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaFormat {
    pub style: String,
    #[serde(default, skip_serializing_if = "ParaAttrs::is_empty")]
    pub para: ParaAttrs,
    /// Paragraph-level character overrides are stored on the char runs; this holds nothing.
    #[serde(default, skip_serializing_if = "CharAttrs::is_empty")]
    pub chars: CharAttrs,
    /// The table anchored in this paragraph (key into [`Story::tables`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<u64>,
}

impl Default for ParaFormat {
    fn default() -> Self {
        ParaFormat { style: BASIC_PARAGRAPH.into(), para: ParaAttrs::default(), chars: CharAttrs::default(), table: None }
    }
}

/// The format a story without runs reports (a valid story always has one run).
static NO_FORMAT: std::sync::LazyLock<CharFormat> = std::sync::LazyLock::new(CharFormat::default);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharFormat {
    pub style: String,
    #[serde(default, skip_serializing_if = "CharAttrs::is_empty")]
    pub over: CharAttrs,
}

impl Default for CharFormat {
    fn default() -> Self {
        CharFormat { style: NO_CHAR_STYLE.into(), over: CharAttrs::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharRun {
    pub len: usize,
    #[serde(flatten)]
    pub format: CharFormat,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Story {
    pub id: StoryId,
    pub text: String,
    pub paras: Vec<ParaFormat>,
    pub chars: Vec<CharRun>,
    /// Text frames in thread order.
    pub frames: Vec<ItemId>,
    /// Column progression, independent of paragraph bidi direction and frame threading.
    #[serde(default)]
    pub direction: crate::TextDirection,
    /// Story direction: vertical stories set their lines top to bottom, following each other
    /// right to left, in every frame of the thread.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical: bool,
    /// Bumped on every edit (composition cache key).
    #[serde(default)]
    pub rev: u64,
    /// Tables anchored in the story, by id (see [`crate::table`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tables: BTreeMap<u64, Arc<Table>>,
    /// Footnotes, one per [`crate::notes::FOOTNOTE_REF`] in text order (see [`crate::notes`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Arc<crate::notes::Footnote>>,
    /// Endnotes, one per [`crate::endnotes::ENDNOTE_REF`] in text order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endnotes: Vec<Arc<crate::notes::Footnote>>,
    /// Text anchors, one per [`crate::xref::ANCHOR_MARK`] in text order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<Arc<crate::xref::TextAnchor>>,
    /// Cross-references, one per [`crate::xref::XREF_MARK`] in text order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xrefs: Vec<Arc<crate::xref::CrossRef>>,
    /// Index page references, one per [`crate::index::INDEX_MARK`] in text order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index_refs: Vec<Arc<crate::index::IndexRef>>,
    /// Anchored objects, one per [`crate::anchored::OBJECT_MARK`] in text order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<Arc<crate::anchored::AnchoredObject>>,
    /// Editorial notes, one per [`crate::endnotes::NOTE_MARK`] in text order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub editorial: Vec<Arc<crate::endnotes::EditorialNote>>,
    /// Place and Link: the parent story this one copies, and the parent's revision when copied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<(StoryId, u64)>,
}

impl Story {
    pub fn new(id: StoryId) -> Self {
        Story {
            id,
            text: String::new(),
            paras: vec![ParaFormat::default()],
            chars: vec![CharRun { len: 0, format: CharFormat::default() }],
            frames: vec![],
            direction: crate::TextDirection::LeftToRight,
            vertical: false,
            rev: 0,
            tables: BTreeMap::new(),
            notes: Vec::new(),
            endnotes: Vec::new(),
            anchors: Vec::new(),
            xrefs: Vec::new(),
            index_refs: Vec::new(),
            objects: Vec::new(),
            editorial: Vec::new(),
            link: None,
        }
    }

    pub fn with_text(id: StoryId, text: &str, para: ParaFormat) -> Self {
        let mut s = Story::new(id);
        s.paras[0] = para;
        s.insert(0, text);
        s
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Byte ranges of paragraphs (without the trailing `\n`).
    pub fn para_ranges(&self) -> Vec<std::ops::Range<usize>> {
        let mut v = Vec::with_capacity(self.paras.len());
        let mut start = 0;
        for (i, c) in self.text.char_indices() {
            if c == '\n' {
                v.push(start..i);
                start = i + 1;
            }
        }
        v.push(start..self.text.len());
        v
    }

    /// Index of the paragraph containing byte `pos`.
    pub fn para_at(&self, pos: usize) -> usize {
        self.text[..floor_char_boundary(&self.text, pos)].bytes().filter(|b| *b == b'\n').count()
    }

    /// Character format at byte `pos` (the run containing it; at a boundary, the preceding run —
    /// the format new typing takes).
    pub fn char_format_at(&self, pos: usize) -> &CharFormat {
        let mut start = 0;
        for r in &self.chars {
            let end = start + r.len;
            if pos < end || (pos == end && pos > start) {
                return &r.format;
            }
            start = end;
        }
        self.chars.last().map_or(&NO_FORMAT, |r| &r.format)
    }

    /// Format of the character starting at byte `pos` (the run containing `pos`), unlike
    /// [`Self::char_format_at`], which at a run boundary returns the format of the character
    /// before the caret. Past the end, the last run's format.
    pub fn char_format_of(&self, pos: usize) -> &CharFormat {
        let mut start = 0usize;
        for r in &self.chars {
            let end = start.saturating_add(r.len);
            if pos < end {
                return &r.format;
            }
            start = end;
        }
        self.chars.last().map_or(&NO_FORMAT, |r| &r.format)
    }

    /// Iterate `(range, format)` of character runs.
    pub fn runs(&self) -> impl Iterator<Item = (std::ops::Range<usize>, &CharFormat)> {
        let mut start = 0;
        self.chars.iter().map(move |r| {
            let s = start;
            start += r.len;
            (s..start, &r.format)
        })
    }

    /// Insert `text` at byte `pos` using the format at that position.
    pub fn insert(&mut self, pos: usize, text: &str) {
        let fmt = self.char_format_at(pos).clone();
        self.insert_with(pos, text, fmt);
    }

    /// Insert with an explicit character format.
    pub fn insert_with(&mut self, pos: usize, text: &str, fmt: CharFormat) {
        self.insert_with_raw(pos, text, fmt);
        self.fix_tables();
    }

    /// Insert without the table fix-up (callers fix up themselves).
    pub(crate) fn insert_raw(&mut self, pos: usize, text: &str) {
        let fmt = self.char_format_at(pos).clone();
        self.insert_with_raw(pos, text, fmt);
    }

    fn insert_with_raw(&mut self, pos: usize, text: &str, fmt: CharFormat) {
        if text.is_empty() {
            return;
        }
        let pos = floor_char_boundary(&self.text, pos);
        let refs = text.matches(crate::notes::FOOTNOTE_REF).count();
        if refs > 0 {
            let k = self.notes_before(pos);
            self.notes_inserted(k, refs);
        }
        let erefs = text.matches(crate::endnotes::ENDNOTE_REF).count();
        if erefs > 0 {
            let k = self.endnotes_before(pos);
            self.endnotes_inserted(k, erefs);
        }
        if text.contains([
            crate::xref::ANCHOR_MARK,
            crate::xref::XREF_MARK,
            crate::index::INDEX_MARK,
            crate::anchored::OBJECT_MARK,
            crate::endnotes::NOTE_MARK,
        ]) {
            self.marks_inserted(pos, text);
        }
        // Paragraph formats: each new '\n' splits the current paragraph; the new paragraphs copy it.
        let pi = self.para_at(pos);
        let newlines = text.bytes().filter(|b| *b == b'\n').count();
        if newlines > 0 {
            let f = self.paras[pi].clone();
            for _ in 0..newlines {
                self.paras.insert(pi + 1, f.clone());
            }
        }
        self.text.insert_str(pos, text);
        // Character runs.
        self.splice_runs(pos, 0, text.len(), fmt);
        self.normalize();
        self.rev += 1;
    }

    /// Delete the byte range (clamped to char boundaries). Merged paragraphs keep the first one's format.
    pub fn delete(&mut self, range: std::ops::Range<usize>) {
        let a = floor_char_boundary(&self.text, range.start.min(self.text.len()));
        let b = floor_char_boundary(&self.text, range.end.min(self.text.len())).max(a);
        if a == b {
            return;
        }
        let refs = self.text[a..b].matches(crate::notes::FOOTNOTE_REF).count();
        if refs > 0 {
            let k = self.notes_before(a);
            self.notes.drain(k..(k + refs).min(self.notes.len()));
        }
        let erefs = self.text[a..b].matches(crate::endnotes::ENDNOTE_REF).count();
        if erefs > 0 {
            let k = self.endnotes_before(a);
            self.endnotes.drain(k..(k + erefs).min(self.endnotes.len()));
        }
        self.marks_deleted(a, b);
        let pi = self.para_at(a);
        let removed = self.text[a..b].bytes().filter(|c| *c == b'\n').count();
        // Merged paragraphs keep a table whose anchor survives the deletion.
        if removed > 0 && !self.tables.is_empty() {
            let ranges = self.para_ranges();
            let first = self.paras[pi].table.filter(|_| self.text[ranges[pi].start..a].contains(TABLE_ANCHOR));
            let last = self.paras[pi + removed]
                .table
                .filter(|_| self.text[b.min(ranges[pi + removed].end)..ranges[pi + removed].end].contains(TABLE_ANCHOR));
            self.paras[pi].table = first.or(last);
        }
        self.paras.drain(pi + 1..pi + 1 + removed);
        self.text.replace_range(a..b, "");
        self.remove_run_bytes(a, b - a);
        self.normalize();
        self.fix_tables();
        self.rev += 1;
    }

    /// Replace a range with text (delete + insert using the format at the start of the range).
    pub fn replace(&mut self, range: std::ops::Range<usize>, text: &str) {
        let fmt = if range.start < range.end { self.format_after(range.start).clone() } else { self.char_format_at(range.start).clone() };
        self.delete(range.clone());
        self.insert_with(range.start, text, fmt);
    }

    /// Format of the character *starting* at `pos`.
    pub fn format_after(&self, pos: usize) -> &CharFormat {
        let mut start = 0;
        for r in &self.chars {
            if pos < start + r.len {
                return &r.format;
            }
            start += r.len;
        }
        self.chars.last().map_or(&NO_FORMAT, |r| &r.format)
    }

    /// Apply `f` to the character format of every run intersecting `range` (splitting runs at the
    /// range boundaries). With an empty range nothing changes (callers set typing formats instead).
    pub fn format_chars(&mut self, range: std::ops::Range<usize>, mut f: impl FnMut(&mut CharFormat)) {
        let a = floor_char_boundary(&self.text, range.start);
        let b = floor_char_boundary(&self.text, range.end);
        if a >= b {
            return;
        }
        self.split_at(a);
        self.split_at(b);
        let mut start = 0;
        for r in &mut self.chars {
            let end = start + r.len;
            if start >= a && end <= b && r.len > 0 {
                f(&mut r.format);
            }
            start = end;
        }
        self.normalize();
        self.rev += 1;
    }

    /// Apply `f` to every paragraph intersecting `range` (an empty range = the paragraph at it).
    pub fn format_paras(&mut self, range: std::ops::Range<usize>, mut f: impl FnMut(&mut ParaFormat)) {
        let a = self.para_at(range.start);
        let b = self.para_at(range.end.max(range.start));
        for p in &mut self.paras[a..=b] {
            let table = p.table;
            f(p);
            p.table = table;
        }
        self.rev += 1;
    }

    /// Plain text of a range.
    pub fn slice(&self, range: std::ops::Range<usize>) -> &str {
        let a = floor_char_boundary(&self.text, range.start.min(self.text.len()));
        let b = floor_char_boundary(&self.text, range.end.min(self.text.len())).max(a);
        &self.text[a..b]
    }

    /// Check invariants (tests and debug assertions).
    pub fn check(&self) -> Result<(), String> {
        let n = self.text.matches('\n').count() + 1;
        if self.paras.len() != n {
            return Err(format!("story {}: {} paragraph formats for {} paragraphs", self.id.0, self.paras.len(), n));
        }
        let total: usize = self.chars.iter().map(|r| r.len).sum();
        if total != self.text.len() {
            return Err(format!("story {}: runs cover {total} bytes, text has {}", self.id.0, self.text.len()));
        }
        if self.chars.is_empty() {
            return Err("no char runs".into());
        }
        if !self.text.is_empty() && self.chars.iter().any(|r| r.len == 0) {
            return Err("empty run in non-empty story".into());
        }
        let mut pos = 0;
        for r in &self.chars {
            pos += r.len;
            if !self.text.is_char_boundary(pos) {
                return Err(format!("run boundary {pos} splits a character"));
            }
        }
        self.check_tables()?;
        self.check_notes()?;
        self.check_endnotes()?;
        self.check_marks()
    }

    // ---------- run helpers ----------

    fn split_at(&mut self, pos: usize) {
        let mut start = 0;
        for i in 0..self.chars.len() {
            let end = start + self.chars[i].len;
            if pos > start && pos < end {
                let tail = CharRun { len: end - pos, format: self.chars[i].format.clone() };
                self.chars[i].len = pos - start;
                self.chars.insert(i + 1, tail);
                return;
            }
            start = end;
        }
    }

    fn splice_runs(&mut self, pos: usize, _del: usize, ins: usize, fmt: CharFormat) {
        if self.chars.len() == 1 && self.chars[0].len == 0 {
            self.chars[0] = CharRun { len: ins, format: fmt };
            return;
        }
        self.split_at(pos);
        // Find insertion index: after the run that ends at pos.
        let mut start = 0;
        let mut idx = self.chars.len();
        for (i, r) in self.chars.iter().enumerate() {
            if start == pos {
                idx = i;
                break;
            }
            start += r.len;
        }
        // Extend the neighbouring run if it has the same format.
        if idx > 0 && self.chars[idx - 1].format == fmt {
            self.chars[idx - 1].len += ins;
        } else if idx < self.chars.len() && self.chars[idx].format == fmt {
            self.chars[idx].len += ins;
        } else {
            self.chars.insert(idx, CharRun { len: ins, format: fmt });
        }
    }

    fn remove_run_bytes(&mut self, pos: usize, len: usize) {
        let typing = self.char_format_at(pos).clone();
        let end = pos + len;
        let mut start = 0;
        for r in &mut self.chars {
            let (rs, re) = (start, start + r.len);
            start = re;
            let ov = re.min(end).saturating_sub(rs.max(pos));
            r.len -= ov;
        }
        self.chars.retain(|r| r.len > 0);
        if self.chars.is_empty() {
            self.chars.push(CharRun { len: 0, format: typing });
        }
    }

    fn normalize(&mut self) {
        let mut out: Vec<CharRun> = Vec::with_capacity(self.chars.len());
        for r in self.chars.drain(..) {
            if r.len == 0 && !out.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.format == r.format => last.len += r.len,
                Some(last) if last.len == 0 => *last = r,
                _ => out.push(r),
            }
        }
        if out.is_empty() {
            out.push(CharRun { len: 0, format: CharFormat::default() });
        }
        self.chars = out;
    }
}

pub fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(t: &str) -> Story {
        Story::with_text(StoryId(1), t, ParaFormat::default())
    }

    #[test]
    fn paragraphs_track_newlines() {
        let mut s = st("one\ntwo");
        assert_eq!(s.paras.len(), 2);
        s.insert(3, "\nmid");
        assert_eq!(s.text, "one\nmid\ntwo");
        assert_eq!(s.paras.len(), 3);
        s.delete(3..8);
        assert_eq!(s.text, "onetwo");
        assert_eq!(s.paras.len(), 1);
        s.check().unwrap();
        assert_eq!(s.para_ranges(), vec![0..6]);
    }

    #[test]
    fn char_formats_split_and_merge() {
        let mut s = st("hello world");
        s.format_chars(0..5, |f| f.over.size = Some(20.0));
        assert_eq!(s.chars.len(), 2);
        s.check().unwrap();
        // Typing at the end of the bold run continues it.
        s.insert(5, "!");
        assert_eq!(s.chars[0].len, 6);
        assert_eq!(s.text, "hello! world");
        s.format_chars(0..12, |f| f.over.size = Some(20.0));
        assert_eq!(s.chars.len(), 1);
        s.check().unwrap();
    }

    #[test]
    fn delete_everything_keeps_typing_format() {
        let mut s = st("abc");
        s.format_chars(0..3, |f| f.over.tracking = Some(50.0));
        s.delete(0..3);
        assert!(s.is_empty());
        assert_eq!(s.chars.len(), 1);
        assert_eq!(s.chars[0].format.over.tracking, Some(50.0));
        s.check().unwrap();
        s.insert(0, "x");
        assert_eq!(s.chars[0].format.over.tracking, Some(50.0));
    }

    #[test]
    fn unicode_boundaries_are_respected() {
        let mut s = st("héllo");
        s.delete(1..2); // inside 'é' (2 bytes): clamps
        s.check().unwrap();
        s.format_chars(0..3, |f| f.over.size = Some(9.0));
        s.check().unwrap();
    }

    #[test]
    fn replace_and_format_paras() {
        let mut s = st("a\nb\nc");
        s.format_paras(2..2, |p| p.style = "Head".into());
        assert_eq!(s.paras[1].style, "Head");
        s.replace(0..1, "AAA");
        assert_eq!(s.text, "AAA\nb\nc");
        s.check().unwrap();
        assert_eq!(s.para_at(4), 1);
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        #[derive(Debug, Clone)]
        enum Op {
            Ins(usize, String),
            Del(usize, usize),
            Fmt(usize, usize, u8),
        }

        fn op() -> impl Strategy<Value = Op> {
            prop_oneof![
                (0usize..64, "[a-zé\n ]{0,6}").prop_map(|(p, t)| Op::Ins(p, t)),
                (0usize..64, 0usize..10).prop_map(|(p, n)| Op::Del(p, n)),
                (0usize..64, 0usize..10, 0u8..3).prop_map(|(p, n, k)| Op::Fmt(p, n, k)),
            ]
        }

        proptest! {
            #[test]
            fn invariants_hold(ops in proptest::collection::vec(op(), 0..40)) {
                let mut s = st("seed text\nline two");
                let mut model = s.text.clone();
                for o in ops {
                    match o {
                        Op::Ins(p, t) => {
                            let p = floor_char_boundary(&s.text, p);
                            s.insert(p, &t);
                            model.insert_str(p, &t);
                        }
                        Op::Del(p, n) => {
                            let a = floor_char_boundary(&s.text, p);
                            let b = floor_char_boundary(&s.text, a + n).max(a);
                            s.delete(a..b);
                            model.replace_range(a..b, "");
                        }
                        Op::Fmt(p, n, k) => s.format_chars(p..p + n, |f| f.over.size = Some(8.0 + k as f64)),
                    }
                    prop_assert_eq!(&s.text, &model);
                    prop_assert!(s.check().is_ok(), "{:?}", s.check());
                }
            }
        }
    }
}
