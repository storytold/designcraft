//! The shared harness of the table and layout form tests: the real dialog host over an installed framework, driven
//! by pointer and keyboard events as a user does.

use crate::DesignApp;
use crate::theme::Tokens;
use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use serde_json::{Value, json};

/// A document of `pages` pages with its canvas laid out, in English.
pub(crate) fn app(pages: u64, facing: bool) -> DesignApp {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    app.ui.language = "en".to_owned();
    app.run("file.new", json!({"pages": pages, "facingPages": facing})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0)));
    app
}

/// Opens the dialog the menu command `command` opens.
pub(crate) fn open(app: &mut DesignApp, command: &str) {
    crate::menus::activate(app, command, &Value::Null);
    assert!(app.ui.dialog.is_some(), "{command} opens its dialog");
}

/// The app with the framework installed, showing its active dialog every frame.
pub(crate) fn harness(mut app: DesignApp) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    // The dialog router gates on the installed framework runtime; the pilot flag only asks for it.
    app.framework_transform = false;
    app.framework_opacity = true;
    app.install_framework(&ctx);
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    assert!(app.framework_installed, "the framework forms need the installed runtime");
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(1000.0, 900.0)).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            crate::dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    steps(&mut h, 3);
    h
}

/// Runs `count` frames, each through the app's input hook.
pub(crate) fn steps(h: &mut Harness<'_, DesignApp>, count: usize) {
    for _ in 0..count {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
}

fn event(h: &mut Harness<'_, DesignApp>, event: Event) {
    let mut raw = std::mem::take(h.input_mut());
    raw.events.push(event);
    let ctx = h.ctx.clone();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

/// A key press and release, then a few frames.
pub(crate) fn key(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
    }
    event(h, Event::ModifiersChanged(Modifiers::NONE));
    steps(h, 3);
}

fn click_at(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
    event(h, Event::PointerMoved(at));
    for pressed in [true, false] {
        event(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    steps(h, 3);
}

/// The centre of the `nth` node named `label` that has one of `roles`.
fn centre(h: &Harness<'_, DesignApp>, roles: &[Role], label: &str, nth: usize) -> egui::Pos2 {
    let found: Vec<_> = h.query_all_by_label(label).filter(|node| roles.contains(&node.accesskit_node().role())).collect();
    let node = found.get(nth).unwrap_or_else(|| panic!("no {roles:?} named {label:?} (number {nth}); {} found", found.len()));
    node.rect().center()
}

/// Clicks the `nth` control named `label` that has one of `roles`.
pub(crate) fn click(h: &mut Harness<'_, DesignApp>, roles: &[Role], label: &str, nth: usize) {
    let at = centre(h, roles, label, nth);
    click_at(h, at);
}

/// Opens the picker named `label` and picks the entry named `choice`.
pub(crate) fn pick(h: &mut Harness<'_, DesignApp>, label: &str, choice: &str) {
    click(h, &[Role::ComboBox], label, 0);
    let at = h.get_by_label(choice).rect().center();
    click_at(h, at);
}

/// Whether the `nth` control named `label` that has one of `roles` is disabled.
pub(crate) fn disabled(h: &Harness<'_, DesignApp>, roles: &[Role], label: &str, nth: usize) -> bool {
    let found: Vec<_> = h.query_all_by_label(label).filter(|node| roles.contains(&node.accesskit_node().role())).collect();
    found.get(nth).unwrap_or_else(|| panic!("no {roles:?} named {label:?} (number {nth})")).accesskit_node().is_disabled()
}

/// The roles a text-like field can report: a plain text field, a number field and an editable picker.
pub(crate) const FIELD: &[Role] = &[Role::TextInput, Role::SpinButton, Role::ComboBox];

/// Types `text` over whatever the `nth` field named `label` holds, as a user does: click, select all, type.
pub(crate) fn type_into(h: &mut Harness<'_, DesignApp>, label: &str, nth: usize, text: &str) {
    click(h, FIELD, label, nth);
    key(h, Key::A, Modifiers::COMMAND);
    event(h, Event::Text(text.to_owned()));
    steps(h, 3);
}

/// Types `text` into the field and leaves it with Tab, so the field commits.
pub(crate) fn type_and_commit(h: &mut Harness<'_, DesignApp>, label: &str, nth: usize, text: &str) {
    type_into(h, label, nth, text);
    key(h, Key::Tab, Modifiers::NONE);
}

/// The open dialog's draft.
pub(crate) fn draft<'a>(h: &'a Harness<'_, DesignApp>) -> &'a super::Dialog {
    h.state().ui.dialog.as_ref().expect("the dialog is open")
}

/// The commands the journal holds under `id`.
pub(crate) fn journal(h: &Harness<'_, DesignApp>, id: &str) -> Vec<(String, Value)> {
    h.state().session.journal.iter().filter(|(command, _)| command == id).cloned().collect()
}

/// How many undo steps the active document holds.
pub(crate) fn undo_steps(h: &Harness<'_, DesignApp>) -> usize {
    h.state().session.doc().unwrap().history.undo.len()
}

/// A document with a text frame of two paragraphs, "One" and "Two", the caret in the first, whose Word Spacing Minimum is
/// 70%; and the story's id.
pub(crate) fn two_paragraphs() -> (DesignApp, u64) {
    let mut app = app(1, false);
    let frame = app.run("frame.create", json!({"rect": [72, 72, 300, 400], "content": "text", "text": "One\nTwo"})).unwrap();
    app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
    app.run("type.para", json!({"attrs": {"wordSpaceMin": 0.7}})).unwrap();
    let story = frame["story"].as_u64().unwrap();
    (app, story)
}

/// The resolved paragraph attribute `key` of the paragraph at byte `at` of `story`; the caret ends there.
pub(crate) fn para_attr(h: &mut Harness<'_, DesignApp>, story: u64, at: usize, key: &str) -> Value {
    h.state_mut().run("text.select", json!({"story": story, "anchor": at, "focus": at})).unwrap();
    h.state_mut().run("type.selectionAttrs", json!({})).unwrap()["para"][key].clone()
}

/// Clicks the first node named `label`, whatever its role (a row of a list, a segment of a strip).
pub(crate) fn click_first(h: &mut Harness<'_, DesignApp>, label: &str) {
    let at = h.query_all_by_label(label).next().unwrap_or_else(|| panic!("nothing named {label:?}")).rect().center();
    click_at(h, at);
}

/// The roles a check box, a switch and a toggle button can report.
pub(crate) const TOGGLE: &[Role] = &[Role::CheckBox, Role::Switch, Role::Button, Role::RadioButton];

/// Clicks the first node whose name contains `part`, whatever its role.
pub(crate) fn click_containing(h: &mut Harness<'_, DesignApp>, part: &str) {
    let at = h.query_all_by_label_contains(part).next().unwrap_or_else(|| panic!("nothing named like {part:?}")).rect().center();
    click_at(h, at);
}

/// Types `text` into whatever has the keyboard, then a few frames.
pub(crate) fn type_text(h: &mut Harness<'_, DesignApp>, text: &str) {
    event(h, Event::Text(text.to_owned()));
    steps(h, 3);
}
