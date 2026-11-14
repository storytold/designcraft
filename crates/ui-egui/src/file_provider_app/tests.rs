use super::*;
use designcraft_doc::{AssetData, LinkedSource};
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
fn provider_save_rejects_referenced_links_before_emitting_a_save_dialog() {
    const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8"/></svg>"#;
    let mut app = app();
    app.session.execute("file.place", &json!({"name":"Linked.svg","base64":designcraft_engine::cmd::base64_encode(SVG)})).unwrap();
    let st = app.session.active_mut().unwrap();
    let uid = st.uid;
    let doc = std::sync::Arc::make_mut(&mut st.doc);
    let id = *doc.used_assets().iter().next().expect("placed SVG is referenced");
    let asset = std::sync::Arc::make_mut(doc.assets.get_mut(&id).unwrap());
    let path = "Linked.svg".to_owned();
    asset.link = Some(path.clone());
    asset.storage = Storage::Linked;
    asset.data = AssetData::linked(LinkedSource { path, relative: None, document: None, len: SVG.len() as u64, fingerprint: None, hash: None });

    assert!(app.session.active().unwrap().is_dirty());
    for save_as in [false, true] {
        assert_eq!(save(&mut app, save_as, Some(After::Close(uid))).unwrap_err(), EMBED_LINKS);
        assert!(app.file_providers.take_events().is_empty());
        assert!(!provider_operations_pending(&app));
        let active = app.session.active().unwrap();
        assert_eq!(active.uid, uid);
        assert!(active.is_dirty());
    }
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
        let error = app.run("file.close", json!({"index":0})).unwrap_err();
        assert_eq!(app.ui.status, error);
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
fn idle_provider_does_not_refuse_close_for_transient_editor_state() {
    for kind in ["interaction", "preview", "ime"] {
        let mut app = app();
        match kind {
            "interaction" => app.session.begin_interaction("Transient preview").unwrap(),
            "preview" => {
                let doc = app.session.active_mut().unwrap();
                doc.preview_stash = Some(doc.doc.clone());
            }
            _ => {
                app.session.execute("frame.create", &json!({"rect":[36,36,300,200],"content":"text"})).unwrap();
                app.session.set_tool("type");
                app.session.tool_preedit("marked", None, designcraft_engine::ViewInfo::default()).unwrap();
                assert!(app.session.tool_composing());
            }
        }
        assert!(guard_command(&app, "file.revert", &json!({})).is_ok(), "{kind}");
        app.run("file.close", json!({})).unwrap();
        assert!(app.session.documents().is_empty(), "{kind}");
    }
}

#[test]
fn provider_save_dialog_suggests_a_portable_native_name_without_changing_the_title() {
    let long = "🧪".repeat(120);
    let cases = [("Q1/Q2 report", "Q1 Q2 report.designcraft"), ("   ", "Untitled.designcraft"), ("CON", "_CON.designcraft")];
    for (title, expected) in
        cases.into_iter().map(|(title, name)| (title, name.to_string())).chain([(long.as_str(), format!("{}.designcraft", "🧪".repeat(60)))])
    {
        let mut app = app();
        std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = title.into();
        save(&mut app, false, None).unwrap();
        let events = app.file_providers.take_events();
        assert_eq!(events[0].metadata["type"], "saveDialog");
        assert_eq!(events[0].metadata["filename"], expected);
        assert_eq!(app.session.active().unwrap().doc.title, title);
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
    // Closing B selects its neighbor. Establish C as the active UID before the asynchronous ACK.
    activate(&mut app, c);
    assert_eq!(active_uid(&app), c);
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
            app.file_providers.take_events();
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
    app.file_providers.take_events();
    assert!(!app.file_providers.busy());
    assert!(application_block(&app).is_some());
    assert!(app.finish_confirmed_quit().is_err());
    assert!(app.run("app.close", json!({})).is_err());
    assert!(app.run("app.revert", json!({})).is_err());
    assert!(app.run("file.close", json!({})).is_err());
    assert!(save(&mut app, false, None).is_err());
    assert!(has_document(&app, a));
    settled(&mut app, &egui::Context::default());
    assert!(application_block(&app).is_none());
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
    for index in 0..MAX_CONTINUATIONS {
        let request = choose(&mut app, Some(After::Test(Box::new(|_| Ok(Value::Null)))));
        app.file_providers.resolve_dialog(&request, "provider", "Captured.designcraft").unwrap();
        app.file_providers.take_events();
        app.file_providers.result(request, "saved".into(), format!("continuation-{index}")).unwrap();
        // Apply transport completion only. Settled UI callbacks have deliberately not run.
        tick(&mut app);
        app.file_providers.take_events();
        assert!(!app.file_providers.busy());
        app.run("file.new", json!({})).unwrap();
    }
    assert_eq!(app.provider_after.len(), MAX_CONTINUATIONS);
    assert!(app.provider_after.values().all(|after| after.ready));
    let error = save(&mut app, false, Some(After::Test(Box::new(|_| Ok(Value::Null))))).unwrap_err();
    assert_eq!(error, "finish or cancel pending save continuations before starting another");
    assert_eq!(app.provider_after.len(), MAX_CONTINUATIONS);
}

fn control(app: &mut DesignApp, method: &str, params: Value) -> Value {
    let (request, _) = crate::control::ControlRequest::new(method, params);
    match crate::control::handle(app, &egui::Context::default(), &request) {
        crate::control::Outcome::Done(value) => value,
        _ => panic!("expected a synchronous command response"),
    }
}

fn provider_rows(app: &DesignApp) -> String {
    let connection = app.file_providers.connection().unwrap();
    app.file_providers
        .set_resources(
            &connection,
            vec![crate::file_providers::Resource { id: "resource-1".into(), name: "Native.designcraft".into(), updated_at: String::new() }],
        )
        .unwrap();
    connection
}

#[test]
fn provider_recent_is_discoverable_and_control_engine_execute_emits_the_scoped_open_request() {
    let mut app = app();
    let connection = provider_rows(&app);
    let catalog = control(&mut app, "engine.commands", json!({}));
    let command = catalog["result"].as_array().unwrap().iter().find(|row| row["id"] == "file.openProviderResource").unwrap();
    assert_eq!(command["ui"], true);
    assert_eq!(command["enabled"], true);
    assert!(command["params"].as_str().unwrap().contains("connectionId"));
    let response = control(
        &mut app,
        "engine.execute",
        json!({"command":"file.openProviderResource","params":{"connectionId":connection,"resourceId":"resource-1"}}),
    );
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["pending"], "fileProvider");
    let request = response["result"]["requestId"].as_str().unwrap();
    let events = app.file_providers.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].metadata["type"], "openRequested");
    assert_eq!(events[0].metadata["requestId"], request);
    assert_eq!(events[0].metadata["connectionId"], connection);
    assert_eq!(events[0].metadata["resourceId"], "resource-1");
    assert_eq!(events[0].metadata["namespace"], "account");
    assert!(events[0].bytes.is_none());
    assert_eq!(app.session.documents().len(), 1);
}

#[test]
fn provider_recent_command_is_disabled_by_default_and_refusal_remains_in_app_status() {
    let mut app = app();
    app.file_providers.disconnect().unwrap();
    assert!(!crate::menus::enabled(&app, "file.openProviderResource"));
    let catalog = control(&mut app, "engine.commands", json!({}));
    let command = catalog["result"].as_array().unwrap().iter().find(|row| row["id"] == "file.openProviderResource").unwrap();
    assert_eq!(command["enabled"], false);
    let response = control(
        &mut app,
        "engine.execute",
        json!({"command":"file.openProviderResource","params":{"connectionId":"stale","resourceId":"resource-1"}}),
    );
    assert_eq!(response["ok"], false);
    assert_eq!(app.ui.status, response["error"].as_str().unwrap());
    let error = app.ui.status.clone();
    tick(&mut app);
    assert_eq!(app.ui.status, error);
    assert!(app.file_providers.take_events().is_empty());
    assert_eq!(app.session.documents().len(), 1);
    // The ordinary local command remains an engine command with its original path contract.
    assert!(designcraft_engine::find_command("file.open").unwrap().params.contains("path"));
}

#[test]
fn an_old_recent_choice_cannot_open_the_same_resource_id_in_a_new_account() {
    let mut app = app();
    let old_connection = provider_rows(&app);
    app.file_providers.disconnect().unwrap();
    app.file_providers.connect("test".into(), "Other account".into(), "account-b".into()).unwrap();
    let current_connection = provider_rows(&app);
    assert_ne!(old_connection, current_connection);
    let stale = app.run("file.openProviderResource", json!({"connectionId":old_connection,"resourceId":"resource-1"})).unwrap_err();
    assert_eq!(app.ui.status, stale);
    tick(&mut app);
    assert_eq!(app.ui.status, stale);
    assert!(app.file_providers.take_events().is_empty());
    app.run("file.openProviderResource", json!({"connectionId":current_connection,"resourceId":"resource-1"})).unwrap();
    let events = app.file_providers.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].metadata["namespace"], "account-b");
}

#[test]
fn malformed_unknown_and_backpressured_recent_requests_report_persistent_errors_without_fallback() {
    let mut app = app();
    let connection = provider_rows(&app);
    for params in [
        json!({}),
        json!({"connectionId":connection}),
        json!({"connectionId":17,"resourceId":"resource-1"}),
        json!({"connectionId":connection,"resourceId":"unknown"}),
    ] {
        let error = app.run("file.openProviderResource", params).unwrap_err();
        assert_eq!(app.ui.status, error);
        assert!(app.file_providers.take_events().is_empty());
    }
    for _ in 0..16 {
        app.run("file.openProviderResource", json!({"connectionId":connection,"resourceId":"resource-1"})).unwrap();
    }
    let error = app.run("file.openProviderResource", json!({"connectionId":connection,"resourceId":"resource-1"})).unwrap_err();
    assert_eq!(app.ui.status, error);
    assert_eq!(app.file_providers.take_events().len(), 16);
    assert_eq!(app.session.documents().len(), 1);
}

#[test]
fn failed_delivery_uses_the_real_consumer_preserves_project_resources_and_checkpoint_and_cancels_close() {
    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#2468ac"/></svg>"##;
    for failed_event in ["saveDialog", "save"] {
        let mut app = app();
        app.session.execute("file.place", &json!({"name":"Original.svg","base64":designcraft_engine::cmd::base64_encode(SVG)})).unwrap();
        let original = app.session.active().unwrap().clone();
        let uid = original.uid;
        let value = save(&mut app, false, Some(After::Close(uid))).unwrap();
        let failed_request = value["requestId"].as_str().unwrap().to_string();
        if failed_event == "save" {
            app.file_providers.take_events();
            app.file_providers.resolve_dialog(&failed_request, "provider", "Captured.designcraft").unwrap();
        }
        let doc = app.session.active_mut().unwrap();
        std::sync::Arc::make_mut(&mut doc.doc).title = "Newer layout survives".into();
        doc.bump_revision();
        let newer = doc.clone();
        let host = app.file_providers.clone();
        assert!(host.publish_events_with(
            |metadata, bytes| {
                assert_eq!(metadata["type"], failed_event);
                assert_eq!(metadata["requestId"], failed_request);
                if let Some(bytes) = bytes {
                    let restored = designcraft_engine::cmd::from_bytes(&bytes).unwrap();
                    assert_eq!(serde_json::to_value(&restored).unwrap(), serde_json::to_value(original.doc.as_ref()).unwrap());
                    let asset = restored.assets.values().find(|asset| asset.name == "Original.svg").expect("restored SVG missing");
                    assert_eq!(asset.data.embedded_bytes().expect("restored SVG must be embedded").as_slice(), SVG);
                }
                Err("injected delivery failure")
            },
            |_| {},
        ));
        assert!(host.pending_for(uid));
        tick(&mut app);
        let current = app.session.active().unwrap();
        assert_eq!(current.uid, uid);
        assert!(std::sync::Arc::ptr_eq(&current.doc, &newer.doc));
        assert!(std::sync::Arc::ptr_eq(&current.saved_doc, &original.saved_doc));
        assert_eq!(current.saved_revision, original.saved_revision);
        assert_eq!(current.revision, newer.revision);
        assert!(current.is_dirty());
        assert!(current.path.is_none());
        assert!(app.provider_after.is_empty(), "failed choice delivery must not leave an orphaned close continuation");
        assert!(app.quitting.is_none());
        assert!(!host.pending_for(uid));
        assert_eq!(host.bound_document("not-persisted"), None);
        assert!(host.snapshot()["resources"].as_array().unwrap().is_empty());
        assert!(host.result(failed_request.clone(), "saved".into(), "late".into()).is_err());
        let mut failed = 0;
        assert!(host.publish_events_with(
            |metadata, _| {
                assert_eq!(metadata["type"], "saveFailed");
                assert_eq!(metadata["requestId"], failed_request);
                assert_eq!(metadata["documentId"], uid.to_string());
                failed += 1;
                Ok::<_, ()>(())
            },
            |_| {},
        ));
        assert_eq!(failed, 1);
        let retry = request(&mut app);
        host.resolve_dialog(&retry, "provider", "Retry.designcraft").unwrap();
        let event = host.take_events().remove(0);
        assert_eq!(event.metadata["filename"], "Retry.designcraft");
        let restored = designcraft_engine::cmd::from_bytes(&event.bytes.unwrap()).unwrap();
        assert_eq!(serde_json::to_value(&restored).unwrap(), serde_json::to_value(newer.doc.as_ref()).unwrap());
        let asset = restored.assets.values().find(|asset| asset.name == "Original.svg").expect("restored SVG missing");
        assert_eq!(asset.data.embedded_bytes().expect("restored SVG must be embedded").as_slice(), SVG);
        host.result(retry, "saved".into(), "retry-resource".into()).unwrap();
        tick(&mut app);
        assert_eq!(host.bound_document("retry-resource"), Some(uid));
        assert!(!app.session.active().unwrap().is_dirty());
    }
}

#[test]
fn backpressured_failed_read_cannot_open_a_document_from_a_later_callback_and_fresh_retry_decodes() {
    let mut app = app();
    let original = app.session.active().unwrap().clone();
    let serialized = app.session.execute("file.serialize", &json!({})).unwrap();
    let bytes = designcraft_engine::cmd::base64_decode(serialized["base64"].as_str().unwrap());
    let host = app.file_providers.clone();
    let connection = host.connection().unwrap();
    host.set_resources(
        &connection,
        vec![crate::file_providers::Resource { id: "remote".into(), name: "Remote.designcraft".into(), updated_at: String::new() }],
    )
    .unwrap();
    let failed = host.request_open("remote").unwrap();
    let later = host.request_open("remote").unwrap();
    let mut late_response_accepted = None;
    assert!(host.publish_events_with(
        |metadata, _| {
            if metadata["type"] == "openRequested" && metadata["requestId"] == failed {
                fill_event_queue(&host);
                Err("request dispatch failed")
            } else {
                if metadata["type"] == "openRequested" && metadata["requestId"] == later {
                    late_response_accepted = Some(host.open(failed.clone(), "Late.designcraft".into(), bytes.clone()).is_ok());
                } else {
                    assert!(matches!(metadata["type"].as_str(), Some("saveDialog" | "saveCancelled")));
                }
                Ok(())
            }
        },
        |_| {},
    ));
    // Do not drain after the late callback: the production consumer must see any illicit Open.
    tick(&mut app);
    assert_eq!(late_response_accepted, Some(false));
    assert_eq!(app.session.documents().len(), 1);
    let current = app.session.active().unwrap();
    assert_eq!(current.uid, original.uid);
    assert!(std::sync::Arc::ptr_eq(&current.doc, &original.doc));
    assert!(std::sync::Arc::ptr_eq(&current.saved_doc, &original.saved_doc));
    assert_eq!(current.revision, original.revision);
    assert_eq!(current.saved_revision, original.saved_revision);
    assert_eq!(host.bound_document("remote"), None);
    let mut failures = 0;
    assert!(host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["type"], "openFailed");
            assert_eq!(metadata["requestId"], failed);
            assert_eq!(metadata["resourceId"], "remote");
            failures += 1;
            Ok::<_, ()>(())
        },
        |_| {},
    ));
    assert_eq!(failures, 1);
    host.cancel_open(later.clone(), String::new()).unwrap();
    assert!(host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["type"], "openCancelled");
            assert_eq!(metadata["requestId"], later);
            Ok::<_, ()>(())
        },
        |_| {},
    ));
    let retry = host.request_open("remote").unwrap();
    assert_ne!(retry, failed);
    assert!(host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["type"], "openRequested");
            assert_eq!(metadata["requestId"], retry);
            host.open(retry.clone(), "Retry.designcraft".into(), bytes.clone()).unwrap();
            Ok::<_, ()>(())
        },
        |_| {},
    ));
    tick(&mut app);
    assert_eq!(app.session.documents().len(), 2);
    let opened = app.session.active().unwrap();
    assert_ne!(opened.uid, original.uid);
    let mut expected = original.doc.as_ref().clone();
    expected.title = "Retry".into();
    assert_eq!(serde_json::to_value(opened.doc.as_ref()).unwrap(), serde_json::to_value(expected).unwrap());
    assert_eq!(host.bound_document("remote"), Some(opened.uid));
    assert!(host.publish_events_with(
        |metadata, _| {
            assert_eq!(metadata["type"], "opened");
            assert_eq!(metadata["requestId"], retry);
            assert_eq!(metadata["resourceId"], "remote");
            Ok::<_, ()>(())
        },
        |_| {},
    ));
    assert!(!host.busy());
}
