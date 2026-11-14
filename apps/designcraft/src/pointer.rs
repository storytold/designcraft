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
    from_appkit(at.x, at.y, height)
}

/// A point in AppKit's global coordinates (origin at the bottom left of the main display, the one
/// with the menu bar, y up) in winit's (origin at its top left, y down), given that display's
/// height. Only the main display's height: winit flips window positions the same way
/// (`flip_window_screen_coordinates`, against `CGMainDisplayID`), whatever display the window is
/// on, so a display left of or above the main one gives negative coordinates in both.
fn from_appkit(x: f64, y: f64, main_height: f64) -> Option<egui::Pos2> {
    let y = main_height - y;
    (x.is_finite() && y.is_finite()).then(|| egui::pos2(x as f32, y as f32))
}

#[cfg(test)]
mod tests {
    use super::from_appkit;

    #[test]
    fn appkit_points_flip_against_the_main_display_only() {
        // The main display is 1000 points high: its top left is (0, 0), its bottom left (0, 1000).
        assert_eq!(from_appkit(0.0, 1000.0, 1000.0), Some(egui::pos2(0.0, 0.0)));
        assert_eq!(from_appkit(200.0, 100.0, 1000.0), Some(egui::pos2(200.0, 900.0)));
        // A second display above the main one, and one to its left: negative, as winit reports a
        // window there.
        assert_eq!(from_appkit(300.0, 1200.0, 1000.0), Some(egui::pos2(300.0, -200.0)));
        assert_eq!(from_appkit(-800.0, 500.0, 1000.0), Some(egui::pos2(-800.0, 500.0)));
        assert_eq!(from_appkit(f64::NAN, 0.0, 1000.0), None);
        assert_eq!(from_appkit(0.0, 0.0, f64::INFINITY), None);
    }
}
