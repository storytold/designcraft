//! Real installed-component interactions; run serially (`--test-threads=1`).
//! `data.preview` is a non-journaled query. Its successful application replaces the
//! document and allocates one global revision. In these quiescent, single-document
//! fixtures revision deltas witness successful applications, not failed/raw attempts.

use super::*;
use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
use egui::{Event, Key, Modifiers, PointerButton, UiBuilder, vec2};
use egui_kittest::Harness;
use std::collections::HashSet;
use std::sync::Arc;

struct Window {
    app: DesignApp,
    controls: Option<Arc<Recorded>>,
    enabled: bool,
    sizing: bool,
}

struct Effects {
    revision: u64,
    undo: usize,
    redo: usize,
    journal: Vec<(String, Value)>,
    document: Arc<designcraft_doc::Document>,
    saved: Arc<designcraft_doc::Document>,
    template: Option<Arc<designcraft_doc::Document>>,
}

fn step(h: &mut Harness<'_, Window>) {
    let mut raw = std::mem::take(h.input_mut());
    let ctx = h.ctx.clone();
    h.state_mut().app.raw_input_hook(&ctx, &mut raw);
    *h.input_mut() = raw;
    h.step();
}

fn idle(h: &mut Harness<'_, Window>, count: usize) {
    for _ in 0..count {
        step(h);
    }
}

fn event(h: &mut Harness<'_, Window>, event: Event) {
    h.input_mut().events.push(event);
    step(h);
}

fn key(h: &mut Harness<'_, Window>, key: Key) {
    for pressed in [true, false] {
        event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    idle(h, 2);
}

fn button(h: &Harness<'_, Window>, label: &str) -> Element {
    let name = tr(&h.state().app.ui.language, label);
    let found: Vec<_> = h
        .state()
        .controls
        .as_ref()
        .unwrap()
        .elements
        .iter()
        .filter(|element| element.facts.as_ref().is_some_and(|facts| facts.role == "Button" && facts.name.as_deref() == Some(name)))
        .cloned()
        .collect();
    assert_eq!(found.len(), 1, "one installed SDK Button named {label}, not an egui fallback or a caption");
    found[0].clone()
}

fn click(h: &mut Harness<'_, Window>, element: &Element) {
    let pos = element.rect.center();
    event(h, Event::PointerMoved(pos));
    for pressed in [true, false] {
        event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    idle(h, 3);
}

fn tab_to(h: &mut Harness<'_, Window>, target: &Element) {
    let mut visited = HashSet::new();
    loop {
        let focused = h.ctx.memory(|memory| memory.focused());
        if focused == Some(target.id) {
            return;
        }
        assert!(visited.insert(focused), "natural Tab did not reach the observed Data Merge button {:?}", target.facts);
        key(h, Key::Tab);
    }
}

fn effects(h: &Harness<'_, Window>) -> Effects {
    let app = &h.state().app;
    let st = app.session.active().unwrap();
    Effects {
        revision: st.revision,
        undo: st.history.undo.len(),
        redo: st.history.redo.len(),
        journal: app.session.journal.clone(),
        document: st.doc.clone(),
        saved: st.saved_doc.clone(),
        template: st.preview_stash.clone(),
    }
}

fn assert_effects(h: &Harness<'_, Window>, before: &Effects, record: Option<u32>, applications: u64) {
    let app = &h.state().app;
    let st = app.session.active().unwrap();
    assert_eq!(st.preview_record, record, "the actual engine preview selected the expected record");
    // Revisions come from one process-wide counter that other tests running in parallel also advance, so a step is
    // checked as "no preview" (the revision is exactly unchanged) or "one preview" (it moved). Every caller checks a
    // single action, and the idle-frame checks keep the "exactly once" part.
    assert!(applications <= 1, "one action per check");
    if applications == 0 {
        assert_eq!(st.revision, before.revision, "no successful preview, including across idle frames");
    } else {
        assert!(st.revision > before.revision, "one successful preview");
    }
    assert_eq!((st.history.undo.len(), st.history.redo.len()), (before.undo, before.redo), "preview never changes document undo/redo history");
    assert_eq!(app.session.journal, before.journal, "preview/fields queries are not journaled commands");
    // No placeholders/images: preview can change its session record without editing
    // template content, the linked CSV, saved state or document history.
    assert_eq!(st.doc.as_ref(), before.document.as_ref());
    assert_eq!(st.saved_doc.as_ref(), before.saved.as_ref());
    assert_eq!(st.preview_stash.as_deref(), before.template.as_deref());
    assert!(st.path.is_none());
    assert_eq!(app.file_dialogs.queued(), 0, "no navigation gesture asks for native IO");
    assert!(!app.file_dialogs_pending());
}

fn fixture(preview: bool) -> Harness<'static, Window> {
    let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
    // Install a refusing/canceling fake BEFORE any frames; never open native UI.
    app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
    app.run("file.new", json!({})).unwrap();
    app.run(
        "data.source.select",
        json!({
            "bytes": designcraft_engine::cmd::base64_encode(b"Name\nAda\nGrace\nKatherine\n"),
            "name": "people.csv"
        }),
    )
    .unwrap();
    assert_eq!(app.session.active().unwrap().doc.data_merge.sources[0].rows.len(), 3);
    assert!(app.session.active().unwrap().doc.data_merge.placeholders.is_empty());
    if preview {
        app.run("data.preview", json!({"record": 2})).unwrap();
    }
    app.framework_opacity = true;
    let ctx = egui::Context::default();
    app.install_framework(&ctx);
    assert!(app.framework_installed);
    app.raw_input_hook(&ctx, &mut egui::RawInput::default());
    let mut ready = false;
    let mut h = Harness::builder().with_context(ctx).with_size(vec2(540.0, 820.0)).build_ui_state(
        move |ui, state: &mut Window| {
            if !ready {
                crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                ready = true;
                return;
            }
            let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
            ui.scope_builder(builder, |ui| {
                ui.add_enabled_ui(state.enabled, |ui| {
                    state.controls = record(ui, egui::Id::unique("datamerge-preview-port-tests"), |ui| show(&mut state.app, ui)).1;
                });
            });
        },
        Window { app, controls: None, enabled: true, sizing: false },
    );
    h.input_mut().max_texture_side = Some(8192);
    idle(&mut h, 4);
    let ids: HashSet<_> = ["First", "Previous", "Next", "Last"].map(|name| button(&h, name).id).into_iter().collect();
    assert_eq!(ids.len(), 4, "four genuinely distinct installed controls");
    h
}

#[test]
fn installed_datamerge_preview_pointer_actions_apply_exact_records_once_without_history() {
    let mut h = fixture(true);
    for (name, record, applications) in
        [("Next", 3, 1), ("Last", 3, 0), ("Previous", 2, 1), ("First", 1, 1), ("Previous", 1, 0), ("Last", 3, 1), ("Next", 3, 0)]
    {
        let target = button(&h, name);
        assert!(!target.facts.as_ref().unwrap().disabled);
        let before = effects(&h);
        click(&mut h, &target);
        assert_effects(&h, &before, Some(record), applications);
        idle(&mut h, 5);
        assert_effects(&h, &before, Some(record), applications);
    }
}

#[test]
fn installed_datamerge_preview_natural_tab_space_enter_preserve_query_policy() {
    let mut h = fixture(true);
    for (name, activation, record) in [("First", Key::Space, 1), ("Next", Key::Enter, 2), ("Last", Key::Space, 3), ("Previous", Key::Enter, 2)] {
        let before = effects(&h);
        let current = h.state().app.session.active().unwrap().preview_record;
        let target = button(&h, name);
        event(&mut h, Event::PointerMoved(target.rect.center()));
        idle(&mut h, 3);
        tab_to(&mut h, &target);
        assert_effects(&h, &before, current, 0);
        key(&mut h, activation);
        assert_effects(&h, &before, Some(record), 1);
        idle(&mut h, 5);
        assert_effects(&h, &before, Some(record), 1);
    }
}

#[test]
fn installed_datamerge_preview_disabled_and_sizing_passes_never_defer_actions() {
    let mut off = fixture(false);
    let before = effects(&off);
    for name in ["First", "Previous", "Next", "Last"] {
        let target = button(&off, name);
        assert!(target.facts.as_ref().unwrap().disabled, "preview-off disables every navigation control");
        click(&mut off, &target);
        assert_effects(&off, &before, None, 0);
    }
    drop(off);

    let mut h = fixture(true);
    let next = button(&h, "Next");
    tab_to(&mut h, &next);
    let before = effects(&h);
    h.state_mut().enabled = false;
    idle(&mut h, 2);
    for name in ["First", "Previous", "Next", "Last"] {
        let target = button(&h, name);
        assert!(target.facts.as_ref().unwrap().disabled, "SDK controls inherit the enclosing disabled scope");
        click(&mut h, &target);
    }
    key(&mut h, Key::Enter);
    key(&mut h, Key::Space);
    assert_effects(&h, &before, Some(2), 0);

    h.state_mut().enabled = true;
    idle(&mut h, 2);
    let next = button(&h, "Next");
    tab_to(&mut h, &next);
    h.state_mut().sizing = true;
    click(&mut h, &next);
    key(&mut h, Key::Enter);
    key(&mut h, Key::Space);
    assert_effects(&h, &before, Some(2), 0);
    h.state_mut().sizing = false;
    idle(&mut h, 6); // Same fixture: a deferred sizing activation must not fire now.
    assert_effects(&h, &before, Some(2), 0);
    assert!(!button(&h, "Next").facts.as_ref().unwrap().disabled);
    let next = button(&h, "Next");
    click(&mut h, &next); // Negative controls did not leave the real button inert.
    assert_effects(&h, &before, Some(3), 1);
}
