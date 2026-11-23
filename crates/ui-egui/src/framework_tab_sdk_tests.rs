#![cfg(not(target_arch = "wasm32"))]

//! One bounded falsifier for the Tab diagnosis: does Tab move and commit between two FRAMEWORK stops in the same
//! corrected host setup (event-bearing raw hook + public `set_input`)? The Transform pilot's X/Y/W/H fields are
//! enabled alongside the Opacity field, so both stops are framework components in the same panel.
//!
//! If this passes, a framework-to-framework route exists and the mixed-host failure is specifically about a
//! handoff to the host's own stops. If it fails, no such route is proven and the mixed-host failure cannot yet be
//! attributed to a handoff gap.

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

fn opacity_rect(h: &Harness<'_, DesignApp>) -> egui::Rect {
    h.state()
        .framework_opacity_fields
        .iter()
        .rev()
        .find(|(k, _)| k == "opacity")
        .map(|(_, r)| *r)
        .expect("the Opacity pilot field is on screen")
}

#[test]
fn tab_between_two_framework_stops_commits_and_moves_to_the_next_framework_stop() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"title": "Two stops"})).expect("a new document");
    // Points, so the Transform pilot really draws its framework fields (picas keep the bespoke ones).
    app.run("document.preferences", json!({"horizontalUnits": "points", "verticalUnits": "points"})).expect("points");
    let id = designcraft_doc::ItemId(app.run("frame.create", json!({"rect": [72, 72, 272, 172]})).expect("a frame")["id"].as_u64().expect("an id"));
    app.run("selection.set", json!({"ids": [id.0]})).expect("the frame is selected");
    let mut h = window_harness(app);
    // Both pilots must really be on screen: the Opacity field and the Transform fields.
    let opacity = opacity_rect(&h);
    let width = field_rect(&h, "x").expect("the Transform pilot's X field is on screen beside Opacity");
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
    let next_stop = field_rect(&h, "x");
    eprintln!(
        "SDK->SDK: in_opacity={in_opacity:?} next={next:?} next_rect={next_rect:?} x_field={next_stop:?} opacity={value} undo={} (before {steps_before})",
        h.state().session.doc().expect("doc").history.undo.len()
    );
    assert!((value - f64::from(0.4_f32)).abs() < 1e-12, "Tab committed the typed percentage between framework stops: {value}");
    assert_eq!(h.state().session.doc().expect("doc").history.undo.len() - steps_before, 1, "and it is one undo step");
    assert_ne!(next, Some(in_opacity), "the keyboard left the Opacity stop");
    // The stop it landed on must be an IDENTIFIED framework stop, not merely a different id: the Transform
    // pilot's X field — the first framework stop in the panel, which Tab wraps round to from Opacity — at the
    // rect that pilot recorded this pass.
    let next_stop = next_stop.expect("the Transform X field is still on screen after the move");
    let landed = next_rect.expect("the stop that took the keyboard has a response rect");
    // Its text part is narrower than the field's box, so the stop is identified by lying inside exactly one of
    // the framework fields this pass recorded — and that one must be the Transform pilot's X field.
    let inside: Vec<&str> = ["x", "y", "width", "height"]
        .into_iter()
        .filter(|key| field_rect(&h, key).is_some_and(|r| r.contains_rect(landed)))
        .collect();
    assert_eq!(inside, ["x"], "the keyboard landed inside the Transform X framework stop {next_stop:?}, inside {inside:?} (landed {landed:?})");
}
