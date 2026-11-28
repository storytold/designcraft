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
            "{ids, story?, pos? (default: the text insertion point), position?: inline|aboveLine|custom, yOffset?, align?: left|center|right, spaceBefore?, spaceAfter?, xRelative? | yRelative?: anchor|textFrame|columnEdge|pageMargin|pageEdge, xOffset?, objectPoint? | refPoint?: 0..8, keepWithinColumn?, spineRelative? (custom: the settings describe a left-hand page and mirror on a right-hand page)} — moves the items into the text",
            has_doc,
            insert
        ),
        cmd!(
            "anchored.options",
            "Anchored Object Options…",
            ["Object", "Anchored Object"],
            None,
            "{story?, index? (default: the selected anchored object, else the one at the insertion point), position?: inline|aboveLine|custom, yOffset?, align?: left|center|right, spaceBefore?, spaceAfter?, xRelative? | yRelative?: anchor|textFrame|columnEdge|pageMargin|pageEdge, xOffset?, objectPoint? | refPoint?: 0..8, keepWithinColumn?, spineRelative? (custom: the settings describe a left-hand page and mirror on a right-hand page)}",
            has_doc,
            options
        ),
        cmd!(
            "anchored.release",
            "Release",
            ["Object", "Anchored Object"],
            None,
            "{story?, index? (default: the selected anchored object, else the one at the insertion point)} — puts the object back on the page where it is now and removes it from the text",
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
            let (xr0, yr0, xo0, yo0, op0, rp0, k0, sr0) = match b {
                AnchorPosition::Custom {
                    x_relative,
                    y_relative,
                    x_offset,
                    y_offset,
                    object_point,
                    ref_point,
                    keep_within_column,
                    spine_relative,
                } => (*x_relative, *y_relative, *x_offset, *y_offset, *object_point, *ref_point, *keep_within_column, *spine_relative),
                _ => (R::TextFrame, R::Anchor, 0.0, 0.0, 0, 0, false, false),
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
                spine_relative: p.get("spineRelative").and_then(Value::as_bool).unwrap_or(sr0),
            }
        }
        (o, _) => return Err(bad("anchored", format!("unknown position `{o}`"))),
    })
}

/// Validate anchored frame threads and reject cycles before committing the edit.
fn check_anchored(d: &Document, cmd: &str) -> Result<()> {
    d.check().map_err(|e| bad(cmd, format!("these items cannot be anchored here: {e}")))
}

/// Anchor copies of `items` (in spread coordinates) at `pos` of `sid`; returns the end position.
pub(crate) fn anchor_items(d: &mut Document, sid: StoryId, pos: usize, items: Vec<Item>, position: &AnchorPosition) -> Result<usize> {
    let count = items.len();
    let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
    let mut at = pos.min(st.len());
    for it in items {
        st.insert_object(at, AnchoredObject::new(it, position.clone()));
        at += OBJECT_MARK.len_utf8();
    }
    d.adjust_text_ranges(sid, pos..pos, count * OBJECT_MARK.len_utf8());
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
        // Into spread coordinates (items inside groups carry their parents' transforms).
        let mut it = it.clone();
        it.xf = st.doc.parent_xf(&loc) * it.xf;
        items.push(it);
    }
    let has_text = items.iter().any(|item| {
        let mut found = false;
        item.walk(&mut |i| found |= i.text_frame().is_some());
        found
    });
    s.edit(|d, sel| {
        for id in &ids {
            d.take_item(*id)?;
        }
        let end = anchor_items(d, sid, pos, items.clone(), &position)?;
        if has_text {
            check_anchored(d, "anchored.insert")?;
        }
        *sel = designcraft_doc::Selection::text(TextSel { story: sid, anchor: end, focus: end, frame: None, cell: None });
        Ok(json!({"story": sid.0, "pos": end}))
    })
}

/// Whether the `k`-th anchored object of story `sid` can't be selected or changed: it, or the text
/// frame holding it, is locked or on a locked layer (as for other page items).
fn is_locked(s: &Session, sid: StoryId, k: usize) -> Result<bool> {
    let d = &s.doc()?.doc;
    let locked = |it: &designcraft_doc::Item| it.locked || d.layer(it.layer).is_some_and(|l| l.locked);
    let o = d.story(sid).and_then(|st| st.objects.get(k)).ok_or_else(|| bad("anchored", "no such anchored object"))?;
    if locked(&o.item) {
        return Ok(true);
    }
    // The frame composition placed it in; before it is placed, any of the story's frames.
    let cs = s.cache.get(d, sid, None);
    let host = cs.frames.iter().find(|ft| ft.objects.iter().any(|x| x.index == k)).map(|ft| ft.frame);
    let frames: Vec<designcraft_doc::ItemId> = match host {
        Some(f) => vec![f],
        None => d.story(sid).map(|st| st.frames.clone()).unwrap_or_default(),
    };
    Ok(frames.iter().filter_map(|f| d.item(*f)).any(locked))
}

/// [`is_locked`] as the error a command returns.
fn check_unlocked(s: &Session, sid: StoryId, k: usize, cmd: &str) -> Result<()> {
    if is_locked(s, sid, k)? {
        return Err(bad(cmd, "the anchored object or its frame is locked, or on a locked layer"));
    }
    Ok(())
}

/// `{story, index}`, else the selected anchored object, else the one at (or just before) the text
/// insertion point.
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
    if s.doc()?.doc.story(sid).and_then(|x| x.objects.get(k)).is_none() {
        return Err(bad("anchored.select", "no such anchored object"));
    }
    check_unlocked(s, sid, k, "anchored.select")?;
    let st = s.doc_mut()?;
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
    check_unlocked(s, sid, k, "anchored.resize")?;
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
    // A locked object keeps its place in the text (Delete does nothing to it, as to a locked item).
    check_unlocked(s, a.story, a.index, "edit.clear")?;
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
    check_unlocked(s, sid, k, "anchored.options")?;
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
    check_unlocked(s, sid, k, "anchored.release")?;
    let d0 = s.doc()?.doc.clone();
    let st = d0.story(sid).ok_or_else(|| bad("anchored.release", "no such story"))?;
    let obj = st.objects.get(k).cloned().ok_or_else(|| bad("anchored.release", "no such anchored object"))?;
    let pos = st.text.match_indices(OBJECT_MARK).nth(k).map(|(i, _)| i).ok_or_else(|| bad("anchored.release", "no mark"))?;
    // Where it shows now: its frame's transform and its placement.
    let cs = s.cache.get(&d0, sid, None);
    let placed = cs.frames.iter().find_map(|ft| ft.objects.iter().find(|o| o.index == k).map(|o| (ft.frame, o.origin)));
    let (spread, xf) = match placed {
        Some((fid, origin)) => {
            let frame = d0.item(fid).or_else(|| d0.anchored_item(fid)).ok_or_else(|| bad("anchored.release", "frame not found"))?;
            let (spread, parent) = if let Some(loc) = d0.find(fid) {
                (loc.spread, d0.parent_xf(&loc))
            } else {
                let placement = designcraft_compose::frame_placement_with(&d0, sid, fid, &mut |host| s.cache.get(&d0, host, None))
                    .ok_or_else(|| bad("anchored.release", "frame not found"))?;
                (placement.spread, placement.xf)
            };
            (spread, parent * d0.text_xf(frame) * Affine::translate(origin.to_vec2()))
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
        // A text frame keeps its place in its story's thread under its new id.
        if let Some(st) = it.text_frame().and_then(|t| d.story_mut(t.story)) {
            match st.frames.iter_mut().find(|f| **f == obj.item.id) {
                Some(f) => *f = it.id,
                None => st.frames.push(it.id),
            }
        }
        let id = d.insert_item(spread, it, None)?;
        let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
        let len = OBJECT_MARK.len_utf8();
        st.delete(pos..pos + len);
        d.adjust_text_ranges(sid, pos..pos + len, 0);
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

    /// Review of #1195: an anchored selection follows its object through edits of the story. An
    /// object inserted before it moves it along; resizing it keeps it selected; deleting its
    /// text deletes it from the selection too, so Delete never takes another object.
    #[test]
    fn an_anchored_selection_follows_its_object() {
        let (mut s, sid) = inline_box();
        let id = |s: &Session, k: usize| s.doc().unwrap().doc.story(sid).unwrap().objects[k].item.id;
        s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
        let selected = id(&s, 0);
        // Another object anchored at the start of the story, before the selected one.
        let r = s.execute("frame.create", &json!({"rect": [450, 300, 470, 320]})).unwrap();
        s.execute("anchored.insert", &json!({"ids": [r["id"]], "story": sid.0, "pos": 0})).unwrap();
        // The selected object is now the second.
        let st = s.doc_mut().unwrap();
        st.selection = designcraft_doc::Selection { anchored: Some(AnchoredSel { story: sid, index: 1 }), ..Default::default() };
        assert_eq!(id(&s, 1), selected);
        // An edit that inserts an object before it: the selection moves with it.
        s.edit(|d, _| {
            let st = d.story_mut(sid).unwrap();
            let first = (*st.objects[0]).clone();
            let mut copy = first;
            copy.item.id = ItemId(9_999);
            st.insert_object(0, copy);
            Ok(())
        })
        .unwrap();
        let a = s.doc().unwrap().selection.anchored.unwrap();
        assert_eq!((a.index, id(&s, a.index)), (2, selected), "follows its object");
        // Resizing keeps it selected.
        s.execute("anchored.resize", &json!({"from": [0, 0, 24, 40], "to": [0, 0, 30, 40]})).unwrap();
        assert_eq!(s.doc().unwrap().selection.anchored.map(|a| a.index), Some(2));
        // Its text deleted with the object: nothing stays selected, rather than another object.
        let pos = s.doc().unwrap().doc.story(sid).unwrap().text.match_indices(designcraft_doc::OBJECT_MARK).nth(2).unwrap().0;
        s.edit(|d, _| {
            d.story_mut(sid).unwrap().delete(pos..pos + designcraft_doc::OBJECT_MARK.len_utf8());
            Ok(())
        })
        .unwrap();
        assert_eq!(s.doc().unwrap().selection.anchored, None);
        let _ = s.execute("edit.clear", &json!({}));
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().objects.len(), 2, "Delete took nothing");
    }

    /// A locked anchored object, one on a locked layer, or one in a locked frame or a frame on a
    /// locked layer can't be selected, resized, deleted, changed in Anchored Object Options or
    /// released: it stays in the text with its marker.
    #[test]
    fn locked_anchored_objects_are_left_alone() {
        type Lock = fn(&mut designcraft_doc::Document, StoryId);
        let lock_object: Lock = |d, sid| Arc::make_mut(&mut d.story_mut(sid).unwrap().objects[0]).item.locked = true;
        let lock_object_layer: Lock = |d, sid| {
            let layer = d.story(sid).unwrap().objects[0].item.layer;
            d.layers.iter_mut().find(|l| l.id == layer).unwrap().locked = true;
        };
        let lock_frame: Lock = |d, sid| {
            let f = d.story(sid).unwrap().frames[0];
            d.item_mut(f).unwrap().locked = true;
        };
        let lock_frame_layer: Lock = |d, sid| {
            let f = d.story(sid).unwrap().frames[0];
            let layer = d.item(f).unwrap().layer;
            d.layers.iter_mut().find(|l| l.id == layer).unwrap().locked = true;
        };
        for (what, lock) in
            [("object", lock_object), ("its layer", lock_object_layer), ("its frame", lock_frame), ("its frame's layer", lock_frame_layer)]
        {
            let (mut s, sid) = inline_box();
            // Selected before it is locked, so Delete and Resize meet the lock themselves.
            s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
            s.edit(|d, _| {
                lock(d, sid);
                Ok(Value::Null)
            })
            .unwrap();
            assert!(s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).is_err(), "{what}: select");
            assert!(s.execute("anchored.resize", &json!({"from": [0, 0, 24, 40], "to": [0, 0, 24, 60]})).is_err(), "{what}: resize");
            assert!(s.execute("edit.clear", &json!({})).is_err(), "{what}: delete");
            // Anchored Object Options and Release (on the selected object, as from the menu).
            assert!(s.execute("anchored.options", &json!({"position": "aboveLine"})).is_err(), "{what}: options");
            assert!(s.execute("anchored.release", &json!({})).is_err(), "{what}: release");
            let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
            assert_eq!(st.objects.len(), 1, "{what}: the object stays");
            assert_eq!(st.text.matches(OBJECT_MARK).count(), 1, "{what}: and its marker");
            assert_eq!(st.objects[0].size(), (24.0, 40.0), "{what}: at its size");
            assert!(matches!(st.objects[0].position, AnchorPosition::Inline { .. }), "{what}: inline still");
        }
    }

    /// Placing a graphic selects the new frame alone: a selected anchored object is deselected.
    #[test]
    fn placing_a_graphic_deselects_an_anchored_object() {
        let (mut s, sid) = inline_box();
        s.execute("anchored.select", &json!({"story": sid.0, "index": 0})).unwrap();
        // A video (any bytes: media is placed by its name) gets a media frame of its own.
        let b64 = crate::cmd::base64_encode(b"not really a video");
        let r = s.execute("file.place", &json!({"name": "clip.mp4", "base64": b64, "x": 500, "y": 500})).unwrap();
        let sel = s.doc().unwrap().selection.clone();
        assert_eq!(sel.items, [ItemId(r["id"].as_u64().unwrap())]);
        assert_eq!(sel.anchored, None, "not both the new frame and the anchored object");
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
            layer_options: Default::default(),
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
        // Relative to Spine keeps the other custom settings.
        s.execute("anchored.options", &json!({"story": sid.0, "index": 0, "spineRelative": true})).unwrap();
        let st = s.doc().unwrap().doc.story(sid).unwrap().clone();
        assert!(matches!(
            st.objects[0].position,
            designcraft_doc::AnchorPosition::Custom { spine_relative: true, x_offset, object_point: 2, .. } if x_offset == -6.0
        ));
    }
}

#[cfg(test)]
mod text_frame_tests {
    use designcraft_doc::{ParaFormat, StoryId};
    use designcraft_geom::Rect;
    use serde_json::json;

    use super::*;

    /// Anchor, at `pos` of `host`, a 200 × 40 pt text frame showing a new story with `text`.
    fn anchor(d: &mut Document, host: StoryId, pos: usize, text: &str) -> designcraft_doc::Result<(StoryId, ItemId)> {
        let layer = d.default_layer();
        let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 40.0), layer, text, ParaFormat::default())?;
        let item = d.item(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?.clone();
        d.spread_mut(SpreadRef::Doc(0)).ok_or(designcraft_doc::DocError::NoItem(fid))?.items.retain(|i| i.id != fid);
        d.story_mut(host).ok_or(designcraft_doc::DocError::NoStory(host))?.insert_object(pos, AnchoredObject::new(item, AnchorPosition::default()));
        Ok((sid, fid))
    }

    /// A new document with a text frame whose story holds, after "Host ", an anchored text frame
    /// showing "Anchored words " and, in that, one showing "Deeper". Returns the host story, the
    /// anchored story and the anchored frame's id.
    fn session() -> (Session, StoryId, StoryId, ItemId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Host  end"})).unwrap();
        let host = StoryId(r["story"].as_u64().unwrap());
        let (sid, id) = s
            .edit(|d, _| {
                let (sid, id) = anchor(d, host, 5, "Anchored words ")?;
                anchor(d, sid, 15, "Deeper")?;
                Ok((sid, id))
            })
            .unwrap();
        s.doc().unwrap().doc.check().unwrap();
        (s, host, sid, id)
    }

    /// The document is valid (and opens again once saved); returns the texts its anchored text
    /// frames show, sorted, after checking each has a story of its own.
    fn shown(s: &Session) -> Vec<String> {
        let d = &s.doc().unwrap().doc;
        d.check().unwrap();
        designcraft_format::load(&designcraft_format::save(d).unwrap()).unwrap();
        let stories: Vec<StoryId> = d.anchored_items().iter().filter_map(|(_, i)| i.text_frame()).map(|t| t.story).collect();
        let distinct: std::collections::HashSet<_> = stories.iter().collect();
        assert_eq!(distinct.len(), stories.len(), "a story of its own each");
        let mut texts: Vec<String> = stories.iter().map(|sid| d.story(*sid).unwrap().text.clone()).collect();
        texts.sort();
        texts
    }

    const SHOWN: [&str; 2] = ["Anchored words \u{E00E}", "Deeper"];

    #[test]
    fn releasing_an_anchored_text_frame_threads_its_story_to_it() {
        let (mut s, host, sid, _) = session();
        s.execute("anchored.release", &json!({"story": host.0, "index": 0})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        d.check().unwrap();
        let id = s.doc().unwrap().selection.items[0];
        assert_eq!(d.story(sid).unwrap().frames, vec![id]);
        assert_eq!(d.item(id).unwrap().text_frame().unwrap().story, sid);
        assert!(s.cache.get(&d, sid, None).frame(id).is_some_and(|f| !f.lines.is_empty()));
        assert_eq!(shown(&s), ["Deeper"]);
    }

    #[test]
    fn copied_anchored_text_frames_get_stories_of_their_own() {
        let (mut s, host, sid, id) = session();
        // Text copied and pasted in the same story.
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": 9})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let len = s.doc().unwrap().doc.story(host).unwrap().len();
        s.execute("text.select", &json!({"story": host.0, "anchor": len, "focus": len})).unwrap();
        s.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(shown(&s), ["Anchored words \u{E00E}", "Anchored words \u{E00E}", "Deeper", "Deeper"]);
        // The original keeps its frame and story.
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().frames, vec![id]);
        // The host frame duplicated.
        let frame = s.doc().unwrap().doc.story(host).unwrap().frames[0];
        s.execute("selection.set", &json!({"ids": [frame.0]})).unwrap();
        s.execute("edit.duplicate", &json!({})).unwrap();
        assert_eq!(shown(&s).len(), 8);
        // Text dragged with a copy.
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": 9})).unwrap();
        s.edit(|d, _| {
            let st = d.story_mut(host).ok_or(designcraft_doc::DocError::NoStory(host))?;
            let piece = st.extract(0..9);
            st.insert_story(0, &piece);
            Ok(())
        })
        .unwrap();
        assert_eq!(shown(&s).len(), 10);
    }

    /// The endnote frame lists the endnotes of anchored stories copied or deleted with their
    /// text at once: it used to be brought up to date before the copies were made.
    #[test]
    fn the_endnote_frame_follows_anchored_stories_copied_and_deleted() {
        let (mut s, host, sid, _) = session();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("endnote.insert", &json!({"text": "A note"})).unwrap();
        let notes = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            d.story(d.endnote_story.unwrap()).unwrap().text.matches("A note").count()
        };
        assert_eq!(notes(&s), 1);
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": 9})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let len = s.doc().unwrap().doc.story(host).unwrap().len();
        s.execute("text.select", &json!({"story": host.0, "anchor": len, "focus": len})).unwrap();
        s.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(notes(&s), 2, "copied");
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": 9})).unwrap();
        s.execute("edit.cut", &json!({})).unwrap();
        assert_eq!(notes(&s), 1, "deleted");
    }

    /// Text with an anchored text frame pasted into the generated endnote story is replaced when
    /// the endnotes are set again; the repair after it leaves no story threaded to the dropped frame.
    #[test]
    fn anchored_text_frames_pasted_into_the_endnote_story_leave_a_valid_document() {
        let (mut s, host, sid, _) = session();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("endnote.insert", &json!({"text": "A note"})).unwrap();
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": 9})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let notes = s.doc().unwrap().doc.endnote_story.unwrap();
        let len = s.doc().unwrap().doc.story(notes).unwrap().len();
        s.execute("text.select", &json!({"story": notes.0, "anchor": len, "focus": len})).unwrap();
        let _ = s.execute("edit.paste", &json!({}));
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn deleted_anchored_text_frames_take_their_stories_along() {
        let (mut s, host, sid, _) = session();
        let stories = s.doc().unwrap().doc.stories.len();
        // Cut with the text it is in, and pasted back.
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": 9})).unwrap();
        s.execute("edit.cut", &json!({})).unwrap();
        assert!(shown(&s).is_empty());
        assert!(s.doc().unwrap().doc.story(sid).is_none());
        assert_eq!(s.doc().unwrap().doc.stories.len(), stories - 2);
        s.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(shown(&s), SHOWN);
        assert_eq!(s.doc().unwrap().doc.stories.len(), stories);
        // Deleted with the frame it is anchored in; undo brings it back.
        let frame = s.doc().unwrap().doc.story(host).unwrap().frames[0];
        s.execute("selection.set", &json!({"ids": [frame.0]})).unwrap();
        s.execute("edit.clear", &json!({})).unwrap();
        assert!(shown(&s).is_empty());
        assert!(s.doc().unwrap().doc.stories.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(shown(&s), SHOWN);
    }

    #[test]
    fn anchored_text_frames_paste_into_another_document() {
        let (mut s, host, _, _) = session();
        let frame = s.doc().unwrap().doc.story(host).unwrap().frames[0];
        // A second document, whose ids are those of the first one.
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Other "})).unwrap();
        let other = StoryId(r["story"].as_u64().unwrap());
        s.edit(|d, _| Ok(anchor(d, other, 6, "Elsewhere")?)).unwrap();
        // Copied text.
        s.set_active(0);
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": 9})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s.set_active(1);
        let len = s.doc().unwrap().doc.story(other).unwrap().len();
        s.execute("text.select", &json!({"story": other.0, "anchor": len, "focus": len})).unwrap();
        s.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(shown(&s), ["Anchored words \u{E00E}", "Deeper", "Elsewhere"]);
        // A copied frame.
        s.set_active(0);
        s.execute("selection.set", &json!({"ids": [frame.0]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s.set_active(1);
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        s.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(shown(&s), ["Anchored words \u{E00E}", "Anchored words \u{E00E}", "Deeper", "Deeper", "Elsewhere"]);
    }

    /// Place a 4 × 4 pt graphic of grey `v`; returns its frame and its asset.
    fn place(s: &mut Session, v: u8) -> (ItemId, designcraft_doc::AssetId) {
        let png = designcraft_render::Rendered { width: 4, height: 4, pixels: vec![v; 64] }.to_png();
        let r =
            s.execute("file.place", &json!({"base64": crate::cmd::file::base64_encode(&png), "name": format!("{v}.png"), "x": 72, "y": 72})).unwrap();
        (ItemId(r["id"].as_u64().unwrap()), designcraft_doc::AssetId(r["asset"].as_u64().unwrap()))
    }

    /// The files the graphics anchored in the active document show (in its anchored text frames'
    /// stories too), sorted; `None` for a graphic whose asset is missing.
    fn anchored_files(s: &Session) -> Vec<Option<Arc<Vec<u8>>>> {
        let d = &s.doc().unwrap().doc;
        d.check().unwrap();
        let mut files: Vec<_> =
            d.anchored_items().iter().filter_map(|(_, i)| i.graphic()).map(|g| d.assets.get(&g.asset).map(|a| a.data.load().unwrap())).collect();
        files.sort();
        files
    }

    /// Copied text pasted into another document takes along the files of the graphics anchored
    /// in it and in the stories of its anchored text frames: their asset ids may name other files
    /// there, or none.
    #[test]
    fn pasted_text_takes_its_graphics_into_another_document() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let (inline, a) = place(&mut s, 10);
        let (nested, _) = place(&mut s, 20);
        let r = s.execute("frame.create", &json!({"rect": [72, 200, 400, 500], "content": "text", "text": "Host  end"})).unwrap();
        let host = StoryId(r["story"].as_u64().unwrap());
        let (sid, _) = s.edit(|d, _| Ok(anchor(d, host, 5, "Anchored words ")?)).unwrap();
        // A graphic in the copied text and one in its anchored text frame's story.
        s.execute("anchored.insert", &json!({"ids": [inline.0], "story": host.0, "pos": 0})).unwrap();
        s.execute("anchored.insert", &json!({"ids": [nested.0], "story": sid.0, "pos": 0})).unwrap();
        let files = anchored_files(&s);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(Option::is_some));
        let len = s.doc().unwrap().doc.story(host).unwrap().len();
        s.execute("text.select", &json!({"story": host.0, "anchor": 0, "focus": len})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let plain = s.text_clipboard.as_ref().unwrap().1.clone();

        // Another document whose own graphic has the asset id of the first copied one.
        s.execute("file.new", &json!({})).unwrap();
        let (_, b) = place(&mut s, 30);
        assert_eq!(a, b, "fresh documents allocate the same first asset id");
        let own = s.doc().unwrap().doc.assets.get(&b).unwrap().data.clone();
        let r = s.execute("frame.create", &json!({"rect": [72, 200, 400, 500], "content": "text", "text": "Other "})).unwrap();
        let other = StoryId(r["story"].as_u64().unwrap());
        // Into text.
        s.execute("text.select", &json!({"story": other.0, "anchor": 6, "focus": 6})).unwrap();
        s.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(anchored_files(&s), files);
        // Into a new text frame.
        s.execute("edit.deselectAll", &json!({})).unwrap();
        s.execute("edit.paste", &json!({"text": plain})).unwrap();
        let twice: Vec<_> = files.iter().flat_map(|f| [f.clone(), f.clone()]).collect();
        assert_eq!(anchored_files(&s), twice);
        // The document's own graphic keeps its file.
        assert_eq!(s.doc().unwrap().doc.assets.get(&b).unwrap().data, own);
    }
}

#[cfg(test)]
mod anchored_text_frame_tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::Session;

    fn setup(text: &str) -> (Session, ItemId, StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text", "text": text})).unwrap();
        (s, ItemId(r["id"].as_u64().unwrap()), StoryId(r["story"].as_u64().unwrap()))
    }

    fn add_note(s: &mut Session, text: &str) -> (ItemId, StoryId) {
        let r = s.execute("frame.create", &json!({"rect": [72, 400, 140, 450], "content": "text", "text": text})).unwrap();
        (ItemId(r["id"].as_u64().unwrap()), StoryId(r["story"].as_u64().unwrap()))
    }

    fn anchor(s: &mut Session, note: ItemId, host: StoryId, pos: u64, extra: Value) {
        let mut p = json!({"ids": [note.0], "story": host.0, "pos": pos});
        if let (Some(dst), Some(src)) = (p.as_object_mut(), extra.as_object()) {
            dst.extend(src.clone());
        }
        s.execute("anchored.insert", &p).unwrap();
    }

    #[test]
    fn inline_anchored_text_frame_composes_and_moves_with_its_anchor() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note A\nNote B");
        let r = s.execute("anchored.insert", &json!({"ids": [note.0], "story": host_sid.0, "pos": 7})).unwrap();
        assert_eq!(r["pos"], json!(7 + OBJECT_MARK.len_utf8()));
        let d = s.doc().unwrap().doc.clone();
        assert!(d.item(note).is_none(), "moved into the text");
        assert!(d.anchored_item(note).is_some());
        assert_eq!(d.story(note_sid).unwrap().frames, [note]);
        d.check().unwrap();
        let host = s.cache.get(&d, host_sid, None);
        let first = host.frames[0].objects[0].origin;
        let note_frames = s.cache.get(&d, note_sid, None);
        assert_eq!(note_frames.frames.len(), 1);
        assert!(!note_frames.is_overset());
        assert!(note_frames.frames[0].lines.len() >= 2);
        // Typing before the anchor moves the anchored frame with its line.
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("text.insert", &json!({"text": "Words "})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let moved = s.cache.get(&d, host_sid, None).frames[0].objects[0].origin;
        assert!(moved.x > first.x + 1e-6, "{first:?} -> {moved:?}");
        let note_frames = s.cache.get(&d, note_sid, None);
        assert!(!note_frames.is_overset());
        d.check().unwrap();
    }

    #[test]
    fn anchored_text_frame_supports_inline_above_line_and_custom_positions() {
        let positions = [
            ("inline", json!({"position": "inline"})),
            ("aboveLine", json!({"position": "aboveLine", "align": "center", "spaceAfter": 4.0})),
            (
                "custom",
                json!({"position": "custom", "xRelative": "textFrame", "yRelative": "anchor", "xOffset": 190.0, "objectPoint": 0, "refPoint": 0}),
            ),
        ];
        for (name, position) in positions {
            let (mut s, _host, host_sid) = setup("Anchor line here");
            let (note, note_sid) = add_note(&mut s, "Note");
            anchor(&mut s, note, host_sid, 6, position);
            let d = s.doc().unwrap().doc.clone();
            d.check().unwrap();
            let cs = s.cache.get(&d, note_sid, None);
            assert_eq!(cs.frames.len(), 1, "{name}");
            assert!(!cs.is_overset(), "{name}");
            assert!(!cs.frames[0].lines.is_empty(), "{name}");
            assert_eq!(d.story(host_sid).unwrap().objects.len(), 1, "{name}");
            assert!(d.anchored_item(note).is_some(), "{name}");
        }
    }

    #[test]
    fn anchoring_keeps_the_text_frames_story_text_styles_corners_and_appearance() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note A\nNote B");
        let len = s.doc().unwrap().doc.story(note_sid).unwrap().len();
        s.execute("text.select", &json!({"story": note_sid.0, "anchor": 0, "focus": len})).unwrap();
        s.execute("type.char", &json!({"size": 18.0, "fontStyle": "Bold"})).unwrap();
        s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
        s.execute("object.cornerOptions", &json!({"ids": [note.0], "shape": "rounded", "size": 9.0})).unwrap();
        s.execute("object.fill", &json!({"ids": [note.0], "swatch": "[Black]"})).unwrap();
        s.execute("object.stroke", &json!({"ids": [note.0], "swatch": "[Black]", "weight": 3.0})).unwrap();
        let before = s.doc().unwrap().doc.item(note).unwrap().clone();
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let d = s.doc().unwrap().doc.clone();
        let after = d.anchored_item(note).unwrap();
        assert_eq!(d.story(note_sid).unwrap().text, "Note A\nNote B");
        let st = d.story(note_sid).unwrap();
        assert_eq!(st.char_format_at(0).over.size, Some(18.0));
        assert_eq!(st.char_format_at(0).over.font_style.as_deref(), Some("Bold"));
        assert_eq!(st.paras[0].para.align, Some(designcraft_doc::Align::Center));
        assert_eq!(after.corners, before.corners, "corner options");
        assert_eq!(after.fill, before.fill, "fill");
        assert_eq!(after.stroke, before.stroke, "stroke");
        d.check().unwrap();
    }

    #[test]
    fn an_anchored_text_frame_follows_its_anchor_across_a_page_break_and_undo_redo() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false, "width": 300, "height": 300})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 264, 80], "content": "text", "text": "One\nTwo\nThree\nFour\nFive"})).unwrap();
        let host = ItemId(r["id"].as_u64().unwrap());
        let host_sid = StoryId(r["story"].as_u64().unwrap());
        let r = s.execute("frame.create", &json!({"spread": 1, "rect": [36, 36, 264, 260], "content": "unassigned"})).unwrap();
        let next = ItemId(r["id"].as_u64().unwrap());
        s.execute("text.thread", &json!({"from": host.0, "to": next.0})).unwrap();
        let (note, note_sid) = add_note(&mut s, "Note");
        let at = s.doc().unwrap().doc.story(host_sid).unwrap().text.find("Two").unwrap();
        anchor(
            &mut s,
            note,
            host_sid,
            at as u64,
            json!({"position": "custom", "xRelative": "pageEdge", "yRelative": "anchor", "xOffset": 5.0, "objectPoint": 0, "refPoint": 0}),
        );
        let placement = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            designcraft_compose::frame_placement(d, note_sid, note).expect("the note is placed by its host")
        };
        let first = placement(&s);
        assert_eq!(first.spread, designcraft_doc::SpreadRef::Doc(0));
        // Four extra lines push "Two" into the second frame on the next page.
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": 0, "focus": 0})).unwrap();
        s.execute("text.insert", &json!({"text": "A\nB\nC\nD\n"})).unwrap();
        let moved = placement(&s);
        assert_eq!(moved.spread, designcraft_doc::SpreadRef::Doc(1));
        // Undo and redo restore the anchored position on the page it was on.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(placement(&s), first);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(placement(&s), moved);
        // Removing the text moves it back; undoing the removal moves it across again.
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": 0, "focus": "A\nB\nC\nD\n".len()})).unwrap();
        s.execute("edit.clear", &json!({})).unwrap();
        assert_eq!(placement(&s), first);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(placement(&s), moved);
        s.doc().unwrap().doc.check().unwrap();
    }

    #[test]
    fn deleting_an_anchored_text_frames_mark_removes_the_frame_and_undo_restores_it() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark + OBJECT_MARK.len_utf8()})).unwrap();
        s.execute("edit.clear", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(host_sid).unwrap().objects.is_empty());
        assert!(d.anchored_item(note).is_none());
        assert!(d.story(note_sid).is_none(), "the frame's story goes with it");
        d.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(host_sid).unwrap().objects[0].item.id, note);
        assert_eq!(d.story(note_sid).unwrap().text, "Note");
        assert_eq!(d.story(note_sid).unwrap().frames, [note]);
        d.check().unwrap();
        // Backspace after the mark removes it too.
        let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap() + OBJECT_MARK.len_utf8();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(host_sid).unwrap().objects.is_empty());
        assert!(d.anchored_item(note).is_none());
        d.check().unwrap();
        // Typing over the mark replaces it too.
        s.execute("edit.undo", &json!({})).unwrap();
        let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark + OBJECT_MARK.len_utf8()})).unwrap();
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(host_sid).unwrap().objects.is_empty());
        assert!(d.anchored_item(note).is_none());
        d.check().unwrap();
    }

    #[test]
    fn deleting_a_grouped_anchor_unthreads_the_text_frame_inside_it() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        let rect = s.execute("frame.create", &json!({"rect": [0, 0, 20, 20]})).unwrap()["id"].as_u64().unwrap();
        let group = s.execute("object.group", &json!({"ids": [note.0, rect]})).unwrap()["id"].as_u64().unwrap();
        anchor(&mut s, ItemId(group), host_sid, 7, json!({"position": "inline"}));
        let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark + OBJECT_MARK.len_utf8()})).unwrap();
        s.execute("edit.clear", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.anchored_item(note).is_none());
        assert!(d.story(note_sid).is_none(), "the nested frame's story goes with it");
        d.check().unwrap();
        // Typing over the grouped anchor does the same.
        s.execute("edit.undo", &json!({})).unwrap();
        let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark + OBJECT_MARK.len_utf8()})).unwrap();
        s.execute("text.insert", &json!({"text": "x"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.anchored_item(note).is_none());
        assert!(d.story(note_sid).is_none());
        d.check().unwrap();
    }

    #[test]
    fn removing_a_frame_unthreads_text_frames_anchored_in_its_story() {
        let (mut s, host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        s.execute("selection.set", &json!({"ids": [host.0]})).unwrap();
        s.execute("edit.clear", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(host_sid).is_none());
        assert!(d.anchored_item(note).is_none());
        assert!(d.story(note_sid).is_none(), "the nested frame's story goes too");
        d.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(host_sid).unwrap().objects[0].item.id, note);
        assert_eq!(d.story(note_sid).unwrap().text, "Note");
        d.check().unwrap();
    }

    #[test]
    fn pasting_objects_over_a_text_selection_unthreads_the_old_anchor() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let rect = s.execute("frame.create", &json!({"rect": [450, 450, 474, 490]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [rect]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark + OBJECT_MARK.len_utf8()})).unwrap();
        s.execute("edit.paste", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(note_sid).is_none(), "the replaced anchor's story goes too");
        assert_eq!(d.story(host_sid).unwrap().objects.len(), 1);
        assert!(d.story(host_sid).unwrap().objects[0].item.text_frame().is_none());
        d.check().unwrap();
    }

    #[test]
    fn replacing_an_anchor_through_story_or_find_change_unthreads_its_story() {
        for find_change in [false, true] {
            let (mut s, _host, host_sid) = setup("Before after and more words.");
            let (note, note_sid) = add_note(&mut s, "Note");
            anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
            if find_change {
                s.execute("find.change", &json!({"find": "Before.*after", "grep": true, "change": "x", "scope": "story", "story": host_sid.0}))
                    .unwrap();
            } else {
                let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap();
                s.execute("story.replaceRange", &json!({"story": host_sid.0, "start": mark, "end": mark + OBJECT_MARK.len_utf8(), "text": ""}))
                    .unwrap();
            }
            let d = s.doc().unwrap().doc.clone();
            assert!(d.anchored_item(note).is_none(), "find_change={find_change}");
            assert!(d.story(note_sid).is_none(), "find_change={find_change}");
            d.check().unwrap();
        }
    }

    #[test]
    fn replacing_an_anchor_through_footnote_table_or_math_unthreads_it() {
        for kind in ["footnote", "table", "math"] {
            let (mut s, _host, host_sid) = setup("Before after and more words.");
            let (note, note_sid) = add_note(&mut s, "Note");
            anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
            let len = s.doc().unwrap().doc.story(host_sid).unwrap().len();
            s.execute("text.select", &json!({"story": host_sid.0, "anchor": 0, "focus": len})).unwrap();
            match kind {
                "footnote" => {
                    s.execute("footnote.insert", &json!({"text": "Source"})).unwrap();
                }
                "table" => {
                    s.execute("table.insert", &json!({"rows": 1, "cols": 1})).unwrap();
                }
                _ => {
                    s.execute("math.insert", &json!({"latex": "x"})).unwrap();
                }
            }
            let d = s.doc().unwrap().doc.clone();
            assert!(d.anchored_item(note).is_none(), "{kind}");
            assert!(d.story(note_sid).is_none(), "{kind}");
            d.check().unwrap();
        }
    }

    #[test]
    fn delete_surrounding_unthreads_a_removed_anchor() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let after = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap() + OBJECT_MARK.len_utf8();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": after, "focus": after})).unwrap();
        s.execute("text.deleteSurrounding", &json!({"before": 1})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.anchored_item(note).is_none());
        assert!(d.story(note_sid).is_none());
        d.check().unwrap();
    }

    #[test]
    fn removing_a_story_text_removes_its_anchored_frames_and_the_frames_they_hold() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (outer, outer_sid) = add_note(&mut s, "Outer");
        anchor(&mut s, outer, host_sid, 7, json!({"position": "inline"}));
        let (inner, inner_sid) = add_note(&mut s, "Inner");
        anchor(&mut s, inner, outer_sid, 0, json!({"position": "inline"}));
        assert!(s.doc().unwrap().doc.story(inner_sid).is_some());
        s.execute("story.setText", &json!({"story": host_sid.0, "text": "Replaced"})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(host_sid).unwrap().text, "Replaced");
        assert!(d.anchored_item(outer).is_none());
        assert!(d.story(outer_sid).is_none());
        assert!(d.story(inner_sid).is_none(), "an emptied story's own anchored frame is unthreaded too");
        d.check().unwrap();
    }

    #[test]
    fn nested_anchor_release_and_pdf_link() {
        let (mut s, _, host) = setup("Before after and more words.");
        let (outer, outer_sid) = add_note(&mut s, "Outer");
        let (inner, inner_sid) = add_note(&mut s, "Inner link");
        s.execute("text.select", &json!({"story": inner_sid.0, "anchor": 0, "focus": 10})).unwrap();
        s.execute("hyperlink.create", &json!({"url": "https://example.com/nested"})).unwrap();
        anchor(&mut s, outer, host, 7, json!({"position": "inline"}));
        anchor(&mut s, inner, outer_sid, 0, json!({"position": "inline"}));
        let d = s.doc().unwrap().doc.clone();
        let placement = designcraft_compose::frame_placement(&d, inner_sid, inner).unwrap();
        let expected_xf = placement.xf * d.anchored_item(inner).unwrap().xf;
        let pdf = s.execute("file.exportPdf", &json!({})).unwrap();
        let bytes = crate::cmd::base64_decode(pdf["base64"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&bytes).contains("https://example.com/nested"));
        let released = s.execute("anchored.release", &json!({"story": outer_sid.0, "index": 0})).unwrap();
        let id = ItemId(released["id"].as_u64().unwrap());
        assert_eq!(s.doc().unwrap().doc.story(inner_sid).unwrap().frames, [id]);
        assert_eq!(s.doc().unwrap().doc.item(id).unwrap().xf, expected_xf);
        s.doc().unwrap().doc.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.anchored_item(inner).is_some());
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.item(id).is_some());
    }

    #[test]
    fn tracked_deletion_of_an_anchor_cleans_up_when_accepted() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        s.execute("changes.track", &json!({"on": true})).unwrap();
        let mark = s.doc().unwrap().doc.story(host_sid).unwrap().text.find(OBJECT_MARK).unwrap();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark + OBJECT_MARK.len_utf8()})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        s.execute("changes.accept", &json!({"story": host_sid.0, "start": mark})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.anchored_item(note).is_none());
        assert!(d.story(note_sid).is_none());
        d.check().unwrap();
    }

    #[test]
    fn tracked_deletion_of_an_inserted_anchor_unthreads_it() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let d = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let st = d.story_mut(host_sid).unwrap();
        let mark = st.text.find(OBJECT_MARK).unwrap();
        st.format_chars(mark..mark + OBJECT_MARK.len_utf8(), |f| f.over.change = Some(designcraft_doc::ChangeMark::Inserted));
        s.execute("changes.track", &json!({"on": true})).unwrap();
        s.execute("text.select", &json!({"story": host_sid.0, "anchor": mark, "focus": mark + OBJECT_MARK.len_utf8()})).unwrap();
        s.execute("text.delete", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.anchored_item(note).is_none());
        assert!(d.story(note_sid).is_none());
        d.check().unwrap();
    }

    #[test]
    fn overset_text_in_an_anchored_frame_is_reported() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let r = s
            .execute("frame.create", &json!({"rect": [0, 0, 50, 25], "content": "text", "text": "One two three four five six seven eight nine ten"}))
            .unwrap();
        let note = ItemId(r["id"].as_u64().unwrap());
        let note_sid = StoryId(r["story"].as_u64().unwrap());
        anchor(&mut s, note, host_sid, 7, json!({"position": "aboveLine"}));
        let d = s.doc().unwrap().doc.clone();
        assert!(s.cache.get(&d, note_sid, None).is_overset(), "the note's text doesn't fit its frame");
        let report = s.execute("preflight.run", &json!({})).unwrap();
        let overset: Vec<_> = report["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "overset").collect();
        assert_eq!(overset.len(), 1, "{report}");
        assert!(overset[0]["message"].as_str().unwrap().contains("Overset text"), "{report}");
    }

    #[test]
    fn korean_text_in_an_anchored_frame_has_no_missing_glyphs_and_renders() {
        designcraft_fonts::FontDb::global().add_font(designcraft_fonts::testing::font_with("DC Test Anchored Korean", &['한']).unwrap());
        let (mut s, _host, host_sid) = setup("Latin body only.");
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 30, 30], "content": "text", "text": "한"})).unwrap();
        let note = ItemId(r["id"].as_u64().unwrap());
        let note_sid = StoryId(r["story"].as_u64().unwrap());
        s.execute("text.select", &json!({"story": note_sid.0, "anchor": 0, "focus": "한".len()})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "DC Test Anchored Korean", "language": "Korean"})).unwrap();
        anchor(
            &mut s,
            note,
            host_sid,
            6,
            json!({"position": "custom", "xRelative": "pageEdge", "yRelative": "anchor", "xOffset": 5.0, "objectPoint": 0, "refPoint": 0}),
        );
        let d = s.doc().unwrap().doc.clone();
        let cs = s.cache.get(&d, note_sid, None);
        let glyphs: Vec<_> = cs.frames[0].lines.iter().flat_map(|l| &l.glyphs).filter(|g| g.visible && !g.rendered_char.is_whitespace()).collect();
        assert_eq!(glyphs.len(), 1);
        assert_ne!(glyphs[0].gid, 0, "the Korean glyph is drawn");
        assert_eq!(glyphs[0].face.family, "DC Test Anchored Korean");
        let report = s.execute("preflight.run", &json!({})).unwrap();
        assert!(!report["issues"].as_array().unwrap().iter().any(|i| i["kind"] == "missingGlyphs"), "{report}");
        // It is drawn where the frame is placed, clear of the body text.
        let placed = designcraft_compose::frame_placement(&d, note_sid, note).unwrap();
        let bounds = placed.xf.transform_rect_bbox(d.anchored_item(note).unwrap().bounds());
        let image = designcraft_render::Renderer::new().render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let dark = (bounds.x0.max(0.0) as u32..bounds.x1.max(0.0) as u32)
            .flat_map(|x| (bounds.y0.max(0.0) as u32..bounds.y1.max(0.0) as u32).map(move |y| (x, y)))
            .filter(|&(x, y)| x < image.width && y < image.height && image.pixel(x, y)[0] < 128)
            .count();
        assert!(dark > 0, "the anchored Korean text renders: {bounds:?}");
    }

    #[test]
    fn anchoring_a_text_frame_from_a_threaded_story_keeps_the_thread_valid() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "One\nTwo\nThree\nFour");
        let r = s.execute("frame.create", &json!({"rect": [300, 400, 500, 600], "content": "unassigned"})).unwrap();
        let next = ItemId(r["id"].as_u64().unwrap());
        s.execute("text.thread", &json!({"from": note.0, "to": next.0})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(note_sid).unwrap().frames, [note, next]);
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(note_sid).unwrap().frames, [note, next], "the rest of the thread stays");
        assert!(d.item(note).is_none(), "the anchored frame left the spread");
        assert!(d.item(next).is_some(), "the next frame stays on its spread");
        let cs = s.cache.get(&d, note_sid, None);
        assert_eq!(cs.frames.len(), 2);
        assert!(cs.frames.iter().any(|f| f.frame == note));
        assert!(cs.frames.iter().any(|f| f.frame == next));
        assert_eq!(cs.frames.iter().map(|f| f.range.end).max(), Some(d.story(note_sid).unwrap().len()), "the thread keeps its text");
        assert!(!cs.is_overset());
        d.check().unwrap();
    }

    #[test]
    fn a_group_holding_a_text_frame_anchors_and_composes_it() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        let rect = s.execute("frame.create", &json!({"rect": [0, 0, 20, 20]})).unwrap()["id"].as_u64().unwrap();
        let group = s.execute("object.group", &json!({"ids": [note.0, rect]})).unwrap()["id"].as_u64().unwrap();
        anchor(&mut s, ItemId(group), host_sid, 7, json!({"position": "inline"}));
        let d = s.doc().unwrap().doc.clone();
        d.check().unwrap();
        assert!(d.item(ItemId(group)).is_none());
        assert!(d.anchored_item(note).is_some());
        assert_eq!(d.story(note_sid).unwrap().frames, [note]);
        let cs = s.cache.get(&d, note_sid, None);
        assert_eq!(cs.frames.len(), 1);
        assert!(!cs.is_overset());
    }

    #[test]
    fn releasing_an_anchored_text_frame_keeps_its_story_threaded() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note");
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        s.execute("anchored.release", &json!({"story": host_sid.0, "index": 0})).unwrap();
        let id = s.doc().unwrap().selection.items[0];
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(host_sid).unwrap().objects.is_empty());
        assert!(d.anchored_item(note).is_none());
        assert_eq!(d.item(id).unwrap().text_frame().unwrap().story, note_sid);
        assert_eq!(d.story(note_sid).unwrap().frames, [id]);
        d.check().unwrap();
        // Undo puts the frame back in the text; redo releases it under its new id.
        s.execute("edit.undo", &json!({})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(host_sid).unwrap().objects[0].item.id, note);
        assert_eq!(d.story(note_sid).unwrap().frames, [note]);
        d.check().unwrap();
        s.execute("edit.redo", &json!({})).unwrap();
        let id = s.doc().unwrap().selection.items[0];
        let d = s.doc().unwrap().doc.clone();
        assert_eq!(d.story(note_sid).unwrap().frames, [id]);
        d.check().unwrap();
    }

    #[test]
    fn anchoring_a_text_frame_in_its_own_story_is_refused() {
        let (mut s, host, host_sid) = setup("Before after");
        let before = s.doc().unwrap().doc.clone();
        let e = s.execute("anchored.insert", &json!({"ids": [host.0], "story": host_sid.0, "pos": 0})).unwrap_err().to_string();
        assert!(e.contains("cyclic") || e.contains("cannot be anchored"), "{e}");
        assert_eq!(s.doc().unwrap().doc, before, "a refused edit changes nothing");
    }

    #[test]
    fn the_business_plan_case_round_trips_through_pdfx4_and_idml() {
        designcraft_fonts::FontDb::global()
            .add_font(designcraft_fonts::testing::font_with("DC Test Business Korean", &['성', '장', '한', '국', '어']).unwrap());
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false, "width": 400, "height": 400})).unwrap();
        let sections: Vec<String> = (1..=10)
            .map(|i| format!("SECTION {i}\nThis is the flowing business plan text for section {i}. It is long enough to move through the thread and across the page break."))
            .collect();
        let body = sections.join("\n");
        let r = s.execute("frame.create", &json!({"rect": [36, 36, 364, 160], "content": "text", "text": body})).unwrap();
        let host_sid = StoryId(r["story"].as_u64().unwrap());
        let first = ItemId(r["id"].as_u64().unwrap());
        s.execute("text.thread", &json!({"from": first.0, "rect": [36, 36, 364, 364], "spread": 1})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(host_sid).unwrap().frames.len(), 2);
        // A rounded stat card with Korean text and a hyperlink, anchored in the threaded host.
        let r = s.execute("frame.create", &json!({"rect": [0, 0, 150, 70], "content": "text", "text": "성장\n+120%"})).unwrap();
        let card = ItemId(r["id"].as_u64().unwrap());
        let card_sid = StoryId(r["story"].as_u64().unwrap());
        let len = s.doc().unwrap().doc.story(card_sid).unwrap().len();
        let korean = "성장".len();
        s.execute("text.select", &json!({"story": card_sid.0, "anchor": 0, "focus": korean})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "DC Test Business Korean", "language": "Korean", "size": 18.0})).unwrap();
        s.execute("text.select", &json!({"story": card_sid.0, "anchor": 0, "focus": len})).unwrap();
        s.execute("type.char", &json!({"size": 18.0})).unwrap();
        s.execute("type.para", &json!({"attrs": {"align": "center"}})).unwrap();
        s.execute("object.cornerOptions", &json!({"ids": [card.0], "shape": "rounded", "size": 10.0})).unwrap();
        s.execute("hyperlink.create", &json!({"url": "https://example.com/plan"})).unwrap();
        let at = s.doc().unwrap().doc.story(host_sid).unwrap().text.find("SECTION 6").unwrap();
        anchor(&mut s, card, host_sid, at as u64, json!({"position": "aboveLine", "align": "right", "spaceAfter": 6.0}));
        let d = s.doc().unwrap().doc.clone();
        d.check().unwrap();
        let placed = designcraft_compose::frame_placement(&d, card_sid, card).unwrap();
        assert_eq!(placed.spread, designcraft_doc::SpreadRef::Doc(1));
        let cs = s.cache.get(&d, card_sid, None);
        assert!(!cs.is_overset());
        assert!(cs.frames[0].lines.iter().flat_map(|l| &l.glyphs).any(|g| g.rendered_char == '성' && g.gid != 0));
        let report = s.execute("preflight.run", &json!({})).unwrap();
        assert!(!report["issues"].as_array().unwrap().iter().any(|i| i["kind"] == "missingGlyphs"), "{report}");
        // PDF/X-4 keeps its real text and hyperlink.
        let r = s.execute("file.exportPdf", &json!({"standard": "x4"})).unwrap();
        let pdf = crate::cmd::base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(designcraft_pdf::check_pdfx4(&pdf), Vec::<String>::new());
        let text = crate::tests::pdf_text(&pdf).join("\n");
        assert!(text.contains('성') && text.contains("+120%"), "{text}");
        let raw = String::from_utf8_lossy(&pdf);
        assert!(raw.contains("/Subtype/Link") && raw.contains("https://example.com/plan"));
        // IDML reopens the threaded host and the anchored frame with its text and corners.
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
        back.check().unwrap();
        let host = back.stories.values().find(|st| !st.objects.is_empty()).unwrap();
        assert_eq!(host.frames.len(), 2, "the host stays threaded");
        let anchored = &host.objects[0].item;
        let inner = anchored.text_frame().unwrap();
        assert_eq!(back.story(inner.story).unwrap().text, "성장\n+120%");
        assert_eq!(anchored.corners, d.anchored_item(card).unwrap().corners);
    }

    #[test]
    fn an_anchored_text_frame_exports_to_pdf_and_idml() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, _note_sid) = add_note(&mut s, "Note");
        s.execute("object.cornerOptions", &json!({"ids": [note.0], "shape": "rounded", "size": 9.0})).unwrap();
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let d = s.doc().unwrap().doc.clone();
        let corners = d.anchored_item(note).unwrap().corners;
        let pdf = designcraft_pdf::export_pdf(&d, &s.cache, &Default::default()).unwrap();
        let text = crate::tests::pdf_text(&pdf).join("\n");
        assert!(text.contains("Before") && text.contains("after"), "{text}");
        assert!(text.contains("Note"), "{text}");
        let back = designcraft_idml::import_idml(&designcraft_idml::export_idml(&d)).unwrap();
        back.check().unwrap();
        let host = back.stories.values().find(|st| !st.objects.is_empty()).unwrap();
        assert_eq!(host.objects.len(), 1);
        let anchored = &host.objects[0].item;
        let inner = anchored.text_frame().unwrap();
        assert_eq!(back.story(inner.story).unwrap().text, "Note");
        assert!(back.story(inner.story).unwrap().frames.contains(&anchored.id));
        assert_eq!(anchored.corners, corners, "rounded corners survive IDML");
    }

    #[test]
    fn pdf_keeps_an_anchored_frames_text_and_hyperlink_and_passes_pdfx4() {
        let (mut s, _host, host_sid) = setup("Before after and more words.");
        let (note, note_sid) = add_note(&mut s, "Note link");
        let len = s.doc().unwrap().doc.story(note_sid).unwrap().len();
        s.execute("text.select", &json!({"story": note_sid.0, "anchor": 0, "focus": len})).unwrap();
        s.execute("hyperlink.create", &json!({"url": "https://example.com/anchored"})).unwrap();
        anchor(&mut s, note, host_sid, 7, json!({"position": "inline"}));
        let r = s.execute("file.exportPdf", &json!({"standard": "x4"})).unwrap();
        let bytes = crate::cmd::base64_decode(r["base64"].as_str().unwrap());
        let text = crate::tests::pdf_text(&bytes).join("\n");
        assert!(text.contains("Note link"), "{text}");
        let raw = String::from_utf8_lossy(&bytes);
        assert!(raw.contains("/Subtype/Link"), "the hyperlink annotation is written");
        assert!(raw.contains("https://example.com/anchored"), "the hyperlink target is written");
        assert_eq!(designcraft_pdf::check_pdfx4(&bytes), Vec::<String>::new());
    }
}
