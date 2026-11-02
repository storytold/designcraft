//! macOS: the applications that can open a file, for Edit › Edit With (Launch Services, through
//! NSWorkspace's safe bindings in objc2-app-kit).

use std::path::{Path, PathBuf};

use designcraft_ui_egui::edit_original::AppChoice;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};

/// The longest list the submenu shows.
const MAX_APPS: usize = 64;

/// The applications that can open `file`: its default one first, then the rest by name. Two
/// copies of one application (another version, another volume) show once, the first found.
pub fn applications(file: &Path) -> Vec<AppChoice> {
    let Some(path) = file.to_str() else { return Vec::new() };
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let ws = NSWorkspace::sharedWorkspace();
    let default = ws.URLForApplicationToOpenURL(&url).and_then(|u| choice(&u));
    let mut rest: Vec<AppChoice> = ws.URLsForApplicationsToOpenURL(&url).to_vec().iter().filter_map(|u| choice(u)).collect();
    rest.sort_by_key(|a| a.name.to_lowercase());
    let mut out: Vec<AppChoice> = Vec::new();
    for a in default.into_iter().chain(rest) {
        if out.len() >= MAX_APPS {
            break;
        }
        if !out.iter().any(|o| o.path == a.path || o.name == a.name) {
            out.push(a);
        }
    }
    out
}

/// An application bundle's URL as a menu row: its name without `.app`.
fn choice(url: &NSURL) -> Option<AppChoice> {
    let path = PathBuf::from(url.path()?.to_string());
    let name = path.file_stem()?.to_string_lossy().to_string();
    (!name.is_empty()).then_some(AppChoice { name, path })
}
