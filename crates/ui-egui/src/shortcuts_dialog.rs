//! Edit › Keyboard Shortcuts: craft-keymap's shared dialog over DesignCraft's keymap and the user's
//! named shortcut sets.
//!
//! The dialog edits `DesignApp::keymap` and `UiState::shortcut_sets` in place, so menus show new
//! keys at once; Cancel puts both back. `UiState::shortcuts` follows the keymap every frame: it is
//! what ui.json saves for earlier versions and what `sync_keymap` applies. While the dialog is open
//! `app.ui.dialog` holds a `keyboardShortcuts` entry, so the canvas, the menus' key dispatch and
//! the control channel treat it as the open modal dialog.
//!
//! The InDesign importer stays here (`shortcut_import`): the dialog's Import offers its format
//! next to craft-keymap's JSON, and the dialog reviews the set before it becomes a new set.

use std::borrow::Cow;

use craft_keymap::ui::{DialogState, Format, Options, Outcome, ShortcutsDialog, Text};
use craft_keymap::{Bindable, Overrides};
use serde_json::json;

use crate::DesignApp;
use crate::i18n::tr;

/// The `app.ui.dialog` id while the dialog is open.
pub const DIALOG_ID: &str = "keyboardShortcuts";

/// DesignCraft's built-in shortcut set: its default keys.
pub static BUILTINS: [(&str, Overrides); 1] = [("Default", Overrides::new())];

/// The name edits of the built-in set are saved under (translated, made unique).
pub const CUSTOM_SET: &str = "Custom";

/// The file-picker purpose of the dialog's Import (`Services::pick_open`, `open_async`).
pub const PURPOSE: &str = "shortcuts";

/// A set saved by InDesign's Show Set, read by `shortcut_import`.
pub const INDESIGN_FORMAT: Format = Format { id: "indesign", label: "InDesign Shortcut Set (Show Set text)", extensions: &["txt"] };

/// The open dialog: its state and options.
pub struct Host {
    state: DialogState,
    options: Options,
}

impl Host {
    fn new() -> Host {
        Host { state: DialogState::new(), options: options() }
    }

    pub(crate) fn recording(&self) -> bool {
        self.state.is_recording()
    }
}

/// The owner-approved behavior: live editing with Cancel restoring, conflicts asked about, no
/// double presses, the US keyboard pane, InDesign sets to import, and no rows for the engine
/// commands only scripts run.
fn options() -> Options {
    Options::default().with(|o| {
        o.keyboard = true;
        o.import_formats = vec![INDESIGN_FORMAT];
        o.visible = Some(crate::keys::listed);
    })
}

/// The dialog's words through DesignCraft's catalogs; strings without a translation stay English.
pub struct DcText<'a> {
    pub lang: &'a str,
}

impl Text for DcText<'_> {
    fn ui<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        tr(self.lang, source).into()
    }

    fn plural(&self, n: u64, one: &str, other: &str) -> String {
        tr(self.lang, if n == 1 { one } else { other }).replace("{n}", &n.to_string())
    }

    fn command<'a>(&'a self, b: &'a Bindable) -> Cow<'a, str> {
        tr(self.lang, &b.label).into()
    }

    fn segment<'a>(&'a self, s: &'a str) -> Cow<'a, str> {
        tr(self.lang, s).into()
    }

    fn rtl(&self) -> bool {
        crate::i18n::is_rtl(self.lang)
    }
}

/// Edit › Keyboard Shortcuts…; with `select`, the dialog opens on that command.
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

/// The dialog's size: most of the window, within the dialog's own limits.
fn size(screen: egui::Vec2) -> egui::Vec2 {
    let max = egui::vec2(1320.0, 900.0).min(screen - egui::vec2(48.0, 96.0)).max(egui::vec2(240.0, 180.0));
    let min = egui::vec2(640.0, 480.0).min(max);
    (screen * 0.8).clamp(min, max)
}

/// One frame of the open dialog, drawn in DesignCraft's movable modal.
pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let mut host = app.shortcut_dialog.take().unwrap_or_else(|| Box::new(Host::new()));
    let lang = app.ui.language.clone();
    let text = DcText { lang: &lang };
    let title = tr(&lang, "Keyboard Shortcuts");
    let area = egui::Id::unique("dialog");
    let instance = area.with(DIALOG_ID);
    let body = size(ctx.content_rect().size());
    let mut out = None;
    crate::modal_drag::modal(ctx, area, instance).backdrop_color(egui::Color32::TRANSPARENT).show(ctx, |ui| {
        let title_top = ui.cursor().top();
        crate::rtl::label(ui, egui::RichText::new(title).font(crate::theme::semibold(16.0)));
        let margin = egui::Frame::popup(ui.style()).inner_margin;
        let title_row = egui::Rect::from_min_max(
            egui::pos2(ui.max_rect().left() - margin.leftf(), title_top - margin.topf()),
            egui::pos2(ui.max_rect().left() + body.x + margin.rightf(), ui.min_rect().bottom()),
        );
        crate::modal_drag::title_drag(ui, instance, title_row);
        ui.add_space(8.0);
        let (rect, _) = ui.allocate_exact_size(body, egui::Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let dialog = ShortcutsDialog::new(&mut app.keymap, &text).options(&host.options).sets(&mut app.ui.shortcut_sets, &BUILTINS);
        out = dialog.ui(&mut child, &mut host.state);
    });
    // Live: the menus, `sync_keymap` and ui.json follow every edit, and a Cancel. The dialog
    // saves the sets itself.
    if app.ui.shortcuts != *app.keymap.overrides() {
        app.ui.shortcuts = app.keymap.overrides().clone();
    }
    match out {
        Some(Outcome::Import { .. }) => request_import(app, &mut host),
        Some(Outcome::Export { name, json }) => save_file(app, &format!("{name}.json"), json.as_bytes()),
        Some(Outcome::Summary { name, html, .. }) => save_file(app, &format!("{name}.html"), html.as_bytes()),
        Some(Outcome::Remembered(answers)) => app.shortcut_answers = answers,
        _ => {}
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
/// another dialog taking its place. An OK the dialog refuses (no room for another set) leaves it
/// open with its message and returns the error.
pub(crate) fn close(app: &mut DesignApp, ok: bool) -> Result<(), String> {
    let Some(mut host) = app.shortcut_dialog.take() else { return Ok(()) };
    let lang = app.ui.language.clone();
    let text = DcText { lang: &lang };
    let dialog = ShortcutsDialog::new(&mut app.keymap, &text).options(&host.options).sets(&mut app.ui.shortcut_sets, &BUILTINS);
    if ok {
        dialog.ok(&mut host.state);
    } else {
        dialog.cancel(&mut host.state);
    }
    crate::keys::store(app);
    if ok && host.state.is_open() {
        app.shortcut_dialog = Some(host);
        app.ui.dialog = Some(crate::dialogs::Dialog::new(DIALOG_ID, json!({})));
        return Err("the shortcut set could not be saved".into());
    }
    Ok(())
}

/// Import: a native picker reads the file at once; in a browser it arrives later through
/// [`deliver`].
fn request_import(app: &mut DesignApp, host: &mut Host) {
    if let Some(pick) = app.services.pick_open.as_mut() {
        let Some(path) = pick(PURPOSE) else { return };
        match crate::shortcut_import::read_file(&path) {
            Ok(text) => review(app, host, &text),
            Err(e) => host.state.show_error(e),
        }
        return;
    }
    let request = app.import_request(PURPOSE);
    if let Some(open) = app.services.open_async.as_mut() {
        open(request);
    }
}

/// A set file chosen in the browser, for the open dialog. False when the dialog is closed.
pub(crate) fn deliver(app: &mut DesignApp, bytes: &[u8]) -> bool {
    let Some(mut host) = app.shortcut_dialog.take() else { return false };
    match crate::shortcut_import::decode(bytes) {
        Ok(text) => review(app, &mut host, &text),
        Err(e) => host.state.show_error(e),
    }
    app.shortcut_dialog = Some(host);
    true
}

/// A set file's text into the dialog: craft-keymap JSON becomes a new set at once (the dialog
/// shows why it cannot); an InDesign set opens the import review.
fn review(app: &mut DesignApp, host: &mut Host, text: &str) {
    let lang = app.ui.language.clone();
    let words = DcText { lang: &lang };
    if text.trim_start().starts_with('{') {
        let dialog = ShortcutsDialog::new(&mut app.keymap, &words).options(&host.options).sets(&mut app.ui.shortcut_sets, &BUILTINS);
        // A refused file is reported in the dialog.
        let _ = dialog.import_json(&mut host.state, text);
        return;
    }
    match crate::shortcut_import::read_set(text) {
        Ok(set) => {
            let options = crate::shortcut_import::import_options(app.shortcut_answers.clone());
            let dialog = ShortcutsDialog::new(&mut app.keymap, &words).options(&host.options).sets(&mut app.ui.shortcut_sets, &BUILTINS);
            dialog.review_import(&mut host.state, set, options);
        }
        Err(e) => host.state.show_error(e),
    }
}

/// Writes a file the dialog made where the user chooses (a download on the web).
fn save_file(app: &mut DesignApp, name: &str, bytes: &[u8]) {
    let name: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':') { '_' } else { c }).collect();
    let Some(path) = app.services.pick_save.as_mut().and_then(|f| f(&name)) else { return };
    let result = match app.services.write.as_mut() {
        Some(write) => write(&path, bytes),
        None => Err("no writer".into()),
    };
    match result {
        Ok(()) => app.status(tr(&app.ui.language, "Saved to {path}").replace("{path}", &path)),
        Err(e) => app.status(e),
    }
}

/// Loads the active set into the keymap and `ui.shortcuts` (after an import made a new set).
pub(crate) fn load_active_set(app: &mut DesignApp) {
    let overrides = app.ui.shortcut_sets.active_overrides(&BUILTINS).clone();
    app.keymap.set_overrides(overrides.clone());
    app.ui.shortcuts = overrides;
}

/// The strings the dialog looks up: `craft_keymap::ui::STRINGS` and both forms of its plurals.
#[cfg(test)]
pub(crate) fn strings() -> impl Iterator<Item = &'static str> {
    craft_keymap::ui::STRINGS.iter().copied().chain(craft_keymap::ui::PLURALS.iter().flat_map(|(one, other)| [*one, *other]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_keymap::{CommandRef, Trigger};
    use egui::{Key, Modifiers, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use crate::menus::shortcut_of;

    fn app() -> DesignApp {
        DesignApp::new(designcraft_engine::Session::new(), crate::Services::default())
    }

    /// The app's frame as far as shortcuts go: key dispatch, then the open dialog.
    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        let mut ready = false;
        // Shortcut editing is tested here; optional platform-symbol fonts are not a prerequisite.
        let mut h = Harness::builder().allow_missing_glyphs().with_size(vec2(1280.0, 860.0)).build_ui_state(
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
        h.run();
        h
    }

    fn opened() -> Harness<'static, DesignApp> {
        let mut app = app();
        app.run("app.keyboardShortcuts", json!({})).unwrap();
        harness(app)
    }

    fn click(h: &mut Harness<'_, DesignApp>, label: &str) {
        h.get_by_label(label).click();
        h.run();
    }

    fn press(h: &mut Harness<'_, DesignApp>, modifiers: Modifiers, key: Key) {
        h.key_press_modifiers(modifiers, key);
        h.run();
    }

    fn has(h: &Harness<'_, DesignApp>, label: &str) -> bool {
        h.query_by_label(label).is_some()
    }

    fn chord(h: &Harness<'_, DesignApp>, text: &str) -> String {
        Trigger::parse(text).unwrap().display(h.state().keymap.platform())
    }

    /// Searches for a command and returns the label of its row's shortcut cell, which starts
    /// recording.
    fn row(h: &mut Harness<'_, DesignApp>, label: &str, id: &str) -> String {
        h.get_by_label("Search:").focus();
        h.run();
        h.get_by_label("Search:").type_text(label);
        h.run();
        format!("{label}: {}", crate::keys::shortcut_display(&h.state().keymap, &CommandRef::new(id)).unwrap())
    }

    /// A command's shortcut with no changes.
    fn default_of(id: &str) -> Option<String> {
        crate::keys::shortcut(&crate::keys::new_keymap(Overrides::new()), &CommandRef::new(id))
    }

    fn open(h: &Harness<'_, DesignApp>) -> bool {
        h.state().ui.dialog.as_ref().is_some_and(|d| d.id == DIALOG_ID) && h.state().shortcut_dialog.is_some()
    }

    #[test]
    fn a_recorded_chord_runs_nothing_changes_the_menus_at_once_and_ok_keeps_a_custom_set() {
        let mut h = opened();
        let quick_apply = row(&mut h, "Quick Apply…", "app.palette");
        click(&mut h, &quick_apply);
        // Preferences' chord is recorded, not run.
        press(&mut h, Modifiers::COMMAND, Key::K);
        assert!(open(&h), "Preferences did not open");
        assert!(has(&h, &format!("⚠ {} is used by Preferences…", chord(&h, "Cmd+K"))));
        click(&mut h, "Take It");
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("Cmd+K"));
        assert_eq!(shortcut_of(h.state(), "app.preferences"), None);
        assert_eq!(h.state().ui.shortcuts.get("app.palette"), Some(&["Cmd+K".to_string()][..]), "ui.json follows live");
        assert!(h.state().ui.shortcut_sets.sets().is_empty(), "the set is saved by OK");

        click(&mut h, "OK");
        assert!(!open(&h) && h.state().ui.dialog.is_none());
        let sets = &h.state().ui.shortcut_sets;
        assert_eq!(sets.active(), "Custom", "edits of the built-in set become a copy");
        assert_eq!(sets.active_overrides(&BUILTINS), &h.state().ui.shortcuts);
        let saved = serde_json::to_value(&h.state().ui).unwrap();
        assert_eq!(saved["shortcuts"]["app.palette"], "Cmd+K", "the map earlier versions read");
        assert_eq!(saved["shortcutSets"]["active"], "Custom");
        // The next frames keep it.
        h.run();
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("Cmd+K"));
    }

    #[test]
    fn assign_takes_a_held_shortcut_in_one_step_for_the_menus_and_the_keys() {
        let mut app = app();
        super::open(&mut app, Some(&CommandRef::new("app.paragraphStylesPanel")));
        let mut h = harness(app);
        click(&mut h, "New shortcut: None");
        press(&mut h, Modifiers::COMMAND, Key::P);
        assert!(has(&h, "⚠ Print…"), "Print is listed as holding the chord");
        click(&mut h, "Assign");
        click(&mut h, "OK");
        assert!(!open(&h));
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
        let quick_apply = row(&mut h, "Quick Apply…", "app.palette");
        click(&mut h, &quick_apply);
        press(&mut h, Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT, Key::Num9);
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("Cmd+Alt+Shift+9"));
        click(&mut h, "Cancel");
        h.run();
        assert!(!open(&h));
        assert_eq!(shortcut_of(h.state(), "app.palette"), default_of("app.palette"));
        assert!(h.state().ui.shortcuts.is_empty());
        assert_eq!(h.state().ui.shortcut_sets, craft_keymap::Profiles::default());

        // `ui.dialog.cancel` from the control channel does the same.
        h.state_mut().run("app.keyboardShortcuts", json!({"command": "app.palette"})).unwrap();
        h.run();
        let quick_apply = row(&mut h, "Quick Apply…", "app.palette");
        click(&mut h, &quick_apply);
        press(&mut h, Modifiers::NONE, Key::F2);
        assert_eq!(shortcut_of(h.state(), "app.palette").as_deref(), Some("F2"));
        crate::dialogs::cancel(h.state_mut());
        h.run();
        assert!(!open(&h));
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
        app.services.pick_open = Some(Box::new(move |purpose| (purpose == PURPOSE).then(|| picked.clone())));
        app.run("app.keyboardShortcuts", json!({})).unwrap();
        let mut h = harness(app);
        click(&mut h, "Import…");
        assert!(has(&h, "Import “Studio”"));
        assert!(h.state().ui.shortcuts.is_empty(), "the review changes nothing");
        click(&mut h, "Import as New Set");
        assert!(has(&h, "Imported as “Studio”"));
        let panel = |h: &Harness<'_, DesignApp>| crate::keys::shortcut(&h.state().keymap, &crate::keys::panel("character"));
        assert_eq!(panel(&h).as_deref(), Some("Cmd+T"));
        assert_eq!(shortcut_of(h.state(), "app.characterPanel").as_deref(), Some("Cmd+T"), "the panel's twin keeps the key they share");
        assert_eq!(crate::keys::shortcut(&h.state().keymap, &crate::keys::tool("pen")).as_deref(), Some("Cmd+Shift+F3"));
        click(&mut h, "Done");
        click(&mut h, "OK");
        assert!(!open(&h));
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
        let text = DcText { lang: "" };
        for s in strings() {
            assert_eq!(text.ui(s), s);
        }
        assert_eq!(craft_keymap::ui::STRINGS.len(), 175);
        assert_eq!(craft_keymap::ui::PLURALS.len(), 11);
        assert_eq!(text.plural(2, "{n} command will change", "{n} commands will change"), "2 commands will change");
        assert_eq!(DcText { lang: "fr" }.ui("Cancel"), tr("fr", "Cancel"));
    }
}
