//! Flex Layout (InDesign 2026): a frame whose objects are placed in rows or columns by direction,
//! wrap, justification, alignment, padding and spacing, like CSS Flexbox. The container is a frame
//! holding its objects as nested items (as Edit › Paste Into does), so it draws its fill, clips its
//! objects and strokes like any frame; [`layout`] moves the objects to where the settings put them.

use designcraft_geom::{Affine, Rect, Vec2, shapes};
use serde::{Deserialize, Serialize};

use crate::{Content, Item};

/// The main axis and the way along it (Flex Layout panel › Direction).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlexDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl FlexDirection {
    pub fn is_row(self) -> bool {
        matches!(self, FlexDirection::Row | FlexDirection::RowReverse)
    }
    pub fn is_reverse(self) -> bool {
        matches!(self, FlexDirection::RowReverse | FlexDirection::ColumnReverse)
    }
}

/// How the objects of a row (or column) share its length (Flex Layout panel › Justification).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlexJustify {
    #[default]
    FlexStart,
    Center,
    FlexEnd,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

impl FlexJustify {
    /// The space modes work the spacing out themselves (the panel shows it, greyed).
    pub fn distributes(self) -> bool {
        matches!(self, FlexJustify::SpaceBetween | FlexJustify::SpaceAround | FlexJustify::SpaceEvenly)
    }
}

/// Where rows sit across the container, and objects across their row (the alignment grid's other
/// axis).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlexAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Width or height of a container: Fixed, or Auto (it grows and shrinks with its objects).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlexSize {
    #[default]
    Fixed,
    Auto,
}

/// A Flex Layout container's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FlexLayout {
    pub direction: FlexDirection,
    /// Wrap: objects that don't fit start a new row (column).
    pub wrap: bool,
    pub justify: FlexJustify,
    pub align: FlexAlign,
    pub width: FlexSize,
    pub height: FlexSize,
    /// Padding inside the container: top, left, bottom, right.
    pub padding: [f64; 4],
    /// Spacing between objects horizontally and vertically.
    pub gap_h: f64,
    pub gap_v: f64,
}

impl Default for FlexLayout {
    fn default() -> Self {
        FlexLayout {
            direction: FlexDirection::Row,
            wrap: false,
            justify: FlexJustify::FlexStart,
            align: FlexAlign::Start,
            width: FlexSize::Fixed,
            height: FlexSize::Fixed,
            padding: [0.0; 4],
            gap_h: 0.0,
            gap_v: 0.0,
        }
    }
}

impl FlexLayout {
    /// Spacing along the main axis and across it.
    pub fn gaps(&self) -> (f64, f64) {
        if self.direction.is_row() { (self.gap_h, self.gap_v) } else { (self.gap_v, self.gap_h) }
    }
}

/// Where [`arrange`] puts things: the container's rectangle and each object's offset, in the
/// container's inner space.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrangement {
    pub rect: Rect,
    pub moves: Vec<Vec2>,
    /// The spacing the main axis ends up with between the first row's objects (what the panel shows
    /// for a space mode).
    pub main_gap: f64,
}

/// Lay out objects of the given bounds in `rect` (both in the container's inner space).
pub fn arrange(f: &FlexLayout, rect: Rect, bounds: &[Rect]) -> Arrangement {
    let row = f.direction.is_row();
    let fin = |v: f64| if v.is_finite() { v.max(0.0) } else { 0.0 };
    let [pt, pl, pb, pr] = f.padding.map(fin);
    let (gap_main, gap_cross) = f.gaps();
    let (gap_main, gap_cross) = (fin(gap_main), fin(gap_cross));
    let size = |b: &Rect| if row { (b.width(), b.height()) } else { (b.height(), b.width()) };
    let (main_auto, cross_auto) =
        if row { (f.width == FlexSize::Auto, f.height == FlexSize::Auto) } else { (f.height == FlexSize::Auto, f.width == FlexSize::Auto) };
    let (pad_main_start, pad_main_end, pad_cross_start, pad_cross_end) = if row { (pl, pr, pt, pb) } else { (pt, pb, pl, pr) };
    let (rect_main, rect_cross) = if row { (rect.width(), rect.height()) } else { (rect.height(), rect.width()) };
    let mut main_len = (rect_main - pad_main_start - pad_main_end).max(0.0);
    // Rows: the objects in each, broken where the next one wouldn't fit (Wrap on a fixed length).
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (i, b) in bounds.iter().enumerate() {
        let (m, _) = size(b);
        let breaks = f.wrap && !main_auto && lines.last().is_some_and(|l| !l.is_empty() && used + gap_main + m > main_len + 1e-6);
        match lines.last_mut() {
            Some(l) if !breaks => {
                used += if l.is_empty() { m } else { gap_main + m };
                l.push(i);
            }
            _ => {
                lines.push(vec![i]);
                used = m;
            }
        }
    }
    let line_main = |l: &[usize]| l.iter().map(|&i| size(&bounds[i]).0).sum::<f64>() + gap_main * l.len().saturating_sub(1) as f64;
    let line_cross = |l: &[usize]| l.iter().map(|&i| size(&bounds[i]).1).fold(0.0, f64::max);
    if main_auto {
        main_len = lines.iter().map(|l| line_main(l)).fold(0.0, f64::max);
    }
    let content_cross = lines.iter().map(|l| line_cross(l)).sum::<f64>() + gap_cross * lines.len().saturating_sub(1) as f64;
    let cross_len = if cross_auto { content_cross } else { (rect_cross - pad_cross_start - pad_cross_end).max(0.0) };
    // The container: Auto sides fit the objects, from its top left.
    let (w, h) = if row {
        (main_len + pad_main_start + pad_main_end, cross_len + pad_cross_start + pad_cross_end)
    } else {
        (cross_len + pad_cross_start + pad_cross_end, main_len + pad_main_start + pad_main_end)
    };
    let out_rect = Rect::new(
        rect.x0,
        rect.y0,
        rect.x0 + if (row && main_auto) || (!row && cross_auto) { w } else { rect.width() },
        rect.y0 + if (!row && main_auto) || (row && cross_auto) { h } else { rect.height() },
    );
    let shift = |free: f64| match f.align {
        FlexAlign::Start => 0.0,
        FlexAlign::Center => free / 2.0,
        FlexAlign::End => free,
    };
    let mut moves = vec![Vec2::ZERO; bounds.len()];
    let mut cross_at = shift(cross_len - content_cross);
    let mut main_gap = gap_main;
    for (li, l) in lines.iter().enumerate() {
        let lc = line_cross(l);
        let free = main_len - line_main(l);
        let n = l.len() as f64;
        let (start, gap) = match f.justify {
            _ if free <= 0.0 => (0.0, gap_main),
            FlexJustify::FlexStart => (0.0, gap_main),
            FlexJustify::Center => (free / 2.0, gap_main),
            FlexJustify::FlexEnd => (free, gap_main),
            FlexJustify::SpaceBetween if l.len() > 1 => (0.0, gap_main + free / (n - 1.0)),
            FlexJustify::SpaceBetween => (0.0, gap_main),
            FlexJustify::SpaceAround => (free / (2.0 * n), gap_main + free / n),
            FlexJustify::SpaceEvenly => (free / (n + 1.0), gap_main + free / (n + 1.0)),
        };
        if li == 0 {
            main_gap = gap;
        }
        let mut at = start;
        for &i in l {
            let (m, c) = size(&bounds[i]);
            // Reverse directions run from the end.
            let main = if f.direction.is_reverse() { main_len - at - m } else { at };
            let cross = cross_at + shift(lc - c);
            let (x, y) = if row {
                (out_rect.x0 + pad_main_start + main, out_rect.y0 + pad_cross_start + cross)
            } else {
                (out_rect.x0 + pad_cross_start + cross, out_rect.y0 + pad_main_start + main)
            };
            moves[i] = Vec2::new(x - bounds[i].x0, y - bounds[i].y0);
            at += m + gap;
        }
        cross_at += lc + gap_cross;
    }
    Arrangement { rect: out_rect, moves, main_gap }
}

/// Lay out a container's objects (and size it, for Auto sides). Returns whether anything moved.
pub fn layout(it: &mut Item) -> bool {
    let Some(f) = it.flex.clone() else { return false };
    let Content::Group { items } = &it.content else { return false };
    let rect = it.inner_bounds();
    let bounds: Vec<Rect> = items.iter().map(|c| c.bounds()).collect();
    let a = arrange(&f, rect, &bounds);
    let mut changed = false;
    if (a.rect.width() - rect.width()).abs() > 1e-6 || (a.rect.height() - rect.height()).abs() > 1e-6 {
        it.path = shapes::rectangle(a.rect);
        changed = true;
    }
    if let Content::Group { items } = &mut it.content {
        for (c, d) in items.iter_mut().zip(&a.moves) {
            if d.hypot() > 1e-6 {
                let c = std::sync::Arc::make_mut(c);
                c.xf = Affine::translate(*d) * c.xf;
                changed = true;
            }
        }
    }
    changed
}

/// Lay out every container among `items` and the objects nested in them, inner containers first
/// (an Auto container's size is its parent's object size). Only changed items are copied.
pub fn layout_all(items: &mut [std::sync::Arc<Item>]) {
    for a in items.iter_mut() {
        if !contains_flex(a) {
            continue;
        }
        let it = std::sync::Arc::make_mut(a);
        if let Content::Group { items } = &mut it.content {
            layout_all(items);
        }
        layout(it);
    }
}

/// Is `it` a container, or does it hold one?
pub fn contains_flex(it: &Item) -> bool {
    it.flex.is_some() || matches!(&it.content, Content::Group { items } if items.iter().any(|c| contains_flex(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f64, y0: f64, w: f64, h: f64) -> Rect {
        Rect::new(x0, y0, x0 + w, y0 + h)
    }

    fn placed(f: &FlexLayout, rect: Rect, bounds: &[Rect]) -> (Rect, Vec<(f64, f64)>) {
        let a = arrange(f, rect, bounds);
        (a.rect, bounds.iter().zip(&a.moves).map(|(b, m)| ((b.x0 + m.x).round(), (b.y0 + m.y).round())).collect())
    }

    #[test]
    fn a_row_justifies_and_aligns() {
        let boxes = [r(500.0, 500.0, 100.0, 50.0), r(0.0, 0.0, 100.0, 80.0), r(0.0, 0.0, 100.0, 50.0)];
        let mut f = FlexLayout { gap_h: 10.0, ..Default::default() };
        let rect = r(0.0, 0.0, 400.0, 100.0);
        assert_eq!(placed(&f, rect, &boxes).1, [(0.0, 0.0), (110.0, 0.0), (220.0, 0.0)]);
        f.justify = FlexJustify::FlexEnd;
        assert_eq!(placed(&f, rect, &boxes).1[0], (80.0, 0.0));
        f.justify = FlexJustify::Center;
        assert_eq!(placed(&f, rect, &boxes).1[0], (40.0, 0.0));
        f.justify = FlexJustify::SpaceBetween;
        assert_eq!(placed(&f, rect, &boxes).1, [(0.0, 0.0), (150.0, 0.0), (300.0, 0.0)]);
        assert_eq!(arrange(&f, rect, &boxes).main_gap, 50.0);
        f.justify = FlexJustify::SpaceEvenly;
        assert_eq!(placed(&f, rect, &boxes).1[0], (20.0, 0.0));
        f.justify = FlexJustify::SpaceAround;
        assert_eq!(placed(&f, rect, &boxes).1[0], (13.0, 0.0));
        // Across: the 80 tall row; the 50 tall boxes centre or end in it, and the row in the container.
        f.justify = FlexJustify::FlexStart;
        f.align = FlexAlign::Center;
        assert_eq!(placed(&f, rect, &boxes).1, [(0.0, 25.0), (110.0, 10.0), (220.0, 25.0)]);
        f.align = FlexAlign::End;
        assert_eq!(placed(&f, rect, &boxes).1[1], (110.0, 20.0));
        f.direction = FlexDirection::RowReverse;
        f.align = FlexAlign::Start;
        assert_eq!(placed(&f, rect, &boxes).1, [(300.0, 0.0), (190.0, 0.0), (80.0, 0.0)]);
    }

    /// The video's second layout: a column that wraps into two, 56 apart, 38 between objects,
    /// centred in the container both ways.
    #[test]
    fn a_wrapped_column_centres() {
        let boxes = [r(0.0, 0.0, 300.0, 400.0); 4];
        let f = FlexLayout {
            direction: FlexDirection::Column,
            wrap: true,
            justify: FlexJustify::Center,
            align: FlexAlign::Center,
            gap_h: 56.0,
            gap_v: 38.0,
            ..Default::default()
        };
        let (rect, at) = placed(&f, r(0.0, 0.0, 868.0, 912.0), &boxes);
        assert_eq!(rect, r(0.0, 0.0, 868.0, 912.0));
        // Each column 400 + 38 + 400 = 838 tall, centred in 912; the two 300 wide columns and the
        // 56 between them (656) centred in 868.
        assert_eq!(at, [(106.0, 37.0), (106.0, 475.0), (462.0, 37.0), (462.0, 475.0)]);
    }

    #[test]
    fn auto_sides_fit_the_objects_and_padding() {
        let boxes = [r(0.0, 0.0, 100.0, 50.0), r(0.0, 0.0, 60.0, 70.0)];
        let f = FlexLayout { width: FlexSize::Auto, height: FlexSize::Auto, gap_h: 20.0, padding: [5.0, 10.0, 15.0, 30.0], ..Default::default() };
        let (rect, at) = placed(&f, r(40.0, 40.0, 10.0, 10.0), &boxes);
        assert_eq!(rect, Rect::new(40.0, 40.0, 40.0 + 10.0 + 180.0 + 30.0, 40.0 + 5.0 + 70.0 + 15.0));
        assert_eq!(at, [(50.0, 45.0), (170.0, 45.0)]);
        // Auto ignores Wrap: one row.
        let wrapped = FlexLayout { wrap: true, ..f };
        assert_eq!(placed(&wrapped, r(40.0, 40.0, 10.0, 10.0), &boxes).1, at);
    }

    #[test]
    fn hostile_numbers_dont_break_the_layout() {
        let f = FlexLayout { gap_h: f64::NAN, padding: [f64::INFINITY, -5.0, 0.0, 0.0], wrap: true, ..Default::default() };
        let a = arrange(&f, r(0.0, 0.0, 10.0, 10.0), &[r(0.0, 0.0, 50.0, 50.0), r(0.0, 0.0, 50.0, 50.0)]);
        assert!(a.moves.iter().all(|m| m.x.is_finite() && m.y.is_finite()));
        assert!(arrange(&f, r(0.0, 0.0, 10.0, 10.0), &[]).moves.is_empty());
    }
}
