use super::{DesignApp, UiRequest, Value, canvas, dialogs, json, menus};

impl DesignApp {
    /// THE entry point for every action (menus, shortcuts, palette, panels, control channel).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if matches!(id, "app.pdfExportStatus" | "app.cancelPdfExport") {
            designcraft_engine::check_keys(id, "{operationId?}", &params).map_err(|e| e.to_string())?;
        }
        if id == "app.pdfExportStatus" {
            if let Some(value) = params.get("operationId") {
                let id = value.as_u64().filter(|id| *id != 0).ok_or("operationId must be a positive integer")?;
                if self.pdf_export.status().operation_id != Some(id) {
                    return Err("Unknown PDF export operation".into());
                }
            }
            return serde_json::to_value(self.pdf_export.status()).map_err(|e| e.to_string());
        }
        if id == "app.cancelPdfExport" {
            if let Some(value) = params.get("operationId") {
                let id = value.as_u64().filter(|id| *id != 0).ok_or("operationId must be a positive integer")?;
                if self.pdf_export.status().operation_id != Some(id) {
                    return Err("Unknown PDF export operation".into());
                }
            }
            if !self.pdf_export.cancel() {
                return Err("PDF export is not cancellable; saving may have started".into());
            }
            self.quit_after_export = false;
            self.status("PDF export cancelled");
            return serde_json::to_value(self.pdf_export.status()).map_err(|e| e.to_string());
        }
        if self.pdf_export.busy()
            && !matches!(
                id,
                "document.inspect"
                    | "view.zoomIn"
                    | "view.zoomOut"
                    | "view.zoom"
                    | "view.fitPage"
                    | "view.fitSpread"
                    | "view.fitSelection"
                    | "view.actualSize"
                    | "view.entirePasteboard"
            )
        {
            if id == "app.quit" {
                self.quit_after_export = true;
                self.status("Quit queued until PDF export finishes");
                return Ok(self.pdf_export.queued());
            }
            return Err("Please wait for the PDF export to finish".into());
        }
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

    /// Handle requests produced by tools/commands (dialogs, view changes, file pickers).
    pub fn after_engine(&mut self) {
        let reqs = std::mem::take(&mut self.session.ui_requests);
        for r in reqs {
            let was_find = self.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange");
            match r {
                UiRequest::Dialog { id, params } if id == "colorTheme" => dialogs::open_color_theme(self, params),
                UiRequest::Dialog { id, params } if id == "frameSize" => self.ui.dialog = Some(dialogs::frame_size_dialog(self, params)),
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

    pub fn select_tool(&mut self, id: &str) {
        let _ = self.run("tool.select", json!({"tool": id}));
        self.ui.flyout = None;
    }
}
