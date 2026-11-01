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
    /// A manifest reorder invalidates numbering until successful pagination or Book TOC refresh.
    pub numbering_needs_update: bool,
    /// Bounded semantic checkpoint, recomputed from current sources before Book TOC export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toc_state: Option<Value>,
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

/// The same canonical identity policy is used for membership, duplicate detection and open tabs.
#[cfg(not(target_arch = "wasm32"))]
fn identity(path: &str) -> Result<std::path::PathBuf> {
    crate::io::canonicalize(path).map_err(|e| EngineError::Other(format!("{path}: {e}")))
}

/// Membership of the active document in the open Book is independent of other chapters' availability.
/// Inaccessible entries cannot establish membership; full collection still validates every entry.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn active_member(s: &Session) -> Result<usize> {
    let b = s.book.as_ref().ok_or_else(|| bad("toc", "Entire Book requires an open Book containing the active document"))?;
    if b.documents.len() > MAX_BATCH_CHAPTERS {
        return Err(bad("toc", "Entire Book supports at most 4096 chapters"));
    }
    let active = s.doc()?;
    let path = active.path.as_deref().ok_or_else(|| bad("toc", "save the active document and add it to the open Book first"))?;
    let active_identity = identity(path)?;
    let mut seen = std::collections::HashSet::new();
    let mut member = None;
    for (i, stored) in b.documents.iter().enumerate() {
        let Ok(resolved) = chapter(b, stored).and_then(|path| identity(&path)) else { continue };
        if !seen.insert(resolved.clone()) {
            return Err(bad("toc", format!("duplicate Book chapter: {stored}")));
        }
        if resolved == active_identity {
            member = Some(i);
        }
    }
    member.ok_or_else(|| bad("toc", "the active document is not a member of the currently open Book"))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn active_member(_: &Session) -> Result<usize> {
    Err(bad("toc", "Entire Book requires a native file system"))
}

/// Complete traversal is stricter than membership: no missing/refused chapter may be skipped.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn toc_paths(s: &Session) -> Result<(Vec<String>, usize)> {
    let member = active_member(s)?;
    let b = s.book.as_ref().ok_or_else(no_book)?;
    let paths = chapters(b)?;
    for path in &paths {
        identity(path)?;
    }
    Ok((paths, member))
}

/// Read-only, bounded-memory chapter traversal. The active chapter comes from its live model;
/// other open chapters are refused, never read from potentially stale backing files.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn visit_toc(s: &Session, mut visit: impl FnMut(&str, &designcraft_doc::Document) -> Result<()>) -> Result<()> {
    let (paths, member) = toc_paths(s)?;
    let active = s.doc()?;
    // Preflight every open identity before collecting any chapter.
    for (i, path) in paths.iter().enumerate() {
        let chapter_identity = identity(path)?;
        for open in &s.docs {
            if open.uid != active.uid && open.path.as_ref().is_some_and(|p| identity(p).is_ok_and(|p| p == chapter_identity)) {
                return Err(bad("toc", format!("Book chapter {path} is open; save and close other Book chapters before using Entire Book")));
            }
        }
        if i == member && active.interaction.is_some() {
            return Err(bad("toc", "finish the active document interaction before using Entire Book"));
        }
    }
    for (i, path) in paths.iter().enumerate() {
        if i == member {
            visit(path, &active.doc)?;
        } else {
            let mut d = load(path)?;
            let (fonts, _, warnings) = super::file::load_document_fonts(&mut d, path);
            if !warnings.is_empty() {
                return Err(bad("toc", format!("cannot guarantee Book heading layout in {path}: {}", warnings.join("; "))));
            }
            visit(path, &d)?;
            drop(fonts);
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn visit_toc(_: &Session, _: impl FnMut(&str, &designcraft_doc::Document) -> Result<()>) -> Result<()> {
    Err(bad("toc", "Entire Book requires a native file system"))
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
#[path = "book/toc.rs"]
mod toc;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use toc::PendingToc;

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn active_toc(s: &mut Session, params: Option<&Value>) -> Result<Value> {
    toc::active(s, params)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn finish_toc_save(s: &mut Session) -> Result<()> {
    toc::finish_save(s)
}

#[cfg(not(target_arch = "wasm32"))]
struct Numbering {
    next: u32,
    chapter: u32,
}
#[cfg(not(target_arch = "wasm32"))]
impl Numbering {
    fn new() -> Self {
        Self { next: 1, chapter: 0 }
    }
    fn apply(&mut self, d: &mut designcraft_doc::Document) -> Result<()> {
        start_at(d, self.next);
        let pages = u32::try_from(d.page_count()).map_err(|_| bad("book.paginate", "too many pages for continuous numbering"))?;
        self.next = self.next.checked_add(pages).ok_or_else(|| bad("book.paginate", "too many pages for continuous numbering"))?;
        self.chapter = match d.settings.chapter_source {
            designcraft_doc::ChapterSource::Automatic => self.chapter.saturating_add(1),
            designcraft_doc::ChapterSource::UserDefined => d.settings.chapter_number.max(1),
            designcraft_doc::ChapterSource::SameAsPrevious => self.chapter.max(1),
        };
        d.settings.chapter_number = self.chapter;
        Ok(())
    }
}

fn refresh_available(s: &Session) -> std::result::Result<(), String> {
    has_book(s)?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        toc::closed(s).map_err(|e| e.to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("Book TOC refresh requires a native file system".into())
    }
}

fn refresh_toc(s: &mut Session) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        toc::refresh(s)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Err(bad("book.refreshToc", "books need a file system"))
    }
}
fn toc_freshness(s: &Session) -> Result<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(toc::freshness(s))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Err(bad("book.tocFreshness", "books need a file system"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn paginate(s: &Session) -> Result<Value> {
    let b = s.book.as_ref().ok_or_else(no_book)?;
    if b.documents.is_empty() {
        return Ok(json!({"pages": 0, "chapters": []}));
    }
    let mut batch = ChapterBatch::new(b.documents.len(), "book.paginate")?;
    let mut numbering = Numbering::new();
    let mut chapters = Vec::with_capacity(b.documents.len());
    for path in &self::chapters(b)? {
        writable_chapter(s, path, "book.paginate")?;
        let mut d = load(path)?;
        numbering.apply(&mut d)?;
        chapters.push(d.chapter_label());
        batch.stage(path, &d)?;
    }
    batch.commit()?;
    Ok(json!({"pages": numbering.next.saturating_sub(1), "chapters": chapters}))
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
        cmd!(noundo "book.openDocument", "Open Book Document", [], None,
        "{index} — open the chapter at this zero-based Book index, resolving its path relative to the Book file", has_book, |s, p| {
            let index = p.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| bad("book.openDocument", "`index` required"))?;
            let b = s.book.as_ref().ok_or_else(no_book)?;
            let stored = b.documents.get(index).ok_or_else(|| bad("book.openDocument", format!("no document {index}")))?;
            let path = chapter(b, stored)?;
            s.execute("file.open", &json!({"path": path}))
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
        cmd!(noundo "book.move", "Move Book Document", [], None,
        "{from, to} — zero-based indices; to is the final index after removal (0..document count). Saves only the manifest; then Update Numbering and TOC", has_book, |s, p| {
            let index = |key| p.get(key).and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| bad("book.move", format!("`{key}` must be a non-negative index")));
            let from = index("from")?;
            let to = index("to")?;
            let mut b = s.book.as_ref().ok_or_else(no_book)?.clone();
            if from >= b.documents.len() || to >= b.documents.len() {
                return Err(bad("book.move", "source and destination must be existing document indices"));
            }
            if from == to {
                return Ok(json!({"moved": false, "numberingNeedsUpdate": b.numbering_needs_update}));
            }
            let document = b.documents.remove(from);
            b.documents.insert(to, document);
            b.style_source = if b.style_source == from { to }
                else if from < b.style_source && b.style_source <= to { b.style_source.saturating_sub(1) }
                else if to <= b.style_source && b.style_source < from { b.style_source.saturating_add(1) }
                else { b.style_source };
            b.numbering_needs_update = true;
            // Commit the cloned manifest atomically before publishing any changed session state.
            save(&b)?;
            s.book = Some(b);
            Ok(json!({"moved": true, "numberingNeedsUpdate": true}))
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
            Ok(json!({"path": b.path, "styleSource": b.style_source, "numberingNeedsUpdate": b.numbering_needs_update, "documents": docs}))
        }),
        cmd!(noundo "book.paginate", "Update Numbering", [], None, "{} — each document starts numbering where the previous one ended, and takes its chapter number (automatic: one more than the previous document's; same as previous; or its own start) (files are saved) → {pages, chapters}", has_book, |s, _| {
            let result = paginate(s)?;
            let mut b = s.book.as_ref().ok_or_else(no_book)?.clone();
            if b.numbering_needs_update {
                b.numbering_needs_update = false;
                save(&b)?;
                s.book = Some(b);
            }
            Ok(result)
        }),
        cmd!(noundo "book.syncStyles", "Synchronize Book", [], None, "{} — paragraph and character styles and swatches of the style source go into every document (by name)", has_book, |s, _| {
            sync_styles(s)
        }),
        cmd!(noundo "book.refreshToc", "Refresh Table of Contents", [], None,
        "{} — close all Book documents first; stabilize one configured Entire Book TOC and Book numbering in at most four passes, then save", refresh_available, |s, _| refresh_toc(s)),
        cmd!(query "book.tocFreshness", "Book Table of Contents Freshness", [], None,
        "{} → {configured, fresh, reason?}; read-only validation of saved Book TOC sources, numbering and content", has_book, |s, _| toc_freshness(s)),
        cmd!(noundo "book.exportPdf", "Export Book to PDF…", [], None, "{path?} — every document in order as one PDF → {path, bytes, pages} (no path: {base64, …})", has_book, |s, p| {
            #[cfg(not(target_arch = "wasm32"))]
            let validated = toc::export_snapshot(s)?;
            let b = s.book.clone().ok_or_else(no_book)?;
            let mut parts = Vec::new();
            for (index, path) in chapters(&b)?.iter().enumerate() {
                #[cfg(not(target_arch = "wasm32"))]
                let mut d = match &validated { Some(snapshot) => snapshot.document(index)?.clone(), None => load(path)? };
                #[cfg(target_arch = "wasm32")]
                let mut d = { let _ = index; load(path)? };
                // Keep the validated snapshot's font scopes alive; do not reload its sources.
                #[cfg(not(target_arch = "wasm32"))]
                let _fonts = if validated.is_none() { super::file::load_document_fonts(&mut d, path).0 } else { None };
                #[cfg(target_arch = "wasm32")]
                let _fonts = super::file::load_document_fonts(&mut d, path).0;
                let cache = designcraft_compose::Cache::new();
                let r = designcraft_pdf::export_pdf_with_report(&super::interchange::with_pdf_boxes(&d), &cache, &designcraft_pdf::PdfOptions::default()).map_err(|e| EngineError::Other(format!("Book PDF preparation failed for {path}: {e}")))?;
                let n = designcraft_render::pdf_page_count(&r.bytes).unwrap_or(0);
                let sizes: Vec<(f32, f32)> = (0..n).filter_map(|i| designcraft_render::pdf_page_size(&r.bytes, i)).map(|(w, h)| (w as f32, h as f32)).collect();
                parts.push((r.bytes, sizes));
            }
            let pages: usize = parts.iter().map(|p| p.1.len()).sum();
            let bytes = designcraft_pdf::merge_pdfs(&parts, None).map_err(|e| EngineError::Other(e.to_string()))?;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(snapshot) = &validated { snapshot.verify(s)?; }
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

    fn reorder_fixture() -> std::result::Result<(tempfile::TempDir, Session), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("order.dcbook");
        std::fs::write(&path, serde_json::to_vec(&json!({"documents":["A", "B", "C", "D", "E"], "styleSource":1}))?)?;
        let mut s = Session::new();
        s.execute("book.open", &json!({"path":path}))?;
        Ok((dir, s))
    }

    #[test]
    fn book_move_final_index_persists_reload_and_preserves_source() {
        for (from, to, expected) in [
            (0, 4, vec!["B", "C", "D", "E", "A"]),
            (4, 0, vec!["E", "A", "B", "C", "D"]),
            (3, 1, vec!["A", "D", "B", "C", "E"]),
            (1, 3, vec!["A", "C", "D", "B", "E"]),
            (2, 2, vec!["A", "B", "C", "D", "E"]),
        ] {
            let (_dir, mut s) = reorder_fixture().unwrap();
            let path = s.book.as_ref().unwrap().path.clone();
            let before = std::fs::read(&path).unwrap();
            let result = s.execute("book.move", &json!({"from":from, "to":to})).unwrap();
            assert_eq!(result["moved"], from != to);
            let b = s.book.as_ref().unwrap();
            assert_eq!(b.documents, expected);
            assert_eq!(b.documents[b.style_source], "B");
            assert_eq!(b.numbering_needs_update, from != to);
            if from == to {
                assert_eq!(std::fs::read(&path).unwrap(), before);
            }
            s.execute("book.close", &json!({})).unwrap();
            s.execute("book.open", &json!({"path":path})).unwrap();
            let b = s.book.as_ref().unwrap();
            assert_eq!(b.documents, expected);
            assert_eq!(b.documents[b.style_source], "B");
            assert_eq!(b.numbering_needs_update, from != to);
        }
    }

    #[test]
    fn book_move_invalid_indices_and_save_failure_leave_state_unchanged() {
        let (dir, mut s) = reorder_fixture().unwrap();
        let path = s.book.as_ref().unwrap().path.clone();
        let before = std::fs::read(&path).unwrap();
        for params in [
            json!({"from":5,"to":0}),
            json!({"from":0,"to":5}),
            json!({"from":-1,"to":0}),
            json!({"from":0,"to":-1}),
            json!({"from":0}),
            json!({"from":u64::MAX,"to":0}),
        ] {
            assert!(s.execute("book.move", &params).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(s.book.as_ref().unwrap().documents, ["A", "B", "C", "D", "E"]);
        }
        // A directory is an unwritable manifest destination on every supported platform.
        s.book.as_mut().unwrap().path = dir.path().to_string_lossy().into_owned();
        let error = s.execute("book.move", &json!({"from":0,"to":4})).unwrap_err();
        assert!(!error.to_string().is_empty());
        let b = s.book.as_ref().unwrap();
        assert_eq!(b.documents, ["A", "B", "C", "D", "E"]);
        assert_eq!(b.style_source, 1);
        assert!(!b.numbering_needs_update);
        assert_eq!(std::fs::read(&path).unwrap(), before);
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

    #[test]
    fn book_pdf_preparation_error_names_member_and_placed_asset() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let pdf = s.execute("file.exportPdf", &json!({})).unwrap();
        let mut bytes = super::super::file::base64_decode(pdf["base64"].as_str().unwrap());
        bytes[5..8].copy_from_slice(b"2.0");
        s.execute("file.place", &json!({"base64":super::super::file::base64_encode(&bytes),"name":"unsupported-logo.pdf","x":72,"y":72})).unwrap();
        s.execute("file.saveAs", &json!({"path":dir.path().join("bad-member.designcraft")})).unwrap();
        s.execute("file.close", &json!({})).unwrap();
        s.execute("book.new", &json!({"path":dir.path().join("test.dcbook")})).unwrap();
        s.execute("book.add", &json!({"path":dir.path().join("bad-member.designcraft")})).unwrap();
        let output = dir.path().join("must-not-exist.pdf");
        let error = s.execute("book.exportPdf", &json!({"path":output})).unwrap_err().to_string();
        assert!(error.contains("bad-member.designcraft") && error.contains("unsupported-logo.pdf") && error.contains("2.0"), "{error}");
        assert!(!output.exists());
    }
}
