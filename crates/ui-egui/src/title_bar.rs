//! The application bar as the window's title bar (macOS): an empty part of it moves the window
//! when dragged and follows the system's title-bar double-click setting.
//!
//! The window has a full-size content view, so AppKit hands every click in the title-bar band to
//! the app (that is how the bar's own controls work) and neither drags nor zooms the window
//! itself.

use std::sync::{Once, OnceLock};

/// System Settings › Desktop & Dock › "Double-click a window's title bar to".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoubleClick {
    Zoom,
    Minimize,
    Nothing,
}

/// The value of the global default `AppleActionOnDoubleClick`, as `defaults read` prints it.
/// Anything other than "Minimize" or "None" zooms, the macOS default ("Maximize"; macOS 15 adds
/// "Fill", which zooming comes closest to).
pub fn parse(defaults_output: &str) -> DoubleClick {
    match defaults_output.trim() {
        "Minimize" => DoubleClick::Minimize,
        "None" => DoubleClick::Nothing,
        _ => DoubleClick::Zoom,
    }
}

static ACTION: OnceLock<DoubleClick> = OnceLock::new();
static READ: Once = Once::new();

/// Starts reading the system setting on a background thread, once per run.
fn prefetch() {
    READ.call_once(|| {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            // No thread, no setting: the bar zooms.
            let _ = std::thread::Builder::new().name("title-bar-setting".into()).spawn(|| {
                let action = match std::process::Command::new("/usr/bin/defaults").args(["read", "-g", "AppleActionOnDoubleClick"]).output() {
                    Ok(out) if out.status.success() => parse(&String::from_utf8_lossy(&out.stdout)),
                    // Absent (never changed) or unreadable: the default.
                    _ => DoubleClick::Zoom,
                };
                let _ = ACTION.set(action);
            });
        }
    });
}

/// The system setting; zoom until it has been read.
pub fn action() -> DoubleClick {
    ACTION.get().copied().unwrap_or(DoubleClick::Zoom)
}

/// What a double-click on the empty title bar asks of the window.
pub fn double_click_command(action: DoubleClick, maximized: bool) -> Option<egui::ViewportCommand> {
    match action {
        DoubleClick::Zoom => Some(egui::ViewportCommand::Maximized(!maximized)),
        DoubleClick::Minimize => Some(egui::ViewportCommand::Minimized(true)),
        DoubleClick::Nothing => None,
    }
}

/// Makes the empty part of `rect` act as a title bar. Call it before drawing the bar's controls:
/// they sit on top and take their own clicks.
///
/// The area senses clicks only. A drag-sensing background would also receive drags that start
/// on a click-only control (egui gives clicks and drags to different widgets), so a drag is
/// detected here as a press on the empty area that has stopped being a click.
pub fn empty_area(ui: &mut egui::Ui, rect: egui::Rect) {
    prefetch();
    let resp = ui.interact(rect, ui.id().with("title_bar_empty"), egui::Sense::click());
    if resp.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized).unwrap_or(false);
        if let Some(cmd) = double_click_command(action(), maximized) {
            ui.ctx().send_viewport_cmd(cmd);
        }
    }
    // egui forgets which widget a press landed on once it can no longer be a click, so the press
    // is remembered (by its start time) while it still can.
    let armed = resp.id.with("drag_press");
    let press = ui.input(|i| i.pointer.press_start_time());
    if resp.is_pointer_button_down_on() {
        ui.data_mut(|d| d.insert_temp(armed, press));
    }
    if press.is_some() && ui.input(|i| i.pointer.is_decidedly_dragging()) && ui.data(|d| d.get_temp::<Option<f64>>(armed)) == Some(press) {
        // Once per press: AppKit's drag loop takes the mouse-up, so egui may still see the button
        // down afterwards.
        ui.data_mut(|d| d.remove::<Option<f64>>(armed));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_defaults_output() {
        assert_eq!(parse("Maximize\n"), DoubleClick::Zoom);
        assert_eq!(parse("Minimize\n"), DoubleClick::Minimize);
        assert_eq!(parse("None\n"), DoubleClick::Nothing);
        for junk in ["", "Fill", "minimize", "1", "The domain/default pair of (kCFPreferencesAnyApplication, X) does not exist"] {
            assert_eq!(parse(junk), DoubleClick::Zoom, "{junk:?}");
        }
    }

    #[test]
    fn zoom_toggles_and_minimize_minimizes() {
        use egui::ViewportCommand::{Maximized, Minimized};
        assert_eq!(double_click_command(DoubleClick::Zoom, false), Some(Maximized(true)));
        assert_eq!(double_click_command(DoubleClick::Zoom, true), Some(Maximized(false)));
        assert_eq!(double_click_command(DoubleClick::Minimize, false), Some(Minimized(true)));
        assert_eq!(double_click_command(DoubleClick::Nothing, true), None);
    }
}
