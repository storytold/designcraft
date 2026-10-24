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
        let mut out = ctx.run_ui(raw, |ui| app.logic(ui.ctx()));
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
    fn pdf_export_replays_dropped_graphics_at_the_original_pointer() {
        let ctx = egui::Context::default();
        let mut app = ready_app(&ctx).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0)));
        *app.view_mut().unwrap() = crate::View { zoom: 1.0, origin: designcraft_geom::Point::ZERO, fitted: true, rotation: 0 };
        let pointer = Some(egui::pos2(25.0, 35.0));
        let (_, expected) = crate::canvas::drop_point(&app, pointer).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("placed.svg");
        std::fs::write(
            &path,
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        app.pdf_export.pending = Some(rx);
        app.deferred_drops.push((path.to_string_lossy().into_owned(), pointer));
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.deferred_drops.len(), 1);
        assert!(app.session.doc().unwrap().doc.spreads[0].items.is_empty());
        tx.send(Err("synthetic export failure".into())).unwrap();
        logic_frame(&mut app, &ctx, false);
        assert!(app.deferred_drops.is_empty());
        assert_eq!(app.session.documents().len(), 1, "a graphic must be placed, not opened as a document");
        let items = &app.session.doc().unwrap().doc.spreads[0].items;
        assert_eq!(items.len(), 1);
        let bounds = items[0].bounds();
        assert!((bounds.x0 - expected.x).abs() < 0.01 && (bounds.y0 - expected.y).abs() < 0.01, "{bounds:?} vs {expected:?}");
        logic_frame(&mut app, &ctx, false);
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 1);
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
