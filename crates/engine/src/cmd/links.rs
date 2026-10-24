//! Window › Links: placed graphics with their status, effective resolution and uses; relink,
//! update, embed, go to.
//!
//! Originals stay cached in the live document. Native working files can store links only;
//! portable serialization and recovery always retain the originals. Link status compares the
//! source file with the cached copy: missing, modified (the bytes differ) or OK.
//!
//! A linked text file (File › Place with Create Links) remembers its file's size and hash on the
//! story instead; updating it imports the file again into the story.
//!
//! A link is followed only where [`crate::io::document_path`] allows it, resolved against the
//! document's own file; a refused link is missing, with the reason in `links.list`.

use std::path::Path;
use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::{AssetId, Content, Document, Fingerprint, Item, ItemId, Selection, Story, StoryId, TextFileLink};
use designcraft_geom::Affine;
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "links.storage",
            "Linked Artwork Storage",
            [],
            None,
            "{mode?: linked|embedded} — native working saves keep linked originals external or include them; no mode reports the document's choice",
            has_doc,
            super::native_links::storage
        ),
        cmd!(query "links.list", "Links", [], None, "{} → [{asset, name, path, status: ok|modified|missing|embedded, refused?: why a missing link isn't followed (a path on another computer, a device), pixels, pdfCrop (placed PDFs: the box shown), uses: [{id, page, ppi}]}], then linked text files: [{story, name, path, status (also tooLarge: unchanged size, too big to check), edited (the story was changed in the layout), uses: [{id, page}]}]", has_doc, |s, _| {
            // Taken out for the call so the document needn't be cloned on every poll.
            let mut checks = std::mem::take(&mut s.text_link_checks);
            let links = s.doc().map(|d| list(&d.doc, &s.cache, d.path.as_deref().map(Path::new), &mut checks));
            s.text_link_checks = checks;
            Ok(Value::Array(links?))
        }),
        cmd!(
            "links.relink",
            "Relink…",
            ["Window", "Links"],
            None,
            "{asset | story, path} — use another file for every placement of the graphic (keeps the selected PDF page; error if missing), or import another text file into the linked story",
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
                    if a.data.is_empty() {
                        return Err(bad("links.embed", "relink the missing artwork before embedding it"));
                    }
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
            st.bump_revision();
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

/// Why a link stored in the document at `document` isn't followed, if it isn't.
pub fn refused(link: &str, document: Option<&Path>) -> Option<crate::io::Refused> {
    crate::io::document_path(link, document).err()
}

/// Whether an asset's linked file is gone ([`status`] "missing" without reading the file). An
/// asset can hold data while its file is missing: the document's preview of it. `document` is
/// the document's file, which relative links are resolved against.
pub fn link_missing(a: &designcraft_doc::Asset, document: Option<&Path>) -> bool {
    let Some(link) = &a.link else { return false };
    #[cfg(not(target_arch = "wasm32"))]
    {
        !crate::io::document_path(link, document).is_ok_and(crate::io::exists)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (link, document);
        false
    }
}

/// Link status of an asset in the document at `document`. Only a regular file is compared, so a
/// link to a device (`/dev/zero`) or a FIFO is missing rather than read.
pub fn status(a: &designcraft_doc::Asset, document: Option<&Path>) -> &'static str {
    let Some(link) = &a.link else { return "embedded" };
    // Nothing was read from the file (a package's link that wasn't followed): missing, as in
    // Preflight.
    if a.data.is_empty() {
        return "missing";
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(path) = crate::io::document_path(link, document) else { return "missing" };
        if !crate::io::is_file(&path) {
            return "missing";
        }
        match crate::io::metadata(&path) {
            Err(_) => "missing",
            Ok(m) if m.len() as usize != a.data.len() => "modified",
            // Same size: compare the bytes (cheap for the sizes involved, and exact).
            Ok(_) => match crate::io::read_capped(&path, a.data.len() as u64) {
                Ok(b) if b == *a.data => "ok",
                Ok(_) => "modified",
                Err(_) => "missing",
            },
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (link, document);
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
/// or synced again without changes stays OK. `document` is the document's file.
pub(crate) fn text_status(l: &TextFileLink, checks: &mut TextChecks, document: Option<&Path>) -> &'static str {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Ok(path) = crate::io::document_path(&l.path, document) else { return "missing" };
        if !crate::io::is_file(&path) {
            return "missing";
        }
        let now = match crate::io::metadata(&path) {
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
            _ => match read_text(&path) {
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
        let _ = (l, checks, document);
        "ok"
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// A graphic showing an asset: the item, its page, and the transform from its parent's space to
/// the spread.
struct Shown<'a> {
    item: &'a Item,
    page: Option<usize>,
    parent_xf: Affine,
}

/// Where an asset shows: frames on spreads (see [`uses`]) and anchored or inline objects, whose
/// page and position come from the frame their story lays them out in.
fn shown<'a>(d: &'a Document, cache: &Cache, aid: AssetId) -> Vec<Shown<'a>> {
    fn walk<'a>(it: &'a Item, xf: Affine, aid: AssetId, page: Option<usize>, out: &mut Vec<Shown<'a>>) {
        if let Content::Graphic(g) = &it.content
            && g.asset == aid
        {
            out.push(Shown { item: it, page, parent_xf: xf });
        }
        for c in it.children() {
            walk(c, xf * it.xf, aid, page, out);
        }
    }
    let mut out: Vec<Shown> = uses(d, aid)
        .into_iter()
        .filter_map(|id| {
            let loc = d.find(id)?;
            Some(Shown { item: d.item_at(&loc)?, page: d.page_of_item(id), parent_xf: d.parent_xf(&loc) })
        })
        .collect();
    for st in d.stories.values() {
        let mut composed = None;
        for (k, o) in st.objects.iter().enumerate() {
            let mut found = false;
            o.item.walk(&mut |i| found |= matches!(&i.content, Content::Graphic(g) if g.asset == aid));
            if !found {
                continue;
            }
            let cs = composed.get_or_insert_with(|| cache.get(d, st.id, None));
            let placed = cs.frames.iter().find_map(|ft| ft.objects.iter().find(|p| p.index == k).map(|p| (ft.frame, p.origin)));
            // Overset: no page; the resolution of the object as it would show.
            let (page, xf) = match placed {
                Some((fid, origin)) => {
                    let frame_xf =
                        d.find(fid).map_or(Affine::IDENTITY, |loc| d.parent_xf(&loc)) * d.item(fid).map_or(Affine::IDENTITY, |i| d.text_xf(i));
                    (d.page_of_item(fid), frame_xf * Affine::translate(origin.to_vec2()))
                }
                None => (None, Affine::IDENTITY),
            };
            walk(&o.item, xf, aid, page, &mut out);
        }
    }
    out
}

fn list(d: &Document, cache: &Cache, document: Option<&Path>, checks: &mut TextChecks) -> Vec<Value> {
    let texts = d.stories.values().filter_map(|st| {
        let l = st.file_link.as_ref()?;
        let uses: Vec<Value> = st.frames.iter().map(|id| json!({"id": id.0, "page": d.page_of_item(*id).map(|p| d.page_name(p))})).collect();
        Some(json!({"story": st.id.0, "name": file_name(&l.path), "path": l.path, "status": text_status(l, checks, document), "refused": refused(&l.path, document).map(|r| r.to_string()), "edited": story_hash(st) != l.story_hash, "uses": uses}))
    });
    let used = d.used_assets();
    d.assets
        .values()
        .filter(|a| used.contains(&a.id))
        .map(|a| {
            let uses: Vec<Value> = shown(d, cache, a.id)
                .into_iter()
                .map(|u| {
                    let it = u.item;
                    // Effective resolution: image pixels over the size it is shown at.
                    let ppi = (|| {
                        let Content::Graphic(g) = &it.content else { return None };
                        // Vector (placed PDF or SVG): no effective resolution.
                        if a.mime == "application/pdf" || a.mime == "image/svg+xml" {
                            return None;
                        }
                        let (pw, _) = a.pixels?;
                        let xf = u.parent_xf * it.xf * g.xf;
                        let shown_w = xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1)).width();
                        (shown_w > 0.0).then(|| (pw as f64 / (shown_w / 72.0)).round())
                    })();
                    let page = u.page.map(|p| d.page_name(p));
                    json!({"id": it.id.0, "page": page, "ppi": ppi})
                })
                .collect();
            let pdf_crop = (a.mime == "application/pdf").then(|| a.pdf_crop.name());
            let why = a.link.as_deref().and_then(|l| refused(l, document)).map(|r| r.to_string());
            json!({"asset": a.id.0, "name": a.name, "path": a.link, "status": status(a, document), "refused": why, "pixels": a.pixels, "pdfCrop": pdf_crop, "uses": uses})
        })
        .chain(texts)
        .collect()
}

/// A linked text file's bytes, with its size and time from before they were read.
#[cfg(not(target_arch = "wasm32"))]
fn read_text(path: impl AsRef<Path>) -> Result<(Vec<u8>, Fingerprint)> {
    let path = path.as_ref();
    let err = |e: std::io::Error| EngineError::Other(format!("{}: {e}", path.display()));
    // The roots' own message when they refuse the path.
    crate::file_access::check_read(path).map_err(EngineError::Other)?;
    let m = crate::io::metadata(path).map_err(err)?;
    if !crate::io::is_file(path) {
        return Err(EngineError::Other(format!("{}: not a file", path.display())));
    }
    let too_large = || EngineError::Other(format!("{}: larger than {} MiB", path.display(), MAX_TEXT_LINK_BYTES / (1024 * 1024)));
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
fn read_text(path: impl AsRef<Path>) -> Result<(Vec<u8>, Fingerprint)> {
    Err(EngineError::Other(format!("{}: files can't be read on the web", path.as_ref().display())))
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
fn reload(path: impl AsRef<Path>) -> Result<(Vec<u8>, (u32, u32))> {
    reload_capped(path, super::interchange::MAX_LINKED_FILE_BYTES)
}

/// A linked graphic read again from disk, and its pixel size, reading at most `cap` bytes.
///
/// How large a linked file is belongs to whoever wrote the document, not to whoever opens it: the
/// path comes out of the document, and Update, Relink and Relink Folder all follow it. An
/// unbounded `read` of a link pointing at a multi-GB file exhausted memory (#1596). The cap is
/// the one packaging already applies to a linked file, and a file over it is refused by name
/// rather than read.
#[cfg(not(target_arch = "wasm32"))]
fn reload_capped(path: impl AsRef<Path>, cap: u64) -> Result<(Vec<u8>, (u32, u32))> {
    let path = path.as_ref();
    let b = crate::io::read_capped(path, cap).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
    if b.len() as u64 > cap {
        return Err(bad("links", format!("{}: the linked file is larger than {} MB", path.display(), cap / (1024 * 1024))));
    }
    if designcraft_render::is_pdf(&b) {
        designcraft_render::check_pdf_input_size(b.len()).map_err(|e| bad("links", format!("{}: {e}", path.display())))?;
    }
    let px = designcraft_render::image_size(&b).ok_or_else(|| bad("links", format!("{}: unsupported or corrupt image", path.display())))?;
    Ok((b, px))
}

#[cfg(target_arch = "wasm32")]
fn reload(path: impl AsRef<Path>) -> Result<(Vec<u8>, (u32, u32))> {
    Err(EngineError::Other(format!("{}: files can't be read on the web", path.as_ref().display())))
}

/// The replacement for `a`: bytes read from `read`, with `link` recorded as the stored link.
/// Relink passes the file the user chose as both. Update passes the path the stored link resolved
/// to and the stored link itself, so refreshing a link never rewrites what the document holds.
fn relinked_asset(a: &designcraft_doc::Asset, read: &Path, link: &str) -> Result<Arc<designcraft_doc::Asset>> {
    let (bytes, px) = reload(read)?;
    let page = if designcraft_render::is_pdf(&bytes) { a.page } else { 0 };
    let px = if page > 0 {
        designcraft_render::pdf_page_size(&bytes, page as usize)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
            .ok_or_else(|| bad("links", format!("{}: can't read PDF page {}", read.display(), u64::from(page) + 1)))?
    } else {
        px
    };
    let mut a = a.clone();
    a.mime = designcraft_render::image_mime(&bytes).to_string();
    a.data = Arc::new(bytes);
    a.pixels = Some(px);
    a.page = page;
    a.name = Path::new(link).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| link.to_string());
    a.link = Some(link.to_string());
    // A placed PDF keeps its crop choice; its box is found on the new file.
    super::file::crop_pdf(&mut a);
    Ok(Arc::new(a))
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
    let a = s.doc()?.doc.assets.get(&aid).ok_or_else(|| bad("links.relink", "no such graphic"))?;
    // Build the replacement before the edit: a file that cannot be read leaves the document as it was.
    let fresh = relinked_asset(a, Path::new(&path), &path)?;
    s.edit(|d, _| {
        let name = fresh.name.clone();
        d.assets.insert(aid, fresh);
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
    if ext.as_ref().is_some_and(|e| e.contains(['/', '\\', '\0'])) {
        return Err(bad("links.relinkFolder", "extension must not contain path separators or NUL"));
    }
    if dir.is_none() && ext.is_none() {
        return Err(bad("links.relinkFolder", "give `dir` and/or `extension`"));
    }
    let missing_only = p.get("missingOnly").and_then(Value::as_bool).unwrap_or(dir.is_some() && ext.is_none());
    let only: Option<Vec<u64>> = p.get("assets").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect());
    let st = s.doc()?;
    let (d, document) = (&st.doc, st.path.as_deref().map(Path::new));
    let mut todo: Vec<(AssetId, std::path::PathBuf)> = Vec::new();
    let mut not_found = Vec::new();
    for a in d.assets.values() {
        if only.as_ref().is_some_and(|o| !o.contains(&a.id.0)) || (missing_only && status(a, document) != "missing") {
            continue;
        }
        let old = std::path::PathBuf::from(a.link.clone().unwrap_or_else(|| a.name.clone()));
        let mut name = old.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_else(|| a.name.clone().into());
        if let Some(e) = &ext {
            name = std::path::Path::new(&name).with_extension(e).into_os_string();
        }
        // The link's own folder only where the link may be followed; the name only as a plain
        // file name (not a device).
        let folder = match &dir {
            Some(dir) => Some(dir.clone()),
            None => a.link.as_deref().and_then(|l| crate::io::document_path(l, document).ok()).and_then(|p| p.parent().map(Path::to_path_buf)),
        };
        let plain_name = name.to_str().filter(|n| !n.contains(['/', '\\']));
        // A name that resolves to another file name (a `file:` URL such as `file:%2Ftmp%2Fx.png`,
        // a drive-relative `C:x.png`) or a folder the stored-path rules read differently (`a\b`
        // on Unix, `alias/..` through a symlink) could lead outside the folder, so only the name
        // itself, directly in the folder as the file system resolves it, counts. The link keeps
        // the folder as given.
        let cand = folder.zip(plain_name).and_then(|(f, n)| {
            let c = crate::io::folder_path(n, &f).ok()?;
            let real = |p: &Path| crate::io::canonicalize(p).ok();
            let same_folder = c.parent().and_then(real).is_some_and(|p| real(&f) == Some(p));
            (c.file_name().is_some_and(|c| c == n) && same_folder).then_some(c)
        });
        if let Some(cand) = cand.filter(|c| crate::io::exists(c)) {
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
        fresh.push((aid, relinked_asset(a, &path, &path.to_string_lossy())?));
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
    let (d, document) = {
        let st = s.doc()?;
        (st.doc.clone(), st.path.clone())
    };
    let document = document.as_deref().map(Path::new);
    let checks = &mut s.text_link_checks;
    let (only_asset, only_story) = (p.get("asset").and_then(Value::as_u64), story_param(p));
    let all = only_asset.is_none() && only_story.is_none();
    // A link craft-docpath refuses is missing, so it is never read or refreshed.
    let resolved = |link: &str| crate::io::document_path(link, document).ok();
    let texts: Vec<(StoryId, String, std::path::PathBuf)> = d
        .stories
        .values()
        .filter(|st| all || only_story == Some(st.id))
        .filter_map(|st| {
            let l = st.file_link.as_ref().filter(|l| text_status(l, checks, document) == "modified")?;
            Some((st.id, l.path.clone(), resolved(&l.path)?))
        })
        .collect();
    let mut fresh = Vec::new();
    for a in d.assets.values().filter(|a| all || only_asset == Some(a.id.0)).filter(|a| status(a, document) == "modified") {
        // The bytes come from where the stored link resolves; the link itself stays as the
        // document stored it, so refreshing never rewrites a relative link to an absolute one.
        if let Some((link, at)) = a.link.as_deref().and_then(|l| resolved(l).map(|at| (l, at))) {
            fresh.push((a.id, relinked_asset(a, &at, link)?));
        }
    }
    let mut fresh_texts = Vec::new();
    for (sid, path, at) in texts {
        let (bytes, before) = read_text(&at)?;
        fresh_texts.push((sid, path, bytes, before));
    }
    let n = fresh.len() + fresh_texts.len();
    if n == 0 {
        return Ok(json!({"updated": 0}));
    }
    s.edit(|d, _| {
        for (aid, a) in fresh {
            d.assets.insert(aid, a);
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

    /// A linked file's size is the document's choice, so a link is read under a cap (#1596).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_linked_file_over_the_cap_is_refused_instead_of_read() {
        let dir = std::env::temp_dir().join(format!("dc-link-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.png");
        let bytes = png(40, 40);
        std::fs::write(&path, &bytes).unwrap();

        // Under the cap the file is read and measured as before.
        let (read, px) = reload_capped(&path, bytes.len() as u64).expect("a file within the cap is read");
        assert_eq!((read.len(), px), (bytes.len(), (40, 40)));

        // Over it the file is refused by name, and the error says what the limit is.
        let err = reload_capped(&path, (bytes.len() - 1) as u64).expect_err("a file over the cap is refused");
        let msg = err.to_string();
        assert!(msg.contains("big.png") && msg.contains("larger than"), "the error names the file and the limit: {msg}");

        // The real cap is the one packaging uses, so an ordinary graphic still loads.
        assert!(reload(&path).is_ok(), "a normal linked graphic is unaffected");
        let _ = std::fs::remove_dir_all(&dir);
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
    fn anchored_image_lists_its_page_and_resolution() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2, "facingPages": false})).unwrap();
        let r = s
            .execute("frame.create", &json!({"spread": 1, "rect": [72, 72, 400, 400], "content": "text", "text": "Before after and more words."}))
            .unwrap();
        let sid = s.doc().unwrap().doc.item(ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        let placed = s
            .execute(
                "file.place",
                &json!({"base64": super::super::file::base64_encode(&png(10, 10)), "name": "dot.png", "x": 450, "y": 450, "width": 36}),
            )
            .unwrap();
        let id = placed["id"].as_u64().unwrap();
        let before = s.execute("links.list", &json!({})).unwrap();
        assert_eq!(before[0]["uses"][0]["ppi"], 20.0);
        s.execute("anchored.insert", &json!({"ids": [id], "story": sid.0, "pos": 7})).unwrap();
        let list = s.execute("links.list", &json!({})).unwrap();
        let uses = list[0]["uses"].as_array().unwrap();
        assert_eq!(uses.len(), 1, "anchored image must have one use: {list}");
        assert_eq!(uses[0]["page"], "2");
        assert_eq!(uses[0]["ppi"], 20.0);
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
        z.by_name(&format!("OEBPS/images/logo-{}.png", aid.0)).unwrap().read_to_end(&mut image).unwrap();
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
        // The asset is embedded, so no link is followed and the document is irrelevant.
        assert_eq!(status(&embedded.assets[&aid], None), "embedded");
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
        archive.by_name(&format!("OEBPS/images/logo-{}.png", aid.0)).unwrap().read_to_end(&mut image).unwrap();
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
            let params = match command {
                "links.relinkFolder" => json!({"dir": dir.join("new").to_string_lossy(), "missingOnly": false}),
                "links.update" => json!({"asset": aid.0}),
                _ => json!({"asset": aid.0, "path": replacement.to_string_lossy()}),
            };
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
        // `revision` is drawn from a process-wide counter, so a test running beside this one
        // can take the next value: that the batch commits *once* is what the single undo step is.
        assert_ne!(st.revision, revision, "the batch commits");
        assert_eq!(st.history.undo.len(), undo + 1, "exactly one undo step");
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
            data: Arc::new(data),
            ..Default::default()
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
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
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
                ..Default::default()
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
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        src.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let ids = super::super::object::duplicate_from(&mut dst, &src, &[iid], SpreadRef::Doc(0), designcraft_geom::Vec2::ZERO).unwrap();
        let g = dst.item(ids[0]).and_then(|i| i.graphic()).unwrap();
        assert_ne!(g.asset, missing);
        assert!(!dst.assets.contains_key(&g.asset));
    }

    #[test]
    fn pasted_missing_graphic_never_takes_an_asset_placed_later() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Graphic, Item, ItemId, Shape, SpreadRef};
        let mut src = Document::new(&NewDocument::default());
        let mut dst = Document::new(&NewDocument::default());
        // The source's missing id is ahead of the target's counter: the target would allocate it to its next image.
        for _ in 0..50 {
            src.alloc();
        }
        let missing = AssetId(src.alloc());
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
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [1.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        src.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let ids = super::super::object::duplicate_from(&mut dst, &src, &[iid], SpreadRef::Doc(0), designcraft_geom::Vec2::ZERO).unwrap();
        let g = dst.item(ids[0]).and_then(|i| i.graphic()).unwrap().asset;
        assert!((0..100).all(|_| AssetId(dst.alloc()) != g), "a later image in the target takes the pasted graphic's id");
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
        assert_eq!(super::text_status(back.story(sid).unwrap().file_link.as_ref().unwrap(), &mut Default::default(), None), "ok");

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
        promptly(move || text_status(&l, &mut Default::default(), None))
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
