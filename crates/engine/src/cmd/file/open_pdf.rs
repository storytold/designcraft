//! Open a PDF as a DesignCraft document of editable paths and images (File › Open on a `.pdf`).

use std::sync::Arc;

use designcraft_doc::build::NewDocument;
use designcraft_doc::{Asset, AssetId, Content, Fill, Graphic, Item, ItemId, Margins, Selection, Shape, Stroke, StrokeType, SpreadRef};
use designcraft_geom::{Affine, PathData, Point, Rect, Shape as GeomShape, shapes};
use designcraft_render::{PdfArtCap, PdfArtImage, PdfArtJoin, PdfArtOp, PdfArtPaint, PdfArtPath, PdfArtStroke, PdfPageArt};
use serde_json::{Value, json};

use super::{DocState, Document, Result, Session, bad, str_param};
use crate::cmd::{EngineError, base64_decode, id_param};

const ID: &str = "file.openPdf";

/// Open a PDF as a new document of editable vectors (and placed images where the PDF draws them).
pub(super) fn file_open_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name) = read_pdf(p)?;
    designcraft_render::check_pdf_input_size(bytes.len()).map_err(|e| bad(ID, e.to_string()))?;
    if !designcraft_render::is_pdf(&bytes) {
        return Err(bad(ID, "not a PDF file"));
    }
    let pages = designcraft_render::pdf_document_art(&bytes).map_err(|e| bad(ID, e.to_string()))?;
    let page_count = pages.len();
    let Some(first) = pages.first() else {
        return Err(bad(ID, "the PDF has no pages"));
    };
    let title = stem_title(&name);
    let first_w = first.width.max(1.0);
    let first_h = first.height.max(1.0);
    let nd = NewDocument {
        title: title.clone(),
        width: first_w,
        height: first_h,
        pages: page_count.max(1),
        facing_pages: false,
        margins: Margins { top: 0.0, bottom: 0.0, inside: 0.0, outside: 0.0 },
        columns: 1,
        gutter: 0.0,
        ..NewDocument::default()
    };
    let mut d = Document::new(&nd);
    let layer = d.default_layer();
    let mut warnings = Vec::new();
    let mut total_ids = Vec::new();
    for (i, art) in pages.into_iter().enumerate() {
        if (art.width - first_w).abs() > 0.5 || (art.height - first_h).abs() > 0.5 {
            warnings.push(format!(
                "page {}: size {:.1}×{:.1} pt differs from the first page ({:.1}×{:.1}); content is placed top-left on the document page",
                i + 1,
                art.width,
                art.height,
                first_w,
                first_h
            ));
        }
        warnings.extend(art.warnings.iter().cloned());
        let origin = d.page(i).map(|pg| Point::new(pg.bounds().x0, pg.bounds().y0)).unwrap_or(Point::ZERO);
        let shift = Affine::translate((origin.x, origin.y));
        let spread = SpreadRef::Doc(d.page_loc(i).map(|(si, _)| si).unwrap_or(0));
        let ids = place_page_art(&mut d, &art, layer, shift, spread)?;
        total_ids.extend(ids);
    }
    let mut st = DocState::new(d, None);
    // Leave nothing selected: selecting every converted path draws blue edges over the art and
    // looks like a layering bug. The Objects / Layers panel still lists them in paint order.
    st.selection = Selection::default();
    let index = s.add_document(st);
    if let Some(path) = str_param(p, "path") {
        s.prefs.note_recent_file(path);
    }
    Ok(json!({
        "index": index,
        "title": title,
        "pages": page_count,
        "ids": total_ids.iter().map(|i| i.0).collect::<Vec<_>>(),
        "warnings": warnings,
    }))
}

/// Insert editable PDF art into `d` on `spread`, transforming page-space geometry by `shift`.
/// Objects are inserted in PDF paint order (first = back, last = front).
pub(crate) fn place_page_art(
    d: &mut Document,
    art: &PdfPageArt,
    layer: designcraft_doc::LayerId,
    shift: Affine,
    spread: SpreadRef,
) -> Result<Vec<ItemId>> {
    let mut ids = Vec::new();
    let mut image_index = 0usize;
    for op in &art.ops {
        match op {
            PdfArtOp::Path(path) => {
                if let Some(it) = path_item(d, path, layer, shift) {
                    ids.push(it.id);
                    d.insert_item(spread, it, None)?;
                }
            }
            PdfArtOp::Image(image) => {
                let it = image_item(d, image, layer, shift, image_index)?;
                image_index = image_index.saturating_add(1);
                ids.push(it.id);
                d.insert_item(spread, it, None)?;
            }
        }
    }
    Ok(ids)
}

fn path_item(d: &mut Document, art: &PdfArtPath, layer: designcraft_doc::LayerId, shift: Affine) -> Option<Item> {
    let data = PathData::from_bezpath(&(shift * art.path.clone()));
    if data.subpaths.is_empty() {
        return None;
    }
    let anchors: usize = data.subpaths.iter().map(|s| s.anchors.len()).sum();
    let straight = |a: &designcraft_geom::Anchor| a.h_in == a.p && a.h_out == a.p;
    let line = data.subpaths.len() == 1 && anchors == 2 && data.subpaths.iter().all(|s| !s.closed && s.anchors.iter().all(straight));
    let paint_opacity = art.fill.map(|f| f.opacity).or(art.stroke.as_ref().map(|(c, _)| c.opacity)).unwrap_or(1.0);
    let mut it = Item::new(ItemId(d.alloc()), layer, if line { Shape::GraphicLine } else { Shape::Path }, data);
    it.fill = match art.fill {
        Some(f) => Fill::swatch(&swatch_for(d, f.rgb)),
        None => Fill::none(),
    };
    it.stroke = match &art.stroke {
        Some((c, s)) => stroke_from(d, *c, s),
        None => Stroke::none(),
    };
    it.opacity = (art.opacity * paint_opacity).clamp(0.0, 1.0);
    if art.even_odd {
        // PDF `f*` / compound paths with holes (e.g. a white page matte around artwork).
        it.path.fill_rule = designcraft_geom::FillRule::EvenOdd;
    }
    Some(it)
}

fn stroke_from(d: &mut Document, paint: PdfArtPaint, s: &PdfArtStroke) -> Stroke {
    Stroke {
        swatch: swatch_for(d, paint.rgb),
        weight: s.width,
        cap: match s.cap {
            PdfArtCap::Butt => designcraft_doc::Cap::Butt,
            PdfArtCap::Round => designcraft_doc::Cap::Round,
            PdfArtCap::Square => designcraft_doc::Cap::Projecting,
        },
        join: match s.join {
            PdfArtJoin::Miter => designcraft_doc::Join::Miter,
            PdfArtJoin::Round => designcraft_doc::Join::Round,
            PdfArtJoin::Bevel => designcraft_doc::Join::Bevel,
        },
        miter_limit: s.miter,
        kind: s.dashes.clone().map_or(StrokeType::Solid, |pattern| StrokeType::Dashed { pattern }),
        ..Stroke::default()
    }
}

fn image_item(d: &mut Document, image: &PdfArtImage, layer: designcraft_doc::LayerId, shift: Affine, index: usize) -> Result<Item> {
    let (bw, bh) = (f64::from(image.width), f64::from(image.height));
    if !(bw > 0.0 && bh > 0.0) {
        return Err(bad(ID, "PDF image has no size"));
    }
    let xf = shift * image.xf;
    let r = GeomShape::bounding_box(&(xf * Rect::new(0.0, 0.0, bw, bh).to_path(0.1)));
    if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) {
        return Err(bad(ID, "PDF image transform is not finite"));
    }
    let aid = AssetId(d.alloc());
    let aid = d.add_asset(Asset {
        id: aid,
        name: format!("pdf-image-{index}.png"),
        mime: "image/png".into(),
        data: Arc::new(image.png.clone()),
        pixels: Some((image.width, image.height)),
        ..Default::default()
    });
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, layer, Shape::Rectangle, shapes::rectangle(r));
    it.object_style = d.styles.default_graphic_frame.clone();
    if let Some(e) = d.styles.object_effects(&it.object_style) {
        it.effects = e.clone();
    }
    it.stroke = Stroke::none();
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
    Ok(it)
}

fn swatch_for(d: &mut Document, rgb: [u8; 3]) -> String {
    use designcraft_color::swatch::{Swatch, SwatchValue};
    let color = designcraft_color::Color::rgb8(rgb[0], rgb[1], rgb[2]);
    if let Some(w) = d.swatches.iter().find(|w| matches!(&w.value, SwatchValue::Color { color: c, .. } if *c == color)) {
        return w.name.clone();
    }
    let base = format!("R={} G={} B={}", rgb[0], rgb[1], rgb[2]);
    let name = designcraft_doc::Styles::unique_name(|n| d.swatch(n).is_some(), &base);
    d.swatches.push(Swatch {
        name: name.clone(),
        value: SwatchValue::Color { color, color_type: designcraft_color::ColorType::Process },
        locked: false,
        named: false,
        hidden: true,
    });
    name
}

fn read_pdf(p: &Value) -> Result<(Vec<u8>, String)> {
    if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes = crate::io::read_file(path).map_err(EngineError::Other)?;
            let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document.pdf".into());
            return Ok((bytes, name));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err(EngineError::Other("use base64 on the web".into()));
        }
    }
    let b64 = str_param(p, "base64").ok_or_else(|| bad(ID, "missing path or base64"))?;
    let bytes = base64_decode(b64);
    let name = str_param(p, "name").unwrap_or("document.pdf").to_string();
    Ok((bytes, name))
}

fn stem_title(name: &str) -> String {
    let path = std::path::Path::new(name);
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "Untitled".into())
}

/// Convert a placed PDF graphic in the selection into editable paths on the same spread.
pub(crate) fn convert_placed_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    const CID: &str = "object.convertPdf";
    let st = s.doc()?;
    let ids: Vec<ItemId> = if let Some(id) = id_param(p, "id") {
        vec![id]
    } else {
        st.selection.items.clone()
    };
    if ids.is_empty() {
        return Err(bad(CID, "nothing selected"));
    }
    let layer = st.active_layer;
    s.edit(|d, sel| {
        let mut out = Vec::new();
        let mut warnings = Vec::new();
        let mut remove = Vec::new();
        for id in ids {
            let Some(it) = d.item(id) else {
                continue;
            };
            let Some(g) = it.graphic() else {
                continue;
            };
            let Some(asset) = d.assets.get(&g.asset) else {
                continue;
            };
            if !designcraft_render::is_pdf(&asset.data) {
                continue;
            }
            let page = asset.page as usize;
            let art = designcraft_render::pdf_page_art(&asset.data, page).map_err(|e| bad(CID, e.to_string()))?;
            warnings.extend(art.warnings.iter().cloned());
            let shift = g.xf;
            let spread = d.find(id).map(|loc| loc.spread).unwrap_or(SpreadRef::Doc(0));
            let new_ids = place_page_art(d, &art, layer, shift, spread)?;
            out.extend(new_ids);
            remove.push(id);
        }
        if out.is_empty() {
            return Err(bad(CID, "no placed PDF in the selection"));
        }
        for id in remove {
            let _ = d.remove_item(id);
        }
        *sel = Selection::items(out.clone());
        Ok(json!({"ids": out.iter().map(|i| i.0).collect::<Vec<_>>(), "warnings": warnings}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;

    fn pdf_rect() -> Vec<u8> {
        let content = "1 0 0 rg 10 20 30 40 re f";
        let bodies = [
            b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
            b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
            b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Contents 4 0 R>>".to_vec(),
            {
                let mut s = format!("<< /Length {}>>\nstream\n", content.len()).into_bytes();
                s.extend_from_slice(content.as_bytes());
                s.extend_from_slice(b"\nendstream");
                s
            },
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", bodies.len() + 1).as_bytes());
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", bodies.len() + 1).as_bytes());
        out
    }

    #[test]
    fn open_pdf_creates_editable_paths() {
        let mut s = Session::new();
        let b64 = crate::cmd::base64_encode(&pdf_rect());
        let r = s.execute("file.openPdf", &json!({"base64": b64, "name": "box.pdf"})).unwrap();
        assert_eq!(r["title"], "box");
        assert_eq!(r["pages"], 1);
        let d = s.doc().unwrap();
        assert!((d.doc.settings.page_width - 200.0).abs() < 0.01);
        assert!((d.doc.settings.page_height - 100.0).abs() < 0.01);
        let items = d.doc.all_items();
        assert!(!items.is_empty(), "{r}");
        assert!(items.iter().any(|&id| {
            d.doc.item(id).is_some_and(|it| matches!(it.shape, Shape::Path | Shape::Rectangle | Shape::GraphicLine))
        }));
    }

    #[test]
    fn convert_placed_pdf_replaces_graphic_with_paths() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200.0, "height": 100.0, "facingPages": false})).unwrap();
        let b64 = crate::cmd::base64_encode(&pdf_rect());
        let placed = s.execute("file.place", &json!({"base64": b64, "name": "box.pdf", "x": 0, "y": 0, "width": 200})).unwrap();
        let id = ItemId(placed["id"].as_u64().unwrap());
        let r = s.execute("object.convertPdf", &json!({"id": id.0})).unwrap();
        assert!(r["ids"].as_array().unwrap().len() >= 1, "{r}");
        assert!(s.doc().unwrap().doc.item(id).is_none(), "placed graphic removed");
    }
}
