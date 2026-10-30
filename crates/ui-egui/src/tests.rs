use super::*;

/// The Control panel's two-row groups were centred as if they were one row tall, so the second
/// row hung below the panel and was cut off.
#[test]
fn control_bar_shows_its_second_row() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    app.run("file.newSample", json!({})).unwrap();
    app.ui.control_bar = true;
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut DesignApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    let bar = egui::containers::panel::PanelState::load(&h.ctx, h.ui_id().with("control_bar")).unwrap().outer_rect;
    assert!(bar.height() > 40.0, "{bar:?}");
    // The X/Y/W/H group: its second row holds Y: and H:.
    let group = h.ctx.read_response(egui::Id::unique(("object_control", "dimensions"))).unwrap().rect;
    assert!(bar.contains(group.left_top()), "the X/Y/W/H group {group:?} is in the Control panel {bar:?}");
    assert!(group.bottom() <= bar.bottom(), "X/Y/W/H group {group:?} hangs below the Control panel {bar:?}");
}

#[test]
fn key_names_sent_as_text_are_dropped() {
    let text = |t: &str| egui::Event::Text(t.to_string());
    let events =
        || vec![text("AltGraph"), text("é"), text("Dead"), text("A"), text("Unidentified"), text("ß"), text("AudioVolumeUp"), text("ab"), text("Æ")];
    let mut raw = egui::RawInput { events: events(), ..Default::default() };
    drop_key_name_text(&mut raw, false);
    assert_eq!(raw.events, vec![text("é"), text("A"), text("ß"), text("ab"), text("Æ")]);
    assert!(is_key_name("F13") && is_key_name("Escape") && is_key_name("ZoomToggle") && !is_key_name("F") && !is_key_name("Fx"));
    // Capitalised words (mobile autocompletion) are text.
    let words = vec![text("Hello"), text("Tokyo"), text("OK"), text("Hi there")];
    let mut raw = egui::RawInput { events: words.clone(), ..Default::default() };
    drop_key_name_text(&mut raw, false);
    assert_eq!(raw.events, words);
    // A focused text field or a canvas text caret gets its text from the browser's input
    // events, which hold whole words, key-name-like ones too ("Help").
    let mut raw = egui::RawInput { events: events(), ..Default::default() };
    drop_key_name_text(&mut raw, true);
    assert_eq!(raw.events, events());
}

/// InDesign shows 100% K as pure black on screen unless Appearance of Black says otherwise
/// (Display All Blacks as Rich Black is its default); settings saved without the field get it too.
#[test]
fn black_shows_as_rich_black_by_default() {
    assert!(UiState::default().rich_black);
    let saved: UiState = serde_json::from_str("{}").unwrap();
    assert!(saved.rich_black);
    let accurate: UiState = serde_json::from_str(r#"{"richBlack": false}"#).unwrap();
    assert!(!accurate.rich_black, "a saved choice is kept");
}

/// Tool readouts measured in picas whatever the document's units (the UI's unit is never set):
/// they follow the active document's ruler units.
#[test]
fn tool_readouts_measure_in_the_documents_units() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    assert_eq!(app.view_info().unit, Unit::Picas, "no document: the UI's default");
    app.run("file.new", json!({})).unwrap();
    app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "millimeters"})).unwrap();
    assert_eq!(app.view_info().unit, Unit::Inches);
}

/// Save and Export PDF dialogs started in the home folder (#1653): they start beside a saved
/// document, and for one never saved in the folder of the last save or export.
#[test]
fn save_dialogs_start_beside_the_document_or_in_the_last_folder() {
    use std::{cell::RefCell, rc::Rc};
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let asked: Rc<RefCell<Vec<String>>> = Rc::default();
    let answer: Rc<RefCell<Option<String>>> = Rc::default();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    let (seen, reply) = (asked.clone(), answer.clone());
    app.answer_dialogs(move |request| {
        let craft_dialogs::Request::File(dialog) = request else { return craft_dialogs::Answer::Cancelled };
        let name = dialog.file_name.clone().unwrap_or_default();
        seen.borrow_mut().push(dialog.directory.as_ref().map_or_else(|| name.clone(), |folder| folder.join(&name).to_string_lossy().to_string()));
        reply.borrow().clone().map_or(craft_dialogs::Answer::Cancelled, |path| craft_dialogs::Answer::Paths(vec![path.into()]))
    });
    app.services.write = Some(Box::new(|_: &str, _: &[u8]| Ok(())));
    let last = |asked: &Rc<RefCell<Vec<String>>>| asked.borrow().last().cloned().unwrap_or_default();
    let export_pdf = |app: &mut DesignApp| {
        app.ui.dialog = Some(dialogs::Dialog::new("pdfExport", json!({})));
        app.ui.dialog.as_mut().unwrap().fields.insert("title".into(), json!("Report"));
        dialogs::confirm(app).unwrap();
        app.settle_dialogs();
    };
    let save_dialog = |app: &mut DesignApp| {
        app.run("app.saveDialog", json!({})).unwrap();
        app.settle_dialogs();
    };

    app.run("file.new", json!({})).unwrap();
    let untitled = format!("{}.designcraft", app.session.active().unwrap().doc.title);
    save_dialog(&mut app);
    assert_eq!(last(&asked), untitled, "never saved, nothing remembered: the system's own folder");

    let saved = home.path().join("Brochure.designcraft");
    *answer.borrow_mut() = Some(saved.to_string_lossy().to_string());
    save_dialog(&mut app);
    assert_eq!(app.session.active().unwrap().path.as_deref(), Some(&*saved.to_string_lossy()));
    // An export into another folder becomes the last folder used.
    *answer.borrow_mut() = Some(elsewhere.path().join("Proof.pdf").to_string_lossy().to_string());
    export_pdf(&mut app);
    *answer.borrow_mut() = None;
    export_pdf(&mut app);
    assert_eq!(last(&asked), home.path().join("Report.pdf").to_string_lossy(), "a saved document exports beside itself");

    app.run("file.new", json!({})).unwrap();
    let untitled = format!("{}.designcraft", app.session.active().unwrap().doc.title);
    save_dialog(&mut app);
    assert_eq!(last(&asked), elsewhere.path().join(untitled).to_string_lossy(), "a new document starts in the last folder used");
}

/// No name (Save with no document) suggested the remembered folder itself as the file, and a
/// title with a slash moved the suggestion out of the document's folder.
#[test]
fn save_dialog_suggestions_keep_the_folder_and_only_a_file_name() {
    let folder = tempfile::tempdir().unwrap();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    app.ui.last_save_folder = Some(folder.path().join("Docs").to_string_lossy().to_string());
    assert_eq!(app.suggested_save_path(""), "");
    assert_eq!(app.suggested_save_path("Q1/Q2.pdf"), folder.path().join("Docs").join("Q2.pdf").to_string_lossy());
}

/// An app with one document that has an unsaved edit.
fn app_with_unsaved_document() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    assert!(!app.session.documents()[0].is_dirty(), "a new document has nothing to save");
    app.run("frame.create", json!({"rect": [36, 36, 136, 136]})).unwrap();
    assert!(app.session.documents()[0].is_dirty());
    app
}

/// The recovery timer wrote the whole document on the interface thread: on a catalogue with
/// gigabytes of linked files the window stopped answering for minutes after every edit.
#[test]
fn the_recovery_timer_writes_off_the_interface_thread() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    let dir = tempfile::tempdir().unwrap();
    app.session.recovery_dir = Some(dir.path().to_path_buf());
    app.last_recovery = -1.0e9;
    let uid = app.session.documents()[0].uid;
    let entry = dir.path().join(format!("{uid}.designcraft"));
    let mut written = false;
    for _ in 0..600 {
        shot_frame(&mut app, &ctx, egui::vec2(1400.0, 900.0));
        // The timer fired and the write finished: the file is there and nothing is still queued.
        if entry.exists() && !app.recovery_writing {
            written = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(written, "the timer's background write published the document");
    assert!(app.last_recovery > 0.0, "the timer restarted its interval");
    // The checkpoint is the document's: saving it removes the recovery entry.
    let saved = dir.path().join("saved.designcraft");
    app.run("file.saveAs", json!({"path": saved.to_string_lossy()})).unwrap();
    assert!(!dir.path().join(format!("{uid}.designcraft")).exists());
}

/// File ▸ Close (and its shortcut) closed a document with unsaved changes without asking, and
/// closing discards the document's recovery data too.
#[test]
fn closing_an_unsaved_document_asks_first() {
    let mut app = app_with_unsaved_document();
    menus::activate(&mut app, "file.close", &Value::Null);
    assert_eq!(app.session.documents().len(), 1, "the document stays open until the user answers");
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("closeDocument"));
    // Cancel: nothing happens.
    app.ui.dialog = None;
    assert!(app.session.documents()[0].is_dirty());
    // The tab's × asks too.
    menus::close_document(&mut app, Some(0));
    assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("closeDocument"));
    assert_eq!(app.session.documents().len(), 1);
}

#[test]
fn closing_without_saving_discards_the_document() {
    let mut app = app_with_unsaved_document();
    menus::close_document(&mut app, None);
    app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
    dialogs::confirm(&mut app).unwrap();
    assert!(app.session.documents().is_empty());
    assert!(app.ui.dialog.is_none());
}

#[test]
fn closing_with_save_writes_the_file_then_closes() {
    let mut app = app_with_unsaved_document();
    let path = std::env::temp_dir().join(format!("designcraft-close-test-{}.designcraft", std::process::id()));
    app.run("file.saveAs", json!({"path": path})).unwrap();
    let saved = std::fs::metadata(&path).unwrap().len();
    app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
    menus::close_document(&mut app, None);
    dialogs::confirm(&mut app).unwrap();
    assert!(app.session.documents().is_empty());
    let written = std::fs::metadata(&path).unwrap().len();
    std::fs::remove_file(&path).unwrap();
    assert!(written > saved, "the second frame was saved ({saved} → {written} bytes)");
}

/// Save on a document that has no file opens the file picker; cancelling it must not close the
/// document. (No picker service here: the same as cancelling.)
#[test]
fn a_cancelled_save_keeps_the_document_open() {
    let mut app = app_with_unsaved_document();
    menus::close_document(&mut app, None);
    dialogs::confirm(&mut app).unwrap();
    assert_eq!(app.session.documents().len(), 1);
    assert!(app.session.documents()[0].is_dirty());
}

#[test]
fn closing_a_saved_document_does_not_ask() {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    menus::activate(&mut app, "file.close", &Value::Null);
    assert!(app.session.documents().is_empty());
    assert!(app.ui.dialog.is_none());
}

/// One frame of the quit handling, with the window asked to close when `close`; returns the
/// commands sent to the window.
fn quit_frame(app: &mut DesignApp, ctx: &egui::Context, close: bool) -> Vec<egui::ViewportCommand> {
    let mut raw = egui::RawInput::default();
    if close {
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    }
    let mut out = ctx.run_ui(raw, |ui| app.handle_quit(ui.ctx()));
    out.textures_delta.clear();
    out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
}

fn dialog_uid(app: &DesignApp) -> Option<u64> {
    app.ui.dialog.as_ref().filter(|d| d.id == "closeDocument").and_then(|d| d.fields.get("uid")?.as_u64())
}

/// Quitting closed the window at once, losing unsaved changes (#544).
#[test]
fn quitting_with_unsaved_changes_asks_and_cancel_keeps_everything() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    let cmds = quit_frame(&mut app, &ctx, true);
    assert!(cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
    assert_eq!(dialog_uid(&app), Some(app.session.documents()[0].uid));
    // Cancel: the window and the document stay as they were.
    app.ui.dialog = None;
    for _ in 0..2 {
        assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    }
    assert_eq!(app.session.documents().len(), 1);
    assert!(app.session.documents()[0].is_dirty());
    // A Save whose file picker is cancelled stops quitting too.
    quit_frame(&mut app, &ctx, true);
    dialogs::confirm(&mut app).unwrap();
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert!(app.session.documents()[0].is_dirty());
}

#[test]
fn quitting_asks_about_each_unsaved_document_then_closes() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [36, 36, 136, 136]})).unwrap();
    app.run("file.new", json!({})).unwrap();
    let uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
    for (n, uid) in uids.iter().take(2).enumerate() {
        assert_eq!(dialog_uid(&app), Some(*uid), "question {n}");
        assert_eq!(app.session.active().map(|d| d.uid), Some(*uid), "the document asked about is shown");
        app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
        dialogs::confirm(&mut app).unwrap();
        let cmds = quit_frame(&mut app, &ctx, false);
        assert_eq!(cmds.contains(&egui::ViewportCommand::Close), n == 1, "{cmds:?}");
    }
    // All documents stay open until the backend's final close commits the decision.
    assert_eq!(app.session.documents().len(), 3);
    assert!(quit_frame(&mut app, &ctx, true).is_empty());
    assert!(app.session.documents().is_empty());
}

/// A later Cancel must not destroy documents already approved for discard.
#[test]
fn quitting_later_cancel_retains_every_document_and_active_tab() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
    let uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    let active = app.session.active().unwrap().uid;
    let revisions: Vec<u64> = app.session.documents().iter().map(|d| d.revision).collect();
    let docs: Vec<_> = app.session.documents().iter().map(|d| d.doc.clone()).collect();
    quit_frame(&mut app, &ctx, true);
    app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
    dialogs::confirm(&mut app).unwrap();
    quit_frame(&mut app, &ctx, false);
    assert_eq!(dialog_uid(&app), Some(uids[1]));
    app.ui.dialog = None;
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.documents().iter().map(|d| d.uid).collect::<Vec<_>>(), uids);
    assert_eq!(app.session.active().unwrap().uid, active);
    assert_eq!(app.session.documents().iter().map(|d| d.revision).collect::<Vec<_>>(), revisions);
    for (d, original) in app.session.documents().iter().zip(docs) {
        assert!(std::sync::Arc::ptr_eq(&d.doc, &original));
        assert!(d.is_dirty());
        assert!(!d.history.undo.is_empty());
    }
}

fn second_unsaved_document(app: &mut DesignApp) {
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect": [20, 20, 120, 120]})).unwrap();
}

fn discard_quit_dialog(app: &mut DesignApp) {
    app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
    dialogs::confirm(app).unwrap();
}

#[test]
fn native_quit_prompts_restore_window_visibility_without_focusing_completed_quits() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    let commands = quit_frame(&mut app, &ctx, true);
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    for command in [egui::ViewportCommand::Visible(true), egui::ViewportCommand::Minimized(false), egui::ViewportCommand::Focus] {
        assert!(commands.contains(&command), "unsaved prompt must restore the window: {commands:?}");
    }
    discard_quit_dialog(&mut app);
    let approved = quit_frame(&mut app, &ctx, false);
    assert!(approved.contains(&egui::ViewportCommand::Close));
    let completed = quit_frame(&mut app, &ctx, true);
    assert!(app.session.documents().is_empty());

    let mut clean = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    clean.run("file.new", json!({})).unwrap();
    let clean_close = quit_frame(&mut clean, &ctx, true);
    let mut control = app_with_unsaved_document();
    control.quit_without_asking = true;
    let control_close = quit_frame(&mut control, &ctx, true);
    for commands in [&approved, &completed, &clean_close, &control_close] {
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                egui::ViewportCommand::Visible(true) | egui::ViewportCommand::Minimized(false) | egui::ViewportCommand::Focus
            )),
            "completed and immediate quits must not activate the window: {commands:?}"
        );
    }
}

/// Dock Quit may arrive again while the first request's dialogs are still open.
#[test]
fn repeated_native_quit_requests_preserve_partial_approvals_and_cancel() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    second_unsaved_document(&mut app);
    let original_active = app.session.active().unwrap().uid;
    let uids: Vec<_> = app.session.documents().iter().map(|d| d.uid).collect();
    let docs: Vec<_> = app.session.documents().iter().map(|d| d.doc.clone()).collect();
    let directory = tempfile::tempdir().unwrap();
    app.session.recovery_dir = Some(directory.path().to_path_buf());
    app.run("file.recovery.save", json!({})).unwrap();
    assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(dialog_uid(&app), Some(uids[0]));
    discard_quit_dialog(&mut app);
    quit_frame(&mut app, &ctx, false);
    assert_eq!(dialog_uid(&app), Some(uids[1]));
    for _ in 0..3 {
        assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
        assert_eq!(dialog_uid(&app), Some(uids[1]));
    }
    app.ui.dialog = None; // Cancel the second document, retaining the first Don't Save answer.
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.active().unwrap().uid, original_active);
    assert_eq!(app.session.documents().iter().map(|d| d.uid).collect::<Vec<_>>(), uids);
    for (document, original) in app.session.documents().iter().zip(docs) {
        assert!(std::sync::Arc::ptr_eq(&document.doc, &original));
        assert!(document.is_dirty());
        assert!(!document.history.undo.is_empty());
        assert!(directory.path().join(format!("{}.designcraft", document.uid)).exists());
    }
    // A fresh Dock Quit must ask about the first document again after cancellation.
    assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(dialog_uid(&app), Some(uids[0]));
}

#[test]
fn repeated_native_quit_requests_save_and_discard_only_after_all_answers() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    second_unsaved_document(&mut app);
    let uids: Vec<_> = app.session.documents().iter().map(|d| d.uid).collect();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved.designcraft");
    let chosen = path.to_string_lossy().to_string();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(chosen.clone())));
    app.session.recovery_dir = Some(directory.path().join("recovery"));
    app.run("file.recovery.save", json!({})).unwrap();
    quit_frame(&mut app, &ctx, true);
    assert_eq!(dialog_uid(&app), Some(uids[0]));
    dialogs::confirm(&mut app).unwrap(); // Save the first document.
    app.settle_dialogs();
    assert!(!app.session.documents()[0].is_dirty());
    quit_frame(&mut app, &ctx, false);
    assert_eq!(dialog_uid(&app), Some(uids[1]));
    assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(dialog_uid(&app), Some(uids[1]));
    discard_quit_dialog(&mut app);
    assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.documents().len(), 2);
    assert!(app.session.recovery_dir.as_ref().unwrap().join(format!("{}.designcraft", uids[1])).exists());
    assert!(quit_frame(&mut app, &ctx, true).is_empty());
    assert!(app.session.documents().is_empty());
    assert!(path.exists());
    assert_eq!(designcraft_engine::Session::new().execute("file.open", &json!({"path": path})).unwrap()["index"], 0);
    assert_eq!(std::fs::read_dir(app.session.recovery_dir.as_ref().unwrap()).unwrap().count(), 0);
}

#[test]
fn quitting_first_cancel_restores_original_clean_active_document() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    app.run("file.new", json!({})).unwrap();
    let original_active = app.session.active().unwrap().uid;
    quit_frame(&mut app, &ctx, true);
    assert_ne!(app.session.active().unwrap().uid, original_active);
    app.ui.dialog = None;
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.active().unwrap().uid, original_active);
    assert_eq!(app.session.documents().len(), 2);
}

#[test]
fn quitting_save_as_cancel_after_discard_keeps_all_documents() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    second_unsaved_document(&mut app);
    let active = app.session.active().unwrap().uid;
    quit_frame(&mut app, &ctx, true);
    discard_quit_dialog(&mut app);
    quit_frame(&mut app, &ctx, false);
    dialogs::confirm(&mut app).unwrap(); // No picker: Save As was cancelled.
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.documents().len(), 2);
    assert!(app.session.documents().iter().all(|d| d.is_dirty()));
    assert_eq!(app.session.active().unwrap().uid, active);
}

#[test]
fn quitting_save_failure_after_discard_keeps_documents_and_can_retry() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    second_unsaved_document(&mut app);
    let path = std::env::temp_dir()
        .join(format!("designcraft-quit-missing-{}-{}", std::process::id(), app.session.active().unwrap().uid))
        .join("missing")
        .join("document.designcraft");
    assert!(!path.parent().unwrap().exists());
    let picked = path.to_string_lossy().to_string();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
    quit_frame(&mut app, &ctx, true);
    discard_quit_dialog(&mut app);
    quit_frame(&mut app, &ctx, false);
    // The folder is missing: saving there fails and Quit stops.
    dialogs::confirm(&mut app).unwrap();
    app.settle_dialogs();
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.documents().len(), 2);
    assert!(app.session.documents().iter().all(|d| d.is_dirty()));
    // Starting over must ask about the previously discarded first document again.
    quit_frame(&mut app, &ctx, true);
    assert_eq!(dialog_uid(&app), Some(app.session.documents()[0].uid));
}

/// Quit › Save on a document never saved, then Cancel in the save dialog: Quit stops and the
/// document stays open with its changes.
#[test]
fn quit_save_cancelled_in_the_save_dialog_stops_quitting() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
    quit_frame(&mut app, &ctx, true);
    dialogs::confirm(&mut app).unwrap(); // Save
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close), "Quit waits for the save dialog");
    assert!(app.quitting.is_some());
    app.settle_dialogs();
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert!(app.quitting.is_none(), "Quit stopped");
    assert_eq!(app.session.documents().len(), 1);
    assert!(app.session.documents()[0].is_dirty());
}

/// Close › Save on a document never saved, then Cancel in the save dialog: the document stays
/// open with its changes.
#[test]
fn close_save_cancelled_in_the_save_dialog_keeps_the_document_open() {
    let mut app = app_with_unsaved_document();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
    crate::menus::close_document(&mut app, Some(0));
    dialogs::confirm(&mut app).unwrap(); // Save
    app.settle_dialogs();
    assert_eq!(app.session.documents().len(), 1);
    assert!(app.session.documents()[0].is_dirty());
}

#[test]
fn quitting_save_then_later_cancel_keeps_saved_document_open() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    second_unsaved_document(&mut app);
    let first = app.session.documents()[0].uid;
    let path = std::env::temp_dir().join(format!("designcraft-quit-save-{}-{first}.designcraft", std::process::id()));
    let picked = path.to_string_lossy().to_string();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
    quit_frame(&mut app, &ctx, true);
    dialogs::confirm(&mut app).unwrap();
    app.settle_dialogs();
    quit_frame(&mut app, &ctx, false);
    assert_eq!(app.session.documents().len(), 2);
    assert!(!app.session.documents()[0].is_dirty());
    assert!(path.exists());
    app.ui.dialog = None;
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.documents()[0].uid, first);
    assert!(app.session.documents()[1].is_dirty());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn quitting_rechecks_discard_approval_after_a_later_edit() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    second_unsaved_document(&mut app);
    let first = app.session.documents()[0].uid;
    quit_frame(&mut app, &ctx, true);
    discard_quit_dialog(&mut app);
    quit_frame(&mut app, &ctx, false);
    app.run("file.activate", json!({"index": 0})).unwrap();
    app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
    discard_quit_dialog(&mut app); // Approval is bound to the second UID, despite the switch.
    assert!(!quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(dialog_uid(&app), Some(first), "new edit needs a fresh decision");
    assert_eq!(app.session.documents().len(), 2);
}

#[test]
fn quitting_final_close_rechecks_new_unsaved_document() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    quit_frame(&mut app, &ctx, true);
    discard_quit_dialog(&mut app);
    assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    second_unsaved_document(&mut app); // Arrives after Close was queued, before its event.
    let new_uid = app.session.active().unwrap().uid;
    assert!(quit_frame(&mut app, &ctx, true).contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(dialog_uid(&app), Some(new_uid));
    assert_eq!(app.session.documents().len(), 2);
}

#[test]
fn quitting_pending_save_uses_dialog_uid_after_tab_switch() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    second_unsaved_document(&mut app);
    let first = app.session.documents()[0].uid;
    let path = std::env::temp_dir().join(format!("designcraft-quit-uid-{}-{first}.designcraft", std::process::id()));
    let picked = path.to_string_lossy().to_string();
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked.clone())));
    quit_frame(&mut app, &ctx, true);
    app.run("file.activate", json!({"index": 1})).unwrap();
    dialogs::confirm(&mut app).unwrap();
    app.settle_dialogs();
    assert_eq!(app.session.documents()[0].path.as_deref(), path.to_str());
    assert!(!app.session.documents()[0].is_dirty());
    assert!(app.session.documents()[1].is_dirty());
    quit_frame(&mut app, &ctx, false);
    assert_eq!(dialog_uid(&app), Some(app.session.documents()[1].uid));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn quitting_discard_preserves_recovery_until_final_commit() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    let uid = app.session.active().unwrap().uid;
    let dir = std::env::temp_dir().join(format!("designcraft-quit-recovery-{}-{uid}", std::process::id()));
    app.session.recovery_dir = Some(dir.clone());
    app.run("file.recovery.save", json!({})).unwrap();
    let entry = dir.join(format!("{uid}.designcraft"));
    quit_frame(&mut app, &ctx, true);
    discard_quit_dialog(&mut app);
    assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert!(entry.exists(), "a queued close is not a committed close");
    assert!(quit_frame(&mut app, &ctx, true).is_empty());
    assert!(!entry.exists(), "accepted Don't Save must not be recovered on restart");
    assert!(app.session.documents().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn quitting_native_exit_commits_completed_approvals_before_close_event() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    quit_frame(&mut app, &ctx, true);
    discard_quit_dialog(&mut app);
    assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert_eq!(app.session.documents().len(), 1);
    app.finish_confirmed_quit().unwrap();
    assert!(app.session.documents().is_empty());
}

#[test]
fn quitting_native_exit_retains_incomplete_stale_and_new_document_plans() {
    for case in ["incomplete", "stale", "new document"] {
        let ctx = egui::Context::default();
        let mut app = app_with_unsaved_document();
        if case == "incomplete" {
            second_unsaved_document(&mut app);
        }
        quit_frame(&mut app, &ctx, true);
        discard_quit_dialog(&mut app);
        quit_frame(&mut app, &ctx, false);
        if case == "stale" {
            app.run("frame.create", json!({"rect": [200, 200, 300, 300]})).unwrap();
        } else if case == "new document" {
            second_unsaved_document(&mut app);
        }
        let active = app.session.active().unwrap().uid;
        let before: Vec<_> = app
            .session
            .documents()
            .iter()
            .map(|d| (d.uid, d.revision, d.doc.clone(), d.selection.items.clone(), d.history.undo.len(), d.history.redo.len()))
            .collect();
        app.finish_confirmed_quit().unwrap();
        assert_eq!(app.session.documents().len(), before.len(), "{case}");
        assert_eq!(app.session.active().unwrap().uid, active, "{case}");
        for (d, (uid, revision, doc, selection, undo, redo)) in app.session.documents().iter().zip(before) {
            assert_eq!(
                (d.uid, d.revision, &d.selection.items, d.history.undo.len(), d.history.redo.len()),
                (uid, revision, &selection, undo, redo),
                "{case}"
            );
            assert!(std::sync::Arc::ptr_eq(&d.doc, &doc), "{case}");
        }
    }
}

#[test]
fn quitting_without_unsaved_changes_closes_at_once() {
    let ctx = egui::Context::default();
    let mut app = DesignApp::new(designcraft_engine::Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    // Quit (⌘Q) closes the window like its close button.
    app.run("app.quit", json!({})).unwrap();
    assert!(quit_frame(&mut app, &ctx, false).contains(&egui::ViewportCommand::Close));
    assert!(quit_frame(&mut app, &ctx, true).is_empty());
    assert!(app.ui.dialog.is_none());
}

/// Scripts must not hang on a question: the control channel's quit never asks.
#[test]
fn control_channel_quit_does_not_ask() {
    let ctx = egui::Context::default();
    let mut app = app_with_unsaved_document();
    let (req, _rx) = control::ControlRequest::new("app.quit", json!({}));
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        control::handle(&mut app, ui.ctx(), &req);
    });
    out.textures_delta.clear();
    assert!(quit_frame(&mut app, &ctx, true).is_empty());
    assert!(app.ui.dialog.is_none());
}

/// The Control panel's two-row groups were centred as if they were one row tall, so the second
/// row hung below the panel and was cut off.
#[test]
fn opening_missing_fonts_offers_repeated_replacement_and_keeps_errors_visible() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    app.run("frame.create", json!({"rect":[0,0,200,200], "content":"text", "text":"Sample"})).unwrap();
    let d = std::sync::Arc::make_mut(&mut app.session.doc_mut().unwrap().doc);
    let sid = *d.stories.keys().next().unwrap();
    d.story_mut(sid).unwrap().format_chars(0..6, |f| {
        f.over.font_family = Some("Unavailable Test Family".into());
        f.over.font_style = Some("Regular".into());
    });
    let path = std::env::temp_dir().join(format!("designcraft-font-open-{}.designcraft", std::process::id()));
    app.run("file.save", json!({"path":path})).unwrap();
    app.run("file.open", json!({"path":path})).unwrap();
    let dialog = app.ui.dialog.as_mut().unwrap();
    assert_eq!(dialog.id, "findFont");
    assert_eq!(dialog.fields["onOpen"], true);
    dialog.fields.insert("family".into(), json!("Unavailable Test Family"));
    dialog.fields.insert("style".into(), json!("Regular"));
    dialog.fields.insert("toFamily".into(), json!("Unavailable Target"));
    dialog.fields.insert("toStyle".into(), json!("Regular"));
    assert!(dialogs::confirm(&mut app).is_err());
    assert!(app.ui.dialog.as_ref().unwrap().fields.contains_key("error"));
    app.ui.dialog.as_mut().unwrap().fields.insert("toFamily".into(), json!("Source Sans 3"));
    dialogs::confirm(&mut app).unwrap();
    assert!(!app.ui.dialog.as_ref().unwrap().fields.contains_key("_fonts"));
    assert!(app.run("font.list", json!({})).unwrap().as_array().unwrap().iter().all(|f| f["missing"] == false));
    app.ui.dialog = None;
    app.run("file.save", json!({"path":path})).unwrap();
    app.run("file.open", json!({"path":path})).unwrap();
    assert!(app.ui.dialog.is_none());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn snap_view_uses_the_saved_switches() {
    let mut ui = UiState::default();
    assert!(ui.snap_view().snap_to_guides);
    assert!(!ui.snap_view().snap_to_document_grid);
    assert_eq!(ui.snap_view().zone_px, 4.0);
    ui.snap_to_guides = false;
    ui.snap_zone = 0.0;
    assert!(!ui.snap_view().snap_to_guides);
    assert_eq!(ui.snap_view().zone_px, 0.0);
}

#[test]
fn pdf_export_settings_round_trip() {
    let ui = UiState::default();
    let json = serde_json::to_string(&ui).unwrap();
    let parsed: UiState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.pdf_export, PdfExportSettings::default());
}

#[test]
fn pdf_export_settings_defaults() {
    let settings = PdfExportSettings::default();
    assert_eq!(settings.preset, "Desktop Printing");
    assert_eq!(settings.standard, "none");
    assert!(!settings.compress_images);
    assert_eq!(settings.flatten, "");
    assert!(!settings.spreads);
    assert!(settings.bleed);
    assert!(!settings.marks_crop);
    assert!(!settings.marks_bleed);
    assert!(!settings.marks_page_info);
    assert_eq!(settings.marks_weight, "0.25");
    assert_eq!(settings.marks_offset, "6");
    assert!(settings.tagged);
}

#[test]
fn pdf_export_settings_survive_missing_key() {
    let json = r#"{ "language": "en" }"#;
    let ui: UiState = serde_json::from_str(json).unwrap();
    assert_eq!(ui.pdf_export, PdfExportSettings::default());
}

/// A temporary file named `name` whose bytes aren't a document or a graphic.
fn unreadable_file(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("designcraft-ui-{}-{name}", std::process::id()));
    std::fs::write(&path, b"neither a document nor a graphic").unwrap();
    path
}

/// The open alert dialog: (title, message).
fn alert(app: &DesignApp) -> (String, String) {
    let d = app.ui.dialog.as_ref().expect("an alert is open");
    assert_eq!(d.id, "alert");
    (d.fields["title"].as_str().unwrap().to_string(), d.fields["message"].as_str().unwrap().to_string())
}

#[test]
fn dropped_documents_open_and_the_rest_is_placed() {
    for name in ["a.designcraft", "B.IDML", "c.indd", "D.INDT"] {
        assert!(opens_as_document(name), "{name}");
    }
    for name in ["a.png", "b.pdf", "idml.txt", "c.idml.zip", "e.indd.zip"] {
        assert!(!opens_as_document(name), "{name}");
    }
}

/// Files without an extension: dropped, a document opens and anything else is placed; asked
/// for by the operating system, they wait for the window and then open as documents.
#[test]
fn extensionless_documents_open_by_content() {
    let path = std::env::temp_dir().join(format!("designcraft-ui-{}-Brochure", std::process::id()));
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({})).unwrap();
    let bytes = designcraft_engine::cmd::base64_decode(app.run("file.serialize", json!({})).unwrap()["base64"].as_str().unwrap());
    std::fs::write(&path, bytes).unwrap();
    let p = path.to_string_lossy().to_string();
    app.open_dropped(&p).unwrap();
    assert_eq!(app.session.documents().len(), 2, "a document opens, it isn't placed");
    app.open_documents_later([p.clone()]);
    assert_eq!(app.session.documents().len(), 2, "queued until the window is ready");
    app.open_os_documents();
    assert_eq!(app.session.documents().len(), 3);
    assert_eq!(app.session.active().and_then(|d| d.path.clone()), Some(p));
    let _ = std::fs::remove_file(&path);
    let notes = unreadable_file("notes");
    let e = app.open_dropped(&notes.to_string_lossy()).unwrap_err();
    let _ = std::fs::remove_file(&notes);
    assert_eq!(alert(&app), ("Can't Place the File".to_string(), e));
}

#[test]
fn status_messages_follow_the_interface_language_but_engine_errors_stay_english() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.status_error("Table", "No table selected");
    assert_eq!(app.ui.status, "Table: No table selected");
    app.ui.language = "cs".into();
    app.status_error("Table", "No table selected");
    assert_eq!(app.ui.status, format!("{}: No table selected", crate::i18n::tr("cs", "Table")));
}

#[test]
fn dropping_an_unreadable_file_alerts() {
    // A document opens, so it says it can't be opened.
    let path = unreadable_file("broken.designcraft");
    let mut app = DesignApp::new(Session::new(), Services::default());
    let r = app.open_dropped(&path.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    let e = r.unwrap_err();
    let (title, message) = alert(&app);
    assert_eq!(title, "Can't Open the File", "a document opens, it isn't placed");
    assert_eq!(message, e);
    assert!(app.ui.status.contains(&e), "the status line says it too");
    assert!(app.session.documents().is_empty());
    dialogs::confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none(), "OK closes the alert");
    // Anything else is placed, so it says it can't be placed.
    app.run("file.new", json!({})).unwrap();
    let path = unreadable_file("notes.xyz");
    let r = app.open_dropped(&path.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    let e = r.unwrap_err();
    assert_eq!(alert(&app), ("Can't Place the File".to_string(), e));
}

#[test]
fn a_dropped_indd_file_the_converter_rejects_alerts() {
    let path = unreadable_file("brochure.indd");
    let mut app = DesignApp::new(Session::new(), Services::default());
    let r = app.open_dropped(&path.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    let e = r.unwrap_err();
    assert!(e.contains("InDesign file"), "{e}");
    assert_eq!(alert(&app), ("Can't Open the File".to_string(), e));
    assert!(app.session.documents().is_empty());
}

#[test]
fn dropping_an_ase_file_loads_its_swatches() {
    let mut source = Session::new();
    source.execute("file.new", &json!({})).unwrap();
    source.execute("swatch.create", &json!({"name": "Dropped Teal", "color": "#108080"})).unwrap();
    let data = source.execute("swatch.save", &json!({"names": ["Dropped Teal"]})).unwrap();
    let bytes = designcraft_engine::cmd::base64_decode(data["base64"].as_str().unwrap());
    for name in ["palette.ase", "PALETTE.ASE"] {
        let path = std::env::temp_dir().join(format!("designcraft-ui-{}-{name}", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.open_dropped(&path.to_string_lossy());
        let _ = std::fs::remove_file(&path);
        assert!(app.ui.dialog.is_none(), "{name}: {:?}", app.ui.dialog.as_ref().map(|d| &d.fields));
        assert_eq!(r, Ok(json!({"added": 1, "replaced": 0})), "{name}");
        let doc = &app.session.doc().unwrap().doc;
        assert!(doc.swatches.iter().any(|s| s.name == "Dropped Teal"), "{name}");
        assert!(doc.assets.is_empty(), "{name} isn't placed");
    }
}

#[test]
fn a_file_dropped_while_a_parent_is_open_lands_on_the_parent() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"pages": 3})).unwrap();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    app.run("layout.parents.edit", json!({"on": true})).unwrap();
    let _ = app.view_mut();
    let path = std::env::temp_dir().join(format!("designcraft-ui-{}-drop.png", std::process::id()));
    std::fs::write(&path, designcraft_render::Rendered { width: 10, height: 10, pixels: vec![200; 400] }.to_png()).unwrap();
    let r = app.open_dropped(&path.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    r.unwrap();
    let d = &app.session.doc().unwrap().doc;
    assert_eq!(d.parents[0].items.len(), 1, "the graphic is on the parent spread");
    assert!(d.spreads.iter().all(|s| s.items.is_empty()), "no document page gets it");
}

#[test]
fn file_open_failures_show_an_alert() {
    let path = unreadable_file("open.designcraft");
    let picked = path.to_string_lossy().to_string();
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(picked)));
    // The error comes with the dialog's answer, on a later frame: the alert and the status line.
    app.run("app.openDialog", json!({})).unwrap();
    app.settle_dialogs();
    assert_eq!(alert(&app), ("Can't Open the File".to_string(), app.ui.status.clone()));
    // Placing it says so too.
    app.ui.dialog = None;
    app.run("file.new", json!({})).unwrap();
    app.run("app.placeDialog", json!({})).unwrap();
    app.settle_dialogs();
    let _ = std::fs::remove_file(&path);
    assert_eq!(alert(&app), ("Can't Place the File".to_string(), app.ui.status.clone()));
}

#[test]
fn other_command_errors_stay_in_the_status_line() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    assert!(app.run("file.open", json!({"path": "/nonexistent/x.designcraft"})).is_err());
    assert!(app.ui.dialog.is_none(), "a scripted file.open doesn't raise an alert");
    assert!(!app.ui.status.is_empty());
    // The control channel's app.open answers with the error; no alert waits for a click.
    let (req, _rx) = control::ControlRequest::new("app.open", json!({"path": "/nonexistent/x.designcraft"}));
    let control::Outcome::Done(r) = control::handle(&mut app, &egui::Context::default(), &req) else { panic!("app.open answers") };
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(app.ui.dialog.is_none(), "a control-channel app.open doesn't raise an alert");
}

/// Copied objects were pasted on the first spread whatever page was in view.
#[test]
fn paste_lands_on_the_spread_in_view() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"pages": 3, "facingPages": false})).unwrap();
    let id = app.run("frame.create", json!({"rect": [36, 36, 136, 136]})).unwrap()["id"].clone();
    app.run("selection.set", json!({"ids": [id]})).unwrap();
    app.run("edit.copy", json!({})).unwrap();
    let pasted_on = |app: &DesignApp| {
        let st = app.session.active().unwrap();
        st.doc.find(st.selection.items[0]).unwrap().spread
    };
    // Nothing is in view before the canvas is laid out: the first spread, as in a script.
    app.run("edit.paste", json!({})).unwrap();
    assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Doc(0));
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    canvas::go_to_page(&mut app, 2);
    menus::activate(&mut app, "edit.pasteInPlace", &Value::Null);
    assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Doc(2));
    canvas::go_to_page(&mut app, 1);
    app.run("edit.paste", json!({})).unwrap();
    assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Doc(1));
    // A parent spread while parents are edited.
    app.run("layout.parents.edit", json!({"on": true})).unwrap();
    app.run("edit.paste", json!({})).unwrap();
    assert_eq!(pasted_on(&app), designcraft_doc::SpreadRef::Parent(0));
    // With an insertion point Paste still goes into the text, where the object is anchored.
    let story = app.run("frame.create", json!({"rect": [36, 200, 300, 400], "content": "text", "text": "ab"})).unwrap()["story"].clone();
    app.run("text.select", json!({"story": story, "anchor": 2})).unwrap();
    assert_eq!(app.run("edit.paste", json!({})).unwrap()["anchored"], 1);
}

// ---------------------------------------------------------------------------------------
// egui 0.37 lifecycle, hidden input and screenshot completion regressions.
// ---------------------------------------------------------------------------------------

/// One frame of the real callback order the native and web hosts use.
fn shot_frame(app: &mut DesignApp, ctx: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
    shot_frame_at(app, ctx, egui::Rect::from_min_size(egui::Pos2::ZERO, size), true)
}

/// One frame at a chosen window rect. `inner_rect = false` is the web/egui_kittest shape:
/// `screen_rect` is set but `ViewportInfo::inner_rect` is not.
fn shot_frame_at(app: &mut DesignApp, ctx: &egui::Context, rect: egui::Rect, inner_rect: bool) -> egui::FullOutput {
    let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
    raw.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            inner_rect: inner_rect.then_some(rect),
            native_pixels_per_point: Some(1.0),
            minimized: Some(false),
            occluded: Some(false),
            ..Default::default()
        },
    );
    let mut output = ctx.run_ui(raw, |ui| {
        let ctx = ui.ctx().clone();
        app.logic(&ctx);
        app.ui(ui);
    });
    output.textures_delta.clear();
    output
}

/// A hidden host iteration: the raw input is prepared and then only the logic callback runs.
fn hidden_tick(app: &mut DesignApp, ctx: &egui::Context, raw: &mut egui::RawInput) -> egui::LogicOutput {
    app.raw_input_hook(ctx, raw);
    ctx.run_logic(raw, |ctx| app.logic(ctx))
}

/// Build a 640x480 root viewport marked minimized and occluded.
fn hidden_raw() -> egui::RawInput {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
    let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
    raw.viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            inner_rect: Some(rect),
            native_pixels_per_point: Some(1.0),
            minimized: Some(true),
            occluded: Some(true),
            ..Default::default()
        },
    );
    raw
}

/// Count root `RequestPaintWhileHidden` commands.
fn hidden_paints(output: &egui::LogicOutput) -> usize {
    output
        .viewport_commands
        .get(&egui::ViewportId::ROOT)
        .map_or(0, |commands| commands.iter().filter(|c| matches!(c, egui::ViewportCommand::RequestPaintWhileHidden)).count())
}

/// Remove and return the root screenshot callback, replacing its command with `Focus`.
fn screenshot_callback(output: &mut egui::FullOutput) -> Option<egui::ScreenshotCallback> {
    let viewport = output.viewport_output.get_mut(&egui::ViewportId::ROOT)?;
    for command in &mut viewport.commands {
        if matches!(command, egui::ViewportCommand::Screenshot(_)) {
            let egui::ViewportCommand::Screenshot(callback) = std::mem::replace(command, egui::ViewportCommand::Focus) else {
                return None;
            };
            return Some(callback);
        }
    }
    None
}

/// Create a recording-write app with a new document and four settling frames.
fn screenshot_app() -> (DesignApp, egui::Context, std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>>) {
    let writes: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>> = Default::default();
    let sink = writes.clone();
    let mut app = DesignApp::new(
        Session::new(),
        Services {
            write: Some(Box::new(move |path, bytes| {
                sink.lock().unwrap().push((path.to_string(), bytes.len()));
                Ok(())
            })),
            ..Default::default()
        },
    );
    app.run("file.new", json!({"title": "shots"})).unwrap();
    let ctx = egui::Context::default();
    for _ in 0..4 {
        let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        output.textures_delta.clear();
    }
    (app, ctx, writes)
}

/// The callback only fills a slot; the next logic pass writes and replies once and releases admission.
#[test]
fn screenshot_callback_stays_weak_and_writes_only_from_logic() {
    let (mut app, ctx, writes) = screenshot_app();
    let (reply, replies) = std::sync::mpsc::channel();
    let path = std::env::temp_dir().join(format!("designcraft-shot-{}.png", std::process::id()));
    app.admit_screenshot(Some(path.to_string_lossy().to_string()), reply).unwrap();
    app.queued_shots[0].1 = 0.0;
    let mut callback = None;
    for _ in 0..3 {
        let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        if let Some(cb) = screenshot_callback(&mut output) {
            callback = Some(cb);
        }
    }
    let callback = callback.expect("the settling frames issue one capture");
    assert!(writes.lock().unwrap().is_empty());
    // The callback only fills its slot; writing happens in the next logic pass.
    callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
    assert!(writes.lock().unwrap().is_empty(), "the callback must not write files");
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    let written = writes.lock().unwrap().clone();
    assert_eq!(written.len(), 1, "one file after the logic drain");
    assert_eq!(written[0].0, path.to_string_lossy());
    let reply = replies.recv().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"]["width"], 640);
    assert_eq!(reply["result"]["height"], 480);
    assert!(app.pending_shots.is_empty(), "the reservation is released");
}

/// Admission enforces the request count, aggregate RGBA byte budget and per-image limit before capture.
#[test]
fn screenshot_admission_bounds_count_aggregate_and_image_bytes() {
    let (mut app, ctx, _writes) = screenshot_app();
    let admit = |app: &mut DesignApp, path: &str| {
        let (reply, _replies) = std::sync::mpsc::channel();
        app.admit_screenshot(Some(path.into()), reply)
    };
    for i in 0..SCREENSHOT_MAX_REQUESTS {
        assert!(admit(&mut app, &format!("shot-{i}.png")).is_ok(), "request {i}");
    }
    let error = admit(&mut app, "too-many.png").unwrap_err();
    assert_eq!(error["ok"], false, "{error}");
    // Drain the admitted requests without replying so the byte-budget check is exercised.
    app.pending_shots.clear();
    app.queued_shots.clear();
    for _ in 0..2 {
        shot_frame(&mut app, &ctx, egui::vec2(5120.0, 5120.0));
    }
    assert!(admit(&mut app, "hundred-a.png").is_ok());
    assert!(admit(&mut app, "hundred-b.png").is_ok());
    let error = admit(&mut app, "over-aggregate.png").unwrap_err();
    assert!(error["error"].as_str().unwrap_or_default().contains("budget"), "{error}");
    app.pending_shots.clear();
    app.queued_shots.clear();
    for _ in 0..2 {
        shot_frame(&mut app, &ctx, egui::vec2(6000.0, 6000.0));
    }
    let error = admit(&mut app, "too-large.png").unwrap_err();
    assert!(error["error"].as_str().unwrap_or_default().contains("image limit"), "{error}");
}

/// A resize invalidates in-flight pixels and retries within the deadline; a timeout errors and late callbacks are inert.
#[test]
fn screenshot_epoch_change_and_timeout_drop_stale_pixels() {
    let (mut app, ctx, writes) = screenshot_app();
    let (reply, replies) = std::sync::mpsc::channel();
    app.admit_screenshot(Some("stale.png".into()), reply).unwrap();
    app.queued_shots[0].1 = 0.0;
    let mut callback = None;
    for _ in 0..3 {
        let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        if let Some(cb) = screenshot_callback(&mut output) {
            callback = Some(cb);
        }
    }
    let callback = callback.expect("issued");
    // Resize away and back: the epoch advances even though the size ends up the same.
    shot_frame(&mut app, &ctx, egui::vec2(800.0, 600.0));
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    callback.clone().complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    assert!(replies.try_recv().is_err(), "stale pixels must not be replied");
    assert!(!app.queued_shots.is_empty(), "the request is retried inside its deadline");
    assert!(writes.lock().unwrap().is_empty());
    // Let the deadline pass: the retry reports an error and a later callback is inert.
    app.pending_shots[0].deadline = 0.0;
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    let reply = replies.try_recv().unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    assert!(replies.try_recv().is_err(), "no second reply from a late callback");
    assert!(writes.lock().unwrap().is_empty());
    assert!(app.pending_shots.is_empty());
}

/// A host without `inner_rect` uses `screen_rect` and can still admit and complete a capture.
#[test]
fn screenshot_geometry_uses_screen_rect_when_inner_rect_is_absent() {
    let (mut app, ctx, writes) = screenshot_app();
    app.screenshot_geometry = None;
    app.screenshot_epoch = 0;
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
    // Web eframe and egui_kittest set screen_rect but not ViewportInfo::inner_rect.
    for _ in 0..2 {
        shot_frame_at(&mut app, &ctx, rect, false);
    }
    let geometry = app.screenshot_geometry.expect("the screen rect is the logical viewport");
    assert_eq!((geometry.width, geometry.height), (640, 480), "the fallback is the same logical geometry");
    let (reply, replies) = std::sync::mpsc::channel();
    app.admit_screenshot(Some("web-like.png".into()), reply).expect("an inner-rect-less host admits screenshots");
    app.queued_shots[0].1 = 0.0;
    let mut callback = None;
    for _ in 0..3 {
        if let Some(cb) = screenshot_callback(&mut shot_frame_at(&mut app, &ctx, rect, false)) {
            callback = Some(cb);
        }
    }
    callback.expect("the capture is issued").complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
    shot_frame_at(&mut app, &ctx, rect, false);
    let reply = replies.recv().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(writes.lock().unwrap().len(), 1);
}

/// Apply native pixels-per-point and UI zoom exactly once.
#[test]
fn screenshot_geometry_applies_native_dpi_and_zoom_once() {
    let (mut app, ctx, _writes) = screenshot_app();
    app.ui.ui_scale = 1.5;
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 50.0));
    let raw = {
        let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
        raw.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo { inner_rect: Some(rect), native_pixels_per_point: Some(2.0), minimized: Some(false), ..Default::default() },
        );
        raw
    };
    // The first pass applies the zoom; the next logic tick measures the new geometry.
    for _ in 0..2 {
        let mut raw = raw.clone();
        raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().inner_rect = Some(rect);
        ctx.run_ui(raw, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        })
        .textures_delta
        .clear();
    }
    let geometry = app.screenshot_geometry.expect("geometry");
    // 100 x 50 points at 2 physical pixels per point and 1.5 UI zoom: apply each exactly once.
    assert_eq!((geometry.width, geometry.height), (300, 150));
}

/// Moving the window leaves the capture epoch unchanged; resizing and resizing back advance it.
#[test]
fn screenshot_epoch_ignores_window_position_but_tracks_size_changes() {
    let (mut app, ctx, _writes) = screenshot_app();
    let before = app.screenshot_epoch;
    let moved = egui::Rect::from_min_size(egui::pos2(120.0, 80.0), egui::vec2(640.0, 480.0));
    shot_frame_at(&mut app, &ctx, moved, true);
    assert_eq!(app.screenshot_epoch, before, "moving the window is not a capture-surface change");
    shot_frame_at(&mut app, &ctx, egui::Rect::from_min_size(egui::pos2(120.0, 80.0), egui::vec2(800.0, 600.0)), true);
    let resized = app.screenshot_epoch;
    assert_ne!(resized, before, "a size change advances the epoch");
    shot_frame_at(&mut app, &ctx, egui::Rect::from_min_size(egui::pos2(120.0, 80.0), egui::vec2(640.0, 480.0)), true);
    assert_ne!(app.screenshot_epoch, resized, "and resizing back advances it again");
}

/// `ui.screenshot` is admitted, waits for capture, then writes once and replies with the dimensions.
#[test]
fn control_screenshot_replies_with_the_saved_capture() {
    let (mut app, ctx, writes) = screenshot_app();
    let (requests, receiver) = std::sync::mpsc::channel();
    app = app.with_control(receiver);
    let (reply, replies) = std::sync::mpsc::channel();
    requests.send(ControlRequest { method: "ui.screenshot".into(), params: json!({"path": "control.png", "focus": false}), reply }).unwrap();
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    assert_eq!(app.pending_shots.len(), 1, "ui.screenshot is admitted, not issued from the queue");
    assert!(replies.try_recv().is_err(), "the reply waits for the capture");
    app.queued_shots[0].1 = 0.0;
    let mut callback = None;
    for _ in 0..3 {
        if let Some(cb) = screenshot_callback(&mut shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0))) {
            callback = Some(cb);
        }
    }
    let callback = callback.expect("the settled frames issue one capture");
    callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    let reply = replies.recv().unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"]["width"], 640);
    assert_eq!(reply["result"]["height"], 480);
    let written = writes.lock().unwrap().clone();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, "control.png");
    assert!(written[0].1 > 0);
    assert!(app.pending_shots.is_empty(), "the reservation is released");
    assert!(app.queued_shots.is_empty());
}

/// A timed-out capture replies with a no-frame error and writes no file.
#[test]
fn control_screenshot_times_out_without_writing_a_file() {
    let (mut app, ctx, writes) = screenshot_app();
    let (requests, receiver) = std::sync::mpsc::channel();
    app = app.with_control(receiver);
    let (reply, replies) = std::sync::mpsc::channel();
    requests.send(ControlRequest { method: "ui.screenshot".into(), params: json!({"path": "control.png", "focus": false}), reply }).unwrap();
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    assert_eq!(app.pending_shots.len(), 1);
    app.pending_shots[0].deadline = 0.0;
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    let reply = replies.recv().unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    assert!(reply["error"].as_str().unwrap_or_default().contains("no frame"), "{reply}");
    assert!(writes.lock().unwrap().is_empty(), "a timed-out request writes no file");
    assert!(app.pending_shots.is_empty());
}

/// A multipass frame ticks for every pass but runs once-per-frame work once.
#[test]
fn discarded_pass_runs_the_frame_stage_once_but_ticks_every_pass() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"title": "multipass"})).unwrap();
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let mut raw = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
    raw.viewports
        .insert(egui::ViewportId::ROOT, egui::ViewportInfo { inner_rect: Some(rect), native_pixels_per_point: Some(1.0), ..Default::default() });
    let mut passes = 0;
    let mut output = ctx.run_ui(raw, |ui| {
        passes += 1;
        if passes == 1 {
            // Requested before `app.ui` so the first pass is a discarded one and must not
            // acknowledge injected input.
            ui.ctx().request_discard("forced multipass regression");
        }
        let ctx = ui.ctx().clone();
        app.logic(&ctx);
        app.ui(ui);
    });
    assert_eq!(output.platform_output.num_completed_passes, 2, "the frame really was multipass");
    // 0.37's TexturesDelta panics if dropped unapplied; this test only inspects the passes.
    output.textures_delta.clear();
    assert_eq!(app.logic_tick_ticks, 2, "every pass ticks queues and deadlines");
    assert_eq!(app.logic_frame_ticks, 1, "once-per-frame work runs once");
    assert_eq!(app.input_ticks, 2);
}

/// Hidden logic requests one paint per tick, injects one queued step at a time, treats unknown visibility as hidden, and lets one visible pass consume the step.
#[test]
fn hidden_logic_requests_exactly_one_paint_per_tick_until_a_settled_ui_pass() {
    let (mut app, ctx, _writes) = screenshot_app();
    let mut raw = hidden_raw();
    // Three pointer steps queued: one batch may be in the host's pending raw input at a time.
    app.control_synthetic = vec![
        egui::Event::PointerMoved(egui::pos2(10.0, 10.0)),
        egui::Event::PointerMoved(egui::pos2(20.0, 20.0)),
        egui::Event::PointerMoved(egui::pos2(30.0, 30.0)),
    ];
    app.raw_input_hook(&ctx, &mut raw);
    let injected = raw.events.clone();
    assert_eq!(app.injected_events.len(), 1, "one pointer event in the batch");
    assert_eq!(app.control_synthetic.len(), 2, "the rest wait for later host iterations");
    for tick in 0..3 {
        let output = hidden_tick(&mut app, &ctx, &mut raw);
        assert_eq!(hidden_paints(&output), 1, "tick {tick}: exactly one hidden paint request");
        assert!(app.synthetic_injected, "the injected step is still unapplied");
        assert!(app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "control provenance is retained while hidden");
        assert_eq!(raw.events, injected, "tick {tick}: no second batch accumulated into the pending raw input");
        assert_eq!(app.control_synthetic.len(), 2, "tick {tick}: the queue did not drain another step");
    }
    // A real visible UI pass takes exactly the one pending event (no duplicated PointerMoved).
    raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
    raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().occluded = Some(false);
    let mut seen = Vec::new();
    ctx.run_ui(raw, |ui| {
        seen = ui.ctx().input(|i| i.events.clone());
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    })
    .textures_delta
    .clear();
    assert_eq!(seen.iter().filter(|event| matches!(event, egui::Event::PointerMoved(_))).count(), 1);
    assert!(!app.synthetic_injected, "a settled UI pass acknowledges delivery");
    assert!(app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "the pass's end guard still sees the provenance");
    app.control_synthetic.clear();
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    assert!(!app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "the next host iteration clears it");
    // Unknown visibility is treated like hidden, and the next queued step is injected.
    let mut unknown = egui::RawInput::default();
    unknown.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo::default());
    app.control_synthetic.push(egui::Event::Text("x".into()));
    app.raw_input_hook(&ctx, &mut unknown);
    let output = ctx.run_logic(&unknown, |ctx| app.logic(ctx));
    assert_eq!(hidden_paints(&output), 1, "visible() == None is treated like hidden");
}

/// A font-gated first pass requeues injected input instead of acknowledging it; a later settled pass consumes it.
#[test]
fn font_gated_first_pass_requeues_injected_input_instead_of_acknowledging_it() {
    let mut app = DesignApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"title": "fonts"})).unwrap();
    let ctx = egui::Context::default();
    app.control_synthetic.push(egui::Event::Text("x".into()));
    let mut raw = hidden_raw();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(app.synthetic_injected);
    // The first real pass installs fonts and returns before any widget consumes input.
    ctx.run_ui(raw, |ui| {
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    })
    .textures_delta
    .clear();
    assert!(!app.synthetic_injected, "a font-gated pass is not an acknowledgement");
    assert_eq!(app.control_synthetic, vec![egui::Event::Text("x".into())], "the batch is back in its queue");
    assert!(!app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed));
    // Once fonts are ready the same step is injected and a settled pass consumes it.
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    assert!(app.fonts_ready);
    let mut raw = hidden_raw();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(app.synthetic_injected);
    raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
    ctx.run_ui(raw, |ui| {
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    })
    .textures_delta
    .clear();
    assert!(!app.synthetic_injected);
    assert!(app.control_synthetic.is_empty());
}

/// Injected input is acknowledged only by the settled multipass frame.
#[test]
fn discarded_pass_does_not_acknowledge_injected_input() {
    let (mut app, ctx, _writes) = screenshot_app();
    app.control_synthetic.push(egui::Event::Text("d".into()));
    let mut raw = hidden_raw();
    raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
    app.raw_input_hook(&ctx, &mut raw);
    assert!(app.synthetic_injected);
    let mut passes = 0;
    let output = ctx.run_ui(raw, |ui| {
        passes += 1;
        if passes == 1 {
            ui.ctx().request_discard("injected input must survive a discarded pass");
        }
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    });
    assert_eq!(output.platform_output.num_completed_passes, 2);
    assert!(!app.synthetic_injected, "the settled second pass acknowledges it");
    assert!(app.control_synthetic.is_empty());
}

/// A document change drops the stale batch and removes its events from pending raw input.
#[test]
fn a_document_switch_before_the_ui_pass_drops_the_pending_batch() {
    let (mut app, ctx, _writes) = screenshot_app();
    let pos = egui::pos2(10.0, 10.0);
    app.control_synthetic.push(egui::Event::PointerMoved(pos));
    let mut raw = hidden_raw();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(app.synthetic_injected);
    let _ = hidden_tick(&mut app, &ctx, &mut raw);
    // A menu/control action switched documents before the forced paint.
    app.run("file.new", json!({"title": "handoff"})).unwrap();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(!app.synthetic_injected, "the stale batch was discarded");
    assert!(app.injected_events.is_empty());
    assert!(!raw.events.iter().any(|event| matches!(event, egui::Event::PointerMoved(p) if *p == pos)), "the event was removed from pending input");
    raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized = Some(false);
    let mut seen = Vec::new();
    ctx.run_ui(raw, |ui| {
        seen = ui.ctx().input(|i| i.events.clone());
        app.logic(&ui.ctx().clone());
        app.ui(ui);
    })
    .textures_delta
    .clear();
    assert!(!seen.iter().any(|event| matches!(event, egui::Event::PointerMoved(p) if *p == pos)), "the new document did not receive it");
}

/// Save a document, make an unsaved edit and return the temporary directory.
#[cfg(not(target_arch = "wasm32"))]
fn prepare_revert(app: &mut DesignApp) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    app.run("file.saveAs", json!({"path": dir.path().join("revert.designcraft")})).unwrap();
    app.run("frame.create", json!({"rect": [72, 72, 144, 144], "content": "text", "text": "unsaved"})).unwrap();
    assert!(app.session.active().unwrap().is_dirty());
    dir
}

/// Revert keeps the document uid, advances the identity epoch and clears dirty state.
#[cfg(not(target_arch = "wasm32"))]
fn revert_preserving_uid(app: &mut DesignApp) {
    let before = app.session.active().unwrap();
    let (uid, epoch) = (before.uid, before.identity_epoch);
    app.run("file.revert", json!({})).unwrap();
    let after = app.session.active().unwrap();
    assert_eq!(after.uid, uid, "Revert keeps the open document's stable identity");
    assert_ne!(after.identity_epoch, epoch, "Revert replaces the document state");
    assert!(!after.is_dirty());
}

/// Input captured before Revert is neither acknowledged nor delivered to the restored document.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_revert_discards_a_buffered_input_batch() {
    let (mut app, ctx, _writes) = screenshot_app();
    let _dir = prepare_revert(&mut app);
    let pos = egui::pos2(10.0, 10.0);
    app.control_synthetic.push(egui::Event::PointerMoved(pos));
    let mut raw = hidden_raw();
    app.raw_input_hook(&ctx, &mut raw);
    assert!(app.synthetic_injected);
    revert_preserving_uid(&mut app);
    app.raw_input_hook(&ctx, &mut raw);
    assert!(!app.synthetic_injected, "input captured before Revert must not reach the restored document");
    assert!(app.injected_events.is_empty());
    assert!(!raw.events.iter().any(|event| matches!(event, egui::Event::PointerMoved(p) if *p == pos)));
}

/// Return `hidden_raw` with the root viewport marked visible.
#[cfg(not(target_arch = "wasm32"))]
fn visible_raw() -> egui::RawInput {
    let mut raw = hidden_raw();
    let viewport = raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
    viewport.minimized = Some(false);
    viewport.occluded = Some(false);
    raw
}

/// Build Cmd-key press and release events.
#[cfg(not(target_arch = "wasm32"))]
fn command_key(key: egui::Key) -> Vec<egui::Event> {
    let modifiers = egui::Modifiers { command: true, mac_cmd: true, ..egui::Modifiers::NONE };
    [true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).to_vec()
}

/// A different document with one selectable item, for an asynchronous open.
#[cfg(not(target_arch = "wasm32"))]
fn document_with_a_frame() -> Vec<u8> {
    let mut session = Session::new();
    session.execute("file.new", &json!({"title": "Incoming"})).unwrap();
    session.execute("frame.create", &json!({"rect": [10, 10, 50, 50]})).unwrap();
    let serialized = session.execute("file.serialize", &json!({})).unwrap();
    designcraft_engine::cmd::base64_decode(serialized["base64"].as_str().unwrap())
}

/// One host iteration in eframe's order: `raw_input_hook`, then `run_ui` calling
/// `App::logic` and `App::ui` (eframe native `update`).
#[cfg(not(target_arch = "wasm32"))]
fn run_host_frame(app: &mut DesignApp, ctx: &egui::Context, mut raw: egui::RawInput) {
    app.raw_input_hook(ctx, &mut raw);
    ctx.run_ui(raw, |ui| {
        let ctx = ui.ctx().clone();
        app.logic(&ctx);
        if ctx.current_pass_index() == 0 {
            app.poll_native_actions(&ctx, || None);
        }
        app.ui(ui);
    })
    .textures_delta
    .clear();
}

/// A native Revert runs only after the injected press/release settles; pointer state and user clipboard provenance are preserved.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_native_revert_waits_for_the_control_gesture() {
    let (mut app, ctx, _writes) = screenshot_app();
    let _dir = prepare_revert(&mut app);
    let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
    for pressed in [true, false] {
        app.control_synthetic.push(egui::Event::PointerButton {
            pos: egui::pos2(320.0, 240.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    for step in 0..3 {
        let mut raw = visible_raw();
        app.raw_input_hook(&ctx, &mut raw);
        ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            if ctx.current_pass_index() == 0 {
                app.poll_native_actions(&ctx, || actions.pop_front());
            }
            app.ui(ui);
        })
        .textures_delta
        .clear();
        assert_eq!(app.session.active().unwrap().is_dirty(), step < 2, "Revert runs only after the release");
        assert_eq!(ctx.input(|input| input.pointer.primary_down()), step == 0, "pointer state is not reset by the boundary");
    }
    assert!(actions.is_empty());
    assert!(app.session.active().unwrap().selection.is_empty());
    assert!(!app.control_input_frame.load(std::sync::atomic::Ordering::Relaxed), "native actions keep user clipboard provenance");
}

/// Close is cancelled and deferred while old-document input is pending, then the request survives to the next frame.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_close_waits_for_input_and_keeps_the_request() {
    let (mut app, ctx, _writes) = screenshot_app();
    let _dir = prepare_revert(&mut app);
    app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let mut raw = visible_raw();
    raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close);
    app.raw_input_hook(&ctx, &mut raw);
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(&ui.ctx().clone());
        assert!(app.ui.dialog.is_none(), "the close dialog must not receive the old document's input");
        app.ui(ui);
    });
    assert!(out.viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::CancelClose));
    out.textures_delta.clear();
    assert_eq!(app.session.documents().len(), 1);
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(dialog_uid(&app), Some(app.session.active().unwrap().uid), "the close request survives the deferred frame");
}

/// A later control command cannot overtake queued input and eventually completes.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_control_command_waits_for_the_preceding_input() {
    let (mut app, ctx, _writes) = screenshot_app();
    let (requests, receiver) = std::sync::mpsc::channel();
    app = app.with_control(receiver);
    let (movement, first_reply) = ControlRequest::new("ui.move", json!({"x": 10, "y": 10}));
    let (command, second_reply) = ControlRequest::new("engine.execute", json!({"command": "frame.create", "params": {"rect": [72,72,144,144]}}));
    requests.send(movement).unwrap();
    requests.send(command).unwrap();
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(first_reply.try_recv().unwrap()["ok"], true);
    assert!(second_reply.try_recv().is_err(), "later commands must not overtake queued input");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert!(second_reply.try_recv().is_err(), "the injected input must settle first");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(second_reply.try_recv().unwrap()["ok"], true, "the later command is eventually handled");
}

/// Revert targets the original document before deferred OS-open delivery resumes.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_native_revert_precedes_the_deferred_os_open() {
    let (mut app, ctx, _writes) = screenshot_app();
    let dir = prepare_revert(&mut app);
    let original = app.session.active().unwrap().uid;
    let path = dir.path().join("incoming.designcraft");
    std::fs::write(&path, document_with_a_frame()).unwrap();
    app.open_documents_later([path.to_string_lossy().to_string()]);
    app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
    let mut raw = visible_raw();
    app.raw_input_hook(&ctx, &mut raw);
    ctx.run_ui(raw, |ui| {
        app.logic(&ui.ctx().clone());
        if ui.ctx().current_pass_index() == 0 {
            app.poll_native_actions(ui.ctx(), || actions.pop_front());
        }
        app.ui(ui);
    })
    .textures_delta
    .clear();
    assert_eq!(app.session.documents().len(), 1);
    assert!(app.session.active().unwrap().is_dirty());
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.active().unwrap().uid, original);
    assert!(!app.session.active().unwrap().is_dirty(), "Revert runs before delivery resumes");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 2);
    assert_ne!(app.session.active().unwrap().uid, original);
    assert!(!app.session.documents().iter().find(|doc| doc.uid == original).unwrap().is_dirty(), "Revert targeted the original document");
}

/// Preview cleanup cannot replace the input batch's document and runs after the batch settles.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_stale_preview_waits_for_the_input_batch() {
    let (mut app, ctx, _writes) = screenshot_app();
    let original = app.session.active().unwrap().uid;
    app.session.preview_new_document(&json!({"title": "Preview"})).unwrap();
    let preview = app.session.new_document_preview();
    assert!(preview.is_some());
    app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.new_document_preview(), preview, "preview cleanup cannot replace the input's document");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert!(app.session.new_document_preview().is_none());
    assert_eq!(app.session.active().unwrap().uid, original);
}

/// Revert waits for Select All's forwarded text input to settle.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_native_choices_wait_for_forwarded_text_input() {
    let (mut app, ctx, _writes) = screenshot_app();
    let _dir = prepare_revert(&mut app);
    let mut text = String::from("native field");
    ctx.run_ui(visible_raw(), |ui| {
        ui.text_edit_singleline(&mut text).request_focus();
    })
    .textures_delta
    .clear();
    assert!(ctx.text_edit_focused(), "the native edit command must forward to an actual text field");
    let mut actions = std::collections::VecDeque::from([("edit.selectAll".to_owned(), Value::Null), ("file.revert".to_owned(), json!({}))]);
    app.poll_native_actions(&ctx, || actions.pop_front());
    run_host_frame(&mut app, &ctx, visible_raw());
    assert!(app.session.active().unwrap().is_dirty(), "Revert waits for Select All's forwarded input");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert!(!app.session.active().unwrap().is_dirty());
}

/// A queued native menu event precedes and blocks deferred OS-open delivery without synthetic input.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_arriving_native_event_blocks_an_os_open_without_synthetic_input() {
    let (mut app, ctx, _writes) = screenshot_app();
    let dir = prepare_revert(&mut app);
    let original = app.session.active().unwrap().uid;
    let path = dir.path().join("incoming.designcraft");
    std::fs::write(&path, document_with_a_frame()).unwrap();
    app.open_documents_later([path.to_string_lossy().to_string()]);
    let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
    app.native_action_notifier(ctx.clone())();
    let mut raw = visible_raw();
    app.raw_input_hook(&ctx, &mut raw);
    ctx.run_ui(raw, |ui| {
        app.logic(&ui.ctx().clone());
        assert_eq!(app.session.documents().len(), 1, "a menu event already queued by the host must precede the OS open");
        if ui.ctx().current_pass_index() == 0 {
            app.poll_native_actions(ui.ctx(), || actions.pop_front());
        }
        app.ui(ui);
    })
    .textures_delta
    .clear();
    assert!(!app.session.active().unwrap().is_dirty(), "the menu action ran in the real UI callback");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 2);
    assert_ne!(app.session.active().unwrap().uid, original);
}

/// Hidden logic-only ticks cannot execute native menu actions; a real UI pass can.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_deferred_native_action_waits_for_a_real_ui_pass() {
    let (mut app, ctx, _writes) = screenshot_app();
    let _dir = prepare_revert(&mut app);
    app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    let mut actions = std::collections::VecDeque::from([("file.revert".to_owned(), json!({}))]);
    let mut raw = visible_raw();
    app.raw_input_hook(&ctx, &mut raw);
    ctx.run_ui(raw, |ui| {
        app.logic(&ui.ctx().clone());
        if ui.ctx().current_pass_index() == 0 {
            app.poll_native_actions(ui.ctx(), || actions.pop_front());
        }
        app.ui(ui);
    })
    .textures_delta
    .clear();
    assert!(app.session.active().unwrap().is_dirty());
    let mut raw = hidden_raw();
    app.raw_input_hook(&ctx, &mut raw);
    let _ = ctx.run_logic(&raw, |ctx| app.logic(ctx));
    assert!(app.session.active().unwrap().is_dirty(), "a hidden logic-only tick must not execute native menu actions");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert!(!app.session.active().unwrap().is_dirty(), "the deferred action runs when a UI callback resumes");
}

/// An inbox delivery runs only after input sampled for the current document has settled, so
/// a pending Select All cannot select text in the opened document.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_inbox_open_must_not_receive_a_pending_shortcut() {
    let (mut app, ctx, _writes) = screenshot_app();
    app.services.inbox = Some(Inbox::default());
    let request = app.import_request("open");
    let file = ImportedFile { request, name: "incoming.designcraft".into(), bytes: document_with_a_frame() };
    app.services.inbox.as_ref().unwrap().lock().unwrap().push(file);
    app.control_synthetic.extend(command_key(egui::Key::A));
    run_host_frame(&mut app, &ctx, visible_raw());
    // A deferred delivery is handled by the next settled host iteration.
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 2, "the inbox file is still opened");
    assert!(app.session.active().unwrap().selection.is_empty(), "Select All sampled before the asynchronous open must not reach the opened document");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_os_document_must_not_receive_a_pending_shortcut() {
    let (mut app, ctx, _writes) = screenshot_app();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incoming.designcraft");
    std::fs::write(&path, document_with_a_frame()).unwrap();
    app.open_documents_later([path.to_string_lossy().to_string()]);
    app.control_synthetic.extend(command_key(egui::Key::A));
    run_host_frame(&mut app, &ctx, visible_raw());
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 2, "the OS document is still opened");
    assert!(app.session.active().unwrap().selection.is_empty(), "Select All sampled before the OS open must not reach the opened document");
}

/// The intended boundary: an imported file waits while the batch that was sampled for the
/// current document is pending, and is handled once that batch has settled.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_inbox_delivery_waits_for_a_settled_batch_then_runs() {
    let (mut app, ctx, _writes) = screenshot_app();
    app.services.inbox = Some(Inbox::default());
    let request = app.import_request("open");
    let file = ImportedFile { request, name: "incoming.designcraft".into(), bytes: document_with_a_frame() };
    app.services.inbox.as_ref().unwrap().lock().unwrap().push(file);
    app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 1, "the delivery waited for the injected step");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 2, "the deferred delivery is not lost");
}

/// Deferred OS open waits for injected input and is not lost.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_host_order_os_document_waits_for_a_settled_batch_then_runs() {
    let (mut app, ctx, _writes) = screenshot_app();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incoming.designcraft");
    std::fs::write(&path, document_with_a_frame()).unwrap();
    app.open_documents_later([path.to_string_lossy().to_string()]);
    app.control_synthetic.push(egui::Event::PointerMoved(egui::pos2(10.0, 10.0)));
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 1, "the OS open waited for the injected step");
    run_host_frame(&mut app, &ctx, visible_raw());
    assert_eq!(app.session.documents().len(), 2, "the deferred OS open is not lost");
}

/// Revert drops the buffered native drop so it cannot modify the restored document.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_revert_discards_a_buffered_native_drop() {
    let (mut app, ctx) = drop_test_app();
    let dir = prepare_revert(&mut app);
    let path = dir.path().join("drop.png");
    std::fs::write(&path, designcraft_render::Rendered { width: 10, height: 10, pixels: vec![200; 400] }.to_png()).unwrap();
    let mut raw = visible_raw_with_drop(&[&path]);
    app.raw_input_hook(&ctx, &mut raw);
    assert_eq!(app.pending_drops.len(), 1);
    revert_preserving_uid(&mut app);
    let assets_before = app.session.active().unwrap().doc.assets.len();
    run_ui_pass(&mut app, &ctx, raw);
    assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "a stale native drop must not modify the restored document");
    assert!(app.pending_drops.is_empty());
}

/// Revert rejects pre-issue and in-flight screenshots with a document error, issues no retry and writes no pixels.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_revert_rejects_screenshots_before_issue_and_in_flight() {
    for in_flight in [false, true] {
        let (mut app, ctx, writes) = screenshot_app();
        let _dir = prepare_revert(&mut app);
        let (reply, replies) = std::sync::mpsc::channel();
        app.admit_screenshot(Some("revert-stale.png".into()), reply).unwrap();
        app.queued_shots[0].1 = 0.0;
        let mut callback = None;
        if in_flight {
            for _ in 0..3 {
                let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
                if let Some(cb) = screenshot_callback(&mut output) {
                    callback = Some(cb);
                }
            }
            assert!(callback.is_some(), "the in-flight case must actually issue a capture");
        }
        revert_preserving_uid(&mut app);
        if let Some(callback) = callback {
            callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        }
        for _ in 0..4 {
            let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
            assert!(screenshot_callback(&mut output).is_none(), "Revert must not issue or retry an old capture (in_flight={in_flight})");
        }
        let reply = replies.try_recv().expect("the replaced document's screenshot receives an error");
        assert_eq!(reply["ok"], false, "in_flight={in_flight}: {reply}");
        assert!(reply["error"].as_str().unwrap_or_default().contains("document"));
        assert!(writes.lock().unwrap().is_empty(), "stale pixels must not be saved");
        assert!(app.pending_shots.is_empty());
        assert!(app.queued_shots.is_empty());
    }
}

// ---------------------------------------------------------------------------------------
// Document-generation regressions: reused screenshot slots and buffered native drops must
// not cross a document/dialog/focus change.
// ---------------------------------------------------------------------------------------

/// A document switch before issue errors the request and leaves no pending or queued capture.
#[test]
fn design_generation_screenshot_switch_before_issue_does_not_capture_the_new_document() {
    let (mut app, ctx, writes) = screenshot_app();
    let document_a = app.session.active().unwrap().uid;
    let (requests, receiver) = std::sync::mpsc::channel();
    app = app.with_control(receiver);
    let (reply, replies) = std::sync::mpsc::channel();
    requests
        .send(ControlRequest { method: "ui.screenshot".into(), params: json!({"path": "switch-before-issue.png", "focus": false}), reply })
        .unwrap();
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    assert_eq!(app.pending_shots.len(), 1, "ui.screenshot is admitted");
    assert_eq!(app.pending_shots[0].geometry.document, document_a);
    // A menu/control action switches documents before the settling rule issues the capture.
    app.run("file.new", json!({"title": "handoff-before-issue"})).unwrap();
    let document_b = app.session.active().unwrap().uid;
    assert_ne!(document_b, document_a);
    if let Some(at) = app.queued_shots.first_mut() {
        at.1 = 0.0;
    }
    let mut callback = None;
    for _ in 0..3 {
        let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        if let Some(cb) = screenshot_callback(&mut output) {
            callback = Some(cb);
        }
    }
    if let Some(callback) = callback {
        callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    }
    assert!(writes.lock().unwrap().is_empty(), "the switched request must not write a file");
    let reply = replies.try_recv().expect("the switched request must receive an error receipt");
    assert_eq!(reply["ok"], false, "the old document's request must not succeed on the new one: {reply}");
    assert!(reply["error"].as_str().unwrap_or_default().contains("document"), "the error must name the document change: {reply}");
    assert!(app.pending_shots.is_empty(), "the stale request released its admission");
    assert!(app.queued_shots.is_empty(), "the stale request left no repaint token");
}

/// An in-flight old-document capture cannot complete or retry on the new document; the request and tokens are released.
#[test]
fn design_generation_screenshot_in_flight_after_switch_does_not_complete_on_the_new_document() {
    let (mut app, ctx, writes) = screenshot_app();
    let document_a = app.session.active().unwrap().uid;
    let (reply, replies) = std::sync::mpsc::channel();
    app.admit_screenshot(Some("switch-in-flight.png".into()), reply).unwrap();
    app.queued_shots[0].1 = 0.0;
    let mut callback = None;
    for _ in 0..3 {
        let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        if let Some(cb) = screenshot_callback(&mut output) {
            callback = Some(cb);
        }
    }
    let callback = callback.expect("the request is issued on the first document");
    assert_eq!(app.pending_shots[0].geometry.document, document_a);
    // The capture is in flight on A when the app switches to B.
    app.run("file.new", json!({"title": "handoff-in-flight"})).unwrap();
    assert_ne!(app.session.active().unwrap().uid, document_a);
    callback.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
    shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    // Old code retries the stale request on B; follow any retry through a completion so the
    // regression proves that B never receives the old capture.
    if let Some(at) = app.queued_shots.first_mut() {
        at.1 = 0.0;
    }
    let mut retried = None;
    for _ in 0..3 {
        let mut output = shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
        if let Some(cb) = screenshot_callback(&mut output) {
            retried = Some(cb);
        }
    }
    if let Some(retried) = retried {
        retried.complete(std::sync::Arc::new(egui::ColorImage::filled([640, 480], egui::Color32::BLACK)));
        shot_frame(&mut app, &ctx, egui::vec2(640.0, 480.0));
    }
    assert!(writes.lock().unwrap().is_empty(), "the in-flight request must not write a file after the switch");
    let reply = replies.try_recv().expect("the in-flight request must receive an error receipt");
    assert_eq!(reply["ok"], false, "the in-flight request must not succeed on the new document: {reply}");
    assert!(reply["error"].as_str().unwrap_or_default().contains("document"), "the error must name the document change: {reply}");
    assert!(app.pending_shots.is_empty(), "the stale request released its admission");
    assert!(app.queued_shots.is_empty(), "the stale request left no repaint token");
}

/// Admission stamps the current session document, not cached geometry from the previous document.
#[test]
fn design_generation_screenshot_admission_uses_the_current_document_not_the_cached_geometry() {
    let (mut app, _ctx, _writes) = screenshot_app();
    let document_a = app.session.active().unwrap().uid;
    // `screenshot_app` populated the geometry cache for A. Switch documents without a frame
    // between the switch and admission, as two control requests in one logic tick would.
    app.run("file.new", json!({"title": "admission-current-document"})).unwrap();
    let document_b = app.session.active().unwrap().uid;
    assert_ne!(document_b, document_a);
    let (reply, replies) = std::sync::mpsc::channel();
    app.admit_screenshot(None, reply).unwrap();
    assert_eq!(app.pending_shots[0].geometry.document, document_b, "admission must use the current session document, not the last cached geometry");
    assert!(replies.try_recv().is_err(), "admission alone replies only when it fails");
    app.pending_shots.clear();
    app.queued_shots.clear();
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct TestDroppedFile(std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl egui::DroppedFile for TestDroppedFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

/// Wrap a path as an egui dropped-file handle backed by the test type.
#[cfg(not(target_arch = "wasm32"))]
fn dropped_file(path: &std::path::Path) -> egui::DroppedFileHandle {
    std::sync::Arc::new(TestDroppedFile(path.to_path_buf()))
}

/// Create a screenshot app with a fixed canvas rectangle and an initialized view for drop tests.
#[cfg(not(target_arch = "wasm32"))]
fn drop_test_app() -> (DesignApp, egui::Context) {
    let (mut app, ctx, _writes) = screenshot_app();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
    let _ = app.view_mut();
    (app, ctx)
}

/// Write a tiny unique PNG for drop tests.
#[cfg(not(target_arch = "wasm32"))]
fn temporary_png(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("designcraft-ui-{}-{name}.png", std::process::id()));
    std::fs::write(&path, designcraft_render::Rendered { width: 10, height: 10, pixels: vec![200; 400] }.to_png()).unwrap();
    path
}

/// Build visible raw input with a pointer move and dropped-file handles.
#[cfg(not(target_arch = "wasm32"))]
fn visible_raw_with_drop(paths: &[&std::path::Path]) -> egui::RawInput {
    let mut raw = hidden_raw();
    let viewport = raw.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
    viewport.minimized = Some(false);
    viewport.occluded = Some(false);
    raw.events.push(egui::Event::PointerMoved(egui::pos2(100.0, 100.0)));
    raw.dropped_files.extend(paths.iter().map(|path| dropped_file(path)));
    raw
}

/// Run one logic-plus-UI frame and discard its texture deltas.
#[cfg(not(target_arch = "wasm32"))]
fn run_ui_pass(app: &mut DesignApp, ctx: &egui::Context, raw: egui::RawInput) {
    ctx.run_ui(raw, |ui| {
        let ctx = ui.ctx().clone();
        app.logic(&ctx);
        app.ui(ui);
    })
    .textures_delta
    .clear();
}

/// A drop buffered before a document switch cannot add assets to the switched document.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_generation_native_drop_is_discarded_when_the_document_changes_before_ui() {
    let (mut app, ctx) = drop_test_app();
    let path = temporary_png("drop-document-change");
    let mut raw = visible_raw_with_drop(&[&path]);
    app.raw_input_hook(&ctx, &mut raw);
    assert!(raw.dropped_files.is_empty(), "raw_input_hook captures the native drop");
    assert_eq!(app.pending_drops.len(), 1, "the raw drop is buffered for the UI pass");
    // A menu/control action switches documents before the settled UI pass consumes it.
    app.run("file.new", json!({"title": "drop-target"})).unwrap();
    let assets_before = app.session.active().unwrap().doc.assets.len();
    run_ui_pass(&mut app, &ctx, raw);
    assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "the stale drop must not land in the switched document");
    assert!(app.pending_drops.is_empty(), "the discarded drop released its buffer");
    let _ = std::fs::remove_file(path);
}

/// A drop buffered before a dialog opens cannot land behind it.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_generation_native_drop_is_discarded_when_a_dialog_opens_before_ui() {
    let (mut app, ctx) = drop_test_app();
    let path = temporary_png("drop-dialog-change");
    let mut raw = visible_raw_with_drop(&[&path]);
    app.raw_input_hook(&ctx, &mut raw);
    assert_eq!(app.pending_drops.len(), 1);
    app.run("app.preferences", json!({})).unwrap();
    assert!(app.ui.dialog.is_some(), "a real dialog changed the generation");
    let assets_before = app.session.active().unwrap().doc.assets.len();
    run_ui_pass(&mut app, &ctx, raw);
    assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "the stale drop must not land behind the dialog");
    assert!(app.pending_drops.is_empty(), "the discarded drop released its buffer");
    let _ = std::fs::remove_file(path);
}

/// A keyboard-focus change invalidates a buffered drop.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_generation_native_drop_is_discarded_when_focus_changes_before_ui() {
    let (mut app, ctx) = drop_test_app();
    let path = temporary_png("drop-focus-change");
    let mut raw = visible_raw_with_drop(&[&path]);
    app.raw_input_hook(&ctx, &mut raw);
    assert_eq!(app.pending_drops.len(), 1);
    let focus = egui::Id::unique("design-generation-drop-focus");
    assert_ne!(app.input_generation().focus, Some(focus));
    ctx.memory_mut(|memory| memory.request_focus(focus));
    assert_eq!(app.input_generation().focus, Some(focus), "keyboard focus changed after the drop was buffered");
    let assets_before = app.session.active().unwrap().doc.assets.len();
    run_ui_pass(&mut app, &ctx, raw);
    assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before, "the stale drop must not land after a focus change");
    assert!(app.pending_drops.is_empty(), "the discarded drop released its buffer");
    let _ = std::fs::remove_file(path);
}

/// With no generation change, every dropped file in a batch is placed.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn design_generation_native_drop_batch_without_a_change_still_places_every_file() {
    let (mut app, ctx) = drop_test_app();
    let first = temporary_png("drop-batch-a");
    let second = temporary_png("drop-batch-b");
    let mut raw = visible_raw_with_drop(&[&first, &second]);
    app.raw_input_hook(&ctx, &mut raw);
    assert_eq!(app.pending_drops.len(), 2, "both files were captured from the raw input");
    let assets_before = app.session.active().unwrap().doc.assets.len();
    run_ui_pass(&mut app, &ctx, raw);
    assert_eq!(app.session.active().unwrap().doc.assets.len(), assets_before + 2, "both files in the valid batch are placed");
    assert!(app.pending_drops.is_empty());
    let _ = std::fs::remove_file(first);
    let _ = std::fs::remove_file(second);
}
