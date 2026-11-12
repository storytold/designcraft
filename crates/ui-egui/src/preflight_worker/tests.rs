use std::sync::Arc;

use egui::vec2;
use serde_json::json;

use super::Checked;
use crate::test_window;

/// A document whose one text frame is too small for its text (one preflight error), with the
/// frame's id.
fn overset_app() -> (crate::DesignApp, u64) {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 3})).unwrap();
    let text = "Words that do not fit the frame. ".repeat(40);
    let r = app.run("frame.create", json!({"rect": [72, 72, 200, 100], "content": "text", "text": text})).unwrap();
    app.run("edit.deselectAll", json!({})).unwrap();
    (app, r["id"].as_u64().unwrap())
}

/// The active document's current state.
fn current(app: &crate::DesignApp) -> Checked {
    Checked::new(app).unwrap().0
}

/// The errors in the issues last found for the active document, and whether they are of its
/// current state.
fn found(app: &crate::DesignApp) -> Option<(usize, bool)> {
    let now = current(app);
    let f = app.preflight.found.iter().find(|f| f.of.uid == now.uid)?;
    Some((f.issues.iter().filter(|i| i.severity == "error").count(), f.of.same(&now)))
}

/// A worker whose thread never takes a job: the test reads the queue.
fn stalled_worker(app: &mut crate::DesignApp) -> (std::sync::mpsc::Receiver<super::Job>, std::sync::mpsc::Sender<super::Found>) {
    let (tx, jobs) = std::sync::mpsc::sync_channel(1);
    let (done, rx) = std::sync::mpsc::channel();
    app.preflight.worker.channels = Some((tx, rx));
    (jobs, done)
}

/// Frames with the status bar and the Preflight panel showing check each state of the document
/// once, however many frames draw it.
#[test]
fn each_document_state_is_checked_once() {
    let (mut app, id) = overset_app();
    app.ui.dock_tab = "properties".into();
    let mut h = test_window::open(app, vec2(1200.0, 700.0));
    h.state_mut().app.run("window.panel", json!({"panel": "preflight"})).unwrap();
    h.run_steps(20);
    assert_eq!(h.state().app.preflight.inline_checks, 1);
    assert_eq!(found(&h.state().app), Some((1, true)));
    for k in 0..5 {
        let dx = if k % 2 == 0 { 1.0 } else { -1.0 };
        h.state_mut().app.run("transform.move", json!({"ids": [id], "dx": dx, "dy": 0.0})).unwrap();
        h.run_steps(10);
    }
    assert_eq!(h.state().app.preflight.inline_checks, 6, "one check per edit");
    assert_eq!(found(&h.state().app), Some((1, true)));
}

/// Saving under a new path changes where relative links resolve, and a font reload replaces the
/// compositions and bumps the revision: either way the document is checked again.
#[test]
fn a_new_save_path_or_font_reload_is_checked_again() {
    let (mut app, _) = overset_app();
    let ctx = egui::Context::default();
    assert!(super::issues(&mut app, &ctx).is_some());
    assert!(super::issues(&mut app, &ctx).is_some());
    assert_eq!(app.preflight.inline_checks, 1);
    let dir = tempfile::tempdir().unwrap();
    app.run("file.saveAs", json!({"path": dir.path().join("a.designcraft").to_string_lossy()})).unwrap();
    assert!(super::issues(&mut app, &ctx).is_some());
    assert_eq!(app.preflight.inline_checks, 2, "saved under a path");
    app.session.cache = Arc::new(designcraft_compose::Cache::new());
    assert!(super::issues(&mut app, &ctx).is_some());
    assert_eq!(app.preflight.inline_checks, 3, "new compositions");
    if let Some(st) = app.session.active_mut() {
        st.bump_revision();
    }
    assert!(super::issues(&mut app, &ctx).is_some());
    assert_eq!(app.preflight.inline_checks, 4, "a new revision");
    assert_eq!(found(&app), Some((1, true)));
}

/// States asked for while the worker is busy don't queue up: one job waits in the worker's queue
/// and one beside it, the newest state, which replaces the one before.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_busy_worker_keeps_one_waiting_job() {
    let (mut app, id) = overset_app();
    let ctx = egui::Context::default();
    let (jobs, _done) = stalled_worker(&mut app);
    for k in 0..20 {
        let dx = if k % 2 == 0 { 1.0 } else { -1.0 };
        app.run("transform.move", json!({"ids": [id], "dx": dx, "dy": 0.0})).unwrap();
        assert!(super::issues(&mut app, &ctx).is_none(), "nothing found yet");
    }
    assert_eq!(app.preflight.inline_checks, 0);
    let now = current(&app);
    let waiting = app.preflight.worker.waiting.as_ref().expect("the newest state waits");
    assert!(waiting.of.same(&now));
    let queued = jobs.try_recv().expect("the first state is queued");
    assert!(!queued.of.same(&now));
    assert!(jobs.try_recv().is_err(), "nothing else is queued");
}

/// An unchanged document is checked again once its result is older than the refresh interval
/// (linked files can change outside the app), and not before.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_unchanged_document_is_checked_again_after_the_refresh_interval() {
    let (mut app, _) = overset_app();
    let ctx = egui::Context::default();
    let (jobs, done) = stalled_worker(&mut app);
    assert!(super::issues_at(&mut app, &ctx, 10.0).is_none());
    let job = jobs.try_recv().expect("the state is queued");
    done.send(super::Found { of: job.of, issues: Arc::new(vec![]), took: std::time::Duration::ZERO, asked_at: 0.0 }).unwrap();
    assert!(super::issues_at(&mut app, &ctx, 11.0).is_some(), "the worker's result");
    let refresh = super::REFRESH_MIN.as_secs_f64();
    assert!(super::issues_at(&mut app, &ctx, 11.0 + refresh - 0.5).is_some());
    assert!(jobs.try_recv().is_err(), "not checked again yet");
    assert!(super::issues_at(&mut app, &ctx, 11.0 + refresh).is_some(), "the last issues stay shown");
    assert!(jobs.try_recv().expect("checked again").of.same(&current(&app)));
    assert!(super::issues_at(&mut app, &ctx, 11.0 + refresh + 0.5).is_some());
    assert!(jobs.try_recv().is_err(), "asked once");
    // A check that outlasts the refresh interval isn't asked for again while it runs.
    assert!(super::issues_at(&mut app, &ctx, 11.0 + 3.0 * refresh).is_some());
    assert!(jobs.try_recv().is_err(), "the check of this state is still running");
    assert_eq!(app.preflight.inline_checks, 0);
}

/// Issues shown before they are due to be checked again ask for a frame at that time, so the check
/// runs even when nothing else wakes the interface.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shown_issues_ask_for_a_frame_when_they_are_due_again() {
    let (mut app, _) = overset_app();
    let ctx = egui::Context::default();
    let (jobs, done) = stalled_worker(&mut app);
    assert!(super::issues_at(&mut app, &ctx, 10.0).is_none());
    let job = jobs.try_recv().expect("the state is queued");
    done.send(super::Found { of: job.of, issues: Arc::new(vec![]), took: std::time::Duration::ZERO, asked_at: 0.0 }).unwrap();
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    }
    assert!(!ctx.has_requested_repaint());
    assert!(super::issues_at(&mut app, &ctx, 11.0).is_some());
    assert!(ctx.has_requested_repaint());
}

/// A worker that ends leaves checks to the interface thread, and a new one is tried after
/// `RETRY_AFTER`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_worker_that_ends_is_tried_again_later() {
    let (mut app, _) = overset_app();
    let ctx = egui::Context::default();
    let (_jobs, done) = stalled_worker(&mut app);
    drop(done);
    assert_eq!(super::issues(&mut app, &ctx).map(|i| i.iter().filter(|i| i.severity == "error").count()), Some(1));
    assert_eq!(app.preflight.inline_checks, 1, "checked on the interface thread");
    assert!(app.preflight.worker.channels.is_none() && !app.preflight.worker.retry_due());
    // The shown issues ask for a frame when a new worker may be tried.
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    }
    assert!(!ctx.has_requested_repaint());
    assert!(super::issues(&mut app, &ctx).is_some());
    assert!(ctx.has_requested_repaint());
    assert_eq!(app.preflight.inline_checks, 1);
    let worker = &mut app.preflight.worker;
    worker.failed = std::time::Instant::now().checked_sub(super::RETRY_AFTER);
    assert!(worker.retry_due());
    assert_eq!(worker.retry_wait(), Some(std::time::Duration::ZERO));
}

/// A check of another document that has been running longer than `SUPERSEDE_AFTER` (a file
/// on a share that doesn't answer) doesn't hold up the active document: a fresh worker takes it.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_blocked_check_of_another_document_is_superseded() {
    let (mut app, _) = overset_app();
    let ctx = egui::Context::default();
    let (jobs, _done) = stalled_worker(&mut app);
    let mut other = current(&app);
    other.uid += 1;
    let long_ago = std::time::Instant::now().checked_sub(super::SUPERSEDE_AFTER * 2).unwrap();
    let in_flight = |app: &crate::DesignApp| app.preflight.worker.in_flight.iter().map(|(of, _)| of.same(&current(app))).collect::<Vec<_>>();
    // Asked while the other document's check had only just started: the active document's job is
    // queued behind it.
    app.preflight.worker.in_flight.push_back((other.clone(), std::time::Instant::now()));
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    }
    assert!(!ctx.has_requested_repaint());
    assert!(super::issues(&mut app, &ctx).is_none());
    assert!(ctx.has_requested_repaint(), "a frame comes back to see whether it is blocked");
    assert!(jobs.try_recv().is_ok_and(|job| job.of.same(&current(&app))), "queued behind the other document's check");
    assert_eq!(in_flight(&app), vec![false, true]);
    // A later frame, once that check has been running too long, moves the job to a fresh worker.
    app.preflight.worker.in_flight.front_mut().unwrap().1 = long_ago;
    assert!(super::issues(&mut app, &ctx).is_none());
    assert!(jobs.try_recv().is_err(), "the blocked worker gets nothing more");
    assert_eq!(in_flight(&app), vec![true], "the fresh worker has the active document");
    // The same document's own earlier state waits for its worker instead.
    let (jobs, _done) = stalled_worker(&mut app);
    let mut earlier = current(&app);
    earlier.revision += 1;
    app.preflight.worker.in_flight = [(earlier, long_ago)].into();
    app.preflight.worker.asked = Checked::default();
    assert!(super::issues(&mut app, &ctx).is_none());
    assert!(jobs.try_recv().is_ok(), "queued on the same worker");
}

/// A worker checking an older state of the document leaves the interface thread's composition of
/// the current state in its cache.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_check_of_an_older_state_keeps_the_current_compositions() {
    let (mut app, id) = overset_app();
    let (older, _) = Checked::new(&app).unwrap();
    let older_doc = Arc::clone(&app.session.active().unwrap().doc);
    app.run("transform.move", json!({"ids": [id], "dx": 1.0, "dy": 0.0})).unwrap();
    let doc = Arc::clone(&app.session.active().unwrap().doc);
    let sid = *doc.stories.keys().next().unwrap();
    let cache = Arc::clone(&app.session.cache);
    let current = cache.get(&doc, sid, None);
    let mut own = None;
    let found = super::check(&mut own, super::Job { of: older, doc: Arc::clone(&older_doc), cache: Arc::clone(&cache) });
    assert_eq!(found.issues.iter().filter(|i| i.severity == "error").count(), 1);
    assert!(Arc::ptr_eq(&cache.get(&doc, sid, None), &current), "the current state's composition is kept");
    // The worker keeps its compositions for the next check of the document over the same cache.
    let earlier = own.as_ref().unwrap().1.get(&older_doc, sid, None);
    let (of, doc) = Checked::new(&app).unwrap();
    super::check(&mut own, super::Job { of, doc, cache: Arc::clone(&cache) });
    assert!(Arc::ptr_eq(&own.as_ref().unwrap().1.get(&older_doc, sid, None), &earlier), "kept for the next check");
    let reloaded = Arc::new(designcraft_compose::Cache::new());
    let (of, doc) = Checked::new(&app).unwrap();
    super::check(&mut own, super::Job { of, doc, cache: Arc::clone(&reloaded) });
    assert!(own.as_ref().unwrap().1.base().is_some_and(|b| Arc::ptr_eq(b, &reloaded)), "a new cache after a font reload");
}

/// Closing the last document drops the job waiting for a busy worker, which holds that document.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn closing_the_last_document_drops_the_waiting_job() {
    let (mut app, id) = overset_app();
    let ctx = egui::Context::default();
    let (_jobs, _done) = stalled_worker(&mut app);
    for dx in [1.0, -1.0] {
        app.run("transform.move", json!({"ids": [id], "dx": dx, "dy": 0.0})).unwrap();
        assert!(super::issues(&mut app, &ctx).is_none());
    }
    assert!(app.preflight.worker.waiting.is_some());
    app.run("file.close", json!({})).unwrap();
    assert!(app.session.active().is_none());
    assert!(super::issues(&mut app, &ctx).is_none());
    assert!(app.preflight.worker.waiting.is_none(), "the waiting job is dropped");
    assert!(app.preflight.found.is_empty());
}

/// A result for another document keeps the active document's issues, and the issues of at most
/// `MAX_DOCUMENTS` documents are kept.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn another_documents_result_keeps_the_active_documents_issues() {
    let (mut app, _) = overset_app();
    let ctx = egui::Context::default();
    let errors = |app: &mut crate::DesignApp| super::issues(app, &ctx).map(|i| i.iter().filter(|i| i.severity == "error").count());
    assert_eq!(errors(&mut app), Some(1));
    let (_jobs, done) = stalled_worker(&mut app);
    for k in 1..=super::MAX_DOCUMENTS as u64 + 2 {
        let mut other = current(&app);
        other.uid += k;
        done.send(super::Found { of: other, issues: Arc::new(vec![]), took: std::time::Duration::ZERO, asked_at: 0.0 }).unwrap();
        assert_eq!(errors(&mut app), Some(1), "the active document's issues");
    }
    assert_eq!(app.preflight.found.len(), super::MAX_DOCUMENTS);
    assert_eq!(app.preflight.inline_checks, 1);
}

/// With the worker running, edits and the frames that draw them run no check on the interface
/// thread. The count last found stays shown until the worker has checked the current state.
#[test]
fn the_worker_checks_and_the_interface_thread_does_not() {
    let (app, id) = overset_app();
    let mut h = test_window::open(app, vec2(1200.0, 700.0));
    let ctx = h.ctx.clone();
    assert_eq!(found(&h.state().app), Some((1, true)), "checked on the interface thread before the worker starts");
    super::start_worker(&mut h.state_mut().app, &ctx);
    h.state_mut().app.preflight.inline_checks = 0;
    for k in 0..20 {
        let dx = if k % 2 == 0 { 1.0 } else { -1.0 };
        h.state_mut().app.run("transform.move", json!({"ids": [id], "dx": dx, "dy": 0.0})).unwrap();
        h.run_steps(1);
        assert_eq!(found(&h.state().app).map(|f| f.0), Some(1), "the count last found stays shown");
    }
    assert_eq!(h.state().app.preflight.inline_checks, 0, "no check ran on the interface thread");
    // The last frame handed the current state to the worker, whose last answer is for it.
    while found(&h.state().app) != Some((1, true)) {
        super::wait_for_worker(&mut h.state_mut().app);
    }
    let issues = super::issues(&mut h.state_mut().app, &ctx).unwrap();
    assert_eq!(issues.iter().filter(|i| i.severity == "error").count(), 1);
    assert_eq!(h.state().app.preflight.inline_checks, 0, "the worker's answer is kept, not checked again");
}
