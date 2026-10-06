//! Notes panel: the document's editorial notes; add one at the text cursor, go to, convert or
//! delete a note.

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
            Err(e) => app.status(format!("Notes: {e}")),
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
            ui.label(n["text"].as_str().unwrap_or(""));
            ui.horizontal(|ui| {
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Go To"))).clicked() {
                    let _ = app.run("text.select", json!({"story": story, "anchor": at, "focus": at + 3}));
                }
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Convert to Text"))).clicked() {
                    let _ = app.run("note.convertToText", json!({"story": story, "id": nid}));
                }
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete"))).clicked() {
                    let _ = app.run("note.delete", json!({"story": story, "id": nid}));
                }
            });
        });
    }
}
