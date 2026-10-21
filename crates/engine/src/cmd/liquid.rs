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
            "{name, width, height, from?: source layout name|index (default: the first), fromName?: names the source layout, rule?: page rule for the copies (default: keep each page's)} → {layout, firstPage, pages} — the source layout's pages copied after the last page at the new size (a layout of their own, its own column in the Pages panel), objects following the liquid rules",
            has_doc,
            create_alternate
        ),
        cmd!(
            "layout.alternate.duplicate",
            "Duplicate Alternate Layout",
            ["Layout"],
            None,
            "{layout: name|index, name?: (default \"<layout> copy\")} → {layout, firstPage, pages} — the layout's pages and objects copied at the same size",
            has_doc,
            duplicate_alternate
        ),
        cmd!(
            "layout.alternate.delete",
            "Delete Alternate Layout",
            ["Layout"],
            None,
            "{layout: name|index} — delete the layout's pages and their objects; the last layout cannot be deleted",
            has_doc,
            delete_alternate
        ),
        cmd!(
            "layout.alternate.options",
            "Alternate Layout Options…",
            ["Layout"],
            None,
            "{layout: name|index, name?, width?, height?} → {layout} — rename the layout, or resize its pages (objects follow the liquid rules)",
            has_doc,
            alternate_options
        ),
    ]
}

/// A layout named by a command parameter: its name or its index in [`Document::layouts`].
fn layout_param(id: &str, d: &Document, v: Option<&Value>) -> Result<designcraft_doc::LayoutRange> {
    let layouts = d.layouts();
    let found = match v {
        Some(Value::Number(n)) => n.as_u64().and_then(|i| usize::try_from(i).ok()).and_then(|i| layouts.get(i)),
        Some(Value::String(s)) => layouts.iter().find(|l| l.name == *s),
        Some(_) => return Err(bad(id, "`layout` is a layout name or index")),
        None => return Err(bad(id, "`layout` names a layout")),
    };
    found.cloned().ok_or_else(|| bad(id, format!("no layout {}", v.map(Value::to_string).unwrap_or_default())))
}

/// A layout name parameter: trimmed, 1–100 characters.
fn layout_name(id: &str, v: &str) -> Result<String> {
    let v = v.trim();
    if v.is_empty() || v.chars().count() > 100 {
        return Err(bad(id, "a layout name has 1 to 100 characters"));
    }
    Ok(v.to_string())
}

/// `base`, or `base 2`, `base 3`… when another layout (but `except`) has the name.
fn unique_layout_name(d: &Document, base: &str, except: Option<usize>) -> String {
    let taken: Vec<String> = d.layouts().into_iter().enumerate().filter(|(i, _)| Some(*i) != except).map(|(_, l)| l.name).collect();
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..=taken.len().saturating_add(2)).map(|k| format!("{base} {k}")).find(|c| !taken.contains(c)).unwrap_or_else(|| base.to_string())
}

/// The section starting at absolute page `start`, made when there is none.
fn section_at(d: &mut Document, start: usize) -> Option<&mut designcraft_doc::Section> {
    if !d.sections.iter().any(|x| x.start == start) {
        d.sections.push(designcraft_doc::Section {
            start,
            start_number: if start == 0 { Some(1) } else { None },
            style: Default::default(),
            prefix: String::new(),
            marker: String::new(),
            include_prefix: false,
            layout: None,
        });
        d.sections.sort_by_key(|x| x.start);
    }
    d.sections.iter_mut().find(|x| x.start == start)
}

/// Copy the pages of layout `from` after the last page as a new layout `name`, sized `size`
/// (None keeps each page's), with `rule` as the copies' page rule. → {layout, firstPage, pages}
fn copy_layout(
    id: &str,
    d: &mut Document,
    from: &designcraft_doc::LayoutRange,
    name: &str,
    size: Option<(f64, f64)>,
    rule: Option<LiquidRule>,
) -> Result<Value> {
    let n = d.page_count();
    let count = from.end.saturating_sub(from.start);
    if count == 0 || from.end > n {
        return Err(bad(id, format!("layout {} has no pages", from.name)));
    }
    let name = unique_layout_name(d, name, None);
    let src = d.clone();
    let last_parent = d.page_loc(n.saturating_sub(1)).and_then(|(si, pi)| d.spreads.get(si).and_then(|sp| sp.pages.get(pi))).and_then(|p| p.parent);
    d.insert_pages(n.checked_sub(1), count, last_parent)?;
    // The new layout starts numbering at 1 and a spread of its own.
    d.sections.retain(|x| x.start != n);
    d.sections.push(designcraft_doc::Section {
        start: n,
        start_number: Some(1),
        style: Default::default(),
        prefix: String::new(),
        marker: name.clone(),
        include_prefix: false,
        layout: Some(name.clone()),
    });
    d.sections.sort_by_key(|x| x.start);
    d.repaginate();
    for k in 0..count {
        let i = from.start + k;
        let (Some((si, pi)), Some((ti, tj))) = (src.page_loc(i), d.page_loc(n + k)) else { continue };
        let Some(from_sp) = src.spreads.get(si) else { continue };
        let Some(from_pg) = from_sp.pages.get(pi) else { continue };
        let pb = from_pg.bounds();
        let last = from_sp.pages.len().saturating_sub(1);
        let ids: Vec<_> = from_sp
            .items
            .iter()
            .filter(|it| {
                let c = it.bounds().center().x;
                (pi == 0 || c >= pb.x0) && (pi == last || c < pb.x1)
            })
            .map(|it| it.id)
            .collect();
        let Some(sp) = d.spreads.get_mut(ti) else { continue };
        let sp = Arc::make_mut(sp);
        let Some(pg) = sp.pages.get_mut(tj) else { continue };
        // The copy is the page it copies (size, margins, columns, parent, guides), with the
        // page rule given or its own.
        let dx = pg.x - from_pg.x;
        (pg.width, pg.height, pg.margins, pg.columns, pg.parent) =
            (from_pg.width, from_pg.height, from_pg.margins, from_pg.columns.clone(), from_pg.parent);
        pg.liquid = rule.unwrap_or(from_pg.liquid);
        pg.guides = from_pg
            .guides
            .iter()
            .cloned()
            .map(|mut g| {
                if g.orientation == Orientation::Vertical {
                    g.position += dx;
                }
                g
            })
            .collect();
        sp.relayout();
        let to = sp.pages.get(tj).map_or(pb, |p| p.bounds());
        let new = super::object::duplicate_from(d, &src, &ids, SpreadRef::Doc(ti), designcraft_geom::Vec2::new(to.x0 - pb.x0, to.y0 - pb.y0))?;
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
    if let Some((w, h)) = size {
        let pages: Vec<usize> = (n + 1..=n + count).collect();
        super::layout::resize_pages(d, &pages, Some(w), Some(h));
    }
    Ok(json!({"layout": name, "firstPage": n + 1, "pages": count}))
}

fn create_alternate(s: &mut crate::Session, p: &Value) -> Result<Value> {
    const ID: &str = "layout.createAlternate";
    let name = layout_name(ID, str_param(p, "name").unwrap_or("Alternate"))?;
    let size = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| bad(ID, format!("`{k}` is a number")));
    let (w, h) = (size("width")?.clamp(1.0, 15_552.0), size("height")?.clamp(1.0, 15_552.0));
    let rule = p.get("rule").map(|_| rule_param(p, ID)).transpose()?;
    let from_name = str_param(p, "fromName").map(|v| layout_name(ID, v)).transpose()?;
    let from = match p.get("from") {
        None | Some(Value::Null) => s.doc()?.doc.layouts().into_iter().next().ok_or_else(|| bad(ID, "no layout"))?,
        v => layout_param(ID, &s.doc()?.doc, v)?,
    };
    s.edit(|d, _| {
        if let Some(fname) = from_name {
            let index = d.layouts().iter().position(|l| l.start == from.start);
            let fname = unique_layout_name(d, &fname, index);
            if let Some(sec) = section_at(d, from.start) {
                sec.layout = Some(fname);
            }
        }
        copy_layout(ID, d, &from, &name, Some((w, h)), rule)
    })
}

fn duplicate_alternate(s: &mut crate::Session, p: &Value) -> Result<Value> {
    const ID: &str = "layout.alternate.duplicate";
    let from = layout_param(ID, &s.doc()?.doc, p.get("layout"))?;
    let name = match str_param(p, "name") {
        Some(v) => layout_name(ID, v)?,
        None => format!("{} copy", from.name),
    };
    s.edit(|d, _| copy_layout(ID, d, &from, &name, None, None))
}

fn delete_alternate(s: &mut crate::Session, p: &Value) -> Result<Value> {
    const ID: &str = "layout.alternate.delete";
    let d = &s.doc()?.doc;
    let layout = layout_param(ID, d, p.get("layout"))?;
    if d.layouts().len() < 2 {
        return Err(bad(ID, "the only layout cannot be deleted"));
    }
    let pages: Vec<usize> = (layout.start..layout.end).collect();
    s.edit(|d, sel| {
        d.delete_pages(&pages)?;
        *sel = Default::default();
        Ok(json!({"layout": layout.name, "pages": pages.len()}))
    })
}

fn alternate_options(s: &mut crate::Session, p: &Value) -> Result<Value> {
    const ID: &str = "layout.alternate.options";
    let d = &s.doc()?.doc;
    let layout = layout_param(ID, d, p.get("layout"))?;
    let index = d.layouts().iter().position(|l| l.start == layout.start);
    let name = str_param(p, "name").map(|v| layout_name(ID, v)).transpose()?;
    let dim = |k: &str| -> Result<Option<f64>> {
        p.get(k)
            .map(|v| v.as_f64().filter(|v| v.is_finite()).map(|v| v.clamp(1.0, 15_552.0)).ok_or_else(|| bad(ID, format!("`{k}` is a number"))))
            .transpose()
    };
    let (w, h) = (dim("width")?, dim("height")?);
    if name.as_ref().is_none_or(|n| *n == layout.name) && w.is_none() && h.is_none() {
        return Ok(json!({"layout": layout.name}));
    }
    s.edit(|d, _| {
        let mut out = layout.name.clone();
        if let Some(name) = name {
            out = unique_layout_name(d, &name, index);
            if let Some(sec) = section_at(d, layout.start) {
                sec.layout = Some(out.clone());
            }
        }
        if w.is_some() || h.is_some() {
            let pages: Vec<usize> = (layout.start + 1..=layout.end).collect();
            super::layout::resize_pages(d, &pages, w, h);
        }
        Ok(json!({"layout": out}))
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

    fn layouts(s: &Session) -> Vec<(String, usize, usize)> {
        s.doc().unwrap().doc.layouts().into_iter().map(|l| (l.name, l.start, l.end)).collect()
    }

    #[test]
    fn alternate_layouts_copy_one_layout_and_can_be_duplicated_renamed_and_deleted() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "width": 600, "height": 800, "facingPages": true})).unwrap();
        assert_eq!(layouts(&s), vec![("Layout 1".into(), 0, 3)]);
        let r = s.execute("layout.createAlternate", &json!({"name": "Tablet H", "width": 1024, "height": 768, "fromName": "Print"})).unwrap();
        assert_eq!(r["layout"], "Tablet H");
        assert_eq!(layouts(&s), vec![("Print".into(), 0, 3), ("Tablet H".into(), 3, 6)]);
        // Again from the first layout: only its 3 pages are copied, and the name is made unique.
        let r = s.execute("layout.createAlternate", &json!({"name": "Tablet H", "width": 768, "height": 1024, "from": "Print"})).unwrap();
        assert_eq!(r["layout"], "Tablet H 2");
        assert_eq!(layouts(&s).last().cloned(), Some(("Tablet H 2".into(), 6, 9)));
        // Every layout starts a spread of its own.
        let d = s.doc().unwrap().doc.clone();
        for l in d.layouts() {
            assert_eq!(d.page_loc(l.start).unwrap().1, 0, "{l:?} starts a spread");
        }
        let (si, pi) = d.page_loc(3).unwrap();
        assert!((d.spreads[si].pages[pi].width - 1024.0).abs() < 1e-6);
        // Duplicate keeps the size; delete removes the layout's pages; undo brings them back.
        let r = s.execute("layout.alternate.duplicate", &json!({"layout": "Tablet H"})).unwrap();
        assert_eq!(r["layout"], "Tablet H copy");
        let d = s.doc().unwrap().doc.clone();
        let (si, pi) = d.page_loc(9).unwrap();
        assert!((d.spreads[si].pages[pi].width - 1024.0).abs() < 1e-6);
        assert_eq!(d.page_count(), 12);
        s.execute("layout.alternate.delete", &json!({"layout": "Tablet H"})).unwrap();
        assert_eq!(layouts(&s), vec![("Print".into(), 0, 3), ("Tablet H 2".into(), 3, 6), ("Tablet H copy".into(), 6, 9)]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(layouts(&s).len(), 4);
        // Rename (kept unique) and resize.
        let r = s.execute("layout.alternate.options", &json!({"layout": 3, "name": "Print"})).unwrap();
        assert_eq!(r["layout"], "Print 2");
        s.execute("layout.alternate.options", &json!({"layout": "Print 2", "width": 500})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let (si, pi) = d.page_loc(9).unwrap();
        assert!((d.spreads[si].pages[pi].width - 500.0).abs() < 1e-6);
        // Deleting the first layout leaves the next one first.
        s.execute("layout.alternate.delete", &json!({"layout": 0})).unwrap();
        assert_eq!(layouts(&s).first().cloned(), Some(("Tablet H".into(), 0, 3)));
        // Inserting a page at the end of a layout keeps the next layout's pages its own.
        s.execute("layout.pages.insert", &json!({"after": 2, "count": 1})).unwrap();
        assert_eq!(layouts(&s)[0], ("Tablet H".into(), 0, 4));
        assert_eq!(layouts(&s)[1].1, 4);
    }

    #[test]
    fn alternate_layout_commands_refuse_bad_input() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        // The only layout cannot be deleted.
        assert!(s.execute("layout.alternate.delete", &json!({"layout": 0})).is_err());
        assert_eq!(s.doc().unwrap().doc.page_count(), 1);
        for p in [json!({}), json!({"layout": "Nope"}), json!({"layout": 7}), json!({"layout": true}), json!({"layout": -1})] {
            assert!(s.execute("layout.alternate.duplicate", &p).is_err(), "{p}");
            assert!(s.execute("layout.alternate.options", &p).is_err(), "{p}");
        }
        assert!(s.execute("layout.alternate.options", &json!({"layout": 0, "name": " "})).is_err());
        assert!(s.execute("layout.alternate.options", &json!({"layout": 0, "width": "wide"})).is_err());
        assert!(s.execute("layout.createAlternate", &json!({"name": "", "width": 10, "height": 10})).is_err());
        assert!(s.execute("layout.createAlternate", &json!({"name": "X", "height": 10})).is_err());
        assert!(s.execute("layout.createAlternate", &json!({"name": "X", "width": 10, "height": 10, "from": "Nope"})).is_err());
        assert!(s.doc().unwrap().history.undo.is_empty());
        // A layout survives Numbering & Section Options › remove section.
        s.execute("layout.createAlternate", &json!({"name": "Wide", "width": 900, "height": 600})).unwrap();
        s.execute("layout.section", &json!({"page": 2, "remove": true})).unwrap();
        assert_eq!(s.doc().unwrap().doc.layouts().len(), 2);
    }

    #[test]
    fn deleting_a_layouts_first_page_keeps_the_layout_when_a_section_follows() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        s.execute("layout.createAlternate", &json!({"name": "Wide", "width": 900, "height": 600})).unwrap();
        // The layout starts at page 3; a numbering section starts at page 4.
        s.execute("layout.section", &json!({"page": 4, "startNumber": 7})).unwrap();
        s.execute("layout.pages.delete", &json!({"pages": [2]})).unwrap();
        let names: Vec<(String, usize, usize)> = layouts(&s);
        assert_eq!(names, vec![("Layout 1".into(), 0, 2), ("Wide".into(), 2, 3)]);
        // The kept section's numbering stays.
        assert_eq!(s.doc().unwrap().doc.page_number(2), 7);
    }

    #[test]
    fn layouts_survive_a_save_and_old_sections_load() {
        let sec: designcraft_doc::Section = serde_json::from_value(json!({"start": 2, "marker": "x"})).unwrap();
        assert_eq!(sec.layout, None);
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        s.execute("layout.createAlternate", &json!({"name": "Wide", "width": 900, "height": 600})).unwrap();
        let d = &s.doc().unwrap().doc;
        let back: Vec<designcraft_doc::Section> = serde_json::from_value(serde_json::to_value(&d.sections).unwrap()).unwrap();
        assert_eq!(back, d.sections);
        assert!(back.iter().any(|x| x.layout.as_deref() == Some("Wide")));
    }
}
