//! The cloud storage contract designcraft apps connect through.
//!
//! `storytold/craft-libs#160` adds a vendor-neutral `CloudBackend`, a
//! `ShareBackend`, and the Open, Save and Share pages that read them, to
//! `craft_ui::cloud`. Until that lands and the workspace's craft-libs pins
//! move to a revision that carries it, this crate states the shape of the
//! contract itself: [`CloudBackend`] and [`ShareBackend`] below.
//!
//! A host application that connects one gets an account note in the
//! application bar; connecting the other gets a Share control there. Neither
//! is drawn without a backend. No concrete provider lives in this crate, in
//! an app crate, or anywhere else in this repository: a build today wires no
//! backend at all, so nothing cloud-shaped is drawn anywhere.
//!
//! When craft-libs#160 merges, flipping the workspace's craft-ui and
//! craft-cloud pins and wiring the apps to the real `craft_ui::cloud` contract
//! replaces this module and its call sites in one commit.

use egui::Ui;

/// Where a document can be read from, and written back to, beyond this
/// computer. A host application supplies one through [`crate::DesignApp`];
/// without one, [`account_note`] draws nothing.
pub trait CloudBackend: Send + Sync {
    /// The signed-in account's display name, for the account note in the
    /// application bar. `None` before a sign-in has completed.
    fn account(&self) -> Option<String>;
}

/// Where a layout can be shared with somebody else. Kept apart from
/// [`CloudBackend`] because a host can offer one without the other: an
/// account that reads and writes files, but shares with nobody.
pub trait ShareBackend: Send + Sync {
    /// A short label for the Share control, naming who it shares through.
    fn label(&self) -> String;
}

/// Refused by every `cloud.*` command except `cloud.status` when no backend
/// is connected.
pub const UNAVAILABLE: &str = "no cloud service is configured";

/// Whether a cloud backend is connected, for the File menu's cloud entry and
/// the account note.
pub fn available(app: &crate::DesignApp) -> bool {
    app.cloud_backend.is_some()
}

/// Whether a share backend is connected, for the title bar's Share control.
pub fn shares(app: &crate::DesignApp) -> bool {
    app.share_backend.is_some()
}

/// The account note drawn in the application bar, beside the workspace
/// picker, when a cloud backend is connected and has reported who is signed
/// in. Draws nothing otherwise, including while a backend is connected but
/// has not yet answered.
pub fn account_note(app: &crate::DesignApp, ui: &mut Ui) {
    let Some(backend) = &app.cloud_backend else { return };
    let Some(name) = backend.account() else { return };
    ui.label(name);
}
