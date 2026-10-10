//! Object › Select (stacking order, container and content, objects in a group) and Object ›
//! Convert Shape.

use designcraft_doc::{Content, Document, Item, ItemId, ItemLoc, Selection, Shape, SpreadRef};
use designcraft_geom::corners::{CornerOptions, CornerShape};
use designcraft_geom::{Point, Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, has_selection, ok, str_param, targets};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "select.firstAbove", "First Object Above", ["Object", "Select"], Some("Cmd+Alt+Shift+]"), "{} — the frontmost object on the spread (layers included)", has_doc, |s, _| select_stacked(s, Step::Top)),
        cmd!(noundo "select.nextAbove", "Next Object Above", ["Object", "Select"], Some("Cmd+Alt+]"), "{}", has_doc, |s, _| select_stacked(s, Step::Up)),
        cmd!(noundo "select.nextBelow", "Next Object Below", ["Object", "Select"], Some("Cmd+Alt+["), "{}", has_doc, |s, _| select_stacked(s, Step::Down)),
        cmd!(noundo "select.lastBelow", "Last Object Below", ["Object", "Select"], Some("Cmd+Alt+Shift+["), "{} — the backmost object on the spread", has_doc, |s, _| select_stacked(s, Step::Bottom)),
        cmd!(noundo "select.container", "Container", ["Object", "Select"], None, "{} — the group or frame holding the selection", has_selection, |s, _| select_container(s)),
        cmd!(noundo "select.content", "Content", ["Object", "Select"], None, "{} — the graphic in a frame, or the first object in a group", has_selection, |s, _| select_content(s)),
        cmd!(noundo "select.nextInGroup", "Next Object in Group", ["Object", "Select"], None, "{}", has_selection, |s, _| select_sibling(s, 1)),
        cmd!(noundo "select.previousInGroup", "Previous Object in Group", ["Object", "Select"], None, "{}", has_selection, |s, _| select_sibling(s, -1)),
        cmd!(
            "object.convertShape",
            "Convert Shape",
            ["Object", "Convert Shape"],
            None,
            "{to: rectangle|roundedRectangle|beveledRectangle|inverseRoundedRectangle|ellipse|triangle|polygon|line|orthogonalLine|openPath|closedPath, ids?}",
            has_selection,
            convert_shape
        ),
    ]
}

#[derive(Clone, Copy)]
enum Step {
    Top,
    Up,
    Down,
    Bottom,
}

/// Selectable top-level items of spread `r`, back to front.
fn stacking(d: &Document, r: SpreadRef) -> Vec<ItemId> {
    let Some(sp) = d.spread(r) else { return vec![] };
    let mut v = Vec::new();
    for layer in d.layers.iter().rev() {
        if !layer.visible || layer.locked {
            continue;
        }
        v.extend(sp.items.iter().filter(|it| it.layer == layer.id && !it.hidden && !it.locked).map(|it| it.id));
    }
    v
}

fn select_stacked(s: &mut Session, how: Step) -> Result<Value> {
    let st = s.doc_mut()?;
    let cur = st.selection.items.first().and_then(|id| st.doc.find(*id).map(|l| (*id, l)));
    let r = cur.as_ref().map(|(_, l)| l.spread).or_else(|| st.doc.spread_refs().next()).ok_or_else(|| bad("select", "no spread"))?;
    let order = stacking(&st.doc, r);
    if order.is_empty() {
        return ok();
    }
    // Nested selections step from their top-level object.
    let at = cur
        .and_then(|(_, l)| st.doc.spread(l.spread).and_then(|sp| sp.items.get(l.top())).map(|it| it.id))
        .and_then(|id| order.iter().position(|x| *x == id));
    let n = order.len();
    let i = match (how, at) {
        (Step::Top, _) | (Step::Down, None) => n - 1,
        (Step::Bottom, _) | (Step::Up, None) => 0,
        (Step::Up, Some(i)) => (i + 1).min(n - 1),
        (Step::Down, Some(i)) => i.saturating_sub(1),
    };
    st.selection = Selection::items(vec![order[i]]);
    st.bump_revision();
    Ok(json!({"id": order[i].0}))
}

fn parent_loc(l: &ItemLoc) -> Option<ItemLoc> {
    (l.path.len() > 1).then(|| ItemLoc { spread: l.spread, path: l.path[..l.path.len() - 1].to_vec() })
}

fn select_container(s: &mut Session) -> Result<Value> {
    let st = s.doc_mut()?;
    let Some(id) = st.selection.items.first().copied() else { return ok() };
    if st.selection.content {
        st.selection = Selection::items(vec![id]);
    } else if let Some(p) = st.doc.find(id).and_then(|l| parent_loc(&l)).and_then(|l| st.doc.item_at(&l).map(|it| it.id)) {
        st.selection = Selection::items(vec![p]);
    } else {
        return ok();
    }
    st.bump_revision();
    Ok(json!({"id": st.selection.items[0].0}))
}

fn select_content(s: &mut Session) -> Result<Value> {
    let st = s.doc_mut()?;
    let Some(id) = st.selection.items.first().copied() else { return ok() };
    let Some(it) = st.doc.item(id) else { return ok() };
    if let Some(c) = it.children().first() {
        st.selection = Selection::items(vec![c.id]);
    } else if matches!(it.content, Content::Graphic(_)) {
        st.selection = Selection { content: true, ..Selection::items(vec![id]) };
    } else {
        return ok();
    }
    st.bump_revision();
    Ok(json!({"id": st.selection.items[0].0}))
}

fn select_sibling(s: &mut Session, dir: isize) -> Result<Value> {
    let st = s.doc_mut()?;
    let Some(id) = st.selection.items.first().copied() else { return ok() };
    let Some(l) = st.doc.find(id) else { return ok() };
    let Some(pl) = parent_loc(&l) else { return ok() };
    let Some(parent) = st.doc.item_at(&pl) else { return ok() };
    let kids = parent.children();
    let n = kids.len() as isize;
    let Some(&i) = l.path.last() else { return ok() };
    if n == 0 {
        return ok();
    }
    // InDesign's "next" goes down the stack (towards the back), wrapping.
    let j = (i as isize - dir).rem_euclid(n) as usize;
    let Some(next) = kids.get(j).map(|k| k.id) else { return ok() };
    st.selection = Selection::items(vec![next]);
    st.bump_revision();
    Ok(json!({"id": next.0}))
}

fn convert_shape(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.convertShape";
    let to = str_param(p, "to").ok_or_else(|| bad(ID, "`to` required"))?.to_string();
    let ids = targets(s, p)?;
    let sides = s.prefs.polygon_sides;
    let inset = s.prefs.star_inset;
    s.edit(|d, _| {
        let mut changed = 0;
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            if it.shape == Shape::Group {
                continue;
            }
            convert(it, &to, sides, inset).map_err(|e| bad(ID, e))?;
            changed += 1;
        }
        Ok(json!({"changed": changed}))
    })
}

fn convert(it: &mut Item, to: &str, sides: u32, inset: f64) -> std::result::Result<(), String> {
    let r: Rect = it.path.bounds().unwrap_or(Rect::ZERO);
    let text = matches!(it.content, Content::Text(_));
    let corners = |shape: CornerShape| CornerOptions::uniform(shape, (r.width().min(r.height()) / 6.0).clamp(1.0, 12.0));
    let (path, shape, c) = match to {
        "rectangle" => (shapes::rectangle(r), Shape::Rectangle, CornerOptions::default()),
        "roundedRectangle" => (shapes::rectangle(r), Shape::Rectangle, corners(CornerShape::Rounded)),
        "beveledRectangle" => (shapes::rectangle(r), Shape::Rectangle, corners(CornerShape::Bevel)),
        "inverseRoundedRectangle" => (shapes::rectangle(r), Shape::Rectangle, corners(CornerShape::InverseRounded)),
        "ellipse" => (shapes::ellipse(r), Shape::Oval, CornerOptions::default()),
        "triangle" => (super::object::polygon_in(r, 3, 0.0), Shape::Polygon, CornerOptions::default()),
        "polygon" => (super::object::polygon_in(r, sides, inset), Shape::Polygon, CornerOptions::default()),
        "line" | "orthogonalLine" if text => return Err("a text frame can't become a line".into()),
        "line" => (shapes::line(Point::new(r.x0, r.y0), Point::new(r.x1, r.y1)), Shape::GraphicLine, CornerOptions::default()),
        "orthogonalLine" => {
            let c = r.center();
            let (a, b) =
                if r.width() >= r.height() { (Point::new(r.x0, c.y), Point::new(r.x1, c.y)) } else { (Point::new(c.x, r.y0), Point::new(c.x, r.y1)) };
            (shapes::line(a, b), Shape::GraphicLine, CornerOptions::default())
        }
        "openPath" | "closedPath" => {
            let closed = to == "closedPath";
            if !closed && text {
                return Err("a text frame needs a closed path".into());
            }
            let mut path = it.path.clone();
            for sp in &mut path.subpaths {
                sp.closed = closed;
            }
            (path, Shape::Path, it.corners)
        }
        o => return Err(format!("unknown shape `{o}`")),
    };
    it.path = path;
    it.shape = shape;
    it.corners = c;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    fn ids(s: &Session) -> Vec<u64> {
        s.doc().unwrap().selection.items.iter().map(|i| i.0).collect()
    }

    #[test]
    fn select_by_stacking_order_and_groups() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let mk = |s: &mut Session, x: f64| s.execute("frame.create", &json!({"rect": [x, 100.0, x + 50.0, 150.0]})).unwrap()["id"].as_u64().unwrap();
        let (a, b, c) = (mk(&mut s, 0.0), mk(&mut s, 60.0), mk(&mut s, 120.0));
        s.execute("select.lastBelow", &json!({})).unwrap();
        assert_eq!(ids(&s), [a]);
        s.execute("select.nextAbove", &json!({})).unwrap();
        assert_eq!(ids(&s), [b]);
        s.execute("select.firstAbove", &json!({})).unwrap();
        assert_eq!(ids(&s), [c]);
        s.execute("select.nextAbove", &json!({})).unwrap();
        assert_eq!(ids(&s), [c], "already on top");
        s.execute("select.nextBelow", &json!({})).unwrap();
        assert_eq!(ids(&s), [b]);
        let g = s.execute("object.group", &json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
        s.execute("edit.deselectAll", &json!({})).unwrap();
        s.doc_mut().unwrap().selection = designcraft_doc::Selection::items(vec![designcraft_doc::ItemId(g)]);
        s.execute("select.content", &json!({})).unwrap();
        let first = ids(&s)[0];
        assert!(first == a || first == b);
        s.execute("select.nextInGroup", &json!({})).unwrap();
        assert_ne!(ids(&s)[0], first);
        s.execute("select.nextInGroup", &json!({})).unwrap();
        assert_eq!(ids(&s)[0], first, "wraps");
        s.execute("select.container", &json!({})).unwrap();
        assert_eq!(ids(&s), [g]);
    }

    #[test]
    fn convert_shape_keeps_bounds() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 160]})).unwrap()["id"].as_u64().unwrap();
        let item = |s: &Session| s.doc().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().clone();
        for to in ["ellipse", "triangle", "polygon", "roundedRectangle", "rectangle"] {
            s.execute("object.convertShape", &json!({"to": to})).unwrap();
            let b = item(&s).bounds();
            assert!((b.width() - 100.0).abs() < 1e-6 && (b.height() - 60.0).abs() < 1e-6, "{to}: {b:?}");
        }
        s.execute("object.convertShape", &json!({"to": "orthogonalLine"})).unwrap();
        let it = item(&s);
        assert_eq!(it.shape, designcraft_doc::Shape::GraphicLine);
        assert_eq!(it.bounds().height(), 0.0);
        s.execute("object.convertShape", &json!({"to": "closedPath"})).unwrap();
        assert!(item(&s).path.subpaths[0].closed);
        let t = s.execute("frame.create", &json!({"rect": [0, 0, 50, 50], "content": "text"})).unwrap()["id"].as_u64().unwrap();
        assert!(s.execute("object.convertShape", &json!({"to": "line", "ids": [t]})).is_err());
        assert!(s.execute("object.convertShape", &json!({"to": "blob", "ids": [a]})).is_err());
    }
}
