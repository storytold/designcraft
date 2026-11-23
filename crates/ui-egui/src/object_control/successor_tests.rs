//! Successors, on the framework Control bar, of the three native object Control panel tests the wave 4 cleanup removed: the second row's
//! height spinner edits the selection, the numeric controls stay inactive with nothing selected, and in a narrow window the
//! alignment controls are reached by scrolling the bar. They run the shipping window (the Advanced workspace), with the craft UI runtime.

use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::json;

use crate::test_window::{self, wheel};

fn window(selected: bool, size: egui::Vec2) -> egui_kittest::Harness<'static, test_window::Window> {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    app.run("file.new", json!({})).unwrap();
    app.run("window.workspace", json!({"name": "Advanced"})).unwrap();
    assert!(app.ui.control_bar);
    if selected {
        app.run("frame.create", json!({"rect": [20, 30, 80, 90]})).unwrap();
        app.run("edit.selectAll", json!({})).unwrap();
    }
    test_window::open_stepped(app, size, 1.0, 0.05)
}

/// The control bar's number field named like `part`: the topmost spin button or text field whose name contains it.
fn field<'a>(h: &'a egui_kittest::Harness<'static, test_window::Window>, part: &'a str) -> egui_kittest::Node<'a> {
    h.query_all_by_label_contains(part)
        .filter(|n| matches!(n.accesskit_node().role(), Role::SpinButton | Role::TextInput))
        .min_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .unwrap_or_else(|| panic!("no field named like {part:?} in the Control bar"))
}

/// Successor of `bottom_row_spinner_edits_height_inside_the_control_panel`: the Height field of the Control bar's second row is inside
/// the bar and one step up on it changes the selection's height by 1 through `transform.set`.
#[test]
fn control_bar_height_field_steps_the_selection_height() {
    let mut h = window(true, vec2(1440.0, 900.0));
    let before = crate::panels::sel_info(&h.state().app).unwrap().page_rect.height();
    let height = field(&h, "Height");
    let rect = height.rect();
    assert!(rect.bottom() <= 90.0, "the field is inside the Control bar: {rect:?}");
    test_window::click_at(&mut h, rect.center());
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(4);
    let after = crate::panels::sel_info(&h.state().app).unwrap().page_rect.height();
    assert!((after - before - 1.0).abs() < 1e-6, "the height spinner reaches transform.set: {before} -> {after}");
}

/// Successor of `empty_selection_keeps_numeric_controls_inactive`: with nothing selected the Control bar's number fields are disabled,
/// and a click on one does not attempt a transform.
#[test]
fn control_bar_numeric_fields_are_inactive_without_a_selection() {
    let mut h = window(false, vec2(1440.0, 900.0));
    let status = h.state().app.ui.status.clone();
    let height = field(&h, "Height");
    assert!(height.accesskit_node().is_disabled(), "the Height field is disabled");
    let at = height.rect().center();
    test_window::click_at(&mut h, at);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(3);
    assert_eq!(h.state().app.ui.status, status, "disabled spinners do not attempt transforms");
    assert!(crate::panels::sel_info(&h.state().app).is_none());
}

/// Successor of `narrow_windows_can_scroll_to_the_alignment_controls`: at the minimum width the Control bar overflows, and scrolling it
/// sideways brings its last group (the alignment buttons) into the window.
#[test]
fn a_narrow_window_scrolls_the_control_bar_to_the_alignment_controls() {
    let mut h = window(true, vec2(900.0, 700.0));
    let window = h.ctx.content_rect();
    let align = |h: &egui_kittest::Harness<'static, test_window::Window>| {
        h.query_all_by_label_contains("Align")
            .filter(|n| n.accesskit_node().role() == Role::Button && n.rect().min.y < 90.0)
            .map(|n| n.rect())
            .max_by(|a, b| a.right().total_cmp(&b.right()))
    };
    let before = align(&h).expect("the alignment buttons are in the Control bar");
    assert!(before.right() > window.right(), "the bar overflows at the minimum width: {before:?}");
    wheel(&mut h, egui::pos2(700.0, 30.0), vec2(-500.0, 0.0));
    h.run_steps(6);
    let after = align(&h).unwrap();
    assert!(after.left() < before.left() && after.right() <= window.right() + 1.0, "scrolling reaches the last group: {before:?} -> {after:?}");
}
