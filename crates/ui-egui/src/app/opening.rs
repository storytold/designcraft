//! Opening a document file without stopping the UI: the file is read and parsed on a worker
//! thread ([`designcraft_engine::cmd::read_document`]), and `file.open` opens what it read on a
//! later frame. A large file with embedded images takes seconds to read; the status line says
//! which file is opening, and Esc stops waiting for it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use designcraft_engine::cmd::ReadDocument;

use super::{DesignApp, Value, json};

/// A document file being read on a worker thread.
pub(crate) struct Opening {
    path: String,
    /// The file name, for the status line and the alert.
    name: String,
    cancel: Arc<AtomicBool>,
    read: Receiver<Result<ReadDocument, String>>,
}

impl Drop for Opening {
    /// A read nobody waits for any more stops at its next step.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl DesignApp {
    /// Read the file at `path` on a worker thread; [`Self::poll_opening`] opens it when it is in.
    /// The same file asked for again while it is read opens once.
    pub(super) fn open_in_background(&mut self, path: String) -> Result<Value, String> {
        if self.opening.iter().any(|o| o.path == path) {
            return Ok(Value::Null);
        }
        let name = std::path::Path::new(&path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        let cancel = Arc::new(AtomicBool::new(false));
        let (send, read) = std::sync::mpsc::channel();
        // The file roots in force here apply on the worker too.
        let roots = designcraft_engine::file_access::current();
        let wake = self.egui_ctx.clone();
        let (worker_path, worker_cancel) = (path.clone(), Arc::clone(&cancel));
        let spawned = std::thread::Builder::new().name("designcraft-open".into()).spawn(move || {
            let read = designcraft_engine::file_access::confine(roots.as_ref(), || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| designcraft_engine::cmd::read_document(&worker_path, &worker_cancel)))
            });
            let read = match read {
                Ok(read) => read.map_err(|e| e.to_string()),
                Err(_) => Err(format!("{worker_path}: the file could not be read")),
            };
            // The app may have stopped waiting (Esc): nobody to tell then.
            let _ = send.send(read);
            if let Some(ctx) = wake {
                ctx.request_repaint();
            }
        });
        if let Err(e) = spawned {
            log::warn!("opening {path} without a thread: {e}");
            return self.open_file("file.open", json!({"path": path}));
        }
        self.opening.push(Opening { path, name, cancel, read });
        self.show_opening();
        Ok(Value::Null)
    }

    /// The status line while files are read: the first one still opening.
    fn show_opening(&mut self) {
        if let Some(name) = self.opening.first().map(|o| o.name.clone()) {
            self.status(crate::i18n::tr(&self.ui.language, "Opening {name}…").replace("{name}", &name));
        }
    }

    /// Open the files whose reads are done, in the order they were asked for, and stop waiting
    /// for all of them on Esc. Repaints at a low rate while reads run, so the status line stays.
    pub(crate) fn poll_opening(&mut self, ctx: &egui::Context) {
        if self.opening.is_empty() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            // Dropping them tells their workers to stop.
            self.opening.clear();
            self.status(crate::i18n::tr(&self.ui.language, "Opening cancelled"));
            return;
        }
        let mut done = Vec::new();
        for o in std::mem::take(&mut self.opening) {
            match o.read.try_recv() {
                Ok(read) => done.push((o, read)),
                Err(TryRecvError::Empty) => self.opening.push(o),
                Err(TryRecvError::Disconnected) => {
                    let lost = format!("{}: the file could not be read", o.path);
                    done.push((o, Err(lost)));
                }
            }
        }
        for (o, read) in done {
            // The status line said the file was opening; what the open reports replaces it.
            self.status(String::new());
            let params = json!({"path": o.path});
            match read {
                Ok(read) => {
                    self.session.stage_open(read);
                    let _ = self.open_named_file("file.open", params, o.name.clone());
                }
                Err(e) => {
                    let alert = json!({"title": "Can't Open the File", "file": o.name, "message": e});
                    self.status(e);
                    self.ui.dialog = Some(crate::dialogs::Dialog::new("alert", alert));
                }
            }
        }
        if self.opening.is_empty() {
            return;
        }
        if self.ui.status.is_empty() {
            self.show_opening();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
