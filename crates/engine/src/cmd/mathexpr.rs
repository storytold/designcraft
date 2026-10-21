//! Math expressions: LaTeX typeset by [`crate::math`] and placed as a graphic, inline at the
//! text insertion point (an anchored object) or as a frame. The LaTeX is kept as the object's
//! alt text, so it can be edited and exports accessibly.

use std::sync::Arc;

use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape};
use designcraft_geom::{Affine, Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

/// A graphic item showing `latex` typeset at `size` with `opts`, its top-left at `at`, with
/// the LaTeX as its alt text.
fn math_item(
    d: &mut designcraft_doc::Document,
    latex: &str,
    size: f64,
    opts: &crate::math::Options,
    lid: designcraft_doc::LayerId,
    at: (f64, f64),
) -> Item {
    let (svg, (w, h)) = crate::math::to_svg_with(latex, size, opts);
    let fresh = AssetId(d.alloc());
    let aid = d.add_asset(Asset { id: fresh, name: "math.svg".into(), mime: "image/svg+xml".into(), data: Arc::new(svg), ..Default::default() });
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
        clip: None,
        fill: Default::default(),
    });
    it.alt_text = latex.to_string();
    it.name = "Math".into();
    it
}

/// The math commands: `math.insert` and `math.svg`.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "math.insert",
            "Insert Math Expression…",
            ["Type"],
            None,
            "{latex, size?: points (12), display?: bool (false; display style: `\\lim` etc. stack their limits), family?: installed font family (else the bundled serif), x?, y?, spread?} or {id, latex, size?, display?, family?} — at the text insertion point (inline), else as a frame; with `id`: edits that equation, which keeps its family and style unless `family` (\"\" for the bundled serif) or `display` is given",
            has_doc,
            insert
        ),
        cmd!(query "math.svg", "Math Expression SVG", [], None, "{latex, size?, display?: bool, family?} → {svg, width, height} — typeset without placing", has_doc, |_s, p| {
            let latex = str_param(p, "latex").ok_or_else(|| bad("math.svg", "`latex` required"))?;
            let (svg, (w, h)) = crate::math::to_svg_with(latex, p.get("size").and_then(Value::as_f64).unwrap_or(12.0), &opts_param(p));
            Ok(json!({"svg": String::from_utf8_lossy(&svg), "width": w, "height": h}))
        }),
    ]
}

/// The typesetting options a math command takes.
fn opts_param(p: &Value) -> crate::math::Options {
    crate::math::Options {
        display: p.get("display").and_then(Value::as_bool).unwrap_or(false),
        family: str_param(p, "family").filter(|f| !f.trim().is_empty()).map(str::to_string),
    }
}

/// `math.insert`: edits the equation `id` in place, or places a new one inline at the text
/// insertion point or as a frame.
fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let mut opts = opts_param(p);
    let keep_family = p.get("family").is_none();
    let keep_display = p.get("display").is_none();
    let latex = str_param(p, "latex").ok_or_else(|| bad("math.insert", "`latex` required"))?.to_string();
    let size = p.get("size").and_then(Value::as_f64).unwrap_or(12.0).clamp(2.0, 400.0);
    let lid = s.doc()?.active_layer;
    // Edit an existing expression in place.
    if let Some(id) = super::id_param(p, "id") {
        return s.edit(|d, _| {
            let old = d.item(id).ok_or_else(|| bad("math.insert", "no such object"))?;
            // The path and graphic live in the item's inner space, which `xf` maps to the spread:
            // build the new ones at the old inner corner and keep `xf`.
            let at = old.path.bounds().ok_or_else(|| bad("math.insert", "equation has no geometry"))?;
            // Without a `family` or `display`, the equation stays in the family and style it was
            // set in.
            let svg = match &old.content {
                Content::Graphic(g) => d.assets.get(&g.asset).map(|a| a.data.clone()),
                _ => None,
            };
            if keep_family {
                opts.family = svg.as_ref().and_then(|b| crate::math::family_of(b));
            }
            if keep_display {
                opts.display = svg.as_ref().is_some_and(|b| crate::math::display_of(b));
            }
            let it = math_item(d, &latex, size, &opts, lid, (at.x0, at.y0));
            let target = d.item_mut(id).ok_or_else(|| bad("math.insert", "no such object"))?;
            target.path = it.path;
            target.content = it.content;
            target.alt_text = latex.clone();
            Ok(json!({"id": id.0}))
        });
    }
    let text = s.doc()?.selection.text.filter(|t| t.cell.is_none());
    let spread = super::spread_param(p, "math.insert")?;
    let (px, py) = (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64));
    s.edit(|d, sel| {
        if let Some(t) = text {
            let it = math_item(d, &latex, size, &opts, lid, (0.0, 0.0));
            let r = t.range();
            if let Some(st) = d.story_mut(t.story) {
                st.delete(r.start.min(st.len())..r.end.min(st.len()));
            }
            let end = super::anchored::anchor_items(d, t.story, r.start, vec![it], &Default::default())?;
            sel.text = Some(designcraft_doc::TextSel { anchor: end, focus: end, ..t });
            return Ok(json!({"inline": true, "pos": end}));
        }
        let pr = d.spread(spread).and_then(|sp| sp.pages.first()).map(|pg| pg.margin_rect()).unwrap_or(Rect::new(36.0, 36.0, 300.0, 300.0));
        let it = math_item(d, &latex, size, &opts, lid, (px.unwrap_or(pr.x0), py.unwrap_or(pr.y0)));
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

    /// `math.insert` places a frame that draws, edits it in place by id, and anchors one inline
    /// at the text insertion point.
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

    /// `display` is a documented parameter (so the unknown-parameter check accepts it) and
    /// stacks a named operator's limits.
    #[test]
    fn display_style_stacks_operator_limits() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let h = |s: &mut Session, display: bool| {
            s.execute("math.svg", &json!({"latex": r"\lim_{x\to 0} f(x)", "display": display})).unwrap()["height"].as_f64().unwrap()
        };
        let (text, display) = (h(&mut s, false), h(&mut s, true));
        assert!(display > text + 2.0, "{text} {display}");
        let r = s.execute("math.insert", &json!({"latex": r"\max_{k} a_k", "display": true, "x": 100, "y": 100})).unwrap();
        assert!(r["id"].as_u64().is_some());
    }

    /// An edit that doesn't give `display` keeps the equation's style; giving it changes it.
    #[test]
    fn an_edit_keeps_display_style() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let src = r"\lim_{x\to 0} f(x)";
        let r = s.execute("math.insert", &json!({"latex": src, "display": true, "x": 100, "y": 100})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let h = |s: &Session| s.doc().unwrap().doc.item(id).unwrap().bounds().height();
        let shown = h(&s);
        s.execute("math.insert", &json!({"id": id.0, "latex": src})).unwrap();
        assert!((h(&s) - shown).abs() < 1e-6, "{} {shown}", h(&s));
        s.execute("math.insert", &json!({"id": id.0, "latex": src, "display": false})).unwrap();
        let inline = h(&s);
        assert!(inline < shown - 2.0, "{inline} {shown}");
        s.execute("math.insert", &json!({"id": id.0, "latex": src})).unwrap();
        assert!((h(&s) - inline).abs() < 1e-6);
    }

    /// The family the equation `id` was set in, read from its SVG.
    fn family(s: &Session, id: designcraft_doc::ItemId) -> Option<String> {
        let d = &s.doc().unwrap().doc;
        let designcraft_doc::Content::Graphic(g) = &d.item(id).unwrap().content else { panic!("not a graphic") };
        crate::math::family_of(&d.assets[&g.asset].data)
    }

    /// `family` is a documented parameter that sets the equation in that installed family; an
    /// edit keeps it unless it names another (or `""`, the bundled serif), and it survives a
    /// save and reopen and an IDML round trip. A family that isn't installed sets the bundled serif.
    #[test]
    fn family_sets_the_equation_and_survives_edits() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let svg = s.execute("math.svg", &json!({"latex": "x+1", "family": "JetBrains Mono"})).unwrap();
        assert!(svg["svg"].as_str().unwrap().contains("'JetBrains Mono'"), "{svg}");
        let r = s.execute("math.insert", &json!({"latex": r"\frac{a}{b} + 1", "family": "jetbrains mono", "x": 100, "y": 100})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        assert_eq!(family(&s, id).as_deref(), Some("JetBrains Mono"));
        // It draws.
        let d = s.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 2.0, false, &Default::default()).unwrap();
        let b = d.item(id).unwrap().bounds();
        let dark = (b.x0 as u32 * 2..b.x1 as u32 * 2)
            .flat_map(|x| (b.y0 as u32 * 2..b.y1 as u32 * 2).map(move |y| (x, y)))
            .filter(|&(x, y)| img.pixel(x, y)[0] < 100)
            .count();
        assert!(dark > 30, "{dark}");
        // An edit that doesn't name a family keeps it, also after a save and reopen.
        s.execute("math.insert", &json!({"id": id.0, "latex": r"\frac{a}{b} + 2"})).unwrap();
        assert_eq!(family(&s, id).as_deref(), Some("JetBrains Mono"));
        let bytes = super::super::file::to_bytes(&s.doc().unwrap().doc).unwrap();
        let mut s2 = Session::new();
        s2.execute("file.openBytes", &json!({"name": "eq.designcraft", "base64": super::super::file::base64_encode(&bytes)})).unwrap();
        assert_eq!(family(&s2, id).as_deref(), Some("JetBrains Mono"));
        s2.execute("math.insert", &json!({"id": id.0, "latex": "y"})).unwrap();
        assert_eq!(family(&s2, id).as_deref(), Some("JetBrains Mono"));
        // IDML embeds the equation's SVG, so an edit after the round trip keeps the family too
        // (the LaTeX alt text doesn't make that trip yet, so the edit gives it again).
        let idml = designcraft_idml::export_idml(&s.doc().unwrap().doc);
        let mut s3 = Session::new();
        s3.execute("file.openBytes", &json!({"name": "eq.idml", "base64": super::super::file::base64_encode(&idml)})).unwrap();
        let d3 = &s3.doc().unwrap().doc;
        let eq = d3.all_items().into_iter().find(|&i| d3.item(i).is_some_and(|it| it.name == "Math")).unwrap();
        assert_eq!(family(&s3, eq).as_deref(), Some("JetBrains Mono"));
        s3.execute("math.insert", &json!({"id": eq.0, "latex": "z"})).unwrap();
        assert_eq!(family(&s3, eq).as_deref(), Some("JetBrains Mono"));
        // Naming another family switches; "" goes back to the bundled serif.
        s.execute("math.insert", &json!({"id": id.0, "latex": "x", "family": "Source Sans 3"})).unwrap();
        assert_eq!(family(&s, id).as_deref(), Some("Source Sans 3"));
        s.execute("math.insert", &json!({"id": id.0, "latex": "x", "family": ""})).unwrap();
        assert_eq!(family(&s, id), None);
        // A family that isn't installed: set in the bundled serif, no error, and the name kept
        // through an edit (it is used once the family is installed).
        let r = s.execute("math.insert", &json!({"latex": "x", "family": "No Such Family 7", "x": 100, "y": 300})).unwrap();
        let missing = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        assert_eq!(family(&s, missing).as_deref(), Some("No Such Family 7"));
        s.execute("math.insert", &json!({"id": missing.0, "latex": "x+1"})).unwrap();
        assert_eq!(family(&s, missing).as_deref(), Some("No Such Family 7"));
        // Inline equations take it too.
        let f = s.execute("frame.create", &json!({"rect": [72, 400, 400, 500], "content": "text", "text": "Sum: "})).unwrap();
        s.execute("text.select", &json!({"story": f["story"], "anchor": 5, "focus": 5})).unwrap();
        assert_eq!(s.execute("math.insert", &json!({"latex": "a+b", "family": "Inter"})).unwrap()["inline"], true);
    }

    /// The item's top-left corner in spread space (inner path corner through its transform).
    fn origin(s: &Session, id: designcraft_doc::ItemId) -> designcraft_geom::Point {
        let d = &s.doc().unwrap().doc;
        let it = d.item(id).unwrap();
        let r = it.path.bounds().unwrap();
        it.xf * designcraft_geom::Point::new(r.x0, r.y0)
    }

    #[test]
    fn editing_a_transformed_equation_keeps_its_place() {
        for step in ["move", "rotate", "scale"] {
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            let r = s.execute("math.insert", &json!({"latex": r"\frac{a}{b}", "x": 100, "y": 100})).unwrap();
            let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
            match step {
                "move" => s.execute("transform.move", &json!({"ids": [id.0], "dx": 50, "dy": 40})),
                "rotate" => s.execute("transform.rotate", &json!({"ids": [id.0], "angle": 30})),
                _ => s.execute("transform.scale", &json!({"ids": [id.0], "sx": 2.0, "sy": 1.5})),
            }
            .unwrap();
            let before = origin(&s, id);
            s.execute("math.insert", &json!({"id": id.0, "latex": r"\frac{a}{b}"})).unwrap();
            let after = origin(&s, id);
            assert!((before.x - after.x).abs() < 1e-6 && (before.y - after.y).abs() < 1e-6, "{step}: {before:?} -> {after:?}");
        }
    }

    #[test]
    fn editing_a_moved_equation_keeps_its_bounds() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("math.insert", &json!({"latex": "x", "x": 100, "y": 100})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        s.execute("transform.move", &json!({"ids": [id.0], "dx": 50, "dy": 40})).unwrap();
        let before = s.doc().unwrap().doc.item(id).unwrap().bounds();
        s.execute("math.insert", &json!({"id": id.0, "latex": "x"})).unwrap();
        let after = s.doc().unwrap().doc.item(id).unwrap().bounds();
        assert_eq!((before.x0, before.y0), (after.x0, after.y0));
        assert_eq!((after.x0, after.y0), (150.0, 140.0));
    }
}
