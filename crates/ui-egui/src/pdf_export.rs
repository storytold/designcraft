//! Native PDF jobs retain an immutable document and its font scope. The UI keeps pumping
//! window events and shows honest, indeterminate progress while the shared exporter works.
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, mpsc};

#[cfg(not(target_arch = "wasm32"))]
struct Done {
    report: designcraft_engine::cmd::PdfExportReport,
    written: Option<usize>,
    roots: Option<Arc<designcraft_engine::file_access::AutomationRoots>>,
}

#[derive(Default)]
pub(crate) struct Export {
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<mpsc::Receiver<Result<Done, String>>>,
    path: String,
    started: f64,
    pub last: Option<serde_json::Value>,
    notice: bool,
}
impl Export {
    pub fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
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
        if self.busy() {
            return Err("A PDF export is already running".into());
        }
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
            let result = result.and_then(|done| {
                let report=done.report;
                if done.written.is_none() {
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
                Err(e) => serde_json::json!({"error":e}),
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
                egui::Modal::new(egui::Id::new("pdf-export-result")).show(ctx, |ui| {
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
        egui::Modal::new(egui::Id::new("pdf-export-progress")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.heading("Exporting PDF…");
            ui.label(&self.path);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{:.0} seconds elapsed", (crate::now_ms() - self.started) / 1000.0));
            });
            ui.label("Please keep DesignCraft open until the export finishes.");
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn background_export_keeps_the_callers_automation_write_limits() {
        use designcraft_engine::file_access::{AutomationRoots, confine};
        let temp = tempfile::tempdir().unwrap();
        let inside = temp.path().join("allowed");
        std::fs::create_dir(&inside).unwrap();
        let roots = AutomationRoots::new(Some(&inside), Some(&inside)).unwrap().unwrap();
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
        assert!(app.run("app.quit", json!({})).is_err(), "export must not silently disappear on normal Quit");
        let report = designcraft_engine::cmd::export_pdf_document(&app.session.active().unwrap().doc, &app.session.cache, &json!({})).unwrap();
        app.services.write = Some(Box::new(|_, _| Err("disk full".into())));
        tx.send(Ok(Done { report, written: None, roots: None })).unwrap();
        app.logic(&ctx);
        assert!(!app.pdf_export.busy());
        assert!(app.ui.status.contains("disk full"));
        assert!(app.pdf_export.last.as_ref().unwrap()["error"].as_str().unwrap().contains("disk full"));
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
