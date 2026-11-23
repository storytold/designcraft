//! Successors, on the framework menu bar, of four native menu tests: a long menu in a short window stays inside it and
//! reaches its last row, a menu reopens at the top, a tiny window does not force the menu taller than itself, and a menu
//! row is chosen by a click at its far end. They run the shipping window, with the craft UI runtime.

use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

use crate::test_window;

const ROWS: &[Role] = &[Role::MenuItem, Role::MenuItemRadio, Role::Button];

/// The window, with no document open, at `size`.
fn open(size: egui::Vec2) -> egui_kittest::Harness<'static, test_window::Window> {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    test_window::open_stepped(app, size, 1.0, 0.05)
}

/// The menu bar's `title` button.
fn bar_item(h: &egui_kittest::Harness<'static, test_window::Window>, title: &str) -> egui::Pos2 {
    h.query_all_by_label(title)
        .filter(|node| ROWS.contains(&node.accesskit_node().role()))
        .min_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .unwrap_or_else(|| panic!("no menu bar item {title:?}"))
        .rect()
        .center()
}

/// The row named `label` (a part of its name) of the open menu.
fn row(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> Option<egui::Rect> {
    h.query_all_by_label_contains(label).filter(|node| ROWS.contains(&node.accesskit_node().role())).map(|node| node.rect()).last()
}

fn open_menu(h: &mut egui_kittest::Harness<'static, test_window::Window>, title: &str) {
    let at = bar_item(h, title);
    test_window::click_at(h, at);
}

/// The bounds of the open menu: the smallest menu node holding the row `label`.
fn menu_bounds(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> Option<egui::Rect> {
    let row = row(h, label)?;
    h.query_all_by_role(Role::Menu)
        .map(|node| node.rect())
        .filter(|rect| rect.contains(row.center()) || rect.intersects(row))
        .min_by(|a, b| a.area().total_cmp(&b.area()))
}

/// Successor of `oversized_edit_menu_scrolls_to_preferences`: in a window shorter than the Edit menu the menu stays inside the
/// window, clicking in it does not dismiss it, its last row (Preferences…) is brought into view by scrolling, and a click on it opens
/// Preferences.
#[test]
fn the_framework_edit_menu_in_a_short_window_scrolls_to_preferences() {
    let mut h = open(vec2(700.0, 260.0));
    open_menu(&mut h, "Edit");
    let window = h.ctx.content_rect();
    let menu = menu_bounds(&h, "Undo").expect("the Edit menu is open");
    assert!(window.expand(1.0).contains_rect(menu), "the menu {menu:?} stays inside the window {window:?}");
    let prefs =
        h.query_all_by_label_contains("Preferences").find(|node| ROWS.contains(&node.accesskit_node().role())).expect("Preferences… is listed");
    prefs.scroll_to_me();
    h.run_steps(4);
    let rect = row(&h, "Preferences").expect("Preferences… is still listed");
    assert!(window.expand(1.0).contains_rect(rect), "scrolling brings Preferences… into view: {rect:?} in {window:?}");
    assert!(row(&h, "Undo").is_some(), "the menu stayed open while it scrolled");
    test_window::click_at(&mut h, rect.center());
    assert_eq!(h.state().app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
}

/// Successor of `menu_button_fits_its_anchor_and_resets_scroll_on_reopen`: a long menu fits its window wherever it is opened
/// from, and reopened after being scrolled it starts at its first row again.
#[test]
fn a_framework_menu_fits_its_window_and_reopens_at_the_top() {
    let mut h = open(vec2(700.0, 260.0));
    let window = h.ctx.content_rect();
    open_menu(&mut h, "Edit");
    let menu = menu_bounds(&h, "Undo").expect("the Edit menu is open");
    assert!(window.expand(1.0).contains_rect(menu), "the menu {menu:?} fits the window {window:?}");
    h.query_all_by_label_contains("Preferences")
        .find(|node| ROWS.contains(&node.accesskit_node().role()))
        .expect("Preferences… is listed")
        .scroll_to_me();
    h.run_steps(4);
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(row(&h, "Undo").is_none(), "Esc closes the menu");
    open_menu(&mut h, "Edit");
    let undo = row(&h, "Undo").expect("the reopened menu lists Undo");
    assert!(window.expand(1.0).contains_rect(undo), "the reopened menu starts at its first row: {undo:?} in {window:?}");
}

/// Successor of `tiny_menu_viewport_does_not_force_the_default_scroll_height`: in a very short window the menu is no taller than
/// the window.
#[test]
fn a_framework_menu_in_a_tiny_window_is_no_taller_than_it() {
    let mut h = open(vec2(700.0, 120.0));
    let window = h.ctx.content_rect();
    open_menu(&mut h, "Edit");
    let menu = menu_bounds(&h, "Undo").expect("the Edit menu is open");
    assert!(menu.height() <= window.height() + 1.0, "the menu {menu:?} is no taller than the window {window:?}");
}

/// Successor of `menu_rows_are_selectable_across_the_full_width`: a click at the far end of a menu row, past its text, chooses the
/// row.
#[test]
fn a_framework_menu_row_is_chosen_by_a_click_at_its_far_end() {
    let mut h = open(vec2(1000.0, 700.0));
    open_menu(&mut h, "Edit");
    let rect = row(&h, "Preferences").expect("Preferences… is listed");
    assert!(rect.width() >= 200.0, "the row is the menu's full width: {rect:?}");
    let far = egui::pos2(rect.right() - 6.0, rect.center().y);
    test_window::click_at(&mut h, far);
    assert!(h.state().app.ui.dialog.is_some(), "clicking the empty end of the Preferences… row chooses it");
}
