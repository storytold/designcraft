//! Anchored objects (Object › Anchored Object): page items that flow with the text.
//!
//! An anchored object is an [`OBJECT_MARK`] character in a story; [`Story::objects`] holds one
//! [`AnchoredObject`] per mark in text order (like footnotes). The item's geometry is kept with
//! its top-left at the origin; composition places it on the line (inline) or on a line of its
//! own above the line it is anchored in (above line).
//!
//! An anchored text frame (also one in an anchored group, a footnote or a table cell) is in its
//! story's thread like a frame on a page: [`Document::fix_anchored_text_frames`] keeps it so
//! after edits that copy or delete text and frames, and [`Document::adopt_anchored`] gives
//! copies brought into a document stories of their own.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Document;
use crate::ids::{AssetId, ItemId, StoryId};
use crate::item::Item;
use crate::story::Story;

/// How deep anchored text frames (and footnotes and cells) are followed into each other: the
/// nesting [`Document::check`] accepts.
const NESTING: usize = 64;

/// An anchored object's position in the text.
pub const OBJECT_MARK: char = '\u{E00E}';

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum AnchorPosition {
    /// Sits on the baseline like a character, raised by `y_offset`.
    Inline {
        #[serde(default)]
        y_offset: f64,
    },
    /// On its own line above the anchor's line.
    AboveLine {
        #[serde(default)]
        align: AnchorAlign,
        #[serde(default)]
        space_before: f64,
        #[serde(default)]
        space_after: f64,
    },
    /// Custom: placed relative to the frame, column, page margins or page edge (or the anchor
    /// marker / line), taking no space in the text. `object_point` is the point on the object and
    /// `ref_point` the point on the reference area that meet (0–8, row-major), plus the offsets.
    Custom {
        #[serde(default)]
        x_relative: AnchorRelative,
        #[serde(default)]
        y_relative: AnchorRelative,
        #[serde(default)]
        x_offset: f64,
        #[serde(default)]
        y_offset: f64,
        #[serde(default)]
        object_point: u8,
        #[serde(default)]
        ref_point: u8,
        /// Keep Within Top/Bottom Column Boundaries.
        #[serde(default)]
        keep_within_column: bool,
        /// Relative to Spine: the settings describe a left-hand page, and on a right-hand page
        /// the reference point, the object point and the X offset mirror horizontally, so an
        /// object in the outside margin stays in the outside margin.
        #[serde(default)]
        spine_relative: bool,
    },
}

/// What a custom anchored object is positioned against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorRelative {
    /// The anchor marker (x) / the anchor's line baseline (y).
    #[default]
    Anchor,
    TextFrame,
    ColumnEdge,
    PageMargin,
    PageEdge,
}

impl Default for AnchorPosition {
    fn default() -> Self {
        AnchorPosition::Inline { y_offset: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchoredObject {
    /// The item, with its bounds' top-left at the origin of its parent space.
    pub item: Item,
    #[serde(default)]
    pub position: AnchorPosition,
}

impl AnchoredObject {
    /// Wrap `item`, moving it so its bounds start at the origin.
    pub fn new(mut item: Item, position: AnchorPosition) -> Self {
        let b = item.bounds();
        item.xf = designcraft_geom::Affine::translate((-b.x0, -b.y0)) * item.xf;
        AnchoredObject { item, position }
    }

    /// Width and height.
    pub fn size(&self) -> (f64, f64) {
        let b = self.item.bounds();
        (b.width(), b.height())
    }
}

impl Default for AnchoredObject {
    /// An empty placeholder (an object mark typed or pasted without its object).
    fn default() -> Self {
        let path = designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 0.0, 0.0));
        let item = Item::new(crate::ids::ItemId(0), crate::ids::LayerId(0), crate::item::Shape::Rectangle, path);
        AnchoredObject { item, position: AnchorPosition::default() }
    }
}

impl Story {
    /// Insert an anchored object at `pos`.
    pub fn insert_object(&mut self, pos: usize, obj: AnchoredObject) {
        let pos = crate::story::floor_char_boundary(&self.text, pos.min(self.len()));
        self.insert(pos, &OBJECT_MARK.to_string());
        let k = self.text[..pos].matches(OBJECT_MARK).count();
        self.objects[k] = Arc::new(obj);
    }

    /// The anchored object whose mark is at byte `pos`.
    pub fn object_at(&self, pos: usize) -> Option<&AnchoredObject> {
        if !self.text[pos.min(self.text.len())..].starts_with(OBJECT_MARK) {
            return None;
        }
        self.objects.get(self.text[..pos].matches(OBJECT_MARK).count()).map(|o| &**o)
    }
}

/// Visit the items anchored in `st`, its footnotes, endnotes and table cells (and theirs, so
/// deep), in order: a story's objects (a group before its items), then its notes and its cells.
fn items(st: &Story, depth: usize, f: &mut dyn FnMut(&Item)) {
    for o in &st.objects {
        o.item.walk(&mut |i| f(i));
    }
    if depth >= NESTING {
        return;
    }
    for n in st.notes.iter().chain(&st.endnotes) {
        items(&n.text, depth + 1, f);
    }
    for t in st.tables.values() {
        for c in &t.cells {
            items(&c.text, depth + 1, f);
        }
    }
}

/// [`items`], mutably (copy-on-write only where items are).
fn items_mut(st: &mut Story, depth: usize, f: &mut dyn FnMut(&mut Item)) {
    for o in &mut st.objects {
        walk_mut(&mut Arc::make_mut(o).item, f);
    }
    if depth >= NESTING {
        return;
    }
    for n in st.notes.iter_mut().chain(st.endnotes.iter_mut()) {
        if has_items(&n.text) {
            items_mut(&mut Arc::make_mut(n).text, depth + 1, f);
        }
    }
    for t in st.tables.values_mut() {
        if t.cells.iter().any(|c| has_items(&c.text)) {
            for c in &mut Arc::make_mut(t).cells {
                items_mut(&mut c.text, depth + 1, f);
            }
        }
    }
}

fn walk_mut(it: &mut Item, f: &mut dyn FnMut(&mut Item)) {
    f(it);
    if let Some(children) = it.children_mut() {
        for c in children {
            walk_mut(Arc::make_mut(c), f);
        }
    }
}

/// Does `st` (or a footnote or cell in it) hold anchored objects?
fn has_items(st: &Story) -> bool {
    let mut todo = vec![st];
    while let Some(s) = todo.pop() {
        if !s.objects.is_empty() {
            return true;
        }
        todo.extend(s.notes.iter().chain(&s.endnotes).map(|n| &n.text));
        todo.extend(s.tables.values().flat_map(|t| t.cells.iter().map(|c| &c.text)));
    }
    false
}

impl Document {
    /// Does any story (its footnotes and table cells included) hold anchored objects?
    pub fn has_anchored_items(&self) -> bool {
        self.stories.values().any(|s| has_items(s))
    }

    /// The stories the anchored text frames in `st` show, and the stories shown by the frames
    /// anchored in those: what copied text takes along.
    pub fn anchored_stories(&self, st: &Story) -> BTreeMap<StoryId, Arc<Story>> {
        let mut out = BTreeMap::new();
        let mut todo = Vec::new();
        items(st, 0, &mut |i| todo.extend(i.text_frame().map(|t| (t.story, 1))));
        while let Some((sid, depth)) = todo.pop() {
            if depth > NESTING || out.contains_key(&sid) {
                continue;
            }
            let Some(s) = self.stories.get(&sid) else { continue };
            out.insert(sid, s.clone());
            items(s, 0, &mut |i| todo.extend(i.text_frame().map(|t| (t.story, depth + 1))));
        }
        out
    }

    /// Text copied into this document (from it or another one): give the items anchored in `st`
    /// fresh ids, and each anchored text frame among them a copy of its story from `src` (an
    /// empty story when `src` hasn't it) threaded to the copied frames that show it, copying the
    /// frames anchored in those stories the same way.
    pub fn adopt_anchored(&mut self, st: &mut Story, src: &dyn Fn(StoryId) -> Option<Arc<Story>>) {
        self.adopt(&mut |f| items_mut(st, 0, f), src, None, &mut Vec::new(), &mut HashMap::new());
    }

    /// [`Document::adopt_anchored`] for text copied from document `src`: the copies of the
    /// stories its anchored text frames show also take the assets they place along from `src`
    /// (or reuse the same file here), since `src`'s asset ids may name other files here.
    pub fn adopt_anchored_from(&mut self, st: &mut Story, src: &Document) {
        self.adopt(&mut |f| items_mut(st, 0, f), &|id| src.stories.get(&id).cloned(), Some(src), &mut Vec::new(), &mut HashMap::new());
    }

    /// `visit` runs a function on every copied item. `path`: the stories being copied (a frame
    /// showing one of them again gets an empty story, so copies never nest without end).
    /// `assets`: the document the copied stories' assets are in, when it isn't this one.
    /// `copies`: the copy made of each source story in this operation. A story shown by frames in
    /// several copied stories is copied once and threads all their copies, as the original does,
    /// so a graph of shared stories doesn't multiply into exponentially many copies.
    fn adopt(
        &mut self,
        visit: &mut dyn FnMut(&mut dyn FnMut(&mut Item)),
        src: &dyn Fn(StoryId) -> Option<Arc<Story>>,
        assets: Option<&Document>,
        path: &mut Vec<StoryId>,
        copies: &mut HashMap<StoryId, StoryId>,
    ) {
        let mut next = self.next_id;
        let mut frames = Vec::new();
        visit(&mut |i| {
            next = next.saturating_add(1);
            i.id = ItemId(next);
            frames.extend(i.text_frame().map(|t| (i.id, t.story)));
        });
        self.next_id = next;
        // The story each copied frame shows: the operation's copy of its story, or an empty one
        // where copying would loop or nest too deep.
        let mut shows: HashMap<ItemId, StoryId> = HashMap::new();
        let mut empty: HashMap<StoryId, StoryId> = HashMap::new();
        for &(fid, old) in &frames {
            let looped = path.contains(&old) || path.len() >= NESTING;
            if let Some(&id) = if looped { empty.get(&old) } else { copies.get(&old) } {
                shows.insert(fid, id);
                continue;
            }
            let id = StoryId(self.alloc());
            shows.insert(fid, id);
            let copy = match src(old) {
                Some(s) if !looped => {
                    copies.insert(old, id);
                    let mut copy = (*s).clone();
                    copy.id = id;
                    copy.frames.clear();
                    if let Some(from) = assets {
                        // The graphics it places, under this document's ids.
                        let mut map = HashMap::new();
                        for a in crate::story_asset_refs(&copy) {
                            let here = self.adopt_asset(from, a).unwrap_or_else(|| AssetId(self.alloc()));
                            if here != a {
                                map.insert(a, here);
                            }
                        }
                        crate::remap_story_assets(&mut copy, &map);
                    }
                    path.push(old);
                    self.adopt(&mut |f| items_mut(&mut copy, 0, f), src, assets, path, copies);
                    path.pop();
                    copy
                }
                _ => {
                    if looped {
                        empty.insert(old, id);
                    } else {
                        copies.insert(old, id);
                    }
                    Story::new(id)
                }
            };
            self.stories.insert(id, Arc::new(copy));
        }
        visit(&mut |i| {
            if let Some(id) = shows.get(&i.id)
                && let Some(t) = i.text_frame_mut()
            {
                t.story = *id;
            }
        });
        for (fid, _) in frames {
            if let Some(st) = shows.get(&fid).and_then(|id| self.stories.get_mut(id)) {
                Arc::make_mut(st).frames.push(fid);
            }
        }
    }

    /// Ids of the items on spreads and parents, and of the items anchored in stories.
    fn live_items(&self) -> HashSet<ItemId> {
        let mut live = HashSet::new();
        for sp in self.spreads.iter().chain(&self.parents) {
            for it in &sp.items {
                it.walk(&mut |i| {
                    live.insert(i.id);
                });
            }
        }
        for st in self.stories.values() {
            items(st, 0, &mut |i| {
                live.insert(i.id);
            });
        }
        live
    }

    /// Keep anchored items valid ([`Document::check`]) after edits that copy or delete text and
    /// frames: an anchored item whose id is taken (text copied in the document) gets a fresh one;
    /// threads lose frames that are gone, and a story whose last frame went goes too (unless a
    /// frame still shows it); an anchored text frame its story doesn't thread is added to the
    /// story when that has no frames left, else gets a copy of it (an empty story when it is
    /// missing). Returns how many items, threads and stories changed.
    pub fn fix_anchored_text_frames(&mut self) -> usize {
        let mut changed = 0;
        // Fresh ids for copies: items on spreads keep theirs, then the first in story order.
        let mut seen = HashSet::new();
        for sp in self.spreads.iter().chain(&self.parents) {
            for it in &sp.items {
                it.walk(&mut |i| {
                    seen.insert(i.id);
                });
            }
        }
        let sids: Vec<StoryId> = self.stories.keys().copied().collect();
        for sid in &sids {
            let Some(st) = self.stories.get(sid) else { continue };
            let mut ids = Vec::new();
            items(st, 0, &mut |i| ids.push(i.id));
            let mut own = HashSet::new();
            if !ids.iter().any(|id| seen.contains(id) || !own.insert(*id)) {
                seen.extend(ids);
                continue;
            }
            let mut next = self.next_id;
            let Some(st) = self.stories.get_mut(sid) else { continue };
            let st = Arc::make_mut(st);
            items_mut(st, 0, &mut |i| {
                if !seen.insert(i.id) {
                    next = next.saturating_add(1);
                    i.id = ItemId(next);
                    seen.insert(i.id);
                    changed += 1;
                }
            });
            st.rev += 1;
            self.next_id = next;
        }
        // Frames that are gone (deleted with their text or their host's frame) leave their
        // threads; a story left without frames goes with them, and so on into the stories its
        // anchored frames showed.
        let mut live = seen;
        for round in 0..=self.stories.len() {
            if round > 0 {
                live = self.live_items();
            }
            let dangling: Vec<StoryId> =
                self.stories.iter().filter(|(_, st)| st.frames.iter().any(|f| !live.contains(f))).map(|(id, _)| *id).collect();
            if dangling.is_empty() {
                break;
            }
            let mut shown = HashSet::new();
            for sp in self.spreads.iter().chain(&self.parents) {
                for it in &sp.items {
                    it.walk(&mut |i| shown.extend(i.text_frame().map(|t| t.story)));
                }
            }
            for st in self.stories.values() {
                items(st, 0, &mut |i| shown.extend(i.text_frame().map(|t| t.story)));
            }
            for sid in dangling {
                let Some(st) = self.stories.get_mut(&sid) else { continue };
                let st = Arc::make_mut(st);
                st.frames.retain(|f| live.contains(f));
                changed += 1;
                if st.frames.is_empty() && !shown.contains(&sid) {
                    self.stories.remove(&sid);
                }
            }
        }
        // Anchored text frames their stories don't thread: copies, or frames whose story went.
        let mut frames = Vec::new();
        for (owner, st) in &self.stories {
            items(st, 0, &mut |i| frames.extend(i.text_frame().map(|t| (*owner, i.id, t.story))));
        }
        for (owner, fid, sid) in frames {
            let story = match self.stories.get_mut(&sid) {
                Some(st) if st.frames.contains(&fid) => continue,
                Some(st) if st.frames.is_empty() => {
                    Arc::make_mut(st).frames.push(fid);
                    changed += 1;
                    continue;
                }
                Some(st) => {
                    let mut copy = (**st).clone();
                    let id = StoryId(self.alloc());
                    copy.id = id;
                    let snapshot = self.stories.clone();
                    self.adopt(&mut |f| items_mut(&mut copy, 0, f), &|s| snapshot.get(&s).cloned(), None, &mut vec![sid], &mut HashMap::new());
                    copy.frames = vec![fid];
                    self.stories.insert(id, Arc::new(copy));
                    id
                }
                None => {
                    let id = StoryId(self.alloc());
                    let mut st = Story::new(id);
                    st.frames = vec![fid];
                    self.stories.insert(id, Arc::new(st));
                    id
                }
            };
            if let Some(st) = self.stories.get_mut(&owner) {
                let st = Arc::make_mut(st);
                items_mut(st, 0, &mut |i| {
                    if i.id == fid
                        && let Some(t) = i.text_frame_mut()
                    {
                        t.story = story;
                    }
                });
                st.rev += 1;
            }
            changed += 1;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use designcraft_geom::Rect;

    use super::*;
    use crate::build::NewDocument;
    use crate::{ParaFormat, SpreadRef};

    /// A document with a text frame (story returned) on its first page.
    fn doc() -> (Document, StoryId) {
        let mut d = Document::new(&NewDocument { pages: 1, ..Default::default() });
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 300.0), lid, "Host ", ParaFormat::default()).unwrap();
        (d, sid)
    }

    /// Anchor, at the end of `host`, a text frame showing a new story with `text`.
    fn anchor(d: &mut Document, host: StoryId, text: &str) -> (StoryId, ItemId) {
        let lid = d.default_layer();
        let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 100.0, 40.0), lid, text, ParaFormat::default()).unwrap();
        let it = d.item(fid).unwrap().clone();
        d.spread_mut(SpreadRef::Doc(0)).unwrap().items.retain(|i| i.id != fid);
        let h = d.story_mut(host).unwrap();
        let n = h.len();
        h.insert_object(n, AnchoredObject::new(it, AnchorPosition::default()));
        (sid, fid)
    }

    /// The stories the anchored text frames show, with their texts.
    fn shown(d: &Document) -> Vec<(StoryId, String)> {
        d.anchored_items().iter().filter_map(|(_, i)| i.text_frame()).map(|t| (t.story, d.story(t.story).unwrap().text.clone())).collect()
    }

    /// Stories shared between layers (two per layer, each anchoring a frame showing each story of
    /// the next layer) are copied once each when their text is copied, and each copy threads the
    /// copied frames that show it, as the original does. Copying them again for every frame that
    /// shows them doubled the work with every layer.
    #[test]
    fn a_graph_of_shared_stories_is_copied_once_per_story() {
        const LAYERS: usize = 12;
        let (mut d, host) = doc();
        let lid = d.default_layer();
        let layers: Vec<[StoryId; 2]> = (0..LAYERS)
            .map(|_| {
                [0, 1].map(|_| {
                    let id = StoryId(d.alloc());
                    d.stories.insert(id, Arc::new(Story::new(id)));
                    id
                })
            })
            .collect();
        // Anchor at the end of `host` a text frame showing `shows`, threaded to it.
        let anchor_showing = |d: &mut Document, host: StoryId, shows: StoryId| {
            let (fid, own) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 100.0, 40.0), lid, "", ParaFormat::default()).unwrap();
            let mut it = d.item(fid).unwrap().clone();
            d.spread_mut(SpreadRef::Doc(0)).unwrap().items.retain(|i| i.id != fid);
            d.stories.remove(&own);
            it.text_frame_mut().unwrap().story = shows;
            Arc::make_mut(d.stories.get_mut(&shows).unwrap()).frames.push(fid);
            let h = d.story_mut(host).unwrap();
            let n = h.len();
            h.insert_object(n, AnchoredObject::new(it, AnchorPosition::default()));
        };
        for &top in &layers[0] {
            anchor_showing(&mut d, host, top);
        }
        for k in 0..LAYERS - 1 {
            for s in layers[k] {
                for t in layers[k + 1] {
                    anchor_showing(&mut d, s, t);
                }
            }
        }
        let mut copy = d.story(host).unwrap().extract(0..d.story(host).unwrap().len());
        let stories = d.anchored_stories(&copy);
        assert_eq!(stories.len(), 2 * LAYERS);
        let before: HashSet<StoryId> = d.stories.keys().copied().collect();
        d.adopt_anchored(&mut copy, &|id| stories.get(&id).cloned());
        let added: Vec<StoryId> = d.stories.keys().copied().filter(|id| !before.contains(id)).collect();
        assert_eq!(added.len(), 2 * LAYERS, "one copy per story");
        // Below the top layer each copy threads the two copied frames that show it.
        let threads: Vec<usize> = added.iter().map(|id| d.story(*id).unwrap().frames.len()).collect();
        assert_eq!(threads.iter().filter(|&&n| n == 2).count(), 2 * (LAYERS - 1), "{threads:?}");
        assert_eq!(threads.iter().filter(|&&n| n == 1).count(), 2, "{threads:?}");
    }

    #[test]
    fn copied_anchored_text_frames_get_stories_of_their_own() {
        let (mut d, host) = doc();
        let (inner, _) = anchor(&mut d, host, "Inner ");
        anchor(&mut d, inner, "Deeper");
        d.check().unwrap();
        assert_eq!(d.fix_anchored_text_frames(), 0);
        // Text copied in the document: the same anchored frames twice.
        let copy = d.story(host).unwrap().extract(0..d.story(host).unwrap().len());
        let h = d.story_mut(host).unwrap();
        let n = h.len();
        h.insert_story(n, &copy);
        assert!(d.check().is_err());
        assert!(d.fix_anchored_text_frames() > 0);
        d.check().unwrap();
        let shown = shown(&d);
        assert_eq!(shown.len(), 4, "{shown:?}");
        assert_eq!(shown.iter().map(|(s, _)| *s).collect::<HashSet<_>>().len(), 4, "a story of its own each");
        assert_eq!(shown.iter().filter(|(_, t)| t == "Deeper").count(), 2, "{shown:?}");
        assert_eq!(shown.iter().filter(|(_, t)| t.starts_with("Inner ")).count(), 2, "{shown:?}");
    }

    #[test]
    fn deleted_anchored_text_frames_take_their_stories_along() {
        // With the text they are anchored in.
        let (mut d, host) = doc();
        let (inner, _) = anchor(&mut d, host, "Inner ");
        let (deeper, _) = anchor(&mut d, inner, "Deeper");
        let h = d.story_mut(host).unwrap();
        let n = h.len();
        h.delete(0..n);
        assert!(d.check().is_err());
        d.fix_anchored_text_frames();
        d.check().unwrap();
        assert!(d.story(inner).is_none() && d.story(deeper).is_none());
        // With the frame they are anchored in.
        let (mut d, host) = doc();
        let (inner, _) = anchor(&mut d, host, "Inner ");
        let frame = d.story(host).unwrap().frames[0];
        d.remove_item(frame).unwrap();
        assert!(d.check().is_err());
        d.fix_anchored_text_frames();
        d.check().unwrap();
        assert!(d.story(inner).is_none());
        // An unplaced story stays.
        let (mut d, _) = doc();
        let sid = StoryId(d.alloc());
        d.stories.insert(sid, Arc::new(Story::new(sid)));
        assert_eq!(d.fix_anchored_text_frames(), 0);
        assert!(d.story(sid).is_some());
    }

    #[test]
    fn anchored_text_frames_without_their_stories_get_one() {
        // Missing: an empty story of its own.
        let (mut d, host) = doc();
        let (inner, fid) = anchor(&mut d, host, "Inner");
        d.stories.remove(&inner);
        assert!(d.check().is_err());
        d.fix_anchored_text_frames();
        d.check().unwrap();
        let t = d.anchored_item(fid).unwrap().text_frame().unwrap().story;
        assert!(d.story(t).unwrap().text.is_empty());
        // Without frames (its frame was cut and pasted back): threaded to it again.
        let (mut d, host) = doc();
        let (inner, fid) = anchor(&mut d, host, "Inner");
        Arc::make_mut(d.stories.get_mut(&inner).unwrap()).frames.clear();
        d.fix_anchored_text_frames();
        d.check().unwrap();
        assert_eq!(d.story(inner).unwrap().frames, vec![fid]);
    }

    #[test]
    fn an_anchored_text_frame_copied_into_its_own_story_does_not_show_itself() {
        let (mut d, host) = doc();
        let (inner, _) = anchor(&mut d, host, "Inner ");
        let obj = (*d.story(host).unwrap().objects[0]).clone();
        let st = d.story_mut(inner).unwrap();
        let n = st.len();
        st.insert_object(n, obj);
        assert!(d.check().is_err());
        d.fix_anchored_text_frames();
        d.check().unwrap();
    }

    /// Text copied from another document takes along the graphics placed in the stories its
    /// anchored text frames show: the graphic keeps its own file (a.pdf), though its asset id
    /// names another file (b.pdf) in the target document.
    #[test]
    fn adopted_stories_take_their_graphics_along_from_another_document() {
        use crate::{Asset, Content, Graphic, Shape};
        let pdf = |id: AssetId, name: &str| Asset {
            id,
            name: name.into(),
            mime: "application/pdf".into(),
            data: Arc::new(format!("%PDF-1.7 {name}").into_bytes()),
            ..Default::default()
        };
        let (mut a, host) = doc();
        let (inner, _) = anchor(&mut a, host, "Inner ");
        let aid = AssetId(a.alloc());
        a.assets.insert(aid, Arc::new(pdf(aid, "a.pdf")));
        let mut g =
            Item::new(ItemId(a.alloc()), a.default_layer(), Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 20.0, 20.0)));
        g.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (20.0, 20.0),
            xf: designcraft_geom::Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        let st = a.story_mut(inner).unwrap();
        let n = st.len();
        st.insert_object(n, AnchoredObject::new(g, AnchorPosition::default()));
        a.check().unwrap();
        // The target document has b.pdf under the same id (allocated there too).
        let (mut b, target) = doc();
        while b.alloc() < aid.0 {}
        b.assets.insert(aid, Arc::new(pdf(aid, "b.pdf")));
        let src = a.story(host).unwrap();
        let mut slice = src.extract(0..src.len());
        b.adopt_anchored_from(&mut slice, &a);
        let t = b.story_mut(target).unwrap();
        let n = t.len();
        t.insert_story(n, &slice);
        b.check().unwrap();
        let placed: Vec<String> = b
            .anchored_items()
            .iter()
            .filter_map(|(_, i)| i.graphic())
            .map(|g| b.assets.get(&g.asset).map_or_else(|| "missing".to_string(), |a| a.name.clone()))
            .collect();
        assert_eq!(placed, ["a.pdf"], "the copied story's graphic places its own file");
        assert_eq!(b.assets.get(&aid).map(|a| a.name.as_str()), Some("b.pdf"), "the target's own asset is left alone");
    }

    #[test]
    fn adopted_text_brings_copies_of_its_anchored_stories() {
        let (mut a, host) = doc();
        let (inner, _) = anchor(&mut a, host, "Inner ");
        anchor(&mut a, inner, "Deeper");
        // Another document, whose ids are the same as the first one's.
        let (mut b, target) = doc();
        let (other, _) = anchor(&mut b, target, "Other");
        assert!(b.story(inner).is_some(), "ids collide");
        let src = a.story(host).unwrap();
        let mut slice = src.extract(0..src.len());
        let stories = a.anchored_stories(&slice);
        assert_eq!(stories.len(), 2);
        b.adopt_anchored(&mut slice, &|id| stories.get(&id).cloned());
        let t = b.story_mut(target).unwrap();
        let n = t.len();
        t.insert_story(n, &slice);
        b.check().unwrap();
        let mut texts: Vec<String> = shown(&b).into_iter().map(|(_, t)| t).collect();
        texts.sort();
        assert_eq!(texts, ["Deeper", "Inner \u{E00E}", "Other"]);
        assert_eq!(b.story(other).unwrap().text, "Other");
    }
}
