//! Color panel: the Tint field of a named swatch.

use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::DesignApp;

fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    Harness::builder().with_size(vec2(320.0, 320.0)).build_ui_state(|ui, app: &mut DesignApp| super::swatches::color_panel(app, ui), app)
}

fn fill_tint(app: &DesignApp, id: u64) -> f32 {
    app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.tint
}

/// The Tint row is the panel's first value field, above the C/M/Y/K fields.
fn click_tint_field(h: &mut Harness<'static, DesignApp>) {
    let at = h
        .query_all_by(|n| matches!(n.role(), Role::SpinButton | Role::TextInput))
        .map(|n| n.rect())
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap()
        .center();
    h.hover_at(at);
    h.drag_at(at);
    h.drop_at(at);
    h.run_steps(2);
}

fn typed(h: &Harness<'static, DesignApp>) -> Option<String> {
    h.query_by(|n| n.is_focused()).and_then(|n| n.value())
}

/// Typing "60" over "100%" keeps the text while the field has focus (it used to snap back to
/// 100 after the first digit, giving 10060), applies it on Enter, and a value out of range is
/// clamped when the field loses focus.
#[test]
fn a_typed_tint_is_kept_while_typing_and_applied_on_enter_or_leaving_the_field() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let id = app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
    app.run("object.fill", json!({"ids": [id], "swatch": "C=100 M=0 Y=0 K=0"})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    let mut h = harness(app);
    h.run_steps(3);
    click_tint_field(&mut h);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    for digit in ["6", "0"] {
        h.event(egui::Event::Text(digit.into()));
        h.run_steps(3);
    }
    assert_eq!(typed(&h).as_deref(), Some("60"), "the typed text stays while the field has focus");
    assert_eq!(fill_tint(h.state(), id), 1.0, "nothing is applied while typing");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!((fill_tint(h.state(), id) - 0.6).abs() < 1e-6, "Enter applies 60 %: {}", fill_tint(h.state(), id));
    assert_eq!(h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, "C=100 M=0 Y=0 K=0");
    // Out of range: clamped to 100 % when focus moves away.
    click_tint_field(&mut h);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("150".into()));
    h.run_steps(3);
    let away = egui::pos2(300.0, 310.0);
    h.hover_at(away);
    h.drag_at(away);
    h.drop_at(away);
    h.run_steps(3);
    assert_eq!(fill_tint(h.state(), id), 1.0, "150 is clamped to 100 % when the field loses focus");
    // The slider still applies on release.
    let track = h.get_by_role(Role::Slider).rect();
    h.hover_at(track.right_center());
    h.drag_at(track.right_center());
    h.run_steps(1);
    h.hover_at(track.left_center());
    h.run_steps(1);
    h.drop_at(track.left_center());
    h.run_steps(3);
    assert_eq!(fill_tint(h.state(), id), 0.0, "dragging the slider to the left end applies 0 %");
}
