//! Object › Anchored Object: items that flow with the text (inline or above the line).

use std::sync::Arc;

use designcraft_doc::{
    AnchorPosition, AnchoredObject, Content, Document, Item, ItemId, OBJECT_MARK, SpreadRef, StoryId, TextSel, anchored::AnchorAlign,
};
use designcraft_geom::Affine;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

/// Describe the commands for inserting, editing, and releasing anchored objects.
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
            [],
            None,
            "{image: unsigned item id, caption?: unsigned item id} — caption must be an anchored text frame in the same story, used by at most one image; omit caption to unlink; invalid values are errors",
            has_doc,
            link_caption
        ),
        cmd!(
            "anchored.resize",
            "Resize Anchored Graphic",
            [],
            None,
            "{id, left?, top?, right?, bottom?} crop edge deltas in frame points (clamped to graphic bounds and 1pt minimum frame), OR {id, width, height} explicit frame size (1..100000pt); modes cannot be mixed; frame changes preserve graphic content size; recomposes text",
            has_doc,
            resize_graphic
        ),
        cmd!(
            "anchored.contentTransform",
            "Move or Scale Anchored Image Content",
            [],
            None,
            "{id, dx?, dy?, scale?} — require at least one; dx/dy in frame points (±1000000), scale 0.01..100 around natural graphic centre; can combine move and proportional scale without changing frame",
            has_doc,
            transform_content
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

/// Reject transforms that cannot be serialized and safely inverted by later edits.
fn valid_graphic_transform(xf: Affine) -> bool {
    xf.as_coeffs().iter().all(|v| v.is_finite() && v.abs() <= 1.0e12) && xf.determinant().is_finite() && xf.determinant().abs() >= 1.0e-12
}

/// Associate only explicitly identified objects; adjacent paragraphs are never guessed as captions.
fn link_caption(s: &mut Session, p: &Value) -> Result<Value> {
    let image = ItemId(p.get("image").and_then(Value::as_u64).ok_or_else(|| bad("anchored.linkCaption", "missing image"))?);
    let caption = match p.get("caption") {
        None => None,
        Some(v) => Some(ItemId(v.as_u64().ok_or_else(|| bad("anchored.linkCaption", "caption must be an unsigned integer; omit to unlink"))?)),
    };
    if caption == Some(image) {
        return Err(bad("anchored.linkCaption", "caption cannot be image"));
    }
    s.edit(|d, _| {
        let (sid, idx) = d
            .stories
            .values()
            .find_map(|st| st.objects.iter().position(|o| o.item.id == image).map(|i| (st.id, i)))
            .ok_or_else(|| bad("anchored.linkCaption", "image not found"))?;
        let story = d.story_mut(sid).ok_or_else(|| bad("anchored.linkCaption", "story not found"))?;
        if !matches!(story.objects[idx].item.content, Content::Graphic(_)) {
            return Err(bad("anchored.linkCaption", "source is not a graphic"));
        }
        if let Some(cid) = caption {
            let other = story
                .objects
                .iter()
                .find(|o| o.item.id == cid)
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
        let v = match p.get(key) {
            None => 0.0,
            Some(value) => value.as_f64().ok_or_else(|| bad("anchored.resize", format!("{key} must be numeric")))?,
        };
        if !v.is_finite() || v.abs() > 1_000_000.0 {
            return Err(bad("anchored.resize", format!("{key} out of range")));
        }
        Ok(v)
    };
    // Retain the legacy explicit-size command for scripts and existing documents.
    // Interactive handle drags use edge deltas instead, and are crop-limited.
    let explicit_size = match (p.get("width"), p.get("height")) {
        (Some(w), Some(h)) => {
            let w = w.as_f64().ok_or_else(|| bad("anchored.resize", "width must be numeric"))?;
            let h = h.as_f64().ok_or_else(|| bad("anchored.resize", "height must be numeric"))?;
            if !w.is_finite() || !h.is_finite() || !(1.0..=100_000.0).contains(&w) || !(1.0..=100_000.0).contains(&h) {
                return Err(bad("anchored.resize", "explicit dimensions must be between 1 and 100000 points"));
            }
            Some((w, h))
        }
        (None, None) => None,
        _ => return Err(bad("anchored.resize", "width and height must be supplied together")),
    };
    if explicit_size.is_some() && ["left", "top", "right", "bottom"].iter().any(|key| p.get(*key).is_some()) {
        return Err(bad("anchored.resize", "explicit dimensions cannot be combined with crop edges"));
    }
    let (left, top, right, bottom) = (read("left")?, read("top")?, read("right")?, read("bottom")?);
    s.edit(|d, _| {
        let (sid, index) = d
            .stories
            .values()
            .find_map(|story| story.objects.iter().position(|o| o.item.id == id).map(|i| (story.id, i)))
            .ok_or_else(|| bad("anchored.resize", "anchored item not found"))?;
        let story = d.story_mut(sid).ok_or_else(|| bad("anchored.resize", "story not found"))?;
        let obj = story.objects.get_mut(index).ok_or_else(|| bad("anchored.resize", "object not found"))?;
        let mut resized = obj.item.clone();
        let Content::Graphic(ref mut graphic) = resized.content else {
            return Err(bad("anchored.resize", "object is not a graphic"));
        };
        let (old_w, old_h) = obj.size();
        if old_w < 1.0 || old_h < 1.0 {
            return Err(bad("anchored.resize", "empty frame"));
        }
        if let Some((width, height)) = explicit_size {
            let sx = width / old_w;
            let sy = height / old_h;
            graphic.xf = Affine::scale_non_uniform(1.0 / sx, 1.0 / sy) * graphic.xf;
            resized.xf = Affine::scale_non_uniform(sx, sy) * resized.xf;
            if !valid_graphic_transform(graphic.xf) || !valid_graphic_transform(resized.xf) {
                return Err(bad("anchored.resize", "invalid resulting graphic transform"));
            }
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
        let image_bounds = (frame_xf * graphic.xf).transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, graphic.size.0, graphic.size.1));
        let l = left.clamp(image_bounds.x0.min(0.0), old_w - 1.0);
        let t = top.clamp(image_bounds.y0.min(0.0), old_h - 1.0);
        let r = right.clamp(old_w - image_bounds.x1.max(old_w), old_w - l - 1.0);
        let b = bottom.clamp(old_h - image_bounds.y1.max(old_h), old_h - t - 1.0);
        let width = old_w - l - r;
        let height = old_h - t - b;
        let sx = width / old_w;
        let sy = height / old_h;
        let inner_offset = frame_xf.inverse() * designcraft_geom::Point::new(-l, -t) - frame_xf.inverse() * designcraft_geom::Point::ORIGIN;
        graphic.xf = Affine::scale_non_uniform(1.0 / sx, 1.0 / sy) * Affine::translate(inner_offset) * graphic.xf;
        resized.xf = Affine::scale_non_uniform(sx, sy) * resized.xf;
        if !valid_graphic_transform(graphic.xf) || !valid_graphic_transform(resized.xf) {
            return Err(bad("anchored.resize", "invalid resulting graphic transform"));
        }
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
        let v = match p.get(key) {
            None => default,
            Some(value) => value.as_f64().ok_or_else(|| bad("anchored.contentTransform", format!("{key} must be numeric")))?,
        };
        if !v.is_finite() {
            return Err(bad("anchored.contentTransform", "non-finite transform"));
        }
        Ok(v)
    };
    if !["dx", "dy", "scale"].iter().any(|key| p.get(*key).is_some()) {
        return Err(bad("anchored.contentTransform", "provide dx, dy, or scale"));
    }
    let (dx, dy, scale) = (read("dx", 0.0)?, read("dy", 0.0)?, read("scale", 1.0)?);
    if dx.abs() > 1_000_000.0 || dy.abs() > 1_000_000.0 {
        return Err(bad("anchored.contentTransform", "movement exceeds 1000000 points"));
    }
    if !(0.01..=100.0).contains(&scale) {
        return Err(bad("anchored.contentTransform", "invalid scale"));
    }
    s.edit(|d, _| {
        let (sid, index) = d
            .stories
            .values()
            .find_map(|story| story.objects.iter().position(|o| o.item.id == id).map(|i| (story.id, i)))
            .ok_or_else(|| bad("anchored.contentTransform", "anchored item not found"))?;
        let story = d.story_mut(sid).ok_or_else(|| bad("anchored.contentTransform", "story not found"))?;
        let obj = story.objects.get_mut(index).ok_or_else(|| bad("anchored.contentTransform", "object not found"))?;
        let mut edited = obj.item.clone();
        let Content::Graphic(ref mut graphic) = edited.content else {
            return Err(bad("anchored.contentTransform", "not a graphic"));
        };
        // Scaling around the graphic's natural centre preserves its proportions.
        let cx = graphic.size.0 / 2.0;
        let cy = graphic.size.1 / 2.0;
        if edited.xf.determinant().abs() < 1e-12 {
            return Err(bad("anchored.contentTransform", "singular frame transform"));
        }
        let inner_delta = edited.xf.inverse() * designcraft_geom::Point::new(dx, dy) - edited.xf.inverse() * designcraft_geom::Point::ORIGIN;
        graphic.xf = Affine::translate(inner_delta) * graphic.xf * Affine::translate((cx, cy)) * Affine::scale(scale) * Affine::translate((-cx, -cy));
        if !valid_graphic_transform(graphic.xf) {
            return Err(bad("anchored.contentTransform", "invalid resulting graphic transform"));
        }
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

/// Move an anchored object back to its spread while preserving its visual placement.
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

    /// Exercise sequential frame cropping and content movement through real Session commands.
    #[test]
    fn actual_resize_and_content_transform_preserve_frame_and_caption() {
        use designcraft_doc::{AssetId, Graphic, Shape};
        use designcraft_geom::{Point, Rect, shapes};
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let host = s.execute("frame.create", &json!({"rect": [72, 72, 400, 500], "content": "text", "text": "Body"})).unwrap();
        let sid = StoryId(host["story"].as_u64().unwrap());
        let id = ItemId(s.doc().unwrap().doc.next_id);
        let layer = s.doc().unwrap().doc.default_layer();
        let mut item = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 60.0)));
        item.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: AssetId(1),
            size: (100.0, 60.0),
            xf: Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        s.edit(|d, _| {
            d.alloc();
            d.story_mut(sid).unwrap().insert_object(0, AnchoredObject::new(item, Default::default()));
            Ok(json!({}))
        })
        .unwrap();
        let displayed = |s: &Session| {
            let obj = &s.doc().unwrap().doc.story(sid).unwrap().objects[0];
            let Content::Graphic(ref g) = obj.item.content else { panic!("not a graphic") };
            (obj.size(), obj.item.xf * g.xf * Point::new(60.0, 35.0))
        };
        assert_eq!(displayed(&s).0, (100.0, 60.0));
        s.execute("anchored.resize", &json!({"id": id.0, "left": 20.0})).unwrap();
        assert!((displayed(&s).0.0 - 80.0).abs() < 1e-9);
        s.execute("anchored.resize", &json!({"id": id.0, "left": 10.0})).unwrap();
        let (size, point) = displayed(&s);
        assert!((size.0 - 70.0).abs() < 1e-9);
        assert!((point.x - 30.0).abs() < 1e-9, "{point:?}");
        s.execute("anchored.contentTransform", &json!({"id": id.0, "dx": 8.0, "dy": -4.0})).unwrap();
        let (after_size, after_point) = displayed(&s);
        assert_eq!(after_size, size);
        assert!((after_point.x - 38.0).abs() < 1e-9);
        assert!((after_point.y - 31.0).abs() < 1e-9);
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().objects[0].caption_id, None);
    }

    /// Reject invalid command inputs before attempting document mutations.
    #[test]
    fn anchored_commands_reject_invalid_parameters_without_mutating_document() {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        for caption in [json!(null), json!("7"), json!(-1), json!(1.5), json!(true)] {
            assert!(session.execute("anchored.linkCaption", &json!({"image": 1, "caption": caption})).is_err());
        }
        for key in ["left", "top", "right", "bottom", "width", "height"] {
            for value in [json!("5"), json!(null), json!(false)] {
                let mut params = json!({"id": 1});
                params[key] = value;
                assert!(session.execute("anchored.resize", &params).is_err(), "{params}");
            }
        }
        for params in [
            json!({"id": 1, "width": 40, "height": 50, "left": 1}),
            json!({"id": 1, "width": 40}),
            json!({"id": 1, "width": 0, "height": 10}),
            json!({"id": 1, "width": 100001, "height": 10}),
        ] {
            assert!(session.execute("anchored.resize", &params).is_err(), "{params}");
        }
        assert!(session.execute("anchored.contentTransform", &json!({"id": 1})).is_err());
        for key in ["dx", "dy", "scale"] {
            for value in [json!(null), json!("5"), json!(true)] {
                let mut params = json!({"id": 1});
                params[key] = value;
                assert!(session.execute("anchored.contentTransform", &params).is_err(), "{params}");
            }
        }
        for params in
            [json!({"id": 1, "dx": 1000001}), json!({"id": 1, "dy": -1000001}), json!({"id": 1, "scale": 0}), json!({"id": 1, "scale": 101})]
        {
            assert!(session.execute("anchored.contentTransform", &params).is_err(), "{params}");
        }
    }

    /// Ensure repeated crops preserve graphic coordinates in frame space.
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
            content = Affine::scale_non_uniform(1.0 / sx, 1.0) * Affine::translate(inner) * content;
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

    /// Ensure caption linking rejects non-graphic sources and never infers body text.
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
            d.anchored_items().iter().filter_map(|(_, i)| i.graphic()).map(|g| d.assets.get(&g.asset).map(|a| a.data.clone())).collect();
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
