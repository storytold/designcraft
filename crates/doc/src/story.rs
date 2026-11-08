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
//! break characters ([`COLUMN_BREAK`], [`FRAME_BREAK`], [`PAGE_BREAK`], [`ODD_PAGE_BREAK`],
//! [`EVEN_PAGE_BREAK`]; each ends its paragraph, so it is followed by `\n`, see
//! [`is_break_char`]), `\u{AD}` discretionary hyphen,
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
pub const ODD_PAGE_BREAK: char = '\u{E011}';
pub const EVEN_PAGE_BREAK: char = '\u{E012}';

/// Encode a table-of-contents separator using InDesign's special-character notation.
pub fn toc_separator_notation(text: &str) -> String {
    text.replace('^', "^^").replace('\t', "^t").replace(RIGHT_INDENT_TAB, "^y").replace('\u{2003}', "^m").replace('\u{2002}', "^>")
}

/// Decode TOC notation, preserving unknown codes and an escaped literal caret.
pub fn toc_separator_text(notation: &str) -> String {
    let mut text = String::new();
    let mut chars = notation.chars();
    while let Some(ch) = chars.next() {
        if ch != '^' {
            text.push(ch);
            continue;
        }
        match chars.next() {
            Some('t') => text.push('\t'),
            Some('y') => text.push(RIGHT_INDENT_TAB),
            Some('m') => text.push('\u{2003}'),
            Some('>') => text.push('\u{2002}'),
            Some('^') | None => text.push('^'),
            Some(other) => {
                text.push('^');
                text.push(other);
            }
        }
    }
    text
}

/// A column, frame or page break character. It ends its paragraph: the story stores it as the
/// paragraph's last character, followed by the `\n` separator (also at the end of the story), and
/// the next paragraph starts in the next column, frame or page. A break inside a paragraph (from
/// an older document or a raw edit) still moves the rest of that paragraph on.
pub fn is_break_char(c: char) -> bool {
    matches!(c, COLUMN_BREAK | FRAME_BREAK | PAGE_BREAK | ODD_PAGE_BREAK | EVEN_PAGE_BREAK)
}

/// `text` with a `\n` after every break character that isn't followed by one. `next` is the
/// character after the text (None at the end of the story).
pub fn end_paragraphs_at_breaks(text: &str, next: Option<char>) -> String {
    let mut out = String::with_capacity(text.len() + 1);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if is_break_char(c) && chars.peek().copied().or(next) != Some('\n') {
            out.push('\n');
        }
    }
    out
}

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
    /// Story ▸ Optical Margin Alignment.
    #[serde(default, skip_serializing_if = "OpticalMargin::is_default")]
    pub optical_margin: OpticalMargin,
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
    /// File › Place with Create Links: the text file the story was placed from (Links panel).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_link: Option<TextFileLink>,
}

/// Story ▸ Optical Margin Alignment: punctuation and the edges of some letters hang outside the
/// frame's edges, in every paragraph of the story except those with Ignore Optical Margin.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OpticalMargin {
    pub on: bool,
    /// The type size (points) the hang amounts are set for: text of this size hangs by the
    /// built-in proportions of each character's advance, and the hang grows with this size.
    pub size: f64,
}

impl Default for OpticalMargin {
    fn default() -> Self {
        OpticalMargin { on: false, size: Self::DEFAULT_SIZE }
    }
}

impl OpticalMargin {
    pub const DEFAULT_SIZE: f64 = 12.0;
    /// InDesign's range for the size.
    pub const MIN_SIZE: f64 = 0.1;
    pub const MAX_SIZE: f64 = 1296.0;

    /// `v` clamped to the size range, or `None` when it isn't a finite number.
    pub fn clamp_size(v: f64) -> Option<f64> {
        v.is_finite().then(|| v.clamp(Self::MIN_SIZE, Self::MAX_SIZE))
    }

    /// The size the story's text hangs for, when the setting is on (a stored size outside the
    /// range is clamped, a non-finite one reads as the default).
    pub fn hang_size(&self) -> Option<f64> {
        self.on.then(|| Self::clamp_size(self.size).unwrap_or(Self::DEFAULT_SIZE))
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A linked text file: where a placed story came from, what the file held, and the Word Import
/// Options that brought it in, so Update Link can import it again the same way.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFileLink {
    pub path: String,
    /// The file's size and FNV-1a hash (hex) when it was placed or last updated.
    pub size: u64,
    pub hash: String,
    /// Its modification time then (seconds since 1970; 0: unknown), as Data Merge keeps for its
    /// source: while size and time are unchanged, the Links panel doesn't read the file.
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub mtime: i64,
    /// Imported style name → document style (`styleMap`), including the names Auto Rename chose.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub style_map: BTreeMap<String, String>,
    /// `styleConflicts`: useExisting, redefine or autoRename.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style_conflicts: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remove_styles: bool,
    /// Hash of the story's text and formatting right after the import: a different one means it
    /// was edited in the layout (an update replaces those edits).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub story_hash: String,
}

fn is_zero_i64(v: &i64) -> bool {
    *v == 0
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
            optical_margin: OpticalMargin::default(),
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
            file_link: None,
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

    /// The paragraphs `r` touches (at least the one at `r.start`), as (index, byte range without
    /// its return), found from `r` itself rather than by listing every paragraph of the story.
    pub fn paras_touching(&self, r: std::ops::Range<usize>) -> Vec<(usize, std::ops::Range<usize>)> {
        let start = floor_char_boundary(&self.text, r.start.min(self.text.len()));
        let mut pi = self.para_at(start);
        let mut a = self.text[..start].rfind('\n').map_or(0, |i| i + 1);
        let mut out = Vec::new();
        loop {
            let b = self.text[a..].find('\n').map_or(self.text.len(), |i| a + i);
            out.push((pi, a..b));
            // The next paragraph starts after this one's return: is it still inside `r`?
            if b >= self.text.len() || b + 1 >= r.end {
                return out;
            }
            (a, pi) = (b + 1, pi + 1);
        }
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

    /// The format text inserted at byte `pos` takes: the run before `pos`
    /// ([`Story::char_format_at`]), except on an empty paragraph, where it is that paragraph's own
    /// mark (the line's only character, so its format is the one chosen for that line).
    pub fn insert_format_at(&self, pos: usize) -> &CharFormat {
        let b = self.text.as_bytes();
        let empty_para = b.get(pos) == Some(&b'\n') && pos.checked_sub(1).and_then(|p| b.get(p)) == Some(&b'\n');
        if empty_para { self.format_after(pos) } else { self.char_format_at(pos) }
    }

    /// Insert `text` at byte `pos` using the format at that position ([`Story::insert_format_at`]).
    pub fn insert(&mut self, pos: usize, text: &str) {
        let fmt = self.insert_format_at(pos).clone();
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

    /// [`Story::insert_with`] without the table fix-up.
    pub(crate) fn insert_with_raw(&mut self, pos: usize, text: &str, fmt: CharFormat) {
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
        if newlines > 0
            && let Some(f) = self.paras.get(pi).cloned()
        {
            // All at once: an insert per return made many returns quadratic (#1588).
            self.paras.splice(pi + 1..pi + 1, std::iter::repeat_n(f, newlines));
        }
        self.text.insert_str(pos, text);
        // Character runs.
        self.splice_runs(pos, 0, text.len(), fmt);
        self.normalize();
        self.rev = self.rev.wrapping_add(1);
    }

    /// End the paragraph after every break character that isn't followed by a `\n`.
    pub fn end_paragraphs_at_breaks(&mut self) {
        let at: Vec<usize> = self
            .text
            .char_indices()
            .filter(|&(i, c)| is_break_char(c) && !self.text[i + c.len_utf8()..].starts_with('\n'))
            .map(|(i, c)| i + c.len_utf8())
            .collect();
        for &pos in at.iter().rev() {
            let fmt = self.char_format_at(pos).clone();
            self.insert_with_raw(pos, "\n", fmt);
        }
        if !at.is_empty() {
            self.fix_tables();
        }
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
        // Merged paragraphs keep the tables whose anchors survive the deletion (the fix-up gives
        // each its own paragraph again).
        let mut claims = Vec::new();
        if removed > 0 && !self.tables.is_empty() {
            let ranges = self.para_ranges();
            let anchor = |pi: usize, r: std::ops::Range<usize>| {
                self.paras[pi].table.and_then(|id| self.text[r.clone()].find(TABLE_ANCHOR).map(|k| (r.start + k, id)))
            };
            let first = anchor(pi, ranges[pi].start..a);
            let end = ranges[pi + removed].end;
            let last = anchor(pi + removed, b.min(end)..end).map(|(at, id)| (at.saturating_sub(b - a), id));
            self.paras[pi].table = first.or(last).map(|(_, id)| id);
            claims.extend(first.into_iter().chain(last));
        }
        self.paras.drain(pi + 1..pi + 1 + removed);
        self.text.replace_range(a..b, "");
        self.remove_run_bytes(a, b - a);
        self.normalize();
        self.fix_tables_with(&claims);
        self.rev = self.rev.wrapping_add(1);
    }

    /// Replace a range with text (delete + insert using the format at the start of the range).
    pub fn replace(&mut self, range: std::ops::Range<usize>, text: &str) {
        let fmt = if range.start < range.end { self.format_after(range.start).clone() } else { self.insert_format_at(range.start).clone() };
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
        self.rev = self.rev.wrapping_add(1);
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
        self.rev = self.rev.wrapping_add(1);
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

    /// `paras_touching` gives the paragraphs a range reaches into (returns included), as listing
    /// every paragraph would, for every range of a story with empty and multi-byte paragraphs.
    #[test]
    fn paras_touching_matches_the_full_listing() {
        let s = st("ab

çé
last");
        let all = s.para_ranges();
        for a in 0..=s.len() {
            for b in a..=s.len() {
                if !s.text.is_char_boundary(a) || !s.text.is_char_boundary(b) {
                    continue;
                }
                let want: Vec<(usize, std::ops::Range<usize>)> =
                    all.iter().cloned().enumerate().filter(|(i, p)| *i == s.para_at(a) || (a < b && p.start < b && a <= p.end)).collect();
                assert_eq!(s.paras_touching(a..b), want, "{a}..{b}");
            }
        }
    }

    #[test]
    fn optical_margin_is_optional_in_saved_stories() {
        let plain = serde_json::to_value(st("Text")).unwrap();
        assert!(plain.get("opticalMargin").is_none());
        let old: Story = serde_json::from_value(plain).unwrap();
        assert_eq!(old.optical_margin, OpticalMargin { on: false, size: 12.0 });
        let mut s = st("Text");
        s.optical_margin = OpticalMargin { on: true, size: 18.0 };
        let back: Story = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back.optical_margin, s.optical_margin);
        // A hostile stored size is clamped where it is used.
        assert_eq!(OpticalMargin { on: true, size: 1e9 }.hang_size(), Some(OpticalMargin::MAX_SIZE));
        assert_eq!(OpticalMargin { on: true, size: -3.0 }.hang_size(), Some(OpticalMargin::MIN_SIZE));
        assert_eq!(OpticalMargin { on: false, size: 18.0 }.hang_size(), None);
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

    /// Each return inserted its paragraph format on its own, quadratic in the returns (#1588).
    #[test]
    fn many_returns_split_a_paragraph_at_once() {
        let para = ParaFormat { style: "Body".into(), ..ParaFormat::default() };
        let started = std::time::Instant::now();
        let s = Story::with_text(StoryId(1), &"\n".repeat(20_000), para.clone());
        let took = started.elapsed();
        assert!(took < std::time::Duration::from_secs(1), "took {took:?}");
        assert_eq!(s.paras.len(), 20_001);
        assert!(s.paras.iter().all(|p| *p == para));
        s.check().unwrap();
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
    fn saved_revision_wraps_without_stopping_edits() {
        let mut s = st("seed");
        s.rev = u64::MAX;
        // Revisions are serialized; a valid saved story can reach the boundary.
        let bytes = serde_json::to_vec(&s).unwrap();
        let mut s: Story = serde_json::from_slice(&bytes).unwrap();
        s.insert(s.len(), " text");
        assert_eq!(s.rev, 0);
        assert_eq!(s.text, "seed text");
        s.rev = u64::MAX;
        s.delete(4..s.len());
        assert_eq!(s.rev, 0);
        assert_eq!(s.text, "seed");
        s.rev = u64::MAX;
        s.format_chars(0..s.len(), |f| f.over.size = Some(18.0));
        assert_eq!(s.rev, 0);
        assert_eq!(s.char_format_at(0).over.size, Some(18.0));
        s.rev = u64::MAX;
        s.format_paras(0..s.len(), |p| p.style = "Heading".into());
        assert_eq!(s.rev, 0);
        assert_eq!(s.paras[0].style, "Heading");
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
