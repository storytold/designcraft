//! Spreads, pages, margins, columns, guides; parent ("master") spreads; sections.

use std::sync::Arc;

use designcraft_geom::Rect;
use serde::{Deserialize, Serialize};

use crate::attrs::NumberStyle;
use crate::ids::{ItemId, PageId, SpreadId};
use crate::item::Item;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Margins {
    pub top: f64,
    pub bottom: f64,
    /// Inside (facing pages) / left.
    pub inside: f64,
    /// Outside (facing pages) / right.
    pub outside: f64,
}

impl Margins {
    pub fn uniform(v: f64) -> Self {
        Margins { top: v, bottom: v, inside: v, outside: v }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Columns {
    pub count: u32,
    pub gutter: f64,
    /// Custom column positions (after dragging column guides); `None` = evenly spaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positions: Option<Vec<f64>>,
}

impl Default for Columns {
    fn default() -> Self {
        Columns { count: 1, gutter: 12.0, positions: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guide {
    pub orientation: Orientation,
    /// Spread coordinate (x for vertical guides, y for horizontal).
    pub position: f64,
    /// Spread guides cross the whole pasteboard; page guides only their page.
    #[serde(default)]
    pub spread: bool,
    #[serde(default)]
    pub locked: bool,
    /// The layer the guide is on: hidden with it, locked with it (`None` = always shown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<crate::LayerId>,
    /// Liquid guide: objects it crosses stretch when a guide-based page changes size.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub liquid: bool,
}

impl Guide {
    /// Shown when its layer is visible with Show Guides on.
    pub fn visible_in(&self, d: &crate::Document) -> bool {
        self.layer.is_none_or(|l| d.layer(l).is_none_or(|l| l.visible && l.show_guides))
    }
    /// Can be dragged: neither it nor its layer is locked.
    pub fn editable_in(&self, d: &crate::Document) -> bool {
        !self.locked && self.layer.is_none_or(|l| d.layer(l).is_none_or(|l| !l.locked && l.visible && l.show_guides))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageSide {
    #[default]
    Single,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: PageId,
    pub width: f64,
    pub height: f64,
    /// Position of the page's left edge in spread coordinates (top is y = 0).
    pub x: f64,
    pub margins: Margins,
    pub columns: Columns,
    /// Applied parent spread (`None` = [None]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SpreadId>,
    #[serde(default)]
    pub side: PageSide,
    /// Parent items overridden on this page (hidden from the parent rendering).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overridden: Vec<ItemId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<Guide>,
    /// Show parent items on this page.
    #[serde(default = "yes")]
    pub show_parent_items: bool,
    /// Liquid page rule: how objects follow when the page changes size.
    #[serde(default, skip_serializing_if = "LiquidRule::is_off")]
    pub liquid: LiquidRule,
    /// Window › Interactive › Page Transitions (the spread's first page holds the spread's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<PageTransition>,
    /// View › Rotate Spread: quarter turns clockwise of the spread on screen (the spread's first
    /// page holds it).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub view_rotation: u8,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// A page transition for interactive PDF.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTransition {
    pub kind: TransitionKind,
    /// Seconds.
    #[serde(default = "one_second")]
    pub duration: f64,
    /// Horizontal (else vertical) for blinds, split; inward (else outward) for box, split.
    #[serde(default)]
    pub horizontal: bool,
}

fn one_second() -> f64 {
    1.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransitionKind {
    Blinds,
    Box,
    Comb,
    Cover,
    Dissolve,
    Fade,
    Push,
    Split,
    Uncover,
    Wipe,
    ZoomIn,
    ZoomOut,
}

impl TransitionKind {
    pub const ALL: [TransitionKind; 12] = [
        TransitionKind::Blinds,
        TransitionKind::Box,
        TransitionKind::Comb,
        TransitionKind::Cover,
        TransitionKind::Dissolve,
        TransitionKind::Fade,
        TransitionKind::Push,
        TransitionKind::Split,
        TransitionKind::Uncover,
        TransitionKind::Wipe,
        TransitionKind::ZoomIn,
        TransitionKind::ZoomOut,
    ];
    pub fn label(self) -> &'static str {
        match self {
            TransitionKind::Blinds => "Blinds",
            TransitionKind::Box => "Box",
            TransitionKind::Comb => "Comb",
            TransitionKind::Cover => "Cover",
            TransitionKind::Dissolve => "Dissolve",
            TransitionKind::Fade => "Fade",
            TransitionKind::Push => "Push",
            TransitionKind::Split => "Split",
            TransitionKind::Uncover => "Uncover",
            TransitionKind::Wipe => "Wipe",
            TransitionKind::ZoomIn => "Zoom In",
            TransitionKind::ZoomOut => "Zoom Out",
        }
    }
}

/// Liquid Layout page rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiquidRule {
    #[default]
    Off,
    /// Everything scales uniformly (letterboxed, centred).
    Scale,
    /// Everything keeps its size, centred on the new page.
    ReCenter,
    /// Objects crossed by liquid guides stretch; the rest keep their size.
    GuideBased,
    /// Each object's pins and resize settings decide (`Item::liquid`).
    ObjectBased,
}

impl LiquidRule {
    pub fn is_off(&self) -> bool {
        *self == LiquidRule::Off
    }
}

fn yes() -> bool {
    true
}

impl Page {
    pub fn bounds(&self) -> Rect {
        Rect::new(self.x, 0.0, self.x + self.width, self.height)
    }
    /// Left and right margins resolved for the page side (inside is at the spine).
    pub fn left_right_margins(&self) -> (f64, f64) {
        match self.side {
            PageSide::Left => (self.margins.outside, self.margins.inside),
            _ => (self.margins.inside, self.margins.outside),
        }
    }
    /// The live area inside the margins.
    pub fn margin_rect(&self) -> Rect {
        let (l, r) = self.left_right_margins();
        let b = self.bounds();
        Rect::new(b.x0 + l, b.y0 + self.margins.top, (b.x1 - r).max(b.x0 + l), (b.y1 - self.margins.bottom).max(b.y0 + self.margins.top))
    }
    /// Column rects within the margins.
    pub fn column_rects(&self) -> Vec<Rect> {
        let m = self.margin_rect();
        column_rects(m, self.columns.count.max(1), self.columns.gutter)
    }
}

/// Split `area` into `n` equal columns separated by `gutter`.
pub fn column_rects(area: Rect, n: u32, gutter: f64) -> Vec<Rect> {
    let n = n.max(1) as usize;
    let w = ((area.width() - gutter * (n as f64 - 1.0)) / n as f64).max(0.0);
    (0..n).map(|i| Rect::new(area.x0 + i as f64 * (w + gutter), area.y0, area.x0 + i as f64 * (w + gutter) + w, area.y1)).collect()
}

/// Parent spread metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentInfo {
    /// `A`, `B`…
    pub prefix: String,
    /// `Parent`.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<SpreadId>,
}

impl ParentInfo {
    pub fn label(&self) -> String {
        format!("{}-{}", self.prefix, self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spread {
    pub id: SpreadId,
    pub pages: Vec<Page>,
    /// Items in z-order (last = front). Coordinates are spread space.
    pub items: Vec<Arc<Item>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentInfo>,
    #[serde(default = "yes")]
    pub allow_shuffle: bool,
}

impl Spread {
    /// Union of the page rects.
    pub fn bounds(&self) -> Rect {
        self.pages.iter().map(Page::bounds).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO)
    }
    /// x of the spine (between left and right pages) or the page centre for single pages.
    pub fn spine_x(&self) -> f64 {
        match self.pages.iter().position(|p| p.side == PageSide::Right) {
            Some(i) => self.pages[i].x,
            // Left pages only (a right-to-left document's first page): the spine is on their right.
            None if !self.pages.is_empty() && self.pages.iter().all(|p| p.side == PageSide::Left) => self.bounds().x1,
            None => self.bounds().center().x,
        }
    }
    /// Indices of the leftmost and rightmost pages. A right-to-left bound spread lists its right
    /// page first, so list order says nothing about position.
    pub fn outer_page_indices(&self) -> Option<(usize, usize)> {
        let mut it = self.pages.iter().enumerate();
        let (first, _) = it.next()?;
        Some(it.fold((first, first), |(l, r), (i, p)| (if p.x < self.pages[l].x { i } else { l }, if p.x > self.pages[r].x { i } else { r })))
    }
    /// The parent page (index into this parent spread) shown behind a document page on `side`:
    /// the leftmost page for a left page and the rightmost for a right page, whichever binding
    /// ordered the list; the last page for a single-sided page or a one-page parent.
    pub fn parent_page_for_side(&self, side: PageSide) -> Option<usize> {
        let (l, r) = self.outer_page_indices()?;
        match side {
            _ if self.pages.len() < 2 => Some(l),
            PageSide::Left => Some(l),
            PageSide::Right => Some(r),
            PageSide::Single => self.pages.len().checked_sub(1),
        }
    }
    /// Index of the page under spread x (nearest page for the pasteboard).
    pub fn page_at_x(&self, x: f64) -> Option<usize> {
        if self.pages.is_empty() {
            return None;
        }
        self.pages.iter().position(|p| x >= p.x && x < p.x + p.width).or_else(|| {
            // The pasteboard: the page nearest that side.
            let (l, r) = self.outer_page_indices()?;
            Some(if x < self.pages[l].x { l } else { r })
        })
    }
    /// Lay the pages out with no gaps from x = 0: left to right, or right to left for a
    /// right-to-left bound pair (its first page is the right one).
    pub fn relayout(&mut self) {
        let rtl = self.pages.len() == 2 && self.pages[0].side == PageSide::Right && self.pages[1].side == PageSide::Left;
        let mut x = 0.0;
        if rtl {
            for p in self.pages.iter_mut().rev() {
                p.x = x;
                x += p.width;
            }
            return;
        }
        for p in &mut self.pages {
            p.x = x;
            x += p.width;
        }
    }
}

/// A numbering section starting at a document page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// Absolute index of the first page of the section.
    pub start: usize,
    /// Start numbering at (None = continue from the previous section).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_number: Option<u32>,
    #[serde(default)]
    pub style: NumberStyle,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub marker: String,
    #[serde(default)]
    pub include_prefix: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(side: PageSide, x: f64) -> Page {
        Page {
            id: PageId(1),
            width: 612.0,
            height: 792.0,
            x,
            margins: Margins { top: 36.0, bottom: 36.0, inside: 54.0, outside: 36.0 },
            columns: Columns { count: 2, gutter: 12.0, positions: None },
            parent: None,
            side,
            overridden: vec![],
            guides: vec![],
            show_parent_items: true,
            liquid: Default::default(),
            transition: None,
            view_rotation: 0,
        }
    }

    #[test]
    fn margins_mirror_on_left_pages() {
        let l = page(PageSide::Left, 0.0);
        let r = page(PageSide::Right, 612.0);
        assert_eq!(l.margin_rect(), Rect::new(36.0, 36.0, 558.0, 756.0));
        assert_eq!(r.margin_rect(), Rect::new(666.0, 36.0, 1188.0, 756.0));
    }

    #[test]
    fn columns_divide_live_area() {
        let p = page(PageSide::Single, 0.0);
        let cols = p.column_rects();
        assert_eq!(cols.len(), 2);
        let total: f64 = cols.iter().map(|c| c.width()).sum::<f64>() + 12.0;
        assert!((total - p.margin_rect().width()).abs() < 1e-9);
        assert!((cols[1].x0 - cols[0].x1 - 12.0).abs() < 1e-9);
    }

    #[test]
    fn spread_spine_and_hit() {
        let s = Spread {
            id: SpreadId(1),
            pages: vec![page(PageSide::Left, 0.0), page(PageSide::Right, 612.0)],
            items: vec![],
            parent: None,
            allow_shuffle: true,
        };
        assert_eq!(s.spine_x(), 612.0);
        assert_eq!(s.page_at_x(700.0), Some(1));
        assert_eq!(s.page_at_x(-50.0), Some(0));
        assert_eq!(s.page_at_x(5000.0), Some(1));
    }
}
