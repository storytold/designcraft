//! Type on a Path: a story set along any path (lines, curves, shapes), with Type on a Path
//! Options (start, flip, alignment) and Delete Type from Path.

use designcraft_doc::{Content, ItemId, PathAlign, PathType, Selection, Story, StoryId, TextFrame, TextSel};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "type.onPath",
            "Type on a Path",
            [],
            None,
            "{id: path, text?, start?: distance along the path, flip?, align?: baseline|ascender|descender|center} → {story} — the text cursor goes into it",
            has_doc,
            on_path
        ),
        cmd!(
            "type.pathOptions",
            "Type on a Path Options…",
            ["Type", "Type on a Path"],
            None,
            "{id?, start?, flip?, align?: baseline|ascender|descender|center, delete?: true (Delete Type from Path)}",
            has_doc,
            path_options
        ),
    ]
}

fn align_param(p: &Value) -> Result<Option<PathAlign>> {
    p.get("align").map(|v| serde_json::from_value(v.clone()).map_err(|e| bad("type.onPath", format!("align: {e}")))).transpose()
}

fn on_path(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::id_param(p, "id").ok_or_else(|| bad("type.onPath", "missing id"))?;
    let text = super::str_param(p, "text").unwrap_or("").to_string();
    let mut pt = PathType {
        start: p.get("start").and_then(Value::as_f64).unwrap_or(0.0).max(0.0),
        flip: p.get("flip").and_then(Value::as_bool).unwrap_or(false),
        ..Default::default()
    };
    if let Some(a) = align_param(p)? {
        pt.align = a;
    }
    s.edit(|d, sel| {
        let it = d.item(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        match &it.content {
            Content::Unassigned => {}
            Content::Text(_) => return Err(bad("type.onPath", "the object already holds text")),
            _ => return Err(bad("type.onPath", "type goes on empty paths, lines and shapes")),
        }
        let sid = StoryId(d.alloc());
        let mut st = Story::with_text(sid, &text, Default::default());
        st.direction = d.new_story_direction();
        st.frames = vec![id];
        d.stories.insert(sid, std::sync::Arc::new(st));
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let mut tf = TextFrame { story: sid, options: Default::default() };
        tf.options.path = Some(pt);
        it.content = Content::Text(tf);
        *sel = Selection::default();
        let end = text.len();
        sel.text = Some(TextSel { story: sid, anchor: end, focus: end, cell: None, frame: Some(id) });
        Ok(json!({"story": sid.0}))
    })
}

fn path_options(s: &mut Session, p: &Value) -> Result<Value> {
    let id: ItemId = match super::id_param(p, "id") {
        Some(i) => i,
        None => {
            let st = s.doc()?;
            st.selection
                .items
                .first()
                .copied()
                .or_else(|| st.selection.text.and_then(|t| st.doc.story(t.story)?.frames.first().copied()))
                .ok_or_else(|| bad("type.pathOptions", "select type on a path"))?
        }
    };
    let align = align_param(p)?;
    let delete = p.get("delete").and_then(Value::as_bool).unwrap_or(false);
    let p = p.clone();
    s.edit(|d, sel| {
        let it = d.item_mut(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
        let Content::Text(tf) = &mut it.content else { return Err(bad("type.pathOptions", "not type on a path")) };
        let Some(pt) = tf.options.path.as_mut() else { return Err(bad("type.pathOptions", "not type on a path")) };
        if delete {
            let sid = tf.story;
            it.content = Content::Unassigned;
            d.stories.remove(&sid);
            sel.text = None;
            return Ok(json!({"deleted": true}));
        }
        if let Some(v) = p.get("start").and_then(Value::as_f64) {
            pt.start = v.max(0.0);
        }
        if let Some(v) = p.get("flip").and_then(Value::as_bool) {
            pt.flip = v;
        }
        if let Some(a) = align {
            pt.align = a;
        }
        Ok(json!({"start": pt.start, "flip": pt.flip, "align": pt.align}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn type_on_a_path_renders_along_the_path() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // A vertical line: the text must run down it, not across.
        let id = s.execute("line.create", &json!({"a": [300, 100], "b": [300, 600]})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("type.onPath", &json!({"id": id, "text": "Along the line we go"})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let d = s.doc().unwrap().doc.clone();
        let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
        assert!(cs.overset_at.is_none());
        let it = d.item(designcraft_doc::ItemId(id)).unwrap();
        let designcraft_doc::Content::Text(tf) = &it.content else { panic!() };
        let glyphs = designcraft_compose::path_glyphs(&cs, &cs.frames[0], &it.path.to_bezpath(), tf.options.path.as_ref().unwrap());
        let b = glyphs.iter().map(|(_, bp)| designcraft_geom::kurbo::Shape::bounding_box(bp)).reduce(|a, b| a.union(b)).unwrap();
        assert!(b.height() > b.width() * 3.0, "runs down the line: {b:?}");
        assert!(b.x0 > 280.0 && b.x1 < 320.0);
        // Typing at the cursor adds to it; options; delete.
        s.execute("text.insert", &json!({"text": "!"})).unwrap();
        assert!(s.doc().unwrap().doc.story(sid).unwrap().text.ends_with('!'));
        let r = s.execute("type.pathOptions", &json!({"id": id, "start": 20, "flip": true, "align": "center"})).unwrap();
        assert_eq!(r["align"], "center");
        // Too much text oversets (one line only).
        s.execute("text.insert", &json!({"text": " more words".repeat(60)})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(designcraft_compose::compose_story(&d, sid, &Default::default()).overset_at.is_some());
        s.execute("type.pathOptions", &json!({"id": id, "delete": true})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        assert!(d.story(sid).is_none());
        assert!(matches!(d.item(designcraft_doc::ItemId(id)).unwrap().content, designcraft_doc::Content::Unassigned));
        // Rendered output has ink along the line.
        let mut s2 = Session::new();
        s2.execute("file.new", &json!({})).unwrap();
        let id = s2.execute("line.create", &json!({"a": [100, 300], "b": [500, 300]})).unwrap()["id"].as_u64().unwrap();
        s2.execute("object.stroke", &json!({"ids": [id], "swatch": "[None]"})).unwrap();
        s2.execute("type.onPath", &json!({"id": id, "text": "MMMMMMMM"})).unwrap();
        let d = s2.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s2.cache, 0, 1.0, false, &Default::default()).unwrap();
        let ink = (100..200).filter(|&x| img.pixel(x, 295)[0] < 128).count();
        assert!(ink > 8, "glyphs sit on the line: {ink}");
        assert_eq!((100..200).filter(|&x| img.pixel(x, 306)[0] < 128).count(), 0, "nothing hangs below the baseline");
        let pdf = designcraft_pdf::export_pdf(&d, &s2.cache, &Default::default()).unwrap();
        assert!(pdf.len() > 1000);
    }
}
