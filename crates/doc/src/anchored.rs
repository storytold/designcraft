//! Anchored objects (Object › Anchored Object): page items that flow with the text.
//!
//! An anchored object is an [`OBJECT_MARK`] character in a story; [`Story::objects`] holds one
//! [`AnchoredObject`] per mark in text order (like footnotes). The item's geometry is kept with
//! its top-left at the origin; composition places it on the line (inline) or on a line of its
//! own above the line it is anchored in (above line).
//!
//! An anchored text frame (or one inside an anchored group) shows a story of its own: a story in
//! [`crate::Document::stories`] with no frames, whose only frame is that anchored item (see
//! [`crate::Document::anchored_text_frame`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Document;
use crate::ids::StoryId;
use crate::item::{Content, Item};
use crate::story::Story;

/// How deep anchored text frames are followed through each other (a frame in a story shown by a
/// frame in a story…) when composing and drawing.
pub const MAX_ANCHOR_DEPTH: usize = 8;

/// Most stories [`Document::fix_anchored_text_frames`] copies in one go.
const MAX_STORY_COPIES: usize = 256;

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

/// An anchored text frame: the story whose object holds it, the object's index, the frame's
/// path through groups inside the object, and the story the frame shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchoredFrameRef {
    pub host: StoryId,
    pub object: usize,
    pub path: Vec<usize>,
    pub story: StoryId,
}

fn text_frames_in(it: &Item, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, StoryId)>) {
    if let Some(t) = it.text_frame() {
        out.push((path.clone(), t.story));
    }
    for (i, c) in it.children().iter().enumerate() {
        path.push(i);
        text_frames_in(c, path, out);
        path.pop();
    }
}

impl Document {
    /// The anchored text frame that shows story `sid`: its host story, the object's index in the
    /// host's objects and the frame item (the object itself or an item in an anchored group).
    pub fn anchored_text_frame(&self, sid: StoryId) -> Option<(StoryId, usize, &Item)> {
        for (hid, st) in &self.stories {
            for (k, o) in st.objects.iter().enumerate() {
                let mut hit = None;
                o.item.walk(&mut |i| {
                    if hit.is_none() && i.text_frame().is_some_and(|t| t.story == sid) {
                        hit = Some(i);
                    }
                });
                if let Some(i) = hit {
                    return Some((*hid, k, i));
                }
            }
        }
        None
    }

    /// Every anchored text frame, by host story, then object, then position in the object.
    pub fn anchored_frame_refs(&self) -> Vec<AnchoredFrameRef> {
        let mut out = Vec::new();
        let mut found = Vec::new();
        for (hid, st) in &self.stories {
            for (k, o) in st.objects.iter().enumerate() {
                found.clear();
                text_frames_in(&o.item, &mut Vec::new(), &mut found);
                out.extend(found.drain(..).map(|(path, story)| AnchoredFrameRef { host: *hid, object: k, path, story }));
            }
        }
        out
    }

    /// Anchored text frames that can't show their story: it is missing, it has frames of its
    /// own, or it shows (through anchored frames) the story the frame is in. Indices into `refs`.
    fn invalid_anchored_frames(&self, refs: &[AnchoredFrameRef]) -> Vec<usize> {
        let mut bad = Vec::new();
        let mut out: BTreeMap<StoryId, Vec<usize>> = BTreeMap::new();
        for (i, r) in refs.iter().enumerate() {
            if r.story != r.host && self.stories.get(&r.story).is_some_and(|s| s.frames.is_empty()) {
                out.entry(r.host).or_default().push(i);
            } else {
                bad.push(i);
            }
        }
        // Cycles: a depth-first walk from host to shown story; an edge back into the walk closes one.
        let mut state: HashMap<StoryId, bool> = HashMap::new(); // true while on the walk
        for &root in out.keys() {
            if state.contains_key(&root) {
                continue;
            }
            state.insert(root, true);
            let mut stack = vec![(root, 0usize)];
            while let Some(top) = stack.last_mut() {
                let node = top.0;
                let next = out.get(&node).and_then(|v| v.get(top.1)).copied();
                top.1 += 1;
                match next {
                    Some(ri) => {
                        let Some(child) = refs.get(ri).map(|r| r.story) else { continue };
                        match state.get(&child) {
                            Some(true) => bad.push(ri),
                            Some(false) => {}
                            None => {
                                state.insert(child, true);
                                stack.push((child, 0));
                            }
                        }
                    }
                    None => {
                        state.insert(node, false);
                        stack.pop();
                    }
                }
            }
        }
        bad.sort_unstable();
        bad
    }

    /// Point an anchored text frame at another story, or (`None`) make it an empty frame.
    fn set_anchored_frame_story(&mut self, r: &AnchoredFrameRef, story: Option<StoryId>) {
        let Some(st) = self.story_mut(r.host) else { return };
        st.rev += 1;
        let Some(o) = st.objects.get_mut(r.object) else { return };
        let mut it = &mut Arc::make_mut(o).item;
        for &i in &r.path {
            let Some(c) = it.children_mut().and_then(|c| c.get_mut(i)) else { return };
            it = Arc::make_mut(c);
        }
        match (story, &mut it.content) {
            (Some(s), Content::Text(t)) => t.story = s,
            (None, c) => *c = Content::Unassigned,
            _ => {}
        }
    }

    /// Make every anchored text frame valid (see [`Document::check`]): frames whose story is
    /// missing, has frames of its own or would show itself become empty frames. A story shown by
    /// more than one anchored frame (text or objects copied in the document) keeps its first
    /// frame; the others get a copy of it with `copy_shared`, else become empty frames. Returns
    /// how many frames changed.
    pub fn fix_anchored_text_frames(&mut self, copy_shared: bool) -> usize {
        let refs = self.anchored_frame_refs();
        if refs.is_empty() {
            return 0;
        }
        let mut changed = 0;
        for i in self.invalid_anchored_frames(&refs) {
            if let Some(r) = refs.get(i) {
                self.set_anchored_frame_story(r, None);
                changed += 1;
            }
        }
        // Copies show the same anchored stories again: share those out on the next round.
        let mut copies = 0;
        loop {
            let refs = self.anchored_frame_refs();
            let mut seen = HashSet::new();
            let extra: Vec<&AnchoredFrameRef> = refs.iter().filter(|r| !seen.insert(r.story)).collect();
            if extra.is_empty() {
                break;
            }
            let copying = copy_shared && copies < MAX_STORY_COPIES;
            for r in extra {
                let copy = match self.stories.get(&r.story) {
                    Some(src) if copying && copies < MAX_STORY_COPIES => {
                        let mut st = (**src).clone();
                        st.id = StoryId(self.alloc());
                        st.frames.clear();
                        copies += 1;
                        let id = st.id;
                        self.stories.insert(id, Arc::new(st));
                        Some(id)
                    }
                    _ => None,
                };
                self.set_anchored_frame_story(r, copy);
                changed += 1;
            }
            if !copying {
                break;
            }
        }
        changed
    }

    /// The anchored-text-frame invariants of [`Document::check`].
    pub(crate) fn check_anchored_text_frames(&self) -> crate::Result<()> {
        let refs = self.anchored_frame_refs();
        let mut hosts: HashSet<StoryId> = HashSet::new();
        for r in &refs {
            let Some(st) = self.stories.get(&r.story) else { return Err(crate::DocError::NoStory(r.story)) };
            if !st.frames.is_empty() {
                return Err(crate::DocError::Invalid(format!("anchored story {} also has frames", r.story)));
            }
            if !hosts.insert(r.story) {
                return Err(crate::DocError::Invalid(format!("story {} is in more than one anchored frame", r.story)));
            }
        }
        if let Some(r) = self.invalid_anchored_frames(&refs).first().and_then(|i| refs.get(*i)) {
            return Err(crate::DocError::Invalid(format!("anchored story {} shows itself (story {})", r.story, r.host)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use designcraft_geom::{Rect, shapes};

    use super::*;
    use crate::build::NewDocument;
    use crate::{ItemId, ParaFormat, Shape, SpreadRef, TextFrame};

    /// A document with a text frame (story returned) on its first page.
    fn doc() -> (Document, StoryId) {
        let mut d = Document::new(&NewDocument { pages: 1, ..Default::default() });
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 300.0), lid, "Host", ParaFormat::default()).unwrap();
        (d, sid)
    }

    fn text_frame(d: &mut Document, story: StoryId) -> Item {
        let mut it = Item::new(ItemId(d.alloc()), d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 40.0)));
        it.content = Content::Text(TextFrame { story, options: Default::default() });
        it
    }

    /// Anchor a text frame showing `story` at the end of `host`.
    fn anchor(d: &mut Document, host: StoryId, story: StoryId) {
        let it = text_frame(d, story);
        let h = d.story_mut(host).unwrap();
        let n = h.len();
        h.insert_object(n, AnchoredObject::new(it, AnchorPosition::default()));
    }

    /// A new frameless story anchored at the end of `host`.
    fn anchored_story(d: &mut Document, host: StoryId, text: &str) -> StoryId {
        let sid = StoryId(d.alloc());
        d.stories.insert(sid, Arc::new(Story::with_text(sid, text, ParaFormat::default())));
        anchor(d, host, sid);
        sid
    }

    #[test]
    fn an_anchored_text_frame_shows_a_frameless_story() {
        let (mut d, host) = doc();
        let inner = anchored_story(&mut d, host, "Inner");
        let deeper = anchored_story(&mut d, inner, "Deeper");
        d.check().unwrap();
        let (h, k, it) = d.anchored_text_frame(inner).unwrap();
        assert_eq!((h, k), (host, 0));
        assert_eq!(it.text_frame().unwrap().story, inner);
        assert_eq!(d.anchored_text_frame(deeper).map(|(h, ..)| h), Some(inner));
        assert!(d.anchored_text_frame(host).is_none());
        assert_eq!(d.fix_anchored_text_frames(true), 0);
    }

    #[test]
    fn check_rejects_invalid_anchored_text_frames() {
        // Missing story.
        let (mut d, host) = doc();
        anchor(&mut d, host, StoryId(9999));
        assert!(d.check().is_err());
        // A story threaded on a spread.
        let (mut d, host) = doc();
        let lid = d.default_layer();
        let (_, other) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(310.0, 36.0, 400.0, 300.0), lid, "x", ParaFormat::default()).unwrap();
        anchor(&mut d, host, other);
        assert!(d.check().is_err());
        // Shown twice.
        let (mut d, host) = doc();
        let inner = anchored_story(&mut d, host, "Inner");
        anchor(&mut d, host, inner);
        assert!(d.check().is_err());
        // Showing itself, directly or through another story.
        let (mut d, host) = doc();
        let inner = anchored_story(&mut d, host, "Inner");
        anchor(&mut d, inner, inner);
        assert!(d.check().is_err());
        let (mut d, host) = doc();
        let a = anchored_story(&mut d, host, "A");
        let b = anchored_story(&mut d, a, "B");
        anchor(&mut d, b, a);
        assert!(d.check().is_err());
    }

    #[test]
    fn fixing_empties_frames_that_cannot_show_their_story() {
        let (mut d, host) = doc();
        anchor(&mut d, host, StoryId(9999));
        let a = anchored_story(&mut d, host, "A");
        let b = anchored_story(&mut d, a, "B");
        anchor(&mut d, b, a);
        anchor(&mut d, b, b);
        assert_eq!(d.fix_anchored_text_frames(false), 3);
        d.check().unwrap();
        // The good ones still show their stories.
        assert_eq!(d.anchored_text_frame(a).map(|(h, ..)| h), Some(host));
        assert_eq!(d.anchored_text_frame(b).map(|(h, ..)| h), Some(a));
        let empty = d.story(b).unwrap().objects.iter().filter(|o| matches!(o.item.content, Content::Unassigned)).count();
        assert_eq!(empty, 2);
    }

    #[test]
    fn a_shared_anchored_story_is_copied_or_emptied() {
        let (mut d, host) = doc();
        let inner = anchored_story(&mut d, host, "Inner");
        let deeper = anchored_story(&mut d, inner, "Deeper");
        // Text copied in the document: the same frames twice.
        let copy = d.story(host).unwrap().objects[0].clone();
        let h = d.story_mut(host).unwrap();
        let n = h.len();
        h.insert_object(n, (*copy).clone());
        assert!(d.check().is_err());
        let mut emptied = d.clone();
        assert_eq!(emptied.fix_anchored_text_frames(false), 1);
        emptied.check().unwrap();
        // Copies, two levels deep.
        assert_eq!(d.fix_anchored_text_frames(true), 2);
        d.check().unwrap();
        let shown: Vec<StoryId> = d.anchored_frame_refs().iter().map(|r| r.story).collect();
        assert_eq!(shown.len(), 4);
        let texts: Vec<&str> = shown.iter().map(|s| d.story(*s).unwrap().text.as_str()).collect();
        assert_eq!(texts.iter().filter(|t| **t == "Inner\u{E00E}").count(), 2, "{texts:?}");
        assert_eq!(texts.iter().filter(|t| **t == "Deeper").count(), 2, "{texts:?}");
        assert!(shown.contains(&inner) && shown.contains(&deeper));
    }
}
