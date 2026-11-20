//! Document fonts handed over as file contents rather than read from a folder: the web build,
//! which can't look beside a file, gets them from a package folder or zip the user picks or drops
//! (see [`FontDb::load_document_font_files`]).

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use super::FontDb;
use super::db::NEXT_SCOPE;
use super::face::{
    DocumentFonts, FontBytes, FontFace, FontSource, MAX_DOCUMENT_FONT_BYTES, MAX_DOCUMENT_FONT_FILES, MAX_DOCUMENT_FONTS_TOTAL, enumerate_faces,
    make_face,
};

/// Whether a `Document Fonts` folder loads a file named `name`: .ttf, .otf, .ttc and .otc, in
/// any case. Web fonts (.woff, .woff2) aren't read.
pub fn is_document_font_name(name: &str) -> bool {
    let ext = std::path::Path::new(name).extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc"))
}

impl FontDb {
    /// [`FontDb::load_document_fonts`] for fonts given as (file name, contents) instead of a folder
    /// to read: the files whose names [`is_document_font_name`] accepts load under a new scope,
    /// for one document alone, with the folder's limits ([`MAX_DOCUMENT_FONT_FILES`] in name
    /// order, [`MAX_DOCUMENT_FONT_BYTES`] each, [`MAX_DOCUMENT_FONTS_TOTAL`] in all). The contents
    /// are untrusted: a file that isn't a font is skipped with a note, like one over a limit. No
    /// font: scope 0.
    pub fn load_document_font_files(&self, files: Vec<(String, Vec<u8>)>) -> DocumentFonts {
        let mut out = DocumentFonts::default();
        let mut files: Vec<(String, Vec<u8>)> = files.into_iter().filter(|(name, _)| is_document_font_name(name)).collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let count = files.len();
        let mut total = 0u64;
        let mut own: Vec<Arc<FontFace>> = Vec::new();
        for (n, (name, data)) in files.into_iter().enumerate() {
            if n == MAX_DOCUMENT_FONT_FILES {
                out.skipped.push(format!("{} more font files not loaded (at most {MAX_DOCUMENT_FONT_FILES} are)", count - n));
                break;
            }
            let len = data.len() as u64;
            if len > MAX_DOCUMENT_FONT_BYTES {
                out.skipped.push(format!("{name}: larger than {} MB", MAX_DOCUMENT_FONT_BYTES >> 20));
                continue;
            }
            if total.saturating_add(len) > MAX_DOCUMENT_FONTS_TOTAL {
                out.skipped.push(format!("{name}: over the {} MB the folder's fonts may take in all", MAX_DOCUMENT_FONTS_TOTAL >> 20));
                continue;
            }
            let data = Arc::new(data);
            let source = FontSource::DocumentContents(name.clone());
            let faces: Vec<Arc<FontFace>> = enumerate_faces(&data)
                .into_iter()
                .filter_map(|(i, family, style, coords)| make_face(FontBytes::Owned(data.clone()), source.clone(), i, family, style, coords))
                .map(Arc::new)
                .collect();
            if faces.is_empty() {
                out.skipped.push(format!("{name}: not a font DesignCraft can read"));
                continue;
            }
            total = total.saturating_add(len);
            // A family and style the folder already gave stays with its first file.
            for f in faces {
                if !own.iter().any(|o| o.family.eq_ignore_ascii_case(&f.family) && o.style.eq_ignore_ascii_case(&f.style)) {
                    own.push(f);
                }
            }
        }
        if !own.is_empty() {
            out.scope = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
            out.faces = own.len();
            self.scopes.write().unwrap_or_else(PoisonError::into_inner).insert(out.scope, own.into());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::is_document_font_name;
    use crate::FontDb;
    use crate::testing::font_with;

    #[test]
    fn font_names_are_recognised_by_their_extension() {
        for name in ["a.ttf", "Document Fonts/B.OTF", "c.ttc", "d.otc"] {
            assert!(is_document_font_name(name), "{name}");
        }
        for name in ["a.woff", "b.woff2", "c.txt", "ttf", "Document Fonts"] {
            assert!(!is_document_font_name(name), "{name}");
        }
    }

    /// Fonts given as contents load under their own scope, with the folder loader's rules: other
    /// files are ignored, damaged ones are skipped with a note, and the first file of a family and
    /// style wins.
    #[test]
    fn document_fonts_load_from_contents() {
        let db = FontDb::global();
        let family = "Contents Loaded Test Family";
        let files = vec![
            ("Document Fonts/b.otf".to_string(), font_with(family, &['H']).unwrap()),
            ("Document Fonts/a.ttf".to_string(), font_with(family, &['H']).unwrap()),
            ("Document Fonts/broken.ttf".to_string(), b"not a font".to_vec()),
            ("Document Fonts/readme.txt".to_string(), b"hello".to_vec()),
        ];
        let r = db.load_document_font_files(files);
        assert_ne!(r.scope, 0);
        assert_eq!(r.faces, 1, "one family and style, from the first file");
        assert_eq!(r.skipped, ["Document Fonts/broken.ttf: not a font DesignCraft can read"]);
        let face = db.scoped(r.scope).face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family));
        assert_eq!(face.source, crate::FontSource::DocumentContents("Document Fonts/a.ttf".into()));
        assert_eq!(face.source.path(), None, "no file here is behind it");
        assert!(!db.scoped(0).face(family, "Regular").family.eq_ignore_ascii_case(family), "other documents don't see it");
        db.close_scope(r.scope);
        assert_eq!(db.load_document_font_files(vec![("x.ttf".into(), Vec::new())]).scope, 0, "no font: no scope");
    }
}
