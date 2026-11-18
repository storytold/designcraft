use super::DesignApp;

impl DesignApp {
    /// Did an IME composition end outside the IME this frame (a click, a tool switch)? egui's
    /// `should_interrupt_composition` toggles winit's IME, which doesn't reach the macOS input
    /// context: it keeps the marked text and types it again into the next composition, so the
    /// host discards it there.
    pub fn take_ime_discard(&mut self) -> bool {
        std::mem::take(&mut self.ime_discard)
    }

    /// Did the canvas ask for the system IME in the last frame (a Type tool caret)? The web
    /// build's hidden text input has the keyboard then, so text events hold what the browser
    /// typed (see [`drop_key_name_text`]).
    pub fn canvas_ime(&self) -> bool {
        self.canvas_ime
    }

    /// The second window uses a stable viewport id; forget its IME context when it closes.
    pub(crate) fn end_second_ime_viewport(&mut self) {
        self.ime_tracker.end_viewport(egui::ViewportId::from_hash_of("designcraft_second_window"));
    }
}
