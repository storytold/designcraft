use super::tree::{menu_items, menu_tree};
use super::{DesignApp, Item};

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

/// The menu bar contents (inside the app bar; macOS uses the native menu instead).
pub fn menu_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let lang = app.ui.language.clone();
    let mut menus = menu_tree();
    if crate::i18n::is_rtl(&lang) {
        menus.reverse();
    }
    for (menu, entries) in menus {
        menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&lang, menu)), |ui| menu_column(app, ui, &[(menu, entries)]));
    }
}

/// One menu's rows, as the menu bar and the canvas context menus show them: runs of items, each
/// with the menu path its rows are hidden by (Edit › Menus).
pub(super) fn menu_column<P: AsRef<str>>(app: &mut DesignApp, ui: &mut egui::Ui, runs: &[(P, Vec<Item>)]) {
    let lang = app.ui.language.clone();
    let rtl = crate::i18n::is_rtl(&lang);
    if rtl {
        ui.set_max_width(320.0);
    }
    ui.with_layout(egui::Layout::top_down_justified(if rtl { egui::Align::Max } else { egui::Align::Min }), |ui| {
        ui.set_min_width(240.0);
        let mut hidden = 0;
        for (path, items) in runs {
            hidden += menu_items(app, ui, items, path.as_ref());
        }
        if hidden > 0 && !app.ui.show_full_menus {
            ui.separator();
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Show All Menu Items"))).clicked() {
                app.ui.show_full_menus = true;
            }
        }
    });
}

/// The menu bar's submenu at `Menu/Submenu`.
pub(super) fn menu_bar_submenu(tree: &[(&str, Vec<Item>)], path: &str) -> Option<Item> {
    let (menu, sub) = path.split_once('/')?;
    tree.iter().find(|(m, _)| *m == menu)?.1.iter().find(|it| matches!(it, Item::Sub(name, _) if name == sub)).cloned()
}
