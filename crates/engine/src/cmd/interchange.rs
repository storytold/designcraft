//! Interchange formats: IDML (InDesign Markup Language) import and export, and INDD/INDT
//! import (converted to IDML in memory by indd-utils).
//!
//! A package's linked files are read from where the link points on this computer, then from the
//! link's trailing names beside the package (where a package moved from its author's computer
//! keeps them). Links to other computers and devices are not followed
//! ([`crate::io::document_path`]). Only regular files that hold an image or a graphic format are
//! read, and only up to [`designcraft_doc::asset_data::MAX_LINKED_BYTES`].

use designcraft_doc::Document;
use serde_json::{Value, json};

use super::file::{base64_decode, base64_encode};
use super::{CommandSpec, always, bad, cmd, has_doc, str_param};
use crate::{DocState, EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "file.exportIdml", "Export IDML…", [], None,
            "{path?, embedImages?: true} — writes an IDML package to `path`, or returns {base64} without a path; `warnings` names each image written as a link because the embedded-image limit was reached",
            has_doc, export_idml),
        cmd!(noundo "file.openIdml", "Open IDML", [], None,
            "{path | base64, name?} — opens an IDML package, or an InDesign document or template (.indd, .indt: converted to IDML first; its conversion warnings join `warnings`), as a new document (linked images are read next to the file or from its Links/ folder; the fonts in a `Document Fonts` folder beside it load first) → {index, documentFonts, warnings: conversion warnings, package parts missing, font files skipped, GREP styles whose pattern can't compile}",
            always, open_idml),
    ]
}

fn export_idml(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let opts = designcraft_idml::ExportOptions { embed_images: p.get("embedImages").and_then(Value::as_bool).unwrap_or(true) };
    // A data-merge preview's fill is session state: the unfilled template is exported (as
    // `file.serialize` and the saves write it).
    let doc = st.preview_stash.as_deref().unwrap_or(st.doc.as_ref());
    let (bytes, warnings) = idml_bytes(doc, &opts)?;
    let mut out = match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &bytes).map_err(EngineError::Other)?;
            json!({"path": path, "bytes": bytes.len()})
        }
        None => json!({"base64": base64_encode(&bytes), "bytes": bytes.len()}),
    };
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(out)
}

/// Whether a placement holds an Object Layer Options choice an export can still apply: a graphic
/// whose asset is a PDF. A choice left on an item that is no longer a placed PDF (its link was
/// replaced with another format) is not one, so the package report does not name it.
pub(crate) fn has_applicable_pdf_layer_overrides(doc: &Document) -> bool {
    doc.placed_items().into_iter().any(|it| {
        it.has_pdf_layer_override()
            && it.graphic().and_then(|g| doc.assets.get(&g.asset)).is_some_and(|a| a.data.load().is_ok_and(|b| designcraft_render::is_pdf(&b)))
    })
}

/// Each placement gets its own embedded variant so two frames sharing a PDF can
/// choose different layers. The live document and original linked bytes stay untouched.
///
/// Parent-page and anchored placements count too, so a page that inherits the placement does not
/// quietly fall back to the file's defaults. Read and write walks also reach table-cell and
/// note text. Endnote variants are prepared too, but IDML endnote structures/artwork are not
/// serialized yet (see `docs/pdf-layer-visibility.md`).
/// `doc` as an IDML package, with its placed PDFs' layer choices (as Export IDML and an IDML save
/// write it).
pub(crate) fn idml_bytes(doc: &Document, opts: &designcraft_idml::ExportOptions) -> Result<(Vec<u8>, Vec<String>)> {
    let layer_doc = with_pdf_layer_variants(doc, opts.embed_images)?;
    Ok(designcraft_idml::export_idml_report(layer_doc.as_ref().unwrap_or(doc), opts))
}

fn with_pdf_layer_variants(doc: &Document, embed: bool) -> Result<Option<Document>> {
    let choices: Vec<_> = doc
        .placed_items()
        .into_iter()
        .filter(|it| it.has_pdf_layer_override())
        .filter_map(|it| {
            let asset =
                it.graphic().and_then(|g| doc.assets.get(&g.asset)).filter(|a| a.data.load().is_ok_and(|b| designcraft_render::is_pdf(&b)))?;
            Some((it.id, asset, it.pdf_hidden_layers.clone(), it.pdf_layer_override))
        })
        .collect();
    if choices.is_empty() {
        return Ok(None);
    }
    if !embed {
        return Err(EngineError::Other("PDF layer choices require embedded images in IDML export".into()));
    }
    let mut out = doc.clone();
    let mut variants = std::collections::HashMap::new();
    // Ids should be unique (#1522); placements that still share one reuse a variant only when source and choice agree.
    let mut expected = std::collections::HashMap::new();
    for (id, asset, hidden, overridden) in choices {
        let signature = (asset.id, hidden.clone(), overridden);
        if let Some(previous) = expected.get(&id) {
            if previous != &signature {
                return Err(EngineError::Other(format!("PDF layer choices: placements sharing id {id} have different sources or layer choices")));
            }
            continue;
        }
        expected.insert(id, signature);
        let source = asset.data.load().map_err(|e| EngineError::Other(format!("{}: {e}", asset.name)))?;
        let bytes = designcraft_render::pdf_hide_layers(&source, &hidden)
            .ok_or_else(|| EngineError::Other(format!("{}: PDF layer choices could not be applied", asset.name)))?;
        let mut variant = (**asset).clone();
        variant.id = designcraft_doc::AssetId(out.alloc());
        variant.data = designcraft_doc::AssetData::embedded(bytes);
        variant.set_link(None, None);
        let aid = variant.id;
        out.assets.insert(aid, std::sync::Arc::new(variant));
        variants.insert(id, aid);
    }
    // Anchored objects live in stories, and parent pages outside the document-spread walk, so
    // the choice is cleared where the placement actually is.
    // Count distinct ids, allowing identical duplicated anchors, but check every hit against
    // the original source and choice before rewriting (including placements using defaults).
    let mut reached = std::collections::HashSet::new();
    let mut conflicting_id = None;
    out.update_placed_items(
        |it| variants.contains_key(&it.id),
        |it| {
            reached.insert(it.id);
            if let Some((asset, hidden, overridden)) = expected.get(&it.id)
                && (it.graphic().map(|g| g.asset) != Some(*asset) || &it.pdf_hidden_layers != hidden || it.pdf_layer_override != *overridden)
            {
                conflicting_id = Some(it.id);
                return;
            }
            if let designcraft_doc::Content::Graphic(g) = &mut it.content
                && let Some(aid) = variants.get(&it.id)
            {
                g.asset = *aid;
            }
            it.pdf_layer_override = false;
            it.pdf_hidden_layers.clear();
        },
    );
    if let Some(id) = conflicting_id {
        return Err(EngineError::Other(format!("PDF layer choices: placements sharing id {id} have different sources or layer choices")));
    }
    if reached.len() != variants.len() {
        return Err(EngineError::Other(format!(
            "PDF layer choices could not be applied to every placement (reached {} of {})",
            reached.len(),
            variants.len()
        )));
    }
    Ok(Some(out))
}

/// [`designcraft_idml::import_idml`] with the linked images read through [`crate::io`] from
/// beside `package` (the package's file, if it has one), so the automation read root applies to
/// the paths the package names, and its PDF crops resolved.
pub(crate) fn import_idml_confined(bytes: &[u8], package: Option<&std::path::Path>) -> designcraft_idml::Result<Document> {
    import_within(bytes, package, &std::collections::HashMap::new(), designcraft_doc::asset_data::MAX_LINKED_BYTES).map(|(document, _, _)| document)
}

/// Import IDML bytes; `package` is the package's file (its folder is where links are looked for).
pub fn import(bytes: &[u8], package: Option<&std::path::Path>) -> Result<Document> {
    import_report(bytes, package).map(|(document, _)| document)
}

/// [`import`], with the problems that did not stop it (parts missing from the package, …).
pub fn import_report(bytes: &[u8], package: Option<&std::path::Path>) -> Result<(Document, Vec<String>)> {
    import_report_with_previews(bytes, package, &std::collections::HashMap::new()).map(|(document, warnings, _)| (document, warnings))
}

/// [`import_report`], showing `previews` (graphic `Self` → image) for linked files that can't be read.
fn import_report_with_previews(
    bytes: &[u8],
    package: Option<&std::path::Path>,
    previews: &std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>,
) -> Result<(Document, Vec<String>, std::collections::HashMap<String, designcraft_doc::StoryId>)> {
    import_within(bytes, package, previews, designcraft_doc::asset_data::MAX_LINKED_BYTES).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
struct ResolvedFile {
    fingerprint: Option<designcraft_doc::Fingerprint>,
    /// Populated once the first placement of this resolved file is linked, so other pages,
    /// PDF crops, or link spellings of the same file reuse its [`designcraft_doc::SourceKey`],
    /// content hash, and resident store entry instead of reading and hashing the file again.
    linked: Option<(designcraft_doc::AssetData, String)>,
}

fn import_within(
    bytes: &[u8],
    package: Option<&std::path::Path>,
    previews: &std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>,
    cap: u64,
) -> designcraft_idml::Result<(Document, Vec<String>, std::collections::HashMap<String, designcraft_doc::StoryId>)> {
    crate::asset_io::install();
    #[cfg(not(target_arch = "wasm32"))]
    let resolved_links = std::cell::RefCell::new(std::collections::HashMap::<String, (String, bool)>::new());
    #[cfg(not(target_arch = "wasm32"))]
    let resolved_files = std::cell::RefCell::new(std::collections::HashMap::<String, ResolvedFile>::new());
    #[cfg(not(target_arch = "wasm32"))]
    let crop_sizes =
        std::cell::RefCell::new(std::collections::HashMap::<(designcraft_doc::SourceKey, u32, designcraft_doc::PdfCrop), (f64, f64)>::new());
    #[cfg(target_arch = "wasm32")]
    let _ = cap;
    let read = |link: &str| -> Option<Vec<u8>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let cached = resolved_links.borrow().get(link).cloned();
            if let Some((path, _)) = cached
                && let Some((data, _)) = resolved_files.borrow().get(&path).and_then(|file| file.linked.as_ref())
                && let Some(bytes) = data.resident()
            {
                resolved_links.borrow_mut().insert(link.to_string(), (path, true));
                return Some((*bytes).clone());
            }
            let (path, data, fingerprint) = read_packaged_link(link, package, cap)?;
            resolved_links.borrow_mut().insert(link.to_string(), (path.clone(), false));
            resolved_files
                .borrow_mut()
                .entry(path)
                .and_modify(|file| file.fingerprint = fingerprint)
                .or_insert(ResolvedFile { fingerprint, linked: None });
            Some(data)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (link, &package);
            None
        }
    };
    let link_asset = |link: &str, asset: &mut designcraft_doc::Asset| {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some((path, from_resident)) = resolved_links.borrow().get(link).cloned() else { return };
            let (fingerprint, existing) = {
                let files = resolved_files.borrow();
                let Some(file) = files.get(&path) else { return };
                (file.fingerprint, file.linked.as_ref().map(|(data, hash)| (data.key(), hash.clone())))
            };
            // Relocated packages must also use the new path for preflight and link updates.
            asset.set_link(Some(path.clone()), fingerprint);
            // Resolve fixed PDF page boxes while the bytes are still in memory, before linking
            // moves them to external storage.
            let crop_size = (!asset.content_box_pending())
                .then(|| asset.data.embedded_bytes().cloned().and_then(|b| super::file::crop_pdf_from(asset, &b)))
                .flatten();
            if let Some((data, hash)) = link_read_file_with(asset, package, existing, from_resident) {
                if let Some(size) = crop_size {
                    crop_sizes.borrow_mut().insert((data.key(), asset.page, asset.pdf_crop), size);
                }
                if let Some(file) = resolved_files.borrow_mut().get_mut(&path)
                    && file.linked.as_ref().map(|(d, _)| d.key()) != Some(data.key())
                {
                    file.linked = Some((data, hash));
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (link, asset);
    };
    let designcraft_idml::Imported { mut document, warnings, story_ids } = designcraft_idml::import_idml_linked(bytes, &read, previews, &link_asset)?;
    #[cfg(not(target_arch = "wasm32"))]
    for asset in document.assets.values_mut() {
        if asset.storage != designcraft_doc::Storage::Linked
            && let Some(link) = asset.link.as_deref()
            && let Some((path, _)) = resolved_links.borrow().get(link).cloned()
        {
            // Embedded graphics sharing a relocated link URI also keep the relocated path.
            let keep_original =
                !std::path::Path::new(link).is_relative() && crate::io::document_path(link, package).is_ok_and(|p| crate::io::is_file(&p));
            if !keep_original {
                let asset = std::sync::Arc::make_mut(asset);
                asset.set_link(Some(path), None);
                asset.proxy = None;
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    resolve_pdf_crops(&mut document, &crop_sizes.borrow());
    #[cfg(target_arch = "wasm32")]
    resolve_pdf_crops(&mut document, &std::collections::HashMap::new());
    #[cfg(not(target_arch = "wasm32"))]
    resolve_packaged_links(&mut document, package);
    Ok((document, warnings, story_ids))
}

/// `asset`, whose embedded bytes were just read from the file its link names, as a linked asset
/// ([`super::links::link_to_file`]), drawn while the file can't be read with a proxy: the
/// document's preview of it when that shows the whole image ([`preview_proxy`],
/// [`shows_whole`]), else one made from the file, else the preview.
#[cfg(all(test, not(target_arch = "wasm32")))]
fn link_read_file(asset: &mut designcraft_doc::Asset, package: Option<&std::path::Path>) {
    let _ = link_read_file_with(asset, package, None, false);
}

#[cfg(not(target_arch = "wasm32"))]
fn link_read_file_with(
    asset: &mut designcraft_doc::Asset,
    package: Option<&std::path::Path>,
    existing: Option<(designcraft_doc::SourceKey, String)>,
    from_resident: bool,
) -> Option<(designcraft_doc::AssetData, String)> {
    let bytes = asset.data.embedded_bytes().cloned()?;
    let path = asset.link.clone()?;
    let preview = asset.proxy.take().and_then(|p| preview_proxy(&p));
    let (whole, fallback) = match preview {
        Some(p) if shows_whole(asset, &bytes, &p) => (Some(p), None),
        p => (None, p),
    };
    let (key, hash) = match existing {
        Some((key, known)) if from_resident => (key, known),
        Some((key, known)) => {
            let current = crate::asset_io::content_hash(&bytes);
            if known == current { (key, known) } else { (designcraft_doc::SourceKey::fresh(), current) }
        }
        None => (designcraft_doc::SourceKey::fresh(), crate::asset_io::content_hash(&bytes)),
    };
    // The fingerprint kept by the read (none for a file just written: its hash tells its bytes).
    let file = super::links::FileLink::of(std::path::Path::new(&path), asset.link_fingerprint);
    super::links::link_to_file_under(asset, bytes, file, key, hash.clone(), package, whole);
    if asset.storage == designcraft_doc::Storage::Linked && asset.proxy.is_none() {
        asset.proxy = fallback;
    }
    (asset.storage == designcraft_doc::Storage::Linked).then(|| (asset.data.clone(), hash))
}

/// A document's stored preview of a linked graphic as a proxy: checked to decode, and made again
/// within the proxy limits ([`designcraft_render::proxy`]) when it is bigger or in another
/// format (a larger one wouldn't be kept by the saved document). `None` when it doesn't decode.
#[cfg(not(target_arch = "wasm32"))]
fn preview_proxy(preview: &designcraft_doc::Proxy) -> Option<designcraft_doc::Proxy> {
    use designcraft_render::proxy::{MAX_BYTES, MAX_SIDE_PX, make_proxy};
    let bytes = preview.data.embedded_bytes()?;
    let mime = designcraft_render::image_mime(bytes);
    let fits = bytes.len() <= MAX_BYTES
        && matches!(mime, "image/jpeg" | "image/png")
        && designcraft_render::image_size(bytes).is_some_and(|(w, h)| w.max(h) <= MAX_SIDE_PX);
    if fits && let Some(pm) = designcraft_render::decode_pixmap(bytes) {
        let pixels = (u32::from(pm.width()), u32::from(pm.height()));
        return Some(designcraft_doc::Proxy { mime: mime.into(), pixels, data: preview.data.clone() });
    }
    let p = make_proxy(bytes, 0)?;
    Some(designcraft_doc::Proxy { mime: p.mime.into(), pixels: p.pixels, data: designcraft_doc::AssetData::embedded(p.data) })
}

/// Whether `preview` shows the whole of `asset`'s image (`bytes`, its file's), as a proxy must:
/// a raster whose preview has the image's proportions. A PDF's preview shows its crop, which may
/// not be the page the proxy stands for, and other files' proportions aren't known here.
#[cfg(not(target_arch = "wasm32"))]
fn shows_whole(asset: &designcraft_doc::Asset, bytes: &[u8], preview: &designcraft_doc::Proxy) -> bool {
    if designcraft_render::is_pdf(bytes) || asset.mime == "application/pdf" {
        return false;
    }
    let Some((w, h)) = designcraft_render::image_size(bytes) else { return false };
    let (pw, ph) = preview.pixels;
    if w == 0 || h == 0 || pw == 0 || ph == 0 {
        return false;
    }
    // Proportions equal but for the preview's rounding to whole pixels.
    let (ratio, preview_ratio) = (f64::from(w) / f64::from(h), f64::from(pw) / f64::from(ph));
    (preview_ratio / ratio - 1.0).abs() <= 1.0 / f64::from(pw) + 1.0 / f64::from(ph) + 0.01
}

/// Placed PDFs read from IDML name their crop (`PDFCrop`): find where each box sits on its page,
/// which the IDML reader can't (it doesn't parse PDFs). The box spans the graphic's
/// `GraphicBounds`; a graphic without bounds takes the box's size. Bounding Box crops interpret
/// the page's drawing to find the box, so they wait for drawing or export ([`designcraft_render::shown_box`],
/// [`with_pdf_boxes`]) unless a graphic needs the box's size now.
fn resolve_pdf_crops(
    d: &mut Document,
    known_sizes: &std::collections::HashMap<(designcraft_doc::SourceKey, u32, designcraft_doc::PdfCrop), (f64, f64)>,
) {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use designcraft_doc::{AssetId, Content, Item, PdfCrop};
    let mut no_bounds: HashSet<AssetId> = HashSet::new();
    for sp in d.spreads.iter().chain(&d.parents) {
        for it in &sp.items {
            it.walk(&mut |i| {
                if let Content::Graphic(g) = &i.content
                    && !(g.size.0 > 0.0 && g.size.1 > 0.0)
                {
                    no_bounds.insert(g.asset);
                }
            });
        }
    }
    let mut sizes: HashMap<AssetId, (f64, f64)> = HashMap::new();
    for a in d.assets.values_mut() {
        if a.pdf_crop == PdfCrop::Crop || (a.content_box_pending() && !no_bounds.contains(&a.id)) {
            continue;
        }
        if a.pdf_box.is_some() {
            if no_bounds.contains(&a.id)
                && let Some(&size) = known_sizes.get(&(a.data.key(), a.page, a.pdf_crop))
            {
                sizes.insert(a.id, size);
            }
            continue;
        }
        let a = Arc::make_mut(a);
        if let Some(size) = super::file::crop_pdf(a) {
            sizes.insert(a.id, size);
        }
    }
    let no_size = |i: &Item| matches!(&i.content, Content::Graphic(g) if !(g.size.0 > 0.0 && g.size.1 > 0.0) && sizes.contains_key(&g.asset));
    fn fix(it: &mut Arc<Item>, sizes: &HashMap<AssetId, (f64, f64)>, no_size: &dyn Fn(&Item) -> bool) {
        let mut any = false;
        it.walk(&mut |i| any |= no_size(i));
        if !any {
            return;
        }
        let it = Arc::make_mut(it);
        if let Content::Graphic(g) = &mut it.content
            && let Some(size) = sizes.get(&g.asset)
            && !(g.size.0 > 0.0 && g.size.1 > 0.0)
        {
            g.size = *size;
        }
        for k in it.children_mut().into_iter().flatten() {
            fix(k, sizes, no_size);
        }
    }
    if sizes.is_empty() {
        return;
    }
    for sp in d.spreads.iter_mut().chain(d.parents.iter_mut()) {
        for it in &mut Arc::make_mut(sp).items {
            fix(it, &sizes, &no_size);
        }
    }
}

/// `d` with the boxes of its Bounding Box crops measured ([`resolve_pdf_crops`] leaves them for
/// later), for writers that don't render: PDF export places the page by the box.
pub(crate) fn with_pdf_boxes(d: &Document) -> std::borrow::Cow<'_, Document> {
    if !d.assets.values().any(|a| a.content_box_pending()) {
        return std::borrow::Cow::Borrowed(d);
    }
    let mut out = d.clone();
    for a in out.assets.values_mut() {
        if a.content_box_pending() {
            // No usable box: the whole page.
            std::sync::Arc::make_mut(a).pdf_box = designcraft_render::shown_box(a);
        }
    }
    std::borrow::Cow::Owned(out)
}

#[cfg(not(target_arch = "wasm32"))]
const MAX_LINK_PATH_BYTES: usize = 128 * 1024;

/// How many trailing folders of a link are tried beside the opened package.
#[cfg(not(target_arch = "wasm32"))]
const MAX_LINK_SUFFIX_PARTS: usize = 8;

/// The files to try for `link` in the package at `package`: the link itself, then its trailing
/// names beside the package (without a package file, only an absolute link). Each is a path
/// [`crate::io::document_path`] allows.
#[cfg(not(target_arch = "wasm32"))]
fn packaged_link_candidates(link: &str, package: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    // Enough for a maximum-length Windows path even with four-byte UTF-8 characters.
    // Bound splitting/allocation for link strings supplied by imported documents.
    if link.len() > MAX_LINK_PATH_BYTES {
        return Vec::new();
    }
    let mut candidates: Vec<std::path::PathBuf> = crate::io::document_path(link, package).into_iter().collect();
    let Some(package) = package else { return candidates };
    // Names relative to the package, resolved beside it.
    let mut beside = |names: &[&str]| {
        if let Ok(path) = crate::io::document_path(&names.join("/"), Some(package))
            && !candidates.contains(&path)
        {
            candidates.push(path);
        }
    };
    // IDML may retain Windows paths even when the package is opened on another platform.
    let parts: Vec<&str> = link.split(['/', '\\']).filter(|part| !part.is_empty()).collect();
    let safe_part = |part: &&str| *part != "." && *part != ".." && !part.contains(':');
    // Packagers can keep subfolders under Links (for example, Links/illustrations/logo.ai).
    // Preserve that suffix instead of flattening every resource to its basename.
    if let Some(index) = parts.iter().rposition(|part| part.eq_ignore_ascii_case("Links")) {
        let suffix = parts.get(index + 1..).unwrap_or_default();
        if !suffix.is_empty() && suffix.iter().all(safe_part) {
            beside(&[&["Links"][..], suffix].concat());
        }
    }
    // A package moved off its author's machine keeps its own subfolders (`/images/logo.png`
    // beside the .idml as `images/logo.png`): try the link's trailing folders, longest first.
    let first = parts.len().saturating_sub(MAX_LINK_SUFFIX_PARTS);
    for start in first..parts.len().saturating_sub(1) {
        if let Some(suffix) = parts.get(start..).filter(|s| s.iter().all(safe_part)) {
            beside(suffix);
        }
    }
    if let Some(name) = parts.last().filter(|part| safe_part(part)) {
        beside(&[name]);
        beside(&["Links", name]);
    }
    candidates
}

/// Bytes an image or a graphic can be placed from (raster formats, PDF, SVG, PSD, EPS).
#[cfg(not(target_arch = "wasm32"))]
fn is_graphic(bytes: &[u8]) -> bool {
    designcraft_images::mime(bytes) != "application/octet-stream" || designcraft_images::is_eps(bytes)
}

/// Embedded images do not call the IDML resource loader. Rebind their missing links
/// beside the opened document too, without replacing their embedded image bytes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_packaged_links(document: &mut Document, package: Option<&std::path::Path>) {
    let Some(package) = package else { return };
    for asset in document.assets.values_mut() {
        // Linked storage finds its files itself (`native_links::resolve`).
        if asset.storage == designcraft_doc::Storage::Linked {
            continue;
        }
        let Some(link) = asset.link.as_deref() else { continue };
        if link.len() > MAX_LINK_PATH_BYTES {
            continue;
        }
        if let Ok(path) = crate::io::document_path(link, Some(package))
            && crate::io::is_file(&path)
        {
            // A link relative to the document (what File › Package writes, `Links/logo.png`) is
            // held as the file it names, as every other link is, so preflight and updates find it.
            if std::path::Path::new(link).is_relative() {
                // The same file under its full name: what is kept of it stays.
                let loaded = asset.link_fingerprint;
                std::sync::Arc::make_mut(asset).set_link(Some(path.to_string_lossy().into_owned()), loaded);
            }
            continue;
        }
        let Some(path) = packaged_link_candidates(link, Some(package)).into_iter().find(|p| crate::io::is_file(p)) else { continue };
        // Another file than the link named, found again by name: the bytes weren't read from it.
        std::sync::Arc::make_mut(asset).set_link(Some(path.to_string_lossy().into_owned()), None);
    }
}

/// The first candidate for `link` ([`packaged_link_candidates`]) that is a regular file no larger
/// than `cap` ([`designcraft_doc::asset_data::MAX_LINKED_BYTES`]) holding an image or a graphic,
/// with its path and the size and time to keep with its bytes ([`super::links::loaded_fingerprint`]).
#[cfg(not(target_arch = "wasm32"))]
fn read_packaged_link(link: &str, package: Option<&std::path::Path>, cap: u64) -> Option<(String, Vec<u8>, Option<designcraft_doc::Fingerprint>)> {
    for path in packaged_link_candidates(link, package) {
        if !crate::io::is_file(&path) {
            continue;
        }
        // The version about to be read: one that replaces it during the read (a syncing or
        // network file) has other metadata, and the next Links check compares the bytes.
        let Ok(metadata) = crate::io::metadata(&path) else { continue };
        if metadata.len() > cap {
            continue;
        }
        let before = super::datamerge::fingerprint_from(&metadata);
        // At most one byte over the limit, in case the file grew since.
        let Ok(bytes) = crate::io::read_capped(&path, cap) else { continue };
        if bytes.len() as u64 > cap || !is_graphic(&bytes) {
            continue;
        }
        let loaded = super::links::loaded_fingerprint(Some(before), &bytes);
        return Some((path.to_string_lossy().into_owned(), bytes, loaded));
    }
    None
}

pub(crate) fn open_idml(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(b) = str_param(p, "base64") {
        let name = str_param(p, "name").unwrap_or_default();
        return import_interchange(s, base64_decode(b), None, name.to_string(), without_document_extension(name).to_string());
    }
    let Some(path) = str_param(p, "path") else { return Err(bad("file.openIdml", "missing `path` or `base64`")) };
    #[cfg(not(target_arch = "wasm32"))]
    let b = crate::io::read_file(path).map_err(EngineError::Other)?;
    #[cfg(target_arch = "wasm32")]
    let b: Vec<u8> = Vec::new();
    open_idml_file(s, b, path)
}

/// Open the IDML or InDesign file at `path`, whose content is `bytes`: titled by its file name,
/// with the fonts in the `Document Fonts` folder beside it.
pub(crate) fn open_idml_file(s: &mut Session, bytes: Vec<u8>, path: &str) -> Result<Value> {
    let pp = std::path::Path::new(path);
    let file = pp.file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string());
    let title = pp.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    import_interchange(s, bytes, Some(pp), file, title)
}

/// Import IDML or InDesign `bytes` named `file` as a new document titled `title`. `package` is
/// the file they were read from, which links resolve against and whose folder's fonts load.
fn import_interchange(s: &mut Session, bytes: Vec<u8>, package: Option<&std::path::Path>, file: String, title: String) -> Result<Value> {
    let mut warnings = Vec::new();
    // InDesign's previews of linked graphics show while their files are missing (graphic `Self` →
    // preview), as InDesign shows them. One preview can stand for any number of graphics: they
    // share its buffer.
    let mut previews = std::collections::HashMap::new();
    let from_indd = is_indd(&file, &bytes);
    let bytes = if from_indd {
        // What the converter can't read still comes over as IDML (#21).
        let conversion = craft_indd::convert(&bytes, &file).map_err(|e| {
            EngineError::Other(format!(
                "can't convert the InDesign file: {e}. In InDesign, choose File ▸ Export… ▸ InDesign Markup (IDML) and open the .idml in DesignCraft."
            ))
        })?;
        warnings.extend(conversion.warnings.iter().map(|w| w.message().to_string()));
        for p in conversion.previews {
            let data = std::sync::Arc::new(p.data);
            for g in p.graphics {
                previews.insert(g, std::sync::Arc::clone(&data));
            }
        }
        conversion.idml
    } else {
        bytes
    };
    let (mut d, import_warnings, story_ids) = import_report_with_previews(&bytes, package, &previews)?;
    warnings.extend(import_warnings);
    if !title.is_empty() {
        d.title = title;
    }
    let (fonts, faces, font_warnings) = match package.and_then(std::path::Path::to_str) {
        Some(path) => super::file::load_document_fonts(&mut d, path),
        None => (None, 0, Vec::new()),
    };
    warnings.extend(font_warnings);
    warnings.extend(super::style::grep_style_warnings(&d));
    let warning_items: Vec<Value> = warnings.iter().map(|w| warning_item(&d, &story_ids, w).map_or(Value::Null, |i| json!(i.0))).collect();
    // Saving IDML (Preferences › File Handling): an opened .idml file is the document's file, and
    // Save writes IDML back to it. Anything else starts unsaved, so neither a converted .indd nor a
    // file that isn't named .idml is ever overwritten (nor is any .idml with the native format).
    let path = match s.prefs.save_format {
        crate::SaveFormat::Idml if !from_indd => {
            package.and_then(std::path::Path::to_str).filter(|path| crate::SaveFormat::of_path(path) == crate::SaveFormat::Idml).map(str::to_string)
        }
        _ => None,
    };
    let mut st = DocState::new(d, path);
    st.fonts = fonts;
    let i = s.add_document(st);
    Ok(json!({"index": i, "documentFonts": faces, "warnings": warnings, "warningItems": warning_items}))
}

/// The frame an import warning is about, so it can be shown: the first frame of the story a
/// "story …: …" warning names, by the converter's INDD id (`story 30892`, which it writes to the
/// IDML as `u78ac`) or by its IDML `Self` (`story u78ac`).
fn warning_item(
    d: &Document,
    story_ids: &std::collections::HashMap<String, designcraft_doc::StoryId>,
    warning: &str,
) -> Option<designcraft_doc::ItemId> {
    let name = warning.strip_prefix("story ")?.split([':', ' ']).next()?;
    let self_id = name.parse::<u32>().map_or_else(|_| name.to_string(), |uid| format!("u{uid:x}"));
    d.stories.get(story_ids.get(&self_id)?)?.frames.first().copied()
}

/// An InDesign document or template (.indd, .indt): known by its signature, or by its name when
/// the bytes are neither a recognized DesignCraft document nor IDML. The converter explains
/// unrecognized data with an InDesign extension.
pub(crate) fn is_indd(name: &str, bytes: &[u8]) -> bool {
    craft_file_formats::detect(name, bytes) == Some(craft_file_formats::Format::Indd) && !super::file::is_document(bytes)
}

/// `name` without a trailing .idml, .indd or .indt (any case).
fn without_document_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if matches!(craft_file_formats::from_extension(ext), Some(craft_file_formats::Format::Idml | craft_file_formats::Format::Indd)) =>
        {
            stem
        }
        _ => name,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// During a data-merge preview, Export IDML (which the web build's Save uses with IDML chosen)
    /// writes the unfilled template, as Save does: the preview's fill is session state.
    #[test]
    fn idml_export_during_a_data_merge_preview_writes_the_template() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("data.source.select", &json!({"bytes": crate::cmd::base64_encode(b"x\nfilled-record\n"), "name": "d.csv"})).unwrap();
        s.execute("data.placeholder.add", &json!({"field": "x", "story": sid.0, "at": 0})).unwrap();
        s.execute("data.preview", &json!({"record": 1})).unwrap();
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "filled-record");
        let b64 = s.execute("file.exportIdml", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
        let back = designcraft_idml::import_idml(&crate::cmd::base64_decode(&b64)).unwrap();
        assert!(back.stories.values().all(|st| !st.text.contains("filled-record")), "the preview's fill isn't exported");
        assert_eq!(s.doc().unwrap().doc.story(sid).unwrap().text, "filled-record", "the preview stays on screen");
    }

    fn linked_idml(link: &str, embed_images: bool) -> Result<(Vec<u8>, Vec<u8>)> {
        let png = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![255; 16] }.to_png();
        let mut session = Session::new();
        session.execute("file.new", &json!({}))?;
        session.execute("file.place", &json!({"base64": base64_encode(&png), "name": "logo.png"}))?;
        session.edit(|doc, _| {
            let asset = doc.assets.values_mut().next().ok_or_else(|| bad("fixture", "missing graphic"))?;
            Arc::make_mut(asset).link = Some(link.into());
            Ok(Value::Null)
        })?;
        let bytes = designcraft_idml::export_idml_with(&session.doc()?.doc, &designcraft_idml::ExportOptions { embed_images });
        Ok((bytes, png))
    }

    #[test]
    fn story_warnings_point_at_the_story_frame() {
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
        let idml = designcraft_idml::export_idml_with(&session.doc().unwrap().doc, &designcraft_idml::ExportOptions { embed_images: true });
        let (d, _, story_ids) = import_report_with_previews(&idml, None, &std::collections::HashMap::new()).unwrap();
        let (self_id, sid) = story_ids.iter().find(|(_, sid)| d.stories.get(*sid).is_some_and(|s| !s.frames.is_empty())).unwrap();
        let frame = d.stories[sid].frames[0];
        assert_eq!(warning_item(&d, &story_ids, &format!("story {self_id}: something")), Some(frame));
        // The INDD converter names stories by their number, which it writes as `u` + hex.
        if let Some(uid) = self_id.strip_prefix('u').and_then(|h| u32::from_str_radix(h, 16).ok()) {
            assert_eq!(warning_item(&d, &story_ids, &format!("story {uid}: text orientation of its frames is not known")), Some(frame));
        }
        assert_eq!(warning_item(&d, &story_ids, "story 999999999: unknown"), None);
        assert_eq!(warning_item(&d, &story_ids, "a table of 2 rows was read as 1"), None);
    }

    #[test]
    fn opens_nested_packaged_links_from_a_different_computer() {
        let dir = std::env::temp_dir().join(format!("dc-idml-nested-links-{}", std::process::id()));
        let path = dir.join("Links").join("illustrations").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("C:\\original-computer\\project\\Links\\illustrations\\logo.png", false).unwrap();
        std::fs::write(&path, &png).unwrap();
        // Not written just now: its size and time can vouch for the imported bytes.
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(earlier).unwrap();
        std::fs::write(dir.join("logo.png"), b"a different file with the same basename").unwrap();

        let doc = import(&idml, Some(&dir.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(*asset.data.resident().unwrap(), png, "the packaged graphic must render even when its author-machine path no longer exists");
        let resolved = std::path::absolute(&path).unwrap();
        assert_eq!(asset.link.as_deref(), resolved.to_str(), "remember the resolved link so update and preflight use the packaged file");
        let expected = super::super::datamerge::fingerprint_from(&std::fs::metadata(&path).unwrap());
        assert_eq!(asset.link_fingerprint, Some(expected), "remember the same version of the file whose bytes were imported");
        assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "ok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opens_links_from_a_subfolder_beside_the_package() {
        let dir = std::env::temp_dir().join(format!("dc-idml-subfolder-links-{}", std::process::id()));
        let path = dir.join("images").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // The author's absolute `/images/…` does not exist here; `images/` beside the .idml does.
        let (idml, png) = linked_idml("/images/logo.png", false).unwrap();
        std::fs::write(&path, &png).unwrap();

        let doc = import(&idml, Some(&dir.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(*asset.data.resident().unwrap(), png);
        assert_eq!(asset.link.as_deref(), std::path::absolute(&path).unwrap().to_str());
        assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "ok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remembers_flat_packaged_link_location() {
        let dir = std::env::temp_dir().join(format!("dc-idml-flat-links-{}", std::process::id()));
        let path = dir.join("Links").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("/original-computer/project/logo.png", false).unwrap();
        std::fs::write(&path, &png).unwrap();

        let doc = import(&idml, Some(&dir.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(*asset.data.resident().unwrap(), png);
        assert_eq!(asset.link.as_deref(), std::path::absolute(&path).unwrap().to_str());
        assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "ok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_link_that_resolves_imports_as_a_linked_asset() {
        let dir = std::env::temp_dir().join(format!("dc-idml-linked-import-{}", std::process::id()));
        let path = dir.join("Links").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("/original-computer/project/logo.png", false).unwrap();
        std::fs::write(&path, &png).unwrap();

        let doc = import(&idml, Some(&dir.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.storage, designcraft_doc::Storage::Linked);
        assert!(asset.data.embedded_bytes().is_none(), "no embedded original");
        assert!(asset.data.is_linked() && asset.data.resident().is_some(), "the bytes read are resident for the first draw");
        assert_eq!(asset.link_hash.as_deref(), Some(crate::asset_io::content_hash(&png).as_str()));
        assert_eq!(asset.link_relative.as_deref(), Some("Links/logo.png"));
        assert!(asset.proxy.is_some(), "a proxy made from the file");
        assert_eq!(asset.data.load().ok().map(|b| (*b).clone()), Some(png));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A document's stored preview stands in for a linked file only when it decodes and shows
    /// the whole image, within the proxy limits (made again when bigger); otherwise the proxy is
    /// made from the file.
    #[test]
    fn a_stored_preview_is_the_proxy_only_when_it_decodes_and_shows_the_whole_image() {
        use designcraft_render::proxy::{MAX_BYTES, MAX_SIDE_PX};
        let dir = std::env::temp_dir().join(format!("dc-idml-previews-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.png");
        let png = |width: u32, height: u32| designcraft_render::Rendered { width, height, pixels: vec![255; (width * height * 4) as usize] }.to_png();
        let original = png(400, 200);
        std::fs::write(&path, &original).unwrap();
        let linked_with = |preview: Vec<u8>| {
            let mut a = designcraft_doc::Asset {
                name: "photo.png".into(),
                mime: "image/png".into(),
                link: Some(path.to_str().unwrap().into()),
                data: designcraft_doc::AssetData::embedded(original.clone()),
                pixels: Some((400, 200)),
                proxy: Some(designcraft_doc::Proxy { mime: "image/png".into(), pixels: (1, 1), data: designcraft_doc::AssetData::embedded(preview) }),
                ..Default::default()
            };
            link_read_file(&mut a, None);
            assert_eq!(a.storage, designcraft_doc::Storage::Linked);
            let proxy = a.proxy.expect("a proxy");
            let bytes = proxy.data.embedded_bytes().expect("embedded").clone();
            let pm = designcraft_render::decode_pixmap(&bytes).expect("the proxy decodes");
            assert_eq!((u32::from(pm.width()), u32::from(pm.height())), proxy.pixels);
            assert!(bytes.len() <= MAX_BYTES && proxy.pixels.0.max(proxy.pixels.1) <= MAX_SIDE_PX);
            (proxy.pixels, bytes)
        };
        let small = png(100, 50);
        assert_eq!(linked_with(small.clone()), ((100, 50), Arc::new(small)), "kept as it is");
        assert_eq!(linked_with(png(2048, 1024)).0, (1024, 512), "made again within the limits");
        assert_eq!(linked_with(png(100, 100)).0, (400, 200), "a crop of the image: made from the file");
        assert_eq!(linked_with(b"not an image".to_vec()).0, (400, 200), "undecodable: made from the file");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_link_that_does_not_resolve_keeps_its_path_for_relink() {
        let (idml, _) = linked_idml("/original-computer/project/logo.png", false).unwrap();
        let doc = import(&idml, None).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.storage, designcraft_doc::Storage::Embedded);
        assert_eq!(asset.link.as_deref(), Some("/original-computer/project/logo.png"));
        assert!(asset.data.embedded_bytes().is_some_and(|b| b.is_empty()) && asset.link_hash.is_none());
    }

    #[test]
    fn a_graphic_embedded_in_the_idml_stays_embedded_beside_its_file() {
        let dir = std::env::temp_dir().join(format!("dc-idml-embedded-stays-{}", std::process::id()));
        let path = dir.join("logo.png");
        std::fs::create_dir_all(&dir).unwrap();
        let (idml, png) = linked_idml(path.to_str().unwrap(), true).unwrap();
        std::fs::write(&path, &png).unwrap();
        let doc = import(&idml, Some(&dir.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.storage, designcraft_doc::Storage::Embedded);
        assert_eq!(asset.data.embedded_bytes().map(|b| (**b).clone()), Some(png));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn embedded_link_resolution_preserves_originals_and_image_bytes() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("dc-embedded-links-{}-{nonce}", std::process::id()));
        let original = dir.join("original").join("logo.png");
        let package = dir.join("package");
        let copy = package.join("Links").join("logo.png");
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml(original.to_str().unwrap(), true).unwrap();
        std::fs::write(&original, b"modified original").unwrap();
        std::fs::write(&copy, &png).unwrap();
        let doc = import(&idml, Some(&package.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(std::path::Path::new(asset.link.as_deref().unwrap()), original.as_path(), "an existing original stays authoritative");
        assert_eq!(**asset.data.embedded_bytes().unwrap(), png);
        assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "modified");

        std::fs::remove_file(&original).unwrap();
        std::fs::write(&copy, b"modified packaged copy").unwrap();
        let doc = import(&idml, Some(&package.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.link.as_deref(), copy.to_str());
        assert_eq!(**asset.data.embedded_bytes().unwrap(), png, "rebinding must not replace embedded pixels");
        assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "modified");

        std::fs::remove_file(&copy).unwrap();
        let packaged = package.join("x.idml");
        for folder in [Some(packaged.as_path()), None] {
            let doc = import(&idml, folder).unwrap();
            let asset = doc.assets.values().next().unwrap();
            assert_eq!(std::path::Path::new(asset.link.as_deref().unwrap()), original.as_path(), "keep a missing link when there is no candidate");
            assert_eq!(**asset.data.embedded_bytes().unwrap(), png);
            assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "missing");
        }
        let oversized = "a/".repeat(MAX_LINK_PATH_BYTES);
        assert!(packaged_link_candidates(&oversized, Some(&packaged)).is_empty());
        let mut doc = import(&idml, None).unwrap();
        Arc::make_mut(doc.assets.values_mut().next().unwrap()).link = Some(oversized.clone());
        resolve_packaged_links(&mut doc, Some(&packaged));
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.link.as_deref(), Some(oversized.as_str()));
        assert_eq!(**asset.data.embedded_bytes().unwrap(), png);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_local_graphic_files_are_read_from_links() {
        let base = std::env::temp_dir().join(format!("dc-idml-link-reach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, outside) = (base.join("package"), base.join("outside"));
        std::fs::create_dir_all(dir.join("Links")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let package = dir.join("x.idml");
        let png = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![255; 16] }.to_png();
        std::fs::write(dir.join("Links").join("notes.png"), b"not an image").unwrap();
        std::fs::write(outside.join("notes.txt"), b"not an image").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/dev/zero", dir.join("Links").join("zero.png")).unwrap();
            std::os::unix::fs::symlink("/dev/zero", outside.join("zero.png")).unwrap();
        }
        let (text, zero) = (outside.join("notes.txt"), outside.join("zero.png"));
        let links = [
            r"\\192.0.2.1\share\x.png",
            "file://192.0.2.1/s/x.png",
            text.to_str().unwrap(),
            zero.to_str().unwrap(),
            "/dev/zero",
            "Links/zero.png",
            "Links/notes.png",
        ];
        for link in links {
            let (idml, _) = linked_idml(link, false).unwrap();
            let (doc, seen) = crate::io::touched(|| import(&idml, Some(&package)).unwrap());
            // Beside the package, and the link itself where it is on this computer (on Windows,
            // `/dev/zero` is `\dev\zero` on the package's drive).
            let itself = crate::io::document_path(link, Some(&package)).ok();
            assert!(seen.iter().all(|p| p.starts_with(&dir) || itself.as_ref() == Some(p)), "{link}: {seen:?}");
            let asset = doc.assets.values().next().unwrap();
            assert!(asset.data.is_empty(), "{link}");
            assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "missing", "{link}");
        }
        // An image elsewhere on this computer is read, and so is one beside the package.
        let away = outside.join("logo.png");
        std::fs::write(&away, &png).unwrap();
        let (idml, _) = linked_idml(away.to_str().unwrap(), false).unwrap();
        let doc = import(&idml, Some(&package)).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!((&*asset.data.resident().unwrap(), asset.link.as_deref()), (&png, away.to_str()));
        assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "ok");
        std::fs::write(dir.join("Links").join("logo.png"), &png).unwrap();
        let (idml, _) = linked_idml("Links/logo.png", false).unwrap();
        let doc = import(&idml, Some(&package)).unwrap();
        assert_eq!(*doc.assets.values().next().unwrap().data.resident().unwrap(), png);
        std::fs::remove_dir_all(base).unwrap();
    }

    /// INDD routing without an InDesign file: bytes with the INDD signature (or an .indd/.indt
    /// name) go to the converter, and what it can't read is an open error that adds no document.
    #[test]
    fn indd_files_go_through_the_converter_and_its_errors_are_open_errors() {
        let mut signed = craft_indd::SIGNATURE.to_vec();
        signed.resize(4096, 0);
        let cases = [(signed, "Brochure"), (vec![0; 4096], "Brochure.indd"), (b"not an InDesign file".to_vec(), "Template.INDT")];
        let mut s = Session::new();
        for (bytes, name) in &cases {
            let e = s.execute("file.openBytes", &json!({"name": name, "base64": base64_encode(bytes)})).unwrap_err().to_string();
            assert!(e.contains("can't convert the InDesign file"), "{name}: {e}");
        }
        let dir = std::env::temp_dir().join(format!("dc-indd-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Brochure.indd");
        std::fs::write(&path, vec![0u8; 64]).unwrap();
        let e = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap_err().to_string();
        assert!(e.contains("can't convert the InDesign file"), "{e}");
        assert!(s.documents().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
        // Anything else keeps its own route.
        assert!(!is_indd("Brochure.idml", b"PK"));
        assert_eq!(without_document_extension("Brochure.INDD"), "Brochure");
        assert_eq!(without_document_extension("notes.txt"), "notes.txt");
    }

    #[test]
    fn packaged_links_respect_cap_link_immediately_and_skip_oversized_metadata() {
        let dir = std::env::temp_dir().join(format!("dc-idml-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("Links").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("/original-computer/project/logo.png", false).unwrap();
        std::fs::write(&path, &png).unwrap();
        let package = dir.join("x.idml");

        // Over the cap: read_packaged_link skips the file without reading its bytes.
        let (skipped, _, _) = import_within(&idml, Some(&package), &Default::default(), (png.len() - 1) as u64).unwrap();
        let skipped_asset = skipped.assets.values().next().unwrap();
        assert_eq!(skipped_asset.storage, designcraft_doc::Storage::Embedded);
        assert!(skipped_asset.data.is_empty());

        // Within the cap: imported directly into Storage::Linked (both open and confined import paths).
        let (linked, _, _) = import_within(&idml, Some(&package), &Default::default(), png.len() as u64).unwrap();
        let asset = linked.assets.values().next().unwrap();
        assert_eq!(asset.storage, designcraft_doc::Storage::Linked);
        assert!(asset.data.embedded_bytes().is_none());
        assert_eq!(super::super::links::status_cached(asset, None, &mut Default::default()), "ok");

        let confined = import_idml_confined(&idml, Some(&package)).unwrap();
        let confined_asset = confined.assets.values().next().unwrap();
        assert_eq!(confined_asset.storage, designcraft_doc::Storage::Linked);
        assert!(confined_asset.data.embedded_bytes().is_none());

        // Sparse file above MAX_LINKED_BYTES (1 GiB) is rejected via metadata without reading.
        let huge = dir.join("Links").join("huge.png");
        std::fs::File::create(&huge).unwrap().set_len(designcraft_doc::asset_data::MAX_LINKED_BYTES + 1).unwrap();
        assert!(read_packaged_link("Links/huge.png", Some(&package), designcraft_doc::asset_data::MAX_LINKED_BYTES).is_none());

        std::fs::remove_dir_all(dir).unwrap();
    }

    fn rewrite_spreads_xml(idml: &[u8], mut transform: impl FnMut(&str) -> String) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        use zip::write::SimpleFileOptions;

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(idml))?;
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            let method = entry.compression();
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            if name.starts_with("Spreads/") && name.ends_with(".xml") {
                let text = String::from_utf8(buf)?;
                buf = transform(&text).into_bytes();
            }
            out.start_file(name, SimpleFileOptions::default().compression_method(method))?;
            out.write_all(&buf)?;
        }
        Ok(out.finish()?.into_inner())
    }

    #[test]
    fn linked_pdf_crops_resolve_and_size_frames_without_bounds() {
        let dir = tempfile::TempDir::new().unwrap();
        let pdf_path = dir.path().join("Links").join("insert.pdf");
        std::fs::create_dir_all(pdf_path.parent().unwrap()).unwrap();
        let mut pdf_src = Session::new();
        pdf_src.execute("file.new", &json!({})).unwrap();
        pdf_src.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
        let pdf = base64_decode(pdf_src.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap());
        std::fs::write(&pdf_path, &pdf).unwrap();

        let mut doc_src = Session::new();
        doc_src.execute("file.new", &json!({})).unwrap();
        for x in [10, 100] {
            doc_src.execute("edit.deselectAll", &json!({})).unwrap();
            doc_src.execute("file.place", &json!({"path": pdf_path, "pdfCrop": "trim", "width": 630, "x": x, "y": 10})).unwrap();
        }
        let idml = designcraft_idml::export_idml_with(&doc_src.doc().unwrap().doc, &designcraft_idml::ExportOptions { embed_images: false });
        // Drop the second placement's GraphicBounds so `resolve_pdf_crops` must also apply the
        // precomputed crop size to a boundsless frame after linking.
        let idml = rewrite_spreads_xml(&idml, |xml| {
            let Some((head, tail)) = xml.rsplit_once("<GraphicBounds ") else { return xml.to_string() };
            let Some((_, rest)) = tail.split_once("/>") else { return xml.to_string() };
            format!("{head}{rest}")
        })
        .unwrap();

        let imported = import(&idml, Some(&dir.path().join("x.idml"))).unwrap();
        assert_eq!(imported.assets.len(), 1);
        let asset = imported.assets.values().next().unwrap();
        assert_eq!(asset.storage, designcraft_doc::Storage::Linked);
        assert!(asset.data.embedded_bytes().is_none());
        assert_eq!(asset.pdf_crop, designcraft_doc::PdfCrop::Trim);
        let shown = asset.shown_box().expect("linked PDF trim crop must be resolved");
        let want = [9.0 / 630.0, 9.0 / 810.0, 621.0 / 630.0, 801.0 / 810.0];
        assert!(shown.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4), "{shown:?}");
        let graphics: Vec<_> = imported.placed_items().into_iter().filter_map(|i| i.graphic().map(|g| g.size)).collect();
        assert_eq!(graphics.len(), 2);
        for (w, h) in graphics {
            assert!((w - 612.0).abs() < 1e-3 && (h - 792.0).abs() < 1e-3, "({w}, {h})");
        }
    }

    #[test]
    fn embedded_graphic_sharing_a_read_link_updates_its_path_and_stays_embedded() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("Links").join("illustrations").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let linked_png = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![200; 16] }.to_png();
        let embedded_png = designcraft_render::Rendered { width: 4, height: 4, pixels: vec![40; 64] }.to_png();
        std::fs::write(&path, &linked_png).unwrap();

        let author_uri = r"C:\original-computer\project\Links\illustrations\logo.png";
        let mut src = Session::new();
        src.execute("file.new", &json!({})).unwrap();
        src.execute("file.place", &json!({"base64": base64_encode(&embedded_png), "name": "logo.png", "x": 0, "y": 0})).unwrap();
        src.execute("edit.deselectAll", &json!({})).unwrap();
        src.execute("file.place", &json!({"base64": base64_encode(&linked_png), "name": "logo.png", "x": 50, "y": 0})).unwrap();
        src.edit(|doc, _| {
            for asset in doc.assets.values_mut() {
                Arc::make_mut(asset).link = Some(author_uri.into());
            }
            Ok(Value::Null)
        })
        .unwrap();
        let idml = designcraft_idml::export_idml_with(&src.doc().unwrap().doc, &designcraft_idml::ExportOptions { embed_images: true });
        // Strip only the second <Contents> element so the embedded graphic appears first and still
        // shares its LinkResourceURI with the later linked placement.
        let idml = rewrite_spreads_xml(&idml, |xml| {
            let Some((head, tail)) = xml.rsplit_once("<Contents>") else { return xml.to_string() };
            let Some((_, rest)) = tail.split_once("</Contents>") else { return xml.to_string() };
            format!("{head}{rest}")
        })
        .unwrap();

        let doc = import(&idml, Some(&dir.path().join("x.idml"))).unwrap();
        assert_eq!(doc.assets.len(), 2);
        let resolved = std::path::absolute(&path).unwrap().to_string_lossy().into_owned();
        let embedded = doc.assets.values().find(|a| a.storage == designcraft_doc::Storage::Embedded).unwrap();
        let linked = doc.assets.values().find(|a| a.storage == designcraft_doc::Storage::Linked).unwrap();
        assert_eq!(embedded.link.as_deref(), Some(resolved.as_str()));
        assert!(embedded.proxy.is_none());
        assert_eq!(embedded.data.embedded_bytes().map(|b| (**b).as_slice()), Some(embedded_png.as_slice()));
        assert_eq!(linked.link.as_deref(), Some(resolved.as_str()));
        assert!(linked.data.embedded_bytes().is_none());
    }

    #[test]
    fn multi_page_and_aliased_links_to_one_file_share_source_key_and_deduplicate() {
        let dir = tempfile::TempDir::new().unwrap();
        let pdf_path = dir.path().join("Links").join("insert.pdf");
        std::fs::create_dir_all(pdf_path.parent().unwrap()).unwrap();
        let mut pdf_src = Session::new();
        pdf_src.execute("file.new", &json!({"pages": 2})).unwrap();
        pdf_src.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
        let pdf = base64_decode(pdf_src.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap());
        std::fs::write(&pdf_path, &pdf).unwrap();

        let mut src = Session::new();
        src.execute("file.new", &json!({})).unwrap();
        let mut ids = Vec::new();
        for x in [0, 50, 100, 150] {
            src.execute("edit.deselectAll", &json!({})).unwrap();
            let placed = src.execute("file.place", &json!({"path": pdf_path, "x": x, "y": 0})).unwrap();
            let item_id = designcraft_doc::ItemId(placed["id"].as_u64().unwrap());
            // Give each placement a distinct initial AssetId so the test configures all 4 links/pages/crops.
            src.edit(|doc, _| {
                let mut asset = (*doc.assets[&designcraft_doc::AssetId(placed["asset"].as_u64().unwrap())]).clone();
                asset.id = designcraft_doc::AssetId(doc.alloc());
                let new_aid = asset.id;
                doc.assets.insert(new_aid, Arc::new(asset));
                if let Some(designcraft_doc::Content::Graphic(g)) = doc.item_mut(item_id).map(|i| &mut i.content) {
                    g.asset = new_aid;
                }
                ids.push(new_aid);
                Ok(Value::Null)
            })
            .unwrap();
        }
        src.edit(|doc, _| {
            let specs = [
                (ids[0], "/author/Links/insert.pdf", 0, designcraft_doc::PdfCrop::Trim),
                (ids[1], "Links/insert.pdf", 0, designcraft_doc::PdfCrop::Trim),
                (ids[2], "Links/insert.pdf", 1, designcraft_doc::PdfCrop::Trim),
                (ids[3], "Links/insert.pdf", 0, designcraft_doc::PdfCrop::Media),
            ];
            for (aid, link, page, crop) in specs {
                let asset = Arc::make_mut(doc.assets.get_mut(&aid).unwrap());
                asset.link = Some(link.into());
                asset.page = page;
                asset.pdf_crop = crop;
            }
            Ok(Value::Null)
        })
        .unwrap();
        let idml = designcraft_idml::export_idml_with(&src.doc().unwrap().doc, &designcraft_idml::ExportOptions { embed_images: false });
        let imported = import(&idml, Some(&dir.path().join("x.idml"))).unwrap();

        // Both URI spellings of page 1 Trim deduplicate once resolved to the same path; page 2 and
        // Media stay distinct assets, and all 3 share one SourceKey and content hash.
        assert_eq!(imported.placed_items().len(), 4);
        assert_eq!(imported.assets.len(), 3);
        assert!(imported.assets.values().all(|a| a.storage == designcraft_doc::Storage::Linked));
        let keys: Vec<_> = imported.assets.values().map(|a| a.data.key()).collect();
        assert!(keys.windows(2).all(|w| w[0] == w[1]), "{keys:?}");
    }

    #[test]
    fn link_read_file_with_mints_fresh_source_key_when_reread_bytes_changed() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("photo.png");
        let original = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![10; 16] }.to_png();
        let modified = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![200; 16] }.to_png();
        std::fs::write(&path, &original).unwrap();

        let make_asset = |bytes: &[u8]| designcraft_doc::Asset {
            name: "photo.png".into(),
            mime: "image/png".into(),
            link: Some(path.to_str().unwrap().into()),
            data: designcraft_doc::AssetData::embedded(bytes.to_vec()),
            ..Default::default()
        };

        let mut first = make_asset(&original);
        let (first_data, first_hash) = link_read_file_with(&mut first, None, None, false).unwrap();

        // Same bytes re-read from disk keep the existing SourceKey and hash.
        let mut same = make_asset(&original);
        let (same_data, same_hash) = link_read_file_with(&mut same, None, Some((first_data.key(), first_hash.clone())), false).unwrap();
        assert_eq!(same_data.key(), first_data.key());
        assert_eq!(same_hash, first_hash);

        // Modified bytes re-read from disk must mint a fresh SourceKey and updated hash.
        std::fs::write(&path, &modified).unwrap();
        let mut changed = make_asset(&modified);
        let (changed_data, changed_hash) = link_read_file_with(&mut changed, None, Some((first_data.key(), first_hash.clone())), false).unwrap();
        assert_ne!(changed_data.key(), first_data.key());
        assert_eq!(changed_hash, crate::asset_io::content_hash(&modified));
        assert_eq!(changed.link_hash.as_deref(), Some(changed_hash.as_str()));
    }
}

#[cfg(test)]
mod nested_layer_tests {
    use std::{io::Read, sync::Arc};

    use designcraft_doc::{AnchoredObject, ItemId, SpreadRef, Story, StoryId, Table};
    use designcraft_geom::Rect;

    use super::*;

    // One default placement and two nested placements share bytes. The latter use a legacy
    // nonempty hidden list and explicit all-on, so each needs a different embedded variant.
    fn fixture(location: &str) -> (Session, ItemId, [ItemId; 2]) {
        let pdf = designcraft_render::pdf_hide_layers(&super::super::pdflayers::tests::two_layer_pdf(), &["Red".into()]).unwrap();
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let placed = session.execute("file.place", &json!({"base64": base64_encode(&pdf), "name": "layers.pdf"})).unwrap();
        let control = ItemId(placed["id"].as_u64().unwrap());
        let ids = {
            let doc = Arc::make_mut(&mut session.doc_mut().unwrap().doc);
            let original = doc.item(control).unwrap().clone();
            let aid = original.graphic().unwrap().asset;
            Arc::make_mut(doc.assets.get_mut(&aid).unwrap()).link = Some("/synthetic/layers.pdf".into());
            let mut hidden = original.clone();
            hidden.id = ItemId(doc.alloc());
            hidden.pdf_hidden_layers = vec!["Blue".into()];
            let mut all_on = original;
            all_on.id = ItemId(doc.alloc());
            all_on.pdf_layer_override = true;
            let ids = [hidden.id, all_on.id];
            let mut nested = Story::new(StoryId(0));
            for item in [hidden, all_on] {
                nested.insert_object(nested.len(), AnchoredObject { item, position: Default::default() });
            }
            let layer = doc.default_layer();
            let (_, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 200.0), layer, "Body", Default::default()).unwrap();
            let story = doc.story_mut(sid).unwrap();
            match location {
                "cell" => {
                    let mut table = Table::new(1, 1, 1, 0, 0, 100.0);
                    table.cells[0].text = nested;
                    story.insert_table(0, table);
                }
                "footnote" => {
                    story.insert_note(0, "Note", Default::default()).unwrap();
                    Arc::make_mut(&mut story.notes[0]).text = nested;
                }
                _ => {
                    assert_eq!(location, "endnote");
                    story.insert_endnote(0, "Endnote", Default::default()).unwrap();
                    Arc::make_mut(&mut story.endnotes[0]).text = nested;
                }
            }
            ids
        };
        (session, control, ids)
    }

    fn states(blue: bool, red: bool) -> Vec<(String, bool)> {
        vec![("Blue".into(), blue), ("Red".into(), red)]
    }

    fn prepared(location: &str) {
        let (session, control, ids) = fixture(location);
        let source = &session.doc().unwrap().doc;
        let original_asset = source.item(control).unwrap().graphic().unwrap().asset;
        let copy = with_pdf_layer_variants(source, true).unwrap().unwrap();
        for (id, expected) in ids.into_iter().zip([states(false, true), states(true, true)]) {
            let item = copy.placed_items().into_iter().find(|i| i.id == id).unwrap();
            let asset = &copy.assets[&item.graphic().unwrap().asset];
            assert_eq!(designcraft_render::pdf_layers(&asset.data.load().unwrap()), expected, "{location}: placement {id}");
            assert_ne!(asset.id, original_asset);
            assert!(asset.link.is_none());
            assert!(!item.pdf_layer_override && item.pdf_hidden_layers.is_empty());
        }
        assert_eq!(copy.item(control).unwrap().graphic().unwrap().asset, original_asset);
        assert!(Arc::ptr_eq(&copy.assets[&original_asset], &source.assets[&original_asset]));
        assert_eq!(designcraft_render::pdf_layers(&source.assets[&original_asset].data.load().unwrap()), states(true, false));
        assert!(source.placed_items().into_iter().filter(|i| ids.contains(&i.id)).all(|i| i.has_pdf_layer_override()));
    }

    fn embedded(location: &str) {
        let (mut session, _, _) = fixture(location);
        let original = session.doc().unwrap().doc.clone();
        let result = session.execute("file.exportIdml", &json!({})).unwrap();
        let bytes = base64_decode(result["base64"].as_str().unwrap());
        let imported = designcraft_idml::import_idml(&bytes).unwrap();
        let mut roundtrip: Vec<_> = imported
            .assets
            .values()
            .filter(|a| designcraft_render::is_pdf(&a.data.load().unwrap()))
            .map(|a| designcraft_render::pdf_layers(&a.data.load().unwrap()))
            .collect();
        let mut expected = vec![states(true, false), states(false, true), states(true, true)];
        roundtrip.sort();
        expected.sort();
        assert_eq!(roundtrip, expected, "{location}: reopened IDML keeps each choice");
        let mut package = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut actual = Vec::new();
        for index in 0..package.len() {
            let mut entry = package.by_index(index).unwrap();
            if !entry.name().ends_with(".xml") {
                continue;
            }
            let mut xml = String::new();
            entry.read_to_string(&mut xml).unwrap();
            for cdata in xml.split("<![CDATA[").skip(1) {
                let data = base64_decode(cdata.split("]]>").next().unwrap());
                if designcraft_render::is_pdf(&data) {
                    actual.push(designcraft_render::pdf_layers(&data));
                }
            }
        }
        actual.sort();
        assert_eq!(actual, expected, "{location}: each placement embeds its own visibility state");
        assert!(Arc::ptr_eq(&original, &session.doc().unwrap().doc), "export must not edit the live document");
    }

    fn external(location: &str) {
        let (mut session, _, ids) = fixture(location);
        // Each choice alone must reject the export, including a legacy nonempty hidden list.
        for active in ids {
            let original = session.doc().unwrap().doc.clone();
            let mut one = (*original).clone();
            one.update_placed_items(
                |i| ids.contains(&i.id) && i.id != active,
                |i| {
                    i.pdf_layer_override = false;
                    i.pdf_hidden_layers.clear();
                },
            );
            let mut single = Session::new();
            single.add_document(DocState::new(one, None));
            let before = single.doc().unwrap().doc.clone();
            let err = single.execute("file.exportIdml", &json!({"embedImages": false})).unwrap_err().to_string();
            assert!(err.contains("embedded images"), "{location}: {err}");
            assert!(Arc::ptr_eq(&before, &single.doc().unwrap().doc));
        }
        Arc::make_mut(&mut session.doc_mut().unwrap().doc).update_placed_items(
            |i| ids.contains(&i.id),
            |i| {
                i.pdf_layer_override = false;
                i.pdf_hidden_layers.clear();
            },
        );
        session.execute("file.exportIdml", &json!({"embedImages": false})).unwrap();
    }

    // Duplicating a text frame that holds an anchored placed PDF copies the placement under a
    // fresh id (#1522). Export must apply the choice to both.
    #[test]
    fn duplicated_frame_keeps_its_anchored_pdf_choice_in_idml() {
        let pdf = designcraft_render::pdf_hide_layers(&super::super::pdflayers::tests::two_layer_pdf(), &["Red".into()]).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let placed = s.execute("file.place", &json!({"base64": base64_encode(&pdf), "name": "layers.pdf"})).unwrap();
        let control = ItemId(placed["id"].as_u64().unwrap());
        let (frame, chosen) = {
            let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let mut item = doc.item(control).unwrap().clone();
            item.id = ItemId(doc.alloc());
            item.pdf_hidden_layers = vec!["Blue".into()];
            let chosen = item.id;
            let layer = doc.default_layer();
            let (frame, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 200.0), layer, "Body", Default::default()).unwrap();
            doc.story_mut(sid).unwrap().insert_object(0, AnchoredObject { item, position: Default::default() });
            (frame, chosen)
        };
        s.execute("transform.move", &json!({"ids": [frame.0], "dx": 10, "dy": 10, "copy": true})).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        let with_choice: Vec<_> = doc.placed_items().into_iter().filter(|i| i.has_pdf_layer_override()).map(|i| i.id).collect();
        assert_eq!(with_choice.len(), 2);
        assert!(with_choice.contains(&chosen) && with_choice[0] != with_choice[1], "{with_choice:?}");
        doc.check().unwrap();
        let result = s.execute("file.exportIdml", &json!({})).unwrap();
        let imported = designcraft_idml::import_idml(&base64_decode(result["base64"].as_str().unwrap())).unwrap();
        let mut layers: Vec<_> = imported
            .assets
            .values()
            .filter(|a| designcraft_render::is_pdf(&a.data.load().unwrap()))
            .map(|a| designcraft_render::pdf_layers(&a.data.load().unwrap()))
            .collect();
        layers.sort();
        assert_eq!(layers, vec![states(false, true), states(true, false)]);
    }

    // Duplication no longer shares anchored ids (#1522), so this fixture forces the shared id.
    // Conflicting choices must not silently select one variant; only the copy's choice changes.
    #[test]
    fn shared_id_with_divergent_pdf_choices_is_rejected() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let pdf = designcraft_render::pdf_hide_layers(&super::super::pdflayers::tests::two_layer_pdf(), &["Red".into()]).unwrap();
        let placed = s.execute("file.place", &json!({"base64": base64_encode(&pdf), "name": "layers.pdf"})).unwrap();
        let control = ItemId(placed["id"].as_u64().unwrap());
        let (frame, anchored) = {
            let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let mut item = doc.item(control).unwrap().clone();
            item.id = ItemId(doc.alloc());
            item.pdf_hidden_layers = vec!["Blue".into()];
            let anchored = item.id;
            let layer = doc.default_layer();
            let (frame, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 200.0), layer, "Body", Default::default()).unwrap();
            doc.story_mut(sid).unwrap().insert_object(0, AnchoredObject { item, position: Default::default() });
            (frame, anchored)
        };
        s.execute("transform.move", &json!({"ids": [frame.0], "dx": 10, "dy": 10, "copy": true})).unwrap();
        // Give ONLY the copy in the newest story the original's id and explicit all-on.
        {
            let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let (_, st) = doc.stories.iter_mut().filter(|(_, st)| !st.objects.is_empty()).last().unwrap();
            let o = &mut Arc::make_mut(st).objects[0];
            let it = &mut Arc::make_mut(o).item;
            it.id = anchored;
            it.pdf_hidden_layers.clear();
            it.pdf_layer_override = true;
        }
        let before = s.doc().unwrap().doc.clone();
        let chosen: Vec<_> = before.placed_items().into_iter().filter(|i| i.has_pdf_layer_override()).collect();
        assert_eq!(chosen.len(), 2);
        assert_eq!(chosen[0].id, chosen[1].id);
        assert_eq!(chosen[0].graphic().unwrap().asset, chosen[1].graphic().unwrap().asset);
        assert_ne!(chosen[0].pdf_hidden_layers, chosen[1].pdf_hidden_layers);
        let err = s.execute("file.exportIdml", &json!({})).unwrap_err().to_string();
        assert!(err.contains("placements sharing id") && err.contains("different sources or layer choices"), "{err}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }

    // Cross-document paste used to keep the anchored placement's source id (#1522), colliding
    // with an unrelated placement in the destination. The fixture forces that collision.
    #[test]
    fn shared_id_with_different_pdf_assets_is_rejected() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let pdf = designcraft_render::pdf_hide_layers(&super::super::pdflayers::tests::two_layer_pdf(), &["Red".into()]).unwrap();
        let a = s.execute("file.place", &json!({"base64": base64_encode(&pdf), "name": "a.pdf"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.layerOptions", &json!({"ids": [a], "hidden": ["Blue"]})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [72, 72, 400, 400], "content": "text", "text": "Hello"})).unwrap()["id"].as_u64().unwrap();
        let sid = s.doc().unwrap().doc.item(ItemId(f)).unwrap().text_frame().unwrap().story;
        s.execute("anchored.insert", &json!({"ids": [a], "story": sid.0, "pos": 0})).unwrap();
        s.execute("selection.set", &json!({"ids": [f]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        // A different, unrelated PDF placement in the destination (no choice).
        let b = s
            .execute("file.place", &json!({"base64": base64_encode(&super::super::pdflayers::tests::two_layer_pdf()), "name": "b.pdf"}))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        s.execute("edit.paste", &json!({})).unwrap();
        {
            let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let (_, st) = doc.stories.iter_mut().find(|(_, st)| !st.objects.is_empty()).unwrap();
            let pasted = &mut Arc::make_mut(&mut Arc::make_mut(st).objects[0]).item;
            assert_ne!(pasted.id.0, b, "paste gives the anchored placement a fresh id");
            pasted.id = ItemId(b);
        }
        let before = s.doc().unwrap().doc.clone();
        let graphics: Vec<_> = before.placed_items().into_iter().filter(|i| i.graphic().is_some()).collect();
        assert_eq!(graphics.len(), 2);
        assert_eq!(graphics[0].id, graphics[1].id);
        assert_ne!(graphics[0].graphic().unwrap().asset, graphics[1].graphic().unwrap().asset);
        assert!(!graphics[0].has_pdf_layer_override(), "destination placement uses file defaults");
        assert!(graphics[1].has_pdf_layer_override());
        let err = s.execute("file.exportIdml", &json!({})).unwrap_err().to_string();
        assert!(err.contains("placements sharing id") && err.contains("different sources or layer choices"), "{err}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }

    #[test]
    fn nested_idml_cell_variants_are_applied() {
        prepared("cell");
    }
    #[test]
    fn nested_idml_footnote_variants_are_applied() {
        prepared("footnote");
    }
    // Endnote structures/artwork are not serialized by IDML yet. Check preparation without
    // mistaking a correct export-only variant for endnote-artwork serialization support.
    #[test]
    fn nested_idml_endnote_variants_are_prepared() {
        prepared("endnote");
    }
    #[test]
    fn nested_idml_cell_embedded_choices() {
        embedded("cell");
    }
    #[test]
    fn nested_idml_footnote_embedded_choices() {
        embedded("footnote");
    }
    #[test]
    fn nested_idml_cell_external_choices_are_rejected() {
        external("cell");
    }
    #[test]
    fn nested_idml_footnote_external_choices_are_rejected() {
        external("footnote");
    }
    #[test]
    fn nested_idml_endnote_external_choices_are_rejected() {
        external("endnote");
    }
}
