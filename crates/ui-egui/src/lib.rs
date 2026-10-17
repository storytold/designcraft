//! DesignCraft's egui frontend: an InDesign-style UI over `designcraft-engine`.
//!
//! The UI is thin: every action goes through [`DesignApp::run`], which dispatches UI commands
//! (view/window) here and everything else to the engine. Menus, shortcuts, the ⌘K palette and
//! the control channel ([`control`]) share that entry point.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

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
pub mod menus;
mod modal_drag;
mod new_document;
mod object_control;
mod panel_docking;
pub mod panels;
mod pdf_export;
mod prefs;
pub mod render_worker;
mod rtl;
mod section_options;
pub mod story_editor;
pub mod taskbar;
pub mod theme;
pub mod title_bar;
pub mod toolbar;
pub mod widgets;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use designcraft_engine::{Session, SnapView, UiRequest, ViewInfo};
use designcraft_geom::{Point, Unit};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use control::{ControlRequest, ControlResponse};

pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;
pub type BackgroundWriteFn = std::sync::Arc<dyn Fn(&str, &[u8]) -> Result<(), String> + Send + Sync>;
pub type WriteFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;
pub type PickFn = Box<dyn FnMut(&str) -> Option<String>>;
/// Context captured before any host file-picker or read await.
#[derive(Clone, Debug)]
pub struct ImportRequest {
    pub purpose: String,
    pub target: Option<u64>,
}
pub struct ImportedFile {
    pub request: ImportRequest,
    pub name: String,
    pub bytes: Vec<u8>,
}
pub type OpenAsyncFn = Box<dyn FnMut(ImportRequest)>;
/// Starts a browser download; an error means no file was handed to the user.
pub type DownloadFn = Box<dyn FnMut(&str, &[u8]) -> Result<(), String>>;
pub type ClipboardFn = Box<dyn FnMut() -> Option<String>>;
/// Files delivered asynchronously with their original operation and document identity.
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<ImportedFile>>>;

/// Platform services injected by the host (desktop or web).
#[derive(Default)]
pub struct Services {
    /// Open-file dialog for a purpose (`open`, `place`) → path.
    pub pick_open: Option<PickFn>,
    /// Save dialog with a suggested name → path.
    pub pick_save: Option<PickFn>,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
    /// Optional thread-safe writer for native background exports.
    pub background_write: Option<BackgroundWriteFn>,
    /// Asynchronous open dialog for a purpose (`open`, `place`); the chosen file arrives later
    /// through [`Services::inbox`]. Used when `pick_open` is unset (web).
    pub open_async: Option<OpenAsyncFn>,
    /// Hand bytes to the user as a named file (browser download). When set, Save uses it
    /// instead of writing to a path.
    pub download: Option<DownloadFn>,
    /// Files delivered asynchronously, drained every frame with their captured request context.
    pub inbox: Option<Inbox>,
    /// The system clipboard's text, for Paste chosen from a menu (egui only delivers it with the
    /// paste keys, which a native menu bar takes first).
    pub clipboard_text: Option<ClipboardFn>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScreenMode {
    #[default]
    Normal,
    Preview,
    Bleed,
    Slug,
    Presentation,
}

/// A saved workspace: which bars show and where the panels are.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedWorkspace {
    pub name: String,
    pub control_bar: bool,
    pub task_bar: bool,
    pub task_bar_pin: Option<[f32; 2]>,
    pub tools_double_column: bool,
    pub dock_tab: String,
    pub dock_expanded: bool,
    pub open_panel: Option<String>,
    pub floating: Vec<(String, [f32; 2])>,
    /// Custom panel membership and placement; absent in older workspace files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docking: Option<craft_ui::docking::Layout<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub docking_hidden: std::collections::BTreeMap<String, craft_ui::docking::Location<String>>,
    /// The workspace's own icon column (see [`UiState::icon_groups`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_groups: Option<Vec<Vec<String>>>,
    /// Saved with New Workspace › Capture › Panel Locations off: choosing the workspace leaves the
    /// bars and panels where they are.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_panels: bool,
    /// New Workspace › Capture › Menu Customization: the menu items hidden with Edit › Menus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_menu_items: Option<Vec<String>>,
}

/// A missing on-by-default bool stays on. `bool::default` is false.
fn default_true() -> bool {
    true
}

/// A missing snap zone stays at the factory width, in screen pixels.
fn default_zone() -> f64 {
    4.0
}

/// File › Export PDF… options: the values persisted in `UiState.pdf_export` and used as the
/// dialog's defaults. Serialised as `pdfExport` with camelCase field names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PdfExportSettings {
    /// Preset (one of the five presets, or "Custom" after a manual edit).
    pub preset: String,
    /// PDF standard (`none`, `x4`, `a2b`).
    pub standard: String,
    /// Compression › Compress images (JPEG).
    pub compress_images: bool,
    /// Transparency flattener preset (`""` = none, `high`, `medium`, `low`).
    pub flatten: String,
    pub spreads: bool,
    pub bleed: bool,
    pub marks_crop: bool,
    pub marks_bleed: bool,
    pub marks_page_info: bool,
    pub marks_weight: String,
    pub marks_offset: String,
    /// File › Export PDF › Advanced › Tagged PDF.
    pub tagged: bool,
}

impl Default for PdfExportSettings {
    fn default() -> Self {
        PdfExportSettings {
            preset: "Desktop Printing".into(),
            standard: "none".into(),
            compress_images: false,
            flatten: "".into(),
            spreads: false,
            bleed: true,
            marks_crop: false,
            marks_bleed: false,
            marks_page_info: false,
            marks_weight: "0.25".into(),
            marks_offset: "6".into(),
            tagged: true,
        }
    }
}

/// Persisted UI state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    pub brightness: theme::Brightness,
    pub screen_mode: ScreenMode,
    /// The screen mode leaving Presentation returns to: the one it was entered from.
    pub presentation_return: ScreenMode,
    /// The view (by its `views` key) Presentation was entered from; leaving puts it back.
    #[serde(skip)]
    pub presentation_view: Option<(u64, View)>,
    pub frame_edges: bool,
    pub rulers: bool,
    pub guides: bool,
    pub baseline_grid: bool,
    pub document_grid: bool,
    pub text_threads: bool,
    pub hidden_characters: bool,
    /// View › Structure › Show Tagged Frames.
    pub tagged_frames: bool,
    /// Edit › Menus: hidden menu items (`menu/label`), and Show Full Menus.
    pub hidden_menu_items: Vec<String>,
    pub show_full_menus: bool,
    /// Scripts panel: saved scripts (name, text).
    pub scripts: Vec<(String, String)>,
    /// View › Structure › Show Tag Markers.
    pub tag_markers: bool,
    /// View › Flattener Preview: objects that involve transparency highlighted in red.
    #[serde(skip)]
    pub flattener_preview: bool,
    /// Tab / Shift+Tab: 1 = every panel hidden, 2 = all but the Tools panel (0 = shown).
    pub hidden_panels: u8,
    /// "Don't show again" in the warning before deleting pages that contain objects.
    pub skip_delete_pages_warning: bool,
    /// The Pages panel's parents part as its divider was last dragged, and the height the two
    /// parts shared then (None: a few rows).
    pub pages_parents_height: Option<[f32; 2]>,
    /// The Pages panel's Panel Options.
    pub pages_panel: panels::pages::PanelOptions,
    /// Edit › Color Settings (re-applied at start).
    pub color_settings: Option<designcraft_color::cms::ColorSettings>,
    /// View › Proof Colors and View › Proof Setup.
    pub proof_colors: bool,
    pub proof_setup: designcraft_color::cms::ProofSetup,
    /// Edit › Spelling › Dynamic Spelling: misspelled words underlined on the canvas.
    pub dynamic_spelling: bool,
    /// Preferences › Story Editor Display: text size (points).
    pub story_editor_size: f32,
    /// Preferences › General › Interface Language: supported codes are listed in `i18n::LANGUAGES`.
    pub language: String,
    /// Edit › Transparency Flattener Presets: "" (none), "high", "medium" or "low" for PDF export.
    pub flattener: String,
    /// File › Export PDF… options (persisted; see `PdfExportSettings`).
    pub pdf_export: PdfExportSettings,
    /// View › Separations Preview: a process plate (0–3) and/or an ink limit (total, 0–4).
    #[serde(skip)]
    pub separation: Option<u8>,
    #[serde(skip)]
    pub ink_limit: Option<f32>,
    /// View › Display Performance.
    pub display_quality: designcraft_render::DisplayQuality,
    /// Preferences › Interface › UI scaling (1 = 100%).
    pub ui_scale: f32,
    /// Preferences › Appearance of Black: 100% K on screen as rich black (InDesign's default,
    /// Display All Blacks as Rich Black; off: accurately, as dark grey).
    pub rich_black: bool,
    /// View › Overprint Preview.
    pub overprint_preview: bool,
    /// Edit › Keyboard Shortcuts: command id → shortcut ("" = none), over the defaults.
    pub shortcuts: std::collections::BTreeMap<String, String>,
    pub control_bar: bool,
    pub tools_double_column: bool,
    /// Expanded right-dock panel group tab.
    pub dock_tab: String,
    /// Panel opened from the collapsed icon column (flyout).
    pub open_panel: Option<String>,
    /// Panels torn off the dock into their own floating windows: (panel id, top-left position).
    pub floating: Vec<(String, [f32; 2])>,
    /// Shared panel layout after the user customizes the legacy dock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docking: Option<craft_ui::docking::Layout<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub docking_hidden: std::collections::BTreeMap<String, craft_ui::docking::Location<String>>,
    /// Invalidates in-flight gestures when a workspace is replaced.
    #[serde(skip)]
    pub docking_generation: u64,
    /// One-frame reveal request; z-order is never persisted.
    #[serde(skip)]
    pub docking_raise: Option<String>,
    /// Last explicitly revealed customized panel, for the legacy closePanel control.
    #[serde(skip)]
    pub docking_opened: Option<String>,
    pub dock_expanded: bool,
    /// A workspace's iconic panel groups (Essentials Classic), shown with their names in the icon
    /// column instead of every panel; `None` is the full icon column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_groups: Option<Vec<Vec<String>>>,
    pub units: Unit,
    pub workspace: String,
    /// Window › Workspace › New Workspace: saved panel arrangements.
    pub custom_workspaces: Vec<SavedWorkspace>,
    /// File › New › Document › Recent: the settings of the last documents made, newest first.
    pub recent_new_documents: Vec<designcraft_doc::build::NewDocument>,
    /// The font menus' sample text (a key of `panels::FONT_SAMPLES`) and size (0–2).
    pub font_sample: String,
    pub font_preview_size: u8,
    /// File › New › Document › Preview, kept for the session.
    #[serde(skip)]
    pub new_document_preview: bool,
    /// Transform reference point (0..8, row-major; 0 = top-left).
    pub ref_point: u8,
    /// Align To target (`selection`, `keyObject`, `margins`, `page`, `spread`).
    pub align_to: String,
    /// Properties > Text Style tab: 0 = Paragraph Styles, 1 = Character Styles.
    pub text_style_tab: u8,
    pub guides_locked: bool,
    pub smart_guides: bool,
    #[serde(default = "default_true")]
    pub snap_to_guides: bool,
    pub snap_to_document_grid: bool,
    #[serde(default = "default_true")]
    pub align_edges: bool,
    #[serde(default = "default_true")]
    pub align_centers: bool,
    #[serde(default = "default_true")]
    pub smart_dimensions: bool,
    #[serde(default = "default_true")]
    pub smart_spacing: bool,
    #[serde(default = "default_zone")]
    pub snap_zone: f64,
    /// Tools panel: Formatting Affects Text (J): with text frames selected, colour edits go to
    /// their text instead of the frames.
    pub formatting_affects_text: bool,
    /// Window > Contextual Task Bar.
    pub task_bar: bool,
    /// Where the Contextual Task Bar is pinned: its top-left, in points from the canvas's top-left
    /// (`None`: it follows the selection).
    pub task_bar_pin: Option<[f32; 2]>,
    /// Where the Contextual Task Bar was last shown, like [`Self::task_bar_pin`].
    #[serde(skip)]
    pub task_bar_at: Option<[f32; 2]>,
    /// Help › About DesignCraft is open.
    pub about: bool,
    /// The About window's tab: 0 About, 1 Contributors, 2 Models (`about::ABOUT_TABS`).
    #[serde(skip)]
    pub about_tab: u8,
    /// URLs to open in the browser on the next frame (Help links, About, start screen).
    pub pending_urls: Vec<String>,
    #[serde(skip)]
    pub status: String,
    #[serde(skip)]
    pub dialog: Option<dialogs::Dialog>,
    #[serde(skip)]
    pub palette: Option<String>,
    #[serde(skip)]
    pub flyout: Option<usize>,
    /// Type › Tabs: the selected stop and where the panel sits.
    #[serde(skip)]
    pub tabs_panel: panels::tabs::PanelState,
    /// The colour and gradient swatches applied last: what Apply Color (,) and Apply Gradient (.)
    /// apply again.
    #[serde(skip)]
    pub last_color: Option<String>,
    #[serde(skip)]
    pub last_gradient: Option<String>,
    /// Fill/Stroke proxy (Tools and Color panels): the stroke is in front, so Apply None and the
    /// Color panel act on it (X toggles).
    #[serde(skip)]
    pub stroke_front: bool,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            brightness: theme::Brightness::MediumDark,
            screen_mode: ScreenMode::Normal,
            presentation_return: ScreenMode::Normal,
            presentation_view: None,
            frame_edges: true,
            rulers: true,
            guides: true,
            baseline_grid: false,
            document_grid: false,
            text_threads: false,
            hidden_characters: false,
            tagged_frames: false,
            hidden_menu_items: Vec::new(),
            show_full_menus: false,
            story_editor_size: 14.0,
            scripts: vec![
                ("Number the pages".into(), "# A page-number frame at the bottom of the current page.\nframe.create {\"rect\": [288, 740, 324, 760], \"content\": \"text\", \"text\": \"\"}\ntext.select {\"story\": \"$0.story\", \"anchor\": 0, \"focus\": 0}\ntext.insert {\"text\": \"\\ue000\", \"raw\": true}".into()),
                ("Two-column grid".into(), "# Four text frames in a 2 × 2 grid.\nframe.grid {\"rect\": [36, 36, 576, 756], \"cols\": 2, \"rows\": 2, \"gutter\": 12}".into()),
            ],
            tag_markers: false,
            flattener_preview: false,
            hidden_panels: 0,
            skip_delete_pages_warning: false,
            pages_parents_height: None,
            pages_panel: Default::default(),
            color_settings: None,
            proof_colors: false,
            proof_setup: Default::default(),
            dynamic_spelling: false,
            language: String::new(),
            flattener: String::new(),
            pdf_export: PdfExportSettings::default(),
            separation: None,
            ink_limit: None,
            display_quality: designcraft_render::DisplayQuality::High,
            ui_scale: 1.0,
            rich_black: true,
            overprint_preview: false,
            shortcuts: Default::default(),
            control_bar: false,
            tools_double_column: false,
            dock_tab: "properties".into(),
            open_panel: None,
            floating: Vec::new(),
            docking: None,
            docking_hidden: Default::default(),
            docking_generation: 0,
            docking_raise: None,
            docking_opened: None,
            dock_expanded: true,
            icon_groups: None,
            units: Unit::Picas,
            workspace: "Essentials".into(),
            custom_workspaces: Vec::new(),
            recent_new_documents: Vec::new(),
            font_sample: "selected".into(),
            font_preview_size: 1,
            new_document_preview: false,
            ref_point: 0,
            align_to: "selection".into(),
            text_style_tab: 0,
            guides_locked: false,
            smart_guides: true,
            snap_to_guides: true,
            snap_to_document_grid: false,
            align_edges: true,
            align_centers: true,
            smart_dimensions: true,
            smart_spacing: true,
            snap_zone: 4.0,
            formatting_affects_text: false,
            task_bar: true,
            task_bar_pin: None,
            task_bar_at: None,
            about: false,
            about_tab: 0,
            pending_urls: Vec::new(),
            status: String::new(),
            dialog: None,
            palette: None,
            flyout: None,
            tabs_panel: Default::default(),
            last_color: None,
            last_gradient: None,
            stroke_front: false,
        }
    }
}

impl UiState {
    /// Copies the live view switches into the snap engine's view.
    pub fn snap_view(&self) -> SnapView {
        SnapView {
            snap_to_guides: self.snap_to_guides,
            snap_to_document_grid: self.snap_to_document_grid,
            show_guides: self.guides,
            smart_guides: self.smart_guides,
            align_edges: self.align_edges,
            align_centers: self.align_centers,
            smart_dimensions: self.smart_dimensions,
            smart_spacing: self.smart_spacing,
            zone_px: self.snap_zone,
        }
    }
}

/// Per-document view: zoom (screen points per document point) and the canvas point at the
/// top-left of the canvas area.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct View {
    pub zoom: f64,
    pub origin: Point,
    pub fitted: bool,
    /// View › Rotate Spread: quarter turns clockwise (0–3).
    pub rotation: u8,
}

impl Default for View {
    fn default() -> Self {
        View { zoom: 0.5, origin: Point::new(-100.0, -100.0), fitted: false, rotation: 0 }
    }
}

/// What the canvas texture currently shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    /// (doc uid, doc pointer, revision, preview).
    pub doc: (u64, u64, u64, bool),
    /// Parent and document spreads have different canvas placement and content.
    pub editing_parents: bool,
    /// Canvas point at the texture's top-left, zoom, and texture size in screen points.
    pub origin: Point,
    pub zoom: f64,
    pub size: (f32, f32),
}

impl CanvasCache {
    pub fn new() -> Self {
        let mut renderer = designcraft_render::Renderer::new();
        renderer.threads = designcraft_render::default_threads();
        CanvasCache {
            renderer,
            texture: None,
            shown: None,
            pending: None,
            token: 0,
            #[cfg(not(target_arch = "wasm32"))]
            worker: None,
            worker_started: false,
            last_ms: 0.0,
            shown_doc: None,
            pending_doc: None,
            display_key: None,
            patcher: designcraft_render::Renderer::new(),
            patches: 0,
        }
    }
}

impl Default for CanvasCache {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CanvasCache {
    pub renderer: designcraft_render::Renderer,
    pub texture: Option<egui::TextureHandle>,
    pub shown: Option<Shown>,
    /// The request in flight on the worker.
    pub pending: Option<(u64, Shown)>,
    pub token: u64,
    #[cfg(not(target_arch = "wasm32"))]
    pub worker: Option<render_worker::Worker>,
    pub worker_started: bool,
    pub last_ms: f64,
    /// The document the texture shows (to repaint only what an edit changed).
    pub shown_doc: Option<std::sync::Arc<designcraft_doc::Document>>,
    /// Document of the request in flight.
    pub pending_doc: Option<std::sync::Arc<designcraft_doc::Document>>,
    /// Display options the texture and the request in flight were rendered with.
    pub display_key: Option<canvas::DisplayKey>,
    /// Renders damaged regions (its own context: region sizes vary).
    pub patcher: designcraft_render::Renderer,
    /// Partial repaints / full renders since start (perf readout).
    pub patches: u64,
}

#[derive(Default, Clone, Copy, Debug, Serialize)]
pub struct Perf {
    pub frame_ms: f64,
    pub render_ms: f64,
    pub fps: f64,
}

pub struct DesignApp {
    pending_pdf: Option<ImportedFile>,
    pdf_export: pdf_export::Export,
    pub session: Session,
    pub ui: UiState,
    pub services: Services,
    pub views: HashMap<u64, View>,
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
    control_input_frame: std::sync::Arc<std::sync::atomic::AtomicBool>,
    clipboard_guard_installed: bool,
    /// Story open in the Story Editor.
    pub story_editor: Option<story_editor::Target>,
    control_rx: Option<Receiver<ControlRequest>>,
    pending_shots: Vec<(u64, Option<String>, Sender<ControlResponse>, f64)>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
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
    /// Shortcuts the native menu handles (skip them in the egui shortcut handler).
    pub native_shortcuts: std::collections::HashSet<String>,
    last_time: f64,
    /// When recovery data was last written (seconds, egui time).
    pub last_recovery: f64,
    /// The egui context (set by the first frame): copied text goes to the system clipboard through it.
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
    /// Quit was chosen: close the window on the next frame, as its close button does.
    quit_requested: bool,
    /// Close the window without asking about unsaved changes (the control channel's `app.quit`).
    pub(crate) quit_without_asking: bool,
    /// No document is discarded until every unsaved document has an up-to-date approval.
    quitting: Option<QuitState>,
    /// The tool cursor bitmaps handed to the OS.
    pub cursors: cursors::Images,
    /// A panel's live preview (a slider being dragged), committed when it is let go.
    live_preview: LivePreview,
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
}

impl DesignApp {
    pub fn new(session: Session, services: Services) -> Self {
        // The font menus and the first file opened need the installed fonts: catalog them now.
        designcraft_fonts::FontDb::global().scan_in_background();
        DesignApp {
            pending_pdf: None,
            pdf_export: Default::default(),
            session,
            ui: UiState::default(),
            services,
            views: HashMap::new(),
            canvas: CanvasCache::new(),
            canvas_rect: None,
            split: false,
            second_window: false,
            pane: 0,
            focus_pane: 0,
            other_pane: None,
            power_zoom: None,
            color_applied: false,
            perf: Perf::default(),
            synthetic: vec![],
            control_synthetic: vec![],
            control_input_frame: Default::default(),
            clipboard_guard_installed: false,
            story_editor: None,
            control_rx: None,
            pending_shots: vec![],
            queued_shots: vec![],
            shot_token: 0,
            styled: false,
            fonts_lang: String::new(),
            fonts_system: false,
            restyle: false,
            fonts_ready: false,
            os_documents: Vec::new(),
            integrated_titlebar: false,
            custom_titlebar: false,
            window_maximized: false,
            pending_window_commands: vec![],
            native_menu: false,
            native_shortcuts: Default::default(),
            last_time: 0.0,
            last_recovery: 0.0,
            egui_ctx: None,
            in_control: false,
            object_clip_text: None,
            ime_marked: None,
            ime_discard: false,
            canvas_ime: false,
            quit_requested: false,
            quit_without_asking: false,
            quitting: None,
            cursors: cursors::Images::default(),
            live_preview: LivePreview::default(),
        }
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// The view key of the active document in the current pane.
    fn view_key(&self) -> Option<u64> {
        let uid = self.session.active()?.uid;
        Some(if self.pane == 1 { uid ^ (1 << 63) } else { uid })
    }
    pub fn view(&self) -> Option<&View> {
        self.views.get(&self.view_key()?)
    }
    pub fn view_mut(&mut self) -> Option<&mut View> {
        let k = self.view_key()?;
        Some(self.views.entry(k).or_default())
    }
    /// Make `pane` current: its canvas cache and rect move in.
    pub fn switch_pane(&mut self, pane: u8) {
        if pane == self.pane {
            return;
        }
        let (c, r) = self.other_pane.take().unwrap_or_default();
        let old = (std::mem::replace(&mut self.canvas, c), std::mem::replace(&mut self.canvas_rect, r));
        self.other_pane = Some(old);
        self.pane = pane;
    }
    pub fn view_info(&self) -> ViewInfo {
        ViewInfo {
            zoom: self.view().map(|v| v.zoom).unwrap_or(1.0),
            snap: self.ui.snap_view(),
            unit: self.ui.units,
            stroke_in_front: self.ui.stroke_front,
        }
    }

    pub fn status(&mut self, s: impl Into<String>) {
        self.ui.status = s.into();
    }

    /// Did an IME composition end outside the IME this frame (a click, a tool switch)? egui's
    /// `should_interrupt_composition` toggles winit's IME, which doesn't reach the macOS input
    /// context: it keeps the marked text and types it again into the next composition, so the
    /// host discards it there.
    pub fn take_ime_discard(&mut self) -> bool {
        std::mem::take(&mut self.ime_discard)
    }

    /// Did the canvas ask for the system IME in the last frame (a Type tool caret)? The web
    /// build's hidden text input has the keyboard then, so text events hold what the browser
    /// typed (see [`drop_key_name_text`]).
    pub fn canvas_ime(&self) -> bool {
        self.canvas_ime
    }

    /// "<feature>: <error>" in the status bar: the feature name is translated, the engine's own
    /// error text stays as the engine wrote it.
    pub fn status_error(&mut self, feature: &str, error: impl std::fmt::Display) {
        self.ui.status = format!("{}: {error}", crate::i18n::tr(&self.ui.language, feature));
    }

    /// THE entry point for every action (menus, shortcuts, palette, panels, control channel).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if self.pdf_export.busy() {
            return Err("Please wait for the PDF export to finish".into());
        }
        if let Some(r) = menus::run_ui(self, id, &params) {
            return r;
        }
        // Only user-initiated runs touch the system clipboard, never the control channel's.
        let system_clipboard = !self.in_control && !self.control_input_frame.load(std::sync::atomic::Ordering::Relaxed);
        let mut params = if system_clipboard { self.with_system_clipboard(id, params) } else { params };
        // Vector art copied in another app (Illustrator, VectorCraft, Figma) is SVG markup on the
        // system clipboard: with no insertion point it pastes as editable paths where text would go.
        let typing = self.session.active().is_some_and(|d| d.selection.text.is_some());
        let id = match params.as_object_mut() {
            Some(o) if id == "edit.paste" && !typing && o.get("text").and_then(Value::as_str).is_some_and(designcraft_engine::cmd::is_svg_markup) => {
                if let Some(svg) = o.remove("text") {
                    o.insert("svg".into(), svg);
                }
                "edit.pasteSvg"
            }
            _ => id,
        };
        // Copied objects paste on the spread in view.
        if matches!(id, "edit.paste" | "edit.pasteInPlace" | "edit.pasteSvg")
            && let Some(spread) = canvas::current_spread(self)
            && let Some(p) = params.as_object_mut()
        {
            p.entry("spread").or_insert(json!(spread));
        }
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        self.after_engine();
        match &r {
            Err(e) => self.status(e.clone()),
            // Copied text goes to the system clipboard too, whichever way Copy was chosen.
            Ok(v) if system_clipboard && matches!(id, "edit.copy" | "edit.cut") => {
                let text = match v.get("text").and_then(Value::as_str) {
                    Some(t) => {
                        self.object_clip_text = None;
                        Some(t.to_string())
                    }
                    None if !self.native_menu => {
                        self.object_clip_text = Some(self.copied_objects_text());
                        self.object_clip_text.clone()
                    }
                    None => {
                        self.object_clip_text = self.services.clipboard_text.as_mut().and_then(|f| f());
                        None
                    }
                };
                if let (Some(t), Some(ctx)) = (text, &self.egui_ctx) {
                    ctx.copy_text(t);
                    ctx.request_repaint();
                }
            }
            Ok(_) => {}
        }
        if r.is_ok() && matches!(id, "file.open" | "file.openIdml" | "file.openBytes") {
            if self.ui.dialog.as_ref().is_some_and(|d| d.id == "findFont") {
                self.ui.dialog = None;
            }
            match self.session.execute("font.list", &json!({})) {
                Ok(fonts) if fonts.as_array().is_some_and(|fonts| fonts.iter().any(|f| f["missing"] == true || f["styleMissing"] == true)) => {
                    self.ui.dialog = Some(dialogs::Dialog::new("findFont", json!({"_fonts": fonts, "onOpen": true})));
                }
                Err(e) => self.status(e.to_string()),
                _ => {}
            }
        }
        if r.is_ok()
            && matches!(id, "font.replace" | "edit.undo" | "edit.redo")
            && let Some(dialog) = self.ui.dialog.as_mut().filter(|d| d.id == "findFont")
        {
            dialog.fields.remove("_fonts");
        }
        r
    }

    /// Show `command` on the page as a live preview named `label` (the undo step's name): each call
    /// replaces the last, and nothing reaches the undo history until [`Self::end_live_preview`]. A
    /// panel calls this, or [`Self::hold_live_preview`], every frame its control is held; a frame
    /// without either commits the preview.
    pub fn live_preview(&mut self, label: &str, command: &str, params: Value) {
        if self.run("edit.preview", json!({"label": label, "command": command, "params": params})).is_ok() {
            self.live_preview = LivePreview { open: true, held: true };
        }
    }

    /// The control showing the live preview is still held (pressed, though not moved this frame).
    pub fn hold_live_preview(&mut self) {
        self.live_preview.held = self.live_preview.open;
    }

    /// Commit the live preview, if one is open, as one undo step.
    pub fn end_live_preview(&mut self) {
        if std::mem::take(&mut self.live_preview).open {
            let _ = self.run("edit.commitPreview", json!({}));
        }
    }

    /// Start of a frame: a live preview no panel held last frame (its control was let go, or the
    /// panel closed) is committed, so it never swallows later edits' undo steps.
    pub(crate) fn settle_live_preview(&mut self) {
        if !std::mem::take(&mut self.live_preview.held) {
            self.end_live_preview();
        }
    }

    /// Paste without the clipboard's text (a menu, not the paste keys): read it here. With no
    /// insertion point, text that doesn't stand for copied objects becomes a new text frame
    /// centred in the view.
    fn with_system_clipboard(&mut self, id: &str, mut params: Value) -> Value {
        let typing = self.session.active().is_some_and(|d| d.selection.text.is_some());
        if !matches!(id, "edit.paste" | "edit.pasteWithoutFormatting") || !(typing || id == "edit.paste") {
            return params;
        }
        if params.get("text").is_none()
            && let Some(text) = self.services.clipboard_text.as_mut().and_then(|f| f())
            && let Some(o) = params.as_object_mut()
        {
            o.insert("text".into(), Value::String(text));
        }
        // Still what an object copy put there: paste those objects, not the placeholder text.
        let norm = |t: &str| t.replace("\r\n", "\n");
        if let Some(copied) = &self.object_clip_text
            && params.get("text").and_then(Value::as_str).is_some_and(|t| norm(t) == norm(copied))
            && let Some(o) = params.as_object_mut()
        {
            o.remove("text");
        }
        if !typing
            && params.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty())
            && let Some((spread, at)) = canvas::view_centre(self)
            && let Ok(spread) = serde_json::to_value(spread)
            && let Some(o) = params.as_object_mut()
        {
            o.entry("spread").or_insert(spread);
            o.entry("at").or_insert(json!([at.x, at.y]));
        }
        params
    }

    /// Plain text for the system clipboard after copying objects: the copied text frames' stories,
    /// or U+FFFC (object replacement character) when none has text.
    fn copied_objects_text(&self) -> String {
        let mut stories = Vec::new();
        if let Some(clip) = &self.session.clipboard
            && let Some(sp) = clip.spreads.first()
        {
            for it in &sp.items {
                it.walk(&mut |i| {
                    if let Some(tf) = i.text_frame()
                        && !stories.contains(&tf.story)
                    {
                        stories.push(tf.story);
                    }
                });
            }
        }
        let text = stories
            .iter()
            .filter_map(|id| self.session.clipboard.as_ref()?.story(*id))
            .map(|st| {
                st.text.chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c)).map(|c| if c == '\u{2028}' { '\n' } else { c }).collect::<String>()
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() { '\u{FFFC}'.to_string() } else { text }
    }

    /// Handle requests produced by tools/commands (dialogs, view changes, file pickers).
    pub fn after_engine(&mut self) {
        let reqs = std::mem::take(&mut self.session.ui_requests);
        for r in reqs {
            let was_find = self.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange");
            match r {
                UiRequest::Dialog { id, params } if id == "colorTheme" => dialogs::open_color_theme(self, params),
                UiRequest::Dialog { id, params } => self.ui.dialog = Some(dialogs::Dialog::new(&id, params)),
                UiRequest::View { params } => canvas::apply_view_request(self, &params),
                UiRequest::Pick { purpose, params } => {
                    let _ = params;
                    let _ = self.pick_and_open(&purpose);
                }
            }
            // Release only when the request actually replaces Find, including a native PDF picker.
            if was_find && self.ui.dialog.as_ref().is_none_or(|d| d.id != "findChange") {
                self.session.reset_find_scope();
            }
        }
        // New documents get a fitted view.
        if let Some(st) = self.session.active() {
            let uid = st.uid;
            self.views.entry(uid).or_default();
        }
    }

    /// Ask the host for a file to open (`open`) or place (`place`). Synchronous pickers run the
    /// command right away; asynchronous ones (web) deliver the file through the inbox.
    pub fn pick_and_open(&mut self, purpose: &str) -> Result<Value, String> {
        let target = self.session.active().map(|d| d.uid);
        if let Some(pick) = self.services.pick_open.as_mut() {
            return match pick(purpose) {
                // Image Import Options: a multi-page PDF asks which page.
                Some(path) if purpose == "place" && path.to_ascii_lowercase().ends_with(".pdf") => {
                    let pages =
                        self.services.read.as_mut().and_then(|r| r(&path).ok()).and_then(|b| designcraft_render::pdf_page_count(&b)).unwrap_or(1);
                    if pages > 1 {
                        self.ui.dialog =
                            Some(dialogs::Dialog::new("pdfImport", json!({"path": path, "page": "1", "pages": pages, "target": target})));
                        Ok(Value::Null)
                    } else {
                        self.place_file(json!({"path": path}))
                    }
                }
                Some(path) if purpose == "place" => self.place_file(json!({"path": path})),
                Some(path) => self.open_file("file.open", json!({"path": path})),
                None => Ok(Value::Null),
            };
        }
        let request = self.import_request(purpose);
        if let Some(open) = self.services.open_async.as_mut() {
            open(request);
        }
        Ok(Value::Null)
    }

    /// Load Styles: ask the host for a document whose styles to load; `kind` is `paragraph`,
    /// `character` or empty for both. A synchronous picker opens the dialog that chooses among
    /// the file's styles; an asynchronous one (web) loads them all when the file arrives.
    pub fn pick_styles(&mut self, kind: &str) -> Result<Value, String> {
        designcraft_engine::cmd::has_doc(&self.session)?;
        if let Some(pick) = self.services.pick_open.as_mut() {
            let Some(path) = pick("styles") else { return Ok(Value::Null) };
            let found = self.run("style.load.list", json!({"path": path}))?;
            let of = |k: &str| if kind.is_empty() || kind == k { found[k].clone() } else { json!([]) };
            self.ui.dialog = Some(dialogs::Dialog::new(
                "loadStyles",
                json!({"path": path, "paragraph": of("paragraph"), "character": of("character"), "conflicts": "redefine"}),
            ));
            return Ok(Value::Null);
        }
        let request = self.import_request(&if kind.is_empty() { "styles".to_string() } else { format!("styles:{kind}") });
        if let Some(open) = self.services.open_async.as_mut() {
            open(request);
        }
        Ok(Value::Null)
    }

    pub fn import_request(&self, purpose: &str) -> ImportRequest {
        ImportRequest { purpose: purpose.into(), target: self.session.active().map(|d| d.uid) }
    }

    pub(crate) fn activate_import_target(&mut self, request: &ImportRequest) -> Result<(), String> {
        let Some(index) = request.target.and_then(|uid| self.session.documents().iter().position(|d| d.uid == uid)) else {
            let message = "Import cancelled: the originating document has closed";
            self.status(crate::i18n::tr(&self.ui.language, message));
            return Err(message.into());
        };
        self.run("file.activate", json!({"index": index}))?;
        Ok(())
    }

    pub(crate) fn cancel_pdf_import(&mut self) {
        self.pending_pdf = None;
    }

    pub(crate) fn confirm_pdf_import(&mut self, page: u64, crop: String) -> Result<Value, String> {
        let file = self.pending_pdf.take().ok_or("PDF import cancelled: no pending file")?;
        self.activate_import_target(&file.request)?;
        let params = json!({"name": file.name, "base64": designcraft_engine::cmd::base64_encode(&file.bytes), "pdfPage": page, "pdfCrop": crop});
        if file.request.purpose == "place" { self.place_file(params) } else { self.open_file("file.place", params) }
    }

    /// A single unresolved PDF owns its bytes. Later deliveries cancel explicitly rather than
    /// replacing its options. Cancellation, replacement and target closure release its payload.
    fn drain_inbox(&mut self) {
        if let Some(file) = &self.pending_pdf {
            let target_exists = self.session.documents().iter().any(|d| Some(d.uid) == file.request.target);
            let owns_dialog =
                self.ui.dialog.as_ref().is_some_and(|d| d.id == "pdfImport" && d.fields.get("async").and_then(Value::as_bool) == Some(true));
            if !target_exists || !owns_dialog {
                self.pending_pdf = None;
                if !target_exists {
                    if owns_dialog {
                        self.ui.dialog = None;
                    }
                    self.status(crate::i18n::tr(&self.ui.language, "PDF import cancelled: the originating document has closed"));
                }
            }
        }
        let Some(inbox) = self.services.inbox.clone() else { return };
        let files = std::mem::take(&mut *inbox.lock().unwrap_or_else(|e| e.into_inner()));
        for file in files {
            if self.ui.dialog.is_some() {
                self.status(
                    crate::i18n::tr(&self.ui.language, "{name}: import cancelled while a dialog is open; choose the file again")
                        .replace("{name}", &file.name),
                );
                continue;
            }
            let name = file.name.clone();
            if let Err(e) = self.import_file(file) {
                self.status(format!("{name}: {e}"));
            }
        }
    }

    fn import_file(&mut self, file: ImportedFile) -> Result<Value, String> {
        let lower = file.name.to_ascii_lowercase();
        if let Some(kind) = file.request.purpose.strip_prefix("styles") {
            self.activate_import_target(&file.request)?;
            let mut params = json!({"name": file.name, "base64": designcraft_engine::cmd::base64_encode(&file.bytes)});
            match kind {
                ":paragraph" => params["character"] = json!([]),
                ":character" => params["paragraph"] = json!([]),
                _ => {}
            }
            return self.run("style.load", params);
        }
        let layout = opens_as_document(&file.name)
            || (std::path::Path::new(&file.name).extension().is_none() && designcraft_engine::cmd::is_document(&file.bytes));
        let place =
            file.request.purpose == "place" || (matches!(file.request.purpose.as_str(), "drop" | "open") && !layout && !lower.ends_with(".ase"));
        if place || lower.ends_with(".ase") || file.request.purpose == "swatches" {
            self.activate_import_target(&file.request)?;
        }
        if place && lower.ends_with(".pdf") {
            let pages = designcraft_render::pdf_page_count(&file.bytes).unwrap_or(1);
            if pages > 1 {
                self.ui.dialog =
                    Some(dialogs::Dialog::new("pdfImport", json!({"async": true, "name": file.name.clone(), "page": "1", "pages": pages})));
                self.pending_pdf = Some(file);
                return Ok(Value::Null);
            }
        }
        let b64 = designcraft_engine::cmd::base64_encode(&file.bytes);
        if lower.ends_with(".ase") || file.request.purpose == "swatches" {
            self.run("swatch.load", json!({"base64": b64}))
        } else if file.request.purpose == "place" {
            self.place_file(json!({"name": file.name, "base64": b64}))
        } else if place {
            self.open_file("file.place", json!({"name": file.name, "base64": b64}))
        } else {
            let title = file.name.rsplit_once('.').map_or(file.name.as_str(), |(stem, _)| stem);
            self.open_file("file.openBytes", json!({"name": title, "base64": b64}))
        }
    }

    pub fn select_tool(&mut self, id: &str) {
        let _ = self.run("tool.select", json!({"tool": id}));
        self.ui.flyout = None;
    }

    /// Per-frame logic before layout.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if let Some(result) = self.pdf_export.poll(&mut self.services.write) {
            self.status(match result {
                Ok(v) => format!("PDF exported: {} ({} pages)", v["path"].as_str().unwrap_or(""), v["pages"]),
                Err(e) => format!("PDF export failed: {e}"),
            });
        }
        if self.egui_ctx.is_none() {
            self.egui_ctx = Some(ctx.clone());
        }
        self.window_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if !self.color_applied {
            self.color_applied = true;
            if let Some(cs) = self.ui.color_settings.clone() {
                let _ = designcraft_color::cms::set_active(&cs.with_known_profiles());
            }
        }
        let scale = self.ui.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        if !self.styled {
            // The UI scale is a preference (applied above); egui's own Cmd+Plus/Minus/0 would rescale the
            // whole window instead of the canvas, only to be reset next frame.
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
            self.fonts_system = theme::install_fonts(ctx, &self.ui.language);
            self.fonts_lang = self.ui.language.clone();
            self.styled = true;
            self.restyle = true;
        } else {
            if self.fonts_lang != self.ui.language || (!self.fonts_system && theme::system_fonts_ready()) {
                self.fonts_system = theme::install_fonts(ctx, &self.ui.language);
                self.fonts_lang = self.ui.language.clone();
            }
            self.fonts_ready = true;
        }
        if !self.fonts_system {
            // The font scan runs in the background: look again shortly.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.restyle {
            theme::apply(ctx, &theme::Tokens::for_brightness(self.ui.brightness));
            self.restyle = false;
        }
        let now = ctx.input(|i| i.time);
        // Crash recovery: unsaved documents are written to the recovery folder every 30 s.
        #[cfg(not(target_arch = "wasm32"))]
        if !self.pdf_export.busy()
            && self.session.recovery_dir.is_some()
            && now - self.last_recovery > (self.session.prefs.recovery_minutes * 60.0).max(5.0)
        {
            self.last_recovery = now;
            if self.session.documents().iter().any(|d| d.is_dirty())
                && let Err(e) = self.session.execute("file.recovery.save", &json!({}))
            {
                self.status(crate::i18n::tr(&self.ui.language, "Couldn't write recovery data: {error}").replace("{error}", &e.to_string()));
            }
        }
        let dt = now - self.last_time;
        if dt > 0.0 {
            self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
        }
        self.last_time = now;
        self.drain_control(ctx);
        new_document::end_stale_preview(self);
        if self.fonts_ready {
            self.drain_inbox();
            self.open_os_documents();
        }
        if !self.synthetic.is_empty() || !self.control_synthetic.is_empty() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        if self.fonts_ready {
            menus::shortcuts(self, ctx);
        }
        self.flush_window_commands(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (dropped, pointer) = ctx.input(|i| (i.raw.dropped_files.clone(), i.pointer.hover_pos()));
            for f in dropped {
                let p = f.path().to_string_lossy().to_string();
                if !p.is_empty() {
                    let _ = self.open_dropped_at(&p, pointer);
                }
            }
        }
        self.handle_quit(ctx);
    }

    /// Quit: close the window the way its close button does, so unsaved documents ask first.
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// Ask about unsaved documents without closing any of them yet. A later Cancel must also
    /// keep documents whose earlier answer was Don't Save, including their history and recovery.
    fn handle_quit(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.pdf_export.busy() && !self.quit_without_asking {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                return;
            }
            if self.quit_without_asking {
                self.quitting = None;
                return;
            }
            if self.quitting.is_none() && !self.session.documents().iter().any(|d| d.is_dirty()) {
                return;
            }
            let approved =
                self.quitting.as_ref().is_some_and(|q| q.pending.is_none()) && self.ui.dialog.is_none() && self.next_unapproved_document().is_none();
            if approved {
                // Commit only when the backend receives the final close, after checking for
                // edits that arrived since the last answer. Closing also discards recovery data.
                if let Err(e) = self.finish_confirmed_quit() {
                    self.status(e);
                    self.cancel_quit();
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                }
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            // Dock Quit can arrive while the app is hidden or minimized. Make its question
            // visible through the normal safe window commands before asking about documents.
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if self.quitting.is_none() {
                self.quitting = Some(QuitState { original_active: self.session.active().map(|d| d.uid), accepted: HashMap::new(), pending: None });
            }
        }
        let Some(q) = self.quitting.as_ref() else { return };
        if let Some(uid) = q.pending {
            let asking = self.ui.dialog.as_ref().is_some_and(|d| {
                d.id == "closeDocument"
                    && d.fields.get("quitting").and_then(Value::as_bool) == Some(true)
                    && d.fields.get("uid").and_then(Value::as_u64) == Some(uid)
            });
            if !asking {
                self.cancel_quit();
            }
            return;
        }
        if self.ui.dialog.is_some() {
            self.cancel_quit();
            return;
        }
        self.ask_next_unsaved(ctx);
    }

    /// Finalize a complete, still-current quit plan at an orderly host exit. The host can
    /// terminate before egui receives its queued Close; an unfinished/stale plan leaves every
    /// document live so the host can preserve its recovery data instead.
    pub fn finish_confirmed_quit(&mut self) -> Result<(), String> {
        let approved =
            self.quitting.as_ref().is_some_and(|q| q.pending.is_none()) && self.ui.dialog.is_none() && self.next_unapproved_document().is_none();
        if !approved {
            return Ok(());
        }
        while !self.session.documents().is_empty() {
            self.run("file.close", json!({"index": 0}))?;
        }
        self.quitting = None;
        Ok(())
    }

    fn next_unapproved_document(&self) -> Option<(usize, u64)> {
        let q = self.quitting.as_ref()?;
        self.session
            .documents()
            .iter()
            .enumerate()
            .find(|(_, d)| d.is_dirty() && q.accepted.get(&d.uid) != Some(&d.revision))
            .map(|(i, d)| (i, d.uid))
    }

    fn cancel_quit(&mut self) {
        let Some(q) = self.quitting.take() else { return };
        if let Some(i) = q.original_active.and_then(|uid| self.session.documents().iter().position(|d| d.uid == uid)) {
            let _ = self.run("file.activate", json!({"index": i}));
        }
    }

    /// Save/Don't Save during Quit approves this UID's current revision, without destroying it.
    pub(crate) fn confirm_quit_document(&mut self, uid: u64, discard: bool) -> Result<Value, String> {
        if self.quitting.as_ref().and_then(|q| q.pending) != Some(uid) {
            return Err("Quit confirmation is no longer active".into());
        }
        let Some(i) = self.session.documents().iter().position(|d| d.uid == uid) else {
            if let Some(q) = self.quitting.as_mut() {
                q.pending = None;
            }
            return Ok(Value::Null);
        };
        if !discard {
            let saved = self.run("file.activate", json!({"index": i})).and_then(|_| self.run("app.save", json!({})));
            if let Err(e) = saved {
                self.cancel_quit();
                return Err(e);
            }
            if self.session.documents().iter().find(|d| d.uid == uid).is_none_or(|d| d.is_dirty()) {
                // No file chosen (or an asynchronous host save has not completed): stop quitting.
                self.cancel_quit();
                return Ok(Value::Null);
            }
        }
        if let Some(q) = self.quitting.as_mut() {
            if let Some(d) = self.session.documents().iter().find(|d| d.uid == uid) {
                q.accepted.insert(uid, d.revision);
            }
            q.pending = None;
        }
        Ok(Value::Null)
    }

    /// Show the next unapproved document, or request a final close that revalidates all approvals.
    fn ask_next_unsaved(&mut self, ctx: &egui::Context) {
        let Some((i, uid)) = self.next_unapproved_document() else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        if self.run("file.activate", json!({"index": i})).is_err() {
            self.cancel_quit();
            return;
        }
        menus::close_document(self, Some(i));
        if let Some(d) = self.ui.dialog.as_mut() {
            d.fields.insert("quitting".into(), json!(true));
        }
        if let Some(q) = self.quitting.as_mut() {
            q.pending = Some(uid);
        }
    }

    /// A file dropped on the window: documents open, the swatches in a swatch exchange (.ase) file
    /// are loaded, and anything else is placed on the spread in view (a parent spread while parents
    /// are being edited). A file without an extension opens when its content is a DesignCraft or
    /// IDML document.
    pub fn open_dropped(&mut self, path: &str) -> Result<Value, String> {
        self.open_dropped_at(path, None)
    }

    /// [`Self::open_dropped`] at the pointer: a graphic's top left lands where the file was
    /// dropped on the canvas, text goes to a new frame on the document page there.
    pub fn open_dropped_at(&mut self, path: &str, pointer: Option<egui::Pos2>) -> Result<Value, String> {
        if path.to_ascii_lowercase().ends_with(".ase") {
            return self.run("swatch.load", json!({"path": path}));
        }
        let document = opens_as_document(path)
            || (std::path::Path::new(path).extension().is_none()
                && designcraft_engine::io::read_capped(path, designcraft_engine::cmd::DOCUMENT_SNIFF_BYTES)
                    .is_ok_and(|b| designcraft_engine::cmd::is_document(&b)));
        if document {
            return self.open_file("file.open", json!({"path": path}));
        }
        let mut params = json!({"path": path});
        if let (Some((sr, at)), Some(o)) = (canvas::drop_point(self, pointer), params.as_object_mut()) {
            o.insert("spread".into(), serde_json::to_value(sr).unwrap_or(Value::Null));
            o.insert("x".into(), json!(at.x));
            o.insert("y".into(), json!(at.y));
            if let (designcraft_doc::SpreadRef::Doc(si), Some(st)) = (sr, self.session.active()) {
                let pi = st.doc.spreads.get(si).and_then(|s| s.page_at_x(at.x)).unwrap_or(0);
                o.insert("page".into(), json!(st.doc.first_page_of_spread(si) + pi + 1));
            }
        }
        self.open_file("file.place", params)
    }

    /// Queue files the operating system asked the app to open. They can arrive before the window
    /// exists (the event that launched the app); the first ready frame opens them, each as a new
    /// document through `file.open`, as File › Open does.
    pub fn open_documents_later(&mut self, paths: impl IntoIterator<Item = String>) {
        self.os_documents.extend(paths);
    }

    fn open_os_documents(&mut self) {
        for path in std::mem::take(&mut self.os_documents) {
            if let Err(e) = self.open_file("file.open", json!({"path": path})) {
                log::warn!("{path}: {e}");
            }
        }
    }

    /// File › Place: the file fills the selected empty or graphic frame; with none, an image loads
    /// into the place cursor (click to place at its natural size, drag to draw the frame).
    pub(crate) fn place_file(&mut self, mut params: Value) -> Result<Value, String> {
        if let Some(o) = params.as_object_mut() {
            o.insert("loadCursor".into(), json!(true));
        }
        self.open_file("file.place", params)
    }

    /// Open (`file.open`, `file.openBytes`) or place (`file.place`) a file the user chose. A file
    /// that can't be opened or placed gets an alert with the reason, as well as the status line.
    /// A successful import with warnings keeps the document and shows the warnings in an alert.
    pub fn open_file(&mut self, cmd: &str, params: Value) -> Result<Value, String> {
        let file = match params.get("path").and_then(Value::as_str) {
            Some(path) => std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string()),
            None => params.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        };
        self.open_named_file(cmd, params, file)
    }

    /// [`Self::open_file`] for a command whose params don't name the file: `file` is shown in the
    /// alert or warnings.
    pub fn open_named_file(&mut self, cmd: &str, params: Value, file: String) -> Result<Value, String> {
        let r = self.run(cmd, params);
        if let Err(e) = &r {
            let title = if cmd == "file.place" { "Can't Place the File" } else { "Can't Open the File" };
            if self.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange") {
                self.session.reset_find_scope();
            }
            self.ui.dialog = Some(dialogs::Dialog::new("alert", json!({"title": title, "file": file, "message": e})));
        } else if let Ok(result) = &r {
            let warnings: Vec<&str> = result
                .get("warnings")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|warning| !warning.is_empty())
                .collect();
            if !warnings.is_empty() {
                self.status(format!("{file}: {}", warnings.join("; ")));
                // Find Font, offered for the document's missing fonts, stays up: the status line
                // keeps the warnings.
                if self.ui.dialog.as_ref().is_none_or(|d| d.id != "findFont") {
                    self.ui.dialog =
                        Some(dialogs::Dialog::new("alert", json!({"title": "Import Warnings", "file": file, "message": warnings.join("\n")})));
                }
            }
        }
        r
    }

    /// Deliver window actions to the host without persisting them in UI preferences.
    pub(crate) fn flush_window_commands(&mut self, ctx: &egui::Context) {
        for command in std::mem::take(&mut self.pending_window_commands) {
            ctx.send_viewport_cmd(command);
        }
    }

    /// Keep widget clipboard output private while a queued control event is running.
    pub(crate) fn install_clipboard_guard(&mut self, ctx: &egui::Context) {
        if self.clipboard_guard_installed {
            return;
        }
        let private = self.control_input_frame.clone();
        ctx.on_end_pass(
            "control clipboard",
            std::sync::Arc::new(move |ui| {
                if private.load(std::sync::atomic::Ordering::Relaxed) {
                    ui.ctx().output_mut(|o| {
                        o.commands.retain(|cmd| !matches!(cmd, egui::OutputCommand::CopyText(_) | egui::OutputCommand::CopyImage(_)))
                    });
                }
            }),
        );
        self.clipboard_guard_installed = true;
    }

    /// Inject synthetic events (one pointer event per frame), retaining control provenance.
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        let private = !self.control_synthetic.is_empty();
        self.control_input_frame.store(private, std::sync::atomic::Ordering::Relaxed);
        let events = if private { &mut self.control_synthetic } else { &mut self.synthetic };
        let Some(first) = events.first() else { return };
        let n = match first {
            egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } => 1,
            _ => events.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(events.len(), |i| i + 1),
        };
        if let egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. } = first {
            raw.events.push(egui::Event::PointerMoved(*p));
        }
        raw.events.extend(events.drain(..n));
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.canvas_ime = false;
        if !self.fonts_ready {
            ctx.request_repaint();
            return;
        }
        self.settle_live_preview();
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
        let presenting = self.ui.screen_mode == ScreenMode::Presentation;
        if presenting || self.ui.hidden_panels != 0 {
            panel_docking::cancel_gesture(self, &ctx);
        }
        canvas::begin_frame(&ctx);
        let dbg = std::env::var_os("DESIGNCRAFT_DEBUG_LAYOUT").is_some();
        if dbg {
            eprintln!("root start {:?}", ui.available_rect_before_wrap());
        }
        if !presenting {
            chrome::app_bar(self, ui);
            if dbg {
                eprintln!("after app bar {:?}", ui.available_rect_before_wrap());
            }
            if self.ui.control_bar && self.session.active().is_some() && self.ui.hidden_panels == 0 {
                chrome::control_bar(self, ui);
            }
            chrome::status_bar(self, ui);
            if dbg {
                eprintln!("after status {:?}", ui.available_rect_before_wrap());
            }
            if self.ui.hidden_panels != 1 {
                toolbar::show(self, ui);
            }
            if self.ui.hidden_panels == 0 {
                dock::show(self, ui);
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.pasteboard)).show(ui, |ui| {
            if self.session.active().is_none() {
                chrome::start_screen(self, ui);
                return;
            }
            if !presenting {
                chrome::doc_tabs(self, ui);
            }
            let split_layout = (self.split && !presenting)
                .then(|| {
                    craft_ui::layout::split_rect(
                        ui.available_rect_before_wrap(),
                        craft_ui::layout::SplitAxis::Horizontal,
                        craft_ui::layout::SplitSize::Ratio(0.5),
                        2.0,
                        0.0,
                    )
                })
                .flatten();
            if let Some(layout) = split_layout {
                // Two panes with a divider; each has its own view (zoom, scroll) and render cache.
                ui.painter().rect_filled(layout.divider, 0.0, t.border);
                // Resolve the clicked pane before either canvas handles this frame's keys.
                if let Some(p) = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten()) {
                    if layout.first.contains(p) {
                        self.focus_pane = 0;
                    } else if layout.second.contains(p) {
                        self.focus_pane = 1;
                    }
                }
                for (k, half) in [layout.first, layout.second].into_iter().enumerate() {
                    self.switch_pane(k as u8);
                    // A new pane starts at the other pane's view.
                    if k == 1 && self.view().is_none() {
                        self.switch_pane(0);
                        let v = self.view().copied();
                        self.switch_pane(1);
                        if let (Some(mut v), Some(m)) = (v, self.view_mut()) {
                            v.fitted = false;
                            *m = v;
                        }
                    }
                    ui.scope_builder(egui::UiBuilder::new().max_rect(half), |ui| canvas::show(self, ui));
                }
                self.switch_pane(self.focus_pane);
            } else {
                self.switch_pane(0);
                // With a second window, the main one takes the focus back when clicked.
                if !self.second_window || (ui.input(|i| i.pointer.any_pressed()) && ui.ui_contains_pointer()) {
                    self.focus_pane = 0;
                }
                canvas::show(self, ui);
            }
        });
        // New Window: pane 1 in a window of its own.
        if self.second_window
            && !presenting
            && let Some(title) = self.session.active().map(|d| format!("{} — 2", d.doc.title))
        {
            let fill = t.pasteboard;
            let ctx = ui.ctx().clone();
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("designcraft_second_window"),
                egui::ViewportBuilder::default().with_title(title).with_inner_size([960.0, 720.0]),
                |ui, _class| {
                    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(fill)).show(ui, |ui| {
                        self.switch_pane(1);
                        if self.view().is_none() {
                            self.switch_pane(0);
                            let v = self.view().copied();
                            self.switch_pane(1);
                            if let (Some(mut v), Some(m)) = (v, self.view_mut()) {
                                v.fitted = false;
                                *m = v;
                            }
                        }
                        if ui.input(|i| i.pointer.any_pressed()) {
                            self.focus_pane = 1;
                        }
                        canvas::show(self, ui);
                    });
                    if ui.input(|i| i.viewport().close_requested()) {
                        self.second_window = false;
                        self.focus_pane = 0;
                    }
                },
            );
            self.switch_pane(self.focus_pane);
        }
        // Tab, Shift+Tab and Presentation hide the floating panels with the docked ones.
        if !presenting && self.ui.hidden_panels == 0 {
            dock::flyout(self, &ctx);
            dock::floating(self, &ctx);
        }
        story_editor::show(self, &ctx);
        dialogs::show(self, &ctx);
        about::show(self, &ctx);
        menus::palette(self, &ctx);
        self.pdf_export.show(&ctx);
        if self.custom_titlebar {
            chrome::resize_borders(self, ui);
        }
        for url in std::mem::take(&mut self.ui.pending_urls) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        self.perf.frame_ms = now_ms() - t0;
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            let reply = req.reply.clone();
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => {
                    self.shot_token += 1;
                    let token = self.shot_token;
                    self.queued_shots.push((token, now_ms() + 120.0, 0));
                    self.pending_shots.push((token, path, reply, now_ms() + 8000.0));
                }
            }
        }
        self.control_rx = Some(rx);
    }

    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
                false
            } else {
                true
            }
        });
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.pending_shots.iter().position(|(t, ..)| *t == token) {
                let (_, path, reply, _) = self.pending_shots.remove(i);
                let _ = reply.send(control::save_screenshot(self, &image, path.as_deref()));
            }
        }
        let now = now_ms();
        self.pending_shots.retain(|(_, _, reply, deadline)| {
            if now < *deadline {
                return true;
            }
            let _ = reply.send(json!({"ok": false, "error": "no frame was presented (screen locked or window hidden); use ui.render"}));
            false
        });
    }
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

/// Files that open as documents (when dropped or picked) rather than being placed: DesignCraft,
/// IDML, and InDesign documents and templates (converted to IDML on open).
pub fn opens_as_document(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".designcraft", ".idml", ".indd", ".indt"].iter().any(|ext| n.ends_with(ext))
}

pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

/// Named `KeyboardEvent.key` values (W3C UI Events) that a browser reports for a key that types
/// nothing, separated by spaces. eframe's web backend sends the ones it doesn't know as text.
const KEY_NAMES: &str = "\
    Unidentified Alt AltGraph CapsLock Control Fn FnLock Hyper Meta NumLock ScrollLock Shift Super Symbol SymbolLock OS Win Enter \
    Tab ArrowDown ArrowLeft ArrowRight ArrowUp End Home PageDown PageUp Backspace Clear Copy CrSel Cut Delete EraseEof ExSel \
    Insert Paste Redo Undo Accept Again Attn Cancel ContextMenu Esc Escape Execute Find Finish Help Pause Play Props Select \
    ZoomIn ZoomOut BrightnessDown BrightnessUp Eject LogOff Power PowerOff PrintScreen Hibernate Standby WakeUp AllCandidates \
    Alphanumeric CodeInput Compose Convert Dead FinalMode GroupFirst GroupLast GroupNext GroupPrevious ModeChange NextCandidate \
    NonConvert PreviousCandidate Process SingleCandidate HangulMode HanjaMode JunjaMode Eisu Hankaku Hiragana HiraganaKatakana \
    KanaMode KanjiMode Katakana Romaji Zenkaku ZenkakuHankaku Soft1 Soft2 Soft3 Soft4 ChannelDown ChannelUp Close MailForward \
    MailReply MailSend MediaClose MediaFastForward MediaPause MediaPlay MediaPlayPause MediaRecord MediaRewind MediaStop \
    MediaTrackNext MediaTrackPrevious New Open Print Save SpellCheck Key11 Key12 AudioBalanceLeft AudioBalanceRight \
    AudioBassBoostDown AudioBassBoostToggle AudioBassBoostUp AudioFaderFront AudioFaderRear AudioSurroundModeNext AudioTrebleDown \
    AudioTrebleUp AudioVolumeDown AudioVolumeUp AudioVolumeMute MicrophoneToggle MicrophoneVolumeDown MicrophoneVolumeUp \
    MicrophoneVolumeMute SpeechCorrectionList SpeechInputToggle LaunchApplication1 LaunchApplication2 LaunchCalendar \
    LaunchContacts LaunchMail LaunchMediaPlayer LaunchMusicPlayer LaunchPhone LaunchScreenSaver LaunchSpreadsheet \
    LaunchWebBrowser LaunchWebCam LaunchWordProcessor BrowserBack BrowserFavorites BrowserForward BrowserHome BrowserRefresh \
    BrowserSearch BrowserStop AppSwitch Call Camera CameraFocus EndCall GoBack GoHome HeadsetHook LastNumberRedial Notification \
    MannerMode VoiceDial TV TV3DMode TVAntennaCable TVAudioDescription TVAudioDescriptionMixDown TVAudioDescriptionMixUp \
    TVContentsMenu TVDataService TVInput TVInputComponent1 TVInputComponent2 TVInputComposite1 TVInputComposite2 TVInputHDMI1 \
    TVInputHDMI2 TVInputHDMI3 TVInputHDMI4 TVInputVGA1 TVMediaContext TVNetwork TVNumberEntry TVPower TVRadioService TVSatellite \
    TVSatelliteBS TVSatelliteCS TVSatelliteToggle TVTerrestrialAnalog TVTerrestrialDigital TVTimer AVRInput AVRPower ColorF0Red \
    ColorF1Green ColorF2Yellow ColorF3Blue ColorF4Grey ColorF5Brown ClosedCaptionToggle Dimmer DisplaySwap DVR Exit \
    FavoriteClear0 FavoriteClear1 FavoriteClear2 FavoriteClear3 FavoriteRecall0 FavoriteRecall1 FavoriteRecall2 FavoriteRecall3 \
    FavoriteStore0 FavoriteStore1 FavoriteStore2 FavoriteStore3 Guide GuideNextDay GuidePreviousDay Info InstantReplay Link \
    ListProgram LiveContent Lock MediaApps MediaAudioTrack MediaLast MediaSkipBackward MediaSkipForward MediaStepBackward \
    MediaStepForward MediaTopMenu NavigateIn NavigateNext NavigateOut NavigatePrevious NextFavoriteChannel NextUserProfile \
    OnDemand Pairing PinPDown PinPMove PinPToggle PinPUp PlaySpeedDown PlaySpeedReset PlaySpeedUp RandomToggle RcLowBattery \
    RecordSpeedNext RfBypass ScanChannelsToggle ScreenModeNext Settings SplitScreenToggle STBInput STBPower Subtitle Teletext \
    VideoModeNext Wink ZoomToggle";

/// Is `text` the name of a key ("AltGraph", "Dead", "F13") instead of typed text?
pub fn is_key_name(text: &str) -> bool {
    let function_key = text.strip_prefix('F').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    function_key || KEY_NAMES.split_ascii_whitespace().any(|k| k == text)
}

/// Drop text events that are key names: eframe's web backend sends the name of every key it
/// doesn't know as text, so AltGr typed "AltGraph" into the story. `text_input_focused`: an egui
/// text field or the canvas's text caret has the keyboard, so the browser's hidden text input
/// sends the text, which holds only typed text (whole words from autocompletion too) and is left
/// alone.
pub fn drop_key_name_text(raw: &mut egui::RawInput, text_input_focused: bool) {
    if !text_input_focused {
        raw.events.retain(|e| !matches!(e, egui::Event::Text(t) if is_key_name(t)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Control panel's two-row groups were centred as if they were one row tall, so the second
    /// row hung below the panel and was cut off.
    #[test]
    fn control_bar_shows_its_second_row() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        app.run("file.newSample", json!({})).unwrap();
        app.ui.control_bar = true;
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        let bar = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("control_bar")).unwrap().outer_rect;
        assert!(bar.height() > 40.0, "{bar:?}");
        // The X/Y/W/H group: its second row holds Y: and H:.
        let group = h.ctx.read_response(egui::Id::new(("object_control", "dimensions"))).unwrap().rect;
        assert!(bar.contains(group.left_top()), "the X/Y/W/H group {group:?} is in the Control panel {bar:?}");
        assert!(group.bottom() <= bar.bottom(), "X/Y/W/H group {group:?} hangs below the Control panel {bar:?}");
    }

    #[test]
    fn key_names_sent_as_text_are_dropped() {
        let text = |t: &str| egui::Event::Text(t.to_string());
        let events = || {
            vec![text("AltGraph"), text("é"), text("Dead"), text("A"), text("Unidentified"), text("ß"), text("AudioVolumeUp"), text("ab"), text("Æ")]
        };
        let mut raw = egui::RawInput { events: events(), ..Default::default() };
        drop_key_name_text(&mut raw, false);
        assert_eq!(raw.events, vec![text("é"), text("A"), text("ß"), text("ab"), text("Æ")]);
        assert!(is_key_name("F13") && is_key_name("Escape") && is_key_name("ZoomToggle") && !is_key_name("F") && !is_key_name("Fx"));
        // Capitalised words (mobile autocompletion) are text.
        let words = vec![text("Hello"), text("Tokyo"), text("OK"), text("Hi there")];
        let mut raw = egui::RawInput { events: words.clone(), ..Default::default() };
        drop_key_name_text(&mut raw, false);
        assert_eq!(raw.events, words);
        // A focused text field or a canvas text caret gets its text from the browser's input
        // events, which hold whole words, key-name-like ones too ("Help").
        let mut raw = egui::RawInput { events: events(), ..Default::default() };
        drop_key_name_text(&mut raw, true);
        assert_eq!(raw.events, events());
    }

    /// InDesign shows 100% K as pure black on screen unless Appearance of Black says otherwise
    /// (Display All Blacks as Rich Black is its default); settings saved without the field get it too.
    #[test]
    fn black_shows_as_rich_black_by_default() {
        assert!(UiState::default().rich_black);
        let saved: UiState = serde_json::from_str("{}").unwrap();
        assert!(saved.rich_black);
        let accurate: UiState = serde_json::from_str(r#"{"richBlack": false}"#).unwrap();
        assert!(!accurate.rich_black, "a saved choice is kept");
    }

    /// An app with one document that has an unsaved edit.
    fn app_with_unsaved_document() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        assert!(!app.session.documents()[0].is_dirty(), "a new document has nothing to save");
        app.run("frame.create", json!({"rect": [36, 36, 136, 136]})).unwrap();
        assert!(app.session.documents()[0].is_dirty());
        app
    }

    /// File ▸ Close (and its shortcut) closed a document with unsaved changes without asking, and
    /// closing discards the document's recovery data too.
    #[test]
    fn closing_an_unsaved_document_asks_first() {
        let mut app = app_with_unsaved_document();
        menus::activate(&mut app, "file.close", &Value::Null);
        assert_eq!(app.session.documents().len(), 1, "the document stays open until the user answers");
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("closeDocument"));
        // Cancel: nothing happens.
        app.ui.dialog = None;
        assert!(app.session.documents()[0].is_dirty());
        // The tab's × asks too.
        menus::close_document(&mut app, Some(0));
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("closeDocument"));
        assert_eq!(app.session.documents().len(), 1);
    }

    #[test]
    fn closing_without_saving_discards_the_document() {
        let mut app = app_with_unsaved_document();
        menus::close_document(&mut app, None);
        app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
        dialogs::confirm(&mut app).unwrap();
        assert!(app.session.documents().is_empty());
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn closing_with_save_writes_the_file_then_closes() {
        let mut app = app_with_unsaved_document();
        let path = std::env::temp_dir().join(format!("designcraft-close-test-{}.designcraft", std::process::id()));
        app.run("file.saveAs", json!({"path": path})).unwrap();
        let saved = std::fs::metadata(&path).unwrap().len();
        app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
        menus::close_document(&mut app, None);
        dialogs::confirm(&mut app).unwrap();
        assert!(app.session.documents().is_empty());
        let written = std::fs::metadata(&path).unwrap().len();
        std::fs::remove_file(&path).unwrap();
        assert!(written > saved, "the second frame was saved ({saved} → {written} bytes)");
    }

    /// Save on a document that has no file opens the file picker; cancelling it must not close the
    /// document. (No picker service here: the same as cancelling.)
    #[test]
    fn a_cancelled_save_keeps_the_document_open() {
        let mut app = app_with_unsaved_document();
        menus::close_document(&mut app, None);
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.documents().len(), 1);
        assert!(app.session.documents()[0].is_dirty());
    }

    #[test]
    fn closing_a_saved_document_does_not_ask() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        menus::activate(&mut app, "file.close", &Value::Null);
        assert!(app.session.documents().is_empty());
        assert!(app.ui.dialog.is_none());
    }

    /// One frame of the quit handling, with the window asked to close when `close`; returns the
    /// commands sent to the window.
    fn quit_frame(app: &mut DesignApp, ctx: &egui::Context, close: bool) -> Vec<egui::ViewportCommand> {
        let mut raw = egui::RawInput::default();
        if close {
            raw.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
        }
        let mut out = ctx.run_ui(raw, |ui| app.handle_quit(ui.ctx()));
        out.textures_delta.clear();
        out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    }

    fn dialog_uid(app: &DesignApp) -> Option<u64> {
        app.ui.dialog.as_ref().filter(|d| d.id == "closeDocument").and_then(|d| d.fields.get("uid")?.as_u64())
    }

    /// Quitting closed the window at once, losing unsaved changes (#544).
    #[test]
    fn quitting_with_unsaved_changes_asks_and_cancel_keeps_everything() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        let cmds = quit_frame(&mut app, &ctx, true);
        assert!(cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
        assert_eq!(dialog_uid(&app), Some(app.session.documents()[0].uid));
        // Cancel: the window and the document stay as they were.
        app.ui.dialog = None;
        for _ in 0..2 {
            assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        }
        assert_eq!(app.session.documents().len(), 1);
        assert!(app.session.documents()[0].is_dirty());
        // A Save whose file picker is cancelled stops quitting too.
        quit_frame(&mut app, &ctx, true);
        dialogs::confirm(&mut app).unwrap();
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert!(app.session.documents()[0].is_dirty());
    }

    #[test]
    fn quitting_asks_about_each_unsaved_document_then_closes() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 136, 136]})).unwrap();
        app.run("file.new", json!({})).unwrap();
        let uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        for (n, uid) in uids.iter().take(2).enumerate() {
            assert_eq!(dialog_uid(&app), Some(*uid), "question {n}");
            assert_eq!(app.session.active().map(|d| d.uid), Some(*uid), "the document asked about is shown");
            app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
            dialogs::confirm(&mut app).unwrap();
            let cmds = quit_frame(&mut app, &ctx, false);
            assert_eq!(cmds.contains(&egui::ViewportCommand::Close), n == 1, "{cmds:?}");
        }
        // All documents stay open until the backend's final close commits the decision.
        assert_eq!(app.session.documents().len(), 3);
        assert!(quit_frame(&mut app, &ctx, true).is_empty());
        assert!(app.session.documents().is_empty());
    }

    /// A later Cancel must not destroy documents already approved for discard.
    #[test]
    fn quitting_later_cancel_retains_every_document_and_active_tab() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
        let uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
        let active = app.session.active().unwrap().uid;
        let revisions: Vec<u64> = app.session.documents().iter().map(|d| d.revision).collect();
        let docs: Vec<_> = app.session.documents().iter().map(|d| d.doc.clone()).collect();
        quit_frame(&mut app, &ctx, true);
        app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
        dialogs::confirm(&mut app).unwrap();
        quit_frame(&mut app, &ctx, false);
        assert_eq!(dialog_uid(&app), Some(uids[1]));
        app.ui.dialog = None;
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.documents().iter().map(|d| d.uid).collect::<Vec<_>>(), uids);
        assert_eq!(app.session.active().unwrap().uid, active);
        assert_eq!(app.session.documents().iter().map(|d| d.revision).collect::<Vec<_>>(), revisions);
        for (d, original) in app.session.documents().iter().zip(docs) {
            assert!(std::sync::Arc::ptr_eq(&d.doc, &original));
            assert!(d.is_dirty());
            assert!(!d.history.undo.is_empty());
        }
    }

    fn second_unsaved_document(app: &mut DesignApp) {
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
    }

    fn discard_quit_dialog(app: &mut DesignApp) {
        app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
        dialogs::confirm(app).unwrap();
    }

    #[test]
    fn native_quit_prompts_restore_window_visibility_without_focusing_completed_quits() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        let commands = quit_frame(&mut app, &ctx, true);
        assert!(commands.contains(&egui::ViewportCommand::CancelClose));
        for command in [egui::ViewportCommand::Visible(true), egui::ViewportCommand::Minimized(false), egui::ViewportCommand::Focus] {
            assert!(commands.contains(&command), "unsaved prompt must restore the window: {commands:?}");
        }
        discard_quit_dialog(&mut app);
        let approved = quit_frame(&mut app, &ctx, false);
        assert!(approved.contains(&egui::ViewportCommand::Close));
        let completed = quit_frame(&mut app, &ctx, true);
        assert!(app.session.documents().is_empty());

        let mut clean = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        clean.run("file.new", json!({})).unwrap();
        let clean_close = quit_frame(&mut clean, &ctx, true);
        let mut control = app_with_unsaved_document();
        control.quit_without_asking = true;
        let control_close = quit_frame(&mut control, &ctx, true);
        for commands in [&approved, &completed, &clean_close, &control_close] {
            assert!(
                !commands.iter().any(|command| matches!(
                    command,
                    egui::ViewportCommand::Visible(true) | egui::ViewportCommand::Minimized(false) | egui::ViewportCommand::Focus
                )),
                "completed and immediate quits must not activate the window: {commands:?}"
            );
        }
    }

    /// Dock Quit may arrive again while the first request's dialogs are still open.
    #[test]
    fn repeated_native_quit_requests_preserve_partial_approvals_and_cancel() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        second_unsaved_document(&mut app);
        let original_active = app.session.active().unwrap().uid;
        let uids: Vec<_> = app.session.documents().iter().map(|d| d.uid).collect();
        let docs: Vec<_> = app.session.documents().iter().map(|d| d.doc.clone()).collect();
        let directory = tempfile::tempdir().unwrap();
        app.session.recovery_dir = Some(directory.path().to_path_buf());
        app.run("file.recovery.save", json!({})).unwrap();
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(dialog_uid(&app), Some(uids[0]));
        discard_quit_dialog(&mut app);
        quit_frame(&mut app, &ctx, false);
        assert_eq!(dialog_uid(&app), Some(uids[1]));
        for _ in 0..3 {
            assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
            assert_eq!(dialog_uid(&app), Some(uids[1]));
        }
        app.ui.dialog = None; // Cancel the second document, retaining the first Don't Save answer.
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.active().unwrap().uid, original_active);
        assert_eq!(app.session.documents().iter().map(|d| d.uid).collect::<Vec<_>>(), uids);
        for (document, original) in app.session.documents().iter().zip(docs) {
            assert!(std::sync::Arc::ptr_eq(&document.doc, &original));
            assert!(document.is_dirty());
            assert!(!document.history.undo.is_empty());
            assert!(directory.path().join(format!("{}.designcraft", document.uid)).exists());
        }
        // A fresh Dock Quit must ask about the first document again after cancellation.
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(dialog_uid(&app), Some(uids[0]));
    }

    #[test]
    fn repeated_native_quit_requests_save_and_discard_only_after_all_answers() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        second_unsaved_document(&mut app);
        let uids: Vec<_> = app.session.documents().iter().map(|d| d.uid).collect();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("saved.designcraft");
        let chosen = path.to_string_lossy().to_string();
        app.services.pick_save = Some(Box::new(move |_| Some(chosen.clone())));
        app.session.recovery_dir = Some(directory.path().join("recovery"));
        app.run("file.recovery.save", json!({})).unwrap();
        quit_frame(&mut app, &ctx, true);
        assert_eq!(dialog_uid(&app), Some(uids[0]));
        dialogs::confirm(&mut app).unwrap(); // Save the first document.
        assert!(!app.session.documents()[0].is_dirty());
        quit_frame(&mut app, &ctx, false);
        assert_eq!(dialog_uid(&app), Some(uids[1]));
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(dialog_uid(&app), Some(uids[1]));
        discard_quit_dialog(&mut app);
        assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.documents().len(), 2);
        assert!(app.session.recovery_dir.as_ref().unwrap().join(format!("{}.designcraft", uids[1])).exists());
        assert!(quit_frame(&mut app, &ctx, true).is_empty());
        assert!(app.session.documents().is_empty());
        assert!(path.exists());
        assert_eq!(designcraft_engine::Session::new().execute("file.open", &json!({"path": path})).unwrap()["index"], 0);
        assert_eq!(std::fs::read_dir(app.session.recovery_dir.as_ref().unwrap()).unwrap().count(), 0);
    }

    #[test]
    fn quitting_first_cancel_restores_original_clean_active_document() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        app.run("file.new", json!({})).unwrap();
        let original_active = app.session.active().unwrap().uid;
        quit_frame(&mut app, &ctx, true);
        assert_ne!(app.session.active().unwrap().uid, original_active);
        app.ui.dialog = None;
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.active().unwrap().uid, original_active);
        assert_eq!(app.session.documents().len(), 2);
    }

    #[test]
    fn quitting_save_as_cancel_after_discard_keeps_all_documents() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        second_unsaved_document(&mut app);
        let active = app.session.active().unwrap().uid;
        quit_frame(&mut app, &ctx, true);
        discard_quit_dialog(&mut app);
        quit_frame(&mut app, &ctx, false);
        dialogs::confirm(&mut app).unwrap(); // No picker: Save As was cancelled.
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.documents().len(), 2);
        assert!(app.session.documents().iter().all(|d| d.is_dirty()));
        assert_eq!(app.session.active().unwrap().uid, active);
    }

    #[test]
    fn quitting_save_failure_after_discard_keeps_documents_and_can_retry() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        second_unsaved_document(&mut app);
        let path = std::env::temp_dir()
            .join(format!("designcraft-quit-missing-{}-{}", std::process::id(), app.session.active().unwrap().uid))
            .join("missing")
            .join("document.designcraft");
        assert!(!path.parent().unwrap().exists());
        let picked = path.to_string_lossy().to_string();
        app.services.pick_save = Some(Box::new(move |_| Some(picked.clone())));
        quit_frame(&mut app, &ctx, true);
        discard_quit_dialog(&mut app);
        quit_frame(&mut app, &ctx, false);
        assert!(dialogs::confirm(&mut app).is_err());
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.documents().len(), 2);
        assert!(app.session.documents().iter().all(|d| d.is_dirty()));
        // Starting over must ask about the previously discarded first document again.
        quit_frame(&mut app, &ctx, true);
        assert_eq!(dialog_uid(&app), Some(app.session.documents()[0].uid));
    }

    #[test]
    fn quitting_save_then_later_cancel_keeps_saved_document_open() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        second_unsaved_document(&mut app);
        let first = app.session.documents()[0].uid;
        let path = std::env::temp_dir().join(format!("designcraft-quit-save-{}-{first}.designcraft", std::process::id()));
        let picked = path.to_string_lossy().to_string();
        app.services.pick_save = Some(Box::new(move |_| Some(picked.clone())));
        quit_frame(&mut app, &ctx, true);
        dialogs::confirm(&mut app).unwrap();
        quit_frame(&mut app, &ctx, false);
        assert_eq!(app.session.documents().len(), 2);
        assert!(!app.session.documents()[0].is_dirty());
        assert!(path.exists());
        app.ui.dialog = None;
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.documents()[0].uid, first);
        assert!(app.session.documents()[1].is_dirty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn quitting_rechecks_discard_approval_after_a_later_edit() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        second_unsaved_document(&mut app);
        let first = app.session.documents()[0].uid;
        quit_frame(&mut app, &ctx, true);
        discard_quit_dialog(&mut app);
        quit_frame(&mut app, &ctx, false);
        app.run("file.activate", json!({"index": 0})).unwrap();
        app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
        discard_quit_dialog(&mut app); // Approval is bound to the second UID, despite the switch.
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(dialog_uid(&app), Some(first), "new edit needs a fresh decision");
        assert_eq!(app.session.documents().len(), 2);
    }

    #[test]
    fn quitting_final_close_rechecks_new_unsaved_document() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        quit_frame(&mut app, &ctx, true);
        discard_quit_dialog(&mut app);
        assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        second_unsaved_document(&mut app); // Arrives after Close was queued, before its event.
        let new_uid = app.session.active().unwrap().uid;
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(dialog_uid(&app), Some(new_uid));
        assert_eq!(app.session.documents().len(), 2);
    }

    #[test]
    fn quitting_pending_save_uses_dialog_uid_after_tab_switch() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        second_unsaved_document(&mut app);
        let first = app.session.documents()[0].uid;
        let path = std::env::temp_dir().join(format!("designcraft-quit-uid-{}-{first}.designcraft", std::process::id()));
        let picked = path.to_string_lossy().to_string();
        app.services.pick_save = Some(Box::new(move |_| Some(picked.clone())));
        quit_frame(&mut app, &ctx, true);
        app.run("file.activate", json!({"index": 1})).unwrap();
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.documents()[0].path.as_deref(), path.to_str());
        assert!(!app.session.documents()[0].is_dirty());
        assert!(app.session.documents()[1].is_dirty());
        quit_frame(&mut app, &ctx, false);
        assert_eq!(dialog_uid(&app), Some(app.session.documents()[1].uid));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn quitting_discard_preserves_recovery_until_final_commit() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        let uid = app.session.active().unwrap().uid;
        let dir = std::env::temp_dir().join(format!("designcraft-quit-recovery-{}-{uid}", std::process::id()));
        app.session.recovery_dir = Some(dir.clone());
        app.run("file.recovery.save", json!({})).unwrap();
        let entry = dir.join(format!("{uid}.designcraft"));
        quit_frame(&mut app, &ctx, true);
        discard_quit_dialog(&mut app);
        assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert!(entry.exists(), "a queued close is not a committed close");
        assert!(quit_frame(&mut app, &ctx, true).is_empty());
        assert!(!entry.exists(), "accepted Don't Save must not be recovered on restart");
        assert!(app.session.documents().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn quitting_native_exit_commits_completed_approvals_before_close_event() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        quit_frame(&mut app, &ctx, true);
        discard_quit_dialog(&mut app);
        assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert_eq!(app.session.documents().len(), 1);
        app.finish_confirmed_quit().unwrap();
        assert!(app.session.documents().is_empty());
    }

    #[test]
    fn quitting_native_exit_retains_incomplete_stale_and_new_document_plans() {
        for case in ["incomplete", "stale", "new document"] {
            let ctx = egui::Context::default();
            let mut app = app_with_unsaved_document();
            if case == "incomplete" {
                second_unsaved_document(&mut app);
            }
            quit_frame(&mut app, &ctx, true);
            discard_quit_dialog(&mut app);
            quit_frame(&mut app, &ctx, false);
            if case == "stale" {
                app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
            } else if case == "new document" {
                second_unsaved_document(&mut app);
            }
            let active = app.session.active().unwrap().uid;
            let before: Vec<_> = app
                .session
                .documents()
                .iter()
                .map(|d| (d.uid, d.revision, d.doc.clone(), d.selection.items.clone(), d.history.undo.len(), d.history.redo.len()))
                .collect();
            app.finish_confirmed_quit().unwrap();
            assert_eq!(app.session.documents().len(), before.len(), "{case}");
            assert_eq!(app.session.active().unwrap().uid, active, "{case}");
            for (d, (uid, revision, doc, selection, undo, redo)) in app.session.documents().iter().zip(before) {
                assert_eq!(
                    (d.uid, d.revision, &d.selection.items, d.history.undo.len(), d.history.redo.len()),
                    (uid, revision, &selection, undo, redo),
                    "{case}"
                );
                assert!(std::sync::Arc::ptr_eq(&d.doc, &doc), "{case}");
            }
        }
    }

    #[test]
    fn quitting_without_unsaved_changes_closes_at_once() {
        let ctx = egui::Context::default();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        // Quit (⌘Q) closes the window like its close button.
        app.run("app.quit", json!({})).unwrap();
        assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
        assert!(quit_frame(&mut app, &ctx, true).is_empty());
        assert!(app.ui.dialog.is_none());
    }

    /// Scripts must not hang on a question: the control channel's quit never asks.
    #[test]
    fn control_channel_quit_does_not_ask() {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        let (req, _rx) = control::ControlRequest::new("app.quit", json!({}));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            control::handle(&mut app, ui.ctx(), &req);
        });
        out.textures_delta.clear();
        assert!(quit_frame(&mut app, &ctx, true).is_empty());
        assert!(app.ui.dialog.is_none());
    }

    /// The Control panel's two-row groups were centred as if they were one row tall, so the second
    /// row hung below the panel and was cut off.
    #[test]
    fn opening_missing_fonts_offers_repeated_replacement_and_keeps_errors_visible() {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect":[0,0,200,200], "content":"text", "text":"Sample"})).unwrap();
        let d = std::sync::Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
        let sid = *d.stories.keys().next().unwrap();
        d.story_mut(sid).unwrap().format_chars(0..6, |f| {
            f.over.font_family = Some("Unavailable Test Family".into());
            f.over.font_style = Some("Regular".into());
        });
        let path = std::env::temp_dir().join(format!("designcraft-font-open-{}.designcraft", std::process::id()));
        app.run("file.save", json!({"path":path})).unwrap();
        app.run("file.open", json!({"path":path})).unwrap();
        let dialog = app.ui.dialog.as_mut().unwrap();
        assert_eq!(dialog.id, "findFont");
        assert_eq!(dialog.fields["onOpen"], true);
        dialog.fields.insert("family".into(), json!("Unavailable Test Family"));
        dialog.fields.insert("style".into(), json!("Regular"));
        dialog.fields.insert("toFamily".into(), json!("Unavailable Target"));
        dialog.fields.insert("toStyle".into(), json!("Regular"));
        assert!(dialogs::confirm(&mut app).is_err());
        assert!(app.ui.dialog.as_ref().unwrap().fields.contains_key("error"));
        app.ui.dialog.as_mut().unwrap().fields.insert("toFamily".into(), json!("Source Sans 3"));
        dialogs::confirm(&mut app).unwrap();
        assert!(!app.ui.dialog.as_ref().unwrap().fields.contains_key("_fonts"));
        assert!(app.run("font.list", json!({})).unwrap().as_array().unwrap().iter().all(|f| f["missing"] == false));
        app.ui.dialog = None;
        app.run("file.save", json!({"path":path})).unwrap();
        app.run("file.open", json!({"path":path})).unwrap();
        assert!(app.ui.dialog.is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn snap_view_uses_the_saved_switches() {
        let mut ui = UiState::default();
        assert!(ui.snap_view().snap_to_guides);
        assert!(!ui.snap_view().snap_to_document_grid);
        assert_eq!(ui.snap_view().zone_px, 4.0);
        ui.snap_to_guides = false;
        ui.snap_zone = 0.0;
        assert!(!ui.snap_view().snap_to_guides);
        assert_eq!(ui.snap_view().zone_px, 0.0);
    }

    #[test]
    fn pdf_export_settings_round_trip() {
        let ui = UiState::default();
        let json = serde_json::to_string(&ui).unwrap();
        let parsed: UiState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pdf_export, PdfExportSettings::default());
    }

    #[test]
    fn pdf_export_settings_defaults() {
        let settings = PdfExportSettings::default();
        assert_eq!(settings.preset, "Desktop Printing");
        assert_eq!(settings.standard, "none");
        assert!(!settings.compress_images);
        assert_eq!(settings.flatten, "");
        assert!(!settings.spreads);
        assert!(settings.bleed);
        assert!(!settings.marks_crop);
        assert!(!settings.marks_bleed);
        assert!(!settings.marks_page_info);
        assert_eq!(settings.marks_weight, "0.25");
        assert_eq!(settings.marks_offset, "6");
        assert!(settings.tagged);
    }

    #[test]
    fn pdf_export_settings_survive_missing_key() {
        let json = r#"{ "language": "en" }"#;
        let ui: UiState = serde_json::from_str(json).unwrap();
        assert_eq!(ui.pdf_export, PdfExportSettings::default());
    }

    /// A temporary file named `name` whose bytes aren't a document or a graphic.
    fn unreadable_file(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("designcraft-ui-{}-{name}", std::process::id()));
        std::fs::write(&path, b"neither a document nor a graphic").unwrap();
        path
    }

    /// The open alert dialog: (title, message).
    fn alert(app: &DesignApp) -> (String, String) {
        let d = app.ui.dialog.as_ref().expect("an alert is open");
        assert_eq!(d.id, "alert");
        (d.fields["title"].as_str().unwrap().to_string(), d.fields["message"].as_str().unwrap().to_string())
    }

    #[test]
    fn dropped_documents_open_and_the_rest_is_placed() {
        for name in ["a.designcraft", "B.IDML", "c.indd", "D.INDT"] {
            assert!(opens_as_document(name), "{name}");
        }
        for name in ["a.png", "b.pdf", "idml.txt", "c.idml.zip", "e.indd.zip"] {
            assert!(!opens_as_document(name), "{name}");
        }
    }

    /// Files without an extension: dropped, a document opens and anything else is placed; asked
    /// for by the operating system, they wait for the window and then open as documents.
    #[test]
    fn extensionless_documents_open_by_content() {
        let path = std::env::temp_dir().join(format!("designcraft-ui-{}-Brochure", std::process::id()));
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        let bytes = designcraft_engine::cmd::base64_decode(app.run("file.serialize", json!({})).unwrap()["base64"].as_str().unwrap());
        std::fs::write(&path, bytes).unwrap();
        let p = path.to_string_lossy().to_string();
        app.open_dropped(&p).unwrap();
        assert_eq!(app.session.documents().len(), 2, "a document opens, it isn't placed");
        app.open_documents_later([p.clone()]);
        assert_eq!(app.session.documents().len(), 2, "queued until the window is ready");
        app.open_os_documents();
        assert_eq!(app.session.documents().len(), 3);
        assert_eq!(app.session.active().and_then(|d| d.path.clone()), Some(p));
        let _ = std::fs::remove_file(&path);
        let notes = unreadable_file("notes");
        let e = app.open_dropped(&notes.to_string_lossy()).unwrap_err();
        let _ = std::fs::remove_file(&notes);
        assert_eq!(alert(&app), ("Can't Place the File".to_string(), e));
    }

    #[test]
    fn status_messages_follow_the_interface_language_but_engine_errors_stay_english() {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.status_error("Table", "No table selected");
        assert_eq!(app.ui.status, "Table: No table selected");
        app.ui.language = "cs".into();
        app.status_error("Table", "No table selected");
        assert_eq!(app.ui.status, "Tabulka: No table selected");
    }

    #[test]
    fn dropping_an_unreadable_file_alerts() {
        // A document opens, so it says it can't be opened.
        let path = unreadable_file("broken.designcraft");
        let mut app = DesignApp::new(Session::new(), Services::default());
        let r = app.open_dropped(&path.to_string_lossy());
        let _ = std::fs::remove_file(&path);
        let e = r.unwrap_err();
        let (title, message) = alert(&app);
        assert_eq!(title, "Can't Open the File", "a document opens, it isn't placed");
        assert_eq!(message, e);
        assert!(app.ui.status.contains(&e), "the status line says it too");
        assert!(app.session.documents().is_empty());
        dialogs::confirm(&mut app).unwrap();
        assert!(app.ui.dialog.is_none(), "OK closes the alert");
        // Anything else is placed, so it says it can't be placed.
        app.run("file.new", json!({})).unwrap();
        let path = unreadable_file("notes.xyz");
        let r = app.open_dropped(&path.to_string_lossy());
        let _ = std::fs::remove_file(&path);
        let e = r.unwrap_err();
        assert_eq!(alert(&app), ("Can't Place the File".to_string(), e));
    }

    #[test]
    fn a_dropped_indd_file_the_converter_rejects_alerts() {
        let path = unreadable_file("brochure.indd");
        let mut app = DesignApp::new(Session::new(), Services::default());
        let r = app.open_dropped(&path.to_string_lossy());
        let _ = std::fs::remove_file(&path);
        let e = r.unwrap_err();
        assert!(e.contains("InDesign file"), "{e}");
        assert_eq!(alert(&app), ("Can't Open the File".to_string(), e));
        assert!(app.session.documents().is_empty());
    }

    #[test]
    fn dropping_an_ase_file_loads_its_swatches() {
        let mut source = Session::new();
        source.execute("file.new", &json!({})).unwrap();
        source.execute("swatch.create", &json!({"name": "Dropped Teal", "color": "#108080"})).unwrap();
        let data = source.execute("swatch.save", &json!({"names": ["Dropped Teal"]})).unwrap();
        let bytes = designcraft_engine::cmd::base64_decode(data["base64"].as_str().unwrap());
        for name in ["palette.ase", "PALETTE.ASE"] {
            let path = std::env::temp_dir().join(format!("designcraft-ui-{}-{name}", std::process::id()));
            std::fs::write(&path, &bytes).unwrap();
            let mut app = DesignApp::new(Session::new(), Services::default());
            app.run("file.new", json!({})).unwrap();
            let r = app.open_dropped(&path.to_string_lossy());
            let _ = std::fs::remove_file(&path);
            assert!(app.ui.dialog.is_none(), "{name}: {:?}", app.ui.dialog.as_ref().map(|d| &d.fields));
            assert_eq!(r, Ok(json!({"added": 1, "replaced": 0})), "{name}");
            let doc = &app.session.doc().unwrap().doc;
            assert!(doc.swatches.iter().any(|s| s.name == "Dropped Teal"), "{name}");
            assert!(doc.assets.is_empty(), "{name} isn't placed");
        }
    }

    #[test]
    fn a_file_dropped_while_a_parent_is_open_lands_on_the_parent() {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"pages": 3})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        let _ = app.view_mut();
        let path = std::env::temp_dir().join(format!("designcraft-ui-{}-drop.png", std::process::id()));
        std::fs::write(&path, designcraft_render::Rendered { width: 10, height: 10, pixels: vec![200; 400] }.to_png()).unwrap();
        let r = app.open_dropped(&path.to_string_lossy());
        let _ = std::fs::remove_file(&path);
        r.unwrap();
        let d = &app.session.doc().unwrap().doc;
        assert_eq!(d.parents[0].items.len(), 1, "the graphic is on the parent spread");
        assert!(d.spreads.iter().all(|s| s.items.is_empty()), "no document page gets it");
    }

    #[test]
    fn file_open_failures_show_an_alert() {
        let path = unreadable_file("open.designcraft");
        let picked = path.to_string_lossy().to_string();
        let services = Services { pick_open: Some(Box::new(move |_| Some(picked.clone()))), ..Default::default() };
        let mut app = DesignApp::new(Session::new(), services);
        let e = app.run("app.openDialog", json!({})).unwrap_err();
        assert_eq!(alert(&app), ("Can't Open the File".to_string(), e));
        // Placing it says so too.
        app.ui.dialog = None;
        app.run("file.new", json!({})).unwrap();
        let e = app.run("app.placeDialog", json!({})).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert_eq!(alert(&app), ("Can't Place the File".to_string(), e));
    }

    #[test]
    fn other_command_errors_stay_in_the_status_line() {
        let mut app = DesignApp::new(Session::new(), Services::default());
        assert!(app.run("file.open", json!({"path": "/nonexistent/x.designcraft"})).is_err());
        assert!(app.ui.dialog.is_none(), "a scripted file.open doesn't raise an alert");
        assert!(!app.ui.status.is_empty());
        // The control channel's app.open answers with the error; no alert waits for a click.
        let (req, _rx) = control::ControlRequest::new("app.open", json!({"path": "/nonexistent/x.designcraft"}));
        let control::Outcome::Done(r) = control::handle(&mut app, &egui::Context::default(), &req) else { panic!("app.open answers") };
        assert_eq!(r["ok"], json!(false), "{r}");
        assert!(app.ui.dialog.is_none(), "a control-channel app.open doesn't raise an alert");
    }

    /// Copied objects were pasted on the first spread whatever page was in view.
    #[test]
    fn paste_lands_on_the_spread_in_view() {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"pages": 3, "facingPages": false})).unwrap();
        let id = app.run("frame.create", json!({"rect": [36, 36, 136, 136]})).unwrap()["id"].clone();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        app.run("edit.copy", json!({})).unwrap();
        let pasted_on = |app: &DesignApp| {
            let st = app.session.active().unwrap();
            st.doc.find(st.selection.items[0]).unwrap().spread
        };
        // Nothing is in view before the canvas is laid out: the first spread, as in a script.
        app.run("edit.paste", json!({})).unwrap();
        assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Doc(0));
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
        canvas::go_to_page(&mut app, 2);
        menus::activate(&mut app, "edit.pasteInPlace", &Value::Null);
        assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Doc(2));
        canvas::go_to_page(&mut app, 1);
        app.run("edit.paste", json!({})).unwrap();
        assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Doc(1));
        // A parent spread while parents are edited.
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        app.run("edit.paste", json!({})).unwrap();
        assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Parent(0));
        // With an insertion point Paste still goes into the text, where the object is anchored.
        let story = app.run("frame.create", json!({"rect": [36, 200, 300, 400], "content": "text", "text": "ab"})).unwrap()["story"].clone();
        app.run("text.select", json!({"story": story, "anchor": 2})).unwrap();
        assert_eq!(app.run("edit.paste", json!({})).unwrap()["anchored"], 1);
    }
}

#[cfg(test)]
mod import_warning_tests {
    use super::*;
    use std::io::Write;

    fn idml(missing_parts: bool) -> Vec<u8> {
        let missing = if missing_parts { r#"<idPkg:Spread src="Spreads/missing.xml"/><idPkg:Story src="Stories/missing.xml"/>"# } else { "" };
        let map = format!(
            r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d"><idPkg:Spread src="Spreads/one.xml"/>{missing}</Document>"#
        );
        let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging"><Spread Self="s"><Page Self="p" GeometricBounds="0 0 100 100" ItemTransform="1 0 0 1 0 0"/><Rectangle Self="r" ItemTransform="1 0 0 1 0 0"/></Spread></idPkg:Spread>"#;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, text) in [("mimetype", designcraft_idml::MIMETYPE), ("designmap.xml", &map), ("Spreads/one.xml", spread)] {
            zip.start_file(name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn app() -> DesignApp {
        let mut app = DesignApp::new(Session::new(), Services { inbox: Some(Inbox::default()), ..Default::default() });
        app.run("file.new", json!({"title": "Existing work"})).unwrap();
        app.run("frame.create", json!({"rect": [10, 10, 50, 50]})).unwrap();
        app
    }

    fn assert_partial_import_warning(app: &mut DesignApp, file: &str) {
        let dialog = app.ui.dialog.as_ref().expect("partial import must show an alert");
        assert_eq!(dialog.id, "alert");
        assert_eq!(dialog.fields["title"], "Import Warnings");
        assert_eq!(dialog.fields["file"], file);
        for part in ["Spreads/missing.xml", "Stories/missing.xml"] {
            assert!(dialog.fields["message"].as_str().unwrap().contains(part), "every missing part is named");
            assert!(app.ui.status.contains(part), "the status line retains the warning");
        }
        assert_eq!(app.session.documents().len(), 2);
        assert_eq!(app.session.documents()[0].doc.title, "Existing work");
        assert!(app.session.documents()[0].is_dirty(), "existing unsaved work stays intact");
        assert_eq!(app.session.documents()[0].doc.spreads[0].items.len(), 1);
        assert_eq!(app.session.doc().unwrap().doc.spreads.len(), 1);
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), 1, "available document content is kept");
        dialogs::confirm(app).unwrap();
        assert!(app.ui.dialog.is_none(), "OK dismisses the warning");
        assert_eq!(app.session.documents().len(), 2, "dismissing the warning keeps the imported document");
    }

    #[test]
    fn byte_open_reports_partial_import_warnings_without_losing_documents() {
        let mut app = app();
        let result =
            app.open_file("file.openBytes", json!({"name": "partial.idml", "base64": designcraft_engine::cmd::base64_encode(&idml(true))})).unwrap();
        assert_eq!(result["warnings"].as_array().unwrap().len(), 2);
        assert_partial_import_warning(&mut app, "partial.idml");
    }

    #[test]
    fn native_open_reports_partial_import_warnings_without_losing_documents() {
        let path = std::env::temp_dir().join(format!("designcraft-partial-import-{}", std::process::id())).join("partial.idml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, idml(true)).unwrap();
        let picked = path.to_string_lossy().to_string();
        let mut app = app();
        app.services.pick_open = Some(Box::new(move |_| Some(picked.clone())));
        let result = app.run("app.openDialog", json!({}));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
        assert_eq!(result.unwrap()["warnings"].as_array().unwrap().len(), 2);
        assert_partial_import_warning(&mut app, "partial.idml");
    }

    #[test]
    fn inbox_open_reports_partial_import_warnings_without_losing_documents() {
        let mut app = app();
        let request = app.import_request("open");
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(ImportedFile { request, name: "partial.idml".into(), bytes: idml(true) });
        app.drain_inbox();
        assert_partial_import_warning(&mut app, "partial");
    }

    #[test]
    fn find_font_on_open_outranks_the_warning_alert() {
        let mut app = app();
        app.run("frame.create", json!({"rect": [0, 0, 200, 200], "content": "text", "text": "Sample"})).unwrap();
        let d = std::sync::Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
        let sid = *d.stories.keys().next().unwrap();
        let story = d.story_mut(sid).unwrap();
        story.format_chars(0..6, |f| f.over.font_family = Some("Unavailable Test Family".into()));
        story.paras[0].para.grep_styles = Some(vec![designcraft_doc::GrepStyle { style: "Keep".into(), pattern: "[a-".into() }]);
        let path = std::env::temp_dir().join(format!("designcraft-font-warning-{}.designcraft", std::process::id()));
        app.run("file.save", json!({"path": path})).unwrap();
        let result = app.open_file("file.open", json!({"path": path}));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(result.unwrap()["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findFont");
        assert!(app.ui.status.contains("GREP style not applied"), "{}", app.ui.status);
    }

    #[test]
    fn complete_import_does_not_show_a_warning() {
        let mut app = app();
        let result = app
            .open_file("file.openBytes", json!({"name": "complete.idml", "base64": designcraft_engine::cmd::base64_encode(&idml(false))}))
            .unwrap();
        assert_eq!(result["warnings"], json!([]));
        assert!(app.ui.dialog.is_none());
        assert!(app.ui.status.is_empty());
        assert_eq!(app.session.documents().len(), 2);
    }
}

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
        });
        h.run_steps(10);
    }

    /// The outer rect of a side or top panel, from egui's memory.
    pub fn panel_rect(h: &Harness<'static, Window>, id: &str) -> egui::Rect {
        egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id)).map(|s| s.outer_rect).unwrap()
    }

    /// A primary click at `pos`, then a few frames.
    pub fn click_at(h: &mut Harness<'static, Window>, pos: egui::Pos2) {
        h.hover_at(pos);
        h.drag_at(pos);
        h.drop_at(pos);
        h.run_steps(4);
    }
}

#[cfg(test)]
mod browser_import_tests {
    use super::*;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(Session::new(), Services { inbox: Some(Inbox::default()), ..Default::default() });
        app.run("file.new", json!({"title": "A"})).unwrap();
        app
    }
    fn deliver(app: &mut DesignApp, request: ImportRequest, name: &str, bytes: Vec<u8>) {
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(ImportedFile { request, name: name.into(), bytes });
        app.drain_inbox();
    }
    fn image() -> Vec<u8> {
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##.to_vec()
    }
    fn pdf_pages(count: usize) -> Vec<u8> {
        let kids = (0..count).map(|i| format!("{} 0 R", 3 + i * 2)).collect::<Vec<_>>().join(" ");
        let mut objects = vec!["<< /Type /Catalog /Pages 2 0 R >>".to_string(), format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>")];
        for (i, color) in ["1 0 0", "0 1 0", "0 0 1"].iter().take(count).enumerate() {
            let content = format!("{color} rg 0 0 200 200 re f\n");
            objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents {} 0 R >>", 4 + i * 2));
            objects.push(format!("<< /Length {} >>\nstream\n{content}endstream", content.len()));
        }
        let mut result = "%PDF-1.4\n".to_string();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(result.len());
            result.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
        }
        let xref = result.len();
        result.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1));
        for offset in offsets {
            result.push_str(&format!("{offset:010} 00000 n \n"));
        }
        result.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
        result.into_bytes()
    }
    pub(super) fn pdf() -> Vec<u8> {
        pdf_pages(3)
    }

    #[test]
    fn picker_receives_context_before_returning_and_cancel_does_not_edit() {
        let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
        let copy = captured.clone();
        let mut app = app();
        app.services.open_async = Some(Box::new(move |request| *copy.lock().unwrap() = Some(request)));
        let uid = app.session.doc().unwrap().uid;
        app.pick_and_open("place").unwrap();
        app.run("file.new", json!({"title": "B"})).unwrap();
        let request = captured.lock().unwrap().take().unwrap();
        assert_eq!(request.purpose, "place");
        assert_eq!(request.target, Some(uid));
        assert!(app.session.documents().iter().all(|d| d.doc.assets.is_empty()));
        deliver(&mut app, request, "broken.pdf", b"not a PDF".to_vec());
        assert!(app.session.documents().iter().all(|d| d.doc.assets.is_empty()));
        assert!(app.pending_pdf.is_none());
    }

    #[test]
    fn delayed_place_activates_original_document_and_undo_belongs_to_it() {
        let mut app = app();
        let request = app.import_request("place");
        app.run("file.new", json!({"title": "B"})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.active_index(), Some(0));
        assert_eq!(app.session.documents()[0].doc.assets.len(), 1);
        assert!(app.session.documents()[1].doc.assets.is_empty());
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.assets.len(), 1);
    }

    #[test]
    fn closed_target_is_not_replaced_by_a_reused_tab_index() {
        let mut app = app();
        let request = app.import_request("place");
        app.run("file.close", json!({})).unwrap();
        app.run("file.new", json!({"title": "replacement"})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
    }

    #[test]
    fn closing_earlier_tab_does_not_shift_import_target() {
        let mut app = app();
        app.run("file.new", json!({"title": "target"})).unwrap();
        let request = app.import_request("place");
        app.run("file.new", json!({"title": "other"})).unwrap();
        app.run("file.close", json!({"index": 0})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.doc().unwrap().doc.title, "target");
        assert_eq!(app.session.documents()[0].doc.assets.len(), 1);
        assert!(app.session.documents()[1].doc.assets.is_empty());
    }

    #[test]
    fn layout_place_and_open_preserve_explicit_intent() {
        let mut source = Session::new();
        source.execute("file.new", &json!({"title": "source"})).unwrap();
        source.execute("frame.create", &json!({"rect": [10, 10, 50, 50], "content": "text", "text": "placed"})).unwrap();
        let native = designcraft_engine::cmd::to_bytes(&source.doc().unwrap().doc).unwrap();
        let idml = designcraft_idml::export_idml(&source.doc().unwrap().doc);
        for (name, bytes) in [("source.designcraft", native), ("source.idml", idml)] {
            let mut app = app();
            let request = app.import_request("place");
            deliver(&mut app, request, name, bytes.clone());
            assert_eq!(app.session.documents().len(), 1);
            assert_eq!(app.session.doc().unwrap().doc.title, "A");
            assert!(app.session.doc().unwrap().doc.stories.values().any(|s| s.text == "placed"));
            app.run("edit.undo", json!({})).unwrap();
            assert!(app.session.doc().unwrap().doc.stories.is_empty());
            app.run("edit.redo", json!({})).unwrap();
            assert!(!app.session.doc().unwrap().doc.stories.is_empty());
            let request = app.import_request("open");
            deliver(&mut app, request, name, bytes);
            assert_eq!(app.session.documents().len(), 2);
        }
    }

    #[test]
    fn pdf_options_keep_bytes_target_and_selected_page() {
        let mut app = app();
        let bytes = pdf();
        let request = app.import_request("place");
        deliver(&mut app, request, "three.pdf", bytes.clone());
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "pdfImport");
        app.run("file.new", json!({"title": "B"})).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!("2"));
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active_index(), Some(0));
        let asset = app.session.doc().unwrap().doc.assets.values().next().unwrap();
        assert_eq!(asset.page, 1);
        assert_eq!(*asset.data, bytes);
        assert!(app.pending_pdf.is_none());
        assert!(app.session.documents()[1].doc.assets.is_empty());
        app.run("place.drop", json!({"x": 36, "y": 36})).unwrap();
        let exported = app.run("file.exportPdf", json!({})).unwrap();
        let exported = designcraft_engine::cmd::base64_decode(exported["base64"].as_str().unwrap());
        let pixels = designcraft_render::render_pdf_page(&exported, 0, 400).unwrap();
        assert!(pixels.data().iter().filter(|p| p.g > 240 && p.r < 10 && p.b < 10).count() > 1000);
        assert_eq!(pixels.data().iter().filter(|p| p.r > 240 && p.g < 10 && p.b < 10).count(), 0);
    }

    #[test]
    fn place_loads_the_cursor_unless_a_frame_can_take_the_graphic() {
        let mut app = app();
        let items = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].items.len();
        // Nothing selected: the graphic waits in the place cursor; Escape unloads it.
        let request = app.import_request("place");
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.tool_id(), "placeGun");
        assert!(app.session.loaded.is_some());
        assert_eq!(items(&app), 0);
        let mods = designcraft_tools::Mods::default();
        app.session.tool_key(designcraft_tools::ToolKey::Escape, mods, designcraft_engine::ViewInfo::at_zoom(1.0)).unwrap();
        assert_eq!(app.session.tool_id(), "selection");
        assert!(app.session.loaded.is_none());
        // A click with the loaded cursor places the graphic at its natural size.
        let request = app.import_request("place");
        deliver(&mut app, request, "red.svg", image());
        app.run("place.drop", json!({"x": 100, "y": 100})).unwrap();
        let it = app.session.doc().unwrap().doc.spreads[0].items.last().unwrap();
        assert_eq!(it.bounds(), designcraft_geom::Rect::new(100.0, 100.0, 107.5, 107.5), "10 CSS px = 7.5 pt");
        // An empty graphic frame selected: the graphic fills it.
        let f = app.run("frame.create", json!({"rect": [200, 200, 300, 300], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [f]})).unwrap();
        let request = app.import_request("place");
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.tool_id(), "selection");
        assert!(app.session.loaded.is_none());
        assert_eq!(items(&app), 2);
        let frame = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap();
        assert!(matches!(frame.content, designcraft_doc::Content::Graphic(_)));
    }

    #[test]
    fn pdf_options_reject_oversized_page_without_editing() {
        let mut app = app();
        for page in [0_u64, 4, 4_294_967_296, 4_294_967_297, u64::MAX] {
            let request = app.import_request("place");
            deliver(&mut app, request, "three.pdf", pdf());
            app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!(page.to_string()));
            // The native dialog clamps zero to one; positive invalid pages must fail.
            if page == 0 {
                assert!(app.confirm_pdf_import(page, "crop".into()).is_err());
            } else {
                assert!(dialogs::confirm(&mut app).is_err());
            }
            app.ui.dialog = None;
            assert!(app.session.doc().unwrap().doc.assets.is_empty());
            assert!(app.pending_pdf.is_none());
        }
        let request = app.import_request("place");
        deliver(&mut app, request, "three.pdf", pdf());
        app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!("2"));
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.assets.values().next().unwrap().page, 1);
    }

    #[test]
    fn drop_routing_and_nonlayout_open_remain_compatible() {
        let mut app = app();
        let native = designcraft_engine::cmd::to_bytes(&app.session.doc().unwrap().doc).unwrap();
        let request = app.import_request("drop");
        deliver(&mut app, request, "document.designcraft", native);
        assert_eq!(app.session.documents().len(), 2);
        let request = app.import_request("drop");
        app.run("file.activate", json!({"index": 0})).unwrap();
        deliver(&mut app, request, "red.svg", image());
        assert_eq!(app.session.active_index(), Some(1));
        assert_eq!(app.session.doc().unwrap().doc.assets.len(), 1);
        let request = app.import_request("open");
        deliver(&mut app, request, "single.pdf", pdf_pages(1));
        assert_eq!(app.session.documents().len(), 2);
        assert!(app.pending_pdf.is_none());
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn ase_swatches_keep_purpose_and_target() {
        let mut source = Session::new();
        source.execute("file.new", &json!({})).unwrap();
        source.execute("swatch.create", &json!({"name": "Import Teal", "color": "#108080"})).unwrap();
        let data = source.execute("swatch.save", &json!({"names": ["Import Teal"]})).unwrap();
        let bytes = designcraft_engine::cmd::base64_decode(data["base64"].as_str().unwrap());
        for (purpose, name) in [("swatches", "picker-file"), ("drop", "palette.ase")] {
            let mut app = app();
            let request = app.import_request(purpose);
            app.run("file.new", json!({"title": "B"})).unwrap();
            deliver(&mut app, request, name, bytes.clone());
            assert_eq!(app.session.active_index(), Some(0));
            assert!(app.session.documents()[0].doc.swatches.iter().any(|s| s.name == "Import Teal"));
            assert!(!app.session.documents()[1].doc.swatches.iter().any(|s| s.name == "Import Teal"));
        }
    }

    #[test]
    fn native_pdf_dialog_keeps_path_and_uid_at_confirmation() {
        let mut app = app();
        let path = std::env::temp_dir().join(format!("designcraft-native-import-{}-{}.pdf", std::process::id(), app.session.doc().unwrap().uid));
        std::fs::write(&path, pdf()).unwrap();
        let pick_path = path.to_string_lossy().to_string();
        app.services.pick_open = Some(Box::new(move |_| Some(pick_path.clone())));
        app.services.read = Some(Box::new(|path| std::fs::read(path).map_err(|e| e.to_string())));
        app.pick_and_open("place").unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "pdfImport");
        app.run("file.new", json!({"title": "B"})).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!("2"));
        dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active_index(), Some(0));
        assert_eq!(app.session.doc().unwrap().doc.assets.values().next().unwrap().page, 1);
        assert!(app.session.documents()[1].doc.assets.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pending_pdf_cancel_close_supersession_and_second_delivery_release_bytes() {
        let mut app = app();
        let request = app.import_request("place");
        deliver(&mut app, request.clone(), "first.pdf", pdf());
        deliver(&mut app, request.clone(), "second.svg", image());
        assert_eq!(app.pending_pdf.as_ref().unwrap().name, "first.pdf");
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
        app.ui.dialog = None;
        app.drain_inbox();
        assert!(app.pending_pdf.is_none());
        deliver(&mut app, request.clone(), "first.pdf", pdf());
        app.ui.dialog = Some(dialogs::Dialog::new("newDocument", json!({})));
        app.drain_inbox();
        assert!(app.pending_pdf.is_none());
        app.ui.dialog = None;
        deliver(&mut app, request, "first.pdf", pdf());
        app.run("file.close", json!({})).unwrap();
        app.run("file.new", json!({})).unwrap();
        assert!(dialogs::confirm(&mut app).is_err());
        assert!(app.pending_pdf.is_none());
        assert!(app.session.doc().unwrap().doc.assets.is_empty());
    }
}

#[cfg(test)]
mod load_styles_tests {
    use super::*;

    /// A client's document on disk: Head based on Body, and a Lead character style.
    fn client_bytes() -> Vec<u8> {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body", "chars": {"size": 10}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Head", "basedOn": "Body", "chars": {"size": 20}})).unwrap();
        s.execute("style.character.create", &json!({"name": "Lead", "chars": {"size": 9}})).unwrap();
        designcraft_engine::cmd::base64_decode(s.execute("file.serialize", &json!({})).unwrap()["base64"].as_str().unwrap())
    }

    /// An app with a document that has its own 8 pt Body, and a picker that answers with the client's file.
    fn app_and_file(test: &str) -> (DesignApp, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("designcraft-load-styles-{}-{test}.designcraft", std::process::id()));
        std::fs::write(&path, client_bytes()).unwrap();
        let picked = path.to_string_lossy().to_string();
        let services = Services { pick_open: Some(Box::new(move |purpose| (purpose == "styles").then(|| picked.clone()))), ..Default::default() };
        let mut app = DesignApp::new(Session::new(), services);
        app.run("file.new", json!({})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Body", "chars": {"size": 8}})).unwrap();
        (app, path)
    }

    fn names(app: &DesignApp) -> (Vec<String>, Vec<String>) {
        let st = &app.session.doc().unwrap().doc.styles;
        (st.paragraph.iter().skip(2).map(|s| s.name.clone()).collect(), st.character.iter().skip(1).map(|s| s.name.clone()).collect())
    }

    #[test]
    fn load_all_text_styles_asks_which_styles_of_the_picked_file_to_load() {
        let (mut app, path) = app_and_file("all");
        app.run("app.loadTextStyles", json!({})).unwrap();
        let d = app.ui.dialog.as_mut().expect("the Load Styles dialog");
        let row = |name: &str, conflict: bool| json!({"name": name, "conflict": conflict});
        assert_eq!(d.id, "loadStyles");
        assert_eq!(d.fields["paragraph"], json!([row("[Basic Paragraph]", false), row("Body", true), row("Head", false)]));
        assert_eq!(d.fields["character"], json!([row("Lead", false)]));
        // Lead is unchecked; the rest loads, and Body takes the file's definition.
        d.fields["character"][0]["on"] = json!(false);
        dialogs::confirm(&mut app).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(app.ui.dialog.is_none());
        assert_eq!(names(&app), (vec!["Body".to_string(), "Head".to_string()], vec![]));
        assert_eq!(app.session.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(10.0));
    }

    #[test]
    fn the_dialog_applies_the_chosen_conflict_rule() {
        let (mut app, path) = app_and_file("rename");
        app.run("app.loadParagraphStyles", json!({})).unwrap();
        let d = app.ui.dialog.as_mut().unwrap();
        assert_eq!(d.fields["character"], json!([]), "Load Paragraph Styles offers no character styles");
        assert_eq!(d.fields["conflicts"], "redefine");
        d.fields.insert("conflicts".into(), json!("autoRename"));
        dialogs::confirm(&mut app).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(names(&app), (vec!["Body".to_string(), "Body 2".to_string(), "Head".to_string()], vec![]));
        assert_eq!(app.session.doc().unwrap().doc.styles.para("Body").unwrap().chars.size, Some(8.0));
    }

    #[test]
    fn load_character_styles_offers_only_character_styles() {
        let (mut app, path) = app_and_file("character");
        app.run("app.loadCharacterStyles", json!({})).unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields["paragraph"], json!([]));
        dialogs::confirm(&mut app).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(names(&app), (vec!["Body".to_string()], vec!["Lead".to_string()]));
    }

    /// A script can put anything into the dialog's rows: drawing and ticking them must not panic.
    #[test]
    fn the_dialog_draws_and_ticks_rows_that_are_no_style_rows() {
        use egui_kittest::kittest::Queryable as _;
        let mut h = test_window::open(DesignApp::new(Session::new(), Services::default()), egui::vec2(1200.0, 800.0));
        h.state_mut().app.ui.dialog = Some(dialogs::Dialog::new(
            "loadStyles",
            json!({"path": "x", "conflicts": 3, "paragraph": [1, "Body", null, {"name": 7}, {"name": "Head", "conflict": true}], "character": "none"}),
        ));
        h.run_steps(3);
        let button = h.get_by_label("Deselect All").rect().center();
        test_window::click_at(&mut h, button);
        let d = h.state().app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.fields["paragraph"][4]["on"], false);
        assert_eq!(d.fields["paragraph"][0], 1, "what is no style row stays as it is");
        assert!(dialogs::confirm(&mut h.state_mut().app).is_err(), "there is no file `x`");
    }

    #[test]
    fn no_file_or_no_document_opens_no_dialog() {
        let (mut app, path) = app_and_file("none");
        std::fs::remove_file(&path).unwrap();
        // The picked file can't be read.
        assert!(app.run("app.loadTextStyles", json!({})).is_err());
        assert!(app.ui.dialog.as_ref().is_none_or(|d| d.id != "loadStyles"));
        // The picker is cancelled.
        app.ui.dialog = None;
        app.services.pick_open = Some(Box::new(|_| None));
        app.run("app.loadTextStyles", json!({})).unwrap();
        assert!(app.ui.dialog.is_none());
        // No document: the picker isn't shown at all.
        app.run("file.close", json!({})).unwrap();
        app.services.pick_open = Some(Box::new(|_| panic!("asked for a file without a document")));
        assert!(app.run("app.loadTextStyles", json!({})).is_err());
    }

    #[test]
    fn styles_from_an_asynchronous_picker_load_into_the_document_that_asked() {
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = asked.clone();
        let services = Services {
            inbox: Some(Inbox::default()),
            open_async: Some(Box::new(move |request| seen.lock().unwrap().push(request))),
            ..Default::default()
        };
        let mut app = DesignApp::new(Session::new(), services);
        app.run("file.new", json!({"title": "A"})).unwrap();
        app.run("app.loadCharacterStyles", json!({})).unwrap();
        let request = asked.lock().unwrap().pop().expect("the picker was asked");
        assert_eq!(request.purpose, "styles:character");
        // Another document is in front when the file arrives.
        app.run("file.new", json!({"title": "B"})).unwrap();
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(ImportedFile { request, name: "client.designcraft".into(), bytes: client_bytes() });
        app.drain_inbox();
        assert_eq!(app.session.active_index(), Some(0));
        assert_eq!(names(&app), (vec![], vec!["Lead".to_string()]));
        assert!(app.session.documents()[1].doc.styles.char_style("Lead").is_none());
    }
}
