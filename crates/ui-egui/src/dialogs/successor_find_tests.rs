//! Successors of the Find/Change scope tests of `dialogs/tests.rs`, on the framework's Find/Change form. The old tests drove
//! the native window through a plain egui context; these keep their bodies and their checks (the retained search bounds, the
//! status line, the document, selection and undo history) and drive the same steps through the real dialog host with an
//! installed framework: the helpers below click the form's controls by name and send keys through the app's input hook.
//! `Rig` dereferences to the app, so a test reads `app.session` and `app.ui.dialog` as the old ones did.

use super::table_layout_kit as kit;
use super::*;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use serde_json::{Value, json};
use std::ops::{Deref, DerefMut};

/// The app in a harness showing its dialogs through the framework host.
struct Rig(egui_kittest::Harness<'static, DesignApp>);

impl Deref for Rig {
    type Target = DesignApp;
    fn deref(&self) -> &DesignApp {
        self.0.state()
    }
}

impl DerefMut for Rig {
    fn deref_mut(&mut self) -> &mut DesignApp {
        self.0.state_mut()
    }
}

/// The roles a button, a mode segment, a check box or an entry of an open list can report.
const CHOICE: &[Role] = &[
    Role::Button,
    Role::RadioButton,
    Role::Tab,
    Role::CheckBox,
    Role::Switch,
    Role::ComboBox,
    Role::ListBoxOption,
    Role::MenuItem,
    Role::MenuItemRadio,
    Role::ListItem,
    Role::Row,
];

/// The entries of the Search picker.
const SCOPES: [&str; 3] = ["Document", "Story", "Selection"];

/// A frame, with `events` sent through the app's input hook first (a key press is followed by its release).
fn dialog_frame(app: &mut Rig, _ctx: &egui::Context, _screen: egui::Rect, events: Vec<egui::Event>) {
    if events.is_empty() {
        kit::steps(&mut app.0, 1);
    }
    for event in events {
        let release = match &event {
            egui::Event::Key { key, physical_key, pressed: true, repeat: false, modifiers } => {
                Some(egui::Event::Key { key: *key, physical_key: *physical_key, pressed: false, repeat: false, modifiers: *modifiers })
            }
            _ => None,
        };
        kit::event(&mut app.0, event);
        if let Some(release) = release {
            kit::event(&mut app.0, release);
        }
    }
    kit::steps(&mut app.0, 3);
}

/// Clicks the control named `label`; the Search entries are picked from the picker, which is opened first when its list is
/// not showing.
fn scope_click(app: &mut Rig, _ctx: &egui::Context, _screen: egui::Rect, label: &str) {
    kit::steps(&mut app.0, 1);
    let h = &mut app.0;
    let listed = |h: &egui_kittest::Harness<'_, DesignApp>| h.query_all_by_label(label).any(|node| node.accesskit_node().role() == Role::ListBoxOption);
    if SCOPES.contains(&label) {
        if !listed(h) {
            kit::click(h, &[Role::ComboBox], "Search:", 0);
        }
        let at = h
            .query_all_by_label(label)
            .find(|node| node.accesskit_node().role() == Role::ListBoxOption)
            .unwrap_or_else(|| panic!("no entry named {label:?} in the Search list"))
            .rect()
            .center();
        kit::click_at(h, at);
    } else {
        kit::click(h, CHOICE, label, 0);
    }
}

/// Runs a control-channel request and returns its result.
fn scope_control(app: &mut Rig, ctx: &egui::Context, method: &str, params: Value) -> Value {
    let (request, _reply) = crate::control::ControlRequest::new(method, params);
    let crate::control::Outcome::Done(value) = crate::control::handle(app, ctx, &request) else { panic!("ordinary control response") };
    assert_eq!(value["ok"], true, "{value}");
    value["result"].clone()
}

fn scope_dialog() -> (Rig, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
    scope_dialog_with_selection("red red | red", 7)
}

fn scope_dialog_with_selection(text: &str, end: usize) -> (Rig, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
    scope_dialog_with_tool(text, end, None)
}

/// A text frame holding `text` with the first `end` bytes selected, `tool` chosen first when given, and Find/Change open
/// with "red" to find and the Selection scope picked.
fn scope_dialog_with_tool(text: &str, end: usize, tool: Option<&str>) -> (Rig, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
    let mut app = kit::app(1, false);
    let frame = app.run("frame.create", json!({"rect": [36, 36, 100, 100], "content": "text", "text": text})).unwrap();
    let sid = designcraft_engine::doc::StoryId(frame["story"].as_u64().unwrap());
    if let Some(tool) = tool {
        let (request, _reply) = crate::control::ControlRequest::new("ui.tool.select", json!({"tool": tool}));
        let crate::control::Outcome::Done(value) = crate::control::handle(&mut app, &egui::Context::default(), &request) else {
            panic!("ordinary control response")
        };
        assert_eq!(value["ok"], true, "{value}");
    }
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": end})).unwrap();
    app.run("app.findChange", json!({})).unwrap();
    let mut app = Rig(kit::harness(app));
    let ctx = app.0.ctx.clone();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 900.0));
    for (field, value) in [("find", "red"), ("change", "unused")] {
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
    }
    kit::steps(&mut app.0, 3);
    scope_click(&mut app, &ctx, screen, "Document");
    scope_click(&mut app, &ctx, screen, "Selection");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("scope"), "selection");
    (app, ctx, screen, sid)
}

fn scope_selected(app: &DesignApp, sid: designcraft_engine::doc::StoryId, a: usize, b: usize) {
    let t = app.session.doc().unwrap().selection.text.unwrap();
    assert_eq!((t.story, t.range(), t.cell), (sid, a..b, None));
}

fn scope_state_unchanged(app: &DesignApp, before: &designcraft_engine::DocState) {
    let after = app.session.doc().unwrap();
    assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
    assert_eq!(after.uid, before.uid);
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.selection, before.selection);
    assert_eq!(after.history.limit, before.history.limit);
    for (actual, expected) in [(&after.history.undo, &before.history.undo), (&after.history.redo, &before.history.redo)] {
        assert_eq!(actual.len(), expected.len());
        for (a, b) in actual.iter().zip(expected) {
            assert_eq!(a.label, b.label);
            assert_eq!(a.selection, b.selection);
            assert!(std::sync::Arc::ptr_eq(&a.doc, &b.doc));
        }
    }
}

fn scope_label(scope: &str) -> &'static str {
    match scope {
        "story" => "Story",
        "selection" => "Selection",
        _ => "Document",
    }
}

fn place_filter(request: &craft_dialogs::Request) -> Option<&str> {
    let craft_dialogs::Request::File(dialog) = request else { return None };
    dialog.filters.first().map(|filter| filter.name.as_str())
}

fn scope_confirm_case(action: &str) {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Count");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    match action {
        "OK" => scope_click(&mut app, &ctx, screen, "OK"),
        "Enter" => {
            dialog_frame(
                &mut app,
                &ctx,
                screen,
                vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
        }
        "control" => {
            assert_eq!(scope_control(&mut app, &ctx, "ui.dialog.confirm", json!({}))["count"], 2);
        }
        _ => panic!("known confirmation route"),
    }
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    // The closed dialog cannot leave its old seven-byte range for a later direct query.
    let matches = app.run("find.find", json!({"find": "blue", "scope": "selection"})).unwrap();
    assert_eq!(matches.as_array().unwrap().len(), 0);
}

/// Successor of `find_scope_dialog_control_effective_noops_retain_bounds`.
#[test]
fn find_scope_form_control_effective_noops_retain_bounds() {
    for (field, value) in [("objectMode", json!(false)), ("story", Value::Null)] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        assert!(!app.ui.dialog.as_ref().unwrap().fields.contains_key(field));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields[field], value);
        scope_state_unchanged(&app, &before);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    }
}

/// Successor of `find_scope_dialog_explicit_story_ignored_scope_preserves_grep_cursor`.
#[test]
fn find_scope_form_explicit_story_ignored_scope_preserves_grep_cursor() {
    for physical in [false, true] {
        for (text, pattern, hits, changed) in [
            ("é", "é|(?:)", vec![(0, 2)], "X"),
            ("é🙂", "é|(?:)", vec![(0, 2), (6, 6)], "X🙂X"),
            ("é🙂", "(?:)", vec![(0, 0), (2, 2), (6, 6)], "XéX🙂X"),
        ] {
            let (mut app, ctx, screen, sid) = scope_dialog_with_selection(text, 0);
            for (field, value) in [("find", json!(pattern)), ("change", json!("X")), ("story", json!(sid.0))] {
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
            }
            scope_click(&mut app, &ctx, screen, "GREP");
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} match(es)", hits.len()));
            for &(a, b) in &hits {
                scope_click(&mut app, &ctx, screen, "Find Next");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "Found");
                scope_selected(&app, sid, a, b);
                let before = app.session.doc().unwrap().clone();
                let cur = app.ui.dialog.as_ref().unwrap().s("scope");
                let next = if cur == "selection" { "document" } else { "selection" };
                if physical {
                    scope_click(&mut app, &ctx, screen, scope_label(&cur));
                    scope_click(&mut app, &ctx, screen, scope_label(next));
                } else {
                    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": next}));
                }
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("scope"), next);
                assert_eq!(app.ui.dialog.as_ref().unwrap().fields["story"], json!(sid.0));
                scope_state_unchanged(&app, &before);
                scope_click(&mut app, &ctx, screen, "Count");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} match(es)", hits.len()));
            }
            scope_click(&mut app, &ctx, screen, "Find Next");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
            // An ignored scope change must not revive an exhausted endpoint either.
            let before = app.session.doc().unwrap().clone();
            let cur = app.ui.dialog.as_ref().unwrap().s("scope");
            let next = if cur == "selection" { "document" } else { "selection" };
            if physical {
                scope_click(&mut app, &ctx, screen, scope_label(&cur));
                scope_click(&mut app, &ctx, screen, scope_label(next));
            } else {
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": next}));
            }
            scope_click(&mut app, &ctx, screen, "Find Next");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
            scope_state_unchanged(&app, &before);
            scope_click(&mut app, &ctx, screen, "Change All");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} replacement(s) made", hits.len()));
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, changed);
        }
    }
}

/// Successor of `find_scope_dialog_explicit_story_transitions_reset_cursor`.
#[test]
fn find_scope_form_explicit_story_transitions_reset_cursor() {
    for remove_override in [false, true] {
        let (mut app, ctx, screen, sid) = scope_dialog_with_selection("é🙂", 0);
        let other = app.run("frame.create", json!({"rect": [180, 36, 100, 100], "content": "text", "text": "other"})).unwrap();
        app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        for (field, value) in [("find", json!("(?:)")), ("story", json!(sid.0))] {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
        }
        scope_click(&mut app, &ctx, screen, "GREP");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 0);
        let before = app.session.doc().unwrap().clone();
        let value = if remove_override { Value::Null } else { other["story"].clone() };
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": value}));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": sid.0}));
        scope_state_unchanged(&app, &before);
        // Away and back still releases continuation even if no Find command intervenes.
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 0);
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 2, 2);
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "é🙂");
        let other_sid = designcraft_engine::doc::StoryId(other["story"].as_u64().unwrap());
        assert_eq!(app.session.doc().unwrap().doc.story(other_sid).unwrap().text, "other");
    }
}

/// Successor of `find_scope_dialog_control_mode_changes_release_bounds`.
#[test]
fn find_scope_form_control_mode_changes_release_bounds() {
    for object_mode in [false, true] {
        let (mut app, ctx, _, sid) = scope_dialog();
        if object_mode {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "objectMode", "value": true}));
        }
        // Direct Session consumers share the retained domain even while Object is shown.
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count.as_array().unwrap().len(), 2);
        scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.next", "params": p}));
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "objectMode", "value": !object_mode}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().b("objectMode"), !object_mode);
        scope_state_unchanged(&app, &before);
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
        assert_eq!(changed, json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
    }
}

/// Successor of `find_scope_dialog_tool_replacement_releases_bounds`.
#[test]
fn find_scope_form_tool_replacement_releases_bounds() {
    for (tool, replacement) in [("page", "cmd:layout.pageSize"), ("rectangleFrame", "frameSize")] {
        let (mut app, ctx, screen, sid) = scope_dialog_with_tool("red red | red", 7, Some(tool));
        assert_eq!(app.session.tool_id(), tool);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        let active = (app.session.active_index(), app.session.documents().len());
        let off = app.session.layout().xf(designcraft_engine::doc::SpreadRef::Doc(0)).translation();
        let x = off.x + before.doc.spreads[0].pages[0].x + 200.0;
        let y = off.y + 200.0;
        // The Page tool opens Page Size on a double-click (a click selects the page).
        let events = if tool == "page" {
            json!([{"kind": "doubleclick", "x": x, "y": y}])
        } else {
            json!([{"kind": "down", "x": x, "y": y}, {"kind": "up", "x": x, "y": y}])
        };
        // Real tool -> Action::Dialog -> after_engine; no injected UI request.
        scope_control(&mut app, &ctx, "ui.pointer", json!({"events": events}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, replacement);
        assert!(app.session.ui_requests.is_empty());
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        scope_click(&mut app, &ctx, screen, "Cancel");
        assert!(app.ui.dialog.is_none());
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        scope_state_unchanged(&app, &before);
        let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
        assert_eq!(changed, json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    }
}

/// Successor of `find_scope_dialog_tool_without_replacement_retains_bounds`.
#[test]
fn find_scope_form_tool_without_replacement_retains_bounds() {
    for tool in ["page", "rectangleFrame"] {
        let (mut app, ctx, screen, sid) = scope_dialog_with_tool("red red | red", 7, Some(tool));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        let fields = app.ui.dialog.as_ref().unwrap().fields.clone();
        scope_control(&mut app, &ctx, "ui.pointer", json!({"events": [{"kind": "move", "x": 200, "y": 200}]}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findChange");
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields, fields);
        scope_state_unchanged(&app, &before);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    }
}

/// Successor of `find_scope_dialog_native_pdf_replacement_releases_bounds`.
#[test]
fn find_scope_form_native_pdf_replacement_releases_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("three.pdf");
    std::fs::write(&path, crate::browser_import_tests::pdf()).unwrap();
    for dismissal in ["Cancel", "Escape", "control"] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        let picked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_pick = picked.clone();
        let seen_read = read.clone();
        let pick_path = path.to_string_lossy().to_string();
        let read_path = pick_path.clone();
        app.answer_dialogs(move |request| {
            assert_eq!(place_filter(request), Some("Graphics and text"));
            seen_pick.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            craft_dialogs::Answer::Paths(vec![pick_path.clone().into()])
        });
        app.services.read = Some(Box::new(move |path| {
            assert_eq!(path, read_path);
            seen_read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::fs::read(path).map_err(|e| e.to_string())
        }));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        let active = (app.session.active_index(), app.session.documents().len());
        // Exercise engine.execute -> UiRequest::Pick -> native host PDF replacement.
        scope_control(&mut app, &ctx, "engine.execute", json!({"command": "file.place", "params": {}}));
        app.settle_dialogs();
        assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 1);
        let dialog = app.ui.dialog.as_ref().unwrap();
        assert_eq!(dialog.id, "pdfImport");
        assert_eq!(dialog.s("path"), path.to_string_lossy());
        assert_eq!(dialog.fields["pages"], 3);
        assert_eq!(dialog.s("page"), "1");
        assert!(!dialog.b("async"));
        assert_eq!(dialog.fields["target"], json!(before.uid));
        assert!(app.session.ui_requests.is_empty());
        assert!(app.pending_pdf.is_none());
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        match dismissal {
            "Cancel" => scope_click(&mut app, &ctx, screen, "Cancel"),
            "Escape" => {
                dialog_frame(
                    &mut app,
                    &ctx,
                    screen,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            "control" => {
                scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
            }
            _ => panic!("known dismissal"),
        }
        assert!(app.ui.dialog.is_none());
        assert!(app.pending_pdf.is_none());
        assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
        assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        scope_state_unchanged(&app, &before);
        let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
        assert_eq!(changed, json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    }
}

/// Successor of `find_scope_dialog_native_picker_cancel_retains_bounds`.
#[test]
fn find_scope_form_native_picker_cancel_retains_bounds() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    let picked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = picked.clone();
    app.answer_dialogs(move |request| {
        assert_eq!(place_filter(request), Some("Graphics and text"));
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        craft_dialogs::Answer::Cancelled
    });
    let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen_read = read.clone();
    app.services.read = Some(Box::new(move |_| {
        seen_read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err("canceled picker must not read".into())
    }));
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    let before = app.session.doc().unwrap().clone();
    let active = (app.session.active_index(), app.session.documents().len());
    let fields = app.ui.dialog.as_ref().unwrap().fields.clone();
    scope_control(&mut app, &ctx, "engine.execute", json!({"command": "file.place", "params": {}}));
    app.settle_dialogs();
    assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findChange");
    assert_eq!(app.ui.dialog.as_ref().unwrap().fields, fields);
    assert_eq!((app.session.active_index(), app.session.documents().len()), active);
    scope_state_unchanged(&app, &before);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
}

/// Successor of `find_scope_dialog_exact_baseline_buttons`.
#[test]
fn find_scope_form_exact_baseline_buttons() {
    let (mut app, ctx, screen, sid) = scope_dialog_with_selection("red red", 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red");
}

/// Successor of `find_scope_dialog_grep_tab_alone_restarts_original_selection`.
#[test]
fn find_scope_form_grep_tab_alone_restarts_original_selection() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "GREP");
    assert!(app.ui.dialog.as_ref().unwrap().b("grep"));
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("find"), "red");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    scope_click(&mut app, &ctx, screen, "Text");
    assert!(!app.ui.dialog.as_ref().unwrap().b("grep"));
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
}

/// Successor of `find_scope_dialog_buttons_retain_nonempty_hits_and_change_all`.
#[test]
fn find_scope_form_buttons_retain_nonempty_hits_and_change_all() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    let history = app.session.doc().unwrap().history.undo.len();
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    scope_click(&mut app, &ctx, screen, "Find Next");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), history + 1);
    scope_click(&mut app, &ctx, screen, "Cancel");
    assert!(app.ui.dialog.is_none());
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
    app.run("edit.redo", json!({})).unwrap();
    let native = app.run("file.serialize", json!({})).unwrap();
    app.run("file.openBytes", json!({"name": "scope.designcraft", "base64": native["base64"]})).unwrap();
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
}

/// Successor of `find_scope_dialog_ok_consumes_retained_bounds`.
#[test]
fn find_scope_form_ok_consumes_retained_bounds() {
    scope_confirm_case("OK");
}

/// Successor of `find_scope_dialog_enter_consumes_retained_bounds`.
#[test]
fn find_scope_form_enter_consumes_retained_bounds() {
    scope_confirm_case("Enter");
}

/// Successor of `find_scope_dialog_control_confirm_consumes_retained_bounds`.
#[test]
fn find_scope_form_control_confirm_consumes_retained_bounds() {
    scope_confirm_case("control");
}

/// Successor of `find_scope_dialog_query_reselection_cancel_and_reopen`.
#[test]
fn find_scope_form_query_reselection_cancel_and_reopen() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "grep", "value": true}));
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "find", "value": "r.d"}));
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
    assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
    app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 7})).unwrap();
    app.run("find.next", json!({"find": "red", "scope": "selection"})).unwrap();
    scope_control(&mut app, &ctx, "ui.dialog.open", json!({"id": "findChange", "fields": {"find": "red", "scope": "selection"}}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
    app.run("text.select", json!({"story": sid.0, "anchor": 4, "focus": 7})).unwrap();
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 4, 7);
    app.run("app.findChange", json!({})).unwrap();
    for (field, value) in [("find", "red"), ("scope", "selection")] {
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
    }
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
}

/// Successor of `find_scope_dialog_word_reselection_limits_count_and_change_buttons`.
#[test]
fn find_scope_form_word_reselection_limits_count_and_change_buttons() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    scope_click(&mut app, &ctx, screen, "Find Next");
    scope_selected(&app, sid, 0, 3);
    let selected = app.session.doc().unwrap().selection.clone();
    let frame = selected.text.unwrap().frame.unwrap();
    let point = {
        let st = app.session.doc().unwrap();
        let cs = app.session.cache.get(&st.doc, sid, None);
        let (fi, x, baseline, ascent, _) = designcraft_compose::caret(&cs, 1).unwrap();
        let inner = designcraft_geom::Point::new(x, baseline - ascent * 0.5);
        assert_eq!(cs.frames[fi].frame, frame);
        assert_eq!(designcraft_compose::hit(&cs, fi, inner), Some(1));
        let loc = st.doc.find(frame).unwrap();
        st.doc.parent_xf(&loc) * st.doc.text_xf(st.doc.item(frame).unwrap()) * inner
    };
    let before = app.session.doc().unwrap().doc.clone();
    let revision = app.session.doc().unwrap().revision;
    app.run("text.selectWord", json!({"frame": frame.0, "point": [point.x, point.y]})).unwrap();
    assert_eq!(app.session.doc().unwrap().selection, selected);
    assert!(app.session.doc().unwrap().revision > revision, "a new revision");
    assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
}

/// Successor of `find_scope_dialog_failed_native_drop_alert_releases_bounds`.
#[test]
fn find_scope_form_failed_native_drop_alert_releases_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.designcraft");
    std::fs::write(&path, b"not a native document").unwrap();
    for dismissal in ["OK", "Escape", "control"] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().doc.clone();
        let selected = app.session.doc().unwrap().selection.clone();
        let revision = app.session.doc().unwrap().revision;
        let history = app.session.doc().unwrap().history.undo.len();
        let uid = app.session.doc().unwrap().uid;
        let error = app.open_dropped(&path.to_string_lossy()).unwrap_err();
        let alert = app.ui.dialog.as_ref().unwrap();
        assert_eq!(alert.id, "alert");
        assert_eq!(alert.s("title"), "Can't Open the File");
        assert_eq!(alert.s("message"), error);
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
        assert_eq!(app.session.doc().unwrap().selection, selected);
        assert_eq!(app.session.doc().unwrap().revision, revision);
        assert_eq!(app.session.doc().unwrap().uid, uid);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
        match dismissal {
            "OK" => scope_click(&mut app, &ctx, screen, "OK"),
            "Escape" => {
                dialog_frame(
                    &mut app,
                    &ctx,
                    screen,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            "control" => {
                scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
            }
            _ => panic!("known dismissal"),
        }
        assert!(app.ui.dialog.is_none());
        let p = json!({"find": "red", "change": "blue", "scope": "selection"});
        assert_eq!(app.run("find.find", p.clone()).unwrap(), json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
        assert_eq!(app.run("find.change", p).unwrap(), json!({"count": 1}));
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history + 1);
    }
}

/// Successor of `find_scope_dialog_child_errors_and_explicit_parent`.
#[test]
fn find_scope_form_child_errors_and_explicit_parent() {
    let (mut app, ctx, screen, sid) = scope_dialog();
    app.run("text.select", json!({"story": sid.0, "anchor": 13, "focus": 13})).unwrap();
    app.run("table.insert", json!({"rows": 1, "cols": 1})).unwrap();
    app.run("text.insert", json!({"text": "red child", "raw": true})).unwrap();
    let child = app.session.doc().unwrap().selection.text.unwrap();
    let before = app.session.doc().unwrap().doc.clone();
    let selection = app.session.doc().unwrap().selection.clone();
    let history = app.session.doc().unwrap().history.undo.len();
    for label in ["Count", "Find Next", "Change All"] {
        scope_click(&mut app, &ctx, screen, label);
        assert!(app.ui.dialog.as_ref().unwrap().s("status").contains("not supported"));
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
        assert_eq!(app.session.doc().unwrap().selection, selection);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
    }
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": sid.0}));
    scope_click(&mut app, &ctx, screen, "Count");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "3 match(es)");
    scope_click(&mut app, &ctx, screen, "Change All");
    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "3 replacement(s) made");
    assert_eq!(app.session.doc().unwrap().doc.text_story(sid, child.cell).unwrap().text, "red child");
    assert!(app.session.doc().unwrap().doc.story(sid).unwrap().text.starts_with("blue blue | blue"));
}

/// Successor of `find_scope_dialog_error_confirmation_and_escape_release_context`.
#[test]
fn find_scope_form_error_confirmation_and_escape_release_context() {
    for escape in [false, true] {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        if escape {
            dialog_frame(
                &mut app,
                &ctx,
                screen,
                vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
        } else {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "find", "value": "["}));
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "grep", "value": true}));
            scope_click(&mut app, &ctx, screen, "OK");
        }
        assert!(app.ui.dialog.is_none());
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
        assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
    }
}

/// Successor of `find_scope_dialog_domain_and_object_changes_release_original_range`.
#[test]
fn find_scope_form_domain_and_object_changes_release_original_range() {
    for route in ["scope", "object", "menu", "command"] {
        let (mut app, ctx, screen, _) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Find Next");
        match route {
            "scope" => {
                for scope in ["story", "selection"] {
                    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": scope}));
                }
            }
            "object" => {
                scope_click(&mut app, &ctx, screen, "Object");
                scope_click(&mut app, &ctx, screen, "Text");
            }
            "menu" => crate::menus::activate(&mut app, "app.newDocumentDialog", &Value::Null),
            "command" => {
                app.run("app.newDocumentDialog", json!({})).unwrap();
            }
            _ => panic!("known reset route"),
        }
        assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
    }
}

