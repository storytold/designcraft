use super::{DesignApp, Value, canvas, json};

impl DesignApp {
    /// Paste without the clipboard's text (a menu, not the paste keys): read it here. With no
    /// insertion point, text that doesn't stand for copied objects becomes a new text frame
    /// centred in the view.
    pub(super) fn with_system_clipboard(&mut self, id: &str, mut params: Value) -> Value {
        let typing = self.session.active().is_some_and(|d| d.selection.text.is_some());
        if !matches!(id, "edit.paste" | "edit.pasteWithoutFormatting") || !(typing || id == "edit.paste") {
            return params;
        }
        if params.get("text").is_none()
            && let Some(text) = self.services.clipboard_text.as_mut().and_then(|f| f())
            && let Some(o) = params.as_object_mut()
        {
            o.insert("text".into(), Value::String(text));
        }
        // Still what an object copy put there: paste those objects, not the placeholder text.
        let norm = |t: &str| t.replace("\r\n", "\n");
        if let Some(copied) = &self.object_clip_text
            && params.get("text").and_then(Value::as_str).is_some_and(|t| norm(t) == norm(copied))
            && let Some(o) = params.as_object_mut()
        {
            o.remove("text");
        }
        if !typing
            && params.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty())
            && let Some((spread, at)) = canvas::view_centre(self)
            && let Ok(spread) = serde_json::to_value(spread)
            && let Some(o) = params.as_object_mut()
        {
            o.entry("spread").or_insert(spread);
            o.entry("at").or_insert(json!([at.x, at.y]));
        }
        params
    }

    /// Keep widget clipboard output private while a queued control event is running.
    pub(crate) fn install_clipboard_guard(&mut self, ctx: &egui::Context) {
        if self.clipboard_guard_installed {
            return;
        }
        let private = self.control_input_frame.clone();
        ctx.on_end_pass(
            "control clipboard",
            std::sync::Arc::new(move |ui| {
                if private.load(std::sync::atomic::Ordering::Relaxed) {
                    ui.ctx().output_mut(|o| {
                        o.commands.retain(|cmd| !matches!(cmd, egui::OutputCommand::CopyText(_) | egui::OutputCommand::CopyImage(_)))
                    });
                }
            }),
        );
        self.clipboard_guard_installed = true;
    }
}
