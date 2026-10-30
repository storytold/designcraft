//! DesignCraft's egui frontend: an InDesign-style UI over `designcraft-engine`.
//!
//! The UI is thin: every action goes through [`DesignApp::run`], which dispatches UI commands
//! (view/window) here and everything else to the engine. Menus, shortcuts, the ⌘K palette and
//! the control channel ([`control`]) share that entry point.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// DesignCraft as craft-update knows it, with its 128 px icon and accent colour: the update
/// prompt shows both, and an AppImage's menu entry gets the icon.
#[cfg(not(target_arch = "wasm32"))]
pub const UPDATE_APP: craft_update::app::App = craft_update::app::App {
    icon_png: Some(include_bytes!("../../../assets/app-icon/hicolor/128x128/apps/ai.storyteller.designcraft.png")),
    accent: Some([0x7b, 0xb5, 0x1c]),
    ..craft_update::suite::DESIGNCRAFT
};

pub mod about;
pub mod canvas;
pub mod chrome;
pub mod control;
pub mod credits;
pub mod cursors;
pub mod dialogs;
pub mod dock;
pub mod effects_dialog;
pub mod i18n;
pub mod icons;
pub mod keys;
pub mod menus;
mod modal_drag;
mod new_document;
mod object_control;
mod page_history;
mod panel_docking;
pub mod panels;
mod prefs;
pub mod render_worker;
mod rtl;
mod section_options;
pub mod separations;
mod shortcut_import;
mod shortcuts_dialog;
pub mod story_editor;
pub mod taskbar;
pub mod theme;
pub mod toolbar;
pub mod updates_ui;
pub mod widgets;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use designcraft_engine::{Session, SnapView, UiRequest, ViewInfo};
use designcraft_geom::{Point, Unit};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse};
pub use prefs::ReadPreferences;

mod app;
mod clock;
mod key_names;
mod services;
mod state;

#[cfg(test)]
use app::browser_import_tests;
pub use app::opens_as_document;
pub use clock::now_ms;
pub use key_names::{drop_key_name_text, is_key_name};
pub use services::{ClipboardFn, DownloadFn, Inbox, OpenAsyncFn, ReadFn, Services, WriteFn};
pub(crate) use state::second_window_viewport;
pub use state::{CanvasCache, HiddenPanels, ImportRequest, ImportedFile, PdfExportSettings, Perf, SavedWorkspace, ScreenMode, Shown, UiState, View};

/// The bounded admission policy for `ui.screenshot` captures.
pub(crate) const SCREENSHOT_MAX_REQUESTS: usize = 4;
pub(crate) const SCREENSHOT_MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const SCREENSHOT_MAX_AGGREGATE_BYTES: usize = 256 * 1024 * 1024;

/// The viewport geometry a capture was admitted for.
#[derive(Clone, Copy, PartialEq, Debug)]
struct ScreenshotGeometry {
    width: usize,
    height: usize,
    /// The active document state epoch at admission; a switch or Revert invalidates it.
    document: u64,
    /// Logical size, DPI, zoom and fullscreen state. The window position is deliberately absent:
    /// moving the window does not change the captured surface or its content. eframe 0.37 exposes
    /// no portable surface-generation identity, so a same-size surface recreation is still a
    /// documented qualification gap.
    signature: [u32; 6],
}

/// The document, dialog and keyboard focus an injected input batch was captured for. If any of
/// them changed before a settled UI pass took the batch, the batch no longer belongs to the new
/// state and is discarded instead of replaying stale clicks or text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct InputGeneration {
    document: u64,
    dialog: Option<egui::Id>,
    focus: Option<egui::Id>,
}

/// One native file drop captured from the raw input. The generation travelled with it so a
/// document, dialog or focus change before the next settled UI pass discards the stale drop.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct PendingDrop {
    path: String,
    pointer: Option<egui::Pos2>,
    generation: InputGeneration,
}

/// One admitted `ui.screenshot` request. The token, path, deadline and epoch are app-owned; the
/// shared capture slot only correlates one completion with this record.
struct ShotRequest {
    token: u64,
    path: Option<String>,
    reply: Sender<ControlResponse>,
    deadline: f64,
    viewport: egui::ViewportId,
    epoch: u64,
    geometry: ScreenshotGeometry,
    reserved_bytes: usize,
    slot: Option<craft_ui::capture::CaptureSlot>,
}

pub struct DesignApp {
    pending_pdf: Option<ImportedFile>,
    /// File and message dialogs: asked for during a frame, shown by the host at its end, answered
    /// on a later frame (see `app/file_dialogs.rs`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) file_dialogs: app::file_dialogs::Dialogs,
    /// The dialogs asked for and not yet answered, in order: the same one isn't asked twice.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) asked: Vec<app::file_dialogs::AskKey>,
    /// Dialog answers waiting for an egui dialog to close.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) held_answers: Vec<app::file_dialogs::Held>,
    /// The context the dialogs' wake-up repaints, once the first frame has run.
    #[cfg(not(target_arch = "wasm32"))]
    dialog_wake: std::sync::Arc<std::sync::OnceLock<egui::Context>>,
    pub session: Session,
    pub ui: UiState,
    pub services: Services,
    pub views: HashMap<u64, View>,
    /// Layout ▸ Go Back / Go Forward, by view (like `views`).
    pub(crate) page_history: HashMap<u64, page_history::PageHistory>,
    /// The canvas cache and rect of the pane in `pane` (the other pane's are in `other_pane`).
    pub canvas: CanvasCache,
    pub canvas_rect: Option<egui::Rect>,
    /// Window › Arrange › Split Window: two views of the document side by side.
    pub split: bool,
    /// Window › Arrange › New Window: the second view in its own window (pane 1).
    pub second_window: bool,
    /// The current pane (0 or 1): views, cache and rect refer to it.
    pub pane: u8,
    /// The pane last clicked (menu zoom and scroll go there).
    pub focus_pane: u8,
    pub other_pane: Option<(CanvasCache, Option<egui::Rect>)>,
    /// Power Zoom in progress: the zoom to return to and the canvas point it will centre on.
    pub power_zoom: Option<(f64, designcraft_geom::Point)>,
    /// The persisted colour settings have been applied this session.
    pub color_applied: bool,
    pub perf: Perf,
    pub synthetic: Vec<egui::Event>,
    /// Control input stays separate from native-menu input until its frame is processed.
    pub(crate) control_synthetic: Vec<egui::Event>,
    /// Resolved native menu choices, kept stable across menu rebuilds until input settles.
    native_actions: std::collections::VecDeque<(String, Value)>,
    native_events_pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    control_input_frame: std::sync::Arc<std::sync::atomic::AtomicBool>,
    clipboard_guard_installed: bool,
    /// Story open in the Story Editor.
    pub story_editor: Option<story_editor::Target>,
    control_rx: Option<Receiver<ControlRequest>>,
    pending_shots: Vec<ShotRequest>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
    /// The viewport geometry the live screenshot requests were admitted for.
    screenshot_geometry: Option<ScreenshotGeometry>,
    /// Advanced whenever the viewport size, DPI, zoom or surface changes (even back again).
    screenshot_epoch: u64,
    /// A synthetic input step was injected and has not yet reached a settled UI pass.
    synthetic_injected: bool,
    /// The exact events of that batch, so a generation change can remove them from the host's
    /// pending raw input before they reach a UI pass.
    injected_events: Vec<egui::Event>,
    /// The batch came from the control channel (private clipboard and provenance).
    injected_private: bool,
    /// What the document, dialog and focus were when that batch was injected.
    injected_generation: InputGeneration,
    /// The batch is in the host's pending raw input and not yet consumed by a settled pass.
    injected_pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// A settled `run_ui` pass ended since the batch was injected; the next host iteration
    /// acknowledges it. This also covers harnesses that consume the raw input without `app.ui`.
    injected_settled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The end-of-pass settlement observer is installed on the egui context.
    injected_settle_hook: bool,
    /// Native dropped files with the input generation they were captured for, buffered from the
    /// raw input before a pass so hidden logic cannot replay a previous drop.
    #[cfg(not(target_arch = "wasm32"))]
    pending_drops: Vec<PendingDrop>,
    #[cfg(test)]
    logic_tick_ticks: u32,
    #[cfg(test)]
    logic_frame_ticks: u32,
    #[cfg(test)]
    input_ticks: u32,
    styled: bool,
    /// The interface language the UI fonts were installed for (it orders the CJK fallbacks).
    fonts_lang: String,
    /// The installed CJK faces are among the UI fonts (false while the system fonts are still
    /// being cataloged: the fonts are installed again when they are).
    fonts_system: bool,
    pub restyle: bool,
    fonts_ready: bool,
    /// Files the operating system asked to open (macOS Finder, Dock, Open With), opened once the
    /// window is ready; see [`DesignApp::open_documents_later`].
    os_documents: Vec<String>,
    pub integrated_titlebar: bool,
    /// The desktop host uses our caption controls and window drag/resize regions (Windows).
    pub custom_titlebar: bool,
    pub(crate) window_maximized: bool,
    pub(crate) pending_window_commands: Vec<egui::ViewportCommand>,
    /// The host installed a native menu bar (macOS): don't draw menus in the window.
    pub native_menu: bool,
    /// The commands the native menu bar gives a key equivalent, with that shortcut: the egui
    /// shortcut handler leaves that key to the bar and runs the command's other bindings itself.
    pub native_shortcuts: std::collections::HashMap<String, String>,
    /// Every command's shortcuts, with the user's changes (`ui.shortcuts`) applied.
    pub keymap: craft_keymap::Keymap,
    pub(crate) key_dispatcher: craft_keymap::egui::Dispatcher,
    /// The open Keyboard Shortcuts dialog.
    pub(crate) shortcut_dialog: Option<Box<shortcuts_dialog::Host>>,
    /// Answers the user gave in an import review (item → command), for later imports.
    pub(crate) shortcut_answers: std::collections::BTreeMap<String, craft_keymap::CommandRef>,
    last_time: f64,
    /// When recovery data was last written (seconds on the monotonic clock; see [`now_ms`]).
    pub last_recovery: f64,
    /// Recovery data is being written in the background: poll for it, and keep repainting.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) recovery_writing: bool,
    /// The egui context (set by the first UI pass): copied text goes to the system clipboard through it.
    egui_ctx: Option<egui::Context>,
    /// A control-channel request is being handled: commands it runs never read or write the
    /// user's system clipboard (a control client must not see or replace it).
    pub(crate) in_control: bool,
    /// The system clipboard's text that stands for the last object copy: what that copy put there
    /// (no native menu: without it the paste keys produce no paste event), or what it held then
    /// (native menu). It is never pasted as text: Paste with it pastes the copied objects.
    object_clip_text: Option<String>,
    /// The marked text the system IME last sent (`None` once it commits or clears). When the
    /// composition ends some other way (a click, a tool switch), the IME is told to drop it.
    pub(crate) ime_marked: Option<String>,
    /// The system IME must drop its marked text (see [`Self::take_ime_discard`]).
    pub(crate) ime_discard: bool,
    /// The canvas asked for the system IME in the last frame (the web build's text input has the
    /// keyboard then).
    pub(crate) canvas_ime: bool,
    /// Owner/generation/cause bookkeeping for the canvas story's custom IME producer.
    pub(crate) ime_tracker: egui::ime_context::ImeContextTracker,
    /// An IME delete-surrounding request waiting for the Commit in the same event batch.
    pub(crate) pending_ime_delete: Option<(usize, usize, bool)>,
    /// The owner the current composition began in: a late commit may only land there.
    pub(crate) ime_target: Option<egui::ime_context::ImeTarget>,
    /// This pass dropped a late commit or delete for a composition that had already ended: the
    /// system IME must interrupt its copy (as for a click away).
    pub(crate) ime_interrupt: bool,
    /// What the canvas recorded for this pass's IME observation, applied at the end of the pass.
    pub(crate) ime_canvas: Option<canvas::CanvasIme>,
    /// Quit was chosen: close the window on the next frame, as its close button does.
    quit_requested: bool,
    /// An OS close event deferred while a synthetic gesture or native action is pending.
    close_requested_pending: bool,
    /// Close the window without asking about unsaved changes (the control channel's `app.quit`).
    pub(crate) quit_without_asking: bool,
    /// No document is discarded until every unsaved document has an up-to-date approval.
    quitting: Option<QuitState>,
    /// The tool cursor bitmaps handed to the OS.
    pub cursors: cursors::Images,
    /// A panel's live preview (a slider being dragged), committed when it is let go.
    live_preview: LivePreview,
    pub(crate) updates_rt: updates_ui::Runtime,
}

/// The preview a panel opened with `edit.preview`: it stays open while a panel holds it each frame.
#[derive(Default)]
struct LivePreview {
    open: bool,
    held: bool,
}

struct QuitState {
    original_active: Option<u64>,
    accepted: HashMap<u64, u64>,
    pending: Option<u64>,
    /// The document whose Save answer went to a save dialog that hasn't answered yet.
    saving: Option<u64>,
}

/// Whether CJK-only interface is shown (Preferences › Type › Show CJK Features): the explicit
/// preference, else on for a Japanese, Chinese or Korean interface language.
///
/// Every menu item, dialog section and panel field that only CJK typesetting uses goes behind
/// this check, as in InDesign, whose Roman edition leaves those features out. It hides interface
/// only: the commands behind it run whatever it says (from scripts, the control channel and MCP).
/// Story Direction, the vertical type tools, frame grids, Language, Digits and font naming are
/// in both editions and stay outside it.
pub fn cjk_features(app: &DesignApp) -> bool {
    app.session.prefs.cjk_features.unwrap_or_else(|| i18n::is_cjk(&app.ui.language))
}

#[cfg(test)]
mod tests;

/// The whole window in a headless test harness, at a chosen window size.
#[cfg(test)]
pub(crate) mod test_window {
    use egui_kittest::Harness;

    pub struct Window {
        pub app: crate::DesignApp,
        ready: bool,
    }

    /// The window at `size` with a new document open.
    pub fn open(app: crate::DesignApp, size: egui::Vec2) -> Harness<'static, Window> {
        open_scaled(app, size, 1.0)
    }

    /// The window at a size in points and a scale, with a new document open.
    pub fn open_scaled(app: crate::DesignApp, size: egui::Vec2, pixels_per_point: f32) -> Harness<'static, Window> {
        open_stepped(app, size, pixels_per_point, 0.25)
    }

    /// As `open_scaled`, with `step_dt` seconds between frames (a double-click needs its two
    /// clicks closer together than the default quarter second).
    pub fn open_stepped(mut app: crate::DesignApp, size: egui::Vec2, pixels_per_point: f32, step_dt: f32) -> Harness<'static, Window> {
        if app.session.active().is_none() {
            app.run("file.new", serde_json::json!({})).unwrap();
        }
        let mut h =
            Harness::builder().with_size(size).with_pixels_per_point(pixels_per_point).with_max_steps(1000).with_step_dt(step_dt).build_ui_state(
                |ui, w: &mut Window| {
                    // The builder runs a frame before the texture size can be raised: skip it.
                    if !w.ready {
                        return;
                    }
                    let ctx = ui.ctx().clone();
                    w.app.logic(&ctx);
                    w.app.ui(ui);
                },
                Window { app, ready: false },
            );
        h.input_mut().max_texture_side = Some(8192);
        h.state_mut().ready = true;
        h.run_steps(6);
        h
    }

    /// The mouse wheel turned over `pos`: positive `delta` moves the content right and down.
    pub fn wheel(h: &mut Harness<'static, Window>, pos: egui::Pos2, delta: egui::Vec2) {
        h.hover_at(pos);
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
            source: egui::MouseWheelSource::Unknown,
        });
        h.run_steps(10);
    }

    /// Ensure a Hebrew face is available for the interface's glyph assertions.
    ///
    /// Canonical craft-fonts builds bundle the face. Optional-input builds can pass
    /// `DESIGNCRAFT_TEST_HEBREW_FONT=/abs/path/NotoSansHebrew.ttf` (the pinned OFL fixture)
    /// and otherwise skip their Hebrew-only assertions explicitly.
    pub fn install_test_hebrew_face() -> bool {
        let db = designcraft_fonts::FontDb::global();
        if db.has_family("Noto Sans Hebrew") {
            return true;
        }
        let Ok(path) = std::env::var("DESIGNCRAFT_TEST_HEBREW_FONT") else { return false };
        designcraft_engine::io::read(path).is_ok_and(|bytes| db.add_font(bytes) > 0)
    }

    /// The outer rect of a side or top panel, from egui's memory.
    ///
    /// 0.37 keys a panel by its *parent Ui's* scope: `Ui::scope_id().with_salt(salt)`. In this
    /// harness the app draws into a Ui the harness owns (a child of the viewport root Ui), so
    /// `Harness::ui_id()` — not the viewport's root id — is the parent to salt.
    pub fn panel_rect(h: &Harness<'static, Window>, id: &str) -> egui::Rect {
        egui::containers::panel::PanelState::load(&h.ctx, h.ui_id().with(id)).map(|s| s.outer_rect).unwrap()
    }

    /// A primary click at `pos`, then a few frames.
    pub fn click_at(h: &mut Harness<'static, Window>, pos: egui::Pos2) {
        h.hover_at(pos);
        h.drag_at(pos);
        h.drop_at(pos);
        h.run_steps(4);
    }
}
