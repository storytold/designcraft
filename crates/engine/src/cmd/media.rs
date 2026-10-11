//! Window › Interactive › Media: placed video and sound (shown as a poster or a placeholder,
//! exported as `<video>` / `<audio>` in EPUB).

use std::sync::Arc;

use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, MediaOptions, Shape};
use designcraft_geom::{Affine, Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_selection, ok, targets};
use crate::{Result, Session};

pub(crate) fn place_media(s: &mut Session, p: &Value, name: String, mime: &str, bytes: Vec<u8>, link: Option<String>) -> Result<Value> {
    let video = mime.starts_with("video/");
    let (w, h) = (
        p.get("width").and_then(Value::as_f64).unwrap_or(if video { 320.0 } else { 200.0 }),
        p.get("height").and_then(Value::as_f64).unwrap_or(if video { 180.0 } else { 48.0 }),
    );
    let target = super::id_param(p, "frame").or_else(|| {
        let st = s.active()?;
        st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| matches!(it.content, Content::Unassigned | Content::Graphic(_))))
    });
    let spread = super::spread_param(p, "spread");
    let lid = s.doc()?.active_layer;
    let (px, py) = (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64));
    let mime = mime.to_string();
    s.edit(|d, sel| {
        let fresh = AssetId(d.alloc());
        let aid = d.add_asset(Asset { page: 0, id: fresh, name, mime, link, data: Arc::new(bytes), pixels: None });
        let id = match target {
            Some(fid) => {
                let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
                let r = it.inner_bounds();
                it.content = Content::Graphic(Graphic {
                    asset: aid,
                    size: (r.width(), r.height()),
                    xf: Affine::translate((r.x0, r.y0)),
                    auto_fit: Default::default(),
                    fit_align: 4,
                    crop: [0.0; 4],
                });
                it.media = Some(MediaOptions { controls: true, ..Default::default() });
                fid
            }
            None => {
                let pr = d.spread(spread).and_then(|sp| sp.pages.first()).map(|pg| pg.margin_rect()).unwrap_or(Rect::new(36.0, 36.0, 300.0, 300.0));
                let (x, y) = (px.unwrap_or(pr.x0), py.unwrap_or(pr.y0));
                let id = ItemId(d.alloc());
                let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(Rect::new(x, y, x + w, y + h)));
                it.content = Content::Graphic(Graphic {
                    asset: aid,
                    size: (w, h),
                    xf: Affine::translate((x, y)),
                    auto_fit: Default::default(),
                    fit_align: 4,
                    crop: [0.0; 4],
                });
                it.media = Some(MediaOptions { controls: true, ..Default::default() });
                d.insert_item(spread, it, None)?;
                id
            }
        };
        sel.items = vec![id];
        sel.text = None;
        Ok(json!({"id": id.0, "asset": aid.0, "media": true}))
    })
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "media.options",
            "Media Options",
            [],
            None,
            "{ids?, playOnPageLoad?, loop?, controls?, poster?: {path | base64+name} | null (placeholder)} — for placed video and sound",
            has_selection,
            |s, p| {
                let ids = targets(s, p)?;
                // A poster image is read before the edit.
                let poster = match p.get("poster") {
                    Some(Value::Object(_)) => {
                        let (bytes, name, link) = super::file::read_source(&p["poster"])?;
                        let px = designcraft_render::image_size(&bytes).ok_or_else(|| bad("media.options", "the poster isn't a readable image"))?;
                        Some(Some((bytes, name, link, px)))
                    }
                    Some(Value::Null) => Some(None),
                    _ => None,
                };
                s.edit(|d, _| {
                    let poster_id = match poster {
                        Some(Some((bytes, name, link, px))) => {
                            let fresh = AssetId(d.alloc());
                            let mime = designcraft_render::image_mime(&bytes).to_string();
                            Some(Some(d.add_asset(Asset { page: 0, id: fresh, name, mime, link, data: Arc::new(bytes), pixels: Some(px) })))
                        }
                        Some(None) => Some(None),
                        None => None,
                    };
                    let mut n = 0;
                    for id in &ids {
                        let Some(it) = d.item_mut(*id) else { continue };
                        let Some(m) = &mut it.media else { continue };
                        if let Some(v) = p.get("playOnPageLoad").and_then(Value::as_bool) {
                            m.play_on_page_load = v;
                        }
                        if let Some(v) = p.get("loop").and_then(Value::as_bool) {
                            m.looping = v;
                        }
                        if let Some(v) = p.get("controls").and_then(Value::as_bool) {
                            m.controls = v;
                        }
                        if let Some(ps) = poster_id {
                            m.poster = ps;
                        }
                        n += 1;
                    }
                    if n == 0 {
                        return Err(bad("media.options", "select a video or sound"));
                    }
                    ok()
                })
            }
        ),
        cmd!(query "media.get", "Media", [], None, "{id?} → {kind, name, playOnPageLoad, loop, controls, poster}", has_selection, |s, p| {
            let id = super::id_param(p, "id").or_else(|| s.active().and_then(|d| d.selection.items.first().copied())).ok_or_else(|| bad("media.get", "no selection"))?;
            let d = &s.doc()?.doc;
            let it = d.item(id).ok_or_else(|| bad("media.get", "no such object"))?;
            let (Some(m), Content::Graphic(g)) = (&it.media, &it.content) else { return Err(bad("media.get", "not a video or sound")) };
            let a = d.assets.get(&g.asset);
            Ok(json!({"kind": a.and_then(|a| designcraft_doc::media_kind(&a.mime)), "name": a.map(|a| a.name.clone()), "playOnPageLoad": m.play_on_page_load,
                "loop": m.looping, "controls": m.controls, "poster": m.poster.map(|p| p.0)}))
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn media_frames_place_render_and_export() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let fake = super::super::file::base64_encode(b"\x00\x00\x00\x18ftypmp42");
        let r = s.execute("file.place", &json!({"base64": fake, "name": "clip.mp4", "x": 100, "y": 100})).unwrap();
        let id = r["id"].clone();
        let g = s.execute("media.get", &json!({"id": id})).unwrap();
        assert_eq!(g["kind"], "video");
        assert_eq!(g["controls"], true);
        // The placeholder is dark with a light play mark.
        let d = s.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        assert!(img.pixel(110, 110)[0] < 80, "dark frame");
        assert!(img.pixel(260, 190)[0] > 200, "play mark");
        // A poster replaces the placeholder.
        let px: Vec<u8> = (0..8 * 8).flat_map(|_| [255, 0, 0, 255]).collect();
        let png = designcraft_render::Rendered { width: 8, height: 8, pixels: px }.to_png();
        s.execute(
            "media.options",
            &json!({"ids": [id], "loop": true, "poster": {"base64": super::super::file::base64_encode(&png), "name": "p.png"}}),
        )
        .unwrap();
        let d = s.doc().unwrap().doc.clone();
        let img = rr.render_page(&d, &s.cache, 0, 1.0, false, &Default::default()).unwrap();
        let p = img.pixel(110, 110);
        assert!(p[0] > 200 && p[1] < 60, "poster: {p:?}");
        // PDF export draws it; interactive PDF embeds the video and plays it in a screen annotation.
        let pdf = |s: &mut Session, media: bool| {
            let r = s.execute("file.exportPdf", &json!({"media": media})).unwrap();
            super::super::file::base64_decode(r["base64"].as_str().unwrap())
        };
        let plain = String::from_utf8_lossy(&pdf(&mut s, false)).to_string();
        assert!(!plain.contains("/Subtype/Screen"));
        let rich = pdf(&mut s, true);
        let t = String::from_utf8_lossy(&rich);
        assert!(
            t.contains("/Subtype/Screen") && t.contains("/S/Rendition") && t.contains("/CT(video/mp4)") && t.contains("/RC 0"),
            "screen + rendition"
        );
        assert!(t.contains("/Type/EmbeddedFile/Subtype/video#2Fmp4") && t.contains("ftypmp42"), "the file is embedded");
        assert!(!t.contains("/AcroForm"), "no form without fields");
        // Still a readable PDF.
        assert_eq!(hayro_syntax::Pdf::new(rich.clone()).expect("valid PDF").pages().len(), 1);
        // EPUB carries the video.
        let e = s.execute("file.exportEpub", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(e["base64"].as_str().unwrap());
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut read = |name: &str| {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
            s
        };
        assert!(read("OEBPS/content.opf").contains("video/mp4"), "the video is in the package");
        let x = read("OEBPS/content.xhtml");
        assert!(x.contains("<video src=") && x.contains("loop=") && x.contains("poster="), "{x}");
    }

    #[test]
    fn eps_places_at_its_bounding_box() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 10 10 154 82\nnewpath\n%%EOF\n";
        let r = s.execute("file.place", &json!({"base64": super::super::file::base64_encode(eps), "name": "logo.eps", "x": 72, "y": 72})).unwrap();
        let d = s.doc().unwrap().doc.clone();
        let b = d.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().bounds();
        assert!((b.width() - 144.0).abs() < 1e-6 && (b.height() - 72.0).abs() < 1e-6, "{b:?}");
        // Renders (the placeholder) and exports.
        assert!(s.execute("file.exportPdf", &json!({})).is_ok());
    }
}
