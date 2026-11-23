#![cfg(not(target_arch = "wasm32"))]

//! Keyboard and focus interoperability between a craft framework field and the app's own controls, at the
//! `79ded40ba` library checkpoint.
//!
//! Everything here runs the app's real window pipeline — `DesignApp::raw_input_hook` (what the host calls each
//! frame), then `logic` and `ui` — so the shortcut and canvas paths are live and an absence assertion means
//! something. Each absence is paired with a positive control that must hold in the same harness:
//!
//! - ArrowUp with the canvas focused must move the selected frame and leave a `transform.move` in the journal;
//! - ArrowUp while the framework's field is being typed in must step that field's value and put no
//!   `transform.move` in the journal, with the frame's bounds unchanged;
//! - Tab must commit what was typed, as one undo step, and hand the keyboard to an identified control of the same
//!   row, the framework chevron after the field, whose response has a real rect inside the Properties panel; and Tab
//!   again goes on through the row's framework stops;
//! - Shift+Tab must come back to the framework field's own stop;
//! - Escape must leave a typed draft uncommitted.

use egui_kittest::kittest::Queryable as _;
use crate::DesignApp;
use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// The whole window, as the desktop shell runs it: the host input hook, then `logic` and `ui`.
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
    for _ in 0..6 {
        step(&mut h);
    }
    h
}

/// One frame through the app's real input path, with no event of its own: the host hook runs on this frame's
/// input, then the pass runs.
fn step(h: &mut Harness<'_, DesignApp>) {
    let mut raw = std::mem::take(h.input_mut());
    let ctx = h.ctx.clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

fn steps(h: &mut Harness<'_, DesignApp>, n: usize) {
    for _ in 0..n {
        step(h);
    }
}

/// One frame per event, the way eframe drives it: the event goes into THIS frame's raw input, the app's
/// `raw_input_hook` sees that input, and only then does egui's pass begin. Returns what the hook was given, so a
/// test can assert the hook really saw the event rather than merely being called.
///
/// This is the difference from `Harness::event`, which queues into `queued_events` and only merges it into
/// `self.input` inside `Harness::step` — after any hook the test calls itself (`egui_kittest` `lib.rs:376-398,
/// 658-659`; this workspace builds egui_kittest without its `eframe` feature).
fn event_frame(h: &mut Harness<'_, DesignApp>, event: egui::Event) -> Vec<String> {
    let mut raw = std::mem::take(h.input_mut());
    raw.events.push(event);
    let ctx = h.ctx.clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    let seen: Vec<String> = raw.events.iter().map(|e| format!("{e:?}")).collect();
    *h.input_mut() = raw;
    h.step();
    seen
}

/// A frame whose opacity the framework's field edits.
fn app_with_frame() -> (DesignApp, designcraft_doc::ItemId) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Keys"})).expect("a new document");
    let id = designcraft_doc::ItemId(app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).expect("a frame")["id"].as_u64().expect("an id"));
    app.run("selection.set", json!({"ids": [id.0]})).expect("the frame is selected");
    (app, id)
}

fn rect_of(h: &Harness<'_, DesignApp>, key: &str) -> egui::Rect {
    h.state()
        .framework_opacity_fields
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("the pilot's {key} rect is not on screen: {:?}", h.state().framework_opacity_fields))
}

fn opacity(h: &Harness<'_, DesignApp>, id: designcraft_doc::ItemId) -> f64 {
    f64::from(h.state().session.doc().expect("a document").doc.item(id).expect("the frame").opacity)
}

fn bounds(h: &Harness<'_, DesignApp>, id: designcraft_doc::ItemId) -> designcraft_geom::Rect {
    h.state().session.doc().expect("a document").doc.item(id).expect("the frame").bounds()
}

fn undo_steps(h: &Harness<'_, DesignApp>) -> usize {
    h.state().session.doc().expect("a document").history.undo.len()
}

fn journal(h: &Harness<'_, DesignApp>) -> usize {
    h.state().session.journal.len()
}

fn focused(h: &Harness<'_, DesignApp>) -> Option<egui::Id> {
    h.ctx.memory(|m| m.focused())
}

fn click(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
    event_frame(h, Event::PointerMoved(at));
    event_frame(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    event_frame(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    steps(h, 3);
}

/// A click into the framework's field, select all, then type `text` without committing it.
fn type_draft(h: &mut Harness<'_, DesignApp>, at: egui::Pos2, text: &str) {
    click(h, at);
    event_frame(h, Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    // Release the chord and its modifier before typing: a draft must not depend on a held select-all.
    event_frame(h, Event::Key { key: Key::A, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::COMMAND });
    event_frame(h, Event::ModifiersChanged(Modifiers::NONE));
    steps(h, 1);
    for c in text.chars() {
        event_frame(h, Event::Text(c.to_string()));
    }
    steps(h, 2);
}

fn press(h: &mut Harness<'_, DesignApp>, k: Key, modifiers: Modifiers) {
    event_frame(h, Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers });
    steps(h, 1);
    event_frame(h, Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers });
    steps(h, 3);
}

/// The same press, returning what the app's hook was given for its press frame.
fn press_seen(h: &mut Harness<'_, DesignApp>, k: Key, modifiers: Modifiers) -> Vec<String> {
    let seen = event_frame(h, Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers });
    steps(h, 1);
    event_frame(h, Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers });
    steps(h, 3);
    seen
}

/// The screen rect of an item, through the app's own spread-to-canvas and canvas-to-screen transforms: the
/// same projection the canvas overlays draw with, never a guessed window position.
fn item_screen_rect(h: &Harness<'_, DesignApp>, id: designcraft_doc::ItemId) -> egui::Rect {
    let app = h.state();
    let st = app.session.active().expect("a document");
    let layout = app.session.layout();
    let (to_canvas, _) = crate::canvas::item_canvas_xf(&st.doc, &layout, id).expect("the frame is laid out on the canvas");
    let xf = crate::canvas::Xf::new(app.canvas_rect.expect("the canvas has a rect"), app.view().expect("the canvas has a view"));
    let bounds = st.doc.item(id).expect("the frame").bounds();
    let at = designcraft_geom::Point::new(bounds.center().x, bounds.center().y);
    let c = to_canvas * at;
    let s = xf.to_screen(c);
    egui::Rect::from_center_size(s, vec2(4.0, 4.0))
}

#[test]
fn arrow_up_on_the_focused_canvas_moves_the_selected_frame() {
    let (app, id) = app_with_frame();
    let mut h = window_harness(app);
    // The positive control for every "it did not reach the canvas" assertion below: press on the selected
    // frame's own projected geometry (so the canvas takes the keyboard without deselecting it), then the same
    // key must nudge the selection and reach the journal.
    let on_frame = item_screen_rect(&h, id).center();
    click(&mut h, on_frame);
    assert_eq!(h.state().session.active().expect("a document").selection.items, vec![id], "the press on the frame's own geometry kept it selected");
    let before = bounds(&h, id);
    let mark = journal(&h);
    press(&mut h, Key::ArrowUp, Modifiers::NONE);
    let after = bounds(&h, id);
    assert!(after.y0 < before.y0, "ArrowUp on the canvas moved the frame up: {before:?} → {after:?}");
    let moved = h.state().session.journal[mark..].iter().any(|(command, _)| command == "transform.move");
    assert!(moved, "the canvas nudge reached the engine as transform.move: {:?}", &h.state().session.journal[mark..]);
}

#[test]
fn arrow_up_while_the_framework_field_is_typing_steps_it_and_does_not_reach_the_canvas() {
    let (app, id) = app_with_frame();
    let mut h = window_harness(app);
    let field = rect_of(&h, "opacity");
    type_draft(&mut h, field.center(), "40");
    let in_field = focused(&h);
    assert!(in_field.is_some(), "the framework's field holds the keyboard while typing");
    let before = bounds(&h, id);
    let mark = journal(&h);
    let steps_before = undo_steps(&h);
    press(&mut h, Key::ArrowUp, Modifiers::NONE);
    // The positive half: the field used the key, one percent up from the value it was given.
    assert!((opacity(&h, id) - f64::from(0.41_f32)).abs() < 1e-9, "the field's own step reached the document: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h) - steps_before, 1, "and it is one undo step");
    // The absence half, now meaningful because the same key moves the frame when the canvas has it.
    let commands = &h.state().session.journal[mark..];
    assert!(
        commands.iter().all(|(command, _)| command != "transform.move"),
        "the canvas did not nudge the frame while the field was typing: {commands:?}"
    );
    assert_eq!((bounds(&h, id).x0, bounds(&h, id).y0), (before.x0, before.y0), "the frame did not move");
}

#[test]
fn tab_commits_the_typed_value_and_hands_the_keyboard_to_an_identified_app_control() {
    let (app, id) = app_with_frame();
    let mut h = window_harness(app);
    let steps_before = undo_steps(&h);
    let field = rect_of(&h, "opacity");
    type_draft(&mut h, field.center(), "40");
    let in_field = focused(&h).expect("the field holds the keyboard while typing");
    let seen = press_seen(&mut h, Key::Tab, Modifiers::NONE);
    // The fixture's own guard: the app's host hook was given a Tab press in this frame, so a failure below is
    // about what the pass did with it, not about the event never reaching the app.
    assert!(seen.iter().any(|e| e.contains("Tab")), "the host hook saw the Tab press: {seen:?}");
    assert!((opacity(&h, id) - f64::from(0.4_f32)).abs() < 1e-12, "Tab committed the typed percentage: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h) - steps_before, 1, "and it is one undo step");
    // The stop that takes the keyboard must be a real, identified existing control — not `None`, not another
    // part of the framework component, and not merely "a different id".
    let next = focused(&h).expect("Tab left the keyboard on another stop");
    assert_ne!(next, in_field, "the keyboard left the framework field");
    let rect = h.ctx.read_response(next).map(|r| r.rect).expect("the stop that took the keyboard is an identified control");
    assert!(rect.width() > 0.0 && rect.height() > 0.0, "that control has a real rect: {rect:?}");
    assert!(rect.center().x > h.ctx.content_rect().width() / 2.0, "it is in the Properties panel: {rect:?}");
    // Outside the WHOLE framework component (its box and every Tab stop the framework recorded), so an SDK
    // part that happens to sit outside the field's inner rect cannot satisfy this.
    let component = rect_of(&h, "component");
    assert!(!rect.intersects(component), "the keyboard went outside the framework component: {rect:?} vs {component:?}");
    let stops = h.state().framework_opacity_component_stops.clone();
    assert!(stops.iter().all(|(_, stop)| !stop.intersects(rect)), "and it is not one of the component's own stops {stops:?}: {rect:?}");
    // The identified target: the chevron beside the field, the row's next framework stop, at its recorded rect.
    let more = rect_of(&h, "more");
    assert!((rect.center() - more.center()).length() < 1.5, "the keyboard landed on the row's own chevron control at {more:?}, not {rect:?}");
}

#[test]
fn tab_from_the_chevron_goes_on_to_the_rows_frame_fitting_button() {
    let (app, _id) = app_with_frame();
    let mut h = window_harness(app);
    let field = rect_of(&h, "opacity");
    type_draft(&mut h, field.center(), "40");
    press(&mut h, Key::Tab, Modifiers::NONE);
    let on_chevron = focused(&h).expect("Tab left the keyboard on the chevron");
    let more = rect_of(&h, "more");
    let chevron = h.ctx.read_response(on_chevron).map(|r| r.rect).expect("the chevron is an identified control");
    assert!((chevron.center() - more.center()).length() < 1.5, "the first Tab landed on the chevron at {more:?}, not {chevron:?}");
    // The positive control for "the row is one framework Tab order": the next Tab reaches the row's next control, the
    // Frame Fitting button, not some stop elsewhere in the window and not nothing.
    press(&mut h, Key::Tab, Modifiers::NONE);
    let next = focused(&h).expect("Tab left the keyboard on another stop");
    assert_ne!(next, on_chevron, "the keyboard left the chevron");
    let rect = h.ctx.read_response(next).map(|r| r.rect).expect("the stop that took the keyboard is an identified control");
    let fitting = h.get_by_role_and_label(egui::accesskit::Role::Button, "Frame Fitting").rect();
    assert!((rect.center() - fitting.center()).length() < 1.5_f32, "the keyboard went on to the row's Frame Fitting button at {fitting:?}, not {rect:?}");
}

#[test]
fn shift_tab_returns_to_the_framework_fields_own_stop() {
    let (app, _id) = app_with_frame();
    let mut h = window_harness(app);
    let field = rect_of(&h, "opacity");
    type_draft(&mut h, field.center(), "40");
    let in_field = focused(&h).expect("the field holds the keyboard while typing");
    let seen = press_seen(&mut h, Key::Tab, Modifiers::NONE);
    assert!(seen.iter().any(|e| e.contains("Tab")), "the host hook saw the Tab press: {seen:?}");
    let after_tab = focused(&h).expect("Tab left the keyboard on another stop");
    assert_ne!(after_tab, in_field, "Tab left the framework field");
    press(&mut h, Key::Tab, Modifiers::SHIFT);
    assert_eq!(focused(&h), Some(in_field), "Shift+Tab came back to the framework field's own stop");
}

#[test]
fn escape_leaves_a_typed_draft_uncommitted() {
    let (app, id) = app_with_frame();
    let mut h = window_harness(app);
    let steps_before = undo_steps(&h);
    let field = rect_of(&h, "opacity");
    type_draft(&mut h, field.center(), "40");
    press(&mut h, Key::Escape, Modifiers::NONE);
    assert!((opacity(&h, id) - 1.0).abs() < 1e-9, "Escape left the document's opacity alone: {}", opacity(&h, id));
    assert_eq!(undo_steps(&h), steps_before, "and pushed no undo step");
}
