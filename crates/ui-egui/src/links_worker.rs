//! Native background refreshes for the Links panel.
//!
//! Link status may touch local or network files. Keeping that work off the egui thread means an
//! unavailable share cannot stop the rest of the application from repainting or accepting input.

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Weak};
    use std::time::{Duration, Instant};
    use std::{hash::Hash, hash::Hasher};

    use designcraft_compose::Cache;
    use designcraft_doc::Document;
    use designcraft_engine::cmd::LinkListChecks;
    use serde_json::Value;

    // Document edits change the key and refresh immediately. External changes (a file restored or
    // edited elsewhere) are polled: an unchanged link costs one metadata call, but on a slow share
    // with hundreds of links a check can itself take seconds, so the next one waits at least as
    // long as the last took and the worker is idle at least half the time.
    const REFRESH_MIN: Duration = Duration::from_secs(2);
    const REFRESH_MAX: Duration = Duration::from_secs(5 * 60);
    // A check for another document still running after this long is likely stuck on an unreachable
    // share: the document now shown gets a fresh worker instead of waiting behind it.
    const SUPERSEDE_AFTER: Duration = Duration::from_millis(200);
    // Workers alive at once, counting superseded ones still blocked in a filesystem call. Switching
    // documents while a share is down can't pile up threads beyond this.
    const MAX_WORKERS: usize = 4;

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

    pub(crate) struct Snapshot {
        pub(crate) key: Key,
        pub(crate) doc: Arc<Document>,
        pub(crate) cache: Arc<Cache>,
        pub(crate) document: Option<PathBuf>,
    }

    pub(crate) struct Rows {
        pub(crate) values: Arc<Vec<Value>>,
        pub(crate) checking: bool,
        pub(crate) unavailable: bool,
    }

    struct Job {
        token: u64,
        snapshot: Snapshot,
    }

    struct Done {
        token: u64,
        key: Key,
        /// Keeps the checked document's allocation, so no later document can reuse the address in
        /// `key` and pass for it once this one is dropped.
        pin: Weak<Document>,
        values: Vec<Value>,
        complete: bool,
    }

    enum Poll {
        Done(Done),
        Empty,
        Disconnected,
    }

    type Runner = Box<dyn FnMut(&Snapshot, &mut LinkListChecks) -> Vec<Value> + Send>;
    type Spawn = Box<dyn Fn(egui::Context, Arc<AtomicUsize>) -> Option<Worker> + Send + Sync>;

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
        fn spawn(ctx: egui::Context, live: Arc<AtomicUsize>) -> Option<Self> {
            Self::spawn_with_preview(
                ctx,
                live,
                Some(Box::new(|snapshot, _| {
                    designcraft_engine::cmd::preview_document_links(&snapshot.doc, &snapshot.cache, snapshot.document.as_deref())
                })),
                Box::new(|snapshot, checks| {
                    designcraft_engine::cmd::list_document_links(&snapshot.doc, &snapshot.cache, snapshot.document.as_deref(), checks)
                }),
            )
        }

        #[cfg(test)]
        fn spawn_with(ctx: egui::Context, run: Runner) -> Option<Self> {
            Self::spawn_with_preview(ctx, Arc::default(), None, run)
        }

        fn spawn_with_preview(ctx: egui::Context, live: Arc<AtomicUsize>, mut preview: Option<Runner>, mut run: Runner) -> Option<Self> {
            let (tx, jobs) = std::sync::mpsc::channel::<Job>();
            let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
            live.fetch_add(1, Ordering::Relaxed);
            let alive = Alive { live, ctx: ctx.clone() };
            std::thread::Builder::new()
                .name("designcraft-links".into())
                .spawn(move || {
                    let _alive = alive;
                    let mut checks = LinkListChecks::default();
                    while let Ok(job) = jobs.recv() {
                        if let Some(preview) = &mut preview {
                            let values = preview(&job.snapshot, &mut checks);
                            if done_tx
                                .send(Done {
                                    token: job.token,
                                    key: job.snapshot.key,
                                    pin: Arc::downgrade(&job.snapshot.doc),
                                    values,
                                    complete: false,
                                })
                                .is_err()
                            {
                                break;
                            }
                            ctx.request_repaint();
                        }
                        let values = run(&job.snapshot, &mut checks);
                        if done_tx
                            .send(Done { token: job.token, key: job.snapshot.key, pin: Arc::downgrade(&job.snapshot.doc), values, complete: true })
                            .is_err()
                        {
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

    /// How long completed rows stay current after a check that took `took`.
    fn refresh_after(took: Duration) -> Duration {
        took.clamp(REFRESH_MIN, REFRESH_MAX)
    }

    struct Cached {
        key: Key,
        /// As in [`Done`]: `key` names this document only while its allocation is kept.
        _pin: Weak<Document>,
        refreshed: Instant,
        refresh_after: Duration,
        values: Arc<Vec<Value>>,
        complete: bool,
    }

    #[derive(Clone, Copy)]
    struct InFlight {
        token: u64,
        key: Key,
        started: Instant,
    }

    #[derive(Default)]
    pub(crate) struct State {
        worker: Option<Worker>,
        worker_failed: bool,
        next_token: u64,
        in_flight: Option<InFlight>,
        cached: Option<Cached>,
        live: Arc<AtomicUsize>,
        /// How workers are started; `None` is [`Worker::spawn`].
        spawn: Option<Spawn>,
    }

    impl State {
        pub(crate) fn rows(&mut self, ctx: &egui::Context, snapshot: Snapshot) -> Rows {
            let current_key = snapshot.key;
            match self.worker.as_ref().map(Worker::poll) {
                Some(Poll::Done(done)) => {
                    let mut took = Duration::ZERO;
                    if done.complete
                        && let Some(job) = self.in_flight.filter(|job| job.token == done.token)
                    {
                        took = job.started.elapsed();
                        self.in_flight = None;
                    }
                    let has_completed_rows = self.cached.as_ref().is_some_and(|cached| cached.key.uid == done.key.uid && cached.complete);
                    if done.key == current_key && (done.complete || !has_completed_rows) {
                        self.cached = Some(Cached {
                            key: done.key,
                            _pin: done.pin,
                            refreshed: Instant::now(),
                            refresh_after: refresh_after(took),
                            values: Arc::new(done.values),
                            complete: done.complete,
                        });
                    }
                }
                Some(Poll::Disconnected) => {
                    self.worker = None;
                    self.worker_failed = true;
                    self.in_flight = None;
                }
                Some(Poll::Empty) | None => {}
            }

            // A single worker can't take the next job while a filesystem call for another document
            // is blocked. Leave that worker to finish on its own (its result is dropped with its
            // channel) so the document now shown gets its rows from a fresh one.
            if let Some(job) = self.in_flight.filter(|job| job.key.uid != current_key.uid)
                && self.live.load(Ordering::Relaxed) < MAX_WORKERS
            {
                let elapsed = job.started.elapsed();
                if elapsed >= SUPERSEDE_AFTER {
                    self.worker = None;
                    self.in_flight = None;
                } else {
                    // A blocked worker never asks for a repaint, so come back when it can be replaced.
                    ctx.request_repaint_after(SUPERSEDE_AFTER.saturating_sub(elapsed));
                }
            }
            if self.worker.is_none() && !self.worker_failed {
                let live = Arc::clone(&self.live);
                self.worker = match &self.spawn {
                    Some(spawn) => spawn(ctx.clone(), live),
                    None => Worker::spawn(ctx.clone(), live),
                };
                self.worker_failed = self.worker.is_none();
            }

            let needs_refresh =
                self.cached.as_ref().is_none_or(|cached| cached.key != current_key || cached.refreshed.elapsed() >= cached.refresh_after);
            if needs_refresh && self.in_flight.is_none() && !self.worker_failed {
                self.next_token = self.next_token.wrapping_add(1);
                let token = self.next_token;
                let key = snapshot.key;
                if self.worker.as_ref().is_some_and(|worker| worker.submit(Job { token, snapshot })) {
                    self.in_flight = Some(InFlight { token, key, started: Instant::now() });
                } else {
                    self.worker = None;
                    self.worker_failed = true;
                }
            }

            let values = self
                .cached
                .as_ref()
                .filter(|cached| cached.key.uid == current_key.uid)
                .map_or_else(|| Arc::new(Vec::new()), |cached| Arc::clone(&cached.values));
            let checking = self.in_flight.is_some();
            if !checking
                && !self.worker_failed
                && let Some(cached) = self.cached.as_ref().filter(|cached| cached.key == current_key)
            {
                ctx.request_repaint_after(cached.refresh_after.saturating_sub(cached.refreshed.elapsed()));
            }
            Rows { values, checking, unavailable: self.worker_failed }
        }

        #[cfg(test)]
        pub(crate) fn checking(&self) -> bool {
            self.in_flight.is_some()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn snapshot(key: Key) -> Snapshot {
            Snapshot { key, doc: Arc::new(Document::new(&Default::default())), cache: Arc::new(Cache::new()), document: None }
        }

        fn key(uid: u64, document: usize) -> Key {
            Key { uid, document, path: 0 }
        }

        #[test]
        fn a_blocked_link_check_does_not_block_the_ui_caller() {
            let ctx = egui::Context::default();
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let watchdog = release_tx.clone();
            let _watchdog = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                let _ = watchdog.send(());
            });
            let worker = Worker::spawn_with(
                ctx.clone(),
                Box::new(move |_, _| {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                    Vec::new()
                }),
            )
            .unwrap();
            let mut state = State { worker: Some(worker), ..Default::default() };

            let before = Instant::now();
            let rows = state.rows(&ctx, snapshot(key(1, 1)));
            let elapsed = before.elapsed();
            let started = started_rx.recv_timeout(Duration::from_secs(1)).is_ok();
            let _ = release_tx.send(());

            assert!(rows.checking);
            assert!(started, "the filesystem check starts on the worker");
            assert!(elapsed < Duration::from_millis(500), "the UI call waited {elapsed:?}");
        }

        #[test]
        fn cached_rows_stay_visible_while_a_refresh_is_blocked() {
            let ctx = egui::Context::default();
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let watchdog = release_tx.clone();
            let _watchdog = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                let _ = watchdog.send(());
            });
            let worker = Worker::spawn_with(
                ctx.clone(),
                Box::new(move |_, _| {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                    Vec::new()
                }),
            )
            .unwrap();
            let key = key(1, 1);
            let values = Arc::new(vec![serde_json::json!({"name": "cached"})]);
            let mut state = State {
                worker: Some(worker),
                cached: Some(Cached {
                    key,
                    _pin: Weak::new(),
                    refreshed: Instant::now() - REFRESH_MIN,
                    refresh_after: REFRESH_MIN,
                    values,
                    complete: true,
                }),
                ..Default::default()
            };

            let rows = state.rows(&ctx, snapshot(key));
            let started = started_rx.recv_timeout(Duration::from_secs(1)).is_ok();
            let _ = release_tx.send(());

            assert!(rows.checking);
            assert!(started, "the refresh starts on the worker");
            assert_eq!(rows.values.first().and_then(|value| value["name"].as_str()), Some("cached"));
        }

        #[test]
        fn preview_does_not_replace_completed_rows_during_a_refresh() {
            let ctx = egui::Context::default();
            let (previewed_tx, previewed_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let watchdog = release_tx.clone();
            let _watchdog = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                let _ = watchdog.send(());
            });
            let worker = Worker::spawn_with_preview(
                ctx.clone(),
                Arc::default(),
                Some(Box::new(move |_, _| {
                    let _ = previewed_tx.send(());
                    vec![serde_json::json!({"name": "preview", "status": "checking"})]
                })),
                Box::new(move |_, _| {
                    let _ = release_rx.recv();
                    vec![serde_json::json!({"name": "fresh", "status": "ok"})]
                }),
            )
            .unwrap();
            let key = key(1, 1);
            let values = Arc::new(vec![serde_json::json!({"name": "cached", "status": "ok"})]);
            let mut state = State {
                worker: Some(worker),
                cached: Some(Cached {
                    key,
                    _pin: Weak::new(),
                    refreshed: Instant::now() - REFRESH_MIN,
                    refresh_after: REFRESH_MIN,
                    values,
                    complete: true,
                }),
                ..Default::default()
            };

            assert_eq!(state.rows(&ctx, snapshot(key)).values[0]["name"], "cached");
            assert!(previewed_rx.recv_timeout(Duration::from_secs(1)).is_ok(), "the worker sent its preview");
            let rows = state.rows(&ctx, snapshot(key));
            let _ = release_tx.send(());

            assert!(rows.checking);
            assert_eq!(rows.values[0]["name"], "cached", "a preview must not replace completed status rows");
            assert_eq!(rows.values[0]["status"], "ok");
        }

        #[test]
        fn completed_rows_stay_visible_when_document_content_changes() {
            let ctx = egui::Context::default();
            let (started_tx, started_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let watchdog = release_tx.clone();
            let _watchdog = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(1));
                let _ = watchdog.send(());
            });
            let worker = Worker::spawn_with(
                ctx.clone(),
                Box::new(move |_, _| {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                    Vec::new()
                }),
            )
            .unwrap();
            let values = Arc::new(vec![serde_json::json!({"name": "cached"})]);
            let mut state = State {
                worker: Some(worker),
                cached: Some(Cached {
                    key: key(1, 1),
                    _pin: Weak::new(),
                    refreshed: Instant::now(),
                    refresh_after: REFRESH_MIN,
                    values,
                    complete: true,
                }),
                ..Default::default()
            };

            let rows = state.rows(&ctx, snapshot(key(1, 2)));
            let started = started_rx.recv_timeout(Duration::from_secs(1)).is_ok();
            let _ = release_tx.send(());

            assert!(rows.checking);
            assert!(started, "changed document content starts a refresh");
            assert_eq!(rows.values.first().and_then(|value| value["name"].as_str()), Some("cached"));
        }

        #[test]
        fn a_blocked_check_for_another_document_does_not_gate_the_active_one() {
            let ctx = egui::Context::default();
            let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
            let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
            let watchdog = release_tx.clone();
            let _watchdog = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(2));
                let _ = watchdog.send(());
            });
            let spawn: Spawn = Box::new(move |ctx, live| {
                let release_rx = Arc::clone(&release_rx);
                Worker::spawn_with_preview(
                    ctx,
                    live,
                    None,
                    Box::new(move |snapshot, _| {
                        // Document 1's links are on a share that doesn't answer.
                        if snapshot.key.uid == 1 {
                            let _ = release_rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).recv();
                        }
                        vec![serde_json::json!({"name": format!("doc {}", snapshot.key.uid)})]
                    }),
                )
            });
            let mut state = State { spawn: Some(spawn), ..Default::default() };

            assert!(state.rows(&ctx, snapshot(key(1, 1))).checking);
            std::thread::sleep(SUPERSEDE_AFTER);
            let mut rows = state.rows(&ctx, snapshot(key(2, 1)));
            for _ in 0..500 {
                if !rows.checking {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
                rows = state.rows(&ctx, snapshot(key(2, 1)));
            }
            let _ = release_tx.send(());

            assert!(!rows.checking, "document 2 waited behind document 1's blocked check");
            assert_eq!(rows.values.first().and_then(|value| value["name"].as_str()), Some("doc 2"));
        }

        #[test]
        fn a_switch_before_the_takeover_is_due_schedules_a_repaint_for_it() {
            let ctx = egui::Context::default();
            let delays = Arc::new(std::sync::Mutex::new(Vec::new()));
            let seen = Arc::clone(&delays);
            ctx.set_request_repaint_callback(move |info| seen.lock().unwrap().push(info.delay));
            let worker = Worker::spawn_with(ctx.clone(), Box::new(|_, _| Vec::new())).unwrap();
            let mut state =
                State { worker: Some(worker), in_flight: Some(InFlight { token: 7, key: key(1, 1), started: Instant::now() }), ..Default::default() };

            let rows = state.rows(&ctx, snapshot(key(2, 1)));

            assert!(rows.checking);
            assert_eq!(state.in_flight.map(|job| job.token), Some(7), "not replaced before it is due");
            let delays = delays.lock().unwrap();
            assert!(delays.iter().any(|delay| *delay <= SUPERSEDE_AFTER), "no repaint scheduled for the takeover: {delays:?}");
        }

        #[test]
        fn external_changes_are_polled_within_seconds_and_slow_checks_space_out() {
            assert_eq!(refresh_after(Duration::from_millis(30)), REFRESH_MIN, "a quick check is repeated after two seconds");
            assert_eq!(refresh_after(Duration::from_secs(4)), Duration::from_secs(4), "a slow share is idle as long as it was busy");
            assert_eq!(refresh_after(Duration::from_secs(3600)), REFRESH_MAX);
        }

        #[test]
        fn completed_rows_are_checked_again_after_the_refresh_interval() {
            let ctx = egui::Context::default();
            let worker = Worker::spawn_with(ctx.clone(), Box::new(|_, _| Vec::new())).unwrap();
            let key = key(1, 1);
            let values = Arc::new(vec![serde_json::json!({"name": "cached"})]);
            let fresh =
                |refreshed| Cached { key, _pin: Weak::new(), refreshed, refresh_after: REFRESH_MIN, values: Arc::clone(&values), complete: true };
            let mut state = State { worker: Some(worker), cached: Some(fresh(Instant::now())), ..Default::default() };
            assert!(!state.rows(&ctx, snapshot(key)).checking, "rows just checked are current");

            state.cached = Some(fresh(Instant::now() - REFRESH_MIN));
            assert!(state.rows(&ctx, snapshot(key)).checking, "an external change shows within the interval");
        }

        #[test]
        fn a_superseded_worker_that_ends_wakes_the_document_waiting_at_the_limit() {
            let ctx = egui::Context::default();
            let repaints = Arc::new(std::sync::Mutex::new(0_usize));
            let seen = Arc::clone(&repaints);
            ctx.set_request_repaint_callback(move |_| *seen.lock().unwrap() += 1);
            // Documents 1 and 2 are on shares that don't answer until released.
            let (release_1, blocked_1) = std::sync::mpsc::channel::<()>();
            let (release_2, blocked_2) = std::sync::mpsc::channel::<()>();
            let blocked = Arc::new(std::sync::Mutex::new((blocked_1, blocked_2)));
            for release in [release_1.clone(), release_2.clone()] {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(2));
                    let _ = release.send(());
                });
            }
            let spawn: Spawn = Box::new(move |ctx, live| {
                let blocked = Arc::clone(&blocked);
                Worker::spawn_with_preview(
                    ctx,
                    live,
                    None,
                    Box::new(move |snapshot, _| {
                        let wait = |rx: &std::sync::mpsc::Receiver<()>| {
                            let _ = rx.recv();
                        };
                        match snapshot.key.uid {
                            1 => wait(&blocked.lock().unwrap().0),
                            2 => wait(&blocked.lock().unwrap().1),
                            _ => {}
                        }
                        vec![serde_json::json!({"name": format!("doc {}", snapshot.key.uid)})]
                    }),
                )
            });
            // Two slots left: document 1's worker, then document 2's, which supersedes it.
            let mut state = State { spawn: Some(spawn), live: Arc::new(AtomicUsize::new(MAX_WORKERS - 2)), ..Default::default() };
            state.rows(&ctx, snapshot(key(1, 1)));
            std::thread::sleep(SUPERSEDE_AFTER);
            state.rows(&ctx, snapshot(key(2, 1)));
            std::thread::sleep(SUPERSEDE_AFTER);
            assert!(state.rows(&ctx, snapshot(key(3, 1))).checking, "document 3 waits at the limit");
            assert_eq!(state.live.load(Ordering::Relaxed), MAX_WORKERS);

            *repaints.lock().unwrap() = 0;
            let _ = release_1.send(());
            let woken = (0..500).any(|_| {
                std::thread::sleep(Duration::from_millis(2));
                *repaints.lock().unwrap() > 0 && state.live.load(Ordering::Relaxed) < MAX_WORKERS
            });
            let mut rows = state.rows(&ctx, snapshot(key(3, 1)));
            for _ in 0..500 {
                if !rows.checking {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
                rows = state.rows(&ctx, snapshot(key(3, 1)));
            }
            let _ = release_2.send(());

            assert!(woken, "the ended worker freed its slot and asked for a repaint");
            assert!(!rows.checking, "document 3 got a worker once a slot was free");
            assert_eq!(rows.values.first().and_then(|value| value["name"].as_str()), Some("doc 3"));
        }

        #[test]
        fn completed_rows_keep_their_documents_address_from_being_reused() {
            let ctx = egui::Context::default();
            let worker = Worker::spawn_with(ctx.clone(), Box::new(|_, _| Vec::new())).unwrap();
            let mut state = State { worker: Some(worker), ..Default::default() };
            let doc = Arc::new(Document::new(&Default::default()));
            let key = Key::new(1, &doc, None);
            let snapshot = || Snapshot { key, doc: Arc::clone(&doc), cache: Arc::new(Cache::new()), document: None };
            let mut rows = state.rows(&ctx, snapshot());
            for _ in 0..500 {
                if !rows.checking {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
                rows = state.rows(&ctx, snapshot());
            }
            assert!(!rows.checking, "the check finished");
            let address = Arc::as_ptr(&doc);

            // Edits replace the document; the old one is dropped once nothing else holds it.
            drop(doc);

            // A weak reference keeps the allocation, so no later document can be given the address
            // in `key` while these rows are kept.
            let pin = state.cached.as_ref().map(|cached| &cached._pin);
            assert!(pin.is_some_and(|pin| pin.as_ptr() == address && pin.strong_count() == 0), "the rows don't hold their document's allocation");
        }

        #[test]
        fn superseded_workers_are_bounded() {
            let ctx = egui::Context::default();
            let worker = Worker::spawn_with(ctx.clone(), Box::new(|_, _| Vec::new())).unwrap();
            let mut state = State {
                worker: Some(worker),
                in_flight: Some(InFlight { token: 7, key: key(1, 1), started: Instant::now() - SUPERSEDE_AFTER }),
                live: Arc::new(AtomicUsize::new(MAX_WORKERS)),
                ..Default::default()
            };

            let rows = state.rows(&ctx, snapshot(key(2, 1)));

            assert!(rows.checking);
            assert_eq!(state.in_flight.map(|job| job.token), Some(7), "at the limit the active document waits instead");
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{Key, Snapshot, State};

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub(crate) struct State;
