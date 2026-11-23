//! Notes panel: the document's editorial notes; add one at the text cursor, go to, convert,
//! split or delete a note.

use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

fn action(
    ui: &mut egui::Ui,
    framework: bool,
    identity: (Option<u64>, Option<u64>, &'static str),
    label: &str,
    enabled: bool,
    small: bool,
    legacy: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::buttons::Button;
        use craft_ui_widgets::contract::{Props as _, Show as _};
        use craft_ui_widgets::style::Variant;

        return Button::new(label.to_owned())
            .id_salt(("notes-action", identity))
            .enabled(enabled)
            .variant(if small { Variant::GHOST } else { Variant::SECONDARY })
            .show(ui)
            .clicked();
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, identity, label, enabled, small);
    legacy(ui).clicked()
}

/// The draft remains the existing temporary UI string; creating a note is still only the New action's command.
fn draft_field(ui: &mut egui::Ui, framework: bool, id: egui::Id, language: &str, draft: &mut String) {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _, Sizing};
        let label = crate::i18n::tr(language, "Note text");
        craft_ui_widgets::text::TextField::new(label.to_owned(), Bind::bound(draft))
            .id(id.with("editor"))
            .placeholder(label.to_owned())
            .lines(2)
            .limit(None)
            .tab_inserts(false)
            // framework-gap(keeps_on_escape): restore `.keeps_on_escape()` once craft-libs has it
            .sizing(Sizing::FillWidth)
            .show(ui);
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, id);
    ui.add(
        egui::TextEdit::multiline(draft)
            .hint_text(crate::rtl::widget(ui, crate::i18n::tr(language, "Note text")))
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    #[cfg(not(target_arch = "wasm32"))]
    let framework = app.framework_installed;
    #[cfg(target_arch = "wasm32")]
    let framework = false;
    let t = Tokens::get(ui.ctx());
    let Ok(list) = app.session.execute("note.list", &json!({})) else { return };
    let notes = list.as_array().cloned().unwrap_or_default();
    let has_text = app.session.active().is_some_and(|d| d.selection.text.is_some());
    let id = egui::Id::unique("new_note_text");
    let mut draft: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    draft_field(ui, framework, id, &app.ui.language, &mut draft);
    let new_label = crate::i18n::tr(&app.ui.language, "New Note at Cursor");
    let story_id = app.session.active().and_then(|d| d.selection.text.as_ref().map(|s| s.story.0));
    if action(ui, framework, (story_id, None, "new"), new_label, has_text && !draft.trim().is_empty(), false, |ui| {
        ui.add_enabled(has_text && !draft.trim().is_empty(), egui::Button::new(crate::rtl::widget(ui, new_label)))
    }) {
        match app.run("note.new", json!({"text": draft.trim()})) {
            Ok(_) => draft.clear(),
            Err(e) => app.status_error("Notes", e),
        }
    }
    ui.data_mut(|d| d.insert_temp(id, draft));
    ui.separator();
    if notes.is_empty() {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "No notes in this document.")).size(11.0).color(t.text_dim),
        ));
    }
    for n in notes {
        let (story, nid, at) = (n["story"].clone(), n["id"].clone(), n["at"].as_u64().unwrap_or(0));
        let identity = (story.as_u64(), nid.as_u64());
        let mut draw = |ui: &mut egui::Ui| {
            ui.group(|ui| {
                let author = n["author"].as_str().unwrap_or("");
                if !author.is_empty() {
                    ui.label(egui::RichText::new(author).size(10.5).color(t.text_dim));
                }
                let mut text = n["text"].as_str().unwrap_or("");
                let text_id = egui::Id::unique(("note_text", story.as_u64(), nid.as_u64()));
                ui.add(egui::TextEdit::multiline(&mut text).id(text_id).frame(egui::Frame::NONE).desired_rows(1).desired_width(f32::INFINITY));
                let split_at = split_point(ui.ctx(), text_id, text);
                ui.horizontal(|ui| {
                    let label = crate::i18n::tr(&app.ui.language, "Go To");
                    if action(ui, framework, (story.as_u64(), nid.as_u64(), "go-to"), label, true, true, |ui| {
                        ui.small_button(crate::rtl::widget(ui, label))
                    }) {
                        let _ = app.run("text.select", json!({"story": story, "anchor": at, "focus": at + 3}));
                    }
                    let label = crate::i18n::tr(&app.ui.language, "Convert to Text");
                    if action(ui, framework, (story.as_u64(), nid.as_u64(), "convert"), label, true, true, |ui| {
                        ui.small_button(crate::rtl::widget(ui, label))
                    }) {
                        let _ = app.run("note.convertToText", json!({"story": story, "id": nid}));
                    }
                    let label = crate::i18n::tr(&app.ui.language, "Split Note");
                    if action(ui, framework, (story.as_u64(), nid.as_u64(), "split"), label, split_at.is_some(), true, |ui| {
                        let split = egui::Button::new(crate::rtl::widget(ui, label)).small();
                        ui.add_enabled(split_at.is_some(), split)
                    }) && let Some(at) = split_at
                        && let Err(e) = app.run("note.split", json!({"story": story, "id": nid, "at": at}))
                    {
                        app.status_error("Notes", e);
                    }
                    let label = crate::i18n::tr(&app.ui.language, "Delete");
                    if action(ui, framework, (story.as_u64(), nid.as_u64(), "delete"), label, true, true, |ui| {
                        ui.small_button(crate::rtl::widget(ui, label))
                    }) {
                        let _ = app.run("note.delete", json!({"story": story, "id": nid}));
                    }
                });
            })
        };
        if framework {
            ui.push_id(("notes-row", identity), draw);
        } else {
            draw(ui);
        }
    }
}

/// The byte offset of the insertion point in a note's text field, when it lies strictly inside
/// the text and selects nothing.
fn split_point(ctx: &egui::Context, id: egui::Id, text: &str) -> Option<usize> {
    let r = egui::TextEdit::load_state(ctx, id)?.cursor.char_range()?;
    let k: usize = r.primary.index.into();
    if r.primary.index != r.secondary.index || k == 0 {
        return None;
    }
    text.char_indices().nth(k).map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};

    use super::*;

    #[test]
    fn split_note_splits_the_note_at_the_insertion_point_in_its_text() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 5, "focus": 5})).unwrap();
        app.run("note.new", json!({"text": "check this", "author": "Ed"})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(320.0, 400.0)).build_ui_state(
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
        assert!(h.get_by_label("Split Note").accesskit_node().is_disabled(), "no insertion point in the note yet");
        let field = h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && n.value().as_deref() == Some("check this")).rect();
        h.hover_at(field.center());
        h.drag_at(field.center());
        h.drop_at(field.center());
        h.run_steps(2);
        h.key_press(egui::Key::Home);
        for _ in 0..6 {
            h.key_press(egui::Key::ArrowRight);
        }
        h.run_steps(2);
        h.get_by_label("Split Note").click();
        h.run_steps(3);
        let list = h.state_mut().session.execute("note.list", &json!({})).unwrap();
        let texts: Vec<_> = list.as_array().unwrap().iter().map(|n| (n["text"].clone(), n["author"].clone())).collect();
        assert_eq!(texts, [(json!("check "), json!("Ed")), (json!("this"), json!("Ed"))]);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod installed_action_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::Value;
    use std::sync::Arc;

    struct State {
        app: DesignApp,
        recorded: Option<Arc<Recorded>>,
    }

    fn text_app() -> (DesignApp, Value) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let made = app.run("frame.create", json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
        app.run("text.select", json!({"story": made["story"], "anchor": 5, "focus": 5})).unwrap();
        (app, made["story"].clone())
    }

    fn harness(mut app: DesignApp) -> Harness<'static, State> {
        app.framework_opacity = true;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        app.install_framework(&ctx);
        assert!(app.framework_installed);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(640.0, 680.0)).build_ui_state(
            |ui, state: &mut State| {
                let region = ui.scope_id();
                let (_, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| show(&mut state.app, ui));
                if recorded.is_some() {
                    state.recorded = recorded;
                }
            },
            State { app, recorded: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, State>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn key(h: &mut Harness<'_, State>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        steps(h, 2);
    }

    fn click_at(h: &mut Harness<'_, State>, pos: egui::Pos2) {
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn buttons(h: &Harness<'_, State>, label: &str) -> Vec<Element> {
        let name = crate::i18n::tr(&h.state().app.ui.language, label);
        h.state()
            .recorded
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|element| {
                element.component == "Button"
                    && element.facts.as_ref().is_some_and(|facts| facts.role == "Button" && facts.name.as_deref() == Some(name))
            })
            .cloned()
            .collect()
    }

    fn click(h: &mut Harness<'_, State>, label: &str, row: usize) {
        let target = buttons(h, label)[row].clone();
        let rect = h.state().recorded.as_ref().unwrap().hits.iter().find(|hit| hit.owner == target.id).map_or(target.rect, |hit| hit.rect);
        click_at(h, rect.center());
    }

    fn focused(h: &Harness<'_, State>, label: &str, row: usize) -> bool {
        let name = crate::i18n::tr(&h.state().app.ui.language, label);
        let nodes: Vec<_> = h.query_all_by(|node| node.role() == egui::accesskit::Role::Button && node.label().as_deref() == Some(name)).collect();
        assert_eq!(nodes.len(), buttons(h, label).len(), "bare duplicate labels retain the actual fixture row order");
        nodes[row].is_focused()
    }

    fn tab_to(h: &mut Harness<'_, State>, label: &str, row: usize) {
        let native = h.state().recorded.as_ref().unwrap().elements.iter().filter(|element| element.stop.is_some()).count();
        let fields = h.query_all_by(|node| node.role() == egui::accesskit::Role::MultilineTextInput).count();
        for _ in 0..=native + fields {
            if focused(h, label, row) {
                return;
            }
            key(h, Key::Tab, Modifiers::NONE);
        }
        panic!("natural Tab did not reach the actual {label} button in one fixture focus cycle");
    }

    fn list(h: &mut Harness<'_, State>) -> Vec<Value> {
        h.state_mut().app.session.execute("note.list", &json!({})).unwrap().as_array().unwrap().clone()
    }

    fn calls(h: &Harness<'_, State>, from: usize, id: &str) -> Vec<Value> {
        h.state().app.session.journal[from..].iter().filter(|(command, _)| command == id).map(|(_, params)| params.clone()).collect()
    }

    fn type_draft(h: &mut Harness<'_, State>, text: &str) {
        let field = h.query_all_by(|node| node.role() == egui::accesskit::Role::MultilineTextInput).next().unwrap().rect();
        click_at(h, field.center());
        key(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(text.into()));
    }

    #[test]
    fn installed_new_note_gates_trimmed_draft_preserves_refusal_and_clears_only_after_one_success() {
        let (mut app, story) = text_app();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = harness(app);
        type_draft(&mut h, "  memo  ");
        let undo = h.state().app.session.active().unwrap().history.undo.len();
        let doc = serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap();
        assert_eq!(buttons(&h, "New Note at Cursor").len(), 1);
        assert!(h.get_by_label("New Note at Cursor").accesskit_node().is_disabled());
        click(&mut h, "New Note at Cursor", 0);
        assert!(list(&mut h).is_empty());
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo);
        assert_eq!(serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap(), doc);
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
        type_draft(&mut h, "   ");
        assert!(buttons(&h, "New Note at Cursor")[0].facts.as_ref().unwrap().disabled, "whitespace is not a draft");
        type_draft(&mut h, "  memo  ");
        h.state_mut().app.run("footnote.insert", json!({"text": "Footnote"})).unwrap();
        steps(&mut h, 2);
        let undo = h.state().app.session.active().unwrap().history.undo.len();
        let refusal_doc = serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap();
        let status = h.state().app.ui.status.clone();
        click(&mut h, "New Note at Cursor", 0);
        assert!(list(&mut h).is_empty(), "editorial notes are refused inside footnote text");
        assert_ne!(h.state().app.ui.status, status, "the existing refusal still reaches the status bar");
        assert_eq!(serde_json::to_value(h.state().app.session.active().unwrap().doc.as_ref()).unwrap(), refusal_doc);
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo);
        assert!(
            h.query_by(|node| {
                node.role() == egui::accesskit::Role::MultilineTextInput
                    && node.label().as_deref() == Some(crate::i18n::tr(&h.state().app.ui.language, "Note text"))
                    && node.value().as_deref() == Some("  memo  ")
            })
            .is_some()
        );
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
        steps(&mut h, 2);
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, "New Note at Cursor", 0);
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(list(&mut h)[0]["text"], "memo");
        assert_eq!(calls(&h, journal, "note.new"), [json!({"text": "memo"})]);
        assert!(
            h.query_by(|node| {
                node.role() == egui::accesskit::Role::MultilineTextInput
                    && node.label().as_deref() == Some(crate::i18n::tr(&h.state().app.ui.language, "Note text"))
                    && node.value().as_deref() == Some("")
            })
            .is_some()
        );
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo + 1);
        steps(&mut h, 4);
        assert_eq!(calls(&h, journal, "note.new").len(), 1, "idle passes do not create another note");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert!(list(&mut h).is_empty());
    }

    fn two_notes() -> (DesignApp, Value) {
        let (mut app, story) = text_app();
        app.run("note.new", json!({"text": "αβ😀tail", "author": "Ed"})).unwrap();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        app.run("note.new", json!({"text": "first", "author": "Ada"})).unwrap();
        (app, story)
    }

    #[test]
    fn installed_duplicate_row_actions_keep_note_identity_after_delete_and_convert_once() {
        let (app, story) = two_notes();
        let mut h = harness(app);
        let notes = list(&mut h);
        assert_eq!(buttons(&h, "Delete").len(), 2);
        let kept_id = buttons(&h, "Delete")[1].id;
        let undo = h.state().app.session.active().unwrap().history.undo.len();
        let journal = h.state().app.session.journal.len();
        click(&mut h, "Go To", 1);
        let at = notes[1]["at"].as_u64().unwrap();
        assert_eq!(calls(&h, journal, "text.select"), [json!({"story": story, "anchor": at, "focus": at + 3})]);
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo);
        click(&mut h, "Delete", 0);
        assert_eq!(calls(&h, journal, "note.delete"), [json!({"story": story, "id": notes[0]["id"]})]);
        assert_eq!(list(&mut h)[0]["text"], "αβ😀tail");
        assert_eq!(buttons(&h, "Delete")[0].id, kept_id, "moving to an earlier row does not change the remaining note's SDK identity");
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, journal, "note.delete").len(), 1);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        steps(&mut h, 2);
        tab_to(&mut h, "Convert to Text", 1);
        key(&mut h, Key::Space, Modifiers::NONE);
        assert_eq!(calls(&h, journal, "note.convertToText"), [json!({"story": story, "id": notes[1]["id"]})]);
        assert_eq!(list(&mut h).len(), 1);
        let exported = h.state_mut().app.run("file.exportText", json!({"story": story})).unwrap();
        assert!(exported["text"].as_str().unwrap().contains("αβ😀tail"));
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo + 1);
    }

    #[test]
    fn installed_split_uses_the_readonly_fields_unicode_caret_byte_offset_and_one_undo() {
        let (app, story) = two_notes();
        let mut h = harness(app);
        let notes = list(&mut h);
        assert!(buttons(&h, "Split Note")[1].facts.as_ref().unwrap().disabled);
        let field = h.get_by(|node| node.role() == egui::accesskit::Role::MultilineTextInput && node.value().as_deref() == Some("αβ😀tail")).rect();
        click_at(&mut h, field.center());
        key(&mut h, Key::Home, Modifiers::NONE);
        key(&mut h, Key::ArrowRight, Modifiers::NONE);
        key(&mut h, Key::ArrowRight, Modifiers::NONE);
        assert!(!buttons(&h, "Split Note")[1].facts.as_ref().unwrap().disabled);
        let undo = h.state().app.session.active().unwrap().history.undo.len();
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, "Split Note", 1);
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(calls(&h, journal, "note.split"), [json!({"story": story, "id": notes[1]["id"], "at": "αβ".len()})]);
        let after = list(&mut h);
        assert_eq!((after[1]["text"].clone(), after[2]["text"].clone()), (json!("αβ"), json!("😀tail")));
        assert_eq!((after[1]["author"].clone(), after[2]["author"].clone()), (json!("Ed"), json!("Ed")));
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), undo + 1);
        steps(&mut h, 4);
        assert_eq!(calls(&h, journal, "note.split").len(), 1);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(list(&mut h), notes);
    }
}


#[cfg(all(test, not(target_arch = "wasm32")))]
mod draft_composition_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use egui::{Event, Key, Modifiers, PointerButton, Rect, UiBuilder, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::Value;
    use std::sync::Arc;

    struct Window {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        viewport: Rect,
        enabled: bool,
        sizing: bool,
    }

    fn text_app() -> (DesignApp, Value) {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Draft copy."})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 5, "focus": 5})).unwrap();
        (app, frame["story"].clone())
    }

    // A case-local real installed face, not a new production/default font policy or a reduced Unicode corpus.
    fn arabic_fixture(ctx: &egui::Context, language: &str) {
        if language != "ar" {
            return;
        }
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the complete Arabic case requires the real existing {family}");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family) && !face.is_placeholder() && face.style.eq_ignore_ascii_case("Regular"));
        let texts = ["Note text", "New Note at Cursor", "No notes in this document.", "Go To", "Convert to Text", "Split Note", "Delete"];
        for text in texts.into_iter().map(|text| crate::i18n::tr(language, text)).chain(std::iter::once("مرحبا")) {
            for ch in text.chars().filter(|ch| designcraft_fonts::is_rtl(*ch)) {
                assert!(face.covers(ch), "{family} must cover the full actual U+{:04X}", u32::from(ch));
            }
        }
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!("Notes Arabic case uses actual {} {}, face {}, {} existing bytes", face.family, face.style, face.index(), face.data().len());
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, Window> {
        app.framework_transform = false;
        app.framework_opacity = installed;
        let ctx = egui::Context::default();
        if installed {
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed);
        app.egui_ctx = Some(ctx.clone());
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut fonts = false;
        let mut arabic = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(340.0, 640.0)).build_ui_state(
            move |ui, w: &mut Window| {
                if !fonts {
                    crate::theme::install_fonts(ui.ctx(), &w.app.ui.language);
                    fonts = true;
                    return;
                }
                if !arabic {
                    arabic_fixture(ui.ctx(), &w.app.ui.language);
                    arabic = true;
                    return;
                }
                let builder = if w.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(w.enabled, |ui| {
                        w.controls = record(ui, egui::Id::unique("notes-draft-create-workflow"), |ui| {
                            // Exact shared Dock body premise: margin, normal ScrollArea and actual Notes dispatch.
                            egui::Frame::NONE.inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                                let output = egui::ScrollArea::vertical()
                                    .id_salt(("shared-panel-body", "notes"))
                                    .auto_shrink([false, true])
                                    .show(ui, |ui| crate::dock::panel_body(&mut w.app, ui, "notes"));
                                w.viewport = output.inner_rect;
                            });
                        })
                        .1;
                    });
                });
            },
            Window { app, controls: None, viewport: Rect::NOTHING, enabled: true, sizing: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 4);
        h
    }

    fn steps(h: &mut Harness<'_, Window>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }
    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }
    fn key(h: &mut Harness<'_, Window>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        steps(h, 2);
    }
    fn click_at(h: &mut Harness<'_, Window>, pos: egui::Pos2) {
        assert!(h.state().viewport.contains(pos), "physical click must be inside the actual scroll viewport");
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }
    fn label(h: &Harness<'_, Window>, caption: &str) -> String {
        crate::i18n::tr(&h.state().app.ui.language, caption).to_owned()
    }
    fn draft(h: &Harness<'_, Window>) -> String {
        h.ctx.data(|data| data.get_temp::<String>(egui::Id::unique("new_note_text"))).unwrap_or_default()
    }
    fn element(h: &Harness<'_, Window>, component: &str, caption: &str) -> Element {
        let name = label(h, caption);
        let matches: Vec<_> = h
            .state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.component == component && e.part.is_none() && e.facts.as_ref().is_some_and(|f| f.name.as_deref() == Some(name.as_str())))
            .collect();
        assert_eq!(matches.len(), 1, "exact actual {component}/{name} field or action");
        matches[0].clone()
    }
    fn field(h: &Harness<'_, Window>) -> (Rect, String, Option<String>, bool, bool) {
        let name = label(h, "Note text");
        let expected = draft(h);
        let installed = h.state().app.framework_installed;
        let node = h.get_by(|n| {
            n.role() == egui::accesskit::Role::MultilineTextInput
                && if installed { n.label().as_deref() == Some(name.as_str()) } else { n.value().unwrap_or_default() == expected }
        });
        let access = node.accesskit_node();
        (node.rect(), format!("{:?}", access.id()), access.value().map(|s| s.to_string()), access.is_focused(), access.is_disabled())
    }
    fn assert_draft(h: &Harness<'_, Window>, text: &str) {
        assert_eq!(draft(h), text, "the original temporary UI string writes through before blur");
        assert_eq!(field(h).2.as_deref().unwrap_or(""), text, "the actual multiline field exposes the complete text");
    }
    fn paste(h: &mut Harness<'_, Window>, text: &str) {
        let (rect, _, _, _, disabled) = field(h);
        assert!(!disabled);
        if h.state().app.framework_installed {
            assert!(rect.contains(element(h, "TextField", "Note text").rect.center()));
        }
        click_at(h, rect.center());
        assert!(field(h).3, "the original physical click must focus the actual multiline editor");
        key(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Paste(text.to_owned()));
        assert_draft(h, text);
    }
    fn new_rect(h: &Harness<'_, Window>) -> (Rect, String) {
        let name = label(h, "New Note at Cursor");
        let node = h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str()));
        (node.rect(), format!("{:?}", node.accesskit_node().id()))
    }
    fn click_new(h: &mut Harness<'_, Window>) {
        let (before, ax) = new_rect(h);
        if !h.state().viewport.contains(before.center()) {
            let semantic = h.state().app.framework_installed.then(|| element(h, "Button", "New Note at Cursor").id);
            let viewport = h.state().viewport;
            event(h, Event::PointerMoved(viewport.center()));
            event(
                h,
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, viewport.center().y - before.center().y),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: Modifiers::NONE,
                },
            );
            // ScrollArea consumes wheel at end(); its next begin() lays out the moved content.
            steps(h, 1);
            let (after, after_ax) = new_rect(h);
            assert_eq!(after_ax, ax, "native scroll must preserve the same full AX action identity");
            if let Some(id) = semantic {
                assert_eq!(element(h, "Button", "New Note at Cursor").id, id);
            }
            assert!(h.state().viewport.contains(after.center()));
            eprintln!(
                "Notes genuine wheel preserves New action {ax}: {:?} -> {:?}, viewport {:?}",
                before.center(),
                after.center(),
                h.state().viewport
            );
        }
        let center = new_rect(h).0.center();
        click_at(h, center);
    }
    fn new_focused(h: &Harness<'_, Window>) -> bool {
        let name = label(h, "New Note at Cursor");
        h.get_by(|n| n.role() == egui::accesskit::Role::Button && n.label().as_deref() == Some(name.as_str())).accesskit_node().is_focused()
    }
    fn tab_to_new(h: &mut Harness<'_, Window>) {
        let native = h.query_all_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput).count();
        let stops = h.state().controls.as_ref().unwrap().elements.iter().filter(|e| e.stop.is_some()).count();
        for _ in 0..=native + stops {
            if new_focused(h) {
                return;
            }
            key(h, Key::Tab, Modifiers::NONE);
        }
        panic!("natural Tab failed to reach the actual New action in one real focus cycle");
    }
    type Mark = (Arc<designcraft_doc::Document>, String, (usize, usize));
    fn mark(h: &Harness<'_, Window>) -> Mark {
        let st = h.state().app.session.active().unwrap();
        (
            st.doc.clone(),
            format!("{:?}", (st.uid, st.identity_epoch, &st.selection.items, &st.selection.text, &st.selection.cells)),
            (st.history.undo.len(), st.history.redo.len()),
        )
    }
    fn unchanged(h: &Harness<'_, Window>, before: &Mark) {
        let now = mark(h);
        assert_eq!(now.0.as_ref(), before.0.as_ref());
        assert_eq!(now.1, before.1);
        assert_eq!(now.2, before.2);
    }
    fn notes(h: &mut Harness<'_, Window>) -> Vec<Value> {
        h.state_mut().app.session.execute("note.list", &json!({})).unwrap().as_array().unwrap().clone()
    }
    fn calls(h: &Harness<'_, Window>, start: usize) -> Vec<Value> {
        h.state().app.session.journal[start..].iter().filter(|(command, _)| command == "note.new").map(|(_, params)| params.clone()).collect()
    }
    fn glyphs(h: &Harness<'_, Window>, text: &str) {
        h.ctx.fonts_mut(|fonts| {
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                for ch in text.chars().filter(|ch| !ch.is_control() && !ch.is_whitespace()) {
                    assert!(fonts.glyph_width(&egui::FontId::new(13.0, family.clone()), ch) > 0.0, "actual {family:?} lacks U+{:04X}", u32::from(ch));
                }
            }
        });
    }

    #[test]
    fn notes_draft_full_unicode_multiline_escape_pointer_create_once_undo_redo() {
        let (app, story) = text_app();
        let mut h = harness(app, true);
        let text = format!("  αβ😀 &<>\\\" {}\nsecond line\tend  ", "é".repeat(1300));
        glyphs(&h, &text);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        let id = element(&h, "TextField", "Note text").id;
        let ax = field(&h).1;
        paste(&mut h, &text);
        unchanged(&h, &before);
        assert_eq!(h.state().app.session.journal.len(), start);
        key(&mut h, Key::Escape, Modifiers::NONE);
        assert_draft(&h, &text);
        unchanged(&h, &before);
        click_new(&mut h);
        assert_eq!(calls(&h, start), [json!({"text": text.trim()})]);
        let list = notes(&mut h);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["text"], text.trim());
        assert_eq!(list[0]["story"], story);
        assert_eq!(list[0]["at"], 5);
        assert_draft(&h, "");
        assert_eq!(element(&h, "TextField", "Note text").id, id);
        assert_eq!(field(&h).1, ax);
        assert_eq!(mark(&h).2, (before.2.0 + 1, 0));
        let after = mark(&h).0;
        steps(&mut h, 4);
        assert_eq!(calls(&h, start).len(), 1);
        assert_draft(&h, "");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        steps(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        assert_eq!(mark(&h).1, before.1);
        assert!(notes(&mut h).is_empty());
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        steps(&mut h, 2);
        assert_eq!(mark(&h).0.as_ref(), after.as_ref());
        assert_eq!(notes(&mut h)[0]["text"], text.trim());
        assert_eq!(calls(&h, start).len(), 1);
        assert_draft(&h, "");
    }

    #[test]
    fn notes_draft_enter_is_a_newline_tab_shift_tab_and_keyboard_create_clear_once() {
        let (app, _) = text_app();
        let mut h = harness(app, true);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        paste(&mut h, "αβ😀");
        key(&mut h, Key::Enter, Modifiers::NONE);
        event(&mut h, Event::Text("second line".into()));
        assert_draft(&h, "αβ😀\nsecond line");
        unchanged(&h, &before);
        assert!(calls(&h, start).is_empty(), "Enter inside the draft must not create a note");
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert!(new_focused(&h), "plain Tab navigates rather than inserting a tab");
        key(&mut h, Key::Tab, Modifiers::SHIFT);
        assert!(field(&h).3, "Shift+Tab naturally returns to the editor");
        assert_draft(&h, "αβ😀\nsecond line");
        key(&mut h, Key::Tab, Modifiers::NONE);
        assert!(new_focused(&h));
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(calls(&h, start), [json!({"text": "αβ😀\nsecond line"})]);
        assert_draft(&h, "");
        assert_eq!(mark(&h).2.0, before.2.0 + 1);
        let center = field(&h).0.center();
        click_at(&mut h, center);
        assert!(field(&h).3);
        assert_draft(&h, "");
        event(&mut h, Event::Text("next draft".into()));
        assert_draft(&h, "next draft");
        assert_eq!(notes(&mut h).len(), 1, "refocusing a cleared editor must not resurrect or submit the old draft");
        steps(&mut h, 4);
        assert_eq!(calls(&h, start).len(), 1);
    }

    #[test]
    fn notes_draft_missing_selection_whitespace_and_disabled_parent_preserve_state() {
        let (mut app, story) = text_app();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = harness(app, true);
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        paste(&mut h, "  αβ😀 memo  ");
        assert!(element(&h, "Button", "New Note at Cursor").facts.unwrap().disabled);
        click_new(&mut h);
        unchanged(&h, &before);
        assert_draft(&h, "  αβ😀 memo  ");
        assert!(calls(&h, start).is_empty());
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
        steps(&mut h, 2);
        paste(&mut h, " \t\n  ");
        assert!(element(&h, "Button", "New Note at Cursor").facts.unwrap().disabled);
        click_new(&mut h);
        assert_draft(&h, " \t\n  ");
        paste(&mut h, "kept αβ😀");
        let before = mark(&h);
        let id = element(&h, "TextField", "Note text").id;
        let ax = field(&h).1;
        h.state_mut().enabled = false;
        steps(&mut h, 2);
        assert!(element(&h, "TextField", "Note text").facts.unwrap().disabled);
        assert!(element(&h, "Button", "New Note at Cursor").facts.unwrap().disabled);
        let center = field(&h).0.center();
        click_at(&mut h, center);
        event(&mut h, Event::Paste("must not replace".into()));
        click_new(&mut h);
        unchanged(&h, &before);
        assert_draft(&h, "kept αβ😀");
        assert!(calls(&h, start).is_empty());
        h.state_mut().enabled = true;
        steps(&mut h, 2);
        assert_eq!(element(&h, "TextField", "Note text").id, id);
        assert_eq!(field(&h).1, ax);
        assert!(!element(&h, "Button", "New Note at Cursor").facts.unwrap().disabled);
    }

    #[test]
    fn notes_draft_real_footnote_refusal_retains_full_text_then_targets_story_once() {
        let (mut app, story) = text_app();
        app.run("footnote.insert", json!({"text": "Footnote"})).unwrap();
        let mut h = harness(app, true);
        paste(&mut h, "  αβ😀\nrefused & retained  ");
        let before = mark(&h);
        let status = h.state().app.ui.status.clone();
        let start = h.state().app.session.journal.len();
        let id = element(&h, "TextField", "Note text").id;
        let ax = field(&h).1;
        assert!(!element(&h, "Button", "New Note at Cursor").facts.unwrap().disabled, "the real UI gate remains has_text, not a new cell policy");
        click_new(&mut h);
        unchanged(&h, &before);
        assert!(notes(&mut h).is_empty());
        assert_ne!(h.state().app.ui.status, status, "the existing Notes status error must still report the real backend refusal");
        assert_draft(&h, "  αβ😀\nrefused & retained  ");
        assert_eq!(element(&h, "TextField", "Note text").id, id);
        assert_eq!(field(&h).1, ax);
        assert!(calls(&h, start).is_empty(), "failed commands are not successful journal entries");
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
        steps(&mut h, 2);
        let before_success = mark(&h);
        tab_to_new(&mut h);
        key(&mut h, Key::Space, Modifiers::NONE);
        assert_eq!(calls(&h, start), [json!({"text": "αβ😀\nrefused & retained"})]);
        assert_eq!(notes(&mut h)[0]["story"], story);
        assert_eq!(notes(&mut h)[0]["at"], 5);
        assert_draft(&h, "");
        assert_eq!(mark(&h).2.0, before_success.2.0 + 1);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(mark(&h).0.as_ref(), before_success.0.as_ref());
        assert_eq!(mark(&h).1, before_success.1);
    }

    #[test]
    fn notes_draft_german_arabic_parent_names_and_full_unicode_create_at_cursor() {
        for language in ["de", "ar"] {
            let (mut app, story) = text_app();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true);
            let text = if language == "ar" { "  مرحبا αβ😀\nملاحظة &<>  " } else { "  Grüße αβ😀\nNotiz &<>  " };
            glyphs(&h, text);
            glyphs(&h, &label(&h, "Note text"));
            glyphs(&h, &label(&h, "New Note at Cursor"));
            assert_ne!(label(&h, "Note text"), "Note text");
            assert_ne!(label(&h, "New Note at Cursor"), "New Note at Cursor");
            let id = element(&h, "TextField", "Note text").id;
            let ax = field(&h).1;
            let before = mark(&h);
            let start = h.state().app.session.journal.len();
            paste(&mut h, text);
            unchanged(&h, &before);
            assert!(calls(&h, start).is_empty());
            click_new(&mut h);
            assert_eq!(calls(&h, start), [json!({"text": text.trim()})]);
            assert_eq!(notes(&mut h)[0]["story"], story);
            assert_eq!(notes(&mut h)[0]["at"], 5);
            assert_eq!(notes(&mut h)[0]["text"], text.trim());
            assert_eq!(element(&h, "TextField", "Note text").id, id);
            assert_eq!(field(&h).1, ax);
            assert_eq!(mark(&h).2.0, before.2.0 + 1);
            assert_draft(&h, "");
            h.state_mut().app.run("edit.undo", json!({})).unwrap();
            assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
            assert_eq!(mark(&h).1, before.1);
        }
    }

    #[test]
    fn notes_draft_document_switch_preserves_panel_draft_but_targets_current_document() {
        let (app, _) = text_app();
        let mut h = harness(app, true);
        paste(&mut h, "  current document αβ😀  ");
        let original = mark(&h);
        let uid = h.state().app.session.active().unwrap().uid;
        h.state_mut().app.run("file.new", json!({})).unwrap();
        let made = h.state_mut().app.run("frame.create", json!({"rect": [30, 30, 300, 200], "content": "text", "text": "Other story."})).unwrap();
        h.state_mut().app.run("text.select", json!({"story": made["story"], "anchor": 2, "focus": 2})).unwrap();
        steps(&mut h, 2);
        assert_ne!(h.state().app.session.active().unwrap().uid, uid);
        assert_draft(&h, "  current document αβ😀  ");
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        click_new(&mut h);
        assert_eq!(calls(&h, start), [json!({"text": "current document αβ😀"})]);
        assert_eq!(notes(&mut h)[0]["story"], made["story"]);
        assert_eq!(notes(&mut h)[0]["at"], 2);
        assert_eq!(mark(&h).2.0, before.2.0 + 1);
        let retained = h.state().app.session.documents().iter().find(|d| d.uid == uid).unwrap();
        assert_eq!(retained.doc.as_ref(), original.0.as_ref());
        assert_eq!(
            format!("{:?}", (retained.uid, retained.identity_epoch, &retained.selection.items, &retained.selection.text, &retained.selection.cells)),
            original.1
        );
        assert_eq!((retained.history.undo.len(), retained.history.redo.len()), original.2);
        assert_draft(&h, "");
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        assert_eq!(mark(&h).1, before.1);
    }

    #[test]
    fn notes_draft_native_uninstalled_fallback_keeps_actual_input_creation_and_undo() {
        let (app, story) = text_app();
        let mut h = harness(app, false);
        assert!(!h.state().app.framework_installed);
        assert!(
            h.state().controls.as_ref().is_none_or(|record| record.elements.is_empty()),
            "the uninstalled route must not create SDK controls; an empty inspection region is permitted"
        );
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        paste(&mut h, "  αβ😀\nlegacy draft  ");
        key(&mut h, Key::Escape, Modifiers::NONE);
        assert_draft(&h, "  αβ😀\nlegacy draft  ");
        unchanged(&h, &before);
        click_new(&mut h);
        assert_eq!(calls(&h, start), [json!({"text": "αβ😀\nlegacy draft"})]);
        assert_eq!(notes(&mut h)[0]["story"], story);
        assert_draft(&h, "");
        assert_eq!(mark(&h).2.0, before.2.0 + 1);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        assert_eq!(mark(&h).0.as_ref(), before.0.as_ref());
        assert_eq!(mark(&h).1, before.1);
    }

    #[test]
    fn notes_draft_no_document_and_real_sizing_parent_do_not_create_or_change_draft() {
        let app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        let empty = harness(app, true);
        assert!(empty.state().controls.as_ref().unwrap().elements.is_empty());
        assert_eq!(empty.query_all_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput).count(), 0);
        assert!(empty.state().app.session.documents().is_empty());
        assert!(empty.state().app.session.journal.is_empty());
        let (app, _) = text_app();
        let mut h = harness(app, true);
        paste(&mut h, "keep through sizing αβ😀");
        let before = mark(&h);
        let start = h.state().app.session.journal.len();
        let center = field(&h).0.center();
        let button = new_rect(&h).0.center();
        h.state_mut().sizing = true;
        click_at(&mut h, center);
        event(&mut h, Event::Paste("must not replace".into()));
        click_at(&mut h, button);
        unchanged(&h, &before);
        assert_eq!(draft(&h), "keep through sizing αβ😀");
        assert!(calls(&h, start).is_empty());
        h.state_mut().sizing = false;
        steps(&mut h, 2);
        assert_draft(&h, "keep through sizing αβ😀");
    }
}
