//! Native background link checks for the Links panel.
//!
//! Whether a linked file is still there and unchanged is asked of the file system, and a network
//! share that doesn't answer keeps the caller waiting for an operating-system timeout. The
//! panel's rows therefore come from the document alone, on the UI thread, and their statuses
//! from a check on a worker thread: the statuses last found stay until newer ones arrive, so an
//! unavailable share can't stop the application from repainting or accepting input.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};
use std::{hash::Hash, hash::Hasher};

use designcraft_compose::Cache;
use designcraft_doc::{AssetId, Document, Fingerprint};
use designcraft_engine::cmd::LinkListChecks;
use serde_json::Value;

// A document edit is checked at once. External changes (a file restored or edited elsewhere) are
// polled: an unchanged link costs one metadata call, but on a slow share with hundreds of links a
// check can itself take seconds, so the next one waits at least as long as the last took and the
// worker is idle at least half the time.
const REFRESH_MIN: Duration = Duration::from_secs(2);
const REFRESH_MAX: Duration = Duration::from_secs(5 * 60);
// A check for another document than the one shown is told to stop. One still running this long
// after is blocked in a file system call: the document shown gets a fresh worker instead of
// waiting behind it, and the blocked one ends when its call returns.
const SUPERSEDE_AFTER: Duration = Duration::from_millis(200);
// Workers alive at once, counting replaced ones still blocked in a file system call. Switching
// documents while a share is down can't pile up threads beyond this.
const MAX_WORKERS: usize = 4;
// A worker that couldn't be started, or that ended on its own (a panic under the link check),
// is replaced after this long: one failure doesn't leave the panel without statuses for the
// rest of the session, and a failure that repeats isn't retried on every frame.
const RETRY_AFTER: Duration = Duration::from_secs(30);
// Documents whose statuses are kept: going back to one shows them at once.
const MAX_DOCUMENTS: usize = 8;

/// A document as a check sees it: which one, which state of it, and where it is saved (relative
/// links resolve against that).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Key {
    pub(crate) uid: u64,
    document: usize,
    path: u64,
}

impl Key {
    pub(crate) fn new(uid: u64, document: &Arc<Document>, path: Option<&str>) -> Self {
        let mut path_hash = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut path_hash);
        Self { uid, document: Arc::as_ptr(document) as usize, path: path_hash.finish() }
    }
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(crate) key: Key,
    pub(crate) doc: Arc<Document>,
    pub(crate) cache: Arc<Cache>,
    pub(crate) document: Option<PathBuf>,
}

pub(crate) struct Rows {
    /// The document's links as it is now, each with the status last found for it (`checking`
    /// until one is).
    pub(crate) values: Arc<Vec<Value>>,
    /// No check can run for now (the worker couldn't be started, or ended): statuses may be old.
    pub(crate) unavailable: bool,
    /// Linked images a check just read and found unchanged, for
    /// [`designcraft_engine::cmd::remember_link_files`]: only when the check was of the document
    /// as it is now.
    pub(crate) verified: Vec<(AssetId, Fingerprint)>,
}

struct Job {
    token: u64,
    snapshot: Snapshot,
    /// Set by the UI thread to end the check before its next file.
    stop: Arc<AtomicBool>,
}

struct Done {
    token: u64,
    key: Key,
    /// Keeps the checked document's allocation, so no later document can reuse the address in
    /// `key` and pass for it once this one is dropped.
    pin: Weak<Document>,
    /// The checked rows; `None` when the check was stopped.
    rows: Option<Vec<Value>>,
    verified: Vec<(AssetId, Fingerprint)>,
    /// How long the check took and when it ended, on the worker's clock: the result may wait to
    /// be read (the panel can be hidden meanwhile).
    took: Duration,
    finished: Instant,
}

enum Poll {
    Done(Done),
    Empty,
    Disconnected,
}

type Runner = Box<dyn FnMut(&Snapshot, &mut LinkListChecks, &AtomicBool) -> Option<Vec<Value>> + Send>;

/// What the checks remember (files already compared), handed from job to job and from a replaced
/// worker to the next. Locked only between jobs, never while a file is asked for.
type Pool = Arc<Mutex<LinkListChecks>>;

/// Counts a worker thread as alive until it ends, however it ends. Its end frees a slot below
/// [`MAX_WORKERS`], so the UI is woken to replace a worker it was waiting behind.
struct Alive {
    live: Arc<AtomicUsize>,
    ctx: egui::Context,
}

impl Drop for Alive {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
        self.ctx.request_repaint();
    }
}

struct Worker {
    tx: std::sync::mpsc::Sender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
}

impl Worker {
    fn spawn(ctx: egui::Context, live: Arc<AtomicUsize>, pool: Pool) -> Option<Self> {
        Self::spawn_with(
            ctx,
            live,
            pool,
            Box::new(|snapshot, checks, stop| {
                designcraft_engine::cmd::list_document_links(&snapshot.doc, &snapshot.cache, snapshot.document.as_deref(), checks, stop)
            }),
        )
    }

    fn spawn_with(ctx: egui::Context, live: Arc<AtomicUsize>, pool: Pool, mut run: Runner) -> Option<Self> {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        live.fetch_add(1, Ordering::Relaxed);
        let alive = Alive { live, ctx: ctx.clone() };
        std::thread::Builder::new()
            .name("designcraft-links".into())
            .spawn(move || {
                let _alive = alive;
                while let Ok(Job { token, snapshot, stop }) = jobs.recv() {
                    let (key, pin, started) = (snapshot.key, Arc::downgrade(&snapshot.doc), Instant::now());
                    let mut checks = std::mem::take(&mut *pool.lock().unwrap_or_else(PoisonError::into_inner));
                    let rows = run(&snapshot, &mut checks, &stop);
                    let verified = checks.take_verified();
                    pool.lock().unwrap_or_else(PoisonError::into_inner).absorb(checks);
                    // Before the result is sent: whoever receives it finds the document no longer
                    // held here (an edited document isn't kept alive by its check).
                    drop(snapshot);
                    let done = Done { token, key, pin, rows, verified, took: started.elapsed(), finished: Instant::now() };
                    if done_tx.send(done).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Self { tx, rx })
    }

    fn submit(&self, job: Job) -> bool {
        self.tx.send(job).is_ok()
    }

    fn poll(&self) -> Poll {
        let mut latest = None;
        loop {
            match self.rx.try_recv() {
                Ok(done) => latest = Some(done),
                Err(std::sync::mpsc::TryRecvError::Empty) => return latest.map_or(Poll::Empty, Poll::Done),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return latest.map_or(Poll::Disconnected, Poll::Done),
            }
        }
    }
}

/// How long a check that took `took` stays current.
fn refresh_after(took: Duration) -> Duration {
    took.clamp(REFRESH_MIN, REFRESH_MAX)
}

/// The document's rows with, where a status waits for the files, the one the last check found
/// for the same link (the same graphic or story, at the same path).
fn with_statuses(document: &[Value], checked: Option<&[Value]>) -> Vec<Value> {
    let Some(checked) = checked else { return document.to_vec() };
    let link = |row: &Value| (row["asset"].as_u64(), row["story"].as_u64(), row["path"].as_str().map(str::to_owned));
    let found: HashMap<_, &Value> = checked.iter().map(|row| (link(row), &row["status"])).collect();
    document
        .iter()
        .map(|row| {
            let mut row = row.clone();
            if row["status"] == "checking"
                && let Some(status) = found.get(&link(&row))
                && let Some(fields) = row.as_object_mut()
            {
                fields.insert("status".into(), (*status).clone());
            }
            row
        })
        .collect()
}

/// The latest check of a document.
struct Checked {
    key: Key,
    /// As in [`Done`]: `key` names this document only while its allocation is kept.
    _pin: Weak<Document>,
    rows: Vec<Value>,
    finished: Instant,
    /// When its files are asked again.
    due: Instant,
}

/// The rows of the document shown, from the document alone.
struct Structure {
    key: Key,
    _pin: Weak<Document>,
    rows: Vec<Value>,
}

struct InFlight {
    token: u64,
    key: Key,
    stop: Arc<AtomicBool>,
    /// When the check was told to stop.
    stopping: Option<Instant>,
}

#[cfg(test)]
type Spawn = Box<dyn Fn(egui::Context, Arc<AtomicUsize>, Pool) -> Option<Worker> + Send + Sync>;

/// What tests put in place of the worker's check and of the document's rows.
#[cfg(test)]
struct Hooks {
    spawn: Spawn,
    preview: Box<dyn Fn(&Snapshot) -> Vec<Value> + Send + Sync>,
}

#[derive(Default)]
pub(crate) struct State {
    worker: Option<Worker>,
    /// When to try again after the worker couldn't be started or was found ended.
    retry_at: Option<Instant>,
    next_token: u64,
    in_flight: Option<InFlight>,
    /// The latest check of each document shown lately, by uid.
    checked: HashMap<u64, Checked>,
    structure: Option<Structure>,
    /// `structure` with the statuses of the check that ended at the given time (none: `None`):
    /// what the panel draws.
    shown: Option<(Option<Instant>, Arc<Vec<Value>>)>,
    live: Arc<AtomicUsize>,
    pool: Pool,
    #[cfg(test)]
    hooks: Option<Hooks>,
}

impl State {
    pub(crate) fn rows(&mut self, ctx: &egui::Context, snapshot: Snapshot) -> Rows {
        let key = snapshot.key;
        let mut verified = Vec::new();
        match self.worker.as_ref().map(Worker::poll) {
            Some(Poll::Done(done)) => {
                if self.in_flight.as_ref().is_some_and(|job| job.token == done.token) {
                    self.in_flight = None;
                }
                // A check of an earlier state of the document still says how its files are: its
                // statuses are shown while the next check runs. What it read vouches only for
                // the bytes it compared, so that is passed on for this very state alone.
                if let Some(rows) = done.rows {
                    if done.key == key {
                        verified = done.verified;
                    }
                    self.keep(Checked {
                        key: done.key,
                        _pin: done.pin,
                        rows,
                        finished: done.finished,
                        due: done.finished + refresh_after(done.took),
                    });
                }
            }
            Some(Poll::Disconnected) => {
                self.worker = None;
                self.retry_at = Some(Instant::now() + RETRY_AFTER);
                self.in_flight = None;
            }
            Some(Poll::Empty) | None => {}
        }

        // A check of another document than the one shown ends before its next file. If a file
        // system call keeps it (a share that doesn't answer), it is left to finish on its own,
        // its result dropped with its channel, and the document shown gets a fresh worker.
        if let Some(job) = self.in_flight.as_mut().filter(|job| job.key.uid != key.uid) {
            if job.stopping.is_none() {
                job.stop.store(true, Ordering::Relaxed);
                job.stopping = Some(Instant::now());
            }
            let waited = job.stopping.map_or(Duration::ZERO, |at| at.elapsed());
            if waited < SUPERSEDE_AFTER {
                // A blocked worker never asks for a repaint, so come back when it can be replaced.
                ctx.request_repaint_after(SUPERSEDE_AFTER.saturating_sub(waited));
            } else if self.live.load(Ordering::Relaxed) < MAX_WORKERS {
                self.worker = None;
                self.in_flight = None;
            }
        }
        if self.retry_at.is_some_and(|at| Instant::now() >= at) {
            self.retry_at = None;
        }
        if self.worker.is_none() && self.retry_at.is_none() {
            self.worker = self.spawn(ctx);
            if self.worker.is_none() {
                self.retry_at = Some(Instant::now() + RETRY_AFTER);
            }
        }

        let due = self.checked.get(&key.uid).is_none_or(|checked| checked.key != key || Instant::now() >= checked.due);
        if due && self.in_flight.is_none() && self.retry_at.is_none() {
            self.next_token = self.next_token.wrapping_add(1);
            let (token, stop) = (self.next_token, Arc::new(AtomicBool::new(false)));
            if self.worker.as_ref().is_some_and(|worker| worker.submit(Job { token, snapshot: snapshot.clone(), stop: Arc::clone(&stop) })) {
                self.in_flight = Some(InFlight { token, key, stop, stopping: None });
            } else {
                self.worker = None;
                self.retry_at = Some(Instant::now() + RETRY_AFTER);
            }
        }

        let values = self.shown(&snapshot);
        if let Some(at) = self.retry_at {
            // Nothing else wakes the panel for the next try.
            ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
        } else if self.in_flight.is_none()
            && let Some(checked) = self.checked.get(&key.uid).filter(|checked| checked.key == key)
        {
            ctx.request_repaint_after(checked.due.saturating_duration_since(Instant::now()));
        }
        Rows { values, unavailable: self.retry_at.is_some(), verified }
    }

    fn spawn(&self, ctx: &egui::Context) -> Option<Worker> {
        let (live, pool) = (Arc::clone(&self.live), Arc::clone(&self.pool));
        #[cfg(test)]
        if let Some(hooks) = &self.hooks {
            return (hooks.spawn)(ctx.clone(), live, pool);
        }
        Worker::spawn(ctx.clone(), live, pool)
    }

    /// The document's rows, looking at no file: an edit shows at once, whatever the worker is
    /// waiting for.
    fn preview(&self, snapshot: &Snapshot) -> Vec<Value> {
        #[cfg(test)]
        if let Some(hooks) = &self.hooks {
            return (hooks.preview)(snapshot);
        }
        designcraft_engine::cmd::preview_document_links(&snapshot.doc, &snapshot.cache, snapshot.document.as_deref())
    }

    /// The rows to draw: built again only when the document or its latest check changes.
    fn shown(&mut self, snapshot: &Snapshot) -> Arc<Vec<Value>> {
        let key = snapshot.key;
        if self.structure.as_ref().is_none_or(|structure| structure.key != key) {
            let rows = self.preview(snapshot);
            self.structure = Some(Structure { key, _pin: Arc::downgrade(&snapshot.doc), rows });
            self.shown = None;
        }
        let checked = self.checked.get(&key.uid);
        let checked_at = checked.map(|checked| checked.finished);
        if let Some((at, rows)) = &self.shown
            && *at == checked_at
        {
            return Arc::clone(rows);
        }
        let document = self.structure.as_ref().map_or(&[][..], |structure| &structure.rows);
        let rows = Arc::new(with_statuses(document, checked.map(|checked| checked.rows.as_slice())));
        self.shown = Some((checked_at, Arc::clone(&rows)));
        rows
    }

    fn keep(&mut self, checked: Checked) {
        self.checked.insert(checked.key.uid, checked);
        if self.checked.len() > MAX_DOCUMENTS
            && let Some(oldest) = self.checked.iter().min_by_key(|(_, checked)| checked.finished).map(|(uid, _)| *uid)
        {
            self.checked.remove(&oldest);
        }
    }

    #[cfg(test)]
    pub(crate) fn checking(&self) -> bool {
        self.in_flight.is_some()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn snapshot(key: Key) -> Snapshot {
        Snapshot { key, doc: Arc::new(Document::new(&Default::default())), cache: Arc::new(Cache::new()), document: None }
    }

    fn key(uid: u64, document: usize) -> Key {
        Key { uid, document, path: 0 }
    }

    fn row(asset: u64, status: &str) -> Value {
        json!({"asset": asset, "name": format!("{asset}.png"), "path": format!("/links/{asset}.png"), "status": status})
    }

    fn statuses(rows: &Rows) -> Vec<String> {
        rows.values.iter().map(|row| row["status"].as_str().unwrap_or_default().to_string()).collect()
    }

    /// Something a test's check waits for: opened by the test, or after two seconds so a failing
    /// test can't hang.
    #[derive(Clone, Default)]
    struct Gate(Arc<(Mutex<bool>, std::sync::Condvar)>);

    impl Gate {
        fn new() -> Self {
            let gate = Self::default();
            let timeout = gate.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(2));
                timeout.open();
            });
            gate
        }

        fn open(&self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }

        fn wait(&self) {
            let mut open = self.0.0.lock().unwrap();
            while !*open {
                open = self.0.1.wait(open).unwrap();
            }
        }
    }

    /// A state whose document rows are `preview` and whose workers check with `run`; and how many
    /// workers it has started.
    fn state(
        preview: impl Fn(&Snapshot) -> Vec<Value> + Send + Sync + 'static,
        run: impl Fn(&Snapshot, &AtomicBool) -> Option<Vec<Value>> + Send + Sync + 'static,
    ) -> (State, Arc<AtomicUsize>) {
        let (run, started) = (Arc::new(run), Arc::new(AtomicUsize::new(0)));
        let count = Arc::clone(&started);
        let spawn: Spawn = Box::new(move |ctx, live, pool| {
            count.fetch_add(1, Ordering::Relaxed);
            let run = Arc::clone(&run);
            Worker::spawn_with(ctx, live, pool, Box::new(move |snapshot, _, stop| run(snapshot, stop)))
        });
        (State { hooks: Some(Hooks { spawn, preview: Box::new(preview) }), ..Default::default() }, started)
    }

    /// Ask for the rows as the panel does, frame after frame, until no check is in flight.
    fn settle(state: &mut State, ctx: &egui::Context, key: Key) -> Rows {
        let mut rows = state.rows(ctx, snapshot(key));
        for _ in 0..1000 {
            if !state.checking() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
            rows = state.rows(ctx, snapshot(key));
        }
        rows
    }

    /// A worker the test answers for: the jobs sent to it, and where its results go.
    fn by_hand(state: &mut State) -> (std::sync::mpsc::Receiver<Job>, std::sync::mpsc::Sender<Done>) {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        state.worker = Some(Worker { tx, rx });
        (jobs, done_tx)
    }

    fn done(token: u64, key: Key, rows: Vec<Value>) -> Done {
        Done { token, key, pin: Weak::new(), rows: Some(rows), verified: Vec::new(), took: Duration::ZERO, finished: Instant::now() }
    }

    #[test]
    fn the_rows_are_there_while_the_check_is_blocked() {
        let ctx = egui::Context::default();
        let gate = Gate::new();
        let blocked = gate.clone();
        let (mut state, _) = state(
            |_| vec![row(1, "checking")],
            move |_, _| {
                blocked.wait();
                Some(vec![row(1, "ok")])
            },
        );

        let before = Instant::now();
        let rows = state.rows(&ctx, snapshot(key(1, 1)));
        let elapsed = before.elapsed();

        assert!(elapsed < Duration::from_millis(500), "the UI call waited {elapsed:?}");
        assert_eq!(statuses(&rows), ["checking"], "the document's rows, before any file answered");
        assert!(state.checking());
        gate.open();
        assert_eq!(statuses(&settle(&mut state, &ctx, key(1, 1))), ["ok"]);
    }

    #[test]
    fn an_edit_shows_at_once_with_the_statuses_last_found() {
        let ctx = egui::Context::default();
        let gate = Gate::new();
        let blocked = gate.clone();
        // The edit (document 2) places a second graphic; its check doesn't answer for a while.
        let (mut state, _) = state(
            |snapshot| if snapshot.key.document == 1 { vec![row(1, "checking")] } else { vec![row(1, "checking"), row(2, "checking")] },
            move |snapshot, _| {
                if snapshot.key.document == 1 {
                    return Some(vec![row(1, "modified")]);
                }
                blocked.wait();
                Some(vec![row(1, "ok"), row(2, "ok")])
            },
        );
        assert_eq!(statuses(&settle(&mut state, &ctx, key(1, 1))), ["modified"]);

        let rows = state.rows(&ctx, snapshot(key(1, 2)));

        assert_eq!(statuses(&rows), ["modified", "checking"], "the new graphic is listed, the other keeps its status");
        assert!(state.checking());
        gate.open();
        assert_eq!(statuses(&settle(&mut state, &ctx, key(1, 2))), ["ok", "ok"]);
    }

    #[test]
    fn a_check_of_an_earlier_edit_still_fills_in_statuses() {
        let ctx = egui::Context::default();
        let (first, second) = (Gate::new(), Gate::new());
        let (wait_first, wait_second) = (first.clone(), second.clone());
        let (mut state, _) = state(
            |_| vec![row(1, "checking")],
            move |snapshot, _| {
                if snapshot.key.document == 1 {
                    wait_first.wait();
                    return Some(vec![row(1, "missing")]);
                }
                wait_second.wait();
                Some(vec![row(1, "ok")])
            },
        );
        // The document is edited while its check runs: the check isn't thrown away, or typing
        // faster than a slow share answers would leave the statuses as they were for good.
        state.rows(&ctx, snapshot(key(1, 1)));
        assert_eq!(statuses(&state.rows(&ctx, snapshot(key(1, 2)))), ["checking"]);
        first.open();
        let mut rows = state.rows(&ctx, snapshot(key(1, 2)));
        for _ in 0..1000 {
            if statuses(&rows) != ["checking"] {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
            rows = state.rows(&ctx, snapshot(key(1, 2)));
        }

        assert_eq!(statuses(&rows), ["missing"], "what the earlier check found is shown");
        assert!(state.checking(), "and the document as it is now is checked next");
        second.open();
        assert_eq!(statuses(&settle(&mut state, &ctx, key(1, 2))), ["ok"]);
    }

    #[test]
    fn a_check_is_timed_by_the_worker_not_by_when_its_result_is_read() {
        let ctx = egui::Context::default();
        let (mut state, _) = state(|_| vec![row(1, "checking")], |_, _| None);
        let (_jobs, results) = by_hand(&mut state);
        state.rows(&ctx, snapshot(key(1, 1)));
        // The panel may be hidden while a check ends: what counts is when it ended, there.
        let (finished, took) = (Instant::now(), Duration::from_secs(4));
        results.send(Done { took, finished, ..done(1, key(1, 1), vec![row(1, "ok")]) }).unwrap();

        let rows = state.rows(&ctx, snapshot(key(1, 1)));

        assert_eq!(statuses(&rows), ["ok"]);
        let checked = &state.checked[&1];
        assert_eq!((checked.finished, checked.due), (finished, finished + took));
    }

    #[test]
    fn external_changes_are_polled_within_seconds_and_slow_checks_space_out() {
        assert_eq!(refresh_after(Duration::from_millis(30)), REFRESH_MIN, "a quick check is repeated after two seconds");
        assert_eq!(refresh_after(Duration::from_secs(4)), Duration::from_secs(4), "a slow share is idle as long as it was busy");
        assert_eq!(refresh_after(Duration::from_secs(3600)), REFRESH_MAX);
    }

    #[test]
    fn the_files_are_asked_again_when_a_check_is_due() {
        let ctx = egui::Context::default();
        let (mut state, _) = state(|_| vec![row(1, "checking")], |_, _| Some(vec![row(1, "ok")]));
        assert_eq!(statuses(&settle(&mut state, &ctx, key(1, 1))), ["ok"]);
        state.rows(&ctx, snapshot(key(1, 1)));
        assert!(!state.checking(), "a check just made is current");

        state.checked.get_mut(&1).unwrap().due = Instant::now();
        state.rows(&ctx, snapshot(key(1, 1)));

        assert!(state.checking(), "an external change shows within the interval");
    }

    #[test]
    fn a_check_of_another_document_is_stopped_and_its_worker_goes_on() {
        let ctx = egui::Context::default();
        // Document 1's check has many files to go; like the engine's, it looks at the stop flag
        // between them.
        let (mut state, started) = state(
            |snapshot| vec![row(snapshot.key.uid, "checking")],
            |snapshot, stop| {
                if snapshot.key.uid == 1 {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    return None;
                }
                Some(vec![row(2, "ok")])
            },
        );
        state.rows(&ctx, snapshot(key(1, 1)));

        let rows = settle(&mut state, &ctx, key(2, 1));

        assert_eq!(statuses(&rows), ["ok"]);
        assert_eq!(started.load(Ordering::Relaxed), 1, "no thread is left behind for a check that can be stopped");
    }

    #[test]
    fn a_blocked_check_of_another_document_does_not_gate_the_one_shown() {
        let ctx = egui::Context::default();
        let gate = Gate::new();
        let blocked = gate.clone();
        // Document 1's links are on a share that doesn't answer.
        let (mut state, started) = state(
            |snapshot| vec![row(snapshot.key.uid, "checking")],
            move |snapshot, _| {
                if snapshot.key.uid == 1 {
                    blocked.wait();
                }
                Some(vec![row(snapshot.key.uid, "ok")])
            },
        );
        state.rows(&ctx, snapshot(key(1, 1)));
        state.rows(&ctx, snapshot(key(2, 1)));
        std::thread::sleep(SUPERSEDE_AFTER);

        let rows = settle(&mut state, &ctx, key(2, 1));
        gate.open();

        assert_eq!(statuses(&rows), ["ok"], "document 2 waited behind document 1's blocked check");
        assert_eq!(started.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_switch_before_the_takeover_is_due_schedules_a_repaint_for_it() {
        let ctx = egui::Context::default();
        let delays = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&delays);
        ctx.set_request_repaint_callback(move |info| seen.lock().unwrap().push(info.delay));
        let gate = Gate::new();
        let blocked = gate.clone();
        let (mut state, started) = state(
            |_| Vec::new(),
            move |_, _| {
                blocked.wait();
                None
            },
        );
        state.rows(&ctx, snapshot(key(1, 1)));
        delays.lock().unwrap().clear();

        state.rows(&ctx, snapshot(key(2, 1)));

        assert!(state.in_flight.as_ref().is_some_and(|job| job.key.uid == 1 && job.stop.load(Ordering::Relaxed)), "told to stop, not replaced yet");
        assert_eq!(started.load(Ordering::Relaxed), 1);
        let delays = delays.lock().unwrap();
        assert!(delays.iter().any(|delay| *delay <= SUPERSEDE_AFTER), "no repaint scheduled for the takeover: {delays:?}");
        gate.open();
    }

    #[test]
    fn a_replaced_worker_that_ends_wakes_the_document_waiting_at_the_limit() {
        let ctx = egui::Context::default();
        let repaints = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&repaints);
        ctx.set_request_repaint_callback(move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        // Documents 1 and 2 are on shares that don't answer until released.
        let (first, second) = (Gate::new(), Gate::new());
        let (wait_first, wait_second) = (first.clone(), second.clone());
        let (mut state, started) = state(
            |snapshot| vec![row(snapshot.key.uid, "checking")],
            move |snapshot, _| {
                match snapshot.key.uid {
                    1 => wait_first.wait(),
                    2 => wait_second.wait(),
                    _ => {}
                }
                Some(vec![row(snapshot.key.uid, "ok")])
            },
        );
        // Two slots left: document 1's worker, then document 2's, which replaces it.
        state.live = Arc::new(AtomicUsize::new(MAX_WORKERS - 2));
        let show = |state: &mut State, uid: u64| {
            state.rows(&ctx, snapshot(key(uid, 1)));
            std::thread::sleep(SUPERSEDE_AFTER);
            state.rows(&ctx, snapshot(key(uid, 1)))
        };
        show(&mut state, 1);
        show(&mut state, 2);
        show(&mut state, 3);
        assert!(state.in_flight.as_ref().is_some_and(|job| job.key.uid == 2), "document 3 waits at the limit");
        assert_eq!((state.live.load(Ordering::Relaxed), started.load(Ordering::Relaxed)), (MAX_WORKERS, 2));

        repaints.store(0, Ordering::Relaxed);
        first.open();
        let woken = (0..1000).any(|_| {
            std::thread::sleep(Duration::from_millis(2));
            repaints.load(Ordering::Relaxed) > 0 && state.live.load(Ordering::Relaxed) < MAX_WORKERS
        });
        let rows = settle(&mut state, &ctx, key(3, 1));
        second.open();

        assert!(woken, "the ended worker freed its slot and asked for a repaint");
        assert_eq!(statuses(&rows), ["ok"], "document 3 got a worker once a slot was free");
        assert_eq!(started.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn replaced_workers_are_bounded() {
        let ctx = egui::Context::default();
        let (mut state, started) = state(|_| Vec::new(), |_, _| None);
        let (_jobs, _results) = by_hand(&mut state);
        state.live = Arc::new(AtomicUsize::new(MAX_WORKERS));
        state.in_flight = Some(InFlight { token: 7, key: key(1, 1), stop: Arc::default(), stopping: Some(Instant::now()) });
        std::thread::sleep(SUPERSEDE_AFTER);

        state.rows(&ctx, snapshot(key(2, 1)));

        assert_eq!(state.in_flight.as_ref().map(|job| job.token), Some(7), "at the limit the document shown waits instead");
        assert_eq!(started.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn a_worker_that_fails_is_replaced_after_a_while() {
        let ctx = egui::Context::default();
        let delays = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&delays);
        ctx.set_request_repaint_callback(move |info| seen.lock().unwrap().push(info.delay));
        let (mut state, started) = state(|_| vec![row(1, "checking")], |_, _| Some(vec![row(1, "ok")]));
        // The first worker can't be started; the next ones can.
        let hooks = state.hooks.take().unwrap();
        let (attempts, spawn) = (Arc::new(AtomicUsize::new(0)), hooks.spawn);
        let count = Arc::clone(&attempts);
        let spawn: Spawn = Box::new(move |ctx, live, pool| if count.fetch_add(1, Ordering::Relaxed) == 0 { None } else { spawn(ctx, live, pool) });
        state.hooks = Some(Hooks { spawn, preview: hooks.preview });

        let rows = settle(&mut state, &ctx, key(1, 1));
        assert!(rows.unavailable);
        assert_eq!(statuses(&rows), ["checking"], "the document's rows are still listed");
        assert!(state.rows(&ctx, snapshot(key(1, 1))).unavailable, "not tried again on the next frame");
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        let retry = |delay: &Duration| *delay > RETRY_AFTER / 2 && *delay <= RETRY_AFTER;
        assert!(delays.lock().unwrap().iter().any(retry), "no repaint scheduled for the next try");

        state.retry_at = Some(Instant::now());
        let rows = settle(&mut state, &ctx, key(1, 1));
        assert!(!rows.unavailable);
        assert_eq!(statuses(&rows), ["ok"]);

        // The same for a worker found ended: its channels are closed.
        let (jobs, results) = by_hand(&mut state);
        drop((jobs, results));
        let rows = state.rows(&ctx, snapshot(key(1, 1)));
        assert!(rows.unavailable);
        assert_eq!(statuses(&rows), ["ok"], "the statuses last found stay");
        state.retry_at = Some(Instant::now());
        state.checked.clear();
        let rows = settle(&mut state, &ctx, key(1, 1));
        assert!(!rows.unavailable);
        assert_eq!(statuses(&rows), ["ok"]);
        assert_eq!((attempts.load(Ordering::Relaxed), started.load(Ordering::Relaxed)), (3, 2));
    }

    #[test]
    fn statuses_are_kept_for_each_document_lately_shown() {
        let ctx = egui::Context::default();
        let (mut state, _) = state(|snapshot| vec![row(snapshot.key.uid, "checking")], |snapshot, _| Some(vec![row(snapshot.key.uid, "ok")]));
        settle(&mut state, &ctx, key(1, 1));
        settle(&mut state, &ctx, key(2, 1));

        let rows = state.rows(&ctx, snapshot(key(1, 1)));

        assert_eq!(statuses(&rows), ["ok"], "going back to a document shows its statuses at once");
        assert!(!state.checking(), "and a check still current isn't made again");
        for uid in 3..=(MAX_DOCUMENTS as u64 + 2) {
            settle(&mut state, &ctx, key(uid, 1));
        }
        assert_eq!(state.checked.len(), MAX_DOCUMENTS);
        assert!(!state.checked.contains_key(&1), "the document checked longest ago goes first");
    }

    #[test]
    fn files_found_unchanged_are_passed_on_only_for_the_document_as_it_was_checked() {
        let ctx = egui::Context::default();
        let (mut state, _) = state(|_| vec![row(1, "checking")], |_, _| None);
        let (_jobs, results) = by_hand(&mut state);
        let found = vec![(AssetId(1), Fingerprint { size: 4, mtime: 9 })];
        state.rows(&ctx, snapshot(key(1, 1)));
        results.send(Done { verified: found.clone(), ..done(1, key(1, 1), vec![row(1, "ok")]) }).unwrap();
        assert_eq!(state.rows(&ctx, snapshot(key(1, 1))).verified, found);
        assert_eq!(state.rows(&ctx, snapshot(key(1, 1))).verified, [], "once");

        // Edited since the check started: other bytes may be linked now.
        state.rows(&ctx, snapshot(key(1, 2)));
        results.send(Done { verified: found, ..done(2, key(1, 2), vec![row(1, "modified")]) }).unwrap();
        let rows = state.rows(&ctx, snapshot(key(1, 3)));
        assert_eq!(rows.verified, []);
        assert_eq!(statuses(&rows), ["modified"], "its statuses are still shown");
    }

    #[test]
    fn what_a_check_remembers_goes_to_the_worker_that_replaces_it() {
        let dir = std::env::temp_dir().join(format!("dc-links-worker-pool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.png");
        std::fs::write(&path, designcraft_render::Rendered { width: 2, height: 2, pixels: vec![200; 16] }.to_png()).unwrap();
        let age = |secs: u64| {
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(std::time::SystemTime::now() - Duration::from_secs(secs)).unwrap();
        };
        // Placed, then given a new time (a restored folder): the check has to compare its bytes.
        age(300);
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.execute("file.place", &json!({"path": path.to_string_lossy()})).unwrap();
        age(60);
        let doc = Arc::clone(&session.doc().unwrap().doc);
        let real = Snapshot { key: Key::new(1, &doc, None), doc, cache: Arc::clone(&session.cache), document: None };
        // Each worker says how much it was handed before checking.
        let (handed_tx, handed) = std::sync::mpsc::channel();
        let handed_tx = Mutex::new(handed_tx);
        let spawn: Spawn = Box::new(move |ctx, live, pool| {
            let handed = handed_tx.lock().unwrap().clone();
            Worker::spawn_with(
                ctx,
                live,
                pool,
                Box::new(move |snapshot, checks, stop| {
                    let _ = handed.send(checks.len());
                    designcraft_engine::cmd::list_document_links(&snapshot.doc, &snapshot.cache, None, checks, stop)
                }),
            )
        });
        let ctx = egui::Context::default();
        let mut state = State { hooks: Some(Hooks { spawn, preview: Box::new(|_| Vec::new()) }), ..Default::default() };
        let check = |state: &mut State| {
            state.rows(&ctx, real.clone());
            for _ in 0..1000 {
                if !state.checking() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
                state.rows(&ctx, real.clone());
            }
        };

        check(&mut state);
        // A replaced worker: as when a check of another document was blocked.
        state.worker = None;
        state.checked.clear();
        check(&mut state);

        assert_eq!(handed.try_iter().collect::<Vec<_>>(), [0, 1], "the second worker starts from what the first compared");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_check_keeps_its_documents_address_from_being_reused() {
        let ctx = egui::Context::default();
        let (mut state, _) = state(|_| Vec::new(), |_, _| Some(Vec::new()));
        let doc = Arc::new(Document::new(&Default::default()));
        let key = Key::new(1, &doc, None);
        let snapshot = || Snapshot { key, doc: Arc::clone(&doc), cache: Arc::new(Cache::new()), document: None };
        state.rows(&ctx, snapshot());
        for _ in 0..1000 {
            if !state.checking() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
            state.rows(&ctx, snapshot());
        }
        assert!(!state.checking(), "the check finished");
        let address = Arc::as_ptr(&doc);

        // Edits replace the document; the old one is dropped once nothing else holds it.
        drop(doc);

        // A weak reference keeps the allocation, so no later document can be given the address
        // in `key` while this check is kept. The worker let go of the document before it
        // answered, so nothing else holds it.
        let pin = state.checked.get(&1).map(|checked| &checked._pin);
        assert!(pin.is_some_and(|pin| pin.as_ptr() == address && pin.strong_count() == 0), "the check doesn't hold its document's allocation");
    }

    #[test]
    fn statuses_go_to_the_same_link_only() {
        let document =
            [row(1, "checking"), json!({"story": 1, "path": "/links/1.png", "status": "checking"}), row(2, "embedded"), row(3, "checking")];
        // Graphic 3 was relinked since: what was found is of another file.
        let checked = [row(1, "modified"), json!({"asset": 3, "path": "/elsewhere/3.png", "status": "ok"}), row(2, "ok")];

        let rows = with_statuses(&document, Some(&checked));

        let status: Vec<_> = rows.iter().map(|row| row["status"].as_str().unwrap()).collect();
        assert_eq!(status, ["modified", "checking", "embedded", "checking"]);
        assert_eq!(with_statuses(&document, None), document);
    }
}
