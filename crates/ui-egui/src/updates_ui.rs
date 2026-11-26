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
            let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_string();
            let message = tr("Updates are managed by {who}.").replace("{who}", who);
            app.updates_rt.managed_window_open = message_dialog(ctx, "updates_managed_dialog", tr("DesignCraft Updates"), message, tr("OK"));
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
        let tr = |s: &str| crate::i18n::tr(&app.ui.language, s).to_string();
        let message = tr("Update check is not supported in the web browser.");
        app.updates_rt.manual_window_open = message_dialog(ctx, "updates_web_dialog", tr("DesignCraft Updates"), message, tr("OK"));
    }
}

/// A message window of the updater: `title` over `message`, with one OK button, as a framework dialog in its own layer.
/// Enter, Escape and OK close it. Returns whether it is still open.
fn message_dialog(ctx: &egui::Context, salt: &'static str, title: String, message: String, ok_label: String) -> bool {
    use craft_ui_panels::dialog::Dialog as FrameworkDialog;
    use craft_ui_panels::sections::{DialogRole, FooterButton};
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::Text;
    use craft_ui_widgets::runtime::overlay::Backdrop;
    let mut opened = true;
    let shown = egui::Area::new(egui::Id::new((salt, "host"))).order(egui::Order::Foreground).show(ctx, |ui| {
        FrameworkDialog::new(title)
            .id_salt(salt)
            .opened(Bind::bound(&mut opened))
            .backdrop(Backdrop::None)
            .closable(false)
            .outer_gesture(false)
            .close_on_enter(true)
            .buttons(vec![FooterButton::new(ok_label, DialogRole::Accept)])
            .show(ui, |ui, _| {
                Text::new(message.clone()).sizing(Sizing::FillWidth).show(ui);
            })
            .output
    });
    opened && shown.inner.closed.is_none() && shown.inner.picked.is_none()
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

    /// Successor of the managed-copy window's egui `Window`: the framework dialog shows who updates this copy, stays open
    /// while nothing answers it, and Enter (its OK) closes it.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_managed_copy_dialog_shows_who_updates_it_and_enter_closes_it() {
        let mut app = DesignApp::new(Session::new(), Default::default());
        app.set_updates_managed(Some("ArtCraft Launcher"));
        check(&mut app).unwrap();
        let ctx = crate::test_window::context();
        ctx.enable_accesskit();
        let mut frame = |app: &mut DesignApp, events: Vec<egui::Event>| {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| show(app, ui.ctx()));
            out.textures_delta.clear();
            out.platform_output
                .accesskit_update
                .map(|update| update.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect::<Vec<_>>())
        };
        let mut labels = Vec::new();
        for _ in 0..3 {
            labels = frame(&mut app, Vec::new()).unwrap_or_default();
        }
        assert!(app.updates_rt.managed_window_open, "the dialog stays open while nothing answers it");
        assert!(labels.iter().any(|l| l == "Updates are managed by ArtCraft Launcher."), "the dialog says who updates this copy: {labels:?}");
        assert!(labels.iter().any(|l| l == "DesignCraft Updates"), "the dialog has its title: {labels:?}");
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
        }
        frame(&mut app, Vec::new());
        assert!(!app.updates_rt.managed_window_open, "Enter, the dialog's OK, closes it");
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
