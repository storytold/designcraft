//! DesignCraft desktop app.
//!
//! Usage: `designcraft [--control <port>] [--sample] [files…]`
//!
//! `--control <port>` (or `DESIGNCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `designcraft_ui_egui::control` for the methods.
//!
//! The graphics backend (DirectX 12, Vulkan, Metal or OpenGL) and the adapter that draws the
//! window are chosen in `gpu`, with a fallback for a driver or adapter that fails at start-up;
//! `WGPU_BACKEND`, `WGPU_POWER_PREF` and `WGPU_ADAPTER_NAME` override them. Why the app stopped
//! is logged (`logging`), also when the window never opened.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "macos")]
mod apple_events;
mod control_server;
mod gpu;
mod gpu_limits;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;
mod profile;

use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};

struct App {
    app: DesignApp,
    #[cfg(target_os = "macos")]
    menu: Option<native_menu::NativeMenu>,
    /// The window's start: the adapter and backend chosen for it; see `gpu`.
    startup: std::sync::Arc<gpu::Startup>,
    /// Documents and quit requests from Finder, the Dock and Open With.
    #[cfg(target_os = "macos")]
    apple_events: fmv_macos_events::Inbox,
    /// The window's `movable` flag as last set; see `craft_window::title_bar`.
    #[cfg(target_os = "macos")]
    movable: Option<bool>,
    /// Frames begun so far.
    frames: u32,
    /// The device whose uncaptured errors are logged once the window is past its start-up frames
    /// (`gpu_limits::log_uncaptured_errors`). Until then wgpu's default handler panics, so a
    /// failing adapter still makes `gpu` start again on the next one.
    log_errors_after_startup: Option<eframe::wgpu::Device>,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.startup.enter();
        // Queue/deadline work only. A hidden `run_logic` tick also reports pass index zero, so
        // startup counters and native-menu polling must not live here.
        self.app.logic(ctx);
        self.startup.leave();
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.startup.enter();
        // `App::ui` is the only signal that a real layout pass ran. A discarded multipass first
        // pass reports index 0 once, so these once-per-frame counters/menu polls still run once.
        if ui.ctx().current_pass_index() == 0 {
            if let Some(note) = self.startup.take_notice() {
                self.app.status(note);
            }
            // The third UI callback: two frames went through `ui`, so the first was presented and
            // the driver survived it (a fault at the first present is what `gpu` falls back for).
            if self.frames == 2 {
                self.startup.presented();
            }
            self.frames = self.frames.saturating_add(1);
            if u64::from(self.frames) >= gpu::STARTUP_FRAMES
                && let Some(device) = self.log_errors_after_startup.take()
            {
                gpu_limits::log_uncaptured_errors(&device);
            }
            #[cfg(target_os = "macos")]
            {
                if self.menu.is_none() && std::env::var_os("DESIGNCRAFT_NO_NATIVE_MENU").is_none() {
                    self.menu = Some(native_menu::NativeMenu::install(&mut self.app, ui.ctx()));
                }
                if let Some(m) = &mut self.menu {
                    m.poll(&mut self.app, ui.ctx());
                }
                // Noted for a start again after a graphics failure, which reopens them (`gpu::restart`).
                self.startup.opening(apple_events::poll(&self.apple_events, &mut self.app, ui.ctx()));
            }
        }
        self.app.ui(ui);
        self.startup.leave();
        #[cfg(target_os = "macos")]
        if self.app.take_ime_discard() {
            discard_marked_text();
        }
        #[cfg(target_os = "macos")]
        craft_window::title_bar::sync_movable(ui.ctx(), &mut self.movable);
    }
    fn on_exit(&mut self) {
        // A normal exit before the third frame is not a crash.
        self.startup.presented();
        if let Err(e) = preserve_documents_on_exit(&mut self.app) {
            log::error!("Couldn't preserve documents on exit: {e}");
            eprintln!("designcraft: couldn't preserve documents on exit: {e}");
        }
        save_prefs(&self.app);
    }
}

/// Tell the macOS input method to drop its composition (the Type tool kept the marked text as
/// typed). winit's IME toggle only clears its own copy, so the IME would type it again.
#[cfg(target_os = "macos")]
fn discard_marked_text() {
    if let Some(mtm) = objc2::MainThreadMarker::new()
        && let Some(ic) = objc2_app_kit::NSTextInputContext::currentInputContext(mtm)
    {
        ic.discardMarkedText();
    }
}

/// Orderly OS termination can bypass egui's cancelable close event. Finish only a complete,
/// current quit plan; otherwise preserve every live document, including edits after the timer.
fn preserve_documents_on_exit(app: &mut DesignApp) -> Result<(), String> {
    app.finish_confirmed_quit()?;
    if app.session.documents().is_empty() {
        return Ok(());
    }
    if app.session.recovery_dir.is_none() {
        return if app.session.documents().iter().any(|d| d.is_dirty()) {
            Err("Recovery is unavailable; unsaved documents could not be preserved on exit".into())
        } else {
            Ok(())
        };
    }
    // Also discard stale recovery for clean documents (for example, Undo back to the saved
    // state after the last timer tick). Never choose a fallback recovery path here.
    // A forced OS exit can arrive while the PDF modal blocks ordinary UI commands.
    // Recovery is the final safety write, so dispatch it directly to the engine.
    app.session.execute("file.recovery.save", &serde_json::json!({})).map(|_| ()).map_err(|e| e.to_string())
}

/// `DESIGNCRAFT_NO_PREFS`: read and write nothing in the settings directory (tests, demos).
fn no_prefs() -> bool {
    std::env::var_os("DESIGNCRAFT_NO_PREFS").is_some()
}

fn prefs_path() -> Option<std::path::PathBuf> {
    profile::config_dir().map(|b| b.join("ui.json"))
}

/// Set when `ui.json` could not be read in full and no copy of it could be kept: the file is
/// then left as it is rather than overwritten.
static KEEP_UI_PREFS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Copy an unreadable (or partly unreadable) `ui.json` beside it, as `ui.unreadable.json` or the
/// first free `ui.unreadable-N.json`, before the next save replaces it. A newer DesignCraft may
/// have written it.
fn keep_unreadable_prefs(path: &std::path::Path, bytes: &[u8]) {
    use std::io::Write as _;
    for n in 1..100u32 {
        let name = if n == 1 { "ui.unreadable.json".to_string() } else { format!("ui.unreadable-{n}.json") };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(path.with_file_name(name)) {
            Ok(mut file) => {
                if file.write_all(bytes).is_ok() {
                    return;
                }
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => break,
        }
    }
    KEEP_UI_PREFS.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn load_prefs(app: &mut DesignApp) {
    if no_prefs() {
        return;
    }
    if let Some(p) = prefs_path()
        && let Ok(bytes) = std::fs::read(&p)
    {
        match designcraft_ui_egui::UiState::read_preferences(&bytes) {
            Ok(read) => {
                app.ui = read.ui;
                if !read.intact {
                    keep_unreadable_prefs(&p, &bytes);
                }
            }
            Err(_) => keep_unreadable_prefs(&p, &bytes),
        }
        app.sync_keymap();
    }
    // Engine preferences (Preferences dialog, favourites…) live beside the UI state.
    if let Some(p) = prefs_path().map(|p| p.with_file_name("prefs.json"))
        && let Ok(bytes) = std::fs::read(&p)
        && let Ok(prefs) = serde_json::from_slice::<designcraft_engine::Prefs>(&bytes)
    {
        app.session.prefs = prefs;
    }
}

fn save_prefs(app: &DesignApp) {
    if no_prefs() {
        return;
    }
    if let Some(p) = prefs_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(std::path::Path::new(".")));
        if !KEEP_UI_PREFS.load(std::sync::atomic::Ordering::Relaxed)
            && let Ok(bytes) = serde_json::to_vec_pretty(&app.ui)
        {
            let _ = std::fs::write(&p, bytes);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&app.session.prefs) {
            let _ = std::fs::write(p.with_file_name("prefs.json"), bytes);
        }
    }
}

/// One file-type row in an open dialog. `open_filters` is what `pick_open` applies.
struct OpenFilter {
    name: &'static str,
    extensions: &'static [&'static str],
}

fn open_filters(purpose: &str) -> &'static [OpenFilter] {
    match purpose {
        "swatches" => &[OpenFilter { name: "Swatch Exchange (ASE)", extensions: &["ase"] }],
        "script" => &[OpenFilter { name: "Script", extensions: &["dcscript", "txt", "json"] }],
        "shortcuts" => &[OpenFilter { name: "Shortcut set (JSON, InDesign text)", extensions: &["json", "txt"] }],
        "icc" => &[OpenFilter { name: "ICC profile", extensions: &["icc", "icm"] }],
        "book" => &[OpenFilter { name: "Book", extensions: &["dcbook"] }],
        "xml" => &[OpenFilter { name: "XML", extensions: &["xml"] }],
        "library" => &[OpenFilter { name: "Object Library", extensions: &["dclib"] }],
        "relink" => &[OpenFilter {
            name: "Graphics",
            extensions: &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "psd", "svg", "pdf", "ai", "eps"],
        }],
        "dataMerge" => &[OpenFilter { name: "Data source (CSV, TSV, text, Excel)", extensions: &["csv", "tsv", "tab", "txt", "xlsx"] }],
        "place" => &[
            OpenFilter {
                name: "Graphics and text",
                extensions: &[
                    "png",
                    "jpg",
                    "jpeg",
                    "gif",
                    "webp",
                    "tif",
                    "tiff",
                    "bmp",
                    "psd",
                    "svg",
                    "pdf",
                    "ai",
                    "eps",
                    "txt",
                    "docx",
                    "rtf",
                    "md",
                    "xlsx",
                    "idml",
                    "designcraft",
                    "mp4",
                    "m4v",
                    "mov",
                    "webm",
                    "mp3",
                    "m4a",
                    "wav",
                    "ogg",
                ],
            },
            OpenFilter {
                name: "Graphics",
                extensions: &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "psd", "svg", "pdf", "ai", "eps"],
            },
            OpenFilter { name: "Text (Word, RTF, plain, Excel)", extensions: &["docx", "rtf", "txt", "md", "xlsx"] },
            OpenFilter { name: "Video and sound", extensions: &["mp4", "m4v", "mov", "webm", "mp3", "m4a", "wav", "ogg"] },
        ],
        _ => &[
            OpenFilter { name: "DesignCraft, IDML or InDesign", extensions: &["designcraft", "idml", "indd", "indt"] },
            OpenFilter { name: "DesignCraft", extensions: &["designcraft"] },
            OpenFilter { name: "InDesign Markup (IDML)", extensions: &["idml"] },
            OpenFilter { name: "InDesign document or template (INDD, INDT)", extensions: &["indd", "indt"] },
            // Files saved without an extension; `file.open` tells the formats apart by content.
            OpenFilter { name: "All files", extensions: &[ALL_FILES] },
        ],
    }
}

/// The "All files" pattern. The macOS panel merges every filter into one list of allowed types,
/// which takes type identifiers as well as extensions: `public.data` admits any file.
const ALL_FILES: &str = if cfg!(target_os = "macos") { "public.data" } else { "*" };

/// The extension of the file name a save dialog suggests (`Brochure.designcraft`), if it has one.
/// The dialog filters on it, so the macOS and Windows dialogs add it to a name typed without it.
fn suggested_extension(name: &str) -> Option<&str> {
    std::path::Path::new(name).extension().and_then(std::ffi::OsStr::to_str).filter(|e| !e.is_empty() && !e.contains(char::is_whitespace))
}

/// The file a save dialog's answer writes: `picked` with the suggested extension added when it has
/// none (Linux dialogs don't add it). A document always ends in `.designcraft`, as `file.save`
/// writes it.
fn picked_save_path(picked: std::path::PathBuf, ext: Option<&str>) -> std::path::PathBuf {
    let Some(ext) = ext else { return picked };
    if ext.eq_ignore_ascii_case("designcraft") {
        let p = picked.to_string_lossy();
        return designcraft_engine::cmd::with_document_extension(&p).into();
    }
    if picked.extension().is_some() || picked.file_name().is_none() {
        return picked;
    }
    let mut s = picked.into_os_string();
    s.push(".");
    s.push(ext);
    s.into()
}

/// The folder a save dialog starts in and the name it suggests. A suggestion with a folder in
/// front (the document's, or the last one used) starts there while that folder still exists;
/// otherwise the system picks, as it did before.
fn save_dialog_start(suggested: &str) -> (Option<std::path::PathBuf>, String) {
    let path = std::path::Path::new(suggested);
    let name = path.file_name().map_or_else(|| suggested.to_string(), |n| n.to_string_lossy().to_string());
    let folder = path.parent().filter(|f| !f.as_os_str().is_empty() && f.is_dir()).map(std::path::Path::to_path_buf);
    (folder, name)
}

/// The save dialog: the suggested name's extension as its filter, and the extension added to a
/// name typed without it. Adding it names another file than the one the dialog checked, so
/// replacing an existing one asks again.
fn pick_save(suggested: &str) -> Option<String> {
    let (folder, name) = save_dialog_start(suggested);
    let name = name.as_str();
    let ext = suggested_extension(name);
    let mut dialog = rfd::FileDialog::new().set_file_name(name);
    if let Some(folder) = folder {
        dialog = dialog.set_directory(folder);
    }
    if let Some(ext) = ext {
        dialog = dialog.add_filter(ext.to_ascii_uppercase(), &[ext]);
    }
    let picked = dialog.save_file()?;
    let path = picked_save_path(picked.clone(), ext);
    if path != picked && path.exists() {
        let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Replace File?")
            .set_description(format!("“{file}” already exists. Do you want to replace it?"))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer != rfd::MessageDialogResult::Yes {
            return None;
        }
    }
    Some(path.to_string_lossy().to_string())
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|purpose: &str| {
            let mut dialog = rfd::FileDialog::new();
            for filter in open_filters(purpose) {
                dialog = dialog.add_filter(filter.name, filter.extensions);
            }
            dialog.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(pick_save)),
        read: Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string()))),
        write: Some(Box::new(|p: &str, b: &[u8]| designcraft_engine::io::write_file(p, b))),
        background_write: Some(std::sync::Arc::new(|p: &str, b: &[u8]| designcraft_engine::io::write_file(p, b))),
        clipboard_text: Some(Box::new(|| arboard::Clipboard::new().and_then(|mut c| c.get_text()).ok())),
        ..Default::default()
    }
}

/// Desktop app ID: the Linux `.desktop` file name and hicolor icon name (Wayland matches windows by it).
const APP_ID: &str = "ai.storyteller.designcraft";

/// The window, Dock, taskbar and Alt-Tab icon. macOS gets Apple's icon grid (transparent margin);
/// elsewhere the full tile. Regenerate with `packaging/icons.sh`.
fn app_icon() -> Option<egui::IconData> {
    #[cfg(target_os = "macos")]
    let png: &[u8] = include_bytes!("../../../assets/app-icon/designcraft-macos-512.png");
    #[cfg(not(target_os = "macos"))]
    let png: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.designcraft.png");
    eframe::icon_data::from_png_bytes(png).map_err(|e| log::warn!("app icon: {e}")).ok()
}

/// The control port from `--control <port>` or `DESIGNCRAFT_CONTROL_PORT` (`source`); a value that
/// is not a port is logged and ignored instead of silently starting no control channel.
fn control_port_from(source: &str, value: Option<String>) -> Option<u16> {
    let value = value?;
    let port = value.trim().parse().ok();
    if port.is_none() {
        log::warn!("{source}: {value:?} is not a port number; the control channel is off");
    }
    port
}

fn main() -> std::process::ExitCode {
    // Windows: the update helper is a copy of this program started with `--finish-update`.
    #[cfg(windows)]
    if let Some(result) = craft_update::self_update::finish(&craft_update::suite::DESIGNCRAFT, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        std::process::exit(i32::from(result.is_err()));
    }
    // First, so every start-up record (and the engine's panic hook, installed with the first
    // Session) is captured; see `logging`.
    let logger = logging::install();
    let mut control_port = control_port_from("DESIGNCRAFT_CONTROL_PORT", std::env::var("DESIGNCRAFT_CONTROL_PORT").ok());
    let mut files = Vec::new();
    let mut sample = false;
    let mut skip = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = control_port_from("--control", args.next()),
            "--sample" => sample = true,
            "--version" => {
                println!("designcraft {}", env!("CARGO_PKG_VERSION"));
                return std::process::ExitCode::SUCCESS;
            }
            _ => match gpu::skip_arg(&a) {
                Some(list) => skip = list,
                None => files.push(a),
            },
        }
    }
    // A portable.txt whose DesignCraftData folder cannot be written: the platform folders are used
    // instead (see `profile`), and the user is told, in the log and in the status bar.
    let portable_note = profile::unwritable_portable_dir().map(|dir| {
        log::warn!("portable mode is off: {} cannot be written; settings and recovery use the usual folders", dir.display());
        format!("Portable mode is off: {} cannot be written, so settings are kept in the usual place.", dir.display())
    });
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let (Some(logger), Some(dir)) = (logger, profile::config_dir()) {
        match logger.attach_dir(&dir.join("logs")) {
            Ok(path) => log::info!("DesignCraft {}, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
            // Standard error only by now (`attach_dir` gave up on the file); unlike `eprintln!`, never panics.
            Err(e) => log::warn!("no log file: {e}"),
        }
    }
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DesignCraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_decorations(!cfg!(target_os = "windows"))
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        // DesignCraft's own FontDb supplies script-specific system fallbacks; egui's system-font
        // provider would put its discovery ahead of that order and change line metrics.
        system_font_fallback: false,
        ..Default::default()
    };
    options.viewport = options.viewport.with_app_id(APP_ID);
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    // The adapter's largest texture side instead of eframe's 8192, so a window across several
    // monitors keeps its surface (see `gpu_limits`).
    gpu_limits::request_adapter_texture_size(&mut options);
    // The graphics backend, decided before the window exists: a driver that faults takes the
    // process down before any Rust code can catch it (see `gpu::backend`).
    let fallback = gpu::backend::Fallback::begin(
        if no_prefs() { None } else { profile::config_dir().map(|d| d.join(gpu::backend::FILE)) },
        env!("CARGO_PKG_VERSION"),
        &gpu::backend::skipped_backends(&skip),
    );
    let instance_backends = fallback.as_ref().map(gpu::backend::Fallback::backends);
    let startup = std::sync::Arc::new(gpu::Startup::new(fallback, skip.clone()));
    // The adapter: the GPU that drives the (primary) display unless `WGPU_POWER_PREF` chooses a
    // kind; a GPU without a monitor reset its driver and took every monitor down. Logged first:
    // the first question in every black-window report.
    let power_env = eframe::wgpu::PowerPreference::from_env();
    let power = gpu::power_preference(power_env);
    let displays = gpu::preferred_displays(power_env);
    let chosen_gpu = !gpu::automatic(power_env);
    if chosen_gpu {
        log::info!("graphics processor chosen by WGPU_POWER_PREF ({power:?}): which GPU drives the display isn't considered");
    } else {
        let listed: Vec<String> = displays.iter().map(ToString::to_string).collect();
        log::info!("display GPUs (PCI vendor:device): {}", if listed.is_empty() { "unknown".to_string() } else { listed.join(", ") });
    }
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        if let Some(backends) = instance_backends {
            create.instance_descriptor.backends = backends;
        }
        create.instance_descriptor.flags = gpu::instance_flags();
        // Only adapters that can show the window, in the order `gpu` gives.
        create.native_adapter_selector = Some(gpu::selector(power, displays, startup.clone()));
    }
    gpu::watch_panics();
    gpu::watch_first_frame(startup.clone());
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping images from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; DESIGNCRAFT_WAYLAND=1
    // keeps the native Wayland backend.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("DESIGNCRAFT_WAYLAND").is_none() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // AppKit Dock Quit otherwise terminates before egui can cancel the close. Route it through
    // ordinary CloseRequested events, preserving the same transactional Save/Don't Save flow.
    #[cfg(target_os = "macos")]
    {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_close_requested_on_terminate(true);
        }));
    }
    // Registered before the event loop starts, so it catches the Finder event that launched the app
    // as well as later ones. Lives until the event loop returns; the app creator only borrows it.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let events = &apple_events;
    let created = startup.clone();
    // A panic that ends the window (egui-wgpu's own) is caught here: never a crash.
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            "DesignCraft",
            options,
            Box::new(move |cc| {
                let mut restarted = None;
                let mut device = None;
                if let Some(rs) = &cc.wgpu_render_state {
                    device = Some(rs.device.clone());
                    // What a bug report needs to know about the graphics driver.
                    let info = rs.adapter.get_info();
                    log::info!(
                        "graphics: {} on {:?} ({:?}; driver {} {}; power preference {power:?})",
                        info.name,
                        info.backend,
                        info.device_type,
                        info.driver,
                        info.driver_info
                    );
                    created.started_with(info.backend);
                    created.created(&cc.egui_ctx);
                    if !skip.is_empty() {
                        // A processor chosen by `WGPU_POWER_PREF` can be the one that can't show the window.
                        let hint = if chosen_gpu { " (without WGPU_POWER_PREF it uses the one that drives your display)" } else { "" };
                        restarted = Some(format!(
                            "The graphics processor tried first couldn't show the window, so DesignCraft started again on {} ({:?}, {:?}){hint}.",
                            info.name.trim(),
                            info.backend,
                            info.device_type
                        ));
                    }
                }
                let mut session = Session::new();
                // Crash recovery: reopen what a previous run left unsaved, then keep it current.
                session.recovery_dir = profile::recovery_dir();
                let recovered = session.execute("file.recovery.open", &serde_json::json!({})).ok();
                let mut app = DesignApp::new(session, services());
                app.set_startup_update_check(true);
                if let Some(line) = restarted.or_else(|| created.status_line()) {
                    app.status(line);
                }
                // Recovered documents matter more than the graphics note; both are in the log.
                if let Some(n) = recovered.as_ref().and_then(|r| r["opened"].as_array()).map(Vec::len).filter(|n| *n > 0) {
                    app.status(format!("Recovered {n} unsaved document{} from the last session.", if n == 1 { "" } else { "s" }));
                }
                load_prefs(&mut app);
                // After `load_prefs`, which replaces the UI state and with it the status line.
                if let Some(note) = portable_note {
                    app.status(note);
                }
                app.integrated_titlebar = cfg!(target_os = "macos");
                app.custom_titlebar = cfg!(target_os = "windows");
                if let Some(port) = control_port {
                    let rx = control_server::start(port, cc.egui_ctx.clone());
                    app = app.with_control(rx);
                }
                if sample {
                    let _ = app.run("file.newSample", serde_json::json!({}));
                }
                for f in files {
                    if let Err(e) = app.open_file("file.open", serde_json::json!({"path": f})) {
                        log::warn!("{f}: {e}");
                    }
                }
                // The first frame is due from now (`gpu::watch_first_frame`).
                created.ready();
                Ok(Box::new(App {
                    app,
                    #[cfg(target_os = "macos")]
                    menu: None,
                    startup: created,
                    #[cfg(target_os = "macos")]
                    apple_events: events.connect(&cc.egui_ctx),
                    #[cfg(target_os = "macos")]
                    movable: None,
                    frames: 0,
                    log_errors_after_startup: device,
                }))
            }),
        )
    }))
    .map_err(|payload| designcraft_engine::guard::panic_message(payload.as_ref()));
    // The window's end, or a graphics failure that starts the app again. A failure is logged:
    // standard error and the log file, since a Windows start has no console to show it.
    // Documents that arrived after the last frame, for a start again to open too.
    #[cfg(target_os = "macos")]
    startup.opening(apple_events.queued());
    match gpu::finish(run, &startup) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            log::error!("DesignCraft stopped: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_unreadable_ui_json_is_kept_beside_it() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("designcraft-ui-prefs-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("ui.json");
        super::keep_unreadable_prefs(&path, b"{\"docking\": {\"root\": {\"FutureLayout\": {}}}}");
        super::keep_unreadable_prefs(&path, b"not json");
        let first = std::fs::read(dir.join("ui.unreadable.json")).unwrap();
        let second = std::fs::read(dir.join("ui.unreadable-2.json")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(first.starts_with(b"{\"docking\""), "the first copy is not replaced");
        assert_eq!(second, b"not json");
        assert!(!super::KEEP_UI_PREFS.load(std::sync::atomic::Ordering::Relaxed), "ui.json can be saved once a copy is kept");
    }

    #[test]
    fn relink_open_dialog_lists_graphics() {
        let filters = super::open_filters("relink");
        let exts: Vec<&str> = filters.iter().flat_map(|filter| filter.extensions.iter().copied()).collect();
        for ext in ["png", "jpg", "tif", "psd", "svg", "pdf", "ai", "eps"] {
            assert!(exts.contains(&ext), "{ext} is missing from the relink dialog: {exts:?}");
        }
        assert!(!exts.contains(&"designcraft") && !exts.contains(&"idml"));
    }

    #[test]
    fn control_port_values_that_are_not_ports_are_ignored() {
        assert_eq!(super::control_port_from("--control", Some("7979".into())), Some(7979));
        assert_eq!(super::control_port_from("--control", Some(" 7979 ".into())), Some(7979));
        assert_eq!(super::control_port_from("--control", None), None);
        for bad in ["", "abc", "-1", "65536", "99999999999999999999"] {
            assert_eq!(super::control_port_from("--control", Some(bad.into())), None, "{bad:?}");
        }
    }

    #[test]
    fn data_merge_open_dialog_lists_table_extensions() {
        let filters = super::open_filters("dataMerge");
        let exts: Vec<&str> = filters.iter().flat_map(|filter| filter.extensions.iter().copied()).collect();
        for ext in ["csv", "tsv", "tab", "txt", "xlsx"] {
            assert!(exts.contains(&ext), "{ext} is missing from the dataMerge dialog: {exts:?}");
        }
        assert!(!exts.iter().any(|ext| *ext == "designcraft" || *ext == "idml"), "dataMerge must not fall through to the document filters: {exts:?}");
        let place = super::open_filters("place");
        assert!(place.len() > 1, "the place dialog keeps a filter for each kind of file");
        assert!(place.iter().any(|filter| filter.extensions.contains(&"png")));
        let documents = super::open_filters("");
        for ext in ["designcraft", "idml", "indd", "indt"] {
            assert!(documents.first().is_some_and(|filter| filter.extensions.contains(&ext)), "{ext} is missing from the first Open filter");
        }
        assert!(documents.last().is_some_and(|filter| filter.extensions == [super::ALL_FILES]), "files without an extension can be picked");
    }

    #[test]
    fn save_dialog_answers_get_the_suggested_extension() {
        use std::path::PathBuf;
        let save = |picked: &str, name: &str| super::picked_save_path(PathBuf::from(picked), super::suggested_extension(name));
        assert_eq!(save("/d/Brochure", "Untitled.designcraft"), PathBuf::from("/d/Brochure.designcraft"));
        assert_eq!(save("/d/Brochure.idml", "Untitled.designcraft"), PathBuf::from("/d/Brochure.idml.designcraft"));
        assert_eq!(save("/d/Brochure.designcraft", "Untitled.designcraft"), PathBuf::from("/d/Brochure.designcraft"));
        assert_eq!(save("/d/Brochure", "Untitled.pdf"), PathBuf::from("/d/Brochure.pdf"));
        assert_eq!(save("/d/Brochure.txt", "Untitled.rtf"), PathBuf::from("/d/Brochure.txt"), "another export format is kept");
        assert_eq!(save("/d/Package", "Untitled 1.2 Folder"), PathBuf::from("/d/Package"), "a folder has no extension");
    }

    /// #1653: the save dialog opens in the folder the suggestion names, unless it is gone.
    #[test]
    fn save_dialog_starts_in_the_suggested_folder_while_it_exists() {
        let folder = std::env::temp_dir();
        let suggested = folder.join("Brochure.pdf");
        assert_eq!(super::save_dialog_start(&suggested.to_string_lossy()), (Some(folder.clone()), "Brochure.pdf".to_string()));
        let gone = folder.join(format!("designcraft-gone-{}", std::process::id())).join("Brochure.pdf");
        assert_eq!(super::save_dialog_start(&gone.to_string_lossy()), (None, "Brochure.pdf".to_string()), "a folder deleted since: the system's own");
        assert_eq!(super::save_dialog_start("Untitled 1.2 Folder"), (None, "Untitled 1.2 Folder".to_string()), "a bare name: the system's own");
    }
}

#[cfg(all(test, unix))]
mod atomic_writer_tests {
    #[test]
    fn desktop_write_preserves_an_existing_reader_and_private_symlink_target() {
        use std::io::Read;
        use std::os::unix::fs::{PermissionsExt, symlink};
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("designcraft-desktop-atomic-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let fixture = Fixture(dir);
        let target = fixture.0.join(format!("{}.png", "a".repeat(243)));
        let old = b"previous private export";
        std::fs::write(&target, old).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let alias = fixture.0.join("linked.png");
        symlink(target.file_name().unwrap(), &alias).unwrap();
        let mut reader = std::fs::File::open(&target).unwrap();
        let mut write = super::services().write.unwrap();
        write(alias.to_str().unwrap(), b"complete new export").unwrap();
        let mut observed = Vec::new();
        reader.read_to_end(&mut observed).unwrap();
        assert!(observed == old, "desktop export changed bytes under an existing reader");
        assert_eq!(std::fs::read(&target).unwrap(), b"complete new export");
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(std::fs::symlink_metadata(alias).unwrap().file_type().is_symlink());
    }
}

#[cfg(test)]
mod exit_recovery_tests {
    use super::*;
    use serde_json::json;

    fn dirty_app(label: &str) -> (DesignApp, std::path::PathBuf) {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
        let uid = app.session.active().unwrap().uid;
        let dir = std::env::temp_dir().join(format!("designcraft-exit-{label}-{}-{uid}", std::process::id()));
        assert!(!dir.exists());
        app.session.recovery_dir = Some(dir.clone());
        (app, dir)
    }

    fn close_frame(app: &mut DesignApp, ctx: &egui::Context, close: bool) {
        let mut raw = egui::RawInput::default();
        if close {
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        }
        let mut out = ctx.run_ui(raw, |ui| app.logic(ui.ctx()));
        out.textures_delta.clear();
    }

    #[test]
    fn exit_recovery_preserves_latest_dirty_document_without_mutating_it() {
        let (mut app, dir) = dirty_app("latest");
        app.run("file.recovery.save", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
        let d = app.session.active().unwrap();
        let (uid, revision, saved_revision, doc, selection, undo, redo) =
            (d.uid, d.revision, d.saved_revision, d.doc.clone(), d.selection.items.clone(), d.history.undo.len(), d.history.redo.len());
        preserve_documents_on_exit(&mut app).unwrap();
        let current = app.session.active().unwrap();
        assert_eq!(
            (current.uid, current.revision, current.saved_revision, &current.selection.items, current.history.undo.len(), current.history.redo.len()),
            (uid, revision, saved_revision, &selection, undo, redo)
        );
        assert!(std::sync::Arc::ptr_eq(&current.doc, &doc));
        assert!(current.is_dirty());
        let recovered = designcraft_engine::cmd::from_bytes(&std::fs::read(dir.join(format!("{uid}.designcraft"))).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(&recovered).unwrap(), serde_json::to_value(doc.as_ref()).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exit_recovery_preserves_dirty_documents_during_background_pdf_export() {
        let (mut app, dir) = dirty_app("pdf-pending");
        let uid = app.session.active().unwrap().uid;
        let doc = app.session.active().unwrap().doc.clone();
        let (started, wait_started) = std::sync::mpsc::sync_channel(1);
        let (release, wait_release) = std::sync::mpsc::sync_channel(1);
        let wait_release = std::sync::Mutex::new(wait_release);
        app.services.background_write = Some(std::sync::Arc::new(move |_, _| {
            started.send(()).map_err(|e| e.to_string())?;
            wait_release.lock().unwrap().recv().map_err(|e| e.to_string())
        }));
        let queued = app.run("app.exportPdf", json!({"path":dir.join("export.pdf").to_string_lossy()})).unwrap();
        assert_eq!(queued["queued"], true);
        wait_started.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        assert!(app.run("file.recovery.save", json!({})).is_err(), "the modal still blocks ordinary commands");
        let saved = preserve_documents_on_exit(&mut app);
        release.send(()).unwrap();
        saved.unwrap();
        let recovered = designcraft_engine::cmd::from_bytes(&std::fs::read(dir.join(format!("{uid}.designcraft"))).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(&recovered).unwrap(), serde_json::to_value(doc.as_ref()).unwrap());
        assert!(std::sync::Arc::ptr_eq(&app.session.active().unwrap().doc, &doc));
        assert!(app.session.active().unwrap().is_dirty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exit_recovery_removes_stale_data_after_undo_to_saved_state() {
        let (mut app, dir) = dirty_app("clean");
        let uid = app.session.active().unwrap().uid;
        app.run("file.recovery.save", json!({})).unwrap();
        let entry = dir.join(format!("{uid}.designcraft"));
        assert!(entry.exists());
        app.run("edit.undo", json!({})).unwrap();
        assert!(!app.session.active().unwrap().is_dirty());
        let doc = app.session.active().unwrap().doc.clone();
        preserve_documents_on_exit(&mut app).unwrap();
        assert!(!entry.exists(), "clean exit must not resurrect the pre-Undo edits");
        assert!(std::sync::Arc::ptr_eq(&app.session.active().unwrap().doc, &doc));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exit_recovery_reports_unavailable_or_failed_storage_without_changing_documents() {
        let (mut app, dir) = dirty_app("failure");
        let doc = app.session.active().unwrap().doc.clone();
        app.session.recovery_dir = None;
        assert!(preserve_documents_on_exit(&mut app).unwrap_err().contains("unavailable"));
        std::fs::create_dir(&dir).unwrap();
        let blocked = dir.join("not-a-directory");
        std::fs::write(&blocked, b"preserve this file").unwrap();
        app.session.recovery_dir = Some(blocked.clone());
        assert!(preserve_documents_on_exit(&mut app).is_err());
        assert_eq!(std::fs::read(&blocked).unwrap(), b"preserve this file");
        assert!(std::sync::Arc::ptr_eq(&app.session.active().unwrap().doc, &doc));
        assert!(app.session.active().unwrap().is_dirty());
        assert_eq!(app.session.documents().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn exit_recovery_respects_complete_incomplete_stale_and_new_document_quit_plans() {
        for case in ["complete", "incomplete", "stale", "new"] {
            let (mut app, dir) = dirty_app(case);
            if case == "incomplete" {
                app.run("file.new", json!({})).unwrap();
                app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
            }
            app.run("file.recovery.save", json!({})).unwrap();
            let first_uid = app.session.documents()[0].uid;
            let ctx = egui::Context::default();
            close_frame(&mut app, &ctx, true);
            app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
            designcraft_ui_egui::dialogs::confirm(&mut app).unwrap();
            close_frame(&mut app, &ctx, false);
            if case == "stale" {
                app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
            } else if case == "new" {
                app.run("file.new", json!({})).unwrap();
                app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
            }
            let active = app.session.active().unwrap().uid;
            let before: Vec<_> = app
                .session
                .documents()
                .iter()
                .map(|d| (d.uid, d.revision, d.doc.clone(), d.selection.items.clone(), d.history.undo.len(), d.history.redo.len()))
                .collect();
            preserve_documents_on_exit(&mut app).unwrap();
            if case == "complete" {
                assert!(app.session.documents().is_empty());
                assert!(!dir.join(format!("{first_uid}.designcraft")).exists(), "accepted Don't Save must not be recovered");
            } else {
                assert_eq!(app.session.documents().len(), before.len(), "{case}");
                assert_eq!(app.session.active().unwrap().uid, active, "{case}");
                for (d, (uid, revision, doc, selection, undo, redo)) in app.session.documents().iter().zip(before) {
                    assert_eq!(
                        (d.uid, d.revision, &d.selection.items, d.history.undo.len(), d.history.redo.len()),
                        (uid, revision, &selection, undo, redo),
                        "{case}"
                    );
                    assert!(std::sync::Arc::ptr_eq(&d.doc, &doc), "{case}");
                    let recovered = designcraft_engine::cmd::from_bytes(&std::fs::read(dir.join(format!("{uid}.designcraft"))).unwrap()).unwrap();
                    assert_eq!(serde_json::to_value(&recovered).unwrap(), serde_json::to_value(doc.as_ref()).unwrap(), "{case}");
                }
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
