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
pub type OpenAsyncFn = Box<dyn FnMut(&str)>;
pub type DownloadFn = Box<dyn FnMut(&str, &[u8])>;
pub type ClipboardFn = Box<dyn FnMut() -> Option<String>>;
/// Files `(name, bytes)` delivered asynchronously by the host (web file picker, dropped files).
pub type Inbox = std::sync::Arc<std::sync::Mutex<Vec<(String, Vec<u8>)>>>;

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
    /// Files delivered asynchronously, drained every frame: `.designcraft` → `file.openBytes`,
    /// anything else → `file.place`.
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
    /// Tools panel: Formatting Affects Text (J): with text frames selected, colour edits go to
    /// their text instead of the frames.
    pub formatting_affects_text: bool,
    /// Window > Contextual Task Bar.
    pub task_bar: bool,
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
            formatting_affects_text: false,
            task_bar: true,
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
    /// The egui context (set by the first frame): copied text goes to the system clipboard through it.
    egui_ctx: Option<egui::Context>,
    /// A control-channel request is being handled: commands it runs never read or write the
    /// user's system clipboard (a control client must not see or replace it).
    pub(crate) in_control: bool,
}

impl DesignApp {
    pub fn new(session: Session, services: Services) -> Self {
        // The font menus and the first file opened need the installed fonts: catalog them now.
        designcraft_fonts::FontDb::global().scan_in_background();
        DesignApp {
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
            egui_ctx: None,
            in_control: false,
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
        // Only user-initiated runs touch the system clipboard, never the control channel's.
        let system_clipboard = !self.in_control;
        let params = if system_clipboard { self.with_system_clipboard(id, params) } else { params };
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        self.after_engine();
        match &r {
            Err(e) => self.status(e.clone()),
            // Copied text goes to the system clipboard too, whichever way Copy was chosen.
            Ok(v) if system_clipboard && matches!(id, "edit.copy" | "edit.cut") => {
                if let (Some(t), Some(ctx)) = (v.get("text").and_then(Value::as_str), &self.egui_ctx) {
                    ctx.copy_text(t.to_string());
                    ctx.request_repaint();
                }
            }
            Ok(_) => {}
        }
        r
    }

    /// Paste into text without the clipboard's text (a menu, not the paste keys): read it here.
    fn with_system_clipboard(&mut self, id: &str, mut params: Value) -> Value {
        let typing = self.session.active().is_some_and(|d| d.selection.text.is_some());
        if typing
            && matches!(id, "edit.paste" | "edit.pasteWithoutFormatting")
            && params.get("text").is_none()
            && let Some(text) = self.services.clipboard_text.as_mut().and_then(|f| f())
            && let Some(o) = params.as_object_mut()
        {
            o.insert("text".into(), Value::String(text));
        }
        params
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
        let cmd = if purpose == "place" { "file.place" } else { "file.open" };
        if let Some(pick) = self.services.pick_open.as_mut() {
            return match pick(purpose) {
                // Image Import Options: a multi-page PDF asks which page.
                Some(path) if purpose == "place" && path.to_ascii_lowercase().ends_with(".pdf") => {
                    let pages =
                        self.services.read.as_mut().and_then(|r| r(&path).ok()).and_then(|b| designcraft_render::pdf_page_count(&b)).unwrap_or(1);
                    if pages > 1 {
                        self.ui.dialog = Some(dialogs::Dialog::new("pdfImport", json!({"path": path, "page": "1", "pages": pages})));
                        Ok(Value::Null)
                    } else {
                        self.run(cmd, json!({"path": path}))
                    }
                }
                Some(path) => self.run(cmd, json!({"path": path})),
                None => Ok(Value::Null),
            };
        }
        if let Some(open) = self.services.open_async.as_mut() {
            open(purpose);
        }
        Ok(Value::Null)
    }

    /// Open or place files delivered through the inbox.
    fn drain_inbox(&mut self) {
        let Some(inbox) = self.services.inbox.clone() else { return };
        let files = std::mem::take(&mut *inbox.lock().unwrap_or_else(|e| e.into_inner()));
        for (name, bytes) in files {
            let b64 = designcraft_engine::cmd::base64_encode(&bytes);
            let lower = name.to_ascii_lowercase();
            let r = if lower.ends_with(".ase") {
                self.run("swatch.load", json!({"base64": b64}))
            } else if lower.ends_with(".designcraft") || lower.ends_with(".idml") {
                let title = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
                self.run("file.openBytes", json!({"name": title, "base64": b64}))
            } else {
                self.run("file.place", json!({"name": name, "base64": b64}))
            };
            if let Err(e) = r {
                self.status(format!("{name}: {e}"));
            }
        }
    }

    pub fn select_tool(&mut self, id: &str) {
        let _ = self.run("tool.select", json!({"tool": id}));
        self.ui.flyout = None;
    }

    /// Per-frame logic before layout.
    pub fn logic(&mut self, ctx: &egui::Context) {
        if self.egui_ctx.is_none() {
            self.egui_ctx = Some(ctx.clone());
        }
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
            {
                let p = f.path().to_string_lossy().to_string();
                if p.is_empty() {
                    continue;
                }
                let lp = p.to_ascii_lowercase();
                let cmd = if lp.ends_with(".designcraft") || lp.ends_with(".idml") { "file.open" } else { "file.place" };
                let _ = self.run(cmd, json!({"path": p}));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
