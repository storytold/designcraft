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
        if let Some(os) = self.styles.object_style(&self.styles.default_text_frame).cloned() {
            if let Some(f) = os.fill {
                item.fill = f;
            }
            if let Some(s) = os.stroke {
                item.stroke = s;
            }
        }
        item.content = Content::Text(TextFrame { story: sid, options: TextFrameOptions::default() });
        self.insert_item(r, item, None)?;
        Ok((id, sid))
    }

    /// Thread `from` → `to`: `to` joins `from`'s story right after `from`. If `to` held its own text,
    /// that text is appended to the story (as a new paragraph) and its old story is removed.
    pub fn thread(&mut self, from: ItemId, to: ItemId) -> Result<()> {
        if from == to {
            return Err(DocError::Invalid("cannot thread a frame to itself".into()));
        }
        let fs = self.item(from).and_then(|i| i.text_frame()).map(|t| t.story).ok_or(DocError::NoItem(from))?;
        // Convert unassigned/graphic-less frames into text frames.
        let ts = match self.item(to).ok_or(DocError::NoItem(to))?.content.clone() {
            Content::Text(t) => Some(t.story),
            Content::Unassigned => None,
            _ => return Err(DocError::Invalid("can only thread to text or empty frames".into())),
        };
        if ts == Some(fs) {
            return Err(DocError::Invalid("frames are already in the same story".into()));
        }
        if let Some(ts) = ts {
            let other = self.stories.remove(&ts).ok_or(DocError::NoStory(ts))?;
            let other = Arc::unwrap_or_clone(other);
            // Frames of the other story after `to` follow along; frames before it are left as empty new stories.
            let pos_to = other.frames.iter().position(|f| *f == to).unwrap_or(0);
            let before: Vec<ItemId> = other.frames[..pos_to].to_vec();
            let after: Vec<ItemId> = other.frames[pos_to..].to_vec();
            {
                let st = self.story_mut(fs).ok_or(DocError::NoStory(fs))?;
                if !other.text.is_empty() {
                    st.text.push('\n');
                    st.text.push_str(&other.text);
                    st.paras.extend(other.paras.iter().cloned());
                    // Char runs: '\n' takes the last run's format, then the other story's runs.
                    if let Some(last) = st.chars.last_mut() {
                        last.len += 1;
                    }
                    st.chars.extend(other.chars.iter().filter(|r| r.len > 0).cloned());
                    st.tables.extend(other.tables.iter().map(|(k, v)| (*k, v.clone())));
                    for n in &other.notes {
                        let id = st.next_note_id();
                        st.notes.push(Arc::new(crate::Footnote { id, text: n.text.clone() }));
                    }
                    st.anchors.extend(other.anchors.iter().cloned());
                    st.xrefs.extend(other.xrefs.iter().cloned());
                    st.index_refs.extend(other.index_refs.iter().cloned());
                    st.objects.extend(other.objects.iter().cloned());
                    st.rev += 1;
                }
                let at = st.frames.iter().position(|f| *f == from).map_or(st.frames.len(), |p| p + 1);
                for (k, f) in after.iter().enumerate() {
                    st.frames.insert(at + k, *f);
                }
            }
            for f in &after {
                if let Some(t) = self.item_mut(*f).and_then(Item::text_frame_mut) {
                    t.story = fs;
                }
            }
            if !before.is_empty() {
                let nid = StoryId(self.alloc());
                let mut ns = Story::new(nid);
                ns.frames = before.clone();
                ns.vertical = other.vertical;
                ns.direction = other.direction;
                for f in &before {
                    if let Some(t) = self.item_mut(*f).and_then(Item::text_frame_mut) {
                        t.story = nid;
                    }
                }
                self.stories.insert(nid, Arc::new(ns));
            }
        } else {
            let st = self.story_mut(fs).ok_or(DocError::NoStory(fs))?;
            let at = st.frames.iter().position(|f| *f == from).map_or(st.frames.len(), |p| p + 1);
            st.frames.insert(at, to);
            let it = self.item_mut(to).ok_or(DocError::NoItem(to))?;
            it.content = Content::Text(TextFrame { story: fs, options: TextFrameOptions::default() });
        }
        if let Some(st) = self.story_mut(fs) {
            st.rev += 1;
        }
        Ok(())
    }

    /// Break the thread after `frame`: the following frames become a new empty story (the text
    /// stays with the first part and may become overset).
    pub fn unthread_after(&mut self, frame: ItemId) -> Result<()> {
        let sid = self.item(frame).and_then(|i| i.text_frame()).map(|t| t.story).ok_or(DocError::NoItem(frame))?;
        let (tail, vertical, direction) = {
            let st = self.story_mut(sid).ok_or(DocError::NoStory(sid))?;
            let p = st.frames.iter().position(|f| *f == frame).ok_or(DocError::NoItem(frame))?;
            let tail = st.frames.split_off(p + 1);
            st.rev += 1;
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
        st.anchors.clear();
        st.xrefs.clear();
        st.index_refs.clear();
        st.objects.clear();
        st.fix_marks();
        st.rev += 1;
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
    designcraft_geom::hit::stroke_contains(&bp, it.stroke.weight.max(1.0), tol, inner)
}
