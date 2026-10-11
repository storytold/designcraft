//! Numbering that runs through the document in page order: named lists that continue across
//! stories, footnotes (when they never restart) and endnotes.
//!
//! Stories are ordered by the page of their first frame, then by id. [`NumberStarts`] holds where
//! every story's numbering starts, built in one pass over the items and stories, so composing a
//! long document doesn't search the item tree once per story and per list.

use std::collections::HashMap;
use std::sync::Arc;

use crate::attrs::ListCounter;
use crate::ids::{ItemId, StoryId};
use crate::item::Item;
use crate::notes::NoteRestart;
use crate::story::Story;

/// Where document-wide numbering starts in each story ([`crate::Document::number_starts`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NumberStarts {
    /// Per list that continues across stories: its numbering after the stories before each story.
    lists: HashMap<String, HashMap<StoryId, ListCounter>>,
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
        self.lists.get(name).and_then(|m| m.get(&sid)).cloned().unwrap_or_default()
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
        let footnotes_run = self.footnote_options.restart == NoteRestart::Never;
        let any_endnotes = self.stories.values().any(|st| !st.endnotes.is_empty());
        let mut out =
            NumberStarts { footnote_default: self.footnote_options.start_at, endnote_end: self.endnote_options.start_at, ..Default::default() };
        if names.is_empty() && !footnotes_run && !any_endnotes {
            return out;
        }
        let mut counters: HashMap<&str, ListCounter> = names.iter().map(|n| (*n, ListCounter::default())).collect();
        let mut footnote = self.footnote_options.start_at;
        for st in self.stories_in_page_order() {
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
            for (name, n) in &counters {
                out.lists.entry((*name).to_string()).or_default().insert(st.id, n.clone());
            }
            for (p, r) in st.paras.iter().zip(st.para_ranges()) {
                if r.is_empty() {
                    continue;
                }
                let (pp, _) = self.styles.resolve_para(p);
                if pp.list_type != crate::ListType::Numbers {
                    continue;
                }
                if let Some(n) = counters.get_mut(pp.list_name.as_str()) {
                    n.advance(&pp);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
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
}
