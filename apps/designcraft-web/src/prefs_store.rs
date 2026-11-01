//! Preferences kept in the browser's local storage, as the desktop app keeps `prefs.json`: the
//! Preferences dialog's settings (Save documents as among them) survive a reload.

use designcraft_engine::Prefs;

/// The local storage key.
pub(crate) const KEY: &str = "designcraft.prefs";

/// Seconds between checks for changed preferences.
const CHECK_EVERY: f64 = 1.0;

/// The preferences stored under [`KEY`]; `None` when there are none or they can't be read, and
/// the defaults apply. Fields a stored copy lacks (from an older version) take their defaults.
pub(crate) fn decode(stored: Option<&str>) -> Option<Prefs> {
    serde_json::from_str(stored?).ok()
}

/// Decides when to write the preferences back: when they differ from what was last stored, at
/// most once per [`CHECK_EVERY`] seconds of app time.
pub(crate) struct PrefsSync {
    /// The JSON last read from or written to storage.
    last: Option<String>,
    /// App time of the next check.
    next_check: f64,
}

impl PrefsSync {
    /// Starts from what storage held at startup, so unchanged preferences aren't written back.
    pub(crate) fn new(stored: Option<String>) -> Self {
        PrefsSync { last: stored, next_check: 0.0 }
    }

    /// Whether to check now (app time `now`, in seconds); the next check is a second later.
    pub(crate) fn due(&mut self, now: f64) -> bool {
        if !now.is_finite() || now < self.next_check {
            return false;
        }
        self.next_check = now + CHECK_EVERY;
        true
    }

    /// Seconds from `now` until the next check, when one is still to come; `None` when a check is
    /// already due (or `now` isn't a number). The shell asks for a frame then: egui draws no frame
    /// just because time passes, so a change made between checks would otherwise wait for the
    /// next input before it's stored.
    pub(crate) fn wait(&self, now: f64) -> Option<f64> {
        (now.is_finite() && now < self.next_check).then_some(self.next_check - now)
    }

    /// The JSON to store when `prefs` differ from what was last stored; `None` otherwise. Nothing
    /// counts as stored until [`PrefsSync::stored`] says the write succeeded, so a write that
    /// fails (or storage that isn't available) is tried again at the next check.
    pub(crate) fn changed(&self, prefs: &Prefs) -> Option<String> {
        let json = serde_json::to_string(prefs).ok()?;
        (self.last.as_deref() != Some(json.as_str())).then_some(json)
    }

    /// `json` (from [`PrefsSync::changed`]) was written to storage.
    pub(crate) fn stored(&mut self, json: String) {
        self.last = Some(json);
    }
}

#[cfg(test)]
mod tests {
    use super::{KEY, PrefsSync, decode};
    use designcraft_engine::{Prefs, SaveFormat};

    #[test]
    fn stored_preferences_come_back_and_bad_ones_leave_the_defaults() {
        assert!(!KEY.is_empty());
        assert!(decode(None).is_none(), "nothing stored: defaults");
        assert!(decode(Some("not json")).is_none(), "unreadable: defaults");
        let old = decode(Some(r#"{"linkTextFiles": true}"#)).unwrap();
        assert!(old.link_text_files);
        assert_eq!(old.save_format, SaveFormat::Designcraft, "a field an older copy lacks takes its default");
        let prefs = Prefs { save_format: SaveFormat::Idml, ..Prefs::default() };
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(decode(Some(&json)).unwrap().save_format, SaveFormat::Idml);
    }

    #[test]
    fn preferences_are_written_only_when_they_change() {
        let prefs = Prefs::default();
        let stored = serde_json::to_string(&prefs).unwrap();
        let mut sync = PrefsSync::new(Some(stored));
        assert_eq!(sync.changed(&prefs), None, "unchanged since startup: nothing to write");
        let idml = Prefs { save_format: SaveFormat::Idml, ..Prefs::default() };
        let json = sync.changed(&idml).unwrap();
        assert!(json.contains(r#""saveFormat":"idml""#), "{json}");
        // The write failed (or storage was unavailable): still pending at the next check.
        assert_eq!(sync.changed(&idml).as_deref(), Some(json.as_str()), "retried until stored");
        sync.stored(json);
        assert_eq!(sync.changed(&idml), None, "written once");
        // Nothing stored yet: the first check writes.
        assert!(PrefsSync::new(None).changed(&prefs).is_some());
    }

    #[test]
    fn checks_come_at_most_once_a_second() {
        let mut sync = PrefsSync::new(None);
        assert!(sync.due(0.0));
        assert!(!sync.due(0.5));
        assert!(sync.due(1.0));
        assert!(!sync.due(f64::NAN), "a time that isn't a number is never due");
    }

    /// A check that isn't due yet asks for a frame at its time; a due one doesn't need to.
    #[test]
    fn a_check_not_yet_due_says_when_it_will_be() {
        let mut sync = PrefsSync::new(None);
        assert_eq!(sync.wait(0.0), None, "due at once");
        assert!(sync.due(0.0));
        assert_eq!(sync.wait(0.25), Some(0.75));
        assert_eq!(sync.wait(1.0), None);
        assert_eq!(sync.wait(f64::NAN), None);
    }
}
