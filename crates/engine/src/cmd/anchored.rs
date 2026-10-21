//! Object › Anchored Object: items that flow with the text (inline or above the line).

use std::sync::Arc;

use designcraft_doc::{
    AnchorPosition, AnchoredObject, AnchoredSel, Content, Document, Item, ItemId, OBJECT_MARK, SpreadRef, StoryId, TextSel, anchored::AnchorAlign,
};
use designcraft_geom::Affine;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "anchored.insert",
            "Insert Anchored Object…",
            ["Object", "Anchored Object"],
            None,
            "{ids, story?, pos? (default: the text insertion point), position?: inline|aboveLine|custom, yOffset?, align?: left|center|right, spaceBefore?, spaceAfter?, xRelative? | yRelative?: anchor|textFrame|columnEdge|pageMargin|pageEdge, xOffset?, objectPoint? | refPoint?: 0..8, keepWithinColumn?} — moves the items into the text",
            has_doc,
            insert
        ),
        cmd!(
            "anchored.options",
            "Anchored Object Options…",
            ["Object", "Anchored Object"],
            None,
            "{story?, index? (default: the object at the insertion point), position?: inline|aboveLine|custom, yOffset?, align?: left|center|right, spaceBefore?, spaceAfter?, xRelative? | yRelative?: anchor|textFrame|columnEdge|pageMargin|pageEdge, xOffset?, objectPoint? | refPoint?: 0..8, keepWithinColumn?}",
            has_doc,
            options
        ),
        cmd!(
            "anchored.release",
            "Release",
            ["Object", "Anchored Object"],
            None,
            "{story?, index? (default: the object at the insertion point)} — puts the object back on the page where it is now and removes it from the text",
            has_doc,
            release
        ),
        cmd!(
            noundo "anchored.select",
            "Select Anchored Object",
            [],
            None,
            "{story, index} — selects the anchored object as an object (the Selection tool clicking it); Delete removes it from the text, anchored.resize / options / release act on it",
            has_doc,
            select
        ),
        cmd!(
            "anchored.resize",
            "Resize Anchored Object",
            [],
            None,
            "{story?, index? (default: the selected anchored object), from: rect, to: rect (the object's bounds before and after, in its own space: [x0, y0, x1, y1]), content?: bool (scale its content with it; default: the frame alone resizes and crops it from the sides that moved)} — the text then places it again and reflows around it",
            has_doc,
            resize
        ),
        cmd!(query "anchored.list", "Anchored Objects", [], None, "{story?} → [{story, index, pos, position, size}]", has_doc, |s, p| {
            let d = &s.doc()?.doc;
            let only = p.get("story").and_then(Value::as_u64).map(StoryId);
            let mut out = Vec::new();
            for st in d.stories.values().filter(|st| only.is_none_or(|o| o == st.id)) {
                for (k, ((pos, _), o)) in st.text.match_indices(OBJECT_MARK).zip(&st.objects).enumerate() {
                    out.push(json!({"story": st.id.0, "index": k, "pos": pos, "position": o.position, "size": o.size()}));
                }
            }
            Ok(Value::Array(out))
        }),
    ]
}

fn position_param(p: &Value, base: &AnchorPosition) -> Result<AnchorPosition> {
    let f = |k: &str, d: f64| p.get(k).and_then(Value::as_f64).unwrap_or(d);
    let kind = str_param(p, "position").unwrap_or(match base {
        AnchorPosition::Inline { .. } => "inline",
        AnchorPosition::AboveLine { .. } => "aboveLine",
        AnchorPosition::Custom { .. } => "custom",
    });
    Ok(match (kind, base) {
        ("inline", AnchorPosition::Inline { y_offset }) => AnchorPosition::Inline { y_offset: f("yOffset", *y_offset) },
        ("inline", _) => AnchorPosition::Inline { y_offset: f("yOffset", 0.0) },
        ("aboveLine", b) => {
            let (a0, sb0, sa0) = match b {
                AnchorPosition::AboveLine { align, space_before, space_after } => (*align, *space_before, *space_after),
                _ => (AnchorAlign::Left, 0.0, 0.0),
            };
            let align = match str_param(p, "align") {
                Some("center") => AnchorAlign::Center,
                Some("right") => AnchorAlign::Right,
                Some("left") => AnchorAlign::Left,
                Some(o) => return Err(bad("anchored", format!("unknown align `{o}`"))),
                None => a0,
            };
            AnchorPosition::AboveLine { align, space_before: f("spaceBefore", sb0), space_after: f("spaceAfter", sa0) }
        }
        ("custom", b) => {
            use designcraft_doc::anchored::AnchorRelative as R;
            let (xr0, yr0, xo0, yo0, op0, rp0, k0) = match b {
                AnchorPosition::Custom { x_relative, y_relative, x_offset, y_offset, object_point, ref_point, keep_within_column } => {
                    (*x_relative, *y_relative, *x_offset, *y_offset, *object_point, *ref_point, *keep_within_column)
                }
                _ => (R::TextFrame, R::Anchor, 0.0, 0.0, 0, 0, false),
            };
            let rel = |k: &str, d: R| -> Result<R> {
                match p.get(k) {
                    Some(v) => serde_json::from_value(v.clone())
                        .map_err(|_| bad("anchored", format!("`{k}`: anchor|textFrame|columnEdge|pageMargin|pageEdge"))),
                    None => Ok(d),
                }
            };
            let pt = |k: &str, d: u8| p.get(k).and_then(Value::as_u64).map_or(d, |v| v.min(8) as u8);
            AnchorPosition::Custom {
                x_relative: rel("xRelative", xr0)?,
                y_relative: rel("yRelative", yr0)?,
                x_offset: f("xOffset", xo0),
                y_offset: f("yOffset", yo0),
                object_point: pt("objectPoint", op0),
                ref_point: pt("refPoint", rp0),
                keep_within_column: p.get("keepWithinColumn").and_then(Value::as_bool).unwrap_or(k0),
            }
        }
        (o, _) => return Err(bad("anchored", format!("unknown position `{o}`"))),
    })
}

/// Text frames can't be anchored yet (their stories need frames on a spread).
fn check_item(it: &Item) -> Result<()> {
    let mut ok = true;
    it.walk(&mut |i| ok &= !matches!(i.content, Content::Text(_)));
    if ok { Ok(()) } else { Err(bad("anchored.insert", "text frames can't be anchored yet")) }
}

/// Anchor copies of `items` (in spread coordinates) at `pos` of `sid`; returns the end position.
pub(crate) fn anchor_items(d: &mut Document, sid: StoryId, pos: usize, items: Vec<Item>, position: &AnchorPosition) -> Result<usize> {
    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let mut at = pos.min(st.len());
    for it in items {
        st.insert_object(at, AnchoredObject::new(it, position.clone()));
        at += OBJECT_MARK.len_utf8();
    }
    Ok(at)
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = super::ids_param(p, "ids").ok_or_else(|| bad("anchored.insert", "missing ids"))?;
    let position = position_param(p, &AnchorPosition::default())?;
    let st = s.doc()?;
    let (sid, pos) = match (p.get("story").and_then(Value::as_u64), p.get("pos").and_then(Value::as_u64)) {
        (Some(sid), pos) => (StoryId(sid), pos.map(|v| v as usize).unwrap_or(usize::MAX)),
        _ => {
            let t = st
                .selection
                .text
                .filter(|t| t.cell.is_none())
                .ok_or_else(|| bad("anchored.insert", "give `story` or place the insertion point in story text"))?;
            (t.story, t.range().start)
        }
    };
    let mut items = Vec::new();
    for id in &ids {
        let loc = st.doc.find(*id).ok_or_else(|| bad("anchored.insert", format!("no item {}", id.0)))?;
        let it = st.doc.item(*id).ok_or_else(|| bad("anchored.insert", format!("no item {}", id.0)))?;
        check_item(it)?;
        // Into spread coordinates (items inside groups carry their parents' transforms).
        let mut it = it.clone();
        it.xf = st.doc.parent_xf(&loc) * it.xf;
        items.push(it);
    }
    s.edit(|d, sel| {
        for id in &ids {
            d.remove_item(*id)?;
        }
        let end = anchor_items(d, sid, pos, items.clone(), &position)?;
        *sel = designcraft_doc::Selection::text(TextSel { story: sid, anchor: end, focus: end, frame: None, cell: None });
        Ok(json!({"story": sid.0, "pos": end}))
    })
}

/// `{story, index}`, or the anchored object at (or just before) the text insertion point.
fn object_ref(s: &Session, p: &Value, cmd: &str) -> Result<(StoryId, usize)> {
    if let (Some(sid), Some(k)) = (p.get("story").and_then(Value::as_u64), p.get("index").and_then(Value::as_u64)) {
        return Ok((StoryId(sid), usize::try_from(k).map_err(|_| bad(cmd, "index out of range"))?));
    }
    let st = s.doc()?;
    if let Some(a) = st.selection.anchored {
        return Ok((a.story, a.index));
    }
    let t = st
        .selection
        .text
        .filter(|t| t.cell.is_none())
        .ok_or_else(|| bad(cmd, "give `story` and `index`, or put the insertion point next to an anchored object"))?;
    let story = st.doc.story(t.story).ok_or_else(|| bad(cmd, "no such story"))?;
    let r = t.range();
    let at = [r.start, r.start.saturating_sub(OBJECT_MARK.len_utf8())]
        .into_iter()
        .chain(story.text[r.clone()].find(OBJECT_MARK).map(|i| r.start + i))
        .find(|&i| story.text.get(i..).is_some_and(|x| x.starts_with(OBJECT_MARK)))
        .ok_or_else(|| bad(cmd, "no anchored object at the insertion point"))?;
    Ok((t.story, story.text[..at].matches(OBJECT_MARK).count()))
}

fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let sid = p.get("story").and_then(Value::as_u64).map(StoryId).ok_or_else(|| bad("anchored.select", "missing story"))?;
    let k = p.get("index").and_then(Value::as_u64).and_then(|k| usize::try_from(k).ok()).ok_or_else(|| bad("anchored.select", "missing index"))?;
    let st = s.doc_mut()?;
    if st.doc.story(sid).and_then(|x| x.objects.get(k)).is_none() {
        return Err(bad("anchored.select", "no such anchored object"));
    }
    st.selection = designcraft_doc::Selection { anchored: Some(AnchoredSel { story: sid, index: k }), ..Default::default() };
    st.bump_revision();
    Ok(Value::Null)
}

/// Resize an anchored object's frame from its bounds `from` to `to` (its own space): the content
/// keeps its place while the edges move, so a frame made narrower from the left crops the
/// picture's left side. Its top-left then returns to the origin, where composition places it,
/// and the text reflows around the new size.
fn resize(s: &mut Session, p: &Value) -> Result<Value> {
    let (sid, k) = object_ref(s, p, "anchored.resize")?;
    let from = super::rect_param(p, "from").ok_or_else(|| bad("anchored.resize", "missing from"))?;
    let to = super::rect_param(p, "to").ok_or_else(|| bad("anchored.resize", "missing to"))?;
    let finite = |r: designcraft_geom::Rect| [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite());
    if !finite(from) || !finite(to) || from.width().abs() < 1e-9 || from.height().abs() < 1e-9 {
        return Err(bad("anchored.resize", "degenerate or non-finite rect"));
    }
    // At least a point each way, at most the largest page InDesign allows.
    const MIN: f64 = 1.0;
    const MAX: f64 = 15552.0;
    let sx = to.width().abs().clamp(MIN, MAX) / from.width().abs();
    let sy = to.height().abs().clamp(MIN, MAX) / from.height().abs();
    // The dragged edges move and the content stays where it was on the page (InDesign crops a
    // picture from the side whose handle moved). Then the text places the object again by its
    // top-left, so after a left or top handle the frame and picture shift together.
    let m = Affine::translate((to.x0.min(to.x1), to.y0.min(to.y1))) * Affine::scale_non_uniform(sx, sy) * Affine::translate((-from.x0, -from.y0));
    let content = p.get("content").and_then(Value::as_bool).unwrap_or(false);
    let strokes = s.prefs.scale_strokes;
    s.edit(|d, _| {
        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let o = st.objects.get_mut(k).ok_or_else(|| bad("anchored.resize", "no such anchored object"))?;
        let o = Arc::make_mut(o);
        let it = &mut o.item;
        if it.locked {
            return Err(bad("anchored.resize", "the object is locked"));
        }
        if content {
            super::object::scale_item(it, m, (sx * sy).sqrt(), strokes);
        } else if matches!(it.content, Content::Group { .. }) {
            it.xf = m * it.xf;
        } else {
            let inner = it.xf.inverse() * m * it.xf;
            super::object::bake(it, inner);
            let r = it.inner_bounds();
            if let Content::Graphic(g) = &mut it.content
                && let Some(xf) = g.fitted(r, g.auto_fit)
            {
                g.xf = xf;
            }
        }
        // Composition places the object by its top-left: keep that at the origin.
        let b = it.bounds();
        it.xf = Affine::translate((-b.x0, -b.y0)) * it.xf;
        let size = o.size();
        st.rev = st.rev.wrapping_add(1);
        Ok(json!({"size": [size.0, size.1]}))
    })
}

/// Delete a selected anchored object: its mark leaves the text, and the object with it.
pub(crate) fn delete(s: &mut Session, a: AnchoredSel) -> Result<Value> {
    let pos = {
        let st = s.doc()?.doc.story(a.story).ok_or_else(|| bad("edit.clear", "no such story"))?;
        st.text.match_indices(OBJECT_MARK).nth(a.index).map(|(i, _)| i).ok_or_else(|| bad("edit.clear", "no such anchored object"))?
    };
    s.edit(|d, sel| {
        let st = d.story_mut(a.story).ok_or(designcraft_doc::DocError::NoStory(a.story))?;
        st.delete(pos..pos + OBJECT_MARK.len_utf8());
        *sel = designcraft_doc::Selection::default();
        Ok(json!({"deleted": 1}))
    })
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let (sid, k) = object_ref(s, p, "anchored.options")?;
    let cur = s.doc()?.doc.story(sid).and_then(|st| st.objects.get(k).cloned()).ok_or_else(|| bad("anchored.options", "no such anchored object"))?;
    let position = position_param(p, &cur.position)?;
    s.edit(|d, _| {
        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let o = st.objects.get_mut(k).ok_or_else(|| bad("anchored.options", "no such anchored object"))?;
        Arc::make_mut(o).position = position.clone();
        st.rev = st.rev.wrapping_add(1);
        Ok(json!({"position": position}))
    })
}

fn release(s: &mut Session, p: &Value) -> Result<Value> {
    let (sid, k) = object_ref(s, p, "anchored.release")?;
    let d0 = s.doc()?.doc.clone();
    let st = d0.story(sid).ok_or_else(|| bad("anchored.release", "no such story"))?;
    let obj = st.objects.get(k).cloned().ok_or_else(|| bad("anchored.release", "no such anchored object"))?;
    let pos = st.text.match_indices(OBJECT_MARK).nth(k).map(|(i, _)| i).ok_or_else(|| bad("anchored.release", "no mark"))?;
    // Where it shows now: its frame's transform and its placement.
    let cs = s.cache.get(&d0, sid, None);
    let placed = cs.frames.iter().find_map(|ft| ft.objects.iter().find(|o| o.index == k).map(|o| (ft.frame, o.origin)));
    let (spread, xf) = match placed {
        Some((fid, origin)) => {
            let loc = d0.find(fid).ok_or_else(|| bad("anchored.release", "frame not found"))?;
            let fxf = d0.parent_xf(&loc) * d0.item(fid).map_or(Affine::IDENTITY, |i| d0.text_xf(i));
            (loc.spread, fxf * Affine::translate(origin.to_vec2()))
        }
        None => (SpreadRef::Doc(0), Affine::IDENTITY),
    };
    s.edit(|d, sel| {
        let mut it = obj.item.clone();
        it.id = ItemId(d.alloc());
        it.xf = xf * it.xf;
        if d.layer(it.layer).is_none() {
            it.layer = d.default_layer();
        }
        let id = d.insert_item(spread, it, None)?;
        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        st.delete(pos..pos + OBJECT_MARK.len_utf8());
        *sel = designcraft_doc::Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A text frame whose story holds a 24 × 40 inline box: (session, story).
    fn inline_box() -> (Session, StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Before after and more words."})).unwrap();
        let sid = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let r = s.execute("frame.create", &json!({"rect": [450, 450, 474, 490]})).unwrap();
        s.execute("anchored.insert", &json!({"ids": [r["id"]], "story": sid.0, "pos": 7})).unwrap();
        (s, sid)
    }

    #[test]
    fn a_selected_inline_object_resizes_and_the_line_follows() {
        let (mut s, sid) = inline_box();
        s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
        let sel = s.doc().unwrap().selection.clone();
        assert_eq!(sel.anchored, Some(AnchoredSel { story: sid, index: 0 }));
        assert!(sel.items.is_empty() && sel.text.is_none() && !sel.is_empty());
        // Taller by its bottom handle: the line grows to fit it; its top-left stays at the origin.
        let r = s.execute("anchored.resize", &json!({"from": [0, 0, 24, 40], "to": [0, 0, 24, 60]})).unwrap();
        assert_eq!(r["size"], json!([24.0, 60.0]));
        let d = s.doc().unwrap().doc.clone();
        let b = d.story(sid).unwrap().objects[0].item.bounds();
        assert!(b.x0.abs() < 1e-9 && b.y0.abs() < 1e-9, "{b:?}");
        let cs = s.cache.get(&d, sid, None);
        let o = &cs.frames[0].objects[0];
        assert!(cs.frames[0].lines[o.line].ascent >= 60.0);
        // Narrower from its left handle: still at the origin, 10 pt wide.
        s.execute("anchored.resize", &json!({"from": [0, 0, 24, 60], "to": [14, 0, 24, 60]})).unwrap();
        let o = s.doc().unwrap().doc.story(sid).unwrap().objects[0].clone();
        assert_eq!(o.size(), (10.0, 60.0));
        assert!(o.item.bounds().x0.abs() < 1e-9);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().objects[0].size(), (24.0, 40.0));
    }

    #[test]
    fn a_left_or_top_handle_crops_that_side_of_the_picture() {
        let (mut s, sid) = inline_box();
        let graphic = designcraft_doc::Graphic {
            asset: designcraft_doc::AssetId(1),
            size: (24.0, 40.0),
            xf: Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        };
        s.edit(|d, _| {
            let it = &mut Arc::make_mut(&mut d.story_mut(sid).unwrap().objects[0]).item;
            // At the frame's top-left, in its inner space.
            let inner = it.inner_bounds();
            it.content = Content::Graphic(designcraft_doc::Graphic { xf: Affine::translate((inner.x0, inner.y0)), ..graphic.clone() });
            Ok(Value::Null)
        })
        .unwrap();
        // The picture's top-left relative to the frame's.
        let offset = |s: &Session| {
            let it = s.doc().unwrap().doc.story(sid).unwrap().objects[0].item.clone();
            let Content::Graphic(g) = &it.content else { panic!() };
            let b = it.bounds();
            ((it.xf * g.xf) * designcraft_geom::Point::ORIGIN - designcraft_geom::Point::new(b.x0, b.y0), b)
        };
        s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
        // The left handle 10 pt right and the top handle 20 pt down: the picture stayed put while
        // those edges moved, so its left 10 and top 20 points are cropped.
        s.execute("anchored.resize", &json!({"from": [0, 0, 24, 40], "to": [10, 20, 24, 40]})).unwrap();
        let (d, b) = offset(&s);
        assert!((b.width() - 14.0).abs() < 1e-9 && (b.height() - 20.0).abs() < 1e-9 && b.x0.abs() < 1e-9 && b.y0.abs() < 1e-9, "{b:?}");
        assert!((d.x + 10.0).abs() < 1e-9 && (d.y + 20.0).abs() < 1e-9, "{d:?}");
        // The right and bottom handles leave the picture at the frame's top-left.
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("anchored.resize", &json!({"from": [0, 0, 24, 40], "to": [0, 0, 30, 50]})).unwrap();
        let (d, _) = offset(&s);
        assert!(d.x.abs() < 1e-9 && d.y.abs() < 1e-9, "{d:?}");
    }

    #[test]
    fn delete_removes_a_selected_anchored_object_from_the_text() {
        let (mut s, sid) = inline_box();
        let before = s.doc().unwrap().doc.story(sid).unwrap().text.clone();
        s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
        assert_eq!(s.execute("edit.clear", &json!({})).unwrap()["deleted"], 1);
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert!(st.objects.is_empty() && !st.text.contains(OBJECT_MARK));
        assert!(s.doc().unwrap().selection.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, before);
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn options_and_release_act_on_the_selected_anchored_object() {
        let (mut s, sid) = inline_box();
        s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
        s.execute("anchored.options", &json!({"position": "aboveLine"})).unwrap();
        assert!(matches!(s.doc().unwrap().doc.story(sid).unwrap().objects[0].position, AnchorPosition::AboveLine { .. }));
        s.execute("anchored.release", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.story(sid).unwrap().objects.is_empty());
        assert_eq!(s.doc().unwrap().selection.items.len(), 1, "the released item is selected");
    }

    #[test]
    fn selecting_items_drops_an_anchored_selection_and_bad_input_is_refused() {
        let (mut s, sid) = inline_box();
        assert!(s.execute("anchored.select", &json!({"story": sid.0, "index": 5})).is_err());
        assert!(s.execute("anchored.select", &json!({"story": 999_999, "index": 0})).is_err());
        s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
        for bad in [
            json!({"from": [0, 0, 0, 40], "to": [0, 0, 24, 40]}),
            json!({"from": [0, 0, 24, 40], "to": [0, 0, "wide", 40]}),
            json!({"to": [0, 0, 1, 1]}),
        ] {
            assert!(s.execute("anchored.resize", &bad).is_err(), "{bad}");
        }
        // Dragged inside out or to nothing: at least a point each way.
        s.execute("anchored.resize", &json!({"from": [0, 0, 24, 40], "to": [24, 40, 24, 40]})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().objects[0].size(), (1.0, 1.0));
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        assert!(s.doc().unwrap().selection.anchored.is_none());
    }

    #[test]
    fn inline_object_flows_with_text_and_releases() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Before after and more words."})).unwrap();
        let sid = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let r = s.execute("frame.create", &json!({"rect": [450, 450, 474, 490]})).unwrap();
        let box_id = r["id"].as_u64().unwrap();
        s.execute("anchored.insert", &json!({"ids": [box_id], "story": sid.0, "pos": 7})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(ItemId(box_id)).is_none(), "moved into the text");
        let st = d.story(sid).unwrap();
        assert_eq!(st.objects.len(), 1);
        assert_eq!(st.objects[0].size(), (24.0, 40.0));
        let cs = s.cache.get(&d, sid, None);
        let ft = &cs.frames[0];
        assert_eq!(ft.objects.len(), 1);
        let o = &ft.objects[0];
        let line = &ft.lines[o.line];
        // Inline: bottom on the baseline, the line grows to fit it.
        assert!((o.origin.y + 40.0 - line.baseline).abs() < 1e-6);
        assert!(line.ascent >= 40.0);
        // The text after it moved right by its width.
        let after = line.glyphs.iter().find(|g| g.byte == 7 + OBJECT_MARK.len_utf8()).unwrap();
        assert!(after.x >= o.origin.x + 24.0 - 1e-6);
        // Typing before it moves it along.
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("text.insert", &json!({"text": "Words "})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let x2 = s.cache.get(&d, sid, None).frames[0].objects[0].origin.x;
        assert!(x2 > o.origin.x);
        // Above line: the object sits over its line, which moves down.
        s.execute("anchored.options", &json!({"story": sid.0, "index": 0, "position": "aboveLine", "align": "center", "spaceAfter": 4.0})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, sid, None);
        let o = &cs.frames[0].objects[0];
        let line = &cs.frames[0].lines[o.line];
        assert!(o.origin.y + 40.0 + 4.0 <= line.baseline - 5.0, "{o:?} {}", line.baseline);
        // Release: back on the page where it was shown.
        s.execute("anchored.release", &json!({"story": sid.0, "index": 0})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(sid).unwrap().objects.is_empty());
        let id = s.doc().unwrap().selection.items[0];
        let b = d.item(id).unwrap().bounds();
        // (The frame's inner space is spread space here.)
        assert!((b.width() - 24.0).abs() < 1e-6 && (b.x0 - o.origin.x).abs() < 1e-6 && (b.y0 - o.origin.y).abs() < 1e-6, "{b:?}");
        d.check().unwrap();
    }
}

#[cfg(test)]
mod custom_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn custom_anchored_object_sits_in_the_page_margin_and_takes_no_space() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Side note here"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let end_before = s.cache.get(&s.doc().unwrap().doc, sid, None).frames[0].lines[0].end_x;
        let box_id = s.execute("frame.create", &json!({"rect": [0, 0, 30, 20], "content": "unassigned"})).unwrap()["id"].clone();
        // To the left of the page margin, top of the box on the anchor's baseline.
        s.execute(
            "anchored.insert",
            &json!({"ids": [box_id], "story": sid.0, "pos": 4, "position": "custom", "xRelative": "pageMargin", "yRelative": "anchor", "refPoint": 0, "objectPoint": 2, "xOffset": -6}),
        )
        .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, sid, None);
        let ft = &cs.frames[0];
        assert!((ft.lines[0].end_x - end_before).abs() < 1e-6, "the marker takes no space");
        let o = &ft.objects[0];
        // The frame's inner space is the spread's here (no rotation or offset).
        let margin_x = d.spreads[0].pages[0].margin_rect().x0;
        assert!((o.origin.x + o.size.0 - (margin_x - 6.0)).abs() < 1e-6, "right edge 6 pt left of the margin: {:?}", o.origin);
        assert!((o.origin.y - ft.lines[0].baseline).abs() < 1e-6);
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
        let obj = back.stories.values().flat_map(|st| st.objects.iter()).next().unwrap();
        assert!(
            matches!(obj.position, designcraft_doc::AnchorPosition::Custom { x_offset, object_point: 2, .. } if x_offset == -6.0),
            "IDML Anchored position"
        );
    }
}
