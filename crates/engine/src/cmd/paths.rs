//! Path operations from the Object and Type menus: Make / Release Compound Path, Create Outlines,
//! and the Scissors tool's split.

use std::sync::Arc;

use designcraft_doc::{Content, Document, Fill, Item, ItemId, Selection, Shape, SpreadRef, Stroke};
use designcraft_geom::{Affine, BezPath, PathData, SubPath};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, point_param, targets};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "object.makeCompoundPath",
            "Make Compound Path",
            ["Object", "Paths"],
            Some("Cmd+8"),
            "{ids?} — one path from the selected paths (the backmost object's look); paths inside others become holes → {id}",
            has_selection,
            make_compound
        ),
        cmd!(
            "object.releaseCompoundPath",
            "Release Compound Path",
            ["Object", "Paths"],
            Some("Cmd+Alt+Shift+8"),
            "{ids?} — each subpath becomes its own object → {ids}",
            has_selection,
            release_compound
        ),
        cmd!(
            "type.createOutlines",
            "Create Outlines",
            ["Type"],
            Some("Cmd+Shift+O"),
            "{ids?} — text frames become compound paths of their glyphs (one per text colour, grouped when several) → {ids}",
            has_selection,
            create_outlines
        ),
        cmd!(
            "object.pathfinder",
            "Pathfinder",
            ["Object", "Pathfinder"],
            None,
            "{op: add|subtract|intersect|exclude|minusBack, ids?} — combine the selected shapes into one (the backmost keeps its look; the frontmost for minusBack) → {id}",
            has_selection,
            pathfinder
        ),
        cmd!(
            "object.gradientFeather",
            "Gradient Feather",
            ["Object", "Effects"],
            None,
            "{on?: true, radial?, angle?, start?: opacity 0–100 (100), end?: (0), from?: [x,y], to?: [x,y] (spread coords: the Gradient Feather tool's drag), ids?}",
            has_selection,
            gradient_feather
        ),
        cmd!(
            "path.split",
            "Split Path",
            [],
            None,
            "{id, at: [x,y] (spread coords; the nearest point on the path)} — Scissors: an open path becomes two, a closed one opens there → {ids}",
            has_selection,
            split_path
        ),
    ]
}

/// Item → spread transform.
fn spread_xf(d: &Document, id: ItemId) -> Option<(SpreadRef, Affine)> {
    let loc = d.find(id)?;
    Some((loc.spread, d.parent_xf(&loc) * d.item_at(&loc)?.xf))
}

/// Orient subpaths so those nested an odd number of times run against the others: with the
/// non-zero rule they become holes.
fn orient_holes(path: &mut PathData) {
    let beziers: Vec<BezPath> = path.subpaths.iter().map(|s| PathData::single(s.clone()).to_bezpath()).collect();
    let n = path.subpaths.len();
    for i in 0..n {
        let Some(p) = path.subpaths[i].anchors.first().map(|a| a.p) else { continue };
        let depth = (0..n).filter(|&j| j != i && designcraft_geom::hit::fill_contains(&beziers[j], designcraft_geom::FillRule::NonZero, p)).count();
        let sp = &mut path.subpaths[i];
        let cw = sp.area() > 0.0;
        if cw == (depth % 2 == 1) {
            sp.reverse();
        }
    }
}

fn make_compound(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if ids.len() < 2 {
        return Err(bad("object.makeCompoundPath", "select two or more paths"));
    }
    s.edit(|d, sel| {
        // Back to front within the spread.
        let (sr, _) = spread_xf(d, ids[0]).ok_or(designcraft_doc::DocError::NoItem(ids[0]))?;
        let order: Vec<ItemId> = d.spread(sr).map(|sp| sp.items.iter().map(|i| i.id).filter(|i| ids.contains(i)).collect()).unwrap_or_default();
        let base_id = *order.first().ok_or_else(|| bad("object.makeCompoundPath", "the paths must be on one spread"))?;
        let (_, base_xf) = spread_xf(d, base_id).ok_or(designcraft_doc::DocError::NoItem(base_id))?;
        let to_base = base_xf.inverse();
        let mut subpaths: Vec<SubPath> = d.item(base_id).map(|i| i.path.subpaths.clone()).unwrap_or_default();
        for id in order.iter().skip(1) {
            let Some((_, xf)) = spread_xf(d, *id) else { continue };
            let Some(it) = d.item(*id) else { continue };
            if matches!(it.content, Content::Group { .. }) {
                continue;
            }
            subpaths.extend(it.path.transformed(to_base * xf).subpaths);
            d.remove_item(*id)?;
        }
        let it = d.item_mut(base_id).ok_or(designcraft_doc::DocError::NoItem(base_id))?;
        it.path = PathData::new(subpaths);
        orient_holes(&mut it.path);
        it.shape = Shape::Path;
        *sel = Selection::items(vec![base_id]);
        Ok(json!({"id": base_id.0}))
    })
}

fn pathfinder(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_geom::pathfinder::{Op, combine};
    let op_s = super::str_param(p, "op").unwrap_or("add");
    let op = Op::parse(op_s).ok_or_else(|| bad("object.pathfinder", format!("unknown op `{op_s}`")))?;
    let ids = targets(s, p)?;
    if ids.len() < 2 {
        return Err(bad("object.pathfinder", "select two or more shapes"));
    }
    s.edit(|d, sel| {
        let (sr, _) = spread_xf(d, ids[0]).ok_or(designcraft_doc::DocError::NoItem(ids[0]))?;
        let order: Vec<ItemId> = d
            .spread(sr)
            .map(|sp| {
                sp.items
                    .iter()
                    .filter(|i| ids.contains(&i.id) && !matches!(i.content, Content::Group { .. } | Content::Text(_)))
                    .map(|i| i.id)
                    .collect()
            })
            .unwrap_or_default();
        if order.len() < 2 {
            return Err(bad("object.pathfinder", "select two or more shapes on one spread (not text frames or groups)"));
        }
        let keep = if op == Op::MinusBack { order[order.len() - 1] } else { order[0] };
        let shapes: Vec<PathData> =
            order.iter().filter_map(|id| spread_xf(d, *id).and_then(|(_, xf)| Some(d.item(*id)?.path.transformed(xf)))).collect();
        let Some(result) = combine(op, &shapes) else { return Err(bad("object.pathfinder", "the shapes don't overlap: nothing is left")) };
        let (_, kxf) = spread_xf(d, keep).ok_or(designcraft_doc::DocError::NoItem(keep))?;
        for id in &order {
            if *id != keep {
                d.remove_item(*id)?;
            }
        }
        let it = d.item_mut(keep).ok_or(designcraft_doc::DocError::NoItem(keep))?;
        it.path = result.transformed(kxf.inverse());
        orient_holes(&mut it.path);
        it.shape = Shape::Path;
        it.corners = Default::default();
        *sel = Selection::items(vec![keep]);
        Ok(json!({"id": keep.0}))
    })
}

fn gradient_feather(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let p = p.clone();
    s.edit(|d, _| {
        for id in &ids {
            let Some((_, xf)) = spread_xf(d, *id) else { continue };
            let Some(it) = d.item_mut(*id) else { continue };
            let gf = &mut it.effects.gradient_feather;
            gf.on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
            if let Some(v) = p.get("radial").and_then(Value::as_bool) {
                gf.radial = v;
            }
            if let Some(v) = p.get("angle").and_then(Value::as_f64) {
                gf.angle = v;
                gf.vector = None;
            }
            if let Some(v) = p.get("start").and_then(Value::as_f64) {
                gf.start = (v / 100.0).clamp(0.0, 1.0) as f32;
            }
            if let Some(v) = p.get("end").and_then(Value::as_f64) {
                gf.end = (v / 100.0).clamp(0.0, 1.0) as f32;
            }
            if let (Some(a), Some(b)) = (point_param(&p, "from"), point_param(&p, "to")) {
                let inv = xf.inverse();
                let (a, b) = (inv * a, inv * b);
                if (b - a).hypot() > 1e-6 {
                    gf.vector = Some([a.x, a.y, b.x, b.y]);
                }
            }
        }
        Ok(json!({"changed": ids.len()}))
    })
}

fn release_compound(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        let mut out = Vec::new();
        for id in &ids {
            let Some(it) = d.item(*id).cloned() else { continue };
            if it.path.subpaths.len() < 2 || matches!(it.content, Content::Group { .. }) {
                out.push(*id);
                continue;
            }
            let (sr, _) = spread_xf(d, *id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            let at = d.spread(sr).and_then(|sp| sp.items.iter().position(|i| i.id == *id));
            for (k, sp) in it.path.subpaths.iter().enumerate() {
                let mut part = it.clone();
                part.path = PathData::single(sp.clone());
                if k == 0 {
                    if let Some(x) = d.item_mut(*id) {
                        x.path = part.path;
                    }
                    out.push(*id);
                    continue;
                }
                part.id = ItemId(d.alloc());
                part.content = Content::Unassigned;
                out.push(part.id);
                d.insert_item(sr, part, at.map(|a| a + k))?;
            }
        }
        *sel = Selection::items(out.clone());
        Ok(json!({"ids": out.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

fn create_outlines(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        let db = designcraft_fonts::FontDb::global();
        let mut out = Vec::new();
        for id in &ids {
            let Some(it) = d.item(*id).cloned() else { continue };
            let Content::Text(tf) = &it.content else {
                out.push(*id);
                continue;
            };
            let cs = designcraft_compose::compose_story(d, tf.story, &Default::default());
            let Some(ft) = cs.frame(*id) else { continue };
            // One path per text look (fill, stroke).
            let mut runs: Vec<((String, f32, String, f32, f64), BezPath)> = Vec::new();
            for l in &ft.lines {
                for g in l.glyphs.iter().filter(|g| g.visible) {
                    let st = &cs.styles[g.style as usize];
                    let outline = db.outline(&g.face, g.gid);
                    if outline.elements().is_empty() {
                        continue;
                    }
                    let skew = if st.skew != 0.0 { Affine::new([1.0, 0.0, -st.skew.to_radians().tan(), 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
                    let shatai = st.shatai_xf(g, l.baseline).unwrap_or(Affine::IDENTITY);
                    let a = shatai * Affine::translate((g.x, l.baseline + g.y)) * skew * Affine::scale_non_uniform(g.sx, g.sy);
                    let key = (st.fill.clone(), st.fill_tint, st.stroke.clone(), st.stroke_tint, st.stroke_weight);
                    let k = match runs.iter().position(|r| r.0 == key) {
                        Some(k) => k,
                        None => {
                            runs.push((key, BezPath::new()));
                            runs.len() - 1
                        }
                    };
                    let bp = &mut runs[k].1;
                    bp.extend((a * outline.as_ref().clone()).elements().iter().copied());
                }
            }
            if runs.is_empty() {
                continue;
            }
            let (sr, _) = spread_xf(d, *id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
            let at = d.spread(sr).and_then(|sp| sp.items.iter().position(|i| i.id == *id));
            let text_xf = d.text_xf(&it);
            let make = |d: &mut Document, ((fill, ft, stroke, stt, sw), bp): ((String, f32, String, f32, f64), BezPath)| {
                let mut o = Item::new(ItemId(d.alloc()), it.layer, Shape::Path, PathData::from_bezpath(&bp));
                o.xf = text_xf;
                o.fill = Fill { swatch: fill, tint: ft, ..Fill::none() };
                o.stroke = if stroke == designcraft_color::swatch::NONE {
                    Stroke::none()
                } else {
                    Stroke { swatch: stroke, tint: stt, weight: sw, ..Stroke::default() }
                };
                o
            };
            let mut parts: Vec<Item> = runs.into_iter().map(|r| make(d, r)).collect();
            d.remove_item(*id)?;
            let new = if parts.len() == 1
                && let Some(one) = parts.pop()
            {
                one
            } else {
                let mut g = Item::new(ItemId(d.alloc()), it.layer, Shape::Group, PathData::default());
                for p in &mut parts {
                    p.xf = Affine::IDENTITY;
                }
                g.xf = it.xf;
                g.content = Content::Group { items: parts.into_iter().map(Arc::new).collect() };
                g
            };
            out.push(new.id);
            d.insert_item(sr, new, at)?;
        }
        *sel = Selection::items(out.clone());
        Ok(json!({"ids": out.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

fn split_path(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("path.split", "missing id"))?;
    let at = point_param(p, "at").ok_or_else(|| bad("path.split", "missing at"))?;
    s.edit(|d, sel| {
        let (sr, xf) = spread_xf(d, id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let it = d.item(id).cloned().ok_or(designcraft_doc::DocError::NoItem(id))?;
        if !matches!(it.content, Content::Unassigned) {
            return Err(bad("path.split", "only empty paths and frames can be cut"));
        }
        let (si, seg, t, _, _) = it.path.nearest(xf.inverse() * at).ok_or_else(|| bad("path.split", "the object has no path"))?;
        let mut sp = it.path.subpaths[si].clone();
        // Insert an anchor at the cut unless it's (nearly) on one.
        let ai = if t < 1e-3 {
            seg
        } else if t > 1.0 - 1e-3 {
            (seg + 1) % sp.anchors.len()
        } else {
            sp.insert_anchor(seg, t)
        };
        let n = sp.anchors.len();
        let mut pieces: Vec<SubPath> = Vec::new();
        if sp.closed {
            // Open at the cut: it becomes both ends.
            let mut a: Vec<_> = sp.anchors[ai..].iter().chain(sp.anchors[..ai].iter()).copied().collect();
            a.push(sp.anchors[ai]);
            if let Some(f) = a.first_mut() {
                f.h_in = f.p;
            }
            if let Some(l) = a.last_mut() {
                l.h_out = l.p;
            }
            pieces.push(SubPath::new(a, false));
        } else {
            if ai == 0 || ai >= n - 1 {
                return Err(bad("path.split", "that's an end point"));
            }
            let (mut a, mut b) = (sp.anchors[..=ai].to_vec(), sp.anchors[ai..].to_vec());
            if let Some(l) = a.last_mut() {
                l.h_out = l.p;
            }
            if let Some(f) = b.first_mut() {
                f.h_in = f.p;
            }
            pieces.push(SubPath::new(a, false));
            pieces.push(SubPath::new(b, false));
        }
        sp.anchors.clear();
        let mut ids = vec![id];
        let first = pieces.remove(0);
        let it_m = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        it_m.path.subpaths[si] = first;
        it_m.shape = if it_m.shape == Shape::GraphicLine { Shape::GraphicLine } else { Shape::Path };
        for piece in pieces {
            let mut other = it.clone();
            other.id = ItemId(d.alloc());
            other.path = PathData::single(piece);
            other.shape = Shape::Path;
            ids.push(other.id);
            d.insert_item(sr, other, None)?;
        }
        *sel = Selection::items(ids.clone());
        Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_paths_make_holes_and_release() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [150, 150, 250, 250]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [a], "swatch": "[Black]"})).unwrap();
        let r = s.execute("object.makeCompoundPath", &json!({"ids": [a, b]})).unwrap();
        assert_eq!(r["id"], a);
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(ItemId(b)).is_none());
        let it = d.item(ItemId(a)).unwrap();
        assert_eq!(it.path.subpaths.len(), 2);
        // The inner square is a hole: the centre stays white, the ring is black.
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        assert!(img.pixel(200, 200)[0] > 200, "hole: {:?}", img.pixel(200, 200));
        assert!(img.pixel(120, 200)[0] < 60, "ring: {:?}", img.pixel(120, 200));
        let r = s.execute("object.releaseCompoundPath", &json!({})).unwrap();
        assert_eq!(r["ids"].as_array().unwrap().len(), 2);
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn pathfinder_subtracts_front_from_back() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [200, 100, 400, 300], "shape": "ellipse"})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a, b]})).unwrap();
        let r = s.execute("object.pathfinder", &json!({"op": "subtract"})).unwrap();
        assert_eq!(r["id"], a);
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(ItemId(b)).is_none());
        let bb = d.item(ItemId(a)).unwrap().bounds();
        assert!((bb.x1 - 300.0).abs() < 0.5 && (bb.x0 - 100.0).abs() < 0.5, "{bb:?}");
        assert!(s.execute("object.pathfinder", &json!({"op": "nope"})).is_err());
    }

    #[test]
    fn gradient_feather_fades_on_screen_and_in_pdf() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
        s.execute("object.gradientFeather", &json!({"ids": [id], "from": [100, 150], "to": [300, 150]})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let (l, m, r) = (img.pixel(105, 150)[0], img.pixel(200, 150)[0], img.pixel(295, 150)[0]);
        assert!(l < 60 && m > l + 40 && r > 220, "fades left to right: {l} {m} {r}");
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        let t = String::from_utf8_lossy(&pdf);
        assert!(t.contains("/SMask") || t.contains("/Luminosity"), "a soft mask");
    }

    #[test]
    fn create_outlines_turns_text_into_paths() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Outline"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let r = s.execute("type.createOutlines", &json!({})).unwrap();
        let new = ItemId(r["ids"][0].as_u64().unwrap());
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(ItemId(id)).is_none());
        let it = d.item(new).unwrap();
        assert_eq!(it.shape, Shape::Path);
        assert!(it.path.subpaths.len() >= 7, "a contour or more per letter: {}", it.path.subpaths.len());
        let b = it.bounds();
        assert!(b.x0 >= 72.0 && b.y0 >= 72.0 && b.x1 < 400.0, "{b:?}");
        assert!(d.stories.is_empty() || !d.stories.values().any(|st| st.frames.contains(&ItemId(id))));
    }

    #[test]
    fn scissors_split_open_and_closed_paths() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let line = s.execute("line.create", &json!({"a": [100, 100], "b": [300, 100]})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("path.split", &json!({"id": line, "at": [200, 101]})).unwrap();
        assert_eq!(r["ids"].as_array().unwrap().len(), 2);
        let d = s.doc().unwrap().doc.clone();
        let w: Vec<f64> =
            r["ids"].as_array().unwrap().iter().map(|i| d.item(ItemId(i.as_u64().unwrap())).unwrap().bounds().width().round()).collect();
        assert_eq!(w, [100.0, 100.0]);
        let rect = s.execute("frame.create", &json!({"rect": [100, 200, 200, 300]})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("path.split", &json!({"id": rect, "at": [150, 200]})).unwrap();
        assert_eq!(r["ids"].as_array().unwrap().len(), 1);
        let d = s.doc().unwrap().doc.clone();
        let it = d.item(ItemId(rect)).unwrap();
        assert!(!it.path.is_closed());
        assert_eq!(it.path.anchor_count(), 6);
    }
}
