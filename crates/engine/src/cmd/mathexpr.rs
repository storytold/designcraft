//! Math expressions: LaTeX typeset by [`crate::math`] and placed as a graphic, inline at the
//! text insertion point (an anchored object) or as a frame. The LaTeX is kept as the object's
//! alt text, so it can be edited and exports accessibly.

use std::sync::Arc;

use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape};
use designcraft_geom::{Affine, Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

fn math_item(d: &mut designcraft_doc::Document, latex: &str, size: f64, lid: designcraft_doc::LayerId, at: (f64, f64)) -> Item {
    let (svg, (w, h)) = crate::math::to_svg(latex, size);
    let aid = AssetId(d.alloc());
    d.assets.insert(
        aid,
        Arc::new(Asset { page: 0, id: aid, name: "math.svg".into(), mime: "image/svg+xml".into(), link: None, data: Arc::new(svg), pixels: None }),
    );
    let id = ItemId(d.alloc());
    let r = Rect::new(at.0, at.1, at.0 + w, at.1 + h);
    let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(r));
    it.content = Content::Graphic(Graphic {
        asset: aid,
        size: (w, h),
        xf: Affine::translate(at),
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
    });
    it.alt_text = latex.to_string();
    it.name = "Math".into();
    it
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "math.insert",
            "Insert Math Expression…",
            ["Type"],
            None,
            "{latex, size?: points (12), x?, y?, spread?} — at the text insertion point (inline), else as a frame; or {id, latex} to edit one",
            has_doc,
            insert
        ),
        cmd!(query "math.svg", "Math Expression SVG", [], None, "{latex, size?} → {svg, width, height} — typeset without placing", has_doc, |_s, p| {
            let latex = str_param(p, "latex").ok_or_else(|| bad("math.svg", "`latex` required"))?;
            let (svg, (w, h)) = crate::math::to_svg(latex, p.get("size").and_then(Value::as_f64).unwrap_or(12.0));
            Ok(json!({"svg": String::from_utf8_lossy(&svg), "width": w, "height": h}))
        }),
    ]
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let latex = str_param(p, "latex").ok_or_else(|| bad("math.insert", "`latex` required"))?.to_string();
    let size = p.get("size").and_then(Value::as_f64).unwrap_or(12.0).clamp(2.0, 400.0);
    let lid = s.doc()?.active_layer;
    // Edit an existing expression in place.
    if let Some(id) = super::id_param(p, "id") {
        return s.edit(|d, _| {
            let old = d.item(id).ok_or_else(|| bad("math.insert", "no such object"))?;
            let at = old.bounds();
            let it = math_item(d, &latex, size, lid, (at.x0, at.y0));
            let target = d.item_mut(id).ok_or_else(|| bad("math.insert", "no such object"))?;
            target.path = it.path;
            target.content = it.content;
            target.alt_text = latex.clone();
            Ok(json!({"id": id.0}))
        });
    }
    let text = s.doc()?.selection.text.filter(|t| t.cell.is_none());
    let spread = super::spread_param(p, "spread");
    let (px, py) = (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64));
    s.edit(|d, sel| {
        if let Some(t) = text {
            let it = math_item(d, &latex, size, lid, (0.0, 0.0));
            let r = t.range();
            if let Some(st) = d.story_mut(t.story) {
                st.delete(r.start.min(st.len())..r.end.min(st.len()));
            }
            let end = super::anchored::anchor_items(d, t.story, r.start, vec![it], &Default::default())?;
            sel.text = Some(designcraft_doc::TextSel { anchor: end, focus: end, ..t });
            return Ok(json!({"inline": true, "pos": end}));
        }
        let pr = d.spread(spread).and_then(|sp| sp.pages.first()).map(|pg| pg.margin_rect()).unwrap_or(Rect::new(36.0, 36.0, 300.0, 300.0));
        let it = math_item(d, &latex, size, lid, (px.unwrap_or(pr.x0), py.unwrap_or(pr.y0)));
        let id = it.id;
        d.insert_item(spread, it, None)?;
        sel.items = vec![id];
        Ok(json!({"id": id.0}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn math_places_inline_and_as_frames() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("math.insert", &json!({"latex": r"\frac{a}{b} + \sqrt{x}", "x": 100, "y": 100})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let d = s.doc().unwrap().doc.clone();
        let it = d.item(id).unwrap();
        assert_eq!(it.alt_text, r"\frac{a}{b} + \sqrt{x}");
        assert!(it.bounds().height() > 20.0, "a fraction is tall");
        // It draws: dark pixels inside the frame.
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 2.0, false, &Default::default()).unwrap();
        let b = it.bounds();
        let dark = (b.x0 as u32 * 2..b.x1 as u32 * 2)
            .flat_map(|x| (b.y0 as u32 * 2..b.y1 as u32 * 2).map(move |y| (x, y)))
            .filter(|&(x, y)| img.pixel(x, y)[0] < 100)
            .count();
        assert!(dark > 30, "{dark}");
        // Edit in place.
        s.execute("math.insert", &json!({"id": id.0, "latex": "x"})).unwrap();
        assert!(s.doc().unwrap().doc.item(id).unwrap().bounds().height() < b.height());
        // Inline at the insertion point.
        let f = s.execute("frame.create", &json!({"rect": [72, 300, 400, 400], "content": "text", "text": "Area: "})).unwrap();
        s.execute("text.select", &json!({"story": f["story"], "anchor": 6, "focus": 6})).unwrap();
        assert_eq!(s.execute("math.insert", &json!({"latex": r"\pi r^2"})).unwrap()["inline"], true);
        let st = &s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(f["story"].as_u64().unwrap())];
        assert_eq!(st.objects.len(), 1);
    }
}
