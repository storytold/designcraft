//! Successors, on the framework form, of the native Effects dialog tests the wave 4 cleanup parked: Use Global Light and the angle, the
//! dialog opening on the chosen effect with its preview, the angle dial and Blending's Mode. They run the real dialog host over an
//! installed framework (`table_layout_kit`).

use super::*;
use crate::dialogs::table_layout_kit as kit;
use egui::accesskit::Role;
use egui::{Key, Modifiers, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::{Value, json};

fn app_with_frame() -> (DesignApp, u64) {
    let mut app = kit::app(1, false);
    app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "inches"})).unwrap();
    let id = app.run("frame.create", json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
    app.run("object.fill", json!({"swatch": "[Paper]"})).unwrap();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    (app, id)
}

fn st(h: &mut egui_kittest::Harness<'static, DesignApp>) -> &mut DesignApp {
    h.state_mut()
}

fn fx(h: &egui_kittest::Harness<'static, DesignApp>, id: u64) -> designcraft_doc::Effects {
    h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().effects.clone()
}

/// The effects dialog the menu command `command` opens, in the real host.
fn dialog(command: &str) -> (egui_kittest::Harness<'static, DesignApp>, u64) {
    let (mut app, id) = app_with_frame();
    crate::menus::activate(&mut app, command, &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(ID));
    (kit::harness_sized(app, vec2(1400.0, 1000.0)), id)
}

/// Successor of `the_angle_with_global_light_sets_the_global_light`: with Use Global Light on, the Angle is the document's global
/// light and stays editable: a change there is applied to the document with the effects.
#[test]
fn effects_form_angle_with_global_light_sets_the_global_light() {
    let (mut h, _) = dialog("object.dropShadow");
    set(st(&mut h).ui.dialog.as_mut().unwrap(), &["dropShadow", "globalLight"], json!(true));
    kit::steps(&mut h, 4);
    assert!(
        !kit::disabled(&h, kit::FIELD, "Angle", 0) || h.query_all_by_label_contains("Angle").any(|n| !n.accesskit_node().is_disabled()),
        "the Angle stays editable with Use Global Light"
    );
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    d.fields.insert(GLOBAL_LIGHT.into(), json!(15.0));
    kit::steps(&mut h, 2);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    assert_eq!(h.state().session.doc().unwrap().doc.settings.global_light, 15.0);
}

/// Successor of `the_effects_dialog_opens_on_the_chosen_effect_previews_and_applies_once`: Object > Effects > Inner Shadow opens the
/// dialog on that page, its controls are named as InDesign's, the shadow is on the page while the dialog is open without an undo step,
/// Cancel takes it away, and a change then OK is one undo step.
#[test]
fn effects_form_opens_on_the_chosen_effect_previews_and_applies_once() {
    let (mut h, id) = dialog("object.innerShadow");
    let undo = |h: &egui_kittest::Harness<'static, DesignApp>| h.state().session.doc().unwrap().history.undo.len();
    let u0 = undo(&h);
    for t in [
        "Settings for:",
        "Transparency",
        "Inner Shadow",
        "Drop Shadow",
        "Blending",
        "Position",
        "Distance:",
        "Angle:",
        "Use Global Light",
        "Options",
        "Size:",
        "Choke:",
        "Preview",
    ] {
        assert!(kit::shows(&h, t) || h.query_all_by_label_contains(t).next().is_some(), "{t}");
    }
    let distance =
        h.query_all_by_role(Role::TextInput).chain(h.query_all_by_role(Role::SpinButton)).filter_map(|n| n.value()).any(|v| v.contains("0.0972 in"));
    assert!(distance, "the distance is in inches");
    // Preview: the shadow is on the page while the dialog is open, without an undo step.
    kit::steps(&mut h, 3);
    assert!(fx(&h, id).inner_shadow.on);
    assert_eq!(undo(&h), u0);
    // Cancel: as before.
    kit::click(&mut h, &[Role::Button], "Cancel", 0);
    assert!(!fx(&h, id).inner_shadow.on);
    assert_eq!(undo(&h), u0);
    // Again, with a change, then OK: one undo step.
    crate::menus::activate(st(&mut h), "object.innerShadow", &Value::Null);
    kit::steps(&mut h, 3);
    let d = st(&mut h).ui.dialog.as_mut().unwrap();
    set(d, &["innerShadow", "size"], json!(9.0));
    d.fields.insert("opacity".into(), json!(0.8));
    kit::steps(&mut h, 3);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let e = fx(&h, id);
    assert!(e.inner_shadow.on && e.inner_shadow.size == 9.0);
    assert_eq!(undo(&h), u0 + 1, "one undo step");
    assert!((h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().opacity - 0.8).abs() < 1e-6);
    st(&mut h).run("edit.undo", json!({})).unwrap();
    assert!(!fx(&h, id).inner_shadow.on);
}

/// Successor of `shadows_and_glows_have_a_blend_mode`: Drop Shadow and Outer Glow have InDesign's Mode under Blending (Multiply and
/// Screen to start); a mode picked there is what OK applies.
#[test]
fn effects_form_shadows_and_glows_have_a_blend_mode() {
    let (mut h, id) = dialog("object.dropShadow");
    assert!(kit::shows(&h, "Mode:") || h.query_all_by_label_contains("Mode").next().is_some());
    assert!(h.query_all_by_value("Multiply").next().is_some() || kit::shows(&h, "Multiply"), "Multiply to start");
    kit::click(&mut h, &[Role::Button], "Cancel", 0);
    open(st(&mut h), "outerGlow");
    kit::steps(&mut h, 4);
    assert!(h.query_all_by_value("Screen").next().is_some() || kit::shows(&h, "Screen"), "Screen to start");
    set(st(&mut h).ui.dialog.as_mut().unwrap(), &["outerGlow", "mode"], json!("Normal"));
    kit::steps(&mut h, 3);
    kit::click(&mut h, &[Role::Button], "OK", 0);
    assert!(h.state().ui.dialog.is_none());
    let e = fx(&h, id);
    assert!(e.outer_glow.on);
    assert_eq!(e.outer_glow.mode, designcraft_color::BlendMode::Normal);
}

/// The angle control of the open dialog: the node named like `Angle` that can be turned (a slider or a spin button).
fn angle_node(h: &egui_kittest::Harness<'static, DesignApp>) -> egui::Rect {
    h.query_all_by_label_contains("Angle")
        .find(|n| matches!(n.accesskit_node().role(), Role::Slider | Role::SpinButton | Role::Unknown))
        .unwrap_or_else(|| panic!("no angle control"))
        .rect()
}

/// Successor of `the_angle_dial_points_at_the_pointer` (the dial's own geometry is the framework component's; here the dialog): a click
/// at the left edge of the Angle dial makes it 180 and one above it 90, as InDesign's.
#[test]
fn effects_form_angle_dial_points_at_the_pointer() {
    let (mut h, _) = dialog("object.dropShadow");
    let rect = angle_node(&h);
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0 - 2.0;
    for (at, want) in [(egui::pos2(c.x - r, c.y), 180.0), (egui::pos2(c.x, c.y - r), 90.0), (egui::pos2(c.x + r, c.y), 0.0)] {
        kit::click_at(&mut h, at);
        let got = kit::draft(&h).fields["fx"]["dropShadow"]["angle"].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() <= 2.0, "a click at {at:?} sets {want}: {got}");
    }
}

/// Successor of `the_angle_dial_turns_with_the_keyboard`: with the Angle dial focused the arrows turn it by one degree (Shift: ten),
/// coming round at 180.
#[test]
fn effects_form_angle_dial_turns_with_the_keyboard() {
    let (mut h, _) = dialog("object.dropShadow");
    set(st(&mut h).ui.dialog.as_mut().unwrap(), &["dropShadow", "angle"], json!(175.0));
    kit::steps(&mut h, 3);
    let at = angle_node(&h).center();
    kit::click_at(&mut h, at);
    set(st(&mut h).ui.dialog.as_mut().unwrap(), &["dropShadow", "angle"], json!(175.0));
    kit::steps(&mut h, 3);
    let angle = |h: &egui_kittest::Harness<'static, DesignApp>| kit::draft(h).fields["fx"]["dropShadow"]["angle"].as_f64().unwrap();
    kit::key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(angle(&h), 176.0);
    kit::key(&mut h, Key::ArrowUp, Modifiers::SHIFT);
    assert_eq!(angle(&h), -174.0, "past 180 it comes round to -174");
    kit::key(&mut h, Key::ArrowLeft, Modifiers::SHIFT);
    kit::key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(angle(&h), 175.0);
    assert_eq!(wrap_angle(-180.0), 180.0);
    assert_eq!(wrap_angle(540.0), 180.0);
}
