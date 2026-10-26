//! Books (File › New / Open Book): an ordered list of documents with continuous page numbers,
//! styles synchronised from a style source, and one PDF for the whole book. Saved as `.dcbook`.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, ok, str_param};
use crate::{EngineError, Result, Session};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Book {
    #[serde(skip)]
    pub path: String,
    pub documents: Vec<String>,
    /// Index of the style source document.
    pub style_source: usize,
}

fn has_book(s: &Session) -> std::result::Result<(), String> {
    if s.book.is_some() { Ok(()) } else { Err("no book open".into()) }
}

fn no_book() -> EngineError {
    bad("book", "no book open")
}

#[cfg(not(target_arch = "wasm32"))]
fn save(b: &Book) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(b).map_err(|e| EngineError::Other(e.to_string()))?;
    crate::io::write_file(&b.path, &bytes).map_err(EngineError::Other)
}

#[cfg(target_arch = "wasm32")]
fn save(_: &Book) -> Result<()> {
    Err(EngineError::Other("books need a file system".into()))
}

/// A chapter's file, resolved against the book's file. A chapter on another computer (other than
/// the book's own network share) or a device is not opened ([`crate::io::document_path`]).
fn chapter(b: &Book, path: &str) -> Result<String> {
    crate::io::document_path(path, Some(std::path::Path::new(&b.path)))
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|why| EngineError::Other(format!("{path}: {why}")))
}

/// Every chapter's file ([`chapter`]), in order.
fn chapters(b: &Book) -> Result<Vec<String>> {
    b.documents.iter().map(|path| chapter(b, path)).collect()
}

fn load(path: &str) -> Result<designcraft_doc::Document> {
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = crate::io::read_file(path).map_err(EngineError::Other)?;
    #[cfg(target_arch = "wasm32")]
    let bytes: Vec<u8> = {
        let _ = path;
        vec![]
    };
    if path.to_lowercase().ends_with(".idml") {
        return super::interchange::import_idml_confined(&bytes, Some(std::path::Path::new(path)))
            .map_err(|e| EngineError::Other(format!("{path}: {e}")));
    }
    super::file::from_bytes(&bytes).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

#[cfg(not(target_arch = "wasm32"))]
const MAX_BATCH_CHAPTERS: usize = 4096;

/// A batch keeps only bounded path records in memory. Chapter snapshots live in a private
/// temporary directory; only one document and serialized blob are live while staging.
#[cfg(not(target_arch = "wasm32"))]
struct ChapterBatch {
    directory: tempfile::TempDir,
    chapters: Vec<(String, tempfile::TempPath)>,
    command: &'static str,
}

#[cfg(not(target_arch = "wasm32"))]
impl ChapterBatch {
    fn new(chapters: usize, command: &'static str) -> Result<Self> {
        if chapters > MAX_BATCH_CHAPTERS {
            return Err(bad(command, format!("a book update supports at most {MAX_BATCH_CHAPTERS} chapters")));
        }
        let directory = tempfile::Builder::new()
            .prefix(".designcraft-book-")
            .tempdir()
            .map_err(|e| EngineError::Other(format!("{command}: cannot stage chapter changes: {e}")))?;
        Ok(Self { directory, chapters: Vec::with_capacity(chapters), command })
    }

    fn stage(&mut self, path: &str, doc: &designcraft_doc::Document) -> Result<()> {
        use std::io::Write;

        // Propagate serialization errors; an empty fallback must never replace a chapter.
        let bytes = designcraft_format::save(doc).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        let mut file = tempfile::Builder::new()
            .prefix("chapter-")
            .tempfile_in(self.directory.path())
            .map_err(|e| EngineError::Other(format!("{path}: cannot stage changes: {e}")))?;
        file.write_all(&bytes).map_err(|e| EngineError::Other(format!("{path}: cannot stage changes: {e}")))?;
        self.chapters.push((path.to_string(), file.into_temp_path()));
        Ok(())
    }

    fn commit(self) -> Result<()> {
        self.commit_with(|path, staged| crate::io::replace_with(path, staged))
    }

    fn commit_with(self, mut write: impl FnMut(&std::path::Path, &std::path::Path) -> std::io::Result<()>) -> Result<()> {
        let mut completed = Vec::with_capacity(self.chapters.len());
        for (path, staged) in self.chapters {
            if let Err(e) = write(std::path::Path::new(&path), &staged) {
                // Each file is atomic; the whole book is not. Keep the successful prefix visible
                // rather than pretending that an error undid earlier chapter replacements.
                return Err(EngineError::Other(format!(
                    "{}: failed to save chapter {path}: {e}; completed chapter paths: {completed:?}",
                    self.command
                )));
            }
            completed.push(path);
        }
        Ok(())
    }
}

/// Batch commands only write closed native chapters. IDML is an imported source, and an open
/// chapter's model (including unsaved edits) cannot be reconciled by a disk-only book update.
#[cfg(not(target_arch = "wasm32"))]
fn writable_chapter(s: &Session, path: &str, command: &'static str) -> Result<()> {
    use std::path::Path;

    if Path::new(path).extension().and_then(std::ffi::OsStr::to_str).is_some_and(|ext| ext.eq_ignore_ascii_case("idml")) {
        return Err(bad(command, format!("cannot change imported IDML chapter {path}; convert it to a native DesignCraft document first")));
    }
    let target = crate::io::canonicalize(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if s.docs.iter().any(|doc| doc.path.as_ref().is_some_and(|open| crate::io::canonicalize(open).is_ok_and(|open| open == target))) {
        return Err(bad(command, format!("chapter {path} is open; close it before updating the book")));
    }
    if !crate::io::is_file(&target) {
        return Err(bad(command, format!("chapter {path} is not a regular file")));
    }
    // Check both the file and its resolved parent without truncating it. Replacement needs a
    // writable directory even when an in-place write would be permitted.
    crate::io::writable(&target).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let parent = target.parent().ok_or_else(|| bad(command, format!("chapter {path} has no parent folder")))?;
    tempfile::Builder::new().prefix(".designcraft-book-check-").tempfile_in(parent).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn paginate(s: &Session) -> Result<Value> {
    let b = s.book.as_ref().ok_or_else(no_book)?;
    if b.documents.is_empty() {
        return Ok(json!({"pages": 0, "chapters": []}));
    }
    let mut batch = ChapterBatch::new(b.documents.len(), "book.paginate")?;
    let mut next = 1u32;
    let mut chapter = 0u32;
    let mut chapters = Vec::with_capacity(b.documents.len());
    for path in &self::chapters(b)? {
        writable_chapter(s, path, "book.paginate")?;
        let mut d = load(path)?;
        start_at(&mut d, next);
        let pages = u32::try_from(d.page_count()).map_err(|_| bad("book.paginate", "too many pages for continuous numbering"))?;
        next = next.checked_add(pages).ok_or_else(|| bad("book.paginate", "too many pages for continuous numbering"))?;
        chapter = match d.settings.chapter_source {
            designcraft_doc::ChapterSource::Automatic => chapter.saturating_add(1),
            designcraft_doc::ChapterSource::UserDefined => d.settings.chapter_number.max(1),
            designcraft_doc::ChapterSource::SameAsPrevious => chapter.max(1),
        };
        d.settings.chapter_number = chapter;
        chapters.push(d.chapter_label());
        batch.stage(path, &d)?;
    }
    batch.commit()?;
    Ok(json!({"pages": next - 1, "chapters": chapters}))
}

#[cfg(not(target_arch = "wasm32"))]
fn sync_styles(s: &Session) -> Result<Value> {
    let b = s.book.as_ref().ok_or_else(no_book)?;
    let mut batch = ChapterBatch::new(b.documents.len(), "book.syncStyles")?;
    let paths = chapters(b)?;
    let src_path = paths.get(b.style_source).ok_or_else(|| bad("book.syncStyles", "the book is empty"))?;
    let src = load(src_path)?;
    let styles = src.styles.clone();
    let swatches = src.swatches.clone();
    drop(src);
    let mut n = 0;
    for (i, path) in paths.iter().enumerate() {
        if i == b.style_source {
            continue;
        }
        writable_chapter(s, path, "book.syncStyles")?;
        let mut d = load(path)?;
        {
            let st = d.styles_mut();
            for ps in &styles.paragraph {
                st.paragraph.retain(|x| x.name != ps.name);
                st.paragraph.push(ps.clone());
            }
            for cs in &styles.character {
                st.character.retain(|x| x.name != cs.name);
                st.character.push(cs.clone());
            }
        }
        for sw in &swatches {
            match d.swatches.iter_mut().find(|w| w.name == sw.name) {
                Some(w) if !w.locked => *w = sw.clone(),
                Some(_) => {}
                None => d.swatches.push(sw.clone()),
            }
        }
        batch.stage(path, &d)?;
        n += 1;
    }
    batch.commit()?;
    Ok(json!({"synced": n}))
}

#[cfg(target_arch = "wasm32")]
fn paginate(_: &Session) -> Result<Value> {
    Err(EngineError::Other("books need a file system".into()))
}

#[cfg(target_arch = "wasm32")]
fn sync_styles(_: &Session) -> Result<Value> {
    Err(EngineError::Other("books need a file system".into()))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "book/safety_tests.rs"]
mod safety_tests;

/// Make page numbering of `d` start at `n` (the first section).
#[cfg(not(target_arch = "wasm32"))]
fn start_at(d: &mut designcraft_doc::Document, n: u32) {
    if !d.sections.iter().any(|x| x.start == 0) {
        d.sections.insert(
            0,
            designcraft_doc::Section {
                start: 0,
                start_number: None,
                style: Default::default(),
                prefix: String::new(),
                marker: String::new(),
                include_prefix: false,
            },
        );
    }
    if let Some(first) = d.sections.iter_mut().find(|x| x.start == 0) {
        first.start_number = Some(n.max(1));
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "book.new", "New Book", ["File", "New"], None, "{path} — an empty book file (.dcbook)", always, |s, p| {
            let path = str_param(p, "path").ok_or_else(|| bad("book.new", "`path` required"))?.to_string();
            let b = Book { path, ..Default::default() };
            save(&b)?;
            s.book = Some(b);
            ok()
        }),
        cmd!(noundo "book.open", "Open Book", [], None, "{path} → {documents}", always, |s, p| {
            let path = str_param(p, "path").ok_or_else(|| bad("book.open", "`path` required"))?.to_string();
            #[cfg(not(target_arch = "wasm32"))]
            let text = crate::io::read_to_string(&path).map_err(EngineError::Other)?;
            #[cfg(target_arch = "wasm32")]
            let text = String::new();
            let mut b: Book = serde_json::from_str(&text).map_err(|e| bad("book.open", format!("not a book: {e}")))?;
            b.path = path;
            let n = b.documents.len();
            s.book = Some(b);
            Ok(json!({"documents": n}))
        }),
        cmd!(noundo "book.close", "Close Book", [], None, "{}", has_book, |s, _| {
            s.book = None;
            ok()
        }),
        cmd!(noundo "book.add", "Add Document", [], None, "{path, at?: index}", has_book, |s, p| {
            let path = str_param(p, "path").ok_or_else(|| bad("book.add", "`path` required"))?.to_string();
            load(&path)?;
            let mut b = s.book.as_ref().ok_or_else(no_book)?.clone();
            let at = p.get("at").and_then(Value::as_u64).map_or(b.documents.len(), |i| (i as usize).min(b.documents.len()));
            b.documents.insert(at, path);
            save(&b)?;
            s.book = Some(b);
            ok()
        }),
        cmd!(noundo "book.remove", "Remove Document", [], None, "{index}", has_book, |s, p| {
            let i = p.get("index").and_then(Value::as_u64).ok_or_else(|| bad("book.remove", "`index` required"))? as usize;
            let mut b = s.book.as_ref().ok_or_else(no_book)?.clone();
            if i >= b.documents.len() {
                return Err(bad("book.remove", format!("no document {i}")));
            }
            b.documents.remove(i);
            if b.style_source >= b.documents.len() {
                b.style_source = 0;
            }
            save(&b)?;
            s.book = Some(b);
            ok()
        }),
        cmd!(noundo "book.styleSource", "Style Source", [], None, "{index}", has_book, |s, p| {
            let i = p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let mut b = s.book.as_ref().ok_or_else(no_book)?.clone();
            if i >= b.documents.len() {
                return Err(bad("book.styleSource", format!("no document {i}")));
            }
            b.style_source = i;
            save(&b)?;
            s.book = Some(b);
            ok()
        }),
        cmd!(query "book.list", "Book", [], None, "{} → {path, styleSource, documents: [{path, pages, firstPage}]}", has_book, |s, _| {
            let b = s.book.as_ref().ok_or_else(no_book)?;
            let mut first = 1usize;
            let mut docs = Vec::new();
            for p in &b.documents {
                let pages = chapter(b, p).and_then(|p| load(&p)).map(|d| d.page_count()).unwrap_or(0);
                docs.push(json!({"path": p, "pages": pages, "firstPage": first}));
                first = first.saturating_add(pages);
            }
            Ok(json!({"path": b.path, "styleSource": b.style_source, "documents": docs}))
        }),
        cmd!(noundo "book.paginate", "Update Numbering", [], None, "{} — each document starts numbering where the previous one ended, and takes its chapter number (automatic: one more than the previous document's; same as previous; or its own start) (files are saved) → {pages, chapters}", has_book, |s, _| {
            paginate(s)
        }),
        cmd!(noundo "book.syncStyles", "Synchronize Book", [], None, "{} — paragraph and character styles and swatches of the style source go into every document (by name)", has_book, |s, _| {
            sync_styles(s)
        }),
        cmd!(noundo "book.exportPdf", "Export Book to PDF…", [], None, "{path?} — every document in order as one PDF → {path, bytes, pages} (no path: {base64, …})", has_book, |s, p| {
            let b = s.book.clone().ok_or_else(no_book)?;
            let mut parts = Vec::new();
            for path in &chapters(&b)? {
                let mut d = load(path)?;
                // The chapter's own fonts, for this export (skipped font files are logged).
                let _fonts = super::file::load_document_fonts(&mut d, path).0;
                let cache = designcraft_compose::Cache::new();
                let r = designcraft_pdf::export_pdf_with_report(&super::interchange::with_pdf_boxes(&d), &cache, &designcraft_pdf::PdfOptions::default()).map_err(|e| EngineError::Other(e.to_string()))?;
                let n = designcraft_render::pdf_page_count(&r.bytes).unwrap_or(0);
                let sizes: Vec<(f32, f32)> = (0..n).filter_map(|i| designcraft_render::pdf_page_size(&r.bytes, i)).map(|(w, h)| (w as f32, h as f32)).collect();
                parts.push((r.bytes, sizes));
            }
            let pages: usize = parts.iter().map(|p| p.1.len()).sum();
            let bytes = designcraft_pdf::merge_pdfs(&parts, None).map_err(|e| EngineError::Other(e.to_string()))?;
            match str_param(p, "path") {
                Some(path) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    crate::io::write_file(path, &bytes).map_err(EngineError::Other)?;
                    Ok(json!({"path": path, "bytes": bytes.len(), "pages": pages}))
                }
                None => Ok(json!({"base64": super::file::base64_encode(&bytes), "bytes": bytes.len(), "pages": pages})),
            }
        }),
    ]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn book_numbering_resolves_chapter_numbers_in_order() {
        let dir = std::env::temp_dir().join(format!("dc-book-chapters-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = |n: &str| dir.join(n).to_string_lossy().to_string();
        let mut s = Session::new();
        s.execute("book.new", &json!({"path": p("Field Guide.dcbook")})).unwrap();
        let chapters = [
            ("a.designcraft", json!({"style": "upperRoman"})),
            ("b.designcraft", json!({"source": "sameAsPrevious"})),
            ("c.designcraft", json!({})),
            ("d.designcraft", json!({"start": 7, "style": "lowerLetters"})),
            ("e.designcraft", json!({})),
        ];
        for (name, numbering) in &chapters {
            s.execute("file.new", &json!({})).unwrap();
            if numbering.as_object().is_some_and(|o| !o.is_empty()) {
                s.execute("layout.chapterNumbering", numbering).unwrap();
            }
            s.execute("file.saveAs", &json!({"path": p(name)})).unwrap();
            s.execute("book.add", &json!({"path": p(name)})).unwrap();
            // Book updates write closed chapters only.
            s.execute("file.close", &json!({})).unwrap();
        }
        let r = s.execute("book.paginate", &json!({})).unwrap();
        // Same as previous takes the number, written in the document's own style.
        assert_eq!(r["chapters"], json!(["I", "1", "2", "g", "8"]));
        assert_eq!(super::load(&p("e.designcraft")).unwrap().settings.chapter_number, 8);
        // An open chapter (e) names its book.
        s.execute("file.open", &json!({"path": p("e.designcraft")})).unwrap();
        assert_eq!(s.execute("layout.chapterNumbering", &json!({})).unwrap()["book"], "Field Guide");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn book_paginates_syncs_and_exports() {
        let dir = std::env::temp_dir().join(format!("dc-book-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = |n: &str| dir.join(n).to_string_lossy().to_string();
        let mut s = Session::new();
        for (name, pages, style) in [("a.designcraft", 3, "Chapter Head"), ("b.designcraft", 2, "Other")] {
            s.execute("file.new", &json!({"pages": pages})).unwrap();
            s.execute("style.paragraph.create", &json!({"name": style, "chars": {"size": 20}})).unwrap();
            s.execute("file.saveAs", &json!({"path": p(name)})).unwrap();
            s.execute("file.close", &json!({})).unwrap();
        }
        s.execute("book.new", &json!({"path": p("book.dcbook")})).unwrap();
        s.execute("book.add", &json!({"path": p("a.designcraft")})).unwrap();
        s.execute("book.add", &json!({"path": p("b.designcraft")})).unwrap();
        let l = s.execute("book.list", &json!({})).unwrap();
        assert_eq!(l["documents"][1]["firstPage"], 4);
        assert_eq!(s.execute("book.paginate", &json!({})).unwrap()["pages"], 5);
        let b = super::load(&p("b.designcraft")).unwrap();
        assert_eq!(b.page_name(0), "4", "continues after the first document");
        assert_eq!(s.execute("book.syncStyles", &json!({})).unwrap()["synced"], 1);
        assert!(super::load(&p("b.designcraft")).unwrap().styles.para("Chapter Head").is_some());
        let r = s.execute("book.exportPdf", &json!({})).unwrap();
        assert_eq!(r["pages"], 5);
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(5));
        // A fresh session reopens the book.
        let mut t = Session::new();
        assert_eq!(t.execute("book.open", &json!({"path": p("book.dcbook")})).unwrap()["documents"], 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
