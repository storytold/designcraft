//! Edit › Keyboard Shortcuts: the framework's dialog (`craft_menu::shortcuts`) over DesignCraft's keymap and the user's
//! named shortcut sets.
//!
//! The dialog edits `DesignApp::keymap` and `UiState::shortcut_sets` in place, so menus show new
//! keys at once; Cancel puts both back. `UiState::shortcuts` follows the keymap every frame: it is
//! what ui.json saves for earlier versions and what `sync_keymap` applies. While the dialog is open
//! `app.ui.dialog` holds a `keyboardShortcuts` entry, so the canvas, the menus' key dispatch and
//! the control channel treat it as the open modal dialog.
//!
//! The framework dialog answers OK, Cancel and an import only inside a pass (it reads its words from the pass's
//! scope), so what the app asks of it from outside a frame (`ui.dialog.confirm` and `cancel`, a file the open dialog
//! delivered) waits in [`Host::pending`] and runs in the next frame, before the dialog is drawn.
//!
//! The InDesign importer stays here (`shortcut_import`): the dialog's Import offers its format
//! next to craft-keymap's JSON, and the dialog reviews the set before it becomes a new set.
//!
//! The browser build has no framework dialog (craft-menu is native only): the command says so in the status bar.

use craft_keymap::Overrides;
#[cfg(not(target_arch = "wasm32"))]
use craft_keymap::{ForeignSet, ImportOptions, Keymap, Profiles};
#[cfg(not(target_arch = "wasm32"))]
use craft_menu::shortcuts::{DialogState, Format, Options, Outcome, ShortcutsDialog};
#[cfg(not(target_arch = "wasm32"))]
use serde_json::{Value, json};

use crate::DesignApp;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::file_dialogs::Bound;
#[cfg(not(target_arch = "wasm32"))]
use crate::i18n::tr;

/// The `app.ui.dialog` id while the dialog is open.
pub const DIALOG_ID: &str = "keyboardShortcuts";

/// DesignCraft's built-in shortcut set: its default keys.
pub static BUILTINS: [(&str, Overrides); 1] = [("Default", Overrides::new())];

/// The name edits of the built-in set are saved under (translated, made unique).
pub const CUSTOM_SET: &str = "Custom";

/// The file-picker purpose of the dialog's Import (`DesignApp::ask_open`, `open_async`).
pub const PURPOSE: &str = "shortcuts";

/// A set saved by InDesign's Show Set, read by `shortcut_import`.
#[cfg(not(target_arch = "wasm32"))]
pub const INDESIGN_FORMAT: Format = Format::new("indesign", "InDesign Shortcut Set (Show Set text)", &["txt"]);

/// What the app asked of the dialog from outside a frame.
#[cfg(not(target_arch = "wasm32"))]
enum Pending {
    Ok,
    Cancel,
    /// A craft-keymap JSON set's text: it becomes a new set.
    Json(String),
    /// A set from another application, read, for the import review.
    Review(ForeignSet, ImportOptions),
}

/// The open dialog: its state and options.
#[cfg(not(target_arch = "wasm32"))]
pub struct Host {
    state: DialogState,
    options: Options,
    pending: Vec<Pending>,
    /// Whether a frame drew it: until then nothing was edited, and closing needs no pass.
    shown: bool,
    /// Whether OK or Cancel was asked for (it ends in the next frame).
    closing: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Host {
    fn new() -> Host {
        Host { state: DialogState::new(), options: options(), pending: Vec::new(), shown: false, closing: false }
    }

    pub(crate) fn recording(&self) -> bool {
        self.state.is_recording()
    }
}

/// The owner-approved behavior: live editing with Cancel restoring, conflicts asked about, no
/// double presses, the US keyboard pane, InDesign sets to import, and no rows for the engine
/// commands only scripts run.
#[cfg(not(target_arch = "wasm32"))]
fn options() -> Options {
    Options::default().with(|o| {
        o.keyboard = true;
        o.import_formats = vec![INDESIGN_FORMAT];
        o.visible = Some(crate::keys::listed);
    })
}

/// The dialog for one call: the app's keymap and sets, with the dialog's options.
#[cfg(not(target_arch = "wasm32"))]
fn dialog<'a>(keymap: &'a mut Keymap, sets: &'a mut Profiles, options: &'a Options) -> ShortcutsDialog<'a> {
    ShortcutsDialog::new(keymap).options(options).sets(sets, &BUILTINS)
}

/// Edit › Keyboard Shortcuts…; with `select`, the dialog opens on that command.
#[cfg(not(target_arch = "wasm32"))]
pub fn open(app: &mut DesignApp, select: Option<&craft_keymap::CommandRef>) {
    crate::dialogs::cancel(app);
    app.sync_keymap();
    let mut host = Host::new();
    if let Some(command) = select {
        host.state.select(command);
    }
    app.shortcut_dialog = Some(Box::new(host));
    app.ui.dialog = Some(crate::dialogs::Dialog::new(DIALOG_ID, json!({})));
}

/// One frame of the open dialog: what was asked from outside runs first, then the dialog is drawn in the framework's modal
/// layer, in the interface's language and direction.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context) {
    use craft_ui_widgets::runtime::env::{Direction, WordsOverride, with_env};

    let wanted = app.ui.dialog.as_ref().is_some_and(|d| d.id == DIALOG_ID);
    let mut host = match app.shortcut_dialog.take() {
        Some(host) => host,
        None if wanted => Box::new(Host::new()),
        None => return,
    };
    if !wanted && !host.closing {
        // Another dialog took its place: the edits are cancelled.
        host.pending.push(Pending::Cancel);
        host.closing = true;
    }
    host.shown = true;
    let lang = app.ui.language.clone();
    host.options.title = Some(tr(&lang, "Keyboard Shortcuts").to_owned());
    host.options.custom_set_name = Some(tr(&lang, CUSTOM_SET).to_owned());
    let words = crate::framework_host::words(&lang);
    let direction = if crate::i18n::is_rtl(&lang) { Direction::Rtl } else { Direction::Ltr };
    let asked_ok = host.pending.iter().any(|p| matches!(p, Pending::Ok));
    // This area only supplies a parent: the dialog mounts its own modal layer.
    let shown = egui::Area::new(egui::Id::new("keyboard_shortcuts_host")).order(egui::Order::Foreground).show(ctx, |ui| {
        with_env(
            ui,
            |env| {
                env.direction = Some(direction);
                env.words = Some(WordsOverride::Replace(words));
            },
            |ui| {
                let Host { state, options, pending, .. } = &mut *host;
                for step in std::mem::take(pending) {
                    let dialog = dialog(&mut app.keymap, &mut app.ui.shortcut_sets, options);
                    match step {
                        Pending::Ok => dialog.ok(ui, state),
                        Pending::Cancel => dialog.cancel(ui, state),
                        // A refused file is reported in the dialog.
                        Pending::Json(text) => {
                            let _ = dialog.import_json(ui, state, &text);
                        }
                        Pending::Review(set, import) => dialog.review_import(ui, state, set, import),
                    }
                }
                dialog(&mut app.keymap, &mut app.ui.shortcut_sets, options).show(ui, state).outcome
            },
        )
    });
    let outcome = shown.inner;
    // Live: the menus, `sync_keymap` and ui.json follow every edit, and a Cancel. The dialog
    // saves the sets itself.
    if app.ui.shortcuts != *app.keymap.overrides() {
        app.ui.shortcuts = app.keymap.overrides().clone();
    }
    let refused = asked_ok && !matches!(outcome, Some(Outcome::Ok));
    match outcome {
        Some(Outcome::Import { .. }) => request_import(app, &mut host),
        Some(Outcome::Export { name, json, .. }) => save_file(app, &format!("{name}.json"), json.as_bytes()),
        Some(Outcome::Summary { name, html, .. }) => save_file(app, &format!("{name}.html"), html.as_bytes()),
        Some(Outcome::Remembered(answers)) => app.shortcut_answers = answers,
        _ => {}
    }
    if refused && host.state.is_open() {
        // An OK the dialog refuses (no room for another set) leaves it open with its message.
        host.closing = false;
        app.ui.dialog = Some(crate::dialogs::Dialog::new(DIALOG_ID, json!({})));
    }
    if host.state.is_open() {
        app.shortcut_dialog = Some(host);
        return;
    }
    if app.ui.dialog.as_ref().is_some_and(|d| d.id == DIALOG_ID) {
        app.ui.dialog = None;
    }
    crate::keys::store(app);
}

/// OK (`ok`) or Cancel from outside the dialog: `ui.dialog.confirm` and `ui.dialog.cancel`, or
/// another dialog taking its place. The dialog answers in the next frame; one no frame ever drew has nothing to answer.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn close(app: &mut DesignApp, ok: bool) -> Result<(), String> {
    let Some(host) = app.shortcut_dialog.as_mut() else { return Ok(()) };
    if !host.shown {
        app.shortcut_dialog = None;
        crate::keys::store(app);
        return Ok(());
    }
    host.pending.push(if ok { Pending::Ok } else { Pending::Cancel });
    host.closing = true;
    Ok(())
}

/// Import: the chosen file is read when the open dialog answers, or arrives later through
/// [`deliver`] in a browser.
#[cfg(not(target_arch = "wasm32"))]
fn request_import(app: &mut DesignApp, host: &mut Host) {
    let request = app.import_request(PURPOSE);
    if let Some(open) = app.services.open_async.as_mut() {
        open(request);
        return;
    }
    let asked = app.ask_open(PURPOSE, Bound::App, |app, path| {
        // The dialog closed meanwhile: nothing to import into.
        let Some(mut host) = app.shortcut_dialog.take() else { return Ok(Value::Null) };
        match crate::shortcut_import::read_file(&path) {
            Ok(text) => review(app, &mut host, &text),
            Err(e) => host.state.show_error(e),
        }
        app.shortcut_dialog = Some(host);
        Ok(Value::Null)
    });
    if let Err(e) = asked {
        host.state.show_error(e);
    }
}

/// A set file chosen in the browser, for the open dialog. False when the dialog is closed.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn deliver(app: &mut DesignApp, bytes: &[u8]) -> bool {
    let Some(mut host) = app.shortcut_dialog.take() else { return false };
    match crate::shortcut_import::decode(bytes) {
        Ok(text) => review(app, &mut host, &text),
        Err(e) => host.state.show_error(e),
    }
    app.shortcut_dialog = Some(host);
    true
}

/// A set file's text for the dialog: craft-keymap JSON becomes a new set at once (the dialog
/// shows why it cannot); an InDesign set opens the import review. Both happen in the next frame.
#[cfg(not(target_arch = "wasm32"))]
fn review(app: &mut DesignApp, host: &mut Host, text: &str) {
    if text.trim_start().starts_with('{') {
        host.pending.push(Pending::Json(text.to_owned()));
        return;
    }
    match crate::shortcut_import::read_set(text) {
        Ok(set) => host.pending.push(Pending::Review(set, crate::shortcut_import::import_options(app.shortcut_answers.clone()))),
        Err(e) => host.state.show_error(e),
    }
}

/// Writes a file the dialog made where the user chooses (a download on the web).
#[cfg(not(target_arch = "wasm32"))]
fn save_file(app: &mut DesignApp, name: &str, bytes: &[u8]) {
    let name: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':') { '_' } else { c }).collect();
    let bytes = bytes.to_vec();
    let asked = app.ask_save(&name, Bound::App, move |app, path| {
        let result = match app.services.write.as_mut() {
            Some(write) => write(&path, &bytes),
            None => Err("no writer".into()),
        };
        match result {
            Ok(()) => app.status(tr(&app.ui.language, "Saved to {path}").replace("{path}", &path)),
            Err(e) => app.status(e),
        }
        Ok(Value::Null)
    });
    if let Err(e) = asked {
        app.status(e);
    }
}

/// The browser build has no dialog: the keymap still applies, and `window.setShortcut` edits it.
#[cfg(target_arch = "wasm32")]
pub struct Host;

#[cfg(target_arch = "wasm32")]
impl Host {
    pub(crate) fn recording(&self) -> bool {
        false
    }
}

#[cfg(target_arch = "wasm32")]
pub fn open(app: &mut DesignApp, _select: Option<&craft_keymap::CommandRef>) {
    let message = crate::i18n::tr(&app.ui.language, "The Keyboard Shortcuts dialog is not available in the browser yet.").to_owned();
    app.status(message);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn show(_app: &mut DesignApp, _ctx: &egui::Context) {}

#[cfg(target_arch = "wasm32")]
pub(crate) fn close(_app: &mut DesignApp, _ok: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn deliver(_app: &mut DesignApp, _bytes: &[u8]) -> bool {
    false
}

/// Loads the active set into the keymap and `ui.shortcuts` (after an import made a new set).
pub(crate) fn load_active_set(app: &mut DesignApp) {
    let overrides = app.ui.shortcut_sets.active_overrides(&BUILTINS).clone();
    app.keymap.set_overrides(overrides.clone());
    app.ui.shortcuts = overrides;
}

/// The English strings the dialog looks up: every word of its word table and both forms of its counts.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn strings() -> impl Iterator<Item = &'static str> {
    use craft_menu::shortcuts::ShortcutWords;
    use craft_ui_widgets::runtime::env::WordSet as _;
    ShortcutWords::WORDS.iter().map(|word| word.english).chain(ShortcutWords::COUNTS.iter().flat_map(|count| [count.one, count.word.english]))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use craft_keymap::CommandRef;
    use egui::{Key, Modifiers, accesskit::Role, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use crate::dialogs::table_layout_kit as kit;
    use crate::menus::shortcut_of;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app
    }

    /// The app's frame as far as shortcuts go: key dispatch, then the open dialog, over the installed framework runtime.
    fn harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        app.framework_transform = false;
        app.framework_opacity = true;
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert!(app.framework_installed, "the framework dialog needs the installed runtime");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(1280.0, 860.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(crate::theme::Brightness::default()));
                    ready = true;
                    return;
                }
                crate::menus::shortcuts(app, ui.ctx());
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        kit::steps(&mut h, 3);
        h
    }

    fn opened() -> Harness<'static, DesignApp> {
        let mut app = app();
        app.run("app.keyboardShortcuts", json!({})).unwrap();
        harness(app)
    }

    fn click(h: &mut Harness<'_, DesignApp>, label: &str) {
        kit::click_first(h, label);
    }

    fn press(h: &mut Harness<'_, DesignApp>, modifiers: Modifiers, key: Key) {
        kit::key(h, key, modifiers);
    }

    fn has(h: &Harness<'_, DesignApp>, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    /// Whether a node whose name contains `part` exists.
    fn has_part(h: &Harness<'_, DesignApp>, part: &str) -> bool {
        h.query_all_by_label_contains(part).next().is_some()
    }

    /// Whether a link whose name contains `part` exists.
    fn has_link(h: &Harness<'_, DesignApp>, part: &str) -> bool {
        use egui_kittest::kittest::NodeT;
        h.query_all_by_label_contains(part).any(|node| node.accesskit_node().role() == Role::Link)
    }

    /// Searches for a command and returns the start of the name of its row's shortcut cell, which starts recording.
    fn row(h: &mut Harness<'_, DesignApp>, label: &str) -> String {
        kit::click_first(h, "Search:");
        kit::key(h, Key::A, Modifiers::COMMAND);
        kit::type_text(h, label);
        format!("{label}: ")
    }

    /// Clicks the shortcut cell whose name starts with `start` (the chord follows it as the platform shows it).
    fn click_cell(h: &mut Harness<'_, DesignApp>, start: &str) {
        kit::click_containing(h, start);
    }

    /// A command's shortcut with no changes.
    fn default_of(id: &str) -> Option<String> {
        crate::keys::shortcut(&crate::keys::new_keymap(Overrides::new()), &CommandRef::new(id))
    }

    fn open_now(h: &Harness<'_, DesignApp>) -> bool {
        h.state().ui.dialog.as_ref().is_some_and(|d| d.id == DIALOG_ID) && h.state().shortcut_dialog.is_some()
    }

    #[test]
    fn a_recorded_chord_runs_nothing_changes_the_menus_at_once_and_ok_keeps_a_custom_set() {
        let mut h = opened();
        let quick_apply = row(&mut h, "Quick Apply…");
        click_cell(&mut h, &quick_apply);
        // Preferences' chord is recorded, not run.
        press(&mut h, Modifiers::COMMAND, Key::K);
        assert!(open_now(&h), "Preferences did not open");
        assert!(has_part(&h, "is used by Preferences…"), "the conflict is asked about");
        click(&mut h, "Take It");
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("Cmd+K"));
        assert_eq!(shortcut_of(h.state(), "app.preferences"), None);
        assert_eq!(h.state().ui.shortcuts.get("app.palette"), Some(&["Cmd+K".to_string()][..]), "ui.json follows live");
        assert!(h.state().ui.shortcut_sets.sets().is_empty(), "the set is saved by OK");

        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(!open_now(&h) && h.state().ui.dialog.is_none());
        let sets = &h.state().ui.shortcut_sets;
        assert_eq!(sets.active(), "Custom", "edits of the built-in set become a copy");
        assert_eq!(sets.active_overrides(&BUILTINS), &h.state().ui.shortcuts);
        let saved = serde_json::to_value(&h.state().ui).unwrap();
        assert_eq!(saved["shortcuts"]["app.palette"], "Cmd+K", "the map earlier versions read");
        assert_eq!(saved["shortcutSets"]["active"], "Custom");
        // The next frames keep it.
        kit::steps(&mut h, 2);
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("Cmd+K"));
    }

    #[test]
    fn assign_takes_a_held_shortcut_in_one_step_for_the_menus_and_the_keys() {
        let mut app = app();
        super::open(&mut app, Some(&CommandRef::new("app.paragraphStylesPanel")));
        let mut h = harness(app);
        click(&mut h, "New shortcut: None");
        press(&mut h, Modifiers::COMMAND, Key::P);
        assert!(has_link(&h, "Print…"), "Print is listed as holding the chord");
        click(&mut h, "Assign");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(!open_now(&h));
        let app = h.state();
        assert_eq!(shortcut_of(app, "file.print"), None, "the menus no longer show it on Print");
        let bindings: Vec<String> = app.keymap.bindings(&CommandRef::new("app.paragraphStylesPanel")).iter().map(|b| b.trigger.to_string()).collect();
        assert_eq!(bindings, ["Cmd+F11", "Cmd+P"]);
        // The macOS menu bar holds Paragraph Styles' first shortcut; its new one runs in the app.
        h.state_mut().native_shortcuts = [("app.paragraphStylesPanel".to_string(), "Cmd+F11".to_string())].into();
        assert!(!crate::panel_docking::showing(h.state(), "paragraphStyles"));
        press(&mut h, Modifiers::COMMAND, Key::P);
        assert!(crate::panel_docking::showing(h.state(), "paragraphStyles"), "Cmd+P opens Paragraph Styles");
    }

    #[test]
    fn cancel_puts_the_keys_and_the_sets_back() {
        let mut h = opened();
        let quick_apply = row(&mut h, "Quick Apply…");
        click_cell(&mut h, &quick_apply);
        press(&mut h, Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT, Key::Num9);
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("Cmd+Alt+Shift+9"));
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        kit::steps(&mut h, 3);
        assert!(!open_now(&h));
        assert_eq!(shortcut_of(h.state(), "app.palette"), default_of("app.palette"));
        assert!(h.state().ui.shortcuts.is_empty());
        assert_eq!(h.state().ui.shortcut_sets, craft_keymap::Profiles::default());

        // `ui.dialog.cancel` from the control channel does the same.
        h.state_mut().run("app.keyboardShortcuts", json!({"command": "app.palette"})).unwrap();
        kit::steps(&mut h, 3);
        let quick_apply = row(&mut h, "Quick Apply…");
        click_cell(&mut h, &quick_apply);
        press(&mut h, Modifiers::NONE, Key::F2);
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("F2"));
        crate::dialogs::cancel(h.state_mut());
        kit::steps(&mut h, 3);
        assert!(!open_now(&h));
        assert_eq!(shortcut_of(h.state(), "app.palette"), default_of("app.palette"));
    }

    /// An InDesign set through the dialog's Import: reviewed, then a new set with the Window menu
    /// panel item on the panel and its twin command, and the Tools item on the tool.
    #[test]
    fn an_indesign_set_imports_through_the_review_as_a_new_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.txt");
        std::fs::write(
            &path,
            "Shortcut Set : Studio\r\n\
             Product area : Window Menu\r\n\
             \tType & Tables: Character --- Default: Ctrl+T\r\n\
             Product area : Tools\r\n\
             \tPen Tool --- Default: Shift+Ctrl+F3\r\n\
             \tFrobnicate Tool --- Default: Shift+Ctrl+F4\r\n",
        )
        .unwrap();
        let picked = path.to_string_lossy().to_string();
        let mut app = app();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked)));
        app.run("app.keyboardShortcuts", json!({})).unwrap();
        let mut h = harness(app);
        click(&mut h, "Import…");
        h.state_mut().settle_dialogs();
        kit::steps(&mut h, 3);
        assert!(has(&h, "Import “Studio”"));
        assert!(h.state().ui.shortcuts.is_empty(), "the review changes nothing");
        click(&mut h, "Import as New Set");
        assert!(has_part(&h, "Imported as “Studio”"));
        let panel = |h: &Harness<'_, DesignApp>| crate::keys::shortcut(&h.state().keymap, &crate::keys::panel("character"));
        assert_eq!(panel(&h).as_deref(), Some("Cmd+T"));
        assert_eq!(shortcut_of(h.state(), "app.characterPanel").as_deref(), Some("Cmd+T"), "the panel's twin keeps the key they share");
        assert_eq!(crate::keys::shortcut(&h.state().keymap, &crate::keys::tool("pen")).as_deref(), Some("Cmd+Shift+F3"));
        click(&mut h, "Done");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(!open_now(&h));
        assert_eq!(h.state().ui.shortcut_sets.active(), "Studio");
        assert_eq!(h.state().ui.shortcut_sets.active_overrides(&BUILTINS), &h.state().ui.shortcuts);
        // Back to the defaults by switching sets.
        h.state_mut().run("window.shortcutSets", json!({"select": "Default"})).unwrap();
        assert_eq!(
            crate::keys::shortcut(&h.state().keymap, &crate::keys::tool("pen")),
            crate::keys::shortcut(&crate::keys::new_keymap(Overrides::new()), &crate::keys::tool("pen"))
        );
        assert!(h.state().ui.shortcuts.is_empty());
    }

    #[test]
    fn shortcuts_from_an_earlier_version_become_the_active_set() {
        // A ui.json without sets: its changes become Custom.
        let mut app = app();
        app.ui = crate::UiState::from_preferences_json(br#"{"shortcuts": {"app.palette": "Cmd+Alt+Shift+9"}, "shortcutSets": false}"#).unwrap();
        app.sync_keymap();
        assert_eq!(app.ui.shortcut_sets.active(), "Custom");
        assert_eq!(shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+Alt+Shift+9"));
        // An earlier version changed the shortcuts of the active set "Mine".
        let mut saved = serde_json::to_value(&app.ui).unwrap();
        saved["shortcutSets"] = json!({"version": 1, "active": "Mine", "sets": [{"name": "Mine", "overrides": []}]});
        saved["shortcuts"] = json!({"app.palette": "F2"});
        app.ui = crate::UiState::from_preferences_json(&serde_json::to_vec(&saved).unwrap()).unwrap();
        app.sync_keymap();
        assert_eq!(app.ui.shortcut_sets.active(), "Mine");
        assert_eq!(app.ui.shortcut_sets.active_overrides(&BUILTINS).get("app.palette"), Some(&["F2".to_string()][..]));
        assert_eq!(shortcut_of(&app, "app.palette").as_deref(), Some("F2"));
        let r = app.run("window.shortcutSets", json!({"select": "Default"})).unwrap();
        assert_eq!(r, json!({"active": "Default", "sets": ["Default", "Mine"], "builtIn": ["Default"]}));
        assert_eq!(shortcut_of(&app, "app.palette"), default_of("app.palette"));
        assert!(app.run("window.shortcutSets", json!({"select": "Nope"})).is_err());
        assert!(serde_json::to_value(&app.ui).unwrap()["shortcuts"].as_object().is_some_and(|m| m.is_empty()));
    }

    #[test]
    fn every_dialog_string_reads_in_english_without_a_translation() {
        let mut count = 0;
        for s in strings() {
            assert_eq!(tr("", s), s);
            count += 1;
        }
        assert!(count > 0);
        {
            use craft_menu::shortcuts::ShortcutWords;
            use craft_ui_widgets::runtime::env::WordSet as _;
            assert_eq!(ShortcutWords::COUNTS.len(), 11);
        }
        // One `Words` per language: a new one is a new generation, which builds every view that reads it again.
        let (a, b) = (crate::framework_host::words("fr"), crate::framework_host::words("fr"));
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert!(std::sync::Arc::ptr_eq(&crate::framework_host::host("fr"), &crate::framework_host::host("fr")));
    }
}
