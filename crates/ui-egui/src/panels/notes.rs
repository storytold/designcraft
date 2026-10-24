//! Notes panel: the document's editorial notes; add one at the text cursor, go to, convert,
//! split or delete a note.

use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(list) = app.session.execute("note.list", &json!({})) else { return };
    let notes = list.as_array().cloned().unwrap_or_default();
    let has_text = app.session.active().is_some_and(|d| d.selection.text.is_some());
    let id = egui::Id::new("new_note_text");
    let mut draft: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.add(
        egui::TextEdit::multiline(&mut draft)
            .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Note text")))
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    if ui
        .add_enabled(
            has_text && !draft.trim().is_empty(),
            egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Note at Cursor"))),
        )
        .clicked()
    {
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
        ui.group(|ui| {
            let author = n["author"].as_str().unwrap_or("");
            if !author.is_empty() {
                ui.label(egui::RichText::new(author).size(10.5).color(t.text_dim));
            }
            let mut text = n["text"].as_str().unwrap_or("");
            let text_id = egui::Id::new(("note_text", story.as_u64(), nid.as_u64()));
            ui.add(egui::TextEdit::multiline(&mut text).id(text_id).frame(egui::Frame::NONE).desired_rows(1).desired_width(f32::INFINITY));
            let split_at = split_point(ui.ctx(), text_id, text);
            ui.horizontal(|ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Go To"))).clicked() {
                    let _ = app.run("text.select", json!({"story": story, "anchor": at, "focus": at + 3}));
                }
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Convert to Text"))).clicked() {
                    let _ = app.run("note.convertToText", json!({"story": story, "id": nid}));
                }
                let split = egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Split Note"))).small();
                if ui.add_enabled(split_at.is_some(), split).clicked()
                    && let Some(at) = split_at
                    && let Err(e) = app.run("note.split", json!({"story": story, "id": nid, "at": at}))
                {
                    app.status_error("Notes", e);
                }
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("note.delete", json!({"story": story, "id": nid}));
                }
            });
        });
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
                let id = egui::Id::new("test_fonts");
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
