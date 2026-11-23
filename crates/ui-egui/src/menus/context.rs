use super::bar::{in_app_env, menu_bar_submenu, menu_column, row_label};
use super::tree::{menu_tree, parse_entries};
use super::{DesignApp, Item};

/// What a right-click on the canvas opens a menu for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenu {
    /// The selected objects.
    Object,
    /// The text being edited with the Type tool.
    Text,
    /// The page or pasteboard, where nothing was hit.
    Canvas,
}

// Canvas context menus, in the menu bar's entry syntax; `menu:Menu/Submenu` inserts that menu bar
// submenu as it is.
pub(super) const CONTEXT_OBJECT: &[&str] = &[
    "cmd:edit.undo",
    "cmd:edit.redo",
    "-",
    "cmd:edit.cut",
    "cmd:edit.copy",
    "cmd:edit.paste",
    "cmd:edit.pasteInto",
    "cmd:edit.pasteInPlace",
    "-",
    ">Transform",
    "cmd:transform.rotate|Rotate 90° Clockwise|{\"angle\": -90}",
    "cmd:transform.rotate|Rotate 90° Counterclockwise|{\"angle\": 90}",
    "cmd:transform.rotate|Rotate 180°|{\"angle\": 180}",
    "-",
    "cmd:transform.flip|Flip Horizontal|{\"axis\": \"horizontal\"}",
    "cmd:transform.flip|Flip Vertical|{\"axis\": \"vertical\"}",
    "-",
    "cmd:transform.clear",
    "<",
    "menu:Object/Arrange",
    "menu:Object/Select",
    "-",
    "cmd:object.group",
    "cmd:object.ungroup",
    "cmd:object.lock",
    "cmd:object.unlockAll",
    "cmd:object.hide",
    "cmd:object.showAll",
    "-",
];

/// Added to the object menu when a graphic frame is selected.
pub(super) const CONTEXT_GRAPHIC_FRAME: &[&str] = &["menu:Object/Fitting"];

pub(super) const CONTEXT_OBJECT_TAIL: &[&str] = &["menu:Object/Content", "menu:Object/Effects"];

/// Added to the object menu when a text frame is selected.
pub(super) const CONTEXT_TEXT_FRAME: &[&str] = &["-", "cmd:type.fillWithPlaceholder", "cmd:object.textFrameOptions"];

pub(super) const CONTEXT_TEXT: &[&str] = &[
    "cmd:edit.cut",
    "cmd:edit.copy",
    "cmd:edit.paste",
    "cmd:edit.pasteWithoutFormatting",
    "-",
    "menu:Type/Insert Special Character",
    "menu:Type/Insert White Space",
    "menu:Type/Insert Break Character",
    "cmd:type.fillWithPlaceholder",
    "-",
    "menu:Type/Change Case",
    "-",
    "ui:view.hiddenCharacters",
    "ui:app.storyEditor",
    "-",
    "cmd:edit.selectAll",
];

pub(super) const CONTEXT_CANVAS: &[&str] = &[
    "cmd:edit.undo",
    "cmd:edit.redo",
    "-",
    "cmd:edit.paste",
    "cmd:edit.pasteInPlace",
    "-",
    "ui:view.zoomIn",
    "ui:view.zoomOut",
    "ui:view.fitPage",
    "ui:view.fitSpread",
    "ui:view.actualSize",
    "-",
    "ui:view.rulers",
    "menu:View/Grids & Guides",
    "-",
    "cmd:edit.selectAll",
];

/// The menu-bar path context menu rows are keyed by (Edit › Menus doesn't list them); rows
/// inserted with `menu:` keep their menu bar path.
const CONTEXT_PATH: &str = "Context";

/// The entries of a canvas context menu for the current selection.
fn context_entries(app: &DesignApp, kind: ContextMenu) -> Vec<&'static str> {
    match kind {
        ContextMenu::Object => {
            let (graphic, text) = app.session.active().map_or((false, false), |st| {
                let items = || st.selection.items.iter().filter_map(|i| st.doc.item(*i));
                (items().any(|it| matches!(it.content, designcraft_doc::Content::Graphic(_))), items().any(|it| it.is_text_frame()))
            });
            let mut v = CONTEXT_OBJECT.to_vec();
            if graphic {
                v.extend(CONTEXT_GRAPHIC_FRAME);
            }
            v.extend(CONTEXT_OBJECT_TAIL);
            if text {
                v.extend(CONTEXT_TEXT_FRAME);
            }
            v
        }
        ContextMenu::Text => CONTEXT_TEXT.to_vec(),
        ContextMenu::Canvas => CONTEXT_CANVAS.to_vec(),
    }
}

/// Parses context menu entries: the menu bar's syntax plus `menu:Menu/Submenu` (not inside a `>`
/// submenu). Returns runs of items with the menu path they are hidden by: `Context`, or for an
/// inserted submenu its menu bar menu, so hiding a row in the menu bar hides it here too.
pub(super) fn context_parse(entries: &[&str]) -> Vec<(String, Vec<Item>)> {
    let tree = menu_tree();
    let mut out = Vec::new();
    let mut run: Vec<&str> = Vec::new();
    for e in entries {
        if let Some(path) = e.strip_prefix("menu:") {
            out.push((CONTEXT_PATH.to_owned(), parse_entries(&std::mem::take(&mut run))));
            let menu = path.rsplit_once('/').map_or(path, |(menu, _)| menu);
            out.push((menu.to_owned(), menu_bar_submenu(&tree, path).into_iter().collect()));
        } else {
            run.push(e);
        }
    }
    out.push((CONTEXT_PATH.to_owned(), parse_entries(&run)));
    out
}

/// Where and how a canvas context menu shows: its id, the canvas response it hangs from and the point it hangs at (where
/// the right-click was).
pub struct ContextPlace<'a> {
    pub id: egui::Id,
    pub opener: &'a egui::Response,
    pub at: egui::Pos2,
}

/// A canvas context menu of `kind` while `open` holds: the framework's `Menu` at the right-click, with the menu bar's
/// rows (labels, shortcuts, check marks, greyed when they can't run), scrolling when taller than the window. Escape, a
/// press outside or a row that runs closes it through `open`; the row runs before this returns.
pub fn context_menu(app: &mut DesignApp, ui: &mut egui::Ui, kind: ContextMenu, place: &ContextPlace<'_>, open: &mut bool) {
    use craft_ui::contract::overlay::Anchor;
    use craft_ui::contract::{Bind, Props as _, ShowContainer as _, Trigger};
    use craft_ui::menu::Menu;
    let runs = context_parse(&context_entries(app, kind));
    let lang = app.ui.language.clone();
    // The menu is named for what it acts on, with a menu bar title the interface already translates.
    let label = crate::i18n::tr(
        &lang,
        match kind {
            ContextMenu::Object => "Object",
            ContextMenu::Text => "Type",
            ContextMenu::Canvas => "View",
        },
    );
    let shown: &DesignApp = app;
    let picked = in_app_env(ui, &lang, |ui| {
        Menu::new(row_label(label))
            .id(place.id)
            .opened_by(place.opener, Trigger::Manual)
            .anchor(Anchor::Point(place.at))
            .open(Bind::bound(open))
            .show(ui, |ui, _| menu_column(shown, ui, &runs))
            .inner
            .flatten()
    });
    if let Some(pick) = picked {
        pick.apply(app);
    }
}
