use super::bar::menu_button;
use super::invoke::activate;
use super::state::{checked, menu_enabled, shortcut_label};
use super::tables::{MENUS, WORKSPACES};
use super::ui_commands::shown;
use super::{DesignApp, Item, Value, cmd_item, json};

pub(super) fn parse_entries(entries: &[&str]) -> Vec<Item> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let e = entries[i];
        i += 1;
        if e == "-" {
            out.push(Item::Sep);
        } else if let Some(name) = e.strip_prefix('>') {
            // Find the matching `<` (submenus nest).
            let start = i;
            let mut depth = 1;
            while i < entries.len() {
                if entries[i].starts_with('>') {
                    depth += 1;
                } else if entries[i] == "<" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            let sub = parse_entries(&entries[start..i]);
            i += 1;
            out.push(Item::Sub(name.to_string(), sub));
        } else if e == "ui:window.panel" {
            let items = crate::dock::DOCK_TABS
                .iter()
                .chain(crate::dock::ICON_PANELS)
                .map(|(id, label, _)| Item::Cmd { label: label.to_string(), id: "window.panel".into(), params: json!({"panel": id}), shortcut: None })
                .collect();
            out.push(Item::Sub("Panels".into(), items));
        } else if e == "ui:window.workspace" {
            // Shown as InDesign's "[Essentials]"; Reset names the current workspace (see `menu_items`).
            let mut items: Vec<Item> = WORKSPACES
                .iter()
                .map(|w| Item::Cmd { label: w.to_string(), id: "window.workspace".into(), params: json!({"name": w}), shortcut: None })
                .collect();
            items.push(Item::Sep);
            items.push(cmd_item("window.resetWorkspace", Value::Null));
            items.push(cmd_item("window.newWorkspace", Value::Null));
            out.push(Item::Sub("Workspace".into(), items));
        } else if e == "ui:window.brightness" {
            let items = crate::theme::Brightness::ALL
                .iter()
                .map(|b| Item::Cmd {
                    label: b.label().to_string(),
                    id: "window.brightness".into(),
                    params: json!({"brightness": b.id()}),
                    shortcut: None,
                })
                .collect();
            out.push(Item::Sub("Interface Color Theme".into(), items));
        } else {
            let (_, id) = e.split_once(':').unwrap_or(("cmd", e));
            // `id|Label|{params}`: a fixed-parameter variant of a command.
            let mut parts = id.splitn(3, '|');
            let id = parts.next().unwrap_or(id);
            match (parts.next(), parts.next().and_then(|j| serde_json::from_str::<Value>(j).ok())) {
                (Some(label), Some(params)) => {
                    let mut it = cmd_item(id, params);
                    if let Item::Cmd { label: l, .. } = &mut it {
                        *l = label.to_string();
                    }
                    out.push(it);
                }
                _ => out.push(cmd_item(id, Value::Null)),
            }
        }
    }
    out
}

/// The whole menu tree (InDesign order).
pub fn menu_tree() -> Vec<(&'static str, Vec<Item>)> {
    MENUS.iter().map(|(m, e)| (*m, parse_entries(e))).collect()
}

/// A row of the macOS application menu (the menu named after the app).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMenuItem {
    /// Runs `command` (no parameters); `id` is the native menu item id, `label` goes through `i18n::tr`.
    Cmd {
        id: &'static str,
        label: &'static str,
        command: &'static str,
    },
    Sep,
    /// Interface language: a submenu with each UI language in its own name, the current one
    /// checked (`app.language {lang}`).
    Language,
    Services,
    Hide,
    HideOthers,
    ShowAll,
}

/// The macOS application menu. Settings… is `app.preferences`, which `native_menu_tree` leaves out of Edit.
pub const MAC_APP_MENU: &[AppMenuItem] = &[
    AppMenuItem::Cmd { id: "dc-about", label: "About DesignCraft", command: "help.about" },
    AppMenuItem::Cmd { id: "dc-updates", label: "Check for Updates…", command: "help.checkForUpdates" },
    AppMenuItem::Cmd { id: "dc-discord", label: "Join the ArtCraft Discord…", command: "help.discord" },
    AppMenuItem::Sep,
    AppMenuItem::Cmd { id: "dc-settings", label: "Settings…", command: "app.preferences" },
    AppMenuItem::Language,
    AppMenuItem::Sep,
    AppMenuItem::Services,
    AppMenuItem::Sep,
    AppMenuItem::Hide,
    AppMenuItem::HideOthers,
    AppMenuItem::ShowAll,
    AppMenuItem::Sep,
    // Not the predefined Quit: it terminates the app at once, past the unsaved-changes prompt.
    AppMenuItem::Cmd { id: "dc-quit", label: "Quit DesignCraft", command: "app.quit" },
];

/// A label as the macOS menu bar library takes it: it reads "&" as a mnemonic marker and drops
/// it, and shows "&&" as one "&".
pub fn native_label(label: &str) -> String {
    label.replace('&', "&&")
}

/// The menu tree for the macOS menu bar: `menu_tree` without the commands `MAC_APP_MENU` holds,
/// so each command (and its shortcut) appears once, and with the standard Window › Minimize (⌘M)
/// at the top of the Window menu.
pub fn native_menu_tree() -> Vec<(&'static str, Vec<Item>)> {
    let in_app_menu = |id: &str| MAC_APP_MENU.iter().any(|m| matches!(m, AppMenuItem::Cmd { command, .. } if *command == id));
    let mut tree = menu_tree();
    for (title, entries) in &mut tree {
        without_commands(entries, &in_app_menu);
        if *title == "Window" {
            let minimize = Item::Cmd { label: "Minimize".into(), id: "window.minimize".into(), params: Value::Null, shortcut: Some("Cmd+M") };
            entries.splice(0..0, [minimize, Item::Sep]);
        }
    }
    tree
}

/// Drops the parameterless commands `drop` matches from `entries` (recursively), then any separator
/// the removal left at an end or next to another.
fn without_commands(entries: &mut Vec<Item>, drop: &dyn Fn(&str) -> bool) {
    let before = entries.len();
    entries.retain(|e| !matches!(e, Item::Cmd { id, params, .. } if params.is_null() && drop(id)));
    for e in entries.iter_mut() {
        if let Item::Sub(_, children) = e {
            without_commands(children, drop);
        }
    }
    if entries.len() == before {
        return;
    }
    let mut tidy: Vec<Item> = Vec::with_capacity(entries.len());
    for e in entries.drain(..) {
        if matches!(e, Item::Sep) && tidy.last().is_none_or(|l| matches!(l, Item::Sep)) {
            continue;
        }
        tidy.push(e);
    }
    if matches!(tidy.last(), Some(Item::Sep)) {
        tidy.pop();
    }
    *entries = tidy;
}

/// The key a menu item is hidden by (Edit › Menus).
pub fn menu_key(menu: &str, label: &str) -> String {
    format!("{menu}/{label}")
}

/// Whether any shown item is in `rest`, the entries after a separator: the separator is drawn only then, so hidden
/// CJK commands leave no separator at the end of a menu (in the window and in the native macOS menu bar).
pub fn has_shown_item(app: &DesignApp, rest: &[Item]) -> bool {
    rest.iter().any(|e| match e {
        Item::Cmd { id, .. } => shown(app, id),
        Item::Sep => false,
        Item::Sub(..) => true,
    })
}

/// Draws a menu's items; returns how many were hidden.
pub(super) fn menu_items(app: &mut DesignApp, ui: &mut egui::Ui, items: &[Item], path: &str) -> usize {
    let mut hidden = 0;
    for (i, it) in items.iter().enumerate() {
        match it {
            Item::Sep => {
                if has_shown_item(app, &items[i + 1..]) {
                    ui.separator();
                }
            }
            Item::Sub(name, children) => {
                let sub = format!("{path}/{name}");
                let shown = crate::i18n::tr(&app.ui.language, name).to_owned();
                menu_button(ui, crate::rtl::widget(ui, shown), |ui| {
                    if sub == OPEN_RECENT {
                        recent_menu_items(app, ui);
                    }
                    hidden += menu_items(app, ui, children, &sub);
                });
            }
            Item::Cmd { id, .. } if !shown(app, id) => {}
            Item::Cmd { label, .. } if !app.ui.show_full_menus && app.ui.hidden_menu_items.contains(&menu_key(path, label)) => hidden += 1,
            Item::Cmd { label, id, params, shortcut: _ } => {
                let lang = &app.ui.language;
                let label = match id.as_str() {
                    "window.workspace" => format!("[{}]", crate::i18n::workspace_name(lang, label)),
                    "window.resetWorkspace" => format!("{} {}", crate::i18n::tr(lang, "Reset"), crate::i18n::workspace_name(lang, &app.ui.workspace)),
                    _ => crate::i18n::tr(lang, label).to_owned(),
                };
                let text = match checked(app, id, params) {
                    Some(true) => format!("✓ {label}"),
                    Some(false) => format!("   {label}"),
                    None => label.to_owned(),
                };
                let mut b = egui::Button::new(crate::rtl::widget(ui, text));
                if let Some(sc) = shortcut_label(app, id, params) {
                    b = b.shortcut_text(sc);
                }
                if ui.add_enabled(menu_enabled(app, id), b).clicked() {
                    activate(app, id, params);
                    ui.close();
                }
            }
        }
    }
    hidden
}

/// The File › Open Recent submenu, whose files come before its fixed items.
pub const OPEN_RECENT: &str = "File/Open Recent";

/// A recent file's menu label: its file name.
pub fn recent_label(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string())
}

/// File › Open Recent's files (`app.openRecent {index}`), then a separator; nothing when empty.
pub fn recent_items(app: &DesignApp) -> Vec<Item> {
    let mut out: Vec<Item> = app
        .session
        .prefs
        .recent_files
        .iter()
        .enumerate()
        .map(|(i, path)| Item::Cmd { label: recent_label(path), id: "app.openRecent".into(), params: json!({"index": i}), shortcut: None })
        .collect();
    if !out.is_empty() {
        out.push(Item::Sep);
    }
    out
}

/// The recent files at the top of File › Open Recent: file names (not translated), the full path
/// as tooltip, dimmed when the file is gone (choosing it then says so).
fn recent_menu_items(app: &mut DesignApp, ui: &mut egui::Ui) {
    let dim = crate::theme::Tokens::get(ui.ctx()).text_dim;
    for it in recent_items(app) {
        match it {
            Item::Cmd { label, id, params, .. } => {
                let path = params["index"].as_u64().and_then(|i| app.session.prefs.recent_files.get(i as usize)).cloned().unwrap_or_default();
                let mut text = egui::RichText::new(label);
                if !crate::chrome::recent_file_stat(ui.ctx(), &path).exists {
                    text = text.color(dim);
                }
                if ui.button(crate::rtl::widget(ui, text)).on_hover_text(&path).clicked() {
                    activate(app, &id, &params);
                    ui.close();
                }
            }
            Item::Sep => {
                ui.separator();
            }
            Item::Sub(..) => {}
        }
    }
}

/// Every menu item as (menu path, label), for Edit › Menus.
pub fn all_menu_items() -> Vec<(String, String)> {
    fn walk(items: &[Item], path: &str, out: &mut Vec<(String, String)>) {
        for it in items {
            match it {
                Item::Sub(name, children) => walk(children, &format!("{path}/{name}"), out),
                Item::Cmd { label, .. } => out.push((path.to_string(), label.clone())),
                Item::Sep => {}
            }
        }
    }
    let mut out = Vec::new();
    for (menu, entries) in menu_tree() {
        walk(&entries, menu, &mut out);
    }
    out
}
