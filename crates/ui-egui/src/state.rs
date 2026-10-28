use super::*;

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
    /// View › Story Editor › Show/Hide Depth Ruler.
    pub story_editor_depth_ruler: bool,
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
    /// Native disposable disk previews; originals and memory previews are unaffected.
    pub persist_image_previews: bool,
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
    /// Auto-Collapse Iconic Panels: a flyout closes on Escape or a click outside it. Off by
    /// default. Saved as `autoCollapseIconicPanels`: `dockAutoCollapse` in older preference files
    /// holds a default of on, so it is not read.
    #[serde(rename = "autoCollapseIconicPanels")]
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
            story_editor_depth_ruler: true,
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
            persist_image_previews: true,
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
            dock_auto_collapse: false,
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
            drag: Default::default(),
            previews: Default::default(),
            drag_active: false,
            defer_full: false,
            low_quality: false,
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
    pub(crate) drag: crate::drag_preview::Preview,
    pub(crate) previews: crate::canvas_preview::Previews,
    pub(crate) drag_active: bool,
    pub(crate) defer_full: bool,
    pub(crate) low_quality: bool,
    /// View › Separations Preview: the visible spreads' plates.
    pub separations: separations::Preview,
}

#[derive(Default, Clone, Copy, Debug, Serialize)]
pub struct Perf {
    pub frame_ms: f64,
    pub render_ms: f64,
    pub fps: f64,
}
