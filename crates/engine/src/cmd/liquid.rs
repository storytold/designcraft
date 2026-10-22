//! Liquid Layout (Layout › Liquid Layout, the Page tool): page rules that decide how objects
//! follow when a page changes size, liquid guides, object pins, and alternate layouts.

use std::sync::Arc;

use designcraft_doc::{Content, Document, Item, LiquidRule, ObjectLiquid, Orientation, SpreadRef};
use designcraft_geom::{Affine, Rect};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, has_selection, str_param, targets};
use crate::Result;

/// One axis of an object-based rule: the new (lo, hi) for an object at (lo, hi) on a page that
/// went from (o0, o1) to (n0, n1).
fn axis(lo: f64, hi: f64, o: (f64, f64), n: (f64, f64), pin_lo: bool, pin_hi: bool, resize: bool) -> (f64, f64) {
    let k = (n.1 - n.0) / (o.1 - o.0).max(1e-9);
    let w = hi - lo;
    let nw = if resize { w * k } else { w };
    match (pin_lo, pin_hi) {
        (true, true) if resize => (n.0 + (lo - o.0), n.1 - (o.1 - hi)),
        (true, true) => {
            let c = (n.0 + (lo - o.0) + n.1 - (o.1 - hi)) / 2.0;
            (c - w / 2.0, c + w / 2.0)
        }
        (true, false) => (n.0 + (lo - o.0), n.0 + (lo - o.0) + nw),
        (false, true) => (n.1 - (o.1 - hi) - nw, n.1 - (o.1 - hi)),
        (false, false) => {
            let c = n.0 + ((lo + hi) / 2.0 - o.0) * k;
            (c - nw / 2.0, c + nw / 2.0)
        }
    }
}

/// The new spread-space bounds of an object at `b` under `rule`.
fn target(rule: LiquidRule, b: Rect, old: Rect, new: Rect, liquid: Option<ObjectLiquid>, vguides: &[f64], hguides: &[f64]) -> Rect {
    let (ox, oy, nx, ny) = ((old.x0, old.x1), (old.y0, old.y1), (new.x0, new.x1), (new.y0, new.y1));
    match rule {
        LiquidRule::Off | LiquidRule::ControlledByParent => b,
        LiquidRule::ReCenter => b + (new.center() - old.center()),
        LiquidRule::Scale => {
            let k = (new.width() / old.width()).min(new.height() / old.height());
            let c = new.center() + (b.center() - old.center()) * k;
            Rect::from_center_size(c, (b.width() * k, b.height() * k))
        }
        LiquidRule::GuideBased => {
            let sx = vguides.iter().any(|g| *g > b.x0 && *g < b.x1);
            let sy = hguides.iter().any(|g| *g > b.y0 && *g < b.y1);
            let (x0, x1) = axis(b.x0, b.x1, ox, nx, false, false, sx);
            let (y0, y1) = axis(b.y0, b.y1, oy, ny, false, false, sy);
            Rect::new(x0, y0, x1, y1)
        }
        LiquidRule::ObjectBased => {
            let l = liquid.unwrap_or(ObjectLiquid { resize_width: true, resize_height: true, ..Default::default() });
            let (x0, x1) = axis(b.x0, b.x1, ox, nx, l.pin_left, l.pin_right, l.resize_width);
            let (y0, y1) = axis(b.y0, b.y1, oy, ny, l.pin_top, l.pin_bottom, l.resize_height);
            Rect::new(x0, y0, x1, y1)
        }
    }
}

/// Re-flow page `pi` of spread `sr` after it went from `old` to its current bounds (objects are
/// still placed relative to `old`). Nothing happens with the rule off.
pub(crate) fn apply(d: &mut Document, sr: SpreadRef, pi: usize, old: Rect) {
    let Some(sp) = d.spread(sr) else { return };
    let Some(pg) = sp.pages.get(pi) else { return };
    let (rule, new) = (effective_rule(d, sr, pi), pg.bounds());
    if rule == LiquidRule::Off || old == new || old.width() < 1e-6 || old.height() < 1e-6 {
        return;
    }
    let liquid_guides = |o: Orientation| pg.guides.iter().filter(|g| g.liquid && g.orientation == o).map(|g| g.position).collect::<Vec<_>>();
    let (vg, hg) = (liquid_guides(Orientation::Vertical), liquid_guides(Orientation::Horizontal));
    // The page's objects: centres within its old horizontal extent (outer pages take the pasteboard).
    let last = sp.pages.len() - 1;
    let (lo, hi) = (if pi == 0 { f64::NEG_INFINITY } else { old.x0 }, if pi == last { f64::INFINITY } else { old.x1 });
    let strokes = false;
    let Some(sp) = d.spread_mut(sr) else { return };
    for it in &mut sp.items {
        let b = it.bounds();
        let c = b.center().x;
        if c < lo || c >= hi || b.width() < 1e-9 || b.height() < 1e-9 {
            continue;
        }
        let to = target(rule, b, old, new, it.liquid, &vg, &hg);
        if to == b {
            continue;
        }
        let m = Affine::translate((to.x0, to.y0))
            * Affine::scale_non_uniform(to.width() / b.width(), to.height() / b.height())
            * Affine::translate((-b.x0, -b.y0));
        let it: &mut Item = Arc::make_mut(it);
        if rule == LiquidRule::Scale {
            super::object::scale_item(it, m, (to.width() / b.width()).abs(), strokes);
        } else if matches!(it.content, Content::Group { .. }) {
            it.xf = m * it.xf;
        } else {
            // The frame stretches; content keeps its size unless the frame auto-fits it.
            let inner = it.xf.inverse() * m * it.xf;
            super::object::bake(it, inner);
            let ib = it.inner_bounds();
            if let Content::Graphic(g) = &mut it.content {
                let o = inner * designcraft_geom::Point::ZERO;
                g.xf = Affine::translate(o.to_vec2()) * g.xf;
                if let Some(xf) = g.fitted(ib, g.auto_fit) {
                    g.xf = xf;
                }
            }
        }
    }
}

/// The rule page `pi` of spread `sr` follows: its own, or its parent page's when it's controlled
/// by the parent (Off when that's unset too).
pub(crate) fn effective_rule(d: &Document, sr: SpreadRef, pi: usize) -> LiquidRule {
    let Some(pg) = d.spread(sr).and_then(|sp| sp.pages.get(pi)) else { return LiquidRule::Off };
    if pg.liquid != LiquidRule::ControlledByParent {
        return pg.liquid;
    }
    let SpreadRef::Doc(si) = sr else { return LiquidRule::Off };
    let abs = d.first_page_of_spread(si) + pi;
    match d.parent_page_for(abs).and_then(|(ppi, ppg)| d.parents.get(ppi)?.pages.get(ppg).map(|p| p.liquid)) {
        Some(LiquidRule::ControlledByParent) | None => LiquidRule::Off,
        Some(r) => r,
    }
}

fn rule_param(p: &Value, id: &str) -> Result<LiquidRule> {
    serde_json::from_value(p.get("rule").cloned().unwrap_or(json!("off")))
        .map_err(|_| bad(id, "`rule`: off|scale|reCenter|guideBased|objectBased|controlledByParent"))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "liquid.pageRule",
            "Liquid Page Rule",
            ["Layout", "Liquid Layout"],
            None,
            "{rule: off|scale|reCenter|guideBased|objectBased|controlledByParent (the parent page's rule), pages?: [1-based] (default: all)}",
            has_doc,
            |s, p| {
                let rule = rule_param(p, "liquid.pageRule")?;
                let pages: Option<Vec<usize>> =
                    p.get("pages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|n| n as usize).collect());
                s.edit(|d, _| {
                    let n = d.page_count();
                    let list = pages.clone().unwrap_or_else(|| (1..=n).collect());
                    for page in &list {
                        let Some((si, pi)) = page.checked_sub(1).and_then(|i| d.page_loc(i)) else { continue };
                        Arc::make_mut(&mut d.spreads[si]).pages[pi].liquid = rule;
                    }
                    Ok(json!({"pages": list.len()}))
                })
            }
        ),
        cmd!(
            "liquid.object",
            "Object Liquid Settings",
            [],
            None,
            "{ids?, resizeWidth?, resizeHeight?, pinTop?, pinBottom?, pinLeft?, pinRight?} — for object-based pages",
            has_selection,
            |s, p| {
                let ids = targets(s, p)?;
                s.edit(|d, _| {
                    for id in &ids {
                        let Some(it) = d.item_mut(*id) else { continue };
                        let mut l = it.liquid.unwrap_or(ObjectLiquid { resize_width: true, resize_height: true, ..Default::default() });
                        for (k, f) in [
                            ("resizeWidth", &mut l.resize_width),
                            ("resizeHeight", &mut l.resize_height),
                            ("pinTop", &mut l.pin_top),
                            ("pinBottom", &mut l.pin_bottom),
                            ("pinLeft", &mut l.pin_left),
                            ("pinRight", &mut l.pin_right),
                        ] {
                            if let Some(v) = p.get(k).and_then(Value::as_bool) {
                                *f = v;
                            }
                        }
                        it.liquid = Some(l);
                    }
                    Ok(json!({"objects": ids.len()}))
                })
            }
        ),
        cmd!("guide.liquid", "Convert to Liquid Guide", [], None, "{spread?, page, index, on?: bool (default true)}", has_doc, |s, p| {
            let sr = super::spread_param(p, "guide.liquid")?;
            let pi = p.get("page").and_then(Value::as_u64).unwrap_or(0) as usize;
            let gi = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("guide.liquid", "`index` required"))? as usize;
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
            s.edit(|d, _| {
                let sp = d.spread_mut(sr).ok_or_else(|| bad("guide.liquid", "no such spread"))?;
                let g = sp.pages.get_mut(pi).and_then(|pg| pg.guides.get_mut(gi)).ok_or_else(|| bad("guide.liquid", "no such guide"))?;
                g.liquid = on;
                Ok(Value::Null)
            })
        }),
        cmd!(
            "layout.createAlternate",
            "Create Alternate Layout…",
            ["Layout"],
            None,
            "{name, width, height, rule?: page rule for the copies (default: keep each page's)} — every page copied after the last at the new size (a new section marked `name`), objects following the liquid rules",
            has_doc,
            create_alternate
        ),
    ]
}

fn create_alternate(s: &mut crate::Session, p: &Value) -> Result<Value> {
    const ID: &str = "layout.createAlternate";
    let name = str_param(p, "name").unwrap_or("Alternate").to_string();
    let w = p.get("width").and_then(Value::as_f64).ok_or_else(|| bad(ID, "`width` required"))?.max(1.0);
    let h = p.get("height").and_then(Value::as_f64).ok_or_else(|| bad(ID, "`height` required"))?.max(1.0);
    let rule = p.get("rule").map(|_| rule_param(p, ID)).transpose()?;
    let n = s.doc()?.doc.page_count();
    s.edit(|d, _| {
        let src = d.clone();
        let parent = d.page_loc(n - 1).and_then(|(si, pi)| d.spreads[si].pages[pi].parent);
        d.insert_pages(Some(n - 1), n, parent)?;
        for i in 0..n {
            let (Some((si, pi)), Some((ti, tj))) = (src.page_loc(i), d.page_loc(n + i)) else { continue };
            let from = &src.spreads[si];
            let pb = from.pages[pi].bounds();
            let last = from.pages.len() - 1;
            let ids: Vec<_> = from
                .items
                .iter()
                .filter(|it| {
                    let c = it.bounds().center().x;
                    (pi == 0 || c >= pb.x0) && (pi == last || c < pb.x1)
                })
                .map(|it| it.id)
                .collect();
            // The copy keeps the page's liquid rule (or takes the one given).
            let to = d.spreads[ti].pages[tj].bounds();
            Arc::make_mut(&mut d.spreads[ti]).pages[tj].liquid = rule.unwrap_or(from.pages[pi].liquid);
            Arc::make_mut(&mut d.spreads[ti]).pages[tj].guides = from.pages[pi]
                .guides
                .iter()
                .cloned()
                .map(|mut g| {
                    if g.orientation == Orientation::Vertical {
                        g.position += to.x0 - pb.x0;
                    }
                    g
                })
                .collect();
            let new = super::object::duplicate_from(d, &src, &ids, SpreadRef::Doc(ti), designcraft_geom::Vec2::new(to.x0 - pb.x0, 0.0))?;
            // The copies' stories stay linked to the originals (Links panel: update when they change).
            for (old, new) in ids.iter().zip(&new) {
                let (Some(os), Some(ns)) =
                    (src.item(*old).and_then(|i| i.text_frame()).map(|t| t.story), d.item(*new).and_then(|i| i.text_frame()).map(|t| t.story))
                else {
                    continue;
                };
                let rev = src.story(os).map_or(0, |s| s.rev);
                if let Some(st) = d.story_mut(ns) {
                    st.link = Some((os, rev));
                }
            }
        }
        let pages: Vec<usize> = (n + 1..=2 * n).collect();
        super::layout::resize_pages(d, &pages, Some(w), Some(h));
        d.sections.retain(|x| x.start != n);
        d.sections.push(designcraft_doc::Section {
            start: n,
            start_number: Some(1),
            style: Default::default(),
            prefix: String::new(),
            marker: name.clone(),
            include_prefix: false,
        });
        d.sections.sort_by_key(|x| x.start);
        Ok(json!({"firstPage": n + 1, "pages": n}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn bounds(s: &Session, id: u64) -> designcraft_geom::Rect {
        s.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds()
    }

    #[test]
    fn liquid_rules_follow_a_page_resize() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "width": 600, "height": 800})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        // Re-center: same size, shifted by half the growth.
        s.execute("liquid.pageRule", &json!({"rule": "reCenter"})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 800})).unwrap();
        let b = bounds(&s, a);
        assert!((b.x0 - 200.0).abs() < 1e-6 && (b.width() - 100.0).abs() < 1e-6, "{b:?}");
        // Object-based: pinned left and right, resizable → stretches with the page.
        s.execute("liquid.pageRule", &json!({"rule": "objectBased"})).unwrap();
        s.execute("liquid.object", &json!({"ids": [a], "pinLeft": true, "pinRight": true, "resizeWidth": true, "resizeHeight": false})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 900})).unwrap();
        let b = bounds(&s, a);
        assert!((b.x0 - 200.0).abs() < 1e-6 && (b.x1 - 400.0).abs() < 1e-6, "{b:?}");
        // Scale: uniform, centred.
        s.execute("liquid.pageRule", &json!({"rule": "scale"})).unwrap();
        s.execute("layout.pageSize", &json!({"pages": [1], "width": 450, "height": 400})).unwrap();
        let b = bounds(&s, a);
        assert!((b.width() - 100.0).abs() < 1e-6 && (b.height() - 50.0).abs() < 1e-6, "{b:?}");
    }

    #[test]
    fn guide_based_and_alternate_layout() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1, "width": 600, "height": 800})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [400, 100, 500, 200]})).unwrap()["id"].as_u64().unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 200, "page": 0})).unwrap();
        s.execute("guide.liquid", &json!({"page": 0, "index": 0})).unwrap();
        s.execute("liquid.pageRule", &json!({"rule": "guideBased"})).unwrap();
        let r = s.execute("layout.createAlternate", &json!({"name": "Wide", "width": 1200, "height": 800})).unwrap();
        assert_eq!(r["firstPage"], 2);
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.page_count(), 2);
        let (si, pi) = d.page_loc(1).unwrap();
        let page = d.spreads[si].pages[pi].bounds();
        assert!((page.width() - 1200.0).abs() < 1e-6);
        let copies: Vec<_> = d.spreads[si].items.iter().map(|it| it.bounds()).collect();
        assert_eq!(copies.len(), 2);
        // The frame the liquid guide crosses doubles in width; the other keeps its size.
        let mut widths: Vec<f64> = copies.iter().map(|r| r.width()).collect();
        widths.sort_by(f64::total_cmp);
        assert!((widths[0] - 100.0).abs() < 1e-6 && (widths[1] - 400.0).abs() < 1e-6, "{widths:?}");
        // The originals are untouched, and it is one undo step.
        assert!((bounds(&s, a).width() - 200.0).abs() < 1e-6 && (bounds(&s, b).width() - 100.0).abs() < 1e-6);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
        // Text in an alternate layout stays linked to the original story.
        s.execute("edit.redo", &json!({})).unwrap();
        let t = s.execute("frame.create", &json!({"rect": [100, 300, 300, 400], "content": "text", "text": "Original", "caret": false})).unwrap();
        s.execute("layout.createAlternate", &json!({"name": "Tall", "width": 600, "height": 1200})).unwrap();
        let links = s.execute("story.links", &json!({})).unwrap();
        assert!(links.as_array().unwrap().iter().any(|l| l["parent"] == t["story"]), "{links}");
    }
}
