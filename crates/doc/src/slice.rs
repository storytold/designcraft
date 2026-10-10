//! Formatted text slices: copy a range of a story (with paragraph and character formats,
//! footnotes, cross-references, index markers and tables) and insert it into another story.

use std::sync::Arc;

use crate::ids::StoryId;
use crate::index::INDEX_MARK;
use crate::notes::FOOTNOTE_REF;
use crate::story::{CharRun, Story, TABLE_ANCHOR};
use crate::xref::{ANCHOR_MARK, XREF_MARK};

impl Story {
    /// A frameless story holding `range` with its formatting and the objects anchored in it.
    pub fn extract(&self, range: std::ops::Range<usize>) -> Story {
        let a = crate::story::floor_char_boundary(&self.text, range.start.min(self.len()));
        let b = crate::story::floor_char_boundary(&self.text, range.end.min(self.len())).max(a);
        let mut out = Story::new(StoryId(0));
        out.direction = self.direction;
        out.text = self.text[a..b].to_string();
        let (p0, p1) = (self.para_at(a), self.para_at(b));
        out.paras = self.paras[p0..=p1].to_vec();
        out.chars = Vec::new();
        for (r, f) in self.runs() {
            let s = r.start.max(a);
            let e = r.end.min(b);
            if s < e {
                out.chars.push(CharRun { len: e - s, format: f.clone() });
            }
        }
        if out.chars.is_empty() {
            out.chars.push(CharRun { len: 0, format: self.char_format_at(a).clone() });
        }
        let count = |m: char, upto: usize| self.text[..upto].matches(m).count();
        let span = |m: char| count(m, a)..count(m, b);
        out.notes = self.notes[span(FOOTNOTE_REF)].to_vec();
        out.anchors = self.anchors[span(ANCHOR_MARK)].to_vec();
        out.xrefs = self.xrefs[span(XREF_MARK)].to_vec();
        out.index_refs = self.index_refs[span(INDEX_MARK)].to_vec();
        out.objects = self.objects[span(crate::anchored::OBJECT_MARK)].to_vec();
        for p in &out.paras {
            if let Some(id) = p.table
                && let Some(t) = self.tables.get(&id)
                && out.text.contains(TABLE_ANCHOR)
            {
                out.tables.insert(id, t.clone());
            }
        }
        out.fix_tables();
        out
    }

    /// Insert a slice made by [`Story::extract`] at `pos`, keeping its formatting: the first
    /// pasted paragraph merges into the paragraph at `pos` (it keeps that paragraph's format, as
    /// in InDesign), later paragraphs keep their own formats. Returns the end of the insertion.
    pub fn insert_story(&mut self, pos: usize, src: &Story) -> usize {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        if src.text.is_empty() {
            return pos;
        }
        // Text first (creates empty marker entries), then formats and marker payloads.
        let mut at = pos;
        for (r, f) in src.runs() {
            if r.is_empty() {
                continue;
            }
            self.insert_with(at, &src.text[r.clone()], f.clone());
            at += r.len();
        }
        let end = at;
        // Paragraph formats of the pasted paragraphs after the first.
        let p0 = self.para_at(pos);
        for (k, pf) in src.paras.iter().enumerate().skip(1) {
            if let Some(p) = self.paras.get_mut(p0 + k) {
                *p = crate::story::ParaFormat { table: None, ..pf.clone() };
            }
        }
        // Payloads of footnotes, cross-references and index markers, in order.
        let first = |m: char| self.text[..pos].matches(m).count();
        let (nf, nx, ni, na) = (first(FOOTNOTE_REF), first(XREF_MARK), first(INDEX_MARK), first(ANCHOR_MARK));
        let no = first(crate::anchored::OBJECT_MARK);
        for (k, o) in src.objects.iter().enumerate() {
            if let Some(slot) = self.objects.get_mut(no + k) {
                *slot = o.clone();
            }
        }
        for (k, n) in src.notes.iter().enumerate() {
            if let Some(slot) = self.notes.get_mut(nf + k) {
                let id = slot.id;
                *slot = Arc::new(crate::notes::Footnote { id, text: n.text.clone() });
            }
        }
        for (k, x) in src.xrefs.iter().enumerate() {
            if let Some(slot) = self.xrefs.get_mut(nx + k) {
                *slot = x.clone();
            }
        }
        for (k, r) in src.index_refs.iter().enumerate() {
            if let Some(slot) = self.index_refs.get_mut(ni + k) {
                *slot = r.clone();
            }
        }
        // Anchors are new destinations (fresh ids) with the copied names.
        for (k, a) in src.anchors.iter().enumerate() {
            if let Some(slot) = self.anchors.get_mut(na + k) {
                let id = slot.id;
                *slot = Arc::new(crate::xref::TextAnchor { id, name: a.name.clone() });
            }
        }
        // Tables: re-attach by paragraph (fresh ids so a pasted copy never shares an id).
        if !src.tables.is_empty() {
            let mut next = self.tables.keys().max().copied().unwrap_or(0) + 1;
            for (k, pf) in src.paras.iter().enumerate() {
                let Some(tid) = pf.table else { continue };
                let Some(t) = src.tables.get(&tid) else { continue };
                let pi = p0 + k;
                let mut t = (**t).clone();
                t.id = next;
                next += 1;
                self.tables.insert(t.id, Arc::new(t));
                if let Some(p) = self.paras.get_mut(pi) {
                    p.table = Some(next - 1);
                }
            }
            self.fix_tables();
        }
        self.rev += 1;
        end
    }

    /// Append the whole of `other` as new paragraphs after this story's text (threading two
    /// stories merges them this way). Its formatting, notes, endnotes, editorial notes, markers,
    /// anchored objects and tables come along; footnotes, endnotes, editorial notes and tables get
    /// ids that are free in this story. An empty story simply takes `other`'s content. Frames stay.
    pub fn append_story(&mut self, mut other: Story) {
        if other.text.is_empty() {
            return;
        }
        if self.text.is_empty() {
            let (id, frames, rev, link) = (self.id, std::mem::take(&mut self.frames), self.rev, self.link.take());
            *self = Story { id, frames, rev: rev.saturating_add(1), link, ..other };
            return;
        }
        if !other.tables.is_empty() {
            let mut next = self.tables.keys().max().copied().unwrap_or(0).saturating_add(1);
            let mut ids = std::collections::HashMap::new();
            for (old, t) in std::mem::take(&mut other.tables) {
                let new = next;
                next = next.saturating_add(1);
                let mut t = Arc::unwrap_or_clone(t);
                t.id = new;
                ids.insert(old, new);
                self.tables.insert(new, Arc::new(t));
            }
            for p in &mut other.paras {
                p.table = p.table.and_then(|id| ids.get(&id).copied());
            }
        }
        self.text.push('\n');
        self.text.push_str(&other.text);
        // The paragraph end takes the last run's format.
        if let Some(last) = self.chars.last_mut() {
            last.len += 1;
        }
        self.chars.extend(other.chars.into_iter().filter(|r| r.len > 0));
        self.paras.extend(other.paras);
        for n in other.notes {
            let id = self.next_note_id();
            self.notes.push(Arc::new(crate::notes::Footnote { id, text: n.text.clone() }));
        }
        for n in other.endnotes {
            let id = self.endnotes.iter().map(|x| x.id).max().unwrap_or(0).saturating_add(1);
            self.endnotes.push(Arc::new(crate::notes::Footnote { id, text: n.text.clone() }));
        }
        for n in other.editorial {
            let id = self.editorial.iter().map(|x| x.id).max().unwrap_or(0).saturating_add(1);
            self.editorial.push(Arc::new(crate::endnotes::EditorialNote { id, ..(*n).clone() }));
        }
        self.anchors.extend(other.anchors);
        self.xrefs.extend(other.xrefs);
        self.index_refs.extend(other.index_refs);
        self.objects.extend(other.objects);
        self.rev = self.rev.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::ParaFormat;

    #[test]
    fn copy_paste_keeps_formatting_and_notes() {
        let mut a = Story::with_text(StoryId(1), "Alpha beta\nGamma delta", ParaFormat::default());
        a.format_chars(0..5, |f| f.over.size = Some(20.0));
        a.format_paras(11..11, |p| p.style = "Head".into());
        a.insert_note(8, "A note.", ParaFormat::default());
        a.check().unwrap();
        let slice = a.extract(0..a.len());
        slice.check().unwrap();
        assert_eq!(slice.notes.len(), 1);
        let mut b = Story::with_text(StoryId(2), "Start: end", ParaFormat::default());
        let end = b.insert_story(7, &slice);
        b.check().unwrap();
        assert_eq!(&b.text[7..end], slice.text.as_str());
        assert_eq!(b.char_format_at(8).over.size, Some(20.0));
        assert_eq!(b.paras[1].style, "Head");
        assert_eq!(b.notes.len(), 1);
        assert_eq!(b.notes[0].text.text, "A note.");
        // A partial slice.
        let e = 8 + FOOTNOTE_REF.len_utf8();
        let part = a.extract(2..e);
        part.check().unwrap();
        assert_eq!(part.text, a.text[2..e]);
        assert_eq!(part.notes.len(), 1);
        assert!(a.extract(2..8).notes.is_empty());
    }

    #[test]
    fn append_story_with_maximal_ids_does_not_overflow() {
        let mut a = Story::with_text(StoryId(1), "Alpha", ParaFormat::default());
        a.tables.insert(u64::MAX, Arc::new(crate::table::Table::new(u64::MAX, 1, 1, 0, 0, 100.0)));
        let note = |id| Arc::new(crate::notes::Footnote { id, text: Story::with_text(StoryId(0), "n", ParaFormat::default()) });
        a.endnotes.push(note(u64::MAX));
        a.notes.push(note(u64::MAX));
        a.rev = u64::MAX;
        let mut b = Story::with_text(StoryId(2), "Beta", ParaFormat::default());
        b.tables.insert(1, Arc::new(crate::table::Table::new(1, 1, 1, 0, 0, 100.0)));
        b.endnotes.push(note(1));
        b.notes.push(note(1));
        a.append_story(b);
        assert_eq!(a.text, "Alpha\nBeta");
        assert_eq!(a.endnotes.len(), 2);
        assert_eq!(a.notes.len(), 2);
    }
}
