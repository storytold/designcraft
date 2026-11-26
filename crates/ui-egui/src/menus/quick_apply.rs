use craft_keymap::CommandRef;
use craft_ui::menu::search::{HelpEntry, SearchIndex, SearchWords, Synonyms};

use super::bar::in_app_env;
use super::state::shortcut_of;
use super::ui_commands::{CJK_ONLY, UI_COMMANDS, shown};
use super::{DesignApp, Value, json};

/// Quick Apply (⌘Return): the framework's command palette over the document's styles and every searched command. The
/// palette lists, ranks and picks; the app runs what was picked, as Quick Apply always did (`app.run`). An empty query
/// lists commands only: the styles need a query, else they would crowd the commands out.
pub fn palette(app: &mut DesignApp, ctx: &egui::Context) {
    use craft_ui::contract::Bind;
    use craft_ui::patterns::panels::CommandPalette;
    use craft_ui::runtime::env::Label;
    if app.ui.palette.is_none() {
        return;
    }
    let lang = app.ui.language.clone();
    let entries = palette_entries(app);
    let suggested: Vec<CommandRef> =
        quick_apply_items(&app.session, "", crate::cjk_features(app)).into_iter().map(|it| CommandRef::new(listed_command(&it.id))).collect();
    let mut open = true;
    // This area only supplies a parent: the palette mounts its own modal layer. The area spans the window from its
    // corner, so the palette's box centres in the window (an auto-placed area put it wherever egui found room).
    let screen = ctx.content_rect();
    let shown = egui::Area::new(egui::Id::unique("designcraft-quick-apply-host")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_max_size(screen.size());
        in_app_env(ui, &lang, |ui| {
            // The palette ranks through a word index of its entries, built here in the interface language's words; the
            // entries are this pass's (the document's styles change them).
            let env = craft_ui::runtime::env::Env::of(ui);
            let synonyms = Synonyms::of(&env.words::<SearchWords>());
            let index = SearchIndex::new(&env.view(), &entries, &synonyms);
            CommandPalette::new(crate::i18n::tr(&lang, "Quick Apply").to_owned(), &entries, &index, Bind::bound(&mut open))
                .placeholder(crate::i18n::tr(&lang, "Search styles and commands…").to_owned())
                .suggested((Label::from(crate::i18n::tr(&lang, "Commands").to_owned()), suggested))
                .show(ui)
        })
    });
    // The palette fired its pick on the framework's board too; the app drops what the board fires
    // (`DesignApp::raw_input_hook`), so the command runs once, here.
    if let Some(command) = shown.inner.picked {
        app.ui.palette = None;
        let (id, params) = picked_run(&command);
        let _ = app.run(&id, params);
    } else if !open {
        app.ui.palette = None;
    }
}

/// The palette's entries: the document's styles (named, with their kind as the path), then the searched commands in
/// the interface language, found by their English label too.
fn palette_entries(app: &DesignApp) -> Vec<HelpEntry> {
    let lang = app.ui.language.as_str();
    let cjk = crate::cjk_features(app);
    let mut out: Vec<HelpEntry> = Vec::new();
    if let Some(st) = app.session.active() {
        for (apply, kind, names) in style_names(&st.doc.styles) {
            let kind = crate::i18n::tr(lang, kind).to_owned();
            out.extend(names.into_iter().map(|name| HelpEntry::new(style_command(apply, &name), name).with_path([kind.clone()])));
        }
    }
    let commands = quick_commands().filter(|(id, _, _)| cjk || !CJK_ONLY.contains(id));
    out.extend(
        commands
            .map(|(id, label, _)| HelpEntry::new(CommandRef::new(listed_command(id)), crate::i18n::tr(lang, label).to_owned()).with_english(label)),
    );
    out
}

/// The command the palette lists for the searched command `id`: for an engine command whose default key a UI command
/// stands in for (`keys::build_catalog`: ⌘N is the New Document dialog, ⌘S `app.save`), that UI command, else `id`. The
/// key, the menu row and the palette's row then run the same command, so the row shows the key, and its pick works
/// without the path an engine command such as `file.saveAs` needs. The UI command's own entry lists the same command,
/// which the palette lists once.
pub(super) fn listed_command(id: &str) -> &str {
    let trigger = |shortcut: Option<&str>| shortcut.and_then(|key| craft_keymap::Binding::parse(key).ok()).map(|binding| binding.trigger);
    let Some(key) = designcraft_engine::find_command(id).and_then(|spec| trigger(spec.shortcut)) else { return id };
    UI_COMMANDS.iter().find(|command| trigger(command.2) == Some(key)).map_or(id, |command| command.0)
}

/// Each kind of style Quick Apply applies: the command that applies it, the kind's name, and the document's styles of
/// that kind Quick Apply lists (not the bracketed built-in ones).
fn style_names(styles: &designcraft_doc::Styles) -> [(&'static str, &'static str, Vec<String>); 3] {
    let named = |names: Vec<&String>| names.into_iter().filter(|name| !name.starts_with('[')).cloned().collect::<Vec<String>>();
    [
        (PARAGRAPH, "Paragraph Style", named(styles.paragraph.iter().map(|s| &s.name).collect())),
        (CHARACTER, "Character Style", named(styles.character.iter().map(|s| &s.name).collect())),
        (OBJECT, "Object Style", named(styles.object.iter().map(|s| &s.name).collect())),
    ]
}

const PARAGRAPH: &str = "style.paragraph.apply";
const CHARACTER: &str = "style.character.apply";
const OBJECT: &str = "style.object.apply";

/// The palette's command for applying the style `name` with `apply`: a command of its own on the board, which holds no
/// parameters (`style.paragraph.apply.Body_20Copy`). Bytes other than ASCII letters and digits are written `_XX`.
pub(super) fn style_command(apply: &str, name: &str) -> CommandRef {
    use std::fmt::Write as _;
    let mut id = format!("{apply}.");
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() {
            id.push(char::from(byte));
        } else {
            let _ = write!(id, "_{byte:02X}");
        }
    }
    CommandRef::new(id)
}

/// The style command `command` names, as the command that applies it and the style's name.
fn style_of(command: &CommandRef) -> Option<(&'static str, String)> {
    let (apply, encoded) =
        [PARAGRAPH, CHARACTER, OBJECT].into_iter().find_map(|apply| Some((apply, command.id.strip_prefix(apply)?.strip_prefix('.')?)))?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut rest = encoded.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'_' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    Some((apply, String::from_utf8(bytes).ok()?))
}

/// What running the palette's pick is: a style's apply command with its name, a keymap command as the menus run it
/// (`keys::runs`), and a searched command without parameters (`{}`), as Quick Apply has always run them.
pub(super) fn picked_run(command: &CommandRef) -> (String, Value) {
    if let Some((apply, name)) = style_of(command) {
        return (apply.to_owned(), json!({ "name": name }));
    }
    let (id, params) = crate::keys::runs(command);
    (id, if params.is_null() { json!({}) } else { params })
}

/// The app's commands on the framework's board while Quick Apply is open: the tools (as `toolbar::tool_catalog` has
/// them, for the Tools rail), every command Quick Apply searches and the document's styles, so the palette resolves
/// and picks each of them. The keymap's commands say the app runs their keys itself (`Dispatch::App`), so the rail and
/// the palette's rows show their chords while the app's keymap stays the one dispatcher of every key; the styles have
/// none. `None` while Quick Apply is closed, and when the board refuses the rows (a repeated id): the board then has
/// the tools alone.
///
/// One catalog per interface language and set of style names; the generation changes whenever the rows do.
#[cfg(not(target_arch = "wasm32"))]
pub fn quick_apply_catalog(app: &DesignApp) -> Option<craft_ui::runtime::commands::AppCatalog> {
    use craft_ui::runtime::commands::AppCatalog;
    use std::hash::{Hash as _, Hasher as _};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, PoisonError};
    /// The last catalog, by language and the hash of the style names; a new generation for each one built.
    static LAST: Mutex<Option<(String, u64, AppCatalog)>> = Mutex::new(None);
    /// Generations apart from `tool_catalog`'s, which counts its languages from 1.
    static GENERATION: AtomicU64 = AtomicU64::new(1 << 32);
    app.ui.palette.as_ref()?;
    let lang = app.ui.language.as_str();
    let styles = app.session.active().map(|st| style_names(&st.doc.styles));
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    styles.hash(&mut hasher);
    // The shown chords follow the keymap's platform and the user's overrides.
    app.keymap.platform().hash(&mut hasher);
    app.keymap.overrides().iter().for_each(|entry| entry.hash(&mut hasher));
    let key = hasher.finish();
    let mut last = LAST.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((held, held_key, catalog)) = last.as_ref()
        && held == lang
        && *held_key == key
    {
        return Some(catalog.clone());
    }
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let defs = board_defs(lang, styles.as_ref(), &app.keymap);
    let catalog = match AppCatalog::new(defs, generation) {
        Ok(catalog) => catalog,
        // The board refuses a repeated id; with none, the frame keeps the tools' catalog alone, as it does while Quick
        // Apply is closed.
        Err(error) => {
            log::warn!("Quick Apply's commands are not on the framework's board: {error}");
            return None;
        }
    };
    *last = Some((lang.to_owned(), key, catalog.clone()));
    Some(catalog)
}

/// The definitions of [`quick_apply_catalog`], labelled in `lang`: the keymap's tool and searched commands with the
/// chords `keymap` binds them to, which the app's own key handling runs (`Dispatch::App`), then a command for each style.
#[cfg(not(target_arch = "wasm32"))]
fn board_defs(
    lang: &str,
    styles: Option<&[(&'static str, &'static str, Vec<String>); 3]>,
    keymap: &craft_keymap::Keymap,
) -> Vec<craft_keymap::CommandDef> {
    use craft_ui::runtime::commands::Dispatch;
    let searched: std::collections::HashSet<&str> = quick_commands().map(|(id, _, _)| id).collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut defs: Vec<craft_keymap::CommandDef> = Vec::new();
    for def in crate::keys::catalog() {
        let tool = crate::keys::tool_of(&def.command);
        let quick = def.command.params.is_null() && searched.contains(def.command.id.as_str());
        if !(tool.is_some() || quick) || !seen.insert(def.command.id.clone()) {
            continue;
        }
        // Every keymap command on the board keeps the chords the app's keymap binds it to and says the app runs them:
        // the rail's tools and the palette's rows show them, and the app's keymap dispatches them.
        let mut def = def.clone();
        def.defaults = keymap.bindings(&def.command);
        def.label = crate::i18n::tr(lang, &def.label).to_owned();
        if let Some(icon) = designcraft_tools::TOOL_GROUPS.iter().flat_map(|g| g.iter()).find(|t| Some(t.id) == tool).map(|t| t.icon) {
            def = def.with_icon(icon);
        }
        defs.push(def.with_dispatch(Dispatch::App));
    }
    for (apply, _, names) in styles.into_iter().flatten() {
        for name in names {
            let command = style_command(apply, name);
            if seen.insert(command.id.clone()) {
                defs.push(craft_keymap::CommandDef::new(command, name.clone(), craft_keymap::Scope::app()));
            }
        }
    }
    defs
}

/// One Quick Apply entry: a style to apply or a command to run.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickItem {
    pub label: String,
    pub id: String,
    pub params: Value,
    pub shortcut: Option<String>,
    /// "Paragraph Style", "Character Style", "Object Style", or "" for commands.
    pub kind: &'static str,
}

/// Quick Apply matches (styles first, as InDesign lists them), at most 14.
pub fn quick_apply_items(session: &designcraft_engine::Session, query: &str, cjk: bool) -> Vec<QuickItem> {
    let ql = query.to_lowercase();
    let hit = |label: &str, id: &str| quick_match(&ql, &[label, id]);
    let mut out = Vec::new();
    if let Some(st) = session.active() {
        let styles = &st.doc.styles;
        let mut add = |name: &str, id: &str, kind: &'static str| {
            if name.starts_with('[') || !hit(name, "") {
                return;
            }
            out.push(QuickItem { label: name.to_string(), id: id.into(), params: json!({"name": name}), shortcut: None, kind });
        };
        for p in &styles.paragraph {
            add(&p.name, "style.paragraph.apply", "Paragraph Style");
        }
        for c in &styles.character {
            add(&c.name, "style.character.apply", "Character Style");
        }
        for o in &styles.object {
            add(&o.name, "style.object.apply", "Object Style");
        }
    }
    // Styles need a query, else they would crowd out the commands.
    if ql.is_empty() {
        out.clear();
    }
    let commands = quick_commands().filter(|(id, l, _)| hit(l, id) && (cjk || !CJK_ONLY.contains(id))).map(|(id, l, sc)| QuickItem {
        label: l.to_string(),
        id: id.to_string(),
        params: json!({}),
        shortcut: sc.map(str::to_string),
        kind: "",
    });
    out.extend(commands);
    out.truncate(14);
    out
}

/// UI commands search leaves out: they only run with a parameter search can't supply and have no
/// dialog to ask for it, or without one they reset a setting. Each stays in the menu, panel or
/// dialog that gives it the value (the workspace menus, Keyboard Shortcuts, Preferences…).
const NOT_SEARCHED: &[&str] = &[
    // The workspace to switch to or delete.
    "window.workspace",
    "window.deleteWorkspace",
    // A command and its shortcut, a menu item, a shortcut set; Reset Shortcuts clears every
    // custom shortcut, and the Keyboard Shortcuts dialog offers it.
    "window.setShortcut",
    "window.resetShortcuts",
    "window.importShortcuts",
    "window.hideMenuItem",
    // Without a value these reset the interface to 100% and the dark theme.
    "window.uiScale",
    "window.brightness",
    // A panel, and where it goes.
    "window.floatPanel",
    "window.dockPanel",
    "window.panel.layout",
    "window.panel.move",
    "window.panel.float",
    "window.panel.dock",
    "window.panel.activate",
    "window.panel.close",
    "window.panel.closeGroup",
    // A window edge, an app page, a recent file, a swatch or color group.
    "window.resize",
    "help.app",
    "app.openRecent",
    "app.swatchOptionsDialog",
    "app.colorGroupOptionsDialog",
    // Without a value these clear the flattener preset, go to Normal screen mode or 100% zoom.
    "app.flattener",
    "view.screenMode",
    "view.zoom",
    // Where to pin the Contextual Task Bar.
    "window.taskBarPin",
    // Shared docking commands DesignCraft doesn't provide, those that need a panel or a name,
    // and older ids that share a label with a searched docking command.
    "window.panel",
    "window.panel.window",
    "window.workspace.lock",
    "window.panel.closeOthers",
    "window.panel.minimize",
    "window.panel.iconic",
    "window.workspace.new",
    "window.workspace.delete",
    "ui.dock",
    "ui.dock.reset",
    "window.resetWorkspace",
];

/// The commands Quick Apply and the application bar's search find: (id, label, default shortcut).
pub(super) fn quick_commands() -> impl Iterator<Item = (&'static str, &'static str, Option<&'static str>)> {
    designcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.menu.is_empty() || c.shortcut.is_some())
        .map(|c| (c.id, c.label, c.shortcut))
        .chain(UI_COMMANDS.iter().filter(|c| !NOT_SEARCHED.contains(&c.0)).map(|c| (c.0, c.1, c.2)))
}

/// Whether any of `texts` contains the lowercased query (an empty query matches everything).
fn quick_match(query_lower: &str, texts: &[&str]) -> bool {
    query_lower.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(query_lower))
}

/// The application bar's search: the commands whose label in the interface language, English
/// label or id contains `query`, labelled in the interface language with the shortcut in effect.
/// An engine command a UI command stands in for (⌘S `app.save`) is listed as that command, as
/// Quick Apply lists it. Nothing for an empty query; at most 14.
pub fn command_search(app: &DesignApp, query: &str) -> Vec<QuickItem> {
    let ql = query.trim().to_lowercase();
    if ql.is_empty() {
        return Vec::new();
    }
    let lang = app.ui.language.as_str();
    let mut out: Vec<QuickItem> = Vec::new();
    for (id, label, _) in quick_commands() {
        let shown = crate::i18n::tr(lang, label);
        // The command the key and the menu row run for it (`listed_command`), so the row shows that key.
        let listed = listed_command(id);
        if out.len() >= 14 || !self::shown(app, id) || out.iter().any(|it| it.id == listed) || !quick_match(&ql, &[shown, label, id]) {
            continue;
        }
        out.push(QuickItem { label: shown.to_string(), id: listed.to_string(), params: Value::Null, shortcut: shortcut_of(app, listed), kind: "" });
    }
    out
}
