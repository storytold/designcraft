//! Object › Anchored Object: items that flow with the text (inline or above the line).

use std::sync::Arc;

use designcraft_doc::{
    AnchorPosition, AnchoredObject, Content, Document, Item, ItemId, OBJECT_MARK, SpreadRef, StoryId, TextSel, anchored::AnchorAlign,
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
            "anchored.linkCaption",
            "Link Anchored Image and Caption",
            [], None,
            "{image, caption?} — explicitly associate two story-owned anchored items; omit caption to unlink",
            has_doc, link_caption
        ),
        cmd!(
            "anchored.resize",
            "Resize Anchored Graphic",
            [],
            None,
            "{id, left?, top?, right?, bottom?} edge deltas in frame points, or {id, width, height} together for explicit size; recomposes text",
            has_doc,
            resize_graphic
        ),
        cmd!(
            "anchored.contentTransform",
            "Move or Scale Anchored Image Content",
            [], None,
            "{id, dx?, dy?, scale?} — move in frame points or scale proportionally without resizing the frame",
            has_doc, transform_content
        ),
        cmd!(
            "anchored.options",
            "Anchored Object Options…",
            ["Object", "Anchored Object"],
            None,
            "{story?, index? (default: the object at the insertion point), position?: inline|aboveLine|custom, yOffset?, align?: left|center|right, spaceBefore?, spaceAfter?, xRelative? | yRelative?: anchor|textFrame|columnEdge|pageMargin|pageEdge, xOffset?, objectPoint? | refPoint?: 0..8, keepWithinColumn?, spineRelative? (custom: the settings describe a left-hand page and mirror on a right-hand page)}",
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

/// Associate only explicitly identified objects; adjacent paragraphs are never guessed as captions.
fn link_caption(s: &mut Session, p: &Value) -> Result<Value> {
    let image = ItemId(p.get("image").and_then(Value::as_u64).ok_or_else(|| bad("anchored.linkCaption", "missing image"))?);
    let caption = p.get("caption").and_then(Value::as_u64).map(ItemId);
    if caption == Some(image) { return Err(bad("anchored.linkCaption", "caption cannot be image")); }
    s.edit(|d, _| {
        let (sid, idx) = d.stories.values().find_map(|st| st.objects.iter().position(|o| o.item.id == image).map(|i| (st.id, i)))
            .ok_or_else(|| bad("anchored.linkCaption", "image not found"))?;
        let story = d.story_mut(sid).ok_or_else(|| bad("anchored.linkCaption", "story not found"))?;
        if !matches!(story.objects[idx].item.content, Content::Graphic(_)) {
            return Err(bad("anchored.linkCaption", "source is not a graphic"));
        }
        if let Some(cid) = caption {
            let other = story.objects.iter().find(|o| o.item.id == cid)
                .ok_or_else(|| bad("anchored.linkCaption", "caption must be anchored in the same story"))?;
            if !matches!(other.item.content, Content::Text(_)) {
                return Err(bad("anchored.linkCaption", "caption must be an anchored text frame"));
            }
            if story.objects.iter().any(|o| o.item.id != image && o.caption_id == Some(cid)) {
                return Err(bad("anchored.linkCaption", "caption already linked to another image"));
            }
        }
        let mut obj = (*story.objects[idx]).clone();
        obj.caption_id = caption;
        story.objects[idx] = Arc::new(obj);
        Ok(json!({"image": image.0, "caption": caption.map(|c| c.0)}))
    })
}

/// Crop or reveal the edges of an anchored image without moving its text anchor.
/// Deltas are relative to the original frame during a preview gesture.
fn resize_graphic(s: &mut Session, p: &Value) -> Result<Value> {
    let id = ItemId(p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("anchored.resize", "missing id"))?);
    let read = |key: &str| -> Result<f64> {
        let v = p.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        if !v.is_finite() { return Err(bad("anchored.resize", "non-finite crop")); }
        Ok(v)
    };
    // Retain the legacy explicit-size command for scripts and existing documents.
    // Interactive handle drags use edge deltas instead, and are crop-limited.
    let explicit_size = match (p.get("width").and_then(Value::as_f64), p.get("height").and_then(Value::as_f64)) {
        (Some(w), Some(h)) if w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0 => Some((w, h)),
        (None, None) => None,
        _ => return Err(bad("anchored.resize", "invalid explicit dimensions")),
    };
    let (left, top, right, bottom) = (read("left")?, read("top")?, read("right")?, read("bottom")?);
    s.edit(|d, _| {
        let (sid, index) = d.stories.values().find_map(|story| {
            story.objects.iter().position(|o| o.item.id == id).map(|i| (story.id, i))
        }).ok_or_else(|| bad("anchored.resize", "anchored item not found"))?;
        let story = d.story_mut(sid).ok_or_else(|| bad("anchored.resize", "story not found"))?;
        let obj = story.objects.get_mut(index).ok_or_else(|| bad("anchored.resize", "object not found"))?;
        let mut resized = obj.item.clone();
        let Content::Graphic(ref mut graphic) = resized.content else {
            return Err(bad("anchored.resize", "object is not a graphic"));
        };
        let (old_w, old_h) = obj.size();
        if old_w < 1.0 || old_h < 1.0 { return Err(bad("anchored.resize", "empty frame")); }
        if let Some((width, height)) = explicit_size {
            let sx = width / old_w;
            let sy = height / old_h;
            graphic.xf = Affine::scale_non_uniform(1.0 / sx, 1.0 / sy) * graphic.xf;
            resized.xf = Affine::scale_non_uniform(sx, sy) * resized.xf;
            let mut normalized = AnchoredObject::new(resized, obj.position.clone());
            normalized.caption_id = obj.caption_id;
            *obj = std::sync::Arc::new(normalized);
            return Ok(json!({"id": id.0, "width": width, "height": height}));
        }
        // The placed graphic's transformed natural bounds define the maximum
        // revealable area. Never create empty frame space beyond those bounds.
        // Graphic bounds are in the item's inner coordinates, while the API
        // receives edge deltas in the frame's parent coordinates. Convert the
        // bounds before clamping and convert the final offset back to inner
        // coordinates. Otherwise a second crop multiplies the offset by the
        // scale introduced by the first crop.
        let frame_xf = resized.xf;
        if frame_xf.determinant().abs() < 1e-12 {
            return Err(bad("anchored.resize", "singular frame transform"));
        }
        let image_bounds = (frame_xf * graphic.xf).transform_rect_bbox(
            designcraft_geom::Rect::new(0.0, 0.0, graphic.size.0, graphic.size.1));
        let l = left.clamp(image_bounds.x0.min(0.0), old_w - 1.0);
        let t = top.clamp(image_bounds.y0.min(0.0), old_h - 1.0);
        let r = right.clamp(old_w - image_bounds.x1.max(old_w), old_w - l - 1.0);
        let b = bottom.clamp(old_h - image_bounds.y1.max(old_h), old_h - t - 1.0);
        let width = old_w - l - r;
        let height = old_h - t - b;
        let sx = width / old_w;
        let sy = height / old_h;
        let inner_offset = frame_xf.inverse() * designcraft_geom::Point::new(-l, -t) - frame_xf.inverse() * designcraft_geom::Point::ORIGIN;
        graphic.xf = Affine::scale_non_uniform(1.0 / sx, 1.0 / sy)
            * Affine::translate(inner_offset) * graphic.xf;
        resized.xf = Affine::scale_non_uniform(sx, sy) * resized.xf;
        let mut normalized = AnchoredObject::new(resized, obj.position.clone());
        normalized.caption_id = obj.caption_id;
        *obj = std::sync::Arc::new(normalized);
        Ok(json!({"id": id.0, "width": width, "height": height}))
    })
}

/// Edit only the graphic-to-frame transform; the frame geometry, anchor and story stay intact.
fn transform_content(s: &mut Session, p: &Value) -> Result<Value> {
    let id = ItemId(p.get("id").and_then(Value::as_u64).ok_or_else(|| bad("anchored.contentTransform", "missing id"))?);
    let read = |key: &str, default: f64| -> Result<f64> {
        let v = p.get(key).and_then(Value::as_f64).unwrap_or(default);
        if !v.is_finite() { return Err(bad("anchored.contentTransform", "non-finite transform")); }
        Ok(v)
    };
    let (dx, dy, scale) = (read("dx", 0.0)?, read("dy", 0.0)?, read("scale", 1.0)?);
    if !(0.01..=100.0).contains(&scale) { return Err(bad("anchored.contentTransform", "invalid scale")); }
    s.edit(|d, _| {
        let (sid, index) = d.stories.values().find_map(|story| story.objects.iter().position(|o| o.item.id == id).map(|i| (story.id, i)))
            .ok_or_else(|| bad("anchored.contentTransform", "anchored item not found"))?;
        let story = d.story_mut(sid).ok_or_else(|| bad("anchored.contentTransform", "story not found"))?;
        let obj = story.objects.get_mut(index).ok_or_else(|| bad("anchored.contentTransform", "object not found"))?;
        let mut edited = obj.item.clone();
        let Content::Graphic(ref mut graphic) = edited.content else { return Err(bad("anchored.contentTransform", "not a graphic")); };
        // Scaling around the graphic's natural centre preserves its proportions.
        let cx = graphic.size.0 / 2.0;
        let cy = graphic.size.1 / 2.0;
        if edited.xf.determinant().abs() < 1e-12 {
            return Err(bad("anchored.contentTransform", "singular frame transform"));
        }
        let inner_delta = edited.xf.inverse() * designcraft_geom::Point::new(dx, dy) - edited.xf.inverse() * designcraft_geom::Point::ORIGIN;
        graphic.xf = Affine::translate(inner_delta) * graphic.xf
            * Affine::translate((cx, cy)) * Affine::scale(scale) * Affine::translate((-cx, -cy));
        *obj = Arc::new(AnchoredObject { item: edited, position: obj.position.clone(), caption_id: obj.caption_id });
        Ok(json!({"id": id.0}))
    })
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

/// Text frames can't be anchored yet (their stories need frames on a spread).
pub(crate) fn check_item(it: &Item) -> Result<()> {
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
        return Ok((StoryId(sid), k as usize));
    }
    let st = s.doc()?;
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

    #[test]
    fn consecutive_crops_keep_graphic_coordinates_in_frame_space() {
        use designcraft_geom::Point;
        // A first crop scales the item frame; the second must convert its
        // edge delta through that scaled frame, not treat it as inner points.
        let mut frame = Affine::IDENTITY;
        let mut content = Affine::IDENTITY;
        let natural_point = Point::new(60.0, 35.0);
        let initial = frame * content * natural_point;
        let mut old_width = 100.0;
        for left in [20.0, 10.0] {
            let sx = (old_width - left) / old_width;
            let inv = frame.inverse();
            let inner = inv * Point::new(-left, 0.0) - inv * Point::ORIGIN;
            content = Affine::scale_non_uniform(1.0 / sx, 1.0)
                * Affine::translate(inner) * content;
            frame = Affine::scale_non_uniform(sx, 1.0) * frame;
            let displayed = frame * content * natural_point;
            assert!((displayed.x - (initial.x - (100.0 - old_width) - left)).abs() < 1e-9);
            assert!((displayed.y - initial.y).abs() < 1e-9);
            old_width -= left;
        }
        // A content move is also specified in frame points after both crops.
        let inv = frame.inverse();
        let delta = inv * Point::new(8.0, -4.0) - inv * Point::ORIGIN;
        content = Affine::translate(delta) * content;
        let moved = frame * content * natural_point;
        assert!((moved.x - (initial.x - 30.0 + 8.0)).abs() < 1e-9);
        assert!((moved.y - (initial.y - 4.0)).abs() < 1e-9);
    }

    #[test]
    fn linking_caption_never_guesses_body_text_or_accepts_non_graphics() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let host = s.execute("frame.create", &json!({"rect": [72, 72, 400, 500], "content": "text", "text": "Caption\nBody text."})).unwrap();
        let sid = host["story"].as_u64().unwrap();
        let shape = s.execute("frame.create", &json!({"rect": [420, 72, 510, 150]})).unwrap()["id"].as_u64().unwrap();
        s.execute("anchored.insert", &json!({"ids": [shape], "story": sid, "pos": 0})).unwrap();
        assert!(s.execute("anchored.linkCaption", &json!({"image": shape, "caption": shape})).is_err());
        assert!(s.execute("anchored.linkCaption", &json!({"image": shape, "caption": 999999})).is_err());
        let story = s.doc().unwrap().doc.story(StoryId(sid)).unwrap();
        assert!(story.text.contains("Caption\nBody text."));
        assert_eq!(story.objects.len(), 1);
        assert_eq!(story.objects[0].caption_id, None);
        s.doc().unwrap().doc.check().unwrap();
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
