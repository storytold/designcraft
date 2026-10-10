//! Tool hosting: the active tool receives pointer/key events and its actions are executed here.

use std::sync::Arc;

use designcraft_geom::Unit;
pub use designcraft_tools::SnapView;
use designcraft_tools::{Action, CanvasLayout, Cursor, Mods, Overlay, PointerEvent, PointerKind, ToolContext, ToolKey};
use serde::Serialize;
use serde_json::Value;

use crate::{EngineError, HistoryEntry, Interaction, Result, Session, record_undo};

/// Something the UI should do in response to a tool or command.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UiRequest {
    Dialog {
        id: String,
        params: Value,
    },
    View {
        params: Value,
    },
    /// Open a file picker for the given purpose (`place`, `open`, `saveAs`, `export`).
    Pick {
        purpose: String,
        params: Value,
    },
}

/// View information the UI passes with pointer events.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ViewInfo {
    /// Screen pixels per point.
    pub zoom: f64,
    pub snap: SnapView,
    pub unit: Unit,
}

impl ViewInfo {
    pub const fn at_zoom(zoom: f64) -> Self {
        Self { zoom, snap: SnapView::FACTORY, unit: Unit::Points }
    }
}

impl Default for ViewInfo {
    fn default() -> Self {
        Self::at_zoom(1.0)
    }
}

impl Session {
    pub fn tool_id(&self) -> &'static str {
        self.tool.id()
    }

    pub fn set_tool(&mut self, id: &str) {
        if self.tool.id() == id {
            return;
        }
        // A composition ends with the tool that shows it: its marked text stays as typed.
        if let Err(e) = self.end_composition() {
            log::warn!("ending the IME composition: {e}");
        }
        self.tool = designcraft_tools::create(id);
        // Leaving the Type tool keeps the text frame selected.
        if id != "type"
            && let Some(st) = self.active_mut()
            && let Some(t) = st.selection.text.take()
            && let Some(f) = t.frame.or_else(|| st.doc.story(t.story).and_then(|s| s.frames.first().copied()))
        {
            st.selection.items = vec![f];
            st.selection.cells = None;
        }
    }

    pub fn layout(&self) -> CanvasLayout {
        match self.active() {
            Some(st) => CanvasLayout::new(&st.doc, st.editing_parents),
            None => CanvasLayout::default(),
        }
    }

    fn with_ctx<T>(&mut self, view: ViewInfo, f: impl FnOnce(&mut dyn designcraft_tools::Tool, &ToolContext) -> T) -> Option<T> {
        let st = self.active()?;
        let doc = st.doc.clone();
        let sel = st.selection.clone();
        let layer = st.active_layer;
        let layout = CanvasLayout::new(&doc, st.editing_parents);
        let cache = self.cache.clone();
        let cx = ToolContext {
            doc: &doc,
            selection: &sel,
            cache: &cache,
            layout: &layout,
            zoom: view.zoom.max(1e-6),
            layer,
            snap: view.snap,
            unit: view.unit,
        };
        Some(f(self.tool.as_mut(), &cx))
    }

    pub fn pointer(&mut self, ev: &PointerEvent, view: ViewInfo) -> Result<()> {
        // A press ends a composition, keeping its marked text as typed (like clicking away from
        // one in a native text view); the UI tells the system IME to drop it.
        if matches!(ev.kind, PointerKind::Down | PointerKind::DoubleClick) {
            self.end_composition()?;
        }
        let actions = self.with_ctx(view, |t, cx| t.pointer(cx, ev)).unwrap_or_default();
        self.run_actions(actions)
    }

    pub fn tool_key(&mut self, key: ToolKey, mods: Mods, view: ViewInfo) -> Result<bool> {
        // Keys belong to the IME while it composes: nothing else (Escape's deselect) takes them.
        if self.tool.composing() {
            return Ok(true);
        }
        let actions = self.with_ctx(view, |t, cx| t.key(cx, key, mods)).unwrap_or_default();
        let handled = !actions.is_empty();
        self.run_actions(actions)?;
        Ok(handled)
    }

    pub fn overlays(&mut self, view: ViewInfo) -> Vec<Overlay> {
        self.with_ctx(view, |t, cx| t.overlays(cx)).unwrap_or_default()
    }

    pub fn cursor(&mut self, p: designcraft_geom::Point, mods: Mods, view: ViewInfo) -> Cursor {
        self.with_ctx(view, |t, cx| t.cursor(cx, p, mods)).unwrap_or_default()
    }

    pub fn tool_busy(&self) -> bool {
        self.tool.busy()
    }

    pub fn wants_text(&mut self) -> bool {
        self.with_ctx(ViewInfo::at_zoom(1.0), |t, cx| t.wants_text(cx)).unwrap_or(false)
    }

    /// IME marked text at the text caret (see `Tool::ime_preedit`; `active_chars` counts
    /// characters of `text`). It shows in place as a preview: no undo step, nothing journaled.
    pub fn tool_preedit(&mut self, text: &str, active_chars: Option<std::ops::Range<usize>>, view: ViewInfo) -> Result<()> {
        let actions = self.with_ctx(view, |t, cx| t.ime_preedit(cx, text, active_chars)).unwrap_or_default();
        let r = self.run_actions(actions);
        if r.is_err() && self.tool.composing() {
            // The marked text can't show (its story went away): the composition ends with nothing
            // typed, so no gesture stays open.
            match self.with_ctx(view, |t, cx| t.ime_preedit(cx, "", None)) {
                Some(cancel) => self.run_actions(cancel)?,
                None => self.tool = designcraft_tools::create(self.tool.id()),
            }
        }
        r
    }

    /// The IME committed `text`: it replaces the marked text as one `text.insert` (one undo step,
    /// one journaled command).
    pub fn tool_ime_commit(&mut self, text: &str, view: ViewInfo) -> Result<()> {
        let actions = self.with_ctx(view, |t, cx| t.ime_commit(cx, text)).unwrap_or_default();
        self.run_actions(actions)
    }

    /// Is the active tool showing uncommitted IME text?
    pub fn tool_composing(&self) -> bool {
        self.tool.composing()
    }

    /// End an IME composition from outside the IME (a click, a tool switch, another command): the
    /// marked text stays as typed, as one undo step.
    pub fn end_composition(&mut self) -> Result<()> {
        if !self.tool.composing() {
            return Ok(());
        }
        match self.with_ctx(ViewInfo::default(), |t, cx| t.ime_end(cx)) {
            Some(actions) => self.run_actions(actions),
            None => {
                // No document to type into: only the tool's state goes.
                self.tool = designcraft_tools::create(self.tool.id());
                Ok(())
            }
        }
    }

    /// The caret line (canvas space) the IME candidate window follows.
    pub fn tool_ime_caret(&mut self, view: ViewInfo) -> Option<(designcraft_geom::Point, designcraft_geom::Point)> {
        self.with_ctx(view, |t, cx| t.ime_caret(cx)).flatten()
    }

    pub fn run_actions(&mut self, actions: Vec<Action>) -> Result<()> {
        for a in actions {
            match a {
                Action::Begin(label) => self.begin_interaction(&label)?,
                Action::Preview(cmd, p) => self.preview(&cmd, p)?,
                Action::Commit => self.commit_interaction()?,
                Action::Cancel => self.cancel_interaction(),
                Action::Exec(cmd, p) => {
                    self.execute(&cmd, &p)?;
                }
                Action::Dialog(id, params) => self.ui_requests.push(UiRequest::Dialog { id, params }),
                Action::SwitchTool(id) => self.set_tool(&id),
                Action::View(params) => self.ui_requests.push(UiRequest::View { params }),
            }
        }
        Ok(())
    }

    pub fn begin_interaction(&mut self, label: &str) -> Result<()> {
        let st = self.doc_mut()?;
        st.interaction =
            Some(Interaction { label: label.into(), doc: st.doc.clone(), selection: st.selection.clone(), preview: None, revision: st.revision });
        Ok(())
    }

    /// Apply `cmd` on top of the interaction's snapshot (replacing the previous preview).
    pub fn preview(&mut self, cmd: &str, params: Value) -> Result<()> {
        let st = self.doc_mut()?;
        let Some(it) = st.interaction.clone() else { return Err(EngineError::Other("no interaction".into())) };
        st.doc = it.doc.clone();
        st.selection = it.selection.clone();
        // Not through `execute`: a preview is part of the gesture (an IME composition included),
        // not a command that ends it.
        let r = self.guarded(cmd, |s| s.execute_unguarded(cmd, &params));
        if let Some(st) = self.active_mut()
            && let Some(i) = st.interaction.as_mut()
        {
            i.preview = Some((cmd.to_string(), params));
        }
        // Journal only the committed result.
        if r.is_ok() {
            self.journal.pop();
        }
        r.map(|_| ())
    }

    pub fn commit_interaction(&mut self) -> Result<()> {
        let st = self.doc_mut()?;
        let Some(it) = st.interaction.take() else { return Ok(()) };
        if !Arc::ptr_eq(&it.doc, &st.doc) {
            record_undo(st, HistoryEntry { label: it.label, doc: it.doc, selection: it.selection });
        }
        if let Some((c, p)) = it.preview {
            self.journal.push((c, p));
        }
        // The interaction's last preview is the transform to repeat; the next one starts afresh.
        self.transforms.2 = false;
        Ok(())
    }

    pub fn cancel_interaction(&mut self) {
        if let Some(st) = self.active_mut()
            && let Some(it) = st.interaction.take()
        {
            // Nothing changed: the document goes back to the state and revision it began with.
            st.doc = it.doc;
            st.selection = it.selection;
            st.revision = it.revision;
            if self.transforms.2 {
                self.transforms.0.pop();
                self.transforms.2 = false;
            }
        }
    }
}
