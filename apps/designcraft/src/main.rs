//! DesignCraft desktop app.
//!
//! Usage: `designcraft [--control <port>] [--sample] [files…]`
//!
//! `--control <port>` (or `DESIGNCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `designcraft_ui_egui::control` for the methods.
//!
//! The graphics backend (DirectX 12, Vulkan, Metal or OpenGL) is chosen in `gpu` before the window
//! exists, with a fallback for a driver that crashes at start-up; `WGPU_BACKEND` overrides it.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "macos")]
mod apple_events;
mod control_server;
mod gpu;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;

use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};

struct App {
    app: DesignApp,
    #[cfg(target_os = "macos")]
    menu: Option<native_menu::NativeMenu>,
    /// Documents and quit requests from Finder, the Dock and Open With.
    #[cfg(target_os = "macos")]
    apple_events: fmv_macos_events::Inbox,
    /// The graphics start in progress (`gpu.json`) until a frame has been presented; see `gpu`.
    startup: Option<gpu::Startup>,
    /// Frames begun so far.
    frames: u32,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The third call: two frames went through `ui`, so the first was presented and the driver
        // survived it (a fault at the first present is what the fallback in `gpu` is for).
        if self.frames >= 2
            && let Some(s) = self.startup.as_mut()
        {
            s.presented();
            self.startup = None;
        }
        self.frames = self.frames.saturating_add(1);
        #[cfg(target_os = "macos")]
        {
            if self.menu.is_none() && std::env::var_os("DESIGNCRAFT_NO_NATIVE_MENU").is_none() {
                self.menu = Some(native_menu::NativeMenu::install(&mut self.app));
            }
            if let Some(m) = &mut self.menu {
                m.poll(&mut self.app, ctx);
            }
            apple_events::poll(&self.apple_events, &mut self.app, ctx);
        }
        self.app.logic(ctx);
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }
    fn on_exit(&mut self) {
        // A normal exit before the third frame is not a crash.
        if let Some(s) = self.startup.as_mut() {
            s.presented();
        }
        save_prefs(&self.app);
    }
}

/// The per-user settings directory: `ui.json`, `prefs.json`, `gpu.json` and `logs/` live here.
fn config_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support/DesignCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("DesignCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .map(|c| c.join("designcraft"))
    }
}

/// `DESIGNCRAFT_NO_PREFS`: read and write nothing in the settings directory (tests, demos).
fn no_prefs() -> bool {
    std::env::var_os("DESIGNCRAFT_NO_PREFS").is_some()
}

fn prefs_path() -> Option<std::path::PathBuf> {
    config_dir().map(|b| b.join("ui.json"))
}

fn load_prefs(app: &mut DesignApp) {
    if no_prefs() {
        return;
    }
    if let Some(p) = prefs_path()
        && let Ok(bytes) = std::fs::read(&p)
        && let Ok(ui) = serde_json::from_slice::<designcraft_ui_egui::UiState>(&bytes)
    {
        app.ui = ui;
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
        if let Ok(bytes) = serde_json::to_vec_pretty(&app.ui) {
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
        "icc" => &[OpenFilter { name: "ICC profile", extensions: &["icc", "icm"] }],
        "book" => &[OpenFilter { name: "Book", extensions: &["dcbook"] }],
        "xml" => &[OpenFilter { name: "XML", extensions: &["xml"] }],
        "library" => &[OpenFilter { name: "Object Library", extensions: &["dclib"] }],
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
            OpenFilter { name: "DesignCraft or IDML", extensions: &["designcraft", "idml"] },
            OpenFilter { name: "DesignCraft", extensions: &["designcraft"] },
            OpenFilter { name: "InDesign Markup (IDML)", extensions: &["idml"] },
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
    std::path::Path::new(name).extension().and_then(|e| e.to_str()).filter(|e| !e.is_empty() && !e.contains(char::is_whitespace))
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

/// The save dialog: the suggested name's extension as its filter, and the extension added to a
/// name typed without it. Adding it names another file than the one the dialog checked, so
/// replacing an existing one asks again.
fn pick_save(name: &str) -> Option<String> {
    let ext = suggested_extension(name);
    let mut dialog = rfd::FileDialog::new().set_file_name(name);
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
        write: Some(Box::new(|p: &str, b: &[u8]| std::fs::write(p, b).map_err(|e| e.to_string()))),
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

fn main() -> eframe::Result {
    // First, so every start-up record (and the engine's panic hook, installed with the first
    // Session) is captured; see `logging`.
    let logger = logging::install();
    let mut control_port = control_port_from("DESIGNCRAFT_CONTROL_PORT", std::env::var("DESIGNCRAFT_CONTROL_PORT").ok());
    let mut files = Vec::new();
    let mut sample = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = control_port_from("--control", args.next()),
            "--sample" => sample = true,
            "--version" => {
                println!("designcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let (Some(logger), Some(dir)) = (logger, config_dir()) {
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
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    options.viewport = options.viewport.with_app_id(APP_ID);
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    // The graphics backend, decided before the window exists: a driver that faults takes the
    // process down before any Rust code can catch it (see `gpu`).
    let gpu = gpu::Startup::begin(if no_prefs() { None } else { config_dir().map(|d| d.join(gpu::FILE)) }, env!("CARGO_PKG_VERSION"));
    if let Some(s) = &gpu
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
    {
        create.instance_descriptor.backends = s.backends();
    }
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
    // Registered before the event loop starts, so it catches the Finder event that launched the app
    // as well as later ones. Lives until the event loop returns; the app creator only borrows it.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let apple_events = &apple_events;
    eframe::run_native(
        "DesignCraft",
        options,
        Box::new(move |cc| {
            let mut gpu = gpu;
            if let Some(rs) = &cc.wgpu_render_state {
                // What a bug report needs to know about the graphics driver.
                let info = rs.adapter.get_info();
                log::info!("graphics: {} on {:?} ({:?}; driver {} {})", info.name, info.backend, info.device_type, info.driver, info.driver_info);
                if let (Some(s), Some(backend)) = (gpu.as_mut(), gpu::Backend::of(info.backend)) {
                    s.started_with(backend);
                }
            }
            let mut session = Session::new();
            // Crash recovery: reopen what a previous run left unsaved, then keep it current.
            session.recovery_dir = designcraft_engine::recovery::default_dir();
            let recovered = session.execute("file.recovery.open", &serde_json::json!({})).ok();
            let mut app = DesignApp::new(session, services());
            if let Some(line) = gpu.as_ref().and_then(gpu::Startup::status_line) {
                app.status(line);
            }
            // Recovered documents matter more than the graphics note; both are in the log.
            if let Some(n) = recovered.as_ref().and_then(|r| r["opened"].as_array()).map(Vec::len).filter(|n| *n > 0) {
                app.status(format!("Recovered {n} unsaved document{} from the last session.", if n == 1 { "" } else { "s" }));
            }
            load_prefs(&mut app);
            app.integrated_titlebar = cfg!(target_os = "macos");
            if let Some(port) = control_port {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            if sample {
                let _ = app.run("file.newSample", serde_json::json!({}));
            }
            for f in files {
                if let Err(e) = app.open_file("file.open", serde_json::json!({"path": f})) {
                    eprintln!("designcraft: {f}: {e}");
                }
            }
            Ok(Box::new(App {
                app,
                #[cfg(target_os = "macos")]
                menu: None,
                #[cfg(target_os = "macos")]
                apple_events: apple_events.connect(&cc.egui_ctx),
                startup: gpu,
                frames: 0,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
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
        assert!(documents.first().is_some_and(|filter| filter.extensions == ["designcraft", "idml"]));
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
}
