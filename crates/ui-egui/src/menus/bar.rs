use std::cell::RefCell;

use super::invoke::activate;
use super::tree::{menu_items, menu_tree};
use super::{DesignApp, Item, Value};

/// A menu button whose popup stays within the viewport and scrolls when needed.
/// All menu buttons share this wrapper, including workspace, zoom and panel menus.
pub fn menu_button<'a, R>(
    ui: &mut egui::Ui,
    atoms: impl egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    use egui::containers::menu::{MenuButton, MenuConfig};

    // A menu opens from a button already shown in the previous frame. Base its height on
    // that anchor, not the popup's eventual position, so placement cannot oscillate.
    let button = ui.ctx().read_response(ui.next_auto_id()).map(|response| response.rect);
    let max_height = menu_max_height(ui, button);
    let contents = |ui: &mut egui::Ui| {
        // Override the popup's initial available height, including when it opens upwards.
        ui.set_max_height(max_height);
        let mut scroll = egui::ScrollArea::vertical().max_height(max_height).min_scrolled_height(1.0);
        if ui.is_sizing_pass() {
            // Closing and reopening a menu starts at its first command again.
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show(ui, add_contents).inner
    };
    // Scrollbar clicks must not dismiss the popup; command rows close it explicitly.
    let config = MenuConfig::find(ui).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (response, inner) = if egui::containers::menu::is_in_menu(ui) {
        clipped_submenu(ui, atoms, config, contents)
    } else {
        MenuButton::new(atoms).config(config).ui(ui, contents)
    };
    egui::InnerResponse { response, inner: inner.map(|response| response.inner) }
}

/// The tallest menu that fits on either side of its anchor, including the popup frame.
pub(super) fn menu_max_height(ui: &egui::Ui, button: Option<egui::Rect>) -> f32 {
    let screen = ui.ctx().content_rect();
    let space = match button {
        // Submenus open beside their row, aligned with its top or bottom.
        Some(rect) if egui::containers::menu::is_in_menu(ui) => (screen.bottom() - rect.top()).max(rect.bottom() - screen.top()),
        Some(rect) => (screen.bottom() - rect.bottom()).max(rect.top() - screen.top()),
        None => screen.height(),
    };
    let chrome = egui::Frame::menu(ui.style()).total_margin().sum().y + 8.0;
    (space.min(screen.height()) - chrome).max(1.0)
}

/// egui's submenu hover test uses the full row rect, even outside a scroll area's clip.
fn clipped_submenu<'a, R>(
    ui: &mut egui::Ui,
    atoms: impl egui::IntoAtoms<'a>,
    config: egui::containers::menu::MenuConfig,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::Response, Option<egui::InnerResponse<R>>) {
    use egui::containers::menu::{MenuState, SubMenu, SubMenuButton};

    let id = SubMenu::id_from_widget_id(ui.next_auto_id());
    let open = MenuState::from_ui(ui, |state, _| state.open_item == Some(id));
    let inactive = ui.style().visuals.widgets.inactive;
    if open {
        ui.style_mut().visuals.widgets.inactive = ui.style().visuals.widgets.open;
    }
    let button = SubMenuButton::new(atoms).config(config);
    let response = ui.add(button.button);
    ui.style_mut().visuals.widgets.inactive = inactive;
    let clip = ui.clip_rect();
    if !response.rect.intersect(clip).is_positive() {
        if open {
            MenuState::from_ui(ui, |state, _| state.open_item = None);
        }
        return (response, None);
    }
    // Undo the half-spacing expansion used by SubMenu::show after clamping to the clip.
    let hover_margin = ui.spacing().item_spacing / 2.0;
    let mut clipped = response.clone();
    clipped.rect = response.rect.expand2(hover_margin).intersect(clip).shrink2(hover_margin);
    clipped.interact_rect = response.interact_rect.intersect(clip);
    let inner = button.sub_menu.show(ui, &clipped, contents);
    (response, inner)
}

/// What a row of the menu bar or of a canvas context menu asked for. The rows read the app; the caller applies the pick
/// once the menus are drawn.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Pick {
    /// Run a menu command.
    Run(String, Value),
    /// Show All Menu Items: the rows Edit › Menus hid show again.
    ShowAll,
}

impl Pick {
    /// Does what the row asked: the command through the menus' own dispatch.
    pub(super) fn apply(self, app: &mut DesignApp) {
        match self {
            Pick::Run(id, params) => activate(app, &id, &params),
            Pick::ShowAll => app.ui.show_full_menus = true,
        }
    }
}

/// A label as a framework menu row or title takes it: the framework reads `&` as the mnemonic marker and `&&` as one
/// `&`, so "Grids & Guides" keeps its ampersand.
pub(crate) fn row_label(label: &str) -> String {
    label.replace('&', "&&")
}

/// Runs `body` with the framework's words and layout direction in the interface language `lang`, as the dock gives
/// its panels: the menus outside the dock (the menu bar, Quick Apply) read the same words.
pub(super) fn in_app_env<R>(ui: &mut egui::Ui, lang: &str, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    use craft_ui::runtime::env::Direction;
    let rtl = crate::i18n::is_rtl(lang);
    #[cfg(not(target_arch = "wasm32"))]
    let words = crate::framework_host::words(lang);
    craft_ui::runtime::env::with_env(
        ui,
        |env| {
            env.direction = Some(if rtl { Direction::Rtl } else { Direction::Ltr });
            #[cfg(not(target_arch = "wasm32"))]
            {
                env.words = Some(craft_ui::runtime::env::WordsOverride::Replace(words));
            }
        },
        body,
    )
}

/// The menu bar (inside the app bar; macOS uses the native menu instead): the framework's `MenuBar`, one title per menu
/// of [`menu_tree`], in reading order (a right-to-left interface lays them out from the right).
///
/// The bar leaves F10, Alt and Alt+letter to the app: DesignCraft's keymap has F10 (Stroke panel) and Alt chords of its
/// own, and its menus never took the keyboard.
pub fn menu_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    use craft_ui::contract::Show as _;
    use craft_ui::menu::{MenuBar, MenuTitle};
    let lang = app.ui.language.clone();
    let picked: RefCell<Option<Pick>> = RefCell::new(None);
    {
        let shown: &DesignApp = app;
        let picked = &picked;
        let titles: Vec<MenuTitle<'_>> = menu_tree()
            .into_iter()
            .map(|(menu, entries)| {
                MenuTitle::new(row_label(crate::i18n::tr(&lang, menu)), move |ui: &mut egui::Ui| {
                    if let Some(pick) = menu_column(shown, ui, &[(menu, entries.as_slice())]) {
                        *picked.borrow_mut() = Some(pick);
                    }
                })
            })
            .collect();
        let _ = in_app_env(ui, &lang, |ui| MenuBar::new(crate::i18n::tr(&lang, "Menus").to_owned(), titles).keyboard(false).show(ui));
    }
    if let Some(pick) = picked.into_inner() {
        pick.apply(app);
    }
}

/// One menu's rows, as the menu bar and the canvas context menus show them inside a framework `Menu`: runs of items,
/// each with the menu path its rows are hidden by (Edit › Menus), then Show All Menu Items when a row is hidden. The
/// row that ran this pass, if any.
pub(super) fn menu_column<P: AsRef<str>, I: AsRef<[Item]>>(app: &DesignApp, ui: &mut egui::Ui, runs: &[(P, I)]) -> Option<Pick> {
    use craft_ui::contract::Show as _;
    use craft_ui::display::Divider;
    use craft_ui::menu::MenuItem;
    let mut pick = None;
    let mut hidden = 0;
    for (path, items) in runs {
        hidden += menu_items(app, ui, items.as_ref(), path.as_ref(), &mut pick);
    }
    if hidden > 0 && !app.ui.show_full_menus {
        Divider::horizontal().show(ui);
        // The menu stays open and shows every row, as InDesign's does.
        let label = row_label(crate::i18n::tr(&app.ui.language, "Show All Menu Items"));
        if MenuItem::new(label).keep_open(true).show(ui).ran {
            pick = Some(Pick::ShowAll);
        }
    }
    pick
}

/// The menu bar's submenu at `Menu/Submenu`.
pub(super) fn menu_bar_submenu(tree: &[(&str, Vec<Item>)], path: &str) -> Option<Item> {
    let (menu, sub) = path.split_once('/')?;
    tree.iter().find(|(m, _)| *m == menu)?.1.iter().find(|it| matches!(it, Item::Sub(name, _) if name == sub)).cloned()
}
