//! Numbering that runs through the document in page order: named lists that continue across
//! stories, footnotes (when they never restart) and endnotes.
//!
//! Stories are ordered by the page of their first frame, then by id. [`NumberStarts`] holds where
//! every story's numbering starts, built in one pass over the items and stories, so composing a
//! long document doesn't search the item tree once per story and per list.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::attrs::ListCounter;
use crate::ids::{ItemId, StoryId};
use crate::item::Item;
use crate::notes::NoteRestart;
use crate::story::Story;

/// Where document-wide numbering starts in each story ([`crate::Document::number_starts`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumberStarts {
    /// Position of each story in numbering order (only when some list continues across stories).
    order: HashMap<StoryId, usize>,
    /// Per list that continues across stories: its numbering after each story that numbers a
    /// paragraph of it, by position. Stories that don't use a list add nothing, so the index
    /// grows with the numbered paragraphs, not with lists times stories. Sorted by name, so
    /// [`NumberStarts::lists_in`] lists them in the same order every time it is built.
    lists: BTreeMap<String, Vec<(usize, ListCounter)>>,
    /// First footnote number per story (only when footnote numbering never restarts).
    footnotes: HashMap<StoryId, u32>,
    footnote_default: u32,
    /// First endnote number of each story that has endnotes.
    endnotes: HashMap<StoryId, u32>,
    /// The number after the last endnote (what stories without endnotes get).
    endnote_end: u32,
}

impl NumberStarts {
    /// The numbering of list `name` after the stories before `sid`; a fresh count for lists that
    /// don't continue across stories.
    pub fn list(&self, sid: StoryId, name: &str) -> ListCounter {
        let (Some(pos), Some(marks)) = (self.order.get(&sid), self.lists.get(name)) else { return ListCounter::default() };
        // The last story before `sid` that numbered a paragraph of the list.
        let i = marks.partition_point(|(p, _)| p < pos);
        i.checked_sub(1).and_then(|i| marks.get(i)).map(|(_, n)| n.clone()).unwrap_or_default()
    }

    /// The lists that continue across stories and that story `sid` numbers a paragraph of, by
    /// name, with where each starts in `sid`. Lists `sid` doesn't number can't change its layout.
    pub fn lists_in(&self, sid: StoryId) -> impl Iterator<Item = (&str, ListCounter)> + '_ {
        let pos = self.order.get(&sid).copied();
        self.lists.iter().filter_map(move |(name, marks)| {
            let pos = pos?;
            let i = marks.binary_search_by_key(&pos, |(p, _)| *p).ok()?;
            // The numbering after the story before it that numbered the list.
            let start = i.checked_sub(1).and_then(|i| marks.get(i)).map(|(_, n)| n.clone()).unwrap_or_default();
            Some((name.as_str(), start))
        })
    }

    /// Number of the first footnote of story `sid`.
    pub fn footnote(&self, sid: StoryId) -> u32 {
        self.footnotes.get(&sid).copied().unwrap_or(self.footnote_default)
    }

    /// Number of the first endnote of story `sid`.
    pub fn endnote(&self, sid: StoryId) -> u32 {
        self.endnotes.get(&sid).copied().unwrap_or(self.endnote_end)
    }
}

impl crate::Document {
    /// Absolute page of each of `ids` on the document spreads (the page under the item's centre),
    /// in one walk of the item tree. Items on parent spreads or not found are left out.
    pub fn pages_of_items(&self, ids: &std::collections::HashSet<ItemId>) -> HashMap<ItemId, usize> {
        let mut out = HashMap::with_capacity(ids.len());
        if ids.is_empty() {
            return out;
        }
        let mut first_page = 0usize;
        for sp in &self.spreads {
            // Pre-order, like `find`: the first item with an id wins.
            let mut stack: Vec<&Arc<Item>> = sp.items.iter().rev().collect();
            while let Some(it) = stack.pop() {
                if ids.contains(&it.id) && !out.contains_key(&it.id) {
                    let c = it.bounds().center();
                    out.insert(it.id, first_page.saturating_add(sp.page_at_x(c.x).unwrap_or(0)));
                }
                stack.extend(it.children().iter().rev());
            }
            first_page = first_page.saturating_add(sp.pages.len());
        }
        out
    }

    /// Stories in numbering order: page of their first frame (stories without one last), then id.
    pub fn stories_in_page_order(&self) -> Vec<&Story> {
        let firsts = self.stories.values().filter_map(|st| st.frames.first().copied()).collect();
        let pages = self.pages_of_items(&firsts);
        let mut v: Vec<(usize, &Story)> =
            self.stories.values().map(|st| (st.frames.first().and_then(|f| pages.get(f)).copied().unwrap_or(usize::MAX), st.as_ref())).collect();
        v.sort_by_key(|(page, st)| (*page, st.id.0));
        v.into_iter().map(|(_, st)| st).collect()
    }

    /// Where list, footnote and endnote numbering starts in every story.
    pub fn number_starts(&self) -> NumberStarts {
        let mut names: Vec<&str> = self.settings.lists.iter().filter(|l| l.continue_across_stories).map(|l| l.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        // Footnotes need the page order only when they never restart and some story has one.
        let footnotes_run = self.footnote_options.restart == NoteRestart::Never && self.stories.values().any(|st| !st.notes.is_empty());
        let any_endnotes = self.stories.values().any(|st| !st.endnotes.is_empty());
        let mut out =
            NumberStarts { footnote_default: self.footnote_options.start_at, endnote_end: self.endnote_options.start_at, ..Default::default() };
        if names.is_empty() && !footnotes_run && !any_endnotes {
            return out;
        }
        // One counter per name, at the name's index in the sorted `names`.
        let mut counters: Vec<ListCounter> = vec![ListCounter::default(); names.len()];
        let mut footnote = self.footnote_options.start_at;
        for (pos, st) in self.stories_in_page_order().into_iter().enumerate() {
            if footnotes_run {
                out.footnotes.insert(st.id, footnote);
                footnote = footnote.saturating_add(u32::try_from(st.notes.len()).unwrap_or(u32::MAX));
            }
            if !st.endnotes.is_empty() {
                out.endnotes.insert(st.id, out.endnote_end);
                out.endnote_end = out.endnote_end.saturating_add(u32::try_from(st.endnotes.len()).unwrap_or(u32::MAX));
            }
            if counters.is_empty() {
                continue;
            }
            out.order.insert(st.id, pos);
            let mut touched: BTreeSet<usize> = BTreeSet::new();
            for (p, r) in st.paras.iter().zip(st.para_ranges()) {
                if r.is_empty() {
                    continue;
                }
                let (pp, _) = self.styles.resolve_para(p);
                if pp.list_type != crate::ListType::Numbers {
                    continue;
                }
                let Ok(i) = names.binary_search(&pp.list_name.as_str()) else { continue };
                if let Some(n) = counters.get_mut(i) {
                    n.advance(&pp);
                    touched.insert(i);
                }
            }
            for i in touched {
                if let (Some(name), Some(n)) = (names.get(i), counters.get(i)) {
                    out.lists.entry((*name).to_string()).or_default().push((pos, n.clone()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::NumberStarts;
    use crate::build::NewDocument;
    use crate::geom::Rect;
    use crate::{Document, ListCounter, ListType, NumberedList, ParaAttrs, ParaFormat, ParaProps, SpreadRef, StoryId};

    /// A story with paragraphs `text` in list `list`, in a frame on page `page`.
    fn story(d: &mut Document, page: usize, text: &str, list: &str) -> StoryId {
        let lid = d.default_layer();
        let para = ParaAttrs { list_type: Some(ListType::Numbers), list_name: Some(list.into()), ..Default::default() };
        let r = Rect::new(36.0, 36.0, 300.0, 300.0);
        d.add_text_frame(SpreadRef::Doc(page), r, lid, text, ParaFormat { para, ..Default::default() }).unwrap().1
    }

    #[test]
    fn numbering_runs_in_page_order() {
        let mut d = Document::new(&NewDocument { pages: 3, facing_pages: false, ..Default::default() });
        d.settings.lists.push(NumberedList { name: "Steps".into(), continue_across_stories: true });
        d.settings.lists.push(NumberedList { name: "Solo".into(), continue_across_stories: false });
        // Made in a different order than the pages they sit on.
        let a = story(&mut d, 2, "a\nb", "Steps");
        let b = story(&mut d, 0, "x\ny\nz", "Steps");
        let c = story(&mut d, 1, "p\n\nq", "Steps");
        // An empty paragraph uses no number; Start At restarts the count.
        d.story_mut(c).unwrap().paras[2].para.start_at = Some(Some(10));
        let order: Vec<StoryId> = d.stories_in_page_order().iter().map(|s| s.id).collect();
        assert_eq!(order, [b, c, a]);
        for st in [b, a] {
            d.story_mut(st).unwrap().insert_note(0, "note", ParaFormat::default());
        }
        d.story_mut(b).unwrap().insert_note(0, "note", ParaFormat::default());
        d.story_mut(c).unwrap().insert_endnote(0, "end", ParaFormat::default());
        for _ in 0..2 {
            d.story_mut(a).unwrap().insert_endnote(0, "end", ParaFormat::default());
        }
        let n = d.number_starts();
        // The number the next paragraph of each list gets.
        let next = |s: StoryId, list: &str| {
            let pp = ParaProps { list_type: ListType::Numbers, list_name: list.into(), ..ParaProps::default() };
            n.list(s, list).advance(&pp)
        };
        assert_eq!([b, c, a].map(|s| next(s, "Steps")), [1, 4, 11]);
        assert_eq!([b, c, a].map(|s| next(s, "Solo")), [1, 1, 1]);
        let f = d.footnote_options.start_at;
        assert_eq!([b, c, a].map(|s| n.footnote(s)), [f, f + 2, f + 2]);
        let e = d.endnote_options.start_at;
        assert_eq!([c, a, b].map(|s| n.endnote(s)), [e, e + 1, e + 3], "a story without endnotes gets the end");
        // The per-story methods agree.
        for s in [a, b, c] {
            assert_eq!(d.list_start(s, "Steps"), n.list(s, "Steps"));
            assert_eq!(d.footnote_start(s), n.footnote(s));
            assert_eq!(d.endnote_start(s), n.endnote(s));
        }
        assert_eq!(n.list(StoryId(999), "Steps"), ListCounter::default(), "unknown stories start fresh");
    }

    #[test]
    fn nothing_running_skips_the_page_order() {
        let mut d = Document::new(&NewDocument { pages: 2, facing_pages: false, ..Default::default() });
        // Named lists that don't continue, footnotes that never restart, but no notes anywhere.
        d.settings.lists.push(NumberedList { name: "Solo".into(), continue_across_stories: false });
        let a = story(&mut d, 0, "x\ny", "Solo");
        story(&mut d, 1, "p", "Solo");
        let n = d.number_starts();
        let empty = NumberStarts { footnote_default: d.footnote_options.start_at, endnote_end: d.endnote_options.start_at, ..Default::default() };
        assert_eq!(n, empty, "no story walk when nothing numbers across stories");
        assert_eq!(n.footnote(a), d.footnote_options.start_at);
        assert_eq!(d.list_start(a, "Solo"), ListCounter::default());
    }

    #[test]
    fn list_index_grows_with_numbered_stories_only() {
        let mut d = Document::new(&NewDocument { pages: 4, facing_pages: false, ..Default::default() });
        for k in 0..50 {
            d.settings.lists.push(NumberedList { name: format!("Unused{k}"), continue_across_stories: true });
        }
        d.settings.lists.push(NumberedList { name: "Steps".into(), continue_across_stories: true });
        let a = story(&mut d, 0, "a\nb", "Steps");
        let b = story(&mut d, 1, "x", "Other");
        let c = story(&mut d, 2, "y", "Other");
        let e = story(&mut d, 3, "z", "Steps");
        let n = d.number_starts();
        // One mark per story that numbered a paragraph of a continuing list; none for unused lists.
        assert_eq!(n.lists.values().map(Vec::len).sum::<usize>(), 2);
        let next = |s: StoryId| {
            let pp = ParaProps { list_type: ListType::Numbers, list_name: "Steps".into(), ..ParaProps::default() };
            n.list(s, "Steps").advance(&pp)
        };
        assert_eq!([a, b, c, e].map(next), [1, 3, 3, 3]);
        assert_eq!(n.list(e, "Unused7"), ListCounter::default());
        // The lists each story numbers, with where they start in it.
        assert_eq!(n.lists_in(b).count(), 0, "a list that doesn't continue isn't indexed");
        assert_eq!(n.lists_in(a).collect::<Vec<_>>(), [("Steps", ListCounter::default())]);
        assert_eq!(n.lists_in(e).collect::<Vec<_>>(), [("Steps", n.list(e, "Steps"))]);
        assert_ne!(n.list(e, "Steps"), ListCounter::default());
    }
}
