//! Anchored objects (Object › Anchored Object): page items that flow with the text.
//!
//! An anchored object is an [`OBJECT_MARK`] character in a story; [`Story::objects`] holds one
//! [`AnchoredObject`] per mark in text order (like footnotes). The item's geometry is kept with
//! its top-left at the origin; composition places it on the line (inline) or on a line of its
//! own above the line it is anchored in (above line).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

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
}
