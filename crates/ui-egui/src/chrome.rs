//! Window chrome: application bar (with menus), Control panel, document tabs, status bar, start screen.

use designcraft_doc::Align;
use designcraft_geom::Unit;
#[cfg(test)]
use egui::vec2;
use egui::{Sense, Stroke};
use serde_json::json;

use crate::DesignApp;
#[cfg(test)]
use crate::icons;
use crate::panels;
use crate::theme::Tokens;
#[cfg(test)]
use crate::widgets::NumField;

mod app_bar;
mod control_bar;
mod start_screen;
mod status_bar;

pub(crate) use app_bar::resize_borders;
#[cfg(test)]
use app_bar::search_id;
pub use app_bar::{app_bar, search_had_focus};
pub use control_bar::control_bar;
#[cfg(test)]
use control_bar::icon_field;
pub(crate) use start_screen::recent_file_stat;
pub use start_screen::start_screen;
#[cfg(test)]
use start_screen::{RECENT_STAT_MS, recent_file_stat_at};
#[cfg(test)]
use status_bar::DOC_TAB_MAX_W;
pub use status_bar::{doc_tabs, status_bar};

/// Height of the application bar on macOS, where it is the window's title bar (#333). AppKit draws
/// the traffic lights itself, centred in the standard 28 pt title-bar band at the top of the
/// window; moving them would take AppKit calls (`unsafe`, which the workspace forbids). So the bar
/// is that band: the lights sit vertically centred in it, with the bar's controls beside them.
pub const MAC_APP_BAR: f32 = 28.0;
/// The traffic lights as AppKit lays them out: button diameter, centre-to-centre pitch and the
/// first button's left edge (8–10 pt depending on the macOS version; the larger value keeps the
/// gap after them from shrinking).
const MAC_LIGHT: f32 = 12.0;
const MAC_LIGHT_PITCH: f32 = 20.0;
const MAC_LIGHTS_LEFT: f32 = 10.0;
/// Where the bar's content starts to the right of the traffic lights: the end of the three lights
/// plus the same gap the lights keep from the top of the window.
pub const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_APP_BAR - MAC_LIGHT) / 2.0;

impl DesignApp {
    fn session_home(&mut self) {
        self.ui.status = crate::i18n::tr(&self.ui.language, "Home").into();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod successor_tests;
#[cfg(test)]
mod tests;
