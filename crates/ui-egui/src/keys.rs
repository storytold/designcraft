//! Keyboard shortcuts on `craft-keymap`: the catalog (UI commands, engine commands, panels and
//! tools with their default keys and menu paths), shortcut text, and the edits that
//! `window.setShortcut` and the shortcut-set importer make.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use craft_keymap::{AssignError, Bindable, Binding, Chord, CommandRef, Key, Keymap, Kind, Mods, Overrides, Platform, Policy, Scope, Scopes, Trigger};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::menus::{self, Item};

/// The context bindings such as `Text: Cmd+B` name; active while text is being typed on the canvas.
pub const TEXT_CONTEXT: &str = "Text";

/// The command that opens a panel; a binding on it carries the panel id.
pub const PANEL_COMMAND: &str = "window.panel";
/// The command that selects a tool; a binding on it carries the tool id.
pub const TOOL_COMMAND: &str = "tool.select";

/// InDesign's default keys for the panels DesignCraft has, where DesignCraft's own defaults leave
/// the key free.
const PANEL_KEYS: &[(&str, &str)] = &[
    ("swatches", "F5"),
    ("color", "F6"),
    ("layers", "F7"),
    ("info", "F8"),
    ("stroke", "F10"),
    ("paragraphStyles", "F11"),
    ("characterStyles", "Shift+F11"),
    ("pages", "F12"),
    ("align", "Shift+F7"),
];

/// Other names shortcut sets use for a command (InDesign's feature names), by command id.
const IMPORT_ALIASES: &[(&str, &str)] = &[
    ("app.quit", "Exit"),
    ("app.preferences", "Preferences: General…"),
    ("app.insertTableDialog", "Insert Table"),
    ("view.togglePresentation", "Presentation"),
    ("color.defaultFillStroke", "Apply Default Fill and Stroke Colors"),
    ("color.swapFillStroke", "Swap Fill and Stroke Colors"),
    ("view.togglePreview", "Toggle View Setting Between Default and Preview"),
    ("type.bold", "Apply Bold"),
    ("type.italic", "Apply Italic"),
    ("window.hidePanels", "Show/Hide All Panels"),
    ("window.hidePanelsExceptTools", "Show/Hide All Panels Except Toolbox"),
    ("window.nextDocument", "Next Window"),
    ("window.previousDocument", "Previous Window"),
];

/// Commands that show and hide something without "Show/Hide" in their label. A shortcut set may
/// name a toggle by what it does now (`Hide Hidden Characters`); the importer matches such names
/// only to toggles.
const TOGGLES: &[&str] = &["view.hiddenCharacters", "view.tagMarkers"];

/// Paths only the importer reads, where an alias would leave two commands on one name: InDesign's
/// Tools area has Apply Color, which is also `object.color`'s label.
const IMPORT_PATHS: &[(&str, &[&str])] = &[("color.applyColor", &["Tools", "Apply Color"])];

/// Commands on text-editing chords (Cmd+Z, Cmd+A, and Ctrl+Y off macOS) that run while typing on the
/// canvas too: the text tool has no undo or select-all of its own, and Story Editor toggles from the
/// text. Their defaults get a copy in the Text context, which takes the chords from the text field.
const TYPING_COMMANDS: &[&str] = &["edit.undo", "edit.redo", "edit.selectAll", "app.storyEditor"];

/// Commands that open a panel, by panel id. The panel's binding gets the command's default key
/// too, so the two are twins: a shortcut set that gives either one the key leaves both with it.
const PANEL_TWINS: &[(&str, &str)] = &[
    ("character", "app.characterPanel"),
    ("paragraph", "app.paragraphPanel"),
    ("glyphs", "app.glyphsPanel"),
    ("characterStyles", "app.characterStylesPanel"),
    ("paragraphStyles", "app.paragraphStylesPanel"),
    ("table", "app.tablePanel"),
    ("textWrap", "object.textWrap"),
];

pub fn panel(id: &str) -> CommandRef {
    CommandRef::with_params(PANEL_COMMAND, json!({ "panel": id }))
}

pub fn tool(id: &str) -> CommandRef {
    CommandRef::with_params(TOOL_COMMAND, json!({ "tool": id }))
}

/// The command a menu row or a `window.setShortcut` call names.
pub fn command_ref(id: &str, params: &Value) -> CommandRef {
    match params {
        Value::Null => CommandRef::new(id),
        Value::Object(m) if m.is_empty() => CommandRef::new(id),
        p => CommandRef::with_params(id, p.clone()),
    }
}

/// The browser's operating system as [`note_platform`] recorded it, by [`platform_code`]; 0
/// before the first frame.
static WEB_PLATFORM: AtomicU8 = AtomicU8::new(0);

fn platform_code(p: Platform) -> u8 {
    match p {
        Platform::Mac => 1,
        Platform::Windows => 2,
        Platform::Linux => 3,
        _ => 0,
    }
}

/// The platform shortcuts follow: the build's, and on the web the browser's operating system, so a
/// browser on a Mac reads ⌘ as Cmd and leaves Control as Control.
pub fn platform() -> Platform {
    if !cfg!(target_arch = "wasm32") {
        return Platform::current();
    }
    match WEB_PLATFORM.load(Ordering::Relaxed) {
        1 => Platform::Mac,
        2 => Platform::Windows,
        3 => Platform::Linux,
        _ => Platform::current(),
    }
}

/// Records the platform egui reports, which the web build reads from the browser's user agent.
pub fn note_platform(ctx: &egui::Context) {
    if cfg!(target_arch = "wasm32") {
        WEB_PLATFORM.store(platform_code(craft_keymap::egui::platform(ctx)), Ordering::Relaxed);
    }
}

/// The keymap with the user's overrides, for [`platform`].
pub fn new_keymap(overrides: Overrides) -> Keymap {
    keymap_for(platform(), overrides)
}

/// The keymap for `platform`: the system's own chords are reserved, so neither a user edit nor an
/// imported set can take them.
pub fn keymap_for(platform: Platform, overrides: Overrides) -> Keymap {
    Keymap::new(catalog().to_vec(), Scopes::new().with_context(TEXT_CONTEXT), platform)
        .with_reserved(craft_keymap::reserved::system(platform))
        .with_overrides(overrides)
}

/// Every bindable command, in dispatch order: where two share keys the earlier one runs. UI
/// commands come first (⌘N is the New Document dialog, not a bare `file.new`), then engine
/// commands, panels and tools.
pub fn catalog() -> &'static [Bindable] {
    &built().0
}

/// Whether the Keyboard Shortcuts dialog lists `b`. Engine commands with neither a menu row nor a
/// default key are for scripts and the control channel (`file.save`, `selection.set`): the dialog
/// leaves them out, and they still take keys and count in conflicts.
pub fn listed(b: &Bindable) -> bool {
    !(b.command.params.is_null() && built().1.contains(b.command.id.as_str()))
}

fn built() -> &'static (Vec<Bindable>, HashSet<&'static str>) {
    static CATALOG: OnceLock<(Vec<Bindable>, HashSet<&'static str>)> = OnceLock::new();
    CATALOG.get_or_init(build_catalog)
}

/// The catalog and the engine commands [`listed`] leaves out.
fn build_catalog() -> (Vec<Bindable>, HashSet<&'static str>) {
    let paths = menu_paths();
    // An engine command's own menu placement counts only where no menu row stands: a row there
    // runs a UI command in its place (File › Save is the Save dialog when needed), and two
    // commands on one path leave the importer unable to pick either.
    let normalized = |path: &[String]| path.iter().map(|s| craft_keymap::import::normalize_label(s)).collect::<Vec<_>>();
    let rows: HashSet<Vec<String>> = paths.values().flatten().map(|p| normalized(p)).collect();
    let app = Scope::app();
    let mut out = Vec::new();
    let mut add = |command: CommandRef, label: &str, defaults: &[&str], fallback_path: Option<Vec<String>>, kind: Kind| {
        let mut b = Bindable::new(command, label, app.clone()).with_kind(kind);
        match paths.get(&b.command.key()) {
            Some(found) => b.paths.extend(found.iter().cloned()),
            None => b.paths.extend(fallback_path.filter(|p| !rows.contains(&normalized(p)))),
        }
        for binding in defaults.iter().filter_map(|d| Binding::parse(d).ok()) {
            b = b.with_default(binding);
        }
        out.push(b);
    };
    for (id, label, _, _) in menus::UI_COMMANDS {
        add(CommandRef::new(*id), label, menus::default_shortcut(id).as_slice(), None, Kind::Command);
    }
    // A UI command's default key runs it, never the engine command it stands in for (⌘S is the
    // Save dialog when needed, not a bare `file.save`): the engine command does not get that key.
    let ui_keys: Vec<Trigger> = menus::UI_COMMANDS
        .iter()
        .filter_map(|(id, ..)| menus::default_shortcut(id))
        .filter_map(|d| Binding::parse(d).ok())
        .map(|b| b.trigger)
        .collect();
    let mut internal = HashSet::new();
    for c in designcraft_engine::command_specs() {
        let shortcut = c.shortcut.filter(|d| Binding::parse(d).is_ok_and(|b| !ui_keys.contains(&b.trigger)));
        if c.menu.is_empty() && shortcut.is_none() && !paths.contains_key(c.id) {
            internal.insert(c.id);
        }
        let path = (!c.menu.is_empty()).then(|| c.menu.iter().map(|s| s.to_string()).chain([c.label.to_string()]).collect());
        add(CommandRef::new(c.id), c.label, shortcut.as_slice(), path, Kind::Command);
    }
    let mut seen = Vec::new();
    for (id, label, _) in crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS) {
        if seen.contains(id) {
            continue;
        }
        seen.push(id);
        let own = PANEL_KEYS.iter().find(|(p, _)| p == id).map(|(_, k)| *k);
        let twin = PANEL_TWINS.iter().find(|(p, _)| p == id).and_then(|(_, c)| menus::default_shortcut(c));
        let defaults: Vec<&str> = own.into_iter().chain(twin).collect();
        add(panel(id), label, &defaults, Some(vec!["Window".into(), (*label).into()]), Kind::Command);
    }
    for group in designcraft_tools::TOOL_GROUPS {
        let Some(first) = group.first() else { continue };
        for t in group.iter() {
            let kind = Kind::Tool { group: Some(first.id.to_string()) };
            add(tool(t.id), t.label, t.shortcut.as_slice(), None, kind);
        }
    }
    for b in &mut out {
        // Shortcut sets file tools under Tools, which is no menu here.
        if matches!(b.kind, Kind::Tool { .. }) {
            b.import_paths.push(vec!["Tools".into(), b.label.clone()]);
        }
        if !b.command.params.is_null() {
            continue;
        }
        let id = b.command.id.as_str();
        b.is_toggle = b.label.starts_with("Show/Hide ") || TOGGLES.contains(&id);
        if TYPING_COMMANDS.contains(&id)
            && let Some(text) = Scope::new(TEXT_CONTEXT)
        {
            let copies: Vec<Binding> = b.defaults.iter().filter(|d| d.scope.is_none()).map(|d| Binding::in_scope(d.trigger, text.clone())).collect();
            b.defaults.extend(copies);
        }
        b.aliases.extend(IMPORT_ALIASES.iter().filter(|(c, _)| *c == id).map(|(_, a)| a.to_string()));
        b.import_paths.extend(IMPORT_PATHS.iter().filter(|(c, _)| *c == id).map(|(_, p)| p.iter().map(|s| s.to_string()).collect()));
    }
    (out, internal)
}

/// Menu paths (menu, submenus, label) of every menu row, by command key.
fn menu_paths() -> HashMap<String, Vec<Vec<String>>> {
    fn walk(items: &[Item], path: &mut Vec<String>, out: &mut HashMap<String, Vec<Vec<String>>>) {
        for it in items {
            match it {
                Item::Sub(name, children) => {
                    path.push(name.clone());
                    walk(children, path, out);
                    path.pop();
                }
                Item::Cmd { label, id, params, .. } => {
                    let mut full = path.clone();
                    full.push(label.clone());
                    out.entry(command_ref(id, params).key()).or_default().push(full);
                }
                Item::Sep => {}
            }
        }
    }
    let mut out = HashMap::new();
    for (menu, entries) in menus::menu_tree() {
        walk(&entries, &mut vec![menu.to_string()], &mut out);
    }
    out
}

/// The binding a command shows: the first in its home scope, else its first.
fn shown_binding(keymap: &Keymap, cmd: &CommandRef) -> Option<Binding> {
    let bindings = keymap.bindings(cmd);
    bindings.iter().find(|b| b.scope.is_none()).or_else(|| bindings.first()).cloned()
}

/// The shortcut in effect for a command, as stored and sent ("Cmd+Alt+K", "Text: F7").
pub fn shortcut(keymap: &Keymap, cmd: &CommandRef) -> Option<String> {
    let b = shown_binding(keymap, cmd)?;
    let home = keymap.bindable(cmd).map_or_else(Scope::app, |c| c.scope.clone());
    Some(b.text(&home))
}

/// The shortcut in effect for a command, for menus and lists (⌥⌘K on macOS, Ctrl+Alt+K elsewhere).
pub fn shortcut_display(keymap: &Keymap, cmd: &CommandRef) -> Option<String> {
    shown_binding(keymap, cmd).map(|b| b.trigger.display(keymap.platform()))
}

/// A key press as shortcut text ("Cmd+Shift+K"); `None` for a key the keymap has no name for.
pub fn chord_text(m: egui::Modifiers, key: egui::Key) -> Option<String> {
    let key = Key::from_egui(key)?;
    Some(Chord::new(Mods::from_egui(&m, platform()), key).to_string())
}

/// Copies the keymap's overrides to the saved UI state and, outside the Keyboard Shortcuts
/// dialog (which saves sets itself), to the active shortcut set.
pub fn store(app: &mut DesignApp) {
    if app.ui.shortcuts != *app.keymap.overrides() {
        app.ui.shortcuts = app.keymap.overrides().clone();
    }
    if app.shortcut_dialog.is_none() {
        store_in_set(&mut app.ui);
    }
}

/// Saves `ui.shortcuts` to the active shortcut set when they differ. Edits of the built-in set go
/// to a new set, Custom, which becomes active.
pub fn store_in_set(ui: &mut crate::UiState) {
    let builtins = &crate::shortcuts_dialog::BUILTINS;
    if ui.shortcut_sets.active_overrides(builtins) == &ui.shortcuts {
        return;
    }
    let custom = crate::i18n::tr(&ui.language, crate::shortcuts_dialog::CUSTOM_SET);
    // Refused only with the most sets already made: ui.json still saves the shortcuts themselves,
    // and the next frame tries again.
    let _ = ui.shortcut_sets.store(ui.shortcuts.clone(), builtins, custom);
}

/// Gives a command keys (replacing its others), no keys (`""`), or its defaults back (`None`).
/// Other commands on the same keys keep them unless `policy` is [`Policy::Steal`]. Returns the
/// commands it took the keys from.
pub fn set(app: &mut DesignApp, cmd: &CommandRef, shortcut: Option<&str>, policy: Policy) -> Result<Vec<CommandRef>, String> {
    app.sync_keymap();
    if app.keymap.bindable(cmd).is_none() {
        return Err(format!("unknown command `{cmd}`"));
    }
    let mut taken = Vec::new();
    match shortcut.map(str::trim) {
        None => {
            app.keymap.reset(cmd);
        }
        Some("") => {
            if !app.keymap.clear(cmd) {
                return Err("too many changed shortcuts".into());
            }
        }
        Some(sc) => {
            let binding = Binding::parse(sc).map_err(|e| format!("can't read shortcut `{sc}`: {e}"))?;
            let done = app.keymap.set_bindings(cmd, vec![binding], policy).map_err(|e| assign_error(&app.keymap, sc, &e))?;
            taken = done.taken_from.into_iter().map(|h| h.command).collect();
        }
    }
    store(app);
    Ok(taken)
}

/// Why the keymap refused `sc`. A chord the system takes is named as the menus show it
/// ("⌘Tab is reserved by the system").
fn assign_error(keymap: &Keymap, sc: &str, e: &AssignError) -> String {
    match e {
        AssignError::Reserved(t) => format!("{} is reserved by the system", t.display(keymap.platform())),
        e => format!("can't assign `{sc}`: {e}"),
    }
}

/// Whether `a` and `b` are a panel and the command that opens it ([`PANEL_TWINS`]).
pub fn twins(a: &CommandRef, b: &CommandRef) -> bool {
    let one_way = |p: &CommandRef, c: &CommandRef| {
        p.id == PANEL_COMMAND
            && c.params.is_null()
            && p.params.get("panel").and_then(Value::as_str).is_some_and(|id| PANEL_TWINS.contains(&(id, c.id.as_str())))
    };
    one_way(a, b) || one_way(b, a)
}

/// Other commands on the keys `cmd` holds, by command key. A panel's twin is not a conflict: both
/// open the panel.
pub fn conflicts(keymap: &Keymap, cmd: &CommandRef) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in keymap.conflicts_with(cmd) {
        if twins(cmd, &c.b.0) {
            continue;
        }
        let key = c.b.0.key();
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATFORMS: [Platform; 3] = [Platform::Mac, Platform::Windows, Platform::Linux];

    /// No default shortcut sits on a chord an operating system takes before the app sees it.
    #[test]
    fn defaults_avoid_reserved_chords() {
        for p in PLATFORMS {
            let km = keymap_for(p, Overrides::new());
            let reserved: Vec<String> = km
                .catalog()
                .iter()
                .flat_map(|b| km.default_bindings(&b.command).into_iter().map(move |d| (b.command.key(), d)))
                .filter(|(_, d)| km.is_reserved(&d.trigger.chord))
                .map(|(k, d)| format!("{k} {}", d.trigger))
                .collect();
            assert_eq!(reserved, Vec::<String>::new(), "{p:?}: defaults on reserved chords");
        }
    }

    /// A chord the operating system takes, per platform.
    fn os_chord(p: Platform) -> &'static str {
        match p {
            Platform::Mac => "Cmd+Tab",
            Platform::Windows => "Alt+F4",
            _ => "Cmd+Alt+F2",
        }
    }

    /// Binding a chord the system takes is refused with a message naming it, and a saved override
    /// on one leaves the command its default.
    #[test]
    fn a_reserved_chord_is_refused() {
        let cmd = CommandRef::new("app.palette");
        for p in PLATFORMS {
            let mut km = keymap_for(p, Overrides::new());
            let before = km.bindings(&cmd);
            let sc = os_chord(p);
            let binding = Binding::parse(sc).unwrap();
            let shown = binding.trigger.display(p);
            let err = km.set_bindings(&cmd, vec![binding], Policy::Steal).unwrap_err();
            assert_eq!(assign_error(&km, sc, &err), format!("{shown} is reserved by the system"), "{p:?}");
            assert_eq!(km.bindings(&cmd), before, "{p:?}");
            let mut saved = Overrides::new();
            saved.insert(cmd.key(), vec![sc.to_string()]);
            assert_eq!(keymap_for(p, saved).bindings(&cmd), before, "{p:?}: a saved reserved chord");
        }
        // Through `window.setShortcut`, as the Keyboard Shortcuts dialog and the control channel set keys.
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let sc = os_chord(platform());
        let r = menus::run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": sc, "steal": true})).unwrap();
        assert!(r.unwrap_err().ends_with("is reserved by the system"));
        assert_eq!(menus::shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+Enter"));
    }

    /// The web build follows the operating system in the browser's user agent; an iPad counts as a
    /// Mac.
    #[test]
    fn browser_user_agent_picks_the_platform() {
        use egui::os::OperatingSystem as Os;
        let ctx = egui::Context::default();
        let of = |ua: &str| {
            ctx.set_os(Os::from_user_agent(ua));
            craft_keymap::egui::platform(&ctx)
        };
        let mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15";
        let ipad = "Mozilla/5.0 (iPad; CPU OS 17_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1";
        let windows = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
        let linux = "Mozilla/5.0 (X11; Linux x86_64; rv:125.0) Gecko/20100101 Firefox/125.0";
        let android = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36";
        assert_eq!(of(mac), Platform::Mac);
        assert_eq!(of(ipad), Platform::Mac);
        assert_eq!(of(windows), Platform::Windows);
        assert_eq!(of(linux), Platform::Linux);
        assert_eq!(of(android), Platform::Linux);
        // Native builds keep their own platform whatever egui reports.
        if !cfg!(target_arch = "wasm32") {
            ctx.set_os(Os::Mac);
            note_platform(&ctx);
            assert_eq!(platform(), Platform::current());
        }
    }

    /// An app typing in a text frame holding "Page ", with the caret at its end, and a function
    /// that runs one frame of the whole UI with `events` and `modifiers` held.
    fn typing_in_a_text_frame() -> (DesignApp, Value, impl FnMut(&mut DesignApp, egui::Modifiers, Vec<egui::Event>)) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let story = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Page "})).unwrap()["story"].clone();
        app.select_tool("type");
        app.run("text.select", json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
        assert!(app.session.wants_text());
        let ctx = egui::Context::default();
        let frame = move |app: &mut DesignApp, modifiers: egui::Modifiers, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events: [egui::Event::ModifiersChanged(modifiers)].into_iter().chain(events).collect(),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        (app, story, frame)
    }

    fn story_text(app: &mut DesignApp, story: &Value) -> String {
        app.session.execute("story.get", &json!({"story": story})).unwrap()["text"].as_str().unwrap().to_string()
    }

    /// A key press as egui sends it, with its release.
    fn press(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        [true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).into()
    }

    /// The command modifier as egui reports it on this platform.
    fn cmd() -> egui::Modifiers {
        if platform().is_mac() { egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND } else { egui::Modifiers::CTRL | egui::Modifiers::COMMAND }
    }

    /// Off macOS, Ctrl+Alt+Shift+N inserts the page number while typing; AltGr+E, which a browser
    /// on Windows reports as Ctrl+Alt+E (Fit Content to Frame) followed by the text it typed,
    /// types `€`.
    #[test]
    fn ctrl_alt_chords_run_while_typing_unless_they_type_text() {
        if platform().is_mac() {
            return;
        }
        let (mut app, story, mut frame) = typing_in_a_text_frame();
        frame(&mut app, Default::default(), vec![]);
        let ctrl_alt = egui::Modifiers::CTRL | egui::Modifiers::COMMAND | egui::Modifiers::ALT;
        frame(&mut app, ctrl_alt | egui::Modifiers::SHIFT, press(egui::Key::N, ctrl_alt | egui::Modifiers::SHIFT));
        assert_eq!(story_text(&mut app, &story), "Page \u{e000}");
        let mut altgr = press(egui::Key::E, ctrl_alt);
        altgr.insert(1, egui::Event::Text("€".into()));
        frame(&mut app, ctrl_alt, altgr);
        assert_eq!(story_text(&mut app, &story), "Page \u{e000}€");
    }

    /// While typing, Cmd+Z undoes the document's last edit and Cmd+A selects the story's text: the
    /// text tool has neither of its own.
    #[test]
    fn undo_and_select_all_work_while_typing() {
        let (mut app, story, mut frame) = typing_in_a_text_frame();
        frame(&mut app, Default::default(), vec![]);
        frame(&mut app, Default::default(), vec![egui::Event::Text("X".into())]);
        assert_eq!(story_text(&mut app, &story), "Page X");
        frame(&mut app, cmd(), press(egui::Key::Z, cmd()));
        assert_eq!(story_text(&mut app, &story), "Page ");
        assert!(app.session.wants_text(), "the caret stays");
        frame(&mut app, cmd(), press(egui::Key::A, cmd()));
        frame(&mut app, Default::default(), vec![egui::Event::Text("Y".into())]);
        assert_eq!(story_text(&mut app, &story), "Y");
    }

    /// Cmd+Shift+C centers the paragraph while typing, both as egui-winit sends it (a Copy event
    /// with the modifiers held) and as the web build does (the press, then Copy); nothing is
    /// copied.
    #[test]
    fn cmd_shift_c_centers_the_paragraph_while_typing() {
        let cmd_shift = cmd() | egui::Modifiers::SHIFT;
        let mut web = press(egui::Key::C, cmd_shift);
        web.insert(1, egui::Event::Copy);
        for (form, events) in [("desktop", vec![egui::Event::Copy]), ("web", web)] {
            let (mut app, _, mut frame) = typing_in_a_text_frame();
            frame(&mut app, Default::default(), vec![]);
            frame(&mut app, cmd_shift, events);
            let align = crate::panels::text_attrs(&mut app).unwrap()["para"]["align"].clone();
            assert_eq!(align, "center", "{form}");
        }
    }

    /// An engine command only scripts run has no row in the dialog, yet takes keys and conflicts.
    #[test]
    fn an_unlisted_engine_command_is_still_bindable_and_conflicts() {
        let find = |id: &str| catalog().iter().find(|b| b.command == CommandRef::new(id));
        let internal = find("selection.set").unwrap();
        assert!(!listed(internal));
        assert!(listed(find("edit.undo").unwrap()));
        assert!(catalog().iter().filter(|b| matches!(b.kind, Kind::Tool { .. })).all(listed));
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let r = menus::run_ui(&mut app, "window.setShortcut", &json!({"id": "selection.set", "shortcut": "Cmd+Z"})).unwrap().unwrap();
        assert_eq!(r["conflicts"], json!(["edit.undo"]));
    }

    /// A panel and the command that opens it share their key without a reported conflict.
    #[test]
    fn a_panels_twin_is_not_a_conflict() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        for steal in [false, true] {
            let r = menus::run_ui(&mut app, "window.setShortcut", &json!({"id": "app.characterPanel", "shortcut": "Cmd+T", "steal": steal}))
                .unwrap()
                .unwrap();
            assert_eq!(r["conflicts"], json!([]), "steal: {steal}");
        }
    }
}
