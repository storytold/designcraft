use designcraft_doc::{Document, ItemId, Selection, SpreadRef};
use designcraft_tools::{PointerEvent, PointerKind};
use egui_kittest::{Harness, kittest::Queryable};

use super::*;
use crate::DesignApp;

fn configured_frames() -> (DesignApp, [ItemId; 2]) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({"pages": 1})).unwrap();
    let frames = [36, 336].map(|x| {
        let frame = app.run("frame.create", json!({"rect": [x, 36, x + 260, 436], "content": "text"})).unwrap();
        ItemId(frame["id"].as_u64().unwrap())
    });
    for (id, options) in [
        (
            frames[0],
            json!({"columns": 3, "gutter": 18, "inset": [2, 4, 6, 8], "verticalJustification": "center",
                "columnRule": false, "columnRuleWeight": 1, "columnRuleColor": "[Black]"}),
        ),
        (
            frames[1],
            json!({"columns": 2, "gutter": 24, "inset": [10, 20, 30, 40], "verticalJustification": "bottom",
                "columnRule": true, "columnRuleWeight": 4, "columnRuleColor": "[Paper]", "columnRuleTint": 0.4,
                "columnRuleTopInset": 6, "columnRuleBottomInset": 9, "columnRuleOffset": -2}),
        ),
    ] {
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        app.run("object.textFrameOptions", options).unwrap();
    }
    (app, frames)
}

fn click_caret(app: &mut DesignApp, frame: ItemId) {
    app.select_tool("type");
    let bounds = app.session.doc().unwrap().doc.item(frame).unwrap().bounds();
    let point = app.session.layout().xf(SpreadRef::Doc(0)) * bounds.center();
    for kind in [PointerKind::Down, PointerKind::Up] {
        app.session.pointer(&PointerEvent::new(kind, point.x, point.y), designcraft_engine::ViewInfo::at_zoom(1.0)).unwrap();
    }
    let selection = &app.session.doc().unwrap().selection;
    assert!(selection.items.is_empty(), "the real Type-tool click must leave a text caret");
    assert_eq!(selection.text.unwrap().frame, Some(frame));
}

fn open_options(mut app: DesignApp) -> DesignApp {
    activate(&mut app, "object.textFrameOptions", &Value::Null);
    assert_eq!(app.ui.dialog.as_ref().unwrap().id, "textFrameOptions");
    app
}

fn dialog(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_size(egui::vec2(800.0, 800.0)).build_ui_state(
        move |ui, app| {
            if fonts_installed {
                crate::dialogs::show(app, ui.ctx());
            } else {
                crate::theme::install_fonts(ui.ctx(), "en");
                ui.ctx().request_repaint();
                fonts_installed = true;
            }
        },
        app,
    );
    harness.run();
    harness
}

fn menu_dialog(app: DesignApp) -> Harness<'static, DesignApp> {
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_size(egui::vec2(800.0, 1000.0)).with_max_steps(12).build_ui_state(
        move |ui, app| {
            if !fonts_installed {
                crate::theme::install_fonts(ui.ctx(), "en");
                ui.ctx().request_repaint();
                fonts_installed = true;
            } else if app.ui.dialog.is_some() {
                crate::dialogs::show(app, ui.ctx());
            } else {
                let menus = menu_tree();
                let (_, entries) = menus.iter().find(|(name, _)| *name == "Object").unwrap();
                menu_items(app, ui, entries, "Object");
            }
        },
        app,
    );
    harness.run();
    harness
}

fn press_frame_options_shortcut(app: &mut DesignApp) {
    let ctx = egui::Context::default();
    let events = [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key { key: egui::Key::B, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::COMMAND })
        .collect();
    let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| shortcuts(app, ui.ctx()));
    output.textures_delta.clear();
}

fn edit_measure(harness: &mut Harness<'_, DesignApp>, label: &str, value: &str) {
    harness.get_by_role_and_label(egui::accesskit::Role::TextInput, label).click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.run();
    harness.get_by_role_and_label(egui::accesskit::Role::TextInput, label).type_text(value);
    harness.run();
}

fn assert_roundtrip(app: &mut DesignApp, before: &Document, expected: &Document, selection: &Selection, undo_len: usize) {
    let st = app.session.doc().unwrap();
    assert_eq!(*st.doc, *expected, "only the bound frames' explicit fields may change");
    assert_eq!(st.selection, *selection);
    assert_eq!(st.history.undo.len(), undo_len + 1);
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(*app.session.doc().unwrap().doc, *before);
    assert_eq!(app.session.doc().unwrap().selection, *selection);
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(*app.session.doc().unwrap().doc, *expected);
    assert_eq!(app.session.doc().unwrap().selection, *selection);
}

#[test]
fn tfo_menu_hydrates_and_disables_rules() {
    let (app, frames) = configured_frames();
    let st = app.session.doc().unwrap();
    let before = st.doc.clone();
    let selection = st.selection.clone();
    let undo_len = st.history.undo.len();
    let app = open_options(app);
    let fields = &app.ui.dialog.as_ref().unwrap().fields;
    let units = app.session.doc().unwrap().doc.settings.horizontal_units;
    let measure = |value| json!(designcraft_geom::format_measure(value, units));
    for (key, value) in [
        ("columns", json!(2)),
        ("gutter", measure(24.0)),
        ("insetTop", measure(10.0)),
        ("insetLeft", measure(20.0)),
        ("insetBottom", measure(30.0)),
        ("insetRight", measure(40.0)),
        ("verticalJustification", json!("bottom")),
        ("columnRule", json!(true)),
        ("columnRuleWeight", json!("4 pt")),
        ("columnRuleColor", json!("[Paper]")),
        ("columnRuleTint", json!("40")),
        ("columnRuleTopInset", measure(6.0)),
        ("columnRuleBottomInset", measure(9.0)),
        ("columnRuleOffset", measure(-2.0)),
    ] {
        assert_eq!(fields[key], value, "{key}");
    }
    assert!(std::sync::Arc::ptr_eq(&app.session.doc().unwrap().doc, &before));
    let mut harness = dialog(app);
    let _ = harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Weight");
    harness.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Insert Column Rule").click();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().ui.dialog.is_none());
    let mut expected = (*before).clone();
    expected.item_mut(frames[1]).unwrap().text_frame_mut().unwrap().options.column_rule = false;
    assert_roundtrip(harness.state_mut(), &before, &expected, &selection, undo_len);
}

#[test]
fn tfo_menu_preserves_mixed_partial_edits_and_bound_selection() {
    let (mut app, frames) = configured_frames();
    app.run("selection.set", json!({"ids": [frames[1], frames[0]]})).unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    let undo_len = app.session.doc().unwrap().history.undo.len();
    let mut harness = dialog(open_options(app));
    // The final value equals the first displayed frame but differs on the other bound frame.
    edit_measure(&mut harness, "Top", "99 pt");
    edit_measure(&mut harness, "Top", "10 pt");
    // A control-driven selection change must not retarget the already-open dialog.
    harness.state_mut().run("selection.set", json!({"ids": [frames[1]]})).unwrap();
    let selection = harness.state().session.doc().unwrap().selection.clone();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().ui.dialog.is_none());
    let mut expected = (*before).clone();
    for frame in frames {
        expected.item_mut(frame).unwrap().text_frame_mut().unwrap().options.inset[0] = 10.0;
    }
    assert_roundtrip(harness.state_mut(), &before, &expected, &selection, undo_len);
}

fn check_caret_menu_or_control(control: bool) {
    let (mut app, frames) = configured_frames();
    std::sync::Arc::make_mut(&mut app.session.doc_mut().unwrap().doc).thread(frames[0], frames[1]).unwrap();
    click_caret(&mut app, frames[1]);
    let st = app.session.doc().unwrap();
    let before = st.doc.clone();
    let selection = st.selection.clone();
    let undo_len = st.history.undo.len();
    let mut harness = if control {
        let (request, _reply) = crate::control::ControlRequest::new("ui.dialog.open", json!({"id": "textFrameOptions", "fields": {"insetTop": 7}}));
        let crate::control::Outcome::Done(response) = crate::control::handle(&mut app, &egui::Context::default(), &request) else {
            panic!("dialog open must finish synchronously");
        };
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["result"]["fields"]["insetTop"], "7 pt");
        dialog(app)
    } else {
        assert!(enabled(&app, "object.textFrameOptions"), "the UI must enable TFO for the actual caret frame");
        assert!(menu_enabled(&app, "object.textFrameOptions"));
        let mut harness = menu_dialog(app);
        // Render the Object menu's existing entries and click its normal enabled button.
        // egui includes the rendered shortcut in this button's accessible label.
        let label = format!("Text Frame Options… {}", shortcut_text(&shortcut_of(harness.state(), "object.textFrameOptions").unwrap()));
        harness.get_by_role_and_label(egui::accesskit::Role::Button, &label).click();
        harness.run();
        assert_eq!(harness.state().ui.dialog.as_ref().unwrap().id, "textFrameOptions");
        harness
    };
    assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields["gutter"], "2p0", "hydrate the caret frame, not the first story frame");
    if !control {
        edit_measure(&mut harness, "Top", "7 pt");
    }
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().ui.dialog.is_none());
    let mut expected = (*before).clone();
    expected.item_mut(frames[1]).unwrap().text_frame_mut().unwrap().options.inset[0] = 7.0;
    assert_roundtrip(harness.state_mut(), &before, &expected, &selection, undo_len);
}

#[test]
fn r6_tfo_real_caret_menu_widget_is_enabled_and_applies_only_actual_frame() {
    check_caret_menu_or_control(false);
}

#[test]
fn r6_tfo_caret_control_open_hydrates_and_binds_without_ids() {
    check_caret_menu_or_control(true);
}

#[test]
fn r6_tfo_real_caret_shortcut_hydrates_and_applies_only_actual_frame() {
    let (mut app, frames) = configured_frames();
    std::sync::Arc::make_mut(&mut app.session.doc_mut().unwrap().doc).thread(frames[0], frames[1]).unwrap();
    click_caret(&mut app, frames[1]);
    let st = app.session.doc().unwrap();
    let before = st.doc.clone();
    let selection = st.selection.clone();
    let undo_len = st.history.undo.len();
    assert_eq!(shortcut_of(&app, "object.textFrameOptions").as_deref(), Some("Cmd+B"));
    press_frame_options_shortcut(&mut app);
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields["gutter"], "2p0");
    let mut harness = dialog(app);
    edit_measure(&mut harness, "Top", "7 pt");
    harness.key_press(egui::Key::Enter);
    harness.run();
    let mut expected = (*before).clone();
    expected.item_mut(frames[1]).unwrap().text_frame_mut().unwrap().options.inset[0] = 7.0;
    assert_roundtrip(harness.state_mut(), &before, &expected, &selection, undo_len);
}

#[test]
fn r6_tfo_ui_availability_rejects_missing_and_non_text_targets() {
    let mut empty = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    assert!(!enabled(&empty, "object.textFrameOptions"));
    press_frame_options_shortcut(&mut empty);
    assert!(empty.ui.dialog.is_none());
    let (mut app, frames) = configured_frames();
    let graphic = app.run("frame.create", json!({"rect": [36, 450, 200, 550], "content": "graphic"})).unwrap();
    let graphic = ItemId(graphic["id"].as_u64().unwrap());
    app.run("selection.set", json!({"ids": [graphic]})).unwrap();
    assert!(!enabled(&app, "object.textFrameOptions"));
    assert!(!menu_enabled(&app, "object.textFrameOptions"));
    press_frame_options_shortcut(&mut app);
    assert!(app.ui.dialog.is_none());
    click_caret(&mut app, frames[1]);
    assert!(enabled(&app, "object.textFrameOptions"));
    for frame in [None, Some(graphic), Some(ItemId(u64::MAX))] {
        app.session.doc_mut().unwrap().selection.text.as_mut().unwrap().frame = frame;
        assert!(!enabled(&app, "object.textFrameOptions"), "only a valid actual caret frame enables the UI");
        assert!(!menu_enabled(&app, "object.textFrameOptions"));
        press_frame_options_shortcut(&mut app);
        assert!(app.ui.dialog.is_none());
    }
}

fn owner_control(app: &mut DesignApp, method: &str, params: Value) -> Value {
    let (request, _reply) = crate::control::ControlRequest::new(method, params);
    let crate::control::Outcome::Done(response) = crate::control::handle(app, &egui::Context::default(), &request) else {
        panic!("owner regression control requests must finish synchronously");
    };
    response
}

fn owner_ok(app: &mut DesignApp, method: &str, params: Value) -> Value {
    let response = owner_control(app, method, params);
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}

fn owner_engine(app: &mut DesignApp, command: &str, params: Value) -> Value {
    owner_ok(app, "engine.execute", json!({"command": command, "params": params}))
}

fn owner_colliding_frames(caret: bool) -> (DesignApp, [ItemId; 2]) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    let frames = [2, 10].map(|top| {
        owner_engine(&mut app, "file.new", json!({"pages": 1, "facingPages": false}));
        let frame = owner_engine(&mut app, "frame.create", json!({"rect": [36, 36, 296, 436], "content": "text"}));
        let frame = ItemId(frame["id"].as_u64().unwrap());
        owner_engine(&mut app, "selection.set", json!({"ids": [frame]}));
        owner_engine(&mut app, "object.textFrameOptions", json!({"columns": 2, "gutter": 24, "inset": [top, 4, 6, 8]}));
        frame
    });
    assert_eq!(frames[0], frames[1], "ItemIds collide across these two real documents");
    assert_ne!(app.session.documents()[0].uid, app.session.documents()[1].uid);
    owner_engine(&mut app, "object.textFrameOptions", json!({"gutter": 27}));
    owner_engine(&mut app, "edit.undo", json!({}));
    assert!(!app.session.doc().unwrap().history.redo.is_empty());
    owner_engine(&mut app, "file.activate", json!({"index": 0}));
    if caret {
        click_caret(&mut app, frames[0]);
    }
    (app, frames)
}

fn assert_owner_document_unchanged(actual: &designcraft_engine::DocState, before: &designcraft_engine::DocState) {
    assert_eq!(actual.uid, before.uid);
    assert!(std::sync::Arc::ptr_eq(&actual.doc, &before.doc), "document {} was replaced", actual.uid);
    assert_eq!(*actual.doc, *before.doc);
    assert!(std::sync::Arc::ptr_eq(&actual.saved_doc, &before.saved_doc));
    assert_eq!(actual.selection, before.selection);
    assert_eq!(actual.revision, before.revision);
    assert_eq!(actual.saved_revision, before.saved_revision);
    assert_eq!(actual.path, before.path);
    assert_eq!(actual.active_layer, before.active_layer);
    assert_eq!(actual.history.limit, before.history.limit);
    for (actual, before) in [(&actual.history.undo, &before.history.undo), (&actual.history.redo, &before.history.redo)] {
        assert_eq!(actual.len(), before.len());
        for (actual, before) in actual.iter().zip(before) {
            assert_eq!(actual.label, before.label);
            assert!(std::sync::Arc::ptr_eq(&actual.doc, &before.doc));
            assert_eq!(*actual.doc, *before.doc);
            assert_eq!(actual.selection, before.selection);
        }
    }
}

fn assert_owner_documents_unchanged(app: &DesignApp, before: &[designcraft_engine::DocState], active_uid: u64) {
    assert_eq!(app.session.active().map(|st| st.uid), Some(active_uid));
    assert_eq!(app.session.documents().len(), before.len());
    for (actual, before) in app.session.documents().iter().zip(before) {
        assert_owner_document_unchanged(actual, before);
    }
}

fn assert_owner_redo_retained(app: &mut DesignApp, before: &designcraft_engine::DocState, frame: ItemId) {
    owner_engine(app, "edit.redo", json!({}));
    let mut expected = (*before.doc).clone();
    expected.item_mut(frame).unwrap().text_frame_mut().unwrap().options.gutter = 27.0;
    let st = app.session.doc().unwrap();
    assert_eq!(st.uid, before.uid);
    assert_eq!(*st.doc, expected, "the saved redo entry still restores its original change");
    assert_eq!(st.selection, before.selection);
    assert_eq!(st.history.undo.len(), before.history.undo.len() + 1);
    assert_eq!(st.history.redo.len(), before.history.redo.len() - 1);
}

fn check_owner_control_switch(caret: bool) {
    let (app, frames) = owner_colliding_frames(caret);
    let before = app.session.documents().to_vec();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "file.activate", json!({"index": 1}));
    assert!(harness.state().ui.dialog.is_some(), "the supported control switch leaves the dialog open");
    assert_owner_documents_unchanged(harness.state(), &before, before[1].uid);
    let response = owner_control(harness.state_mut(), "ui.dialog.confirm", json!({}));
    assert_owner_documents_unchanged(harness.state(), &before, before[1].uid);
    assert_eq!(response["ok"], false, "confirmation must reject a different active document: {response}");
    assert!(response["error"].as_str().unwrap().contains("document"));
    assert!(harness.state().ui.dialog.is_none());
    assert_owner_redo_retained(harness.state_mut(), &before[1], frames[1]);
    assert_owner_document_unchanged(&harness.state().session.documents()[0], &before[0]);
}

#[test]
fn r6_tfo_owner_control_switch_rejects_colliding_frame_id() {
    check_owner_control_switch(false);
}

#[test]
fn r6_tfo_owner_caret_control_switch_rejects_colliding_frame_id() {
    check_owner_control_switch(true);
}

#[test]
fn r6_tfo_owner_closed_document_rejects_colliding_frame_id() {
    let (app, frames) = owner_colliding_frames(false);
    let other = app.session.documents()[1].clone();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "file.close", json!({"index": 0}));
    assert_eq!(harness.state().session.active_index(), Some(0), "index 0 now belongs to a different UID");
    assert!(harness.state().ui.dialog.is_some());
    let before = [other];
    assert_owner_documents_unchanged(harness.state(), &before, before[0].uid);
    let response = owner_control(harness.state_mut(), "ui.dialog.confirm", json!({}));
    assert_owner_documents_unchanged(harness.state(), &before, before[0].uid);
    assert_eq!(response["ok"], false, "{response}");
    assert!(response["error"].as_str().unwrap().contains("document"));
    assert!(harness.state().ui.dialog.is_none());
    assert_owner_redo_retained(harness.state_mut(), &before[0], frames[1]);
}

#[test]
fn r6_tfo_owner_return_to_same_uid_applies_only_opening_frame_and_undo() {
    let (app, frames) = owner_colliding_frames(false);
    let before = app.session.documents().to_vec();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "file.activate", json!({"index": 1}));
    assert_owner_documents_unchanged(harness.state(), &before, before[1].uid);
    owner_engine(harness.state_mut(), "file.activate", json!({"index": 0}));
    assert_owner_documents_unchanged(harness.state(), &before, before[0].uid);
    owner_ok(harness.state_mut(), "ui.dialog.confirm", json!({}));
    assert!(harness.state().ui.dialog.is_none());
    assert_eq!(harness.state().session.doc().unwrap().uid, before[0].uid);
    let mut expected = (*before[0].doc).clone();
    expected.item_mut(frames[0]).unwrap().text_frame_mut().unwrap().options.inset[0] = 7.0;
    assert_roundtrip(harness.state_mut(), &before[0].doc, &expected, &before[0].selection, before[0].history.undo.len());
    assert_owner_document_unchanged(&harness.state().session.documents()[1], &before[1]);
}

#[test]
fn r6_tfo_owner_no_edit_and_cancel_after_switch_preserve_both_histories() {
    for caret in [false, true] {
        for (cancel, edit) in [(false, false), (true, false), (true, true)] {
            let (app, frames) = owner_colliding_frames(caret);
            let before = app.session.documents().to_vec();
            let mut harness = dialog(open_options(app));
            if edit {
                edit_measure(&mut harness, "Top", "7 pt");
            }
            owner_engine(harness.state_mut(), "file.activate", json!({"index": 1}));
            owner_ok(harness.state_mut(), if cancel { "ui.dialog.cancel" } else { "ui.dialog.confirm" }, json!({}));
            assert!(harness.state().ui.dialog.is_none());
            assert_owner_documents_unchanged(harness.state(), &before, before[1].uid);
            assert_owner_redo_retained(harness.state_mut(), &before[1], frames[1]);
            assert_owner_document_unchanged(&harness.state().session.documents()[0], &before[0]);
        }
    }
}

fn state_opening_frame(caret: bool) -> (DesignApp, ItemId, designcraft_doc::StoryId, u64) {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    owner_engine(&mut app, "file.new", json!({"pages": 1, "facingPages": false}));
    let next_id = app.session.doc().unwrap().doc.next_id;
    let frame = owner_engine(&mut app, "frame.create", json!({"rect": [36, 36, 296, 436], "content": "text", "text": "opening", "caret": false}));
    let id = ItemId(frame["id"].as_u64().unwrap());
    let story = designcraft_doc::StoryId(frame["story"].as_u64().unwrap());
    if caret {
        click_caret(&mut app, id);
    }
    assert_eq!(app.session.doc().unwrap().history.undo.len(), 1, "creation is the only undoable edit before opening TFO");
    (app, id, story, next_id)
}

fn state_retain_gutter_redo(app: &mut DesignApp, frame: ItemId) {
    owner_engine(app, "selection.set", json!({"ids": [frame]}));
    owner_engine(app, "object.textFrameOptions", json!({"gutter": 27}));
    owner_engine(app, "edit.undo", json!({}));
    assert!(!app.session.doc().unwrap().history.redo.is_empty());
}

fn state_replace_opening_frame(app: &mut DesignApp, frame: ItemId, story: designcraft_doc::StoryId, next_id: u64) {
    let uid = app.session.doc().unwrap().uid;
    let bounds = app.session.doc().unwrap().doc.item(frame).unwrap().bounds();
    owner_engine(app, "edit.undo", json!({}));
    let st = app.session.doc().unwrap();
    assert_eq!(st.uid, uid);
    assert!(st.doc.item(frame).is_none());
    assert_eq!(st.doc.next_id, next_id, "Undo restores the allocator from before frame creation");
    let replacement =
        owner_engine(app, "frame.create", json!({"rect": [336, 36, 596, 436], "content": "text", "text": "replacement", "caret": false}));
    assert_eq!(replacement["id"].as_u64(), Some(frame.0));
    assert_eq!(replacement["story"].as_u64(), Some(story.0));
    let st = app.session.doc().unwrap();
    assert_eq!(st.uid, uid);
    assert_ne!(st.doc.item(frame).unwrap().bounds(), bounds);
    assert_eq!(st.doc.story(story).unwrap().text, "replacement");
    state_retain_gutter_redo(app, frame);
}

fn assert_state_rejection_preserves_document(harness: &mut Harness<'_, DesignApp>) -> designcraft_engine::DocState {
    assert!(harness.state().ui.dialog.is_some());
    let before = harness.state().session.doc().unwrap().clone();
    let response = owner_control(harness.state_mut(), "ui.dialog.confirm", json!({}));
    assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);
    assert_eq!(harness.state().session.active().map(|st| st.uid), Some(before.uid));
    assert_eq!(response["ok"], false, "a stale edited dialog must reject before any engine edit: {response}");
    assert!(response["error"].as_str().unwrap().contains("document changed"), "{response}");
    assert!(harness.state().ui.dialog.is_none());
    before
}

fn check_state_undo_replacement(caret: bool) {
    let (app, frame, story, next_id) = state_opening_frame(caret);
    let opening = app.session.doc().unwrap().clone();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &opening);
    state_replace_opening_frame(harness.state_mut(), frame, story, next_id);
    assert!(!std::sync::Arc::ptr_eq(&harness.state().session.doc().unwrap().doc, &opening.doc));
    let before = assert_state_rejection_preserves_document(&mut harness);
    assert_eq!(before.uid, opening.uid);
    assert_eq!(before.doc.story(story).unwrap().text, "replacement");
    assert_owner_redo_retained(harness.state_mut(), &before, frame);
}

#[test]
fn r6_tfo_state_control_undo_replacement_rejects_reused_frame_and_story_ids() {
    check_state_undo_replacement(false);
}

#[test]
fn r6_tfo_state_caret_control_undo_replacement_rejects_reused_frame_and_story_ids() {
    check_state_undo_replacement(true);
}

#[test]
fn r6_tfo_state_unrelated_document_edit_rejects_and_preserves_redo() {
    let (mut app, frames) = configured_frames();
    owner_engine(&mut app, "selection.set", json!({"ids": [frames[0]]}));
    let opening = app.session.doc().unwrap().clone();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "selection.set", json!({"ids": [frames[1]]}));
    owner_engine(harness.state_mut(), "object.textFrameOptions", json!({"gutter": 37}));
    state_retain_gutter_redo(harness.state_mut(), frames[1]);
    let st = harness.state().session.doc().unwrap();
    assert_eq!(st.uid, opening.uid);
    assert!(!std::sync::Arc::ptr_eq(&st.doc, &opening.doc));
    assert_eq!(st.doc.item(frames[0]), opening.doc.item(frames[0]), "the bound frame itself remains unchanged");
    let before = assert_state_rejection_preserves_document(&mut harness);
    assert_owner_redo_retained(harness.state_mut(), &before, frames[1]);
}

#[test]
fn r6_tfo_state_deleted_opening_frame_rejects_before_engine_dispatch() {
    let (app, frame, _, _) = state_opening_frame(false);
    let uid = app.session.doc().unwrap().uid;
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "edit.clear", json!({}));
    assert!(harness.state().session.doc().unwrap().doc.item(frame).is_none());
    assert_eq!(harness.state().session.doc().unwrap().uid, uid);
    // The specific stale-document error distinguishes UI invalidation from the engine's missing-ID gate.
    assert_state_rejection_preserves_document(&mut harness);
}

#[cfg(not(target_arch = "wasm32"))]
struct StateRevertDirectory(std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl Drop for StateRevertDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn r6_tfo_state_control_revert_rejects_stale_dialog_and_preserves_redo() {
    let (mut app, frame, _, _) = state_opening_frame(false);
    let uid = app.session.doc().unwrap().uid;
    let path = std::env::temp_dir().join(format!("dc-r6-tfo-state-revert-{}-{uid}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    let directory = StateRevertDirectory(path);
    let path = directory.0.join("saved.designcraft");
    owner_engine(&mut app, "file.save", json!({"path": path.to_string_lossy()}));
    let opening = app.session.doc().unwrap().clone();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "object.textFrameOptions", json!({"gutter": 37}));
    owner_engine(harness.state_mut(), "file.revert", json!({}));
    let st = harness.state().session.doc().unwrap();
    assert_eq!(st.uid, opening.uid);
    assert!(st.doc.item(frame).is_some(), "Revert restored the frame with the same numeric ID");
    assert!(!std::sync::Arc::ptr_eq(&st.doc, &opening.doc));
    state_retain_gutter_redo(harness.state_mut(), frame);
    let before = assert_state_rejection_preserves_document(&mut harness);
    assert_owner_redo_retained(harness.state_mut(), &before, frame);
}

#[test]
fn r6_tfo_state_selection_only_changes_preserve_bound_partial_edit() {
    let (mut app, frames) = configured_frames();
    owner_engine(&mut app, "selection.set", json!({"ids": [frames[1], frames[0]]}));
    let opening = app.session.doc().unwrap().clone();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "selection.set", json!({"ids": [frames[1]]}));
    let st = harness.state().session.doc().unwrap();
    assert!(std::sync::Arc::ptr_eq(&st.doc, &opening.doc));
    assert_ne!(st.revision, opening.revision, "selection revisions must not invalidate the opening document identity");
    let selection = st.selection.clone();
    owner_ok(harness.state_mut(), "ui.dialog.confirm", json!({}));
    let mut expected = (*opening.doc).clone();
    for frame in frames {
        expected.item_mut(frame).unwrap().text_frame_mut().unwrap().options.inset[0] = 7.0;
    }
    assert_roundtrip(harness.state_mut(), &opening.doc, &expected, &selection, opening.history.undo.len());
}

#[test]
fn r6_tfo_state_close_owner_rejects_without_an_active_document() {
    let (app, _, _, _) = state_opening_frame(false);
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "file.close", json!({}));
    assert!(harness.state().session.documents().is_empty());
    let response = owner_control(harness.state_mut(), "ui.dialog.confirm", json!({}));
    assert_eq!(response["ok"], false, "{response}");
    assert!(response["error"].as_str().unwrap().contains("document"));
    assert!(harness.state().ui.dialog.is_none());
    assert!(harness.state().session.documents().is_empty());
    assert!(harness.state().session.active().is_none());
}

#[test]
fn r6_tfo_state_no_edit_and_cancel_after_replacement_preserve_history() {
    for caret in [false, true] {
        for (cancel, edit) in [(false, false), (true, false), (true, true)] {
            let (app, frame, story, next_id) = state_opening_frame(caret);
            let mut harness = dialog(open_options(app));
            if edit {
                edit_measure(&mut harness, "Top", "7 pt");
            }
            state_replace_opening_frame(harness.state_mut(), frame, story, next_id);
            let before = harness.state().session.doc().unwrap().clone();
            owner_ok(harness.state_mut(), if cancel { "ui.dialog.cancel" } else { "ui.dialog.confirm" }, json!({}));
            assert!(harness.state().ui.dialog.is_none());
            assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);
            assert_eq!(harness.state().session.active().map(|st| st.uid), Some(before.uid));
            assert_owner_redo_retained(harness.state_mut(), &before, frame);
        }
    }
}

fn r12_confirm_dialog(harness: &mut Harness<'_, DesignApp>, click_ok: bool) {
    if click_ok {
        harness.get_by_role_and_label(egui::accesskit::Role::Button, "  OK  ").click();
    } else {
        harness.key_press(egui::Key::Enter);
    }
    harness.run();
    assert!(harness.state().ui.dialog.is_none());
}

#[test]
fn r12_tfo_status_enter_reports_stale_document_without_mutation() {
    let (mut app, frames) = configured_frames();
    owner_engine(&mut app, "selection.set", json!({"ids": [frames[0]]}));
    let opening = app.session.doc().unwrap().doc.clone();
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "object.textFrameOptions", json!({"ids": [frames[1]], "gutter": 37}));
    state_retain_gutter_redo(harness.state_mut(), frames[1]);
    let before = harness.state().session.documents().to_vec();
    assert!(!std::sync::Arc::ptr_eq(&before[0].doc, &opening));
    assert_eq!(before[0].doc.item(frames[0]), opening.item(frames[0]), "the opening frame itself is unchanged");
    harness.state_mut().ui.status = "Ready".into();

    r12_confirm_dialog(&mut harness, false);
    assert_owner_documents_unchanged(harness.state(), &before, before[0].uid);
    assert_eq!(harness.state().ui.status, "text frame options document changed while the dialog was open");
    assert_owner_redo_retained(harness.state_mut(), &before[0], frames[1]);
}

#[test]
fn r12_tfo_status_ok_reports_owner_change_without_mutation() {
    let (app, frames) = owner_colliding_frames(false);
    let mut harness = dialog(open_options(app));
    edit_measure(&mut harness, "Top", "7 pt");
    owner_engine(harness.state_mut(), "file.activate", json!({"index": 1}));
    let before = harness.state().session.documents().to_vec();
    assert_eq!(frames[0], frames[1], "the documents deliberately reuse the same frame ID");
    harness.state_mut().ui.status = "Ready".into();

    r12_confirm_dialog(&mut harness, true);
    assert_owner_documents_unchanged(harness.state(), &before, before[1].uid);
    assert_eq!(harness.state().ui.status, "text frame options document is no longer active");
    assert_owner_redo_retained(harness.state_mut(), &before[1], frames[1]);
    assert_owner_document_unchanged(&harness.state().session.documents()[0], &before[0]);
}

#[test]
fn r12_tfo_status_valid_enter_and_ok_preserve_success_and_undo() {
    for click_ok in [false, true] {
        let (mut app, frames) = configured_frames();
        state_retain_gutter_redo(&mut app, frames[1]);
        let before = app.session.doc().unwrap().clone();
        let mut harness = dialog(open_options(app));
        edit_measure(&mut harness, "Top", "7 pt");
        harness.state_mut().ui.status = "Ready".into();

        r12_confirm_dialog(&mut harness, click_ok);
        assert_eq!(harness.state().ui.status, "Ready", "successful confirmation must not replace the existing status");
        let mut expected = (*before.doc).clone();
        expected.item_mut(frames[1]).unwrap().text_frame_mut().unwrap().options.inset[0] = 7.0;
        assert_roundtrip(harness.state_mut(), &before.doc, &expected, &before.selection, before.history.undo.len());
        let after = harness.state().session.doc().unwrap();
        assert!(after.history.redo.is_empty(), "the valid edit replaces the old redo branch");
        for (actual, prior) in after.history.undo.iter().zip(&before.history.undo) {
            assert_eq!(actual.label, prior.label);
            assert!(std::sync::Arc::ptr_eq(&actual.doc, &prior.doc));
            assert_eq!(*actual.doc, *prior.doc);
            assert_eq!(actual.selection, prior.selection);
        }
        let last = after.history.undo.last().unwrap();
        assert_eq!(*last.doc, *before.doc);
        assert_eq!(last.selection, before.selection);
    }
}

#[test]
fn r12_tfo_status_stale_no_edit_and_cancel_keep_status_and_both_histories() {
    for click_button in [false, true] {
        for cancel in [false, true] {
            let (app, frames) = owner_colliding_frames(false);
            let mut harness = dialog(open_options(app));
            if cancel {
                edit_measure(&mut harness, "Top", "7 pt");
            }
            owner_engine(harness.state_mut(), "file.activate", json!({"index": 1}));
            let before = harness.state().session.documents().to_vec();
            harness.state_mut().ui.status = "Ready".into();

            if cancel {
                if click_button {
                    harness.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click();
                } else {
                    harness.key_press(egui::Key::Escape);
                }
                harness.run();
                assert!(harness.state().ui.dialog.is_none());
            } else {
                r12_confirm_dialog(&mut harness, click_button);
            }
            assert_owner_documents_unchanged(harness.state(), &before, before[1].uid);
            assert_eq!(harness.state().ui.status, "Ready", "no-edit confirmation and cancellation must not report stale-target errors");
            assert_owner_redo_retained(harness.state_mut(), &before[1], frames[1]);
            assert_owner_document_unchanged(&harness.state().session.documents()[0], &before[0]);
        }
    }
}

#[test]
fn r17_tfo_shared_frame_caret_checkbox_enter_preserves_text_and_history() {
    // Exercise the shared frame and native input hook, not the dialog-only helper.
    fn step(harness: &mut Harness<'_, DesignApp>) {
        let mut raw = std::mem::take(harness.input_mut());
        harness.state_mut().raw_input_hook(&mut raw);
        *harness.input_mut() = raw;
        harness.step();
    }

    fn input(harness: &mut Harness<'_, DesignApp>, method: &str, params: Value) {
        owner_ok(harness.state_mut(), method, params);
        for _ in 0..40 {
            step(harness);
            if harness.state().synthetic.is_empty() {
                break;
            }
        }
        assert!(harness.state().synthetic.is_empty());
        // Opening the scrollable modal or changing its controls can animate its size.
        // Settle that layout before reading widget rectangles for the next real click.
        let frames = if matches!(method, "ui.menu.invoke" | "ui.click") && harness.state().ui.dialog.is_some() { 20 } else { 2 };
        for _ in 0..frames {
            step(harness);
        }
    }

    let (mut app, frames) = configured_frames();
    app.integrated_titlebar = true;
    app.ui.rulers = false;
    app.ui.guides = false;
    app.select_tool("type");
    app.run("edit.deselectAll", json!({})).unwrap();
    let ready = std::rc::Rc::new(std::cell::Cell::new(false));
    let ready_ui = ready.clone();
    let context = std::rc::Rc::new(std::cell::RefCell::new(None));
    let context_ui = context.clone();
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if ready_ui.get() {
                let ctx = ui.ctx().clone();
                *context_ui.borrow_mut() = Some(ctx.clone());
                app.logic(&ctx);
                app.ui(ui);
            }
        },
        app,
    );
    // Match the existing full UI example's font-atlas setup before its first real frame.
    harness.input_mut().max_texture_side = Some(8192);
    ready.set(true);
    for _ in 0..6 {
        step(&mut harness);
    }
    let pos = {
        let app = harness.state();
        let item = app.session.doc().unwrap().doc.item(frames[0]).unwrap();
        let point = app.session.layout().xf(SpreadRef::Doc(0)) * item.bounds().center();
        let rect = app.canvas_rect.unwrap();
        let pos = crate::canvas::Xf::new(rect, app.view().unwrap()).to_screen(point);
        assert!(rect.contains(pos));
        pos
    };
    input(&mut harness, "ui.click", json!({"x": pos.x, "y": pos.y}));
    let before = harness.state().session.doc().unwrap().clone();
    let caret = before.selection.text.unwrap();
    assert!(before.selection.items.is_empty());
    assert_eq!(caret.frame, Some(frames[0]));
    assert!(caret.is_caret());
    assert!(before.interaction.is_none());
    harness.state_mut().ui.status = "Ready".into();

    input(&mut harness, "ui.menu.invoke", json!({"command": "object.textFrameOptions"}));
    let checkbox = harness.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Insert Column Rule").rect().center();
    input(&mut harness, "ui.click", json!({"x": checkbox.x, "y": checkbox.y}));
    // Some platforms give keyboard focus only after Tab; never force egui focus memory.
    for _ in 0..32 {
        if harness.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Insert Column Rule").is_focused() {
            break;
        }
        input(&mut harness, "ui.key", json!({"key": "Tab"}));
    }
    assert!(harness.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Insert Column Rule").is_focused());
    assert!(!context.borrow().as_ref().unwrap().text_edit_focused());
    assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields["columnRule"], true);
    assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);

    input(&mut harness, "ui.key", json!({"key": "Enter"}));
    assert!(harness.state().ui.dialog.is_none());
    assert_eq!(harness.state().ui.status, "Ready");
    assert_eq!(harness.state().session.tool_id(), "type");
    assert!(harness.state().session.doc().unwrap().interaction.is_none());
    let mut expected = (*before.doc).clone();
    expected.item_mut(frames[0]).unwrap().text_frame_mut().unwrap().options.column_rule = true;
    assert_roundtrip(harness.state_mut(), &before.doc, &expected, &before.selection, before.history.undo.len());

    // The same raw Enter remains ordinary text input after the modal has closed.
    input(&mut harness, "ui.click", json!({"x": pos.x, "y": pos.y}));
    let caret = harness.state().session.doc().unwrap().selection.text.unwrap();
    let mut text = harness.state().session.doc().unwrap().doc.story(caret.story).unwrap().text.clone();
    text.insert(caret.focus, '\n');
    input(&mut harness, "ui.key", json!({"key": "Enter"}));
    assert_eq!(harness.state().session.doc().unwrap().doc.story(caret.story).unwrap().text, text);
}

#[test]
fn r22_tfo_shared_frame_cancel_enter_and_confirmation_controls_preserve_exact_state() {
    fn step(harness: &mut Harness<'_, DesignApp>) {
        let mut raw = std::mem::take(harness.input_mut());
        harness.state_mut().raw_input_hook(&mut raw);
        *harness.input_mut() = raw;
        harness.step();
    }

    fn input(harness: &mut Harness<'_, DesignApp>, method: &str, params: Value) {
        owner_ok(harness.state_mut(), method, params);
        for _ in 0..40 {
            step(harness);
            if harness.state().synthetic.is_empty() {
                break;
            }
        }
        assert!(harness.state().synthetic.is_empty());
        // Opening the scrollable modal or changing its controls can animate its size.
        // Settle that layout before reading widget rectangles for the next real click.
        let frames = if matches!(method, "ui.menu.invoke" | "ui.click") && harness.state().ui.dialog.is_some() { 20 } else { 2 };
        for _ in 0..frames {
            step(harness);
        }
    }

    for action in ["cancel_enter", "escape", "mouse_cancel", "default_enter", "checkbox_enter"] {
        let (mut app, frames) = configured_frames();
        // Both branches must be populated, so cancellation cannot pass by clearing empty history.
        state_retain_gutter_redo(&mut app, frames[0]);
        app.integrated_titlebar = true;
        app.ui.rulers = false;
        app.ui.guides = false;
        app.select_tool("type");
        app.run("edit.deselectAll", json!({})).unwrap();
        let ready = std::rc::Rc::new(std::cell::Cell::new(false));
        let ready_ui = ready.clone();
        let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if ready_ui.get() {
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx);
                    app.ui(ui);
                }
            },
            app,
        );
        harness.input_mut().max_texture_side = Some(8192);
        ready.set(true);
        for _ in 0..6 {
            step(&mut harness);
        }
        let pos = {
            let app = harness.state();
            let item = app.session.doc().unwrap().doc.item(frames[0]).unwrap();
            let point = app.session.layout().xf(SpreadRef::Doc(0)) * item.bounds().center();
            let rect = app.canvas_rect.unwrap();
            let pos = crate::canvas::Xf::new(rect, app.view().unwrap()).to_screen(point);
            assert!(rect.contains(pos));
            pos
        };
        input(&mut harness, "ui.click", json!({"x": pos.x, "y": pos.y}));
        let before = harness.state().session.doc().unwrap().clone();
        assert!(before.selection.items.is_empty());
        let caret = before.selection.text.unwrap();
        assert!(caret.is_caret());
        assert_eq!(caret.frame, Some(frames[0]));
        assert!(!before.history.undo.is_empty());
        assert!(!before.history.redo.is_empty());
        assert!(before.interaction.is_none());
        harness.state_mut().ui.status = "Ready".into();

        input(&mut harness, "ui.menu.invoke", json!({"command": "object.textFrameOptions"}));
        let gutter = harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Gutter").rect().center();
        input(&mut harness, "ui.click", json!({"x": gutter.x, "y": gutter.y}));
        input(&mut harness, "ui.key", json!({"key": "A", "cmd": true}));
        input(&mut harness, "ui.text", json!({"text": "29 pt"}));
        assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields["gutter"], "29 pt");
        if action == "checkbox_enter" {
            let checkbox = harness.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Insert Column Rule").rect().center();
            input(&mut harness, "ui.click", json!({"x": checkbox.x, "y": checkbox.y}));
            assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields["columnRule"], true);
        }
        if action != "default_enter" {
            let (role, label) = if action == "checkbox_enter" {
                (egui::accesskit::Role::CheckBox, "Insert Column Rule")
            } else {
                (egui::accesskit::Role::Button, "Cancel")
            };
            let mut tabs = 0;
            while !harness.get_by_role_and_label(role, label).is_focused() && tabs < 32 {
                input(&mut harness, "ui.key", json!({"key": "Tab"}));
                tabs += 1;
            }
            assert!(harness.get_by_role_and_label(role, label).is_focused(), "{action}");
            if action != "checkbox_enter" {
                assert!(tabs > 0, "Cancel must acquire focus through actual Tab traversal");
            }
        } else {
            assert!(harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Gutter").is_focused());
        }
        assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);

        if action == "mouse_cancel" {
            let cancel = harness.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").rect().center();
            input(&mut harness, "ui.click", json!({"x": cancel.x, "y": cancel.y}));
        } else {
            input(&mut harness, "ui.key", json!({"key": if action == "escape" { "Escape" } else { "Enter" }}));
        }
        assert!(harness.state().ui.dialog.is_none(), "{action}");
        assert_eq!(harness.state().ui.status, "Ready", "{action}");
        assert_eq!(harness.state().session.tool_id(), "type", "modal keys must not reach the canvas");
        assert!(harness.state().session.doc().unwrap().interaction.is_none());
        if matches!(action, "default_enter" | "checkbox_enter") {
            let mut expected = (*before.doc).clone();
            let options = &mut expected.item_mut(frames[0]).unwrap().text_frame_mut().unwrap().options;
            options.gutter = 29.0;
            if action == "checkbox_enter" {
                options.column_rule = true;
            }
            assert_roundtrip(harness.state_mut(), &before.doc, &expected, &before.selection, before.history.undo.len());
            assert!(harness.state().session.doc().unwrap().history.redo.is_empty());
        } else {
            // Compares full Documents and every history entry, including labels and saved selections.
            assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);
        }
    }
}

fn r28_tfo_dropdown_enter(rule_color: bool, cancel: bool, choose_option: bool) {
    // Keep the actual shared frame and native input hook used by r17/r22.
    fn step(harness: &mut Harness<'_, DesignApp>) {
        let mut raw = std::mem::take(harness.input_mut());
        harness.state_mut().raw_input_hook(&mut raw);
        *harness.input_mut() = raw;
        harness.step();
    }

    fn input(harness: &mut Harness<'_, DesignApp>, method: &str, params: Value) {
        owner_ok(harness.state_mut(), method, params);
        for _ in 0..40 {
            step(harness);
            if harness.state().synthetic.is_empty() {
                break;
            }
        }
        assert!(harness.state().synthetic.is_empty());
        // Opening the scrollable modal or changing its controls can animate its size.
        // Settle that layout before reading widget rectangles for the next real click.
        let frames = if matches!(method, "ui.menu.invoke" | "ui.click") && harness.state().ui.dialog.is_some() { 20 } else { 2 };
        for _ in 0..frames {
            step(harness);
        }
    }

    fn tab_to(harness: &mut Harness<'_, DesignApp>, role: egui::accesskit::Role, label: &'static str) {
        let mut tabs = 0;
        for _ in 0..64 {
            if harness.get_by_role_and_label(role, label).is_focused() {
                break;
            }
            input(harness, "ui.key", json!({"key": "Tab"}));
            tabs += 1;
        }
        assert!(harness.get_by_role_and_label(role, label).is_focused(), "Tab must actually focus {label}");
        eprintln!("r28_tfo_tab_focus role={role:?} label={label:?} tabs={tabs}");
    }

    let (field, current, choice) = if rule_color { ("columnRuleColor", "[Black]", "[Paper]") } else { ("verticalJustification", "center", "bottom") };
    let (mut app, frames) = configured_frames();
    state_retain_gutter_redo(&mut app, frames[0]);
    app.integrated_titlebar = true;
    app.ui.rulers = false;
    app.ui.guides = false;
    app.select_tool("type");
    app.run("edit.deselectAll", json!({})).unwrap();
    let ready = std::rc::Rc::new(std::cell::Cell::new(false));
    let ready_ui = ready.clone();
    let context = std::rc::Rc::new(std::cell::RefCell::new(None));
    let context_ui = context.clone();
    let mut harness = Harness::builder().with_size(egui::vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, app: &mut DesignApp| {
            if ready_ui.get() {
                let ctx = ui.ctx().clone();
                *context_ui.borrow_mut() = Some(ctx.clone());
                app.logic(&ctx);
                app.ui(ui);
            }
        },
        app,
    );
    harness.input_mut().max_texture_side = Some(8192);
    ready.set(true);
    for _ in 0..6 {
        step(&mut harness);
    }
    let pos = {
        let app = harness.state();
        let item = app.session.doc().unwrap().doc.item(frames[0]).unwrap();
        let point = app.session.layout().xf(SpreadRef::Doc(0)) * item.bounds().center();
        let rect = app.canvas_rect.unwrap();
        let pos = crate::canvas::Xf::new(rect, app.view().unwrap()).to_screen(point);
        assert!(rect.contains(pos));
        pos
    };
    input(&mut harness, "ui.click", json!({"x": pos.x, "y": pos.y}));
    let before = harness.state().session.doc().unwrap().clone();
    assert!(before.selection.items.is_empty());
    let caret = before.selection.text.unwrap();
    assert!(caret.is_caret());
    assert_eq!(caret.frame, Some(frames[0]));
    assert!(!before.history.undo.is_empty());
    assert!(!before.history.redo.is_empty());
    assert!(before.interaction.is_none());
    harness.state_mut().ui.status = "Ready".into();

    input(&mut harness, "ui.menu.invoke", json!({"command": "object.textFrameOptions"}));
    let inset = harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Top").rect().center();
    input(&mut harness, "ui.click", json!({"x": inset.x, "y": inset.y}));
    input(&mut harness, "ui.key", json!({"key": "A", "cmd": true}));
    input(&mut harness, "ui.text", json!({"text": "7 pt"}));
    assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields["insetTop"], "7 pt");
    if rule_color {
        let checkbox = harness.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Insert Column Rule").rect().center();
        input(&mut harness, "ui.click", json!({"x": checkbox.x, "y": checkbox.y}));
    }
    assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields["columnRule"], rule_color);
    // These ComboBoxes expose their selected text as an AccessKit value, not a label.
    let combo = harness.get(egui_kittest::kittest::By::new().role(egui::accesskit::Role::ComboBox).value(current)).rect().center();
    input(&mut harness, "ui.click", json!({"x": combo.x, "y": combo.y}));
    assert!(egui::Popup::is_any_open(context.borrow().as_ref().unwrap()));
    if choose_option {
        // Pointer-hover is not keyboard focus. Reach the actual popup choice with
        // native keys, never focus/popup memory or an accessibility action.
        tab_to(&mut harness, egui::accesskit::Role::Button, choice);
        assert!(egui::Popup::is_any_open(context.borrow().as_ref().unwrap()));
        assert!(!context.borrow().as_ref().unwrap().text_edit_focused());
        assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields[field], current);
        assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);

        input(&mut harness, "ui.key", json!({"key": "Enter"}));
        // BEFORE: early default-action consumption closes the dialog and applies the
        // pending inset with the old dropdown value. This is the regression oracle.
        assert!(harness.state().ui.dialog.is_some(), "{field}: choosing an option must leave TFO open");
        let pending = harness.state().ui.dialog.as_ref().unwrap();
        assert_eq!(pending.fields[field], choice);
        assert_eq!(pending.fields["insetTop"], "7 pt");
        assert_eq!(pending.fields["columnRule"], rule_color);
        assert!(!egui::Popup::is_any_open(context.borrow().as_ref().unwrap()));
        assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);
        assert_eq!(harness.state().ui.status, "Ready");
        assert_eq!(harness.state().session.tool_id(), "type");
        assert!(harness.state().session.doc().unwrap().interaction.is_none());
    }

    let (role, label) = if cancel { (egui::accesskit::Role::Button, "Cancel") } else { (egui::accesskit::Role::CheckBox, "Insert Column Rule") };
    tab_to(&mut harness, role, label);
    if !choose_option {
        // An open popup alone must not steal Enter from a focused dialog control.
        assert!(egui::Popup::is_any_open(context.borrow().as_ref().unwrap()));
        assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields[field], current);
    }
    assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);
    input(&mut harness, "ui.key", json!({"key": "Enter"}));
    assert!(harness.state().ui.dialog.is_none());
    assert_eq!(harness.state().ui.status, "Ready");
    assert_eq!(harness.state().session.tool_id(), "type");
    assert!(harness.state().session.doc().unwrap().interaction.is_none());
    if cancel {
        assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &before);
    } else {
        let mut expected = (*before.doc).clone();
        let options = &mut expected.item_mut(frames[0]).unwrap().text_frame_mut().unwrap().options;
        options.inset[0] = 7.0;
        options.column_rule = rule_color;
        if choose_option {
            if rule_color {
                options.column_rule_color = choice.into();
            } else {
                options.vertical_justification = designcraft_doc::VerticalJustification::Bottom;
            }
        }
        assert_roundtrip(harness.state_mut(), &before.doc, &expected, &before.selection, before.history.undo.len());
        assert!(harness.state().session.doc().unwrap().history.redo.is_empty());
    }
    // Reopening hydrates from the bound frame's committed (or cancelled) state.
    let after = harness.state().session.doc().unwrap().clone();
    input(&mut harness, "ui.menu.invoke", json!({"command": "object.textFrameOptions"}));
    assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields[field], if choose_option && !cancel { choice } else { current });
    assert_eq!(harness.state().ui.dialog.as_ref().unwrap().fields["columnRule"], !cancel && rule_color);
    assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &after);
    input(&mut harness, "ui.key", json!({"key": "Escape"}));
    assert!(harness.state().ui.dialog.is_none());
    assert_owner_document_unchanged(harness.state().session.doc().unwrap(), &after);
}

#[test]
fn r28_tfo_vertical_dropdown_enter_preserves_pending_dialog_and_history() {
    for cancel in [false, true] {
        r28_tfo_dropdown_enter(false, cancel, true);
    }
}

#[test]
fn r28_tfo_rule_color_dropdown_enter_preserves_pending_dialog_and_history() {
    for cancel in [false, true] {
        r28_tfo_dropdown_enter(true, cancel, true);
    }
}

#[test]
fn r28_tfo_open_dropdown_keeps_checkbox_and_cancel_enter_behavior() {
    for rule_color in [false, true] {
        for cancel in [false, true] {
            r28_tfo_dropdown_enter(rule_color, cancel, false);
        }
    }
}
