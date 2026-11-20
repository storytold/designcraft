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
        for (i, pg) in sp.pages.iter().enumerate() {
            let name = if master { sp.parent.as_ref().map(|i| i.prefix.clone()).unwrap_or_default() } else { d.page_name(first_page + i) };
            let applied = if master { based_on } else { pg.parent };
            let overrides: Vec<String> = pg.overridden.iter().map(|i| uid(i.0)).collect();
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
    let pos = pg.columns.edges(m.width());
    crate::xml::from_craft(craft_idml::model::MarginPreference::to_el(
        pg.columns.count.max(1) as usize,
        pg.columns.gutter,
        pg.margins.top,
        pg.margins.bottom,
        pg.margins.inside,
        pg.margins.outside,
        &pos,
    ))
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
        let ge = craft_idml::model::GuideValues {
            self_id: format!("{}g{}", uid(pg.id.0), pe.elements().count()),
            horizontal: !vertical,
            location: if vertical { g.position - origin_x } else { g.position },
            fit_to_page: !g.spread,
            locked: g.locked,
            page_index,
            liquid: g.liquid,
            item_layer: g.layer.map(|l| uid(l.0)),
        }
        .to_el();
        pe.push(crate::xml::from_craft(ge));
    }
}
