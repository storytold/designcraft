//! The menu bar tree (InDesign order), UI-only commands, keyboard shortcuts and Quick Apply.

use serde_json::{Value, json};

use crate::{DesignApp, ScreenMode};

mod bar;
mod context;
mod export;
mod invoke;
mod keys;
mod quick_apply;
mod run;
mod state;
mod tables;
mod tree;
mod ui_commands;

pub use bar::{menu_bar, menu_button};
#[cfg(test)]
use bar::{menu_bar_submenu, menu_column};
#[cfg(test)]
use context::{CONTEXT_CANVAS, CONTEXT_GRAPHIC_FRAME, CONTEXT_OBJECT, CONTEXT_OBJECT_TAIL, CONTEXT_TEXT, CONTEXT_TEXT_FRAME, context_parse};
pub use context::{ContextMenu, context_menu};
pub(crate) use export::{export_epub, export_pdf};
pub use invoke::{activate, activate_native, close_document};
#[cfg(not(target_arch = "wasm32"))]
pub use keys::paste_chord_command;
pub use keys::{forwarded_shortcuts, shortcuts};
#[cfg(test)]
use quick_apply::quick_commands;
pub use quick_apply::{QuickItem, command_search, palette, quick_apply_items};
pub use run::run_ui;
pub(crate) use run::save_then;
pub use state::{
    checked, default_shortcut, enabled, menu_enabled, native_menu_enabled, shortcut_label, shortcut_of, shortcut_text, tool_shortcut_label,
};
pub use tables::{CLASSIC_GROUPS, MENUS, TYPOGRAPHY_GROUPS, WORKSPACE_OPEN_PANELS, WORKSPACES, workspace_open_panel};
#[cfg(test)]
use tree::menu_items;
pub use tree::{
    AppMenuItem, MAC_APP_MENU, OPEN_RECENT, all_menu_items, has_shown_item, menu_key, menu_tree, native_label, native_menu_tree, recent_items,
    recent_label,
};
pub use ui_commands::{CJK_ONLY, UI_COMMANDS, shown, ui_label};

/// A menu entry, shared by the in-window menu bar and the native macOS menu.
#[derive(Clone, Debug)]
pub enum Item {
    Sep,
    Sub(String, Vec<Item>),
    Cmd { label: String, id: String, params: Value, shortcut: Option<&'static str> },
}

fn cmd_item(id: &str, params: Value) -> Item {
    let (label, shortcut) = match ui_label(id) {
        Some((l, sc)) => (l.to_string(), sc),
        None => match designcraft_engine::find_command(id) {
            Some(c) => (c.label.to_string(), c.shortcut),
            None => (id.to_string(), None),
        },
    };
    Item::Cmd { label, id: id.to_string(), params, shortcut }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod swedish_tests {
    use super::*;

    #[test]
    fn swedish_interface_is_selectable_and_translates_every_menu_label() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        run_ui(&mut app, "app.language", &json!({"lang": "sv"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "sv");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "sv"})), Some(true));
        assert_eq!(crate::i18n::tr("sv", "File"), "Arkiv");
        // Every menu title, item and submenu has a Swedish entry.
        let mut untranslated = Vec::new();
        for (title, entries) in menu_tree() {
            let mut labels = vec![title.to_string()];
            let mut stack: Vec<&Item> = entries.iter().collect();
            while let Some(entry) = stack.pop() {
                match entry {
                    Item::Sub(label, children) => {
                        labels.push(label.clone());
                        stack.extend(children.iter());
                    }
                    Item::Cmd { label, id, .. } if id != "app.language" => labels.push(label.clone()),
                    _ => {}
                }
            }
            // Swedish shares many words with English ("Layout", "Cyan"), so look for a catalog entry.
            let size = |l: &str| l.strip_suffix(" pt").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            untranslated.extend(labels.into_iter().filter(|l| l.chars().any(char::is_alphabetic) && !size(l) && !crate::i18n::swedish_has(l)));
        }
        assert!(untranslated.is_empty(), "untranslated Swedish menu labels: {untranslated:?}");
    }
}
