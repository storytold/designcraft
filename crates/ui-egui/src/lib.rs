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

pub type ReadFn = Box<dyn Fn(&str) -> Result<Vec<u8>, String>>;
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
    /// Save dialog with a suggested name → path. The name may start with the folder the dialog
    /// opens in (see [`DesignApp::pick_save`]).
    pub pick_save: Option<PickFn>,
    pub read: Option<ReadFn>,
    pub write: Option<WriteFn>,
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

/// Closed panels, each with where it goes back, in craft-ui's `Workspace::hidden` shape
/// (`[[panel, location], …]`). A floating panel's entry is where Dock Panel returns it.
/// Files from before this shape hold an object keyed by panel; it still loads.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct HiddenPanels(pub Vec<(String, craft_ui::docking::Location<String>)>);

impl<'de> Deserialize<'de> for HiddenPanels {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Shape {
            List(Vec<(String, craft_ui::docking::Location<String>)>),
            Keyed(std::collections::BTreeMap<String, craft_ui::docking::Location<String>>),
        }
        let entries = match Shape::deserialize(deserializer)? {
            Shape::List(entries) => entries,
            Shape::Keyed(entries) => entries.into_iter().collect(),
        };
        let mut hidden = Self::default();
        for (panel, location) in entries {
            hidden.insert(panel, location);
        }
        Ok(hidden)
    }
}

impl HiddenPanels {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn contains_key(&self, panel: &str) -> bool {
        self.0.iter().any(|(p, _)| p == panel)
    }
    pub fn get(&self, panel: &str) -> Option<&craft_ui::docking::Location<String>> {
        self.0.iter().find(|(p, _)| p == panel).map(|(_, location)| location)
    }
    /// Record where `panel` goes back, replacing an earlier entry.
    pub fn insert(&mut self, panel: String, location: craft_ui::docking::Location<String>) {
        self.remove(&panel);
        self.0.push((panel, location));
    }
    pub fn remove(&mut self, panel: &str) {
        self.0.retain(|(p, _)| p != panel);
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(p, _)| p)
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
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
    #[serde(default, skip_serializing_if = "HiddenPanels::is_empty")]
    pub docking_hidden: HiddenPanels,
    /// The workspace's own iconic column (see [`UiState::icon_groups`]).
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
    /// PDF standard (`none`, `x1a`, `x4`, `a2b`).
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

/// The viewport of New Window's OS window, which shows pane 1.
pub(crate) fn second_window_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("designcraft_second_window")
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
    /// The page in that view and the place on it, for Go Back after leaving Presentation.
    #[serde(skip)]
    pub presentation_page: Option<(designcraft_doc::ids::PageId, designcraft_geom::Vec2)>,
    /// That view's Go Back / Go Forward history; leaving puts it back with one jump added.
    #[serde(skip)]
    pub(crate) presentation_history: Option<page_history::PageHistory>,
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
    /// ICC files loaded with Edit › Color Settings › Load Profile… (loaded again at start, before
    /// `color_settings`, so a loaded working space stays chosen).
    pub color_profiles: Vec<String>,
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
    /// The Preview checkbox of Paragraph/Character Style Options, remembered between openings.
    pub style_preview: bool,
    /// View › Separations Preview: the plate shown and the ink limit; `None` is off.
    #[serde(skip)]
    pub separations: Option<separations::Separations>,
    /// View › Display Performance.
    pub display_quality: designcraft_render::DisplayQuality,
    /// Preferences › Interface › UI scaling (1 = 100%).
    pub ui_scale: f32,
    /// Preferences › Appearance of Black: 100% K on screen as rich black (InDesign's default,
    /// Display All Blacks as Rich Black; off: accurately, as dark grey).
    pub rich_black: bool,
    /// View › Overprint Preview.
    pub overprint_preview: bool,
    /// Edit › Keyboard Shortcuts: the active shortcut set's changes to the default shortcuts, which
    /// the keymap uses. Saved as `shortcuts` and `shortcutsMore` (see `prefs::saved_shortcuts`),
    /// which earlier versions read.
    #[serde(flatten, with = "prefs::saved_shortcuts")]
    pub shortcuts: craft_keymap::Overrides,
    /// The user's named shortcut sets and which one is active (see `shortcuts_dialog`).
    #[serde(deserialize_with = "prefs::lenient_sets")]
    pub shortcut_sets: craft_keymap::Profiles,
    pub control_bar: bool,
    pub tools_double_column: bool,
    /// The front tab of the default layout's Properties · Pages · Layers group. `dock_tab`,
    /// `open_panel`, `floating`, `dock_expanded` and `icon_groups` describe a workspace's
    /// default layout (`panel_docking::default_layout`).
    pub dock_tab: String,
    /// The panel it opens below that group (Printing and Proofing: Swatches).
    pub open_panel: Option<String>,
    /// Its floating panels, and the float-only panels' windows (Type › Tabs): (panel id,
    /// top-left position). A customized layout holds its own floating panels.
    pub floating: Vec<(String, [f32; 2])>,
    /// The panel layout once the user customizes it; `None` is the workspace's default layout
    /// (`panel_docking::default_layout`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docking: Option<craft_ui::docking::Layout<String>>,
    #[serde(default, skip_serializing_if = "HiddenPanels::is_empty")]
    pub docking_hidden: HiddenPanels,
    /// The panel (or the canvas) last clicked or selected in the docking layout, while the panel
    /// stays where the user left it.
    #[serde(skip)]
    pub docking_focused: Option<String>,
    /// Invalidates in-flight gestures when a workspace is replaced.
    #[serde(skip)]
    pub docking_generation: u64,
    /// One-frame reveal request; z-order is never persisted.
    #[serde(skip)]
    pub docking_raise: Option<String>,
    /// The panel last revealed explicitly, which `ui.set closePanel` closes.
    #[serde(skip)]
    pub docking_opened: Option<String>,
    /// The panel whose iconic-column flyout is open, as of the last frame or a command that
    /// opened it.
    #[serde(skip)]
    pub docking_flyout: Option<String>,
    /// Auto-Collapse Iconic Panels: a flyout closes on Escape or a click outside it.
    pub dock_auto_collapse: bool,
    /// Whether the default layout's outer column is expanded (false: collapsed to icons).
    pub dock_expanded: bool,
    /// A workspace's iconic panel groups (Essentials Classic), shown with their names in the
    /// iconic column instead of every panel; `None` is one icon per panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_groups: Option<Vec<Vec<String>>>,
    pub units: Unit,
    pub workspace: String,
    /// Window › Workspace › New Workspace: saved panel arrangements.
    pub custom_workspaces: Vec<SavedWorkspace>,
    /// File › New › Document › Recent: the settings of the last documents made, newest first.
    pub recent_new_documents: Vec<designcraft_doc::build::NewDocument>,
    /// The folder of the last file saved or exported: where the save dialog starts for a
    /// document never saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_save_folder: Option<String>,
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
    /// View › Grids & Guides › Lock Guides: ruler guides can't be dragged.
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
            presentation_page: None,
            presentation_history: None,
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
            color_profiles: Vec::new(),
            proof_colors: false,
            proof_setup: Default::default(),
            dynamic_spelling: false,
            language: String::new(),
            flattener: String::new(),
            pdf_export: PdfExportSettings::default(),
            style_preview: false,
            separations: None,
            display_quality: designcraft_render::DisplayQuality::High,
            ui_scale: 1.0,
            rich_black: true,
            overprint_preview: false,
            shortcuts: Default::default(),
            shortcut_sets: Default::default(),
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
            docking_flyout: None,
            dock_auto_collapse: true,
            docking_focused: None,
            dock_expanded: true,
            icon_groups: None,
            units: Unit::Picas,
            workspace: "Essentials".into(),
            custom_workspaces: Vec::new(),
            recent_new_documents: Vec::new(),
            last_save_folder: None,
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
            guides_locked: self.guides_locked,
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
            separations: separations::Preview::default(),
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
    /// View › Separations Preview: the visible spreads' plates.
    pub separations: separations::Preview,
}

#[derive(Default, Clone, Copy, Debug, Serialize)]
pub struct Perf {
    pub frame_ms: f64,
    pub render_ms: f64,
    pub fps: f64,
}

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
}

impl DesignApp {
    /// Start the background font scan, establish the monotonic timing origin, and initialize document, view, input, screenshot, dialog and control state.
    pub fn new(session: Session, services: Services) -> Self {
        // The font menus and the first file opened need the installed fonts: catalog them now.
        designcraft_fonts::FontDb::global().scan_in_background();
        // The app owns the monotonic origin for recovery timing and frame pacing; `now_ms` itself
        // is process-global so screenshot deadlines stay comparable across hosts.
        let now = now_ms() / 1000.0;
        DesignApp {
            pending_pdf: None,
            session,
            ui: UiState::default(),
            services,
            views: HashMap::new(),
            page_history: HashMap::new(),
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
            native_actions: Default::default(),
            native_events_pending: Default::default(),
            control_input_frame: Default::default(),
            clipboard_guard_installed: false,
            story_editor: None,
            control_rx: None,
            pending_shots: vec![],
            queued_shots: vec![],
            shot_token: 0,
            screenshot_geometry: None,
            screenshot_epoch: 0,
            synthetic_injected: false,
            injected_events: Vec::new(),
            injected_private: false,
            injected_generation: InputGeneration::default(),
            injected_pending: Default::default(),
            injected_settled: Default::default(),
            injected_settle_hook: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending_drops: Vec::new(),
            #[cfg(test)]
            logic_tick_ticks: 0,
            #[cfg(test)]
            logic_frame_ticks: 0,
            #[cfg(test)]
            input_ticks: 0,
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
            keymap: keys::new_keymap(craft_keymap::Overrides::new()),
            key_dispatcher: craft_keymap::egui::Dispatcher::new(),
            shortcut_dialog: None,
            shortcut_answers: Default::default(),
            last_time: now,
            last_recovery: now,
            egui_ctx: None,
            in_control: false,
            object_clip_text: None,
            ime_marked: None,
            ime_discard: false,
            canvas_ime: false,
            ime_tracker: egui::ime_context::ImeContextTracker::new(),
            pending_ime_delete: None,
            ime_target: None,
            ime_interrupt: false,
            ime_canvas: None,
            quit_requested: false,
            close_requested_pending: false,
            quit_without_asking: false,
            quitting: None,
            cursors: cursors::Images::default(),
            live_preview: LivePreview::default(),
            updates_rt: updates_ui::Runtime::default(),
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
    pub(crate) fn page_history(&self) -> Option<&page_history::PageHistory> {
        self.page_history.get(&self.view_key()?)
    }
    pub(crate) fn page_history_mut(&mut self) -> Option<&mut page_history::PageHistory> {
        let k = self.view_key()?;
        Some(self.page_history.entry(k).or_default())
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
            // Tool readouts (sizes, smart-guide distances) measure in the document's ruler units,
            // as its Control and Properties fields do.
            unit: self.session.active().map_or(self.ui.units, |st| st.doc.settings.horizontal_units),
            stroke_in_front: self.ui.stroke_front,
        }
    }

    pub fn status(&mut self, s: impl Into<String>) {
        self.ui.status = s.into();
    }

    /// The host's save dialog for `name`, started beside the active document once it has been
    /// saved, else in the folder of the last file saved or exported (#1653). The answer's folder
    /// is remembered for the next one.
    pub(crate) fn pick_save(&mut self, name: &str) -> Option<String> {
        use std::path::{Path, PathBuf};
        let folder_of = |path: &str| Path::new(path).parent().filter(|f| !f.as_os_str().is_empty()).map(Path::to_path_buf);
        let folder = self
            .session
            .active()
            .and_then(|d| d.path.as_deref())
            .and_then(folder_of)
            .or_else(|| self.ui.last_save_folder.as_ref().map(PathBuf::from));
        // Only the last part of a name goes after the folder: a title may hold a slash, and an
        // empty name would make the folder itself the suggested file.
        let file = Path::new(name).file_name();
        let suggested = match (folder, file) {
            (Some(f), Some(file)) => f.join(file).to_string_lossy().to_string(),
            _ => name.to_string(),
        };
        let picked = self.services.pick_save.as_mut().and_then(|f| f(&suggested))?;
        if let Some(folder) = folder_of(&picked) {
            self.ui.last_save_folder = Some(folder.to_string_lossy().to_string());
        }
        Some(picked)
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

    /// The second window uses a stable viewport id; forget its IME context when it closes.
    pub(crate) fn end_second_ime_viewport(&mut self) {
        self.ime_tracker.end_viewport(egui::ViewportId::from_hash_of("designcraft_second_window"));
    }

    /// "<feature>: <error>" in the status bar: the feature name is translated, the engine's own
    /// error text stays as the engine wrote it.
    pub fn status_error(&mut self, feature: &str, error: impl std::fmt::Display) {
        self.ui.status = format!("{}: {error}", crate::i18n::tr(&self.ui.language, feature));
    }

    /// THE entry point for every action (menus, shortcuts, palette, panels, control channel).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
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
                        self.object_clip_text = Some(designcraft_engine::cmd::copied_objects_text(&self.session));
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
            && matches!(id, "font.replace" | "font.reload" | "edit.undo" | "edit.redo")
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
        let files = std::mem::take(&mut *inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        for file in files {
            if file.request.purpose == "shortcuts" {
                shortcut_import::import_delivered(self, &file.bytes);
                continue;
            }
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

    /// Applies `ui.shortcuts` to the keymap when they differ, e.g. after preferences were loaded,
    /// and rebuilds it when the shortcut platform changed (the web build learns the browser's
    /// operating system on the first frame). Outside the Keyboard Shortcuts dialog the active
    /// shortcut set takes `ui.shortcuts` too: an earlier version may have changed them.
    pub fn sync_keymap(&mut self) {
        if self.shortcut_dialog.is_none() {
            keys::store_in_set(&mut self.ui);
        }
        if self.keymap.platform() != keys::platform() {
            self.keymap = keys::new_keymap(self.ui.shortcuts.clone());
        } else if *self.keymap.overrides() != self.ui.shortcuts {
            self.keymap.set_overrides(self.ui.shortcuts.clone());
        }
    }

    /// Whether the Keyboard Shortcuts dialog is recording keys: a native menu bar pauses its key
    /// equivalents meanwhile, so the keys reach the dialog.
    pub fn shortcut_recording(&self) -> bool {
        self.shortcut_dialog.as_ref().is_some_and(|d| d.recording())
    }

    pub fn select_tool(&mut self, id: &str) {
        let _ = self.run("tool.select", json!({"tool": id}));
        self.ui.flyout = None;
    }

    /// First frame only: load the saved colour profiles and make the saved Color Settings active.
    fn apply_saved_color_settings(&mut self) {
        if self.color_applied {
            return;
        }
        self.color_applied = true;
        for path in self.ui.color_profiles.clone() {
            if let Err(e) = self.session.execute("color.loadProfile", &json!({"path": path})) {
                log::warn!("Color Settings: {e}");
                self.status_error("Color Settings", e);
            }
        }
        if let Some(cs) = self.ui.color_settings.clone() {
            let known = cs.with_known_profiles();
            for (saved, used) in [(&cs.rgb, &known.rgb), (&cs.cmyk, &known.cmyk)] {
                if saved != used {
                    let e = format!("saved profile {saved} is not available; using {used}");
                    log::warn!("Color Settings: {e}");
                    self.status_error("Color Settings", e);
                }
            }
            let _ = designcraft_color::cms::set_active(&known);
        }
    }

    /// Install the UI fonts on the first frame, and again when the language or the system fonts change.
    fn update_fonts(&mut self, ctx: &egui::Context) {
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
    }

    /// Queue and deadline work, run once per app callback.
    ///
    /// `logic_tick` runs on every call (a first pass, a later discarded pass, or a hidden
    /// `run_logic` tick) and consumes queues and deadlines once. Work that is genuinely once per
    /// presented layout frame lives in [`Self::ui_setup`] / [`Self::ui_frame_end`], which only a
    /// real `run_ui` pass calls. A hidden `run_logic` tick also reports pass index zero, so the
    /// pass index alone cannot identify a UI pass.
    pub fn logic(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        {
            self.logic_tick_ticks += 1;
        }
        self.logic_tick(ctx);
    }

    /// Work whose identity is a consumed queue item, revision or deadline.
    fn logic_tick(&mut self, ctx: &egui::Context) {
        self.update_screenshot_epoch(ctx);
        self.drain_control(ctx);
        updates_ui::tick(self, ctx);
        if self.fonts_ready && !self.host_order_pending() {
            self.drain_inbox();
            self.open_os_documents();
        }
        if self.host_order_pending() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        // Ending a stale preview is state cleanup after a control/menu action, not a frame effect:
        // it stays here so a hidden tick still closes a preview whose dialog has gone.
        if !self.host_order_pending() {
            new_document::end_stale_preview(self);
        }
        self.request_hidden_paint_for_pending_input(ctx);
        self.handle_quit(ctx);
        self.flush_window_commands(ctx);
    }

    /// The first pass of a real `run_ui`: fonts, theme, scale and window state, before any widget
    /// is laid out. It must run before the font gate in [`Self::ui`], or the app would wait for
    /// fonts that this function is what installs.
    pub(crate) fn ui_setup(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        {
            self.logic_frame_ticks += 1;
        }
        if self.egui_ctx.is_none() {
            self.egui_ctx = Some(ctx.clone());
        }
        keys::note_platform(ctx);
        self.sync_keymap();
        self.window_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        self.apply_saved_color_settings();
        let scale = self.ui.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        self.update_fonts(ctx);
        if !self.fonts_system {
            // The font scan runs in the background: look again shortly.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.restyle {
            theme::apply(ctx, &theme::Tokens::for_brightness(self.ui.brightness));
            self.restyle = false;
        }
    }

    /// The end of a settled `run_ui` pass: pacing, recovery and screenshot settlement. Recovery
    /// uses the monotonic clock, so a wall-clock/NTP jump cannot postpone or advance it.
    fn ui_frame_end(&mut self, ctx: &egui::Context) {
        let now = now_ms() / 1000.0;
        // Crash recovery: unsaved documents are written to the recovery folder every 30 s.
        #[cfg(not(target_arch = "wasm32"))]
        if self.session.recovery_dir.is_some() && now - self.last_recovery > (self.session.prefs.recovery_minutes * 60.0).max(5.0) {
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
        self.issue_screenshots(ctx);
    }

    /// Keep pending synthetic input and screenshot settlement repainting while the window may be
    /// hidden.
    ///
    /// `visible() == None` means unknown and is treated like hidden. The command holds for one
    /// frame and is sent once per tick until a settled UI pass has taken the injected step or
    /// issued the queued capture.
    fn request_hidden_paint_for_pending_input(&self, ctx: &egui::Context) {
        let pending = self.host_order_pending() || !self.queued_shots.is_empty();
        if !pending || ctx.input(|i| i.viewport().visible()) == Some(true) {
            return;
        }
        // The 0.37 web backend ignores `RequestPaintWhileHidden` until the fork carries it. Do not
        // pretend a hidden-tab capture or gesture will run there; the request times out instead.
        #[cfg(not(target_arch = "wasm32"))]
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
    }

    /// Advance the screenshot geometry epoch whenever size, DPI, zoom or surface change.
    fn update_screenshot_epoch(&mut self, ctx: &egui::Context) {
        let Some(mut geometry) = screenshot_geometry(ctx) else { return };
        geometry.document = self.active_document_epoch();
        if self.screenshot_geometry != Some(geometry) {
            self.screenshot_geometry = Some(geometry);
            self.screenshot_epoch = self.screenshot_epoch.wrapping_add(1);
        }
    }

    /// Quit: close the window the way its close button does, so unsaved documents ask first.
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// Ask about unsaved documents without closing any of them yet. A later Cancel must also
    /// keep documents whose earlier answer was Don't Save, including their history and recovery.
    fn handle_quit(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if self.host_order_pending() {
            if close_requested {
                self.close_requested_pending = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            ctx.request_repaint();
            return;
        }
        let replay_close = std::mem::take(&mut self.close_requested_pending);
        if std::mem::take(&mut self.quit_requested) || replay_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if close_requested || replay_close {
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

    /// Open or place each file dropped on a floating panel's own window. `pointer` is in the main
    /// window's coordinates.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_dropped_files(&mut self, files: &[egui::DroppedFileHandle], pointer: Option<egui::Pos2>) {
        for f in files {
            let p = f.path().to_string_lossy().to_string();
            if !p.is_empty() {
                let _ = self.open_dropped_at(&p, pointer);
            }
        }
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

    /// Inject synthetic events (one pointer event per host iteration), retaining control
    /// provenance until a settled UI pass has actually taken the batch.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !self.injected_settle_hook {
            let pending = self.injected_pending.clone();
            let settled = self.injected_settled.clone();
            ctx.on_end_pass(
                "injected input settlement",
                std::sync::Arc::new(move |ui| {
                    if ui.ctx().viewport_id() == egui::ViewportId::ROOT
                        && pending.load(std::sync::atomic::Ordering::Relaxed)
                        && !ui.ctx().will_discard()
                    {
                        settled.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }),
            );
            self.injected_settle_hook = true;
        }
        // A settled pass that did not go through `DesignApp::ui` (a harness consuming raw input
        // directly) still consumed the batch; acknowledge it before considering the next step.
        if self.injected_settled.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.synthetic_injected = false;
            self.injected_events.clear();
            self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
            self.injected_private = false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pointer = raw.events.iter().rev().find_map(|event| match event {
                egui::Event::PointerMoved(pos) | egui::Event::PointerButton { pos, .. } => Some(*pos),
                _ => None,
            });
            let generation = self.input_generation();
            for file in std::mem::take(&mut raw.dropped_files) {
                let path = file.path().to_string_lossy().to_string();
                if !path.is_empty() {
                    self.pending_drops.push(PendingDrop { path, pointer, generation });
                }
            }
        }
        self.discard_stale_injected_batch(raw);
        // A batch acknowledged by the previous pass keeps its private provenance through that
        // pass's end hook (the clipboard guard), and is cleared at the next host iteration.
        if !self.synthetic_injected {
            self.injected_private = false;
        }
        let private = self.injected_private || !self.control_synthetic.is_empty();
        self.control_input_frame.store(private, std::sync::atomic::Ordering::Relaxed);
        // While the previous batch is still unapplied (it lives in eframe's pending raw input),
        // inject nothing more: one pointer step per UI pass, not per hidden logic tick.
        if self.synthetic_injected {
            return;
        }
        let private = !self.control_synthetic.is_empty();
        let events = if private { &mut self.control_synthetic } else { &mut self.synthetic };
        let Some(first) = events.first() else { return };
        let n = match first {
            egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } => 1,
            _ => events.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(events.len(), |i| i + 1),
        };
        let batch: Vec<egui::Event> = events.drain(..n).collect();
        self.injected_generation = self.input_generation();
        self.injected_private = private;
        self.injected_events = batch.clone();
        self.injected_pending.store(true, std::sync::atomic::Ordering::Relaxed);
        raw.events.extend(batch);
        // Held until a settled UI pass runs with the fonts installed; a hidden logic tick, a
        // discarded multipass first pass or a font-gated early return must not acknowledge it.
        self.synthetic_injected = true;
    }

    /// Notify after enqueuing a native menu event, before waking the host. The returned callback
    /// owns its state so the event handler never borrows the app.
    pub fn native_action_notifier(&self, ctx: egui::Context) -> Box<dyn Fn() + Send + Sync> {
        let pending = self.native_events_pending.clone();
        Box::new(move || {
            pending.store(true, std::sync::atomic::Ordering::Release);
            ctx.request_repaint();
        })
    }

    /// Poll from the first real UI pass, under the host's normal UI/watchdog guard. Resolve menu
    /// choices before refreshing their IDs, then execute them once synthetic input has settled.
    /// Event arrival already blocks logic deliveries, so a deferred action keeps its ordering
    /// without executing native dialogs or menu commands from a hidden logic-only tick.
    pub fn poll_native_actions(&mut self, ctx: &egui::Context, mut next: impl FnMut() -> Option<(String, Value)>) {
        // Clear before draining: a notification arriving during the drain stays set for the
        // next pass, even when its channel send races the final empty receive.
        self.native_events_pending.swap(false, std::sync::atomic::Ordering::AcqRel);
        while let Some((id, params)) = next() {
            self.native_actions.push_back((id, params));
        }
        self.apply_native_actions(ctx);
    }

    /// True while an injected batch, control synthetic event or ordinary synthetic event remains queued.
    fn synthetic_input_pending(&self) -> bool {
        self.synthetic_injected || !self.control_synthetic.is_empty() || !self.synthetic.is_empty()
    }

    /// True when synthetic input, a queued native action or a pending native-event notification remains.
    fn host_order_pending(&self) -> bool {
        self.synthetic_input_pending() || !self.native_actions.is_empty() || self.native_events_pending.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Execute queued native actions only after synthetic input settles, and request another repaint while host work remains.
    fn apply_native_actions(&mut self, ctx: &egui::Context) {
        while !self.synthetic_input_pending() {
            let Some((id, params)) = self.native_actions.pop_front() else { break };
            menus::activate_native(self, ctx, &id, &params);
        }
        if self.host_order_pending() {
            ctx.request_repaint();
        }
    }

    /// The current state an injected batch belongs to.
    fn input_generation(&self) -> InputGeneration {
        InputGeneration {
            document: self.active_document_epoch(),
            dialog: self.ui.dialog.as_ref().map(|dialog| egui::Id::unique(&dialog.id)),
            focus: self.egui_ctx.as_ref().and_then(|ctx| ctx.memory(|memory| memory.focused())),
        }
    }

    /// Drop a batch whose document, dialog or focus changed before a UI pass could take it.
    /// The batch is still in the host's pending raw input; remove exactly the events we appended
    /// (contiguously, in order) so a subsequent forced paint cannot replay stale input.
    fn discard_stale_injected_batch(&mut self, raw: &mut egui::RawInput) {
        if !self.synthetic_injected || self.input_generation() == self.injected_generation {
            return;
        }
        let events = std::mem::take(&mut self.injected_events);
        if !events.is_empty()
            && let Some(start) = raw.events.windows(events.len()).rposition(|window| window == events.as_slice())
        {
            raw.events.drain(start..start + events.len());
        }
        self.synthetic_injected = false;
        self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
        self.injected_private = false;
        self.control_input_frame.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// A font-gated first pass saw the events before any widget did. Put the same batch back at
    /// the front of its queue (with the original provenance) so a later UI pass consumes it.
    fn requeue_injected_batch(&mut self) {
        if !std::mem::take(&mut self.synthetic_injected) {
            return;
        }
        let events = std::mem::take(&mut self.injected_events);
        if !events.is_empty() {
            if self.injected_private {
                self.control_synthetic.splice(0..0, events);
            } else {
                self.synthetic.splice(0..0, events);
            }
        }
        self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
        self.injected_private = false;
        self.control_input_frame.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// The document area: the start screen, or the document tabs over one or two canvas panes.
    pub(crate) fn canvas_area(&mut self, ui: &mut egui::Ui, presenting: bool) {
        let t = theme::Tokens::get(ui.ctx());
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
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        {
            self.input_ticks += 1;
        }
        let ctx = ui.ctx().clone();
        self.canvas_ime = false;
        self.ime_canvas = None;
        self.ime_interrupt = false;
        // Fonts/theme must be installed from the real UI pass, before the font gate below: doing
        // this only from a hidden logic tick would make the first shown pass wait forever.
        if ctx.current_pass_index() == 0 {
            self.ui_setup(&ctx);
        }
        if !self.fonts_ready {
            // The first pass installs fonts and returns before any widget sees this frame's input.
            // Put an injected step back rather than acknowledge it as consumed.
            self.requeue_injected_batch();
            ctx.request_repaint();
            return;
        }
        self.settle_live_preview();
        // Input consumers run once, in the live first UI pass, before layout. Hidden logic ticks
        // must not replay shortcuts or dropped files from stale input.
        if ctx.current_pass_index() == 0 {
            #[cfg(not(target_arch = "wasm32"))]
            {
                // Sample the generation once for the whole batch: placing the first valid file
                // must not suppress later files that belong to the same captured batch.
                let current = self.input_generation();
                for drop in std::mem::take(&mut self.pending_drops) {
                    if drop.generation != current {
                        continue;
                    }
                    let _ = self.open_dropped_at(&drop.path, drop.pointer);
                }
            }
            menus::shortcuts(self, &ctx);
        }
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
        let presenting = self.ui.screen_mode == ScreenMode::Presentation;
        if presenting || self.ui.hidden_panels != 0 {
            panel_docking::cancel_gesture(&ctx);
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
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.pasteboard)).show(ui, |ui| {
            // The panel layout holds the canvas as its protected `canvas` panel.
            if presenting || self.ui.hidden_panels != 0 {
                self.canvas_area(ui, presenting);
            } else {
                panel_docking::show(self, ui);
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
                second_window_viewport(),
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
                        self.end_second_ime_viewport();
                        self.second_window = false;
                        self.focus_pane = 0;
                    }
                },
            );
            self.switch_pane(self.focus_pane);
        } else if self.second_window {
            // The child viewport does not run this pass (Presentation, or no document for its
            // title): its pass counter stops advancing, so treat it as gone and let the tracker
            // start a fresh context when the window is shown again.
            self.end_second_ime_viewport();
        }
        // Tab, Shift+Tab and Presentation hide the floating panels with the docked ones.
        if !presenting && self.ui.hidden_panels == 0 {
            dock::float_only_windows(self, &ctx);
        }
        story_editor::show(self, &ctx);
        dialogs::show(self, &ctx);
        about::show(self, &ctx);
        updates_ui::show(self, &ctx);
        menus::palette(self, &ctx);
        // The IME is observed once, here: after the canvas, the floating panels, the story editor,
        // dialogs and Quick Apply have all applied this pass's commands and focus changes.
        canvas::ime_observe(self, &ctx);
        if self.custom_titlebar {
            chrome::resize_borders(self, ui);
        }
        for url in std::mem::take(&mut self.ui.pending_urls) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if !ctx.will_discard() {
            // A settled UI pass has taken the injected step (a discarded pass has not). Keep
            // `injected_private` until the next raw-input hook, so this pass's end-pass clipboard
            // guard still sees the control provenance.
            self.synthetic_injected = false;
            self.injected_events.clear();
            self.injected_pending.store(false, std::sync::atomic::Ordering::Relaxed);
            self.injected_settled.store(false, std::sync::atomic::Ordering::Relaxed);
            self.ui_frame_end(&ctx);
        }
        self.perf.frame_ms = now_ms() - t0;
    }

    /// Defer control commands while host-order input is pending; otherwise handle replies and screenshot admission.
    fn drain_control(&mut self, ctx: &egui::Context) {
        // An injected batch may still be waiting in the host's pending raw input. Running another
        // control command now could switch documents or dialogs under it, so keep the channel
        // queued until a settled UI pass has taken the batch.
        if self.host_order_pending() {
            return;
        }
        let Some(rx) = self.control_rx.take() else { return };
        while !self.host_order_pending() {
            let Ok(req) = rx.try_recv() else { break };
            let reply = req.reply.clone();
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => {
                    if let Err(e) = self.admit_screenshot(path, reply.clone()) {
                        let _ = reply.send(e);
                    }
                }
            }
        }
        self.control_rx = Some(rx);
    }

    /// Admit one `ui.screenshot` request against the app's count and byte budgets.
    ///
    /// Admission happens before any capture is issued: the checked, current viewport dimensions
    /// reserve their RGBA bytes, and queued plus completed images are counted together.
    fn admit_screenshot(&mut self, path: Option<String>, reply: Sender<ControlResponse>) -> Result<u64, serde_json::Value> {
        let Some(mut geometry) = self.screenshot_geometry else {
            return Err(json!({"ok": false, "error": "the window geometry is not known yet"}));
        };
        // The cached geometry may predate a document switch handled earlier in this same logic
        // tick. Admit for the current document, not for the one the cache last saw.
        geometry.document = self.active_document_epoch();
        if self.pending_shots.len() >= SCREENSHOT_MAX_REQUESTS {
            return Err(json!({"ok": false, "error": format!("at most {SCREENSHOT_MAX_REQUESTS} screenshot requests may be outstanding")}));
        }
        let reserved_bytes = geometry
            .width
            .checked_mul(geometry.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| json!({"ok": false, "error": "the screenshot dimensions overflow the addressable size"}))?;
        if reserved_bytes > SCREENSHOT_MAX_IMAGE_BYTES {
            return Err(json!({
                "ok": false,
                "error": format!(
                    "a screenshot of {}×{} exceeds the {SCREENSHOT_MAX_IMAGE_BYTES}-byte image limit",
                    geometry.width, geometry.height
                )
            }));
        }
        let used = self.pending_shots.iter().try_fold(0usize, |total, shot| total.checked_add(shot.reserved_bytes));
        let over_budget = used.and_then(|used| used.checked_add(reserved_bytes)).is_none_or(|total| total > SCREENSHOT_MAX_AGGREGATE_BYTES);
        if over_budget {
            return Err(json!({"ok": false, "error": format!("the {SCREENSHOT_MAX_AGGREGATE_BYTES}-byte screenshot budget is exhausted")}));
        }
        self.shot_token += 1;
        let token = self.shot_token;
        let now = now_ms();
        self.queued_shots.push((token, now + 120.0, 0));
        self.pending_shots.push(ShotRequest {
            token,
            path,
            reply,
            deadline: now + 8000.0,
            viewport: egui::ViewportId::ROOT,
            epoch: self.screenshot_epoch,
            geometry,
            reserved_bytes,
            slot: None,
        });
        Ok(token)
    }

    /// The current document state (`0` when none is open). Revert preserves the UID but
    /// gives the replacement state a new epoch, invalidating old captures and input.
    fn active_document_epoch(&self) -> u64 {
        self.session.active().map_or(0, |document| document.identity_epoch)
    }

    /// Reject one admitted request: remove its admission and queued token and cancel any live
    /// capture slot before replying with the error.
    fn reject_screenshot(&mut self, index: usize, message: &str) {
        let shot = self.pending_shots.remove(index);
        self.queued_shots.retain(|(token, ..)| *token != shot.token);
        if let Some(slot) = &shot.slot {
            slot.cancel();
        }
        let _ = shot.reply.send(json!({"ok": false, "error": message}));
    }

    /// Settle and issue queued captures. Called only at the end of a settled `run_ui` pass, so
    /// `frames` counts actual layout passes; hidden logic ticks never satisfy the three-frame
    /// settling policy on their own.
    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        let mut due = Vec::new();
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                due.push(*token);
                false
            } else {
                true
            }
        });
        for token in due {
            let Some(index) = self.pending_shots.iter().position(|shot| shot.token == token) else { continue };
            if self.pending_shots[index].geometry.document != self.active_document_epoch() {
                self.reject_screenshot(index, "the active document changed before the screenshot was issued");
                continue;
            }
            let Some(current) = self.screenshot_geometry else {
                let shot = self.pending_shots.remove(index);
                let _ = shot.reply.send(json!({"ok": false, "error": "the window geometry is not known yet"}));
                continue;
            };
            // Re-check the current dimensions before issuing: a request admitted before a resize,
            // DPI or zoom change must renew its checked reservation (or fail, never overrun it).
            if self.pending_shots[index].epoch != self.screenshot_epoch {
                let bytes = current.width.checked_mul(current.height).and_then(|pixels| pixels.checked_mul(4));
                let others = self
                    .pending_shots
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .try_fold(0usize, |total, (_, shot)| total.checked_add(shot.reserved_bytes));
                let reserved = bytes.zip(others).and_then(|(bytes, others)| others.checked_add(bytes).map(|total| (bytes, total)));
                let fits = reserved.is_some_and(|(bytes, total)| bytes <= SCREENSHOT_MAX_IMAGE_BYTES && total <= SCREENSHOT_MAX_AGGREGATE_BYTES);
                let Some((bytes, _)) = reserved else {
                    let shot = self.pending_shots.remove(index);
                    let _ = shot.reply.send(json!({
                        "ok": false,
                        "error": "the window changed and the screenshot no longer fits the admitted budget"
                    }));
                    continue;
                };
                if !fits {
                    let shot = self.pending_shots.remove(index);
                    let _ = shot.reply.send(json!({
                        "ok": false,
                        "error": "the window changed and the screenshot no longer fits the admitted budget"
                    }));
                    continue;
                }
                let shot = &mut self.pending_shots[index];
                shot.epoch = self.screenshot_epoch;
                shot.geometry = current;
                shot.reserved_bytes = bytes;
            }
            let shot = &mut self.pending_shots[index];
            let (slot, callback) = craft_ui::capture::CaptureSlot::new(ctx, shot.viewport, SCREENSHOT_MAX_IMAGE_BYTES);
            shot.slot = Some(slot);
            // The callback holds only a weak slot: it never captures the app, writes a file, runs a
            // command or wakes the context itself (the slot's live request does that).
            ctx.send_viewport_cmd_to(shot.viewport, egui::ViewportCommand::Screenshot(callback));
        }
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        // The screenshot command itself forces one hidden paint; the settling frames before it do
        // not, so ask for them while the window may be hidden. The 0.37 web backend does not yet
        // honor this command (fork follow-up): hidden-tab captures time out there rather than
        // pretending to settle.
        if !self.queued_shots.is_empty() && ctx.input(|i| i.viewport().visible()) != Some(true) {
            #[cfg(not(target_arch = "wasm32"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
    }

    /// Take completed captures out of their slots and write files only on this (UI) thread.
    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let now = now_ms();
        let current_document = self.active_document_epoch();
        let mut i = 0;
        while i < self.pending_shots.len() {
            // Check before taking the callback: a request whose document changed must not write,
            // retry, or wait for its slot, even when no frame has completed yet.
            if self.pending_shots.get(i).is_some_and(|shot| shot.geometry.document != current_document) {
                self.reject_screenshot(i, "the active document changed while the screenshot was pending");
                continue;
            }
            let taken = self.pending_shots.get(i).and_then(|shot| shot.slot.as_ref()).and_then(|slot| slot.take());
            let Some(result) = taken else {
                if now >= self.pending_shots[i].deadline {
                    let shot = self.pending_shots.remove(i);
                    self.queued_shots.retain(|(token, ..)| *token != shot.token);
                    if let Some(slot) = &shot.slot {
                        slot.cancel();
                    }
                    let _ = shot.reply.send(json!({"ok": false, "error": "no frame was presented (screen locked or window hidden); use ui.render"}));
                } else {
                    i += 1;
                }
                continue;
            };
            let (epoch_changed, within_reserved) = {
                let shot = &self.pending_shots[i];
                let within = match &result {
                    Ok(image) => {
                        let [width, height] = image.size;
                        width <= shot.geometry.width && height <= shot.geometry.height
                    }
                    Err(_) => true,
                };
                (shot.epoch != self.screenshot_epoch, within)
            };
            if !epoch_changed && within_reserved {
                let shot = self.pending_shots.remove(i);
                self.queued_shots.retain(|(token, ..)| *token != shot.token);
                let value = match result {
                    Ok(image) => control::save_screenshot(self, image.as_ref(), shot.path.as_deref()),
                    Err(e) => json!({"ok": false, "error": e.to_string()}),
                };
                let _ = shot.reply.send(value);
                continue;
            }
            let (retry, retry_token) = {
                let shot = &self.pending_shots[i];
                (now < shot.deadline, shot.token)
            };
            if let Some(slot) = self.pending_shots[i].slot.take() {
                slot.cancel();
            }
            if !retry {
                let shot = self.pending_shots.remove(i);
                self.queued_shots.retain(|(token, ..)| *token != shot.token);
                let message = match result {
                    Err(e) => e.to_string(),
                    Ok(_) if epoch_changed => "the window changed while the screenshot was captured".to_string(),
                    Ok(_) => "the screenshot is larger than the admitted viewport".to_string(),
                };
                let _ = shot.reply.send(json!({"ok": false, "error": message}));
                continue;
            }
            // Reject the stale pixels and retry within the same deadline with a fresh epoch.
            self.queued_shots.push((retry_token, now + 120.0, 0));
            i += 1;
        }
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

/// The physical capture surface of the root viewport, from its logical rect and DPI.
fn screenshot_geometry(ctx: &egui::Context) -> Option<ScreenshotGeometry> {
    let zoom = ctx.zoom_factor();
    let fallback = ctx.pixels_per_point();
    let (rect, native, fullscreen) = ctx.input(|i| {
        let viewport = i.viewport();
        // Web (eframe's web backend) and egui_kittest set `screen_rect` but not `inner_rect`;
        // both describe the same logical content rect for this viewport.
        (viewport.inner_rect.or(i.raw.screen_rect), viewport.native_pixels_per_point, viewport.fullscreen)
    });
    let rect = rect?;
    let pixels_per_point = match native {
        // `native_pixels_per_point` is before egui's zoom
        // (`Context::pixels_per_point() == zoom_factor * native_pixels_per_point`), so apply the
        // zoom exactly once. The fallback already includes it.
        Some(native) => native * zoom,
        None => fallback,
    };
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return None;
    }
    let width = (rect.width() * pixels_per_point).ceil();
    let height = (rect.height() * pixels_per_point).ceil();
    if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
        return None;
    }
    Some(ScreenshotGeometry {
        width: width as usize,
        height: height as usize,
        document: 0,
        signature: [
            rect.width().to_bits(),
            rect.height().to_bits(),
            pixels_per_point.to_bits(),
            u32::from(fullscreen.unwrap_or(false)),
            zoom.to_bits(),
            // Reserved for a surface-generation counter once a backend exposes one.
            0,
        ],
    })
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

/// Milliseconds from a monotonic process-local origin, on native and web alike.
///
/// Screenshot settlement, deadlines and retries must not use wall time: an NTP/manual clock jump
/// would expire or postpone a capture, and `Context::input(i.time)` is stale inside `run_logic`.
/// `web_time::Instant` is `std::time::Instant` natively and `performance.now()` on wasm.
pub fn now_ms() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<web_time::Instant> = OnceLock::new();
    START.get_or_init(web_time::Instant::now).elapsed().as_secs_f64() * 1000.0
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
        let bar = egui::containers::panel::PanelState::load(&h.ctx, h.ui_id().with("control_bar")).unwrap().outer_rect;
        assert!(bar.height() > 40.0, "{bar:?}");
        // The X/Y/W/H group: its second row holds Y: and H:.
        let group = h.ctx.read_response(egui::Id::unique(("object_control", "dimensions"))).unwrap().rect;
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

    /// Tool readouts measured in picas whatever the document's units (the UI's unit is never set):
    /// they follow the active document's ruler units.
    #[test]
    fn tool_readouts_measure_in_the_documents_units() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        assert_eq!(app.view_info().unit, Unit::Picas, "no document: the UI's default");
        app.run("file.new", json!({})).unwrap();
        app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "millimeters"})).unwrap();
        assert_eq!(app.view_info().unit, Unit::Inches);
    }

    /// Save and Export PDF dialogs started in the home folder (#1653): they start beside a saved
    /// document, and for one never saved in the folder of the last save or export.
    #[test]
    fn save_dialogs_start_beside_the_document_or_in_the_last_folder() {
        use std::{cell::RefCell, rc::Rc};
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let answer: Rc<RefCell<Option<String>>> = Rc::default();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        let (seen, reply) = (asked.clone(), answer.clone());
        app.services.pick_save = Some(Box::new(move |name: &str| {
            seen.borrow_mut().push(name.to_string());
            reply.borrow().clone()
        }));
        app.services.write = Some(Box::new(|_: &str, _: &[u8]| Ok(())));
        let last = |asked: &Rc<RefCell<Vec<String>>>| asked.borrow().last().cloned().unwrap_or_default();
        let export_pdf = |app: &mut DesignApp| {
            app.ui.dialog = Some(dialogs::Dialog::new("pdfExport", json!({})));
            app.ui.dialog.as_mut().unwrap().fields.insert("title".into(), json!("Report"));
            dialogs::confirm(app).unwrap();
        };

        app.run("file.new", json!({})).unwrap();
        let untitled = format!("{}.designcraft", app.session.active().unwrap().doc.title);
        app.run("app.saveDialog", json!({})).unwrap();
        assert_eq!(last(&asked), untitled, "never saved, nothing remembered: the system's own folder");

        let saved = home.path().join("Brochure.designcraft");
        *answer.borrow_mut() = Some(saved.to_string_lossy().to_string());
        app.run("app.saveDialog", json!({})).unwrap();
        assert_eq!(app.session.active().unwrap().path.as_deref(), Some(&*saved.to_string_lossy()));
        // An export into another folder becomes the last folder used.
        *answer.borrow_mut() = Some(elsewhere.path().join("Proof.pdf").to_string_lossy().to_string());
        export_pdf(&mut app);
        *answer.borrow_mut() = None;
        export_pdf(&mut app);
        assert_eq!(last(&asked), home.path().join("Report.pdf").to_string_lossy(), "a saved document exports beside itself");

        app.run("file.new", json!({})).unwrap();
        let untitled = format!("{}.designcraft", app.session.active().unwrap().doc.title);
        app.run("app.saveDialog", json!({})).unwrap();
        assert_eq!(last(&asked), elsewhere.path().join(untitled).to_string_lossy(), "a new document starts in the last folder used");
    }

    /// No name (Save with no document) suggested the remembered folder itself as the file, and a
    /// title with a slash moved the suggestion out of the document's folder.
    #[test]
    fn save_dialog_suggestions_keep_the_folder_and_only_a_file_name() {
        use std::{cell::RefCell, rc::Rc};
        let folder = tempfile::tempdir().unwrap();
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
        let seen = asked.clone();
        app.services.pick_save = Some(Box::new(move |name: &str| {
            seen.borrow_mut().push(name.to_string());
            None
        }));
        app.ui.last_save_folder = Some(folder.path().join("Docs").to_string_lossy().to_string());
        app.pick_save("");
        app.pick_save("Q1/Q2.pdf");
        assert_eq!(*asked.borrow(), ["".to_string(), folder.path().join("Docs").join("Q2.pdf").to_string_lossy().to_string()]);
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
        assert_eq!(app.ui.status, format!("{}: No table selected", crate::i18n::tr("cs", "Table")));
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

    // ---------------------------------------------------------------------------------------
    // egui 0.37 lifecycle, hidden input and screenshot completion regressions.
    // ---------------------------------------------------------------------------------------

    /// One frame of the real callback order the native and web hosts use.
    fn shot_frame(app: &mut DesignApp, ctx: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
        shot_frame_at(app, ctx, egui::Rect::from_min_size(egui::Pos2::ZERO, size), true)
    }

    /// One frame at a chosen window rect. `inner_rect = false` is the web/egui_kittest shape:
    /// `screen_rect` is set but `ViewportInfo::inner_rect` is not.
    fn shot_frame_at(app: &mut DesignApp, ctx: &egui::Context, rect: egui::Rect, inner_rect: bool) -> egui::FullOutput {
        let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                inner_rect: inner_rect.then_some(rect),
                native_pixels_per_point: Some(1.0),
                minimized: Some(false),
                occluded: Some(false),
                ..Default::default()
            },
        );
        let mut output = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        });
        output.textures_delta.clear();
        output
    }

    /// A hidden host iteration: the raw input is prepared and then only the logic callback runs.
    fn hidden_tick(app: &mut DesignApp, ctx: &egui::Context, raw: &mut egui::RawInput) -> egui::LogicOutput {
        app.raw_input_hook(ctx, raw);
        ctx.run_logic(raw, |ctx| app.logic(ctx))
    }

    /// Build a 640x480 root viewport marked minimized and occluded.
    fn hidden_raw() -> egui::RawInput {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                inner_rect: Some(rect),
                native_pixels_per_point: Some(1.0),
                minimized: Some(true),
                occluded: Some(true),
                ..Default::default()
            },
        );
        raw
    }

    /// Count root `RequestPaintWhileHidden` commands.
    fn hidden_paints(output: &egui::LogicOutput) -> usize {
        output
            .viewport_commands
            .get(&egui::ViewportId::ROOT)
            .map_or(0, |commands| commands.iter().filter(|c| matches!(c, egui::ViewportCommand::RequestPaintWhileHidden)).count())
    }

    /// Remove and return the root screenshot callback, replacing its command with `Focus`.
    fn screenshot_callback(output: &mut egui::FullOutput) -> Option<egui::ScreenshotCallback> {
        let viewport = output.viewport_output.get_mut(&egui::ViewportId::ROOT)?;
        for command in &mut viewport.commands {
            if matches!(command, egui::ViewportCommand::Screenshot(_)) {
                let egui::ViewportCommand::Screenshot(callback) = std::mem::replace(command, egui::ViewportCommand::Focus) else {
                    return None;
                };
                return Some(callback);
            }
        }
        None
    }

    /// Create a recording-write app with a new document and four settling frames.
    fn screenshot_app() -> (DesignApp, egui::Context, std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>>) {
        let writes: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>> = Default::default();
        let sink = writes.clone();
        let mut app = DesignApp::new(
            Session::new(),
            Services {
                write: Some(Box::new(move |path, bytes| {
                    sink.lock().unwrap().push((path.to_string(), bytes.len()));
                    Ok(())
                })),
                ..Default::default()
            },
        );
        app.run("file.new", json!({"title": "shots"})).unwrap();
        let ctx = egui::Context::default();
        for _ in 0..4 {
            let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
            output.textures_delta.clear();
        }
        (app, ctx, writes)
    }

    /// The callback only fills a slot; the next logic pass writes and replies once and releases admission.
    #[test]
    fn screenshot_callback_stays_weak_and_writes_only_from_logic() {
        let (mut app, ctx, writes) = screenshot_app();
        let (reply, replies) = std::sync::mpsc::channel();
        let path = std::env::temp_dir().join(format!("designcraft-shot-{}.png", std::process::id()));
        app.admit_screenshot(Some(path.to_string_lossy().to_string()), reply).unwrap();
        app.queued_shots[0].1 = 0.0;
        let mut callback = None;
        for _ in 0..3 {
            let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
            if let Some(cb) = screenshot_callback(&mut output) {
                callback = Some(cb);
            }
        }
        let callback = callback.expect("the settling frames issue one capture");
        assert!(writes.lock().unwrap().is_empty());
        // The callback only fills its slot; writing happens in the next logic pass.
        callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        assert!(writes.lock().unwrap().is_empty(), "the callback must not write files");
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        let written = writes.lock().unwrap().clone();
        assert_eq!(written.len(), 1, "one file after the logic drain");
        assert_eq!(written[0].0, path.to_string_lossy());
        let reply = replies.recv().unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["width"], 640);
        assert_eq!(reply["result"]["height"], 480);
        assert!(app.pending_shots.is_empty(), "the reservation is released");
    }

    /// Admission enforces the request count, aggregate RGBA byte budget and per-image limit before capture.
    #[test]
    fn screenshot_admission_bounds_count_aggregate_and_image_bytes() {
        let (mut app, ctx, _writes) = screenshot_app();
        let admit = |app: &mut DesignApp, path: &str| {
            let (reply, _replies) = std::sync::mpsc::channel();
            app.admit_screenshot(Some(path.into()), reply)
        };
        for i in 0..SCREENSHOT_MAX_REQUESTS {
            assert!(admit(&mut app, &format!("shot-{i}.png")).is_ok(), "request {i}");
        }
        let error = admit(&mut app, "too-many.png").unwrap_err();
        assert_eq!(error["ok"], false, "{error}");
        // Drain the admitted requests without replying so the byte-budget check is exercised.
        app.pending_shots.clear();
        app.queued_shots.clear();
        for _ in 0..2 {
            shot_frame(&mut app, &ctx, egui::vec2(5120.0, 5120.0));
        }
        assert!(admit(&mut app, "hundred-a.png").is_ok());
        assert!(admit(&mut app, "hundred-b.png").is_ok());
        let error = admit(&mut app, "over-aggregate.png").unwrap_err();
        assert!(error["error"].as_str().unwrap_or_default().contains("budget"), "{error}");
        app.pending_shots.clear();
        app.queued_shots.clear();
        for _ in 0..2 {
            shot_frame(&mut app, &ctx, egui::vec2(6000.0, 6000.0));
        }
        let error = admit(&mut app, "too-large.png").unwrap_err();
        assert!(error["error"].as_str().unwrap_or_default().contains("image limit"), "{error}");
    }

    /// A resize invalidates in-flight pixels and retries within the deadline; a timeout errors and late callbacks are inert.
    #[test]
    fn screenshot_epoch_change_and_timeout_drop_stale_pixels() {
        let (mut app, ctx, writes) = screenshot_app();
        let (reply, replies) = std::sync::mpsc::channel();
        app.admit_screenshot(Some("stale.png".into()), reply).unwrap();
        app.queued_shots[0].1 = 0.0;
        let mut callback = None;
        for _ in 0..3 {
            let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
            if let Some(cb) = screenshot_callback(&mut output) {
                callback = Some(cb);
            }
        }
        let callback = callback.expect("issued");
        // Resize away and back: the epoch advances even though the size ends up the same.
        shot_frame(&mut app, &ctx, egui::vec2(800.0, 600.0));
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        callback.clone().complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        assert!(replies.try_recv().is_err(), "stale pixels must not be replied");
        assert!(!app.queued_shots.is_empty(), "the request is retried inside its deadline");
        assert!(writes.lock().unwrap().is_empty());
        // Let the deadline pass: the retry reports an error and a later callback is inert.
        app.pending_shots[0].deadline = 0.0;
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        let reply = replies.try_recv().unwrap();
        assert_eq!(reply["ok"], false, "{reply}");
        callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        assert!(replies.try_recv().is_err(), "no second reply from a late callback");
        assert!(writes.lock().unwrap().is_empty());
        assert!(app.pending_shots.is_empty());
    }

    /// A host without `inner_rect` uses `screen_rect` and can still admit and complete a capture.
    #[test]
    fn screenshot_geometry_uses_screen_rect_when_inner_rect_is_absent() {
        let (mut app, ctx, writes) = screenshot_app();
        app.screenshot_geometry = None;
        app.screenshot_epoch = 0;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        // Web eframe and egui_kittest set screen_rect but not ViewportInfo::inner_rect.
        for _ in 0..2 {
            shot_frame_at(&mut app, &ctx, rect, false);
        }
        let geometry = app.screenshot_geometry.expect("the screen rect is the logical viewport");
        assert_eq!((geometry.width, geometry.height), (640, 480), "the fallback is the same logical geometry");
        let (reply, replies) = std::sync::mpsc::channel();
        app.admit_screenshot(Some("web-like.png".into()), reply).expect("an inner-rect-less host admits screenshots");
        app.queued_shots[0].1 = 0.0;
        let mut callback = None;
        for _ in 0..3 {
            if let Some(cb) = screenshot_callback(&mut shot_frame_at(&mut app, &ctx, rect, false)) {
                callback = Some(cb);
            }
        }
        callback.expect("the capture is issued").complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        shot_frame_at(&mut app, &ctx, rect, false);
        let reply = replies.recv().unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(writes.lock().unwrap().len(), 1);
    }

    /// Apply native pixels-per-point and UI zoom exactly once.
    #[test]
    fn screenshot_geometry_applies_native_dpi_and_zoom_once() {
        let (mut app, ctx, _writes) = screenshot_app();
        app.ui.ui_scale = 1.5;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 50.0));
        let raw = {
            let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
            raw.viewports.insert(
                egui::ViewportId::ROOT,
                egui::ViewportInfo { inner_rect: Some(rect), native_pixels_per_point: Some(2.0), minimized: Some(false), ..Default::default() },
            );
            raw
        };
        // The first pass applies the zoom; the next logic tick measures the new geometry.
        for _ in 0..2 {
            let mut raw = raw.clone();
            raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().inner_rect = Some(rect);
            ctx.run_ui(raw, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            })
            .textures_delta
            .clear();
        }
        let geometry = app.screenshot_geometry.expect("geometry");
        // 100 x 50 points at 2 physical pixels per point and 1.5 UI zoom: apply each exactly once.
        assert_eq!((geometry.width, geometry.height), (300, 150));
    }

    /// Moving the window leaves the capture epoch unchanged; resizing and resizing back advance it.
    #[test]
    fn screenshot_epoch_ignores_window_position_but_tracks_size_changes() {
        let (mut app, ctx, _writes) = screenshot_app();
        let before = app.screenshot_epoch;
        let moved = egui::Rect::from_min_size(egui::pos2(120.0, 80.0), egui::vec2(640.0, 480.0));
        shot_frame_at(&mut app, &ctx, moved, true);
        assert_eq!(app.screenshot_epoch, before, "moving the window is not a capture-surface change");
        shot_frame_at(&mut app, &ctx, egui::Rect::from_min_size(egui::pos2(120.0, 80.0), egui::vec2(800.0, 600.0)), true);
        let resized = app.screenshot_epoch;
        assert_ne!(resized, before, "a size change advances the epoch");
        shot_frame_at(&mut app, &ctx, egui::Rect::from_min_size(egui::pos2(120.0, 80.0), egui::vec2(640.0, 480.0)), true);
        assert_ne!(app.screenshot_epoch, resized, "and resizing back advances it again");
    }

    /// `ui.screenshot` is admitted, waits for capture, then writes once and replies with the dimensions.
    #[test]
    fn control_screenshot_replies_with_the_saved_capture() {
        let (mut app, ctx, writes) = screenshot_app();
        let (requests, receiver) = std::sync::mpsc::channel();
        app = app.with_control(receiver);
        let (reply, replies) = std::sync::mpsc::channel();
        requests.send(ControlRequest { method: "ui.screenshot".into(), params: json!({"path": "control.png", "focus": false}), reply }).unwrap();
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        assert_eq!(app.pending_shots.len(), 1, "ui.screenshot is admitted, not issued from the queue");
        assert!(replies.try_recv().is_err(), "the reply waits for the capture");
        app.queued_shots[0].1 = 0.0;
        let mut callback = None;
        for _ in 0..3 {
            if let Some(cb) = screenshot_callback(&mut shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0))) {
                callback = Some(cb);
            }
        }
        let callback = callback.expect("the settled frames issue one capture");
        callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        let reply = replies.recv().unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["width"], 640);
        assert_eq!(reply["result"]["height"], 480);
        let written = writes.lock().unwrap().clone();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, "control.png");
        assert!(written[0].1 > 0);
        assert!(app.pending_shots.is_empty(), "the reservation is released");
        assert!(app.queued_shots.is_empty());
    }

    /// A timed-out capture replies with a no-frame error and writes no file.
    #[test]
    fn control_screenshot_times_out_without_writing_a_file() {
        let (mut app, ctx, writes) = screenshot_app();
        let (requests, receiver) = std::sync::mpsc::channel();
        app = app.with_control(receiver);
        let (reply, replies) = std::sync::mpsc::channel();
        requests.send(ControlRequest { method: "ui.screenshot".into(), params: json!({"path": "control.png", "focus": false}), reply }).unwrap();
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        assert_eq!(app.pending_shots.len(), 1);
        app.pending_shots[0].deadline = 0.0;
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        let reply = replies.recv().unwrap();
        assert_eq!(reply["ok"], false, "{reply}");
        assert!(reply["error"].as_str().unwrap_or_default().contains("no frame"), "{reply}");
        assert!(writes.lock().unwrap().is_empty(), "a timed-out request writes no file");
        assert!(app.pending_shots.is_empty());
    }

    /// A multipass frame ticks for every pass but runs once-per-frame work once.
    #[test]
    fn discarded_pass_runs_the_frame_stage_once_but_ticks_every_pass() {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"title": "multipass"})).unwrap();
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
        raw.viewports
            .insert(egui::ViewportId::ROOT, egui::ViewportInfo { inner_rect: Some(rect), native_pixels_per_point: Some(1.0), ..Default::default() });
        let mut passes = 0;
        let mut output = ctx.run_ui(raw, |ui| {
            passes += 1;
            if passes == 1 {
                // Requested before `app.ui` so the first pass is a discarded one and must not
                // acknowledge injected input.
                ui.ctx().request_discard("forced multipass regression");
            }
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        });
        assert_eq!(output.platform_output.num_completed_passes, 2, "the frame really was multipass");
        // 0.37's TexturesDelta panics if dropped unapplied; this test only inspects the passes.
        output.textures_delta.clear();
        assert_eq!(app.logic_tick_ticks, 2, "every pass ticks queues and deadlines");
        assert_eq!(app.logic_frame_ticks, 1, "once-per-frame work runs once");
        assert_eq!(app.input_ticks, 2);
    }

    /// Hidden logic requests one paint per tick, injects one queued step at a time, treats unknown visibility as hidden, and lets one visible pass consume the step.
    #[test]
    fn hidden_logic_requests_exactly_one_paint_per_tick_until_a_settled_ui_pass() {
        let (mut app, ctx, _writes) = screenshot_app();
        let mut raw = hidden_raw();
        // Three pointer steps queued: one batch may be in the host's pending raw input at a time.
        app.control_synthetic = vec![
            egui::Event::PointerMoved(egui::pos2(10.0, 10.0)),
            egui::Event::PointerMoved(egui::pos2(20.0, 20.0)),
            egui::Event::PointerMoved(egui::pos2(30.0, 30.0)),
        ];
        app.raw_input_hook(&ctx, &mut raw);
        let injected = raw.events.clone();
        assert_eq!(app.injected_events.len(), 1, "one pointer event in the batch");
        assert_eq!(app.control_synthetic.len(), 2, "the rest wait for later host iterations");
        for tick in 0..3 {
            let output = hidden_tick(&mut app, &ctx, &mut raw);
            assert_eq!(hidden_paints(&output), 1, "tick {tick}: exactly one hidden paint request");
            assert!(app.synthetic_injected, "the injected step is still unapplied");
            assert!(app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "control provenance is retained while hidden");
            assert_eq!(raw.events, injected, "tick {tick}: no second batch accumulated into the pending raw input");
            assert_eq!(app.control_synthetic.len(), 2, "tick {tick}: the queue did not drain another step");
        }
        // A real visible UI pass takes exactly the one pending event (no duplicated PointerMoved).
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().occluded = Some(false);
        let mut seen = Vec::new();
        ctx.run_ui(raw, |ui| {
            seen = ui.ctx().input(|i| i.events.clone());
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert_eq!(seen.iter().filter(|event| matches!(event, egui::Event::PointerMoved(_))).count(), 1);
        assert!(!app.synthetic_injected, "a settled UI pass acknowledges delivery");
        assert!(app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "the pass's end guard still sees the provenance");
        app.control_synthetic.clear();
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert!(!app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "the next host iteration clears it");
        // Unknown visibility is treated like hidden, and the next queued step is injected.
        let mut unknown = egui::RawInput::default();
        unknown.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo::default());
        app.control_synthetic.push(egui::Event::Text("x".into()));
        app.raw_input_hook(&ctx, &mut unknown);
        let output = ctx.run_logic(&unknown, |ctx| app.logic(ctx));
        assert_eq!(hidden_paints(&output), 1, "visible() == None is treated like hidden");
    }

    /// A font-gated first pass requeues injected input instead of acknowledging it; a later settled pass consumes it.
    #[test]
    fn font_gated_first_pass_requeues_injected_input_instead_of_acknowledging_it() {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"title": "fonts"})).unwrap();
        let ctx = egui::Context::default();
        app.control_synthetic.push(egui::Event::Text("x".into()));
        let mut raw = hidden_raw();
        app.raw_input_hook(&ctx, &mut raw);
        assert!(app.synthetic_injected);
        // The first real pass installs fonts and returns before any widget consumes input.
        ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert!(!app.synthetic_injected, "a font-gated pass is not an acknowledgement");
        assert_eq!(app.control_synthetic, vec![egui::Event::Text("x".into())], "the batch is back in its queue");
        assert!(!app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed));
        // Once fonts are ready the same step is injected and a settled pass consumes it.
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        assert!(app.fonts_ready);
        let mut raw = hidden_raw();
        app.raw_input_hook(&ctx, &mut raw);
        assert!(app.synthetic_injected);
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
        ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert!(!app.synthetic_injected);
        assert!(app.control_synthetic.is_empty());
    }

    /// Injected input is acknowledged only by the settled multipass frame.
    #[test]
    fn discarded_pass_does_not_acknowledge_injected_input() {
        let (mut app, ctx, _writes) = screenshot_app();
        app.control_synthetic.push(egui::Event::Text("d".into()));
        let mut raw = hidden_raw();
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
        app.raw_input_hook(&ctx, &mut raw);
        assert!(app.synthetic_injected);
        let mut passes = 0;
        let output = ctx.run_ui(raw, |ui| {
            passes += 1;
            if passes == 1 {
                ui.ctx().request_discard("injected input must survive a discarded pass");
            }
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        assert_eq!(output.platform_output.num_completed_passes, 2);
        assert!(!app.synthetic_injected, "the settled second pass acknowledges it");
        assert!(app.control_synthetic.is_empty());
    }

    /// A document change drops the stale batch and removes its events from pending raw input.
    #[test]
    fn a_document_switch_before_the_ui_pass_drops_the_pending_batch() {
        let (mut app, ctx, _writes) = screenshot_app();
        let pos = egui::pos2(10.0, 10.0);
        app.control_synthetic.push(egui::Event::PointerMoved(pos));
        let mut raw = hidden_raw();
        app.raw_input_hook(&ctx, &mut raw);
        assert!(app.synthetic_injected);
        let _ = hidden_tick(&mut app, &ctx, &mut raw);
        // A menu/control action switched documents before the forced paint.
        app.run("file.new", json!({"title": "handoff"})).unwrap();
        app.raw_input_hook(&ctx, &mut raw);
        assert!(!app.synthetic_injected, "the stale batch was discarded");
        assert!(app.injected_events.is_empty());
        assert!(
            !raw.events.iter().any(|event| matches!(event, egui::Event::PointerMoved(p) if *p == pos)),
            "the event was removed from pending input"
        );
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
        let mut seen = Vec::new();
        ctx.run_ui(raw, |ui| {
            seen = ui.ctx().input(|i| i.events.clone());
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert!(!seen.iter().any(|event| matches!(event, egui::Event::PointerMoved(p) if *p == pos)), "the new document did not receive it");
    }

    /// Save a document, make an unsaved edit and return the temporary directory.
    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_revert(app: &mut DesignApp) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        app.run("file.saveAs", json!({"path": dir.path().join("revert.designcraft")})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 144, 144], "content": "text", "text": "unsaved"})).unwrap();
        assert!(app.session.active().unwrap().is_dirty());
        dir
    }

    /// Revert keeps the document uid, advances the identity epoch and clears dirty state.
    #[cfg(not(target_arch = "wasm32"))]
    fn revert_preserving_uid(app: &mut DesignApp) {
        let before = app.session.active().unwrap();
        let (uid, epoch) = (before.uid, before.identity_epoch);
        app.run("file.revert", json!({})).unwrap();
        let after = app.session.active().unwrap();
        assert_eq!(after.uid, uid, "Revert keeps the open document's stable identity");
        assert_ne!(after.identity_epoch, epoch, "Revert replaces the document state");
        assert!(!after.is_dirty());
    }

    /// Input captured before Revert is neither acknowledged nor delivered to the restored document.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_revert_discards_a_buffered_input_batch() {
        let (mut app, ctx, _writes) = screenshot_app();
        let _dir = prepare_revert(&mut app);
        let pos = egui::pos2(10.0, 10.0);
        app.control_synthetic.push(egui::Event::PointerMoved(pos));
        let mut raw = hidden_raw();
        app.raw_input_hook(&ctx, &mut raw);
        assert!(app.synthetic_injected);
        revert_preserving_uid(&mut app);
        app.raw_input_hook(&ctx, &mut raw);
        assert!(!app.synthetic_injected, "input captured before Revert must not reach the restored document");
        assert!(app.injected_events.is_empty());
        assert!(!raw.events.iter().any(|event| matches!(event, egui::Event::PointerMoved(p) if *p == pos)));
    }

    /// Return `hidden_raw` with the root viewport marked visible.
    #[cfg(not(target_arch = "wasm32"))]
    fn visible_raw() -> egui::RawInput {
        let mut raw = hidden_raw();
        let viewport = raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        viewport.minimized = Some(false);
        viewport.occluded = Some(false);
        raw
    }

    /// Build Cmd-key press and release events.
    #[cfg(not(target_arch = "wasm32"))]
    fn command_key(key: egui::Key) -> Vec<egui::Event> {
        let modifiers = egui::Modifiers { command: true, mac_cmd: true, ..egui::Modifiers::NONE };
        [true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).to_vec()
    }

    /// A different document with one selectable item, for an asynchronous open.
    #[cfg(not(target_arch = "wasm32"))]
    fn document_with_a_frame() -> Vec<u8> {
        let mut session = Session::new();
        session.execute("file.new", &json!({"title": "Incoming"})).unwrap();
        session.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap();
        let serialized = session.execute("file.serialize", &json!({})).unwrap();
        designcraft_engine::cmd::base64_decode(serialized["base64"].as_str().unwrap())
    }

    /// One host iteration in eframe's order: `raw_input_hook`, then `run_ui` calling
    /// `App::logic` and `App::ui` (eframe native `update`).
    #[cfg(not(target_arch = "wasm32"))]
    fn run_host_frame(app: &mut DesignApp, ctx: &egui::Context, mut raw: egui::RawInput) {
        app.raw_input_hook(ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            if ctx.current_pass_index() == 0 {
                app.poll_native_actions(&ctx, || None);
            }
            app.ui(ui);
        })
        .textures_delta
        .clear();
    }

    /// A native Revert runs only after the injected press/release settles; pointer state and user clipboard provenance are preserved.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_native_revert_waits_for_the_control_gesture() {
        let (mut app, ctx, _writes) = screenshot_app();
        let _dir = prepare_revert(&mut app);
        let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
        for pressed in [true, false] {
            app.control_synthetic.push(egui::Event::PointerButton {
                pos: egui::pos2(320.0, 240.0),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        for step in 0..3 {
            let mut raw = visible_raw();
            app.raw_input_hook(&ctx, &mut raw);
            ctx.run_ui(raw, |ui| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                if ctx.current_pass_index() == 0 {
                    app.poll_native_actions(&ctx, || actions.pop_front());
                }
                app.ui(ui);
            })
            .textures_delta
            .clear();
            assert_eq!(app.session.active().unwrap().is_dirty(), step < 2, "Revert runs only after the release");
            assert_eq!(ctx.input(|input| input.pointer.primary_down()), step == 0, "pointer state is not reset by the boundary");
        }
        assert!(actions.is_empty());
        assert!(app.session.active().unwrap().selection.is_empty());
        assert!(!app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "native actions keep user clipboard provenance");
    }

    /// Close is cancelled and deferred while old-document input is pending, then the request survives to the next frame.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_close_waits_for_input_and_keeps_the_request() {
        let (mut app, ctx, _writes) = screenshot_app();
        let _dir = prepare_revert(&mut app);
        app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
        let mut raw = visible_raw();
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close);
        app.raw_input_hook(&ctx, &mut raw);
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            assert!(app.ui.dialog.is_none(), "the close dialog must not receive the old document's input");
            app.ui(ui);
        });
        assert!(out.viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::CancelClose));
        out.textures_delta.clear();
        assert_eq!(app.session.documents().len(), 1);
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(dialog_uid(&app), Some(app.session.active().unwrap().uid), "the close request survives the deferred frame");
    }

    /// A later control command cannot overtake queued input and eventually completes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_control_command_waits_for_the_preceding_input() {
        let (mut app, ctx, _writes) = screenshot_app();
        let (requests, receiver) = std::sync::mpsc::channel();
        app = app.with_control(receiver);
        let (movement, first_reply) = ControlRequest::new("ui.move", json!({"x": 10, "y": 10}));
        let (command, second_reply) = ControlRequest::new("engine.execute", json!({"command": "frame.create", "params": {"rect": [72,72,144,144]}}));
        requests.send(movement).unwrap();
        requests.send(command).unwrap();
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(first_reply.try_recv().unwrap()["ok"], true);
        assert!(second_reply.try_recv().is_err(), "later commands must not overtake queued input");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert!(second_reply.try_recv().is_err(), "the injected input must settle first");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(second_reply.try_recv().unwrap()["ok"], true, "the later command is eventually handled");
    }

    /// Revert targets the original document before deferred OS-open delivery resumes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_native_revert_precedes_the_deferred_os_open() {
        let (mut app, ctx, _writes) = screenshot_app();
        let dir = prepare_revert(&mut app);
        let original = app.session.active().unwrap().uid;
        let path = dir.path().join("incoming.designcraft");
        std::fs::write(&path, document_with_a_frame()).unwrap();
        app.open_documents_later([path.to_string_lossy().to_string()]);
        app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
        let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
        let mut raw = visible_raw();
        app.raw_input_hook(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            if ui.ctx().current_pass_index() == 0 {
                app.poll_native_actions(ui.ctx(), || actions.pop_front());
            }
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert_eq!(app.session.documents().len(), 1);
        assert!(app.session.active().unwrap().is_dirty());
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.active().unwrap().uid, original);
        assert!(!app.session.active().unwrap().is_dirty(), "Revert runs before delivery resumes");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 2);
        assert_ne!(app.session.active().unwrap().uid, original);
        assert!(!app.session.documents().iter().find(|doc| doc.uid == original).unwrap().is_dirty(), "Revert targeted the original document");
    }

    /// Preview cleanup cannot replace the input batch's document and runs after the batch settles.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_stale_preview_waits_for_the_input_batch() {
        let (mut app, ctx, _writes) = screenshot_app();
        let original = app.session.active().unwrap().uid;
        app.session.preview_new_document(&json!({"title": "Preview"})).unwrap();
        let preview = app.session.new_document_preview();
        assert!(preview.is_some());
        app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.new_document_preview(), preview, "preview cleanup cannot replace the input's document");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert!(app.session.new_document_preview().is_none());
        assert_eq!(app.session.active().unwrap().uid, original);
    }

    /// Revert waits for Select All's forwarded text input to settle.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_native_choices_wait_for_forwarded_text_input() {
        let (mut app, ctx, _writes) = screenshot_app();
        let _dir = prepare_revert(&mut app);
        let mut text = String::from("native field");
        ctx.run_ui(visible_raw(), |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
        })
        .textures_delta
        .clear();
        assert!(ctx.text_edit_focused(), "the native edit command must forward to an actual text field");
        let mut actions = std::collections::VecDeque::from([("edit.selectAll".to_owned(), Value::Null), ("file.revert".to_owned(), json!({}))]);
        app.poll_native_actions(&ctx, || actions.pop_front());
        run_host_frame(&mut app, &ctx, visible_raw());
        assert!(app.session.active().unwrap().is_dirty(), "Revert waits for Select All's forwarded input");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert!(!app.session.active().unwrap().is_dirty());
    }

    /// A queued native menu event precedes and blocks deferred OS-open delivery without synthetic input.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_arriving_native_event_blocks_an_os_open_without_synthetic_input() {
        let (mut app, ctx, _writes) = screenshot_app();
        let dir = prepare_revert(&mut app);
        let original = app.session.active().unwrap().uid;
        let path = dir.path().join("incoming.designcraft");
        std::fs::write(&path, document_with_a_frame()).unwrap();
        app.open_documents_later([path.to_string_lossy().to_string()]);
        let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
        app.native_action_notifier(ctx.clone())();
        let mut raw = visible_raw();
        app.raw_input_hook(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            assert_eq!(app.session.documents().len(), 1, "a menu event already queued by the host must precede the OS open");
            if ui.ctx().current_pass_index() == 0 {
                app.poll_native_actions(ui.ctx(), || actions.pop_front());
            }
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert!(!app.session.active().unwrap().is_dirty(), "the menu action ran in the real UI callback");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 2);
        assert_ne!(app.session.active().unwrap().uid, original);
    }

    /// Hidden logic-only ticks cannot execute native menu actions; a real UI pass can.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_deferred_native_action_waits_for_a_real_ui_pass() {
        let (mut app, ctx, _writes) = screenshot_app();
        let _dir = prepare_revert(&mut app);
        app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
        let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
        let mut raw = visible_raw();
        app.raw_input_hook(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            if ui.ctx().current_pass_index() == 0 {
                app.poll_native_actions(ui.ctx(), || actions.pop_front());
            }
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert!(app.session.active().unwrap().is_dirty());
        let mut raw = hidden_raw();
        app.raw_input_hook(&ctx, &mut raw);
        let _ = ctx.run_logic(&raw, |ctx| app.logic(ctx));
        assert!(app.session.active().unwrap().is_dirty(), "a hidden logic-only tick must not execute native menu actions");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert!(!app.session.active().unwrap().is_dirty(), "the deferred action runs when a UI callback resumes");
    }

    /// An inbox delivery runs only after input sampled for the current document has settled, so
    /// a pending Select All cannot select text in the opened document.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_inbox_open_must_not_receive_a_pending_shortcut() {
        let (mut app, ctx, _writes) = screenshot_app();
        app.services.inbox = Some(Inbox::default());
        let request = app.import_request("open");
        let file = ImportedFile { request, name: "incoming.designcraft".into(), bytes: document_with_a_frame() };
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(file);
        app.control_synthetic.extend(command_key(egui::Key::A));
        run_host_frame(&mut app, &ctx, visible_raw());
        // A deferred delivery is handled by the next settled host iteration.
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 2, "the inbox file is still opened");
        assert!(
            app.session.active().unwrap().selection.is_empty(),
            "Select All sampled before the asynchronous open must not reach the opened document"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_os_document_must_not_receive_a_pending_shortcut() {
        let (mut app, ctx, _writes) = screenshot_app();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("incoming.designcraft");
        std::fs::write(&path, document_with_a_frame()).unwrap();
        app.open_documents_later([path.to_string_lossy().to_string()]);
        app.control_synthetic.extend(command_key(egui::Key::A));
        run_host_frame(&mut app, &ctx, visible_raw());
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 2, "the OS document is still opened");
        assert!(app.session.active().unwrap().selection.is_empty(), "Select All sampled before the OS open must not reach the opened document");
    }

    /// The intended boundary: an imported file waits while the batch that was sampled for the
    /// current document is pending, and is handled once that batch has settled.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_inbox_delivery_waits_for_a_settled_batch_then_runs() {
        let (mut app, ctx, _writes) = screenshot_app();
        app.services.inbox = Some(Inbox::default());
        let request = app.import_request("open");
        let file = ImportedFile { request, name: "incoming.designcraft".into(), bytes: document_with_a_frame() };
        app.services.inbox.as_ref().unwrap().lock().unwrap().push(file);
        app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 1, "the delivery waited for the injected step");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 2, "the deferred delivery is not lost");
    }

    /// Deferred OS open waits for injected input and is not lost.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_host_order_os_document_waits_for_a_settled_batch_then_runs() {
        let (mut app, ctx, _writes) = screenshot_app();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("incoming.designcraft");
        std::fs::write(&path, document_with_a_frame()).unwrap();
        app.open_documents_later([path.to_string_lossy().to_string()]);
        app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 1, "the OS open waited for the injected step");
        run_host_frame(&mut app, &ctx, visible_raw());
        assert_eq!(app.session.documents().len(), 2, "the deferred OS open is not lost");
    }

    /// Revert drops the buffered native drop so it cannot modify the restored document.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_revert_discards_a_buffered_native_drop() {
        let (mut app, ctx) = drop_test_app();
        let dir = prepare_revert(&mut app);
        let path = dir.path().join("drop.png");
        std::fs::write(&path, designcraft_render::Rendered { width: 10, height: 10, pixels: vec![200; 400] }.to_png()).unwrap();
        let mut raw = visible_raw_with_drop(&[&path]);
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(app.pending_drops.len(), 1);
        revert_preserving_uid(&mut app);
        let assets_before = app.session.active().unwrap().doc.assets.len();
        run_ui_pass(&mut app, &ctx, raw);
        assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "a stale native drop must not modify the restored document");
        assert!(app.pending_drops.is_empty());
    }

    /// Revert rejects pre-issue and in-flight screenshots with a document error, issues no retry and writes no pixels.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_revert_rejects_screenshots_before_issue_and_in_flight() {
        for in_flight in [false, true] {
            let (mut app, ctx, writes) = screenshot_app();
            let _dir = prepare_revert(&mut app);
            let (reply, replies) = std::sync::mpsc::channel();
            app.admit_screenshot(Some("revert-stale.png".into()), reply).unwrap();
            app.queued_shots[0].1 = 0.0;
            let mut callback = None;
            if in_flight {
                for _ in 0..3 {
                    let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
                    if let Some(cb) = screenshot_callback(&mut output) {
                        callback = Some(cb);
                    }
                }
                assert!(callback.is_some(), "the in-flight case must actually issue a capture");
            }
            revert_preserving_uid(&mut app);
            if let Some(callback) = callback {
                callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
            }
            for _ in 0..4 {
                let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
                assert!(screenshot_callback(&mut output).is_none(), "Revert must not issue or retry an old capture (in_flight={in_flight})");
            }
            let reply = replies.try_recv().expect("the replaced document's screenshot receives an error");
            assert_eq!(reply["ok"], false, "in_flight={in_flight}: {reply}");
            assert!(reply["error"].as_str().unwrap_or_default().contains("document"));
            assert!(writes.lock().unwrap().is_empty(), "stale pixels must not be saved");
            assert!(app.pending_shots.is_empty());
            assert!(app.queued_shots.is_empty());
        }
    }

    // ---------------------------------------------------------------------------------------
    // Document-generation regressions: reused screenshot slots and buffered native drops must
    // not cross a document/dialog/focus change.
    // ---------------------------------------------------------------------------------------

    /// A document switch before issue errors the request and leaves no pending or queued capture.
    #[test]
    fn design_generation_screenshot_switch_before_issue_does_not_capture_the_new_document() {
        let (mut app, ctx, writes) = screenshot_app();
        let document_a = app.session.active().unwrap().uid;
        let (requests, receiver) = std::sync::mpsc::channel();
        app = app.with_control(receiver);
        let (reply, replies) = std::sync::mpsc::channel();
        requests
            .send(ControlRequest { method: "ui.screenshot".into(), params: json!({"path": "switch-before-issue.png", "focus": false}), reply })
            .unwrap();
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        assert_eq!(app.pending_shots.len(), 1, "ui.screenshot is admitted");
        assert_eq!(app.pending_shots[0].geometry.document, document_a);
        // A menu/control action switches documents before the settling rule issues the capture.
        app.run("file.new", json!({"title": "handoff-before-issue"})).unwrap();
        let document_b = app.session.active().unwrap().uid;
        assert_ne!(document_b, document_a);
        if let Some(at) = app.queued_shots.first_mut() {
            at.1 = 0.0;
        }
        let mut callback = None;
        for _ in 0..3 {
            let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
            if let Some(cb) = screenshot_callback(&mut output) {
                callback = Some(cb);
            }
        }
        if let Some(callback) = callback {
            callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
            shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        }
        assert!(writes.lock().unwrap().is_empty(), "the switched request must not write a file");
        let reply = replies.try_recv().expect("the switched request must receive an error receipt");
        assert_eq!(reply["ok"], false, "the old document's request must not succeed on the new one: {reply}");
        assert!(reply["error"].as_str().unwrap_or_default().contains("document"), "the error must name the document change: {reply}");
        assert!(app.pending_shots.is_empty(), "the stale request released its admission");
        assert!(app.queued_shots.is_empty(), "the stale request left no repaint token");
    }

    /// An in-flight old-document capture cannot complete or retry on the new document; the request and tokens are released.
    #[test]
    fn design_generation_screenshot_in_flight_after_switch_does_not_complete_on_the_new_document() {
        let (mut app, ctx, writes) = screenshot_app();
        let document_a = app.session.active().unwrap().uid;
        let (reply, replies) = std::sync::mpsc::channel();
        app.admit_screenshot(Some("switch-in-flight.png".into()), reply).unwrap();
        app.queued_shots[0].1 = 0.0;
        let mut callback = None;
        for _ in 0..3 {
            let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
            if let Some(cb) = screenshot_callback(&mut output) {
                callback = Some(cb);
            }
        }
        let callback = callback.expect("the request is issued on the first document");
        assert_eq!(app.pending_shots[0].geometry.document, document_a);
        // The capture is in flight on A when the app switches to B.
        app.run("file.new", json!({"title": "handoff-in-flight"})).unwrap();
        assert_ne!(app.session.active().unwrap().uid, document_a);
        callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        // Old code retries the stale request on B; follow any retry through a completion so the
        // regression proves that B never receives the old capture.
        if let Some(at) = app.queued_shots.first_mut() {
            at.1 = 0.0;
        }
        let mut retried = None;
        for _ in 0..3 {
            let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
            if let Some(cb) = screenshot_callback(&mut output) {
                retried = Some(cb);
            }
        }
        if let Some(retried) = retried {
            retried.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
            shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        }
        assert!(writes.lock().unwrap().is_empty(), "the in-flight request must not write a file after the switch");
        let reply = replies.try_recv().expect("the in-flight request must receive an error receipt");
        assert_eq!(reply["ok"], false, "the in-flight request must not succeed on the new document: {reply}");
        assert!(reply["error"].as_str().unwrap_or_default().contains("document"), "the error must name the document change: {reply}");
        assert!(app.pending_shots.is_empty(), "the stale request released its admission");
        assert!(app.queued_shots.is_empty(), "the stale request left no repaint token");
    }

    /// Admission stamps the current session document, not cached geometry from the previous document.
    #[test]
    fn design_generation_screenshot_admission_uses_the_current_document_not_the_cached_geometry() {
        let (mut app, _ctx, _writes) = screenshot_app();
        let document_a = app.session.active().unwrap().uid;
        // `screenshot_app` populated the geometry cache for A. Switch documents without a frame
        // between the switch and admission, as two control requests in one logic tick would.
        app.run("file.new", json!({"title": "admission-current-document"})).unwrap();
        let document_b = app.session.active().unwrap().uid;
        assert_ne!(document_b, document_a);
        let (reply, replies) = std::sync::mpsc::channel();
        app.admit_screenshot(None, reply).unwrap();
        assert_eq!(
            app.pending_shots[0].geometry.document, document_b,
            "admission must use the current session document, not the last cached geometry"
        );
        assert!(replies.try_recv().is_err(), "admission alone replies only when it fails");
        app.pending_shots.clear();
        app.queued_shots.clear();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[derive(Debug)]
    struct TestDroppedFile(std::path::PathBuf);

    #[cfg(not(target_arch = "wasm32"))]
    impl egui::DroppedFile for TestDroppedFile {
        fn path(&self) -> &std::path::Path {
            &self.0
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|error| error.to_string())
        }
    }

    /// Wrap a path as an egui dropped-file handle backed by the test type.
    #[cfg(not(target_arch = "wasm32"))]
    fn dropped_file(path: &std::path::Path) -> egui::DroppedFileHandle {
        std::sync::Arc::new(TestDroppedFile(path.to_path_buf()))
    }

    /// Create a screenshot app with a fixed canvas rectangle and an initialized view for drop tests.
    #[cfg(not(target_arch = "wasm32"))]
    fn drop_test_app() -> (DesignApp, egui::Context) {
        let (mut app, ctx, _writes) = screenshot_app();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
        let _ = app.view_mut();
        (app, ctx)
    }

    /// Write a tiny unique PNG for drop tests.
    #[cfg(not(target_arch = "wasm32"))]
    fn temporary_png(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("designcraft-ui-{}-{name}.png", std::process::id()));
        std::fs::write(&path, designcraft_render::Rendered { width: 10, height: 10, pixels: vec![200; 400] }.to_png()).unwrap();
        path
    }

    /// Build visible raw input with a pointer move and dropped-file handles.
    #[cfg(not(target_arch = "wasm32"))]
    fn visible_raw_with_drop(paths: &[&std::path::Path]) -> egui::RawInput {
        let mut raw = hidden_raw();
        let viewport = raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        viewport.minimized = Some(false);
        viewport.occluded = Some(false);
        raw.events.push(egui::Event::PointerMoved(egui::pos2(100.0, 100.0)));
        raw.dropped_files.extend(paths.iter().map(|path| dropped_file(path)));
        raw
    }

    /// Run one logic-plus-UI frame and discard its texture deltas.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_ui_pass(app: &mut DesignApp, ctx: &egui::Context, raw: egui::RawInput) {
        ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        })
        .textures_delta
        .clear();
    }

    /// A drop buffered before a document switch cannot add assets to the switched document.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_generation_native_drop_is_discarded_when_the_document_changes_before_ui() {
        let (mut app, ctx) = drop_test_app();
        let path = temporary_png("drop-document-change");
        let mut raw = visible_raw_with_drop(&[&path]);
        app.raw_input_hook(&ctx, &mut raw);
        assert!(raw.dropped_files.is_empty(), "raw_input_hook captures the native drop");
        assert_eq!(app.pending_drops.len(), 1, "the raw drop is buffered for the UI pass");
        // A menu/control action switches documents before the settled UI pass consumes it.
        app.run("file.new", json!({"title": "drop-target"})).unwrap();
        let assets_before = app.session.active().unwrap().doc.assets.len();
        run_ui_pass(&mut app, &ctx, raw);
        assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "the stale drop must not land in the switched document");
        assert!(app.pending_drops.is_empty(), "the discarded drop released its buffer");
        let _ = std::fs::remove_file(path);
    }

    /// A drop buffered before a dialog opens cannot land behind it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_generation_native_drop_is_discarded_when_a_dialog_opens_before_ui() {
        let (mut app, ctx) = drop_test_app();
        let path = temporary_png("drop-dialog-change");
        let mut raw = visible_raw_with_drop(&[&path]);
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(app.pending_drops.len(), 1);
        app.run("app.preferences", json!({})).unwrap();
        assert!(app.ui.dialog.is_some(), "a real dialog changed the generation");
        let assets_before = app.session.active().unwrap().doc.assets.len();
        run_ui_pass(&mut app, &ctx, raw);
        assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "the stale drop must not land behind the dialog");
        assert!(app.pending_drops.is_empty(), "the discarded drop released its buffer");
        let _ = std::fs::remove_file(path);
    }

    /// A keyboard-focus change invalidates a buffered drop.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_generation_native_drop_is_discarded_when_focus_changes_before_ui() {
        let (mut app, ctx) = drop_test_app();
        let path = temporary_png("drop-focus-change");
        let mut raw = visible_raw_with_drop(&[&path]);
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(app.pending_drops.len(), 1);
        let focus = egui::Id::unique("design-generation-drop-focus");
        assert_ne!(app.input_generation().focus, Some(focus));
        ctx.memory_mut(|memory| memory.request_focus(focus));
        assert_eq!(app.input_generation().focus, Some(focus), "keyboard focus changed after the drop was buffered");
        let assets_before = app.session.active().unwrap().doc.assets.len();
        run_ui_pass(&mut app, &ctx, raw);
        assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "the stale drop must not land after a focus change");
        assert!(app.pending_drops.is_empty(), "the discarded drop released its buffer");
        let _ = std::fs::remove_file(path);
    }

    /// With no generation change, every dropped file in a batch is placed.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn design_generation_native_drop_batch_without_a_change_still_places_every_file() {
        let (mut app, ctx) = drop_test_app();
        let first = temporary_png("drop-batch-a");
        let second = temporary_png("drop-batch-b");
        let mut raw = visible_raw_with_drop(&[&first, &second]);
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(app.pending_drops.len(), 2, "both files were captured from the raw input");
        let assets_before = app.session.active().unwrap().doc.assets.len();
        run_ui_pass(&mut app, &ctx, raw);
        assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before + 2, "both files in the valid batch are placed");
        assert!(app.pending_drops.is_empty());
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
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
    fn pdf_options_refuse_a_page_range_instead_of_placing_page_one() {
        for spec in ["4-5", "2,5", "2-3", "2.5", "-3", "0", "nan", "inf"] {
            let mut app = app();
            let request = app.import_request("place");
            deliver(&mut app, request, "three.pdf", pdf());
            app.ui.dialog.as_mut().unwrap().fields.insert("page".into(), json!(spec));
            assert!(dialogs::confirm(&mut app).is_err(), "{spec}");
            assert!(app.session.doc().unwrap().doc.assets.is_empty(), "{spec}");
            assert_eq!(app.ui.dialog.as_ref().unwrap().id, "pdfImport", "{spec}");
        }
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
