//! Endnotes (Type › Insert Endnote).
//!
//! An endnote reference is an [`ENDNOTE_REF`] character; the story's [`Story::endnotes`] holds one
//! entry per reference in text order (like footnotes). Numbering runs through the document in
//! page order. The endnote frame shows a story that is generated from every endnote: a heading
//! and one numbered paragraph per endnote ([`Document::endnote_text`]); its text isn't typed
//! into, endnotes are edited through their references.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attrs::NumberStyle;
use crate::ids::StoryId;
use crate::notes::Footnote;
use crate::story::{BASIC_PARAGRAPH, CharFormat, ParaFormat, Story};

/// Endnote reference character (the number shown in the text).
pub const ENDNOTE_REF: char = '\u{E00F}';

/// Editorial note anchor (Type › Notes): never printed; the note's text is in
/// [`Story::editorial`], one per anchor in text order.
pub const NOTE_MARK: char = '\u{E010}';

/// An editorial note.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorialNote {
    pub id: u64,
    pub author: String,
    pub text: String,
}

/// Document Endnote Options.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EndnoteOptions {
    pub style: NumberStyle,
    pub start_at: u32,
    pub prefix: String,
    pub suffix: String,
    /// Endnote frame heading (empty: none) and its paragraph style.
    pub heading: String,
    pub heading_style: String,
    /// Endnote entries' paragraph style.
    pub para_style: String,
    /// Between the number and the endnote text.
    pub separator: String,
}

impl Default for EndnoteOptions {
    fn default() -> Self {
        EndnoteOptions {
            style: NumberStyle::Arabic,
            start_at: 1,
            prefix: String::new(),
            suffix: String::new(),
            heading: "Endnotes".into(),
            heading_style: BASIC_PARAGRAPH.into(),
            para_style: BASIC_PARAGRAPH.into(),
            separator: "\t".into(),
        }
    }
}

impl EndnoteOptions {
    pub fn label(&self, n: u32) -> String {
        format!("{}{}{}", self.prefix, self.style.format(n), self.suffix)
    }
}

impl Story {
    /// Index (in [`Story::endnotes`]) of the endnote whose reference is the k-th before `pos`.
    pub fn endnotes_before(&self, pos: usize) -> usize {
        let pos = crate::story::floor_char_boundary(&self.text, pos);
        self.text[..pos].matches(ENDNOTE_REF).count()
    }

    pub fn endnote(&self, id: u64) -> Option<&Footnote> {
        self.endnotes.iter().find(|n| n.id == id).map(|n| &**n)
    }

    /// Mutable endnote (copy-on-write); bumps the story revision.
    pub fn endnote_mut(&mut self, id: u64) -> Option<&mut Footnote> {
        let n = self.endnotes.iter_mut().find(|n| n.id == id)?;
        self.rev = self.rev.wrapping_add(1);
        Some(Arc::make_mut(n))
    }

    /// Insert an endnote reference at `pos` with the given text. Returns the endnote id, or
    /// `None` if the story has no endnote for the new reference.
    pub fn insert_endnote(&mut self, pos: usize, text: &str, para: ParaFormat) -> Option<u64> {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        let fmt = self.char_format_at(pos).clone();
        self.insert_with(pos, &ENDNOTE_REF.to_string(), fmt);
        // A file may hold fewer endnotes than references; give every reference one.
        self.fix_endnotes();
        let k = self.endnotes_before(pos);
        let note = Arc::make_mut(self.endnotes.get_mut(k)?);
        note.text = Story::with_text(StoryId(0), text, para);
        Some(note.id)
    }

    /// Insert empty endnotes for `n` new reference characters before which `k` references sit.
    pub(crate) fn endnotes_inserted(&mut self, k: usize, n: usize) {
        for i in 0..n {
            let id = self.endnotes.iter().map(|x| x.id).max().unwrap_or(0) + 1;
            let mut text = Story::new(StoryId(0));
            text.chars[0].format = CharFormat::default();
            self.endnotes.insert((k + i).min(self.endnotes.len()), Arc::new(Footnote { id, text }));
        }
    }

    /// Make the endnote list match the reference characters (after wholesale text changes).
    pub fn fix_endnotes(&mut self) {
        let n = self.text.matches(ENDNOTE_REF).count();
        if n < self.endnotes.len() {
            self.endnotes.truncate(n);
        } else if n > self.endnotes.len() {
            let k = self.endnotes.len();
            self.endnotes_inserted(k, n - k);
        }
    }

    pub(crate) fn check_endnotes(&self) -> Result<(), String> {
        let n = self.text.matches(ENDNOTE_REF).count();
        if n != self.endnotes.len() {
            return Err(format!("story {}: {n} endnote references, {} endnotes", self.id.0, self.endnotes.len()));
        }
        Ok(())
    }
}

impl crate::Document {
    /// Stories with endnotes in numbering order (page of their first frame, then id).
    fn endnote_order(&self) -> Vec<&Story> {
        let key = |st: &Story| -> (usize, u64) { (st.frames.first().and_then(|f| self.page_of_item(*f)).unwrap_or(usize::MAX), st.id.0) };
        let mut v: Vec<&Story> = self.stories.values().map(|s| s.as_ref()).filter(|s| !s.endnotes.is_empty()).collect();
        v.sort_by_key(|s| key(s));
        v
    }

    /// Number of the first endnote of story `sid`.
    pub fn endnote_start(&self, sid: StoryId) -> u32 {
        let mut n = self.endnote_options.start_at;
        for st in self.endnote_order() {
            if st.id == sid {
                break;
            }
            n += st.endnotes.len() as u32;
        }
        n
    }

    /// The endnote frame's text: the heading, then "number separator text" per endnote, in order.
    pub fn endnote_text(&self, id: StoryId) -> Story {
        let o = &self.endnote_options;
        let para = |style: &str| ParaFormat { style: style.to_string(), ..Default::default() };
        let mut out = Story::with_text(id, &o.heading, para(&o.heading_style));
        let mut n = o.start_at;
        for st in self.endnote_order() {
            for note in &st.endnotes {
                let at = out.len();
                let first = at == 0 && o.heading.is_empty();
                if !first {
                    out.insert(at, "\n");
                }
                let pi = out.paras.len() - 1;
                out.paras[pi] = para(&o.para_style);
                let at = out.len();
                out.insert_with(at, &format!("{}{}", o.label(n), o.separator), CharFormat::default());
                for (r, f) in note.text.runs() {
                    let at = out.len();
                    out.insert_with(at, &note.text.text[r].replace('\n', " "), f.clone());
                }
                n += 1;
            }
        }
        out
    }

    /// Bring the endnote frame's story up to date with the endnotes (after any edit).
    pub fn sync_endnote_story(&mut self) {
        let Some(sid) = self.endnote_story else { return };
        let Some(cur) = self.stories.get(&sid) else {
            self.endnote_story = None;
            return;
        };
        let fresh = self.endnote_text(sid);
        if cur.text == fresh.text && cur.paras == fresh.paras && cur.chars == fresh.chars {
            return;
        }
        let Some(st) = self.stories.get_mut(&sid) else { return };
        let st = Arc::make_mut(st);
        let (frames, rev) = (std::mem::take(&mut st.frames), st.rev);
        *st = Story { frames, rev: rev.wrapping_add(1), ..fresh };
    }
}
