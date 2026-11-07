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

/// Chapters whose file is not under the book's own folder, by the path the book names.
///
/// A book names its chapters by path, and `book.paginate` and `book.syncStyles` write numbering
/// and styles into each of them. A book that arrived from somewhere else can therefore name
/// `~/Documents/x.designcraft` and have the user's own file rewritten by opening it and running
/// an update. Which files those are is reported, so the UI can ask before writing (#1597);
/// refusing them outright would break books whose chapters legitimately live in sibling folders,
/// which is how InDesign books are usually arranged.
#[cfg(not(target_arch = "wasm32"))]
fn outside_book_folder(b: &Book) -> Vec<String> {
    // The folder comes from the chapters' own resolution, so the two cannot disagree about what
    // a path means. `book.new` stores the path it was given, so a relative book path left the
    // folder relative while every chapter resolves absolute: `dir/Guide.dcbook` reported the
    // chapters beside the book as outside it, and a bare `Guide.dcbook` gave an empty folder that
    // every absolute path starts with, so nothing was reported at all. `std::path::absolute`
    // fixed those but keeps `..` where `craft_docpath` folds it, so a book opened as
    // `sub/../Guide.dcbook` had a folder no chapter beside it could start with (both noted in
    // review). Resolving a name inside the folder and taking its parent uses one rule for both.
    let Some(folder) = chapter(b, "-").ok().and_then(|p| std::path::Path::new(&p).parent().map(std::path::Path::to_path_buf)) else {
        return b.documents.clone();
    };
    let folder = real_location(&folder);
    b.documents
        .iter()
        .filter(|named| chapter(b, named).map(|resolved| !real_location(std::path::Path::new(&resolved)).starts_with(&folder)).unwrap_or(true))
        .cloned()
        .collect()
}

/// Where a write to `path` lands, with links resolved: the file's own if it exists, else its
/// folder's with the name joined, else `path` as written.
///
/// Names alone are not where an update writes. A chapter named `chapters/x.designcraft` inside the
/// book's folder, where `chapters` is a link to `~/Documents`, is the user's own file, and so is
/// a chapter file that is itself a link out; both read as inside by name (noted in review). The
/// book's folder is resolved the same way, since it can sit behind a link too (`/tmp` is one on
/// macOS).
#[cfg(not(target_arch = "wasm32"))]
fn real_location(path: &std::path::Path) -> std::path::PathBuf {
    crate::io::canonicalize(path)
        .or_else(|_| match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => crate::io::canonicalize(dir).map(|d| d.join(name)),
            _ => Err(std::io::Error::other("no folder to resolve")),
        })
        .unwrap_or_else(|_| path.to_path_buf())
}

/// The chapters outside the book's folder that `command` writes: every one for Update Numbering,
/// all but the style source for Synchronize, which only reads that one.
#[cfg(not(target_arch = "wasm32"))]
fn written_outside(b: &Book, command: &str) -> Vec<String> {
    let source = (command == "book.syncStyles").then(|| b.documents.get(b.style_source)).flatten();
    outside_book_folder(b).into_iter().filter(|named| Some(named) != source).collect()
}

/// Refuse an update that would rewrite chapters outside the book's folder unless the caller said
/// it may (`allowOutside`), before anything is written (#1597).
///
/// Reporting them afterwards, which is what an update did first, told the caller what had already
/// been rewritten and left nothing to ask. Refusing by default protects a script or an MCP client
/// as well as the UI, which asks with `book.outsideFolder` and passes the answer here.
#[cfg(not(target_arch = "wasm32"))]
fn allowed_outside(command: &str, outside: &[String], p: &Value) -> Result<()> {
    if outside.is_empty() || p.get("allowOutside").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    Err(bad(
        command,
        format!(
            "{} outside the book's folder would be rewritten: {}; pass {{\"allowOutside\": true}} to update {}",
            if outside.len() == 1 { "a chapter" } else { "chapters" },
            outside.join(", "),
            if outside.len() == 1 { "it" } else { "them" }
        ),
    ))
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

/// The figure [`MAX_BOOK_CHAPTER_BYTES`] is derived from, not a limit the export is held to.
///
/// Nothing enforces this number. What is enforced is the retained-chapter budget below; this is
/// only the peak that budget is a third of. A chapter is also generated in full before it is
/// weighed, so a single chapter larger than this is built and only then refused -- the peak is
/// not bounded before generation (noted in review).
const MAX_BOOK_PDF_BYTES: usize = 512 << 20;

/// What the retained chapters may add up to: a third of [`MAX_BOOK_PDF_BYTES`], because three
/// things are live at the end of the export -- the chapters held to be merged, the merged output,
/// and the copy `merge_pdfs` makes of the chapter it is reading. The first check counted only the
/// chapters, so the export could reach three times the figure the refusal named (noted in
/// review).
///
/// A third assumes the merged output is no larger than its input. That holds where chapters share
/// fonts and images, and one measurement put it at 0.97 of the input -- but that was one document,
/// and chapters that share nothing have no reason to dedupe at all, so the assumption is not
/// established in general. This is a retained-chapter byte limit, not a guaranteed peak (noted in
/// review).
const MAX_BOOK_CHAPTER_BYTES: usize = MAX_BOOK_PDF_BYTES / 3;

/// The two figures have to move together: if the chapters' budget is ever raised without the peak
/// it is a third of, the export silently goes back to reaching three times what it allows, and
/// the refusal names a number the check no longer enforces.
///
/// Checked here rather than in a test: `assertions_on_constants` refuses these in a test body
/// (the gate runs clippy with `-D warnings`), and a `const` block evaluates them in every build
/// instead of only under `cargo test` (noted in review).
const _: () = {
    assert!(MAX_BOOK_CHAPTER_BYTES * 3 <= MAX_BOOK_PDF_BYTES, "three times the chapter budget is within the peak");
    assert!((MAX_BOOK_CHAPTER_BYTES + 1) * 3 > MAX_BOOK_PDF_BYTES, "and it is as much as a third allows, not less");
    assert!(MAX_BOOK_CHAPTER_BYTES / (1 << 20) == 170, "and the refusal names 170 MB");
};

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
fn paginate(s: &Session, p: &Value) -> Result<Value> {
    let b = s.book.as_ref().ok_or_else(no_book)?;
    let outside = written_outside(b, "book.paginate");
    allowed_outside("book.paginate", &outside, p)?;
    if b.documents.is_empty() {
        // The same shape as a book with chapters, so a caller reads one field either way
        // (noted in review).
        return Ok(json!({"pages": 0, "chapters": [], "outsideFolder": []}));
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
    Ok(json!({"pages": next - 1, "chapters": chapters, "outsideFolder": outside}))
}

#[cfg(not(target_arch = "wasm32"))]
fn sync_styles(s: &Session, p: &Value) -> Result<Value> {
    let b = s.book.as_ref().ok_or_else(no_book)?;
    let outside = written_outside(b, "book.syncStyles");
    allowed_outside("book.syncStyles", &outside, p)?;
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
    Ok(json!({"synced": n, "outsideFolder": outside}))
}

#[cfg(target_arch = "wasm32")]
fn paginate(_: &Session, _: &Value) -> Result<Value> {
    Err(EngineError::Other("books need a file system".into()))
}

#[cfg(target_arch = "wasm32")]
fn sync_styles(_: &Session, _: &Value) -> Result<Value> {
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
                layout: None,
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
        cmd!(noundo "book.paginate", "Update Numbering", [], None, "{allowOutside?: bool} — each document starts numbering where the previous one ended, and takes its chapter number (automatic: one more than the previous document's; same as previous; or its own start) (files are saved; chapters outside the book's folder only with allowOutside, see book.outsideFolder) → {pages, chapters, outsideFolder}", has_book, |s, p| {
            paginate(s, p)
        }),
        cmd!(noundo "book.syncStyles", "Synchronize Book", [], None, "{allowOutside?: bool} — paragraph and character styles and swatches of the style source go into every document (by name) (chapters outside the book's folder only with allowOutside, see book.outsideFolder) → {synced, outsideFolder}", has_book, |s, p| {
            sync_styles(s, p)
        }),
        cmd!(noundo "book.outsideFolder", "Chapters Outside the Book's Folder", [], None, "{command?: \"book.paginate\"|\"book.syncStyles\"} — the chapters that update would rewrite outside the book's own folder, links resolved, to ask about first (default book.paginate, which writes them all) → {chapters}", has_book, |s, p| {
            let b = s.book.as_ref().ok_or_else(no_book)?;
            let command = p.get("command").and_then(Value::as_str).unwrap_or("book.paginate");
            if !matches!(command, "book.paginate" | "book.syncStyles") {
                return Err(bad("book.outsideFolder", format!("`command` is book.paginate or book.syncStyles, not {command}")));
            }
            #[cfg(not(target_arch = "wasm32"))]
            let chapters = written_outside(b, command);
            #[cfg(target_arch = "wasm32")]
            let chapters: Vec<String> = { let _ = b; Vec::new() };
            Ok(json!({"chapters": chapters}))
        }),
        cmd!(noundo "book.exportPdf", "Export Book to PDF…", [], None, "{path?} — every document in order as one PDF → {path, bytes, pages} (no path: {base64, …})", has_book, |s, p| {
            let b = s.book.clone().ok_or_else(no_book)?;
            // The same cap the staged updates use, which this path never applied, plus a budget
            // for what it holds: every chapter's whole PDF is kept in memory to be merged, so a
            // long book of heavy chapters was unbounded (#1597).
            #[cfg(not(target_arch = "wasm32"))]
            if b.documents.len() > MAX_BATCH_CHAPTERS {
                return Err(bad("book.exportPdf", format!("a book export supports at most {MAX_BATCH_CHAPTERS} chapters")));
            }
            let mut held = 0usize;
            let mut parts = Vec::new();
            for path in &chapters(&b)? {
                let mut d = load(path)?;
                // The chapter's own fonts, for this export (skipped font files are logged).
                let _fonts = super::file::load_document_fonts(&mut d, path).0;
                let cache = designcraft_compose::Cache::new();
                let r = designcraft_pdf::export_pdf_with_report(&super::interchange::with_pdf_boxes(&d), &cache, &designcraft_pdf::PdfOptions::default()).map_err(|e| EngineError::Other(e.to_string()))?;
                // Every page's size from one opening of the chapter's PDF (not one per page).
                let sizes: Vec<(f32, f32)> =
                    designcraft_render::pdf_page_sizes(&r.bytes).unwrap_or_default().into_iter().map(|(w, h)| (w as f32, h as f32)).collect();
                held = held.saturating_add(r.bytes.len());
                // Checked as soon as the chapter is exported. A chapter over the budget on its
                // own is still generated once before it is refused: the exporter answers with one
                // buffer, so there is no point before this one at which its size is known.
                if held > MAX_BOOK_CHAPTER_BYTES {
                    return Err(bad(
                        "book.exportPdf",
                        format!("the book's chapters are more than {} MB of PDF: export them in parts", MAX_BOOK_CHAPTER_BYTES / (1 << 20)),
                    ));
                }
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

    /// A book names its chapters by path, and an update writes numbering and styles into each
    /// one, so a book from elsewhere can name a file of the user's own. An update now refuses,
    /// before writing anything, while a chapter lies outside the book's folder, unless the caller
    /// says it may; `book.outsideFolder` names them so the UI can ask first (#1597). Reporting
    /// them after the write, which is what this did first, left nothing to ask (noted in review).
    #[test]
    fn an_update_asks_before_it_rewrites_a_chapter_outside_the_books_folder() {
        let root = std::env::temp_dir().join(format!("dc-book-outside-{}", std::process::id()));
        let (inside, elsewhere) = (root.join("book"), root.join("elsewhere"));
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mut s = Session::new();
        s.execute("book.new", &json!({"path": inside.join("Guide.dcbook").to_string_lossy()})).unwrap();
        for path in [inside.join("ch1.designcraft"), elsewhere.join("mine.designcraft")] {
            s.execute("file.new", &json!({})).unwrap();
            s.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
            s.execute("book.add", &json!({"path": path.to_string_lossy()})).unwrap();
            s.execute("file.close", &json!({})).unwrap();
        }

        // What the UI asks about: the one chapter elsewhere.
        let asked = s.execute("book.outsideFolder", &json!({})).unwrap();
        let outside = asked["chapters"].as_array().expect("the chapters are listed").clone();
        assert_eq!(outside.len(), 1, "one chapter is outside the book's folder: {outside:?}");
        assert!(outside[0].as_str().unwrap_or_default().contains("mine.designcraft"), "{outside:?}");

        // Without consent neither update writes anything, the chapter beside the book included,
        // and the refusal names the chapter it is about.
        let mine = elsewhere.join("mine.designcraft");
        let before = (std::fs::read(inside.join("ch1.designcraft")).unwrap(), std::fs::read(&mine).unwrap());
        for command in ["book.paginate", "book.syncStyles"] {
            let e = s.execute(command, &json!({})).unwrap_err().to_string();
            assert!(e.contains("mine.designcraft") && e.contains("allowOutside"), "{command}: {e}");
            for no in [json!({"allowOutside": false}), json!({"allowOutside": "yes"})] {
                assert!(s.execute(command, &no).is_err(), "{command} with {no}: only `true` is consent");
            }
        }
        let after = (std::fs::read(inside.join("ch1.designcraft")).unwrap(), std::fs::read(&mine).unwrap());
        assert!(before == after, "a refused update rewrote a chapter");

        // With consent both run, and say which chapters elsewhere they wrote.
        let r = s.execute("book.paginate", &json!({"allowOutside": true})).unwrap();
        assert_eq!(r["outsideFolder"], json!(outside));
        assert_ne!(std::fs::read(&mine).unwrap(), before.1, "the consented update wrote the chapter");
        let r = s.execute("book.syncStyles", &json!({"allowOutside": true})).unwrap();
        assert_eq!(r["outsideFolder"].as_array().map(Vec::len), Some(1));

        // A book whose chapters are all beside it reports nothing.
        let mut s = Session::new();
        s.execute("book.new", &json!({"path": inside.join("Tidy.dcbook").to_string_lossy()})).unwrap();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.saveAs", &json!({"path": inside.join("only.designcraft").to_string_lossy()})).unwrap();
        s.execute("book.add", &json!({"path": inside.join("only.designcraft").to_string_lossy()})).unwrap();
        s.execute("file.close", &json!({})).unwrap();
        let r = s.execute("book.paginate", &json!({})).unwrap();
        assert_eq!(r["outsideFolder"], json!([]), "nothing to ask about");

        // An empty book answers in the same shape, rather than leaving the field out.
        let mut s = Session::new();
        s.execute("book.new", &json!({"path": inside.join("Empty.dcbook").to_string_lossy()})).unwrap();
        let r = s.execute("book.paginate", &json!({})).unwrap();
        assert_eq!(r["outsideFolder"], json!([]), "an empty book answers in the same shape");
        let _ = std::fs::remove_dir_all(&root);
    }

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
