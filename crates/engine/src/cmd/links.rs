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
        cmd!(query "links.list", "Links", [], None, "{} → [{asset, name, path, status: ok|modified|missing|embedded, pixels, pdfCrop (placed PDFs: the box shown), uses: [{id, page, ppi}]}]", has_doc, |s, _| {
            Ok(Value::Array(list(&s.doc()?.doc)))
        }),
        cmd!(
            "links.relink",
            "Relink…",
            ["Window", "Links"],
            None,
            "{asset, path} — use another file for every placement of the graphic",
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
            let pdf_crop = (a.mime == "application/pdf").then(|| a.pdf_crop.name());
            json!({"asset": a.id.0, "name": a.name, "path": a.link, "status": status(a), "pixels": a.pixels, "pdfCrop": pdf_crop, "uses": uses})
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

fn relink(s: &mut Session, p: &Value) -> Result<Value> {
    let aid = asset_param(p, "links.relink")?;
    let path = str_param(p, "path").ok_or_else(|| bad("links.relink", "missing path"))?.to_string();
    let (bytes, px) = reload(&path)?;
    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
    s.edit(|d, _| {
        let a = d.assets.get_mut(&aid).ok_or_else(|| bad("links.relink", "no such graphic"))?;
        let a = Arc::make_mut(a);
        a.mime = designcraft_render::image_mime(&bytes).to_string();
        a.data = Arc::new(bytes.clone());
        a.pixels = Some(px);
        a.name = name.clone();
        a.link = Some(path.clone());
        // A placed PDF keeps its crop choice; its box is found on the new file.
        super::file::crop_pdf(a);
        Ok(json!({"name": name}))
    })
}

fn relink_folder(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").map(std::path::PathBuf::from);
    let ext = str_param(p, "extension").map(|e| e.trim_start_matches('.').to_string());
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
    let mut n = 0;
    for (aid, path) in todo {
        relink(s, &json!({"asset": aid.0, "path": path.to_string_lossy()}))?;
        n += 1;
    }
    Ok(json!({"relinked": n, "notFound": not_found}))
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let d = &s.doc()?.doc;
    let which: Vec<(AssetId, String)> = d
        .assets
        .values()
        .filter(|a| p.get("asset").and_then(Value::as_u64).is_none_or(|x| x == a.id.0))
        .filter(|a| status(a) == "modified")
        .filter_map(|a| a.link.clone().map(|l| (a.id, l)))
        .collect();
    let mut fresh = Vec::new();
    for (aid, path) in which {
        fresh.push((aid, reload(&path)?));
    }
    let n = fresh.len();
    if n == 0 {
        return Ok(json!({"updated": 0}));
    }
    s.edit(|d, _| {
        for (aid, (bytes, px)) in &fresh {
            if let Some(a) = d.assets.get_mut(aid) {
                let a = Arc::make_mut(a);
                a.data = Arc::new(bytes.clone());
                a.pixels = Some(*px);
                super::file::crop_pdf(a);
            }
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
    use serde_json::json;

    use crate::Session;

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
