//! Successor, on the framework Control bar, of the native icon field naming test: a number field of the Control bar's Character mode is
//! named by what it sets. It runs the shipping window with the craft UI runtime.

use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::json;

use crate::test_window;

/// Successor of `icon_fields_are_labelled_by_their_icon`: with the Type tool and text selected, the Control bar's Character mode has
/// number fields named Tracking, Kerning and Leading (what each icon says), each a spin button or text field a screen reader can
/// find by that name.
#[test]
fn control_bar_character_fields_are_named_by_what_they_set() {
    let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".into();
    app.run("window.workspace", json!({"name": "Advanced"})).unwrap();
    let mut h = test_window::open(app, vec2(1440.0, 900.0));
    let app = &mut h.state_mut().app;
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hello", "caret": true})).unwrap();
    app.run("tool.select", json!({"tool": "type"})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 5})).unwrap();
    h.run_steps(4);
    for name in ["Tracking", "Kerning", "Leading"] {
        let found = h
            .query_all_by_label_contains(name)
            .any(|n| matches!(n.accesskit_node().role(), Role::SpinButton | Role::TextInput | Role::ComboBox) && n.rect().min.y < 100.0);
        assert!(found, "the Control bar has a field named {name}");
    }
}
