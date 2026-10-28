use super::*;

impl DesignApp {
    /// Quit: close the window the way its close button does, so unsaved documents ask first.
    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    /// A native host may finish an accepted quit before destroying its platform window.
    /// A graphics failure or a cancelled/pending unsaved-document question is not a quit.
    pub fn native_quit_approved(&self) -> bool {
        self.native_close_approved || self.quit_without_asking
    }

    /// Ask about unsaved documents without closing any of them yet. A later Cancel must also
    /// keep documents whose earlier answer was Don't Save, including their history and recovery.
    pub(crate) fn handle_quit(&mut self, ctx: &egui::Context) {
        self.native_close_approved = false;
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
                self.native_close_approved = true;
                return;
            }
            if self.quitting.is_none() && !self.session.documents().iter().any(|d| d.is_dirty()) {
                self.native_close_approved = true;
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
                } else {
                    self.native_close_approved = true;
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
}
