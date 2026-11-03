#[cfg(not(target_arch = "wasm32"))]
use super::EngineError;
#[cfg(test)]
use super::bytes::base64_encode;
use super::bytes::{base64_decode, from_bytes};
use super::{
    Affine, Arc, Asset, AssetId, Content, Graphic, Item, ItemId, PdfCrop, Rect, Result, Selection, Session, Shape, Value, bad, json, ok, shapes,
    str_param,
};

pub(super) fn file_place(s: &mut Session, p: &Value) -> Result<Value> {
    // The file's size and time before it is read (a linked text file remembers them).
    let mut before = None;
    let (bytes, name, link) = if let Some(b) = str_param(p, "base64") {
        (base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None)
    } else if let Some(path) = str_param(p, "path") {
        before = crate::cmd::datamerge::fingerprint_from_path(path);
        #[cfg(not(target_arch = "wasm32"))]
        let b = crate::io::read_file(path).map_err(EngineError::Other)?;
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = vec![];
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
        (b, name, Some(path.to_string()))
    } else {
        s.ui_requests.push(crate::UiRequest::Pick { purpose: "place".into(), params: json!({}) });
        return ok();
    };
    // A page of another layout (IDML or DesignCraft): its objects, as one group.
    let lname = name.to_lowercase();
    if lname.ends_with(".idml") || lname.ends_with(".designcraft") {
        return place_layout_page(s, p, &name, &bytes);
    }
    // Keep the EPS source for link comparison; rendering uses its preview at the bounding box size.
    let eps_size = eps_size(&bytes, "file.place")?;
    // Video and sound: a media frame (Window › Interactive › Media).
    if let Some(mime) = designcraft_doc::media_mime(&name) {
        return crate::cmd::media::place_media(s, p, name, mime, bytes, link);
    }
    // Text files (plain, Word, RTF) flow into frames.
    if designcraft_textimport::is_text_file(&name) {
        return crate::cmd::place_text::place_text(s, p, &name, &bytes, link, before);
    }
    let want_w = p
        .get("width")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_f64()
                .filter(|width| width.is_finite() && *width > 0.0)
                .ok_or_else(|| bad("file.place", "`width` must be a finite number greater than zero"))
        })
        .transpose()?;
    // Image Import Options: which page of a PDF (1-based).
    if designcraft_render::is_pdf(&bytes) {
        designcraft_render::check_pdf_input_size(bytes.len()).map_err(|e| bad("file.place", e.to_string()))?;
    }
    let pdf_page = match p.get("pdfPage").and_then(Value::as_u64) {
        Some(n) if designcraft_render::is_pdf(&bytes) => {
            let count = designcraft_render::pdf_page_count(&bytes).unwrap_or(1);
            let invalid_page = || bad("file.place", format!("the PDF has {count} page(s)"));
            let one_based = usize::try_from(n).map_err(|_| invalid_page())?;
            if one_based == 0 || one_based > count {
                return Err(invalid_page());
            }
            let zero_based = one_based.checked_sub(1).ok_or_else(invalid_page)?;
            u32::try_from(zero_based).map_err(|_| invalid_page())?
        }
        _ => 0,
    };
    let (pw, ph) = match pdf_page {
        0 => designcraft_render::image_size(&bytes).ok_or_else(|| bad("file.place", "unsupported or corrupt image"))?,
        n => designcraft_render::pdf_page_size(&bytes, n as usize)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
            .ok_or_else(|| bad("file.place", "can't read that PDF page"))?,
    };
    // 72 ppi by default unless the file says otherwise; scale so it fits the page when huge. PDF pages
    // and SVGs keep their exact size: rounding it to whole points would stretch them unevenly.
    let (nw, nh) = eps_size.or_else(|| designcraft_render::natural_size(&bytes, pdf_page as usize)).unwrap_or((pw as f64, ph as f64));
    // Image Import Options › Crop to: the graphic spans that box of the PDF page.
    let is_pdf = designcraft_render::is_pdf(&bytes);
    let pdf_crop = match str_param(p, "pdfCrop") {
        Some(k) if is_pdf => PdfCrop::from_name(k)
            .ok_or_else(|| bad("file.place", format!("unknown pdfCrop `{k}` (crop, art, trim, bleed, media, contentVisible, contentAll)")))?,
        _ => PdfCrop::Crop,
    };
    let mime = designcraft_render::image_mime(&bytes).to_string();
    let mut asset = Asset { page: pdf_page, name, mime, link, data: Arc::new(bytes), pixels: Some((pw, ph)), pdf_crop, ..Default::default() };
    // The graphic's natural size: the box in points (the whole page when the box can't be found,
    // and then the document says so).
    let (bw, bh) = match crop_pdf(&mut asset) {
        Some(size) => size,
        None => {
            asset.pdf_crop = PdfCrop::Crop;
            (nw, nh)
        }
    };
    let target_frame = crate::cmd::id_param(p, "frame").or_else(|| {
        if ["x", "y", "spread", "width"].iter().any(|key| p.get(*key).is_some_and(|value| !value.is_null())) {
            return None;
        }
        let st = s.active()?;
        st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| matches!(it.content, Content::Unassigned | Content::Graphic(_))))
    });
    // `"spread": null` counts as omitted, like the other placement parameters.
    let spread = match p.get("spread") {
        Some(Value::Null) => designcraft_doc::SpreadRef::Doc(0),
        _ => crate::cmd::spread_param(p, "file.place")?,
    };
    let lid = s.doc()?.active_layer;
    let px = p.get("x").and_then(Value::as_f64);
    let py = p.get("y").and_then(Value::as_f64);
    if target_frame.is_none() && px.is_none() && py.is_none() && p.get("loadCursor").and_then(Value::as_bool) == Some(true) {
        return load_cursor(s, asset, (bw, bh));
    }
    s.edit(|d, sel| {
        let fresh = AssetId(d.alloc());
        let aid = d.add_asset(Asset { id: fresh, ..asset });
        // The width the whole page (or image) gets; the height follows the proportions.
        let w = match want_w {
            Some(w) => w,
            None => {
                let page_w = d.settings.page_width * 0.6;
                nw.min(page_w)
            }
        };
        let id = if let Some(fid) = target_frame {
            let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
            let r = it.inner_bounds();
            // Fill the frame proportionally once. Auto-Fit stays off, so resizing the frame crops.
            let k = (r.width() / bw).max(r.height() / bh);
            let (gw, gh) = (bw * k, bh * k);
            let xf = Affine::translate((r.x0 + (r.width() - gw) / 2.0, r.y0 + (r.height() - gh) / 2.0)) * Affine::scale(k);
            validate_placement((bw, bh), r, xf, "file.place")?;
            it.content = Content::Graphic(Graphic {
                layer_options: None,
                asset: aid,
                size: (bw, bh),
                xf,
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            fid
        } else {
            let sp = d.spread(spread).ok_or_else(|| bad("file.place", "no such spread"))?;
            let pr = sp.pages.first().map(|pg| pg.margin_rect()).unwrap_or(Rect::new(36.0, 36.0, 300.0, 300.0));
            let (x, y) = (px.unwrap_or(pr.x0), py.unwrap_or(pr.y0));
            let id = ItemId(d.alloc());
            let k = w / nw;
            let r = crate::cmd::finite_rect("file.place", Rect::new(x, y, x + bw * k, y + bh * k))?;
            let xf = Affine::translate((x, y)) * Affine::scale(k);
            validate_placement((bw, bh), r, xf, "file.place")?;
            let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(r));
            it.object_style = d.styles.default_graphic_frame.clone();
            if let Some(e) = d.styles.object_effects(&it.object_style) {
                it.effects = e.clone();
            }
            it.content = Content::Graphic(Graphic {
                layer_options: None,
                asset: aid,
                size: (bw, bh),
                xf,
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            d.insert_item(spread, it, None)?;
            id
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0, "asset": aid.0}))
    })
}

/// Sets `a.pdf_box` to where its `pdf_crop` box sits on its page and returns the box's size in
/// points. The crop box is the whole page and has none; nor has a page that can't be read, which
/// keeps its crop choice (a missing link relinked later gets its box then).
pub(crate) fn crop_pdf(a: &mut Asset) -> Option<(f64, f64)> {
    a.pdf_box = None;
    if a.pdf_crop == PdfCrop::Crop || !designcraft_render::is_pdf(&a.data) {
        return None;
    }
    let (frac, size) = designcraft_render::pdf_crop_box(&a.data, a.page as usize, a.pdf_crop)?;
    a.pdf_box = Some(frac);
    Some(size)
}

pub(crate) fn read_source(p: &Value) -> Result<(Vec<u8>, String, Option<String>)> {
    if let Some(b) = str_param(p, "base64") {
        return Ok((base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None));
    }
    let path = str_param(p, "path").ok_or_else(|| bad("place", "missing `path` or `base64`"))?;
    #[cfg(not(target_arch = "wasm32"))]
    let b = crate::io::read_file(path).map_err(EngineError::Other)?;
    #[cfg(target_arch = "wasm32")]
    let b: Vec<u8> = vec![];
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
    Ok((b, name, Some(path.to_string())))
}

fn eps_size(bytes: &[u8], command: &str) -> Result<Option<(f64, f64)>> {
    if !designcraft_images::is_eps(bytes) {
        return Ok(None);
    }
    designcraft_images::eps_size(bytes)
        .map(Some)
        .ok_or_else(|| bad(command, "the EPS has a missing or invalid bounding box (dimensions must be 0.001–20,000 points)"))
}

/// Fitting can overflow even with finite inputs. Never persist NaN/Inf as JSON nulls.
pub(crate) fn validate_placement(size: (f64, f64), frame: Rect, xf: Affine, command: &str) -> Result<()> {
    let valid_rect = |r: Rect| [r.x0, r.y0, r.x1, r.y1, r.width(), r.height()].iter().all(|n| n.is_finite()) && r.width() >= 0.0 && r.height() >= 0.0;
    if !size.0.is_finite()
        || !size.1.is_finite()
        || size.0 <= 0.0
        || size.1 <= 0.0
        || !valid_rect(frame)
        || !xf.as_coeffs().iter().all(|n| n.is_finite())
        || !valid_rect(xf.transform_rect_bbox(Rect::new(0.0, 0.0, size.0, size.1)))
    {
        return Err(bad(command, "image placement exceeds finite geometry limits"));
    }
    Ok(())
}

pub(super) fn place_load(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name, link) = read_source(p)?;
    let (pw, ph) = designcraft_render::image_size(&bytes).ok_or_else(|| bad("place.load", "unsupported or corrupt image"))?;
    let natural = eps_size(&bytes, "place.load")?.or_else(|| designcraft_render::natural_size(&bytes, 0)).unwrap_or((pw as f64, ph as f64));
    let mime = designcraft_render::image_mime(&bytes).to_string();
    let asset = Asset { name, mime, link, data: Arc::new(bytes), pixels: Some((pw, ph)), ..Default::default() };
    load_cursor(s, asset, natural)
}

/// Adds `asset` to the document (with a fresh id) and loads it into the place cursor.
fn load_cursor(s: &mut Session, mut asset: Asset, size: (f64, f64)) -> Result<Value> {
    let name = asset.name.clone();
    let (pw, ph) = asset.pixels.unwrap_or_default();
    let aid = s.edit(|d, _| {
        asset.id = AssetId(d.alloc());
        Ok(d.add_asset(asset))
    })?;
    s.loaded = Some(crate::LoadedGraphic { asset: aid, size });
    s.set_tool("placeGun");
    Ok(json!({"asset": aid.0, "name": name, "width": pw, "height": ph, "loaded": true}))
}

pub(super) fn place_drop(s: &mut Session, p: &Value) -> Result<Value> {
    let crate::LoadedGraphic { asset: aid, size: (nw, nh) } = s.loaded.ok_or_else(|| bad("place.drop", "nothing loaded in the place cursor"))?;
    // Undo can remove the loaded graphic's asset.
    if !s.doc()?.doc.assets.contains_key(&aid) {
        s.loaded = None;
        return Err(bad("place.drop", "the loaded graphic is no longer in the document"));
    }
    let sr = crate::cmd::spread_param(p, "place.drop")?;
    let lid = s.doc()?.active_layer;
    let frame = crate::cmd::id_param(p, "frame");
    let rect = crate::cmd::rect_param(p, "rect");
    let (x, y) = (crate::cmd::f64_or(p, "x", 0.0), crate::cmd::f64_or(p, "y", 0.0));
    let d = &s.doc()?.doc;
    let (frame_rect, xf, auto_fit) = if let Some(fid) = frame {
        let r = d.item(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?.inner_bounds();
        let k = (r.width() / nw).max(r.height() / nh);
        let xf = Affine::translate((r.x0 + (r.width() - nw * k) / 2.0, r.y0 + (r.height() - nh * k) / 2.0)) * Affine::scale(k);
        // Fill the frame proportionally once. Auto-Fit stays off, so resizing the frame crops.
        (r, xf, Default::default())
    } else {
        if d.spread(sr).is_none() {
            return Err(bad("place.drop", "no such spread"));
        }
        // Drag: fit proportionally; click: actual size at the point.
        let (r, k) = match rect {
            Some(r) if r.width() > designcraft_tools::PLACE_DRAG_MIN && r.height() > designcraft_tools::PLACE_DRAG_MIN => {
                let k = (r.width() / nw).min(r.height() / nh);
                (Rect::new(r.x0, r.y0, r.x0 + nw * k, r.y0 + nh * k), k)
            }
            _ => (Rect::new(x, y, x + nw, y + nh), 1.0),
        };
        (r, Affine::translate((r.x0, r.y0)) * Affine::scale(k), Default::default())
    };
    validate_placement((nw, nh), frame_rect, xf, "place.drop")?;
    let r = s.edit(|d, sel| {
        let graphic = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (nw, nh),
            xf,
            clip: None,
            auto_fit,
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        let id = if let Some(fid) = frame {
            d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?.content = graphic;
            fid
        } else {
            let id = ItemId(d.alloc());
            let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(frame_rect));
            it.object_style = d.styles.default_graphic_frame.clone();
            if let Some(e) = d.styles.object_effects(&it.object_style) {
                it.effects = e.clone();
            }
            it.content = graphic;
            d.insert_item(sr, it, None)?;
            id
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })?;
    s.loaded = None;
    s.set_tool("selection");
    Ok(r)
}

/// File › Place of an IDML or DesignCraft document: the objects of one page (`layoutPage`,
/// 1-based) grouped, top-left at x/y (default: the source position on the first page).
fn place_layout_page(s: &mut Session, p: &Value, name: &str, bytes: &[u8]) -> Result<Value> {
    const ID: &str = "file.place";
    let src = if name.to_lowercase().ends_with(".idml") {
        crate::cmd::interchange::import_idml_confined(bytes, str_param(p, "path").map(std::path::Path::new)).map_err(|e| bad(ID, e.to_string()))?
    } else {
        from_bytes(bytes)?
    };
    let n = src.page_count();
    let page = p.get("layoutPage").and_then(Value::as_u64).unwrap_or(1) as usize;
    if page == 0 || page > n {
        return Err(bad(ID, format!("the document has {n} page(s)")));
    }
    let (si, pi) = src.page_loc(page - 1).ok_or_else(|| bad(ID, "no such page"))?;
    let sp = &src.spreads[si];
    let pr = sp.pages[pi].bounds();
    let ids: Vec<ItemId> = sp.items.iter().filter(|it| !it.hidden && sp.page_at_x(it.bounds().center().x) == Some(pi)).map(|it| it.id).collect();
    if ids.is_empty() {
        return Err(bad(ID, format!("page {page} has no objects")));
    }
    let to = crate::cmd::spread_param(p, ID)?;
    let off = match (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => designcraft_geom::Vec2::new(x - pr.x0, y - pr.y0),
        _ => designcraft_geom::Vec2::new(-pr.x0, -pr.y0),
    };
    let placed = s.edit(|d, sel| {
        for sw in &src.swatches {
            if d.swatch(&sw.name).is_none() {
                d.swatches.push(sw.clone());
            }
        }
        let missing_p: Vec<_> = src.styles.paragraph.iter().filter(|ps| d.styles.para(&ps.name).is_none()).cloned().collect();
        let missing_c: Vec<_> = src.styles.character.iter().filter(|cs| d.styles.char_style(&cs.name).is_none()).cloned().collect();
        if !missing_p.is_empty() || !missing_c.is_empty() {
            let st = d.styles_mut();
            st.paragraph.extend(missing_p);
            st.character.extend(missing_c);
        }
        let new = crate::cmd::object::duplicate_from(d, &src, &ids, to, off)?;
        *sel = Selection::items(new.clone());
        Ok(new)
    })?;
    if placed.len() > 1 {
        let g = s.execute("object.group", &json!({"ids": placed.iter().map(|i| i.0).collect::<Vec<_>>()}))?;
        return Ok(json!({"id": g["id"], "items": placed.len()}));
    }
    Ok(json!({"id": placed[0].0, "items": 1}))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) mod image_place_tests {
    use designcraft_doc::{Content, ItemId};
    use designcraft_geom::Rect;
    use serde_json::{Value, json};

    use crate::Session;

    pub(in crate::cmd::file) fn image_file() -> std::io::Result<tempfile::NamedTempFile> {
        image_file_with_color(30)
    }

    fn image_file_with_color(red: u8) -> std::io::Result<tempfile::NamedTempFile> {
        let file = tempfile::Builder::new().suffix(".png").tempfile()?;
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: [red, 90, 180, 255].repeat(800) }.to_png();
        std::fs::write(file.path(), png)?;
        Ok(file)
    }

    #[test]
    fn consecutive_path_placements_create_distinct_graphics() {
        let images = [image_file_with_color(30).unwrap(), image_file_with_color(31).unwrap(), image_file_with_color(32).unwrap()];
        let mut session = Session::new();
        session.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let mut placements = Vec::new();
        let mut originals = Vec::new();

        for ((spread, x, y), image) in [(0, 0.0, 0.0), (1, 70.0, 90.0), (2, 150.0, 210.0)].into_iter().zip(&images) {
            let placed = session.execute("file.place", &json!({"path": image.path(), "spread": spread, "x": x, "y": y})).unwrap();
            let id = ItemId(placed["id"].as_u64().unwrap());
            assert!(
                placements.iter().all(|previous: &Value| previous["id"] != placed["id"]),
                "each explicit placement needs a distinct frame: {placed}"
            );
            let document = &session.doc().unwrap().doc;
            let item = document.item(id).unwrap();
            assert_eq!(item.bounds(), Rect::new(x, y, x + 40.0, y + 20.0));
            assert!(document.spreads[spread].items.iter().any(|item| item.id == id));
            originals.push(serde_json::to_value(item).unwrap());
            placements.push(placed);
        }

        let document = &session.doc().unwrap().doc;
        assert_eq!(document.spreads.iter().flat_map(|spread| &spread.items).filter(|item| matches!(item.content, Content::Graphic(_))).count(), 3);
        assert_eq!(document.assets.len(), 3);
        for (placed, original) in placements.iter().zip(originals) {
            assert_eq!(serde_json::to_value(document.item(ItemId(placed["id"].as_u64().unwrap())).unwrap()).unwrap(), original);
        }
        let links = session.execute("links.list", &json!({})).unwrap();
        assert_eq!(links.as_array().unwrap().len(), 3);
        for (placed, image) in placements.into_iter().zip(&images) {
            let row = links.as_array().unwrap().iter().find(|row| row["asset"] == placed["asset"]).unwrap();
            assert_eq!(row["path"], json!(image.path()));
            assert_eq!(row["uses"].as_array().unwrap().len(), 1);
            assert_eq!(row["uses"][0]["id"], placed["id"]);
        }
    }

    #[test]
    fn individual_placement_parameters_bypass_selected_frames() {
        let image = image_file().unwrap();
        for graphic in [false, true] {
            for (key, value, expected) in [
                ("x", 0, Rect::new(0.0, 36.0, 40.0, 56.0)),
                ("y", 0, Rect::new(36.0, 0.0, 76.0, 20.0)),
                ("spread", 0, Rect::new(36.0, 36.0, 76.0, 56.0)),
                ("width", 0, Rect::ZERO),
                ("width", 80, Rect::new(36.0, 36.0, 116.0, 76.0)),
            ] {
                let mut session = Session::new();
                session.execute("file.new", &json!({})).unwrap();
                let selected = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]})).unwrap();
                if graphic {
                    session.execute("file.place", &json!({"path": image.path()})).unwrap();
                }
                let id = ItemId(selected["id"].as_u64().unwrap());
                let original = serde_json::to_value(session.doc().unwrap().doc.item(id).unwrap()).unwrap();
                let mut params = json!({"path": image.path()});
                params[key] = json!(value);

                let result = session.execute("file.place", &params);
                if key == "width" && value == 0 {
                    assert!(result.is_err(), "zero width must be rejected, graphic={graphic}");
                    continue;
                }
                let placed = result.unwrap();

                assert_ne!(placed["id"], selected["id"], "{key}={value}, graphic={graphic}");
                let document = &session.doc().unwrap().doc;
                assert_eq!(serde_json::to_value(document.item(id).unwrap()).unwrap(), original);
                let item = document.item(ItemId(placed["id"].as_u64().unwrap())).unwrap();
                assert_eq!(item.bounds(), expected);
                let Content::Graphic(content) = &item.content else { panic!("placed frame must be graphic") };
                let [scale_x, _, _, scale_y, _, _] = content.xf.as_coeffs();
                assert!(scale_x > 0.0 && scale_y > 0.0);
            }
        }
    }

    #[test]
    fn explicit_frame_wins_over_placement_and_selection() {
        let image = image_file().unwrap();
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let target = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]})).unwrap();
        let selected = session.execute("frame.create", &json!({"rect": [300, 320, 400, 420]})).unwrap();

        let placed =
            session.execute("file.place", &json!({"path": image.path(), "frame": target["id"], "x": 0, "y": 0, "spread": 99, "width": 80})).unwrap();

        assert_eq!(placed["id"], target["id"]);
        let document = &session.doc().unwrap().doc;
        let item = document.item(ItemId(target["id"].as_u64().unwrap())).unwrap();
        assert_eq!(item.bounds(), Rect::new(100.0, 120.0, 200.0, 220.0));
        assert!(matches!(item.content, Content::Graphic(_)));
        assert!(matches!(document.item(ItemId(selected["id"].as_u64().unwrap())).unwrap().content, Content::Unassigned));
        assert_eq!(document.spreads[0].items.len(), 2);
    }

    fn assert_width_rejected(width: Value) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let image = image_file()?;
        for graphic in [false, true] {
            for explicit_frame in [false, true] {
                let mut session = Session::new();
                session.execute("file.new", &json!({}))?;
                let target = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]}))?;
                if graphic {
                    session.execute("file.place", &json!({"path": image.path()}))?;
                }
                let mut params = json!({"path": image.path(), "width": width});
                if explicit_frame {
                    params["frame"] = target["id"].clone();
                    session.execute("frame.create", &json!({"rect": [300, 320, 400, 420]}))?;
                }
                let before = session.doc()?;
                let document = before.doc.clone();
                let selection = before.selection.clone();
                let revision = before.revision;
                let assets = document.assets.len();

                let result = session.execute("file.place", &params);

                assert!(
                    matches!(&result, Err(crate::EngineError::BadParams { .. })),
                    "width={width}, graphic={graphic}, explicit_frame={explicit_frame}: expected BadParams, got {result:?}"
                );
                let error = result.err().ok_or_else(|| crate::EngineError::Other("expected placement error".into()))?;
                let crate::EngineError::BadParams { cmd, msg } = error else { return Err(error.into()) };
                assert_eq!(cmd, "file.place");
                assert!(msg.contains("width") && msg.contains("finite") && msg.contains("greater than zero"), "{msg}");
                let after = session.doc()?;
                assert!(std::sync::Arc::ptr_eq(&document, &after.doc));
                assert_eq!(after.selection, selection);
                assert_eq!(after.revision, revision);
                assert_eq!(after.doc.assets.len(), assets);
            }
        }
        Ok(())
    }

    #[test]
    fn width_rejected_when_zero() {
        assert_width_rejected(json!(0)).unwrap();
    }

    #[test]
    fn width_rejected_when_negative() {
        assert_width_rejected(json!(-80)).unwrap();
    }

    #[test]
    fn width_rejected_when_non_numeric() {
        for width in [json!("80"), json!(true), json!([80]), json!({"value": 80})] {
            assert_width_rejected(width).unwrap();
        }
    }

    fn assert_null_placement_uses_selection(keys: &[&str]) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let image = image_file()?;
        let previous_image = image_file_with_color(31)?;
        for graphic in [false, true] {
            let mut session = Session::new();
            session.execute("file.new", &json!({}))?;
            let selected = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]}))?;
            let previous = graphic.then(|| session.execute("file.place", &json!({"path": previous_image.path()}))).transpose()?;
            let mut params = json!({"path": image.path()});
            for key in keys {
                params[*key] = Value::Null;
            }

            let placed = session.execute("file.place", &params)?;

            assert_eq!(placed["id"], selected["id"], "keys={keys:?}, graphic={graphic}");
            if let Some(previous) = previous {
                assert_ne!(placed["asset"], previous["asset"]);
            }
            let document = &session.doc()?.doc;
            let id = ItemId(selected["id"].as_u64().ok_or_else(|| crate::EngineError::Other("frame.create returned no numeric id".into()))?);
            let item = document.item(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
            assert_eq!(item.bounds(), Rect::new(100.0, 120.0, 200.0, 220.0));
            assert!(matches!(&item.content, Content::Graphic(_)), "placed frame must be graphic");
            let Content::Graphic(content) = &item.content else {
                return Err(crate::EngineError::Other("placed frame must be graphic".into()).into());
            };
            assert_eq!(json!(content.asset.0), placed["asset"]);
            assert_eq!(document.spreads[0].items.len(), 1);
        }
        Ok(())
    }

    #[test]
    fn selection_used_when_x_null() {
        assert_null_placement_uses_selection(&["x"]).unwrap();
    }

    #[test]
    fn selection_used_when_y_null() {
        assert_null_placement_uses_selection(&["y"]).unwrap();
    }

    #[test]
    fn selection_used_when_spread_null() {
        assert_null_placement_uses_selection(&["spread"]).unwrap();
    }

    #[test]
    fn selection_used_when_width_null() {
        assert_null_placement_uses_selection(&["width"]).unwrap();
    }

    #[test]
    fn selection_used_when_all_placement_parameters_null() {
        assert_null_placement_uses_selection(&["x", "y", "spread", "width"]).unwrap();
    }

    #[test]
    fn implicit_placement_fills_empty_and_replaces_graphic_selection() {
        let image = image_file().unwrap();
        let previous_image = image_file_with_color(31).unwrap();
        for graphic in [false, true] {
            let mut session = Session::new();
            session.execute("file.new", &json!({})).unwrap();
            let selected = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]})).unwrap();
            let previous = graphic.then(|| session.execute("file.place", &json!({"path": previous_image.path()})).unwrap());

            let placed = session.execute("file.place", &json!({"path": image.path()})).unwrap();

            assert_eq!(placed["id"], selected["id"]);
            if let Some(previous) = previous {
                assert_ne!(placed["asset"], previous["asset"]);
            }
            let document = &session.doc().unwrap().doc;
            let item = document.item(ItemId(placed["id"].as_u64().unwrap())).unwrap();
            assert_eq!(item.bounds(), Rect::new(100.0, 120.0, 200.0, 220.0));
            let Content::Graphic(content) = &item.content else { panic!("placed frame must be graphic") };
            assert_eq!(json!(content.asset.0), placed["asset"]);
            assert_eq!(document.spreads[0].items.len(), 1);
        }
    }
}

#[cfg(test)]
mod pdf_crop_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_pdf_cropped_to_its_trim_box() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
        let b64 = s.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap().to_string();
        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        let size = |t: &mut Session, crop: &str| {
            t.execute("edit.deselectAll", &json!({})).unwrap();
            let r = t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": crop, "width": 630, "x": 0, "y": 0})).unwrap();
            let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
            t.doc().unwrap().doc.item(id).unwrap().bounds()
        };
        let full = size(&mut t, "crop");
        let trim = size(&mut t, "trim");
        assert!((full.width() - 630.0).abs() < 1e-6, "{full:?}");
        assert!((trim.width() - 612.0).abs() < 1e-3 && (trim.height() - 792.0).abs() < 1e-3, "{trim:?}");
        assert!(t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": "nope"})).is_err());
    }

    /// Placing with `trim` stores the choice; the graphic spans the trim box and draws only it,
    /// even in a frame larger than the graphic.
    #[test]
    fn a_pdf_placed_with_its_trim_box_stores_and_draws_that_box() {
        // Letter with a 9 pt bleed: black in the bleed's top-left corner and a black square inside.
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
        for rect in [[-9, -9, 0, 0], [10, 10, 30, 30]] {
            let id = s.execute("frame.create", &json!({"rect": rect})).unwrap()["id"].clone();
            s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
        }
        let b64 = s.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap().to_string();

        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        // `width` is the whole page's (630 pt with its bleed): half size.
        let r = t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": "trim", "width": 315, "x": 100, "y": 100})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let d = t.doc().unwrap().doc.clone();
        let it = d.item(id).unwrap();
        let g = it.graphic().unwrap().clone();
        let a = &d.assets[&g.asset];
        assert_eq!(a.pdf_crop, designcraft_doc::PdfCrop::Trim);
        let b = a.shown_box().unwrap();
        let want = [9.0 / 630.0, 9.0 / 810.0, 621.0 / 630.0, 801.0 / 810.0];
        assert!(b.iter().zip(want).all(|(x, y)| (x - y).abs() < 1e-4), "{b:?}");
        assert!((g.size.0 - 612.0).abs() < 1e-3 && (g.size.1 - 792.0).abs() < 1e-3, "{:?}", g.size);
        let shown = (it.xf * g.xf).transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
        let fb = it.bounds();
        for (x, y) in [(shown.x0, 100.0), (shown.y0, 100.0), (shown.x1, 406.0), (shown.y1, 496.0), (fb.x1, 406.0), (fb.y1, 496.0)] {
            assert!((x - y).abs() < 1e-3, "{shown:?} {fb:?}");
        }

        // A frame larger than the graphic still shows only the trim box.
        t.edit(|d, _| {
            d.item_mut(id).unwrap().path = designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(90.0, 90.0, 416.0, 506.0));
            Ok(())
        })
        .unwrap();
        let d = t.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &t.cache, 0, 1.0, false, &Default::default()).unwrap();
        let dark = |x: u32, y: u32| img.pixel(x, y)[0] < 60;
        // The page sits at 95.5: its bleed corner (95.5–100) is cut off, the square (105–115) shows
        // where it is on the page.
        assert!(!dark(97, 97), "the bleed is outside the trim box");
        assert!(!dark(103, 103), "the page is offset by its bleed");
        assert!(dark(110, 110), "the square inside the trim box");
    }

    #[test]
    fn placed_pdf_keeps_its_exact_page_size() {
        // A 100.4 x 50.4 pt page placed 100.4 pt wide is 50.4 pt tall, not stretched to whole points.
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100.4, "height": 50.4, "pages": 1})).unwrap();
        let b64 = s.execute("file.exportPdf", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        let r = t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "width": 100.4, "x": 0, "y": 0})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let d = &t.doc().unwrap().doc;
        let it = d.item(id).unwrap();
        let b = it.bounds();
        assert!((b.width() - 100.4).abs() < 1e-3 && (b.height() - 50.4).abs() < 1e-3, "{b:?}");
        let designcraft_doc::Content::Graphic(g) = &it.content else { panic!("not a graphic") };
        assert!((g.size.0 - 100.4).abs() < 1e-3 && (g.size.1 - 50.4).abs() < 1e-3, "{:?}", g.size);
    }
}

#[cfg(test)]
mod layout_place_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_a_page_of_an_idml_file_as_a_group() {
        let mut a = Session::new();
        a.execute("file.new", &json!({"pages": 2})).unwrap();
        a.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "text", "text": "From page one", "caret": false})).unwrap();
        a.execute("frame.create", &json!({"rect": [300, 400, 350, 450]})).unwrap();
        let idml = designcraft_idml::export_idml(&a.doc().unwrap().doc);
        let b64 = super::base64_encode(&idml);
        let mut b = Session::new();
        b.execute("file.new", &json!({})).unwrap();
        let r = b.execute("file.place", &json!({"base64": b64, "name": "src.idml", "layoutPage": 1, "x": 0, "y": 0})).unwrap();
        assert_eq!(r["items"], 2);
        let d = &b.doc().unwrap().doc;
        let g = d.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap();
        assert_eq!(g.children().len(), 2, "one group");
        let bb = g.bounds();
        assert!((bb.x0 - 100.0).abs() < 1e-6 && (bb.y0 - 100.0).abs() < 1e-6, "page top-left at 0,0: {bb:?}");
        assert!(d.stories.values().any(|st| st.text == "From page one"), "stories come along");
        assert!(b.execute("file.place", &json!({"base64": b64, "name": "src.idml", "layoutPage": 2})).is_err(), "page 2 is empty");
    }
}
