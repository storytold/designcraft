//! Anchored objects (Object › Anchored Object): page items that flow with the text.
//!
//! An anchored object is an [`OBJECT_MARK`] character in a story; [`Story::objects`] holds one
//! [`AnchoredObject`] per mark in text order (like footnotes). The item's geometry is kept with
//! its top-left at the origin; composition places it on the line (inline) or on a line of its
//! own above the line it is anchored in (above line).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Document;
use crate::ids::{ItemId, StoryId};
use crate::item::Item;
use crate::story::Story;

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

    /// The text frames anchored in `range`, including frames inside groups and in the footnotes,
    /// endnotes and tables the range removes, with their stories.
    pub fn anchored_text_frames_in(&self, range: std::ops::Range<usize>) -> Vec<(StoryId, ItemId)> {
        let a = crate::story::floor_char_boundary(&self.text, range.start.min(self.len()));
        let b = crate::story::floor_char_boundary(&self.text, range.end.min(self.len())).max(a);
        let (Some(before), Some(inside)) = (self.text.get(..a), self.text.get(a..b)) else { return Vec::new() };
        let first = before.matches(OBJECT_MARK).count();
        let n = inside.matches(OBJECT_MARK).count();
        let mut out = Vec::new();
        for o in self.objects.get(first..first.saturating_add(n)).unwrap_or_default() {
            o.item.walk(&mut |i| {
                if let Some(tf) = i.text_frame() {
                    out.push((tf.story, i.id));
                }
            });
        }
        // Story::delete drops the footnotes, endnotes and tables whose marks the range removes.
        let footnotes = inside.matches(crate::notes::FOOTNOTE_REF).count();
        if footnotes > 0 {
            let k = self.notes_before(a);
            for n in self.notes.get(k..k.saturating_add(footnotes)).unwrap_or_default() {
                out.extend(n.text.anchored_text_frames());
            }
        }
        let endnotes = inside.matches(crate::endnotes::ENDNOTE_REF).count();
        if endnotes > 0 {
            let k = self.endnotes_before(a);
            for n in self.endnotes.get(k..k.saturating_add(endnotes)).unwrap_or_default() {
                out.extend(n.text.anchored_text_frames());
            }
        }
        for id in self.tables.keys() {
            if self.table_anchor(*id).is_some_and(|at| at >= a && at < b) {
                let Some(table) = self.tables.get(id) else { continue };
                for c in &table.cells {
                    out.extend(c.text.anchored_text_frames());
                }
            }
        }
        out
    }

    /// Every text frame anchored in this story, its table cells, footnotes and endnotes.
    pub fn anchored_text_frames(&self) -> Vec<(StoryId, ItemId)> {
        let mut out = Vec::new();
        self.collect_anchored_text_frames(0, &mut out);
        out
    }

    /// Depth-bounded collection for [`Story::anchored_text_frames`].
    fn collect_anchored_text_frames(&self, depth: usize, out: &mut Vec<(StoryId, ItemId)>) {
        if depth > 64 {
            return;
        }
        for o in &self.objects {
            o.item.walk(&mut |i| {
                if let Some(tf) = i.text_frame() {
                    out.push((tf.story, i.id));
                }
            });
        }
        for t in self.tables.values() {
            for c in &t.cells {
                c.text.collect_anchored_text_frames(depth + 1, out);
            }
        }
        for n in self.notes.iter().chain(&self.endnotes) {
            n.text.collect_anchored_text_frames(depth + 1, out);
        }
    }
}

impl Document {
    /// Delete a range from a story (or a cell or footnote of it), unthreading the anchored text
    /// frames whose marks it removed.
    pub fn delete_text_range(&mut self, sid: StoryId, cell: Option<crate::CellAddr>, range: std::ops::Range<usize>) -> Result<(), crate::DocError> {
        let doomed = {
            let story = self.text_story_mut(sid, cell).ok_or(crate::DocError::NoStory(sid))?;
            let doomed = story.anchored_text_frames_in(range.clone());
            story.delete(range);
            doomed
        };
        self.unthread_anchored_text_frames(&doomed);
        Ok(())
    }

    /// Replace a range in a story (or a cell or footnote of it), unthreading the anchored text
    /// frames whose marks it removed.
    pub fn replace_text_range(
        &mut self,
        sid: StoryId,
        cell: Option<crate::CellAddr>,
        range: std::ops::Range<usize>,
        text: &str,
    ) -> Result<(), crate::DocError> {
        let doomed = {
            let story = self.text_story_mut(sid, cell).ok_or(crate::DocError::NoStory(sid))?;
            let doomed = story.anchored_text_frames_in(range.clone());
            story.replace(range, text);
            doomed
        };
        self.unthread_anchored_text_frames(&doomed);
        Ok(())
    }

    /// Take `doomed` anchored text frames out of their stories' threads. A story left without a
    /// frame is deleted, and the text frames anchored in it are unthreaded too.
    pub fn unthread_anchored_text_frames(&mut self, doomed: &[(StoryId, ItemId)]) {
        let mut queue = doomed.to_vec();
        let mut seen = std::collections::HashSet::new();
        while let Some((sid, id)) = queue.pop() {
            if !seen.insert((sid, id)) {
                continue;
            }
            let mut nested = Vec::new();
            let mut empty = false;
            if let Some(st) = self.story_mut(sid) {
                st.frames.retain(|f| *f != id);
                if st.frames.is_empty() {
                    nested = st.anchored_text_frames();
                    empty = true;
                }
            }
            if empty {
                self.stories.remove(&sid);
                queue.extend(nested);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ids::LayerId;
    use crate::item::{Content, Shape, TextFrame};
    use crate::story::ParaFormat;
    use crate::table::Table;
    use designcraft_geom::{Rect, shapes};

    fn text_frame(id: u64, story: StoryId) -> Item {
        let mut item = Item::new(ItemId(id), LayerId(1), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
        item.content = Content::Text(TextFrame { story, options: Default::default() });
        item
    }

    #[test]
    fn range_cleanup_sees_frames_inside_footnotes_and_tables() {
        let mut host = Story::with_text(StoryId(1), "a b", ParaFormat::default());
        host.insert_note(1, "note", ParaFormat::default());
        let note = Arc::make_mut(&mut host.notes[0]);
        note.text.insert_object(0, AnchoredObject::new(text_frame(10, StoryId(10)), Default::default()));
        let mut table = Table::new(2, 1, 1, 0, 0, 80.0);
        table.cells[0].text.insert_object(0, AnchoredObject::new(text_frame(11, StoryId(11)), Default::default()));
        host.insert_table(2, table);
        let doomed = host.anchored_text_frames_in(0..host.len());
        assert!(doomed.contains(&(StoryId(10), ItemId(10))), "footnote frame: {doomed:?}");
        assert!(doomed.contains(&(StoryId(11), ItemId(11))), "table cell frame: {doomed:?}");
    }
}
