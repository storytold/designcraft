//! Framework tests for the Media and Page Transitions panels: they drive the installed framework's components
//! with pointer and keyboard input and check the commands, the document and its undo history.

use super::*;
use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
use designcraft_doc::ItemId;
use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role};
use egui_kittest::{Harness, kittest::Queryable};
use std::sync::Arc;

/// A 2x1 RGBA image, inline, for a poster.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADklEQVR4nGP4z8DwHwQBEPgD/U6VwW8AAAAASUVORK5CYII=";
/// The start of an MP4 file: enough for a placed video.
const MP4: &str = "AAAAGGZ0eXBtcDQy";

struct Window {
    app: DesignApp,
    panel: fn(&mut DesignApp, &mut egui::Ui),
    controls: Option<Arc<Recorded>>,
    enabled: bool,
}

fn fixture(mut app: DesignApp, panel: fn(&mut DesignApp, &mut egui::Ui)) -> Harness<'static, Window> {
    app.framework_transform = false;
    app.framework_opacity = true;
    let ctx = egui::Context::default();
    app.install_framework(&ctx);
    assert!(app.framework_installed);
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(320.0, 480.0)).build_ui_state(
        move |ui, state: &mut Window| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), "");
                crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            ui.add_enabled_ui(state.enabled, |ui| {
                state.controls = record(ui, egui::Id::NULL.with("interactive-tests-b"), |ui| (state.panel)(&mut state.app, ui)).1;
            });
        },
        Window { app, panel, controls: None, enabled: true },
    );
    h.input_mut().max_texture_side = Some(8192);
    idle(&mut h, 4);
    h
}

fn step(h: &mut Harness<'_, Window>) {
    let mut raw = std::mem::take(h.input_mut());
    let ctx = h.ctx.clone();
    h.state_mut().app.raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

fn event(h: &mut Harness<'_, Window>, event: Event) {
    h.input_mut().events.push(event);
    step(h);
}

fn idle(h: &mut Harness<'_, Window>, n: usize) {
    for _ in 0..n {
        step(h);
    }
}

fn key_mod(h: &mut Harness<'_, Window>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    event(h, Event::ModifiersChanged(Modifiers::NONE));
    idle(h, 2);
}

fn pointer(h: &mut Harness<'_, Window>, rect: egui::Rect, pressed: bool) {
    let pos = rect.center();
    event(h, Event::PointerMoved(pos));
    event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    idle(h, 2);
}

fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
    pointer(h, rect, true);
    pointer(h, rect, false);
}

fn click(h: &mut Harness<'_, Window>, name: &str) {
    let rect = h.get_by_label(name).rect();
    click_rect(h, rect);
}

fn node(h: &Harness<'_, Window>, component: &str, name: &str) -> Element {
    let found: Vec<_> = h
        .state()
        .controls
        .as_ref()
        .unwrap()
        .elements
        .iter()
        .filter(|e| e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name))
        .cloned()
        .collect();
    assert_eq!(found.len(), 1, "exactly one actual {component} named {name:?}");
    found[0].clone()
}

fn undo(h: &Harness<'_, Window>) -> usize {
    h.state().app.session.active().unwrap().history.undo.len()
}

fn journal(h: &Harness<'_, Window>) -> Vec<(String, Value)> {
    h.state().app.session.journal.clone()
}

fn last(h: &Harness<'_, Window>, command: &str, params: Value) {
    assert_eq!(h.state().app.session.journal.last(), Some(&(command.to_owned(), params)));
}

fn no_replay(h: &mut Harness<'_, Window>) {
    let before = (journal(h), undo(h));
    idle(h, 4);
    assert_eq!((journal(h), undo(h)), before, "an idle frame must not run a command");
}

fn transitions_app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 2})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::pos2(500.0, 0.0), egui::vec2(500.0, 700.0)));
    // The page in view comes from the canvas view; give the document one.
    app.view_mut();
    app
}

fn media_app() -> (DesignApp, ItemId) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    let placed = app.run("file.place", json!({"base64": MP4, "name": "clip.mp4", "x": 100, "y": 100})).unwrap();
    let id = ItemId(placed["id"].as_u64().unwrap());
    app.run("selection.set", json!({"ids": [id.0]})).unwrap();
    app.run("media.options", json!({"loop": false, "controls": true})).unwrap();
    (app, id)
}

#[test]
fn transitions_kind_speed_orientation_and_apply_to_all_run_the_panel_commands() {
    let mut h = fixture(transitions_app(), transitions);
    assert!(h.query_all_by_role_and_label(Role::SpinButton, "Speed").next().is_none(), "no speed before a transition is chosen");
    let before = undo(&h);
    click(&mut h, "Transition");
    click(&mut h, "Blinds");
    last(&h, "page.transition", json!({"spread": 0, "kind": "blinds", "duration": 1.0, "horizontal": false}));
    assert_eq!(undo(&h), before + 1, "one pick is one Undo step");
    no_replay(&mut h);
    click(&mut h, "Horizontal");
    last(&h, "page.transition", json!({"spread": 0, "kind": "blinds", "duration": 1.0, "horizontal": true}));
    let speed = h.get_by_role_and_label(Role::SpinButton, "Speed").rect();
    click_rect(&mut h, speed);
    key_mod(&mut h, Key::A, Modifiers::COMMAND);
    event(&mut h, Event::Text("2.5".to_owned()));
    key_mod(&mut h, Key::Enter, Modifiers::NONE);
    last(&h, "page.transition", json!({"spread": 0, "kind": "blinds", "duration": 2.5, "horizontal": true}));
    click(&mut h, "Apply to All Spreads");
    last(&h, "page.transition", json!({"all": true, "kind": "blinds", "duration": 2.5, "horizontal": true}));
    no_replay(&mut h);
    click(&mut h, "Transition");
    click(&mut h, "None");
    last(&h, "page.transition", json!({"spread": 0, "kind": "none", "duration": 2.5, "horizontal": true}));
    assert!(h.query_all_by_role_and_label(Role::SpinButton, "Speed").next().is_none(), "no speed without a transition");
    no_replay(&mut h);
}

#[test]
fn media_options_and_poster_run_the_panel_commands() {
    let (app, id) = media_app();
    let mut h = fixture(app, media);
    assert_eq!(h.query_all_by_label("None").count(), 0, "a clip without a poster offers no way to clear one");
    let before = undo(&h);
    click(&mut h, "Show Controls");
    last(&h, "media.options", json!({"controls": false}));
    assert_eq!(undo(&h), before + 1);
    click(&mut h, "Loop");
    last(&h, "media.options", json!({"loop": true}));
    no_replay(&mut h);
    h.state_mut().app.run("media.options", json!({"ids": [id.0], "poster": {"base64": PNG, "name": "p.png"}})).unwrap();
    idle(&mut h, 3);
    let before = undo(&h);
    click(&mut h, "None");
    last(&h, "media.options", json!({"poster": null}));
    assert_eq!(undo(&h), before + 1);
    assert_eq!(h.query_all_by_label("None").count(), 0, "the poster is gone, so is its button");
    no_replay(&mut h);
}

#[test]
fn media_and_transitions_do_nothing_under_a_disabled_parent() {
    let (app, _) = media_app();
    let mut h = fixture(app, media);
    h.state_mut().enabled = false;
    idle(&mut h, 3);
    assert!(node(&h, "Checkbox", "Loop").facts.unwrap().disabled);
    let before = (journal(&h), undo(&h));
    click(&mut h, "Loop");
    assert_eq!((journal(&h), undo(&h)), before);
    let mut h = fixture(transitions_app(), transitions);
    h.state_mut().enabled = false;
    idle(&mut h, 3);
    assert!(node(&h, "Button", "Apply to All Spreads").facts.unwrap().disabled);
    let before = (journal(&h), undo(&h));
    click(&mut h, "Apply to All Spreads");
    assert_eq!((journal(&h), undo(&h)), before);
}
