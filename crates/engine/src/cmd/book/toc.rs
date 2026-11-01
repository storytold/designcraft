//! Checked Book TOC snapshots with at most one explicitly permitted live participant. No candidate pass writes a user file.
use super::super::bad;
use super::{Book, ChapterBatch, MAX_BATCH_CHAPTERS, Numbering, chapters, identity, load, no_book, save, writable_chapter};
use crate::{EngineError, Result, Session};
use designcraft_doc::{Document, TocScope};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

const PASSES: usize = 4;
const INPUT_LIMIT: u64 = 128 << 20;
const STATE_LIMIT: usize = 4 << 20;
const REFRESH: &str = "book.refreshToc";
const STALE: &str =
    "The Book Table of Contents is out of date or unverifiable. Save and close all Book documents, then Refresh Table of Contents before exporting.";

pub(super) fn closed(s: &Session) -> Result<()> {
    participants(s, None)
}

fn participants(s: &Session, permitted: Option<u64>) -> Result<()> {
    let b = s.book.as_ref().ok_or_else(no_book)?;
    if b.documents.len() > MAX_BATCH_CHAPTERS {
        return Err(bad(REFRESH, "at most 4096 Book documents are supported"));
    }
    for path in chapters(b)? {
        let id = identity(&path)?;
        for open in &s.docs {
            if open.path.as_ref().is_some_and(|p| identity(p).is_ok_and(|p| p == id)) && Some(open.uid) != permitted {
                return Err(bad(
                    REFRESH,
                    if permitted.is_some() {
                        format!("Close the other Book documents before updating the Entire Book Table of Contents. Open participant: {path}")
                    } else {
                        format!(
                            "Book document {path} is open. Save and close all Book documents, including the TOC document, then Refresh Table of Contents."
                        )
                    },
                ));
            }
        }
    }
    Ok(())
}

fn configured(s: &Session) -> Result<bool> {
    let b = s.book.as_ref().ok_or_else(no_book)?;
    let mut found = false;
    for path in chapters(b)? {
        let d = load(&path)?;
        found |= d.toc.as_ref().is_some_and(|t| t.scope == TocScope::EntireBook);
        // An unsaved scope change in a participant cannot silently bypass export validation.
        let id = identity(&path)?;
        found |= s.docs.iter().any(|st| {
            st.path.as_ref().is_some_and(|p| identity(p).is_ok_and(|p| p == id))
                && st.doc.toc.as_ref().is_some_and(|t| t.scope == TocScope::EntireBook)
        });
    }
    Ok(found)
}

struct Chapter {
    path: String,
    identity: PathBuf,
    bytes: Vec<u8>,
    original: Value,
    doc: Document,
    _fonts: Option<Arc<crate::FontScope>>,
}

struct Live {
    uid: u64,
    revision: u64,
    original: Arc<Document>,
    selection: designcraft_doc::Selection,
}

/// Session-only confirmation; never represents freshness of unsaved disk content.
pub(crate) struct PendingToc {
    uid: u64,
    book_identity: PathBuf,
    state: Value,
}

pub(super) struct Snapshot {
    book: Book,
    book_identity: PathBuf,
    book_bytes: Vec<u8>,
    chapters: Vec<Chapter>,
    by_identity: HashMap<PathBuf, usize>,
    toc_identity: PathBuf,
    live: Option<Live>,
}

impl Snapshot {
    fn load(s: &Session, writing: bool) -> Result<Self> {
        Self::load_with(s, writing, None)
    }
    fn load_with(s: &Session, writing: bool, candidate: Option<(Document, designcraft_doc::Selection)>) -> Result<Self> {
        let live = if let Some((_, selection)) = &candidate {
            let st = s.doc()?;
            let path = st.path.as_ref().ok_or_else(|| bad(REFRESH, "Save this document before creating an Entire Book Table of Contents."))?;
            identity(path).map_err(|_| bad(REFRESH, "Save this document before creating an Entire Book Table of Contents."))?;
            super::active_member(s)?;
            Some(Live { uid: st.uid, revision: st.revision, original: st.doc.clone(), selection: selection.clone() })
        } else {
            None
        };
        participants(s, live.as_ref().map(|l| l.uid))?;
        let live_id =
            if live.is_some() { Some(identity(s.doc()?.path.as_deref().ok_or_else(|| bad(REFRESH, "Save this document first"))?)?) } else { None };
        let book = s.book.as_ref().ok_or_else(no_book)?.clone();
        if book.documents.len() > MAX_BATCH_CHAPTERS {
            return Err(bad(REFRESH, "at most 4096 Book documents are supported"));
        }
        let book_identity = identity(&book.path)?;
        let book_bytes = crate::io::read_file(&book.path).map_err(EngineError::Other)?;
        let disk: Book = serde_json::from_slice(&book_bytes).map_err(|e| bad(REFRESH, e.to_string()))?;
        if value(&disk)? != value(&book)? {
            return Err(bad(REFRESH, "Book manifest changed on disk; reopen the Book and retry"));
        }
        let mut chapters_out = Vec::new();
        let mut by_identity = HashMap::new();
        let mut toc_identity = None;
        let mut total = 0u64;
        for path in chapters(&book)? {
            let id = identity(&path)?;
            if by_identity.insert(id.clone(), chapters_out.len()).is_some() {
                return Err(bad(REFRESH, format!("duplicate Book chapter: {path}")));
            }
            let is_live = live_id.as_ref() == Some(&id);
            if writing && !is_live {
                writable_chapter(s, &path, REFRESH)?;
            }
            let len = crate::io::metadata(&path).map_err(|e| bad(REFRESH, e.to_string()))?.len();
            total = total.checked_add(len).ok_or_else(|| bad(REFRESH, "Book snapshot input is too large"))?;
            if total > INPUT_LIMIT {
                return Err(bad(REFRESH, "Book TOC snapshots support at most 128 MiB of source files"));
            }
            let bytes = crate::io::read_file(&path).map_err(EngineError::Other)?;
            if bytes.len() as u64 != len {
                return Err(bad(REFRESH, "Book source changed during loading; retry"));
            }
            // IDML remains read-only for refresh; it can still be a normal Book PDF input.
            let mut doc = if path.to_lowercase().ends_with(".idml") { load(&path)? } else { super::super::file::from_bytes(&bytes)? };
            let original = value(&doc)?;
            if is_live {
                doc = candidate.as_ref().ok_or_else(|| bad(REFRESH, "live TOC candidate missing"))?.0.clone();
            }
            let (fonts, warnings) = if is_live {
                (s.doc()?.fonts.clone(), Vec::new())
            } else {
                let (fonts, _, warnings) = super::super::file::load_document_fonts(&mut doc, &path);
                (fonts, warnings)
            };
            if !warnings.is_empty() {
                return Err(bad(REFRESH, format!("cannot verify layout in {path}: {}", warnings.join("; "))));
            }
            if doc.toc.as_ref().is_some_and(|t| t.scope == TocScope::EntireBook) && toc_identity.replace(id.clone()).is_some() {
                return Err(bad(REFRESH, "refresh currently supports exactly one Entire Book TOC; multiple Book TOCs are unsupported"));
            }
            chapters_out.push(Chapter { path, identity: id, bytes, original, doc, _fonts: fonts });
        }
        let toc_identity = toc_identity
            .ok_or_else(|| bad(REFRESH, "configure and save an Entire Book TOC in a Book document first; Current Document TOCs are not converted"))?;
        let snapshot = Self { book, book_identity, book_bytes, chapters: chapters_out, by_identity, toc_identity, live };
        snapshot.verify(s)?;
        Ok(snapshot)
    }

    fn toc_index(&self) -> Result<usize> {
        self.by_identity.get(&self.toc_identity).copied().ok_or_else(|| bad(REFRESH, "TOC is not a member of this Book snapshot"))
    }
    pub(super) fn document(&self, index: usize) -> Result<&Document> {
        self.chapters.get(index).map(|c| &c.doc).ok_or_else(|| bad(REFRESH, "Book snapshot index is invalid"))
    }
    fn numbered(&mut self) -> Result<()> {
        let mut numbering = Numbering::new();
        for chapter in &mut self.chapters {
            numbering.apply(&mut chapter.doc)?;
        }
        Ok(())
    }
    fn found(&self) -> Result<Vec<super::super::toc::Found>> {
        let toc = self.document(self.toc_index()?)?.toc.as_ref().ok_or_else(|| bad(REFRESH, "TOC definition missing"))?;
        let mut collector = super::super::toc::BookCollector::new(&toc.entries)?;
        for chapter in &self.chapters {
            collector.visit(&chapter.identity.to_string_lossy(), &chapter.doc)?;
        }
        collector.finish()
    }
    fn signature(&self) -> Result<Value> {
        let d = self.document(self.toc_index()?)?;
        let toc = d.toc.as_ref().ok_or_else(|| bad(REFRESH, "TOC definition missing"))?;
        let story = d.story(toc.story).ok_or_else(|| bad(REFRESH, "TOC story is missing"))?;
        let cs = designcraft_compose::compose_story(d, toc.story, &Default::default());
        let mut normalized_story = value(story)?;
        if let Some(object) = normalized_story.as_object_mut() {
            object.remove("rev");
        }
        let state = json!({"version":1,"chapters":self.chapters.iter().map(|c|json!({
            "identity":c.identity,"pages":c.doc.page_count(),"labels":(0..c.doc.page_count()).map(|i|c.doc.page_name(i)).collect::<Vec<_>>(),
            "sections":c.doc.sections,"chapterLabel":c.doc.chapter_label()
        })).collect::<Vec<_>>(),"headings":self.found()?,"definition":toc,"story":normalized_story,"styles":d.styles,
        "frames":story.frames.iter().map(|id|json!({"item":d.item(*id),"page":d.page_of_item(*id),
            "layout":cs.frame(*id).map(|f|json!({"range":[f.range.start,f.range.end],"height":f.content_height,"lines":f.lines.len()}))})).collect::<Vec<_>>(),
        "overset":cs.overset_at});
        if serde_json::to_vec(&state).map_err(|e| bad(REFRESH, e.to_string()))?.len() > STATE_LIMIT {
            return Err(bad(REFRESH, "Book TOC semantic state exceeds the 4 MiB capacity"));
        }
        Ok(state)
    }
    fn pass(&mut self, prefs: &crate::Prefs) -> Result<(Value, bool)> {
        self.numbered()?;
        let found = self.found()?;
        let used = value(&found)?;
        let index = self.toc_index()?;
        let doc = self.document(index)?.clone();
        let toc = doc.toc.clone().ok_or_else(|| bad(REFRESH, "TOC definition missing"))?;
        let mut worker = Session::new();
        worker.prefs = prefs.clone();
        worker.add_document(crate::DocState::new(doc, None));
        let before = worker.active().map(|st| (st.uid, st.doc.clone(), st.selection.clone()));
        worker.edit(|d, _| {
            super::super::toc::fill(d, &toc, None, Some(found))?;
            Ok(())
        })?;
        let spec = crate::find_command("toc.update").ok_or_else(|| bad(REFRESH, "TOC layout command is unavailable"))?;
        worker.layout_after_edit(spec, before.as_ref());
        let result = (*worker.doc()?.doc).clone();
        let composed = designcraft_compose::compose_story(&result, toc.story, &Default::default());
        if composed.overset_at.is_some() {
            return Err(bad(
                REFRESH,
                "Book TOC does not fit its frames. Configure primary-story reflow or allocate more fixed space, then retry Refresh Table of Contents.",
            ));
        }
        self.chapters.get_mut(index).ok_or_else(|| bad(REFRESH, "TOC is not in the Book"))?.doc = result;
        self.numbered()?;
        let correspond = used == value(&self.found()?)?;
        Ok((self.signature()?, correspond))
    }
    pub(super) fn verify(&self, s: &Session) -> Result<()> {
        participants(s, self.live.as_ref().map(|l| l.uid))?;
        if let Some(live) = &self.live {
            let st = s.doc()?;
            if st.uid != live.uid
                || st.revision != live.revision
                || !Arc::ptr_eq(&st.doc, &live.original)
                || st.path.as_ref().is_none_or(|p| !identity(p).is_ok_and(|id| id == self.toc_identity))
            {
                return Err(bad(REFRESH, "Active TOC document changed during stabilization; retry"));
            }
        }
        if identity(&self.book.path)? != self.book_identity || crate::io::read_file(&self.book.path).map_err(EngineError::Other)? != self.book_bytes {
            return Err(bad(REFRESH, "Book manifest changed during processing; reopen the Book and retry"));
        }
        for chapter in &self.chapters {
            if identity(&chapter.path)? != chapter.identity || crate::io::read_file(&chapter.path).map_err(EngineError::Other)? != chapter.bytes {
                return Err(bad(REFRESH, format!("Book source {} changed during processing; retry without overwriting it", chapter.path)));
            }
        }
        Ok(())
    }
    fn check_fresh(&mut self, s: &Session) -> Result<()> {
        if self.book.numbering_needs_update {
            return Err(bad(REFRESH, "Book order changed; refresh the Book TOC"));
        }
        let current = self.signature()?;
        if self.book.toc_state.as_ref() != Some(&current) {
            return Err(bad(REFRESH, "Book sources, TOC definition or generated content changed, or this TOC has not been refreshed"));
        }
        let originals = self.chapters.iter().map(|c| c.doc.clone()).collect::<Vec<_>>();
        let checked = self.pass(&s.prefs);
        for (chapter, doc) in self.chapters.iter_mut().zip(originals) {
            chapter.doc = doc;
        }
        let (expected, correspond) = checked?;
        if !correspond || current != expected {
            return Err(bad(REFRESH, "stored Book TOC no longer matches final layout and labels"));
        }
        self.verify(s)
    }
}

fn value(v: &impl Serialize) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| bad(REFRESH, e.to_string()))
}

/// A finite signature budget; equality alone is insufficient without final-source correspondence.
struct Confirmation {
    previous: Option<Value>,
    passes: usize,
}
impl Confirmation {
    fn observe(&mut self, state: Value, correspond: bool) -> Result<bool> {
        self.passes += 1;
        let equal = self.previous.as_ref() == Some(&state);
        self.previous = Some(state);
        if equal && correspond {
            return Ok(true);
        }
        if self.passes >= PASSES {
            return Err(bad(
                REFRESH,
                "Book TOC did not stabilize within four passes. Reserve more TOC space or simplify its layout, then retry; no chapter files were changed.",
            ));
        }
        Ok(false)
    }
}

struct Prepared {
    snapshot: Snapshot,
    state: Value,
    history: Vec<Value>,
}
impl Prepared {
    fn new(s: &Session) -> Result<Self> {
        Self::from_snapshot(s, Snapshot::load(s, true)?)
    }
    fn from_snapshot(s: &Session, mut snapshot: Snapshot) -> Result<Self> {
        let mut confirmation = Confirmation { previous: None, passes: 0 };
        let mut history = Vec::new();
        for pass in 1..=PASSES {
            let (state, correspond) = snapshot.pass(&s.prefs)?;
            let same = confirmation.previous.as_ref() == Some(&state);
            history.push(
                json!({"pass":pass,"tocPages":snapshot.document(snapshot.toc_index()?)?.page_count(),"labelsAgree":correspond,"stateUnchanged":same}),
            );
            if confirmation.observe(state.clone(), correspond)? {
                return Ok(Self { snapshot, state, history });
            }
        }
        Err(bad(REFRESH, "Book TOC confirmation budget exhausted"))
    }
    fn commit(self, s: &mut Session) -> Result<Value> {
        let snapshot = self.snapshot;
        let mut batch = ChapterBatch::new(snapshot.chapters.len(), REFRESH)?;
        let mut affected = Vec::new();
        for chapter in &snapshot.chapters {
            if snapshot.live.is_some() && chapter.identity == snapshot.toc_identity {
                continue;
            }
            writable_chapter(s, &chapter.path, REFRESH)?;
            if value(&chapter.doc)? != chapter.original {
                batch.stage(&chapter.path, &chapter.doc)?;
                affected.push(chapter.path.clone());
            }
        }
        // Final preflight happens after serialization/staging, before any Book/chapter mutation.
        snapshot.verify(s)?;
        let mut book = snapshot.book.clone();
        book.toc_state = None;
        save(&book)?;
        s.book = Some(book.clone());
        let invalidated = crate::io::read_file(&book.path).map_err(EngineError::Other)?;
        batch.commit_with(|path, staged| {
            let chapter = snapshot
                .chapters
                .iter()
                .find(|c| std::path::Path::new(&c.path) == path)
                .ok_or_else(|| std::io::Error::other("chapter missing from verified snapshot"))?;
            if identity(&chapter.path).map_err(|e| std::io::Error::other(e.to_string()))? != chapter.identity
                || crate::io::read_file(path).map_err(std::io::Error::other)? != chapter.bytes
            {
                return Err(std::io::Error::other("Book source changed during commit; retry"));
            }
            crate::io::replace_with(path, staged)
        })?;
        let finalized = (|| -> Result<()> {
            // Recheck unchanged inputs and the saved candidate models before recording freshness.
            for chapter in &snapshot.chapters {
                if identity(&chapter.path)? != chapter.identity {
                    return Err(bad(REFRESH, "Book source identity changed during final commit; freshness was not recorded"));
                }
                let current = crate::io::read_file(&chapter.path).map_err(EngineError::Other)?;
                let unchanged = (snapshot.live.is_some() && chapter.identity == snapshot.toc_identity) || chapter.original == value(&chapter.doc)?;
                let matches =
                    if unchanged { current == chapter.bytes } else { value(&super::super::file::from_bytes(&current)?)? == value(&chapter.doc)? };
                if !matches {
                    return Err(bad(REFRESH, "Book source changed during final commit; freshness was not recorded. Retry."));
                }
            }
            if identity(&book.path)? != snapshot.book_identity || crate::io::read_file(&book.path).map_err(EngineError::Other)? != invalidated {
                return Err(bad(
                    REFRESH,
                    "Book manifest changed during final commit; chapters were saved but freshness was not recorded. Reopen the Book and retry.",
                ));
            }
            book.numbering_needs_update = snapshot.live.is_some();
            book.toc_state = if snapshot.live.is_none() { Some(self.state.clone()) } else { None };
            save(&book)?;
            Ok(())
        })();
        finalized.map_err(|e| {
            EngineError::Other(format!("{REFRESH}: finalization failed: {e}; completed chapter paths: {affected:?}; freshness was not recorded"))
        })?;
        s.book = Some(book);
        let live_result = snapshot.live.is_some();
        if let Some(live) = &snapshot.live {
            let document = snapshot.document(snapshot.toc_index()?)?.clone();
            s.edit(|d, selection| {
                *d = document;
                *selection = live.selection.clone();
                Ok(())
            })?;
            s.pending_book_toc = Some(PendingToc { uid: live.uid, book_identity: snapshot.book_identity.clone(), state: self.state });
        }
        let toc = if live_result { s.doc()?.doc.toc.as_ref() } else { snapshot.document(snapshot.toc_index()?)?.toc.as_ref() }
            .ok_or_else(|| bad(REFRESH, "TOC definition missing"))?;
        Ok(
            json!({"story":toc.story.0,"entries":snapshot.found()?.len(),"passes":self.history.len(),"fresh":!live_result,"pendingSave":live_result,"history":self.history}),
        )
    }
}

pub(super) fn active(s: &mut Session, params: Option<&Value>) -> Result<Value> {
    let st = s.doc()?;
    if st.path.as_ref().is_none_or(|p| identity(p).is_err()) {
        return Err(bad(REFRESH, "Save this document before creating an Entire Book Table of Contents."));
    }
    let mut worker = Session::new();
    worker.add_document(crate::DocState::new((*st.doc).clone(), None));
    worker.doc_mut()?.selection = st.selection.clone();
    if let Some(params) = params {
        super::super::toc::configure_book(&mut worker, params)?;
    }
    let candidate = ((*worker.doc()?.doc).clone(), worker.doc()?.selection.clone());
    let snapshot = Snapshot::load_with(s, true, Some(candidate))?;
    if snapshot.live.is_none()
        || identity(s.doc()?.path.as_deref().ok_or_else(|| bad(REFRESH, "Save this document first"))?)? != snapshot.toc_identity
    {
        return Err(bad(REFRESH, "The active document must contain the Entire Book Table of Contents"));
    }
    Prepared::from_snapshot(s, snapshot)?.commit(s)
}

pub(super) fn finish_save(s: &mut Session) -> Result<()> {
    let Some(pending) = &s.pending_book_toc else { return Ok(()) };
    if s.active().is_none_or(|st| st.uid != pending.uid || st.is_dirty()) {
        return Ok(());
    }
    let Some(book) = &s.book else { return Ok(()) };
    if identity(&book.path).map_err(saved_toc_error)? != pending.book_identity {
        return Ok(());
    }
    let expected = pending.state.clone();
    let st = s.doc().map_err(saved_toc_error)?;
    let Ok(snapshot) = Snapshot::load_with(s, false, Some(((*st.doc).clone(), st.selection.clone()))) else { return Ok(()) };
    // Undo, later edits or changed sources must not publish the old pending confirmation.
    let active_chapter = snapshot
        .chapters
        .get(snapshot.toc_index().map_err(saved_toc_error)?)
        .ok_or_else(|| saved_toc_error(bad("file.save", "TOC snapshot missing")))?;
    if active_chapter.original != value(&active_chapter.doc).map_err(saved_toc_error)? {
        return Err(saved_toc_error(bad("file.save", "Saved TOC file changed before Book freshness could be recorded; retry.")));
    }
    if snapshot.signature().map_err(saved_toc_error)? != expected {
        return Ok(());
    }
    publish_saved_toc(s, snapshot, expected)
}

fn saved_toc_error(error: EngineError) -> EngineError {
    bad("file.save", format!("Document saved, but Book TOC freshness was not recorded: {error}"))
}

/// Recheck the saved snapshot immediately before publishing its freshness checkpoint.
fn publish_saved_toc(s: &mut Session, snapshot: Snapshot, expected: Value) -> Result<()> {
    snapshot.verify(s).map_err(saved_toc_error)?;
    let mut book = snapshot.book;
    book.numbering_needs_update = false;
    book.toc_state = Some(expected);
    save(&book).map_err(saved_toc_error)?;
    s.book = Some(book);
    Ok(())
}

pub(super) fn refresh(s: &mut Session) -> Result<Value> {
    Prepared::new(s)?.commit(s)
}

pub(super) fn freshness(s: &Session) -> Value {
    match configured(s) {
        Ok(false) => json!({"configured":false,"fresh":true}),
        Err(e) => json!({"configured":true,"fresh":false,"reason":e.to_string()}),
        Ok(true) => match Snapshot::load(s, false).and_then(|mut snapshot| snapshot.check_fresh(s)) {
            Ok(()) => json!({"configured":true,"fresh":true}),
            Err(e) => json!({"configured":true,"fresh":false,"reason":e.to_string()}),
        },
    }
}
pub(super) fn export_snapshot(s: &Session) -> Result<Option<Snapshot>> {
    if !configured(s).map_err(|e| bad("book.exportPdf", format!("{STALE} {e}")))? {
        return Ok(None);
    }
    let mut snapshot = Snapshot::load(s, false).map_err(|e| bad("book.exportPdf", format!("{STALE} {e}")))?;
    snapshot.check_fresh(s).map_err(|e| bad("book.exportPdf", format!("{STALE} {e}")))?;
    Ok(Some(snapshot))
}

#[cfg(test)]
#[path = "toc_tests.rs"]
mod tests;
