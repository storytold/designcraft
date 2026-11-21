//! Background exports: a long export runs on its own thread and reports progress, so the
//! interface stays responsive and can show a progress bar.
//!
//! `file.exportPdf {background: true, path}` queues one and returns its id at once; the job writes
//! the file itself, atomically, as a synchronous export does. `file.export.progress` reports what
//! the job is doing and, once it has finished, what it wrote (or the error that stopped it).
//!
//! The job is a snapshot of the document and its composition cache, so it never waits on the
//! session. The session freezes the document while the job runs (`EXPORTING`): the export writes
//! what the pages showed when it started, so changing them meanwhile would make the file and the
//! screen disagree. Closing another document, saving, activating and exporting again stay allowed.
//! A job is only queued from a document that is settled: no data-merge preview and no open tool
//! gesture, whose transient content would otherwise be what the file holds.
//! On the web, which has no threads, the job runs to completion before the command returns and the
//! progress command reports the finished job.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// The most finished jobs remembered; a result is dropped only after a caller has read it.
const MAX_JOBS: usize = 8;

/// What a job is doing, as `file.export.progress` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Progress {
    /// A short phase name: `preparing`, `flattening`, `rasterizing`, `pages`, `finishing`.
    pub phase: String,
    /// Pages written so far, and how many there are, during `pages`.
    pub done: usize,
    pub total: usize,
    /// The sheet being written, as the document names it (`1`, `1–2`), during `pages`.
    pub page: String,
}

/// A finished job's result.
#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    pub path: String,
    pub bytes: usize,
    pub pages: usize,
    pub warnings: Vec<String>,
    /// The job was cancelled before it wrote its file.
    pub cancelled: bool,
}

/// A job's state, written by the exporting thread and read by `file.export.progress`.
struct Shared {
    /// The `DocState.uid` that was exported, so the session can freeze it while the job runs.
    doc_uid: u64,
    /// Set by `file.export.cancel`; the worker checks it between pages.
    cancel: AtomicBool,
    /// Set when the worker starts writing the file: from then on no cancel is accepted, so a
    /// `cancelRequested: true` reply always means no file will be written.
    writing: AtomicBool,
    /// Set once a reporting command has returned the job's result, so an outcome nobody has read
    /// yet is not evicted by [`MAX_JOBS`].
    observed: AtomicBool,
    progress: Mutex<Progress>,
    outcome: Mutex<Option<std::result::Result<Outcome, String>>>,
}

impl Shared {
    fn new(doc_uid: u64) -> Self {
        Self {
            doc_uid,
            cancel: AtomicBool::new(false),
            writing: AtomicBool::new(false),
            observed: AtomicBool::new(false),
            progress: Mutex::new(Progress { phase: "preparing".into(), done: 0, total: 0, page: String::new() }),
            outcome: Mutex::new(None),
        }
    }

    fn set(&self, phase: &str, done: usize, total: usize, page: &str) {
        let mut p = self.progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *p = Progress { phase: phase.into(), done, total, page: page.into() };
    }

    fn finish(&self, outcome: std::result::Result<Outcome, String>) {
        let mut slot = self.outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(outcome);
    }

    fn progress(&self) -> Progress {
        self.progress.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn outcome(&self) -> Option<std::result::Result<Outcome, String>> {
        self.outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// From the moment the file is written, a cancel cannot prevent it.
    fn start_writing(&self) {
        self.writing.store(true, Ordering::Relaxed);
    }

    fn writing(&self) -> bool {
        self.writing.load(Ordering::Relaxed)
    }

    /// Note that a caller has been given this job's result.
    fn observe(&self) {
        self.observed.store(true, Ordering::Relaxed);
    }

    fn observed(&self) -> bool {
        self.observed.load(Ordering::Relaxed)
    }
}

/// The session's background export jobs.
#[derive(Default)]
pub(crate) struct Jobs {
    next: u64,
    latest: Option<u64>,
    jobs: BTreeMap<u64, Arc<Shared>>,
}

impl Jobs {
    /// Remember `shared` under a new id and return it.
    fn insert(&mut self, shared: Arc<Shared>) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        self.jobs.insert(id, shared);
        self.latest = Some(id);
        // A finished job keeps its result until a reporting command has returned it: a poll that
        // arrives after many exports still finds the file it wrote. Only read results age out, and
        // a running job is never dropped. An unread result is small once its worker is done (no
        // document, no cache), so the history can pass `MAX_JOBS` while one waits to be read.
        while self.jobs.len() > MAX_JOBS {
            let read = self.jobs.iter().find(|(_, s)| s.outcome().is_some() && s.observed()).map(|(id, _)| *id);
            let Some(oldest) = read else { break };
            self.jobs.remove(&oldest);
            if self.latest == Some(oldest) {
                self.latest = self.jobs.keys().next_back().copied();
            }
        }
        id
    }

    pub(crate) fn latest(&self) -> Option<u64> {
        self.latest
    }

    fn get(&self, id: u64) -> Option<&Arc<Shared>> {
        self.jobs.get(&id)
    }

    /// Is a background export of document `uid` still running?
    pub(crate) fn is_running(&self, uid: u64) -> bool {
        self.jobs.values().any(|s| s.doc_uid == uid && s.outcome().is_none())
    }

    /// Ask job `id` to stop: `Some(true)` when it was running and no file had been started,
    /// `Some(false)` when it had already finished or the write had begun, `None` when there is no
    /// such job.
    pub(crate) fn cancel(&self, id: u64) -> Option<bool> {
        let shared = self.jobs.get(&id)?;
        if shared.outcome().is_some() || shared.writing() {
            return Some(false);
        }
        shared.cancel.store(true, Ordering::Relaxed);
        Some(true)
    }

    /// The ids of every remembered job, oldest first, for `file.export.jobs`.
    pub(crate) fn ids(&self) -> Vec<u64> {
        self.jobs.keys().copied().collect()
    }
}

/// One job's input: a snapshot of what it exports.
struct Job {
    doc: Arc<designcraft_doc::Document>,
    cache: Arc<designcraft_compose::Cache>,
    /// The document's fonts, kept alive for the whole export: dropping the scope closes it in the
    /// shared font database, and a story composed after that could fall back to a different font
    /// and a different layout. Closing the document does not close the scope while this holds it.
    _fonts: Option<Arc<crate::FontScope>>,
    /// `file.exportPdf` parameters without `path` and `background`.
    params: Value,
    path: String,
}

/// Queue the PDF export `p` describes and return its job id.
pub(crate) fn start_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.exportPdf";
    let path =
        p.get("path").and_then(Value::as_str).map(str::to_string).ok_or_else(|| crate::cmd::bad(ID, "`background` needs a `path` to write"))?;
    let st = s.doc()?;
    // A data-merge preview replaces the document with a filled record; exporting it would write
    // the preview, not the template the user sees when the preview ends.
    if st.preview_stash.is_some() {
        return Err(crate::cmd::bad(ID, "stop the data preview before exporting"));
    }
    // A tool interaction holds the document with its gesture's preview applied: exporting it would
    // write content the gesture can still cancel, and the snapshot would keep it.
    if st.interaction.is_some() {
        return Err(crate::cmd::bad(ID, "finish the gesture before exporting"));
    }
    let doc = st.doc.clone();
    let fonts = st.fonts.clone();
    let cache = s.cache.clone();
    let mut params = p.clone();
    if let Some(o) = params.as_object_mut() {
        o.remove("path");
        o.remove("background");
    }
    // The worker is another thread, so the caller's file-access confinement does not follow it:
    // an embedded backend's automation roots must still cover the linked-file reads and the write.
    #[cfg(not(target_arch = "wasm32"))]
    let roots = crate::file_access::current();
    let shared = Arc::new(Shared::new(st.uid));
    let id = s.export_jobs.insert(shared.clone());
    let job = Job { doc, cache, _fonts: fonts, params, path };

    #[cfg(not(target_arch = "wasm32"))]
    {
        let thread = {
            let shared = shared.clone();
            let work = move || crate::file_access::confine(roots.as_ref(), || run(job, shared));
            std::thread::Builder::new().name("designcraft-export".into()).spawn(work)
        };
        if let Err(e) = thread {
            shared.finish(Err(e.to_string()));
            return Err(EngineError::Other(format!("could not start the export: {e}")));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        // No threads on the web: finish it now; the progress command then reports the outcome.
        run(job, shared);
    }
    Ok(json!({"job": id}))
}

/// Run `job` to completion, reporting progress and recording what it wrote.
///
/// A panic in the export (a bug in flattening, rasterizing or writing) becomes the job's error,
/// as a panic in a command becomes [`EngineError::Internal`]: the caller that polls the job is not
/// left waiting for a result that will never come.
fn run(job: Job, shared: Arc<Shared>) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export(job, &shared)))
        .unwrap_or_else(|payload| Err(crate::guard::panic_message(payload.as_ref())));
    shared.finish(outcome);
}

/// The export itself; see [`run`].
fn export(job: Job, shared: &Shared) -> std::result::Result<Outcome, String> {
    let should_cancel = || shared.cancelled();
    let mut report = |p: designcraft_pdf::ExportProgress| shared.set(p.phase, p.done, p.total, &p.page);
    let outcome = crate::cmd::pdf_export_report_with_progress(&job.doc, &job.cache, &job.params, &mut report, &should_cancel);
    let r = match outcome {
        Ok(r) => r,
        Err(e) => {
            return if should_cancel() { Ok(Outcome::cancelled()) } else { Err(e.to_string()) };
        }
    };
    // A cancel from here on is refused: the write is about to start and the file will exist. A
    // cancel that arrived before this line is honoured below, before anything is written.
    shared.start_writing();
    // Cancelled after the last page but before the write: leave no file behind.
    if should_cancel() {
        return Ok(Outcome::cancelled());
    }
    shared.set("finishing", 0, 0, "");
    crate::io::write_file(&job.path, &r.bytes).map_err(|e| e.to_string())?;
    Ok(Outcome { path: job.path, bytes: r.bytes.len(), pages: r.pages, warnings: r.warnings, cancelled: false })
}

impl Outcome {
    /// The result of a job that was cancelled before it wrote anything.
    fn cancelled() -> Self {
        Self { path: String::new(), bytes: 0, pages: 0, warnings: Vec::new(), cancelled: true }
    }
}

/// What `file.export.progress` reports about one job.
fn job_value(id: u64, shared: &Shared) -> Value {
    let progress = shared.progress();
    let outcome = shared.outcome();
    if outcome.is_some() {
        // This result reaches a caller now, so it may age out of the history.
        shared.observe();
    }
    let mut out = json!({
        "job": id,
        "busy": outcome.is_none(),
        "phase": progress.phase,
        "done": progress.done,
        "total": progress.total,
        "page": progress.page,
    });
    match outcome {
        None => {}
        Some(Ok(o)) => {
            out["path"] = json!(o.path);
            out["bytes"] = json!(o.bytes);
            out["pages"] = json!(o.pages);
            out["warnings"] = json!(o.warnings);
            out["cancelled"] = json!(o.cancelled);
        }
        Some(Err(e)) => {
            out["error"] = json!(e);
            out["cancelled"] = json!(false);
        }
    }
    out
}

/// `file.export.progress`: what a background export job is doing, or what it wrote.
pub(crate) fn progress(_s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.export.progress";
    let job = match p.get("job") {
        None | Some(Value::Null) => _s.export_jobs.latest(),
        Some(v) => Some(v.as_u64().ok_or_else(|| crate::cmd::bad(ID, "`job`: the id file.exportPdf returned"))?),
    };
    let id = job.ok_or_else(|| crate::cmd::bad(ID, "no background export has been queued"))?;
    let shared = _s.export_jobs.get(id).ok_or_else(|| crate::cmd::bad(ID, format!("no such background export `{id}`")))?;
    Ok(job_value(id, shared))
}

/// `file.export.jobs`: every background export the session remembers, oldest first.
pub(crate) fn jobs(_s: &mut Session, _p: &Value) -> Result<Value> {
    let jobs: Vec<Value> = _s.export_jobs.ids().into_iter().filter_map(|id| _s.export_jobs.get(id).map(|shared| job_value(id, shared))).collect();
    Ok(json!({"jobs": jobs}))
}

/// `file.export.cancel`: ask a running job to stop at its next page. A job that already finished
/// is reported as it is (`cancelRequested: false`).
pub(crate) fn cancel(_s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.export.cancel";
    let id = p.get("job").and_then(Value::as_u64).ok_or_else(|| crate::cmd::bad(ID, "`job`: the id file.exportPdf returned"))?;
    match _s.export_jobs.cancel(id) {
        Some(true) => Ok(json!({"job": id, "cancelRequested": true, "busy": true})),
        Some(false) => {
            let shared = _s.export_jobs.get(id).ok_or_else(|| crate::cmd::bad(ID, format!("no such background export `{id}`")))?;
            let mut out = job_value(id, shared);
            out["cancelRequested"] = json!(false);
            Ok(out)
        }
        None => Err(crate::cmd::bad(ID, format!("no such background export `{id}`"))),
    }
}

#[cfg(test)]
mod tests;
