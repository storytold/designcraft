//! macOS open-documents and quit Apple events.
//!
//! Finder double-clicks, Open With, drops on the Dock icon and `open -a DesignCraft brochure.idml`
//! don't pass paths on the command line: LaunchServices sends the running (or just-launched) app a
//! `kAEOpenDocuments` ('odoc') Apple event. winit 0.30 doesn't handle it and owns the
//! `NSApplicationDelegate`, and handling it ourselves needs Objective-C class declarations, i.e.
//! `unsafe`, which this workspace forbids. The `fmv-macos-events` crate wraps exactly that (an
//! `NSAppleEventManager` handler registered before Finder's launch event, leaving winit's delegate
//! alone) behind a safe main-thread API.

use designcraft_ui_egui::DesignApp;
use fmv_macos_events::{Event, Inbox, Registration};

/// Keeps the Apple-event handlers registered; hold it until the event loop returns.
pub struct AppleEvents {
    _registration: Registration,
    inbox: Inbox,
}

impl AppleEvents {
    /// Register the handlers. Call on the main thread before the event loop starts, so the event
    /// that launched the app (a Finder double-click) is caught too.
    pub fn install() -> Self {
        let (registration, inbox) = Registration::install();
        Self { _registration: registration, inbox }
    }

    /// The queue the app drains every frame ([`poll`]); events arriving later wake `ctx`.
    pub fn connect(&self, ctx: &egui::Context) -> Inbox {
        let ctx = ctx.clone();
        self.inbox.set_wake(move || ctx.request_repaint());
        self.inbox.clone()
    }
}

/// Hand the documents that arrived since the last frame to the app, which opens them as File ›
/// Open does once its window is ready; a quit request (Dock › Quit) closes the window, as its close
/// button does.
pub fn poll(inbox: &Inbox, app: &mut DesignApp, ctx: &egui::Context) {
    for e in inbox.drain() {
        match e {
            Event::Open(paths) => app.open_documents_later(paths.into_iter().map(|p| p.to_string_lossy().into_owned())),
            Event::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}
