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
