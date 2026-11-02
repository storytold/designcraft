use super::{DesignApp, LivePreview, Value, View, ViewInfo, json, page_history};

impl DesignApp {
    /// The view key of the active document in the current pane.
    pub(crate) fn view_key(&self) -> Option<u64> {
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
}
