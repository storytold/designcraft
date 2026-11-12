//! Real NumField input at the provider ACK boundary; no storage or browser is simulated here.
#![allow(clippy::unwrap_used)]
use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct Window {
    app: DesignApp,
    show_field: bool,
    seed_focus: bool,
    commit: bool,
    run_completion: bool,
    value: f64,
    key: Option<egui::Id>,
}

fn window(app: DesignApp) -> Harness<'static, Window> {
    let mut harness = Harness::builder().with_size(egui::vec2(300.0, 140.0)).build_ui_state(
        |ui, state: &mut Window| {
            let ctx = ui.ctx().clone();
            state.app.egui_ctx = Some(ctx.clone());
            tick(&mut state.app);
            observe_ui(&state.app, &ctx);
            if state.show_field {
                let original = ui.scope_id().with("providerDraft");
                let key = drafts::field_key(ui, original).unwrap();
                state.key = Some(key);
                if std::mem::take(&mut state.seed_focus) {
                    ui.memory_mut(|memory| memory.request_focus(key.with("te")));
                }
                if let Some(value) = crate::widgets::NumField::number("providerDraft", Some(state.value), "", 0).show(ui) {
                    state.value = value;
                    if state.commit {
                        let doc = state.app.session.active_mut().unwrap();
                        std::sync::Arc::make_mut(&mut doc.doc).title = format!("Committed {value}");
                        doc.bump_revision();
                    }
                }
            }
            if state.run_completion {
                settled(&mut state.app, &ctx);
            }
        },
        Window { app, show_field: true, seed_focus: true, commit: true, run_completion: false, value: 10.0, key: None },
    );
    harness.run_steps(2);
    harness
}

fn app() -> DesignApp {
    let mut session = designcraft_engine::Session::new();
    session.execute("file.new", &json!({})).unwrap();
    DesignApp::new(session, crate::Services::default())
}

fn connect(app: &DesignApp) {
    app.file_providers.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
    app.file_providers.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
}

fn choose_close(app: &mut DesignApp) -> (u64, String) {
    let uid = app.session.active().unwrap().uid;
    let result = save(app, false, Some(After::Close(uid))).unwrap();
    let request = result["requestId"].as_str().unwrap().to_string();
    app.file_providers.take_events();
    app.file_providers.resolve_dialog(&request, "provider", "Captured.designcraft").unwrap();
    app.file_providers.take_events();
    (uid, request)
}

fn buffer(harness: &Harness<'static, Window>, key: egui::Id) -> String {
    harness.ctx.data(|data| data.get_temp::<String>(key)).unwrap()
}

#[test]
fn opting_in_and_disconnecting_preserve_a_focused_local_numeric_draft_and_its_id() {
    let mut h = window(app());
    let local_key = h.state().key.unwrap();
    h.event(egui::Event::Text("7".into()));
    h.run_steps(1);
    let draft = buffer(&h, local_key);
    assert!(draft.contains('7'));
    assert_eq!(h.state().value, 10.0);
    connect(&h.state().app);
    h.run_steps(1);
    assert_eq!(h.state().key, Some(local_key));
    assert_eq!(buffer(&h, local_key), draft);
    h.state().app.file_providers.set_enabled(false).unwrap();
    h.run_steps(1);
    assert_eq!(h.state().key, Some(local_key));
    h.state().app.file_providers.disconnect().unwrap();
    h.run_steps(1);
    assert_eq!(h.state().key, Some(local_key));
    assert_eq!(buffer(&h, local_key), draft);
    assert_eq!(h.state().value, 10.0);
    assert!(guard_command(&h.state().app, "file.close", &json!({})).is_err());
    assert!(guard_command(&h.state().app, "file.revert", &json!({})).is_err());
}

#[test]
fn raw_numeric_input_in_the_ack_frame_keeps_a_open_even_before_engine_revision_changes() {
    let mut app = app();
    connect(&app);
    let (uid, request) = choose_close(&mut app);
    let mut h = window(app);
    let revision = h.state().app.session.active().unwrap().revision;
    h.state().app.file_providers.result(request, "saved".into(), "a".into()).unwrap();
    h.state_mut().run_completion = true;
    h.event(egui::Event::Text("7".into()));
    h.run_steps(1);
    let doc = h.state().app.session.active().unwrap();
    assert_eq!(doc.uid, uid);
    assert_eq!(doc.revision, revision);
    assert!(!doc.is_dirty());
    assert_eq!(h.state().value, 10.0);
    assert!(h.state().app.provider_after.values().any(|after| after.uid == uid));
    assert!(drafts::pending(&h.ctx, (doc.uid, doc.identity_epoch)));
}

#[test]
fn b_numeric_draft_does_not_block_a_typed_close_or_get_retargeted_to_a() {
    let mut app = app();
    connect(&app);
    let (a, request) = choose_close(&mut app);
    app.run("file.new", json!({})).unwrap();
    let b = app.session.active().unwrap().uid;
    let mut h = window(app);
    h.state().app.file_providers.result(request, "saved".into(), "a".into()).unwrap();
    h.state_mut().run_completion = true;
    h.event(egui::Event::Text("7".into()));
    h.run_steps(1);
    assert!(!h.state().app.session.documents().iter().any(|doc| doc.uid == a));
    let doc = h.state().app.session.active().unwrap();
    assert_eq!(doc.uid, b);
    assert!(drafts::pending(&h.ctx, (doc.uid, doc.identity_epoch)));
    assert!(buffer(&h, h.state().key.unwrap()).contains('7'));
    assert_eq!(h.state().value, 10.0);
    assert!(guard_command(&h.state().app, "file.close", &json!({})).is_err());
}

#[test]
fn a_numeric_draft_stays_owned_by_a_when_b_is_drawn_with_the_same_field_id() {
    let app = app();
    connect(&app);
    let a = app.session.active().unwrap().uid;
    let mut h = window(app);
    h.event(egui::Event::Text("7".into()));
    h.run_steps(1);
    let a_key = h.state().key.unwrap();
    let a_buffer = buffer(&h, a_key);
    h.state_mut().app.run("file.new", json!({})).unwrap();
    let b = h.state().app.session.active().unwrap().uid;
    h.state_mut().seed_focus = true;
    h.run_steps(1);
    let b_key = h.state().key.unwrap();
    assert_ne!(a_key, b_key);
    assert_eq!(h.state().value, 10.0);
    assert_eq!(buffer(&h, a_key), a_buffer);
    let a_doc = h.state().app.session.documents().iter().find(|doc| doc.uid == a).unwrap();
    assert!(drafts::pending(&h.ctx, (a_doc.uid, a_doc.identity_epoch)));
    let b_doc = h.state().app.session.documents().iter().find(|doc| doc.uid == b).unwrap();
    assert!(!drafts::pending(&h.ctx, (b_doc.uid, b_doc.identity_epoch)));
    assert!(guard_command(&h.state().app, "file.close", &json!({"index":0})).is_err());
    assert!(guard_command(&h.state().app, "file.close", &json!({"index":1})).is_ok());
}

#[test]
fn hidden_numeric_draft_can_be_discarded_for_its_document_even_after_disconnect() {
    let app = app();
    connect(&app);
    let a = app.session.active().unwrap().uid;
    let mut h = window(app);
    h.event(egui::Event::Text("7".into()));
    h.run_steps(1);
    let a_key = h.state().key.unwrap();
    let a_buffer = buffer(&h, a_key);
    let a_revision = h.state().app.session.active().unwrap().revision;
    h.state_mut().app.run("file.new", json!({})).unwrap();
    let b = h.state().app.session.active().unwrap().uid;
    h.state_mut().seed_focus = true;
    h.run_steps(1);
    h.event(egui::Event::Text("8".into()));
    h.run_steps(1);
    let b_key = h.state().key.unwrap();
    let b_buffer = buffer(&h, b_key);
    assert_ne!(a_key, b_key);
    assert!(a_buffer.contains('7') && b_buffer.contains('8'));

    h.state_mut().show_field = false;
    h.state_mut().app.run("file.activate", json!({"index":0})).unwrap();
    h.state().app.file_providers.disconnect().unwrap();
    h.run_steps(2);
    assert_eq!(buffer(&h, a_key), a_buffer);
    let error = h.state_mut().app.run("file.close", json!({})).unwrap_err();
    assert_eq!(h.state().app.ui.status, error);
    assert!(error.contains("Discard Numeric Drafts"));
    assert!(crate::menus::menu_enabled(&h.state().app, "edit.discardNumericDrafts"));
    assert_eq!(h.state_mut().app.run("edit.discardNumericDrafts", json!({})).unwrap()["discarded"], 1);
    assert!(h.ctx.data(|data| data.get_temp::<String>(a_key)).is_none());
    assert_eq!(buffer(&h, b_key), b_buffer);
    let doc = h.state().app.session.active().unwrap();
    assert_eq!((doc.uid, doc.revision), (a, a_revision));
    assert!(!crate::menus::menu_enabled(&h.state().app, "edit.discardNumericDrafts"));
    h.state_mut().app.run("file.close", json!({"index":0})).unwrap();
    assert_eq!(h.state().app.session.active().unwrap().uid, b);
    assert_eq!(buffer(&h, b_key), b_buffer);
}

#[test]
fn start_screen_provider_recents_keep_the_next_section_in_reach_and_scroll_to_the_last_project() {
    let app = app();
    connect(&app);
    let host = app.file_providers.clone();
    let connection = host.connection().unwrap();
    host.set_resources(
        &connection,
        (0..100)
            .map(|index| crate::file_providers::Resource {
                id: format!("project-{index}"),
                name: format!("Project {index:02}.designcraft"),
                updated_at: String::new(),
            })
            .collect(),
    )
    .unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(500.0, 320.0)).build_ui_state(
        move |ui, choice: &mut Option<crate::file_providers::RecentChoice>| {
            if let Some(clicked) = host.start_screen_recents(ui, "") {
                *choice = Some(clicked);
            }
            ui.label("Document presets");
        },
        None,
    );
    h.run_steps(3);
    let first = h.get_by_label("Project 00.designcraft").rect();
    let presets = h.get_by_label("Document presets").rect();
    let device = h.get_by_label("Recent files on this device").rect();
    assert!(presets.bottom() < 320.0 && device.top() < presets.top());
    h.event(egui::Event::PointerMoved(first.center()));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -4000.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
        source: egui::MouseWheelSource::Unknown,
    });
    h.run_steps(20);
    let last = h.get_by_label("Project 99.designcraft");
    assert!(last.rect().bottom() < device.top());
    last.click();
    h.run_steps(2);
    let choice = h.state().as_ref().unwrap();
    assert_eq!((choice.connection_id.as_str(), choice.resource_id.as_str()), (connection.as_str(), "project-99"));
}
