//! Interchange formats: IDML (InDesign Markup Language) import and export.

use designcraft_doc::Document;
use serde_json::{Value, json};

use super::file::base64_encode;
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
        document
    };
    Ok(document)
}

#[cfg(unix)]
fn read_packaged_link(link: &str, dir: Option<&std::path::Path>) -> Option<(String, Vec<u8>)> {
    read_packaged_link_with_limit(link, dir, designcraft_idml::MAX_LINKED_RESOURCE_BYTES)
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn read_packaged_link(_link: &str, _dir: Option<&std::path::Path>) -> Option<(String, Vec<u8>)> {
    // External IDML links need descriptor-relative traversal that rejects reparse points at every
    // component. Until the native platform has that implementation, fail closed; embedded
    // resources continue to import normally.
    None
}

#[cfg(unix)]
fn read_packaged_link_with_limit(link: &str, dir: Option<&std::path::Path>, max_bytes: u64) -> Option<(String, Vec<u8>)> {
    use std::path::{Component, Path};

    use rustix::fs::{FileType, Mode, OFlags, fstat, open};

    let relative = Path::new(link);
    if relative.as_os_str().is_empty()
        || link.contains(['\\', ':'])
        || !relative.components().all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let dir = dir?;
    let root = open(dir, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).ok()?;
    let name = relative.file_name()?;
    let candidates = [relative.to_path_buf(), Path::new("Links").join(relative), Path::new(name).to_path_buf(), Path::new("Links").join(name)];
    let mut seen = std::collections::HashSet::new();
    for candidate in candidates {
        if !seen.insert(candidate.clone()) {
            continue;
        }
        let Some(file) = open_beneath(&root, &candidate) else {
            continue;
        };
        let Ok(stat) = fstat(&file) else {
            continue;
        };
        let Ok(size) = u64::try_from(stat.st_size) else {
            continue;
        };
        if !FileType::from_raw_mode(stat.st_mode).is_file() || size > max_bytes {
            continue;
        }
        let capacity = usize::try_from(size).ok()?;
        let mut data = Vec::with_capacity(capacity);
        let mut chunk = [0u8; 8 * 1024];
        loop {
            let Ok(read) = rustix::io::read(&file, &mut chunk) else {
                data.clear();
                break;
            };
            if read == 0 {
                break;
            }
            if (data.len() as u64).checked_add(read as u64).is_none_or(|total| total > max_bytes) {
                data.clear();
                break;
            }
            data.extend_from_slice(&chunk[..read]);
        }
        if data.len() as u64 == size {
            let display = std::path::absolute(dir.join(&candidate)).ok()?;
            return Some((display.to_string_lossy().into_owned(), data));
        }
    }
    None
}

#[cfg(unix)]
fn open_beneath(root: &impl std::os::fd::AsFd, relative: &std::path::Path) -> Option<std::os::fd::OwnedFd> {
    use std::path::Component;

    use rustix::fs::{Mode, OFlags, openat};

    let mut components = relative.components().peekable();
    let mut current = openat(root, ".", OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).ok()?;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return None;
        };
        let flags = if components.peek().is_some() {
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
        } else {
            // Opening a named pipe read-only can otherwise block before `fstat` rejects it.
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC
        };
        current = openat(&current, name, flags, Mode::empty()).ok()?;
    }
    Some(current)
}

fn decode_idml_base64(encoded: &str) -> Result<Vec<u8>> {
    let canonical_len = designcraft_idml::MAX_ARCHIVE_BYTES
        .checked_add(2)
        .and_then(|value| value.checked_div(3))
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| EngineError::Other("IDML base64 size overflow".into()))?;
    let input_limit = canonical_len
        .checked_add(canonical_len / 16)
        .and_then(|value| value.checked_add(1_024))
        .ok_or_else(|| EngineError::Other("IDML base64 size overflow".into()))?;
    decode_base64_bounded(encoded, designcraft_idml::MAX_ARCHIVE_BYTES, input_limit)
}

fn decode_base64_bounded(encoded: &str, max_output: usize, max_input: usize) -> Result<Vec<u8>> {
    if encoded.len() > max_input {
        return Err(EngineError::Other(format!("IDML base64 exceeds {max_input} encoded bytes")));
    }
    let capacity = encoded.len().saturating_mul(3).saturating_div(4).min(max_output);
    let mut output = Vec::with_capacity(capacity);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        buffer = buffer << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            if output.len() >= max_output {
                return Err(EngineError::Other(format!("IDML archive exceeds {max_output} bytes")));
            }
            output.push((buffer >> bits) as u8);
        }
    }
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_idml_file(path: &str) -> Result<Vec<u8>> {
    read_idml_file_bounded(path, designcraft_idml::MAX_ARCHIVE_BYTES as u64)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_idml_file_bounded(path: &str, max_bytes: u64) -> Result<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let metadata = file.metadata().map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if !metadata.is_file() {
        return Err(EngineError::Other(format!("{path}: not a regular file")));
    }
    if metadata.len() > max_bytes {
        return Err(EngineError::Other(format!("{path}: IDML archive exceeds {max_bytes} bytes")));
    }
    let capacity = usize::try_from(metadata.len()).map_err(|_| EngineError::Other(format!("{path}: IDML archive is too large for this platform")))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if bytes.len() as u64 > max_bytes {
        return Err(EngineError::Other(format!("{path}: IDML archive exceeds {max_bytes} bytes")));
    }
    Ok(bytes)
}

pub(crate) fn open_idml(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, dir, name) = if let Some(b) = str_param(p, "base64") {
        (decode_idml_base64(b)?, None, str_param(p, "name").map(|n| n.trim_end_matches(".idml").to_string()))
    } else if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let b = read_idml_file(path)?;
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn test_dir(case: &str) -> std::path::PathBuf {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("dc-idml-{case}-{}-{id}", std::process::id()))
    }

    fn linked_idml(link: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        let png = designcraft_render::Rendered { width: 2, height: 2, pixels: vec![255; 16] }.to_png();
        let mut session = Session::new();
        session.execute("file.new", &json!({}))?;
        session.execute("file.place", &json!({"base64": base64_encode(&png), "name": "logo.png"}))?;
        session.edit(|doc, _| {
            let asset = doc.assets.values_mut().next().ok_or_else(|| bad("fixture", "missing graphic"))?;
            Arc::make_mut(asset).link = Some(link.into());
            Ok(Value::Null)
        })?;
        let bytes = designcraft_idml::export_idml_with(&session.doc()?.doc, &designcraft_idml::ExportOptions { embed_images: false });
        Ok((bytes, png))
    }

    #[cfg(unix)]
    #[test]
    fn opens_nested_packaged_links_from_a_different_computer() {
        let dir = test_dir("nested-links");
        let path = dir.join("Links").join("illustrations").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("C:\\original-computer\\project\\Links\\illustrations\\logo.png").unwrap();
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

    #[cfg(unix)]
    #[test]
    fn remembers_flat_packaged_link_location() {
        let dir = test_dir("flat-links");
        let path = dir.join("Links").join("logo.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (idml, png) = linked_idml("/original-computer/project/logo.png").unwrap();
        std::fs::write(&path, &png).unwrap();

        let doc = import(&idml, Some(&dir)).unwrap();
        let asset = doc.assets.values().next().unwrap();
        assert_eq!(*asset.data, png);
        assert_eq!(asset.link.as_deref(), std::path::absolute(&path).unwrap().to_str());
        assert_eq!(super::super::links::status(asset), "ok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn linked_resources_stay_beside_the_package() {
        let root = test_dir("confined-links");
        let package = root.join("package");
        std::fs::create_dir_all(package.join("Links")).unwrap();
        std::fs::write(package.join("Links/photo.jpg"), b"jpeg").unwrap();
        std::fs::write(package.join("large.jpg"), b"12345").unwrap();
        assert_eq!(read_packaged_link_with_limit("photo.jpg", Some(&package), 4).map(|(_, data)| data), Some(b"jpeg".to_vec()));
        assert!(read_packaged_link_with_limit("../photo.jpg", Some(&package), 4).is_none());
        assert!(read_packaged_link_with_limit("/photo.jpg", Some(&package), 4).is_none());
        assert!(read_packaged_link_with_limit("large.jpg", Some(&package), 4).is_none());

        use std::os::unix::fs::symlink;

        let outside = root.join("outside.jpg");
        std::fs::write(&outside, b"data").unwrap();
        symlink(&outside, package.join("escape.jpg")).unwrap();
        assert!(read_packaged_link_with_limit("escape.jpg", Some(&package), 4).is_none());

        let outside_dir = root.join("outside");
        std::fs::create_dir_all(&outside_dir).unwrap();
        std::fs::write(outside_dir.join("nested.jpg"), b"data").unwrap();
        symlink(&outside_dir, package.join("Links/nested")).unwrap();
        assert!(read_packaged_link_with_limit("nested/nested.jpg", Some(&package), 4).is_none());

        let alias = root.join("package-alias");
        symlink(&package, &alias).unwrap();
        assert!(read_packaged_link_with_limit("photo.jpg", Some(&alias), 4).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn idml_sources_are_bounded_before_ingestion() {
        assert_eq!(decode_base64_bounded("YWJj", 3, 4).unwrap(), b"abc");
        assert!(decode_base64_bounded("YWJjZA==", 3, 8).is_err());
        assert!(decode_base64_bounded("YWJj ", 4, 4).is_err());

        let root = test_dir("bounded-source");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("document.idml");
        std::fs::write(&path, b"12345").unwrap();
        assert!(read_idml_file_bounded(path.to_str().unwrap(), 4).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
