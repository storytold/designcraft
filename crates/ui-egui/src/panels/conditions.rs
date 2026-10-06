//! Conditional Text panel: conditions with their visibility and indicator colour; click a name to
//! apply it to the selected text (the box shows whether the selection has it).

use egui::{Sense, vec2};
use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let conditions = st.doc.conditions.clone();
    let has_text = st.selection.text.is_some();
    // Conditions at the selection start (all runs share them for a "checked" box).
    let applied: Vec<String> = super::text_attrs(app)
        .and_then(|a| a["chars"]["conditions"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()))
        .unwrap_or_default();
    if conditions.is_empty() {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "No conditions. Create one, then select text and click it to apply."))
                .size(11.0)
                .color(t.text_dim),
        ));
    }
    for c in &conditions {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        // Visibility (eye).
        let eye = egui::Rect::from_min_size(row.min + vec2(2.0, 3.0), vec2(16.0, 16.0));
        let er = ui.interact(eye, ui.id().with(("cond_eye", &c.name)), Sense::click()).on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show / hide text with this condition"));
        });
        if c.visible {
            crate::icons::paint(ui.painter(), eye, "eye", t.text_dim);
        }
        if er.clicked() {
            let _ = app.run("condition.options", json!({"name": c.name, "visible": !c.visible}));
        }
        // Applied box.
        let bx = egui::Rect::from_min_size(row.min + vec2(24.0, 5.0), vec2(12.0, 12.0));
        ui.painter().rect_stroke(bx, 2.0, egui::Stroke::new(1.0, t.text_dim), egui::StrokeKind::Inside);
        if applied.contains(&c.name) {
            ui.painter()
                .line_segment([bx.left_center() + vec2(2.0, 0.0), bx.center_bottom() - vec2(0.0, 3.0)], egui::Stroke::new(1.5, t.text_strong));
            ui.painter().line_segment([bx.center_bottom() - vec2(0.0, 3.0), bx.right_top() + vec2(-2.0, 3.0)], egui::Stroke::new(1.5, t.text_strong));
        }
        // Name (click: toggle on the selected text) and indicator colour.
        let name_r = egui::Rect::from_min_max(row.min + vec2(42.0, 0.0), row.max - vec2(24.0, 0.0));
        let nr = ui.interact(name_r, ui.id().with(("cond_name", &c.name)), Sense::click());
        if nr.hovered() {
            ui.painter().rect_filled(name_r, 0.0, t.hover);
        }
        ui.painter().text(name_r.left_center() + vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, &c.name, egui::FontId::proportional(12.5), t.text);
        let chip = egui::Rect::from_min_size(egui::pos2(row.max.x - 20.0, row.min.y + 5.0), vec2(14.0, 12.0));
        ui.painter().rect_filled(chip, 2.0, egui::Color32::from_rgb(c.color[0], c.color[1], c.color[2]));
        if nr.clicked() {
            if has_text {
                let on = !applied.contains(&c.name);
                if let Err(e) = app.run("condition.apply", json!({"name": c.name, "on": on})) {
                    app.status(format!("Conditional Text: {e}"));
                }
            } else {
                app.status("Select text to apply a condition".to_string());
            }
        }
        nr.context_menu(|ui| {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Condition"))).clicked() {
                let _ = app.run("condition.delete", json!({"name": c.name}));
                ui.close();
            }
        });
    }
    ui.add_space(6.0);
    let id = egui::Id::new("new_condition_name");
    let mut name: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New condition name")))
                .desired_width(150.0),
        );
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New"))).clicked() && !name.trim().is_empty() {
            match app.run("condition.new", json!({"name": name.trim()})) {
                Ok(_) => name.clear(),
                Err(e) => app.status(format!("Conditional Text: {e}")),
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, name));
    if has_text
        && !applied.is_empty()
        && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Remove Conditions from Selection"))).clicked()
    {
        let _ = app.run("condition.apply", json!({"name": null, "only": true}));
    }
}
