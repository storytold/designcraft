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

/// The most columns a page can have, as in InDesign.
pub const MAX_COLUMNS: u32 = 216;

/// How far, in points, the first and last custom column positions may sit from the margin box's
/// edges: room for float noise in files and page arithmetic, far less than any dragged guide.
pub const COLUMN_POSITION_TOLERANCE: f64 = 0.01;

/// Whether `positions` are usable custom column positions for `count` columns across a margin box
/// `margin_width` wide, in IDML's `ColumnsPositions` form: each column's left and right edge as an
/// offset from the margin box's left edge, so `2 × count` finite numbers, ascending across the
/// whole list, the first at 0 and the last at `margin_width` (within
/// [`COLUMN_POSITION_TOLERANCE`]).
pub fn valid_column_positions(positions: &[f64], count: u32, margin_width: f64) -> bool {
    (1..=MAX_COLUMNS).contains(&count)
        && positions.len() == 2 * count as usize
        && margin_width.is_finite()
        && positions.iter().all(|v| v.is_finite())
        && positions.windows(2).all(|w| matches!(w, [a, b] if a <= b))
        && positions.first().is_some_and(|v| v.abs() <= COLUMN_POSITION_TOLERANCE)
        && positions.last().is_some_and(|v| (v - margin_width).abs() <= COLUMN_POSITION_TOLERANCE)
}

impl Columns {
    /// The custom positions when they are valid for a margin box `margin_width` wide (see
    /// [`valid_column_positions`]). Anything else means the columns are evenly spaced: a stored set
    /// that no longer fits (a page resized after the drag, a hostile file) is not drawn.
    pub fn valid_positions(&self, margin_width: f64) -> Option<&[f64]> {
        let positions = self.positions.as_deref()?;
        valid_column_positions(positions, self.count, margin_width).then_some(positions)
    }

    /// Every column's left and right edge as offsets from the margin box's left edge: the valid
    /// custom positions, else the even split. This is what IDML writes as `ColumnsPositions`.
    pub fn edges(&self, margin_width: f64) -> Vec<f64> {
        let w = if margin_width.is_finite() { margin_width.max(0.0) } else { 0.0 };
        match self.valid_positions(w) {
            Some(p) => p.iter().map(|&v| inside(v, w)).collect(),
            None => column_rects(Rect::new(0.0, 0.0, w, 0.0), self.count.max(1), self.gutter).iter().flat_map(|r| [r.x0, r.x1]).collect(),
        }
    }

    /// The column edges (as [`Columns::edges`]) with gutter `gutter` (0 is the one between the first
    /// two columns) moved so its left edge sits at `new_left`, an offset from the margin box's left
    /// edge. Both of the gutter's guides move, so it keeps its width, and it stops at its
    /// neighbouring gutters or the margins: no column gets a negative width. `None` when there is
    /// no such gutter, `new_left` is not finite, or the result would not be valid.
    pub fn with_gutter_moved(&self, margin_width: f64, gutter: usize, new_left: f64) -> Option<Vec<f64>> {
        if !new_left.is_finite() {
            return None;
        }
        let mut pos = self.edges(margin_width);
        let i = gutter.checked_mul(2)?.checked_add(1)?;
        let &[lo, left, right, hi] = pos.get(i - 1..i.checked_add(3)?)? else {
            return None;
        };
        let width = right - left;
        let at = new_left.min(hi - width).max(lo);
        let [l, r] = pos.get_mut(i..i + 2)? else {
            return None;
        };
        *l = at;
        *r = (at + width).min(hi);
        valid_column_positions(&pos, self.count, margin_width).then_some(pos)
    }
}

/// `v` limited to `0..=w` without `clamp`, which panics when `w` is NaN.
fn inside(v: f64, w: f64) -> f64 {
    v.max(0.0).min(w)
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
    /// Margins and columns set on this page itself. Without them a document page takes its
    /// parent page's margins and columns and follows their changes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_margins: bool,
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
    /// Page tool › Show Parent Page Overlay: the parent page's size drawn over this page.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_parent_overlay: bool,
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
    /// The rule of the page's parent page (InDesign's "Controlled by Parent"; Off without one).
    ControlledByParent,
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
    /// Same margins and columns as `other`.
    pub fn same_margins(&self, other: &Page) -> bool {
        self.margins == other.margins && self.columns == other.columns
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
    /// Column rects within the margins: the custom positions when the column guides have been
    /// dragged and still fit the margin box (see [`valid_column_positions`]), otherwise an even
    /// split.
    pub fn column_rects(&self) -> Vec<Rect> {
        let m = self.margin_rect();
        let w = m.width();
        match self.columns.valid_positions(w) {
            Some(p) => p.as_chunks::<2>().0.iter().map(|&[a, b]| Rect::new(m.x0 + inside(a, w), m.y0, m.x0 + inside(b, w), m.y1)).collect(),
            None => column_rects(m, self.columns.count.max(1), self.columns.gutter),
        }
    }
}

/// Split `area` into `n` equal columns separated by `gutter`.
pub fn column_rects(area: Rect, n: u32, gutter: f64) -> Vec<Rect> {
    // The public allocator also receives native/imported options, outside command validation.
    let n = n.clamp(1, MAX_COLUMNS) as usize;
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
    /// The bleed guide: one rectangle around the spread. `bleed` is the document's top, bottom,
    /// inside, outside; the inside value applies only at an outer edge on the spine side (a lone
    /// left page's right edge). The outer edges are the leftmost and rightmost pages, whichever
    /// binding ordered the list. None without pages or without any bleed.
    pub fn bleed_rect(&self, bleed: [f64; 4]) -> Option<Rect> {
        let (l, r) = self.outer_page_indices()?;
        let (first, last) = (self.pages.get(l)?, self.pages.get(r)?);
        if !bleed.iter().any(|v| *v > 0.0) {
            return None;
        }
        let [top, bottom, inside, outside] = bleed;
        let l = if first.side == PageSide::Left { outside } else { inside };
        let r = if matches!(last.side, PageSide::Right | PageSide::Single) { outside } else { inside };
        let b = self.bounds();
        Some(Rect::new(b.x0 - l, b.y0 - top, b.x1 + r, b.y1 + bottom))
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
    /// ordered the list; the first page for a single-sided page or a one-page parent.
    pub fn parent_page_for_side(&self, side: PageSide) -> Option<usize> {
        let (l, r) = self.outer_page_indices()?;
        match side {
            _ if self.pages.len() < 2 => Some(l),
            PageSide::Left => Some(l),
            PageSide::Right => Some(r),
            PageSide::Single => Some(0),
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

/// The highest start page number a document or section accepts.
pub const MAX_PAGE_NUMBER: u32 = 99_999;

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
            local_margins: false,
            parent: None,
            side,
            overridden: vec![],
            guides: vec![],
            show_parent_items: true,
            liquid: Default::default(),
            transition: None,
            view_rotation: 0,
            show_parent_overlay: false,
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

    /// Dragged column guides place the columns; a set that cannot mean anything falls back to the
    /// even split rather than producing nothing or a column outside the margins (#1096).
    #[test]
    fn custom_column_positions_place_the_columns() {
        let mut p = page(PageSide::Single, 0.0);
        let m = p.margin_rect();
        let w = m.width();
        let even = p.column_rects();

        p.columns.positions = Some(vec![0.0, 100.0, 130.0, w]);
        assert_eq!(p.column_rects(), vec![Rect::new(m.x0, m.y0, m.x0 + 100.0, m.y1), Rect::new(m.x0 + 130.0, m.y0, m.x1, m.y1)]);
        // Float noise at the edges is absorbed and kept inside the box.
        p.columns.positions = Some(vec![-0.001, 100.0, 130.0, w + 0.001]);
        assert_eq!((p.column_rects()[0].x0, p.column_rects()[1].x1), (m.x0, m.x1));

        let huge: Vec<f64> = (0..10_000).map(f64::from).collect();
        let unusable = [
            vec![],
            vec![10.0],
            vec![0.0, w],                             // one column's worth for two columns
            vec![0.0, 100.0, 130.0, 200.0, 230.0, w], // three columns' worth
            vec![0.0, 100.0, 90.0, w],                // descending across pairs
            vec![0.0, 130.0, 100.0, w],               // an inverted gutter
            vec![0.0, f64::NAN, 130.0, w],
            vec![0.0, 100.0, f64::INFINITY, w],
            vec![5.0, 100.0, 130.0, w],         // not from the left margin
            vec![0.0, 100.0, 130.0, w + 500.0], // a page narrowed after the drag
            vec![0.0, 100.0, 130.0, w - 50.0],  // a page widened after the drag
            huge,
        ];
        for bad in unusable {
            p.columns.positions = Some(bad.clone());
            assert_eq!(p.column_rects(), even, "{:?}", bad.iter().take(6).collect::<Vec<_>>());
            assert_eq!(p.columns.edges(w), even.iter().flat_map(|r| [r.x0 - m.x0, r.x1 - m.x0]).collect::<Vec<_>>());
        }
        p.columns.positions = None;
        assert_eq!(p.column_rects(), even);

        // A count outside 1..=216 has no valid positions, however many are given.
        for count in [0, 217, u32::MAX] {
            let c = Columns { count, gutter: 0.0, positions: Some(vec![0.0, w]) };
            assert!(c.valid_positions(w).is_none(), "{count}");
        }
    }

    /// Moving a gutter moves both its guides, keeps its width and stops at the neighbouring gutters
    /// and the margins; the result is valid custom positions.
    #[test]
    fn with_gutter_moved_moves_a_gutter_pair_and_clamps() {
        // Three columns across 450 with gutter 15: 0 140 155 295 310 450.
        let c = Columns { count: 3, gutter: 15.0, positions: None };
        let even = c.edges(450.0);
        assert_eq!(even, vec![0.0, 140.0, 155.0, 295.0, 310.0, 450.0]);

        assert_eq!(c.with_gutter_moved(450.0, 0, 100.0), Some(vec![0.0, 100.0, 115.0, 295.0, 310.0, 450.0]));
        // Past the left margin: the first column closes to nothing.
        assert_eq!(c.with_gutter_moved(450.0, 0, -50.0), Some(vec![0.0, 0.0, 15.0, 295.0, 310.0, 450.0]));
        // Past the next gutter: the middle column closes to nothing, the next gutter stays put.
        assert_eq!(c.with_gutter_moved(450.0, 0, 400.0), Some(vec![0.0, 280.0, 295.0, 295.0, 310.0, 450.0]));
        // The last gutter stops at the right margin.
        assert_eq!(c.with_gutter_moved(450.0, 1, 1000.0), Some(vec![0.0, 140.0, 155.0, 435.0, 450.0, 450.0]));

        // It starts from custom positions already there, and its result is accepted as custom.
        let moved = Columns { positions: c.with_gutter_moved(450.0, 0, 100.0), ..c.clone() };
        assert!(moved.valid_positions(450.0).is_some());
        assert_eq!(moved.with_gutter_moved(450.0, 1, 200.0), Some(vec![0.0, 100.0, 115.0, 200.0, 215.0, 450.0]));

        // No such gutter, or no position to move it to.
        assert_eq!(c.with_gutter_moved(450.0, 2, 100.0), None);
        assert_eq!(c.with_gutter_moved(450.0, usize::MAX, 100.0), None);
        assert_eq!(c.with_gutter_moved(450.0, 0, f64::NAN), None);
        assert_eq!(Columns { count: 1, gutter: 12.0, positions: None }.with_gutter_moved(450.0, 0, 100.0), None);
        assert_eq!(c.with_gutter_moved(f64::NAN, 0, 100.0), None);
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

    /// The outside bleed goes on the leftmost and rightmost pages, also when right-to-left
    /// binding lists the right page first.
    #[test]
    fn bleed_rect_uses_the_outer_pages() {
        let bleed = [1.0, 2.0, 3.0, 4.0];
        for pages in [vec![page(PageSide::Left, 0.0), page(PageSide::Right, 612.0)], vec![page(PageSide::Right, 612.0), page(PageSide::Left, 0.0)]] {
            let s = Spread { id: SpreadId(1), pages, items: vec![], parent: None, allow_shuffle: true };
            assert_eq!(s.bleed_rect(bleed), Some(Rect::new(-4.0, -1.0, 1228.0, 794.0)));
            assert_eq!(s.bleed_rect([0.0; 4]), None);
        }
        let lone_left = Spread { id: SpreadId(1), pages: vec![page(PageSide::Left, 0.0)], items: vec![], parent: None, allow_shuffle: true };
        assert_eq!(lone_left.bleed_rect(bleed), Some(Rect::new(-4.0, -1.0, 615.0, 794.0)));
    }
}

#[cfg(test)]
mod column_allocation_safety_tests {
    use super::*;

    #[test]
    fn column_allocation_caps_page_count_and_preserves_geometry() {
        let area = Rect::new(0., 0., 300., 100.);
        // Fail safely at a small boundary before exercising hostile counts.
        assert_eq!(column_rects(area, 217, 0.).len(), 216);
        assert_eq!(column_rects(area, u32::MAX, 0.).len(), 216);
        assert_eq!(column_rects(area, 0, 12.), vec![area]);
        assert_eq!(column_rects(area, 2, 12.), vec![Rect::new(0., 0., 144., 100.), Rect::new(156., 0., 300., 100.)]);
    }
}
