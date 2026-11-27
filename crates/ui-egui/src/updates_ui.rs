//! Software updates, background check at startup, and startup prompt modal.

use serde_json::Value;

use crate::DesignApp;

#[cfg(not(target_arch = "wasm32"))]
use craft_update::updater::Status;
#[cfg(not(target_arch = "wasm32"))]
use craft_update::updater::{Action, Updater};
#[cfg(not(target_arch = "wasm32"))]
use craft_update::{CheckResultDialog, CheckResultResponse};

pub(crate) struct Runtime {
    #[cfg(not(target_arch = "wasm32"))]
    updater: Updater,
    /// The desktop app turns the check at start on; tests and the web build leave it off.
    #[cfg(not(target_arch = "wasm32"))]
    startup_check: bool,
    #[cfg(not(target_arch = "wasm32"))]
    check_requested: bool,
    /// The context of the last frame, to wake the UI when a check is queued from a menu callback.
    #[cfg(not(target_arch = "wasm32"))]
    repaint: Option<egui::Context>,
    /// The web build has no updater; its Help ▸ Check for Updates opens a window with a fixed message.
    #[cfg(target_arch = "wasm32")]
    pub(crate) manual_window_open: bool,
    /// A copy whose updates another program installs (a Launcher Nightly): Help ▸ Check for Updates
    /// says so, and the updater never runs or offers a build that would replace this channel.
    #[cfg(not(target_arch = "wasm32"))]
    managed: Option<&'static str>,
    /// The managed-copy message window is open.
    #[cfg(not(target_arch = "wasm32"))]
    managed_window_open: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            updater: Updater::new(&crate::UPDATE_APP, env!("CARGO_PKG_VERSION")),
            #[cfg(not(target_arch = "wasm32"))]
            startup_check: false,
            #[cfg(not(target_arch = "wasm32"))]
            check_requested: false,
            #[cfg(not(target_arch = "wasm32"))]
            repaint: None,
            #[cfg(target_arch = "wasm32")]
            manual_window_open: false,
            #[cfg(not(target_arch = "wasm32"))]
            managed: None,
            #[cfg(not(target_arch = "wasm32"))]
            managed_window_open: false,
        }
    }
}

impl DesignApp {
    /// Whether the app asks for a newer release when it starts (off until the desktop app turns it on).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_startup_update_check(&mut self, on: bool) {
        self.updates_rt.startup_check = on;
    }

    /// This copy's updates are installed by `who` (the Launcher for a Nightly): no check, no offer,
    /// and Help ▸ Check for Updates explains who updates it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_updates_managed(&mut self, who: Option<&'static str>) {
        self.updates_rt.managed = who;
        if who.is_some() {
            self.updates_rt.startup_check = false;
            self.updates_rt.check_requested = false;
        }
    }

    /// Whether another program installs this copy's updates.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn updates_managed(&self) -> bool {
        self.updates_rt.managed.is_some()
    }
}

pub(crate) fn check(app: &mut DesignApp) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if app.updates_rt.managed.is_some() {
            // Explain the Launcher instead of offering a build that would replace this channel.
            app.updates_rt.managed_window_open = true;
            if let Some(ctx) = &app.updates_rt.repaint {
                ctx.request_repaint();
            }
            return Ok(Value::Null);
        }
        app.updates_rt.check_requested = true;
        if let Some(ctx) = &app.updates_rt.repaint {
            ctx.request_repaint();
        }
        Ok(Value::Null)
    }
    #[cfg(target_arch = "wasm32")]
    {
        app.updates_rt.manual_window_open = true;
        Ok(Value::Null)
    }
}

pub(crate) fn tick(app: &mut DesignApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let rt = &mut app.updates_rt;
        rt.repaint = Some(ctx.clone());
        if rt.managed.is_some() {
            // Never tick a managed copy's updater: it would offer the stable channel's build.
            return;
        }
        if std::mem::take(&mut rt.check_requested) {
            rt.updater.check_now(ctx);
        } else if rt.startup_check {
            rt.updater.check_at_start(ctx);
        }
        rt.updater.tick(ctx);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}

/// Whether the check result window shows: for every status but idle (a requested check, and the
/// first start after an update, which reports that it installed or why it failed), and never over
/// the startup prompt.
#[cfg(not(target_arch = "wasm32"))]
fn result_window_shows(status: &Status, prompt_open: bool) -> bool {
    !prompt_open && !matches!(status, Status::Idle)
}

pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(who) = app.updates_rt.managed
            && app.updates_rt.managed_window_open
        {
            let mut open = true;
            let title = crate::i18n::tr(&app.ui.language, "DesignCraft Updates");
            let ok_label = crate::i18n::tr(&app.ui.language, "OK");
            egui::Window::new(title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    ui.label(format!("Updates are managed by {who}."));
                    ui.add_space(8.0);
                    if ui.button(ok_label).clicked() {
                        app.updates_rt.managed_window_open = false;
                    }
                });
            if !open {
                app.updates_rt.managed_window_open = false;
            }
        }
        if app.updates_rt.managed.is_some() {
            return;
        }
        // The updater asks to close only when no document has unsaved changes; the install starts when the app exits.
        let clean = !app.session.documents().iter().any(|d| d.is_dirty());
        match app.updates_rt.updater.show_if(ctx, clean) {
            Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::None => {}
        }
        // Help ▸ Check for Updates answers in the shared dialog, which follows the updater's status.
        if result_window_shows(app.updates_rt.updater.status(), app.updates_rt.updater.prompt_open()) {
            let dialog = CheckResultDialog::new(&crate::UPDATE_APP, env!("CARGO_PKG_VERSION"));
            match dialog.show(ctx, app.updates_rt.updater.status()) {
                CheckResultResponse::None => {}
                CheckResultResponse::Close => app.updates_rt.updater.dismiss_status(),
                CheckResultResponse::CheckAgain => app.updates_rt.check_requested = true,
                // The next `show_if` asks the window to close once no document has unsaved changes.
                CheckResultResponse::RestartToUpdate => app.updates_rt.updater.restart_to_update(),
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    if app.updates_rt.manual_window_open {
        let mut open = true;
        let title = crate::i18n::tr(&app.ui.language, "DesignCraft Updates");
        let ok_label = crate::i18n::tr(&app.ui.language, "OK");
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(crate::i18n::tr(&app.ui.language, "Update check is not supported in the web browser."));
                ui.add_space(8.0);
                if ui.button(ok_label).clicked() {
                    app.updates_rt.manual_window_open = false;
                }
            });
        if !open {
            app.updates_rt.manual_window_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_engine::Session;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_managed_copy_never_checks_and_explains_who_updates_it() {
        let mut app = DesignApp::new(Session::new(), Default::default());
        app.set_startup_update_check(true);
        app.set_updates_managed(Some("ArtCraft Launcher"));
        assert!(app.updates_managed());
        check(&mut app).unwrap();
        assert!(app.updates_rt.managed_window_open, "Help ▸ Check for Updates explains the Launcher");
        assert!(!app.updates_rt.check_requested, "a managed copy must not ask the stable feed");
        tick(&mut app, &egui::Context::default());
        assert!(!app.updates_rt.startup_check, "a managed copy never checks at start");
        app.set_updates_managed(None);
        assert!(!app.updates_managed());
    }

    #[test]
    fn default_runtime_is_idle() {
        let rt = Runtime::default();
        #[cfg(target_arch = "wasm32")]
        assert!(!rt.manual_window_open);
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert!(!rt.startup_check);
            assert!(!rt.updater.prompt_open());
            assert_eq!(rt.updater.status(), &Status::Idle);
        }
    }

    /// Offers `release` and returns once its prompt is open. The updater reads the installed files
    /// for an offer on a thread of its own. That thread sends its answer and then asks the context
    /// the updater keeps for an immediate repaint, and the next tick opens the prompt. The test
    /// fails when the offer opens no prompt, for example when the launcher manages this copy.
    #[cfg(not(target_arch = "wasm32"))]
    fn offer_and_wait(updater: &mut Updater, release: craft_update::Release) {
        let ctx = egui::Context::default();
        let (answered, wait) = std::sync::mpsc::channel();
        ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                let _ = answered.send(());
            }
        });
        // Keeps `ctx` for the offer's thread.
        updater.tick(&ctx);
        updater.offer(release);
        assert!(updater.managed().is_none(), "the launcher manages this copy, so it ignored the offer");
        // Without its thread, the offer opens the prompt at once.
        if !updater.prompt_open() {
            wait.recv().unwrap();
            updater.tick(&ctx);
        }
        assert!(updater.prompt_open(), "the offer opened no prompt (launcher choice: {:?})", updater.managed());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn startup_prompt_shows_and_closes() {
        let mut app = DesignApp::new(Session::new(), Default::default());
        let ctx = egui::Context::default();
        let release = craft_update::Release {
            tag: "v99.0.0".into(),
            version: craft_update::parse_version("99.0.0"),
            name: "DesignCraft 99.0.0".into(),
            html_url: "https://github.com/storytold/designcraft/releases/tag/v99.0.0".into(),
            ..Default::default()
        };
        offer_and_wait(&mut app.updates_rt.updater, release);

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(&mut app, ui.ctx());
        });
        output.textures_delta.clear();
        assert!(app.updates_rt.updater.prompt_open());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn result_window_shows_every_status_but_idle() {
        let shown = [
            Status::Checking,
            Status::UpToDate,
            Status::Failed("no network".into()),
            Status::Installing,
            Status::Ready,
            Status::Managed,
            Status::Updated("0.2.0".into()),
            Status::InstallFailed("disk full".into()),
        ];
        for status in &shown {
            assert!(result_window_shows(status, false), "{status:?}");
            assert!(!result_window_shows(status, true), "{status:?} over the prompt");
        }
        assert!(!result_window_shows(&Status::Idle, false));
    }
}
