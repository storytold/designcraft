//! Where spreads sit on the canvas (the pasteboard): stacked vertically, spines aligned at x = 0,
//! parents shown on their own canvas when a parent spread is being edited.

use designcraft_doc::{Document, SpreadRef};
use designcraft_geom::{Affine, Point, Rect, Vec2};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SpreadSlot {
    pub spread: SpreadRef,
    /// Spread → canvas: a translation, turned in quarter turns for a rotated spread view.
    #[serde(skip)]
    pub xf: Affine,
    /// Quarter turns clockwise (View › Rotate Spread).
    pub rotation: u8,
    /// Spread page bounds in canvas coordinates.
    pub bounds: Rect,
    /// Canvas area this slot can draw anything in: its pages, the pasteboard around them, and
    /// the outlines of every item an overlay draws for it -- this spread's own and the parent
    /// items shown through its pages, either of which may sit past the pasteboard. This and not
    /// [`Self::bounds`] is what an overlay has to test to know the slot cannot show (#1343).
    /// Not serialised, like `xf`.
    #[serde(skip)]
    pub reach: Rect,
}

impl SpreadSlot {
    pub fn to_canvas(&self, p: Point) -> Point {
        self.xf * p
    }
    pub fn to_spread(&self, p: Point) -> Point {
        self.xf.inverse() * p
    }
    /// A canvas movement as a spread movement.
    pub fn delta_to_spread(&self, d: Vec2) -> Vec2 {
        let m = self.xf.inverse();
        (m * Point::new(d.x, d.y)) - (m * Point::ORIGIN)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CanvasLayout {
    pub slots: Vec<SpreadSlot>,
    /// Pasteboard extent (canvas coordinates).
    pub pasteboard: Rect,
}

/// Vertical gap between spreads (points).
pub const SPREAD_GAP: f64 = 72.0;

impl CanvasLayout {
    /// Layout of the document spreads (or the parent spreads when `parents` is true).
    pub fn new(doc: &Document, parents: bool) -> Self {
        let list: Vec<SpreadRef> =
            if parents { (0..doc.parents.len()).map(SpreadRef::Parent).collect() } else { (0..doc.spreads.len()).map(SpreadRef::Doc).collect() };
        let mut slots = Vec::with_capacity(list.len());
        let mut y = 0.0;
        let mut pb: Option<Rect> = None;
        let (px, py) = doc.settings.pasteboard;
        for r in list {
            let Some(sp) = doc.spread(r) else { continue };
            let b = sp.bounds();
            let rotation = sp.pages.first().map_or(0, |p| p.view_rotation % 4);
            // Turned about the spread's centre, then placed: centred where the unturned spread
            // would be, its top at `y`.
            let turn = Affine::rotate_about(rotation as f64 * std::f64::consts::FRAC_PI_2, b.center());
            let tb = turn.transform_rect_bbox(b);
            let cx = b.center().x - sp.spine_x();
            let xf = Affine::translate((cx - tb.center().x, y - tb.y0)) * turn;
            let cb = xf.transform_rect_bbox(b);
            let w = cb.width();
            let pr = Rect::new(cb.x0 - px.max(w), cb.y0 - py * 0.5, cb.x1 + px.max(w), cb.y1 + py * 0.5);
            pb = Some(pb.map_or(pr, |p| p.union(pr)));
            // Everything an overlay may draw for this slot: the pasteboard rectangle, the
            // outlines of this spread's items, and the parent items shown through its pages,
            // which the canvas draws as dotted edges. An item can be dragged -- or placed by a
            // command -- past the pasteboard, and its edges are still drawn there, so a cull
            // that tested the pasteboard rectangle alone would drop them (#1343).
            let mut reach = pr;
            let mut cover = |m: Affine, it: &designcraft_doc::Item| reach = reach.union(m.transform_rect_bbox(it.visible_bounds()));
            for it in &sp.items {
                cover(xf, it);
            }
            for (pi, page) in sp.pages.iter().enumerate() {
                for (ppi, ppg) in doc.shown_parent_pages(r, pi) {
                    let Some(parent) = doc.parents.get(ppi) else { continue };
                    let Some(ppage) = parent.pages.get(ppg) else { continue };
                    let shift = xf * Affine::translate((page.x - ppage.x, 0.0));
                    for it in &parent.items {
                        cover(shift, it);
                    }
                }
            }
            slots.push(SpreadSlot { spread: r, xf, rotation, bounds: cb, reach });
            y += cb.height() + SPREAD_GAP;
        }
        let pasteboard = pb.unwrap_or(Rect::new(-500.0, -500.0, 500.0, 500.0)).inflate(0.0, py * 0.5);
        CanvasLayout { slots, pasteboard }
    }

    /// The spread under (or nearest to) a canvas point, and the point in spread coordinates.
    pub fn spread_at(&self, p: Point) -> Option<(SpreadRef, Point)> {
        let s = self.slots.iter().min_by(|a, b| dist_y(a.bounds, p.y).total_cmp(&dist_y(b.bounds, p.y)))?;
        Some((s.spread, s.to_spread(p)))
    }

    pub fn slot(&self, r: SpreadRef) -> Option<&SpreadSlot> {
        self.slots.iter().find(|s| s.spread == r)
    }

    /// Spread → canvas transform of spread `r`.
    pub fn xf(&self, r: SpreadRef) -> Affine {
        self.slot(r).map(|s| s.xf).unwrap_or(Affine::IDENTITY)
    }

    pub fn to_canvas(&self, r: SpreadRef, p: Point) -> Point {
        self.xf(r) * p
    }

    pub fn to_spread(&self, r: SpreadRef, p: Point) -> Point {
        self.xf(r).inverse() * p
    }

    /// A canvas movement as a movement in spread `r`.
    pub fn delta_to_spread(&self, r: SpreadRef, d: Vec2) -> Vec2 {
        let m = self.xf(r).inverse();
        (m * Point::new(d.x, d.y)) - (m * Point::ORIGIN)
    }

    /// Canvas rect of a document page.
    pub fn page_rect(&self, doc: &Document, abs: usize) -> Option<Rect> {
        let (si, pi) = doc.page_loc(abs)?;
        Some(self.xf(SpreadRef::Doc(si)).transform_rect_bbox(doc.spreads[si].pages[pi].bounds()))
    }
}

fn dist_y(r: Rect, y: f64) -> f64 {
    if y < r.y0 - SPREAD_GAP / 2.0 {
        r.y0 - y
    } else if y > r.y1 + SPREAD_GAP / 2.0 {
        y - r.y1
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::build::NewDocument;

    /// Each slot knows the canvas area it can hold anything in: the pages plus the pasteboard
    /// around them. An overlay culls on this rather than the page bounds, so an item sitting off
    /// the spread is not skipped (#1343).
    #[test]
    fn a_slots_reach_covers_the_pasteboard_around_its_pages() {
        let d = Document::new(&NewDocument { pages: 6, ..Default::default() });
        let l = CanvasLayout::new(&d, false);
        assert!(l.slots.len() >= 3, "{}", l.slots.len());
        let (px, py) = d.settings.pasteboard;
        assert!(px > 0.0 && py > 0.0, "the fixture has a pasteboard: {px} x {py}");
        for s in &l.slots {
            // The pages are inside it, with room on every side.
            assert!(s.reach.x0 < s.bounds.x0 && s.reach.x1 > s.bounds.x1, "{:?} vs {:?}", s.reach, s.bounds);
            assert!(s.reach.y0 < s.bounds.y0 && s.reach.y1 > s.bounds.y1, "{:?} vs {:?}", s.reach, s.bounds);
            // Sideways it reaches at least the pasteboard width, so an item beside the spread is
            // covered however wide the spread is.
            assert!(s.reach.x0 <= s.bounds.x0 - px.min(s.bounds.width()), "{:?}", s.reach);
            // With nothing placed off the pasteboard, that is all it is.
            assert!(l.pasteboard.contains_rect(s.reach), "{:?} outside {:?}", s.reach, l.pasteboard);
        }
        // Slots far apart on the canvas do not reach each other's pages, so culling one keeps the
        // other: the first and last of six pages are three spreads apart.
        let (first, last) = (l.slots[0].reach, l.slots[l.slots.len() - 1].reach);
        assert!(first.y1 < last.y0 || last.y1 < first.y0, "{first:?} {last:?}");
    }

    /// An item can be put past the pasteboard -- dragged there, or placed by a command, which
    /// takes any rectangle -- and the canvas still draws its edges. The reach covers every
    /// outline an overlay draws for the slot, or culling on it would drop them (#1343).
    #[test]
    fn a_slots_reach_covers_an_item_placed_past_the_pasteboard() {
        use designcraft_doc::{Item, ItemId, Shape};
        use designcraft_geom::shapes;

        let mut d = Document::new(&NewDocument { pages: 4, ..Default::default() });
        let layer = d.default_layer();
        let inside = CanvasLayout::new(&d, false);
        let slot = inside.slots[1];

        // Far to the left of the pasteboard and far above the spread, in spread coordinates.
        let far = Rect::new(-9000.0, -4000.0, -8900.0, -3900.0);
        let item = Item::new(ItemId(d.alloc()), layer, Shape::Rectangle, shapes::rectangle(far));
        d.insert_item(slot.spread, item, None).unwrap();

        let after = CanvasLayout::new(&d, false).slots[1];
        let drawn = slot.xf.transform_rect_bbox(far);
        assert!(after.reach.contains_rect(drawn), "the reach covers the item's outline: {:?} vs {drawn:?}", after.reach);
        assert!(!slot.reach.contains_rect(drawn), "which the pasteboard rectangle alone did not: {:?}", slot.reach);
        // The pages and the pasteboard around them are still inside it.
        assert!(after.reach.contains_rect(slot.reach), "{:?} vs {:?}", after.reach, slot.reach);
        // Only this slot grew: the others still cull.
        let others = CanvasLayout::new(&d, false);
        assert_eq!(others.slots[0].reach, inside.slots[0].reach, "another slot's reach is unchanged");
    }

    #[test]
    fn spreads_stack_on_the_spine() {
        let d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let l = CanvasLayout::new(&d, false);
        assert_eq!(l.slots.len(), 2);
        // Page 1 is a right page: it starts at the spine (x = 0).
        assert_eq!(l.slots[0].bounds.x0, 0.0);
        // Spread 2 (pages 2–3) straddles the spine.
        assert_eq!(l.slots[1].bounds.x0, -612.0);
        assert_eq!(l.slots[1].bounds.y0, 792.0 + SPREAD_GAP);
        let (r, p) = l.spread_at(Point::new(-100.0, 900.0)).unwrap();
        assert_eq!(r, SpreadRef::Doc(1));
        assert_eq!(p, Point::new(512.0, 900.0 - 792.0 - SPREAD_GAP));
        assert_eq!(l.page_rect(&d, 2).unwrap().x0, 0.0);
    }
}
