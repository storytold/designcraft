//! macOS: AppKit moves the window when a drag starts anywhere in the title-bar band, on the
//! application bar's controls too. The window is made unmovable while the pointer is on one of
//! them (`designcraft_ui_egui::title_bar::window_movable`), so only the bar's empty space moves it.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSWindowStyleMask};

/// Sets the window's `movable` flag from this frame's title bar. `applied` holds the value last
/// set, so AppKit is only called when it changes.
pub fn sync_movable(ctx: &egui::Context, applied: &mut Option<bool>) {
    let movable = designcraft_ui_egui::title_bar::window_movable(ctx);
    if *applied == Some(movable) {
        return;
    }
    // eframe runs the app on the main thread; anywhere else AppKit is out of reach.
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let mut found = false;
    for window in NSApplication::sharedApplication(mtm).windows().to_vec() {
        // The main window is the only one with a full-size content view.
        if window.styleMask().contains(NSWindowStyleMask::FullSizeContentView) {
            window.setMovable(movable);
            found = true;
        }
    }
    // Before the window is on screen there is nothing to set yet; try again next frame.
    if found {
        *applied = Some(movable);
    }
}
