//! Successors, on the framework Tools rail, of two native tool flyout tests: a flyout whose rows have long translated labels stays
//! usable in a narrow window, and a flyout opened by holding its group button stays open after the release and closes on Esc
//! without changing the tool. Both run the shipping window, with the craft UI runtime.

use egui::{Event, Modifiers, PointerButton, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};

use crate::test_window::{self, wheel};

fn secondary_click(h: &mut egui_kittest::Harness<'static, test_window::Window>, at: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(4);
}

/// The flyout's row named `label`.
fn flyout_row(h: &egui_kittest::Harness<'static, test_window::Window>, label: &str) -> Option<egui::Rect> {
    h.query_all_by_label(label)
        .find(|node| matches!(node.accesskit_node().role(), egui::accesskit::Role::MenuItem | egui::accesskit::Role::MenuItemRadio))
        .map(|node| node.rect())
}

/// Successor of `hungarian_flyout_scrolls_horizontally_in_narrow_windows`: with the Hungarian interface and a shortcut on the
/// Ellipse Frame tool, in one and two columns and in windows 180 and 240 points wide, the framework flyout of the frame group
/// opens inside the window, the translated label is readable, the whole shortcut can be brought into view by scrolling
/// sideways, and a click on its row selects the tool and closes the flyout.
#[test]
fn hungarian_framework_flyout_stays_usable_in_narrow_windows() {
    for double in [false, true] {
        for width in [180.0, 240.0] {
            let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.ui.tools_double_column = double;
            app.ui.language = "hu".into();
            let chord = "Ctrl+Alt+Shift+F12";
            app.run(
                "window.setShortcut",
                serde_json::json!({"id": "tool.select", "params": {"tool": "ellipseFrame"}, "shortcut": chord, "steal": true}),
            )
            .unwrap();
            app.select_tool("rectangleFrame");
            let mut h = test_window::open_stepped(app, vec2(width, 1000.0), 1.0, 0.05);
            let group = h.get_by_label("Téglalapkeret eszköz (F)").rect().center();
            secondary_click(&mut h, group);
            let label = "Ellipsziskeret eszköz";
            let row = flyout_row(&h, label).unwrap_or_else(|| panic!("the flyout lists {label} ({width}, double {double})"));
            let viewport = h.ctx.content_rect();
            assert!(
                viewport.expand(0.5).intersects(row) && viewport.expand(0.5).contains(row.left_center()),
                "the row {row:?} starts inside the window {viewport:?}"
            );
            let menu = h.query_all_by_role(egui::accesskit::Role::Menu).map(|node| node.rect()).find(|rect| rect.intersects(row));
            if let Some(menu) = menu {
                assert!(viewport.expand(0.5).contains_rect(menu), "the flyout {menu:?} stays inside the window {viewport:?}");
            }
            // The shortcut shows with its row, or scrolling sideways brings it into view.
            let shortcut = crate::menus::shortcut_text(chord);
            let shown = |h: &egui_kittest::Harness<'static, test_window::Window>| {
                h.query_all_by_label_contains(&shortcut).any(|node| viewport.expand(0.5).contains_rect(node.rect()))
            };
            if !shown(&h) {
                wheel(&mut h, row.center(), vec2(-1000.0, 0.0));
                assert!(shown(&h), "scrolling sideways makes the complete shortcut readable ({width}, double {double})");
            }
            assert_eq!(crate::menus::tool_shortcut_label(&h.state().app, "ellipseFrame"), Some(shortcut));
            // The row is still reachable after the scroll: a click on it selects the tool and closes the flyout.
            let row = flyout_row(&h, label).expect("the row is still listed");
            let at = row.intersect(viewport).center();
            h.input_mut().events.push(Event::PointerMoved(at));
            for pressed in [true, false] {
                h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            }
            h.run_steps(4);
            assert_eq!(h.state().app.session.tool_id(), "ellipseFrame", "{width}, double {double}");
            assert!(flyout_row(&h, label).is_none(), "choosing a tool closes the flyout");
        }
    }
}

/// Successor of `a_tool_flyout_opened_by_holding_stays_open_and_esc_keeps_the_tool`: holding the group button for half a second
/// opens the framework flyout, the release keeps it open, and Esc closes it with the tool unchanged.
#[test]
fn a_framework_flyout_opened_by_holding_stays_open_and_esc_keeps_the_tool() {
    let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let mut h = test_window::open_stepped(app, vec2(1400.0, 900.0), 1.0, 1.0 / 60.0);
    h.state_mut().app.select_tool("rectangleFrame");
    h.run_steps(2);
    let at = h.get_by_label("Rectangle Frame Tool (F)").rect().center();
    h.input_mut().events.push(Event::PointerMoved(at));
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    // Held for half a second: shorter than egui's longest click.
    h.run_steps(30);
    assert!(flyout_row(&h, "Ellipse Frame Tool").is_some(), "holding opens the flyout");
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert!(flyout_row(&h, "Ellipse Frame Tool").is_some(), "the release keeps it open");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(flyout_row(&h, "Ellipse Frame Tool").is_none(), "Esc closes it");
    assert_eq!(h.state().app.session.tool_id(), "rectangleFrame", "and keeps the tool");
}
