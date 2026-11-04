//! Where spreads sit on the canvas (the pasteboard): stacked vertically, spines aligned at x = 0,
//! parents shown on their own canvas when a parent spread is being edited.

use designcraft_doc::{Document, Selection, SpreadRef};
use designcraft_geom::{Affine, Point, Rect, Vec2};
use serde::Serialize;
use serde_json::{Value, json};

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
            slots.push(SpreadSlot { spread: r, xf, rotation, bounds: cb });
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

    /// The Page tool selection on the canvas currently shown.
    pub fn selected_page(&self, doc: &Document, selection: &Selection) -> Option<(SpreadRef, usize)> {
        let (sr, pi) = if self.slots.first().is_some_and(|s| matches!(s.spread, SpreadRef::Parent(_))) {
            let (id, pi) = selection.parent_page?;
            (SpreadRef::Parent(doc.parent_index(id)?), pi)
        } else {
            let (si, pi) = doc.page_loc(selection.page?)?;
            (SpreadRef::Doc(si), pi)
        };
        self.slot(sr)?;
        doc.spread(sr)?.pages.get(pi)?;
        Some((sr, pi))
    }

    /// A page on the visible canvas, including parent pages.
    pub fn page_at(&self, doc: &Document, p: Point) -> Option<(SpreadRef, usize)> {
        self.slots
            .iter()
            .find_map(|slot| doc.spread(slot.spread)?.pages.iter().position(|pg| pg.bounds().contains(slot.to_spread(p))).map(|pi| (slot.spread, pi)))
    }

    pub fn spread_page_rect(&self, doc: &Document, sr: SpreadRef, pi: usize) -> Option<Rect> {
        Some(self.slot(sr)?.xf.transform_rect_bbox(doc.spread(sr)?.pages.get(pi)?.bounds()))
    }

    /// Canvas rect of a document page.
    pub fn page_rect(&self, doc: &Document, abs: usize) -> Option<Rect> {
        let (si, pi) = doc.page_loc(abs)?;
        Some(self.xf(SpreadRef::Doc(si)).transform_rect_bbox(doc.spreads[si].pages[pi].bounds()))
    }
}

/// The same resize contract for panels, handles and the exact-size dialog.
pub fn page_size_command(doc: &Document, sr: SpreadRef, page: usize, mut params: Value) -> Option<(&'static str, Value)> {
    let sp = doc.spread(sr)?;
    sp.pages.get(page)?;
    let command = match sr {
        SpreadRef::Doc(si) => {
            params["pages"] = json!([doc.first_page_of_spread(si) + page + 1]);
            "layout.pageSize"
        }
        SpreadRef::Parent(_) => {
            params["parent"] = json!(sp.id.0);
            params["page"] = json!(page);
            "layout.parentPageSize"
        }
    };
    Some((command, params))
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
