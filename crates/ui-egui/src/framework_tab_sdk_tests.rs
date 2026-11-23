#![cfg(not(target_arch = "wasm32"))]

//! Tab between two FRAMEWORK stops in the app's real host setup (event-bearing raw hook + public `set_input`): from the
//! typed Opacity field, Tab commits the draft as one undo step and moves to the next framework stop of the panel, the
//! Opacity row's own chevron, with the Transform section's X/Y/W/H framework fields on screen in the same panel.

use crate::DesignApp;
use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn window_harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    app.framework_opacity = true;
    app.framework_transform = true;
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
        h.step_with_hook();
    }
    h
}

/// The harness trait this file's helpers need; kept here so the test states the pipeline it drives.
trait Pipeline {
    fn step_with_hook(&mut self);
    fn event_through_hook(&mut self, event: egui::Event) -> Vec<String>;
}

impl Pipeline for Harness<'_, DesignApp> {
    fn step_with_hook(&mut self) {
        let mut raw = std::mem::take(self.input_mut());
        let ctx = self.ctx.clone();
        self.state_mut().raw_input_hook(&ctx, &mut raw);
        *self.input_mut() = raw;
        self.step();
    }

    fn event_through_hook(&mut self, event: egui::Event) -> Vec<String> {
        let mut raw = std::mem::take(self.input_mut());
        raw.events.push(event);
        let ctx = self.ctx.clone();
        self.state_mut().raw_input_hook(&ctx, &mut raw);
        let seen: Vec<String> = raw.events.iter().map(|e| format!("{e:?}")).collect();
        *self.input_mut() = raw;
        self.step();
        seen
    }
}

fn steps(h: &mut Harness<'_, DesignApp>, n: usize) {
    for _ in 0..n {
        h.step_with_hook();
    }
}

fn focused(h: &Harness<'_, DesignApp>) -> Option<egui::Id> {
    h.ctx.memory(|m| m.focused())
}

/// The rect of a framework field this pass, by its pilot key.
fn field_rect(h: &Harness<'_, DesignApp>, key: &str) -> Option<egui::Rect> {
    h.state().framework_fields.iter().rev().find(|(k, _)| k == key).map(|(_, r)| *r)
}

/// The rect of the Opacity row's control `key` this pass: the field (`opacity`) or its chevron (`more`).
fn opacity_rect(h: &Harness<'_, DesignApp>, key: &str) -> egui::Rect {
    h.state()
        .framework_opacity_fields
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("the Opacity row's {key} is on screen: {:?}", h.state().framework_opacity_fields))
}

#[test]
fn tab_between_two_framework_stops_commits_and_moves_to_the_next_framework_stop() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Two stops"})).expect("a new document");
    // Points, so the Transform fields are the framework's number fields.
    app.run("document.preferences", json!({"horizontalUnits": "points", "verticalUnits": "points"})).expect("points");
    let id = designcraft_doc::ItemId(app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).expect("a frame")["id"].as_u64().expect("an id"));
    app.run("selection.set", json!({"ids": [id.0]})).expect("the frame is selected");
    let mut h = window_harness(app);
    // Both sections must really be on screen: the Opacity field and the Transform fields.
    let opacity = opacity_rect(&h, "opacity");
    let width = field_rect(&h, "x").expect("the Transform section's X field is on screen beside Opacity");
    assert!(opacity.width() > 0.0 && width.width() > 0.0, "both framework stops are laid out: {opacity:?} {width:?}");
    // Type a draft into the Opacity field.
    let at = opacity.center();
    h.event_through_hook(Event::PointerMoved(at));
    h.event_through_hook(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event_through_hook(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    steps(&mut h, 3);
    h.event_through_hook(Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    // Release the chord and its modifier before typing, so nothing rides on a held select-all.
    h.event_through_hook(Event::Key { key: Key::A, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::COMMAND });
    h.event_through_hook(Event::ModifiersChanged(Modifiers::NONE));
    for c in "40".chars() {
        h.event_through_hook(Event::Text(c.to_string()));
    }
    steps(&mut h, 2);
    let in_opacity = focused(&h).expect("the Opacity field holds the keyboard");
    let steps_before = h.state().session.doc().expect("doc").history.undo.len();
    // Tab, through the hook.
    let seen = h.event_through_hook(Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    assert!(seen.iter().any(|e| e.contains("Tab")), "the host hook saw the Tab press: {seen:?}");
    steps(&mut h, 2);
    h.event_through_hook(Event::Key { key: Key::Tab, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    steps(&mut h, 3);
    let value = f64::from(h.state().session.doc().expect("doc").doc.item(id).expect("frame").opacity);
    let next = focused(&h);
    let next_rect = next.and_then(|id| h.ctx.read_response(id).map(|r| r.rect));
    assert!((value - f64::from(0.4_f32)).abs() < 1e-12, "Tab committed the typed percentage between framework stops: {value}");
    assert_eq!(h.state().session.doc().expect("doc").history.undo.len() - steps_before, 1, "and it is one undo step");
    assert_ne!(next, Some(in_opacity), "the keyboard left the Opacity stop");
    // The stop it landed on must be an IDENTIFIED framework stop, not merely a different id: the Opacity row's chevron,
    // the framework button the row draws right after its field, at the rect the row recorded this pass.
    let landed = next_rect.expect("the stop that took the keyboard has a response rect");
    let more = opacity_rect(&h, "more");
    assert!((landed.center() - more.center()).length() < 1.5, "the keyboard landed on the Opacity row's chevron {more:?}, not {landed:?}");
    // And not on any Transform field, which are framework stops of the same panel.
    let inside: Vec<&str> =
        ["x", "y", "width", "height"].into_iter().filter(|key| field_rect(&h, key).is_some_and(|r| r.intersects(landed))).collect();
    assert!(inside.is_empty(), "the chevron is none of the Transform fields: {inside:?} (landed {landed:?})");
}
