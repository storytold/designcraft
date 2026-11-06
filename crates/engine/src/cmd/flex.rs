//! Flex Layout: Object › Flex Layout and the Flex Layout panel. A container is a frame whose
//! nested objects [`designcraft_doc::flex::layout`] places; [`sync`] lays every container out
//! after each edit, so moving, resizing or editing an object inside one reflows it.

use std::sync::Arc;

use designcraft_doc::flex::{FlexAlign, FlexDirection, FlexJustify, FlexLayout, FlexSize};
use designcraft_doc::{Content, Document, Item, ItemId, Selection, Shape};
use designcraft_geom::{Rect, shapes};
use serde_json::{Value, json};

use super::object::RemoveKeep;
use super::{CommandSpec, bad, cmd, has_doc, has_selection, id_param, targets};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "flex.add",
            "Add to Flex Layout",
            ["Object", "Flex Layout"],
            None,
            "{ids?} — the selected objects (top level, one spread) in a new Flex Layout container where they are; aligned objects get that direction, Auto size and Space Between, others a wrapping row of their width. One group or empty rectangle frame is converted instead (flex.convert) → {id}",
            has_selection,
            add
        ),
        cmd!(
            "flex.convert",
            "Convert to Flex Layout",
            ["Object", "Flex Layout"],
            None,
            "{ids?} — a group (its objects) or a rectangle frame (with what is pasted into it) becomes a Flex Layout container → {id}",
            has_selection,
            convert
        ),
        cmd!(
            "flex.remove",
            "Remove Flex Layout",
            ["Object", "Flex Layout"],
            None,
            "{ids?} — the container goes; its objects stay where they are, grouped when there are several (a container with its own fill or stroke stays as a frame holding them) → {ids}",
            is_container,
            remove
        ),
        cmd!(
            "flex.set",
            "Flex Layout Options",
            [],
            None,
            "{ids?, direction?: row|rowReverse|column|columnReverse, wrap?: bool, justify?: flexStart|center|flexEnd|spaceBetween|spaceAround|spaceEvenly, align?: start|center|end (across), cell?: 0..8 (the alignment grid, row by row from the top left: sets justification and alignment by direction), width?: auto|fixed|number, height?: auto|fixed|number, padding?: number|[top, left, bottom, right], gapH?, gapV?} → {changed}",
            is_container,
            set
        ),
        cmd!(
            "flex.reorder",
            "Reorder in Flex Layout",
            [],
            None,
            "{id (an object in a container), to?: index | by?: ±n} — its place in the container's order → {index}",
            has_doc,
            reorder
        ),
    ]
}

fn is_container(s: &Session) -> std::result::Result<(), String> {
    has_selection(s)?;
    let st = s.active().ok_or("no document")?;
    if st.selection.items.iter().any(|i| st.doc.item(*i).is_some_and(|it| it.flex.is_some())) {
        Ok(())
    } else {
        Err("select a Flex Layout container".into())
    }
}

/// Lay out every container (called after each edit). Spreads without one aren't copied.
pub(crate) fn sync(d: &mut Document) {
    for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
        if sp.items.iter().any(|i| designcraft_doc::flex::contains_flex(i)) {
            designcraft_doc::flex::layout_all(&mut Arc::make_mut(sp).items);
        }
    }
}

fn in_line_gap(bounds: &[Rect], row: bool, tol: f64) -> Option<f64> {
    let mut v: Vec<&Rect> = bounds.iter().collect();
    v.sort_by(|a, b| if row { a.x0.total_cmp(&b.x0) } else { a.y0.total_cmp(&b.y0) });
    let apart = v.windows(2).all(|w| if row { w[1].x0 >= w[0].x1 - tol } else { w[1].y0 >= w[0].y1 - tol });
    let gaps: Vec<f64> = v.windows(2).map(|w| if row { w[1].x0 - w[0].x1 } else { w[1].y0 - w[0].y1 }).collect();
    (apart && bounds.len() > 1).then(|| gaps.iter().sum::<f64>() / gaps.len().max(1) as f64)
}

fn detect_align(bounds: &[Rect], row: bool, tol: f64) -> FlexAlign {
    let same = |f: &dyn Fn(&Rect) -> f64| bounds.windows(2).all(|w| (f(&w[0]) - f(&w[1])).abs() <= tol);
    let (a, c, e): (&dyn Fn(&Rect) -> f64, &dyn Fn(&Rect) -> f64, &dyn Fn(&Rect) -> f64) =
        if row { (&|r| r.y0, &|r| r.center().y, &|r| r.y1) } else { (&|r| r.x0, &|r| r.center().x, &|r| r.x1) };
    if same(a) {
        FlexAlign::Start
    } else if same(c) {
        FlexAlign::Center
    } else if same(e) {
        FlexAlign::End
    } else {
        FlexAlign::Start
    }
}

fn auto_flex(gap: f64, row: bool, align: FlexAlign) -> FlexLayout {
    FlexLayout {
        direction: if row { FlexDirection::Row } else { FlexDirection::Column },
        justify: FlexJustify::SpaceBetween,
        align,
        width: FlexSize::Auto,
        height: FlexSize::Auto,
        gap_h: if row { gap.max(0.0) } else { 0.0 },
        gap_v: if row { 0.0 } else { gap.max(0.0) },
        ..Default::default()
    }
}

/// InDesign's detection when objects become a container: objects in a row (or column) keep it,
/// with Auto size and Space Between at their own spacing; others wrap in a row of their width.
fn detect(bounds: &[Rect]) -> FlexLayout {
    let tol = 0.5;
    match (in_line_gap(bounds, true, tol), in_line_gap(bounds, false, tol)) {
        (Some(gap), _) => auto_flex(gap, true, detect_align(bounds, true, tol)),
        (None, Some(gap)) => auto_flex(gap, false, detect_align(bounds, false, tol)),
        _ => FlexLayout { wrap: true, height: FlexSize::Auto, ..Default::default() },
    }
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if let [one] = ids.as_slice()
        && s.doc()?.doc.item(*one).is_some_and(convertible)
    {
        return convert(s, p);
    }
    if ids.is_empty() {
        return Err(bad("flex.add", "select the objects to put in a Flex Layout"));
    }
    s.edit(|d, sel| {
        let sr = d.find(ids[0]).map(|l| l.spread).ok_or(designcraft_doc::DocError::NoItem(ids[0]))?;
        let order: Vec<ItemId> = d.spread(sr).map(|sp| sp.items.iter().map(|i| i.id).filter(|i| ids.contains(i)).collect()).unwrap_or_default();
        if order.len() != ids.len() {
            return Err(bad("flex.add", "select objects on one spread, not inside a group or frame"));
        }
        let front = d.spread(sr).and_then(|sp| sp.items.iter().position(|i| Some(&i.id) == order.last())).unwrap_or(0);
        let layer = d.item(order[0]).map(|i| i.layer).unwrap_or_default();
        let mut kids: Vec<Item> = Vec::new();
        for id in &order {
            kids.push(d.remove_item_keep_story(*id)?);
        }
        // Their order in the container: along the line they are in, else top to bottom, left to right.
        let bounds: Vec<Rect> = kids.iter().map(Item::bounds).collect();
        let f = detect(&bounds);
        // A row reads left to right (wrapped: row by row); a column top to bottom (wrapped: column by
        // column).
        let key = |r: &Rect| if f.direction.is_row() != f.wrap { (r.x0, r.y0) } else { (r.y0, r.x0) };
        kids.sort_by(|a, b| {
            let (ka, kb) = (key(&a.bounds()), key(&b.bounds()));
            ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        });
        let rect = bounds.iter().copied().reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
        let cid = ItemId(d.alloc());
        let mut c = Item::new(cid, layer, Shape::Rectangle, shapes::rectangle(rect));
        c.content = Content::Group { items: kids.into_iter().map(Arc::new).collect() };
        c.flex = Some(f);
        d.insert_item(sr, c, Some(front + 1 - order.len()))?;
        *sel = Selection::items(vec![cid]);
        Ok(json!({"id": cid.0}))
    })
}

/// A group, or a rectangle frame holding nothing or pasted-in objects.
fn convertible(it: &Item) -> bool {
    it.flex.is_none() && (it.is_group() || it.shape == Shape::Rectangle && matches!(it.content, Content::Unassigned | Content::Group { .. }))
}

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let [id] = ids.as_slice() else { return Err(bad("flex.convert", "select one group or rectangle frame")) };
    let id = *id;
    s.edit(|d, sel| {
        let it = d.item(id).ok_or(designcraft_doc::DocError::NoItem(id))?.clone();
        if !convertible(&it) {
            return Err(bad("flex.convert", "select one group or rectangle frame"));
        }
        let kids: Vec<Arc<Item>> = match &it.content {
            Content::Group { items } => items.clone(),
            _ => Vec::new(),
        };
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        if it.is_group() {
            // The group's box becomes the container's, in its own space.
            let rect = kids.iter().map(|k| k.bounds()).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
            it.shape = Shape::Rectangle;
            it.path = shapes::rectangle(rect);
        }
        let bounds: Vec<Rect> = kids.iter().map(|k| k.bounds()).collect();
        it.flex = Some(if bounds.is_empty() { FlexLayout::default() } else { detect(&bounds) });
        it.content = Content::Group { items: kids };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    s.edit(|d, sel| {
        let mut out = Vec::new();
        for id in &ids {
            let Some(loc) = d.find(*id) else { continue };
            let Some(c) = d.item(*id).cloned() else { continue };
            if c.flex.is_none() {
                continue;
            }
            let kids = match &c.content {
                Content::Group { items } => items.clone(),
                _ => Vec::new(),
            };
            // A frame with its own look stays one, holding its objects.
            if !c.fill.is_none() || !c.stroke.is_none() || kids.is_empty() {
                if let Some(it) = d.item_mut(*id) {
                    it.flex = None;
                }
                out.push(*id);
                continue;
            }
            if loc.path.len() != 1 {
                return Err(bad("flex.remove", "a Flex Layout inside a group or another container keeps its objects; remove the outer one first"));
            }
            let Some(sp) = d.spread_mut(loc.spread) else { continue };
            sp.items.remove(loc.path[0]);
            let new = if let [one] = kids.as_slice() {
                let mut k = (**one).clone();
                k.xf = c.xf * k.xf;
                k
            } else {
                let mut g = Item::new(c.id, c.layer, Shape::Group, designcraft_geom::PathData::default());
                g.xf = c.xf;
                g.content = Content::Group { items: kids };
                g
            };
            out.push(new.id);
            sp.items.insert(loc.path[0], Arc::new(new));
        }
        *sel = Selection::items(out.clone());
        Ok(json!({"ids": out.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

fn parse<T: serde::de::DeserializeOwned>(p: &Value, key: &str) -> Result<Option<T>> {
    match p.get(key) {
        None => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|_| bad("flex.set", format!("`{key}`: unknown value {v}"))),
    }
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let direction: Option<FlexDirection> = parse(p, "direction")?;
    let justify: Option<FlexJustify> = parse(p, "justify")?;
    let align: Option<FlexAlign> = parse(p, "align")?;
    let cell = p.get("cell").and_then(Value::as_u64);
    if cell.is_some_and(|c| c > 8) {
        return Err(bad("flex.set", "`cell` is 0..8"));
    }
    let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 100_000.0));
    let padding: Option<[f64; 4]> = match p.get("padding") {
        None => None,
        Some(Value::Number(n)) => n.as_f64().filter(|v| v.is_finite()).map(|v| [v.clamp(0.0, 100_000.0); 4]),
        Some(v) => {
            let a: Vec<f64> = serde_json::from_value(v.clone()).map_err(|_| bad("flex.set", "`padding`: a number or [top, left, bottom, right]"))?;
            let [t, l, b, r] = a.as_slice() else { return Err(bad("flex.set", "`padding`: a number or [top, left, bottom, right]")) };
            Some([*t, *l, *b, *r].map(|v| if v.is_finite() { v.clamp(0.0, 100_000.0) } else { 0.0 }))
        }
    };
    let size = |k: &str| -> Result<Option<(FlexSize, Option<f64>)>> {
        match p.get(k) {
            None => Ok(None),
            Some(Value::String(s)) if s == "auto" => Ok(Some((FlexSize::Auto, None))),
            Some(Value::String(s)) if s == "fixed" => Ok(Some((FlexSize::Fixed, None))),
            Some(Value::Number(n)) => Ok(n.as_f64().filter(|v| v.is_finite() && *v > 0.0).map(|v| (FlexSize::Fixed, Some(v.min(100_000.0))))),
            Some(v) => Err(bad("flex.set", format!("`{k}`: auto, fixed or a size, not {v}"))),
        }
    };
    let (width, height) = (size("width")?, size("height")?);
    let (gap_h, gap_v, wrap) = (num("gapH"), num("gapV"), p.get("wrap").and_then(Value::as_bool));
    s.edit(|d, _| {
        let mut changed = 0;
        for id in &ids {
            let Some(it) = d.item_mut(*id) else { continue };
            let Some(f) = it.flex.as_mut() else { continue };
            if let Some(v) = direction {
                f.direction = v;
            }
            if let Some(v) = wrap {
                f.wrap = v;
            }
            if let Some(v) = justify {
                f.justify = v;
            }
            if let Some(v) = align {
                f.align = v;
            }
            if let Some(c) = cell {
                // The grid is what you see: its column is left/centre/right, its row top/middle/bottom.
                let pos = |i: u64| [FlexAlign::Start, FlexAlign::Center, FlexAlign::End][i as usize % 3];
                let (h, v) = (pos(c % 3), pos(c / 3));
                let (main, cross) = if f.direction.is_row() { (h, v) } else { (v, h) };
                let main = if f.direction.is_reverse() {
                    match main {
                        FlexAlign::Start => FlexAlign::End,
                        FlexAlign::End => FlexAlign::Start,
                        m => m,
                    }
                } else {
                    main
                };
                if !f.justify.distributes() {
                    f.justify = match main {
                        FlexAlign::Start => FlexJustify::FlexStart,
                        FlexAlign::Center => FlexJustify::Center,
                        FlexAlign::End => FlexJustify::FlexEnd,
                    };
                }
                f.align = cross;
            }
            if let Some(v) = padding {
                f.padding = v;
            }
            if let Some(v) = gap_h {
                f.gap_h = v;
            }
            if let Some(v) = gap_v {
                f.gap_v = v;
            }
            let r = it.inner_bounds();
            let mut rect = r;
            if let Some((mode, value)) = width {
                if let Some(f) = it.flex.as_mut() {
                    f.width = mode;
                }
                if let Some(w) = value {
                    rect.x1 = rect.x0 + w;
                }
            }
            if let Some((mode, value)) = height {
                if let Some(f) = it.flex.as_mut() {
                    f.height = mode;
                }
                if let Some(h) = value {
                    rect.y1 = rect.y0 + h;
                }
            }
            if rect != r {
                it.path = shapes::rectangle(rect);
            }
            changed += 1;
        }
        Ok(json!({"changed": changed}))
    })
}

fn reorder(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p, "id").ok_or_else(|| bad("flex.reorder", "missing `id`"))?;
    let to = p.get("to").and_then(Value::as_i64);
    let by = p.get("by").and_then(Value::as_i64);
    if to.is_none() && by.is_none() {
        return Err(bad("flex.reorder", "give `to` or `by`"));
    }
    s.edit(|d, sel| {
        let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let (last, parent_path) = loc.path.split_last().ok_or(designcraft_doc::DocError::NoItem(id))?;
        let parent_loc = designcraft_doc::ItemLoc { spread: loc.spread, path: parent_path.to_vec() };
        let parent = d
            .item_at(&parent_loc)
            .filter(|c| c.flex.is_some())
            .map(|c| c.id)
            .ok_or_else(|| bad("flex.reorder", "the object isn't in a Flex Layout"))?;
        let c = d.item_mut(parent).ok_or(designcraft_doc::DocError::NoItem(parent))?;
        let Content::Group { items } = &mut c.content else { return Err(bad("flex.reorder", "the object isn't in a Flex Layout")) };
        let n = items.len() as i64;
        let from = *last as i64;
        let target = to.unwrap_or(from.saturating_add(by.unwrap_or(0))).clamp(0, n - 1);
        let moved = items.remove(from as usize);
        items.insert(target as usize, moved);
        *sel = Selection::items(vec![id]);
        Ok(json!({"index": target}))
    })
}

/// An object moved inside a container goes to the place it was dropped on: before the nearest
/// object if it lies before that one's centre along the main axis, else after it.
pub(crate) fn drop_in_container(d: &mut Document, id: ItemId) {
    let Some(loc) = d.find(id) else { return };
    let Some((&at, parent)) = loc.path.split_last() else { return };
    let parent_loc = designcraft_doc::ItemLoc { spread: loc.spread, path: parent.to_vec() };
    let Some(cid) = d.item_at(&parent_loc).filter(|c| c.flex.is_some()).map(|c| c.id) else { return };
    let Some(c) = d.item_mut(cid) else { return };
    let Some(f) = c.flex.clone() else { return };
    let Content::Group { items } = &mut c.content else { return };
    if at >= items.len() {
        return;
    }
    let moved = items.remove(at);
    let m = moved.bounds().center();
    let along = |p: designcraft_geom::Point| if f.direction.is_row() { p.x } else { p.y };
    let nearest = items.iter().enumerate().map(|(i, k)| (i, k.bounds().center())).min_by(|a, b| (a.1 - m).hypot().total_cmp(&(b.1 - m).hypot()));
    let index = match nearest {
        Some((i, k)) => {
            let before = (along(m) < along(k)) != f.direction.is_reverse();
            if before { i } else { i + 1 }
        }
        None => 0,
    };
    items.insert(index.min(items.len()), moved);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_boxes(boxes: &[[f64; 4]]) -> (Session, Vec<u64>) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 1920, "height": 1080, "margins": 84})).unwrap();
        let ids = boxes.iter().map(|r| s.execute("frame.create", &json!({"rect": r})).unwrap()["id"].as_u64().unwrap()).collect();
        (s, ids)
    }

    fn rect_of(s: &Session, id: u64) -> Rect {
        s.doc().unwrap().doc.item(ItemId(id)).unwrap().bounds()
    }

    #[test]
    fn reorder_by_a_huge_step_clamps_instead_of_overflowing() {
        let (mut s, ids) = session_with_boxes(&[[84.0, 84.0, 184.0, 184.0], [284.0, 84.0, 384.0, 184.0]]);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        s.execute("flex.add", &json!({})).unwrap();
        // `from + by` used to overflow (a panic with overflow checks on).
        s.execute("flex.reorder", &json!({"id": ids[0], "by": i64::MAX})).unwrap();
        s.execute("flex.reorder", &json!({"id": ids[0], "by": i64::MIN})).unwrap();
    }

    /// The video: four objects in a row become a container that keeps them where they are (Row,
    /// Auto size, Space Between at their spacing, aligned to the top), then a fixed 2-column layout.
    #[test]
    fn four_objects_in_a_row_then_two_wrapped_columns() {
        // As in the video after Shift+Cmd shrinks them: 400 × 430, 22 apart.
        let boxes = [[84.0, 461.0, 484.0, 891.0], [506.0, 461.0, 906.0, 891.0], [928.0, 461.0, 1328.0, 891.0], [1350.0, 461.0, 1750.0, 891.0]];
        let (mut s, ids) = session_with_boxes(&boxes);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        let c = s.execute("flex.add", &json!({})).unwrap()["id"].as_u64().unwrap();
        let d = s.doc().unwrap().doc.clone();
        let it = d.item(ItemId(c)).unwrap();
        let f = it.flex.clone().unwrap();
        assert_eq!(
            (f.direction, f.justify, f.align, f.width, f.height, f.wrap),
            (FlexDirection::Row, FlexJustify::SpaceBetween, FlexAlign::Start, FlexSize::Auto, FlexSize::Auto, false)
        );
        assert!((f.gap_h - 22.0).abs() < 1e-9);
        assert_eq!(it.bounds(), Rect::new(84.0, 461.0, 1750.0, 891.0));
        for (id, b) in ids.iter().zip(&boxes) {
            assert_eq!(rect_of(&s, *id), Rect::new(b[0], b[1], b[2], b[3]), "nothing moves");
        }
        assert_eq!(d.spreads[0].items.len(), 1, "the objects are in the container");
        // Resize the container (Fixed), Flex Start, a wrapping column, spacing, centred.
        s.execute(
            "flex.set",
            &json!({"width": 868, "height": 912, "justify": "flexStart", "direction": "column", "wrap": true, "gapH": 56, "gapV": 38}),
        )
        .unwrap();
        s.execute("flex.set", &json!({"cell": 4})).unwrap();
        let f = s.doc().unwrap().doc.item(ItemId(c)).unwrap().flex.clone().unwrap();
        assert_eq!((f.justify, f.align, f.width), (FlexJustify::Center, FlexAlign::Center, FlexSize::Fixed));
        // Two columns of two: 400 + 56 + 400 across 868, 430 + 38 + 430 down 912, centred.
        let r: Vec<Rect> = ids.iter().map(|i| rect_of(&s, *i)).collect();
        assert_eq!(r[0].x0, r[1].x0);
        assert_eq!(r[2].x0, r[3].x0);
        assert!((r[2].x0 - r[0].x1 - 56.0).abs() < 1e-6);
        assert!((r[1].y0 - r[0].y1 - 38.0).abs() < 1e-6);
        let c_rect = s.doc().unwrap().doc.item(ItemId(c)).unwrap().bounds();
        assert!((r[0].x0 - c_rect.x0 - 6.0).abs() < 1e-6 && (r[0].y0 - c_rect.y0 - 7.0).abs() < 1e-6, "{r:?} in {c_rect:?}");
        // Reorder: 2 goes to the top of the second column.
        s.execute("flex.reorder", &json!({"id": ids[1], "to": 2})).unwrap();
        assert_eq!(rect_of(&s, ids[1]).x0, r[2].x0, "now in the second column");
        assert_eq!(rect_of(&s, ids[1]).y0, r[0].y0);
        // One undo step per command; undo the reorder.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(rect_of(&s, ids[1]), r[1]);
    }

    /// Resizing the container by its handles changes its box (Fixed), not its objects' size.
    #[test]
    fn resizing_a_container_reflows_instead_of_scaling() {
        let (mut s, ids) = session_with_boxes(&[[100.0, 100.0, 200.0, 200.0], [220.0, 100.0, 320.0, 200.0]]);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        let c = s.execute("flex.add", &json!({})).unwrap()["id"].as_u64().unwrap();
        s.execute("flex.set", &json!({"justify": "flexEnd"})).unwrap();
        s.execute("transform.resize", &json!({"from": [100, 100, 320, 200], "to": [100, 100, 520, 300]})).unwrap();
        let it = s.doc().unwrap().doc.item(ItemId(c)).unwrap().clone();
        let b = it.bounds();
        assert!((b.x1 - 520.0).abs() < 1e-6 && (b.y1 - 300.0).abs() < 1e-6 && b.x0 == 100.0, "{b:?}");
        let f = it.flex.unwrap();
        assert_eq!((f.width, f.height), (FlexSize::Fixed, FlexSize::Fixed));
        let r1 = rect_of(&s, ids[1]);
        assert!((r1.x1 - 520.0).abs() < 1e-6 && (r1.width() - 100.0).abs() < 1e-6 && r1.y0 == 100.0, "same size, at the end of the row: {r1:?}");
        assert_eq!(rect_of(&s, ids[0]).width(), 100.0);
        s.execute("transform.set", &json!({"width": 300, "ref": 0})).unwrap();
        assert!((rect_of(&s, ids[1]).x1 - 400.0).abs() < 1e-6);
    }

    /// Moving an object inside the container is undone by the layout (it places them); a new
    /// object in the container is laid out at once.
    #[test]
    fn the_layout_follows_every_edit() {
        let (mut s, ids) = session_with_boxes(&[[0.0, 0.0, 100.0, 50.0], [150.0, 0.0, 250.0, 50.0]]);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        s.execute("flex.add", &json!({})).unwrap();
        s.execute("transform.set", &json!({"ids": [ids[0]], "width": 300, "ref": 0})).unwrap();
        // Auto width: the container grows, the second object follows at the spacing.
        assert_eq!(rect_of(&s, ids[1]).x0, 350.0);
    }

    #[test]
    fn remove_groups_the_objects_where_they_are() {
        let (mut s, ids) = session_with_boxes(&[[0.0, 0.0, 100.0, 50.0], [150.0, 0.0, 250.0, 50.0]]);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        s.execute("flex.add", &json!({})).unwrap();
        s.execute("flex.set", &json!({"justify": "flexStart", "gapH": 10})).unwrap();
        let before = rect_of(&s, ids[1]);
        let r = s.execute("flex.remove", &json!({})).unwrap();
        let g = r["ids"][0].as_u64().unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(ItemId(g)).unwrap().is_group());
        assert_eq!(rect_of(&s, ids[1]), before);
        assert_eq!(before.x0, 110.0);
        // A group converts back.
        s.execute("flex.convert", &json!({"ids": [g]})).unwrap();
        assert!(s.doc().unwrap().doc.item(ItemId(g)).unwrap().flex.is_some());
    }

    /// Dragging an object in a container drops it in the nearest place (the video moves group 2 to
    /// the top of the second column).
    #[test]
    fn dragging_an_object_reorders_the_container() {
        let (mut s, ids) = session_with_boxes(&[[0.0, 0.0, 100.0, 50.0], [110.0, 0.0, 210.0, 50.0], [220.0, 0.0, 320.0, 50.0]]);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        s.execute("flex.add", &json!({})).unwrap();
        s.execute("flex.set", &json!({"justify": "flexStart"})).unwrap();
        // The first box dropped past the third's centre: it goes last.
        s.execute("transform.move", &json!({"ids": [ids[0]], "dx": 250, "dy": 5})).unwrap();
        let x: Vec<f64> = ids.iter().map(|i| rect_of(&s, *i).x0.round()).collect();
        assert_eq!(x, [220.0, 0.0, 110.0]);
        // Back before the second (now at 0): first again.
        s.execute("transform.move", &json!({"ids": [ids[0]], "dx": -225, "dy": 0})).unwrap();
        let x: Vec<f64> = ids.iter().map(|i| rect_of(&s, *i).x0.round()).collect();
        assert_eq!(x, [0.0, 110.0, 220.0]);
    }

    /// Flex settings are saved with the document.
    #[test]
    fn a_container_round_trips_through_the_file() {
        let (mut s, ids) = session_with_boxes(&[[0.0, 0.0, 100.0, 50.0], [110.0, 0.0, 210.0, 50.0]]);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        let c = s.execute("flex.add", &json!({})).unwrap()["id"].as_u64().unwrap();
        s.execute("flex.set", &json!({"direction": "columnReverse", "wrap": true, "padding": [1, 2, 3, 4], "gapV": 7})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let back = crate::cmd::from_bytes(&crate::cmd::to_bytes(&d).unwrap()).unwrap();
        assert_eq!(back.item(ItemId(c)).unwrap().flex, d.item(ItemId(c)).unwrap().flex);
        assert!(back.item(ItemId(c)).unwrap().flex.is_some());
    }

    #[test]
    fn bad_params_are_refused() {
        let (mut s, ids) = session_with_boxes(&[[0.0, 0.0, 100.0, 50.0]]);
        s.execute("selection.set", &json!({"ids": ids})).unwrap();
        s.execute("flex.add", &json!({})).unwrap();
        assert!(s.execute("flex.set", &json!({"direction": "diagonal"})).is_err());
        assert!(s.execute("flex.set", &json!({"cell": 9})).is_err());
        assert!(s.execute("flex.set", &json!({"padding": [1, 2]})).is_err());
        assert!(s.execute("flex.reorder", &json!({"id": ids[0]})).is_err());
        s.execute("flex.set", &json!({"gapH": f64::MAX, "width": -5})).unwrap();
    }
}
