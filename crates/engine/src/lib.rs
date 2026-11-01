//! The DesignCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`frame.create`, `text.insert`,
//! `layout.pages.insert`…) and JSON parameters. The egui UI, the CLI, the control channel and MCP
//! all go through [`Session::execute`]. Tools (pointer gestures) are hosted here and reduce to
//! commands, so every gesture is journaled and replayable.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
pub mod atomic;
pub mod autosize;
pub mod cmd;
pub mod dtd;
pub mod file_access;
pub mod guard;
pub mod io;
pub mod links;
pub mod math;
pub mod recovery;
pub mod sample;
pub mod script;
pub mod separations;
mod tooling;

use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::{Document, LayerId, Selection};
use serde_json::Value;

pub use cmd::params::{check_keys, check_params};
pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use designcraft_compose as compose;
pub use designcraft_doc as doc;
pub use designcraft_tools as tools;
pub use tooling::{ImeMutation, ImeTextContext, SnapView, UiRequest, ViewInfo, ime_delete_ranges};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    #[error("command `{0}` is not available right now: {1}")]
    Disabled(String, String),
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    #[error("no active document")]
    NoDocument,
    #[error("{0}")]
    Other(String),
    /// A command panicked; the guard kept the document (a bug: please report it).
    #[error("internal error in `{0}` (the document was kept as it was): {1}")]
    Internal(String, String),
}

impl From<designcraft_doc::DocError> for EngineError {
    fn from(e: designcraft_doc::DocError) -> Self {
        EngineError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EngineError>;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub label: String,
    pub doc: Arc<Document>,
    pub selection: Selection,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub struct Interaction {
    pub label: String,
    pub doc: Arc<Document>,
    pub selection: Selection,
    pub preview: Option<(String, Value)>,
    /// The document's revision when the gesture began: a cancel puts it back.
    pub revision: u64,
}

#[derive(Clone, Debug)]
pub struct DocState {
    pub doc: Arc<Document>,
    pub selection: Selection,
    pub history: History,
    pub path: Option<String>,
    /// Names the document state for caches keyed `(uid, revision)` (see
    /// [`DocState::bump_revision`]).
    pub revision: u64,
    pub saved_revision: u64,
    /// The document as last saved (dirty = the current document is a different allocation).
    pub saved_doc: Arc<Document>,
    pub active_layer: LayerId,
    pub interaction: Option<Interaction>,
    /// Editing parent spreads (Pages panel double-click on a parent).
    pub editing_parents: bool,
    pub uid: u64,
    /// Changes whenever this document state is recreated, even if [`Self::uid`] is preserved
    /// (File › Revert keeps the uid for recovery but replaces the document).
    pub identity_epoch: u64,
    /// Recovery entries this state actually opened or published, for repeated-open and cleanup.
    /// Bounded, deduplicated checkpoint addresses; never inferred from uid alone.
    pub(crate) recovery_entries: Vec<(std::path::PathBuf, u64)>,
    /// Unfilled document while data-merge preview is showing a record. Not saved.
    pub preview_stash: Option<Arc<Document>>,
    /// 1-based record currently previewed.
    pub preview_record: Option<u32>,
    /// The fonts the document brought (`doc.font_scope`), found while it is open.
    pub fonts: Option<Arc<FontScope>>,
}

/// A document's font scope ([`designcraft_fonts::FontDb::load_document_fonts`]): its fonts are
/// found until the document's state (with every copy of it) is gone — closed, reverted, or its
/// session ended.
#[derive(Debug)]
pub struct FontScope(pub u32);

/// A graphic loaded in the place cursor (`place.load`, File › Place with no frame to fill).
#[derive(Clone, Copy, Debug)]
pub struct LoadedGraphic {
    pub asset: designcraft_doc::AssetId,
    /// Natural size in points: the Image Import Options › Crop to box for a PDF page (the asset's
    /// `pdf_box`).
    pub size: (f64, f64),
}

impl Drop for FontScope {
    fn drop(&mut self) {
        designcraft_fonts::FontDb::global().close_scope(self.0);
    }
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static NEXT_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl DocState {
    /// Initialize saved/live document identity, revision, epoch, active default layer, empty history with limit 1,000 and the initial path.
    pub fn new(doc: Document, path: Option<String>) -> Self {
        let active_layer = doc.default_layer();
        let doc = Arc::new(doc);
        let revision = next_revision();
        let identity_epoch = NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        DocState {
            saved_doc: doc.clone(),
            doc,
            selection: Selection::default(),
            history: History { limit: 1000, ..Default::default() },
            path,
            revision,
            saved_revision: revision,
            active_layer,
            interaction: None,
            editing_parents: false,
            uid: identity_epoch,
            identity_epoch,
            recovery_entries: Vec::new(),
            preview_stash: None,
            preview_record: None,
            fonts: None,
        }
    }
    /// The document changed: it gets a revision no document state had before. A cancelled
    /// gesture puts back the revision it began at, so the numbers its previews used are never
    /// handed out again (a cache entry for one would show the cancelled preview).
    pub fn bump_revision(&mut self) {
        self.revision = next_revision();
    }
    pub fn is_dirty(&self) -> bool {
        !Arc::ptr_eq(&self.doc, &self.saved_doc)
    }
    pub fn title(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.doc.title.clone())
    }
}

/// Preferences that the engine needs (UI keeps its own).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    pub show_hidden_characters: bool,
    pub typographers_quotes: bool,
    /// Polygon Settings (double-click the Polygon tool): number of sides, star inset (0–1).
    pub polygon_sides: u32,
    pub star_inset: f64,
    /// Scaling objects scales their stroke weights (Preferences › General › When Scaling).
    pub scale_strokes: bool,
    /// X/Y/W/H in the Control and Properties panels measure the stroke's outer edge.
    pub dimensions_include_stroke: bool,
    /// Preferences › General › Transformations are Totals: rotation, shear and scale of nested
    /// objects (a graphic in a frame, objects in groups) are measured on the pasteboard rather
    /// than relative to their container.
    pub transformations_are_totals: bool,
    /// Preferences › General › Page Numbering View: Absolute (1, 2, 3… from the first page) instead
    /// of section numbering (the page names, e.g. "iv", "A-3").
    pub absolute_page_numbers: bool,
    /// Preferences › Composition › Highlight: H&J violations, custom tracking/kerning, substituted
    /// fonts (screen only).
    pub highlight_hj: bool,
    pub highlight_keeps: bool,
    pub highlight_custom_tracking: bool,
    pub highlight_substituted_fonts: bool,
    /// Preferences › Appearance of Black › Printing / Exporting: 100% K as rich (pure) black in
    /// RGB output (PNG) instead of the accurate dark grey.
    pub rich_black_output: bool,
    /// Font menu favourites (family names).
    pub favorite_fonts: Vec<String>,
    /// Fonts picked in the font menus, most recent first (shown at the menus' top).
    pub recent_fonts: Vec<String>,
    /// Preferences › Type › Show Font Names in English: the font menus show CJK families by their
    /// English names instead of their native ones. Documents store the English name either way.
    pub show_font_names_in_english: bool,
    /// Preferences › Type › Show CJK Features: the menu items, dialogs and panel fields that only
    /// Japanese, Chinese and Korean typesetting uses. `None` follows the interface language. It
    /// hides interface only: layout, file formats and commands work either way.
    pub cjk_features: Option<bool>,
    /// Preferences › Type › Smart Text Reflow: pages follow the primary text frame's story
    /// (added while it oversets, empty ones at the end removed).
    pub smart_text_reflow: bool,
    /// Preferences › Autocorrect: typing a space or punctuation after a listed word replaces it.
    pub autocorrect: bool,
    /// Misspelled word → correction (lowercase; the correction follows the typed capitalisation).
    pub autocorrect_list: Vec<(String, String)>,
    /// Preferences › Track Changes › Show: added text highlighted on screen.
    pub show_added_text: bool,
    /// Preferences › Notes: note anchors shown in layout view.
    pub show_note_anchors: bool,
    /// Preferences › File Handling: minutes between document recovery saves.
    pub recovery_minutes: f64,
    /// File › New › Document › Save Document Preset: saved document presets, by name (`title`).
    pub document_presets: Vec<designcraft_doc::build::NewDocument>,
    /// Preferences › File Handling › Create Links When Placing Text Files: placed text files are
    /// linked (Links panel) unless `file.place` says otherwise.
    pub link_text_files: bool,
    /// File › Open Recent: documents opened or saved, most recent first.
    pub recent_files: Vec<String>,
    /// Preferences › File Handling › Number of Recent Items to Display (0 keeps no list).
    pub recent_files_max: u32,
    /// Preferences › Units & Increments set with no document open: what new documents start
    /// with, as InDesign's application defaults. A document keeps its own once made.
    pub default_units: DefaultUnits,
}

/// Units & Increments for new documents (Preferences with no document open), in the keys
/// `document.preferences` uses.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DefaultUnits {
    pub horizontal_units: designcraft_geom::Unit,
    pub vertical_units: designcraft_geom::Unit,
    pub ruler_origin: designcraft_doc::RulerOrigin,
    /// Cursor Key, in points.
    pub keyboard_increment: f64,
    /// Kerning/Tracking, in 1/1000 em.
    pub kerning_increment: f64,
}

impl Default for DefaultUnits {
    /// A new document's own settings.
    fn default() -> Self {
        let d = designcraft_doc::DocSettings::default();
        DefaultUnits {
            horizontal_units: designcraft_geom::Unit::Picas,
            vertical_units: designcraft_geom::Unit::Picas,
            ruler_origin: d.ruler_origin,
            keyboard_increment: d.keyboard_increment,
            kerning_increment: d.kerning_increment,
        }
    }
}

impl DefaultUnits {
    /// Gives a new document these defaults. Unless it was made from a preset (whose units are its
    /// own), its vertical ruler follows the preference while it keeps the default horizontal
    /// units (units picked in New Document set both rulers).
    pub fn apply(&self, settings: &mut designcraft_doc::DocSettings, preset: bool) {
        if !preset && settings.horizontal_units == self.horizontal_units {
            settings.vertical_units = self.vertical_units;
        }
        settings.ruler_origin = self.ruler_origin;
        settings.keyboard_increment = self.keyboard_increment;
        settings.kerning_increment = self.kerning_increment;
    }

    /// Refuses increments that aren't finite and positive (at most 1000).
    pub fn check(&self) -> std::result::Result<(), String> {
        for (name, v) in [("keyboardIncrement", self.keyboard_increment), ("kerningIncrement", self.kerning_increment)] {
            if !(v.is_finite() && v > 0.0 && v <= 1000.0) {
                return Err(format!("`defaultUnits.{name}` must be more than 0 and at most 1000"));
            }
        }
        Ok(())
    }
}

/// The most recent files Preferences lets File › Open Recent keep.
pub const MAX_RECENT_FILES: u32 = 100;

impl Prefs {
    /// Move `path` to the front of File › Open Recent (no duplicates, at most `recent_files_max`).
    pub fn note_recent_file(&mut self, path: &str) {
        if path.is_empty() {
            return;
        }
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_string());
        self.trim_recent_files();
    }

    /// Drop the entries beyond `recent_files_max` (capped at `MAX_RECENT_FILES`).
    pub fn trim_recent_files(&mut self) {
        self.recent_files_max = self.recent_files_max.min(MAX_RECENT_FILES);
        self.recent_files.truncate(self.recent_files_max as usize);
    }
}

/// A starter autocorrect list (common English typing slips).
pub fn default_autocorrect() -> Vec<(String, String)> {
    [
        ("teh", "the"),
        ("adn", "and"),
        ("taht", "that"),
        ("recieve", "receive"),
        ("seperate", "separate"),
        ("occured", "occurred"),
        ("untill", "until"),
        ("wich", "which"),
        ("becuase", "because"),
        ("definately", "definitely"),
        ("thier", "their"),
        ("alot", "a lot"),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            document_presets: Vec::new(),
            show_hidden_characters: false,
            typographers_quotes: true,
            polygon_sides: 6,
            star_inset: 0.0,
            scale_strokes: true,
            dimensions_include_stroke: true,
            transformations_are_totals: true,
            absolute_page_numbers: false,
            highlight_hj: false,
            highlight_keeps: false,
            highlight_custom_tracking: false,
            highlight_substituted_fonts: true,
            rich_black_output: false,
            favorite_fonts: Vec::new(),
            recent_fonts: Vec::new(),
            show_font_names_in_english: false,
            cjk_features: None,
            smart_text_reflow: true,
            autocorrect: false,
            autocorrect_list: default_autocorrect(),
            show_added_text: true,
            show_note_anchors: true,
            recovery_minutes: 0.5,
            link_text_files: false,
            recent_files: Vec::new(),
            recent_files_max: 20,
            default_units: DefaultUnits::default(),
        }
    }
}

pub struct Session {
    docs: Vec<DocState>,
    active: Option<usize>,
    /// An undoable command is running: the commands it runs are part of its undo step.
    in_undo_step: bool,
    pub prefs: Prefs,
    pub cache: Arc<Cache>,
    pub(crate) tool: Box<dyn designcraft_tools::Tool>,
    /// The selection tool a Cmd-held press borrows for one gesture while another tool stays
    /// chosen (#919). Dropped on release.
    pub(crate) temp_tool: Option<Box<dyn designcraft_tools::Tool>>,
    /// The selection tool (`selection` or `directSelection`) chosen last: the one Cmd borrows.
    pub(crate) last_selection_tool: &'static str,
    pub journal: Vec<(String, Value)>,
    pub(crate) find_context: Option<cmd::find::FindContext>,
    pub clipboard: Option<Arc<Document>>,
    /// The x the clipboard's objects are measured from on their spread: its spine, or the left edge of
    /// single-sided pages. Paste keeps their distance from it.
    pub clipboard_origin: f64,
    /// Crash-recovery folder (the app sets it; saving or closing a document removes its entry).
    pub recovery_dir: Option<std::path::PathBuf>,
    /// Copied text with its formatting (a frameless story slice) and its plain text.
    pub text_clipboard: Option<(Arc<designcraft_doc::Story>, String)>,
    /// Requests for the UI (dialogs, view changes) produced by tools/commands.
    pub ui_requests: Vec<UiRequest>,
    /// Graphic loaded in the place cursor.
    pub loaded: Option<LoadedGraphic>,
    pub(crate) untitled: u32,
    /// Transforms applied to the current selection, oldest first (Transform Again / Sequence
    /// Again), with the selection they were applied to and whether the last one came from a
    /// tool interaction (its previews replace each other).
    pub(crate) transforms: (Vec<(String, Value)>, Vec<designcraft_doc::ItemId>, bool),
    /// Drag and drop text editing: the press landed in the selected text.
    pub(crate) text_drag: bool,
    /// The word, line, paragraph or story the last `text.selectAt` selected: a drag that follows a
    /// double, triple or quadruple click extends from it by whole units (#981).
    pub(crate) text_unit: Option<designcraft_doc::TextSel>,
    /// Character formatting chosen at a bare caret, for the text typed there next.
    pub(crate) typing_format: Option<cmd::text::TypingFormat>,
    /// The open Object Library (File › New / Open Library).
    pub library: Option<cmd::library::Library>,
    /// The open book (File › Open Book).
    pub book: Option<cmd::book::Book>,
    /// Content Collector conveyor: collected objects as snippets (name, bytes).
    pub conveyor: Vec<(String, Vec<u8>)>,
    /// New Document's Preview: the preview document's uid and the uid of the document that was
    /// active before it (see [`Session::preview_new_document`]).
    pub(crate) new_document_preview: Option<(u64, Option<u64>)>,
    /// Linked text files checked by the Links panel: path → the size and modification time the
    /// file had and the hash of its contents then, so a file touched but not changed is read once.
    pub(crate) text_link_checks: std::collections::HashMap<String, (designcraft_doc::Fingerprint, String)>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// The name `file.new` gives the next document without a title (New Document shows it).
    pub fn next_untitled_title(&self) -> String {
        format!("Untitled-{}", self.untitled.saturating_add(1))
    }

    pub fn new() -> Self {
        guard::install_panic_hook();
        Session {
            docs: vec![],
            active: None,
            in_undo_step: false,
            prefs: Prefs::default(),
            cache: Arc::new(Cache::new()),
            tool: designcraft_tools::create("selection"),
            temp_tool: None,
            last_selection_tool: "selection",
            journal: vec![],
            find_context: None,
            clipboard: None,
            clipboard_origin: 0.0,
            recovery_dir: None,
            text_clipboard: None,
            text_link_checks: Default::default(),
            ui_requests: vec![],
            loaded: None,
            transforms: Default::default(),
            text_drag: false,
            text_unit: None,
            typing_format: None,
            library: None,
            conveyor: Vec::new(),
            book: None,
            untitled: 0,
            new_document_preview: None,
        }
    }

    pub fn documents(&self) -> &[DocState] {
        &self.docs
    }
    pub fn active_index(&self) -> Option<usize> {
        self.active
    }
    /// A page's label as the UI shows it (Page Numbering View).
    pub fn page_label(&self, abs: usize) -> String {
        match self.active() {
            Some(d) if !self.prefs.absolute_page_numbers => d.doc.page_name(abs),
            _ => (abs + 1).to_string(),
        }
    }

    /// The page a typed page reference means: `+n` is always absolute; otherwise a section page
    /// name (section numbering) or a position (absolute numbering, or no page has that name).
    pub fn resolve_page(&self, s: &str) -> Option<usize> {
        let d = self.active()?;
        let n = d.doc.page_count();
        let s = s.trim();
        let pos = |t: &str| t.parse::<usize>().ok().filter(|k| (1..=n).contains(k)).map(|k| k - 1);
        if let Some(t) = s.strip_prefix('+') {
            return pos(t);
        }
        if !self.prefs.absolute_page_numbers
            && let Some(i) = (0..n).find(|i| d.doc.page_name(*i).eq_ignore_ascii_case(s))
        {
            return Some(i);
        }
        pos(s)
    }

    pub fn active(&self) -> Option<&DocState> {
        self.active.and_then(|i| self.docs.get(i))
    }
    pub fn active_mut(&mut self) -> Option<&mut DocState> {
        self.active.and_then(|i| self.docs.get_mut(i))
    }
    pub fn doc(&self) -> Result<&DocState> {
        self.active().ok_or(EngineError::NoDocument)
    }
    pub fn doc_mut(&mut self) -> Result<&mut DocState> {
        self.active_mut().ok_or(EngineError::NoDocument)
    }
    /// Release the transient Find/Change domain when its caller starts or closes a search.
    pub fn reset_find_scope(&mut self) {
        self.find_context = None;
    }

    fn discard_stale_find_scope(&mut self) {
        if self.find_context.as_ref().is_some_and(|c| self.active().is_none_or(|st| !c.is_current(st))) {
            self.reset_find_scope();
        }
    }

    pub fn set_active(&mut self, i: usize) {
        if i < self.docs.len() && self.active != Some(i) {
            // The composition belongs to the document it shows in.
            if let Err(e) = self.end_composition() {
                log::warn!("ending the IME composition: {e}");
            }
            self.reset_find_scope();
            self.stop_data_preview();
            self.active = Some(i);
        }
    }
    pub fn add_document(&mut self, d: DocState) -> usize {
        if let Err(e) = self.end_composition() {
            log::warn!("ending the IME composition: {e}");
        }
        self.reset_find_scope();
        self.stop_data_preview();
        self.docs.push(d);
        self.active = Some(self.docs.len() - 1);
        self.docs.len() - 1
    }
    /// Put the unfilled template back. Preview is session state and must not be saved.
    pub fn stop_data_preview(&mut self) {
        let Some(st) = self.active_mut() else { return };
        st.preview_record = None;
        let Some(stash) = st.preview_stash.take() else { return };
        st.doc = stash;
        st.bump_revision();
    }
    pub fn close_document(&mut self, i: usize) {
        if self.active == Some(i) {
            self.stop_data_preview();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let recovery_dir = self.recovery_dir.clone();
            recovery::discard_document(self, i, recovery_dir.as_deref());
        }
        if i < self.docs.len() {
            self.reset_find_scope();
            self.docs.remove(i);
            self.active = if self.docs.is_empty() { None } else { Some(i.min(self.docs.len() - 1)) };
            if self.docs.is_empty() {
                // Retire this document generation's cache. In-flight render jobs may still
                // own it, but must not repopulate the idle session's cache after closing.
                self.cache = Arc::new(Cache::new());
            }
        }
    }

    /// Run a command by id. Edits push one undo step (unless inside an interaction).
    /// Run command `id`. A panic inside it becomes [`EngineError::Internal`] and leaves the
    /// document as it was (see [`guard`]).
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        // A command from outside the IME ends its composition first, keeping the marked text as
        // typed: Undo then takes back that typing, and nothing runs on top of the preview the
        // composition would drop. Queries and recovery saves leave it alone.
        if self.tool.composing() && id != "file.recovery.save" && find_command(id).is_some_and(|c| c.journal || c.undoable) {
            self.end_composition()?;
        }
        self.guarded(id, |s| s.execute_unguarded(id, params))
    }

    fn execute_unguarded(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
        // The crate's own tests are the audit of the params descriptions (#776): a key a test
        // passes that its command's description doesn't name fails the test.
        #[cfg(test)]
        check_params(id, params)?;
        self.check_enabled(spec, id, params)?;
        // Preview is temporary. Save, merge, and every other edit restore the template first.
        // Queries stay read-only so the panel can poll fields during a preview. `data.preview`
        // itself is the one command that fills the live document.
        if id != "data.preview" && (spec.undoable || spec.journal) {
            self.stop_data_preview();
        }
        // Preview restoration can replace the document before a Find handler resolves bounds.
        self.discard_stale_find_scope();
        let before = self.active().map(|d| (d.uid, d.doc.clone(), d.selection.clone()));
        // What an undoable command runs is part of its undo step.
        let nested = self.in_undo_step;
        self.in_undo_step = nested || spec.undoable;
        let ran = (spec.run)(self, params);
        self.in_undo_step = nested;
        let r = match ran {
            Ok(r) => r,
            Err(e) => {
                // What it did before it failed can be undone too.
                self.keep_guide_selection_valid(before.as_ref(), id);
                self.record_undo_step(spec, before, nested);
                return Err(e);
            }
        };
        // Undo, redo or deleting layers can take the active layer away: new objects would land on
        // a layer that isn't there (invisible, not exported). Fall back to the top layer.
        if let Some(st) = self.active_mut()
            && st.doc.layer(st.active_layer).is_none()
        {
            st.active_layer = st.doc.default_layer();
        }
        self.record_transform(id, params);
        if self.prefs.smart_text_reflow && spec.undoable {
            self.smart_reflow();
        }
        // Live captions follow their sources.
        if spec.undoable
            && let Some(st) = self.active_mut()
            && st.interaction.is_none()
            && st.doc.spreads.iter().any(|sp| sp.items.iter().any(|i| i.live_caption.is_some()))
        {
            let mut d = (*st.doc).clone();
            if cmd::captions::refresh_live(&mut d) {
                st.doc = Arc::new(d);
            }
        }
        self.autosize_changed(spec, before.as_ref());
        self.keep_guide_selection_valid(before.as_ref(), id);
        self.record_undo_step(spec, before, nested);
        if spec.journal {
            self.journal.push((id.to_string(), params.clone()));
        }
        self.discard_stale_find_scope();
        Ok(r)
    }

    /// Fails with `Disabled` unless the command is enabled or names objects it can act on.
    fn check_enabled(&self, spec: &CommandSpec, id: &str, params: &Value) -> Result<()> {
        if let Err(e) = (spec.enabled)(self) {
            // A command that takes `ids` acts on the objects the call names, so the selection
            // doesn't matter: with a text caret, nothing selected, or a selection the command
            // can't act on, `{"ids": [5]}` still runs.
            let named = if cmd::selection_reason(&e) { cmd::named_targets(spec, params) } else { None };
            let Some(ids) = named else { return Err(EngineError::Disabled(id.into(), e)) };
            let st = self.doc()?;
            if let Some(gone) = ids.iter().find(|i| st.doc.item(**i).is_none()) {
                return Err(EngineError::Disabled(id.into(), format!("no object with id {}", gone.0)));
            }
        }
        Ok(())
    }

    /// Auto-size frames fit the text the command changed, in the same undo step.
    fn autosize_changed(&mut self, spec: &CommandSpec, before: Option<&(u64, Arc<Document>, Selection)>) {
        if spec.undoable
            && let Some(st) = self.active_mut()
        {
            let prev = before.filter(|(uid, _, _)| *uid == st.uid).map(|(_, d, _)| d.as_ref());
            if prev.is_none_or(|p| !std::ptr::eq(p, st.doc.as_ref())) {
                let todo = autosize::candidates(&st.doc, prev);
                if !todo.is_empty() {
                    let mut d = (*st.doc).clone();
                    if autosize::fit(&mut d, todo) {
                        st.doc = Arc::new(d);
                        st.bump_revision();
                    }
                }
            }
        }
    }

    /// Drops selected guides the command `id` made stale, against the document as it will be
    /// recorded: after the passes that run with the command (smart reflow, live captions, autosize).
    fn keep_guide_selection_valid(&mut self, before: Option<&(u64, Arc<Document>, Selection)>, id: &str) {
        if let Some(st) = self.active_mut() {
            let old = before.filter(|(uid, _, _)| *uid == st.uid).map(|(_, d, sel)| (d.as_ref(), sel));
            cmd::guides::keep_selection_valid(st, old, id);
        }
    }

    /// Record undo if the document changed (and we're not previewing an interaction), with the
    /// selection from before the command. A command run by an undoable one leaves the step to it:
    /// one command is one undo step.
    fn record_undo_step(&mut self, spec: &CommandSpec, before: Option<(u64, Arc<Document>, Selection)>, nested: bool) {
        if let (Some((uid, old, selection)), Some(st)) = (before, self.active_mut())
            && st.uid == uid
            && !Arc::ptr_eq(&old, &st.doc)
            && st.interaction.is_none()
            && spec.undoable
            && !nested
        {
            let entry = HistoryEntry { label: spec.label.to_string(), doc: old, selection };
            push_undo(st, entry);
        }
    }

    /// Smart Text Reflow for the primary story: add threaded pages while it oversets; remove
    /// trailing pages whose only object is an empty frame of it.
    fn smart_reflow(&mut self) {
        let Some(st) = self.active() else { return };
        if st.interaction.is_some() {
            return;
        }
        let Some(sid) = st.doc.settings.primary_story else { return };
        let Some(story) = st.doc.story(sid) else { return };
        let cs = self.cache.get(&st.doc, sid, None);
        let overset = cs.overset_at.is_some();
        let empty_tail = story.frames.len() > 1 && story.frames.last().is_some_and(|f| cs.frame(*f).is_none_or(|ft| ft.range.is_empty()));
        if !overset && !empty_tail {
            return;
        }
        let mut d = (*st.doc).clone();
        if overset {
            let _ = cmd::place_text_autoflow(&mut d, sid, 500, "Smart Text Reflow", true);
        } else {
            // Drop the last pages while all they hold is empty frames of the story.
            while let Some(story) = d.story(sid) {
                let Some(&last) = story.frames.last() else { break };
                let Some(page) = d.page_of_item(last) else { break };
                let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
                let on_page: Vec<_> =
                    d.spreads.iter().flat_map(|sp| sp.items.iter()).map(|i| i.id).filter(|id| d.page_of_item(*id) == Some(page)).collect();
                let empty = |id: &designcraft_doc::ItemId| story.frames.contains(id) && cs.frame(*id).is_none_or(|ft| ft.range.is_empty());
                if page + 1 != d.page_count() || d.page_count() < 2 || !on_page.iter().all(empty) || on_page.len() >= story.frames.len() {
                    break;
                }
                if on_page.iter().any(|id| d.remove_item(*id).is_err()) || d.delete_pages(&[page]).is_err() {
                    break;
                }
            }
        }
        if let Some(st) = self.active_mut() {
            st.doc = Arc::new(d);
            st.bump_revision();
        }
    }

    /// Remember a move/rotate/scale/shear/flip for Transform Again.
    fn record_transform(&mut self, id: &str, params: &Value) {
        if !matches!(id, "transform.move" | "transform.rotate" | "transform.scale" | "transform.shear" | "transform.flip") {
            return;
        }
        let Some(st) = self.active() else { return };
        let sel = st.selection.items.clone();
        let live = st.interaction.is_some();
        let mut p = params.clone();
        if let Some(o) = p.as_object_mut() {
            o.remove("ids");
            o.remove("id");
        }
        let (list, on, was_live) = &mut self.transforms;
        if *on != sel {
            list.clear();
            *on = sel;
        } else if *was_live && live && list.last().is_some_and(|l| l.0 == id) {
            list.pop();
        }
        list.push((id.to_string(), p));
        *was_live = live;
    }

    pub fn commands(&self) -> Vec<CommandInfo> {
        command_specs().iter().map(|c| c.info(self)).collect()
    }

    /// Mutate the active document (copy-on-write) and bump the revision.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Document, &mut Selection) -> Result<T>) -> Result<T> {
        let st = self.doc_mut()?;
        let mut doc = (*st.doc).clone();
        let mut sel = st.selection.clone();
        let r = f(&mut doc, &mut sel)?;
        crate::cmd::sync_placeholders(&mut doc);
        crate::cmd::flex::sync(&mut doc);
        doc.sync_endnote_story();
        st.doc = Arc::new(doc);
        st.selection = sel;
        st.bump_revision();
        self.reset_find_scope();
        Ok(r)
    }
}

fn push_undo(st: &mut DocState, e: HistoryEntry) {
    st.history.undo.push(e);
    st.history.redo.clear();
    if st.history.undo.len() > st.history.limit.max(1) {
        st.history.undo.remove(0);
    }
}

pub(crate) use push_undo as record_undo;

#[cfg(test)]
mod tests;
