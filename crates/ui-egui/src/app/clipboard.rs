use super::{DesignApp, Value, canvas, json};
use crate::services::ClipboardImage;

/// What Paste takes from the system clipboard.
pub(super) enum Pasted {
    /// The command's params, with the clipboard's text when it has some.
    Params(Value),
    /// An image, pasted as a placed image instead.
    Image(ClipboardImage),
}

/// Text that is only the address of a file or a web page: what a browser puts beside a copied
/// image. Paste prefers the image to such text.
fn is_bare_address(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty()
        && !t.contains(char::is_whitespace)
        && ["http://", "https://", "file://", "data:image/"]
            .iter()
            .any(|scheme| t.get(..scheme.len()).is_some_and(|s| s.eq_ignore_ascii_case(scheme)))
}

impl DesignApp {
    /// Paste without the clipboard's text (a menu, not the paste keys): read it here. With no
    /// insertion point, text that doesn't stand for copied objects becomes a new text frame
    /// centered in the view, and an image on the clipboard (when there is no text, or the text is
    /// only an address) becomes a placed image, unless nothing was copied to the clipboard since
    /// objects were last copied here.
    pub(super) fn with_system_clipboard(&mut self, id: &str, mut params: Value) -> Pasted {
        let typing = self.session.active().is_some_and(|d| d.selection.text.is_some());
        if !matches!(id, "edit.paste" | "edit.pasteWithoutFormatting") || !(typing || id == "edit.paste") {
            return Pasted::Params(params);
        }
        // Empty text counts as no text, so an image beside it still pastes.
        if params.get("text").is_none()
            && let Some(text) = self.services.clipboard_text.as_mut().and_then(|f| f()).filter(|t| !t.is_empty())
            && let Some(o) = params.as_object_mut()
        {
            o.insert("text".into(), Value::String(text));
        }
        let unchanged = self.clipboard_unchanged_since_object_copy();
        // Still what an object copy put there: paste those objects, not the placeholder text.
        let norm = |t: &str| t.replace("\r\n", "\n");
        let ours = unchanged
            && self
                .object_clip_text
                .as_ref()
                .is_some_and(|copied| params.get("text").and_then(Value::as_str).is_some_and(|t| norm(t) == norm(copied)));
        if ours && let Some(o) = params.as_object_mut() {
            o.remove("text");
        }
        let text = params.get("text").and_then(Value::as_str);
        let copied_here =
            text.is_none() && self.session.clipboard.is_some() && self.object_clip_text.is_none() && self.object_clip_change.is_some() && unchanged;
        if !typing
            && !ours
            && !copied_here
            && text.is_none_or(|t| t.trim().is_empty() || is_bare_address(t))
            && let Some(image) = self.services.clipboard_image.as_mut().and_then(|f| f())
        {
            return Pasted::Image(image);
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
        Pasted::Params(params)
    }

    /// Nothing was copied to the clipboard since objects were copied here (native menu, where
    /// the system raises a change count with every copy).
    fn clipboard_unchanged_since_object_copy(&mut self) -> bool {
        match self.object_clip_change {
            Some(at_copy) => self.services.clipboard_change.as_mut().and_then(|f| f()) == Some(at_copy),
            None => true,
        }
    }

    /// Paste an image from the system clipboard as a placed image (`edit.pasteImage`), centered
    /// in the view or in the selected empty frame. Text being edited takes no image. An image the
    /// clipboard held before objects were copied here (native menu) is not pasted: the menu's
    /// Paste pastes those objects.
    pub(crate) fn paste_clipboard_image(&mut self, image: ClipboardImage) -> Result<Value, String> {
        let user = !self.in_control && !self.control_input_frame.load(std::sync::atomic::Ordering::Relaxed);
        if user && self.session.clipboard.is_some() && self.object_clip_change.is_some() && self.clipboard_unchanged_since_object_copy() {
            return Ok(Value::Null);
        }
        let typing = self.session.active().is_some_and(|d| d.selection.text.is_some());
        let file = if typing {
            Err("To paste the image, stop editing the text first (Esc).".to_string())
        } else {
            match image {
                ClipboardImage::Pixels { width, height, rgba } => {
                    designcraft_engine::cmd::pasted_png(width, height, rgba).map(|png| (png, designcraft_engine::cmd::PASTED_IMAGE_NAME.to_string()))
                }
                ClipboardImage::File { name, bytes } => Ok((bytes, name)),
            }
        };
        let (bytes, name) = match file {
            Ok(file) => file,
            Err(e) => {
                self.status(e.clone());
                return Err(e);
            }
        };
        let mut params = json!({"base64": designcraft_engine::cmd::base64_encode(&bytes), "name": name});
        if let Some((spread, at)) = canvas::view_centre(self)
            && let Ok(spread) = serde_json::to_value(spread)
            && let Some(o) = params.as_object_mut()
        {
            o.insert("spread".into(), spread);
            o.insert("at".into(), json!([at.x, at.y]));
        }
        self.run("edit.pasteImage", params)
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
