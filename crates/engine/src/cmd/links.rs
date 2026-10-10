//! Window › Links: placed graphics with their status, effective resolution and uses; relink,
//! update, embed, go to.
//!
//! Every placed graphic keeps its bytes in the document; a linked graphic also remembers its
//! file. Its status compares that file with the copy in the document: missing, modified (the
//! file's bytes differ) or OK.

use std::sync::Arc;

use designcraft_doc::{AssetId, Content, Document, ItemId, Selection};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "links.list", "Links", [], None, "{} → [{asset, name, path, status: ok|modified|missing|embedded, pixels, uses: [{id, page, ppi}]}]", has_doc, |s, _| {
            Ok(Value::Array(list(&s.doc()?.doc)))
        }),
        cmd!(
            "links.relink",
            "Relink…",
            ["Window", "Links"],
            None,
            "{asset, path} — use another file for every placement of the graphic; keeps the selected PDF page (error if missing)",
            has_doc,
            relink
        ),
        cmd!(
            "links.relinkFolder",
            "Relink to Folder…",
            ["Window", "Links"],
            None,
            "{dir?: folder to look in (default: each link's own folder), extension?: e.g. \"tif\" (Relink File Extension), assets?: [ids], missingOnly?: bool (default true with `dir`)} → {relinked, notFound: [names]}",
            has_doc,
            relink_folder
        ),
        cmd!(
            "links.update",
            "Update Link",
            ["Window", "Links"],
            None,
            "{asset?} — reload modified links from their files (all when no asset)",
            has_doc,
            update
        ),
        cmd!("links.embed", "Embed Link", ["Window", "Links"], None, "{asset} — keep only the copy in the document", has_doc, |s, p| {
            let aid = asset_param(p, "links.embed")?;
            s.edit(|d, _| {
                let a = d.assets.get_mut(&aid).ok_or_else(|| bad("links.embed", "no such graphic"))?;
                Arc::make_mut(a).link = None;
                Ok(Value::Null)
            })
        }),
        cmd!(noundo "links.goTo", "Go To Link", ["Window", "Links"], None, "{asset} — selects the frames showing the graphic", has_doc, |s, p| {
            let aid = asset_param(p, "links.goTo")?;
            let st = s.doc_mut()?;
            let ids = uses(&st.doc, aid);
            if ids.is_empty() {
                return Err(bad("links.goTo", "the graphic isn't placed"));
            }
            st.selection = Selection::items(ids.clone());
            st.revision += 1;
            Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
        }),
    ]
}

fn asset_param(p: &Value, cmd: &str) -> Result<AssetId> {
    p.get("asset").and_then(Value::as_u64).map(AssetId).ok_or_else(|| bad(cmd, "missing asset"))
}

/// Frames (graphic content) showing an asset, top-level or in groups.
fn uses(d: &Document, aid: AssetId) -> Vec<ItemId> {
    let mut out = Vec::new();
    for sp in d.spreads.iter().chain(&d.parents) {
        for it in &sp.items {
            it.walk(&mut |i| {
                if let Content::Graphic(g) = &i.content
                    && g.asset == aid
                {
                    out.push(i.id);
                }
            });
        }
    }
    out
}

/// Link status of an asset.
pub fn status(a: &designcraft_doc::Asset) -> &'static str {
    let Some(path) = &a.link else { return "embedded" };
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::fs::metadata(path) {
            Err(_) => "missing",
            Ok(m) if m.len() as usize != a.data.len() => "modified",
            // Same size: compare the bytes (cheap for the sizes involved, and exact).
            Ok(_) => match std::fs::read(path) {
                Ok(b) if b == *a.data => "ok",
                Ok(_) => "modified",
                Err(_) => "missing",
            },
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        "ok"
    }
}

fn list(d: &Document) -> Vec<Value> {
    d.assets
        .values()
        .map(|a| {
            let uses: Vec<Value> = uses(d, a.id)
                .into_iter()
                .map(|id| {
                    // Effective resolution: image pixels over the size it is shown at.
                    let ppi = (|| {
                        let loc = d.find(id)?;
                        let it = d.item_at(&loc)?;
                        let Content::Graphic(g) = &it.content else { return None };
                        // Vector (placed PDF or SVG): no effective resolution.
                        if a.mime == "application/pdf" || a.mime == "image/svg+xml" {
                            return None;
                        }
                        let (pw, _) = a.pixels?;
                        let xf = d.parent_xf(&loc) * it.xf * g.xf;
                        let shown_w = xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1)).width();
                        (shown_w > 0.0).then(|| (pw as f64 / (shown_w / 72.0)).round())
                    })();
                    let page = d.page_of_item(id).map(|p| d.page_name(p));
                    json!({"id": id.0, "page": page, "ppi": ppi})
                })
                .collect();
            json!({"asset": a.id.0, "name": a.name, "path": a.link, "status": status(a), "pixels": a.pixels, "uses": uses})
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn reload(path: &str) -> Result<(Vec<u8>, (u32, u32))> {
    let b = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let px = designcraft_render::image_size(&b).ok_or_else(|| bad("links", format!("{path}: unsupported or corrupt image")))?;
    Ok((b, px))
}

#[cfg(target_arch = "wasm32")]
fn reload(path: &str) -> Result<(Vec<u8>, (u32, u32))> {
    Err(EngineError::Other(format!("{path}: files can't be read on the web")))
}

fn relinked_asset(a: &designcraft_doc::Asset, path: &str) -> Result<Arc<designcraft_doc::Asset>> {
    let (bytes, px) = reload(path)?;
    let page = if designcraft_render::is_pdf(&bytes) { a.page } else { 0 };
    let px = if page > 0 {
        designcraft_render::pdf_page_size(&bytes, page as usize)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
            .ok_or_else(|| bad("links", format!("{path}: can't read PDF page {}", u64::from(page) + 1)))?
    } else {
        px
    };
    let mut a = a.clone();
    a.mime = designcraft_render::image_mime(&bytes).to_string();
    a.data = Arc::new(bytes);
    a.pixels = Some(px);
    a.page = page;
    a.name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    a.link = Some(path.to_string());
    Ok(Arc::new(a))
}

fn relink(s: &mut Session, p: &Value) -> Result<Value> {
    let aid = asset_param(p, "links.relink")?;
    let path = str_param(p, "path").ok_or_else(|| bad("links.relink", "missing path"))?;
    let a = s.doc()?.doc.assets.get(&aid).ok_or_else(|| bad("links.relink", "no such graphic"))?;
    let fresh = relinked_asset(a, path)?;
    s.edit(|d, _| {
        let name = fresh.name.clone();
        d.assets.insert(aid, fresh);
        Ok(json!({"name": name}))
    })
}

fn relink_folder(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").map(std::path::PathBuf::from);
    let ext = str_param(p, "extension").map(|e| e.trim_start_matches('.').to_string());
    if ext.as_ref().is_some_and(|e| e.contains(['/', '\\', '\0'])) {
        return Err(bad("links.relinkFolder", "extension must not contain path separators or NUL"));
    }
    if dir.is_none() && ext.is_none() {
        return Err(bad("links.relinkFolder", "give `dir` and/or `extension`"));
    }
    let missing_only = p.get("missingOnly").and_then(Value::as_bool).unwrap_or(dir.is_some() && ext.is_none());
    let only: Option<Vec<u64>> = p.get("assets").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect());
    let d = &s.doc()?.doc;
    let mut todo: Vec<(AssetId, std::path::PathBuf)> = Vec::new();
    let mut not_found = Vec::new();
    for a in d.assets.values() {
        if only.as_ref().is_some_and(|o| !o.contains(&a.id.0)) || (missing_only && status(a) != "missing") {
            continue;
        }
        let old = std::path::PathBuf::from(a.link.clone().unwrap_or_else(|| a.name.clone()));
        let mut name = old.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| a.name.clone().into());
        if let Some(e) = &ext {
            name = std::path::Path::new(&name).with_extension(e).into_os_string();
        }
        let folder = dir.clone().or_else(|| old.parent().map(|p| p.to_path_buf())).unwrap_or_default();
        let cand = folder.join(&name);
        if cand.exists() {
            todo.push((a.id, cand));
        } else {
            not_found.push(name.to_string_lossy().to_string());
        }
    }
    // Load every replacement before committing any of them: a bad file must not leave an
    // unrecorded partial edit. Successful batches commit once and have one undo step.
    let mut fresh = Vec::new();
    for (aid, path) in todo {
        let a = d.assets.get(&aid).ok_or_else(|| bad("links.relinkFolder", "no such graphic"))?;
        fresh.push((aid, relinked_asset(a, &path.to_string_lossy())?));
    }
    let n = fresh.len();
    if n > 0 {
        s.edit(|d, _| {
            for (aid, a) in fresh {
                d.assets.insert(aid, a);
            }
            Ok(())
        })?;
    }
    Ok(json!({"relinked": n, "notFound": not_found}))
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let mut fresh = Vec::new();
    for a in d.assets.values().filter(|a| p.get("asset").and_then(Value::as_u64).is_none_or(|x| x == a.id.0)).filter(|a| status(a) == "modified") {
        if let Some(path) = &a.link {
            fresh.push((a.id, relinked_asset(a, path)?));
        }
    }
    let n = fresh.len();
    if n == 0 {
        return Ok(json!({"updated": 0}));
    }
    s.edit(|d, _| {
        for (aid, a) in fresh {
            d.assets.insert(aid, a);
        }
        Ok(json!({"updated": n}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        designcraft_render::Rendered { width: w, height: h, pixels: vec![200u8; (w * h * 4) as usize] }.to_png()
    }

    fn pdf(sizes: &[(u32, u32)]) -> Vec<u8> {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": sizes.len(), "facingPages": false})).unwrap();
        for (i, (width, height)) in sizes.iter().enumerate() {
            s.execute("layout.pageSize", &json!({"pages": [i + 1], "width": width, "height": height})).unwrap();
        }
        let result = s.execute("file.exportPdf", &json!({})).unwrap();
        super::super::file::base64_decode(result["base64"].as_str().unwrap())
    }

    #[test]
    fn eps_web_exports_convert_tiff_and_skip_bad_used_assets() {
        use std::io::Read;
        let bytes = super::super::file::eps_place_tests::tiff_eps();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let placed = s.execute("file.place", &json!({"base64":super::super::file::base64_encode(&bytes),"name":"logo.eps"})).unwrap();
        let aid = AssetId(placed["asset"].as_u64().unwrap());
        let before = s.doc().unwrap().doc.clone();
        let html = s.execute("file.exportHtml", &json!({})).unwrap();
        let encoded = html["text"].as_str().unwrap().split("data:image/png;base64,").nth(1).unwrap().split('"').next().unwrap();
        let png = super::super::file::base64_decode(encoded);
        assert!(png.starts_with(b"\x89PNG"));
        assert_eq!(designcraft_render::image_size(&png), Some((2, 2)));
        assert_eq!(designcraft_render::decode_pixmap(&png).unwrap().data()[0], designcraft_render::decode_pixmap(&bytes).unwrap().data()[0]);
        let result = s.execute("file.exportEpub", &json!({})).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(super::super::file::base64_decode(result["base64"].as_str().unwrap()))).unwrap();
        let mut image = Vec::new();
        z.by_name(&format!("OEBPS/images/{}.png", aid.0)).unwrap().read_to_end(&mut image).unwrap();
        assert_eq!(image, png);
        let mut opf = String::new();
        z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
        assert!(opf.contains("media-type=\"image/png\""));
        assert!(!opf.contains("image/tiff"));
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        s.edit(|d, _| {
            Arc::make_mut(d.assets.get_mut(&aid).unwrap()).data = Arc::new(b"%!PS-Adobe-3.0 EPSF-3.0\n%%EOF\n".to_vec());
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.clone();
        for command in ["file.exportHtml", "file.exportEpub"] {
            let result = s.execute(command, &json!({})).unwrap();
            assert_eq!(result["warnings"].as_array().unwrap().len(), 1, "{command}");
            if command == "file.exportHtml" {
                assert!(!result["text"].as_str().unwrap().contains("<img "));
            }
        }
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
        s.execute("edit.clear", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.assets.contains_key(&aid));
        for command in ["file.exportHtml", "file.exportEpub"] {
            let result = s.execute(command, &json!({})).unwrap();
            assert!(result["warnings"].as_array().unwrap().is_empty(), "unused EPS: {command}");
        }
    }

    #[test]
    fn eps_status_tracks_source_bytes_across_save_update_relink_and_embed() {
        let dir = std::env::temp_dir().join(format!("dc-eps-status-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("logo.eps");
        let first = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n0 0 0 setrgbcolor\n%%EOF\n";
        let changed = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n1 0 0 setrgbcolor\n%%EOF\n";
        assert_eq!(first.len(), changed.len());
        assert_eq!(designcraft_images::eps_proxy(first), designcraft_images::eps_proxy(changed), "same preview, different source");
        std::fs::write(&path, first).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let aid = AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap()["asset"].as_u64().unwrap());
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        let before = s.doc().unwrap().doc.clone();
        assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 0);
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before), "unchanged EPS must not create an edit");
        let saved = designcraft_format::save(&before).unwrap();
        let reopened = designcraft_format::load(&saved).unwrap();
        assert_eq!(reopened.assets[&aid].data.as_slice(), first);
        let mut s = Session::new();
        s.add_document(crate::DocState::new(reopened, None));
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        std::fs::write(&path, changed).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "modified");
        assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        assert_eq!(s.doc().unwrap().doc.assets[&aid].data.as_slice(), changed);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "missing");
        let replacement = dir.join("replacement.eps");
        std::fs::write(&replacement, changed).unwrap();
        s.execute("links.relink", &json!({"asset": aid.0, "path": replacement.to_string_lossy()})).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "missing");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        s.execute("links.embed", &json!({"asset": aid.0})).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "embedded");
        std::fs::remove_file(&replacement).unwrap();
        let embedded = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        assert_eq!(status(&embedded.assets[&aid]), "embedded");
        assert_eq!(embedded.assets[&aid].data.as_slice(), changed);
        assert!(designcraft_render::decode_pixmap(&embedded.assets[&aid].data).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn eps_legacy_preview_document_is_upgraded_by_update() {
        let dir = std::env::temp_dir().join(format!("dc-eps-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("logo.eps");
        let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n%%EOF\n";
        std::fs::write(&path, source).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let aid = AssetId(s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap()["asset"].as_u64().unwrap());
        let proxy = designcraft_images::eps_proxy(source).unwrap().0;
        s.edit(|d, _| {
            let a = Arc::make_mut(d.assets.get_mut(&aid).unwrap());
            a.data = Arc::new(proxy);
            a.mime = "image/png".into();
            Ok(())
        })
        .unwrap();
        let old = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        let mut s = Session::new();
        s.add_document(crate::DocState::new(old, None));
        assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        assert_eq!(s.doc().unwrap().doc.assets[&aid].data.as_slice(), source);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(designcraft_render::decode_pixmap(&s.doc().unwrap().doc.assets[&aid].data).is_some(), "old preview remains renderable");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn eps_source_exports_preview_to_pdf_html_and_epub_without_editing_document() {
        use hayro_syntax::object::Name;
        use std::io::Read;

        let source = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 20 10\n%%EOF\n";
        let proxy = designcraft_images::eps_proxy(source).unwrap().0;
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let aid = AssetId(
            s.execute("file.place", &json!({"base64": super::super::file::base64_encode(source), "name": "logo.eps"})).unwrap()["asset"]
                .as_u64()
                .unwrap(),
        );
        // A document retaining the original EPS rather than the legacy preview-only asset.
        s.edit(|d, _| {
            let a = Arc::make_mut(d.assets.get_mut(&aid).unwrap());
            a.data = Arc::new(source.to_vec());
            a.mime = "application/postscript".into();
            Ok(())
        })
        .unwrap();
        let before = s.doc().unwrap().doc.clone();
        let result = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(result["warnings"].as_array().unwrap().is_empty(), "{:?}", result["warnings"]);
        let pdf = hayro_syntax::Pdf::new(super::super::file::base64_decode(result["base64"].as_str().unwrap())).unwrap();
        assert_eq!(
            pdf.objects()
                .into_iter()
                .filter_map(|o| o.into_stream())
                .filter(|st| st.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
                .count(),
            1
        );
        let result = s.execute("file.exportHtml", &json!({})).unwrap();
        let encoded = result["text"].as_str().unwrap().split("data:image/png;base64,").nth(1).unwrap().split('"').next().unwrap();
        assert_eq!(super::super::file::base64_decode(encoded), proxy);
        let result = s.execute("file.exportEpub", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(result["base64"].as_str().unwrap());
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut image = Vec::new();
        archive.by_name(&format!("OEBPS/images/{}.png", aid.0)).unwrap().read_to_end(&mut image).unwrap();
        assert_eq!(image, proxy);
        let mut opf = String::new();
        archive.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
        assert!(opf.contains("media-type=\"image/png\""));
        assert!(!opf.contains("application/postscript"));
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before), "exports must leave the source document intact");
        assert_eq!(before.assets[&aid].data.as_slice(), source);
    }

    #[test]
    fn eps_relink_update_and_folder_relink_retain_source() {
        let dir = std::env::temp_dir().join(format!("dc-links-eps-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("new")).unwrap();
        let eps = |w: u32| format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 {w} 10\n%%EOF\n");
        let original = dir.join("original.eps");
        let replacement = dir.join("replacement.eps");
        std::fs::write(&original, eps(10)).unwrap();
        std::fs::write(&replacement, eps(30)).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": original.to_string_lossy()})).unwrap();
        let aid = s.doc().unwrap().doc.assets.keys().next().copied().unwrap();
        std::fs::write(&original, eps(20)).unwrap();
        assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
        assert_eq!(s.doc().unwrap().doc.assets[&aid].pixels, Some((40, 20)));
        s.execute("links.relink", &json!({"asset": aid.0, "path": replacement.to_string_lossy()})).unwrap();
        let a = &s.doc().unwrap().doc.assets[&aid];
        assert_eq!(a.pixels, Some((60, 20)));
        assert_eq!(a.mime, "application/postscript");
        assert_eq!(a.data.as_slice(), eps(30).as_bytes());
        let moved = dir.join("new/replacement.eps");
        std::fs::rename(&replacement, &moved).unwrap();
        assert_eq!(s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap()["relinked"], 1);
        assert_eq!(s.doc().unwrap().doc.assets[&aid].link.as_deref(), moved.to_str());
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "ok");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pdf_relink_and_update_preserve_selected_page_size() {
        let dir = std::env::temp_dir().join(format!("dc-links-pdf-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("original.pdf");
        let replacement = dir.join("replacement.pdf");
        std::fs::write(&original, pdf(&[(10, 10), (20, 30)])).unwrap();
        std::fs::write(&replacement, pdf(&[(40, 40), (50, 60)])).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": original.to_string_lossy(), "pdfPage": 2})).unwrap();
        let aid = s.doc().unwrap().doc.assets.keys().next().copied().unwrap();
        s.execute("links.relink", &json!({"asset": aid.0, "path": replacement.to_string_lossy()})).unwrap();
        let a = &s.doc().unwrap().doc.assets[&aid];
        assert_eq!(a.page, 1);
        assert_eq!(a.pixels, Some((50, 60)));
        assert!(designcraft_render::render_pdf_page(&a.data, a.page as usize, 64).is_some());
        std::fs::write(&replacement, pdf(&[(70, 70), (80, 90)])).unwrap();
        assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
        assert_eq!(s.doc().unwrap().doc.assets[&aid].pixels, Some((80, 90)));
        let raster = dir.join("replacement.png");
        std::fs::write(&raster, png(4, 5)).unwrap();
        s.execute("links.relink", &json!({"asset": aid.0, "path": raster.to_string_lossy()})).unwrap();
        assert_eq!(s.doc().unwrap().doc.assets[&aid].page, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pdf_reload_rejects_missing_page_without_edit() {
        let full = pdf(&[(10, 10), (20, 30)]);
        let short = pdf(&[(10, 10)]);
        let dir = std::env::temp_dir().join(format!("dc-links-pdf-page-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("new")).unwrap();
        let original = dir.join("book.pdf");
        let replacement = dir.join("new/book.pdf");
        std::fs::write(&replacement, short.clone()).unwrap();
        for command in ["links.relink", "links.relinkFolder", "links.update"] {
            std::fs::write(&original, &full).unwrap();
            let mut s = Session::new();
            s.execute("file.new", &json!({})).unwrap();
            s.execute("file.place", &json!({"path": original.to_string_lossy(), "pdfPage": 2})).unwrap();
            let aid = s.doc().unwrap().doc.assets.keys().next().copied().unwrap();
            if command == "links.update" {
                std::fs::write(&original, &short).unwrap();
            }
            let st = s.doc().unwrap();
            let before = st.doc.clone();
            let revision = st.revision;
            let undo = st.history.undo.len();
            let journal = s.journal.len();
            let params =
                json!({"asset": aid.0, "path": replacement.to_string_lossy(), "dir": dir.join("new").to_string_lossy(), "missingOnly": false});
            let err = s.execute(command, &params).unwrap_err();
            assert!(err.to_string().contains("PDF page 2"), "{command}: {err}");
            let st = s.doc().unwrap();
            assert!(Arc::ptr_eq(&st.doc, &before), "{command}");
            assert_eq!(st.revision, revision);
            assert_eq!(st.history.undo.len(), undo);
            assert_eq!(s.journal.len(), journal);
            assert!(designcraft_render::render_pdf_page(&st.doc.assets[&aid].data, 1, 64).is_some());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn link_status_relink_update_embed() {
        let dir = std::env::temp_dir().join(format!("dc-links-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        std::fs::write(&a, png(10, 10)).unwrap();
        std::fs::write(&b, png(20, 10)).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": a.to_string_lossy(), "x": 72, "y": 72, "width": 72})).unwrap();
        let l = s.execute("links.list", &json!({})).unwrap();
        assert_eq!(l[0]["status"], "ok");
        assert_eq!(l[0]["uses"][0]["ppi"], 10.0, "10 px over 1 inch");
        let aid = l[0]["asset"].as_u64().unwrap();
        // The file changes on disk.
        std::fs::write(&a, png(30, 10)).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "modified");
        s.execute("links.update", &json!({})).unwrap();
        let l = s.execute("links.list", &json!({})).unwrap();
        assert_eq!(l[0]["status"], "ok");
        assert_eq!(l[0]["pixels"], json!([30, 10]));
        // Missing, then relinked.
        std::fs::remove_file(&a).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "missing");
        s.execute("links.relink", &json!({"asset": aid, "path": b.to_string_lossy()})).unwrap();
        let l = s.execute("links.list", &json!({})).unwrap();
        assert_eq!((l[0]["status"].as_str(), l[0]["name"].as_str()), (Some("ok"), Some("b.png")));
        // Go to selects the frame; embed forgets the file.
        let r = s.execute("links.goTo", &json!({"asset": aid})).unwrap();
        assert_eq!(r["ids"].as_array().unwrap().len(), 1);
        s.execute("links.embed", &json!({"asset": aid})).unwrap();
        assert_eq!(s.execute("links.list", &json!({})).unwrap()[0]["status"], "embedded");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod relink_folder_tests {
    use std::sync::Arc;

    use serde_json::json;

    use crate::Session;

    fn two_missing_links(case: &str) -> (Session, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dc-relink-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("new")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for name in ["a.svg", "b.svg"] {
            let path = dir.join(name);
            std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#).unwrap();
            s.doc_mut().unwrap().selection = Default::default();
            s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap();
            std::fs::rename(&path, dir.join("new").join(name)).unwrap();
        }
        (s, dir)
    }

    #[test]
    fn folder_relink_failure_keeps_document_and_history() {
        let (mut s, dir) = two_missing_links("atomic");
        std::fs::write(dir.join("new/b.svg"), b"not an image").unwrap();
        let st = s.doc().unwrap();
        let before = st.doc.clone();
        let revision = st.revision;
        let undo = st.history.undo.len();
        let journal = s.journal.len();
        let err = s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap_err();
        assert!(err.to_string().contains("unsupported or corrupt image"), "{err}");
        let st = s.doc().unwrap();
        assert!(Arc::ptr_eq(&st.doc, &before), "a failed batch must not leave its first link changed");
        assert_eq!(st.revision, revision);
        assert_eq!(st.history.undo.len(), undo);
        assert_eq!(s.journal.len(), journal);
        s.execute("frame.create", &json!({"rect": [0, 0, 20, 20]})).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn folder_relink_is_one_undo_step() {
        let (mut s, dir) = two_missing_links("undo");
        let st = s.doc().unwrap();
        let before = st.doc.clone();
        let revision = st.revision;
        let undo = st.history.undo.len();
        let r = s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap();
        assert_eq!(r["relinked"], 2);
        let st = s.doc().unwrap();
        assert_eq!(st.revision, revision + 1, "the batch commits once");
        assert_eq!(st.history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relink_extension_rejects_path_separators_without_panicking() {
        let (mut s, dir) = two_missing_links("extension");
        for extension in ["png/other", "png\\other", "png\0other"] {
            // Call directly so the command guard cannot hide a panic in Path::with_extension.
            let err = super::relink_folder(&mut s, &json!({"extension": extension})).unwrap_err();
            assert!(matches!(err, crate::EngineError::BadParams { .. }), "{err}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relink_missing_links_to_a_folder_and_by_extension() {
        let dir = std::env::temp_dir().join(format!("dc-relink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("new")).unwrap();
        let svg =
            |c: &str| format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="{c}"/></svg>"##);
        let orig = dir.join("logo.svg");
        std::fs::write(&orig, svg("red")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": orig.to_string_lossy()})).unwrap();
        // The file moves away: relink from the new folder.
        std::fs::rename(&orig, dir.join("new").join("logo.svg")).unwrap();
        let r = s.execute("links.relinkFolder", &json!({"dir": dir.join("new").to_string_lossy()})).unwrap();
        assert_eq!(r["relinked"], 1);
        let link = s.doc().unwrap().doc.assets.values().next().unwrap().link.clone().unwrap();
        assert!(link.contains("new"), "{link}");
        // Relink File Extension: the same name as .png doesn't exist → reported.
        let r = s.execute("links.relinkFolder", &json!({"extension": "png"})).unwrap();
        assert_eq!(r["relinked"], 0);
        assert_eq!(r["notFound"], json!(["logo.png"]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
