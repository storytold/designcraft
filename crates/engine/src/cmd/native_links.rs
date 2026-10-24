//! External artwork in native working files. Full originals are cached in the live document
//! for rendering, export and undo, but omitted on disk when the user selects linked storage.

use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use designcraft_doc::Document;
use serde_json::{Value, json};

use super::bad;
use crate::{EngineError, Result, Session};

#[cfg(not(target_arch = "wasm32"))]
const MAX_LINK_BYTES: u64 = 512 * 1024 * 1024;
// Match the native archive's total byte ceiling. Include cached originals already in
// the document; external references must not bypass that ceiling through many reads.
const MAX_HYDRATION_BYTES: u64 = 4 << 30;
const MAX_HYDRATION_SOURCES: usize = 4096;
const MAX_LINK_WARNINGS: usize = 32;

/// Validate an optional save override before touching the live document.
pub(super) fn save_options(doc: &mut Document, params: &Value) -> Result<()> {
    if let Some(value) = params.get("embedImages") {
        let embed = value.as_bool().ok_or_else(|| bad("file.save", "`embedImages` must be true or false"))?;
        if embed {
            require_originals(doc)?;
        }
        doc.settings.external_links = !embed;
    }
    Ok(())
}

pub(super) fn storage(session: &mut Session, params: &Value) -> Result<Value> {
    if let Some(mode) = params.get("mode") {
        let external = match mode.as_str() {
            Some("linked") => true,
            Some("embedded") => false,
            _ => return Err(bad("links.storage", "`mode` must be linked or embedded")),
        };
        // Never offer to embed an unresolved link: there is no original to preserve.
        if !external {
            require_originals(&session.doc()?.doc)?;
        }
        session.edit(|doc, _| {
            doc.settings.external_links = external;
            Ok(Value::Null)
        })?;
    }
    Ok(json!({"mode": if session.doc()?.doc.settings.external_links { "linked" } else { "embedded" }}))
}

fn require_originals(doc: &Document) -> Result<()> {
    for id in doc.used_assets() {
        if let Some(asset) = doc.assets.get(&id)
            && asset.data.is_empty()
        {
            return Err(bad("links.storage", format!("{}: relink the missing artwork before embedding it", asset.name)));
        }
    }
    Ok(())
}

/// Resolve live references against the source document before Save As changes its path.
/// Apply only to the prepared clone, so a refused save leaves the live document untouched.
pub(super) fn prepare_paths(saved: &mut Document, document: Option<&Path>) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    if saved.settings.external_links {
        for id in saved.used_assets() {
            let Some(asset) = saved.assets.get_mut(&id) else { continue };
            if !asset.is_linkable_graphic() {
                continue;
            }
            let Some(link) = asset.link.as_deref() else { continue };
            // Byte-opened layouts have no source folder. Giving a relative reference the
            // destination's folder would let a later reopen adopt unrelated artwork there.
            if document.is_none() && craft_docpath::resolve(link, None).is_ok_and(|path| !path.is_absolute()) {
                return Err(bad(
                    "file.save",
                    format!("{}: relative artwork has no source document folder; Relink it before saving links only", asset.name),
                ));
            }
            match crate::io::document_path(link, document) {
                Ok(source) => Arc::make_mut(asset).link = Some(source.to_string_lossy().into_owned()),
                Err(why) if !asset.data.is_empty() => {
                    return Err(bad("file.save", format!("{}: {why}; Relink or Embed it before saving links only", asset.name)));
                }
                // An unresolved reference can still be saved, without following or rewriting it.
                Err(_) => {}
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (saved, document);
    Ok(())
}

/// Serialize for a known destination. Only the saved clone gets relative paths; Save As and
/// Save a Copy must not rebase the live document or its undo history.
pub(super) fn save(doc: &Document, destination: &Path) -> Result<Vec<u8>> {
    save_with_preview(doc, destination, None)
}

pub(super) fn save_with_preview(doc: &Document, destination: &Path, preview: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut saved = doc.clone();
    rebase(&mut saved, destination)?;
    designcraft_format::save_working_with_preview(&saved, preview).map_err(|e| EngineError::Other(e.to_string()))
}

fn rebase(saved: &mut Document, destination: &Path) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    if saved.settings.external_links {
        let destination = std::path::absolute(destination).map_err(|e| EngineError::Other(e.to_string()))?;
        let dir = destination.parent().ok_or_else(|| bad("file.save", "destination has no parent folder"))?;
        for id in saved.used_assets() {
            let Some(asset) = saved.assets.get_mut(&id) else { continue };
            if !asset.is_linkable_graphic() {
                continue;
            }
            let Some(link) = asset.link.as_deref() else { continue };
            let source = match crate::io::document_path(link, Some(&destination)) {
                Ok(source) => source,
                Err(why) if !asset.data.is_empty() => {
                    return Err(bad("file.save", format!("{}: {why}; Relink or Embed it before saving links only", asset.name)));
                }
                Err(_) => continue,
            };
            // A missing link with a cached original would lose its last copy on disk. Keep
            // the old file and unsaved state intact; Embed or Relink is an explicit choice.
            if !asset.data.is_empty() && (!crate::io::is_file(&source) || crate::io::read_capped(&source, 0).is_err()) {
                return Err(bad("file.save", format!("{}: linked artwork is unavailable; Relink or Embed it before saving links only", asset.name)));
            }
            if crate::io::metadata(&source).is_ok_and(|metadata| metadata.len() > MAX_LINK_BYTES) {
                return Err(bad("file.save", format!("{}: sources larger than 512 MiB need embedded storage", asset.name)));
            }
            let relative = super::datamerge::relative_between(dir, &source);
            #[cfg(target_os = "windows")]
            let relative = relative.replace('\\', "/");
            Arc::make_mut(asset).link = Some(relative);
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = destination;
    #[cfg(target_arch = "wasm32")]
    let _ = saved;
    Ok(())
}

/// Hydrate only native documents explicitly saved in external-link mode. Byte-only opening
/// has no trusted base folder and never follows document-supplied paths.
pub(super) fn resolve(doc: &mut Document, path: Option<&Path>) -> Vec<String> {
    resolve_with_limits(doc, path, MAX_HYDRATION_BYTES, MAX_HYDRATION_SOURCES)
}

fn resolve_with_limits(doc: &mut Document, path: Option<&Path>, byte_limit: u64, source_limit: usize) -> Vec<String> {
    if !doc.settings.external_links {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    let mut omitted_warnings = 0;
    let mut warn = |message| {
        if warnings.len() < MAX_LINK_WARNINGS {
            warnings.push(message);
        } else {
            omitted_warnings += 1;
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    let mut loaded = std::collections::HashMap::<(std::path::PathBuf, String), Arc<Vec<u8>>>::new();
    #[cfg(not(target_arch = "wasm32"))]
    let (mut bytes_left, mut sources_left) = {
        let mut seen = std::collections::HashSet::new();
        let resident = doc.assets.values().filter(|a| seen.insert(Arc::as_ptr(&a.data))).fold(0u64, |sum, a| sum.saturating_add(a.data.len() as u64));
        (byte_limit.saturating_sub(resident), source_limit)
    };
    for id in doc.used_assets() {
        let Some(asset) = doc.assets.get_mut(&id) else { continue };
        let Some(link) = asset.link.clone() else { continue };
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(document) = path
            && asset.is_linkable_graphic()
        {
            let source = crate::io::document_path(&link, Some(document));
            if let Ok(source) = source {
                let asset = Arc::make_mut(asset);
                asset.link = Some(source.to_string_lossy().into_owned());
                // Portable recovery/clipboard files can carry a cached original. Keep it:
                // status reporting must still detect changes relative to those bytes.
                if !asset.data.is_empty() {
                    continue;
                }
                let key = (source.clone(), asset.mime.clone());
                if let Some(bytes) = loaded.get(&key) {
                    if let Err(error) = install_graphic(asset, bytes.clone()) {
                        warn(format!("{}: {error}; use Relink to locate {link}", asset.name));
                    }
                    continue;
                }
                if sources_left == 0 {
                    warn(format!(
                        "Linked artwork reached the {source_limit}-source automatic-open limit; remaining links were not loaded. Use Relink or Update for individual sources."
                    ));
                    break;
                }
                sources_left -= 1;
                match read_graphic(&source, &asset.mime, bytes_left) {
                    Ok(bytes) => {
                        bytes_left -= bytes.len() as u64;
                        loaded.insert(key, bytes.clone());
                        if let Err(error) = install_graphic(asset, bytes) {
                            warn(format!("{}: {error}; use Relink to locate {link}", asset.name));
                        }
                        continue;
                    }
                    Err(error) => {
                        warn(format!("{}: {error}; use Relink to locate {link}", asset.name));
                        continue;
                    }
                }
            } else if let Err(why) = source {
                warn(format!("{}: {why}; use Relink to locate {link}", asset.name));
                continue;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (path, byte_limit, source_limit);
        if asset.data.is_empty() {
            warn(format!("{}: linked artwork is unavailable; use Relink to locate {link}", asset.name));
        }
    }
    if omitted_warnings > 0 {
        warnings.push(format!("{omitted_warnings} additional linked-artwork warnings omitted; inspect the Links panel for unresolved artwork."));
    }
    warnings
}

#[cfg(not(target_arch = "wasm32"))]
fn install_graphic(asset: &mut designcraft_doc::Asset, bytes: Arc<Vec<u8>>) -> std::result::Result<(), String> {
    let pixels = if asset.mime == "application/pdf" && asset.page > 0 {
        designcraft_render::pdf_page_size(&bytes, asset.page as usize)
            .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
    } else {
        designcraft_render::image_size(&bytes)
    };
    // A source may have changed while the layout was closed. Match Update Link's pixel
    // metadata refresh so the Links panel's effective resolution describes the loaded file.
    asset.pixels = Some(pixels.ok_or("linked artwork is corrupt or its selected PDF page is unavailable")?);
    if designcraft_images::is_eps(&bytes) {
        asset.mime = "application/postscript".into();
    }
    asset.data = bytes;
    // The source can change while closed; a stored PDF crop belongs to the previous bytes.
    super::file::crop_pdf(asset);
    Ok(())
}

/// External sources are untrusted input too. Bound reads and use fallible allocation, so an
/// absurd length cannot abort the process. Do not let a document disguise arbitrary text as
/// an image and later copy it into a portable save.
#[cfg(not(target_arch = "wasm32"))]
fn read_graphic(path: &Path, mime: &str, bytes_left: u64) -> std::result::Result<Arc<Vec<u8>>, String> {
    let metadata = crate::io::metadata(path).map_err(|e| format!("linked artwork is unavailable ({e})"))?;
    let size = metadata.len();
    if !crate::io::is_file(path) || size == 0 || size > MAX_LINK_BYTES {
        return Err("linked artwork must be nonempty and at most 512 MiB".into());
    }
    if size > bytes_left {
        return Err("linked artwork exceeds the remaining automatic-open byte budget; use embedded storage or load individual sources".into());
    }
    let size_usize = usize::try_from(size).map_err(|e| e.to_string())?;
    let bytes = crate::io::read_capped(path, MAX_LINK_BYTES.min(bytes_left)).map_err(|e| e.to_string())?;
    if bytes.len() != size_usize {
        return Err("linked artwork changed while it was being read".into());
    }
    // Keep the EPS original, just as Place and Update Link do. Earlier working files may
    // identify their cached EPS preview as PNG; accept that migration without losing the source.
    let retained_eps = designcraft_images::is_eps(&bytes);
    if retained_eps && designcraft_images::eps_size(&bytes).is_none() {
        return Err("linked EPS has no usable bounding box".into());
    }
    if designcraft_images::mime(&bytes) != mime && !(retained_eps && mime == "image/png") {
        return Err("linked artwork no longer matches its saved image type".into());
    }
    Ok(Arc::new(bytes))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn linked_document(links: &[&str]) -> std::result::Result<Document, Box<dyn std::error::Error>> {
        let mut session = Session::new();
        session.execute("file.new", &json!({}))?;
        for (i, _) in links.iter().enumerate() {
            let bytes = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![i as u8; 16] }.to_png();
            session.execute("file.place", &json!({"base64": super::super::base64_encode(&bytes), "name": format!("{i}.png"), "x": 0, "y": 0}))?;
        }
        let mut doc = (*session.doc()?.doc).clone();
        doc.settings.external_links = true;
        assert_eq!(doc.assets.len(), links.len());
        for asset in doc.assets.values_mut() {
            let asset = Arc::make_mut(asset);
            let i: usize = asset.name.trim_end_matches(".png").parse()?;
            asset.link = Some(links[i].into());
            asset.data = Arc::new(Vec::new());
        }
        Ok(doc)
    }

    fn png() -> Vec<u8> {
        designcraft_render::Rendered { width: 2, height: 2, pixels: vec![255; 16] }.to_png()
    }

    #[test]
    fn native_link_hydration_shares_a_byte_budget_and_reuses_loaded_sources() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = png();
        for name in ["a.png", "b.png"] {
            std::fs::write(temp.path().join(name), &bytes).unwrap();
        }
        let mut doc = linked_document(&["a.png", "b.png", "a.png"]).unwrap();
        let warnings = resolve_with_limits(&mut doc, Some(&temp.path().join("layout.designcraft")), bytes.len() as u64, 2);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("byte budget"));
        let loaded: Vec<_> = doc.assets.values().filter(|a| !a.data.is_empty()).collect();
        assert_eq!(loaded.len(), 2);
        assert!(Arc::ptr_eq(&loaded[0].data, &loaded[1].data), "repeat placements must not consume another read or byte allowance");
        assert!(doc.assets.values().find(|a| a.name == "1.png").unwrap().data.is_empty());
    }

    #[test]
    fn native_link_hydration_counts_cached_buffers_once_without_replacing_them() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = Arc::new(png());
        std::fs::write(temp.path().join("new.png"), &*bytes).unwrap();
        let mut doc = linked_document(&["cached.png", "cached.png", "new.png"]).unwrap();
        for asset in doc.assets.values_mut().filter(|a| a.name != "2.png") {
            Arc::make_mut(asset).data = bytes.clone();
        }
        let original = doc.clone();
        let path = temp.path().join("layout.designcraft");
        assert!(resolve_with_limits(&mut doc, Some(&path), 2 * bytes.len() as u64, 1).is_empty());
        assert!(doc.assets.values().all(|a| !a.data.is_empty()));
        for asset in doc.assets.values().filter(|a| a.name != "2.png") {
            assert!(Arc::ptr_eq(&asset.data, &bytes));
        }
        let mut doc = original;
        assert!(resolve_with_limits(&mut doc, Some(&path), bytes.len() as u64, 1)[0].contains("byte budget"));
        assert!(doc.assets.values().find(|a| a.name == "2.png").unwrap().data.is_empty());
    }

    #[test]
    fn native_link_hydration_limits_failed_source_probes_and_warning_growth() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("layout.designcraft");
        let mut doc = linked_document(&["missing.png", "never-probed.png"]).unwrap();
        let (warnings, touched) = crate::io::touched(|| resolve_with_limits(&mut doc, Some(&path), 1024, 1));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[1].contains("1-source"));
        assert!(touched.iter().all(|p| !p.ends_with("never-probed.png")), "{touched:?}");
        assert!(doc.assets.values().all(|a| a.data.is_empty()));
        let mut doc = linked_document(&["missing.png"; MAX_LINK_WARNINGS + 8]).unwrap();
        let warnings = resolve_with_limits(&mut doc, Some(&path), 1024, MAX_LINK_WARNINGS + 8);
        assert_eq!(warnings.len(), MAX_LINK_WARNINGS + 1);
        assert!(warnings.last().unwrap().contains("8 additional"));
    }
}
