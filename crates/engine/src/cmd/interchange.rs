//! Interchange formats: IDML (InDesign Markup Language) import and export.

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
            "{path | base64, name?} — opens an IDML package as a new document (linked images are read next to the file or from its Links/ folder)",
            always, open_idml),
    ]
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
    let dir = dir.map(|d| d.to_path_buf());
    let read = move |link: &str| -> Option<Vec<u8>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Ok(b) = std::fs::read(link) {
                return Some(b);
            }
            let name = link.rsplit(['/', '\\']).next()?;
            let dir = dir.as_ref()?;
            std::fs::read(dir.join(name)).or_else(|_| std::fs::read(dir.join("Links").join(name))).ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (link, &dir);
            None
        }
    };
    designcraft_idml::import_idml_with(bytes, &read).map_err(|e| EngineError::Other(e.to_string()))
}

pub(crate) fn open_idml(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, dir, name) = if let Some(b) = str_param(p, "base64") {
        (base64_decode(b), None, str_param(p, "name").map(|n| n.trim_end_matches(".idml").trim_end_matches(".indd").to_string()))
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
    // InDesign documents are converted to an IDML package first.
    let bytes = if designcraft_indd::is_indd(&bytes) {
        let title = name.as_deref().map(|n| format!("{n}.indd")).unwrap_or_else(|| "document.indd".into());
        designcraft_indd::to_idml_named(&bytes, &title).map_err(|e| EngineError::Other(e.to_string()))?
    } else {
        bytes
    };
    let mut d = import(&bytes, dir.as_deref())?;
    if let Some(n) = name {
        d.title = n;
    }
    // Never save over the .idml with the native format: the document starts unsaved.
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}
