//! Live preflight for the status bar and the Preflight panel.
//!
//! A check reads every story's composition and every placed graphic's linked file, so on a long
//! document it takes much longer than a frame. On native builds it runs on a worker thread, which
//! checks the newest state of the document it was given; the issues last found for the document
//! stay shown until the check of its current state ends. The worker checks an unchanged document
//! again from time to time, for linked files changed outside the app. The web build and tests
//! check on the interface thread, once per state of the document.

use std::sync::{Arc, Weak};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

use designcraft_compose::Cache;
use designcraft_doc::Document;
use designcraft_engine::cmd::preflight::{Issue, check_document};

use crate::DesignApp;

/// Lowest effective resolution, in pixels per inch, before a placed image is reported.
pub(crate) const MIN_PPI: f64 = 150.0;

/// Documents whose issues are kept: going back to one shows its issues at once.
const MAX_DOCUMENTS: usize = 8;

// An unchanged document is checked again after as long as its last check took, at least this
// long and at most `REFRESH_MAX`, so the worker is idle at least half the time.
#[cfg(not(target_arch = "wasm32"))]
const REFRESH_MIN: Duration = Duration::from_secs(2);
#[cfg(not(target_arch = "wasm32"))]
const REFRESH_MAX: Duration = Duration::from_secs(5 * 60);
// A check of another document still running this long after it was queued is blocked (a linked
// file on a share that doesn't answer): the active document gets a fresh worker.
#[cfg(not(target_arch = "wasm32"))]
const SUPERSEDE_AFTER: Duration = Duration::from_millis(200);
// Workers alive at once, counting replaced ones still blocked.
#[cfg(not(target_arch = "wasm32"))]
const MAX_WORKERS: usize = 4;
// A worker that couldn't be started, or that ended (a panic in a check), is tried again after
// this long.
#[cfg(not(target_arch = "wasm32"))]
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// A state of a document as a check sees it: which document, which state of it with which
/// compositions (a font reload replaces the cache and bumps the revision), and where it is saved
/// (relative links resolve against that). The state isn't kept alive: once it is dropped, no
/// later state can pass for it.
#[derive(Clone, Default)]
struct Checked {
    uid: u64,
    revision: u64,
    doc: Weak<Document>,
    cache: Weak<Cache>,
    path: Option<String>,
}

impl Checked {
    fn new(app: &DesignApp) -> Option<(Self, Arc<Document>)> {
        let st = app.session.active()?;
        let of = Checked {
            uid: st.uid,
            revision: st.revision,
            doc: Arc::downgrade(&st.doc),
            cache: Arc::downgrade(&app.session.cache),
            path: st.path.clone(),
        };
        Some((of, Arc::clone(&st.doc)))
    }

    fn same(&self, other: &Checked) -> bool {
        self.uid == other.uid
            && self.revision == other.revision
            && self.path == other.path
            && Weak::ptr_eq(&self.cache, &other.cache)
            && self.doc.upgrade().zip(other.doc.upgrade()).is_some_and(|(a, b)| Arc::ptr_eq(&a, &b))
    }
}

/// The issues found for one state of a document.
struct Found {
    of: Checked,
    issues: Arc<Vec<Issue>>,
    /// How long the check took.
    #[cfg(not(target_arch = "wasm32"))]
    took: Duration,
    /// When the interface last asked for this state (`egui` input time, seconds).
    #[cfg(not(target_arch = "wasm32"))]
    asked_at: f64,
}

#[derive(Default)]
pub(crate) struct State {
    /// The issues last found for each of the documents checked last, oldest first.
    found: Vec<Found>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Worker,
    /// Checks run on the interface thread.
    #[cfg(test)]
    pub(crate) inline_checks: usize,
}

impl State {
    /// Keeps `found` in place of the issues found before for its document.
    fn keep(&mut self, found: Found) {
        self.found.retain(|f| f.of.uid != found.of.uid);
        if self.found.len() >= MAX_DOCUMENTS {
            self.found.remove(0);
        }
        self.found.push(found);
    }
}

/// The issues of the active document: those last found, which may be of an earlier state of it
/// while the worker checks the current one. `None` until the first check of the document ends.
pub(crate) fn issues(app: &mut DesignApp, ctx: &egui::Context) -> Option<Arc<Vec<Issue>>> {
    let now = ctx.input(|i| i.time);
    issues_at(app, ctx, now)
}

fn issues_at(app: &mut DesignApp, ctx: &egui::Context, now: f64) -> Option<Arc<Vec<Issue>>> {
    let (of, doc) = Checked::new(app)?;
    let cache = Arc::clone(&app.session.cache);
    let state = &mut app.preflight;
    #[cfg(not(target_arch = "wasm32"))]
    for mut done in state.worker.poll() {
        done.asked_at = now;
        state.keep(done);
    }
    if let Some(i) = state.found.iter().position(|f| f.of.same(&of)) {
        // The active document's issues are the last to be dropped.
        let shown = state.found.remove(i);
        state.found.push(shown);
    }
    if let Some(found) = state.found.last_mut().filter(|f| f.of.same(&of)) {
        let issues = Arc::clone(&found.issues);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(wait) = state.worker.retry_wait() {
            if wait.is_zero() {
                found.asked_at = now;
                state.worker.ask(ctx, of, doc, &cache, true);
            } else {
                // Come back when a new worker may be tried.
                ctx.request_repaint_after(wait);
            }
        } else if state.worker.enabled() {
            let wait = refresh_after(found.took).as_secs_f64() - (now - found.asked_at);
            if wait > 0.0 {
                // Come back when the issues are due to be checked again, in case nothing else
                // wakes the interface.
                ctx.request_repaint_after(Duration::from_secs_f64(wait));
            } else {
                found.asked_at = now;
                state.worker.ask(ctx, of, doc, &cache, true);
            }
        }
        return Some(issues);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if state.worker.ask(ctx, of.clone(), Arc::clone(&doc), &cache, false) {
        return state.found.iter().find(|f| f.of.uid == of.uid).map(|f| Arc::clone(&f.issues));
    }
    let _ = (ctx, now);
    let issues = Arc::new(check_document(&doc, &cache, of.path.as_deref().map(std::path::Path::new), MIN_PPI));
    #[cfg(test)]
    {
        state.inline_checks += 1;
    }
    state.keep(Found {
        of,
        issues: Arc::clone(&issues),
        #[cfg(not(target_arch = "wasm32"))]
        took: Duration::ZERO,
        #[cfg(not(target_arch = "wasm32"))]
        asked_at: now,
    });
    Some(issues)
}

/// How long a check that took `took` stays current.
#[cfg(not(target_arch = "wasm32"))]
fn refresh_after(took: Duration) -> Duration {
    took.clamp(REFRESH_MIN, REFRESH_MAX)
}

#[cfg(not(target_arch = "wasm32"))]
struct Job {
    of: Checked,
    doc: Arc<Document>,
    cache: Arc<Cache>,
}

#[cfg(not(target_arch = "wasm32"))]
type Channels = (std::sync::mpsc::SyncSender<Job>, std::sync::mpsc::Receiver<Found>);

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Worker {
    channels: Option<Channels>,
    /// The state last asked for.
    asked: Checked,
    /// The job for that state while the worker's queue is full. A newer state replaces it, so at
    /// most one job waits here, one in the queue and one is being checked.
    waiting: Option<Job>,
    /// The jobs queued whose results haven't arrived, oldest (the one being checked) first, each
    /// with when it was queued. The worker answers each job in order, so there are at most two.
    in_flight: std::collections::VecDeque<(Checked, Instant)>,
    /// Worker threads alive, counting replaced ones still blocked.
    live: Arc<std::sync::atomic::AtomicUsize>,
    /// When the worker could not be started, or ended (a panic in a check). Checks run on the
    /// interface thread until a new worker is tried, [`RETRY_AFTER`] later.
    failed: Option<Instant>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    /// Whether a worker runs or may be started.
    fn enabled(&self) -> bool {
        self.channels.is_some() || (background() && self.retry_due())
    }

    fn retry_due(&self) -> bool {
        self.failed.is_none_or(|at| at.elapsed() >= RETRY_AFTER)
    }

    /// How long until a new worker may be tried after one failed; `None` while one runs or when
    /// none failed.
    fn retry_wait(&self) -> Option<Duration> {
        if self.channels.is_some() {
            return None;
        }
        self.failed.map(|at| RETRY_AFTER.saturating_sub(at.elapsed()))
    }

    fn fail(&mut self) {
        self.channels = None;
        self.asked = Checked::default();
        self.waiting = None;
        self.in_flight.clear();
        self.failed = Some(Instant::now());
    }

    /// Asks the worker for state `of` unless it was the last state asked for (`again` asks for it
    /// anyway). False when there is no worker, so the caller checks on its own thread.
    fn ask(&mut self, ctx: &egui::Context, of: Checked, doc: Arc<Document>, cache: &Arc<Cache>, again: bool) -> bool {
        if self.channels.is_none() {
            if !background() || !self.retry_due() {
                return false;
            }
            self.failed = None;
            self.channels = spawn(ctx.clone(), &self.live);
            if self.channels.is_none() {
                self.fail();
                return false;
            }
        }
        self.supersede_blocked(ctx, &of);
        // A state already queued or being checked isn't asked for again.
        let pending = self.in_flight.iter().any(|(job, _)| job.same(&of)) || self.waiting.as_ref().is_some_and(|job| job.of.same(&of));
        if (again && !pending) || !self.asked.same(&of) {
            self.asked = of.clone();
            self.waiting = Some(Job { of, doc, cache: Arc::clone(cache) });
        }
        self.send_waiting();
        self.channels.is_some()
    }

    /// Replaces the worker when it is still on a job for another document queued more than
    /// [`SUPERSEDE_AFTER`] ago, unless [`MAX_WORKERS`] are alive. The replaced thread ends when
    /// its check returns; a job queued behind it is asked of the new worker.
    fn supersede_blocked(&mut self, ctx: &egui::Context, of: &Checked) {
        let Some(running) = self.in_flight.front().filter(|(job, _)| job.uid != of.uid).map(|(_, at)| at.elapsed()) else { return };
        if running < SUPERSEDE_AFTER {
            // Come back when it has run that long, in case nothing else wakes the interface.
            ctx.request_repaint_after(SUPERSEDE_AFTER - running);
            return;
        }
        if self.live.load(std::sync::atomic::Ordering::Relaxed) >= MAX_WORKERS {
            return;
        }
        if let Some(channels) = spawn(ctx.clone(), &self.live) {
            self.channels = Some(channels);
            self.in_flight.clear();
            self.asked = Checked::default();
        }
    }

    /// Queues the waiting job if the worker's queue has room.
    fn send_waiting(&mut self) {
        let (Some(job), Some((tx, _))) = (self.waiting.take(), self.channels.as_ref()) else { return };
        let of = job.of.clone();
        match tx.try_send(job) {
            Ok(()) => self.in_flight.push_back((of, Instant::now())),
            Err(std::sync::mpsc::TrySendError::Full(job)) => self.waiting = Some(job),
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => self.fail(),
        }
    }

    /// The results the worker sent since the last poll, oldest first.
    fn poll(&mut self) -> Vec<Found> {
        let Some((_, rx)) = self.channels.as_ref() else { return Vec::new() };
        let mut latest = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(found) => {
                    self.in_flight.pop_front();
                    latest.push(found);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.fail();
                    break;
                }
            }
        }
        // The worker took a job and has room for the next.
        self.send_waiting();
        latest
    }

    /// Waits for the worker's next result.
    #[cfg(test)]
    fn wait(&mut self) -> Option<Found> {
        let found = self.channels.as_ref()?.1.recv().ok()?;
        self.in_flight.pop_front();
        self.send_waiting();
        Some(found)
    }
}

/// Whether checks run on the worker: not in tests (they can start one with [`start_worker`]) and
/// not with `DESIGNCRAFT_SYNC_RENDER` set.
#[cfg(not(target_arch = "wasm32"))]
fn background() -> bool {
    !cfg!(test) && std::env::var_os("DESIGNCRAFT_SYNC_RENDER").is_none()
}

/// Counts a worker thread as alive until it ends, however it ends.
#[cfg(not(target_arch = "wasm32"))]
struct Alive(Arc<std::sync::atomic::AtomicUsize>);

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Alive {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(ctx: egui::Context, live: &Arc<std::sync::atomic::AtomicUsize>) -> Option<Channels> {
    let (tx, jobs) = std::sync::mpsc::sync_channel::<Job>(1);
    let (done_tx, rx) = std::sync::mpsc::channel::<Found>();
    live.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let alive = Alive(Arc::clone(live));
    std::thread::Builder::new()
        .name("designcraft-preflight".into())
        .spawn(move || {
            let _alive = alive;
            while let Ok(job) = jobs.recv() {
                let started = Instant::now();
                let issues = check_document(&job.doc, &job.cache, job.of.path.as_deref().map(std::path::Path::new), MIN_PPI);
                let found = Found { of: job.of, issues: Arc::new(issues), took: started.elapsed(), asked_at: 0.0 };
                // The checked state isn't kept alive by its check once the result is sent.
                drop(job.doc);
                if done_tx.send(found).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        })
        .ok()?;
    Some((tx, rx))
}

/// Starts the worker in a test, where checks otherwise run on the interface thread.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn start_worker(app: &mut DesignApp, ctx: &egui::Context) {
    app.preflight.worker.channels = spawn(ctx.clone(), &app.preflight.worker.live);
}

/// Waits in a test for the worker's next result and keeps it, as a frame that polls would.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn wait_for_worker(app: &mut DesignApp) {
    if let Some(found) = app.preflight.worker.wait() {
        app.preflight.keep(found);
    }
}

#[cfg(test)]
mod tests;
