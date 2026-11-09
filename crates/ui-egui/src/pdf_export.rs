//! Native PDF jobs retain an immutable document and its font scope. The UI keeps pumping
//! window events and shows honest, indeterminate progress while the shared exporter works.
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
    mpsc,
};

// The worker and UI race only on this write boundary; SAVING cannot be cancelled.
const GENERATING: u8 = 0;
const SAVING: u8 = 1;
const CANCELLED: u8 = 2;

#[cfg(not(target_arch = "wasm32"))]
struct Done {
    report: designcraft_engine::cmd::PdfExportReport,
    written: Option<usize>,
    roots: Option<Arc<designcraft_engine::file_access::AutomationRoots>>,
}

/// Version 2 native job status. Nullable fields are always serialized.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfExportStatus {
    pub version: u8,
    pub operation_id: Option<u64>,
    pub state: &'static str,
    pub busy: bool,
    pub worker_active: bool,
    pub cancellable: bool,
    pub elapsed: f64,
    pub last: Option<PdfExportResult>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfExportResult {
    pub operation_id: u64,
    pub path: String,
    pub bytes: Option<u64>,
    pub pages: Option<u64>,
    pub warnings: Vec<String>,
    pub seconds: f64,
    pub error: Option<String>,
    pub cancelled: bool,
}

#[derive(Default)]
pub(crate) struct Export {
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<mpsc::Receiver<Result<Done, String>>>,
    #[cfg(not(target_arch = "wasm32"))]
    phase: Option<Arc<AtomicU8>>,
    operation_id: u64,
    path: String,
    started: f64,
    pub last: Option<serde_json::Value>,
    notice: bool,
}
impl Export {
    pub fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some() && !self.cancelled()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub(crate) fn cancelled(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.phase.as_ref().is_some_and(|p| p.load(Ordering::Acquire) == CANCELLED)
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub fn worker_active(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub fn cancellable(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some() && self.phase.as_ref().is_some_and(|p| p.load(Ordering::Acquire) == GENERATING)
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    pub fn status(&self) -> PdfExportStatus {
        // One atomic read makes state/cancellable coherent across the worker's write claim.
        #[cfg(not(target_arch = "wasm32"))]
        let phase = self.phase.as_ref().map_or(GENERATING, |p| p.load(Ordering::Acquire));
        #[cfg(target_arch = "wasm32")]
        let phase = GENERATING;
        let worker_active = self.worker_active();
        let cancelled = phase == CANCELLED;
        let busy = worker_active && !cancelled;
        let last = self.last.as_ref().map(|v| PdfExportResult {
            operation_id: self.operation_id,
            path: self.path.clone(),
            bytes: v.get("bytes").and_then(serde_json::Value::as_u64),
            pages: v.get("pages").and_then(serde_json::Value::as_u64),
            warnings: v
                .get("warnings")
                .and_then(serde_json::Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            seconds: v.get("seconds").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
            error: v.get("error").and_then(serde_json::Value::as_str).map(str::to_owned),
            cancelled,
        });
        let saving = phase == SAVING;
        PdfExportStatus {
            version: 2,
            operation_id: (self.operation_id != 0).then_some(self.operation_id),
            state: if cancelled {
                "cancelled"
            } else if busy {
                if saving { "saving" } else { "generating" }
            } else if last.as_ref().is_some_and(|r| r.error.is_some()) {
                "failed"
            } else if last.is_some() {
                "succeeded"
            } else {
                "idle"
            },
            busy,
            worker_active,
            cancellable: busy && !saving,
            elapsed: if busy { (crate::now_ms() - self.started) / 1000.0 } else { last.as_ref().map_or(0.0, |r| r.seconds) },
            last,
        }
    }

    pub fn queued(&self) -> serde_json::Value {
        serde_json::json!({"version":2,"path":self.path,"queued":true,"operationId":self.operation_id,"statusCommand":"app.pdfExportStatus","bytes":null,"pages":null,"warnings":null})
    }

    /// Cancel only before the writer claims the job. A live cancelled decoder keeps its
    /// one worker slot until it exits, but never blocks editing or starts a late write.
    pub fn cancel(&mut self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if self.pending.is_some()
            && self.phase.as_ref().is_some_and(|p| p.compare_exchange(GENERATING, CANCELLED, Ordering::AcqRel, Ordering::Acquire).is_ok())
        {
            self.last = Some(serde_json::json!({"cancelled":true,"seconds":(crate::now_ms()-self.started)/1000.0}));
            self.notice = false;
            return true;
        }
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(
        &mut self,
        st: &designcraft_engine::DocState,
        cache: Arc<designcraft_compose::Cache>,
        path: String,
        params: serde_json::Value,
        ctx: egui::Context,
        writer: Option<crate::BackgroundWriteFn>,
    ) -> Result<(), String> {
        if self.worker_active() {
            return Err("A PDF export worker is still running".into());
        }
        let operation_id = self.operation_id.checked_add(1).ok_or("PDF export operation IDs exhausted")?;
        let phase = Arc::new(AtomicU8::new(GENERATING));
        let worker_phase = phase.clone();
        let doc = st.doc.clone();
        let fonts = st.fonts.clone();
        let roots = designcraft_engine::file_access::current();
        let (tx, rx) = mpsc::sync_channel(1);
        let target = path.clone();
        std::thread::Builder::new()
            .name("designcraft-pdf-export".into())
            .spawn(move || {
                let _fonts = fonts;
                let result = designcraft_engine::file_access::confine(roots.as_ref(), || {
                    designcraft_engine::file_access::check_write(&target)?;
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| designcraft_engine::cmd::export_pdf_document(&doc, &cache, &params)))
                        .map_err(|_| "PDF export failed unexpectedly".to_string())
                        .and_then(|r| r.map_err(|e| e.to_string()))
                        .and_then(|mut report| {
                            let written = if let Some(write) = writer {
                                // Claim the write atomically against cancellation. Never report
                                // cancellation once an arbitrary host writer has begun.
                                worker_phase
                                    .compare_exchange(GENERATING, SAVING, Ordering::AcqRel, Ordering::Acquire)
                                    .map_err(|_| "PDF export cancelled".to_string())?;
                                ctx.request_repaint();
                                let bytes = report.bytes.len();
                                write(&target, &report.bytes)?;
                                report.bytes = Vec::new();
                                Some(bytes)
                            } else {
                                None
                            };
                            Ok(Done { report, written, roots: roots.clone() })
                        })
                });
                let _ = tx.send(result);
                ctx.request_repaint();
            })
            .map_err(|e| format!("Couldn't start PDF export: {e}"))?;
        self.pending = Some(rx);
        self.phase = Some(phase);
        self.operation_id = operation_id;
        self.path = path;
        self.started = crate::now_ms();
        self.last = None;
        self.notice = false;
        Ok(())
    }

    pub fn poll(&mut self, writer: &mut Option<crate::WriteFn>) -> Option<Result<serde_json::Value, String>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let result = match self.pending.as_ref()?.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => Err("PDF export worker stopped unexpectedly".into()),
            };
            self.pending = None;
            if self.cancelled() {
                return None;
            }
            let result = result.and_then(|done| {
                let report=done.report;
                if done.written.is_none() {
                    if let Some(phase) = &self.phase {
                        phase.compare_exchange(GENERATING, SAVING, Ordering::AcqRel, Ordering::Acquire).map_err(|_| "PDF export cancelled".to_string())?;
                    }
                    designcraft_engine::file_access::confine(done.roots.as_ref(), || {
                        designcraft_engine::file_access::check_write(&self.path)?;
                        let write = writer.as_mut().ok_or("no writer")?;
                        write(&self.path, &report.bytes)
                    })?;
                }
                Ok(serde_json::json!({"path":self.path,"bytes":done.written.unwrap_or(report.bytes.len()),"pages":report.pages,"warnings":report.warnings,"seconds":(crate::now_ms()-self.started)/1000.0}))
            });
            self.notice = match &result {
                Err(_) => true,
                Ok(v) => v["warnings"].as_array().is_some_and(|w| !w.is_empty()),
            };
            self.last = Some(match &result {
                Ok(v) => v.clone(),
                Err(e) => serde_json::json!({"error":e,"seconds":(crate::now_ms()-self.started)/1000.0}),
            });
            Some(result)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = writer;
            None
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.busy() {
            if self.notice {
                egui::Modal::new(egui::Id::unique("pdf-export-result")).show(ctx, |ui| {
                    ui.set_max_width(520.0);
                    if let Some(last) = &self.last {
                        if let Some(error) = last["error"].as_str() {
                            ui.heading("PDF export failed");
                            ui.label(error);
                        } else {
                            ui.heading("PDF exported with warnings");
                            if let Some(warnings) = last["warnings"].as_array() {
                                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                    for warning in warnings {
                                        if let Some(text) = warning.as_str() {
                                            ui.label(text);
                                        }
                                    }
                                });
                            }
                        }
                    }
                    if ui.button("Close").clicked() {
                        self.notice = false;
                    }
                });
            }
            return;
        }
        egui::Modal::new(egui::Id::unique("pdf-export-progress")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading("Exporting PDF…");
            ui.label(&self.path);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{:.0} seconds elapsed", (crate::now_ms() - self.started) / 1000.0));
            });
            if self.cancellable() {
                if ui.button("Cancel export").clicked() {
                    self.cancel();
                }
                ui.label("Cancelling discards the PDF before saving starts.");
            } else {
                ui.label("Saving PDF… Please keep DesignCraft open until saving finishes.");
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use serde_json::json;

    fn pending_app(phase: u8) -> Result<(crate::DesignApp, mpsc::SyncSender<Result<Done, String>>), String> {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width":64,"height":64,"pages":1}))?;
        let (tx, rx) = mpsc::sync_channel(1);
        app.pdf_export.pending = Some(rx);
        app.pdf_export.phase = Some(Arc::new(AtomicU8::new(phase)));
        app.pdf_export.operation_id = 1;
        app.pdf_export.path = "cancelled.pdf".into();
        app.pdf_export.started = crate::now_ms();
        Ok((app, tx))
    }

    #[test]
    fn pdf_progress_cancel_button_dismisses_the_modal_before_saving() {
        use egui_kittest::kittest::Queryable;
        let (app, _tx) = pending_app(0).unwrap();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(700.0, 350.0))
            .build_ui_state(|ui, app: &mut crate::DesignApp| app.pdf_export.show(ui.ctx()), app);
        h.run_steps(3);
        h.get_by_label("Cancel export").click();
        h.run_steps(2);
        assert!(!h.state().pdf_export.busy());
        assert_eq!(h.state().pdf_export.status().state, "cancelled");
    }

    /// Opt-in offscreen visual evidence; ordinary CI needs no GPU.
    #[test]
    #[ignore = "offscreen GPU visual check"]
    fn pdf_progress_visual_evidence() {
        let output = std::env::var("PDF_PROGRESS_SHOTS").expect("output directory");
        let services =
            crate::Services { background_write: Some(Arc::new(|path, bytes| designcraft_engine::io::write_file(path, bytes))), ..Default::default() };
        let mut smoke = crate::DesignApp::new(designcraft_engine::Session::new(), services);
        smoke.run("file.newSample", json!({})).unwrap();
        let path = std::path::Path::new(&output).join("synthetic.pdf");
        let queued = smoke.run("app.exportPdf", json!({"path":path.to_string_lossy(),"tagged":true})).unwrap();
        assert_eq!(queued["version"], 2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while smoke.pdf_export.worker_active() && std::time::Instant::now() < deadline {
            if let Some(result) = smoke.pdf_export.poll(&mut smoke.services.write) {
                result.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!smoke.pdf_export.worker_active());
        assert_eq!(smoke.pdf_export.status().state, "succeeded");
        assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
        std::fs::write(std::path::Path::new(&output).join("pdf-result.json"), serde_json::to_vec_pretty(&smoke.pdf_export.status()).unwrap())
            .unwrap();
        for (phase, name) in [(0, "generating"), (1, "saving")] {
            let (app, _tx) = pending_app(phase).unwrap();
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(700.0, 350.0))
                .with_pixels_per_point(2.0)
                .wgpu()
                .build_ui_state(|ui, app: &mut crate::DesignApp| app.pdf_export.show(ui.ctx()), app);
            h.run_steps(3);
            h.render().unwrap().save(std::path::Path::new(&output).join(format!("pdf-{name}.png"))).unwrap();
        }
    }

    #[test]
    fn cancelled_export_never_writes_late_results_or_queues_unbounded_workers() {
        let (mut app, tx) = pending_app(0).unwrap();
        let report = designcraft_engine::cmd::export_pdf_document(&app.session.active().unwrap().doc, &app.session.cache, &json!({})).unwrap();
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = writes.clone();
        app.services.write = Some(Box::new(move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        app.run("app.quit", json!({})).unwrap();
        assert!(app.ui.status.contains("Quit queued"));
        let cancelled = app.run("app.cancelPdfExport", json!({"operationId":1})).unwrap();
        assert_eq!(cancelled["state"], "cancelled");
        assert_eq!(cancelled["workerActive"], true);
        assert!(!app.pdf_export.busy());
        assert!(!app.quit_after_export);
        app.run("file.new", json!({"width":80,"height":80})).unwrap();
        assert!(app.run("app.exportPdf", json!({"path":"other.pdf"})).is_err());
        tx.send(Ok(Done { report, written: None, roots: None })).unwrap();
        app.logic(&egui::Context::default());
        assert_eq!(writes.load(Ordering::SeqCst), 0);
        assert!(!app.pdf_export.worker_active());
        let status = app.run("app.pdfExportStatus", json!({"operationId":1})).unwrap();
        assert_eq!(status["last"]["cancelled"], true);
        assert!(status["last"]["bytes"].is_null());
        assert!(app.run("app.pdfExportStatus", json!({"operationId":2})).is_err());
        for id in [json!(-1), json!("1"), json!(0), json!(1.5)] {
            assert!(app.run("app.cancelPdfExport", json!({"operationId":id})).is_err());
        }
    }

    #[test]
    fn saving_cannot_be_cancelled_or_reported_cancelled_after_a_write_started() {
        let (mut app, tx) = pending_app(1).unwrap();
        assert!(app.run("app.cancelPdfExport", json!({"operationId":1})).is_err());
        assert_eq!(app.pdf_export.status().state, "saving");
        assert!(!app.pdf_export.status().cancellable);
        assert!(app.pdf_export.busy());
        tx.send(Err("disk full".into())).unwrap();
        app.logic(&egui::Context::default());
        let status = app.run("app.pdfExportStatus", json!({})).unwrap();
        assert_eq!(status["state"], "failed");
        assert_eq!(status["last"]["error"], "disk full");
        assert_eq!(status["last"]["cancelled"], false);
    }

    #[test]
    fn busy_export_allows_inspection_and_view_commands_with_structured_edit_errors() {
        let (mut app, _tx) = pending_app(0).unwrap();
        app.run("document.inspect", json!({})).unwrap();
        app.run("view.zoomIn", json!({})).unwrap();
        assert_eq!(app.pdf_export.queued()["operationId"], 1);
        assert!(app.pdf_export.queued()["pages"].is_null());
        let (req, _) = crate::control::ControlRequest::new("engine.execute", json!({"command":"file.new","params":{}}));
        let crate::control::Outcome::Done(response) = crate::control::handle(&mut app, &egui::Context::default(), &req) else {
            panic!("expected error response")
        };
        assert_eq!(response["ok"], false);
        assert_eq!(response["errorCode"], "EXPORT_IN_PROGRESS");
        assert_eq!(response["errorData"]["operationId"], 1);
        assert_eq!(response["errorData"]["state"], "generating");
    }

    #[test]
    fn background_export_keeps_the_callers_automation_write_limits() {
        use designcraft_engine::file_access::{AutomationRoots, confine};
        let temp = tempfile::tempdir().unwrap();
        let inside = temp.path().join("allowed");
        std::fs::create_dir(&inside).unwrap();
        let roots = AutomationRoots::new(&[&inside], &[&inside]).unwrap().unwrap();
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages":1})).unwrap();
        let writer: crate::BackgroundWriteFn = Arc::new(|_, _| panic!("a refused output must never reach the host writer"));
        let ctx = egui::Context::default();
        confine(Some(&roots), || {
            app.pdf_export
                .start(
                    app.session.active().unwrap(),
                    app.session.cache.clone(),
                    temp.path().join("outside.pdf").to_string_lossy().into_owned(),
                    json!({}),
                    ctx.clone(),
                    Some(writer),
                )
                .unwrap();
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.pdf_export.busy() && std::time::Instant::now() < deadline {
            app.logic(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.pdf_export.busy());
        assert!(app.pdf_export.last.as_ref().unwrap()["error"].as_str().unwrap().contains("outside the write root"));
        assert!(!temp.path().join("outside.pdf").exists());
    }

    #[test]
    fn background_export_preserves_multiple_roots_for_native_and_deferred_writes() {
        use designcraft_engine::file_access::{AutomationRoots, check_write, confine};
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let read_only = temp.path().join("read-only");
        for dir in [&first, &second, &read_only] {
            std::fs::create_dir(dir).unwrap();
        }
        let roots = AutomationRoots::new(&[&first, &read_only], &[&first, &second]).unwrap().unwrap();
        for native in [false, true] {
            let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({"pages":1})).unwrap();
            let forbidden = temp.path().join("not-granted.pdf");
            let deferred_forbidden = forbidden.clone();
            app.services.write = Some(Box::new(move |path, bytes| {
                assert!(check_write(&deferred_forbidden).is_err(), "the deferred host writer must retain the caller's roots");
                designcraft_engine::io::write_file(path, bytes)
            }));
            let background: crate::BackgroundWriteFn = Arc::new(move |path, bytes| {
                assert!(check_write(&forbidden).is_err(), "the native host writer must retain the caller's roots");
                designcraft_engine::io::write_file(path, bytes)
            });
            let ctx = egui::Context::default();
            for (folder, allowed) in [(first.as_path(), true), (second.as_path(), true), (read_only.as_path(), false), (temp.path(), false)] {
                let path = folder.join(if native { "native.pdf" } else { "deferred.pdf" });
                confine(Some(&roots), || {
                    app.pdf_export
                        .start(
                            app.session.active().unwrap(),
                            app.session.cache.clone(),
                            path.to_string_lossy().into_owned(),
                            json!({}),
                            ctx.clone(),
                            native.then(|| background.clone()),
                        )
                        .unwrap();
                });
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while app.pdf_export.busy() && std::time::Instant::now() < deadline {
                    app.logic(&ctx);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert!(!app.pdf_export.busy());
                let result = app.pdf_export.last.as_ref().unwrap();
                if allowed {
                    assert!(result.get("error").is_none(), "{result}");
                    assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
                } else {
                    assert!(result["error"].as_str().unwrap().contains("outside the write roots"), "{result}");
                    assert!(!path.exists());
                }
            }
        }
    }

    #[test]
    fn pending_export_keeps_frames_running_and_reports_writer_failure() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width":64,"height":64,"pages":1})).unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        app.pdf_export.pending = Some(rx);
        app.pdf_export.path = "test.pdf".into();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut out = ctx.run_ui(Default::default(), |ui| app.logic(ui.ctx()));
            out.textures_delta.clear();
        }
        for _ in 0..3 {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                app.logic(ui.ctx());
                app.pdf_export.show(ui.ctx());
            });
            out.textures_delta.clear();
            assert!(app.pdf_export.busy());
        }
        let mut raw = egui::RawInput::default();
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        let mut out = ctx.run_ui(raw, |ui| app.handle_quit(ui.ctx()));
        out.textures_delta.clear();
        assert!(out.viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(app.run("app.quit", json!({})).unwrap()["queued"], true, "normal Quit waits for the export");
        let report = designcraft_engine::cmd::export_pdf_document(&app.session.active().unwrap().doc, &app.session.cache, &json!({})).unwrap();
        app.services.write = Some(Box::new(|_, _| Err("disk full".into())));
        tx.send(Ok(Done { report, written: None, roots: None })).unwrap();
        app.logic(&ctx);
        assert!(!app.pdf_export.busy());
        assert!(app.ui.status.contains("disk full"));
        assert!(app.pdf_export.last.as_ref().unwrap()["error"].as_str().unwrap().contains("disk full"));
    }

    fn logic_frame(app: &mut crate::DesignApp, ctx: &egui::Context, close: bool) -> Vec<egui::ViewportCommand> {
        let mut raw = egui::RawInput::default();
        if close {
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        }
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    }

    fn ready_app(ctx: &egui::Context) -> Result<crate::DesignApp, String> {
        let services = crate::Services { inbox: Some(crate::Inbox::default()), ..Default::default() };
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), services);
        app.run("file.new", json!({"width":100,"height":100,"pages":1,"facingPages":false}))?;
        logic_frame(&mut app, ctx, false);
        logic_frame(&mut app, ctx, false);
        assert!(app.fonts_ready);
        Ok(app)
    }

    #[test]
    fn pdf_export_defers_inbox_and_os_opens_until_the_terminal_result() {
        let ctx = egui::Context::default();
        let mut app = ready_app(&ctx).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("queued.designcraft");
        app.run("file.save", json!({"path":path})).unwrap();
        let request = app.import_request("open");
        let inbox = app.services.inbox.as_ref().unwrap().clone();
        inbox.lock().unwrap().push(crate::ImportedFile { request, name: "inbox.designcraft".into(), bytes: std::fs::read(&path).unwrap() });
        app.open_documents_later([path.to_string_lossy().into_owned()]);
        let (tx, rx) = mpsc::sync_channel(1);
        app.pdf_export.pending = Some(rx);
        for _ in 0..3 {
            logic_frame(&mut app, &ctx, false);
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(inbox.lock().unwrap().len(), 1);
            assert_eq!(app.os_documents.len(), 1);
        }
        // A failed export is terminal too; queued inputs must still be delivered once.
        tx.send(Err("synthetic export failure".into())).unwrap();
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.session.documents().len(), 3);
        assert!(inbox.lock().unwrap().is_empty());
        assert!(app.os_documents.is_empty());
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.session.documents().len(), 3);
    }

    #[test]
    fn pdf_export_defers_floating_window_drops_until_terminal_result() {
        #[derive(Debug)]
        struct DroppedDocument(std::path::PathBuf);
        impl egui::DroppedFile for DroppedDocument {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|error| error.to_string())
            }
        }
        let ctx = egui::Context::default();
        let mut app = ready_app(&ctx).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("floating.designcraft");
        let mut saved = designcraft_engine::Session::new();
        saved.execute("file.new", &json!({})).unwrap();
        saved.execute("file.save", &json!({"path":path})).unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        app.pdf_export.pending = Some(rx);
        let samples = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = samples.clone();
        app.services.pointer = Some(Box::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(egui::pos2(20.0, 20.0))
        }));
        app.open_dropped_files(&[Arc::new(DroppedDocument(path)) as egui::DroppedFileHandle], None);
        for _ in 0..3 {
            logic_frame(&mut app, &ctx, false);
            assert_eq!(app.session.documents().len(), 1, "export must defer the floating-window drop");
            assert_eq!(app.pending_drops.len(), 1);
            assert!(app.pending_drops[0].pointer_sampled);
            assert_eq!(app.pending_drops[0].pointer, None, "floating-window fallback stays captured");
            assert_eq!(samples.load(std::sync::atomic::Ordering::Relaxed), 0);
        }
        tx.send(Err("synthetic export failure".into())).unwrap();
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.session.documents().len(), 2, "the dropped document opens after the terminal result");
        assert!(app.pending_drops.is_empty());
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.session.documents().len(), 2, "the drop is delivered exactly once");
    }

    #[test]
    fn pdf_export_replays_dropped_graphics_at_the_original_pointer() {
        let ctx = egui::Context::default();
        let mut app = ready_app(&ctx).unwrap();
        *app.view_mut().unwrap() = crate::View { zoom: 1.0, origin: designcraft_geom::Point::ZERO, fitted: true, rotation: 0 };
        let canvas = app.canvas_rect.expect("the ready UI has a canvas");
        let drop_position = canvas.min + egui::vec2(25.0, 35.0);
        assert!(canvas.contains(drop_position), "exercise a real canvas drop, not the center fallback");
        let pointer = Some(drop_position);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("placed.svg");
        std::fs::write(
            &path,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        app.pdf_export.pending = Some(rx);
        app.pending_drops.push(crate::PendingDrop {
            path: path.to_string_lossy().into_owned(),
            pointer,
            generation: app.input_generation(),
            starts_batch: true,
            pointer_sampled: true,
        });
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.pending_drops.len(), 1);
        assert!(app.session.doc().unwrap().doc.spreads[0].items.is_empty());
        let (_, expected) = crate::canvas::drop_point(&app, pointer).unwrap();
        tx.send(Err("synthetic export failure".into())).unwrap();
        logic_frame(&mut app, &ctx, false);
        assert!(app.pending_drops.is_empty());
        assert_eq!(app.session.documents().len(), 1, "a graphic must be placed, not opened as a document");
        let items = &app.session.doc().unwrap().doc.spreads[0].items;
        assert_eq!(items.len(), 1);
        let bounds = items[0].bounds();
        assert!((bounds.x0 - expected.x).abs() < 0.01 && (bounds.y0 - expected.y).abs() < 0.01, "{bounds:?} vs {expected:?}");
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 1);
    }

    #[test]
    fn pdf_export_preserves_first_frame_placement_for_each_deferred_drop_batch() {
        #[derive(Debug)]
        struct DroppedGraphic(std::path::PathBuf);
        impl egui::DroppedFile for DroppedGraphic {
            fn path(&self) -> &std::path::Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|error| error.to_string())
            }
        }
        for floating in [false, true] {
            for separate_batches in [false, true] {
                let ctx = egui::Context::default();
                let mut app = ready_app(&ctx).unwrap();
                let frame = app.run("frame.create", json!({"rect":[10,10,40,40]})).unwrap()["id"].as_u64().unwrap();
                app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
                *app.view_mut().unwrap() = crate::View { zoom: 1.0, origin: designcraft_geom::Point::new(-200.0, -200.0), fitted: true, rotation: 0 };
                let layout = designcraft_tools::CanvasLayout::new(&app.session.doc().unwrap().doc, false);
                let slot = layout.slot(designcraft_doc::SpreadRef::Doc(0)).unwrap();
                let point = slot.to_canvas(designcraft_geom::Point::new(20.0, 20.0));
                let pointer = crate::canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(point);
                assert_eq!(crate::canvas::drop_frame(&app, Some(pointer)), Some(designcraft_doc::ItemId(frame)));
                let dir = tempfile::tempdir().unwrap();
                let mut files = Vec::new();
                for name in ["first.svg", "second.svg"] {
                    let path = dir.path().join(name);
                    std::fs::write(
                        &path,
                        br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##,
                    )
                    .unwrap();
                    files.push(Arc::new(DroppedGraphic(path)) as egui::DroppedFileHandle);
                }
                let (tx, rx) = mpsc::sync_channel(1);
                app.pdf_export.pending = Some(rx);
                let mut capture = |files: &[egui::DroppedFileHandle]| {
                    if floating {
                        app.open_dropped_files(files, Some(pointer));
                    } else {
                        let mut raw = egui::RawInput::default();
                        raw.events.push(egui::Event::PointerMoved(pointer));
                        raw.dropped_files.extend_from_slice(files);
                        app.raw_input_hook(&ctx, &mut raw);
                    }
                };
                if separate_batches {
                    for file in &files {
                        capture(std::slice::from_ref(file));
                    }
                } else {
                    capture(&files);
                }
                app.replay_dropped_files();
                assert_eq!(app.pending_drops.len(), 2, "busy export retains both drops");
                assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 1);
                tx.send(Err("synthetic export failure".into())).unwrap();
                app.logic(&ctx);
                app.replay_dropped_files();
                assert!(app.pending_drops.is_empty());
                let doc = &app.session.doc().unwrap().doc;
                assert_eq!(doc.spreads[0].items.len(), if separate_batches { 1 } else { 2 }, "floating={floating}, separate={separate_batches}");
                assert!(matches!(doc.item(designcraft_doc::ItemId(frame)).unwrap().content, designcraft_doc::Content::Graphic(_)));
                app.replay_dropped_files();
                assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), if separate_batches { 1 } else { 2 }, "replayed exactly once");
            }
        }
    }

    #[test]
    fn pdf_export_captures_system_drop_pointer_before_deferring_placement() {
        for zoom in [1.0f32, 2.0] {
            for available in [false, true] {
                let ctx = egui::Context::default();
                let mut app = ready_app(&ctx).unwrap();
                // App initialization applies the saved UI scale; request the test scale after it.
                ctx.set_zoom_factor(zoom);
                let frame = app.run("frame.create", json!({"rect":[10,10,40,40]})).unwrap()["id"].as_u64().unwrap();
                app.run("selection.set", json!({"ids":[]})).unwrap();
                app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
                *app.view_mut().unwrap() = crate::View { zoom: 1.0, origin: designcraft_geom::Point::new(-200.0, -200.0), fitted: true, rotation: 0 };
                let layout = designcraft_tools::CanvasLayout::new(&app.session.doc().unwrap().doc, false);
                let slot = layout.slot(designcraft_doc::SpreadRef::Doc(0)).unwrap();
                let point = slot.to_canvas(designcraft_geom::Point::new(20.0, 20.0));
                let inside = crate::canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(point);
                let window = egui::vec2(300.0, 200.0);
                let pointer = Arc::new(std::sync::Mutex::new(available.then_some(((inside + window).to_vec2() * zoom).to_pos2())));
                let source = pointer.clone();
                app.services.pointer = Some(Box::new(move || *source.lock().unwrap()));
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("system-drop.svg");
                std::fs::write(
                    &path,
                    br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##,
                )
                .unwrap();
                app.pending_drops.push(crate::PendingDrop {
                    path: path.to_string_lossy().into_owned(),
                    pointer: None,
                    generation: app.input_generation(),
                    starts_batch: true,
                    pointer_sampled: false,
                });
                let (tx, rx) = mpsc::sync_channel(1);
                app.pdf_export.pending = Some(rx);
                let raw = || {
                    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO + window, egui::vec2(1400.0, 900.0));
                    let mut raw =
                        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, rect.size())), ..Default::default() };
                    raw.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo { inner_rect: Some(rect), ..Default::default() });
                    raw
                };
                ctx.run_ui(raw(), |ui| app.take_pending_drops(ui.ctx())).textures_delta.clear();
                assert_eq!(app.pending_drops.len(), 1, "busy export retains the drop");
                assert!(app.pending_drops[0].pointer_sampled);
                assert_eq!(app.pending_drops[0].pointer, available.then_some(inside));
                // Moving the system pointer, or making it available later, cannot retarget the drop.
                *pointer.lock().unwrap() = Some(egui::pos2(1300.0, 800.0));
                ctx.run_ui(raw(), |ui| app.take_pending_drops(ui.ctx())).textures_delta.clear();
                assert_eq!(app.pending_drops[0].pointer, available.then_some(inside));
                tx.send(Err("synthetic export failure".into())).unwrap();
                app.logic(&ctx);
                ctx.run_ui(raw(), |ui| app.take_pending_drops(ui.ctx())).textures_delta.clear();
                assert!(app.pending_drops.is_empty());
                let doc = &app.session.doc().unwrap().doc;
                assert_eq!(doc.spreads[0].items.len(), if available { 1 } else { 2 });
                assert_eq!(matches!(doc.item(designcraft_doc::ItemId(frame)).unwrap().content, designcraft_doc::Content::Graphic(_)), available);
                ctx.run_ui(raw(), |ui| app.take_pending_drops(ui.ctx())).textures_delta.clear();
                assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), if available { 1 } else { 2 }, "exactly once");
            }
        }
    }

    #[test]
    fn pdf_export_replays_menu_and_window_quit_after_success_or_failure() {
        for menu in [false, true] {
            for success in [false, true] {
                let ctx = egui::Context::default();
                let mut app = ready_app(&ctx).unwrap();
                app.run("frame.create", json!({"rect":[10,10,20,20]})).unwrap();
                let report =
                    designcraft_engine::cmd::export_pdf_document(&app.session.active().unwrap().doc, &app.session.cache, &json!({})).unwrap();
                let (tx, rx) = mpsc::sync_channel(1);
                app.pdf_export.pending = Some(rx);
                if menu {
                    assert_eq!(app.run("app.quit", json!({})).unwrap()["queued"], true);
                    logic_frame(&mut app, &ctx, false);
                } else {
                    assert!(logic_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
                }
                assert!(app.quit_after_export);
                assert!(app.ui.dialog.is_none());
                tx.send(if success { Ok(Done { report, written: Some(1), roots: None }) } else { Err("synthetic failure".into()) }).unwrap();
                assert!(logic_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
                assert!(!app.quit_after_export);
                assert!(logic_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
                assert_eq!(app.ui.dialog.as_ref().unwrap().id, "closeDocument", "unsaved changes still ask after export");
                app.ui.dialog = None; // Cancel the resumed Quit.
                assert!(!logic_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
                assert_eq!(app.session.documents().len(), 1);
                assert!(app.session.active().unwrap().is_dirty());
            }
        }
    }

    #[test]
    fn snapshot_finishes_after_document_closes_and_returns_real_pdf_bytes() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width":64,"height":64,"pages":2})).unwrap();
        let ctx = egui::Context::default();
        app.pdf_export
            .start(app.session.active().unwrap(), app.session.cache.clone(), "copy.pdf".into(), json!({"tagged":true}), ctx.clone(), None)
            .unwrap();
        app.session.close_document(0);
        let data = Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = data.clone();
        app.services.write = Some(Box::new(move |_, bytes| {
            *output.lock().unwrap() = bytes.to_vec();
            Ok(())
        }));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.pdf_export.busy() && std::time::Instant::now() < deadline {
            app.logic(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.pdf_export.busy());
        assert_eq!(app.pdf_export.last.as_ref().unwrap()["pages"], 2);
        assert!(data.lock().unwrap().starts_with(b"%PDF-"));
    }
    #[test]
    fn native_writer_runs_off_the_ui_thread_and_does_not_use_the_ui_writer() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages":1})).unwrap();
        let ui_thread = std::thread::current().id();
        let written = Arc::new(std::sync::Mutex::new(0usize));
        let count = written.clone();
        let writer: crate::BackgroundWriteFn = Arc::new(move |_, bytes| {
            assert_ne!(std::thread::current().id(), ui_thread);
            *count.lock().unwrap() = bytes.len();
            Ok(())
        });
        let ctx = egui::Context::default();
        app.pdf_export
            .start(app.session.active().unwrap(), app.session.cache.clone(), "background.pdf".into(), json!({}), ctx.clone(), Some(writer))
            .unwrap();
        app.services.write = Some(Box::new(|_, _| panic!("UI writer must not run")));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.pdf_export.busy() && std::time::Instant::now() < deadline {
            app.logic(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.pdf_export.busy());
        assert_eq!(app.pdf_export.last.as_ref().unwrap()["bytes"].as_u64().unwrap(), *written.lock().unwrap() as u64);
        assert!(*written.lock().unwrap() > 0);
    }
}
