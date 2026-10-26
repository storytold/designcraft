//! Scribus documents (`.sla`): opened through [`designcraft_sla`], their linked images read
//! beside the file through [`crate::io`] (the automation read root applies), as IDML packages'
//! are.

use serde_json::{Value, json};

use super::file::base64_decode;
use super::{CommandSpec, always, bad, cmd, str_param};
use crate::{DocState, EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(noundo "file.openScribus", "Open Scribus Document", [], None,
        "{path | base64, name?} — opens a Scribus document (.sla, 1.5 to 1.7) as a new document: pages and master pages, layers, colours, paragraph and character styles, threaded text frames, images (linked images are read beside the file), shapes and groups; the fonts in a `Document Fonts` folder beside it load first → {index, documentFonts, warnings: what couldn't be carried over (tables, text on a path, missing or unsupported images, …), font files skipped}",
        always, open_sla)]
}

/// Is `bytes` a Scribus document? `file.open` and `file.openBytes` route these here.
pub(crate) fn is_sla(bytes: &[u8]) -> bool {
    designcraft_sla::is_sla(bytes)
}

pub(crate) fn open_sla(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, file, title) = if let Some(b) = str_param(p, "base64") {
        let name = str_param(p, "name").unwrap_or_default();
        if base64_decoded_len(b) > designcraft_sla::MAX_BYTES {
            return Err(too_large());
        }
        (base64_decode(b), None, without_sla_extension(name).to_string())
    } else if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        let b = crate::io::read_capped(path, designcraft_sla::MAX_BYTES as u64).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        #[cfg(not(target_arch = "wasm32"))]
        if b.len() > designcraft_sla::MAX_BYTES {
            return Err(too_large());
        }
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = Vec::new();
        let pp = std::path::Path::new(path);
        (b, Some(pp.to_path_buf()), pp.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
    } else {
        return Err(bad("file.openScribus", "missing `path` or `base64`"));
    };
    let read = |link: &str| -> Option<Vec<u8>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            super::interchange::read_packaged_link(link, file.as_deref()).map(|(_, data)| data).or_else(|| {
                // A file that is there but holds no format DesignCraft places (GIMP's .xcf): its
                // first bytes let the import say so instead of calling it missing. A graphic
                // refused for its size is never handed over cut short.
                let path = super::interchange::find_packaged_file(link, file.as_deref())?;
                crate::io::read_capped(&path, 64).ok().filter(|b| is_unplaceable(b))
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = link;
            None
        }
    };
    let has_family = |family: &str| designcraft_fonts::FontDb::global().has_family(family);
    let imported = designcraft_sla::import_sla_report(&bytes, &read, &has_family).map_err(|e| EngineError::Other(e.to_string()))?;
    let mut d = imported.document;
    let mut warnings = imported.warnings;
    #[cfg(not(target_arch = "wasm32"))]
    super::interchange::resolve_packaged_links(&mut d, file.as_deref());
    if !title.is_empty() {
        d.title = title;
    }
    let (fonts, faces, font_warnings) = match str_param(p, "path").filter(|_| str_param(p, "base64").is_none()) {
        Some(path) => super::file::load_document_fonts(&mut d, path),
        None => (None, 0, Vec::new()),
    };
    warnings.extend(font_warnings);
    warnings.extend(super::style::grep_style_warnings(&d));
    // Never save over the .sla with the native format: the document starts unsaved.
    let mut st = DocState::new(d, None);
    st.fonts = fonts;
    let i = s.add_document(st);
    Ok(json!({"index": i, "documentFonts": faces, "warnings": warnings}))
}

/// The size base64 `b64` decodes to, counting only the characters the decoder reads (it skips
/// line breaks and other whitespace).
fn base64_decoded_len(b64: &str) -> usize {
    let digits = b64.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').count();
    digits / 4 * 3 + (digits % 4).saturating_sub(1)
}

fn too_large() -> EngineError {
    EngineError::Other(format!("the Scribus document is larger than {} MB", designcraft_sla::MAX_BYTES >> 20))
}

/// Bytes no placeable format starts with (`read_packaged_link` read every file that is one).
fn is_unplaceable(head: &[u8]) -> bool {
    !head.is_empty() && designcraft_images::mime(head) == "application/octet-stream" && !designcraft_images::is_eps(head)
}

/// `name` without a trailing .sla (any case).
fn without_sla_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case(designcraft_sla::EXTENSION) => stem,
        _ => name,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const SLA: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SCRIBUSUTF8NEW Version="1.7.2"><Document PageWidth="200" PageHeight="100">
  <Color Name="Black" Space="CMYK" C="0" M="0" Y="0" K="100"/>
  <Page PageXPosition="100" PageYPosition="20" PageWidth="200" PageHeight="100" PageNumber="0"/>
  <PageObject XPosition="110" YPosition="30" OwnPage="0" ItemID="1" ItemType="4" Width="80" Height="40" NextItem="-1" BackItem="-1">
    <StoryText><DefaultStyle/><Content Chars="Hello"/><trail/></StoryText>
  </PageObject>
  <PageObject XPosition="200" YPosition="30" OwnPage="0" ItemID="2" ItemType="2" Width="50" Height="50" ImageFileName="missing.png"/>
</Document></SCRIBUSUTF8NEW>"#;

    #[test]
    fn file_open_recognises_scribus_documents_by_content() {
        let dir = tempfile::tempdir().unwrap();
        // No extension: the content decides.
        let path = dir.path().join("Flyer");
        std::fs::write(&path, SLA).unwrap();
        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("missing.png")), "{r}");
        let st = s.doc().unwrap();
        assert_eq!(st.doc.title, "Flyer");
        assert_eq!(st.path, None, "saving never overwrites the .sla");
        let story = st.doc.stories.values().next().unwrap();
        assert_eq!(story.text, "Hello");
    }

    #[test]
    fn open_bytes_and_the_scribus_command() {
        let mut s = Session::new();
        let b64 = super::super::file::base64_encode(SLA.as_bytes());
        s.execute("file.openBytes", &json!({"base64": b64, "name": "Flyer.sla"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.title, "Flyer");
        let err = s.execute("file.openScribus", &json!({"base64": super::super::file::base64_encode(b"<html/>")})).unwrap_err();
        assert!(err.to_string().contains("not a Scribus document"), "{err}");
    }

    #[test]
    fn base64_size_ignores_line_breaks_and_padding() {
        let encoded = super::super::file::base64_encode(&[7u8; 1000]);
        let wrapped: String = encoded.as_bytes().chunks(76).map(|l| format!("{}\r\n", String::from_utf8_lossy(l))).collect();
        assert_eq!((base64_decoded_len(&encoded), base64_decoded_len(&wrapped)), (1000, 1000));
        for n in 0..6 {
            assert_eq!(base64_decoded_len(&super::super::file::base64_encode(&vec![1u8; n])), n);
        }
    }

    #[test]
    fn only_unplaceable_files_are_probed() {
        assert!(is_unplaceable(b"gimp xcf v011\0\0\0\x10"));
        // The start of a graphic too large to read is not a stand-in for it.
        assert!(!is_unplaceable(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"));
        assert!(!is_unplaceable(b"%!PS-Adobe-3.0 EPSF-3.0"));
        assert!(!is_unplaceable(b""));
    }

    #[test]
    fn unplaceable_linked_files_are_reported_as_such() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("missing.png"), b"gimp xcf v011 not an image").unwrap();
        let path = dir.path().join("Flyer.sla");
        std::fs::write(&path, SLA).unwrap();
        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        let warnings: Vec<&str> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
        assert!(warnings.iter().any(|w| w.contains("can't place") && w.contains("missing.png")), "{warnings:?}");
        assert!(s.doc().unwrap().doc.assets.values().all(|a| a.data.is_empty()), "no partial data is kept");
    }

    #[test]
    fn linked_images_are_read_beside_the_document() {
        let dir = tempfile::tempdir().unwrap();
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="2"/></svg>"#;
        std::fs::write(dir.path().join("missing.png"), svg).unwrap();
        let path = dir.path().join("Flyer.sla");
        std::fs::write(&path, SLA).unwrap();
        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("missing.png")), "{r}");
        let asset = s.doc().unwrap().doc.assets.values().next().unwrap().clone();
        assert_eq!((asset.mime.as_str(), asset.data.len()), ("image/svg+xml", svg.len()));
        assert!(asset.link.as_deref().is_some_and(|l| std::path::Path::new(l).is_absolute()), "the link is held as the file it names");
    }
}
