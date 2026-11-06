//! Object › Anchored Object: items that flow with the text (inline or above the line).

use std::sync::Arc;

use designcraft_doc::{AnchorPosition, AnchoredObject, Document, Item, ItemId, OBJECT_MARK, SpreadRef, StoryId, TextSel, anchored::AnchorAlign};
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

/// Whether `it`, a group or an anchored object inside it holds a text frame.
fn contains_text_frame(it: &Item) -> bool {
    let mut found = false;
    it.walk(&mut |i| found |= i.text_frame().is_some());
    found
}

/// Anchored text frames need their stories and threads to stay valid (no self-anchoring or
/// cycles). Checked after the edit, which is discarded when this fails.
fn check_anchored(d: &Document, cmd: &str) -> Result<()> {
    d.check().map_err(|e| bad(cmd, format!("these items cannot be anchored here: {e}")))
}

/// Delete `range` from `st`, returning the anchored text frames whose marks it removed. Callers
/// unthread the returned frames once the story borrow ends.
pub(crate) fn delete_range(st: &mut designcraft_doc::Story, range: std::ops::Range<usize>) -> Vec<(StoryId, ItemId)> {
    let doomed = st.anchored_text_frames_in(range.clone());
    st.delete(range);
    doomed
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

/// `anchored.insert`: move items into story text at a position, inline, above the line or custom.
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
    let has_text = items.iter().any(contains_text_frame);
    s.edit(|d, sel| {
        // A text frame keeps its own story threaded to its id; only the frame leaves the spread.
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

/// `anchored.release`: put an anchored item back on its spread and remove its text mark.
fn release(s: &mut Session, p: &Value) -> Result<Value> {
    let (sid, k) = object_ref(s, p, "anchored.release")?;
    let d0 = s.doc()?.doc.clone();
    let st = d0.story(sid).ok_or_else(|| bad("anchored.release", "no such story"))?;
    let obj = st.objects.get(k).cloned().ok_or_else(|| bad("anchored.release", "no such anchored object"))?;
    let pos = st.text.match_indices(OBJECT_MARK).nth(k).map(|(i, _)| i).ok_or_else(|| bad("anchored.release", "no mark"))?;
    // Its own story stays threaded to the frame; releasing moves that thread to the new id.
    let inner = obj.item.text_frame().map(|t| (t.story, obj.item.id));
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
        {
            let st = d.story_mut(sid).ok_or(designcraft_doc::DocError::NoStory(sid))?;
            st.delete(pos..pos + OBJECT_MARK.len_utf8());
        }
        if let Some((inner, old)) = inner {
            let st = d.story_mut(inner).ok_or(designcraft_doc::DocError::NoStory(inner))?;
            match st.frames.iter().position(|f| *f == old) {
                Some(at) => st.frames[at] = id,
                None => st.frames.push(id),
            }
            st.rev = st.rev.wrapping_add(1);
            check_anchored(d, "anchored.release")?;
        }
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
