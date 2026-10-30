//! Layout › Resize Book Cover: a flat cover (back, spine, front, left to right, one page) gets
//! wider or narrower by the change in spine width.

use std::sync::Arc;

use designcraft_doc::{Content, Item};
use designcraft_geom::{Affine, Rect};

use super::margins::set_page_width;
use super::{Result, Value, bad, json};

/// How far from a spine edge an item's edge may be and still count as sitting on it, in points.
/// A printer's spine and the one a file was drawn with differ by a fraction of a point.
const EDGE_TOLERANCE: f64 = 0.5;

/// Where an item that spans `x0..x1` goes when the spine band `l..r` changes width by `d`.
/// Wholly left of the band: stays. Wholly right: moves by `d`. Wholly on the band: moves by `d / 2`
/// and keeps its size, except that a plain shape (`fill_spine`) that fills the band grows with it.
/// Otherwise each edge is mapped on its own, so the item stretches. Edges within
/// [`EDGE_TOLERANCE`] of a spine edge count as on it.
pub(crate) fn place(x0: f64, x1: f64, l: f64, r: f64, d: f64, fill_spine: bool) -> (f64, f64) {
    let snap = |x: f64| {
        if (x - l).abs() <= EDGE_TOLERANCE {
            l
        } else if (x - r).abs() <= EDGE_TOLERANCE {
            r
        } else {
            x
        }
    };
    let (s0, s1) = (snap(x0), snap(x1));
    let edge = |x: f64, s: f64| {
        if s <= l {
            x
        } else if s >= r {
            x + d
        } else {
            x + d / 2.0
        }
    };
    if s1 <= l {
        (x0, x1)
    } else if s0 >= r {
        (x0 + d, x1 + d)
    } else if s0 >= l && s1 <= r {
        if fill_spine && s0 == l && s1 == r { (x0, x1 + d) } else { (x0 + d / 2.0, x1 + d / 2.0) }
    } else {
        (edge(x0, s0), edge(x1, s1))
    }
}

/// A transform that only scales and moves, with no flip or turn: the one kind the spine rule can
/// be worked through.
fn axis_aligned(m: Affine) -> bool {
    let [a, b, c, d, _, _] = m.as_coeffs();
    b.abs() < 1e-9 && c.abs() < 1e-9 && a > 1e-9 && d > 1e-9
}

/// A transform that scales, moves, mirrors or turns by quarter turns: its picture still fills a
/// box, so the bounding box tells where it lies.
fn quarter_turned(m: Affine) -> bool {
    let [a, b, c, d, _, _] = m.as_coeffs();
    let (flat, turned) = (b.abs() < 1e-9 && c.abs() < 1e-9, a.abs() < 1e-9 && d.abs() < 1e-9);
    (flat && a.abs() > 1e-9 && d.abs() > 1e-9) || (turned && b.abs() > 1e-9 && c.abs() > 1e-9)
}

/// Does the picture in this frame cover all of it (so it was placed to fill the frame)?
fn picture_fills_frame(it: &Item) -> bool {
    let Content::Graphic(g) = &it.content else { return false };
    if !quarter_turned(g.xf) {
        return false;
    }
    let (pic, frame) = (g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1)), it.inner_bounds());
    pic.x0 <= frame.x0 + EDGE_TOLERANCE
        && pic.x1 >= frame.x1 - EDGE_TOLERANCE
        && pic.y0 <= frame.y0 + EDGE_TOLERANCE
        && pic.y1 >= frame.y1 - EDGE_TOLERANCE
}

/// Stretch `it` from `b` to `to`. A picture that filled its frame is scaled up to fill the new one,
/// so no bare strip is left; any other picture keeps its size and place in the frame.
fn stretch(it: &mut Item, b: Rect, to: Rect) {
    let fills = picture_fills_frame(it);
    crate::cmd::liquid::place_item(it, b, to, false);
    if fills {
        let frame = it.inner_bounds();
        if let Content::Graphic(g) = &mut it.content
            && let Some(xf) = refilled(g, frame)
        {
            g.xf = xf;
        }
    }
}

/// The graphic's transform scaled evenly until its box fills `frame`, with its mirror or turn kept.
/// It is placed by the same crop and alignment as `Graphic::fitted`, which cannot be used here:
/// it builds an upright transform and would drop the mirror or turn.
fn refilled(g: &designcraft_doc::Graphic, frame: Rect) -> Option<Affine> {
    let cur = g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1));
    let [t, l, b, r] = g.crop;
    let area = Rect::new(frame.x0 - l, frame.y0 - t, frame.x1 + r, frame.y1 + b);
    if cur.width() <= 0.0 || cur.height() <= 0.0 || area.width() <= 0.0 || area.height() <= 0.0 {
        return None;
    }
    let k = (area.width() / cur.width()).max(area.height() / cur.height());
    let (fx, fy) = ([0.0, 0.5, 1.0][(g.fit_align % 3) as usize], [0.0, 0.5, 1.0][(g.fit_align / 3).min(2) as usize]);
    let (x, y) = (area.x0 + (area.width() - cur.width() * k) * fx, area.y0 + (area.height() - cur.height() * k) * fy);
    Some(Affine::translate((x, y)) * Affine::scale(k) * Affine::translate((-cur.x0, -cur.y0)) * g.xf)
}

/// The spine rule applied to each object inside a group, not to the group as one box: a group that
/// reaches across the spine would otherwise stretch objects that sit wholly on one panel.
/// `outer` is the transform of the group's parent.
fn place_in_group(g: &mut Item, outer: Affine, l: f64, r: f64, d: f64) {
    let a = outer * g.xf;
    let [sx, _, _, _, tx, _] = a.as_coeffs();
    let Some(kids) = g.children_mut() else { return };
    for kid in kids.iter_mut() {
        let cb = kid.bounds();
        let (x0, x1) = (sx * cb.x0 + tx, sx * cb.x1 + tx);
        let (n0, n1) = place(x0, x1, l, r, d, matches!(kid.content, Content::Unassigned));
        if n0 == x0 && n1 == x1 {
            continue;
        }
        let kid = Arc::make_mut(kid);
        let (lx0, lx1) = ((n0 - tx) / sx, (n1 - tx) / sx);
        if ((n1 - n0) - (x1 - x0)).abs() < 1e-9 {
            kid.xf = Affine::translate((lx0 - cb.x0, 0.0)) * kid.xf;
        } else if kid.is_group() && axis_aligned(a * kid.xf) {
            place_in_group(kid, a, l, r, d);
        } else {
            stretch(kid, cb, Rect::new(lx0, cb.y0, lx1, cb.y1));
        }
    }
}

pub(super) fn cover_resize(s: &mut crate::Session, p: &Value) -> Result<Value> {
    const ID: &str = "layout.coverResize";
    let page = p.get("page").and_then(Value::as_u64).filter(|n| *n >= 1).ok_or_else(|| bad(ID, "`page` required (1-based)"))? as usize;
    let spine = |k: &str| {
        p.get(k)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && (0.0..=crate::cover::MAX_SPINE_PT).contains(v))
            .ok_or_else(|| bad(ID, format!("`{k}` required: 0 to {} pt", crate::cover::MAX_SPINE_PT)))
    };
    let (old_spine, new_spine) = (spine("oldSpine")?, spine("newSpine")?);
    let (si, pi) = s.doc()?.doc.page_loc(page - 1).ok_or_else(|| bad(ID, "no such page"))?;
    let (pages_in_spread, old_width) = {
        let sp = &s.doc()?.doc.spreads[si];
        (sp.pages.len(), sp.pages[pi].width)
    };
    if pages_in_spread != 1 {
        return Err(bad(ID, "Resize Book Cover works on a one-page cover. This page shares its spread with another."));
    }
    if old_spine > old_width {
        return Err(bad(ID, "the old spine is wider than the page"));
    }
    let d = new_spine - old_spine;
    if old_width + d < 1.0 {
        return Err(bad(ID, "the cover would be narrower than 1 pt"));
    }
    if d == 0.0 {
        return Ok(json!({"changed": false, "width": old_width, "spine": new_spine, "moved": 0, "stretched": 0, "unchanged": 0, "locked": 0}));
    }
    s.edit(|doc, _| {
        let sp = Arc::make_mut(&mut doc.spreads[si]);
        let c = sp.pages[pi].x + old_width / 2.0;
        let (l, r) = (c - old_spine / 2.0, c + old_spine / 2.0);
        // Dragged column guides are kept relative to the margin box: map them like items, so
        // the ones that mark the back and front edges stay on them.
        let (margin_left, columns) = (sp.pages[pi].margin_rect().x0, sp.pages[pi].columns.positions.clone());
        let page_x = sp.pages[pi].x;
        set_page_width(&mut sp.pages[pi], old_width + d);
        if let Some(old) = columns {
            let width = sp.pages[pi].margin_rect().width();
            let mut mapped: Vec<f64> = old.iter().map(|p| place(margin_left + p, margin_left + p, l, r, d, false).0 - margin_left).collect();
            if let Some(first) = mapped.first_mut() {
                *first = 0.0;
            }
            if let Some(last) = mapped.last_mut() {
                *last = width;
            }
            sp.pages[pi].columns.positions = Some(mapped);
        }
        // Relayout puts the page back at x = 0, but a cover moved by Layout > Page Size sits
        // elsewhere and its items are placed against that spot: the one page stays where it was.
        sp.relayout();
        sp.pages[pi].x = page_x;
        let (mut moved, mut stretched, mut unchanged, mut locked) = (0u64, 0u64, 0u64, 0u64);
        for it in &mut sp.items {
            let b = it.bounds();
            let (n0, n1) = place(b.x0, b.x1, l, r, d, matches!(it.content, designcraft_doc::Content::Unassigned));
            if n0 == b.x0 && n1 == b.x1 {
                unchanged += 1;
                continue;
            }
            let it = Arc::make_mut(it);
            if it.locked {
                locked += 1;
            }
            if ((n1 - n0) - b.width()).abs() < 1e-9 {
                it.xf = Affine::translate((n0 - b.x0, 0.0)) * it.xf;
                moved += 1;
            } else if it.is_group() && axis_aligned(it.xf) {
                place_in_group(it, Affine::IDENTITY, l, r, d);
                stretched += 1;
            } else {
                stretch(it, b, Rect::new(n0, b.y0, n1, b.y1));
                stretched += 1;
            }
        }
        for g in sp.pages[pi].guides.iter_mut().filter(|g| g.orientation == designcraft_doc::Orientation::Vertical) {
            g.position = place(g.position, g.position, l, r, d, false).0;
        }
        // The spine's two edges get a guide each, unless one is already there, so the new spine
        // can be checked against the artwork by eye.
        let mut guides_added = 0u64;
        for x in [l, r + d] {
            let page = &mut sp.pages[pi];
            if !page.guides.iter().any(|g| g.orientation == designcraft_doc::Orientation::Vertical && (g.position - x).abs() <= EDGE_TOLERANCE) {
                page.guides.push(designcraft_doc::Guide { orientation: designcraft_doc::Orientation::Vertical, position: x, spread: false, locked: false, layer: None, liquid: false });
                guides_added += 1;
            }
        }
        Ok(json!({"changed": true, "width": old_width + d, "spine": new_spine, "moved": moved, "stretched": stretched, "unchanged": unchanged, "locked": locked, "guidesAdded": guides_added}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use designcraft_geom::Rect;

    /// A 400 x 400 one-page document: back, front, a spine label, a full-width background.
    /// The spine is 40 pt wide, centred: 180 to 220.
    fn cover() -> (Session, [u64; 4]) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 400, "height": 400, "facingPages": false})).unwrap();
        let mut ids = [0; 4];
        for (i, r) in [[20, 50, 100, 150], [300, 50, 380, 150], [185, 50, 215, 150], [0, 0, 400, 400]].iter().enumerate() {
            ids[i] = s.execute("frame.create", &json!({"rect": r})).unwrap()["id"].as_u64().unwrap();
        }
        (s, ids)
    }
    fn bounds(s: &Session, id: u64) -> Rect {
        s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds()
    }
    fn near(a: Rect, b: [f64; 4]) {
        let got = [a.x0, a.y0, a.x1, a.y1];
        assert!(got.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6), "{got:?} != {b:?}");
    }
    fn width(s: &Session) -> f64 {
        s.doc().unwrap().doc.spreads[0].pages[0].width
    }

    #[test]
    fn items_are_placed_by_where_they_sit() {
        let (mut s, [back, front, label, bg]) = cover();
        let r = s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        assert_eq!(width(&s), 410.0);
        near(bounds(&s, back), [20.0, 50.0, 100.0, 150.0]);
        near(bounds(&s, front), [310.0, 50.0, 390.0, 150.0]);
        near(bounds(&s, label), [190.0, 50.0, 220.0, 150.0]);
        near(bounds(&s, bg), [0.0, 0.0, 410.0, 400.0]);
        assert_eq!((r["moved"].as_u64(), r["stretched"].as_u64(), r["unchanged"].as_u64()), (Some(2), Some(1), Some(1)));
        assert_eq!(r["width"].as_f64(), Some(410.0));
    }

    #[test]
    fn vertical_guides_follow_the_same_rule() {
        let (mut s, _) = cover();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 300})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 50})).unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        let mut at: Vec<f64> = s.doc().unwrap().doc.spreads[0].pages[0].guides.iter().map(|g| g.position).collect();
        at.sort_by(f64::total_cmp);
        assert_eq!(at, vec![50.0, 180.0, 230.0, 310.0], "the two guides given, moved by the rule, plus the two spine edges");
    }

    fn vertical_guides(s: &Session) -> Vec<f64> {
        let mut at: Vec<f64> = s.doc().unwrap().doc.spreads[0].pages[0]
            .guides
            .iter()
            .filter(|g| g.orientation == designcraft_doc::Orientation::Vertical)
            .map(|g| g.position)
            .collect();
        at.sort_by(f64::total_cmp);
        at
    }

    #[test]
    fn the_two_spine_edges_get_guides_when_the_cover_has_none() {
        let (mut s, _) = cover();
        let r = s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        assert_eq!(vertical_guides(&s), vec![180.0, 230.0]);
        assert_eq!(r["guidesAdded"], 2);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(vertical_guides(&s).is_empty(), "one undo takes the guides away too");
    }

    #[test]
    fn guides_already_on_the_spine_edges_are_not_doubled() {
        let (mut s, _) = cover();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 180})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 220})).unwrap();
        let r = s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        assert_eq!(vertical_guides(&s), vec![180.0, 230.0]);
        assert_eq!(r["guidesAdded"], 0);
        // A second resize finds both in place and adds nothing.
        let r = s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 50, "newSpine": 45})).unwrap();
        assert_eq!((vertical_guides(&s), r["guidesAdded"].as_u64()), (vec![180.0, 225.0], Some(0)));
    }

    #[test]
    fn a_cover_with_no_spine_gets_one_guide() {
        let (mut s, _) = cover();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 0})).unwrap();
        assert_eq!(vertical_guides(&s), vec![180.0]);
    }

    #[test]
    fn shrinking_mirrors_growing() {
        let (mut s, [back, front, label, bg]) = cover();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 30})).unwrap();
        assert_eq!(width(&s), 390.0);
        near(bounds(&s, back), [20.0, 50.0, 100.0, 150.0]);
        near(bounds(&s, front), [290.0, 50.0, 370.0, 150.0]);
        near(bounds(&s, label), [180.0, 50.0, 210.0, 150.0]);
        near(bounds(&s, bg), [0.0, 0.0, 390.0, 400.0]);
    }

    #[test]
    fn no_change_does_nothing_and_says_so() {
        let (mut s, [_, front, ..]) = cover();
        let r = s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 40})).unwrap();
        assert_eq!(r["changed"], false);
        assert_eq!(width(&s), 400.0);
        near(bounds(&s, front), [300.0, 50.0, 380.0, 150.0]);
    }

    #[test]
    fn one_undo_puts_everything_back() {
        let (mut s, [_, front, _, bg]) = cover();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(width(&s), 400.0);
        near(bounds(&s, front), [300.0, 50.0, 380.0, 150.0]);
        near(bounds(&s, bg), [0.0, 0.0, 400.0, 400.0]);
    }

    #[test]
    fn locked_items_move_too_and_are_counted() {
        let (mut s, [_, front, ..]) = cover();
        s.edit(|d, _| {
            let sp = std::sync::Arc::make_mut(&mut d.spreads[0]);
            for it in &mut sp.items {
                if it.id == designcraft_doc::ItemId(front) {
                    std::sync::Arc::make_mut(it).locked = true;
                }
            }
            Ok(Value::Null)
        })
        .unwrap();
        let r = s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        near(bounds(&s, front), [310.0, 50.0, 390.0, 150.0]);
        assert_eq!(r["locked"], 1);
    }

    #[test]
    fn a_missing_page_or_a_two_page_spread_is_refused() {
        let (mut s, _) = cover();
        assert!(s.execute("layout.coverResize", &json!({"page": 2, "oldSpine": 40, "newSpine": 50})).is_err());
        assert!(s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 500, "newSpine": 50})).is_err(), "spine wider than the page");
        assert!(s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40})).is_err(), "newSpine is required");
        assert!(s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": -1, "newSpine": 5})).is_err());
        let mut two = Session::new();
        two.execute("file.new", &json!({"width": 400, "height": 400, "pages": 3, "facingPages": true})).unwrap();
        assert!(two.execute("layout.coverResize", &json!({"page": 2, "oldSpine": 40, "newSpine": 50})).is_err());
        assert_eq!(width(&s), 400.0, "a refused call changes nothing");
    }

    #[test]
    fn the_page_cannot_shrink_to_nothing() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 100, "facingPages": false})).unwrap();
        // 100 - 100 + 0 = 0 pt wide: refused.
        assert!(s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 100, "newSpine": 0})).is_err());
        assert_eq!(width(&s), 100.0);
    }

    #[test]
    fn a_horizontal_line_across_the_spine_survives_the_stretch() {
        let (mut s, _) = cover();
        let line = s.execute("line.create", &json!({"a": [0, 300], "b": [400, 300]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        near(bounds(&s, line), [0.0, 300.0, 410.0, 300.0]);
    }

    #[test]
    fn a_vertical_line_inside_the_stretch_range_is_not_stretched_into_nothing() {
        let (mut s, _) = cover();
        // Crosses the spine edge on the left side only: its x range is a single point, so it moves, not stretches.
        let line = s.execute("line.create", &json!({"a": [300, 0], "b": [300, 400]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        near(bounds(&s, line), [310.0, 0.0, 310.0, 400.0]);
    }

    #[test]
    fn a_plain_block_as_wide_as_the_spine_grows_with_it_but_a_text_frame_stays_centred() {
        let (mut s, _) = cover();
        let block = s.execute("frame.create", &json!({"rect": [180, 0, 220, 400]})).unwrap()["id"].as_u64().unwrap();
        let title =
            s.execute("frame.create", &json!({"rect": [180, 0, 220, 400], "content": "text", "text": "Arabic", "caret": false})).unwrap()["id"]
                .as_u64()
                .unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        near(bounds(&s, block), [180.0, 0.0, 230.0, 400.0]);
        near(bounds(&s, title), [185.0, 0.0, 225.0, 400.0]);
    }

    #[test]
    fn edges_a_hair_off_the_spine_still_count_as_on_it() {
        // The printer's spine is 40.14 pt but the file's panels end at 180 and 220: 0.07 pt apart.
        let (mut s, [back, front, ..]) = cover();
        let back_bg = s.execute("frame.create", &json!({"rect": [0, 0, 180, 400]})).unwrap()["id"].as_u64().unwrap();
        let front_bg = s.execute("frame.create", &json!({"rect": [220, 0, 400, 400]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40.14, "newSpine": 50})).unwrap();
        near(bounds(&s, back_bg), [0.0, 0.0, 180.0, 400.0]);
        near(bounds(&s, front_bg), [229.86, 0.0, 409.86, 400.0]);
        near(bounds(&s, back), [20.0, 50.0, 100.0, 150.0]);
        let _ = front;
    }

    #[test]
    fn dragged_column_guides_follow_the_spine_rule_and_the_ends_stay_on_the_margins() {
        let (mut s, _) = cover();
        let m = s.doc().unwrap().doc.spreads[0].pages[0].margin_rect();
        s.edit(|d, _| {
            let pg = &mut std::sync::Arc::make_mut(&mut d.spreads[0]).pages[0];
            // Two columns dragged to end at 100 and start at 300, with the margins at the ends.
            pg.columns = designcraft_doc::Columns { count: 2, gutter: 12.0, positions: Some(vec![0.0, 100.0 - m.x0, 300.0 - m.x0, m.width()]) };
            pg.local_margins = true;
            Ok(Value::Null)
        })
        .unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        let pg = s.doc().unwrap().doc.spreads[0].pages[0].clone();
        let got = pg.columns.positions.clone().unwrap();
        let want = [0.0, 100.0 - m.x0, 310.0 - m.x0, pg.margin_rect().width()];
        assert!(got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6), "{got:?} != {want:?}");
    }

    #[test]
    fn undo_restores_every_item_guide_and_the_page() {
        let (mut s, ids) = cover();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 300})).unwrap();
        let snap = |s: &Session| {
            let sp = &s.doc().unwrap().doc.spreads[0];
            (
                sp.pages[0].width,
                sp.items.iter().map(|it| it.bounds()).collect::<Vec<_>>(),
                sp.pages[0].guides.iter().map(|g| g.position).collect::<Vec<_>>(),
            )
        };
        let before = snap(&s);
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 55})).unwrap();
        assert_ne!(snap(&s), before);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(snap(&s), before);
        let _ = ids;
    }

    #[test]
    fn a_cover_that_sits_away_from_zero_stays_where_it_is() {
        // Layout > Page Size moves a lone left page to a negative x. The resize must not snap it to 0.
        let (mut s, [back, front, label, bg]) = cover();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 300})).unwrap();
        s.edit(|d, _| {
            let sp = std::sync::Arc::make_mut(&mut d.spreads[0]);
            sp.pages[0].x = -250.0;
            for it in &mut sp.items {
                let it = std::sync::Arc::make_mut(it);
                it.xf = designcraft_geom::Affine::translate((-250.0, 0.0)) * it.xf;
            }
            for g in sp.pages[0].guides.iter_mut() {
                g.position -= 250.0;
            }
            Ok(Value::Null)
        })
        .unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].x, -250.0, "the page did not jump");
        near(bounds(&s, back), [-230.0, 50.0, -150.0, 150.0]);
        near(bounds(&s, front), [60.0, 50.0, 140.0, 150.0]);
        near(bounds(&s, label), [-60.0, 50.0, -30.0, 150.0]);
        near(bounds(&s, bg), [-250.0, 0.0, 160.0, 400.0]);
        assert_eq!(s.doc().unwrap().doc.spreads[0].pages[0].guides[0].position, 60.0);
        assert_eq!(vertical_guides(&s), vec![-70.0, -20.0, 60.0], "spine guides follow the page's place");
    }

    fn set_graphic(s: &mut Session, id: u64, size: (f64, f64)) {
        set_graphic_xf(s, id, size, designcraft_geom::Affine::IDENTITY);
    }

    fn set_graphic_xf(s: &mut Session, id: u64, size: (f64, f64), xf: designcraft_geom::Affine) {
        s.edit(|d, _| {
            for it in &mut std::sync::Arc::make_mut(&mut d.spreads[0]).items {
                if it.id == designcraft_doc::ItemId(id) {
                    let g = designcraft_doc::Graphic {
                        layer_options: None,
                        asset: designcraft_doc::AssetId(1),
                        size,
                        xf,
                        clip: None,
                        auto_fit: designcraft_doc::Fitting::None,
                        fit_align: 4,
                        crop: [0.0; 4],
                        wrap: Default::default(),
                        fill: Default::default(),
                    };
                    std::sync::Arc::make_mut(it).content = designcraft_doc::Content::Graphic(g);
                }
            }
            Ok(Value::Null)
        })
        .unwrap();
    }

    fn picture_box(s: &Session, id: u64) -> (Rect, Rect) {
        let it = s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap();
        let designcraft_doc::Content::Graphic(g) = &it.content else { panic!("not a picture") };
        (g.xf.transform_rect_bbox(Rect::new(0.0, 0.0, g.size.0, g.size.1)), it.inner_bounds())
    }

    #[test]
    fn a_picture_that_filled_a_stretched_frame_still_fills_it() {
        let (mut s, [.., bg]) = cover();
        set_graphic(&mut s, bg, (400.0, 400.0));
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        let (pic, frame) = picture_box(&s, bg);
        assert!((frame.width() - 410.0).abs() < 1e-6, "{frame:?}");
        let covers = pic.x0 <= frame.x0 + 1e-6 && pic.x1 >= frame.x1 - 1e-6 && pic.y0 <= frame.y0 + 1e-6 && pic.y1 >= frame.y1 - 1e-6;
        assert!(covers, "the picture {pic:?} leaves bare frame {frame:?}");
    }

    #[test]
    fn a_flipped_or_turned_picture_that_filled_a_stretched_frame_still_fills_it_and_keeps_its_turn() {
        use designcraft_geom::Affine;
        let flips: [(&str, Affine); 3] = [
            ("mirrored left to right", Affine::new([-1.0, 0.0, 0.0, 1.0, 400.0, 0.0])),
            ("upside down", Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, 400.0])),
            ("turned a quarter", Affine::new([0.0, 1.0, -1.0, 0.0, 400.0, 0.0])),
        ];
        for (what, xf) in flips {
            let (mut s, [.., bg]) = cover();
            set_graphic_xf(&mut s, bg, (400.0, 400.0), xf);
            s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
            let (pic, frame) = picture_box(&s, bg);
            let covers = pic.x0 <= frame.x0 + 1e-6 && pic.x1 >= frame.x1 - 1e-6 && pic.y0 <= frame.y0 + 1e-6 && pic.y1 >= frame.y1 - 1e-6;
            assert!(covers, "{what}: the picture {pic:?} leaves bare frame {frame:?}");
            let it = s.doc().unwrap().doc.item(designcraft_doc::ItemId(bg)).unwrap();
            let designcraft_doc::Content::Graphic(g) = &it.content else { panic!("not a picture") };
            let [a, b, c, d, ..] = g.xf.as_coeffs();
            let [oa, ob, oc, od, ..] = xf.as_coeffs();
            let same_turn = |n: f64, o: f64| n.signum() == o.signum() || (n.abs() < 1e-9 && o.abs() < 1e-9);
            assert!(same_turn(a, oa) && same_turn(b, ob) && same_turn(c, oc) && same_turn(d, od), "{what}: the flip or turn was lost: {:?}", g.xf);
        }
    }

    #[test]
    fn a_picture_that_was_placed_small_in_a_stretched_frame_keeps_its_place() {
        let (mut s, [.., bg]) = cover();
        set_graphic(&mut s, bg, (100.0, 100.0));
        let before = picture_box(&s, bg).0;
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        assert_eq!(picture_box(&s, bg).0, before, "a picture that never filled the frame is left alone");
    }

    #[test]
    fn a_group_across_the_spine_is_placed_object_by_object() {
        let (mut s, [..]) = cover();
        let a = s.execute("frame.create", &json!({"rect": [100, 50, 300, 150]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [20, 200, 100, 300]})).unwrap()["id"].as_u64().unwrap();
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        let sp = &s.doc().unwrap().doc.spreads[0];
        let group = sp.items.iter().find(|it| it.id == designcraft_doc::ItemId(g)).unwrap();
        let kids: Vec<Rect> = group.children().iter().map(|c| c.bounds()).collect();
        near(kids[0], [100.0, 50.0, 310.0, 150.0]);
        near(kids[1], [20.0, 200.0, 100.0, 300.0]);
        assert!(r["stretched"].as_u64() >= Some(1));
    }

    #[test]
    fn a_group_on_the_front_panel_still_moves_whole() {
        let (mut s, _) = cover();
        let a = s.execute("frame.create", &json!({"rect": [260, 50, 300, 150]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [320, 50, 360, 150]})).unwrap()["id"].as_u64().unwrap();
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        s.execute("layout.coverResize", &json!({"page": 1, "oldSpine": 40, "newSpine": 50})).unwrap();
        near(bounds(&s, g), [270.0, 50.0, 370.0, 150.0]);
    }

    #[test]
    fn place_maps_each_case() {
        // Spine band 180 to 220, spine change +10.
        assert_eq!(place(20.0, 100.0, 180.0, 220.0, 10.0, false), (20.0, 100.0));
        assert_eq!(place(300.0, 380.0, 180.0, 220.0, 10.0, false), (310.0, 390.0));
        assert_eq!(place(185.0, 215.0, 180.0, 220.0, 10.0, false), (190.0, 220.0));
        assert_eq!(place(0.0, 400.0, 180.0, 220.0, 10.0, false), (0.0, 410.0));
        // Ends inside the band move by half.
        assert_eq!(place(100.0, 200.0, 180.0, 220.0, 10.0, false), (100.0, 205.0));
        assert_eq!(place(200.0, 300.0, 180.0, 220.0, 10.0, false), (205.0, 310.0));
        // A plain shape that fills the band grows with it; anything else on the band stays centred.
        assert_eq!(place(180.0, 220.0, 180.0, 220.0, 10.0, true), (180.0, 230.0));
        assert_eq!(place(180.0, 220.0, 180.0, 220.0, 10.0, false), (185.0, 225.0));
        // Edges within half a point of the band edge count as on it.
        assert_eq!(place(0.0, 180.3, 180.0, 220.0, 10.0, false), (0.0, 180.3));
        assert_eq!(place(219.7, 400.0, 180.0, 220.0, 10.0, false), (229.7, 410.0));
    }
}
