use super::DesignApp;

impl DesignApp {
    /// Applies `ui.shortcuts` to the keymap when they differ, e.g. after preferences were loaded,
    /// and rebuilds it when the shortcut platform changed (the web build learns the browser's
    /// operating system on the first frame). Outside the Keyboard Shortcuts dialog the active
    /// shortcut set takes `ui.shortcuts` too: an earlier version may have changed them.
    pub fn sync_keymap(&mut self) {
        if self.shortcut_dialog.is_none() {
            crate::keys::store_in_set(&mut self.ui);
        }
        if self.keymap.platform() != crate::keys::platform() {
            self.keymap = crate::keys::new_keymap(self.ui.shortcuts.clone());
        } else if *self.keymap.overrides() != self.ui.shortcuts {
            self.keymap.set_overrides(self.ui.shortcuts.clone());
        }
    }

    /// Whether the Keyboard Shortcuts dialog is recording keys: a native menu bar pauses its key
    /// equivalents meanwhile, so the keys reach the dialog.
    pub fn shortcut_recording(&self) -> bool {
        self.shortcut_dialog.as_ref().is_some_and(|d| d.recording())
    }
}
