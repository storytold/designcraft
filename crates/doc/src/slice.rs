//! Formatted text slices: copy a range of a story (with paragraph and character formats,
//! footnotes, endnotes, editorial notes, cross-references, index markers and tables) and insert
//! it into another story.

use std::sync::Arc;

use crate::endnotes::{ENDNOTE_REF, NOTE_MARK};
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
        // Payloads in text order; a file may hold fewer payloads than markers, so the lists are
        // read with `get` and the slice's own lists are made to match its markers below.
        let span = |m: char| {
            let lo = self.text[..a].matches(m).count();
            lo..lo + self.text[a..b].matches(m).count()
        };
        out.notes = marked(&self.notes, span(FOOTNOTE_REF));
        out.endnotes = marked(&self.endnotes, span(ENDNOTE_REF));
        out.editorial = marked(&self.editorial, span(NOTE_MARK));
        out.anchors = marked(&self.anchors, span(ANCHOR_MARK));
        out.xrefs = marked(&self.xrefs, span(XREF_MARK));
        out.index_refs = marked(&self.index_refs, span(INDEX_MARK));
        out.objects = marked(&self.objects, span(crate::anchored::OBJECT_MARK));
        out.fix_notes();
        out.fix_endnotes();
        out.fix_marks();
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
        // The table anchored in the paragraph at `pos` keeps its anchor wherever the paste moves it.
        let p0 = self.para_at(pos);
        let own = self.paras.get(p0).and_then(|p| p.table).and_then(|id| {
            let r = self.para_ranges().get(p0)?.clone();
            Some((r.start + self.text.get(r)?.find(TABLE_ANCHOR)?, id))
        });
        // Text first (creates empty marker entries), then formats and marker payloads; tables are
        // fixed up once at the end.
        let mut at = pos;
        for (r, f) in src.runs() {
            if r.is_empty() {
                continue;
            }
            self.insert_with_raw(at, &src.text[r.clone()], f.clone());
            at += r.len();
        }
        let end = at;
        let mut claims: Vec<(usize, u64)> = own.map(|(a, id)| (if a >= pos { a + (end - pos) } else { a }, id)).into_iter().collect();
        // Paragraph formats of the pasted paragraphs after the first.
        for (k, pf) in src.paras.iter().enumerate().skip(1) {
            if let Some(p) = self.paras.get_mut(p0 + k) {
                *p = crate::story::ParaFormat { table: None, ..pf.clone() };
            }
        }
        // Marker payloads in text order, keeping newly allocated destination note ids.
        let first = |m: char| self.text[..pos].matches(m).count();
        let (nf, nx, ni, na) = (first(FOOTNOTE_REF), first(XREF_MARK), first(INDEX_MARK), first(ANCHOR_MARK));
        let (no, ne, nn) = (first(crate::anchored::OBJECT_MARK), first(ENDNOTE_REF), first(NOTE_MARK));
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
        // Notes keep the ids made for their new markers, so a pasted copy never shares an id.
        // Payload copies stop at the inserted markers so excess source entries cannot overwrite
        // notes already following the insertion point.
        for (k, n) in src.endnotes.iter().take(src.text.matches(ENDNOTE_REF).count()).enumerate() {
            if let Some(slot) = self.endnotes.get_mut(ne + k) {
                let id = slot.id;
                *slot = Arc::new(crate::notes::Footnote { id, text: n.text.clone() });
            }
        }
        for (k, n) in src.editorial.iter().take(src.text.matches(NOTE_MARK).count()).enumerate() {
            if let Some(slot) = self.editorial.get_mut(nn + k) {
                let id = slot.id;
                *slot = Arc::new(crate::endnotes::EditorialNote { id, ..(**n).clone() });
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
        // Tables: re-attach at their anchors (fresh ids so a pasted copy never shares an id).
        if !src.tables.is_empty() {
            let mut next = self.tables.keys().max().copied().unwrap_or(0) + 1;
            let ranges = src.para_ranges();
            for (pf, r) in src.paras.iter().zip(ranges) {
                let Some(tid) = pf.table else { continue };
                let Some(t) = src.tables.get(&tid) else { continue };
                let Some(k) = src.text.get(r.clone()).and_then(|t| t.find(TABLE_ANCHOR)) else { continue };
                let mut t = (**t).clone();
                t.id = next;
                next += 1;
                claims.push((pos + r.start + k, t.id));
                self.tables.insert(t.id, Arc::new(t));
            }
        }
        let splits = self.fix_tables_with(&claims);
        self.rev = self.rev.wrapping_add(1);
        // Past any return the fix-up put inside the pasted text.
        end + splits.iter().filter(|&&s| s < end).count()
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
            let mut ids = std::collections::HashMap::new();
            let mut next = crate::ids::next_local_id(self.tables.keys().copied());
            for (old, t) in std::mem::take(&mut other.tables) {
                let new = next;
                let mut t = Arc::unwrap_or_clone(t);
                t.id = new;
                ids.insert(old, new);
                self.tables.insert(new, Arc::new(t));
                // Continue through free ids without rescanning the table map for its maximum.
                // On exhaustion wrap to gaps; an allocated map cannot occupy every u64 id.
                next = next.checked_add(1).unwrap_or(1);
                while self.tables.contains_key(&next) {
                    next = next.checked_add(1).unwrap_or(1);
                }
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
            let id = crate::ids::next_local_id(self.endnotes.iter().map(|x| x.id));
            self.endnotes.push(Arc::new(crate::notes::Footnote { id, text: n.text.clone() }));
        }
        for n in other.editorial {
            let id = crate::ids::next_local_id(self.editorial.iter().map(|x| x.id));
            self.editorial.push(Arc::new(crate::endnotes::EditorialNote { id, ..(*n).clone() }));
        }
        self.anchors.extend(other.anchors);
        self.xrefs.extend(other.xrefs);
        self.index_refs.extend(other.index_refs);
        self.objects.extend(other.objects);
        self.rev = self.rev.saturating_add(1);
    }
}

/// The payloads of the markers numbered `span` (in text order), as far as `list` holds them.
fn marked<T: Clone>(list: &[T], span: std::ops::Range<usize>) -> Vec<T> {
    let n = list.len();
    list.get(span.start.min(n)..span.end.min(n)).map(<[T]>::to_vec).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::ParaFormat;

    #[test]
    fn pasting_a_table_next_to_another_gives_each_its_own_paragraph() {
        let mut s = Story::with_text(StoryId(1), "x", ParaFormat::default());
        let a = s.insert_table(1, crate::Table::new(5, 1, 1, 0, 0, 10.0));
        let n = TABLE_ANCHOR.len_utf8();
        let copy = s.extract(a..a + n);
        // After the anchor: the copy follows the original.
        let mut after = s.clone();
        let end = after.insert_story(a + n, &copy);
        after.check().unwrap();
        assert_eq!(after.text, format!("x\n{TABLE_ANCHOR}\n{TABLE_ANCHOR}\n"));
        assert_eq!((after.table_anchor(5), after.table_anchor(6)), (Some(a), Some(a + n + 1)));
        assert_eq!(end, a + 2 * n + 1);
        // Before the anchor: the copy comes first and the original keeps its table.
        let mut before = s.clone();
        let end = before.insert_story(a, &copy);
        before.check().unwrap();
        assert_eq!((before.table_anchor(6), before.table_anchor(5)), (Some(a), Some(a + n + 1)));
        assert_eq!(end, a + n);
        // Paragraphs pasted before the anchor keep the table too.
        let mut text = s.clone();
        text.insert_story(a, &Story::with_text(StoryId(2), "Hello\nWorld", ParaFormat::default()));
        text.check().unwrap();
        assert_eq!(text.table_anchor(5), Some(a + "Hello\nWorld".len()));
    }

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

    fn append_note_pair(st: &mut Story, text: &str) {
        st.insert_endnote(st.len(), text, ParaFormat::default());
        st.insert(st.len(), &NOTE_MARK.to_string());
        let note = Arc::make_mut(st.editorial.last_mut().unwrap());
        note.text = text.into();
        note.author = format!("{text} author");
    }

    #[test]
    fn clipped_endnotes_and_editorial_keep_payloads_with_fresh_destination_ids() {
        let mut source = Story::new(StoryId(1));
        append_note_pair(&mut source, "outside before");
        let start = source.len();
        append_note_pair(&mut source, "Citation é\nsecond");
        let end = source.len();
        let citation = &mut Arc::make_mut(&mut source.endnotes[1]).text;
        citation.format_chars(0..8, |f| f.over.size = Some(19.0));
        citation.paras[1].style = "Citation".into();
        append_note_pair(&mut source, "outside after");
        source.check().unwrap();
        let original = source.clone();
        let slice = source.extract(start..end);
        slice.check().unwrap();
        assert_eq!(slice.text, format!("{ENDNOTE_REF}{NOTE_MARK}"));
        assert_eq!(slice.endnotes, vec![source.endnotes[1].clone()]);
        assert_eq!(slice.editorial, vec![source.editorial[1].clone()]);

        let mut dest = Story::new(StoryId(2));
        append_note_pair(&mut dest, "destination before");
        let at = dest.len();
        append_note_pair(&mut dest, "destination after");
        let before = dest.clone();
        assert_eq!(dest.insert_story(at, &slice), at + slice.len());
        dest.check().unwrap();
        assert_eq!(dest.text, format!("{ENDNOTE_REF}{NOTE_MARK}").repeat(3));
        assert_eq!(dest.endnotes[1].text, slice.endnotes[0].text);
        assert_eq!(dest.editorial[1].text, "Citation é\nsecond");
        assert_eq!(dest.editorial[1].author, "Citation é\nsecond author");
        assert_eq!((dest.endnotes[1].id, dest.editorial[1].id), (3, 3));
        assert_eq!(dest.endnotes[0], before.endnotes[0]);
        assert_eq!(dest.endnotes[2], before.endnotes[1]);
        assert_eq!(dest.editorial[0], before.editorial[0]);
        assert_eq!(dest.editorial[2], before.editorial[1]);
        Arc::make_mut(&mut dest.endnotes[1]).text.insert(0, "changed");
        Arc::make_mut(&mut dest.editorial[1]).author = "changed".into();
        assert_eq!(source, original);
        assert_eq!(slice.endnotes[0], original.endnotes[1]);
        assert_eq!(slice.editorial[0], original.editorial[1]);
    }

    #[test]
    fn extract_tolerates_missing_endnote_and_editorial_payloads() {
        let mut source = Story::new(StoryId(1));
        for text in ["before", "selected", "missing"] {
            append_note_pair(&mut source, text);
        }
        let pair_len = ENDNOTE_REF.len_utf8() + NOTE_MARK.len_utf8();
        for available in [0, 1, 2] {
            let mut damaged = source.clone();
            damaged.endnotes.truncate(available);
            damaged.editorial.truncate(available);
            let original = damaged.clone();
            let slice = damaged.extract(pair_len..damaged.len());
            assert_eq!(slice.text, format!("{ENDNOTE_REF}{NOTE_MARK}").repeat(2));
            let expected = if available == 2 { 1 } else { 0 };
            // The slice is a valid story: missing payloads become empty notes.
            slice.check().unwrap();
            assert_eq!((slice.endnotes.len(), slice.editorial.len()), (2, 2));
            if expected == 1 {
                assert_eq!(slice.endnotes[0], source.endnotes[1]);
                assert_eq!(slice.editorial[0], source.editorial[1]);
            } else {
                assert!(slice.endnotes[0].text.is_empty());
                assert!(slice.editorial[0].text.is_empty());
            }
            let mut dest = Story::new(StoryId(2));
            append_note_pair(&mut dest, "untouched");
            let tail = dest.clone();
            dest.insert_story(0, &slice);
            dest.check().unwrap();
            assert_eq!(dest.text, format!("{ENDNOTE_REF}{NOTE_MARK}").repeat(3));
            assert_eq!((dest.endnotes.len(), dest.editorial.len()), (3, 3));
            assert_eq!(dest.endnotes[2], tail.endnotes[0]);
            assert_eq!(dest.editorial[2], tail.editorial[0]);
            assert_eq!(dest.endnotes[0].text.text, if expected == 1 { "selected" } else { "" });
            assert_eq!(dest.editorial[0].text, if expected == 1 { "selected" } else { "" });
            assert!(dest.endnotes[1].text.is_empty());
            assert!(dest.editorial[1].text.is_empty());
            assert_eq!(damaged, original);
        }
    }

    #[test]
    fn append_tables_preserves_sparse_and_wrapping_ids() {
        for existing in [vec![1, 3, u64::MAX], vec![1, 3, u64::MAX - 1], vec![1, 3, 8]] {
            let story = |id, ids: &[u64]| {
                let mut s = Story::with_text(StoryId(id), &vec![crate::TABLE_ANCHOR.to_string(); ids.len()].join("\n"), ParaFormat::default());
                for (p, &tid) in s.paras.iter_mut().zip(ids) {
                    p.table = Some(tid);
                    let mut t = crate::table::Table::new(tid, 1, 1, 0, 0, 100.0);
                    t.cells[0].text = Story::with_text(StoryId(0), &format!("{id}:{tid}"), ParaFormat::default());
                    s.tables.insert(tid, Arc::new(t));
                }
                s
            };
            let mut a = story(1, &existing);
            let b = story(2, &[1, 2, 3]);
            a.check().unwrap();
            b.check().unwrap();
            a.append_story(b);
            a.check().unwrap();
            assert_eq!(a.tables.len(), 6);
            for (p, tid) in a.paras.iter().take(3).zip(&existing) {
                assert_eq!(p.table, Some(*tid));
                assert_eq!(a.tables[tid].cells[0].text.text, format!("1:{tid}"));
            }
            for (p, tid) in a.paras.iter().skip(3).zip(1..=3) {
                assert_eq!(a.tables[&p.table.unwrap()].cells[0].text.text, format!("2:{tid}"));
            }
        }
    }

    #[test]
    fn excess_note_payloads_do_not_overwrite_destination_neighbors() {
        let mut payload = Story::new(StoryId(1));
        append_note_pair(&mut payload, "copied");
        for has_markers in [false, true] {
            let mut src = if has_markers { payload.clone() } else { Story::with_text(StoryId(1), "plain", ParaFormat::default()) };
            src.endnotes.push(payload.endnotes[0].clone());
            src.editorial.push(payload.editorial[0].clone());
            let original = src.clone();
            let mut dest = Story::new(StoryId(2));
            append_note_pair(&mut dest, "before");
            let at = dest.len();
            append_note_pair(&mut dest, "after");
            let before = dest.clone();
            assert_eq!(dest.insert_story(at, &src), at + src.len());
            dest.check().unwrap();
            let pair = format!("{ENDNOTE_REF}{NOTE_MARK}");
            assert_eq!(dest.text, format!("{pair}{}{pair}", src.text));
            let tail = if has_markers { 2 } else { 1 };
            assert_eq!((dest.endnotes.len(), dest.editorial.len()), (tail + 1, tail + 1));
            assert_eq!(dest.endnotes[0], before.endnotes[0]);
            assert_eq!(dest.endnotes[tail], before.endnotes[1]);
            assert_eq!(dest.editorial[0], before.editorial[0]);
            assert_eq!(dest.editorial[tail], before.editorial[1]);
            if has_markers {
                assert_eq!(dest.endnotes[1].text, payload.endnotes[0].text);
                assert_eq!((dest.editorial[1].text.as_str(), dest.editorial[1].author.as_str()), ("copied", "copied author"));
                assert_eq!((dest.endnotes[1].id, dest.editorial[1].id), (3, 3));
            }
            assert_eq!(src, original);
        }
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

    #[test]
    fn copy_paste_keeps_endnotes_and_editorial_notes() {
        let mut a = Story::with_text(StoryId(1), "One two three", ParaFormat::default());
        a.insert_endnote(3, "An endnote.", ParaFormat::default());
        let at = a.text.find("three").unwrap();
        a.insert(at, &NOTE_MARK.to_string());
        let n = Arc::make_mut(&mut a.editorial[0]);
        (n.author, n.text) = ("Ed".into(), "Check this.".into());
        a.check().unwrap();
        let slice = a.extract(0..a.len());
        slice.check().unwrap();
        assert_eq!(slice.endnotes[0].text.text, "An endnote.");
        assert_eq!(slice.editorial[0].text, "Check this.");
        // Duplicate into the same story: the copies get their own ids and are independent.
        let len = a.len();
        a.insert_story(len, &slice);
        a.check().unwrap();
        assert_eq!(a.endnotes.len(), 2);
        assert_eq!(a.editorial.len(), 2);
        assert_ne!(a.endnotes[0].id, a.endnotes[1].id);
        assert_ne!(a.editorial[0].id, a.editorial[1].id);
        assert_eq!(a.endnotes[1].text.text, "An endnote.");
        assert_eq!((a.editorial[1].author.as_str(), a.editorial[1].text.as_str()), ("Ed", "Check this."));
        let copy = a.endnotes[1].id;
        a.endnote_mut(copy).unwrap().text = Story::with_text(StoryId(0), "Changed.", ParaFormat::default());
        Arc::make_mut(&mut a.editorial[1]).text = "Changed.".into();
        assert_eq!(a.endnotes[0].text.text, "An endnote.");
        assert_eq!(a.editorial[0].text, "Check this.");
        assert_eq!(slice.endnotes[0].text.text, "An endnote.");
        // Into another story, between existing notes.
        let mut b = Story::with_text(StoryId(2), "x y", ParaFormat::default());
        b.insert_endnote(0, "B first.", ParaFormat::default());
        b.insert_endnote(b.len(), "B last.", ParaFormat::default());
        let mid = b.text.find(' ').unwrap();
        b.insert_story(mid, &slice);
        b.check().unwrap();
        let texts: Vec<_> = b.endnotes.iter().map(|n| n.text.text.as_str()).collect();
        assert_eq!(texts, ["B first.", "An endnote.", "B last."]);
        assert_eq!(b.editorial[0].text, "Check this.");
    }

    #[test]
    fn extract_survives_more_markers_than_payloads() {
        let mut a = Story::with_text(StoryId(1), "a b c d e f", ParaFormat::default());
        for (pos, m) in [(1, FOOTNOTE_REF), (4, ENDNOTE_REF), (7, NOTE_MARK), (10, XREF_MARK)] {
            a.insert(pos, &format!("{m}{m}"));
        }
        // A hostile file: two markers of each kind, at most one payload.
        a.notes.truncate(1);
        a.endnotes.truncate(1);
        a.editorial.clear();
        a.xrefs.clear();
        for r in [0..a.len(), 2..a.len(), 0..3] {
            let slice = a.extract(r);
            slice.check().unwrap();
            let mut b = Story::with_text(StoryId(2), "z", ParaFormat::default());
            b.insert_story(1, &slice);
            b.check().unwrap();
        }
    }
}
