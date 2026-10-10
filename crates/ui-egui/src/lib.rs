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
pub mod dialogs;
pub mod dock;
pub mod i18n;
pub mod icons;
pub mod menus;
pub mod panels;
pub mod render_worker;
mod rtl;
pub mod story_editor;
pub mod taskbar;
pub mod theme;
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
pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;
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
    /// Asynchronous open dialog for a purpose (`open`, `place`); the chosen file arrives later
    /// through [`Services::inbox`]. Used when `pick_open` is unset (web).
    pub open_async: Option<OpenAsyncFn>,
    /// Hand bytes to the user as a named file (browser download). When set, Save uses it
    /// instead of writing to a path.
    pub download: Option<DownloadFn>,
    /// Files delivered asynchronously, drained every frame with their captured request context.
    pub inbox: Option<Inbox>,
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
    /// Edit › Color Settings (re-applied at start).
    pub color_settings: Option<designcraft_color::cms::ColorSettings>,
    /// View › Proof Colors and View › Proof Setup.
    pub proof_colors: bool,
    pub proof_setup: designcraft_color::cms::ProofSetup,
    /// Edit › Spelling › Dynamic Spelling: misspelled words underlined on the canvas.
    pub dynamic_spelling: bool,
    /// Preferences › Story Editor Display: text size (points).
    pub story_editor_size: f32,
    /// Edit › Interface Language: supported codes are listed in `i18n::LANGUAGES`.
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
    /// Preferences › Appearance of Black: 100% K on screen as rich black.
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
    pub dock_expanded: bool,
    pub units: Unit,
    pub workspace: String,
    /// Window › Workspace › New Workspace: saved panel arrangements.
    pub custom_workspaces: Vec<SavedWorkspace>,
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
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            brightness: theme::Brightness::MediumDark,
            screen_mode: ScreenMode::Normal,
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
            rich_black: false,
            overprint_preview: false,
            shortcuts: Default::default(),
            control_bar: false,
            tools_double_column: false,
            dock_tab: "properties".into(),
            open_panel: None,
            floating: Vec::new(),
            dock_expanded: true,
            units: Unit::Picas,
            workspace: "Essentials".into(),
            custom_workspaces: Vec::new(),
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
    /// Story open in the Story Editor.
    pub story_editor: Option<designcraft_doc::StoryId>,
    control_rx: Option<Receiver<ControlRequest>>,
    pending_shots: Vec<(u64, Option<String>, Sender<ControlResponse>, f64)>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
    styled: bool,
    /// The interface language the UI fonts were installed for (it orders the CJK fallbacks).
    fonts_lang: String,
    pub restyle: bool,
    fonts_ready: bool,
    pub integrated_titlebar: bool,
    /// The host installed a native menu bar (macOS): don't draw menus in the window.
    pub native_menu: bool,
    /// Shortcuts the native menu handles (skip them in the egui shortcut handler).
    pub native_shortcuts: std::collections::HashSet<String>,
    last_time: f64,
    /// When recovery data was last written (seconds, egui time).
    pub last_recovery: f64,
}

impl DesignApp {
    pub fn new(session: Session, services: Services) -> Self {
        // The font menus and the first file opened need the installed fonts: catalog them now.
        designcraft_fonts::FontDb::global().scan_in_background();
        DesignApp {
            pending_pdf: None,
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
            story_editor: None,
            control_rx: None,
            pending_shots: vec![],
            queued_shots: vec![],
            shot_token: 0,
            styled: false,
            fonts_lang: String::new(),
            restyle: false,
            fonts_ready: false,
            integrated_titlebar: false,
            native_menu: false,
            native_shortcuts: Default::default(),
            last_time: 0.0,
            last_recovery: 0.0,
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
        ViewInfo { zoom: self.view().map(|v| v.zoom).unwrap_or(1.0), snap: self.ui.snap_view(), unit: self.ui.units }
    }

    pub fn status(&mut self, s: impl Into<String>) {
        self.ui.status = s.into();
    }

    /// THE entry point for every action (menus, shortcuts, palette, panels, control channel).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if let Some(r) = menus::run_ui(self, id, &params) {
            return r;
        }
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        self.after_engine();
        if let Err(e) = &r {
            self.status(e.clone());
        }
        r
    }

    /// Handle requests produced by tools/commands (dialogs, view changes, file pickers).
    pub fn after_engine(&mut self) {
        let reqs = std::mem::take(&mut self.session.ui_requests);
        for r in reqs {
            match r {
                UiRequest::Dialog { id, params } => self.ui.dialog = Some(dialogs::Dialog::new(&id, params)),
                UiRequest::View { params } => canvas::apply_view_request(self, &params),
                UiRequest::Pick { purpose, params } => {
                    let _ = params;
                    let _ = self.pick_and_open(&purpose);
                }
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
        let cmd = if purpose == "place" { "file.place" } else { "file.open" };
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
                        self.open_file(cmd, json!({"path": path}))
                    }
                }
                Some(path) => self.open_file(cmd, json!({"path": path})),
                None => Ok(Value::Null),
            };
        }
        let request = self.import_request(purpose);
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
            self.status(message);
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
        self.open_file(
            "file.place",
            json!({"name": file.name, "base64": designcraft_engine::cmd::base64_encode(&file.bytes), "pdfPage": page, "pdfCrop": crop}),
        )
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
                    self.status("PDF import cancelled: the originating document has closed");
                }
            }
        }
        let Some(inbox) = self.services.inbox.clone() else { return };
        let files = std::mem::take(&mut *inbox.lock().unwrap_or_else(|e| e.into_inner()));
        for file in files {
            if self.ui.dialog.is_some() {
                self.status(format!("{}: import cancelled while a dialog is open; choose the file again", file.name));
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
        let layout = opens_as_document(&file.name);
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
        if !self.color_applied {
            self.color_applied = true;
            if let Some(cs) = self.ui.color_settings.clone() {
                let _ = designcraft_color::cms::set_active(&cs);
            }
        }
        let scale = self.ui.ui_scale.clamp(0.5, 3.0);
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        if !self.styled {
            theme::install_fonts(ctx, &self.ui.language);
            self.fonts_lang = self.ui.language.clone();
            self.styled = true;
            self.restyle = true;
        } else {
            if self.fonts_lang != self.ui.language {
                theme::install_fonts(ctx, &self.ui.language);
                self.fonts_lang = self.ui.language.clone();
            }
            self.fonts_ready = true;
        }
        if self.restyle {
            theme::apply(ctx, &theme::Tokens::for_brightness(self.ui.brightness));
            self.restyle = false;
        }
        let now = ctx.input(|i| i.time);
        // Crash recovery: unsaved documents are written to the recovery folder every 30 s.
        #[cfg(not(target_arch = "wasm32"))]
        if self.session.recovery_dir.is_some() && now - self.last_recovery > (self.session.prefs.recovery_minutes * 60.0).max(5.0) {
            self.last_recovery = now;
            if self.session.documents().iter().any(|d| d.is_dirty())
                && let Err(e) = self.session.execute("file.recovery.save", &json!({}))
            {
                self.status(format!("Couldn't write recovery data: {e}"));
            }
        }
        let dt = now - self.last_time;
        if dt > 0.0 {
            self.perf.fps = self.perf.fps * 0.9 + (1.0 / dt).min(240.0) * 0.1;
        }
        self.last_time = now;
        self.drain_control(ctx);
        if self.fonts_ready {
            self.drain_inbox();
        }
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        if self.fonts_ready {
            menus::shortcuts(self, ctx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let p = f.path().to_string_lossy().to_string();
            if !p.is_empty() {
                let _ = self.open_dropped(&p);
            }
        }
    }

    /// A file dropped on the window: documents open, anything else is placed.
    pub fn open_dropped(&mut self, path: &str) -> Result<Value, String> {
        let cmd = if opens_as_document(path) { "file.open" } else { "file.place" };
        self.open_file(cmd, json!({"path": path}))
    }

    /// Open (`file.open`, `file.openBytes`) or place (`file.place`) a file the user chose. A file
    /// that can't be opened or placed gets an alert with the reason, as well as the status line.
    pub fn open_file(&mut self, cmd: &str, params: Value) -> Result<Value, String> {
        let file = match params.get("path").and_then(Value::as_str) {
            Some(path) => std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string()),
            None => params.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        };
        let r = self.run(cmd, params);
        if let Err(e) = &r {
            let title = if cmd == "file.place" { "Can't Place the File" } else { "Can't Open the File" };
            self.ui.dialog = Some(dialogs::Dialog::new("alert", json!({"title": title, "file": file, "message": e})));
        }
        r
    }

    /// Inject synthetic events (one pointer event per frame).
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if self.synthetic.is_empty() {
            return;
        }
        let n = match self.synthetic[0] {
            egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } => 1,
            _ => self.synthetic.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1),
        };
        if let Some(egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. }) = self.synthetic.first() {
            raw.events.push(egui::Event::PointerMoved(*p));
        }
        raw.events.extend(self.synthetic.drain(..n));
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.fonts_ready {
            ctx.request_repaint();
            return;
        }
        let t0 = now_ms();
        let t = theme::Tokens::get(&ctx);
        let presenting = self.ui.screen_mode == ScreenMode::Presentation;
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
            if self.split && !presenting {
                // Two panes with a divider; each has its own view (zoom, scroll) and render cache.
                let r = ui.available_rect_before_wrap();
                let mid = r.center().x;
                let halves = [
                    egui::Rect::from_min_max(r.min, egui::pos2(mid - 1.0, r.max.y)),
                    egui::Rect::from_min_max(egui::pos2(mid + 1.0, r.min.y), r.max),
                ];
                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(mid - 1.0, r.min.y), egui::pos2(mid + 1.0, r.max.y)), 0.0, t.border);
                for (k, half) in halves.into_iter().enumerate() {
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
                    if ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| half.contains(p))) {
                        self.focus_pane = k as u8;
                    }
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
        dock::flyout(self, &ctx);
        dock::floating(self, &ctx);
        story_editor::show(self, &ctx);
        dialogs::show(self, &ctx);
        about::show(self, &ctx);
        menus::palette(self, &ctx);
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

/// Files that open as documents (when dropped or picked) rather than being placed: DesignCraft
/// and IDML.
pub fn opens_as_document(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".designcraft", ".idml"].iter().any(|ext| n.ends_with(ext))
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

/// Is `text` the name of a key ("AltGraph", "Dead", "AudioVolumeUp") instead of typed text?
/// Browsers report a key press as a string: the character it types, or a name made of ASCII
/// letters and digits that starts with a capital. One key press never types such a word.
pub fn is_key_name(text: &str) -> bool {
    text.len() > 1 && text.starts_with(|c: char| c.is_ascii_uppercase()) && text.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Drop text events that are key names. eframe's web backend sends the name of every key it
/// doesn't know as text, so AltGr typed "AltGraph" into the story. `text_field_focused`: an
/// egui text field has the keyboard; its text comes from the browser's input events, which can
/// hold whole words, and is left alone.
pub fn drop_key_name_text(raw: &mut egui::RawInput, text_field_focused: bool) {
    if !text_field_focused {
        raw.events.retain(|e| !matches!(e, egui::Event::Text(t) if is_key_name(t)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The Control panel's two-row groups were centred as if they were one row tall, so the second
    /// row hung below the panel and was cut off.
    #[test]
    fn control_bar_shows_its_second_row() {
        use egui_kittest::kittest::Queryable as _;
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
        let rows: Vec<egui::Rect> = h.query_all_by_label("Y:").map(|n| n.rect()).filter(|r| bar.contains(r.left_top())).collect();
        assert_eq!(rows.len(), 1, "the Y: caption of the Control panel");
        assert!(rows[0].bottom() <= bar.bottom(), "Y: caption {:?} hangs below the Control panel {bar:?}", rows[0]);
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
        // A focused text field gets its text from input events, which can be whole words.
        let mut raw = egui::RawInput { events: events(), ..Default::default() };
        drop_key_name_text(&mut raw, true);
        assert_eq!(raw.events, events());
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
        for name in ["a.designcraft", "B.IDML"] {
            assert!(opens_as_document(name), "{name}");
        }
        for name in ["a.png", "b.pdf", "idml.txt", "c.idml.zip"] {
            assert!(!opens_as_document(name), "{name}");
        }
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
    pub fn open(mut app: crate::DesignApp, size: egui::Vec2) -> Harness<'static, Window> {
        if app.session.active().is_none() {
            app.run("file.new", serde_json::json!({})).unwrap();
        }
        let mut h = Harness::builder().with_size(size).with_max_steps(1000).build_ui_state(
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
    fn pdf() -> Vec<u8> {
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
        let native = designcraft_engine::cmd::to_bytes(&source.doc().unwrap().doc);
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
        let exported = app.run("file.exportPdf", json!({})).unwrap();
        let exported = designcraft_engine::cmd::base64_decode(exported["base64"].as_str().unwrap());
        let pixels = designcraft_render::render_pdf_page(&exported, 0, 400).unwrap();
        assert!(pixels.data().iter().filter(|p| p.g > 240 && p.r < 10 && p.b < 10).count() > 1000);
        assert_eq!(pixels.data().iter().filter(|p| p.r > 240 && p.g < 10 && p.b < 10).count(), 0);
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
        let native = designcraft_engine::cmd::to_bytes(&app.session.doc().unwrap().doc);
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
