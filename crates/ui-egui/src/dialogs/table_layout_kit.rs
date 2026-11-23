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
pub(crate) fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
    harness_sized(app, vec2(1000.0, 900.0))
}

/// [`harness`] in a window of `size`.
pub(crate) fn harness_sized(mut app: DesignApp, size: egui::Vec2) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    // The dialog router gates on the installed framework runtime; the pilot flag only asks for it.
    app.install_framework(&ctx);
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(size).build_ui_state(
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

pub(crate) fn event(h: &mut Harness<'_, DesignApp>, event: Event) {
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

pub(crate) fn click_at(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
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
    // The entry is a node of the opened list; a title or a label of the same words can share its name, so the list's own
    // roles come first and the last node named so (the popup is drawn above the dialog) is the fallback.
    let options = [Role::ListBoxOption, Role::MenuItem, Role::MenuItemRadio, Role::ListItem, Role::Row];
    let found: Vec<_> = h.query_all_by_label(choice).collect();
    let node = found
        .iter()
        .find(|node| options.contains(&node.accesskit_node().role()))
        .or(found.last())
        .unwrap_or_else(|| panic!("no entry named {choice:?}"));
    let at = node.rect().center();
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

/// The centre of the `nth` node with one of `roles` whose name contains `part`: a field whose name the form builds from a
/// caption (with or without the colon a field adds) is found by the words of its caption.
pub(crate) fn centre_containing(h: &Harness<'_, DesignApp>, roles: &[Role], part: &str, nth: usize) -> egui::Pos2 {
    let found: Vec<_> = h.query_all_by_label_contains(part).filter(|node| roles.contains(&node.accesskit_node().role())).collect();
    let node = found.get(nth).unwrap_or_else(|| panic!("no {roles:?} named like {part:?} (number {nth}); {} found", found.len()));
    node.rect().center()
}

/// Clicks the `nth` node with one of `roles` whose name contains `part`.
pub(crate) fn click_named_like(h: &mut Harness<'_, DesignApp>, roles: &[Role], part: &str, nth: usize) {
    let at = centre_containing(h, roles, part, nth);
    click_at(h, at);
}

/// Types `text` over the `nth` field whose name contains `part` and leaves it with Tab, so the field commits.
pub(crate) fn type_and_commit_named_like(h: &mut Harness<'_, DesignApp>, part: &str, nth: usize, text: &str) {
    click_named_like(h, FIELD, part, nth);
    key(h, Key::A, Modifiers::COMMAND);
    event(h, Event::Text(text.to_owned()));
    steps(h, 3);
    key(h, Key::Tab, Modifiers::NONE);
}

/// Opens the `nth` picker whose name contains `part` and picks the entry named `choice`.
pub(crate) fn pick_named_like(h: &mut Harness<'_, DesignApp>, part: &str, nth: usize, choice: &str) {
    click_named_like(h, &[Role::ComboBox], part, nth);
    let options = [Role::ListBoxOption, Role::MenuItem, Role::MenuItemRadio, Role::ListItem, Role::Row];
    let found: Vec<_> = h.query_all_by_label(choice).collect();
    let node = found
        .iter()
        .find(|node| options.contains(&node.accesskit_node().role()))
        .or(found.last())
        .unwrap_or_else(|| panic!("no entry named {choice:?}"));
    let at = node.rect().center();
    click_at(h, at);
}

/// Opens the dialog the menu command `command` opens, in a harness that is already showing dialogs, and runs a few frames.
pub(crate) fn reopen(h: &mut Harness<'_, DesignApp>, command: &str) {
    open(h.state_mut(), command);
    steps(h, 3);
}

/// Whether any node is named `label`.
pub(crate) fn shows(h: &Harness<'_, DesignApp>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

/// Resizes the window and runs a few frames.
pub(crate) fn resize(h: &mut Harness<'_, DesignApp>, size: egui::Vec2) {
    h.set_size(size);
    steps(h, 4);
}

/// The window.
pub(crate) fn screen(h: &Harness<'_, DesignApp>) -> egui::Rect {
    h.ctx.content_rect()
}

/// The bounds of the first node named `label` that is not a dialog frame; `None` when nothing is named so.
pub(crate) fn rect_of(h: &Harness<'_, DesignApp>, label: &str) -> Option<egui::Rect> {
    h.query_all_by_label(label).find(|node| node.accesskit_node().role() != Role::Dialog).map(|node| node.rect())
}

/// Whether a node named `label` lies wholly inside the window.
pub(crate) fn in_view(h: &Harness<'_, DesignApp>, label: &str) -> bool {
    h.query_all_by_label(label).any(|node| node.accesskit_node().role() != Role::Dialog && screen(h).contains_rect(node.rect()))
}

/// Scrolls the node named `label` into view, as an assistive tool would, and returns its bounds; panics when it cannot be
/// reached.
pub(crate) fn reach(h: &mut Harness<'_, DesignApp>, label: &str) -> egui::Rect {
    for _ in 0..4 {
        if let Some(rect) = rect_of(h, label).filter(|rect| screen(h).contains_rect(*rect)) {
            return rect;
        }
        if let Some(node) = h.query_all_by_label(label).find(|node| node.accesskit_node().role() != Role::Dialog) {
            node.scroll_to_me();
        }
        steps(h, 3);
    }
    let found = rect_of(h, label);
    let window = screen(h);
    panic!("{label:?} cannot be reached: {found:?} in {window:?}");
}

/// The centre of the text field whose text is `shown`, ignoring case and a leading `#`.
pub(crate) fn field_showing(h: &Harness<'_, DesignApp>, shown: &str) -> egui::Pos2 {
    let want = shown.trim_start_matches('#').to_lowercase();
    h.query_all_by_role(Role::TextInput)
        .find(|node| node.value().is_some_and(|value| value.trim_start_matches('#').to_lowercase() == want))
        .unwrap_or_else(|| panic!("no text field shows {shown:?}"))
        .rect()
        .center()
}

/// Types `text` over the field that shows `shown` and leaves it with Tab.
pub(crate) fn retype_showing(h: &mut Harness<'_, DesignApp>, shown: &str, text: &str) {
    let at = field_showing(h, shown);
    click_at(h, at);
    key(h, Key::A, Modifiers::COMMAND);
    event(h, Event::Text(text.to_owned()));
    steps(h, 3);
    key(h, Key::Tab, Modifiers::NONE);
}

/// Drags the pointer from `from` to `to` in eight steps, as a person does.
pub(crate) fn drag(h: &mut Harness<'_, DesignApp>, from: egui::Pos2, to: egui::Pos2) {
    event(h, Event::PointerMoved(from));
    event(h, Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    for step in 1..=8 {
        event(h, Event::PointerMoved(from + (to - from) * (step as f32 / 8.0)));
    }
    event(h, Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    steps(h, 3);
}

/// The bounds of the dialog named `title`.
pub(crate) fn dialog_rect(h: &Harness<'_, DesignApp>, title: &str) -> egui::Rect {
    h.get_by_role_and_label(Role::Dialog, title).rect()
}

/// A panel alone on an installed framework: `show` draws it in a window of `size`, frames 50 ms apart, every frame through
/// the app's input hook (use [`steps`], [`event`] and the click helpers).
pub(crate) fn panel_harness(mut app: DesignApp, size: egui::Vec2, show: fn(&mut DesignApp, &mut egui::Ui)) -> Harness<'static, DesignApp> {
    let ctx = egui::Context::default();
    app.install_framework(&ctx);
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(size).with_step_dt(0.05).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                ready = true;
                return;
            }
            show(app, ui);
        },
        app,
    );
    h.input_mut().max_texture_side = Some(8192);
    steps(&mut h, 4);
    h
}

/// A press at `from`, a drag in eight steps to `to` and the release there, with `mods` held throughout.
pub(crate) fn drag_with(h: &mut Harness<'_, DesignApp>, from: egui::Pos2, to: egui::Pos2, mods: Modifiers) {
    if mods != Modifiers::NONE {
        event(h, Event::ModifiersChanged(mods));
    }
    event(h, Event::PointerMoved(from));
    steps(h, 2);
    event(h, Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: mods });
    steps(h, 1);
    for step in 1..=8 {
        event(h, Event::PointerMoved(from.lerp(to, step as f32 / 8.0)));
    }
    steps(h, 2);
    event(h, Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: mods });
    if mods != Modifiers::NONE {
        event(h, Event::ModifiersChanged(Modifiers::NONE));
    }
    steps(h, 3);
}

/// The centre of the node named `label`, preferring a row or a button when several nodes share the name.
pub(crate) fn centre_of(h: &Harness<'_, DesignApp>, label: &str) -> egui::Pos2 {
    let nodes: Vec<_> = h.query_all_by_label(label).collect();
    let pick = [Role::TreeItem, Role::Button]
        .into_iter()
        .find_map(|role| nodes.iter().find(|n| n.accesskit_node().role() == role))
        .or(nodes.first())
        .unwrap_or_else(|| panic!("no node named {label:?}"));
    pick.rect().center()
}
