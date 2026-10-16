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
        // A text frame keeps its place in its story's thread under its new id.
        if let Some(st) = it.text_frame().and_then(|t| d.story_mut(t.story)) {
            match st.frames.iter_mut().find(|f| **f == obj.item.id) {
                Some(f) => *f = it.id,
                None => st.frames.push(it.id),
            }
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
}
