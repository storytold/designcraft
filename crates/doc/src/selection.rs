//! What is selected: page items (containers or their content) or a text range in a story.

use serde::{Deserialize, Serialize};

use crate::edit::SpreadRef;
use crate::ids::{ItemId, StoryId};
use crate::table::CellRange;

/// A selected ruler guide: index `index` in the guides of page `page` of spread `spread`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideSel {
    pub spread: SpreadRef,
    pub page: usize,
    pub index: usize,
}

/// A table cell: the table (in the selection's story) and the owning cell's grid position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CellAddr {
    pub table: u64,
    pub row: usize,
    pub col: usize,
}

impl CellAddr {
    /// The text of footnote `id` (see [`crate::notes::FOOTNOTE_TABLE`]).
    pub fn footnote(id: u64) -> Self {
        CellAddr { table: crate::notes::FOOTNOTE_TABLE, row: id as usize, col: 0 }
    }
    /// The footnote id when this addresses footnote text.
    pub fn footnote_id(&self) -> Option<u64> {
        (self.table == crate::notes::FOOTNOTE_TABLE).then_some(self.row as u64)
    }
}

/// Selected table cells (Table > Select, dragging across cells).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableSel {
    pub story: StoryId,
    pub table: u64,
    pub range: CellRange,
}

/// A caret or text range in a story. `anchor` stays put while `focus` moves (Shift-arrows).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSel {
    pub story: StoryId,
    pub anchor: usize,
    pub focus: usize,
    /// The frame the caret is shown in (for carets at frame boundaries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<ItemId>,
    /// Text in a table cell: `anchor`/`focus` index the cell's story instead of `story`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<CellAddr>,
}

impl TextSel {
    pub fn caret(story: StoryId, pos: usize) -> Self {
        TextSel { story, anchor: pos, focus: pos, frame: None, cell: None }
    }
    pub fn range(&self) -> std::ops::Range<usize> {
        self.anchor.min(self.focus)..self.anchor.max(self.focus)
    }
    pub fn is_caret(&self) -> bool {
        self.anchor == self.focus
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// Selected items (top-level or, with the Direct Selection tool, nested items).
    pub items: Vec<ItemId>,
    /// Content selection (graphic inside its frame) instead of the container.
    #[serde(default)]
    pub content: bool,
    /// Direct selection: selected anchors as (item, subpath, anchor).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchors: Vec<(ItemId, usize, usize)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextSel>,
    /// Selected table cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells: Option<TableSel>,
    /// Key object for Align.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<ItemId>,
    /// The page selected with the Page tool (absolute index).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    /// Selected ruler guides. Never together with items, text or cells: selecting one clears
    /// the other.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<GuideSel>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.text.is_none() && self.cells.is_none()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn items(ids: Vec<ItemId>) -> Self {
        Selection { items: ids, ..Default::default() }
    }
    pub fn text(t: TextSel) -> Self {
        Selection { text: Some(t), ..Default::default() }
    }
    pub fn contains(&self, id: ItemId) -> bool {
        self.items.contains(&id)
    }
}
