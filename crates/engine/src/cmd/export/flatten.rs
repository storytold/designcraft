use super::{EngineError, Result};

/// Does this object (or anything in it) need the transparency flattener?
pub(super) fn transparent(it: &designcraft_doc::Item) -> bool {
    it.involves_transparency()
}

/// Transparency Flattener: each object involving transparency becomes an opaque image of its
/// area as it looks over what's beneath it (so the rest stays vector); spreads whose parent items
/// involve transparency are rasterised whole. Works on a copy of the document made for export.
/// Returns the copy and the number of rasterised regions.
pub(super) fn flatten(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache, ppi: f64) -> (designcraft_doc::Document, usize) {
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape, SpreadRef};
    let mut out = d.clone();
    let mut n = 0;
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let k = ppi / 72.0;
    let parent_transparent = |pid: Option<designcraft_doc::SpreadId>| {
        pid.and_then(|id| d.parent_index(id))
            .is_some_and(|pi| d.parent_chain(pi).iter().filter_map(|&i| d.parents.get(i)).any(|p| p.items.iter().any(|it| transparent(it))))
    };
    // An opaque image item showing `img` over `r`.
    let image_item =
        |out: &mut designcraft_doc::Document, r: designcraft_geom::Rect, img: designcraft_render::Rendered, layer: designcraft_doc::LayerId| {
            let (w, h) = (img.width, img.height);
            let aid = AssetId(out.alloc());
            out.assets.insert(
                aid,
                std::sync::Arc::new(Asset {
                    id: aid,
                    name: format!("flattened-{}.png", aid.0),
                    mime: "image/png".into(),
                    data: std::sync::Arc::new(img.to_png()),
                    pixels: Some((w, h)),
                    ..Default::default()
                }),
            );
            let mut it = Item::new(ItemId(out.alloc()), layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
            it.content = Content::Graphic(Graphic {
                layer_options: None,
                asset: aid,
                size: (w as f64, h as f64),
                xf: designcraft_geom::Affine::translate((r.x0, r.y0))
                    * designcraft_geom::Affine::scale_non_uniform(r.width() / w as f64, r.height() / h as f64),
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            it.stroke.weight = 0.0;
            it
        };
    for (si, sp) in d.spreads.iter().enumerate() {
        let whole = sp.pages.iter().any(|pg| pg.show_parent_items && parent_transparent(pg.parent));
        if whole {
            // Parent items involve transparency: every page of the spread as one image.
            let first = d.first_page_of_spread(si);
            let lid = out.default_layer();
            let mut images = Vec::new();
            for (pi, pg) in sp.pages.iter().enumerate() {
                if let Some(img) = rr.render_page(d, cache, first + pi, k, false, &Default::default()) {
                    images.push((pg.bounds(), img));
                }
            }
            let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
            osp.items.clear();
            for pg in &mut osp.pages {
                pg.show_parent_items = false;
            }
            for (b, img) in images {
                let it = image_item(&mut out, b, img, lid);
                let _ = out.insert_item(SpreadRef::Doc(si), it, None);
                n += 1;
            }
            continue;
        }
        // Region by region, bottom to top.
        for (idx, it) in sp.items.iter().enumerate() {
            if it.hidden || !transparent(it) {
                continue;
            }
            let reach = it.bounds().inflate(
                designcraft_render::effect_outset(it) + it.stroke.extent() + 1.0,
                designcraft_render::effect_outset(it) + it.stroke.extent() + 1.0,
            );
            // Only the part on the spread's pages and pasteboard that prints.
            let (w, h) = ((reach.width() * k).ceil().max(1.0) as u32, (reach.height() * k).ceil().max(1.0) as u32);
            if w as u64 * h as u64 > 80_000_000 {
                continue;
            }
            // The stack up to and including this object.
            let mut below = d.clone();
            std::sync::Arc::make_mut(&mut below.spreads[si]).items.truncate(idx + 1);
            let view = designcraft_geom::Affine::scale(k) * designcraft_geom::Affine::translate((-reach.x0, -reach.y0));
            let opts = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
            let img = rr.render(
                &below,
                cache,
                &[designcraft_render::Placed { spread: SpreadRef::Doc(si), xf: designcraft_geom::Affine::translate(designcraft_geom::Vec2::ZERO) }],
                (w, h),
                view,
                &opts,
            );
            let new = image_item(&mut out, reach, img, it.layer);
            let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
            let Some(pos) = osp.items.iter().position(|x| x.id == it.id) else {
                continue;
            };
            if threaded(d, it) {
                // Keep the frame (its story flows on) under the image.
                osp.items.insert(pos + 1, std::sync::Arc::new(new));
            } else {
                osp.items[pos] = std::sync::Arc::new(new);
            }
            n += 1;
        }
    }
    (out, n)
}

/// A text frame in a story that runs through other frames (replacing it would reflow the story).
pub(super) fn threaded(d: &designcraft_doc::Document, it: &designcraft_doc::Item) -> bool {
    it.text_frame().and_then(|t| d.story(t.story)).is_some_and(|st| st.frames.len() > 1)
}

/// Soft effects other than the gradient feather (which PDF export draws itself).
fn raster_effects(it: &designcraft_doc::Item) -> bool {
    let e = &it.effects;
    e.drop_shadow.on
        || e.feather > 0.0
        || e.inner_shadow.on
        || e.outer_glow.on
        || e.inner_glow.on
        || e.bevel.on
        || e.satin.on
        || e.directional_feather.on
}

/// A copy of `d` where spread-level objects with soft effects are 300 ppi images of their whole
/// appearance (transparent around them, so they composite over what's behind). Objects in `skip`
/// stay as they are.
pub(super) fn rasterize_effects(
    d: &designcraft_doc::Document,
    cache: &designcraft_compose::Cache,
    skip: &std::collections::HashMap<designcraft_doc::ItemId, designcraft_pdf::ShadowMask>,
) -> (Option<designcraft_doc::Document>, usize) {
    rasterize_where(d, cache, 300.0, true, |it| raster_effects(it) && !threaded(d, it) && !skip.contains_key(&it.id))
}

/// A drop shadow is the object's only soft effect, and the PDF writer draws its shadow itself
/// (a leaf object, not a group or a frame with items pasted into it).
fn shadow_only(it: &designcraft_doc::Item) -> bool {
    let e = &it.effects;
    e.drop_shadow.on
        && !(e.feather > 0.0
            || e.inner_shadow.on
            || e.outer_glow.on
            || e.inner_glow.on
            || e.bevel.on
            || e.satin.on
            || e.directional_feather.on
            || e.gradient_feather.on)
        && it.children().is_empty()
        && !it.has_nested_items()
}

/// For PDF/X-1a: the coverage of each spread object's soft drop shadow at `ppi`, so the PDF draws
/// the shadow in its own swatch's inks through a mask (which craft-prepress flattens per ink)
/// instead of an RGB image of the object. Objects whose only soft effect is the drop shadow.
///
/// The coverage comes from the object drawn alone, at full opacity and Normal blending, with its
/// shadow and without it ([`shadow_coverage`]): the shadow's own alpha, its opacity included. The
/// object's own opacity and blend mode stay on the object's group in the PDF.
pub(super) fn shadow_masks(
    d: &designcraft_doc::Document,
    cache: &designcraft_compose::Cache,
    ppi: f64,
) -> std::collections::HashMap<designcraft_doc::ItemId, designcraft_pdf::ShadowMask> {
    use designcraft_doc::SpreadRef;
    let mut out = std::collections::HashMap::new();
    let todo: Vec<(usize, designcraft_doc::ItemId)> = d
        .spreads
        .iter()
        .enumerate()
        .flat_map(|(si, sp)| sp.items.iter().filter(|it| !it.hidden && shadow_only(it)).map(move |it| (si, it.id)))
        .collect();
    if todo.is_empty() {
        return out;
    }
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let k = ppi.clamp(36.0, 1200.0) / 72.0;
    for (si, id) in todo {
        let Some(it) = d.item(id) else { continue };
        let grow = designcraft_render::effect_outset(it) + it.stroke.extent() + 2.0;
        let reach = it.bounds().inflate(grow, grow);
        let (w, h) = ((reach.width() * k).ceil().max(1.0), (reach.height() * k).ceil().max(1.0));
        if !(w.is_finite() && h.is_finite()) || w * h > 60_000_000.0 {
            continue;
        }
        let (w, h) = (w as u32, h as u32);
        let mut alpha = |shadow: bool| {
            let mut solo = d.clone();
            let Some(sp) = solo.spreads.get_mut(si) else { return Vec::new() };
            let sp = std::sync::Arc::make_mut(sp);
            sp.items.retain(|x| x.id == id);
            for pg in &mut sp.pages {
                pg.show_parent_items = false;
            }
            if let Some(x) = sp.items.first_mut() {
                let x = std::sync::Arc::make_mut(x);
                x.opacity = 1.0;
                x.blend = Default::default();
                x.effects.drop_shadow.on = shadow;
            }
            let view = designcraft_geom::Affine::scale(k) * designcraft_geom::Affine::translate((-reach.x0, -reach.y0));
            let opts = designcraft_render::RenderOptions { paper: false, background: None, printing_only: true, ..Default::default() };
            let placed = [designcraft_render::Placed { spread: SpreadRef::Doc(si), xf: designcraft_geom::Affine::IDENTITY }];
            rr.render(&solo, cache, &placed, (w, h), view, &opts).pixels.as_chunks::<4>().0.iter().map(|px| px[3]).collect::<Vec<u8>>()
        };
        let (with, without) = (alpha(true), alpha(false));
        let n = w as usize * h as usize;
        if with.len() != n || without.len() != n {
            continue;
        }
        let coverage: Vec<u8> = with.iter().zip(&without).map(|(a, b)| shadow_coverage(*a, *b)).collect();
        out.insert(id, designcraft_pdf::ShadowMask { rect: reach, width: w, height: h, coverage: std::sync::Arc::new(coverage) });
    }
    out
}

/// The shadow's alpha `s` at a pixel where the object alone has alpha `without` and the object
/// over its shadow `with`: `with = a + s(1 − a)` for the object's alpha `a`, so the PDF, which
/// draws the object over the shadow, gets the same pixel back. 0 where the object is opaque.
pub(super) fn shadow_coverage(with: u8, without: u8) -> u8 {
    let rest = 255 - u32::from(without);
    if rest == 0 {
        return 0;
    }
    let shown = u32::from(with.saturating_sub(without));
    u8::try_from(((shown * 255 + rest / 2) / rest).min(255)).unwrap_or(u8::MAX)
}

/// A copy of `d` where the spread-level objects `pred` picks are images of their appearance at
/// `ppi` (transparent around them). With `keep_wrap`, each object also stays behind as a
/// nonprinting [`wrap_stand_in`], so text wraps around it as before.
pub(super) fn rasterize_where(
    d: &designcraft_doc::Document,
    cache: &designcraft_compose::Cache,
    ppi: f64,
    keep_wrap: bool,
    pred: impl Fn(&designcraft_doc::Item) -> bool,
) -> (Option<designcraft_doc::Document>, usize) {
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape, SpreadRef};
    let todo: Vec<(usize, ItemId)> =
        d.spreads.iter().enumerate().flat_map(|(si, sp)| sp.items.iter().filter(|it| !it.hidden && pred(it)).map(move |it| (si, it.id))).collect();
    if todo.is_empty() {
        return (None, 0);
    }
    let mut out = d.clone();
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let k = ppi / 72.0;
    let mut n = 0;
    for (si, id) in todo {
        let Some(it) = d.item(id) else { continue };
        let reach = it.bounds().inflate(
            designcraft_render::effect_outset(it) + it.stroke.extent() + 2.0,
            designcraft_render::effect_outset(it) + it.stroke.extent() + 2.0,
        );
        // Just this object on its spread, no paper.
        let mut solo = d.clone();
        let sp = std::sync::Arc::make_mut(&mut solo.spreads[si]);
        sp.items.retain(|x| x.id == id);
        for pg in &mut sp.pages {
            pg.show_parent_items = false;
        }
        let (w, h) = ((reach.width() * k).ceil().max(1.0) as u32, (reach.height() * k).ceil().max(1.0) as u32);
        if w as u64 * h as u64 > 60_000_000 {
            continue;
        }
        let view = designcraft_geom::Affine::scale(k) * designcraft_geom::Affine::translate((-reach.x0, -reach.y0));
        let opts = designcraft_render::RenderOptions { paper: false, background: None, printing_only: true, ..Default::default() };
        let img = rr.render(
            &solo,
            cache,
            &[designcraft_render::Placed { spread: SpreadRef::Doc(si), xf: designcraft_geom::Affine::translate(designcraft_geom::Vec2::ZERO) }],
            (w, h),
            view,
            &opts,
        );
        let aid = AssetId(out.alloc());
        out.assets.insert(
            aid,
            std::sync::Arc::new(Asset {
                id: aid,
                name: format!("effects-{}.png", id.0),
                mime: "image/png".into(),
                data: std::sync::Arc::new(img.to_png()),
                pixels: Some((w, h)),
                ..Default::default()
            }),
        );
        let mut img_item = Item::new(ItemId(out.alloc()), it.layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(reach));
        img_item.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (w as f64, h as f64),
            xf: designcraft_geom::Affine::translate((reach.x0, reach.y0)) * designcraft_geom::Affine::scale(1.0 / k),
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        img_item.stroke.weight = 0.0;
        img_item.alt_text = it.alt_text.clone();
        img_item.export_options = it.export_options.clone();
        // Same place in the stacking order.
        let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
        if let Some(pos) = osp.items.iter().position(|x| x.id == id) {
            osp.items[pos] = std::sync::Arc::new(img_item);
            if keep_wrap && has_wrap(it, 0) {
                osp.items.insert(pos, std::sync::Arc::new(wrap_stand_in(it, 0)));
            }
            n += 1;
        }
    }
    (Some(out), n)
}

/// Whether `it`, its placed graphic or an item in it wraps text.
fn has_wrap(it: &designcraft_doc::Item, depth: usize) -> bool {
    use designcraft_doc::{Content, WrapMode};
    if depth > MAX_STAND_IN_DEPTH {
        return false;
    }
    it.wrap.mode != WrapMode::None
        || match &it.content {
            Content::Graphic(g) => g.wrap.mode != WrapMode::None,
            Content::Group { items } => items.iter().any(|c| has_wrap(c, depth + 1)),
            _ => false,
        }
}

/// Nesting depth past which groups inside a rasterised object keep no wrap stand-ins.
const MAX_STAND_IN_DEPTH: usize = 64;

/// A nonprinting copy of `it` that keeps its text wrap (and its graphic's and its items') and
/// nothing else that reaches the PDF: no text, form field, button or media. The image of the
/// object stands in for all of that, as it does without the copy.
fn wrap_stand_in(it: &designcraft_doc::Item, depth: usize) -> designcraft_doc::Item {
    use designcraft_doc::Content;
    let mut c = it.clone();
    c.nonprinting = true;
    c.form_field = None;
    c.button = None;
    c.media = None;
    c.content = match &it.content {
        Content::Text(_) => Content::Unassigned,
        Content::Group { items } if depth < MAX_STAND_IN_DEPTH => {
            Content::Group { items: items.iter().map(|i| std::sync::Arc::new(wrap_stand_in(i, depth + 1))).collect() }
        }
        Content::Group { .. } => Content::Group { items: Vec::new() },
        other => other.clone(),
    };
    c
}

/// A copy of `d` where placed PDFs with Object Layer Options hidden layers are 300 ppi images.
///
/// Parent-page and anchored placements count too, including anchors inside table-cell,
/// footnote and endnote text. Only the export copy is rewritten.
pub(super) fn rasterize_layered(d: &designcraft_doc::Document) -> Result<(Option<designcraft_doc::Document>, usize)> {
    let choices: Vec<_> = d
        .placed_items()
        .into_iter()
        .filter(|it| it.has_pdf_layer_override())
        .filter_map(|it| {
            let g = it.graphic()?;
            let a = d.assets.get(&g.asset).filter(|a| designcraft_render::is_pdf(&a.data))?;
            Some((it.id, g.clone(), a.clone(), it.pdf_hidden_layers.clone()))
        })
        .collect();
    if choices.is_empty() {
        return Ok((None, 0));
    }
    let mut out = d.clone();
    let mut images = std::collections::HashMap::new();
    let mut n = 0;
    for (id, g, a, hidden) in choices {
        // The whole page at 300 ppi (the graphic may show only a box of it).
        let shown = designcraft_render::shown_box(&a);
        let [l, t, r, b] = shown.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let page_side = (g.size.0 / (r - l)).max(g.size.1 / (b - t));
        let side = (page_side * 300.0 / 72.0).clamp(64.0, 8000.0) as u32;
        let png = designcraft_render::pdf_page_png_options_checked(&a.data, a.page as usize, side, Some(&hidden), shown)
            .map_err(|e| EngineError::Other(format!("{}: {e}", a.name)))?;
        let px = designcraft_render::image_size(&png);
        let aid = designcraft_doc::AssetId(out.alloc());
        out.assets.insert(
            aid,
            std::sync::Arc::new(designcraft_doc::Asset {
                id: aid,
                name: format!("{}.png", a.name),
                mime: "image/png".into(),
                data: std::sync::Arc::new(png),
                pixels: px,
                ..Default::default()
            }),
        );
        images.insert(id, aid);
        n += 1;
    }
    out.update_placed_items(
        |it| images.contains_key(&it.id),
        |it| {
            if let designcraft_doc::Content::Graphic(gg) = &mut it.content
                && let Some(aid) = images.get(&it.id)
            {
                gg.asset = *aid;
            }
        },
    );
    Ok((Some(out), n))
}

#[cfg(test)]
mod placed_layer_tests {
    use std::sync::Arc;

    use designcraft_doc::{AnchoredObject, Document, Item, ItemId, SpreadRef, StoryId, Table};
    use designcraft_geom::Rect;
    use serde_json::json;

    use crate::Session;

    fn two_layer_pdf() -> Vec<u8> {
        let content = "/OC /a BDC 0 0 1 rg 0 0 50 100 re f EMC /OC /b BDC 1 0 0 rg 50 0 50 100 re f EMC";
        let objs = [
            "<</Type/Catalog/Lang(en-GB)/Pages 2 0 R/OCProperties<</OCGs[5 0 R 6 0 R]/D<</Order[5 0 R 6 0 R]/Name(Default)/Locked[5 0 R]>>>>>>"
                .to_string(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</Properties<</a 5 0 R/b 6 0 R>>>>/Contents 4 0 R>>".to_string(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
            "<</Type/OCG/Name(Blue)>>".to_string(),
            "<</Type/OCG/Name(Red)>>".to_string(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
        }
        let x = out.len();
        let mut t = format!("xref\n0 {}\n0000000000 65535 f\r\n", objs.len() + 1);
        for o in offs {
            t.push_str(&format!("{o:010} 00000 n\r\n"));
        }
        t.push_str(&format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1));
        out.extend_from_slice(t.as_bytes());
        out
    }

    fn placement<'a>(d: &'a Document, location: &str, id: ItemId, sid: StoryId) -> &'a Item {
        match location {
            "cell" => &d.story(sid).unwrap().tables.values().next().unwrap().cells[0].text.objects[0].item,
            "footnote" => &d.story(sid).unwrap().notes[0].text.objects[0].item,
            "endnote" => &d.story(sid).unwrap().endnotes[0].text.objects[0].item,
            "anchor" => &d.story(sid).unwrap().objects[0].item,
            _ => d.item(id).unwrap(),
        }
    }

    #[test]
    fn stale_layer_fields_on_non_pdf_placements_do_not_block_pdf_a() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let result = s
            .execute(
                "file.place",
                &json!({
                    "base64": crate::cmd::file::base64_encode(&two_layer_pdf()), "name": "layers.pdf",
                }),
            )
            .unwrap();
        let id = ItemId(result["id"].as_u64().unwrap());
        s.execute("object.layerOptions", &json!({"ids": [id.0], "hidden": ["Blue"]})).unwrap();
        // Relinking to an ordinary image can leave legacy fields on the placement.
        // They are not an actionable placed-PDF override and must not fail closed.
        let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let aid = d.item(id).unwrap().graphic().unwrap().asset;
        let asset = Arc::make_mut(d.assets.get_mut(&aid).unwrap());
        asset.mime = "image/png".into();
        asset.data = Arc::new(designcraft_render::Rendered { width: 1, height: 1, pixels: vec![0, 0, 0, 0] }.to_png());
        let (copy, count) = super::rasterize_layered(d).unwrap();
        assert_eq!(count, 0);
        assert!(copy.is_none());
        s.execute("file.exportPdf", &json!({"standard": "a2b"})).unwrap();
    }

    fn check_placement(location: &str) {
        for all_on in [false, true] {
            check_choice(location, all_on);
        }
    }

    fn check_choice(location: &str, all_on: bool) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
        let placed = s
            .execute(
                "file.place",
                &json!({
                    "base64": crate::cmd::file::base64_encode(&two_layer_pdf()), "name": "layers.pdf",
                    "x": 0, "y": 0, "width": 100,
                    "spread": {"kind": if location == "parent" { "parent" } else { "doc" }, "index": 0}
                }),
            )
            .unwrap();
        let id = ItemId(placed["id"].as_u64().unwrap());
        let hidden = if all_on { vec![] } else { vec!["Blue"] };
        s.execute("object.layerOptions", &json!({"ids": [id.0], "hidden": hidden, "useDefaults": false})).unwrap();
        let sid = {
            let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let layer = d.default_layer();
            let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 80.0, 80.0), layer, "Body", Default::default()).unwrap();
            if !matches!(location, "doc" | "parent") {
                let item = (**d.spreads[0].items.iter().find(|it| it.id == id).unwrap()).clone();
                Arc::make_mut(&mut d.spreads[0]).items.retain(|it| it.id != id);
                let object = AnchoredObject { item, position: Default::default() };
                let story = d.story_mut(sid).unwrap();
                match location {
                    "cell" => {
                        let mut table = Table::new(1, 1, 1, 0, 0, 20.0);
                        table.cells[0].text.insert_object(0, object);
                        story.insert_table(0, table);
                    }
                    "footnote" => {
                        story.insert_note(0, "Note", Default::default()).unwrap();
                        Arc::make_mut(&mut story.notes[0]).text.insert_object(0, object);
                    }
                    "endnote" => {
                        story.insert_endnote(0, "Endnote", Default::default()).unwrap();
                        Arc::make_mut(&mut story.endnotes[0]).text.insert_object(0, object);
                    }
                    "anchor" => story.insert_object(0, object),
                    _ => assert!(matches!(location, "doc" | "parent")),
                }
            }
            sid
        };
        // Exercise the real export command for each location, not just the traversal helper.
        let result = s.execute("file.exportPdf", &json!({"standard": "a2b"})).unwrap();
        assert!(
            result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("1 placed PDF(s)")),
            "{location}: {}",
            result["warnings"]
        );
        // Check the rewritten placement, including its pixels. Counting a placement without
        // updating the nested object would still export the file's default layer visibility.
        let source = &s.doc().unwrap().doc;
        let (copy, count) = super::rasterize_layered(source).unwrap();
        assert_eq!(count, 1, "{location}");
        let copy = copy.unwrap();
        let item = placement(&copy, location, id, sid);
        let asset = &copy.assets[&item.graphic().unwrap().asset];
        assert_eq!(asset.mime, "image/png", "{location}");
        let image = designcraft_render::images::decoded(&asset.data, 0);
        let image = image.unwrap();
        let pixel = image.data()[usize::from(image.height()) / 2 * usize::from(image.width()) + usize::from(image.width()) / 4];
        assert_eq!(pixel.a, if all_on { 255 } else { 0 }, "Blue visibility at {location}, all_on={all_on}");
        // Export preparation must not change the open document's placement or source asset.
        let original = placement(source, location, id, sid);
        assert_eq!(original.pdf_hidden_layers, hidden);
        assert!(original.pdf_layer_override);
        assert_eq!(source.assets[&original.graphic().unwrap().asset].mime, "application/pdf");
    }

    #[test]
    fn layered_pdf_a_document_placement() {
        check_placement("doc");
    }
    #[test]
    fn layered_pdf_a_parent_placement() {
        check_placement("parent");
    }
    #[test]
    fn layered_pdf_a_story_anchor() {
        check_placement("anchor");
    }
    #[test]
    fn layered_pdf_a_table_cell_anchor() {
        check_placement("cell");
    }
    #[test]
    fn layered_pdf_a_footnote_anchor() {
        check_placement("footnote");
    }
    #[test]
    fn layered_pdf_a_endnote_anchor() {
        check_placement("endnote");
    }
}
