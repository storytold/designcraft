//! DesignCraft desktop app.
//!
//! Usage: `designcraft [--control <port>] [--sample] [files…]`
//!
//! `--control <port>` (or `DESIGNCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `designcraft_ui_egui::control` for the methods.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control_server;
#[cfg(target_os = "macos")]
mod native_menu;

use designcraft_engine::Session;
use designcraft_ui_egui::{DesignApp, Services};

struct App(DesignApp, #[cfg(target_os = "macos")] Option<native_menu::NativeMenu>);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            if self.1.is_none() && std::env::var_os("DESIGNCRAFT_NO_NATIVE_MENU").is_none() {
                self.1 = Some(native_menu::NativeMenu::install(&mut self.0));
            }
            if let Some(m) = &mut self.1 {
                m.poll(&mut self.0);
            }
        }
        self.0.logic(ctx);
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
    fn on_exit(&mut self) {
        save_prefs(&self.0);
    }
}

fn prefs_path() -> Option<std::path::PathBuf> {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support/DesignCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("DesignCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .map(|c| c.join("designcraft"))
    };
    base.map(|b| b.join("ui.json"))
}

fn load_prefs(app: &mut DesignApp) {
    if std::env::var_os("DESIGNCRAFT_NO_PREFS").is_some() {
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
    if std::env::var_os("DESIGNCRAFT_NO_PREFS").is_some() {
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

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|purpose: &str| {
            let d = rfd::FileDialog::new();
            let d = if purpose == "swatches" {
                d.add_filter("Swatch Exchange (ASE)", &["ase"])
            } else if purpose == "script" {
                d.add_filter("Script", &["dcscript", "txt", "json"])
            } else if purpose == "icc" {
                d.add_filter("ICC profile", &["icc", "icm"])
            } else if purpose == "book" {
                d.add_filter("Book", &["dcbook"])
            } else if purpose == "xml" {
                d.add_filter("XML", &["xml"])
            } else if purpose == "library" {
                d.add_filter("Object Library", &["dclib"])
            } else if purpose == "place" {
                d.add_filter(
                    "Graphics and text",
                    &[
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
                )
                .add_filter("Graphics", &["png", "jpg", "jpeg", "gif", "webp", "tif", "tiff", "bmp", "psd", "svg", "pdf", "ai", "eps"])
                .add_filter("Text (Word, RTF, plain, Excel)", &["docx", "rtf", "txt", "md", "xlsx"])
                .add_filter("Video and sound", &["mp4", "m4v", "mov", "webm", "mp3", "m4a", "wav", "ogg"])
            } else {
                d.add_filter("DesignCraft, IDML or InDesign", &["designcraft", "idml", "indd"])
                    .add_filter("DesignCraft", &["designcraft"])
                    .add_filter("InDesign Markup (IDML)", &["idml"])
                    .add_filter("InDesign document (INDD)", &["indd"])
            };
            d.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| rfd::FileDialog::new().set_file_name(name).save_file().map(|p| p.to_string_lossy().to_string()))),
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

fn main() -> eframe::Result {
    let mut control_port: Option<u16> = std::env::var("DESIGNCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut files = Vec::new();
    let mut sample = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--sample" => sample = true,
            "--version" => {
                println!("designcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => files.push(a),
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
    eframe::run_native(
        "DesignCraft",
        options,
        Box::new(move |cc| {
            let mut session = Session::new();
            // Crash recovery: reopen what a previous run left unsaved, then keep it current.
            session.recovery_dir = designcraft_engine::recovery::default_dir();
            let recovered = session.execute("file.recovery.open", &serde_json::json!({})).ok();
            let mut app = DesignApp::new(session, services());
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
                if let Err(e) = app.run("file.open", serde_json::json!({"path": f})) {
                    eprintln!("designcraft: {f}: {e}");
                }
            }
            Ok(Box::new(App(
                app,
                #[cfg(target_os = "macos")]
                None,
            )))
        }),
    )
}
