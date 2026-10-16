//! Window › Links: placed graphics with their status, effective resolution and uses; relink,
//! update, embed, go to.
//!
//! Every placed graphic keeps its bytes in the document; a linked graphic also remembers its
//! file. Its status compares that file with the copy in the document: missing, modified (the
//! file's bytes differ) or OK.
//!
//! A linked text file (File › Place with Create Links) remembers its file's size and hash on the
//! story instead; updating it imports the file again into the story.

use std::sync::Arc;

use designcraft_doc::{AssetId, Content, Document, Fingerprint, ItemId, Selection, Story, StoryId, TextFileLink};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "links.list", "Links", [], None, "{} → [{asset, name, path, status: ok|modified|missing|embedded, pixels, uses: [{id, page, ppi}]}], then linked text files: [{story, name, path, status (also tooLarge: unchanged size, too big to check), edited (the story was changed in the layout), uses: [{id, page}]}]", has_doc, |s, _| {
            let d = s.doc()?.doc.clone();
            Ok(Value::Array(list(&d, &mut s.text_link_checks)))
        }),
        cmd!(
            "links.relink",
            "Relink…",
            ["Window", "Links"],
            None,
            "{asset | story, path} — use another file for every placement of the graphic, or import another text file into the linked story",
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
            "{asset? | story?} — reload modified links from their files (all when neither); a linked text file is imported again into its story with the import options it was placed with, replacing edits made in the layout → {updated, warnings}",
            has_doc,
            update
        ),
        cmd!(
            "links.embed",
            "Embed Link",
            ["Window", "Links"],
            None,
            "{asset | story} — keep only the copy in the document (a text file: unlink the story)",
            has_doc,
            |s, p| {
                if let Some(sid) = story_param(p) {
                    return s.edit(|d, _| {
                        d.story_mut(sid).ok_or_else(|| bad("links.embed", "no such story"))?.file_link = None;
                        Ok(Value::Null)
                    });
                }
                let aid = asset_param(p, "links.embed")?;
                s.edit(|d, _| {
                    let a = d.assets.get_mut(&aid).ok_or_else(|| bad("links.embed", "no such graphic"))?;
                    Arc::make_mut(a).link = None;
                    Ok(Value::Null)
                })
            }
        ),
        cmd!(noundo "links.goTo", "Go To Link", ["Window", "Links"], None, "{asset | story} — selects the frames showing the graphic or the linked text", has_doc, |s, p| {
            let st = s.doc_mut()?;
            let ids = match story_param(p) {
                Some(sid) => st.doc.story(sid).map(|s| s.frames.clone()).unwrap_or_default(),
                None => uses(&st.doc, asset_param(p, "links.goTo")?),
            };
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

fn story_param(p: &Value) -> Option<StoryId> {
    p.get("story").and_then(Value::as_u64).map(StoryId)
}

/// FNV-1a (64-bit) of `bytes`, in hex: a linked text file's fingerprint.
pub(crate) fn fnv_hex(bytes: &[u8]) -> String {
    let h = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("{h:016x}")
}

/// Fingerprint of a story's text and formatting (edits made in the layout change it).
pub(crate) fn story_hash(st: &Story) -> String {
    fnv_hex(serde_json::to_string(&(&st.text, &st.paras, &st.chars)).unwrap_or_default().as_bytes())
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
        match crate::io::metadata(path) {
            Err(_) => "missing",
            Ok(m) if m.len() as usize != a.data.len() => "modified",
            // Same size: compare the bytes (cheap for the sizes involved, and exact).
            Ok(_) => match crate::io::read_file(path) {
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

/// The largest linked text file read (the Data Merge source limit). Only regular files are read,
/// so a link to a FIFO or a device (e.g. `/dev/zero`) can't block or fill memory.
const MAX_TEXT_LINK_BYTES: u64 = super::datamerge::MAX_DATA_FILE_BYTES;

/// Can a linked text file this size be checked and updated?
pub(crate) fn checkable_size(len: usize) -> bool {
    len as u64 <= MAX_TEXT_LINK_BYTES
}

/// Can a size and time be trusted to change when the file does? Times are whole seconds, so not
/// while the file was changed in the last two: another change in that second keeps the time. A
/// time in the future (clock skew, a file copied from another machine) counts as settled, or such
/// a file would be read on every poll; a later save gives it a time unlike the remembered one.
pub(crate) fn settled(f: &Fingerprint) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        f.mtime > now || now.saturating_sub(f.mtime) >= 2
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = f;
        false
    }
}

/// Checked linked text files: path → (size and time, hash of the contents then).
pub(crate) type TextChecks = std::collections::HashMap<String, (Fingerprint, String)>;

/// Link status of a linked text file: `tooLarge` when it is the recorded size but over
/// [`MAX_TEXT_LINK_BYTES`], so its contents aren't checked. The file is read only when its size
/// is the recorded one but its time isn't, and then once per new time (`checks`): a file saved
/// or synced again without changes stays OK.
pub(crate) fn text_status(l: &TextFileLink, checks: &mut TextChecks) -> &'static str {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !crate::io::is_file(&l.path) {
            return "missing";
        }
        let now = match crate::io::metadata(&l.path) {
            Err(_) => return "missing",
            Ok(m) => super::datamerge::fingerprint_from(&m),
        };
        if now.size != l.size {
            return "modified";
        }
        if l.mtime != 0 && now.mtime == l.mtime {
            return "ok";
        }
        let hash = match checks.get(&l.path) {
            Some((at, hash)) if *at == now => hash.clone(),
            _ if now.size > MAX_TEXT_LINK_BYTES => return "tooLarge",
            _ => match read_text(&l.path) {
                Ok((b, at)) => {
                    let hash = fnv_hex(&b);
                    // The time from before the read: a change during it is seen next time.
                    if settled(&at) {
                        checks.insert(l.path.clone(), (at, hash.clone()));
                    }
                    hash
                }
                Err(_) => return "missing",
            },
        };
        if hash == l.hash { "ok" } else { "modified" }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (l, checks);
        "ok"
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

fn list(d: &Document, checks: &mut TextChecks) -> Vec<Value> {
    let texts = d.stories.values().filter_map(|st| {
        let l = st.file_link.as_ref()?;
        let uses: Vec<Value> = st.frames.iter().map(|id| json!({"id": id.0, "page": d.page_of_item(*id).map(|p| d.page_name(p))})).collect();
        Some(json!({"story": st.id.0, "name": file_name(&l.path), "path": l.path, "status": text_status(l, checks), "edited": story_hash(st) != l.story_hash, "uses": uses}))
    });
    let used = d.used_assets();
    d.assets
        .values()
        .filter(|a| used.contains(&a.id))
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
        .chain(texts)
        .collect()
}

/// A linked text file's bytes, with its size and time from before they were read.
#[cfg(not(target_arch = "wasm32"))]
fn read_text(path: &str) -> Result<(Vec<u8>, Fingerprint)> {
    let err = |e: std::io::Error| EngineError::Other(format!("{path}: {e}"));
    // The roots' own message when they refuse the path.
    crate::file_access::check_read(path).map_err(EngineError::Other)?;
    let m = crate::io::metadata(path).map_err(err)?;
    if !crate::io::is_file(path) {
        return Err(EngineError::Other(format!("{path}: not a file")));
    }
    let too_large = || EngineError::Other(format!("{path}: larger than {} MiB", MAX_TEXT_LINK_BYTES / (1024 * 1024)));
    if m.len() > MAX_TEXT_LINK_BYTES {
        return Err(too_large());
    }
    // At most one byte over the limit, in case the file grew since.
    let b = crate::io::read_capped(path, MAX_TEXT_LINK_BYTES).map_err(err)?;
    if b.len() as u64 > MAX_TEXT_LINK_BYTES {
        return Err(too_large());
    }
    Ok((b, super::datamerge::fingerprint_from(&m)))
}

#[cfg(target_arch = "wasm32")]
fn read_text(path: &str) -> Result<(Vec<u8>, Fingerprint)> {
    Err(EngineError::Other(format!("{path}: files can't be read on the web")))
}

/// Import a linked text file again; a warning says when edits made in the layout were replaced.
fn reimport(d: &mut Document, sid: StoryId, path: &str, bytes: &[u8], before: Fingerprint) -> Result<Vec<String>> {
    let edited = d.story(sid).is_some_and(|st| st.file_link.as_ref().is_some_and(|l| story_hash(st) != l.story_hash));
    let mut warnings = super::place_text::reimport(d, sid, path, bytes, Some(before))?;
    if edited {
        warnings.insert(0, format!("{}: edits made to story {} in the layout were replaced by the file's text", file_name(path), sid.0));
    }
    Ok(warnings)
}

#[cfg(not(target_arch = "wasm32"))]
fn reload(path: &str) -> Result<(Vec<u8>, (u32, u32))> {
    let b = crate::io::read_file(path).map_err(EngineError::Other)?;
    let px = designcraft_render::image_size(&b).ok_or_else(|| bad("links", format!("{path}: unsupported or corrupt image")))?;
    Ok((b, px))
}

#[cfg(target_arch = "wasm32")]
fn reload(path: &str) -> Result<(Vec<u8>, (u32, u32))> {
    Err(EngineError::Other(format!("{path}: files can't be read on the web")))
}

fn relink(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("links.relink", "missing path"))?.to_string();
    if let Some(sid) = story_param(p) {
        let (bytes, before) = read_text(&path)?;
        let name = file_name(&path);
        return s.edit(|d, _| {
            let warnings = reimport(d, sid, &path, &bytes, before)?;
            Ok(json!({"name": name, "warnings": warnings}))
        });
    }
    let aid = asset_param(p, "links.relink")?;
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
        Ok(json!({"name": name}))
    })
}

fn relink_folder(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = str_param(p, "dir").map(std::path::PathBuf::from);
    if let Some(dir) = &dir {
        // Refused outright: candidates outside the automation read root would only look missing.
        crate::file_access::check_read(dir).map_err(EngineError::Other)?;
    }
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
        if crate::io::exists(&cand) {
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
    let d = s.doc()?.doc.clone();
    let checks = &mut s.text_link_checks;
    let (only_asset, only_story) = (p.get("asset").and_then(Value::as_u64), story_param(p));
    let all = only_asset.is_none() && only_story.is_none();
    let which: Vec<(AssetId, String)> = d
        .assets
        .values()
        .filter(|a| all || only_asset == Some(a.id.0))
        .filter(|a| status(a) == "modified")
        .filter_map(|a| a.link.clone().map(|l| (a.id, l)))
        .collect();
    let texts: Vec<(StoryId, String)> = d
        .stories
        .values()
        .filter(|st| all || only_story == Some(st.id))
        .filter_map(|st| st.file_link.as_ref().filter(|l| text_status(l, checks) == "modified").map(|l| (st.id, l.path.clone())))
        .collect();
    let mut fresh = Vec::new();
    for (aid, path) in which {
        fresh.push((aid, reload(&path)?));
    }
    let mut fresh_texts = Vec::new();
    for (sid, path) in texts {
        let (bytes, before) = read_text(&path)?;
        fresh_texts.push((sid, path, bytes, before));
    }
    let n = fresh.len() + fresh_texts.len();
    if n == 0 {
        return Ok(json!({"updated": 0}));
    }
    s.edit(|d, _| {
        for (aid, (bytes, px)) in &fresh {
            if let Some(a) = d.assets.get_mut(aid) {
                let a = Arc::make_mut(a);
                a.data = Arc::new(bytes.clone());
                a.pixels = Some(*px);
            }
        }
        let mut warnings = Vec::new();
        for (sid, path, bytes, before) in &fresh_texts {
            warnings.extend(reimport(d, *sid, path, bytes, *before)?);
        }
        Ok(json!({"updated": n, "warnings": warnings}))
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

#[cfg(test)]
mod shared_asset_tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        designcraft_render::Rendered { width: w, height: h, pixels: vec![200u8; (w * h * 4) as usize] }.to_png()
    }

    #[test]
    fn placements_of_one_file_share_an_asset_and_saves_drop_replaced_ones() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = super::super::file::base64_encode(&png(10, 10));
        s.execute("file.place", &json!({"base64": a, "name": "a.png", "x": 72, "y": 72})).unwrap();
        s.execute("edit.deselectAll", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64": a, "name": "a.png", "x": 200, "y": 72})).unwrap();
        let l = s.execute("links.list", &json!({})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 1);
        assert_eq!(l[0]["uses"].as_array().unwrap().len(), 2);
        assert_eq!(s.doc().unwrap().doc.assets.len(), 1);

        // A new image into a frame: the old asset stays in memory (undo) but not in the file.
        let f = s.execute("frame.create", &json!({"rect": [100, 300, 200, 400]})).unwrap()["id"].as_u64().unwrap();
        let b = super::super::file::base64_encode(&png(20, 10));
        let c = super::super::file::base64_encode(&png(30, 10));
        s.execute("file.place", &json!({"base64": b, "name": "b.png", "frame": f})).unwrap();
        s.execute("file.place", &json!({"base64": c, "name": "c.png", "frame": f})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.assets.len(), 3);
        assert_eq!(s.execute("links.list", &json!({})).unwrap().as_array().unwrap().len(), 2);
        let back = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        let mut names: Vec<&str> = back.assets.values().map(|a| a.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["a.png", "c.png"]);
    }

    #[test]
    fn files_with_identical_bytes_at_different_paths_keep_their_own_links() {
        let dir = std::env::temp_dir().join(format!("dc-same-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.png"), dir.join("b.png"));
        std::fs::write(&a, png(10, 10)).unwrap();
        std::fs::write(&b, png(10, 10)).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        for (path, x) in [(&a, 72), (&b, 200)] {
            s.execute("edit.deselectAll", &json!({})).unwrap();
            s.execute("file.place", &json!({"path": path.to_string_lossy(), "x": x, "y": 72})).unwrap();
        }
        assert_eq!(s.doc().unwrap().doc.assets.len(), 2);
        let rows = |s: &mut Session| -> Vec<(String, Value)> {
            let l = s.execute("links.list", &json!({})).unwrap();
            l.as_array().unwrap().iter().map(|r| (r["path"].as_str().unwrap().to_string(), r["pixels"].clone())).collect()
        };
        let paths = |r: &[(String, Value)]| r.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();
        let before = rows(&mut s);
        assert_eq!(paths(&before), [a.to_string_lossy(), b.to_string_lossy()]);

        // b.png changes on disk: updating it leaves a.png's placement alone.
        std::fs::write(&b, png(20, 10)).unwrap();
        assert_eq!(s.execute("links.update", &json!({})).unwrap()["updated"], 1);
        let after = rows(&mut s);
        assert_eq!(paths(&after), paths(&before));
        assert_eq!((after[0].1.clone(), after[1].1.clone()), (json!([10, 10]), json!([20, 10])));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pasted_graphics_reuse_the_identical_asset_of_the_target() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Asset, Graphic, Item, ItemId, Shape, SpreadRef};
        let asset = |id: u64, data: Vec<u8>| Asset {
            id: AssetId(id),
            name: "p.png".into(),
            mime: "image/png".into(),
            link: None,
            data: Arc::new(data),
            pixels: None,
            page: 0,
        };
        let mut src = Document::new(&NewDocument::default());
        let mut dst = Document::new(&NewDocument::default());
        let (pa, pb) = (png(4, 4), png(8, 4));
        // The source's asset id is taken in the target by a different file.
        let sid = AssetId(src.alloc());
        src.assets.insert(sid, Arc::new(asset(sid.0, pa.clone())));
        dst.assets.insert(sid, Arc::new(asset(sid.0, pb)));
        dst.alloc();
        let same = AssetId(dst.alloc());
        assert_ne!(same, sid);
        dst.assets.insert(same, Arc::new(asset(same.0, pa)));
        let iid = ItemId(src.alloc());
        let mut it = Item::new(
            iid,
            src.default_layer(),
            Shape::Rectangle,
            designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 9.0, 9.0)),
        );
        it.content = Content::Graphic(Graphic {
            asset: sid,
            size: (4.0, 4.0),
            xf: designcraft_geom::Affine::IDENTITY,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
        });
        src.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let ids = super::super::object::duplicate_from(&mut dst, &src, &[iid], SpreadRef::Doc(0), designcraft_geom::Vec2::ZERO).unwrap();
        let g = dst.item(ids[0]).and_then(|i| i.graphic()).unwrap();
        assert_eq!((g.asset, g.crop), (same, [1.0; 4]));
        assert_eq!(dst.assets.len(), 2);
    }

    #[test]
    fn pasted_missing_graphic_does_not_show_an_unrelated_target_asset() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Asset, Graphic, Item, ItemId, Shape, SpreadRef};
        let mut src = Document::new(&NewDocument::default());
        let mut dst = Document::new(&NewDocument::default());
        // The source graphic points at an asset the source does not have; the target uses that id for another image.
        let missing = AssetId(src.alloc());
        dst.assets.insert(
            missing,
            Arc::new(Asset {
                id: missing,
                name: "p.png".into(),
                mime: "image/png".into(),
                link: None,
                data: Arc::new(png(8, 4)),
                pixels: None,
                page: 0,
            }),
        );
        let iid = ItemId(src.alloc());
        let mut it = Item::new(
            iid,
            src.default_layer(),
            Shape::Rectangle,
            designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(0.0, 0.0, 9.0, 9.0)),
        );
        it.content = Content::Graphic(Graphic {
            asset: missing,
            size: (4.0, 4.0),
            xf: designcraft_geom::Affine::IDENTITY,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
        });
        src.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let ids = super::super::object::duplicate_from(&mut dst, &src, &[iid], SpreadRef::Doc(0), designcraft_geom::Vec2::ZERO).unwrap();
        let g = dst.item(ids[0]).and_then(|i| i.graphic()).unwrap();
        assert_ne!(g.asset, missing);
        assert!(!dst.assets.contains_key(&g.asset));
    }
}

#[cfg(test)]
mod text_link_tests {
    use serde_json::{Value, json};

    use crate::Session;
    use designcraft_doc::{ItemId, StoryId};

    /// A small Word file: a Title paragraph, then Body paragraphs (`body` is WordprocessingML).
    fn docx(body: &str) -> Vec<u8> {
        use std::io::Write;
        const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let styles = format!(
            r#"<w:styles {W}><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:rPr><w:sz w:val="56"/></w:rPr></w:style>
            <w:style w:type="paragraph" w:styleId="Body"><w:name w:val="Body"/><w:rPr><w:sz w:val="20"/></w:rPr></w:style></w:styles>"#
        );
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Head</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Body"/></w:pPr>{body}</w:p></w:body></w:document>"#
        );
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in [("word/document.xml", doc), ("word/styles.xml", styles)] {
            w.start_file(n, o).unwrap();
            w.write_all(c.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn text_rows(s: &mut Session) -> Vec<Value> {
        let l = s.execute("links.list", &json!({})).unwrap();
        l.as_array().unwrap().iter().filter(|r| r.get("story").is_some()).cloned().collect()
    }

    fn story(s: &Session, sid: StoryId) -> designcraft_doc::Story {
        s.doc().unwrap().doc.story(sid).unwrap().clone()
    }

    fn styles(st: &designcraft_doc::Story) -> Vec<String> {
        st.paras.iter().map(|p| p.style.clone()).collect()
    }

    #[test]
    fn linked_word_file_status_update_relink_and_unlink() {
        let dir = std::env::temp_dir().join(format!("dc-text-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("moved")).unwrap();
        let path = dir.join("chapter.docx");
        std::fs::write(&path, docx("<w:r><w:t>First draft.</w:t></w:r>")).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 9}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Headline", "chars": {"size": 30}})).unwrap();
        // Two threaded frames.
        let a = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "content": "text"})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [72, 72, 300, 200], "spread": 1, "content": "text"})).unwrap()["id"].as_u64().unwrap();
        s.edit(|d, _| Ok(d.thread(ItemId(a), ItemId(b))?)).unwrap();
        s.execute("edit.deselectAll", &json!({})).unwrap();

        // Place and link, with a style map and Auto Rename.
        let r = s
            .execute(
                "file.place",
                &json!({"path": path.to_string_lossy(), "frame": a, "link": true, "styleMap": {"Title": "Headline"}, "styleConflicts": "autoRename"}),
            )
            .unwrap();
        let sid = StoryId(r["story"].as_u64().unwrap());
        let placed = story(&s, sid);
        assert_eq!(placed.text, "Head\nFirst draft.");
        assert_eq!(styles(&placed), ["Headline", "Body_wrd_1"]);
        let link = placed.file_link.clone().unwrap();
        assert_eq!(link.path, path.to_string_lossy());
        assert_eq!(link.style_conflicts, "autoRename");
        assert_eq!(link.style_map.get("Title").map(String::as_str), Some("Headline"));
        assert_eq!(link.style_map.get("Body").map(String::as_str), Some("Body_wrd_1"), "Auto Rename's choice is kept");
        let rows = text_rows(&mut s);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0]["story"].as_u64(), rows[0]["name"].as_str()), (Some(sid.0), Some("chapter.docx")));
        assert_eq!((rows[0]["status"].as_str(), rows[0]["edited"].as_bool()), (Some("ok"), Some(false)));
        assert_eq!(rows[0]["uses"].as_array().unwrap().len(), 2, "both threaded frames");

        // The file changes (with tracked changes): Modified; Update imports it as if accepted, with
        // the saved style map, into the same frames, without new renamed styles.
        std::fs::write(
            &path,
            docx(r#"<w:r><w:t xml:space="preserve">Second </w:t></w:r><w:ins w:id="1" w:author="A"><w:r><w:t>revised </w:t></w:r></w:ins><w:del w:id="2" w:author="A"><w:r><w:delText>old </w:delText></w:r></w:del><w:r><w:t>draft.</w:t></w:r>"#),
        )
        .unwrap();
        assert_eq!(text_rows(&mut s)[0]["status"], "modified");
        let r = s.execute("links.update", &json!({"story": sid.0})).unwrap();
        assert_eq!(r["updated"], 1);
        assert_eq!(r["warnings"], json!([]), "no edits in the layout to replace");
        let updated = story(&s, sid);
        assert_eq!(updated.text, "Head\nSecond revised draft.");
        assert_eq!(styles(&updated), ["Headline", "Body_wrd_1"]);
        assert_eq!(updated.frames, [ItemId(a), ItemId(b)], "frames and thread kept");
        assert!(s.doc().unwrap().doc.styles.para("Body_wrd_2").is_none());
        assert!(s.doc().unwrap().doc.styles.para("Title").is_none(), "mapped styles aren't imported");
        assert_eq!(text_rows(&mut s)[0]["status"], "ok");

        // An edit in the layout, then the file changes again: Update replaces the edit and warns.
        s.execute("story.setText", &json!({"story": sid.0, "text": "Fixed in layout"})).unwrap();
        assert_eq!(text_rows(&mut s)[0]["edited"], true);
        std::fs::write(&path, docx("<w:r><w:t>Third draft.</w:t></w:r>")).unwrap();
        let r = s.execute("links.update", &json!({})).unwrap();
        assert_eq!(r["updated"], 1);
        let w = r["warnings"][0].as_str().unwrap();
        assert!(w.contains("chapter.docx") && w.contains("replaced"), "{w}");
        assert_eq!(story(&s, sid).text, "Head\nThird draft.");
        assert_eq!(text_rows(&mut s)[0]["edited"], false);
        // Undo brings the edit back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(story(&s, sid).text, "Fixed in layout");
        s.execute("edit.redo", &json!({})).unwrap();

        // Saved and opened again: the link and its options are kept.
        let back = designcraft_format::load(&designcraft_format::save(&s.doc().unwrap().doc).unwrap()).unwrap();
        assert_eq!(back.story(sid).unwrap().file_link, story(&s, sid).file_link);
        assert_eq!(super::text_status(back.story(sid).unwrap().file_link.as_ref().unwrap(), &mut Default::default()), "ok");

        // The file moves: Missing; Relink to it imports it into the story.
        let moved = dir.join("moved").join("chapter.docx");
        std::fs::write(&moved, docx("<w:r><w:t>Moved draft.</w:t></w:r>")).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(text_rows(&mut s)[0]["status"], "missing");
        s.execute("links.relink", &json!({"story": sid.0, "path": moved.to_string_lossy()})).unwrap();
        let rows = text_rows(&mut s);
        assert_eq!((rows[0]["status"].as_str(), rows[0]["path"].as_str()), (Some("ok"), Some(moved.to_string_lossy().as_ref())));
        assert_eq!(story(&s, sid).text, "Head\nMoved draft.");

        // Go To selects the story's frames; Unlink keeps the text and drops the file.
        let r = s.execute("links.goTo", &json!({"story": sid.0})).unwrap();
        assert_eq!(r["ids"], json!([a, b]));
        s.execute("links.embed", &json!({"story": sid.0})).unwrap();
        assert!(text_rows(&mut s).is_empty());
        assert_eq!(story(&s, sid).text, "Head\nMoved draft.");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_linked_text_files_are_not_read_again() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("dc-text-link-poll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, "Hello").unwrap();
        // Unreadable: a status that reads the file would say missing.
        let readable = |on: bool| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(if on { 0o644 } else { 0o000 })).unwrap();
        // A time `secs` ago (a file changed in the last two seconds is always read).
        let touch = |secs: u64| {
            let f = std::fs::File::options().write(true).open(&path).unwrap();
            f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs)).unwrap();
        };
        touch(60);
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true})).unwrap();
        let status = |s: &mut Session| text_rows(s)[0]["status"].as_str().unwrap().to_string();

        // Size and time as placed: not read.
        readable(false);
        if std::fs::read(&path).is_ok() {
            // Running as root: permissions don't stop reads, so this can't be observed.
            return;
        }
        assert_eq!(status(&mut s), "ok");
        // Saved again unchanged (a new time): read once, then not again.
        readable(true);
        touch(30);
        assert_eq!(status(&mut s), "ok");
        readable(false);
        assert_eq!(status(&mut s), "ok", "the second poll doesn't read it");
        // Changed, same size, new time: modified.
        readable(true);
        std::fs::write(&path, "Jello").unwrap();
        touch(20);
        assert_eq!(status(&mut s), "modified");
        // Changed again in the same second, same size: still seen (just-changed files are read).
        std::fs::write(&path, "Hello").unwrap();
        assert_eq!(status(&mut s), "ok");
        std::fs::write(&path, "Jello").unwrap();
        assert_eq!(status(&mut s), "modified");
        touch(10);
        // Updated: the new size and time are kept, so the next poll doesn't read it.
        s.execute("links.update", &json!({})).unwrap();
        readable(false);
        assert_eq!(status(&mut s), "ok");
        // A time in the future (clock skew): read once, then not on every poll.
        readable(true);
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600)).unwrap();
        assert_eq!(status(&mut s), "ok");
        readable(false);
        assert_eq!(status(&mut s), "ok", "a future time is remembered");
        readable(true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_links_when_placing_text_files_preference() {
        let dir = std::env::temp_dir().join(format!("dc-text-link-pref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.txt");
        std::fs::write(&path, "Plain text").unwrap();
        let place = |s: &mut Session, extra: Value| {
            let mut p = json!({"path": path.to_string_lossy(), "rect": [72, 72, 300, 200]});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            s.execute("edit.deselectAll", &json!({})).unwrap();
            let r = s.execute("file.place", &p).unwrap();
            (s.doc().unwrap().doc.story(StoryId(r["story"].as_u64().unwrap())).unwrap().file_link.is_some(), r)
        };
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert!(!place(&mut s, json!({})).0, "off by default");
        assert!(place(&mut s, json!({"link": true})).0);
        s.execute("prefs.set", &json!({"linkTextFiles": true})).unwrap();
        assert!(place(&mut s, json!({})).0, "the preference links");
        assert!(!place(&mut s, json!({"link": false})).0, "`link` overrides it");
        // Placed from bytes: no file to link.
        let b64 = crate::cmd::base64_encode(b"Plain text");
        let r = s.execute("file.place", &json!({"base64": b64, "name": "notes.txt", "rect": [72, 300, 300, 400]})).unwrap();
        assert!(s.doc().unwrap().doc.story(StoryId(r["story"].as_u64().unwrap())).unwrap().file_link.is_none());
        // Into an insertion point: not linked, with a warning.
        let fr = s.execute("frame.create", &json!({"rect": [72, 500, 300, 600], "content": "text", "text": "Start  end"})).unwrap();
        let sid = fr["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 6, "focus": 6})).unwrap();
        let r = s.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(r["warnings"][0].as_str().unwrap().contains("not linked"), "{r}");
        assert!(s.doc().unwrap().doc.story(StoryId(sid)).unwrap().file_link.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(unix)]
#[cfg(test)]
mod text_link_read_tests {
    use std::time::Duration;

    use designcraft_doc::TextFileLink;

    use super::{MAX_TEXT_LINK_BYTES, read_text, text_status};

    /// `f` on another thread: a call that blocks fails the test instead of hanging it.
    fn promptly<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        // A timeout means the call blocked.
        rx.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    fn status_of(path: &std::path::Path, size: u64) -> &'static str {
        let l = TextFileLink { path: path.to_string_lossy().to_string(), size, hash: "0".into(), ..Default::default() };
        promptly(move || text_status(&l, &mut Default::default()))
    }

    fn read(path: &std::path::Path) -> Result<usize, String> {
        let p = path.to_string_lossy().to_string();
        promptly(move || read_text(&p).map(|(b, _)| b.len()).map_err(|e| e.to_string()))
    }

    #[test]
    fn links_to_fifos_devices_folders_and_huge_files_are_not_read() {
        let dir = std::env::temp_dir().join(format!("dc-text-link-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A FIFO with no writer: reading it would block for ever.
        let fifo = dir.join("pipe.docx");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        assert_eq!(status_of(&fifo, 0), "missing");
        assert!(read(&fifo).is_err());
        // A device that never ends, recorded with size 0 (as its metadata reports).
        let zero = std::path::Path::new("/dev/zero");
        assert_eq!(status_of(zero, 0), "missing");
        assert!(read(zero).is_err());
        // A folder.
        assert_eq!(status_of(&dir, 0), "missing");
        assert!(read(&dir).is_err());
        // Over the limit (sparse, so cheap): not read; the same size is reported as too large
        // to check rather than OK, another size as modified.
        let big = dir.join("big.txt");
        std::fs::File::create(&big).unwrap().set_len(MAX_TEXT_LINK_BYTES + 1).unwrap();
        assert_eq!(status_of(&big, MAX_TEXT_LINK_BYTES + 1), "tooLarge");
        assert_eq!(status_of(&big, 10), "modified");
        assert!(read(&big).unwrap_err().contains("larger than"));
        assert!(!super::checkable_size(MAX_TEXT_LINK_BYTES as usize + 1), "placing it doesn't link it");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
