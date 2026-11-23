//! Window › Type & Tables › Story: the story-wide options.

use serde_json::{Value, json};

use crate::DesignApp;
use crate::widgets::{self, NumField};

// STORY_OPTICAL_MARGIN_APPLICATION_BEGIN
/// The option is story-wide: changing the flag, text range or document revision must not
/// steal focus between successive keyboard toggles. Actual document epochs and target
/// story/frame identities still retire input when its owner changes.
#[cfg(not(target_arch = "wasm32"))]
fn show_framework(app: &mut DesignApp, ui: &mut egui::Ui) {
    use craft_ui_widgets::choice::Checkbox;
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _};
    use craft_ui_widgets::layout_components::Row;
    use craft_ui_widgets::text::NumberField;

    let options = super::text_attrs(app).map(|a| a["storyOptions"].clone());
    let enabled = options.is_some();
    let options = options.unwrap_or(Value::Null);
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let owner = app.session.active().map(|st| {
        (
            st.uid,
            st.identity_epoch,
            st.selection.text.map(|selection| selection.story.0),
            st.selection.cells.map(|selection| selection.story.0),
            st.selection.items.to_vec(),
        )
    });
    // Stories that differ show unchecked, not as a dash; a click turns it on for all of them.
    let on = options["opticalMargin"].as_bool().unwrap_or(false);
    let size = options["opticalMarginSize"].as_f64();
    let mut changes: Vec<Value> = Vec::new();
    Row::new().id_salt("story_options").wrap(true).show(ui, |ui, _| {
        let out = Checkbox::new(Bind::given(on))
            .id(ui.make_persistent_id(("story-optical-margin", &owner)))
            .label(tr("Optical Margin Alignment"))
            .enabled(enabled)
            .show(ui);
        if let Some(after) = out.events.committed() {
            changes.push(json!({"opticalMargin": after}));
        }
        let label = tr("Optical Margin Size");
        let out = NumberField::new(label.clone(), Bind::given(size))
            .id_salt("story_optical_margin_size")
            .unit("pt".to_owned())
            .range(designcraft_doc::OpticalMargin::MIN_SIZE..=designcraft_doc::OpticalMargin::MAX_SIZE)
            .step(1.0)
            .tooltip(label)
            .enabled(enabled)
            .show(ui);
        if let Some(value) = out.edit.events.committed().copied().flatten().filter(|v| v.is_finite()) {
            changes.push(json!({"opticalMarginSize": value}));
        }
    });
    for change in changes {
        let _ = app.run("story.options", change);
    }
}
// STORY_OPTICAL_MARGIN_APPLICATION_END

/// The Story panel: Optical Margin Alignment for the stories of the text selection or the
/// selected text frames, and the type size its hang amounts are set for. Without text or a text
/// frame selected, the controls are disabled.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        show_framework(app, ui);
        return;
    }
    let options = super::text_attrs(app).map(|a| a["storyOptions"].clone());
    let enabled = options.is_some();
    let options = options.unwrap_or(Value::Null);
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal_wrapped(|ui| {
            // Stories that differ show unchecked; a click turns it on for all of them.
            let mut on = options["opticalMargin"].as_bool().unwrap_or(false);
            if widgets::checkbox(ui, &mut on, &tr("Optical Margin Alignment")).changed() {
                let _ = app.run("story.options", json!({"opticalMargin": on}));
            }
            let field = NumField::number("storyOpticalMarginSize", options["opticalMarginSize"].as_f64(), " pt", 2)
                .width(72.0)
                .range(designcraft_doc::OpticalMargin::MIN_SIZE, designcraft_doc::OpticalMargin::MAX_SIZE);
            if let Some(size) = crate::chrome::icon_field(ui, "font-size", &tr("Optical Margin Size"), field) {
                let _ = app.run("story.options", json!({"opticalMarginSize": size}));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    use super::*;

    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(egui::vec2(260.0, 200.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        h
    }

    fn optical_margin(h: &Harness<'static, DesignApp>, story: u64) -> designcraft_doc::OpticalMargin {
        h.state().session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().optical_margin
    }

    fn size_field_text(h: &Harness<'static, DesignApp>) -> Option<String> {
        h.get_all_by_role(egui::accesskit::Role::TextInput).next().unwrap().value()
    }

    #[test]
    fn story_panel_sets_and_shows_the_storys_optical_margin() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let story =
            app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "“Hung”", "caret": true})).unwrap()["story"]
                .as_u64()
                .unwrap();
        app.run("edit.selectAll", json!({})).unwrap();
        let mut h = harness(app);
        assert_eq!(size_field_text(&h).as_deref(), Some("12 pt"));
        h.get_by_label("Optical Margin Alignment").click();
        h.run_steps(3);
        assert!(optical_margin(&h, story).on);
        assert_eq!(h.get_by_label("Optical Margin Alignment").accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
        h.get_all_by_role(egui::accesskit::Role::TextInput).next().unwrap().click();
        h.run_steps(2);
        h.event(egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(egui::Event::Text("18".into()));
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert_eq!(optical_margin(&h, story), designcraft_doc::OpticalMargin { on: true, size: 18.0 });
        // A value set elsewhere shows in the panel.
        h.state_mut().run("story.options", json!({"opticalMarginSize": 30})).unwrap();
        h.run_steps(3);
        assert_eq!(size_field_text(&h).as_deref(), Some("30 pt"));
    }

    #[test]
    fn story_panel_is_disabled_without_text() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut h = harness(app);
        assert!(h.get_by_label("Optical Margin Alignment").accesskit_node().is_disabled());
        h.get_by_label("Optical Margin Alignment").click();
        h.run_steps(3);
        assert_eq!(h.get_by_label("Optical Margin Alignment").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod optical_margin_sdk_tests {
    use super::*;
    use designcraft_doc::StoryId;
    use designcraft_engine::{DocState, HistoryEntry};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::sync::Arc;

    const NAME: &str = "Optical Margin Alignment";
    const REGION: &str = "story_optical_margin_application_region";

    struct State {
        app: DesignApp,
        disabled: bool,
        measure_only: bool,
        visible: bool,
    }
    struct Made {
        frames: [u64; 3],
        stories: [u64; 3],
    }
    fn populate(app: &mut DesignApp) -> Made {
        let mut made = Made { frames: [0; 3], stories: [0; 3] };
        for index in 0..3 {
            let y = 72 + index * 180;
            let frame = app
                .run(
                    "frame.create",
                    json!({
                        "rect": [72, y, 300, y + 140], "content": "text",
                        "text": format!(r#""Story {}" with punctuation and independent sizing."#, index),
                    }),
                )
                .unwrap();
            made.frames[index] = frame["id"].as_u64().unwrap();
            made.stories[index] = frame["story"].as_u64().unwrap();
            app.run(
                "story.options",
                json!({"story": made.stories[index],
                "opticalMargin": index == 2, "opticalMarginSize": ([12.125, 18.75, 7.875][index])}),
            )
            .unwrap();
        }
        select_text(app, made.stories[0]);
        made
    }
    fn fixture() -> (DesignApp, Made) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let made = populate(&mut app);
        // A genuine unrelated edit and Undo leave a nonempty Redo to protect until activation.
        app.run("story.options", json!({"story": made.stories[2], "opticalMargin": false})).unwrap();
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.active().unwrap().history.redo.len(), 1);
        (app, made)
    }
    fn select_text(app: &mut DesignApp, story: u64) {
        app.run("text.select", json!({"story": story, "anchor": 1, "focus": 6})).unwrap();
    }
    fn label(app: &DesignApp) -> String {
        crate::i18n::tr(&app.ui.language, NAME).to_owned()
    }
    // STORY_ARABIC_FIXTURE_BEGIN
    /// A genuine installed face is a localized test precondition, not a production font fix.
    /// Keep the ordinary installed definitions and missing-glyph policy. The face is added
    /// before the first Story draw or user input through the app's baseline-aware API.
    fn install_arabic_fixture(ctx: &egui::Context, language: &str) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the native Arabic fixture requires the already-installed {family}; do not skip or substitute tofu");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family), "the named fixture must resolve to {family}, not {}", face.family);
        assert!(!face.is_placeholder(), "the Arabic fixture must be a genuine face");
        #[cfg(target_os = "macos")]
        assert_eq!(face.index(), 0, "the independently identified Geeza Pro Regular is TTC face 0");
        for c in ['\u{0627}', '\u{0625}', '\u{0637}', '\u{0639}'] {
            assert!(face.covers(c), "{family} lacks the real witness glyph U+{:04X}", u32::from(c));
        }
        // Check the entire original translations, not only the witness characters. Missing
        // translation keys deliberately retain the app's English fallback, never invented Arabic.
        let mut covered = 0;
        for key in [NAME, "Optical Margin Size"] {
            for c in crate::i18n::tr(language, key).chars().filter(|c| designcraft_fonts::is_rtl(*c) && c.is_alphabetic()) {
                assert!(face.covers(c), "{family} lacks the original Story glyph U+{:04X} from {key}", u32::from(c));
                covered += 1;
            }
        }
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Story Arabic fixture: existing {} {}, face {}, {} font bytes; {} original RTL alphabetic occurrences checked",
            face.family,
            face.style,
            face.index(),
            face.data().len(),
            covered
        );
    }
    // STORY_ARABIC_FIXTURE_END

    fn harness(mut app: DesignApp, installed: bool, width: f32) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed);
        let mut ready = false;
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(width, 220.0)).build_ui_state(
            move |ui, state: &mut State| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                // STORY_ARABIC_BOOTSTRAP_BEGIN
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if state.app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &state.app.ui.language);
                        return;
                    }
                }
                // STORY_ARABIC_BOOTSTRAP_END
                let _ = craft_ui_widgets::runtime::inspect::record(ui, egui::Id::NULL.with(REGION), |ui| {
                    if !state.visible {
                        return;
                    }
                    ui.add_enabled_ui(!state.disabled, |ui| {
                        let builder = egui::UiBuilder::new().id_salt("story_application_parent");
                        let builder = if state.measure_only { builder.sizing_pass() } else { builder };
                        ui.scope_builder(builder, |ui| {
                            if state.measure_only {
                                assert!(ui.is_sizing_pass());
                            }
                            show(&mut state.app, ui);
                        });
                    });
                });
            },
            State { app, disabled: false, measure_only: false, visible: true },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }
    fn frame(h: &mut Harness<'_, State>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            frame(h, None);
        }
    }
    fn key_event(h: &mut Harness<'_, State>, key: Key, pressed: bool) {
        frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
    }
    fn key(h: &mut Harness<'_, State>, key: Key) {
        key_event(h, key, true);
        key_event(h, key, false);
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, State>, pos: egui::Pos2, pressed: bool) {
        frame(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn click(h: &mut Harness<'_, State>) {
        let name = label(&h.state().app);
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::CheckBox, &name).count(), 1);
        let pos = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, &name).rect().center();
        frame(h, Some(Event::PointerMoved(pos)));
        pointer(h, pos, true);
        pointer(h, pos, false);
        idle(h, 2);
    }
    fn focused(h: &Harness<'_, State>) {
        let name = label(&h.state().app);
        let target = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, &name);
        assert!(target.is_focused(), "the actual current named checkbox has AX focus");
        let id = target.accesskit_node().id();
        let actual: Vec<_> = h.query_all_by(|node| node.is_focused()).map(|node| node.accesskit_node().id()).collect();
        assert_eq!(actual, vec![id], "one real semantic AX owner, never a native arena ID assumption");
    }
    fn tab_to(h: &mut Harness<'_, State>) {
        let name = label(&h.state().app);
        let stops = h
            .query_all_by(|node| {
                matches!(
                    node.role(),
                    egui::accesskit::Role::Button
                        | egui::accesskit::Role::CheckBox
                        | egui::accesskit::Role::ComboBox
                        | egui::accesskit::Role::TextInput
                        | egui::accesskit::Role::SpinButton
                        | egui::accesskit::Role::RadioButton
                )
            })
            .count();
        for _ in 0..=stops {
            if h.get_by_role_and_label(egui::accesskit::Role::CheckBox, &name).is_focused() {
                focused(h);
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach the current complete Story checkbox name");
    }
    fn control(h: &Harness<'_, State>) -> craft_ui_widgets::runtime::inspect::Element {
        let name = label(&h.state().app);
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
        let matches: Vec<_> = record
            .elements
            .iter()
            .filter(|element| {
                element.component == "Checkbox" && element.facts.as_ref().and_then(|facts| facts.name.as_deref()) == Some(name.as_str())
            })
            .collect();
        assert_eq!(matches.len(), 1, "one actual SDK Checkbox with the complete translated name");
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::CheckBox, &name).count(), 1);
        matches[0].clone()
    }
    #[derive(Clone)]
    struct Quiet {
        docs: Vec<DocState>,
        active: Option<usize>,
        prefs: Value,
        journal: Vec<(String, Value)>,
    }
    fn quiet(app: &DesignApp) -> Quiet {
        Quiet {
            docs: app.session.documents().to_vec(),
            active: app.session.active_index(),
            prefs: serde_json::to_value(&app.session.prefs).unwrap(),
            journal: app.session.journal.clone(),
        }
    }
    fn same_entries(now: &[HistoryEntry], before: &[HistoryEntry]) {
        assert_eq!(now.len(), before.len());
        for (now, before) in now.iter().zip(before) {
            assert_eq!(now.label, before.label);
            assert_eq!(now.selection, before.selection);
            assert_eq!(now.doc.as_ref(), before.doc.as_ref());
            assert!(Arc::ptr_eq(&now.doc, &before.doc));
        }
    }
    fn same_doc(now: &DocState, before: &DocState) {
        assert_eq!(format!("{now:?}"), format!("{before:?}"), "complete authoritative DocState, not just the toggle");
        assert!(Arc::ptr_eq(&now.doc, &before.doc));
        assert!(Arc::ptr_eq(&now.saved_doc, &before.saved_doc));
        same_entries(&now.history.undo, &before.history.undo);
        same_entries(&now.history.redo, &before.history.redo);
        assert_eq!(now.is_dirty(), before.is_dirty());
    }
    fn assert_quiet(app: &DesignApp, before: &Quiet) {
        assert_eq!(app.session.active_index(), before.active);
        assert_eq!(app.session.documents().len(), before.docs.len());
        for (now, before) in app.session.documents().iter().zip(&before.docs) {
            same_doc(now, before);
        }
        assert_eq!(serde_json::to_value(&app.session.prefs).unwrap(), before.prefs);
        assert_eq!(app.session.journal, before.journal, "no added command journal entries, including after idle");
    }
    fn assert_one_edit(app: &DesignApp, before: &Quiet, targets: &[u64], on: bool) {
        assert_eq!(app.session.active_index(), before.active);
        assert_eq!(app.session.documents().len(), before.docs.len());
        assert_eq!(serde_json::to_value(&app.session.prefs).unwrap(), before.prefs);
        assert_eq!(&app.session.journal[..before.journal.len()], before.journal.as_slice());
        assert_eq!(&app.session.journal[before.journal.len()..], &[("story.options".to_owned(), json!({"opticalMargin": on}))]);
        let active = before.active.unwrap();
        for (index, (now, old)) in app.session.documents().iter().zip(&before.docs).enumerate() {
            if index != active {
                same_doc(now, old);
                continue;
            }
            let mut expected = old.clone();
            let mut doc = (*old.doc).clone();
            for target in targets {
                let story = doc.story_mut(StoryId(*target)).unwrap();
                if story.optical_margin.on != on {
                    story.optical_margin.on = on;
                    story.rev = story.rev.wrapping_add(1);
                }
            }
            assert_ne!(&doc, old.doc.as_ref(), "the real action changes a targeted story");
            assert_eq!(now.doc.as_ref(), &doc, "only target story flags/revisions change: full sizes/text/frames/styles/tables remain exact");
            assert!(!Arc::ptr_eq(&now.doc, &old.doc));
            assert!(now.revision > old.revision, "revision IDs are process-global, not a local +1 counter");
            expected.doc = Arc::new(doc);
            expected.revision = now.revision;
            expected.history.undo.push(HistoryEntry { label: "Story Options".into(), doc: old.doc.clone(), selection: old.selection.clone() });
            expected.history.redo.clear();
            assert_eq!(format!("{now:?}"), format!("{expected:?}"));
            same_entries(&now.history.undo[..old.history.undo.len()], &old.history.undo);
            let entry = now.history.undo.last().unwrap();
            assert!(Arc::ptr_eq(&entry.doc, &old.doc));
            assert_eq!(entry.selection, old.selection);
            assert_eq!(entry.label, "Story Options");
            assert!(now.history.redo.is_empty());
            assert!(Arc::ptr_eq(&now.saved_doc, &old.saved_doc));
            assert!(now.is_dirty());
        }
    }
    fn undo_redo(h: &mut Harness<'_, State>, before: &Quiet) {
        let active = before.active.unwrap();
        let after = quiet(&h.state().app);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(h, 2);
        let now = h.state().app.session.active().unwrap();
        assert_eq!(now.doc.as_ref(), before.docs[active].doc.as_ref());
        assert!(Arc::ptr_eq(&now.doc, &before.docs[active].doc));
        assert_eq!(now.selection, before.docs[active].selection);
        same_entries(&now.history.undo, &before.docs[active].history.undo);
        assert_eq!(now.history.redo.len(), 1);
        assert!(Arc::ptr_eq(&now.history.redo[0].doc, &after.docs[active].doc));
        assert!(Arc::ptr_eq(&now.saved_doc, &before.docs[active].saved_doc));
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(h, 2);
        let now = h.state().app.session.active().unwrap();
        assert!(Arc::ptr_eq(&now.doc, &after.docs[active].doc));
        assert_eq!(now.doc.as_ref(), after.docs[active].doc.as_ref());
        assert_eq!(now.selection, after.docs[active].selection);
        same_entries(&now.history.undo, &after.docs[active].history.undo);
        assert!(now.history.redo.is_empty());
        for (index, doc) in h.state().app.session.documents().iter().enumerate() {
            if index != active {
                same_doc(doc, &before.docs[index]);
            }
        }
        let stable = quiet(&h.state().app);
        idle(h, 3);
        assert_quiet(&h.state().app, &stable);
    }

    #[test]
    fn ui_story_optical_margin_pointer_targets_text_frames_mixed_stories_and_table_owner_with_exact_undo() {
        for mode in ["text", "frame", "mixed", "cells"] {
            let (mut app, made) = fixture();
            let mut targets = vec![made.stories[0]];
            match mode {
                "frame" => {
                    app.run("selection.set", json!({"ids": [made.frames[0]]})).unwrap();
                }
                "mixed" => {
                    app.run("story.options", json!({"story": made.stories[1], "opticalMargin": true})).unwrap();
                    app.run("selection.set", json!({"ids": [made.frames[0], made.frames[1]]})).unwrap();
                    targets.push(made.stories[1]);
                    assert!(app.session.execute("type.selectionAttrs", &json!({})).unwrap()["storyOptions"]["opticalMargin"].is_null());
                }
                "cells" => {
                    app.run("text.select", json!({"story": made.stories[0], "anchor": 0, "focus": 0})).unwrap();
                    let table = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
                    app.run("table.setCell", json!({"row": 0, "col": 0, "text": "independent cell text"})).unwrap();
                    app.run("table.select", json!({"story": table["story"], "table": table["table"], "rows": [0, 1], "cols": [0, 1]})).unwrap();
                }
                "text" => {}
                _ => unreachable!(),
            }
            let mut h = harness(app, true, 460.0);
            assert!(!control(&h).facts.unwrap().disabled);
            assert_eq!(
                h.get_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).accesskit_node().toggled(),
                Some(egui::accesskit::Toggled::False)
            );
            for on in [true, false] {
                let before = quiet(&h.state().app);
                click(&mut h);
                assert_one_edit(&h.state().app, &before, &targets, on);
                assert_eq!(
                    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).accesskit_node().toggled(),
                    Some(if on { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False })
                );
                undo_redo(&mut h, &before);
            }
        }
    }

    #[test]
    fn ui_story_optical_margin_natural_tab_space_enter_preserves_focus_and_independent_history_for_repeated_toggles() {
        let (app, made) = fixture();
        let mut h = harness(app, true, 460.0);
        let before_focus = quiet(&h.state().app);
        tab_to(&mut h);
        assert_quiet(&h.state().app, &before_focus);
        let owner = control(&h).id;
        for (key_code, on) in [(Key::Space, true), (Key::Space, false), (Key::Enter, true), (Key::Enter, false)] {
            focused(&h);
            let before = quiet(&h.state().app);
            key(&mut h, key_code);
            assert_one_edit(&h.state().app, &before, &[made.stories[0]], on);
            assert_eq!(control(&h).id, owner, "a value or revision change is not a new story owner");
            focused(&h);
            let stable = quiet(&h.state().app);
            idle(&mut h, 3);
            assert_quiet(&h.state().app, &stable);
        }
        let after = quiet(&h.state().app);
        for _ in 0..4 {
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 2);
        }
        let now = h.state().app.session.active().unwrap();
        assert!(Arc::ptr_eq(&now.doc, &before_focus.docs[before_focus.active.unwrap()].doc));
        same_entries(&now.history.undo, &before_focus.docs[before_focus.active.unwrap()].history.undo);
        assert_eq!(now.history.redo.len(), 4);
        for _ in 0..4 {
            h.state_mut().app.run("edit.redo", json!({})).unwrap();
            idle(&mut h, 2);
        }
        let now = h.state().app.session.active().unwrap();
        assert!(Arc::ptr_eq(&now.doc, &after.docs[after.active.unwrap()].doc));
        same_entries(&now.history.undo, &after.docs[after.active.unwrap()].history.undo);
        assert!(now.history.redo.is_empty());
    }

    #[test]
    fn ui_story_optical_margin_held_pointer_retires_changed_story_document_epoch_and_disabled_or_missing_parent_before_recovery() {
        for change in ["story", "document", "epoch", "no-selection", "parent-disabled", "hidden"] {
            let (app, mut made) = fixture();
            let mut h = harness(app, true, 460.0);
            let old_owner = control(&h).id;
            let pos = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).rect().center();
            let initial = quiet(&h.state().app);
            frame(&mut h, Some(Event::PointerMoved(pos)));
            pointer(&mut h, pos, true);
            assert_quiet(&h.state().app, &initial);
            match change {
                "story" => select_text(&mut h.state_mut().app, made.stories[1]),
                "document" => {
                    h.state_mut().app.run("file.new", json!({})).unwrap();
                    made = populate(&mut h.state_mut().app);
                }
                "epoch" => {
                    let old = h.state().app.session.active().unwrap();
                    let mut replacement = DocState::new((*old.doc).clone(), old.path.clone());
                    replacement.uid = old.uid;
                    replacement.selection = old.selection.clone();
                    assert_ne!(replacement.identity_epoch, old.identity_epoch, "real constructor-assigned epoch");
                    *h.state_mut().app.session.active_mut().unwrap() = replacement;
                }
                "no-selection" => {
                    h.state_mut().app.run("selection.set", json!({"ids": []})).unwrap();
                }
                "parent-disabled" => h.state_mut().disabled = true,
                "hidden" => h.state_mut().visible = false,
                _ => unreachable!(),
            }
            let retired = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            match change {
                "hidden" => assert_eq!(
                    h.query_all_by(|node| node.role() == egui::accesskit::Role::CheckBox && node.label().as_deref() == Some(NAME)).count(),
                    0
                ),
                "parent-disabled" => assert!(control(&h).facts.unwrap().disabled),
                _ => assert_ne!(control(&h).id, old_owner, "actual new owner, not a row ordinal"),
            }
            pointer(&mut h, pos, false);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            h.state_mut().disabled = false;
            h.state_mut().visible = true;
            if change == "no-selection" {
                select_text(&mut h.state_mut().app, made.stories[0]);
            }
            let recovered = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &recovered);
            assert!(!control(&h).facts.unwrap().disabled);
            let target = if change == "story" { made.stories[1] } else { made.stories[0] };
            click(&mut h);
            assert_one_edit(&h.state().app, &recovered, &[target], true);
            undo_redo(&mut h, &recovered);
        }
    }

    #[test]
    fn ui_story_optical_margin_old_space_release_never_activates_a_new_story_or_recreated_document() {
        for change in ["story", "epoch", "hidden"] {
            let (app, made) = fixture();
            let mut h = harness(app, true, 460.0);
            tab_to(&mut h);
            let initial = quiet(&h.state().app);
            key_event(&mut h, Key::Space, true);
            // SDK12 routes the complete press intent at key-down, not an invented release phase.
            assert_one_edit(&h.state().app, &initial, &[made.stories[0]], true);
            match change {
                "story" => select_text(&mut h.state_mut().app, made.stories[1]),
                "epoch" => {
                    let old = h.state().app.session.active().unwrap();
                    let mut replacement = DocState::new((*old.doc).clone(), old.path.clone());
                    replacement.uid = old.uid;
                    replacement.selection = old.selection.clone();
                    assert_ne!(replacement.identity_epoch, old.identity_epoch);
                    *h.state_mut().app.session.active_mut().unwrap() = replacement;
                }
                "hidden" => h.state_mut().visible = false,
                _ => unreachable!(),
            }
            let retired = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            key_event(&mut h, Key::Space, false);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            h.state_mut().visible = true;
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            let before_focus = quiet(&h.state().app);
            tab_to(&mut h);
            assert_quiet(&h.state().app, &before_focus);
            let target = if change == "story" { made.stories[1] } else { made.stories[0] };
            let on = !h.state().app.session.active().unwrap().doc.story(StoryId(target)).unwrap().optical_margin.on;
            key(&mut h, Key::Space);
            assert_one_edit(&h.state().app, &before_focus, &[target], on);
            undo_redo(&mut h, &before_focus);
        }
    }

    #[test]
    fn ui_story_optical_margin_real_no_document_no_selection_and_disabled_parent_are_quiet_and_native_fallback_recovers() {
        for installed in [true, false] {
            for absent in ["no-document", "no-selection", "parent-disabled"] {
                let app = if absent == "no-document" {
                    DesignApp::new(designcraft_engine::Session::new(), crate::Services::default())
                } else {
                    fixture().0
                };
                let mut h = harness(app, installed, 460.0);
                if absent == "no-selection" {
                    h.state_mut().app.run("selection.set", json!({"ids": []})).unwrap();
                }
                if absent == "parent-disabled" {
                    h.state_mut().disabled = true;
                }
                let before = quiet(&h.state().app);
                idle(&mut h, 2);
                assert_quiet(&h.state().app, &before);
                assert!(h.get_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).accesskit_node().is_disabled());
                assert_eq!(h.get_all_by_role(egui::accesskit::Role::TextInput).count(), 1);
                assert!(h.get_all_by_role(egui::accesskit::Role::TextInput).all(|node| node.accesskit_node().is_disabled()));
                click(&mut h);
                key(&mut h, Key::Space);
                assert_quiet(&h.state().app, &before);
                h.state_mut().disabled = false;
                if h.state().app.session.active().is_none() {
                    h.state_mut().app.run("file.new", json!({})).unwrap();
                }
                let made = populate(&mut h.state_mut().app);
                let recovered = quiet(&h.state().app);
                idle(&mut h, 2);
                assert_quiet(&h.state().app, &recovered);
                assert_eq!(h.state().app.framework_installed, installed, "no forged runtime-install flag");
                if installed {
                    assert!(!control(&h).facts.unwrap().disabled);
                } else {
                    let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
                    assert_eq!(record.elements.iter().filter(|element| element.component == "Checkbox").count(), 0);
                }
                click(&mut h);
                assert_one_edit(&h.state().app, &recovered, &[made.stories[0]], true);
                undo_redo(&mut h, &recovered);
            }
        }
    }

    #[test]
    fn ui_story_optical_margin_real_sizing_and_missing_parent_publish_no_live_owner_or_delayed_edit() {
        for mode in ["sizing", "hidden"] {
            let (app, made) = fixture();
            let mut h = harness(app, true, 460.0);
            tab_to(&mut h);
            let pos = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).rect().center();
            let initial = quiet(&h.state().app);
            frame(&mut h, Some(Event::PointerMoved(pos)));
            pointer(&mut h, pos, true);
            assert_quiet(&h.state().app, &initial);
            h.state_mut().measure_only = mode == "sizing";
            h.state_mut().visible = mode != "hidden";
            let before = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &before);
            assert_eq!(
                h.query_all_by(|node| node.role() == egui::accesskit::Role::CheckBox && node.label().as_deref() == Some(NAME)).count(),
                0,
                "no actual AX owner during sizing or absence"
            );
            let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
            assert!(record.hits.is_empty(), "real noninteractive parent registers no SDK hit target");
            assert!(record.elements.iter().all(|element| element.stop.is_none()), "no SDK focus stop");
            pointer(&mut h, pos, false);
            key(&mut h, Key::Space);
            assert_quiet(&h.state().app, &before);
            h.state_mut().measure_only = false;
            h.state_mut().visible = true;
            idle(&mut h, 3);
            assert_quiet(&h.state().app, &before);
            assert!(!control(&h).facts.unwrap().disabled);
            tab_to(&mut h);
            assert_quiet(&h.state().app, &before);
            key(&mut h, Key::Space);
            assert_one_edit(&h.state().app, &before, &[made.stories[0]], true);
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn ui_story_optical_margin_external_truth_full_localized_name_and_narrow_parent_leave_size_and_document_exact() {
        for language in ["en", "de", "ar"] {
            let (mut app, made) = fixture();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true, 460.0);
            let name = label(&h.state().app);
            let owner = control(&h).id;
            for external in [true, false] {
                h.state_mut()
                    .app
                    .run(
                        "story.options",
                        json!({"story": made.stories[0],
                    "opticalMargin": external, "opticalMarginSize": 42.875}),
                    )
                    .unwrap();
                let before = quiet(&h.state().app);
                idle(&mut h, 3);
                assert_quiet(&h.state().app, &before);
                let actual = control(&h);
                assert_eq!(actual.id, owner, "same logical target; no coarse revision-key focus churn");
                assert_eq!(actual.facts.as_ref().unwrap().name.as_deref(), Some(name.as_str()));
                assert_eq!(
                    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, &name).accesskit_node().toggled(),
                    Some(if external { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False })
                );
                let field = h.get_all_by_role(egui::accesskit::Role::TextInput).next().unwrap();
                assert_eq!(field.value().as_deref(), Some("42.88 pt"), "unchanged real point field still formats the authoritative size");
                click(&mut h);
                assert_one_edit(&h.state().app, &before, &[made.stories[0]], !external);
                undo_redo(&mut h, &before);
            }
            let (mut app, _) = fixture();
            app.ui.language = language.to_owned();
            let mut narrow = harness(app, true, 150.0);
            let before = quiet(&narrow.state().app);
            let actual = control(&narrow);
            assert_eq!(actual.facts.unwrap().name.as_deref(), Some(label(&narrow.state().app).as_str()));
            assert!(actual.rect.is_finite());
            idle(&mut narrow, 3);
            assert_quiet(&narrow.state().app, &before);
        }
    }

    #[test]
    fn ui_story_optical_margin_whole_window_panel_routes_the_actual_checkbox_to_only_its_story_and_undo() {
        let (mut app, made) = fixture();
        app.framework_opacity = true;
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        let ctx = h.ctx.clone();
        h.state_mut().app.install_framework(&ctx);
        assert!(h.state().app.framework_installed);
        h.state_mut().app.run("window.panel", json!({"panel": "story"})).unwrap();
        fn step(h: &mut Harness<'_, crate::test_window::Window>, event: Option<Event>) {
            let mut raw = std::mem::take(h.input_mut());
            if let Some(event) = event {
                raw.events.push(event);
            }
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
        for _ in 0..3 {
            step(&mut h, None);
        }
        assert_eq!(h.state().app.session.active().unwrap().selection.text.unwrap().story, StoryId(made.stories[0]));
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).count(), 1);
        assert!(!h.get_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).accesskit_node().is_disabled());
        let before = quiet(&h.state().app);
        let pos = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, NAME).rect().center();
        step(&mut h, Some(Event::PointerMoved(pos)));
        for pressed in [true, false] {
            step(&mut h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
        }
        for _ in 0..3 {
            step(&mut h, None);
        }
        assert_one_edit(&h.state().app, &before, &[made.stories[0]], true);
        let stable = quiet(&h.state().app);
        for _ in 0..3 {
            step(&mut h, None);
        }
        assert_quiet(&h.state().app, &stable);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        for _ in 0..3 {
            step(&mut h, None);
        }
        let now = h.state().app.session.active().unwrap();
        let old = &before.docs[before.active.unwrap()];
        assert!(Arc::ptr_eq(&now.doc, &old.doc));
        assert_eq!(now.doc.as_ref(), old.doc.as_ref());
        assert_eq!(now.selection, old.selection);
        same_entries(&now.history.undo, &old.history.undo);
        assert!(Arc::ptr_eq(&now.saved_doc, &old.saved_doc));
    }

    struct OwnedDocumentDirectory(std::path::PathBuf);
    impl OwnedDocumentDirectory {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("design-story-optical-margin-{}-{stamp}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for OwnedDocumentDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn ui_story_optical_margin_actual_native_save_reopen_keeps_both_flag_values_and_exact_persistent_document() {
        let owned = OwnedDocumentDirectory::new();
        let path = owned.0.join("Story optical margin.designcraft");
        let path = path.to_str().unwrap().to_owned();
        let (app, made) = fixture();
        let mut h = harness(app, true, 460.0);
        h.state_mut().app.session.execute("file.save", &json!({"path": path})).unwrap();
        assert!(std::path::Path::new(&path).is_file());
        idle(&mut h, 2);
        assert!(!h.state().app.session.active().unwrap().is_dirty());
        for on in [true, false] {
            let before = quiet(&h.state().app);
            click(&mut h);
            assert_one_edit(&h.state().app, &before, &[made.stories[0]], on);
            let edited = quiet(&h.state().app);
            let expected = serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap();
            let saved = h.state_mut().app.session.execute("file.save", &json!({"path": path})).unwrap();
            assert_eq!(saved["path"].as_str(), Some(path.as_str()));
            assert!(saved["bytes"].as_u64().unwrap() > 0);
            idle(&mut h, 2);
            let st = h.state().app.session.active().unwrap();
            assert!(!st.is_dirty());
            assert!(Arc::ptr_eq(&st.doc, &st.saved_doc));
            assert_eq!(st.saved_revision, st.revision);
            assert_eq!(serde_json::to_value(st.doc.as_ref()).unwrap(), expected);
            same_entries(&st.history.undo, &edited.docs[edited.active.unwrap()].history.undo);
            assert!(st.history.redo.is_empty());
            let mut reopened = designcraft_engine::Session::new();
            reopened.execute("file.open", &json!({"path": path})).unwrap();
            let restored = reopened.active().unwrap();
            assert_eq!(
                serde_json::to_value(restored.doc.as_ref()).unwrap(),
                expected,
                "all persistent fields use the real native encoder/decoder; runtime-only font scopes are not normalized manually"
            );
            assert_eq!(restored.doc.story(StoryId(made.stories[0])).unwrap().optical_margin.on, on);
            assert_eq!(restored.doc.story(StoryId(made.stories[0])).unwrap().optical_margin.size, 12.125);
            assert!(!restored.is_dirty());
            let after_save = quiet(&h.state().app);
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            idle(&mut h, 2);
            let st = h.state().app.session.active().unwrap();
            assert!(Arc::ptr_eq(&st.doc, &before.docs[before.active.unwrap()].doc));
            assert!(Arc::ptr_eq(&st.saved_doc, &after_save.docs[after_save.active.unwrap()].saved_doc));
            assert!(st.is_dirty());
            h.state_mut().app.run("edit.redo", json!({})).unwrap();
            idle(&mut h, 2);
            let st = h.state().app.session.active().unwrap();
            assert!(Arc::ptr_eq(&st.doc, &after_save.docs[after_save.active.unwrap()].doc));
            assert!(!st.is_dirty());
            let stable = quiet(&h.state().app);
            idle(&mut h, 3);
            assert_quiet(&h.state().app, &stable);
        }
    }
}
