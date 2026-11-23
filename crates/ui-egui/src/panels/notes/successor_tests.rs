//! Successors, on the installed framework, of the native tests of `panels/notes.rs`: each keeps its old steps and checks and runs them in a
//! window whose context has the craft UI runtime installed and the app's framework flag on, so the panel draws its framework
//! body. The old tests stay until each one is deleted with its native body.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::*;

/// Successor of `split_note_splits_the_note_at_the_insertion_point_in_its_text` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn split_note_splits_the_note_at_the_insertion_point_in_its_text() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.framework_opacity = true;
    app.framework_installed = true;
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
    app.run("text.select", json!({"story": r["story"], "anchor": 5, "focus": 5})).unwrap();
    app.run("note.new", json!({"text": "check this", "author": "Ed"})).unwrap();
    let mut h = crate::test_window::builder::<DesignApp>().with_size(egui::vec2(320.0, 400.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let id = egui::Id::unique("test_fonts");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                crate::theme::install_fonts(ui.ctx(), "");
                ui.data_mut(|d| d.insert_temp(id, true));
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.run_steps(3);
    assert!(h.get_by_label("Split Note").accesskit_node().is_disabled(), "no insertion point in the note yet");
    let field = h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && n.value().as_deref() == Some("check this")).rect();
    h.hover_at(field.center());
    h.drag_at(field.center());
    h.drop_at(field.center());
    h.run_steps(2);
    h.key_press(egui::Key::Home);
    for _ in 0..6 {
        h.key_press(egui::Key::ArrowRight);
    }
    h.run_steps(2);
    h.get_by_label("Split Note").click();
    h.run_steps(3);
    let list = h.state_mut().session.execute("note.list", &json!({})).unwrap();
    let texts: Vec<_> = list.as_array().unwrap().iter().map(|n| (n["text"].clone(), n["author"].clone())).collect();
    assert_eq!(texts, [(json!("check "), json!("Ed")), (json!("this"), json!("Ed"))]);
}
