//! The DesignCraft engine façade.
//!
//! Every user-visible action is a command with a stable id (`frame.create`, `text.insert`,
//! `layout.pages.insert`…) and JSON parameters. The egui UI, the CLI, the control channel and MCP
//! all go through [`Session::execute`]. Tools (pointer gestures) are hosted here and reduce to
//! commands, so every gesture is journaled and replayable.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod cmd;
pub mod dtd;
pub mod guard;
pub mod links;
pub mod math;
pub mod recovery;
pub mod sample;
pub mod script;
mod tooling;

use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::{Document, LayerId, Selection};
use serde_json::Value;

pub use cmd::{CommandInfo, CommandSpec, command_specs, find_command};
pub use designcraft_compose as compose;
pub use designcraft_doc as doc;
pub use designcraft_tools as tools;
pub use tooling::{SnapView, UiRequest, ViewInfo};

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
}

#[derive(Clone, Debug)]
pub struct DocState {
    pub doc: Arc<Document>,
    pub selection: Selection,
    pub history: History,
    pub path: Option<String>,
    pub revision: u64,
    pub saved_revision: u64,
    /// The document as last saved (dirty = the current document is a different allocation).
    pub saved_doc: Arc<Document>,
    pub active_layer: LayerId,
    pub interaction: Option<Interaction>,
    /// Editing parent spreads (Pages panel double-click on a parent).
    pub editing_parents: bool,
    pub uid: u64,
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

impl Drop for FontScope {
    fn drop(&mut self) {
        designcraft_fonts::FontDb::global().close_scope(self.0);
    }
}

static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl DocState {
    pub fn new(doc: Document, path: Option<String>) -> Self {
        let active_layer = doc.default_layer();
        let doc = Arc::new(doc);
        DocState {
            saved_doc: doc.clone(),
            doc,
            selection: Selection::default(),
            history: History { limit: 1000, ..Default::default() },
            path,
            revision: 1,
            saved_revision: 1,
            active_layer,
            interaction: None,
            editing_parents: false,
            uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            preview_stash: None,
            preview_record: None,
            fonts: None,
        }
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
    /// Preferences › Type › Show Font Names in English: the font menus show CJK families by their
    /// English names instead of their native ones. Documents store the English name either way.
    pub show_font_names_in_english: bool,
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
            show_font_names_in_english: false,
            smart_text_reflow: true,
            autocorrect: false,
            autocorrect_list: default_autocorrect(),
            show_added_text: true,
            show_note_anchors: true,
            recovery_minutes: 0.5,
        }
    }
}

pub struct Session {
    docs: Vec<DocState>,
    active: Option<usize>,
    pub prefs: Prefs,
    pub cache: Arc<Cache>,
    pub(crate) tool: Box<dyn designcraft_tools::Tool>,
    pub journal: Vec<(String, Value)>,
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
    /// Graphic loaded in the place cursor: (asset, natural size in points).
    pub loaded: Option<(designcraft_doc::AssetId, (f64, f64))>,
    pub(crate) untitled: u32,
    /// Transforms applied to the current selection, oldest first (Transform Again / Sequence
    /// Again), with the selection they were applied to and whether the last one came from a
    /// tool interaction (its previews replace each other).
    pub(crate) transforms: (Vec<(String, Value)>, Vec<designcraft_doc::ItemId>, bool),
    /// Drag and drop text editing: the press landed in the selected text.
    pub(crate) text_drag: bool,
    /// The open Object Library (File › New / Open Library).
    pub library: Option<cmd::library::Library>,
    /// The open book (File › Open Book).
    pub book: Option<cmd::book::Book>,
    /// Content Collector conveyor: collected objects as snippets (name, bytes).
    pub conveyor: Vec<(String, Vec<u8>)>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        guard::install_panic_hook();
        Session {
            docs: vec![],
            active: None,
            prefs: Prefs::default(),
            cache: Arc::new(Cache::new()),
            tool: designcraft_tools::create("selection"),
            journal: vec![],
            clipboard: None,
            clipboard_origin: 0.0,
            recovery_dir: None,
            text_clipboard: None,
            ui_requests: vec![],
            loaded: None,
            transforms: Default::default(),
            text_drag: false,
            library: None,
            conveyor: Vec::new(),
            book: None,
            untitled: 0,
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
    pub fn set_active(&mut self, i: usize) {
        if i < self.docs.len() && self.active != Some(i) {
            self.stop_data_preview();
            self.active = Some(i);
        }
    }
    pub fn add_document(&mut self, d: DocState) -> usize {
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
        st.revision = st.revision.saturating_add(1);
    }
    pub fn close_document(&mut self, i: usize) {
        if self.active == Some(i) {
            self.stop_data_preview();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(dir), Some(d)) = (&self.recovery_dir, self.docs.get(i)) {
            recovery::discard(dir, d.uid);
        }
        if i < self.docs.len() {
            self.docs.remove(i);
            self.active = if self.docs.is_empty() { None } else { Some(i.min(self.docs.len() - 1)) };
        }
    }

    /// Run a command by id. Edits push one undo step (unless inside an interaction).
    /// Run command `id`. A panic inside it becomes [`EngineError::Internal`] and leaves the
    /// document as it was (see [`guard`]).
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        self.guarded(id, |s| s.execute_unguarded(id, params))
    }

    fn execute_unguarded(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
        if let Err(e) = (spec.enabled)(self) {
            // A command that takes `ids` acts on the objects the call names, so it needs no
            // selection: with a text caret or nothing selected, `{"ids": [5]}` still runs.
            let named = if e == cmd::NOTHING_SELECTED { cmd::named_targets(spec, params) } else { None };
            let Some(ids) = named else { return Err(EngineError::Disabled(id.into(), e)) };
            let st = self.doc()?;
            if let Some(gone) = ids.iter().find(|i| st.doc.item(**i).is_none()) {
                return Err(EngineError::Disabled(id.into(), format!("no object with id {}", gone.0)));
            }
        }
        // Preview is temporary. Save, merge, and every other edit restore the template first.
        // Queries stay read-only so the panel can poll fields during a preview. `data.preview`
        // itself is the one command that fills the live document.
        if id != "data.preview" && (spec.undoable || spec.journal) {
            self.stop_data_preview();
        }
        let before = self.active().map(|d| (d.uid, d.doc.clone()));
        let r = (spec.run)(self, params)?;
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
        // Record undo if the document changed (and we're not previewing an interaction).
        if let (Some((uid, old)), Some(st)) = (before, self.active_mut())
            && st.uid == uid
            && !Arc::ptr_eq(&old, &st.doc)
            && st.interaction.is_none()
            && spec.undoable
        {
            let entry = HistoryEntry { label: spec.label.to_string(), doc: old, selection: st.selection.clone() };
            push_undo(st, entry);
        }
        if spec.journal {
            self.journal.push((id.to_string(), params.clone()));
        }
        Ok(r)
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
            let _ = cmd::place_text_autoflow(&mut d, sid, 500);
        } else {
            // Drop empty trailing frames on the last pages (one frame, nothing else there).
            while let Some(story) = d.story(sid) {
                if story.frames.len() < 2 {
                    break;
                }
                let Some(&last) = story.frames.last() else { break };
                let cs = designcraft_compose::compose_story(&d, sid, &Default::default());
                if cs.frame(last).is_some_and(|ft| !ft.range.is_empty()) {
                    break;
                }
                let Some(page) = d.page_of_item(last) else { break };
                let alone = d.spreads.iter().flat_map(|sp| sp.items.iter()).filter(|i| d.page_of_item(i.id) == Some(page)).count() == 1;
                if page + 1 != d.page_count() || !alone || d.page_count() < 2 {
                    break;
                }
                if d.remove_item(last).is_err() || d.delete_pages(&[page]).is_err() {
                    break;
                }
            }
        }
        if let Some(st) = self.active_mut() {
            st.doc = Arc::new(d);
            st.revision += 1;
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
        doc.sync_endnote_story();
        st.doc = Arc::new(doc);
        st.selection = sel;
        st.revision += 1;
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
#[cfg(test)]
mod tests_idml;
#[cfg(test)]
mod tests_table;
