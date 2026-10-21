//! Locating and editing items, threading, and story management.

use std::sync::Arc;

use designcraft_geom::{Affine, Point, Rect, shapes};
use serde::{Deserialize, Serialize};

use crate::ids::{ItemId, LayerId, StoryId};
use crate::item::{Content, Item, Shape, TextFrame, TextFrameOptions};
use crate::page::Spread;
use crate::story::{CharRun, ParaFormat, Story};
use crate::{DocError, Document, Result};

/// Which spread list an item lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "index")]
pub enum SpreadRef {
    Doc(usize),
    Parent(usize),
}

/// Indices from `spread.items` down through nested groups.
pub type ItemPath = Vec<usize>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemLoc {
    pub spread: SpreadRef,
    pub path: ItemPath,
}

impl ItemLoc {
    /// Top-level index in the spread.
    pub fn top(&self) -> usize {
        self.path[0]
    }
}

fn find_in(items: &[Arc<Item>], id: ItemId, path: &mut ItemPath) -> bool {
    for (i, it) in items.iter().enumerate() {
        path.push(i);
        if it.id == id || find_in(it.children(), id, path) {
            return true;
        }
        path.pop();
    }
    false
}

impl Document {
    pub fn spread(&self, r: SpreadRef) -> Option<&Spread> {
        match r {
            SpreadRef::Doc(i) => self.spreads.get(i).map(|s| &**s),
            SpreadRef::Parent(i) => self.parents.get(i).map(|s| &**s),
        }
    }

    pub fn spread_mut(&mut self, r: SpreadRef) -> Option<&mut Spread> {
        match r {
            SpreadRef::Doc(i) => self.spreads.get_mut(i).map(Arc::make_mut),
            SpreadRef::Parent(i) => self.parents.get_mut(i).map(Arc::make_mut),
        }
    }

    /// Where the rulers of spread `r` start before the zero point moves them, for something at
    /// spread x `x` (a page origin is the page there, or the nearest page): spread coordinates.
    pub fn ruler_base(&self, r: SpreadRef, x: f64) -> Option<Point> {
        let sp = self.spread(r)?;
        let b = sp.bounds();
        Some(match self.settings.ruler_origin {
            crate::RulerOrigin::Spread => Point::new(b.x0, b.y0),
            crate::RulerOrigin::Spine => Point::new(sp.spine_x(), b.y0),
            crate::RulerOrigin::Page => {
                let pg = sp.pages.get(sp.page_at_x(x)?)?;
                Point::new(pg.x, b.y0)
            }
        })
    }

    /// Where the rulers and X/Y fields of spread `r` measure from, for something at spread x
    /// `x`: the ruler origin moved by the document's zero point.
    pub fn ruler_origin(&self, r: SpreadRef, x: f64) -> Option<Point> {
        let o = self.ruler_base(r, x)?;
        let [zx, zy] = self.zero_point();
        Some(Point::new(o.x + zx, o.y + zy))
    }

    /// The horizontal ruler of spread `r` in pieces `(from x, to x, base)`, spread coordinates,
    /// covering every x: one piece, or one per page for a page origin.
    pub fn ruler_pieces(&self, r: SpreadRef) -> Vec<(f64, f64, Point)> {
        let Some(sp) = self.spread(r) else { return Vec::new() };
        if self.settings.ruler_origin != crate::RulerOrigin::Page || sp.pages.len() < 2 {
            return self.ruler_base(r, 0.0).map(|o| (f64::NEG_INFINITY, f64::INFINITY, o)).into_iter().collect();
        }
        let mut pages: Vec<&crate::Page> = sp.pages.iter().collect();
        pages.sort_by(|a, b| a.x.total_cmp(&b.x));
        let y = sp.bounds().y0;
        pages
            .iter()
            .enumerate()
            .map(|(i, pg)| {
                let from = if i == 0 { f64::NEG_INFINITY } else { pg.x };
                let to = pages.get(i + 1).map_or(f64::INFINITY, |next| next.x);
                (from, to, Point::new(pg.x, y))
            })
            .collect()
    }

    /// The document's zero point, kept on the pasteboard (a file may hold any numbers).
    pub fn zero_point(&self) -> [f64; 2] {
        self.clamp_zero_point(self.settings.zero_point).unwrap_or([0.0, 0.0])
    }

    /// `p` (a zero point) kept on the pasteboard around the largest spread, the area the canvas
    /// shows; `None` when it isn't a finite point.
    pub fn clamp_zero_point(&self, [x, y]: [f64; 2]) -> Option<[f64; 2]> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let (w, h) = self
            .spreads
            .iter()
            .chain(&self.parents)
            .map(|sp| sp.bounds())
            .fold((0.0_f64, 0.0_f64), |(w, h), b| (w.max(b.width()), h.max(b.height())));
        let finite = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
        let (px, py) = (finite(self.settings.pasteboard.0), finite(self.settings.pasteboard.1));
        // From any ruler origin (a spine or a page can be at the spread's right edge).
        let side = px.max(w);
        Some([x.clamp(-(w + side), w + side), y.clamp(-py, h + py)])
    }

    /// All spread refs: document spreads then parents.
    pub fn spread_refs(&self) -> impl Iterator<Item = SpreadRef> {
        (0..self.spreads.len()).map(SpreadRef::Doc).chain((0..self.parents.len()).map(SpreadRef::Parent))
    }

    pub fn find(&self, id: ItemId) -> Option<ItemLoc> {
        for r in self.spread_refs() {
            let mut path = Vec::new();
            if find_in(&self.spread(r)?.items, id, &mut path) {
                return Some(ItemLoc { spread: r, path });
            }
        }
        None
    }

    pub fn item_at(&self, loc: &ItemLoc) -> Option<&Item> {
        let sp = self.spread(loc.spread)?;
        let mut it: &Item = sp.items.get(*loc.path.first()?)?;
        for &i in &loc.path[1..] {
            it = it.children().get(i)?;
        }
        Some(it)
    }

    pub fn item(&self, id: ItemId) -> Option<&Item> {
        self.item_at(&self.find(id)?)
    }

    /// Items embedded in stories (including footnotes and table cells), with their
    /// owning top-level story. Does not follow text-frame references between stories.
    pub fn anchored_items(&self) -> Vec<(StoryId, &Item)> {
        let mut out = Vec::new();
        let mut stories: Vec<_> = self.stories.iter().map(|(id, story)| (*id, story.as_ref())).collect();
        while let Some((owner, story)) = stories.pop() {
            stories.extend(story.notes.iter().chain(&story.endnotes).map(|n| (owner, &n.text)));
            stories.extend(story.tables.values().flat_map(|t| t.cells.iter().map(|c| (owner, &c.text))));
            let mut items: Vec<&Item> = story.objects.iter().map(|o| &o.item).collect();
            while let Some(item) = items.pop() {
                out.push((owner, item));
                items.extend(item.children().iter().map(AsRef::as_ref));
            }
        }
        out
    }

    /// An embedded item is not a spread selection location (`find`).
    pub fn anchored_item(&self, id: ItemId) -> Option<&Item> {
        self.anchored_items().into_iter().find_map(|(_, item)| (item.id == id).then_some(item))
    }

    pub fn item_mut_at(&mut self, loc: &ItemLoc) -> Option<&mut Item> {
        let sp = self.spread_mut(loc.spread)?;
        let mut it: &mut Item = Arc::make_mut(sp.items.get_mut(*loc.path.first()?)?);
        for &i in &loc.path[1..] {
            it = Arc::make_mut(it.children_mut()?.get_mut(i)?);
        }
        Some(it)
    }

    pub fn item_mut(&mut self, id: ItemId) -> Option<&mut Item> {
        let loc = self.find(id)?;
        self.item_mut_at(&loc)
    }

    /// The top-level item (group root) containing `id`.
    pub fn top_level_of(&self, id: ItemId) -> Option<ItemId> {
        let loc = self.find(id)?;
        Some(self.spread(loc.spread)?.items[loc.top()].id)
    }

    /// The immediate parent group of `id`, if `id` is nested inside a group.
    pub fn parent_of(&self, id: ItemId) -> Option<ItemId> {
        let loc = self.find(id)?;
        if loc.path.len() <= 1 {
            return None;
        }
        let sp = self.spread(loc.spread)?;
        let mut items = &sp.items;
        let mut parent_id = None;
        for &i in &loc.path[..loc.path.len() - 1] {
            let it = items.get(i)?;
            parent_id = Some(it.id);
            items = match &it.content {
                Content::Group { items } => items,
                _ => return None,
            };
        }
        parent_id
    }

    /// Item IDs along the ancestry path from top-level down to `id` (inclusive).
    pub fn ancestry_chain(&self, id: ItemId) -> Vec<ItemId> {
        let Some(loc) = self.find(id) else { return vec![] };
        let Some(sp) = self.spread(loc.spread) else { return vec![] };
        let mut items = &sp.items;
        let mut chain = Vec::with_capacity(loc.path.len());
        for &i in &loc.path {
            let Some(it) = items.get(i) else { break };
            chain.push(it.id);
            items = match &it.content {
                Content::Group { items } => items,
                _ => break,
            };
        }
        chain
    }

    /// Spread-space transform of the parent group chain of the item at `loc` (identity at top level).
    pub fn parent_xf(&self, loc: &ItemLoc) -> designcraft_geom::Affine {
        let mut xf = designcraft_geom::Affine::IDENTITY;
        if let Some(sp) = self.spread(loc.spread) {
            let mut items = &sp.items;
            for &i in &loc.path[..loc.path.len().saturating_sub(1)] {
                let it = &items[i];
                xf *= it.xf;
                items = match &it.content {
                    Content::Group { items } => items,
                    _ => break,
                };
            }
        }
        xf
    }

    /// Insert an item into a spread at `index` (None = front).
    pub fn insert_item(&mut self, r: SpreadRef, item: Item, index: Option<usize>) -> Result<ItemId> {
        let id = item.id;
        let sp = self.spread_mut(r).ok_or_else(|| DocError::Invalid(format!("no spread {r:?}")))?;
        let i = index.unwrap_or(sp.items.len()).min(sp.items.len());
        sp.items.insert(i, Arc::new(item));
        Ok(id)
    }

    /// Remove an item (and its descendants) from the document. Text frames leave their threads;
    /// stories that lose their last frame are deleted.
    pub fn remove_item(&mut self, id: ItemId) -> Result<Arc<Item>> {
        let loc = self.find(id).ok_or(DocError::NoItem(id))?;
        let removed = {
            let sp = self.spread_mut(loc.spread).ok_or(DocError::NoItem(id))?;
            if loc.path.len() == 1 {
                sp.items.remove(loc.path[0])
            } else {
                let mut it: &mut Item = Arc::make_mut(&mut sp.items[loc.path[0]]);
                for &i in &loc.path[1..loc.path.len() - 1] {
                    it = Arc::make_mut(&mut it.children_mut().ok_or(DocError::NoItem(id))?[i]);
                }
                let last = *loc.path.last().ok_or(DocError::NoItem(id))?;
                it.children_mut().ok_or(DocError::NoItem(id))?.remove(last)
            }
        };
        let mut frames = Vec::new();
        removed.walk(&mut |i| {
            if let Some(t) = i.text_frame() {
                frames.push((i.id, t.story));
            }
        });
        for (fid, sid) in frames {
            let empty = {
                let st = self.story_mut(sid).ok_or(DocError::NoStory(sid))?;
                st.frames.retain(|f| *f != fid);
                st.frames.is_empty()
            };
            if empty {
                self.stories.remove(&sid);
            }
        }
        Ok(removed)
    }

    /// The column direction a new story starts with: right to left in a document whose binding is
    /// right to left (Document Setup), else left to right. Stories already made keep theirs.
    pub fn new_story_direction(&self) -> crate::TextDirection {
        if self.settings.right_to_left_binding { crate::TextDirection::RightToLeft } else { crate::TextDirection::LeftToRight }
    }

    /// Create a text frame with a new (or given) story at `rect` on spread `r`.
    pub fn add_text_frame(&mut self, r: SpreadRef, rect: Rect, layer: LayerId, text: &str, para: ParaFormat) -> Result<(ItemId, StoryId)> {
        let id = ItemId(self.alloc());
        let sid = StoryId(self.alloc());
        let mut story = Story::with_text(sid, text, para);
        story.direction = self.new_story_direction();
        story.frames.push(id);
        self.stories.insert(sid, Arc::new(story));
        let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(rect));
        item.object_style = self.styles.default_text_frame.clone();
        // Applied before the content exists, so the frame keeps default text frame options.
        if let Some(os) = self.styles.object_style(&self.styles.default_text_frame) {
            os.apply_to(&mut item);
        }
        item.content = Content::Text(TextFrame { story: sid, options: TextFrameOptions::default() });
        self.insert_item(r, item, None)?;
        Ok((id, sid))
    }

    /// Thread `from` → `to`: `to` joins `from`'s story right after `from` (between `from` and the
    /// frame that followed it, if any). If `to` held its own text, that text is appended to the
    /// story (as new paragraphs) and its old story is removed; frames that followed `to` come
    /// along, frames before it keep an empty story of their own. An empty (unassigned) `from` or
    /// `to` becomes a text frame. `from`'s story keeps its id.
    pub fn thread(&mut self, from: ItemId, to: ItemId) -> Result<()> {
        if from == to {
            return Err(DocError::Invalid("cannot thread a frame to itself".into()));
        }
        self.check_thread_links(from, to, false)?;
        let ts = self.frame_story(to)?;
        let fs = match self.frame_story(from)? {
            Some(s) => s,
            None => self.give_empty_story(from)?,
        };
        if ts == Some(fs) {
            return Err(DocError::Invalid("frames are already in the same story".into()));
        }
        let Some(ts) = ts else {
            let st = self.story_mut(fs).ok_or(DocError::NoStory(fs))?;
            let at = st.frames.iter().position(|f| *f == from).map_or(st.frames.len(), |p| p + 1);
            st.frames.insert(at, to);
            st.rev = st.rev.wrapping_add(1);
            let it = self.item_mut(to).ok_or(DocError::NoItem(to))?;
            it.content = Content::Text(TextFrame { story: fs, options: TextFrameOptions::default() });
            return Ok(());
        };
        let mut other = Arc::unwrap_or_clone(self.stories.remove(&ts).ok_or(DocError::NoStory(ts))?);
        // Frames of the other story from `to` on follow along; frames before it are left as an empty new story.
        let pos_to = other.frames.iter().position(|f| *f == to).unwrap_or(0);
        let after = other.frames.split_off(pos_to);
        let before = std::mem::take(&mut other.frames);
        let (vertical, direction) = (other.vertical, other.direction);
        let offset;
        {
            let st = self.story_mut(fs).ok_or(DocError::NoStory(fs))?;
            offset = if st.text.is_empty() { 0 } else { st.text.len().saturating_add(1) };
            // The story keeps its own direction, even when it was empty and takes the other's text.
            let own = (st.vertical, st.direction);
            st.append_story(other);
            (st.vertical, st.direction) = own;
            let at = st.frames.iter().position(|f| *f == from).map_or(st.frames.len(), |p| p + 1);
            for (k, f) in after.iter().enumerate() {
                st.frames.insert(at + k, *f);
            }
            st.rev = st.rev.wrapping_add(1);
        }
        self.remap_thread_story_refs(ts, fs, offset);
        for f in &after {
            if let Some(t) = self.item_mut(*f).and_then(Item::text_frame_mut) {
                t.story = fs;
            }
        }
        if !before.is_empty() {
            let nid = StoryId(self.alloc());
            let mut ns = Story::new(nid);
            ns.frames = before.clone();
            ns.vertical = vertical;
            ns.direction = direction;
            for f in &before {
                if let Some(t) = self.item_mut(*f).and_then(Item::text_frame_mut) {
                    t.story = nid;
                }
            }
            self.stories.insert(nid, Arc::new(ns));
        }
        Ok(())
    }

    /// [`Document::thread`], except that an empty `from` alone in its story, threaded in front of
    /// the first frame of `to`'s story, joins that story: the story with the text keeps its id
    /// (and whatever refers to it).
    pub fn thread_keeping_story(&mut self, from: ItemId, to: ItemId) -> Result<()> {
        let fs = self.frame_story(from)?;
        let from_empty = fs.is_none_or(|fs| self.story(fs).is_some_and(|s| s.text.is_empty() && s.frames == [from]));
        let ts = self.frame_story(to)?;
        let to_first = ts.and_then(|ts| self.story(ts)).is_some_and(|s| s.frames.first() == Some(&to));
        let (Some(ts), true, true) = (ts, from_empty, to_first && from != to) else { return self.thread(from, to) };
        self.check_thread_links(from, to, true)?;
        if let Some(fs) = fs {
            self.stories.remove(&fs);
            self.remap_thread_story_refs(fs, ts, 0);
        }
        let st = self.story_mut(ts).ok_or(DocError::NoStory(ts))?;
        st.frames.insert(0, from);
        st.rev = st.rev.wrapping_add(1);
        let it = self.item_mut(from).ok_or(DocError::NoItem(from))?;
        match it.text_frame_mut() {
            Some(t) => t.story = ts,
            None => it.content = Content::Text(TextFrame { story: ts, options: TextFrameOptions::default() }),
        }
        Ok(())
    }

    /// Story-external text addresses move with the incoming text when threading merges stories.
    /// Linked children follow the surviving whole story, including for an empty removed parent;
    /// their copied content stays unchanged until an explicit Update Link.
    fn remap_thread_story_refs(&mut self, from: StoryId, to: StoryId, offset: usize) {
        // The child's old snapshot is not a snapshot of the newly merged parent. Force it
        // out of date even when the old and surviving revision numbers happen to coincide.
        let stale_rev = self.story(to).map_or(0, |st| st.rev.wrapping_sub(1));
        for st in self.stories.values_mut() {
            if st.link.is_some_and(|(parent, _)| parent == from) {
                Arc::make_mut(st).link = Some((to, stale_rev));
            }
        }
        for link in &mut self.hyperlinks {
            if let crate::HyperlinkSource::Text { story, start, end } = &mut link.source
                && *story == from
            {
                *story = to;
                *start = start.saturating_add(offset);
                *end = end.saturating_add(offset);
            }
        }
        for placeholder in &mut self.data_merge.placeholders {
            if let crate::PlaceholderAnchor::Text { story, start, end } = &mut placeholder.anchor
                && *story == from
            {
                *story = to;
                *start = start.saturating_add(offset);
                *end = end.saturating_add(offset);
            }
        }
        if self.settings.primary_story == Some(from) {
            self.settings.primary_story = Some(to);
        }
    }

    /// A linked child is replaced wholesale on update. Only empty, unlinked frame
    /// extensions can join it without losing a link or newly appended body text.
    fn check_thread_links(&self, from: ItemId, to: ItemId, keep_to: bool) -> Result<()> {
        let fs = self.frame_story(from)?;
        let ts = self.frame_story(to)?;
        let first = fs.and_then(|id| self.story(id));
        let second = ts.and_then(|id| self.story(id));
        let incompatible = if keep_to {
            first.is_some_and(|st| st.link.is_some()) || second.is_some_and(|st| st.link.is_some_and(|(parent, _)| Some(parent) == fs))
        } else {
            second.is_some_and(|st| st.link.is_some())
                || first.is_some_and(|st| {
                    st.link.is_some()
                        && second.is_some_and(|incoming| !incoming.text.is_empty() || st.link.is_some_and(|(parent, _)| parent == incoming.id))
                })
        };
        // Removing any ancestor of the survivor would redirect a link back into its own chain.
        let (removed, survivor) = if keep_to { (fs, second) } else { (ts, first) };
        let mut parent = survivor.and_then(|st| st.link);
        let mut absorbs_ancestor = false;
        // Bound the walk even when an existing document already contains a link cycle.
        for _ in 0..self.stories.len() {
            let Some((id, _)) = parent else { break };
            if Some(id) == removed {
                absorbs_ancestor = true;
                break;
            }
            parent = self.story(id).and_then(|st| st.link);
        }
        if incompatible || absorbs_ancestor {
            return Err(DocError::Invalid("unlink the linked story before merging its text".into()));
        }
        Ok(())
    }

    /// Can a user thread `from` → `to`? Both are text frames or empty frames (not lines), on
    /// document spreads or on the same parent spread, not in one story already, and no frame
    /// threads into `to` yet. [`Document::thread`] itself is less strict (import and autoflow).
    pub fn check_thread(&self, from: ItemId, to: ItemId) -> Result<()> {
        if from == to {
            return Err(DocError::Invalid("cannot thread a frame to itself".into()));
        }
        for id in [from, to] {
            let it = self.item(id).ok_or(DocError::NoItem(id))?;
            if !matches!(it.content, Content::Text(_) | Content::Unassigned) || it.shape == Shape::GraphicLine {
                return Err(DocError::Invalid(format!("{id} is not a text frame or an empty frame")));
            }
            if it.locked || self.layer(it.layer).is_some_and(|l| l.locked) {
                return Err(DocError::Invalid(format!("{id} is locked or on a locked layer")));
            }
        }
        let (Some(fl), Some(tl)) = (self.find(from), self.find(to)) else { return Err(DocError::NoItem(to)) };
        let same_place = match (fl.spread, tl.spread) {
            (SpreadRef::Doc(_), SpreadRef::Doc(_)) => true,
            (a, b) => a == b,
        };
        if !same_place {
            return Err(DocError::Invalid("frames on a parent spread thread only to frames on the same parent".into()));
        }
        let story = |id: ItemId| self.item(id).and_then(|i| i.text_frame()).map(|t| t.story);
        if story(from).is_some() && story(from) == story(to) {
            return Err(DocError::Invalid("the frames are already in the same thread".into()));
        }
        // Generated text is rebuilt from its source data. Merging body text into it would
        // silently discard that text on the next update (endnotes update after every edit).
        let generated = |sid| {
            self.endnote_story == Some(sid)
                || self.toc.as_ref().is_some_and(|t| t.story == sid)
                || self.index.as_ref().is_some_and(|i| i.story == sid)
        };
        let (fs, ts) = (story(from), story(to));
        if fs.is_some_and(generated) && ts.and_then(|sid| self.story(sid)).is_some_and(|st| !st.text.is_empty()) {
            return Err(DocError::Invalid("cannot merge body text into a generated story".into()));
        }
        if ts.is_some_and(generated) && fs.and_then(|sid| self.story(sid)).is_some_and(|st| !st.text.is_empty() || st.frames != [from]) {
            return Err(DocError::Invalid("a generated story can only take a new empty first frame".into()));
        }
        let keep_to = fs.is_none_or(|sid| self.story(sid).is_some_and(|st| st.text.is_empty() && st.frames == [from]))
            && ts.and_then(|sid| self.story(sid)).is_some_and(|st| st.frames.first() == Some(&to));
        self.check_thread_links(from, to, keep_to)?;
        if self.prev_frame(to).is_some() {
            return Err(DocError::Invalid(format!("text already flows into {to} from another frame: break that thread first")));
        }
        Ok(())
    }

    /// The story of a frame that can take part in a thread: `Some` for a text frame, `None` for
    /// an empty (unassigned) frame.
    fn frame_story(&self, id: ItemId) -> Result<Option<StoryId>> {
        match &self.item(id).ok_or(DocError::NoItem(id))?.content {
            Content::Text(t) => Ok(Some(t.story)),
            Content::Unassigned => Ok(None),
            _ => Err(DocError::Invalid("can only thread text frames or empty frames".into())),
        }
    }

    /// Make an empty frame a text frame with a new empty story.
    fn give_empty_story(&mut self, id: ItemId) -> Result<StoryId> {
        let sid = StoryId(self.alloc());
        let mut st = Story::new(sid);
        st.frames.push(id);
        let it = self.item_mut(id).ok_or(DocError::NoItem(id))?;
        it.content = Content::Text(TextFrame { story: sid, options: TextFrameOptions::default() });
        self.stories.insert(sid, Arc::new(st));
        Ok(sid)
    }

    /// Break the thread after `frame`: the following frames become a new empty story (the text
    /// stays with the first part and may become overset).
    pub fn unthread_after(&mut self, frame: ItemId) -> Result<()> {
        let sid = self.item(frame).and_then(|i| i.text_frame()).map(|t| t.story).ok_or(DocError::NoItem(frame))?;
        let (tail, vertical, direction) = {
            let st = self.story_mut(sid).ok_or(DocError::NoStory(sid))?;
            let p = st.frames.iter().position(|f| *f == frame).ok_or(DocError::NoItem(frame))?;
            let tail = st.frames.split_off(p + 1);
            st.rev = st.rev.wrapping_add(1);
            (tail, st.vertical, st.direction)
        };
        if tail.is_empty() {
            return Ok(());
        }
        let nid = StoryId(self.alloc());
        let mut ns = Story::new(nid);
        ns.frames = tail.clone();
        ns.vertical = vertical;
        ns.direction = direction;
        self.stories.insert(nid, Arc::new(ns));
        for f in tail {
            if let Some(t) = self.item_mut(f).and_then(Item::text_frame_mut) {
                t.story = nid;
            }
        }
        Ok(())
    }

    /// Take `frame` out of its thread: the frames before and after it join up and keep the text,
    /// `frame` stays on the page with a new empty story.
    pub fn remove_from_thread(&mut self, frame: ItemId) -> Result<()> {
        let sid = self.item(frame).and_then(|i| i.text_frame()).map(|t| t.story).ok_or(DocError::NoItem(frame))?;
        let (vertical, direction) = {
            let st = self.story_mut(sid).ok_or(DocError::NoStory(sid))?;
            if st.frames.len() < 2 {
                return Err(DocError::Invalid(format!("frame {frame} is not threaded to another frame")));
            }
            st.frames.retain(|f| *f != frame);
            st.rev = st.rev.wrapping_add(1);
            (st.vertical, st.direction)
        };
        let nid = StoryId(self.alloc());
        let mut ns = Story::new(nid);
        ns.frames = vec![frame];
        ns.vertical = vertical;
        ns.direction = direction;
        self.stories.insert(nid, Arc::new(ns));
        let t = self.item_mut(frame).and_then(Item::text_frame_mut).ok_or(DocError::NoItem(frame))?;
        t.story = nid;
        Ok(())
    }

    /// Next / previous frame in a thread.
    pub fn next_frame(&self, frame: ItemId) -> Option<ItemId> {
        let st = self.story(self.item(frame)?.text_frame()?.story)?;
        let p = st.frames.iter().position(|f| *f == frame)?;
        st.frames.get(p + 1).copied()
    }
    pub fn prev_frame(&self, frame: ItemId) -> Option<ItemId> {
        let st = self.story(self.item(frame)?.text_frame()?.story)?;
        let p = st.frames.iter().position(|f| *f == frame)?;
        p.checked_sub(1).map(|i| st.frames[i])
    }

    /// Does this text frame set its text vertically (its story is vertical; type on a path stays
    /// along the path)?
    pub fn frame_vertical(&self, item: &Item) -> bool {
        item.text_frame().is_some_and(|t| t.options.path.is_none() && self.story(t.story).is_some_and(|s| s.vertical))
    }

    /// Text space → item inner space for a frame's composed text (identity except for vertical
    /// frames).
    pub fn text_local(&self, item: &Item) -> Affine {
        if self.frame_vertical(item) { crate::item::vertical_text_xf(item.text_area()) } else { Affine::IDENTITY }
    }

    /// Text space → spread-parent space for a frame's composed text (the item transform, with the
    /// quarter turn of a vertical frame).
    pub fn text_xf(&self, item: &Item) -> Affine {
        item.xf * self.text_local(item)
    }

    /// Hit test: the frontmost visible, unlocked-layer item on spread `si` containing `p` (spread space).
    /// Frames hit by area if they have a fill or are text/graphic frames; otherwise near the outline.
    pub fn hit_item(&self, si: usize, p: Point, tol: f64) -> Option<ItemId> {
        let sp = self.spreads.get(si)?;
        let mut items: Vec<&Arc<Item>> = sp.items.iter().collect();
        items.sort_by_key(|i| self.layer_rank(i.layer));
        for it in items.into_iter().rev() {
            let layer = self.layer(it.layer);
            if it.hidden || layer.is_some_and(|l| !l.visible || l.locked) {
                continue;
            }
            if item_hit(it, p, tol) {
                return Some(it.id);
            }
        }
        None
    }

    /// The first item on a document or parent spread whose geometry isn't finite
    /// ([`Item::geometry_is_finite`]).
    pub fn non_finite_item(&self) -> Option<ItemId> {
        let mut found = None;
        for sp in self.spreads.iter().chain(&self.parents) {
            for it in &sp.items {
                it.walk(&mut |i| {
                    if found.is_none() && !i.geometry_is_finite() {
                        found = Some(i.id);
                    }
                });
            }
        }
        found
    }

    /// True if `id` itself is locked, sits on a locked layer, or has any locked ancestor group.
    pub fn is_locked(&self, id: ItemId) -> bool {
        let Some(loc) = self.find(id) else { return false };
        let Some(it) = self.item(id) else { return false };
        if it.locked || self.layer(it.layer).is_some_and(|l| l.locked) {
            return true;
        }
        if let Some(sp) = self.spread(loc.spread) {
            let mut items = &sp.items;
            for &i in &loc.path[..loc.path.len().saturating_sub(1)] {
                let Some(group) = items.get(i) else { break };
                if group.locked {
                    return true;
                }
                items = match &group.content {
                    Content::Group { items } => items,
                    _ => break,
                };
            }
        }
        false
    }

    /// Every item id (pre-order) on document spreads.
    pub fn all_items(&self) -> Vec<ItemId> {
        let mut v = Vec::new();
        for sp in &self.spreads {
            for it in &sp.items {
                it.walk(&mut |i| v.push(i.id));
            }
        }
        v
    }

    /// Calls `f` on every item `hit` selects: on document and parent spreads, inside groups and
    /// among anchored objects. Only the branches holding a hit are copied (copy-on-write). Returns
    /// how many items `f` saw.
    pub fn update_items(&mut self, hit: impl Fn(&Item) -> bool, mut f: impl FnMut(&mut Item)) -> usize {
        const MAX_GROUP_DEPTH: usize = 64;
        fn any(it: &Item, hit: &impl Fn(&Item) -> bool) -> bool {
            let mut found = false;
            it.walk(&mut |i| found |= hit(i));
            found
        }
        fn update(it: &mut Item, hit: &impl Fn(&Item) -> bool, f: &mut impl FnMut(&mut Item), n: &mut usize, depth: usize) {
            if hit(it) {
                f(it);
                *n += 1;
            }
            if depth >= MAX_GROUP_DEPTH {
                return;
            }
            if let Some(children) = it.children_mut() {
                for c in children.iter_mut().filter(|c| any(c, hit)) {
                    update(Arc::make_mut(c), hit, f, n, depth + 1);
                }
            }
        }
        let mut n = 0;
        for sp in self.spreads.iter_mut().chain(self.parents.iter_mut()) {
            if !sp.items.iter().any(|i| any(i, &hit)) {
                continue;
            }
            for it in Arc::make_mut(sp).items.iter_mut().filter(|i| any(i, &hit)) {
                update(Arc::make_mut(it), &hit, &mut f, &mut n, 0);
            }
        }
        for st in self.stories.values_mut() {
            if !st.objects.iter().any(|o| any(&o.item, &hit)) {
                continue;
            }
            for o in Arc::make_mut(st).objects.iter_mut().filter(|o| any(&o.item, &hit)) {
                update(&mut Arc::make_mut(o).item, &hit, &mut f, &mut n, 0);
            }
        }
        n
    }

    /// Plain story text with frame/story stats (for agents).
    pub fn story_summary(&self, sid: StoryId) -> Option<serde_json::Value> {
        let st = self.story(sid)?;
        Some(serde_json::json!({
            "id": sid.0,
            "length": st.text.len(),
            "direction": st.direction,
            "paragraphs": st.paras.len(),
            "frames": st.frames.iter().map(|f| f.0).collect::<Vec<_>>(),
            "vertical": st.vertical,
            "text": st.text,
        }))
    }

    /// Replace a story's text and formatting wholesale (agents / import).
    pub fn set_story_text(&mut self, sid: StoryId, text: &str) -> Result<()> {
        let st = self.story_mut(sid).ok_or(DocError::NoStory(sid))?;
        let para = st.paras.first().cloned().map(|p| ParaFormat { table: None, ..p }).unwrap_or_default();
        let fmt = st.chars.first().map(|r| r.format.clone()).unwrap_or_default();
        let n = text.matches('\n').count() + 1;
        st.text = text.to_string();
        st.paras = vec![para; n];
        st.chars = vec![CharRun { len: text.len(), format: fmt }];
        st.tables.clear();
        st.notes.clear();
        st.fix_notes();
        st.endnotes.clear();
        st.fix_endnotes();
        st.anchors.clear();
        st.xrefs.clear();
        st.index_refs.clear();
        st.objects.clear();
        st.editorial.clear();
        st.fix_marks();
        st.rev = st.rev.wrapping_add(1);
        Ok(())
    }
}

/// Does `p` (in the item's parent space) hit `it`?
pub fn item_hit(it: &Item, p: Point, tol: f64) -> bool {
    if let Content::Group { items } = &it.content {
        let inner = it.xf.inverse() * p;
        return items.iter().any(|c| item_hit(c, inner, tol));
    }
    let inner = it.xf.inverse() * p;
    let bp = it.path.to_bezpath();
    let area = !it.fill.is_none() || matches!(it.content, Content::Text(_) | Content::Graphic(_)) || it.path.is_closed() && it.shape != Shape::Path;
    if area && it.path.is_closed() && designcraft_geom::hit::fill_contains(&bp, designcraft_geom::FillRule::NonZero, inner) {
        return true;
    }
    // Type on a path: the letters stand over the path, so a click on them hits it too.
    let reach = if it.text_frame().is_some_and(|t| t.options.path.is_some()) { PATH_TYPE_REACH } else { 0.0 };
    designcraft_geom::hit::stroke_contains(&bp, it.stroke.weight.max(1.0), tol + reach, inner)
}

/// The frontmost descendant of `it` under `p` that `want` accepts, looking inside nested groups;
/// `it` itself when nothing inside it matches and `want` accepts it. `p` is in the space `it`
/// sits in, as [`item_hit`] takes it.
///
/// Children are tried front to back and a hidden one is skipped, so of several that `want`
/// accepts the answer is the one in front. Objects `want` rejects do not hide the ones behind
/// them: a text frame under a rectangle in the same group is still found.
pub fn hit_descendant(it: &Item, p: Point, tol: f64, want: &dyn Fn(&Item) -> bool) -> Option<ItemId> {
    if !item_hit(it, p, tol) {
        return None;
    }
    let inner = it.xf.inverse() * p;
    let children: Vec<&Arc<Item>> = it.shown_children().collect();
    for c in children.into_iter().rev() {
        if c.hidden {
            continue;
        }
        if let Some(id) = hit_descendant(c, inner, tol, want) {
            return Some(id);
        }
    }
    want(it).then_some(it.id)
}

/// How far from its path type on a path is hit (points): about a cap height of 12–18 pt type,
/// either side (the type may sit on or under the path).
const PATH_TYPE_REACH: f64 = 12.0;
