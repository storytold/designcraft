//! Story Editor (Edit → Edit in Story Editor, ⌘Y): the story as plain text with each paragraph's
//! style in the left column and the overset point marked. Edits are applied as minimal range
//! replacements, so formatting around them is kept.

use egui::vec2;
use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(sid) = app.story_editor else { return };
    let Some(st) = app.session.active() else {
        app.story_editor = None;
        return;
    };
    let Some(story) = st.doc.story(sid).cloned() else {
        app.story_editor = None;
        return;
    };
    let t = Tokens::get(ctx);
    let cs = app.session.cache.get(&st.doc, sid, None);
    let overset = cs.overset_at;
    let mut open = true;
    let mut edit: Option<(usize, usize, String)> = None;
    egui::Window::new(format!("Story Editor — {} words", story.text.split_whitespace().count()))
        .id(egui::Id::new("story_editor"))
        .open(&mut open)
        .default_size(vec2(560.0, 520.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // Style column.
                    ui.vertical(|ui| {
                        ui.set_width(130.0);
                        for (i, r) in story.para_ranges().iter().enumerate() {
                            let lines = story.text[r.clone()].len() / 60 + 1;
                            let style = &story.paras[i].style;
                            crate::rtl::label(ui, egui::RichText::new(crate::i18n::style_name(&app.ui.language, style)).size(11.0).color(t.text_dim));
                            for _ in 1..lines {
                                ui.label(egui::RichText::new(" ").size(11.0));
                            }
                        }
                    });
                    ui.separator();
                    let mut text = story.text.clone();
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .font(egui::FontId::proportional(app.ui.story_editor_size.clamp(8.0, 36.0)))
                            .desired_width(f32::INFINITY)
                            .desired_rows(20)
                            .frame(egui::Frame::NONE),
                    );
                    if r.changed() {
                        // Minimal diff: common prefix and suffix.
                        let old = &story.text;
                        let pre = old.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
                        let pre = (0..=pre).rev().find(|&i| old.is_char_boundary(i) && text.is_char_boundary(i)).unwrap_or(0);
                        let max_suf = old.len().min(text.len()) - pre;
                        let suf = old.bytes().rev().zip(text.bytes().rev()).take(max_suf).take_while(|(a, b)| a == b).count();
                        let suf =
                            (0..=suf).rev().find(|&k| old.is_char_boundary(old.len() - k) && text.is_char_boundary(text.len() - k)).unwrap_or(0);
                        edit = Some((pre, old.len() - suf, text[pre..text.len() - suf].to_string()));
                    }
                });
                if let Some(o) = overset {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "OVERSET")).color(egui::Color32::from_rgb(230, 40, 40)).strong(),
                        ));
                        crate::rtl::label(
                            ui,
                            egui::RichText::new(
                                crate::i18n::tr(&app.ui.language, "{count} characters don't fit")
                                    .replace("{count}", &story.text[o.min(story.text.len())..].chars().count().to_string()),
                            )
                            .color(t.text_dim),
                        );
                    });
                }
            });
        });
    if let Some((a, b, text)) = edit {
        let _ = app.run("story.replaceRange", json!({"story": sid.0, "start": a, "end": b, "text": text}));
    }
    if !open {
        app.story_editor = None;
    }
}
