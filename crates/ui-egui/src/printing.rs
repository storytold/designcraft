//! Desktop printing owns a document snapshot. Neither preparing the PDF nor waiting for the
//! system spooler may keep the window's event loop from drawing or handling input.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use designcraft_compose::Cache;
use designcraft_engine::{DocState, Session, file_access};
use serde_json::{Value, json};

use crate::DesignApp;

type PrintResult = Result<Value, String>;
pub(crate) type PrintFn = Box<dyn FnOnce(DocState, Arc<Cache>, Value) -> PrintResult + Send>;

/// No join handle is retained: dropping a window must not block on an unresponsive driver.
/// The worker retains the snapshot (including document fonts) until it has finished.
pub(crate) struct Pending {
    result: Receiver<PrintResult>,
    pub(crate) title: String,
}

impl Pending {
    /// Run one immutable print snapshot with the caller's filesystem authority, waking the
    /// window when a terminal result arrives. The UI retains a receiver, never a join wait.
    fn spawn(state: DocState, cache: Arc<Cache>, params: Value, wake: Option<egui::Context>, work: PrintFn) -> Result<Self, String> {
        let title = state.doc.title.clone();
        let (tx, result) = std::sync::mpsc::channel();
        // Queued UI automation can carry thread-local read/write roots too. A new thread
        // must neither drop that authority boundary nor re-read different process defaults.
        let roots = file_access::current();
        std::thread::Builder::new()
            .name("designcraft-print".into())
            .spawn(move || {
                let run = || work(state, cache, params);
                let done = match roots {
                    Some(roots) => file_access::confine(Some(&roots), run),
                    None => file_access::unconfined(run),
                };
                let _ = tx.send(done);
                if let Some(ctx) = wake {
                    ctx.request_repaint();
                }
            })
            .map_err(|e| format!("couldn't start the print worker: {e}"))?;
        Ok(Self { result, title })
    }
}

/// Give the worker its own session while retaining the selected document, fonts and composition
/// cache. The existing engine command prepares and submits the job without touching live UI state.
fn print(state: DocState, cache: Arc<Cache>, params: Value) -> PrintResult {
    let mut session = Session::new();
    session.cache = cache;
    session.add_document(state);
    session.execute("file.print", &params).map_err(|e| e.to_string())
}

impl DesignApp {
    /// Reject cheap option errors before dismissing the dialog, then queue PDF preparation and
    /// native submission from the current document snapshot. Only one job is admitted at a time.
    pub(crate) fn start_print(&mut self, params: Value) -> PrintResult {
        if self.pending_print.is_some() {
            return Err(crate::i18n::tr(&self.ui.language, "Another print job is still running.").into());
        }
        self.session.check_print_options(&params).map_err(|e| e.to_string())?;
        let state = self.session.doc().map_err(|e| e.to_string())?.clone();
        let cache = self.session.cache.clone();
        let work: PrintFn = Box::new(print);
        #[cfg(test)]
        let work = self.test_print.take().unwrap_or(work);
        self.pending_print = Some(Pending::spawn(state, cache, params, self.egui_ctx.clone(), work)?);
        self.status(crate::i18n::tr(&self.ui.language, "Printing…"));
        Ok(json!({"pending": true}))
    }

    /// A nonblocking poll, also run on hidden logic ticks. Completion is reported only once
    /// the native print operation has returned, never when its background thread starts.
    pub(crate) fn poll_print(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_print else { return };
        let result = match pending.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                // A context wasn't necessarily available when an automation queued the action.
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the print worker stopped without returning a result".into()),
        };
        self.pending_print = None;
        match result {
            Ok(r) => self.print_completed(&r),
            Err(e) => {
                if !self.quit_without_asking {
                    // Cancel both deferred Quit and a native Close received in this tick.
                    // Consuming the event keeps handle_quit from treating it as a fresh close
                    // (or opening an unsaved-changes dialog) after the error is reported.
                    self.close_requested_pending = false;
                    self.quit_requested = false;
                    let cancel_close = ctx.input_mut(|input| {
                        let viewport = input.raw.viewport_id;
                        input.raw.viewports.get_mut(&viewport).is_some_and(|info| {
                            let closing = info.close_requested();
                            info.events.retain(|event| !matches!(event, egui::ViewportEvent::Close));
                            closing
                        })
                    });
                    if cancel_close {
                        // eframe captured the original raw event before running app logic.
                        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    }
                }
                self.status_error("Print", e);
            }
        }
    }

    /// Report the engine's terminal spooler result, not merely that the worker has started.
    /// Acceptance by the system spooler does not claim that paper has left a physical printer.
    pub(crate) fn print_completed(&mut self, result: &Value) {
        let printer = result["printer"].as_str().unwrap_or_else(|| crate::i18n::tr(&self.ui.language, "the default printer"));
        self.status(
            crate::i18n::tr(&self.ui.language, "Sent {count} page(s) to {printer}")
                .replace("{count}", &result["pages"].to_string())
                .replace("{printer}", printer),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use std::sync::mpsc;
    use std::time::Duration;

    /// A clean one-page document keeps close tests independent of unsaved-change prompts.
    fn app() -> DesignApp {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"title":"Printed snapshot"})).unwrap();
        app
    }

    /// Valid print options name an unavailable queue as a second guard behind the fake driver.
    fn synthetic_params() -> Value {
        // An explicit nonexistent queue cannot fall back to the physical default printer,
        // even if asynchronous dispatch or the fake worker hook accidentally regresses.
        json!({"printer":"DesignCraft synthetic regression queue (nonexistent)"})
    }

    /// Wait only in the test, then let the production nonblocking poll process the result.
    fn finish(app: &mut DesignApp) {
        let result = app.pending_print.as_ref().unwrap().result.recv_timeout(Duration::from_secs(10)).unwrap();
        // Put the received result back so the production polling path handles it.
        let (tx, rx) = mpsc::channel();
        tx.send(result).unwrap();
        app.pending_print.as_mut().unwrap().result = rx;
        app.poll_print(&egui::Context::default());
    }

    /// Drive one quit-handling frame, optionally carrying the native window close event.
    fn quit_frame(app: &mut DesignApp, ctx: &egui::Context, close: bool) -> Vec<egui::ViewportCommand> {
        let mut raw = egui::RawInput::default();
        if close {
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        }
        let mut out = ctx.run_ui(raw, |ui| app.handle_quit(ui.ctx()));
        out.textures_delta.clear();
        out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    }

    /// Run the actual logic-tick ordering: collect the print result before handling native Close.
    fn logic_frame(app: &mut DesignApp, ctx: &egui::Context, close: bool) -> Vec<egui::ViewportCommand> {
        let mut raw = egui::RawInput::default();
        if close {
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        }
        let mut out = ctx.run_ui(raw, |ui| app.logic(ui.ctx()));
        out.textures_delta.clear();
        out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    }

    /// A held fake driver leaves editing and tab closure responsive and prints the original snapshot.
    #[test]
    fn desktop_print_returns_before_the_driver_and_keeps_the_snapshot() {
        let mut app = app();
        let (entered, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        app.test_print = Some(Box::new(move |state, _, _| {
            entered.send(state.doc.clone()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(json!({"printer":"Synthetic", "pages":1}))
        }));
        let result = app.run("file.print", synthetic_params()).unwrap();
        assert_eq!(result["pending"], true);
        let snapshot = started.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(Arc::ptr_eq(&snapshot, &app.session.doc().unwrap().doc));
        assert_eq!(app.ui.status, "Printing…");
        assert!(!app.ui.status.contains("Sent"));

        app.run("file.new", json!({"title":"Another document"})).unwrap();
        app.run("file.close", json!({"index":0})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.title, "Another document");
        assert_eq!(snapshot.title, "Printed snapshot");
        app.logic(&egui::Context::default());
        assert!(app.pending_print.is_some(), "a logic tick must not wait for the driver");
        assert!(app.run("file.print", synthetic_params()).is_err());

        release.send(()).unwrap();
        finish(&mut app);
        assert!(app.pending_print.is_none());
        assert_eq!(app.ui.status, "Sent 1 page(s) to Synthetic");
        assert_eq!(app.session.doc().unwrap().doc.title, "Another document");
    }

    /// A terminal driver failure replaces the busy indicator without claiming submission success.
    #[test]
    fn print_worker_failure_is_reported_without_a_success_message() {
        let mut app = app();
        app.test_print = Some(Box::new(|_, _, _| Err("synthetic driver failure".into())));
        app.run("file.print", synthetic_params()).unwrap();
        finish(&mut app);
        assert_eq!(app.ui.status, "Print: synthetic driver failure");
        assert!(app.pending_print.is_none());
    }

    /// An abandoned result channel clears the busy state and reports a terminal worker error.
    #[test]
    fn a_disconnected_worker_is_reported_instead_of_staying_busy() {
        let mut app = app();
        let (tx, result) = mpsc::channel();
        drop(tx);
        app.pending_print = Some(Pending { result, title: "Snapshot".into() });
        app.poll_print(&egui::Context::default());
        assert!(app.pending_print.is_none());
        assert!(app.ui.status.contains("stopped without returning a result"));
    }

    /// Script and dry-run calls retain their established synchronous result and validation behavior.
    #[test]
    fn control_and_dry_run_keep_the_synchronous_engine_result() {
        let mut app = app();
        let params = json!({"dryRun":true});
        assert_eq!(app.run("file.print", params.clone()).unwrap()["pages"], 1);
        app.in_control = true;
        assert_eq!(app.run("file.print", params).unwrap()["pages"], 1);
        assert!(app.pending_print.is_none());
        assert!(app.run("file.print", json!({"pages":"2"})).is_err());
        assert!(app.pending_print.is_none());
    }

    /// Dropping app state never joins a native worker that may be waiting on its driver.
    #[test]
    fn dropping_the_window_does_not_wait_for_the_print_worker() {
        let mut app = app();
        let (release, wait) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        app.test_print = Some(Box::new(move |_, _, _| {
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            finished.send(()).unwrap();
            Ok(Value::Null)
        }));
        app.run("file.print", synthetic_params()).unwrap();
        drop(app);
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(10)).unwrap();
    }

    /// Ordinary menu Quit and native Close wait through a running job and resume after success.
    #[test]
    fn normal_quit_and_window_close_wait_responsively_for_the_print_job() {
        for window_close in [false, true] {
            let mut app = app();
            let ctx = egui::Context::default();
            let (release, wait) = mpsc::channel();
            app.test_print = Some(Box::new(move |_, _, _| {
                wait.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(json!({"pages":1}))
            }));
            app.run("file.print", synthetic_params()).unwrap();
            if !window_close {
                app.request_quit();
            }
            let commands = quit_frame(&mut app, &ctx, window_close);
            assert!(commands.contains(&egui::ViewportCommand::CancelClose));
            assert!(!commands.contains(&egui::ViewportCommand::Close));
            assert!(app.close_requested_pending);
            assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::CancelClose));
            assert_eq!(app.session.doc().unwrap().doc.title, "Printed snapshot");

            release.send(()).unwrap();
            finish(&mut app);
            let commands = quit_frame(&mut app, &ctx, false);
            assert!(commands.contains(&egui::ViewportCommand::Close));
            assert!(!commands.contains(&egui::ViewportCommand::CancelClose));
            assert!(!app.close_requested_pending);
        }
    }

    /// A resumed close still protects unsaved changes, while explicit forced exit stays available.
    #[test]
    fn deferred_quit_still_asks_about_unsaved_changes_and_forced_quit_is_unchanged() {
        let mut app = app();
        let ctx = egui::Context::default();
        let (release, wait) = mpsc::channel();
        app.test_print = Some(Box::new(move |_, _, _| {
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(json!({"pages":1}))
        }));
        app.run("file.print", synthetic_params()).unwrap();
        app.run("frame.create", json!({"rect":[36,36,120,120]})).unwrap();
        assert!(app.session.doc().unwrap().is_dirty());
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        release.send(()).unwrap();
        finish(&mut app);
        assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "closeDocument");
        assert!(app.session.doc().unwrap().is_dirty());

        let (tx, result) = mpsc::channel();
        app.pending_print = Some(Pending { result, title: "Snapshot".into() });
        app.quit_without_asking = true;
        assert!(!quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        drop(tx);
    }

    /// Failure cancels an earlier deferred Quit instead of hiding the error behind automatic exit.
    #[test]
    fn a_failed_job_cancels_deferred_quit_so_its_error_remains_visible() {
        let mut app = app();
        let ctx = egui::Context::default();
        let (release, wait) = mpsc::channel();
        app.test_print = Some(Box::new(move |_, _, _| {
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            Err("synthetic driver failure".into())
        }));
        app.run("file.print", synthetic_params()).unwrap();
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        release.send(()).unwrap();
        finish(&mut app);
        assert!(!app.close_requested_pending);
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.ui.status, "Print: synthetic driver failure");
    }

    /// A Close arriving in the failure's own logic tick is cancelled once, never replayed later.
    #[test]
    fn a_print_failure_and_native_close_in_the_same_tick_keep_the_error_visible() {
        for dirty in [false, true] {
            let mut app = app();
            let ctx = egui::Context::default();
            if dirty {
                app.run("frame.create", json!({"rect":[36,36,120,120]})).unwrap();
            }
            let (tx, result) = mpsc::channel();
            tx.send(Err("synthetic driver failure".into())).unwrap();
            app.pending_print = Some(Pending { result, title: "Snapshot".into() });
            assert_eq!(app.session.doc().unwrap().is_dirty(), dirty);

            let commands = logic_frame(&mut app, &ctx, true);
            assert!(commands.contains(&egui::ViewportCommand::CancelClose), "the current native close must not hide the failure: {commands:?}");
            assert!(!commands.contains(&egui::ViewportCommand::Close));
            assert!(app.pending_print.is_none());
            assert!(!app.quit_requested && !app.close_requested_pending);
            assert!(app.ui.dialog.is_none(), "the failed print cancels this close without an unsaved-changes prompt");
            assert_eq!(app.ui.status, "Print: synthetic driver failure");

            let commands = logic_frame(&mut app, &ctx, false);
            assert!(!commands.contains(&egui::ViewportCommand::Close), "the cancelled close must not be deferred");
            assert!(app.ui.dialog.is_none());
            assert_eq!(app.ui.status, "Print: synthetic driver failure");
            let commands = logic_frame(&mut app, &ctx, true);
            if dirty {
                assert!(commands.contains(&egui::ViewportCommand::CancelClose));
                assert_eq!(app.ui.dialog.as_ref().unwrap().id, "closeDocument", "a later close still protects unsaved changes");
            } else {
                assert!(!commands.contains(&egui::ViewportCommand::CancelClose), "a later explicit close remains available");
            }
        }
    }

    /// A simultaneous print failure must not clear an explicit forced Quit queued for this tick.
    #[test]
    fn a_print_failure_keeps_forced_quit_requests_with_or_without_a_native_close() {
        for native_close in [false, true] {
            let mut app = app();
            let ctx = egui::Context::default();
            let (tx, result) = mpsc::channel();
            tx.send(Err("synthetic driver failure".into())).unwrap();
            app.pending_print = Some(Pending { result, title: "Snapshot".into() });
            app.quit_without_asking = true;
            app.request_quit();
            let commands = logic_frame(&mut app, &ctx, native_close);
            assert!(commands.contains(&egui::ViewportCommand::Close), "an explicit forced Quit must survive print failure: {commands:?}");
            assert!(!commands.contains(&egui::ViewportCommand::CancelClose));
        }
    }

    /// Moving a print to another thread preserves both allowed roots and denied sibling paths.
    #[test]
    fn the_print_worker_preserves_the_callers_read_and_write_roots() {
        let mut app = app();
        let dir = tempfile::tempdir().unwrap();
        let read = dir.path().join("read");
        let write = dir.path().join("write");
        let outside = dir.path().join("outside");
        for path in [&read, &write, &outside] {
            std::fs::create_dir(path).unwrap();
        }
        let roots = file_access::AutomationRoots::new(&[read.as_path()], &[write.as_path()]).unwrap().unwrap();
        let (checked, results) = mpsc::channel();
        app.test_print = Some(Box::new(move |_, _, _| {
            checked
                .send([
                    file_access::check_read(read.join("input.pdf")).is_ok(),
                    file_access::check_write(read.join("new.pdf")).is_ok(),
                    file_access::check_write(write.join("new.pdf")).is_ok(),
                    file_access::check_read(write.join("input.pdf")).is_ok(),
                    file_access::check_read(outside.join("input.pdf")).is_ok(),
                    file_access::check_write(outside.join("new.pdf")).is_ok(),
                ])
                .unwrap();
            Ok(json!({"printer":"Synthetic","pages":1}))
        }));
        file_access::confine(Some(&roots), || app.run("file.print", synthetic_params())).unwrap();
        drop(roots);
        assert_eq!(results.recv_timeout(Duration::from_secs(10)).unwrap(), [true, false, true, false, false, false]);
        finish(&mut app);
        assert_eq!(app.ui.status, "Sent 1 page(s) to Synthetic");
    }

    /// Invalid ranges and layouts leave their raw input and error in the open Print dialog.
    #[test]
    fn invalid_print_options_keep_the_dialog_open_without_starting_a_worker() {
        for fields in [
            json!({"range":"range","pages":"2"}),
            json!({"range":"range","pages":"not-a-range"}),
            json!({"range":"layouts","layouts":["Missing layout"]}),
            json!({"range":"layouts","layouts":"not a layout list"}),
        ] {
            let mut app = app();
            app.test_print = Some(Box::new(|_, _, _| panic!("invalid dialog options must not start a worker")));
            let original = crate::dialogs::Dialog::new("print", fields.clone());
            app.ui.dialog = Some(original);
            let error = crate::dialogs::confirm(&mut app).unwrap_err();
            assert!(app.pending_print.is_none());
            assert!(app.test_print.is_some(), "the worker hook must not even be consumed");
            let dialog = app.ui.dialog.as_ref().unwrap();
            assert_eq!(dialog.id, "print");
            assert_eq!(dialog.s("status"), error);
            for key in ["range", "pages", "layouts"] {
                assert_eq!(dialog.fields.get(key), fields.get(key), "{key}: {fields}");
            }
        }
    }

    /// PDF value validation happens before consuming the fake-driver hook or starting a thread.
    #[test]
    fn invalid_pdf_option_values_are_rejected_before_starting_a_worker() {
        let mut app = app();
        app.test_print = Some(Box::new(|_, _, _| panic!("invalid PDF options must not start a worker")));
        for (key, value) in [
            ("standard", json!("unknown")),
            ("flatten", json!("unknown")),
            ("created", json!("today")),
            ("view", json!("unknown")),
            ("pageLayout", json!("unknown")),
        ] {
            let mut params = synthetic_params();
            params[key] = value;
            assert!(app.run("file.print", params).is_err());
            assert!(app.pending_print.is_none());
            assert!(app.test_print.is_some());
        }
    }
}
