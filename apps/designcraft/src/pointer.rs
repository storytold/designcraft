//! macOS: where the pointer is on the screen, for files dragged in from the Finder. winit sends no
//! pointer events during a drag, so the drop asks AppKit (`NSEvent.mouseLocation`) instead.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen};

/// The pointer, in points from the top left of the main display (where winit, and so egui's
/// `inner_rect`, put the origin). `None` off the main thread or with no display.
pub fn screen_position() -> Option<egui::Pos2> {
    let mtm = MainThreadMarker::new()?;
    // AppKit's origin is the bottom left of the first screen, the one with the menu bar.
    let height = NSScreen::screens(mtm).firstObject()?.frame().size.height;
    let at = NSEvent::mouseLocation();
    let (x, y) = (at.x, height - at.y);
    (x.is_finite() && y.is_finite()).then(|| egui::pos2(x as f32, y as f32))
}
