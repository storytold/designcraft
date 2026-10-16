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
    /// Natural size in points.
    pub size: (f64, f64),
    /// Image Import Options › Crop to: the box of the graphic a new frame shows (x, y, w, h).
    pub crop: Option<(f64, f64, f64, f64)>,
}

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
            recovery_entries: Vec::new(),
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
    /// Fonts picked in the font menus, most recent first (shown at the menus' top).
    pub recent_fonts: Vec<String>,
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
    /// Preferences › File Handling › Create Links When Placing Text Files: placed text files are
    /// linked (Links panel) unless `file.place` says otherwise.
    pub link_text_files: bool,
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
            recent_fonts: Vec::new(),
            show_font_names_in_english: false,
            smart_text_reflow: true,
            autocorrect: false,
            autocorrect_list: default_autocorrect(),
            show_added_text: true,
            show_note_anchors: true,
            recovery_minutes: 0.5,
            link_text_files: false,
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
    pub(crate) find_context: Option<cmd::find::FindContext>,
    pub clipboard: Option<Arc<Document>>,
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
    /// Character formatting chosen at a bare caret, for the text typed there next.
    pub(crate) typing_format: Option<cmd::text::TypingFormat>,
    /// Text an input method is composing (a dead key's accent), shown in the story as an
    /// interaction preview until it commits: (document uid, byte length before the caret).
    pub(crate) composition: Option<(u64, usize)>,
    /// The open Object Library (File › New / Open Library).
    pub library: Option<cmd::library::Library>,
    /// The open book (File › Open Book).
    pub book: Option<cmd::book::Book>,
    /// Content Collector conveyor: collected objects as snippets (name, bytes).
    pub conveyor: Vec<(String, Vec<u8>)>,
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
    pub fn new() -> Self {
        guard::install_panic_hook();
        Session {
            docs: vec![],
            active: None,
            prefs: Prefs::default(),
            cache: Arc::new(Cache::new()),
            tool: designcraft_tools::create("selection"),
            journal: vec![],
            find_context: None,
            clipboard: None,
            recovery_dir: None,
            text_clipboard: None,
            text_link_checks: Default::default(),
            ui_requests: vec![],
            loaded: None,
            transforms: Default::default(),
            text_drag: false,
            typing_format: None,
            composition: None,
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
            self.reset_find_scope();
            self.stop_data_preview();
            self.active = Some(i);
        }
    }
    pub fn add_document(&mut self, d: DocState) -> usize {
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
        st.revision = st.revision.saturating_add(1);
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
        // Text still being composed isn't part of the story: any other edit drops it first.
        if id != "text.compose" && (spec.undoable || spec.journal) {
            self.end_composition();
        }
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
        // Preview restoration can replace the document before a Find handler resolves bounds.
        self.discard_stale_find_scope();
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
        self.discard_stale_find_scope();
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
            let _ = cmd::place_text_autoflow(&mut d, sid, 500, "Smart Text Reflow");
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
        crate::cmd::flex::sync(&mut doc);
        doc.sync_endnote_story();
        st.doc = Arc::new(doc);
        st.selection = sel;
        st.revision += 1;
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
#[cfg(test)]
mod tests_file_guard;
#[cfg(test)]
mod tests_idml;
#[cfg(test)]
mod tests_roots;
#[cfg(test)]
mod tests_table;

#[cfg(test)]
mod resize_anchor_tests {
    use std::sync::Arc;

    use designcraft_doc::{ItemId, SpreadRef};
    use designcraft_geom::{Point, Rect, Unit};
    use designcraft_tools::{Mods, PointerEvent, PointerKind, SnapView};
    use serde_json::json;

    use super::{Session, ViewInfo};

    fn assert_rect(actual: Rect, expected: Rect) {
        for (have, want) in [(actual.x0, expected.x0), (actual.y0, expected.y0), (actual.x1, expected.x1), (actual.y1, expected.y1)] {
            assert!((have - want).abs() < 1e-6, "{actual:?}, want {expected:?}");
        }
    }

    #[test]
    fn resize_alt_and_shift_anchors_survive_previews_and_history() {
        for (mods, down, previews, anchor) in [
            (
                Mods { alt: true, shift: true, ..Mods::default() },
                Point::new(200.0, 150.0),
                [
                    (Point::new(190.0, 140.0), Rect::new(110.0, 105.0, 190.0, 145.0)),
                    (Point::new(175.0, 135.0), Rect::new(125.0, 112.5, 175.0, 137.5)),
                ],
                Point::new(150.0, 125.0),
            ),
            (
                Mods { shift: true, ..Mods::default() },
                Point::new(200.0, 125.0),
                [(Point::new(220.0, 125.0), Rect::new(100.0, 95.0, 220.0, 155.0)), (Point::new(180.0, 125.0), Rect::new(100.0, 105.0, 180.0, 145.0))],
                Point::new(100.0, 125.0),
            ),
        ] {
            let mut s = Session::new();
            s.execute("file.new", &json!({"pages": 1})).unwrap();
            let id =
                ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap());
            s.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[None]", "weight": 0})).unwrap();
            s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
            s.set_tool("selection");
            let before = s.doc().unwrap().doc.clone();
            let undo = s.doc().unwrap().history.undo.len();
            let journal = s.journal.len();
            let view = ViewInfo { zoom: 1.0, snap: SnapView::OFF, unit: Unit::Points, stroke_in_front: false };
            let layout = s.layout();
            let at = layout.to_canvas(SpreadRef::Doc(0), down);
            s.pointer(&PointerEvent::new(PointerKind::Down, at.x, at.y).with_mods(mods), view).unwrap();
            assert!(s.doc().unwrap().interaction.is_some());
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
            for (at, expected) in previews {
                let at = layout.to_canvas(SpreadRef::Doc(0), at);
                s.pointer(&PointerEvent::new(PointerKind::Drag, at.x, at.y).with_mods(mods), view).unwrap();
                let item = s.doc().unwrap().doc.item(id).unwrap();
                assert_rect(item.path.bounds().unwrap(), expected);
                assert_rect(item.bounds(), expected);
                let actual_anchor = if mods.alt { item.bounds().center() } else { Point::new(item.bounds().x0, item.bounds().center().y) };
                assert!((actual_anchor - anchor).hypot() < 1e-6);
                assert_eq!(s.doc().unwrap().history.undo.len(), undo);
                assert_eq!(s.journal.len(), journal);
            }
            let at = layout.to_canvas(SpreadRef::Doc(0), previews[1].0);
            s.pointer(&PointerEvent::new(PointerKind::Up, at.x, at.y).with_mods(mods), view).unwrap();
            let after = s.doc().unwrap().doc.clone();
            assert_rect(after.item(id).unwrap().bounds(), previews[1].1);
            assert_eq!(s.doc().unwrap().selection.items, vec![id]);
            assert!(s.doc().unwrap().interaction.is_none());
            assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
            assert_eq!(s.journal.len(), journal + 1);
            assert_eq!(s.journal.last().unwrap().0, "transform.resize");
            after.check().unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
            s.execute("edit.redo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
        }
    }

    fn resize_cancel_fixture() -> (Session, ItemId, ViewInfo) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let id = ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap());
        s.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[Black]", "weight": 4})).unwrap();
        let target = s.execute("frame.create", &json!({"rect": [400, 100, 534, 200], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.stroke", &json!({"ids": [target], "swatch": "[None]", "weight": 0})).unwrap();
        s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
        s.doc_mut().unwrap().selection.key = Some(id);
        // Content scaling has a fixed physical stroke for the successful Cmd control.
        s.execute("prefs.set", &json!({"scaleStrokes": false})).unwrap();
        s.execute("transform.move", &json!({"dx": 7, "dy": 0})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(!s.doc().unwrap().history.redo.is_empty());
        assert_rect(s.doc().unwrap().doc.item(id).unwrap().bounds(), Rect::new(100.0, 100.0, 200.0, 150.0));
        s.set_tool("selection");
        let view = ViewInfo {
            zoom: 1.0,
            snap: SnapView { smart_guides: true, smart_dimensions: true, ..SnapView::OFF },
            unit: Unit::Points,
            stroke_in_front: false,
        };
        (s, id, view)
    }

    fn assert_resize_history(actual: &super::History, expected: &super::History) {
        assert_eq!(actual.limit, expected.limit);
        assert_eq!(actual.undo.len(), expected.undo.len());
        assert_eq!(actual.redo.len(), expected.redo.len());
        for (have, want) in actual.undo.iter().zip(&expected.undo).chain(actual.redo.iter().zip(&expected.redo)) {
            assert_eq!(have.label, want.label);
            assert!(Arc::ptr_eq(&have.doc, &want.doc));
            assert_eq!(have.selection, want.selection);
        }
    }

    #[test]
    fn resize_escape_restores_whole_document_selection_history_and_redo() {
        let (mut s, id, view) = resize_cancel_fixture();
        let before = s.doc().unwrap().doc.clone();
        let selection = s.doc().unwrap().selection.clone();
        let history = s.doc().unwrap().history.clone();
        let journal = s.journal.clone();
        let layout = s.layout();
        let down = layout.to_canvas(SpreadRef::Doc(0), Point::new(200.0, 125.0));
        let at = layout.to_canvas(SpreadRef::Doc(0), Point::new(228.0, 125.0));
        s.pointer(&PointerEvent::new(PointerKind::Down, down.x, down.y), view).unwrap();
        assert!(s.doc().unwrap().interaction.is_some());
        assert!(s.tool_busy());
        s.pointer(&PointerEvent::new(PointerKind::Drag, at.x, at.y), view).unwrap();
        assert!(!Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        assert_rect(s.doc().unwrap().doc.item(id).unwrap().bounds(), Rect::new(100.0, 100.0, 230.0, 150.0));
        assert!(!s.overlays(view).is_empty());
        assert_resize_history(&s.doc().unwrap().history, &history);
        assert_eq!(s.journal, journal);
        assert!(s.tool_key(designcraft_tools::ToolKey::Escape, Mods::default(), view).unwrap());
        let assert_restored = |s: &Session| {
            let st = s.doc().unwrap();
            assert!(Arc::ptr_eq(&st.doc, &before), "cancellation must restore the whole original document");
            assert_eq!(st.selection, selection);
            assert_resize_history(&st.history, &history);
            assert_eq!(s.journal, journal);
            assert!(st.interaction.is_none());
            assert!(!s.tool_busy());
        };
        assert_restored(&s);
        assert!(s.overlays(view).is_empty());
        for kind in [PointerKind::Up, PointerKind::Drag, PointerKind::Move, PointerKind::Up] {
            s.pointer(&PointerEvent::new(kind, at.x, at.y), view).unwrap();
            assert_restored(&s);
            assert!(s.overlays(view).is_empty());
        }
        // Idle Escape still deselects rather than reopening or cancelling an interaction.
        assert!(s.tool_key(designcraft_tools::ToolKey::Escape, Mods::default(), view).unwrap());
        assert!(s.doc().unwrap().selection.is_empty());
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        assert_resize_history(&s.doc().unwrap().history, &history);
        assert!(s.doc().unwrap().interaction.is_none());
    }

    #[test]
    fn resize_cmd_snaps_fixed_stroke_and_commits_one_history_step() {
        let (mut s, id, view) = resize_cancel_fixture();
        let before = s.doc().unwrap().doc.clone();
        let selection = s.doc().unwrap().selection.clone();
        let undo = s.doc().unwrap().history.undo.len();
        let journal = s.journal.len();
        let layout = s.layout();
        let down = layout.to_canvas(SpreadRef::Doc(0), Point::new(200.0, 125.0));
        let at = layout.to_canvas(SpreadRef::Doc(0), Point::new(228.0, 125.0));
        let mods = Mods { cmd: true, ..Mods::default() };
        s.pointer(&PointerEvent::new(PointerKind::Down, down.x, down.y).with_mods(mods), view).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Drag, at.x, at.y).with_mods(mods), view).unwrap();
        let item = s.doc().unwrap().doc.item(id).unwrap();
        assert_rect(item.bounds(), Rect::new(100.0, 100.0, 230.0, 150.0));
        assert_eq!(item.stroke.weight, 4.0);
        assert!((item.visible_bounds().width() - 134.0).abs() < 1e-6);
        assert!(!s.overlays(view).is_empty());
        assert_eq!(s.doc().unwrap().history.undo.len(), undo);
        assert_eq!(s.journal.len(), journal);
        s.pointer(&PointerEvent::new(PointerKind::Up, at.x, at.y).with_mods(mods), view).unwrap();
        let after = s.doc().unwrap().doc.clone();
        assert_rect(after.item(id).unwrap().bounds(), Rect::new(100.0, 100.0, 230.0, 150.0));
        assert_eq!(s.doc().unwrap().selection, selection);
        assert!(s.doc().unwrap().interaction.is_none());
        assert!(!s.tool_busy());
        assert!(s.overlays(view).is_empty());
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        assert!(s.doc().unwrap().history.redo.is_empty());
        assert_eq!(s.journal.len(), journal + 1);
        let committed = s.journal.last().unwrap();
        assert_eq!(committed.0, "transform.resize");
        assert_eq!(committed.1["content"], true);
        after.check().unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        assert_eq!(s.doc().unwrap().selection, selection);
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
        assert_eq!(s.doc().unwrap().selection, selection);
    }

    #[test]
    fn resize_releasing_cmd_uses_original_stroke_for_dimension_snap_and_history() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        let id = ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap());
        s.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[Black]", "weight": 4})).unwrap();
        let target = s.execute("frame.create", &json!({"rect": [400, 100, 406, 400], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.stroke", &json!({"ids": [target], "swatch": "[None]", "weight": 0})).unwrap();
        s.execute("prefs.set", &json!({"scaleStrokes": true})).unwrap();
        s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
        s.doc_mut().unwrap().selection.key = Some(id);
        s.execute("transform.move", &json!({"dx": 7, "dy": 0})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        s.set_tool("selection");
        let before = s.doc().unwrap().doc.clone();
        let selection = s.doc().unwrap().selection.clone();
        let history = s.doc().unwrap().history.clone();
        let journal = s.journal.clone();
        assert!(s.prefs.scale_strokes);
        assert!(!selection.content);
        assert!(!history.redo.is_empty());
        assert_rect(before.item(id).unwrap().path.bounds().unwrap(), Rect::new(100.0, 100.0, 200.0, 150.0));
        assert_eq!(before.item(id).unwrap().stroke.weight, 4.0);
        assert!((before.item(id).unwrap().visible_bounds().width() - 104.0).abs() < 1e-6);
        let view = ViewInfo {
            zoom: 1.0,
            snap: SnapView { smart_guides: true, smart_dimensions: true, zone_px: 8.0, ..SnapView::OFF },
            unit: Unit::Points,
            stroke_in_front: false,
        };
        let layout = s.layout();
        let event = |kind, at: Point, mods| {
            let at = layout.to_canvas(SpreadRef::Doc(0), at);
            PointerEvent::new(kind, at.x, at.y).with_mods(mods)
        };
        let cmd = Mods { cmd: true, ..Mods::default() };
        s.pointer(&event(PointerKind::Down, Point::new(200.0, 150.0), cmd), view).unwrap();
        assert!(s.doc().unwrap().interaction.is_some());
        assert!(s.tool_busy());
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        s.pointer(&event(PointerKind::Drag, Point::new(300.0, 200.0), cmd), view).unwrap();
        let scaled = s.doc().unwrap().doc.item(id).unwrap();
        assert_rect(scaled.path.bounds().unwrap(), Rect::new(100.0, 100.0, 300.0, 200.0));
        assert_rect(scaled.bounds(), Rect::new(100.0, 100.0, 300.0, 200.0));
        assert_eq!(scaled.stroke.weight, 8.0, "the held Cmd preview must actually scale the stroke");
        assert!((scaled.visible_bounds().width() - 208.0).abs() < 1e-6);
        assert!(s.overlays(view).is_empty());
        assert_eq!(s.doc().unwrap().selection, selection);
        assert_resize_history(&s.doc().unwrap().history, &history);
        assert_eq!(s.journal, journal);

        // Only the literal final path may differ from the original document. In particular,
        // the Cmd preview's stroke 8 and doubled height must not leak into the ordinary preview.
        let mut expected = (*before).clone();
        expected.item_mut(id).unwrap().path = designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 102.0, 150.0));
        for _ in 0..2 {
            s.pointer(&event(PointerKind::Drag, Point::new(103.0, 150.0), Mods::default()), view).unwrap();
            let item = s.doc().unwrap().doc.item(id).unwrap();
            assert_eq!(item.stroke.weight, 4.0, "ordinary preview restores the original stroke");
            assert!(
                (item.visible_bounds().width() - 6.0).abs() < 1e-6,
                "releasing Cmd must snap to feasible physical width 6, got {}",
                item.visible_bounds().width()
            );
            assert_rect(item.path.bounds().unwrap(), Rect::new(100.0, 100.0, 102.0, 150.0));
            assert_rect(item.bounds(), Rect::new(100.0, 100.0, 102.0, 150.0));
            assert_eq!(s.doc().unwrap().doc.as_ref(), &expected);
            assert_eq!(s.doc().unwrap().selection, selection);
            assert!(s.doc().unwrap().interaction.is_some());
            assert!(s.tool_busy());
            assert_resize_history(&s.doc().unwrap().history, &history);
            assert_eq!(s.journal, journal);
            let overlays = s.overlays(view);
            assert_eq!(overlays.iter().filter(|o| matches!(o, designcraft_tools::Overlay::Measure { .. })).count(), 1);
            assert_eq!(overlays.iter().filter(|o| matches!(o, designcraft_tools::Overlay::Guide { .. })).count(), 1);
        }
        let after = s.doc().unwrap().doc.clone();
        s.pointer(&event(PointerKind::Up, Point::new(103.0, 150.0), Mods::default()), view).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
        assert_eq!(s.doc().unwrap().selection, selection);
        assert!(s.doc().unwrap().interaction.is_none());
        assert!(!s.tool_busy());
        assert!(s.overlays(view).is_empty());
        let mut committed_history = history.clone();
        committed_history.redo.clear();
        committed_history.undo.push(super::HistoryEntry { label: "Resize".into(), doc: before.clone(), selection: selection.clone() });
        assert_resize_history(&s.doc().unwrap().history, &committed_history);
        let mut committed_journal = journal;
        committed_journal.push((
            "transform.resize".into(),
            json!({"from": [100.0, 100.0, 200.0, 150.0], "to": [100.0, 100.0, 102.0, 150.0], "content": false, "distribute": false}),
        ));
        assert_eq!(s.journal, committed_journal);
        after.check().unwrap();

        s.execute("edit.undo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
        assert_eq!(s.doc().unwrap().selection, selection);
        let mut undone_history = history;
        undone_history.redo = vec![super::HistoryEntry { label: "Resize".into(), doc: after.clone(), selection: selection.clone() }];
        assert_resize_history(&s.doc().unwrap().history, &undone_history);
        s.execute("edit.redo", &json!({})).unwrap();
        assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
        assert_eq!(s.doc().unwrap().doc.as_ref(), &expected);
        assert_eq!(s.doc().unwrap().selection, selection);
        assert_resize_history(&s.doc().unwrap().history, &committed_history);
    }

    #[test]
    fn resize_flex_releasing_cmd_preserves_first_preview_commit_and_cancel() {
        for cancel in [false, true] {
            let mut s = Session::new();
            s.execute("file.new", &json!({"pages": 1})).unwrap();
            let id =
                ItemId(s.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap());
            s.execute("flex.convert", &json!({"ids": [id.0]})).unwrap();
            s.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[Black]", "weight": 4})).unwrap();
            let target = s.execute("frame.create", &json!({"rect": [400, 100, 406, 400], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
            s.execute("object.stroke", &json!({"ids": [target], "swatch": "[None]", "weight": 0})).unwrap();
            s.execute("prefs.set", &json!({"scaleStrokes": false})).unwrap();
            s.execute("selection.set", &json!({"ids": [id.0]})).unwrap();
            s.doc_mut().unwrap().selection.key = Some(id);
            s.execute("transform.move", &json!({"dx": 7, "dy": 0})).unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            s.set_tool("selection");
            let before = s.doc().unwrap().doc.clone();
            let selection = s.doc().unwrap().selection.clone();
            let history = s.doc().unwrap().history.clone();
            let journal = s.journal.clone();
            assert!(!s.prefs.scale_strokes);
            assert!(!selection.content);
            assert!(!history.redo.is_empty());
            let original = before.item(id).unwrap();
            assert!(original.flex.is_some());
            assert!(matches!(&original.content, designcraft_doc::Content::Group { items } if items.is_empty()));
            assert_rect(original.path.bounds().unwrap(), Rect::new(100.0, 100.0, 200.0, 150.0));
            assert_rect(original.bounds(), Rect::new(100.0, 100.0, 200.0, 150.0));
            assert_eq!(original.stroke.weight, 4.0);
            assert!((original.visible_bounds().width() - 104.0).abs() < 1e-6);
            let view = ViewInfo {
                zoom: 1.0,
                snap: SnapView { smart_guides: true, smart_dimensions: true, zone_px: 8.0, ..SnapView::OFF },
                unit: Unit::Points,
                stroke_in_front: false,
            };
            let layout = s.layout();
            let event = |kind, at: Point, mods| {
                let at = layout.to_canvas(SpreadRef::Doc(0), at);
                PointerEvent::new(kind, at.x, at.y).with_mods(mods)
            };
            let cmd = Mods { cmd: true, ..Mods::default() };
            s.pointer(&event(PointerKind::Down, Point::new(200.0, 150.0), cmd), view).unwrap();
            assert!(s.doc().unwrap().interaction.is_some());
            assert!(s.tool_busy());
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
            s.pointer(&event(PointerKind::Drag, Point::new(300.0, 200.0), cmd), view).unwrap();
            let scaled = s.doc().unwrap().doc.item(id).unwrap();
            assert_rect(scaled.path.bounds().unwrap(), Rect::new(100.0, 100.0, 200.0, 150.0));
            assert_rect(scaled.bounds(), Rect::new(100.0, 100.0, 300.0, 200.0));
            assert_eq!(scaled.stroke.weight, 2.0, "held Cmd must exercise the Flex group's temporary stroke compensation");
            assert!((scaled.visible_bounds().width() - 202.0).abs() < 1e-6);
            assert!(s.overlays(view).is_empty());
            assert_eq!(s.doc().unwrap().selection, selection);
            assert_resize_history(&s.doc().unwrap().history, &history);
            assert_eq!(s.journal, journal);

            // An ordinary Flex resize bakes only its frame. The first released-Cmd preview
            // must already agree with an identical repeated event, using the original stroke 4.
            let mut expected = (*before).clone();
            expected.item_mut(id).unwrap().path = designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 102.0, 150.0));
            for preview in 0..2 {
                s.pointer(&event(PointerKind::Drag, Point::new(103.0, 150.0), Mods::default()), view).unwrap();
                let item = s.doc().unwrap().doc.item(id).unwrap();
                assert!(
                    (item.visible_bounds().width() - 6.0).abs() < 1e-6,
                    "Flex Cmd-release preview {preview}: expected visible width 6, got {}",
                    item.visible_bounds().width()
                );
                assert_rect(item.path.bounds().unwrap(), Rect::new(100.0, 100.0, 102.0, 150.0));
                assert_rect(item.bounds(), Rect::new(100.0, 100.0, 102.0, 150.0));
                assert_eq!(item.stroke.weight, 4.0);
                assert_eq!(s.doc().unwrap().doc.as_ref(), &expected, "only the literal frame path may change");
                assert_eq!(s.doc().unwrap().selection, selection);
                assert!(s.doc().unwrap().interaction.is_some());
                assert!(s.tool_busy());
                assert_resize_history(&s.doc().unwrap().history, &history);
                assert_eq!(s.journal, journal);
                let overlays = s.overlays(view);
                assert_eq!(overlays.iter().filter(|o| matches!(o, designcraft_tools::Overlay::Measure { .. })).count(), 1);
                assert_eq!(overlays.iter().filter(|o| matches!(o, designcraft_tools::Overlay::Guide { .. })).count(), 1);
            }

            if cancel {
                assert!(s.tool_key(designcraft_tools::ToolKey::Escape, Mods::default(), view).unwrap());
                let assert_restored = |s: &mut Session| {
                    let st = s.doc().unwrap();
                    assert!(Arc::ptr_eq(&st.doc, &before));
                    assert_eq!(st.doc.as_ref(), before.as_ref());
                    assert_eq!(st.selection, selection);
                    assert_resize_history(&st.history, &history);
                    assert_eq!(s.journal, journal);
                    assert!(st.interaction.is_none());
                    assert!(!s.tool_busy());
                    assert!(s.overlays(view).is_empty());
                };
                assert_restored(&mut s);
                for kind in [PointerKind::Up, PointerKind::Drag, PointerKind::Move, PointerKind::Up] {
                    s.pointer(&event(kind, Point::new(103.0, 150.0), Mods::default()), view).unwrap();
                    assert_restored(&mut s);
                }
                // Cancellation keeps the pre-existing redo usable, not just its count.
                let redo = history.redo.last().unwrap().doc.clone();
                s.execute("edit.redo", &json!({})).unwrap();
                assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &redo));
                s.doc().unwrap().doc.check().unwrap();
                continue;
            }

            let after = s.doc().unwrap().doc.clone();
            s.pointer(&event(PointerKind::Up, Point::new(103.0, 150.0), Mods::default()), view).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
            assert_eq!(s.doc().unwrap().selection, selection);
            assert!(s.doc().unwrap().interaction.is_none());
            assert!(!s.tool_busy());
            assert!(s.overlays(view).is_empty());
            let mut committed_history = history.clone();
            committed_history.redo.clear();
            committed_history.undo.push(super::HistoryEntry { label: "Resize".into(), doc: before.clone(), selection: selection.clone() });
            assert_resize_history(&s.doc().unwrap().history, &committed_history);
            let mut committed_journal = journal;
            committed_journal.push((
                "transform.resize".into(),
                json!({"from": [100.0, 100.0, 200.0, 150.0], "to": [100.0, 100.0, 102.0, 150.0], "content": false, "distribute": false}),
            ));
            assert_eq!(s.journal, committed_journal);
            after.check().unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &before));
            assert_eq!(s.doc().unwrap().selection, selection);
            let mut undone_history = history;
            undone_history.redo = vec![super::HistoryEntry { label: "Resize".into(), doc: after.clone(), selection: selection.clone() }];
            assert_resize_history(&s.doc().unwrap().history, &undone_history);
            s.execute("edit.redo", &json!({})).unwrap();
            assert!(Arc::ptr_eq(&s.doc().unwrap().doc, &after));
            assert_eq!(s.doc().unwrap().doc.as_ref(), &expected);
            assert_eq!(s.doc().unwrap().selection, selection);
            assert_resize_history(&s.doc().unwrap().history, &committed_history);
        }
    }
}
