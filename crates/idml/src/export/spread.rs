use super::common::{bool_s, uid};
use super::{Affine, Document, El, Ex, PageSide, Spread, names, num};

/// The IDML spread origin in our spread coordinates.
pub(crate) fn spread_origin(sp: &Spread, facing: bool) -> (f64, f64) {
    let h = sp.pages.iter().map(|p| p.height).fold(0.0, f64::max);
    let b = sp.bounds();
    let ox = if facing {
        if let Some(r) = sp.pages.iter().find(|p| p.side == PageSide::Right) {
            r.x
        } else if let Some(l) = sp.pages.iter().rev().find(|p| p.side == PageSide::Left) {
            l.x + l.width
        } else {
            b.center().x
        }
    } else {
        b.center().x
    };
    (ox, h / 2.0)
}

impl<'a> Ex<'a> {
    // ---------- spreads ----------

    pub(super) fn spread_el_doc(&mut self, sp: &Spread, si: usize) -> El {
        let first = self.d.first_page_of_spread(si);
        let mut el = self.spread_common(sp, false, first);
        el.name = "Spread".into();
        el
    }

    pub(super) fn spread_el(&mut self, sp: &Spread, _master: bool) -> El {
        let mut el = self.spread_common(sp, true, 0);
        el.name = "MasterSpread".into();
        el
    }

    fn spread_common(&mut self, sp: &Spread, master: bool, first_page: usize) -> El {
        let d = self.d;
        let facing = d.settings.facing_pages;
        let (ox, oy) = spread_origin(sp, facing);
        let mut el = El::new("Spread").attr("Self", uid(sp.id.0));
        if let (true, Some(info)) = (master, &sp.parent) {
            el.set("Name", info.label());
            el.set("NamePrefix", &info.prefix);
            el.set("BaseName", &info.name);
        }
        el.set("ShowMasterItems", bool_s(sp.pages.iter().all(|p| p.show_parent_items)));
        el.set("PageCount", sp.pages.len());
        if !master {
            el.set("BindingLocation", sp.pages.iter().filter(|p| p.side == PageSide::Left).count());
            el.set("AllowPageShuffle", bool_s(sp.allow_shuffle));
        }
        // View › Rotate Spread is the rotation part of the spread's transform; pages and items stay in
        // unrotated spread coordinates.
        let rotation = if master { 0 } else { sp.pages.first().map_or(0, |p| p.view_rotation) };
        el.set(
            "ItemTransform",
            match rotation {
                1 => "0 1 -1 0 0 0",
                2 => "-1 0 0 -1 0 0",
                3 => "0 -1 1 0 0 0",
                _ => "1 0 0 1 0 0",
            },
        );
        let based_on = sp.parent.as_ref().and_then(|i| i.based_on);
        // The spread's local copies of parent items: (parent item, copy, page the copy is on).
        let mut copies = Vec::new();
        collect_copies(sp, &mut copies);
        let mut used = vec![false; copies.len()];
        // Each page's own copies are paired first, for every page, so one page's spread-wide fallback
        // can't take the copy another page overrides with.
        let mut own: Vec<Vec<Option<usize>>> = Vec::with_capacity(sp.pages.len());
        for (i, pg) in sp.pages.iter().enumerate() {
            let mut row = Vec::with_capacity(pg.overridden.len());
            for parent in &pg.overridden {
                let k = (0..copies.len()).find(|k| !used[*k] && copies[*k].0 == *parent && copies[*k].2 == Some(i));
                if let Some(k) = k {
                    used[k] = true;
                }
                row.push(k);
            }
            own.push(row);
        }
        for (i, pg) in sp.pages.iter().enumerate() {
            let name = if master { sp.parent.as_ref().map(|i| i.prefix.clone()).unwrap_or_default() } else { d.page_name(first_page + i) };
            let applied = if master { based_on } else { pg.parent };
            // InDesign reads OverrideList two ids at a time: the parent item, then the item that
            // overrides it, and a list of parent items alone crashed it on open (#2896). The copy is
            // the one on this page paired in the first pass, else any on the spread not yet paired; a
            // parent item whose override was deleted pairs with an id nothing else uses, as
            // InDesign's own files do.
            let mut overrides: Vec<String> = Vec::with_capacity(pg.overridden.len() * 2);
            for (j, parent) in pg.overridden.iter().enumerate() {
                let k = own.get(i).and_then(|r| r.get(j)).copied().flatten().or_else(|| {
                    let k = (0..copies.len()).find(|k| !used[*k] && copies[*k].0 == *parent);
                    if let Some(k) = k {
                        used[k] = true;
                    }
                    k
                });
                let copy = match k.and_then(|k| copies.get(k)) {
                    Some(c) => uid(c.1.0),
                    None => self.fresh(),
                };
                overrides.push(uid(parent.0));
                overrides.push(copy);
            }
            let pe = El::new("Page")
                .attr("Self", uid(pg.id.0))
                .attr("Name", name)
                .attr("AppliedMaster", applied.map(|s| uid(s.0)).unwrap_or_else(|| "n".into()))
                .attr("LayoutRule", names::liquid_rule_out(pg.liquid))
                .attr("OverrideList", overrides.join(" "))
                .attr("GeometricBounds", format!("0 0 {} {}", num(pg.height), num(pg.width)))
                .attr("ItemTransform", format!("1 0 0 1 {} {}", num(pg.x - ox), num(-oy)))
                .child(margin_el(pg));
            let mut pe = pe;
            push_page_guides(d, sp, pg, &mut pe, first_page + i);
            el.push(pe);
        }
        let origin = Affine::translate((-ox, -oy));
        for it in &sp.items {
            let e = self.item_el(it, origin * it.xf);
            el.push(e);
        }
        el
    }
}

pub(super) fn margin_el(pg: &designcraft_doc::Page) -> El {
    let m = pg.margin_rect();
    // Dragged column guides that still fit the margin box are written as they stand, so they
    // survive the round trip; otherwise the even split, which InDesign writes for unmoved columns.
    let pos: Vec<String> = pg.columns.edges(m.width()).into_iter().map(num).collect();
    El::new("MarginPreference")
        .attr("ColumnCount", pg.columns.count.max(1))
        .attr("ColumnGutter", num(pg.columns.gutter))
        .attr("Top", num(pg.margins.top))
        .attr("Bottom", num(pg.margins.bottom))
        .attr("Left", num(pg.margins.inside))
        .attr("Right", num(pg.margins.outside))
        .attr("ColumnDirection", "Horizontal")
        .attr("ColumnsPositions", pos.join(" "))
}

/// The page's ruler guides, measured from the ruler origin written in the preferences.
fn push_page_guides(d: &Document, sp: &Spread, pg: &designcraft_doc::Page, pe: &mut El, page_index: usize) {
    let facing = d.settings.facing_pages;
    // Ruler guides, measured from the ruler origin written in the preferences: the
    // spread's top left, the page's, or the spine (the import reads them the same way).
    let left = sp.pages.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let left = if left.is_finite() { left } else { 0.0 };
    let spine = sp.pages.iter().map(|p| p.x - left).filter(|x| *x > 1e-6).fold(f64::INFINITY, f64::min);
    let spine = if spine.is_finite() && facing { spine } else { 0.0 };
    let origin_x = match d.settings.ruler_origin {
        designcraft_doc::RulerOrigin::Spread => left,
        designcraft_doc::RulerOrigin::Page => pg.x,
        designcraft_doc::RulerOrigin::Spine => left + spine,
    };
    for g in &pg.guides {
        let vertical = g.orientation == designcraft_doc::Orientation::Vertical;
        let mut ge = El::new("Guide")
            .attr("Self", format!("{}g{}", uid(pg.id.0), pe.elements().count()))
            .attr("Orientation", if vertical { "Vertical" } else { "Horizontal" })
            .attr("Location", num(if vertical { g.position - origin_x } else { g.position }))
            .attr("FitToPage", bool_s(!g.spread))
            .attr("ViewThreshold", "5")
            .attr("Locked", bool_s(g.locked))
            .attr("PageIndex", page_index.to_string())
            .attr("GuideType", if g.liquid { "Liquid" } else { "Ruler" });
        if let Some(l) = g.layer {
            ge.set("ItemLayer", uid(l.0));
        }
        pe.push(ge);
    }
}

/// The items on the spread that override a parent item, in the order they are written: (parent
/// item, item, page). A copy's page is the one under its own centre, a nested copy's through the
/// groups it sits in. The walk keeps its own stack, so it reaches every copy the writer writes,
/// however deep, and asks for the bounds of copies only.
fn collect_copies(sp: &Spread, out: &mut Vec<(designcraft_doc::ItemId, designcraft_doc::ItemId, Option<usize>)>) {
    let mut stack: Vec<(&designcraft_doc::Item, Affine)> = sp.items.iter().rev().map(|it| (&**it, Affine::IDENTITY)).collect();
    while let Some((it, outer)) = stack.pop() {
        if let Some(parent) = it.overrides {
            let centre = bounds_under(it, outer).map_or_else(|| outer * designcraft_geom::Point::ZERO, |r| r.center());
            out.push((parent, it.id, sp.page_at_x(centre.x)));
        }
        let inner = outer * it.xf;
        stack.extend(it.children().iter().rev().map(|c| (&**c, inner)));
    }
}

/// `it`'s bounds under `outer`, as `Item::bounds` measures them (a group's are its children's
/// outlines under the full transform), with a stack of its own: a copy that is a group can hold
/// groups nested deeper than a recursive walk should go (#2940 review). `None` for a group with
/// nothing in it.
fn bounds_under(it: &designcraft_doc::Item, outer: Affine) -> Option<designcraft_geom::Rect> {
    let mut stack = vec![(it, outer)];
    let mut out: Option<designcraft_geom::Rect> = None;
    while let Some((it, outer)) = stack.pop() {
        let xf = outer * it.xf;
        if it.shape == designcraft_doc::Shape::Group
            && let designcraft_doc::Content::Group { items } = &it.content
        {
            stack.extend(items.iter().map(|c| (&**c, xf)));
            continue;
        }
        if let Some(r) = it.path.transformed(xf).bounds() {
            out = Some(out.map_or(r, |o| o.union(r)));
        }
    }
    out
}

#[cfg(test)]
mod bounds_tests {
    use designcraft_doc::{Content, Item, ItemId, LayerId, Shape};
    use designcraft_geom::{Affine, Rect, shapes};
    use std::sync::Arc;

    fn rect(id: u64, r: Rect) -> Item {
        Item::new(ItemId(id), LayerId(0), Shape::Rectangle, shapes::rectangle(r))
    }

    fn group(id: u64, items: Vec<Item>, xf: Affine) -> Item {
        let mut g = Item::new(ItemId(id), LayerId(0), Shape::Group, Default::default());
        g.content = Content::Group { items: items.into_iter().map(Arc::new).collect() };
        g.xf = xf;
        g
    }

    /// The same bounds as `Item::bounds`, for a plain item, nested and turned groups, a frame
    /// with items pasted into it (its own outline) and an empty group.
    #[test]
    fn bounds_match_the_items_own() {
        let inner = group(3, vec![rect(1, Rect::new(0.0, 0.0, 10.0, 20.0)), rect(2, Rect::new(30.0, 5.0, 40.0, 8.0))], Affine::rotate(0.4));
        let outer = group(4, vec![inner.clone(), rect(5, Rect::new(-5.0, -5.0, 0.0, 0.0))], Affine::translate((100.0, 50.0)) * Affine::scale(2.0));
        let mut frame = rect(6, Rect::new(0.0, 0.0, 50.0, 50.0));
        frame.content = Content::Group { items: vec![Arc::new(rect(7, Rect::new(-100.0, -100.0, 200.0, 200.0)))] };
        for it in [rect(8, Rect::new(1.0, 2.0, 3.0, 4.0)), inner, outer, frame] {
            let (want, got) = (it.bounds(), super::bounds_under(&it, Affine::IDENTITY).unwrap());
            for (a, b) in [(want.x0, got.x0), (want.y0, got.y0), (want.x1, got.x1), (want.y1, got.y1)] {
                assert!((a - b).abs() < 1e-9, "{want:?} vs {got:?}");
            }
        }
        assert_eq!(super::bounds_under(&group(9, vec![], Affine::IDENTITY), Affine::IDENTITY), None);
    }

    /// A group 100,000 deep is measured without recursion. It is leaked rather than dropped,
    /// since dropping it recurses.
    #[test]
    fn a_deep_group_is_measured_without_recursion() {
        let mut it = rect(0, Rect::new(10.0, 20.0, 30.0, 40.0));
        for i in 1..=100_000 {
            it = group(i, vec![it], Affine::translate((1.0, 0.0)));
        }
        let r = super::bounds_under(&it, Affine::IDENTITY);
        std::mem::forget(it);
        assert_eq!(r, Some(Rect::new(100_010.0, 20.0, 100_030.0, 40.0)));
    }
}
