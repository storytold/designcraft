//! Conditional Text panel: conditions with their visibility and indicator colour; click a name to
//! apply it to the selected text (the box shows whether the selection has it).

use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

/// Click on a condition name: toggle it on the selected text, or tell the user to select some.
fn apply_condition(app: &mut DesignApp, name: &str, has_text: bool, applied: &[String]) {
    if has_text {
        let on = !applied.iter().any(|a| a == name);
        if let Err(e) = app.run("condition.apply", json!({"name": name, "on": on})) {
            app.status_error("Conditional Text", e);
        }
    } else {
        app.status(crate::i18n::tr(&app.ui.language, "Select text to apply a condition"));
    }
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    show_framework(app, ui);
    // Room under the last row, so the panel does not end at the window's edge.
    ui.add_space(8.0);
}

/// App-owned mapping only: the framework supplies input and focus; the engine still owns eligibility,
/// text-range interpretation, refusals and Undo. Condition names are identities, never row indices.
fn show_framework(app: &mut DesignApp, ui: &mut egui::Ui) {
    use craft_ui_widgets::buttons::Button;
    use craft_ui_widgets::choice::Checkbox;
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::{Picture, Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::menu::{Menu, MenuItem};
    use craft_ui_widgets::primitives::HAlign;
    use craft_ui_widgets::style::TypeRole;
    use craft_ui_widgets::text::TextField;

    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let identity = (st.uid, st.identity_epoch);
    let conditions = st.doc.conditions.clone();
    let has_text = st.selection.text.is_some();
    let applied: Vec<String> = super::text_attrs(app)
        .and_then(|a| a["chars"]["conditions"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()))
        .unwrap_or_default();
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let name_label = tr("New condition name");
    let mut commands: Vec<(&'static str, serde_json::Value)> = Vec::new();
    let mut apply: Option<String> = None;
    // The new-condition draft lives with the document it is typed in, so another document never shows or creates it.
    let draft_id = ui.make_persistent_id(("framework-conditions", identity, "new_condition_name"));
    let mut draft: String = ui.data(|d| d.get_temp(draft_id)).unwrap_or_default();
    let mut create = false;
    // Document-scoped ids keep a draft and focused control from becoming another document's control.
    ui.push_id(("framework-conditions", identity), |ui| {
        Stack::new().id_salt("conditions").fill_width().show(ui, |ui, _| {
            if conditions.is_empty() {
                Text::new(tr("No conditions. Create one, then select text and click it to apply."))
                    .role(TypeRole::Caption)
                    .shade(Shade::Muted)
                    .sizing(Sizing::FillWidth)
                    .show(ui);
            }
            for c in &conditions {
                let marked = applied.contains(&c.name);
                let (dim, strong, chip) = (t.text_dim, t.text_strong, egui::Color32::from_rgb(c.color[0], c.color[1], c.color[2]));
                Row::new().id_salt(("cond_row", &c.name)).fill_width().show(ui, |ui, _| {
                    let visibility_label = tr("Show / hide text with this condition");
                    // The box alone: the row's name button carries the condition's name, so the checkbox's own label
                    // (its accessible name) has no room to show.
                    let (_, slot) = ui.allocate_space(egui::vec2(20.0, 22.0));
                    let visibility = Checkbox::new(Bind::given(c.visible))
                        .id_salt(("cond_eye", &c.name))
                        .label(c.name.clone())
                        .description(visibility_label.clone())
                        .tooltip(visibility_label)
                        .show_at(ui, egui::Rect::from_center_size(slot.center(), egui::vec2(16.0, 16.0)));
                    if let Some(&visible) = visibility.events.committed().filter(|&&v| v != c.visible) {
                        commands.push(("condition.options", json!({"name": c.name, "visible": visible})));
                    }
                    // The existing applied-state indicator, not a second activation target.
                    Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                        let bx = rect.shrink(rect.height() * 0.2);
                        cx.painter.rect_stroke(bx, 2.0, egui::Stroke::new(1.0, dim), egui::StrokeKind::Inside);
                        if marked {
                            let down = egui::vec2(0.0, bx.height() * 0.25);
                            cx.painter
                                .line_segment([bx.left_center() + egui::vec2(2.0, 0.0), bx.center_bottom() - down], egui::Stroke::new(1.5, strong));
                            cx.painter
                                .line_segment([bx.center_bottom() - down, bx.right_top() + egui::vec2(-2.0, 3.0)], egui::Stroke::new(1.5, strong));
                        }
                    })
                    .id_salt(("cond_applied", &c.name))
                    .show(ui);
                    let name = Button::new(c.name.clone()).id_salt(("cond_name", &c.name)).align(HAlign::Start).sizing(Sizing::FillWidth).show(ui);
                    if name.clicked() {
                        apply = Some(c.name.clone());
                    }
                    let _ = Menu::context(tr("Conditional Text"), &name.response).id_salt(("cond_menu", &c.name)).show(ui, |ui, _| {
                        if MenuItem::new(tr("Delete Condition")).id_salt("delete_condition").show(ui).ran {
                            commands.push(("condition.delete", json!({"name": c.name})));
                        }
                    });
                    Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                        cx.painter.rect_filled(egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 12.0)), 2.0, chip);
                    })
                    .id_salt(("cond_chip", &c.name))
                    .show(ui);
                });
            }
            Row::new().id_salt("new_condition").fill_width().show(ui, |ui, _| {
                // The old single-line editor has no 1024-character name limit.
                TextField::new(name_label.clone(), Bind::bound(&mut draft))
                    .id_salt("new_condition_name")
                    .placeholder(name_label.clone())
                    .limit(None)
                    .show(ui);
                create |= Button::new(tr("New")).id_salt("new_condition").show(ui).clicked();
            });
            if has_text && !applied.is_empty() && Button::new(tr("Remove Conditions from Selection")).id_salt("remove_conditions").show(ui).clicked()
            {
                commands.push(("condition.apply", json!({"name": null, "only": true})));
            }
        });
    });
    if let Some(name) = apply {
        apply_condition(app, &name, has_text, &applied);
    }
    for (id, params) in commands {
        let _ = app.run(id, params);
    }
    if create && !draft.trim().is_empty() {
        match app.run("condition.new", json!({"name": draft.trim()})) {
            Ok(_) => draft.clear(),
            Err(e) => app.status_error("Conditional Text", e),
        }
    }
    ui.data_mut(|d| d.insert_temp(draft_id, draft));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;
    use designcraft_doc::StoryId;
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    const TEXT: &str = "AéBC終Z";
    const REMOVE: &str = "Remove Conditions from Selection";

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app
    }

    fn text_app() -> (DesignApp, StoryId, StoryId) {
        let mut app = app();
        let first = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": TEXT})).unwrap();
        let second = app.run("frame.create", json!({"rect": [36, 220, 300, 400], "content": "text", "text": "Sentinel"})).unwrap();
        let story = StoryId(first["story"].as_u64().unwrap());
        let other = StoryId(second["story"].as_u64().unwrap());
        for name in ["EU", "EU-long"] {
            app.run("condition.new", json!({"name": name})).unwrap();
        }
        select(&mut app, story, 1, "AéBC".len());
        (app, story, other)
    }

    fn select(app: &mut DesignApp, story: StoryId, anchor: usize, focus: usize) {
        app.run("text.select", json!({"story": story.0, "anchor": anchor, "focus": focus})).unwrap();
    }

    fn harness(mut app: DesignApp, enabled: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(380.0, 440.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                ui.add_enabled_ui(enabled, |ui| show(app, ui));
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    // Feed each actual host event through the app hook before the pass. No requested focus,
    // fabricated SDK intent, setter-only assertion, delayed retry or widened harness timeout.
    fn event(h: &mut Harness<'_, DesignApp>, event: Event) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn steps(h: &mut Harness<'_, DesignApp>, n: usize) {
        for _ in 0..n {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn pointer(h: &mut Harness<'_, DesignApp>, at: egui::Pos2, button: PointerButton, pressed: bool) {
        event(h, Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE });
    }

    fn click_at(h: &mut Harness<'_, DesignApp>, at: egui::Pos2, button: PointerButton) {
        event(h, Event::PointerMoved(at));
        pointer(h, at, button, true);
        pointer(h, at, button, false);
        steps(h, 3);
    }

    fn click(h: &mut Harness<'_, DesignApp>, role: Role, label: &str) {
        let at = h.get_by_role_and_label(role, label).rect().center();
        click_at(h, at, PointerButton::Primary);
    }

    fn key(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        steps(h, 3);
    }

    fn type_draft(h: &mut Harness<'_, DesignApp>, text: &str) {
        // The SDK also exposes an unnamed IME proxy. Target the actual named field.
        let at = h.get_by_role_and_label(Role::TextInput, crate::i18n::tr(&h.state().ui.language, "New condition name")).rect().center();
        click_at(h, at, PointerButton::Primary);
        key(h, Key::A, Modifiers::COMMAND);
        for ch in text.chars() {
            event(h, Event::Text(ch.to_string()));
        }
        steps(h, 3);
    }

    fn draft(h: &Harness<'_, DesignApp>) -> String {
        h.get_by_role_and_label(Role::TextInput, crate::i18n::tr(&h.state().ui.language, "New condition name")).value().unwrap_or_default()
    }

    fn undo_steps(h: &Harness<'_, DesignApp>) -> usize {
        h.state().session.doc().unwrap().history.undo.len()
    }

    fn condition_commands(h: &Harness<'_, DesignApp>) -> Vec<(String, serde_json::Value)> {
        h.state().session.journal.iter().filter(|(id, _)| id.starts_with("condition.")).cloned().collect()
    }

    fn undo(h: &mut Harness<'_, DesignApp>) {
        h.state_mut().run("edit.undo", json!({})).unwrap();
        steps(h, 3);
    }

    fn assert_range(h: &Harness<'_, DesignApp>, story: StoryId, expected: &[&str]) {
        let story = h.state().session.doc().unwrap().doc.story(story).unwrap();
        assert_eq!(story.text, TEXT, "formatting must not replace the underlying text");
        let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        for (pos, _) in TEXT.char_indices() {
            let actual = story.char_format_of(pos).over.conditions.clone().unwrap_or_default();
            assert_eq!(actual, if (1.."AéBC".len()).contains(&pos) { expected.clone() } else { vec![] }, "byte {pos}");
        }
    }

    #[test]
    fn sdk_conditions_create_duplicate_blank_and_undo() {
        let mut h = harness(app(), true);
        let before = undo_steps(&h);
        let commands = condition_commands(&h);
        click(&mut h, Role::Button, "New");
        assert_eq!(condition_commands(&h), commands, "blank New is a UI no-op");
        assert_eq!(undo_steps(&h), before);
        type_draft(&mut h, "  Europe  ");
        assert_eq!(draft(&h), "  Europe  ");
        assert_eq!(undo_steps(&h), before, "typing the name is not a document edit");
        click(&mut h, Role::Button, "New");
        assert_eq!(draft(&h), "", "success clears the draft");
        assert_eq!(h.state().session.doc().unwrap().doc.conditions[0].name, "Europe");
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.new".into(), json!({"name": "Europe"})));
        assert_eq!(undo_steps(&h), before + 1);
        type_draft(&mut h, "Europe");
        let commands = condition_commands(&h);
        click(&mut h, Role::Button, "New");
        assert_eq!(draft(&h), "Europe", "a refused duplicate stays editable");
        assert!(h.state().ui.status.contains("exists"));
        assert_eq!(h.state().session.doc().unwrap().doc.conditions.len(), 1);
        assert_eq!(condition_commands(&h), commands, "refusal is not a successful command");
        assert_eq!(undo_steps(&h), before + 1);
        undo(&mut h);
        assert!(h.state().session.doc().unwrap().doc.conditions.is_empty());
        assert_eq!(undo_steps(&h), before);
    }

    #[test]
    fn sdk_conditions_apply_toggle_remove_exact_range_and_undo() {
        let (app, story, other) = text_app();
        let selection = app.session.doc().unwrap().selection.clone();
        let untouched = app.session.doc().unwrap().doc.story(other).unwrap().clone();
        let mut h = harness(app, true);
        let before = undo_steps(&h);
        click(&mut h, Role::Button, "EU");
        assert_range(&h, story, &["EU"]);
        assert_eq!(undo_steps(&h), before + 1);
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.apply".into(), json!({"name": "EU", "on": true})));
        click(&mut h, Role::Button, "EU-long");
        assert_range(&h, story, &["EU", "EU-long"]);
        click(&mut h, Role::Button, "EU");
        assert_range(&h, story, &["EU-long"]);
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.apply".into(), json!({"name": "EU", "on": false})));
        undo(&mut h);
        assert_range(&h, story, &["EU", "EU-long"]);
        let before_remove = undo_steps(&h);
        click(&mut h, Role::Button, REMOVE);
        assert_range(&h, story, &[]);
        assert_eq!(undo_steps(&h), before_remove + 1);
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.apply".into(), json!({"name": null, "only": true})));
        assert!(h.query_by_label(REMOVE).is_none(), "remove is absent when the selection has no conditions");
        undo(&mut h);
        assert_range(&h, story, &["EU", "EU-long"]);
        assert_eq!(h.state().session.doc().unwrap().selection, selection);
        assert_eq!(h.state().session.doc().unwrap().doc.story(other).unwrap(), &untouched, "the other story is untouched");
    }

    #[test]
    fn sdk_conditions_visibility_and_keyboard_are_independent_and_undoable() {
        let (app, story, _) = text_app();
        let original_conditions = app.session.doc().unwrap().doc.conditions.clone();
        let mut h = harness(app, true);
        click(&mut h, Role::Button, "EU");
        let before = undo_steps(&h);
        click(&mut h, Role::CheckBox, "EU");
        let conditions = &h.state().session.doc().unwrap().doc.conditions;
        assert!(!conditions[0].visible);
        assert_eq!(conditions[0].color, original_conditions[0].color);
        assert_eq!(conditions[1], original_conditions[1]);
        assert_range(&h, story, &["EU"]);
        assert_eq!(undo_steps(&h), before + 1);
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.options".into(), json!({"name": "EU", "visible": false})));
        // SDK checkbox/button presses deliberately release rather than take keyboard focus.
        // Natural Tab enters the first stop (this checkbox); Space must then reverse visibility.
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert!(h.get_by_role_and_label(Role::CheckBox, "EU").is_focused(), "Tab reaches the actual visibility control");
        key(&mut h, Key::Space, Modifiers::NONE);
        assert!(h.state().session.doc().unwrap().doc.conditions[0].visible);
        assert_eq!(undo_steps(&h), before + 2);
        assert_range(&h, story, &["EU"]);
        undo(&mut h);
        assert!(!h.state().session.doc().unwrap().doc.conditions[0].visible);
        undo(&mut h);
        assert_eq!(h.state().session.doc().unwrap().doc.conditions, original_conditions);
        // The name button also supports keyboard activation, with the same toggle command.
        click(&mut h, Role::Button, "EU");
        assert_range(&h, story, &[]);
        // A pointer activation does not establish a fresh keyboard traversal origin. Start from
        // the actual named field, whose ordinary pointer press takes focus, then traverse the
        // concrete preceding stops with Shift+Tab. Assert every stop rather than guessing where
        // two forward Tabs after the earlier checkbox/Undo sequence would start.
        click(&mut h, Role::TextInput, "New condition name");
        assert!(
            h.get_by_role_and_label(Role::TextInput, "New condition name").is_focused(),
            "the ordinary field click establishes the keyboard starting stop; focused widget: {:?}",
            h.ctx.memory(|m| m.focused())
        );
        for (role, label) in [(Role::Button, "EU-long"), (Role::CheckBox, "EU-long"), (Role::Button, "EU")] {
            key(&mut h, Key::Tab, Modifiers::SHIFT);
            assert!(
                h.get_by_role_and_label(role, label).is_focused(),
                "natural Shift+Tab reaches {role:?} {label}; focused widget: {:?}",
                h.ctx.memory(|m| m.focused())
            );
        }
        assert!(h.get_by_role_and_label(Role::Button, "EU").is_focused(), "natural Tab navigation reaches the name button");
        key(&mut h, Key::Space, Modifiers::NONE);
        assert_range(&h, story, &["EU"]);
    }

    #[test]
    fn sdk_conditions_no_text_feedback_and_caret_keep_existing_command_semantics() {
        let (mut app, story, _) = text_app();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = harness(app, true);
        let before = undo_steps(&h);
        let commands = condition_commands(&h);
        click(&mut h, Role::Button, "EU");
        assert_eq!(h.state().ui.status, "Select text to apply a condition");
        assert_eq!(condition_commands(&h), commands);
        assert_eq!(undo_steps(&h), before);
        assert!(h.query_by_label(REMOVE).is_none());
        select(h.state_mut(), story, 3, 3);
        steps(&mut h, 3);
        click(&mut h, Role::Button, "EU");
        assert_range(&h, story, &[]);
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.apply".into(), json!({"name": "EU", "on": true})));
        // The existing engine admits a caret, skips its empty range, and still records the edit.
        // The port must not change this into a new caret-formatting command or suppress its Undo.
        assert_eq!(undo_steps(&h), before + 1);
        undo(&mut h);
        assert_range(&h, story, &[]);
        assert_eq!(undo_steps(&h), before);
    }

    #[test]
    fn sdk_conditions_context_delete_preserves_text_and_undo_restores_identity() {
        let (app, story, _) = text_app();
        let mut h = harness(app, true);
        click(&mut h, Role::Button, "EU");
        let conditions = h.state().session.doc().unwrap().doc.conditions.clone();
        let before = undo_steps(&h);
        let at = h.get_by_role_and_label(Role::Button, "EU").rect().center();
        click_at(&mut h, at, PointerButton::Secondary);
        // The framework's context menu rows are menu items, not buttons.
        click(&mut h, Role::MenuItem, "Delete Condition");
        assert_eq!(h.state().session.doc().unwrap().doc.conditions, vec![conditions[1].clone()]);
        assert_range(&h, story, &[]);
        assert_eq!(undo_steps(&h), before + 1);
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.delete".into(), json!({"name": "EU"})));
        undo(&mut h);
        assert_eq!(h.state().session.doc().unwrap().doc.conditions, conditions);
        assert_range(&h, story, &["EU"]);
    }

    #[test]
    fn sdk_conditions_document_switch_scopes_drafts_and_rejects_old_pointer_release() {
        let (mut app, first, _) = text_app();
        app.run("file.new", json!({})).unwrap();
        let second = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": TEXT})).unwrap();
        let second = StoryId(second["story"].as_u64().unwrap());
        app.run("condition.new", json!({"name": "EU"})).unwrap();
        select(&mut app, second, 1, "AéBC".len());
        app.run("file.activate", json!({"index": 0})).unwrap();
        let mut h = harness(app, true);
        type_draft(&mut h, "First draft");
        let at = h.get_by_role_and_label(Role::Button, "EU").rect().center();
        event(&mut h, Event::PointerMoved(at));
        pointer(&mut h, at, PointerButton::Primary, true);
        h.state_mut().run("file.activate", json!({"index": 1})).unwrap();
        steps(&mut h, 3);
        assert_eq!(draft(&h), "", "the other document has a different draft");
        let commands = condition_commands(&h);
        pointer(&mut h, at, PointerButton::Primary, false);
        steps(&mut h, 3);
        assert_eq!(condition_commands(&h), commands, "an old document's press cannot activate this document's same-named row");
        assert_range(&h, second, &[]);
        type_draft(&mut h, "Second draft");
        click(&mut h, Role::Button, "EU");
        assert_range(&h, second, &["EU"]);
        h.state_mut().run("file.activate", json!({"index": 0})).unwrap();
        steps(&mut h, 3);
        assert_eq!(draft(&h), "First draft");
        assert_range(&h, first, &[]);
    }

    #[test]
    fn sdk_conditions_inherited_disabled_state_prevents_commands() {
        let (app, story, _) = text_app();
        let mut h = harness(app, false);
        let commands = condition_commands(&h);
        let before = undo_steps(&h);
        assert!(h.get_by_role_and_label(Role::Button, "EU").accesskit_node().is_disabled());
        assert!(h.get_by_role_and_label(Role::CheckBox, "EU").accesskit_node().is_disabled());
        click(&mut h, Role::Button, "EU");
        click(&mut h, Role::CheckBox, "EU");
        click(&mut h, Role::Button, "New");
        assert_eq!(condition_commands(&h), commands);
        assert_eq!(undo_steps(&h), before);
        assert_range(&h, story, &[]);
        assert!(h.state().session.doc().unwrap().doc.conditions[0].visible);
    }

    #[test]
    fn sdk_conditions_localized_create_remains_usable() {
        let mut app = app();
        app.ui.language = "ja".into();
        let mut h = harness(app, true);
        type_draft(&mut h, "日本語");
        click(&mut h, Role::Button, crate::i18n::tr("ja", "New"));
        assert_eq!(h.state().session.doc().unwrap().doc.conditions[0].name, "日本語");
        assert_eq!(draft(&h), "");
        assert_eq!(condition_commands(&h).last().unwrap(), &("condition.new".into(), json!({"name": "日本語"})));
        undo(&mut h);
        assert!(h.state().session.doc().unwrap().doc.conditions.is_empty());
    }
}
