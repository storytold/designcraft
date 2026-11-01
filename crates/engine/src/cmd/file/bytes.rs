#[cfg(test)]
use super::FAIL_TO_BYTES;
use super::{Document, EngineError, Result};

/// The document as `.designcraft` bytes, or an error when it can't be written; never an empty file.
pub fn to_bytes(d: &Document) -> Result<Vec<u8>> {
    save_bytes(d, None)
}

/// [`to_bytes`] with `preview` (PNG bytes) stored as the file's first-page picture.
fn save_bytes(d: &Document, preview: Option<&[u8]>) -> Result<Vec<u8>> {
    #[cfg(test)]
    if FAIL_TO_BYTES.with(std::cell::Cell::get) {
        return Err(EngineError::Other("could not write the document: injected failure".into()));
    }
    designcraft_format::save_with_preview(d, preview).map_err(|e| EngineError::Other(format!("could not write the document: {e}")))
}

/// `path` ending in `.designcraft`: the extension is added when it is missing or another one, so
/// the file opens from the file manager. A path without a file name is left for the write to fail.
pub fn with_document_extension(path: &str) -> String {
    let p = std::path::Path::new(path);
    let ext = p.extension().and_then(std::ffi::OsStr::to_str);
    if p.file_name().is_none() || ext.is_some_and(|e| e.eq_ignore_ascii_case(designcraft_format::EXTENSION)) {
        return path.to_string();
    }
    format!("{path}.{}", designcraft_format::EXTENSION)
}

/// Whether `bytes` are a document `file.open` reads: a DesignCraft package or an IDML package.
/// The first [`DOCUMENT_SNIFF_BYTES`] of a file are enough for packages that start with their
/// `mimetype` entry, as both formats write them.
pub fn is_document(bytes: &[u8]) -> bool {
    designcraft_idml::is_idml(bytes) || designcraft_format::starts_designcraft(bytes) || designcraft_format::is_designcraft(bytes)
}

/// How much of a file [`is_document`] needs to recognise a package by its `mimetype` entry.
pub const DOCUMENT_SNIFF_BYTES: u64 = 64 << 10;

/// The longer side of the first-page picture saved in the file, in pixels.
pub(super) const PREVIEW_PX: f64 = 320.0;

/// [`to_bytes`] with a picture of the first page, which the start screen shows next to the file
/// in its recent files. A page that can't be drawn just leaves the picture out.
pub(super) fn to_bytes_with_preview(d: &Document, cache: &crate::Cache) -> Result<Vec<u8>> {
    let preview = d.page_loc(0).and_then(|(si, pi)| {
        let b = d.spreads.get(si)?.pages.get(pi)?.bounds();
        let side = b.width().max(b.height());
        (side.is_finite() && side > 0.0).then_some(PREVIEW_PX / side)
    });
    let png =
        preview.and_then(|scale| designcraft_render::Renderer::new().render_page(d, cache, 0, scale, false, &Default::default())).map(|r| r.to_png());
    save_bytes(d, png.as_deref())
}

pub fn from_bytes(b: &[u8]) -> Result<Document> {
    load_bytes(b).map(|(d, _)| d)
}

/// Read `.designcraft` bytes and repair what a hand-edited file can get wrong without failing the
/// open, with a warning for each repair.
pub(super) fn load_bytes(b: &[u8]) -> Result<(Document, Vec<String>)> {
    let mut d = designcraft_format::load(b).map_err(|e| EngineError::Other(e.to_string()))?;
    let warnings = d.fix_parent_bases();
    Ok((d, warnings))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod extension_tests {
    use serde_json::json;

    use super::{is_document, with_document_extension};
    use crate::Session;

    #[test]
    fn the_document_extension_is_added_when_missing_or_different() {
        assert_eq!(with_document_extension("/a/Brochure"), "/a/Brochure.designcraft");
        assert_eq!(with_document_extension("/a/Brochure.idml"), "/a/Brochure.idml.designcraft");
        assert_eq!(with_document_extension("/a/v1.2 Brochure"), "/a/v1.2 Brochure.designcraft");
        assert_eq!(with_document_extension("/a/Brochure.DesignCraft"), "/a/Brochure.DesignCraft");
        assert_eq!(with_document_extension("/a/.."), "/a/..");
    }

    #[test]
    fn save_adds_the_extension_and_open_recognises_files_by_content() {
        let dir = std::env::temp_dir().join(format!("dc-file-extension-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();

        let bare = dir.join("Brochure");
        let written = dir.join("Brochure.designcraft");
        let r = s.execute("file.saveAs", &json!({"path": bare.to_string_lossy()})).unwrap();
        assert_eq!(r["path"], json!(written.to_string_lossy()), "{r}");
        assert!(written.is_file() && !bare.exists());
        assert_eq!(s.doc().unwrap().path.as_deref(), written.to_str());
        let r = s.execute("file.saveACopy", &json!({"path": dir.join("Copy").to_string_lossy()})).unwrap();
        assert_eq!(r["path"], json!(dir.join("Copy.designcraft").to_string_lossy()), "{r}");

        // A native file without the extension opens, and Save writes back to that same file.
        std::fs::rename(&written, &bare).unwrap();
        let mut t = Session::new();
        t.execute("file.open", &json!({"path": bare.to_string_lossy()})).unwrap();
        let r = t.execute("file.save", &json!({})).unwrap();
        assert_eq!(r["path"], json!(bare.to_string_lossy()), "{r}");
        assert!(!written.exists());

        // An IDML package without the extension opens as IDML, unsaved.
        let idml = dir.join("Package");
        std::fs::write(&idml, designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        assert!(is_document(&std::fs::read(&idml).unwrap()) && is_document(&std::fs::read(&bare).unwrap()));
        // The start of the file is enough.
        for path in [&idml, &bare] {
            let bytes = std::fs::read(path).unwrap();
            assert!(is_document(&bytes[..bytes.len().min(128)]), "{path:?}");
        }
        t.execute("file.open", &json!({"path": idml.to_string_lossy()})).unwrap();
        assert_eq!(t.doc().unwrap().path, None);

        let notes = dir.join("Notes");
        std::fs::write(&notes, b"plain text").unwrap();
        assert!(!is_document(b"plain text"));
        let e = t.execute("file.open", &json!({"path": notes.to_string_lossy()})).unwrap_err();
        assert!(e.to_string().contains("not a DesignCraft or IDML file"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
