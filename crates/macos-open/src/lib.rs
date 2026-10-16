//! Files opened from Finder or the Dock on macOS: double-click, drag onto the app icon,
//! Open With.
//!
//! macOS does not put these paths in `argv`. It sends a `kAEOpenDocuments` Apple Event, which
//! AppKit hands to the app delegate's `application:openURLs:`. winit 0.30 does not implement that,
//! so the event is dropped. This crate installs its own handler for the event and queues the
//! paths; the app drains the queue each frame with [`take_pending`].
//!
//! Which files Finder offers to the app at all is decided by `CFBundleDocumentTypes` in
//! `packaging/macos/Info.plist.in`, not here.
//!
//! This is the workspace's only `unsafe` code (calls into the Objective-C runtime), kept in its
//! own crate so the rest of the workspace can keep `unsafe_code = "forbid"`. Within the crate
//! `unsafe_code` is denied too, except in the macOS module (`imp`), where each unsafe block carries
//! a `SAFETY:` comment (`clippy::undocumented_unsafe_blocks` is denied). winit 0.30 offers no safe
//! way to receive these events: its documented approach is a custom application delegate, which
//! needs the same unsafe calls. On other platforms the crate is empty apart from no-op stubs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Calls into AppKit; see the crate docs.
mod imp;

/// Starts listening for open-documents events. Call once, on the main thread, before the event
/// loop runs (before `eframe::run_native`), so files the app is launched with are caught too.
pub fn install() {
    #[cfg(target_os = "macos")]
    imp::install();
}

/// Called (from the main thread) whenever paths are queued, so an idle app wakes up to open
/// them; typically `move || ctx.request_repaint()`.
pub fn set_waker(waker: impl Fn() + Send + Sync + 'static) {
    #[cfg(target_os = "macos")]
    imp::set_waker(Box::new(waker));
    #[cfg(not(target_os = "macos"))]
    drop(waker);
}

/// Paths opened since the last call, oldest first.
pub fn take_pending() -> Vec<String> {
    #[cfg(target_os = "macos")]
    return imp::take_pending();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}
