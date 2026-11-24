//! Successor, on the installed framework, of the native Story Editor depth marks test: the same steps and checks with the framework flag
//! on, so the editor draws its framework body.

use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::DesignApp;

fn story_text(app: &DesignApp, doc: usize) -> String {
    let st = &app.session.documents()[doc];
    st.doc.stories.values().next().unwrap().text.clone()
}

fn story_editor_harness(text: &str) -> egui_kittest::Harness<'static, crate::test_window::Window> {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
    app.run("file.new", json!({})).unwrap();
    let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": text, "caret": false})).unwrap();
    app.run("app.storyEditor", json!({"story": r["story"].as_u64().unwrap()})).unwrap();
    let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
    h.run_steps(2);
    h
}

fn text_left(h: &egui_kittest::Harness<'_, crate::test_window::Window>, text: &str) -> f32 {
    h.get_by(|n| n.role() == egui::accesskit::Role::TextRun && !n.is_hidden() && n.value().as_deref() == Some(text)).rect().min.x
}

/// Successor of `depth_marks_wait_for_composition_after_editing` on the installed framework: the same steps and checks, in a window whose context has the craft UI runtime.
#[test]
fn depth_marks_wait_for_composition_after_editing() {
    fn marks(shape: &egui::epaint::Shape, tick: egui::Color32) -> usize {
        match shape {
            egui::epaint::Shape::Vec(shapes) => shapes.iter().map(|s| marks(s, tick)).sum(),
            egui::epaint::Shape::LineSegment { points, stroke }
                if stroke.color == super::OVERSET_RED || (stroke.color == tick && (points[1].x - points[0].x - 8.0).abs() < 0.01) =>
            {
                1
            }
            _ => 0,
        }
    }
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
    app.run("file.new", json!({})).unwrap();
    let text = "First line\nSecond line\nThird line\n".repeat(4);
    let r = app.run("frame.create", json!({"rect": [72, 72, 372, 172], "content": "text", "text": text, "caret": false})).unwrap();
    app.run("app.storyEditor", json!({"story": r["story"].as_u64().unwrap()})).unwrap();
    // `show` only keeps the editor's panel with its story; the dock draws the editor's body in that panel. The body is
    // drawn here as the dock draws it, after the first pass installed the fonts and the theme as the shipping window does.
    let mut ready = false;
    let mut h = crate::test_window::builder::<DesignApp>().with_size(egui::vec2(900.0, 700.0)).with_accessibility_check(false).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            super::body(app, ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(4);
    let tick = crate::theme::Tokens::get(&h.ctx).ruler_tick;
    let count = |h: &egui_kittest::Harness<'_, DesignApp>| h.output().shapes.iter().map(|s| marks(&s.shape, tick)).sum::<usize>();
    assert!(count(&h) >= 2, "the fixture paints ruler ticks and an overset line");
    h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && !n.is_hidden()).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Home);
    h.run_steps(2);
    h.event(egui::Event::Text("Inserted line\n".into()));
    h.step();
    assert_eq!(story_text(h.state(), 0), format!("Inserted line\n{text}"));
    assert_eq!(count(&h), 0, "the edit frame must not map old composition offsets to new text");
    h.step();
    assert!(count(&h) >= 2, "the next frame paints the updated composition");
}
