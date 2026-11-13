//! Object › Captions › Generate Static Caption: a text frame beside each selected object, filled
//! from its metadata.

use designcraft_doc::{Document, Item, ItemId, ParaFormat, Selection};
use designcraft_geom::Rect;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, str_param, targets};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "object.caption",
        "Generate Static Caption",
        ["Object", "Captions"],
        None,
        "{text?: template (default \"{name}\"; variables {name} {path} {altText} {label} {ppi} {dimensions} {format}), position?: below|above|left|right, offset? (pt, 0), height? (pt, 24), style?: paragraph style, live?: bool (kept current as the source changes), ids?} → {ids}",
        has_selection,
        caption
    )]
}

/// The metadata variables of `it`.
fn variable(d: &Document, it: &Item, xf: designcraft_geom::Affine, name: &str) -> String {
    let g = it.graphic();
    let asset = g.and_then(|g| d.assets.get(&g.asset));
    match name {
        "name" => asset.map(|a| a.name.clone()).unwrap_or_default(),
        "path" => asset.and_then(|a| a.link.clone()).unwrap_or_default(),
        "altText" => it.alt_text.clone(),
        "label" => it.label.clone(),
        "format" => asset.map(|a| a.mime.rsplit('/').next().unwrap_or("").to_uppercase()).unwrap_or_default(),
        "dimensions" => asset.and_then(|a| a.pixels).map(|(w, h)| format!("{w} × {h}")).unwrap_or_default(),
        "ppi" => match (g, asset.and_then(|a| a.pixels)) {
            (Some(g), Some((w, _))) if g.size.0 > 0.0 => {
                let m = (xf * g.xf).as_coeffs();
                let scale = m[0].hypot(m[1]);
                let inches = g.size.0 * scale / 72.0;
                if inches > 0.0 { format!("{}", (w as f64 / inches).round()) } else { String::new() }
            }
            _ => String::new(),
        },
        _ => String::new(),
    }
}

pub(crate) fn fill(d: &Document, it: &Item, xf: designcraft_geom::Affine, template: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        match rest[i..].find('}') {
            Some(j) => {
                out.push_str(&variable(d, it, xf, &rest[i + 1..i + j]));
                rest = &rest[i + j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn caption(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.caption";
    let ids = targets(s, p)?;
    let template = str_param(p, "text").unwrap_or("{name}").to_string();
    let pos = str_param(p, "position").unwrap_or("below").to_string();
    if !["below", "above", "left", "right"].contains(&pos.as_str()) {
        return Err(bad(ID, format!("unknown position `{pos}`")));
    }
    let offset = p.get("offset").and_then(Value::as_f64).unwrap_or(0.0);
    let height = p.get("height").and_then(Value::as_f64).unwrap_or(24.0).max(1.0);
    let style = str_param(p, "style").map(str::to_string);
    let live = p.get("live").and_then(Value::as_bool).unwrap_or(false);
    if let Some(st) = &style
        && s.doc()?.doc.styles.para(st).is_none()
    {
        return Err(bad(ID, format!("no paragraph style `{st}`")));
    }
    let lid = s.doc()?.active_layer;
    s.edit(|d, sel| {
        let mut made = Vec::new();
        for id in &ids {
            let Some(loc) = d.find(*id) else { continue };
            let Some(it) = d.item_at(&loc) else { continue };
            let xf = d.parent_xf(&loc) * it.xf;
            let b = xf.transform_rect_bbox(it.inner_bounds());
            let text = fill(d, it, xf, &template);
            let r = match pos.as_str() {
                "above" => Rect::new(b.x0, b.y0 - offset - height, b.x1, b.y0 - offset),
                "left" => Rect::new(b.x0 - offset - 144.0, b.y0, b.x0 - offset, b.y1),
                "right" => Rect::new(b.x1 + offset, b.y0, b.x1 + offset + 144.0, b.y1),
                _ => Rect::new(b.x0, b.y1 + offset, b.x1, b.y1 + offset + height),
            };
            let para = ParaFormat { style: style.clone().unwrap_or_else(|| d.styles.default_paragraph.clone()), ..Default::default() };
            let source = *id;
            let (cid, _) = d.add_text_frame(loc.spread, r, lid, &text, para)?;
            if live && let Some(c) = d.item_mut(cid) {
                c.live_caption = Some(designcraft_doc::LiveCaption { source, template: template.clone() });
            }
            made.push(cid);
        }
        if made.is_empty() {
            return Err(bad(ID, "nothing to caption"));
        }
        *sel = Selection::items(made.clone());
        Ok(json!({"ids": made.iter().map(|i: &ItemId| i.0).collect::<Vec<_>>()}))
    })
}

/// Bring live captions up to date with their source objects (the text frame's story is
/// replaced when its text changed; a caption whose source is gone keeps its last text).
pub(crate) fn refresh_live(d: &mut Document) -> bool {
    let mut todo = Vec::new();
    for id in d.all_items() {
        let Some(c) = d.item(id) else { continue };
        let (Some(lc), Some(tf)) = (&c.live_caption, c.text_frame()) else { continue };
        let (Some(loc), Some(src)) = (d.find(lc.source), d.item(lc.source)) else { continue };
        let text = fill(d, src, d.parent_xf(&loc) * src.xf, &lc.template);
        if d.story(tf.story).is_some_and(|st| st.text != text) {
            todo.push((tf.story, text));
        }
    }
    let changed = !todo.is_empty();
    for (sid, text) in todo {
        if let Some(st) = d.story_mut(sid) {
            let len = st.len();
            st.replace(0..len, &text);
        }
        d.prune_story_text_ranges(sid);
    }
    changed
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn static_caption_from_metadata() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 272, 172]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.altText", &json!({"text": "Harbour at dawn", "ids": [a]})).unwrap();
        s.execute("object.label", &json!({"label": "fig-1", "ids": [a]})).unwrap();
        let r = s.execute("object.caption", &json!({"ids": [a], "text": "Figure {label}: {altText}", "offset": 6})).unwrap();
        let cid = designcraft_doc::ItemId(r["ids"][0].as_u64().unwrap());
        let d = &s.doc().unwrap().doc;
        let c = d.item(cid).unwrap();
        let b = c.bounds();
        assert_eq!((b.x0, b.y0, b.x1), (72.0, 178.0, 272.0));
        let sid = c.text_frame().unwrap().story;
        assert_eq!(d.stories[&sid].text, "Figure fig-1: Harbour at dawn");
        assert!(s.execute("object.caption", &json!({"ids": [a], "position": "sideways"})).is_err());
    }

    #[test]
    fn live_caption_follows_its_source() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 272, 172]})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.altText", &json!({"text": "First", "ids": [a]})).unwrap();
        let r = s.execute("object.caption", &json!({"ids": [a], "text": "Photo: {altText}", "live": true})).unwrap();
        let cid = designcraft_doc::ItemId(r["ids"][0].as_u64().unwrap());
        let text = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            d.stories[&d.item(cid).unwrap().text_frame().unwrap().story].text.clone()
        };
        assert_eq!(text(&s), "Photo: First");
        s.execute("object.altText", &json!({"text": "Second", "ids": [a]})).unwrap();
        assert_eq!(text(&s), "Photo: Second", "updated with the source");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s), "Photo: First", "one undo step for both");
    }
}
