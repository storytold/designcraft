//! Interchange formats: IDML (InDesign Markup Language) import and export, and Affinity import.

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
            "{path | base64, name?} — opens an IDML package as a new document (linked images are read next to the file or from its Links/ folder; the fonts in a `Document Fonts` folder beside it load first) → {index, documentFonts, warnings}",
            always, open_idml),
        cmd!(noundo "file.openAffinity", "Open Affinity Document", [], None,
            "{path | base64, name?} — opens an Affinity document (.afpub, .af, .afdesign, .afphoto) as a new, unsaved document; what can't be imported is listed → {index, warnings}",
            always, open_affinity),
    ]
}

/// File extensions of Affinity documents.
pub const AFFINITY_EXTENSIONS: &[&str] = &["afpub", "af", "afdesign", "afphoto"];

/// Is `path` named like an Affinity document?
pub fn is_affinity_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| AFFINITY_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

pub(crate) fn open_affinity(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.openAffinity";
    let (bytes, name) = if let Some(b) = str_param(p, "base64") {
        let name = str_param(p, "name").map(|n| match n.rsplit_once('.') {
            Some((stem, ext)) if is_affinity_path(n) && !ext.is_empty() => stem.to_string(),
            _ => n.to_string(),
        });
        (base64_decode(b), name)
    } else if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let b = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = Vec::new();
        (b, std::path::Path::new(path).file_stem().map(|n| n.to_string_lossy().to_string()))
    } else {
        return Err(bad(ID, "missing `path` or `base64`"));
    };
    if !designcraft_affinity::is_affinity(&bytes) {
        return Err(bad(ID, "not an Affinity document"));
    }
    let imported = designcraft_affinity::import(&bytes).map_err(|e| EngineError::Other(e.to_string()))?;
    let mut d = imported.document;
    if let Some(n) = name {
        d.title = n;
    }
    // Never save over the Affinity file: the document starts unsaved.
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i, "warnings": imported.warnings}))
}

fn export_idml(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let opts = designcraft_idml::ExportOptions { embed_images: p.get("embedImages").and_then(Value::as_bool).unwrap_or(true) };
    let bytes = designcraft_idml::export_idml_with(&st.doc, &opts);
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            std::fs::write(path, &bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }
        None => Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()})),
    }
}

/// Import IDML bytes; `dir` is the folder the package came from (for relative link lookup).
pub fn import(bytes: &[u8], dir: Option<&std::path::Path>) -> Result<Document> {
    #[cfg(not(target_arch = "wasm32"))]
    let resolved = std::cell::RefCell::new(std::collections::HashMap::new());
    let read = |link: &str| -> Option<Vec<u8>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (path, data) = read_packaged_link(link, dir)?;
            resolved.borrow_mut().insert(link.to_string(), path);
            Some(data)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (link, &dir);
            None
        }
    };
    let document = designcraft_idml::import_idml_with(bytes, &read).map_err(|e| EngineError::Other(e.to_string()))?;
    #[cfg(not(target_arch = "wasm32"))]
    let document = {
        let mut document = document;
        for asset in document.assets.values_mut() {
            if let Some(path) = asset.link.as_ref().and_then(|link| resolved.borrow().get(link).cloned()) {
                // Relocated packages must also use the new path for preflight and link updates.
                std::sync::Arc::make_mut(asset).link = Some(path);
            }
        }
        resolve_packaged_links(&mut document, dir);
        document
    };
    Ok(document)
}

#[cfg(not(target_arch = "wasm32"))]
const MAX_LINK_PATH_BYTES: usize = 128 * 1024;

#[cfg(not(target_arch = "wasm32"))]
fn packaged_link_candidates(link: &str, dir: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    // Enough for a maximum-length Windows path even with four-byte UTF-8 characters.
    // Bound splitting/allocation for link strings supplied by imported documents.
    if link.len() > MAX_LINK_PATH_BYTES {
        return Vec::new();
    }
    let mut candidates = vec![std::path::PathBuf::from(link)];
    if let Some(dir) = dir {
        // IDML may retain Windows paths even when the package is opened on another platform.
        let parts: Vec<&str> = link.split(['/', '\\']).filter(|part| !part.is_empty()).collect();
        let safe_part = |part: &&str| *part != "." && *part != ".." && !part.contains(':');
        if !link.starts_with(['/', '\\']) && parts.iter().all(safe_part) {
            candidates.push(dir.join(parts.iter().collect::<std::path::PathBuf>()));
        }
        // Packagers can keep subfolders under Links (for example, Links/illustrations/logo.ai).
        // Preserve that suffix instead of flattening every resource to its basename.
        if let Some(index) = parts.iter().rposition(|part| part.eq_ignore_ascii_case("Links")) {
            let suffix = &parts[index + 1..];
            if !suffix.is_empty() && suffix.iter().all(safe_part) {
                candidates.push(dir.join("Links").join(suffix.iter().collect::<std::path::PathBuf>()));
            }
        }
        if let Some(name) = parts.last().filter(|part| safe_part(part)) {
            candidates.push(dir.join(name));
            candidates.push(dir.join("Links").join(name));
        }
    }
    candidates
}

/// Embedded images do not call the IDML resource loader. Rebind their missing links
/// beside the opened document too, without replacing their embedded image bytes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_packaged_links(document: &mut Document, dir: Option<&std::path::Path>) {
    let Some(dir) = dir else { return };
    for asset in document.assets.values_mut() {
        let Some(link) = asset.link.as_deref() else { continue };
        if link.len() > MAX_LINK_PATH_BYTES || std::path::Path::new(link).is_file() {
            continue;
        }
        let Some(path) = packaged_link_candidates(link, Some(dir)).into_iter().find(|p| p.is_file()) else { continue };
        // Preserve ordinary Windows paths rather than canonicalizing to a verbatim URI.
        let path = std::path::absolute(&path).unwrap_or(path);
        std::sync::Arc::make_mut(asset).link = Some(path.to_string_lossy().into_owned());
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_packaged_link(link: &str, dir: Option<&std::path::Path>) -> Option<(String, Vec<u8>)> {
    for path in packaged_link_candidates(link, dir) {
        if let Ok(bytes) = std::fs::read(&path) {
            // Keep ordinary Windows drive/UNC syntax for IDML URI export; canonicalize
            // would introduce a verbatim prefix that is not an IDML file URI.
            let path = std::path::absolute(&path).unwrap_or(path);
            return Some((path.to_string_lossy().into_owned(), bytes));
        }
    }
    None
}

pub(crate) fn open_idml(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, dir, name) = if let Some(b) = str_param(p, "base64") {
        (base64_decode(b), None, str_param(p, "name").map(|n| n.trim_end_matches(".idml").to_string()))
    } else if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let b = std::fs::read(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = Vec::new();
        let pp = std::path::Path::new(path);
        (b, pp.parent().map(|d| d.to_path_buf()), pp.file_stem().map(|n| n.to_string_lossy().to_string()))
    } else {
        return Err(bad("file.openIdml", "missing `path` or `base64`"));
    };
    let mut d = import(&bytes, dir.as_deref())?;
    if let Some(n) = name {
        d.title = n;
    }
    let (fonts, faces, warnings) = match str_param(p, "path").filter(|_| str_param(p, "base64").is_none()) {
        Some(path) => super::file::load_document_fonts(&mut d, path),
        None => (None, 0, Vec::new()),
    };
    // Never save over the .idml with the native format: the document starts unsaved.
    let mut st = DocState::new(d, None);
    st.fonts = fonts;
    let i = s.add_document(st);
    Ok(json!({"index": i, "documentFonts": faces, "warnings": warnings}))
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
    fn opens_nested_packaged_links_from_a_different_computer() {
        let dir = std::env::temp_dir().join(format!("dc-idml-nested-links-{}", std::process::id()));
        let path = dir.join("Links").join("illustrations").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("C:\\original-computer\\project\\Links\\illustrations\\logo.png", false).unwrap();
        std::fs::write(&path, &png).unwrap();
        std::fs::write(dir.join("logo.png"), b"a different file with the same basename").unwrap();

        let doc = import(&idml, Some(&dir)).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(*asset.data, png, "the packaged graphic must render even when its author-machine path no longer exists");
        let resolved = std::path::absolute(&path).unwrap();
        assert_eq!(asset.link.as_deref(), resolved.to_str(), "remember the resolved link so update and preflight use the packaged file");
        assert_eq!(super::super::links::status(asset), "ok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remembers_flat_packaged_link_location() {
        let dir = std::env::temp_dir().join(format!("dc-idml-flat-links-{}", std::process::id()));
        let path = dir.join("Links").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("/original-computer/project/logo.png", false).unwrap();
        std::fs::write(&path, &png).unwrap();

        let doc = import(&idml, Some(&dir)).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(*asset.data, png);
        assert_eq!(asset.link.as_deref(), std::path::absolute(&path).unwrap().to_str());
        assert_eq!(super::super::links::status(asset), "ok");
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
        let doc = import(&idml, Some(&package)).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(std::path::Path::new(asset.link.as_deref().unwrap()), original.as_path(), "an existing original stays authoritative");
        assert_eq!(*asset.data, png);
        assert_eq!(super::super::links::status(asset), "modified");

        std::fs::remove_file(&original).unwrap();
        std::fs::write(&copy, b"modified packaged copy").unwrap();
        let doc = import(&idml, Some(&package)).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.link.as_deref(), copy.to_str());
        assert_eq!(*asset.data, png, "rebinding must not replace embedded pixels");
        assert_eq!(super::super::links::status(asset), "modified");

        std::fs::remove_file(&copy).unwrap();
        for folder in [Some(package.as_path()), None] {
            let doc = import(&idml, folder).unwrap();
            let asset = doc.assets.values().next().unwrap();
            assert_eq!(std::path::Path::new(asset.link.as_deref().unwrap()), original.as_path(), "keep a missing link when there is no candidate");
            assert_eq!(*asset.data, png);
            assert_eq!(super::super::links::status(asset), "missing");
        }
        let oversized = "a/".repeat(MAX_LINK_PATH_BYTES);
        assert!(packaged_link_candidates(&oversized, Some(&package)).is_empty());
        let mut doc = import(&idml, None).unwrap();
        Arc::make_mut(doc.assets.values_mut().next().unwrap()).link = Some(oversized.clone());
        resolve_packaged_links(&mut doc, Some(&package));
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(asset.link.as_deref(), Some(oversized.as_str()));
        assert_eq!(*asset.data, png);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A one-page Affinity document with a 100 × 50 px rectangle at 72 dpi (synthetic).
    fn affinity_doc() -> Vec<u8> {
        use designcraft_affinity::synth::{self, F, Method, tag};
        let page = F::Def(4, vec![tag(b"PagR")], vec![(tag(b"rctp"), F::F64s(vec![0.0, 0.0, 300.0, 400.0]))]);
        let rect = F::Def(
            10,
            vec![tag(b"ShpN")],
            vec![
                (tag(b"ShpB"), F::F64s(vec![0.0, 0.0, 100.0, 50.0])),
                (tag(b"Shpe"), F::Def(11, vec![tag(b"ShNR")], vec![])),
                (tag(b"Desc"), F::Str("Box".into())),
            ],
        );
        let spread = F::Def(
            2,
            vec![tag(b"Sprd")],
            vec![(tag(b"SpMd"), F::Obj(tag(b"SpMd"), vec![(tag(b"PagR"), F::Shared(vec![page]))])), (tag(b"Chld"), F::Shared(vec![rect]))],
        );
        let data = synth::stream(&[(tag(b"DocR"), F::Def(1, vec![tag(b"DocN")], vec![(tag(b"Chld"), F::Shared(vec![spread]))]))]);
        synth::container(&[("doc.dat", &data, Method::Zlib)], None)
    }

    #[test]
    fn affinity_documents_open_as_new_unsaved_documents() -> Result<()> {
        let bytes = affinity_doc();
        let mut s = Session::new();
        let r = s.execute("file.openBytes", &json!({"base64": base64_encode(&bytes), "name": "Sample.afpub"}))?;
        assert!(r["warnings"].is_array(), "{r}");
        let st = s.doc()?;
        assert_eq!(st.doc.title, "Sample");
        assert!(st.path.is_none(), "never saved over the Affinity file");
        assert_eq!(st.doc.page_count(), 1);
        assert_eq!((st.doc.settings.page_width, st.doc.settings.page_height), (300.0, 400.0));
        let item = st.doc.spreads[0].items.first().ok_or_else(|| EngineError::Other("no item".into()))?;
        assert_eq!(item.name, "Box");
        assert_eq!(item.bounds(), kurbo::Rect::new(0.0, 0.0, 100.0, 50.0));

        let dir = std::env::temp_dir().join(format!("designcraft-affinity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| EngineError::Other(e.to_string()))?;
        let path = dir.join("Example.af");
        std::fs::write(&path, &bytes).map_err(|e| EngineError::Other(e.to_string()))?;
        s.execute("file.open", &json!({"path": path.to_string_lossy()}))?;
        assert_eq!(s.doc()?.doc.title, "Example");
        std::fs::remove_dir_all(dir).map_err(|e| EngineError::Other(e.to_string()))?;
        Ok(())
    }

    #[test]
    fn damaged_affinity_documents_are_errors() {
        let mut bytes = affinity_doc();
        bytes.truncate(bytes.len() / 2);
        let mut s = Session::new();
        assert!(s.execute("file.openAffinity", &json!({"base64": base64_encode(&bytes)})).is_err());
        assert!(s.execute("file.openAffinity", &json!({"base64": base64_encode(b"PK not affinity")})).is_err());
        assert!(s.execute("file.openAffinity", &json!({})).is_err());
        assert!(s.active().is_none(), "nothing half-open");
    }
}
