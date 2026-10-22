//! Anchor-point editing (the Pen tool group): Add Anchor Point, Delete Anchor Point and Convert
//! Direction Point, and the Pathfinder panel's Join Path and Convert Point. Positions are in spread
//! coordinates; anchors are `(subpath, index)`.

use designcraft_doc::{Document, ItemId, Shape};
use designcraft_geom::{Affine, Anchor, AnchorKind, Point};
use serde_json::{Value, json};

use super::object::path_editable;
use super::{CommandSpec, bad, cmd, has_doc, has_selection, point_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "path.addAnchor",
            "Add Anchor Point",
            [],
            None,
            "{id, at: [x,y] (spread coords; the nearest point on the path), tolerance?: pt (default 4)} → {subpath, anchor}",
            has_doc,
            add_anchor
        ),
        cmd!(
            "path.deleteAnchor",
            "Delete Anchor Point",
            [],
            None,
            "{id, subpath, anchor} — removes the point (and the object when no path is left)",
            has_doc,
            delete_anchor
        ),
        cmd!(
            "path.convertAnchor",
            "Convert Direction Point",
            [],
            None,
            "{id, subpath, anchor, to?: [x,y] (spread coords: drag out symmetric handles to there), corner?: bool} — without `to`, a smooth point becomes a corner and a corner gets smooth handles",
            has_doc,
            convert_anchor
        ),
        cmd!(
            "path.erase",
            "Erase",
            [],
            None,
            "{id, points: [[x,y], …] (spread coords: the Erase tool's drag), tolerance?: pt (default 4)} — removes the stretch of path the drag ran along",
            has_doc,
            |s, p| erase_or_smooth(s, p, false)
        ),
        cmd!(
            "path.smooth",
            "Smooth",
            [],
            None,
            "{id, points: [[x,y], …] (spread coords), tolerance?: pt (default 4)} — re-fits the stretch the drag ran along with fewer, smoother anchors",
            has_doc,
            |s, p| erase_or_smooth(s, p, true)
        ),
        cmd!(
            "path.join",
            "Join",
            ["Object", "Paths"],
            None,
            "{ids?} — one open path: closes it; two open paths: joins them where their ends are nearest (coincident ends become one point) into the first, and removes the second → {id}",
            has_selection,
            join_paths
        ),
        cmd!(
            "path.convertPoints",
            "Convert Point",
            [],
            None,
            "{kind: plain|corner|smooth|symmetrical, ids?} — the Direct Selection tool's selected points, or every point of the selected paths: plain (no direction handles), corner (independent handles), smooth (collinear handles), symmetrical (collinear, equal length) → {converted}",
            has_selection,
            convert_points
        ),
    ]
}

/// Join Path: close one open path, or join two open paths at their nearest ends.
fn join_paths(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = super::targets(s, p)?;
    s.edit(|d, sel| {
        let paths: Vec<ItemId> = ids.iter().copied().filter(|id| d.item(*id).is_some_and(|it| !it.is_group() && it.text_frame().is_none())).collect();
        let ids: Vec<ItemId> = paths.iter().copied().filter(|id| path_editable(d, *id, "path.join").is_ok()).collect();
        // A request for two paths never falls back to closing the one that is editable.
        if ids.len() < paths.len().min(2) {
            for id in &paths {
                path_editable(d, *id, "path.join")?;
            }
        }
        let open = |d: &Document, id: ItemId| d.item(id).and_then(|it| it.path.subpaths.iter().position(|sp| !sp.closed && sp.anchors.len() > 1));
        match ids.as_slice() {
            [id] => {
                let it = d.item_mut(*id).ok_or(designcraft_doc::DocError::NoItem(*id))?;
                let mut closed = 0;
                for sp in it.path.subpaths.iter_mut().filter(|sp| !sp.closed && sp.anchors.len() > 1) {
                    sp.closed = true;
                    closed += 1;
                }
                if closed == 0 {
                    return Err(bad("path.join", "the path is already closed"));
                }
                as_path(&mut it.shape);
                Ok(json!({"id": id.0, "closed": closed}))
            }
            [a, b] => {
                let (Some(sa), Some(sb)) = (open(d, *a), open(d, *b)) else {
                    return Err(bad("path.join", "select two open paths"));
                };
                // b's path in a's space.
                let inv_a = to_inner(d, *a).ok_or(designcraft_doc::DocError::NoItem(*a))?;
                let xf_b = to_inner(d, *b).ok_or(designcraft_doc::DocError::NoItem(*b))?.inverse();
                let m = inv_a * xf_b;
                let mut pb = d.item(*b).and_then(|it| it.path.subpaths.get(sb).cloned()).ok_or(designcraft_doc::DocError::NoItem(*b))?;
                for an in &mut pb.anchors {
                    *an = Anchor { p: m * an.p, h_in: m * an.h_in, h_out: m * an.h_out, kind: an.kind };
                }
                let ita = d.item_mut(*a).ok_or(designcraft_doc::DocError::NoItem(*a))?;
                let pa = ita.path.subpaths.get_mut(sa).ok_or_else(|| bad("path.join", "no such subpath"))?;
                // The pair of ends nearest each other; then a's end meets b's start.
                let ends = |sp: &designcraft_geom::SubPath| (sp.anchors.first().map(|x| x.p), sp.anchors.last().map(|x| x.p));
                let ((Some(a0), Some(a1)), (Some(b0), Some(b1))) = (ends(pa), ends(&pb)) else {
                    return Err(bad("path.join", "select two open paths"));
                };
                let pairs = [(a1, b0, false, false), (a1, b1, false, true), (a0, b0, true, false), (a0, b1, true, true)];
                let Some(&(_, _, rev_a, rev_b)) = pairs.iter().min_by(|x, y| (x.0 - x.1).hypot().total_cmp(&(y.0 - y.1).hypot())) else {
                    return Err(bad("path.join", "select two open paths"));
                };
                if rev_a {
                    pa.reverse();
                }
                if rev_b {
                    pb.reverse();
                }
                let mut rest = pb.anchors.into_iter();
                if let Some(first) = rest.next() {
                    match pa.anchors.last_mut() {
                        Some(last) if (last.p - first.p).hypot() < 0.01 => *last = Anchor::with_handles(last.p, last.h_in, first.h_out),
                        _ => pa.anchors.push(first),
                    }
                }
                pa.anchors.extend(rest);
                as_path(&mut ita.shape);
                d.remove_item(*b)?;
                *sel = designcraft_doc::Selection::items(vec![*a]);
                Ok(json!({"id": a.0}))
            }
            _ => Err(bad("path.join", "select one or two open paths")),
        }
    })
}

/// Convert Point: the selected points (or every point of the selected paths) to a point type.
fn convert_points(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = super::str_param(p, "kind").unwrap_or("plain").to_string();
    if !matches!(kind.as_str(), "plain" | "corner" | "smooth" | "symmetrical") {
        return Err(bad("path.convertPoints", format!("unknown kind `{kind}`")));
    }
    let mut points: Vec<(ItemId, usize, usize)> = if p.get("ids").is_none() { s.doc()?.selection.anchors.clone() } else { Vec::new() };
    if points.is_empty() {
        for id in super::targets(s, p)? {
            if let Some(it) = s.doc()?.doc.item(id) {
                points.extend(it.path.anchors().map(|(si, ai, _)| (id, si, ai)));
            }
        }
    }
    s.edit(|d, _| {
        let mut converted = 0;
        let mut refused = None;
        for (id, si, ai) in &points {
            if d.item(*id).is_none_or(|it| it.is_group()) {
                continue;
            }
            if let Err(e) = path_editable(d, *id, "path.convertPoints") {
                refused.get_or_insert(e);
                continue;
            }
            let Some(it) = d.item_mut(*id) else { continue };
            let Some(sp) = it.path.subpaths.get_mut(*si) else { continue };
            let Some(a) = sp.anchors.get(*ai).copied() else { continue };
            let auto = smooth_auto(&sp.anchors, *ai, sp.closed);
            // The handles: the point's own, or ones worked out from its neighbours.
            let (h_in, h_out) = if a.has_in() || a.has_out() { (a.h_in, a.h_out) } else { (auto.h_in, auto.h_out) };
            let new = match kind.as_str() {
                "plain" => Anchor::corner(a.p),
                "corner" => Anchor { p: a.p, h_in, h_out, kind: AnchorKind::Corner },
                _ => {
                    let dir = if (h_out - h_in).hypot() > 1e-9 { h_out - h_in } else { auto.h_out - auto.h_in };
                    let len = dir.hypot();
                    if len < 1e-9 {
                        a
                    } else {
                        let u = dir / len;
                        let (li, lo) = ((a.p - h_in).hypot(), (h_out - a.p).hypot());
                        let (li, lo) = if kind == "symmetrical" { ((li + lo) / 2.0, (li + lo) / 2.0) } else { (li, lo) };
                        Anchor { p: a.p, h_in: a.p - u * li, h_out: a.p + u * lo, kind: AnchorKind::Smooth }
                    }
                }
            };
            if let Some(slot) = sp.anchors.get_mut(*ai) {
                *slot = new;
                converted += 1;
            }
            as_path(&mut it.shape);
        }
        if let (0, Some(e)) = (converted, refused) {
            return Err(e);
        }
        Ok(json!({"converted": converted}))
    })
}

/// Erase / Smooth: the stretch of one subpath touched by the drag.
fn erase_or_smooth(s: &mut Session, p: &Value, smooth: bool) -> Result<Value> {
    let cmd = if smooth { "path.smooth" } else { "path.erase" };
    let id = super::id_param(p, "id").ok_or_else(|| bad(cmd, "missing id"))?;
    let tol = p.get("tolerance").and_then(Value::as_f64).unwrap_or(4.0);
    let pts: Vec<Point> = p
        .get("points")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| Some(Point::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))).collect())
        .unwrap_or_default();
    s.edit(|d, _| {
        path_editable(d, id, cmd)?;
        let inv = to_inner(d, id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let scale = inv.determinant().abs().sqrt().max(1e-9);
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        // Where the drag touched the path, per subpath.
        let mut hits: Vec<(usize, usize, f64)> = Vec::new();
        for q in &pts {
            if let Some((si, seg, t, _, dist)) = it.path.nearest(inv * *q)
                && dist <= tol * scale
            {
                hits.push((si, seg, t));
            }
        }
        let Some(si) =
            (0..it.path.subpaths.len()).max_by_key(|i| hits.iter().filter(|h| h.0 == *i).count()).filter(|i| hits.iter().any(|h| h.0 == *i))
        else {
            return Err(bad(cmd, "the drag didn't touch the path"));
        };
        let on: Vec<(usize, f64)> = hits.iter().filter(|h| h.0 == si).map(|h| (h.1, h.2)).collect();
        let k = |a: &(usize, f64)| a.0 as f64 + a.1;
        let (Some(&a), Some(&b)) = (on.iter().min_by(|x, y| k(x).total_cmp(&k(y))), on.iter().max_by(|x, y| k(x).total_cmp(&k(y)))) else {
            return Err(bad(cmd, "the drag didn't touch the path"));
        };
        let sp = it.path.subpaths[si].clone();
        if smooth {
            it.path.subpaths[si] = designcraft_geom::edit_path::smooth(&sp, a, b, tol * scale);
        } else {
            let parts = designcraft_geom::edit_path::erase(&sp, a, b);
            it.path.subpaths.remove(si);
            for (k, part) in parts.into_iter().enumerate() {
                it.path.subpaths.insert(si + k, part);
            }
        }
        as_path(&mut it.shape);
        if it.path.subpaths.is_empty() {
            d.remove_item(id)?;
            return Ok(json!({"deleted": true}));
        }
        Ok(json!({"subpaths": it.path.subpaths.len()}))
    })
}

/// Spread → item-space transform of `id`.
fn to_inner(d: &Document, id: ItemId) -> Option<Affine> {
    let loc = d.find(id)?;
    Some((d.parent_xf(&loc) * d.item_at(&loc)?.xf).inverse())
}

fn anchor_param(p: &Value, cmd: &str) -> Result<(ItemId, usize, usize)> {
    let id = super::id_param(p, "id").ok_or_else(|| bad(cmd, "missing id"))?;
    let si = p.get("subpath").and_then(Value::as_u64).unwrap_or(0) as usize;
    let ai = p.get("anchor").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "missing anchor"))? as usize;
    Ok((id, si, ai))
}

/// Editing points makes a drawn rectangle/ellipse/polygon an ordinary path.
fn as_path(shape: &mut Shape) {
    if matches!(shape, Shape::Rectangle | Shape::Oval | Shape::Polygon) {
        *shape = Shape::Path;
    }
}

fn add_anchor(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("path.addAnchor", "missing id"))?;
    let at = point_param(p, "at").ok_or_else(|| bad("path.addAnchor", "missing at"))?;
    let tol = p.get("tolerance").and_then(Value::as_f64).unwrap_or(4.0);
    s.edit(|d, _| {
        path_editable(d, id, "path.addAnchor")?;
        let inv = to_inner(d, id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let q = inv * at;
        let (si, seg, t, _, dist) = it.path.nearest(q).ok_or_else(|| bad("path.addAnchor", "the object has no path"))?;
        // Tolerance is in spread units; the item space may be scaled.
        let scale = inv.determinant().abs().sqrt().max(1e-9);
        if dist > tol * scale {
            return Err(bad("path.addAnchor", "not on the path"));
        }
        let t = t.clamp(1e-4, 1.0 - 1e-4);
        let ai = it.path.subpaths[si].insert_anchor(seg, t);
        as_path(&mut it.shape);
        Ok(json!({"subpath": si, "anchor": ai}))
    })
}

fn delete_anchor(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, si, ai) = anchor_param(p, "path.deleteAnchor")?;
    s.edit(|d, sel| {
        path_editable(d, id, "path.deleteAnchor")?;
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let sp = it.path.subpaths.get_mut(si).ok_or_else(|| bad("path.deleteAnchor", "no such subpath"))?;
        if ai >= sp.anchors.len() {
            return Err(bad("path.deleteAnchor", "no such anchor"));
        }
        sp.anchors.remove(ai);
        // A subpath needs two points (three to stay closed).
        if sp.anchors.len() < 2 {
            it.path.subpaths.remove(si);
        } else if sp.closed && sp.anchors.len() < 3 {
            sp.closed = false;
        }
        as_path(&mut it.shape);
        if it.path.subpaths.is_empty() {
            d.remove_item(id)?;
            sel.items.retain(|i| *i != id);
            return Ok(json!({"deleted": true}));
        }
        Ok(json!({"deleted": false}))
    })
}

fn convert_anchor(s: &mut Session, p: &Value) -> Result<Value> {
    let (id, si, ai) = anchor_param(p, "path.convertAnchor")?;
    let to = point_param(p, "to");
    let corner = p.get("corner").and_then(Value::as_bool);
    s.edit(|d, _| {
        path_editable(d, id, "path.convertAnchor")?;
        let inv = to_inner(d, id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let sp = it.path.subpaths.get_mut(si).ok_or_else(|| bad("path.convertAnchor", "no such subpath"))?;
        let n = sp.anchors.len();
        if ai >= n {
            return Err(bad("path.convertAnchor", "no such anchor"));
        }
        let closed = sp.closed;
        let a = sp.anchors[ai];
        let has_handles = a.has_in() || a.has_out();
        let new = match (to, corner) {
            (Some(t), _) => Anchor::smooth(a.p, inv * t),
            (None, Some(true)) => Anchor::corner(a.p),
            (None, Some(false)) => smooth_auto(&sp.anchors, ai, closed),
            (None, None) if has_handles => Anchor::corner(a.p),
            (None, None) => smooth_auto(&sp.anchors, ai, closed),
        };
        sp.anchors[ai] = new;
        as_path(&mut it.shape);
        Ok(json!({"kind": if new.kind == AnchorKind::Smooth { "smooth" } else { "corner" }}))
    })
}

/// Smooth handles for a corner: parallel to the line through its neighbours, a third of the way
/// to each.
fn smooth_auto(anchors: &[Anchor], i: usize, closed: bool) -> Anchor {
    let n = anchors.len();
    let p = anchors[i].p;
    let prev = if i > 0 {
        Some(anchors[i - 1].p)
    } else if closed {
        Some(anchors[n - 1].p)
    } else {
        None
    };
    let next = if i + 1 < n {
        Some(anchors[i + 1].p)
    } else if closed {
        Some(anchors[0].p)
    } else {
        None
    };
    let (a, b) = (prev.unwrap_or(p), next.unwrap_or(p));
    let dir = b - a;
    let len = dir.hypot();
    if len < 1e-9 {
        return anchors[i];
    }
    let u = dir / len;
    let (lin, lout) = ((p - a).hypot() / 3.0, (b - p).hypot() / 3.0);
    let lin = if prev.is_some() { lin } else { lout };
    let lout = if next.is_some() { lout } else { lin };
    Anchor { p, h_in: p - u * lin, h_out: p + u * lout, kind: AnchorKind::Smooth }
}

/// The anchor of item `id` nearest to `at` (spread coords), within `tol`.
pub fn anchor_near(d: &Document, id: ItemId, at: Point, tol: f64) -> Option<(usize, usize)> {
    let loc = d.find(id)?;
    let m = d.parent_xf(&loc) * d.item_at(&loc)?.xf;
    d.item(id)?
        .path
        .anchors()
        .map(|(si, ai, a)| (si, ai, ((m * a.p) - at).hypot()))
        .filter(|x| x.2 <= tol)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|x| (x.0, x.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
        s.execute("line.create", &json!({"a": a, "b": b})).unwrap()["id"].as_u64().unwrap()
    }

    /// Join Path closes one open path, and joins two open paths at their nearest ends.
    #[test]
    fn join_path_closes_one_path_and_joins_two() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = line(&mut s, [100.0, 100.0], [200.0, 100.0]);
        // b runs back towards a: its end meets a's end at (200, 100).
        let b = line(&mut s, [200.0, 200.0], [200.0, 100.0]);
        let r = s.execute("path.join", &json!({"ids": [a, b]})).unwrap();
        assert_eq!(r["id"], a);
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(ItemId(b)).is_none());
        let sp = &d.item(ItemId(a)).unwrap().path.subpaths[0];
        assert_eq!(sp.anchors.len(), 3, "the coincident ends become one point");
        assert!(!sp.closed);
        let bb = d.item(ItemId(a)).unwrap().bounds();
        assert!((bb.width() - 100.0).abs() < 0.5 && (bb.height() - 100.0).abs() < 0.5, "{bb:?}");
        // Joining it again closes it.
        s.execute("path.join", &json!({"ids": [a]})).unwrap();
        assert!(s.doc().unwrap().doc.item(ItemId(a)).unwrap().path.subpaths[0].closed);
        assert!(s.execute("path.join", &json!({"ids": [a]})).is_err(), "nothing left to close");
        // Ends apart are joined with a straight segment.
        let c = line(&mut s, [300.0, 300.0], [400.0, 300.0]);
        let e = line(&mut s, [300.0, 400.0], [400.0, 400.0]);
        s.execute("path.join", &json!({"ids": [c, e]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.item(ItemId(c)).unwrap().path.subpaths[0].anchors.len(), 4);
    }

    /// Convert Point turns the selected points (or a whole path's) plain, corner, smooth or symmetrical.
    #[test]
    fn convert_point_kinds() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 200]})).unwrap()["id"].as_u64().unwrap();
        let anchor = |s: &Session, i: usize| s.doc().unwrap().doc.item(ItemId(id)).unwrap().path.subpaths[0].anchors[i];
        s.execute("path.convertPoints", &json!({"kind": "smooth", "ids": [id]})).unwrap();
        let a = anchor(&s, 0);
        assert_eq!(a.kind, AnchorKind::Smooth);
        assert!(a.has_in() && a.has_out());
        let (di, dout) = (a.p - a.h_in, a.h_out - a.p);
        assert!((di.x * dout.y - di.y * dout.x).abs() < 1e-6, "collinear handles");
        s.execute("path.convertPoints", &json!({"kind": "symmetrical", "ids": [id]})).unwrap();
        let a = anchor(&s, 1);
        assert!(((a.p - a.h_in).hypot() - (a.h_out - a.p).hypot()).abs() < 1e-6, "equal handles");
        s.execute("path.convertPoints", &json!({"kind": "plain", "ids": [id]})).unwrap();
        let a = anchor(&s, 2);
        assert!(!a.has_in() && !a.has_out() && a.kind == AnchorKind::Corner);
        s.execute("path.convertPoints", &json!({"kind": "corner", "ids": [id]})).unwrap();
        let a = anchor(&s, 2);
        assert!(a.has_in() && a.has_out() && a.kind == AnchorKind::Corner, "a corner point gets independent handles");
        assert!(s.execute("path.convertPoints", &json!({"kind": "nope", "ids": [id]})).is_err());
        assert_eq!(s.doc().unwrap().doc.item(ItemId(id)).unwrap().shape, Shape::Path);
    }

    #[test]
    fn erase_and_smooth_along_a_drag() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let pts: Vec<Value> = (0..=20).map(|i| json!({"p": [100.0 + i as f64 * 20.0, if i % 2 == 0 { 300.0 } else { 306.0 }]})).collect();
        let id = s.execute("path.create", &json!({"anchors": pts})).unwrap()["id"].as_u64().unwrap();
        let n = s.doc().unwrap().doc.item(ItemId(id)).unwrap().path.anchor_count();
        let drag: Vec<Value> = (0..=12).map(|i| json!([150.0 + i as f64 * 20.0, 303.0])).collect();
        s.execute("path.smooth", &json!({"id": id, "points": drag})).unwrap();
        let after = s.doc().unwrap().doc.item(ItemId(id)).unwrap().path.anchor_count();
        assert!(after < n, "{after} < {n}");
        let drag: Vec<Value> = (0..=5).map(|i| json!([200.0 + i as f64 * 20.0, 303.0])).collect();
        let r = s.execute("path.erase", &json!({"id": id, "points": drag})).unwrap();
        assert_eq!(r["subpaths"], 2, "a gap in the middle");
        assert!(s.execute("path.erase", &json!({"id": id, "points": [[100, 600]]})).is_err());
    }

    #[test]
    fn add_convert_delete_anchor_points() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let id = ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap());
        let count = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().path.anchor_count();
        assert_eq!(count(&s), 4);
        assert!(s.execute("path.addAnchor", &json!({"id": id.0, "at": [150, 150]})).is_err(), "not on the path");
        let r = s.execute("path.addAnchor", &json!({"id": id.0, "at": [150, 101]})).unwrap();
        assert_eq!(count(&s), 5);
        let ai = r["anchor"].as_u64().unwrap() as usize;
        let d = s.doc().unwrap().doc.clone();
        let it = d.item(id).unwrap();
        assert_eq!(it.shape, Shape::Path);
        assert!((it.path.subpaths[0].anchors[ai].p.y - 100.0).abs() < 1e-6);
        assert_eq!(anchor_near(&d, id, Point::new(150.5, 100.5), 3.0), Some((0, ai)));
        // Drag out handles, then click again to make it a corner.
        s.execute("path.convertAnchor", &json!({"id": id.0, "anchor": ai, "to": [170, 90]})).unwrap();
        let a = s.doc().unwrap().doc.item(id).unwrap().path.subpaths[0].anchors[ai];
        assert_eq!(a.kind, AnchorKind::Smooth);
        assert!((a.h_out - Point::new(170.0, 90.0)).hypot() < 1e-9 && (a.h_in - Point::new(130.0, 110.0)).hypot() < 1e-9);
        assert_eq!(s.execute("path.convertAnchor", &json!({"id": id.0, "anchor": ai})).unwrap()["kind"], "corner");
        assert_eq!(s.execute("path.convertAnchor", &json!({"id": id.0, "anchor": 0})).unwrap()["kind"], "smooth");
        s.execute("path.deleteAnchor", &json!({"id": id.0, "anchor": ai})).unwrap();
        assert_eq!(count(&s), 4);
        // Deleting down to one point removes the object.
        for _ in 0..2 {
            s.execute("path.deleteAnchor", &json!({"id": id.0, "anchor": 0})).unwrap();
        }
        assert!(!s.doc().unwrap().doc.item(id).unwrap().path.is_closed(), "two points can't stay closed");
        let r = s.execute("path.deleteAnchor", &json!({"id": id.0, "anchor": 0})).unwrap();
        assert_eq!(r["deleted"], true);
        assert!(s.doc().unwrap().doc.item(id).is_none());
    }
}
