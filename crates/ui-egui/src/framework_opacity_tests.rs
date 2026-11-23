#![cfg(not(target_arch = "wasm32"))]

//! Properties › Appearance on the craft UI framework: the Opacity row's percent field, its chevron and the slider
//! popover the chevron opens, and the section's other framework controls, in the real section of a real document,
//! driven with real pointer and key events.
//!
//! Three scenarios matter. A person types a percentage into the framework's field, `object.opacity` runs,
//! the object's opacity in the document is that ratio, and `edit.undo` puts it back in one step; a step key
//! on the focused field is the same one step. A person drags the slider the chevron opens: the page follows
//! the drag without a new undo step, and the release is one step. Escape during that drag goes back through
//! the engine's own `edit.cancelPreview`. The rest are the guards the section needs: a cancel from this row
//! never takes another panel's preview back, the field's rect is this frame's and goes with the selection,
//! without the framework's runtime the row keeps the native field, and the stroke weight field and the effects
//! button run their engine commands as one undo step each.
//!
//! Geometry is the framework's own: the field and the slider are drawn inside `inspect::record`, and the
//! rects these tests press are the hit areas the framework recorded, never a guessed percentage of the
//! panel.

use crate::DesignApp;
use egui::{Event, Key, Modifiers, PointerButton, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// The frame the tests fade.
fn app_with_frame() -> (DesignApp, designcraft_doc::ItemId) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Opacity"})).expect("a new document");
    let id = designcraft_doc::ItemId(app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).expect("a frame")["id"].as_u64().expect("an id"));
    app.run("selection.set", json!({"ids": [id.0]})).expect("the frame is selected");
    (app, id)
}

/// The Properties panel on a context that has the framework's runtime, with `pilot` as the flag.
fn properties_harness(mut app: DesignApp, pilot: bool) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    app.framework_opacity = pilot;
    app.install_framework(&ctx);
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(320.0, 900.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "");
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            crate::panels::properties::show(app, ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(3);
    h
}

/// The whole window, as the desktop shell runs it each pass (`logic` then `ui`), with the pilot on: the app
/// clears the pilot's rects at the start of each pass, so only this pass's are there.
fn window_harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    app.framework_opacity = true;
    app.install_framework(&ctx);
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    h.run_steps(6);
    h
}

/// The rect the Opacity row recorded for `key` in the last pass.
fn rect_of(h: &Harness<'_, DesignApp>, key: &str) -> egui::Rect {
    h.state()
        .framework_opacity_fields
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("the pilot's {key} rect is not on screen: {:?}", h.state().framework_opacity_fields))
}

/// The selected frame's opacity, from the document itself (the document keeps it as `f32`).
fn opacity(h: &Harness<'_, DesignApp>, id: designcraft_doc::ItemId) -> f64 {
    f64::from(h.state().session.doc().expect("a document").doc.item(id).expect("the frame").opacity)
}

/// The undo steps the document's history holds.
fn undo_steps(h: &Harness<'_, DesignApp>) -> usize {
    h.state().session.doc().expect("a document").history.undo.len()
}

/// A click into the field at `rect`, select all, type `text`, then Return.
fn type_value(h: &mut Harness<'_, DesignApp>, rect: egui::Rect, text: &str) {
    let at = rect.center();
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.event(Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    for c in text.chars() {
        h.event(Event::Text(c.to_string()));
    }
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.event(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// A press at `at`, held.
fn press(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// A click at `at`, as a mouse makes it: the pointer arrives, the button goes down in the next frame and up in the
/// one after.
fn click(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
    h.event(Event::PointerMoved(at));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(4);
}

/// The chevron beside the field opens the slider popover, whose track rect it returns.
fn open_slider(h: &mut Harness<'_, DesignApp>) -> egui::Rect {
    let more = rect_of(h, "more");
    let at = more.center();
    click(h, at);
    let track = rect_of(h, "slider");
    assert!(track.width() > 0.0 && track.height() > 0.0, "the framework's slider is laid out: {track:?}");
    track
}

#[test]
fn typing_an_opacity_moves_the_object_and_one_undo_puts_it_back() {
    let (app, id) = app_with_frame();
    let mut h = properties_harness(app, true);
    assert!((opacity(&h, id) - 1.0).abs() < 1e-9, "the frame starts opaque");
    let steps = undo_steps(&h);
    let field = rect_of(&h, "opacity");
    assert!(field.width() > 0.0 && field.height() > 0.0, "the framework's field is laid out: {field:?}");
    type_value(&mut h, field, "40");
    // The document keeps opacity as f32: compare against the same 0.4 widened to f64.
    assert!((opacity(&h, id) - f64::from(0.4_f32)).abs() < 1e-12, "the typed percentage reached the document: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h) - steps, 1, "one gesture, one undo step ({steps} → {})", undo_steps(&h));
    h.state_mut().run("edit.undo", json!({})).expect("undo");
    h.run_steps(3);
    assert!((opacity(&h, id) - 1.0).abs() < 1e-9, "undo restores the object: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h), steps, "and takes the step back");
}

#[test]
fn a_locked_objects_opacity_stays_editable() {
    let (mut app, id) = app_with_frame();
    app.run("object.lock", json!({"ids": [id.0]})).expect("lock");
    // Lock deselects, as Cmd+L does; a person then clicks the locked frame again.
    app.run("selection.set", json!({"ids": [id.0]})).expect("the locked frame is selected again");
    assert!(crate::panels::sel_info(&app).expect("a selection").locked, "the selection is locked");
    let mut h = properties_harness(app, true);
    let steps = undo_steps(&h);
    let field = rect_of(&h, "opacity");
    type_value(&mut h, field, "40");
    // The bespoke row never disabled its opacity field for a locked object, and `object.opacity` has no
    // lock guard: the pilot matches that, and the edit is one undo step either way.
    assert!((opacity(&h, id) - f64::from(0.4_f32)).abs() < 1e-12, "a locked object's opacity field still commits: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h) - steps, 1, "one gesture, one undo step");
}

#[test]
fn a_step_key_commits_one_more_step() {
    let (app, id) = app_with_frame();
    let mut h = properties_harness(app, true);
    let steps = undo_steps(&h);
    let field = rect_of(&h, "opacity");
    type_value(&mut h, field, "40");
    assert!((opacity(&h, id) - f64::from(0.4_f32)).abs() < 1e-12, "typed first: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h) - steps, 1, "the typed value is one step");
    // The field keeps the focus after its commit, and one arrow press is the framework's own step of one
    // percent (a NumberField steps by its `step` on Up/Down): 40 becomes 41, never the value it had before.
    for pressed in [true, false] {
        h.event(Event::Key { key: Key::ArrowUp, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
    assert!(
        (opacity(&h, id) - f64::from(0.41_f32)).abs() < 1e-9,
        "the step key moves one percent from the typed value ({steps} → {} steps): {}",
        undo_steps(&h),
        opacity(&h, id)
    );
    assert_eq!(undo_steps(&h) - steps, 2, "and is one more undo step ({steps} → {})", undo_steps(&h));
    h.state_mut().run("edit.undo", json!({})).expect("undo");
    h.run_steps(3);
    assert!((opacity(&h, id) - f64::from(0.4_f32)).abs() < 1e-12, "undo takes the step back to the typed value: {}", opacity(&h, id));
}

#[test]
fn dragging_the_slider_previews_live_and_commits_one_step() {
    let (app, id) = app_with_frame();
    let mut h = properties_harness(app, true);
    let steps = undo_steps(&h);
    let track = open_slider(&mut h);
    // The thumb of a fully opaque object sits at the right end of the track: a press on it and a drag left
    // is the gesture a person makes.
    let from = pos2(track.right() - 2.0, track.center().y);
    press(&mut h, from);
    assert_eq!(undo_steps(&h), steps, "a press on the thumb on its own pushes nothing");
    let mid = track.center();
    h.event(Event::PointerMoved(mid));
    h.run_steps(2);
    let halfway = opacity(&h, id);
    assert!(halfway < 0.9, "the page followed the drag before the release: {halfway}");
    assert_eq!(undo_steps(&h), steps, "a drag in progress is a preview, not an undo step");
    let far = pos2(track.left() + track.width() * 0.25, track.center().y);
    h.event(Event::PointerMoved(far));
    h.run_steps(2);
    let quarter = opacity(&h, id);
    assert!(quarter < halfway, "the page follows each move: {halfway} → {quarter}");
    assert_eq!(undo_steps(&h), steps, "still no undo step in the middle of the drag");
    h.event(Event::PointerButton { pos: far, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(4);
    let released = opacity(&h, id);
    assert!((released - quarter).abs() < 1e-6, "the release keeps the value the drag reached: {quarter} → {released}");
    assert_eq!(undo_steps(&h) - steps, 1, "the release is one undo step ({steps} → {})", undo_steps(&h));
    h.state_mut().run("edit.undo", json!({})).expect("undo");
    h.run_steps(3);
    assert!((opacity(&h, id) - 1.0).abs() < 1e-9, "undo restores the object: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h), steps, "and takes the step back");
}

#[test]
fn escape_mid_drag_puts_the_object_back_and_pushes_no_step() {
    let (app, id) = app_with_frame();
    let mut h = properties_harness(app, true);
    let steps = undo_steps(&h);
    let track = open_slider(&mut h);
    let from = pos2(track.right() - 2.0, track.center().y);
    press(&mut h, from);
    let mid = track.center();
    h.event(Event::PointerMoved(mid));
    h.run_steps(2);
    assert!(opacity(&h, id) < 0.9, "the drag reached the page: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h), steps, "a drag in progress is a preview, not an undo step");
    // Escape cancels the gesture: the engine's own `edit.cancelPreview` puts the object back.
    for pressed in [true, false] {
        h.event(Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
    assert!((opacity(&h, id) - 1.0).abs() < 1e-9, "cancelled back to the object's own opacity: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h), steps, "and no undo step");
    // The release, and the frames after it, must not commit what the cancel took back.
    h.event(Event::PointerButton { pos: mid, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(6);
    assert!((opacity(&h, id) - 1.0).abs() < 1e-9, "the release did not commit the cancelled drag: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h), steps, "still no undo step");
    assert!(h.state().session.doc().expect("a document").interaction.is_none(), "no interaction is left open");
}

#[test]
fn escape_in_the_popup_leaves_another_panels_preview_alone() {
    let (app, id) = app_with_frame();
    let mut h = properties_harness(app, true);
    let steps = undo_steps(&h);
    // Another panel's drag opened a preview on this object; this row started nothing.
    h.state_mut().live_preview("Other panel", "object.opacity", json!({"opacity": 0.5}));
    h.run_steps(2);
    assert!(h.state().live_preview_open(), "the other panel's preview is open");
    assert!((opacity(&h, id) - 0.5).abs() < 1e-6, "and shows on the object: {}", opacity(&h, id));
    // Opening this row's popup and pressing Escape is not this row's gesture: nothing of the other panel's
    // preview, its value or the history may move.
    let more = rect_of(&h, "more");
    let at = more.center();
    click(&mut h, at);
    for pressed in [true, false] {
        h.event(Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
    assert!((opacity(&h, id) - 0.5).abs() < 1e-6, "the other panel's preview still shows: {}", opacity(&h, id));
    assert!(h.state().live_preview_open(), "and is still open, not cancelled from this row");
    assert_eq!(undo_steps(&h), steps, "and nothing reached the history");
}

#[test]
fn the_field_rect_is_this_frames_and_goes_with_the_selection() {
    let (app, _id) = app_with_frame();
    let mut h = window_harness(app);
    // The field, the framework component's own box and the row's chevron: the set the pilot records each pass.
    let keys: Vec<&str> = h.state().framework_opacity_fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        ["opacity", "component", "more"],
        "the pilot's field, component and chevron are on screen: {:?}",
        h.state().framework_opacity_fields
    );
    h.state_mut().run("edit.deselectAll", json!({})).expect("deselect");
    h.run_steps(3);
    assert!(h.state().framework_opacity_fields.is_empty(), "stale rect with no selection: {:?}", h.state().framework_opacity_fields);
}

/// The framework is on by default on the desktop; with no framework runtime at all (the browser build's case) the
/// row keeps its bespoke field and command. Successor of the old pilot-off test.
#[test]
fn the_row_stays_bespoke_without_the_framework_runtime() {
    let (mut app, id) = app_with_frame();
    app.framework_transform = false;
    let mut h = properties_harness(app, false);
    assert!(!h.state().framework_opacity, "no framework row");
    assert!(!h.state().framework_installed, "so nothing installed the framework's runtime");
    assert!(h.state().framework_opacity_fields.is_empty(), "the bespoke field drew instead: {:?}", h.state().framework_opacity_fields);
    // The row is unchanged: the command behind the bespoke field still moves the object in one step.
    let steps = undo_steps(&h);
    h.state_mut().run("object.opacity", json!({"opacity": 0.25})).expect("object.opacity");
    assert!((opacity(&h, id) - 0.25).abs() < 1e-12, "the bespoke row's command still runs: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h) - steps, 1, "and is one undo step");
}

#[test]
fn typing_a_stroke_weight_runs_one_stroke_step() {
    let (app, _id) = app_with_frame();
    let mut h = properties_harness(app, true);
    let steps = undo_steps(&h);
    let field = h.get_by_role_and_label(egui::accesskit::Role::SpinButton, "Stroke Weight").rect();
    type_value(&mut h, field, "3");
    let weight = crate::panels::sel_info(h.state()).expect("the frame is selected").stroke_weight;
    assert!((weight - 3.0).abs() < 1e-9, "the typed weight reached the document: {weight}");
    assert_eq!(undo_steps(&h) - steps, 1, "one gesture, one undo step ({steps} → {})", undo_steps(&h));
}

#[test]
fn the_effects_button_turns_the_drop_shadow_on_in_one_step() {
    let (app, _id) = app_with_frame();
    let mut h = properties_harness(app, true);
    assert!(!crate::panels::sel_info(h.state()).expect("the frame is selected").shadow, "the frame starts without a shadow");
    let steps = undo_steps(&h);
    let at = h.get_by_role_and_label(egui::accesskit::Role::Button, "Effects: Drop Shadow").rect().center();
    click(&mut h, at);
    assert!(crate::panels::sel_info(h.state()).expect("the frame is selected").shadow, "the button turned the drop shadow on");
    assert_eq!(undo_steps(&h) - steps, 1, "one click, one undo step ({steps} → {})", undo_steps(&h));
}
