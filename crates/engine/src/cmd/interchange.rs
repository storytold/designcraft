//! Interchange formats: IDML (InDesign Markup Language) import and export, and INDD/INDT
//! import (converted to IDML in memory by indd-utils).
//!
//! A package's linked files are read from where the link points on this computer, then from the
//! link's trailing names beside the package (where a package moved from its author's computer
//! keeps them). Links to other computers and devices are not followed
//! ([`crate::io::document_path`]). Only regular files that hold an image or a graphic format are
//! read, and only up to [`MAX_LINKED_FILE_BYTES`].

use designcraft_doc::Document;
use serde_json::{Value, json};

use super::file::{base64_decode, base64_encode};
use super::{CommandSpec, always, bad, cmd, has_doc, str_param};
use crate::{DocState, EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "file.exportIdml", "Export IDML…", [], None,
            "{path?, embedImages?: true} — writes an IDML package to `path`, or returns {base64} without a path",
            has_doc, export_idml),
        cmd!(noundo "file.openIdml", "Open IDML", [], None,
            "{path | base64, name?} — opens an IDML package, or an InDesign document or template (.indd, .indt: converted to IDML first; its conversion warnings join `warnings`), as a new document (linked images are read next to the file or from its Links/ folder; the fonts in a `Document Fonts` folder beside it load first) → {index, documentFonts, warnings: conversion warnings, package parts missing, font files skipped, GREP styles whose pattern can't compile}",
            always, open_idml),
    ]
}

fn export_idml(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let opts = designcraft_idml::ExportOptions { embed_images: p.get("embedImages").and_then(Value::as_bool).unwrap_or(true) };
    let layer_doc = with_pdf_layer_variants(&st.doc, opts.embed_images)?;
    let bytes = designcraft_idml::export_idml_with(layer_doc.as_ref().unwrap_or(&st.doc), &opts);
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }
        None => Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()})),
    }
}

/// Whether a placement holds an Object Layer Options choice an export can still apply: a graphic
/// whose asset is a PDF. A choice left on an item that is no longer a placed PDF (its link was
/// replaced with another format) is not one, so the package report does not name it.
pub(crate) fn has_applicable_pdf_layer_overrides(doc: &Document) -> bool {
    doc.placed_items().into_iter().any(|it| {
        it.has_pdf_layer_override() && it.graphic().and_then(|g| doc.assets.get(&g.asset)).is_some_and(|a| designcraft_render::is_pdf(&a.data))
    })
}

/// Each placement gets its own embedded variant so two frames sharing a PDF can
/// choose different layers. The live document and original linked bytes stay untouched.
///
/// Parent-page and anchored placements count too, so a page that inherits the placement does not
/// quietly fall back to the file's defaults. Read and write walks also reach table-cell and
/// note text. Endnote variants are prepared too, but IDML endnote structures/artwork are not
/// serialized yet (see `docs/pdf-layer-visibility.md`).
fn with_pdf_layer_variants(doc: &Document, embed: bool) -> Result<Option<Document>> {
    let choices: Vec<_> = doc
        .placed_items()
        .into_iter()
        .filter(|it| it.has_pdf_layer_override())
        .filter_map(|it| {
            let asset = it.graphic().and_then(|g| doc.assets.get(&g.asset)).filter(|a| designcraft_render::is_pdf(&a.data))?;
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
    // Duplicated anchors can share an id. Reuse a variant only when source and choice agree.
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
        let bytes = designcraft_render::pdf_hide_layers(&asset.data, &hidden)
            .ok_or_else(|| EngineError::Other(format!("{}: PDF layer choices could not be applied", asset.name)))?;
        let mut variant = (**asset).clone();
        variant.id = designcraft_doc::AssetId(out.alloc());
        variant.data = std::sync::Arc::new(bytes);
        variant.link = None;
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
    let read = |link: &str| -> Option<Vec<u8>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            read_packaged_link(link, package).map(|(_, data)| data)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (link, package);
            None
        }
    };
    let mut d = designcraft_idml::import_idml_with(bytes, &read)?;
    resolve_pdf_crops(&mut d);
    Ok(d)
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
    #[cfg(not(target_arch = "wasm32"))]
    let resolved = std::cell::RefCell::new(std::collections::HashMap::new());
    let read = |link: &str| -> Option<Vec<u8>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (path, data) = read_packaged_link(link, package)?;
            resolved.borrow_mut().insert(link.to_string(), path);
            Some(data)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (link, &package);
            None
        }
    };
    let designcraft_idml::Imported { mut document, warnings, story_ids } =
        designcraft_idml::import_idml_report_with_previews(bytes, &read, previews).map_err(|e| EngineError::Other(e.to_string()))?;
    resolve_pdf_crops(&mut document);
    #[cfg(not(target_arch = "wasm32"))]
    let document = {
        let mut document = document;
        for asset in document.assets.values_mut() {
            if let Some(path) = asset.link.as_ref().and_then(|link| resolved.borrow().get(link).cloned()) {
                // Relocated packages must also use the new path for preflight and link updates.
                std::sync::Arc::make_mut(asset).link = Some(path);
            }
        }
        resolve_packaged_links(&mut document, package);
        document
    };
    Ok((document, warnings, story_ids))
}

/// Placed PDFs read from IDML name their crop (`PDFCrop`): find where each box sits on its page,
/// which the IDML reader can't (it doesn't parse PDFs). The box spans the graphic's
/// `GraphicBounds`; a graphic without bounds takes the box's size. Bounding Box crops interpret
/// the page's drawing to find the box, so they wait for drawing or export ([`designcraft_render::shown_box`],
/// [`with_pdf_boxes`]) unless a graphic needs the box's size now.
pub(crate) fn resolve_pdf_crops(d: &mut Document) {
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
        if a.pdf_crop == PdfCrop::Crop || a.pdf_box.is_some() || (a.content_box_pending() && !no_bounds.contains(&a.id)) {
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

/// Largest linked file read when a package is opened (bytes); bigger files stay missing.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_LINKED_FILE_BYTES: u64 = 256 * 1024 * 1024;

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
                std::sync::Arc::make_mut(asset).link = Some(path.to_string_lossy().into_owned());
            }
            continue;
        }
        let Some(path) = packaged_link_candidates(link, Some(package)).into_iter().find(|p| crate::io::is_file(p)) else { continue };
        std::sync::Arc::make_mut(asset).link = Some(path.to_string_lossy().into_owned());
    }
}

/// The first candidate for `link` ([`packaged_link_candidates`]) that is a regular file no larger
/// than [`MAX_LINKED_FILE_BYTES`] holding an image or a graphic, with its path.
#[cfg(not(target_arch = "wasm32"))]
fn read_packaged_link(link: &str, package: Option<&std::path::Path>) -> Option<(String, Vec<u8>)> {
    for path in packaged_link_candidates(link, package) {
        if !crate::io::is_file(&path) || !crate::io::metadata(&path).is_ok_and(|m| m.len() <= MAX_LINKED_FILE_BYTES) {
            continue;
        }
        // At most one byte over the limit, in case the file grew since.
        let Ok(bytes) = crate::io::read_capped(&path, MAX_LINKED_FILE_BYTES) else { continue };
        if bytes.len() as u64 > MAX_LINKED_FILE_BYTES || !is_graphic(&bytes) {
            continue;
        }
        return Some((path.to_string_lossy().into_owned(), bytes));
    }
    None
}

pub(crate) fn open_idml(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, package, file, title) = if let Some(b) = str_param(p, "base64") {
        let name = str_param(p, "name").unwrap_or_default();
        (base64_decode(b), None, name.to_string(), without_document_extension(name).to_string())
    } else if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let b = crate::io::read_file(path).map_err(EngineError::Other)?;
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = Vec::new();
        let pp = std::path::Path::new(path);
        let file = pp.file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string());
        (b, Some(pp.to_path_buf()), file, pp.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
    } else {
        return Err(bad("file.openIdml", "missing `path` or `base64`"));
    };
    let mut warnings = Vec::new();
    // InDesign's previews of linked graphics show while their files are missing (graphic `Self` →
    // preview), as InDesign shows them. One preview can stand for any number of graphics: they
    // share its buffer.
    let mut previews = std::collections::HashMap::new();
    let bytes = if is_indd(&file, &bytes) {
        // What the converter can't read still comes over as IDML (#21).
        let conversion = indd::convert(&bytes, &file).map_err(|e| {
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
    let (mut d, import_warnings, story_ids) = import_report_with_previews(&bytes, package.as_deref(), &previews)?;
    warnings.extend(import_warnings);
    if !title.is_empty() {
        d.title = title;
    }
    let (fonts, faces, font_warnings) = match str_param(p, "path").filter(|_| str_param(p, "base64").is_none()) {
        Some(path) => super::file::load_document_fonts(&mut d, path),
        None => (None, 0, Vec::new()),
    };
    warnings.extend(font_warnings);
    warnings.extend(super::style::grep_style_warnings(&d));
    let warning_items: Vec<Value> = warnings.iter().map(|w| warning_item(&d, &story_ids, w).map_or(Value::Null, |i| json!(i.0))).collect();
    // Never save over the .idml or .indd with the native format: the document starts unsaved.
    let mut st = DocState::new(d, None);
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
/// the bytes are something else (the converter then says why it can't read them).
pub(crate) fn is_indd(name: &str, bytes: &[u8]) -> bool {
    bytes.starts_with(&indd::header::SIGNATURE) || has_indd_extension(name)
}

pub(crate) fn has_indd_extension(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".indd") || n.ends_with(".indt")
}

/// `name` without a trailing .idml, .indd or .indt (any case).
fn without_document_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if ["idml", "indd", "indt"].iter().any(|e| ext.eq_ignore_ascii_case(e)) => stem,
        _ => name,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;

    use super::*;

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
        std::fs::write(dir.join("logo.png"), b"a different file with the same basename").unwrap();

        let doc = import(&idml, Some(&dir.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(*asset.data, png, "the packaged graphic must render even when its author-machine path no longer exists");
        let resolved = std::path::absolute(&path).unwrap();
        assert_eq!(asset.link.as_deref(), resolved.to_str(), "remember the resolved link so update and preflight use the packaged file");
        assert_eq!(super::super::links::status(asset, None), "ok");
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
        assert_eq!(*asset.data, png);
        assert_eq!(asset.link.as_deref(), std::path::absolute(&path).unwrap().to_str());
        assert_eq!(super::super::links::status(asset, None), "ok");
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
        assert_eq!(*asset.data, png);
        assert_eq!(asset.link.as_deref(), std::path::absolute(&path).unwrap().to_str());
        assert_eq!(super::super::links::status(asset, None), "ok");
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
        assert_eq!(*asset.data, png);
        assert_eq!(super::super::links::status(asset, None), "modified");

        std::fs::remove_file(&original).unwrap();
        std::fs::write(&copy, b"modified packaged copy").unwrap();
        let doc = import(&idml, Some(&package.join("x.idml"))).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.link.as_deref(), copy.to_str());
        assert_eq!(*asset.data, png, "rebinding must not replace embedded pixels");
        assert_eq!(super::super::links::status(asset, None), "modified");

        std::fs::remove_file(&copy).unwrap();
        let packaged = package.join("x.idml");
        for folder in [Some(packaged.as_path()), None] {
            let doc = import(&idml, folder).unwrap();
            let asset = doc.assets.values().next().unwrap();
            assert_eq!(std::path::Path::new(asset.link.as_deref().unwrap()), original.as_path(), "keep a missing link when there is no candidate");
            assert_eq!(*asset.data, png);
            assert_eq!(super::super::links::status(asset, None), "missing");
        }
        let oversized = "a/".repeat(MAX_LINK_PATH_BYTES);
        assert!(packaged_link_candidates(&oversized, Some(&packaged)).is_empty());
        let mut doc = import(&idml, None).unwrap();
        Arc::make_mut(doc.assets.values_mut().next().unwrap()).link = Some(oversized.clone());
        resolve_packaged_links(&mut doc, Some(&packaged));
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.link.as_deref(), Some(oversized.as_str()));
        assert_eq!(*asset.data, png);
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
            assert_eq!(super::super::links::status(asset, None), "missing", "{link}");
        }
        // An image elsewhere on this computer is read, and so is one beside the package.
        let away = outside.join("logo.png");
        std::fs::write(&away, &png).unwrap();
        let (idml, _) = linked_idml(away.to_str().unwrap(), false).unwrap();
        let doc = import(&idml, Some(&package)).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!((&*asset.data, asset.link.as_deref()), (&png, away.to_str()));
        assert_eq!(super::super::links::status(asset, None), "ok");
        std::fs::write(dir.join("Links").join("logo.png"), &png).unwrap();
        let (idml, _) = linked_idml("Links/logo.png", false).unwrap();
        let doc = import(&idml, Some(&package)).unwrap();
        assert_eq!(*doc.assets.values().next().unwrap().data, png);
        std::fs::remove_dir_all(base).unwrap();
    }

    /// INDD routing without an InDesign file: bytes with the INDD signature (or an .indd/.indt
    /// name) go to the converter, and what it can't read is an open error that adds no document.
    #[test]
    fn indd_files_go_through_the_converter_and_its_errors_are_open_errors() {
        let mut signed = indd::header::SIGNATURE.to_vec();
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
            assert_eq!(designcraft_render::pdf_layers(&asset.data), expected, "{location}: placement {id}");
            assert_ne!(asset.id, original_asset);
            assert!(asset.link.is_none());
            assert!(!item.pdf_layer_override && item.pdf_hidden_layers.is_empty());
        }
        assert_eq!(copy.item(control).unwrap().graphic().unwrap().asset, original_asset);
        assert!(Arc::ptr_eq(&copy.assets[&original_asset], &source.assets[&original_asset]));
        assert_eq!(designcraft_render::pdf_layers(&source.assets[&original_asset].data), states(true, false));
        assert!(source.placed_items().into_iter().filter(|i| ids.contains(&i.id)).all(|i| i.has_pdf_layer_override()));
    }

    fn embedded(location: &str) {
        let (mut session, _, _) = fixture(location);
        let original = session.doc().unwrap().doc.clone();
        let result = session.execute("file.exportIdml", &json!({})).unwrap();
        let bytes = base64_decode(result["base64"].as_str().unwrap());
        let imported = designcraft_idml::import_idml(&bytes).unwrap();
        let mut roundtrip: Vec<_> =
            imported.assets.values().filter(|a| designcraft_render::is_pdf(&a.data)).map(|a| designcraft_render::pdf_layers(&a.data)).collect();
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

    // Duplicating a text frame that holds an anchored placed PDF leaves two placements sharing
    // one ItemId (renumber gap, tracked separately). Export must still apply the choice to both.
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
        assert_eq!(with_choice, [chosen, chosen], "both placements share one id");
        assert!(doc.check().unwrap_err().to_string().contains(&format!("duplicate item id {chosen}")));
        let result = s.execute("file.exportIdml", &json!({})).unwrap();
        let imported = designcraft_idml::import_idml(&base64_decode(result["base64"].as_str().unwrap())).unwrap();
        let mut layers: Vec<_> =
            imported.assets.values().filter(|a| designcraft_render::is_pdf(&a.data)).map(|a| designcraft_render::pdf_layers(&a.data)).collect();
        layers.sort();
        assert_eq!(layers, vec![states(false, true), states(true, false)]);
    }

    // Shared anchored ids are a pre-existing duplication gap. Conflicting choices must not
    // silently select one variant; this fixture changes only the copied anchor's choice.
    #[test]
    fn shared_id_with_divergent_pdf_choices_is_rejected() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let pdf = designcraft_render::pdf_hide_layers(&super::super::pdflayers::tests::two_layer_pdf(), &["Red".into()]).unwrap();
        let placed = s.execute("file.place", &json!({"base64": base64_encode(&pdf), "name": "layers.pdf"})).unwrap();
        let control = ItemId(placed["id"].as_u64().unwrap());
        let frame = {
            let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let mut item = doc.item(control).unwrap().clone();
            item.id = ItemId(doc.alloc());
            item.pdf_hidden_layers = vec!["Blue".into()];
            let layer = doc.default_layer();
            let (frame, sid) = doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 200.0, 200.0), layer, "Body", Default::default()).unwrap();
            doc.story_mut(sid).unwrap().insert_object(0, AnchoredObject { item, position: Default::default() });
            frame
        };
        s.execute("transform.move", &json!({"ids": [frame.0], "dx": 10, "dy": 10, "copy": true})).unwrap();
        // Change ONLY the copy in the newest story to explicit all-on.
        {
            let doc = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let (_, st) = doc.stories.iter_mut().filter(|(_, st)| !st.objects.is_empty()).last().unwrap();
            let o = &mut Arc::make_mut(st).objects[0];
            let it = &mut Arc::make_mut(o).item;
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

    // Cross-document paste: the pasted frame's anchored placement keeps its source id,
    // which can collide with an unrelated placement in the destination.
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
        assert_eq!(a, b, "fresh documents allocate the same first placement id");
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
