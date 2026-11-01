use super::*;
use designcraft_engine::Session;
fn app() -> DesignApp {
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let app = DesignApp::new(session, crate::Services::default());
    app.file_providers.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
    app.file_providers.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
    app
}
fn request(app: &mut DesignApp) -> String {
    save(app, false, None).unwrap();
    let events = app.file_providers.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].metadata["type"], "saveDialog");
    events[0].metadata["requestId"].as_str().unwrap().to_string()
}
#[test]
fn provider_ack_cannot_clear_edits_made_after_capture() {
    let mut app = app();
    std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Before capture".into();
    let request = request(&mut app);
    app.file_providers.resolve_dialog(&request, "provider", "saved.designcraft").unwrap();
    let events = app.file_providers.take_events();
    assert!(!events[0].bytes.as_ref().unwrap().is_empty());
    std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "After capture".into();
    app.file_providers.result(request, "saved".into(), "file-1".into()).unwrap();
    tick(&mut app);
    assert!(app.session.active().unwrap().is_dirty());
}
#[test]
fn successful_ack_updates_the_captured_document_not_the_current_tab() {
    let mut app = app();
    std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Before capture".into();
    let uid = app.session.active().unwrap().uid;
    let request = request(&mut app);
    app.file_providers.resolve_dialog(&request, "provider", "saved.designcraft").unwrap();
    app.session.execute("file.new", &json!({})).unwrap();
    assert_ne!(app.session.active().unwrap().uid, uid);
    app.file_providers.result(request, "saved".into(), "file-1".into()).unwrap();
    tick(&mut app);
    assert!(!app.session.documents().iter().find(|d| d.uid == uid).unwrap().is_dirty());
    assert_eq!(app.file_providers.bound_document("file-1"), Some(uid));
}
#[test]
fn download_result_does_not_claim_durable_provider_storage() {
    let mut app = app();
    std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Before capture".into();
    let request = request(&mut app);
    app.file_providers.resolve_dialog(&request, "device", "local.designcraft").unwrap();
    app.file_providers.result(request, "download".into(), String::new()).unwrap();
    tick(&mut app);
    assert!(app.session.active().unwrap().is_dirty());
    assert!(app.file_providers.snapshot()["resources"].as_array().unwrap().is_empty());
}

fn active_uid(app: &DesignApp) -> u64 {
    app.session.active().unwrap().uid
}

fn activate(app: &mut DesignApp, uid: u64) {
    let index = app.session.documents().iter().position(|doc| doc.uid == uid).unwrap();
    app.run("file.activate", json!({"index":index})).unwrap();
}

fn has_document(app: &DesignApp, uid: u64) -> bool {
    app.session.documents().iter().any(|doc| doc.uid == uid)
}

fn choose(app: &mut DesignApp, after: Option<After>) -> String {
    let value = save(app, false, after).unwrap();
    let request = value["requestId"].as_str().unwrap().to_string();
    app.file_providers.take_events();
    request
}

fn save_result(app: &mut DesignApp, request: &str, destination: &str, status: &str) {
    app.file_providers.resolve_dialog(request, destination, "Captured.designcraft").unwrap();
    app.file_providers.take_events();
    app.file_providers.result(request.into(), status.into(), if status == "saved" { "captured".into() } else { String::new() }).unwrap();
}

fn close_after(uid: u64, calls: std::sync::Arc<std::sync::atomic::AtomicUsize>) -> After {
    After::Test(Box::new(move |app| {
        let index = app.session.documents().iter().position(|doc| doc.uid == uid).ok_or("captured document closed")?;
        let value = app.run("file.close", json!({"index":index}))?;
        calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(value)
    }))
}

fn fill_event_queue(host: &Host) {
    host.take_events();
    let mut session = Session::new();
    session.execute("file.new", &json!({})).unwrap();
    let doc = session.active().unwrap().clone();
    for _ in 0..32 {
        let request = host.begin(vec![doc.clone()], doc.uid, doc.revision, "Queue.designcraft".into(), vec![1], false).unwrap();
        host.resolve_dialog(&request, "cancel", "").unwrap();
        let Action::Save { capture, .. } = host.take_actions().remove(0) else { panic!("expected cancel") };
        host.finish_save(&request, &capture, "cancel", "", true);
    }
}

#[test]
fn pending_a_guards_only_a_while_b_can_close_in_each_save_phase() {
    for phase in 0..3 {
        let mut app = app();
        let a = active_uid(&app);
        let request = choose(&mut app, None);
        if phase > 0 {
            app.file_providers.resolve_dialog(&request, "provider", "A.designcraft").unwrap();
        }
        if phase > 1 {
            app.file_providers.result(request.clone(), "saved".into(), "a".into()).unwrap();
        }
        app.run("file.new", json!({})).unwrap();
        let b = active_uid(&app);
        assert!(app.run("file.close", json!({"index":0})).is_err());
        activate(&mut app, a);
        // Revert's engine implementation ignores index and acts on active A.
        assert!(app.run("file.revert", json!({"index":1})).is_err());
        activate(&mut app, b);
        assert!(app.run("app.close", json!({})).is_err());
        assert!(app.run("app.revert", json!({})).is_err());
        app.run("file.close", json!({})).unwrap();
        assert!(has_document(&app, a));
        assert!(!has_document(&app, b));
    }
}

#[test]
fn pending_work_rejects_invalid_close_indices_without_falling_back_to_another_document() {
    let mut app = app();
    choose(&mut app, None);
    app.run("file.new", json!({})).unwrap();
    let b = active_uid(&app);
    for index in [json!(-1), json!("1"), Value::Null, json!(u64::MAX), json!(99)] {
        assert!(app.run("file.close", json!({"index":index})).is_err());
        assert_eq!(app.session.documents().len(), 2);
        assert_eq!(active_uid(&app), b);
    }
}

#[test]
fn a_local_b_can_revert_while_a_has_a_provider_choice() {
    let mut app = app();
    let a = active_uid(&app);
    choose(&mut app, None);
    app.run("file.new", json!({})).unwrap();
    let b = active_uid(&app);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("B.designcraft");
    app.session.execute("file.save", &json!({"path":path.to_str().unwrap()})).unwrap();
    let saved_title = app.session.active().unwrap().doc.title.clone();
    std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Unsaved B".into();
    app.run("file.revert", json!({"index":0})).unwrap();
    assert_eq!(active_uid(&app), b);
    assert_eq!(app.session.active().unwrap().doc.title, saved_title);
    assert!(!app.session.active().unwrap().is_dirty());
    assert!(app.file_providers.pending_for(a));
}

#[test]
fn closing_a_preceding_tab_cannot_retarget_the_ack_or_focus() {
    let mut app = app();
    let b = active_uid(&app);
    app.run("file.new", json!({})).unwrap();
    let a = active_uid(&app);
    let request = choose(&mut app, None);
    app.run("file.new", json!({})).unwrap();
    let c = active_uid(&app);
    app.run("file.close", json!({"index":0})).unwrap();
    assert!(!has_document(&app, b));
    save_result(&mut app, &request, "provider", "saved");
    tick(&mut app);
    assert_eq!(active_uid(&app), c);
    assert_eq!(app.file_providers.bound_document("captured"), Some(a));
    assert!(!app.session.documents().iter().find(|doc| doc.uid == a).unwrap().is_dirty());
}

#[test]
fn close_after_save_waits_for_the_terminal_then_runs_once_and_restores_focus_by_uid() {
    for full in [false, true] {
        let mut app = app();
        let a = active_uid(&app);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request = choose(&mut app, Some(close_after(a, calls.clone())));
        app.run("file.new", json!({})).unwrap();
        let b = active_uid(&app);
        app.file_providers.resolve_dialog(&request, "provider", "A.designcraft").unwrap();
        app.file_providers.take_events();
        if full {
            fill_event_queue(&app.file_providers);
        }
        app.file_providers.result(request, "saved".into(), "a".into()).unwrap();
        tick(&mut app);
        // A hidden logic tick must not close a document before this frame's input.
        assert!(has_document(&app, a));
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
        if full {
            settled(&mut app, &egui::Context::default());
            assert!(has_document(&app, a));
            assert!(app.file_providers.pending_for(a));
            assert_eq!(app.file_providers.take_events().len(), 64);
        }
        assert!(app.run("file.close", json!({"index":0})).is_err());
        settled(&mut app, &egui::Context::default());
        assert!(!has_document(&app, a));
        assert_eq!(active_uid(&app), b);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        settled(&mut app, &egui::Context::default());
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(app.provider_after.is_empty());
    }
}

#[test]
fn cancel_failure_download_and_later_edits_never_run_a_close_continuation() {
    for outcome in ["cancel", "failed", "download", "laterEdit", "identityChanged", "accountChanged"] {
        let mut app = app();
        let a = active_uid(&app);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request = choose(&mut app, Some(close_after(a, calls.clone())));
        let destination = if outcome == "download" { "device" } else { "provider" };
        let status = if matches!(outcome, "cancel" | "failed" | "download") { outcome } else { "saved" };
        save_result(&mut app, &request, destination, status);
        if outcome == "identityChanged" {
            app.session.active_mut().unwrap().identity_epoch = u64::MAX;
        }
        tick(&mut app);
        if outcome == "laterEdit" {
            std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "New input in the ACK frame".into();
        }
        if outcome == "accountChanged" {
            app.file_providers.disconnect().unwrap();
            app.file_providers.connect("test".into(), "Other account".into(), "other".into()).unwrap();
        }
        settled(&mut app, &egui::Context::default());
        assert!(has_document(&app, a), "{outcome}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0, "{outcome}");
        assert!(app.provider_after.is_empty(), "{outcome}");
    }
}

#[test]
fn suspension_does_not_cancel_an_admitted_capture_but_stale_saved_revision_does() {
    for stale in [false, true] {
        let mut app = app();
        let a = active_uid(&app);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request = choose(&mut app, Some(close_after(a, calls.clone())));
        app.file_providers.set_enabled(false).unwrap();
        save_result(&mut app, &request, "provider", "saved");
        tick(&mut app);
        if stale {
            app.session.active_mut().unwrap().saved_revision = u64::MAX;
        }
        settled(&mut app, &egui::Context::default());
        assert_eq!(has_document(&app, a), stale);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), usize::from(!stale));
    }
}

#[test]
fn continuations_keep_global_quit_guarded_after_the_host_has_released_its_owner() {
    let mut app = app();
    let a = active_uid(&app);
    let request = choose(&mut app, Some(After::Test(Box::new(|_| Ok(Value::Null)))));
    save_result(&mut app, &request, "provider", "saved");
    tick(&mut app);
    assert!(!app.file_providers.busy());
    assert!(busy(&app));
    assert!(app.finish_confirmed_quit().is_err());
    assert!(app.run("app.close", json!({})).is_err());
    assert!(app.run("app.revert", json!({})).is_err());
    assert!(app.run("file.close", json!({})).is_err());
    assert!(save(&mut app, false, None).is_err());
    assert!(has_document(&app, a));
    settled(&mut app, &egui::Context::default());
    assert!(!busy(&app));
}

#[test]
fn the_original_local_route_is_unchanged_without_registration() {
    let mut app = app();
    app.file_providers.disconnect().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Local.designcraft");
    app.run("file.save", json!({"path":path.to_str().unwrap()})).unwrap();
    assert!(path.exists());
    assert!(!app.session.active().unwrap().is_dirty());
    assert!(app.file_providers.take_events().is_empty());
    app.run("file.close", json!({})).unwrap();
    assert!(app.session.documents().is_empty());
}

#[test]
fn quit_save_approves_only_after_settlement_and_cancels_on_failure_or_new_input() {
    for outcome in ["saved", "failed", "cancel", "download", "laterEdit"] {
        let mut app = app();
        let a = active_uid(&app);
        app.quitting = Some(crate::QuitState { original_active: Some(a), accepted: Default::default(), pending: Some(a), saving: None });
        app.confirm_quit_document(a, false).unwrap();
        let events = app.file_providers.take_events();
        let request = events[0].metadata["requestId"].as_str().unwrap().to_string();
        let status = if outcome == "laterEdit" { "saved" } else { outcome };
        save_result(&mut app, &request, if status == "download" { "device" } else { "provider" }, status);
        tick(&mut app);
        if outcome == "laterEdit" {
            std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Later input".into();
        }
        settled(&mut app, &egui::Context::default());
        if outcome == "saved" {
            let quit = app.quitting.as_ref().unwrap();
            assert_eq!(quit.saving, None);
            assert_eq!(quit.accepted.get(&a), Some(&app.session.active().unwrap().revision));
        } else {
            assert!(app.quitting.is_none(), "{outcome}");
        }
        assert!(has_document(&app, a));
    }
}

#[test]
fn a_parked_interaction_keeps_its_continuation_open_without_blocking_b() {
    let mut app = app();
    let a = active_uid(&app);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let request = choose(&mut app, Some(close_after(a, calls.clone())));
    app.session.begin_interaction("Move after capture").unwrap();
    app.run("file.new", json!({})).unwrap();
    let b = active_uid(&app);
    save_result(&mut app, &request, "provider", "saved");
    tick(&mut app);
    settled(&mut app, &egui::Context::default());
    assert!(has_document(&app, a));
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
    app.run("file.close", json!({})).unwrap();
    assert!(!has_document(&app, b));
    activate(&mut app, a);
    app.session.cancel_interaction();
    settled(&mut app, &egui::Context::default());
    assert!(!has_document(&app, a));
}

#[test]
fn checkpoint_and_typed_close_preserve_b_composition_and_preview_without_activating_a() {
    for composing in [false, true] {
        let mut app = app();
        let a = active_uid(&app);
        let request = choose(&mut app, Some(After::Close(a)));
        app.run("file.new", json!({})).unwrap();
        let b = active_uid(&app);
        if composing {
            app.session.execute("frame.create", &json!({"rect":[36,36,300,200],"content":"text"})).unwrap();
            app.session.set_tool("type");
            app.session.tool_preedit("marked", None, designcraft_engine::ViewInfo::default()).unwrap();
            assert!(app.session.tool_composing());
        } else {
            let doc = app.session.active_mut().unwrap();
            doc.preview_stash = Some(doc.doc.clone());
            std::sync::Arc::make_mut(&mut doc.doc).title = "B preview".into();
        }
        let b_doc = app.session.active().unwrap().doc.clone();
        let b_preview = app.session.active().unwrap().preview_stash.clone();
        let b_revision = app.session.active().unwrap().revision;
        let b_history = app.session.active().unwrap().history.undo.len();
        save_result(&mut app, &request, "provider", "saved");
        tick(&mut app);
        assert_eq!(active_uid(&app), b);
        assert!(std::sync::Arc::ptr_eq(&app.session.active().unwrap().doc, &b_doc));
        assert_eq!(app.session.tool_composing(), composing);
        settled(&mut app, &egui::Context::default());
        assert!(!has_document(&app, a));
        let doc = app.session.active().unwrap();
        assert_eq!(doc.uid, b);
        assert!(std::sync::Arc::ptr_eq(&doc.doc, &b_doc));
        assert_eq!(doc.revision, b_revision);
        assert_eq!(doc.history.undo.len(), b_history);
        assert_eq!(doc.preview_stash.is_some(), b_preview.is_some());
        if let Some(preview) = b_preview {
            assert!(std::sync::Arc::ptr_eq(doc.preview_stash.as_ref().unwrap(), &preview));
        }
        assert_eq!(app.session.tool_composing(), composing);
    }
}

#[test]
fn closing_by_uid_rejects_a_replaced_identity_and_preserves_an_unrelated_active_uid() {
    let mut app = app();
    let a = active_uid(&app);
    let epoch = app.session.active().unwrap().identity_epoch;
    app.run("file.new", json!({})).unwrap();
    app.run("file.new", json!({})).unwrap();
    let c = active_uid(&app);
    assert!(!app.session.close_document_by_uid(a, u64::MAX));
    assert_eq!(app.session.documents().len(), 3);
    assert!(app.session.close_document_by_uid(a, epoch));
    assert_eq!(active_uid(&app), c);
    assert!(!has_document(&app, a));
}

#[test]
fn admitted_close_continuations_are_bounded_even_after_their_transport_owners_finish() {
    let mut app = app();
    for _ in 0..MAX_CONTINUATIONS {
        choose(&mut app, Some(After::Test(Box::new(|_| Ok(Value::Null)))));
        app.run("file.new", json!({})).unwrap();
    }
    assert!(save(&mut app, false, Some(After::Test(Box::new(|_| Ok(Value::Null))))).is_err());
    assert_eq!(app.provider_after.len(), MAX_CONTINUATIONS);
}
