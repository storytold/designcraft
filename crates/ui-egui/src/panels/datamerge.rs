//! Data Merge panel. It reads the linked source and calls commands. Opening the panel does not merge.

use egui::{Color32, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::{Tokens, semibold};

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let snapshot = app.session.active().map(|st| {
        let source = st.doc.data_merge.sources.first();
        (source.map(|s| s.name.clone()), source.map(|s| s.rows.len()).unwrap_or(0), st.doc.data_merge.options.clone(), st.preview_record.unwrap_or(0))
    });
    let Some((source_name, record_count, options, preview_record)) = snapshot else {
        dim(ui, &t, &lang, "No data source");
        return;
    };
    let fields = app.session.execute("data.fields", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();

    ui.horizontal(|ui| {
        let label = source_name.clone().unwrap_or_else(|| tr(&lang, "No data source").to_string());
        let color = if source_name.is_some() { t.text_strong } else { t.text_dim };
        crate::rtl::label(ui, egui::RichText::new(label).color(color));
    });
    ui.horizontal(|ui| {
        if ui.button(w(ui, &lang, "Select Data Source…")).clicked() {
            select_source(app);
        }
        let has_source = source_name.is_some();
        if ui.add_enabled(has_source, egui::Button::new(w(ui, &lang, "Update"))).clicked() {
            let _ = app.run("data.source.update", json!({}));
        }
        if ui.add_enabled(has_source, egui::Button::new(w(ui, &lang, "Remove"))).clicked() {
            let _ = app.run("data.source.remove", json!({}));
        }
    });
    ui.separator();

    if fields.is_empty() {
        dim(ui, &t, &lang, "Pass csv, rows, or bytes from the control channel.");
    }
    for field in &fields {
        let name = field["name"].as_str().unwrap_or("").to_string();
        let kind = field["kind"].as_str().unwrap_or("text").to_string();
        let uses = field["uses"].as_u64().unwrap_or(0);
        ui.horizontal(|ui| {
            kind_icon(ui, &kind, t.icon);
            crate::rtl::label(ui, egui::RichText::new(name.clone()).color(t.text));
            crate::rtl::label(ui, egui::RichText::new(uses.to_string()).size(11.0).color(t.text_dim));
            let bind = kind == "image" || kind == "qr";
            let caption = if bind { "Bind to frame" } else { "Insert field" };
            if ui.small_button(w(ui, &lang, caption)).clicked() {
                let params = if bind { json!({"field": name}) } else { json!({"field": name, "role": "text"}) };
                let _ = app.run("data.placeholder.add", params);
            }
        });
    }
    ui.separator();

    let mut preview_on = preview_record > 0;
    if ui.checkbox(&mut preview_on, w(ui, &lang, "Preview")).changed() {
        if preview_on {
            let _ = app.run("data.preview", json!({"record": 1}));
        } else {
            let _ = app.run("data.preview.stop", json!({}));
        }
    }
    let max_record = (record_count as u32).max(1);
    let shown = if preview_record == 0 { 1 } else { preview_record.min(max_record) };
    let mut jump = None;
    ui.add_enabled_ui(preview_record > 0, |ui| {
        ui.horizontal(|ui| {
            if ui.small_button(w(ui, &lang, "First")).clicked() {
                jump = Some(1);
            }
            if ui.small_button(w(ui, &lang, "Previous")).clicked() {
                jump = Some(shown.saturating_sub(1).max(1));
            }
            let mut rec = shown;
            let record = ui.add(egui::DragValue::new(&mut rec).range(1..=max_record));
            if record.changed() {
                jump = Some(rec);
            }
            let label = crate::rtl::label(ui, egui::RichText::new(tr(&lang, "Record")).color(t.text));
            record.labelled_by(label.id);
            if ui.small_button(w(ui, &lang, "Next")).clicked() {
                jump = Some(shown.saturating_add(1).min(max_record));
            }
            if ui.small_button(w(ui, &lang, "Last")).clicked() {
                jump = Some(max_record);
            }
        });
    });
    if let Some(rec) = jump.filter(|rec| *rec != preview_record) {
        let _ = app.run("data.preview", json!({"record": rec}));
    }
    ui.separator();

    crate::rtl::label(ui, egui::RichText::new(tr(&lang, "Merge options")).strong().color(t.text_strong));
    let records_name = tr(&lang, "Merge options");
    let record_options = [("all", "All records"), ("one", "One record"), ("range", "Range")];
    if let Some(records) = choice(ui, "dm_records", &options.records, &record_options, &lang, records_name) {
        let _ = app.run("data.options", json!({"records": records}));
    }
    if options.records == "one" {
        let mut one = options.one.max(1);
        if ui.add(egui::DragValue::new(&mut one).range(1..=max_record).prefix(format!("{} ", tr(&lang, "Record")))).changed() {
            let _ = app.run("data.options", json!({"one": one}));
        }
    }
    if options.records == "range" {
        let mut range = options.range.clone();
        if ui.text_edit_singleline(&mut range).changed() {
            let _ = app.run("data.options", json!({"range": range}));
        }
    }
    crate::rtl::label(ui, egui::RichText::new(tr(&lang, "Records per page")).color(t.text));
    let per_page_name = tr(&lang, "Records per page");
    let per_page_options = [("single", "Single record"), ("multiple", "Multiple records")];
    if let Some(per_page) = choice(ui, "dm_per_page", &options.per_page, &per_page_options, &lang, per_page_name) {
        let _ = app.run("data.options", json!({"perPage": per_page}));
    }
    if options.per_page == "multiple" {
        let arrange_options = [("rows", "Rows first"), ("columns", "Columns first")];
        let arrange_name = tr(&lang, "Arrangement");
        if let Some(arrange) = choice(ui, "dm_arrange", &options.arrange, &arrange_options, &lang, arrange_name) {
            let _ = app.run("data.options", json!({"arrange": arrange}));
        }
        let labels = ["Top", "Right", "Bottom", "Left"];
        let mut insets = options.insets;
        let mut insets_changed = false;
        for (i, label) in labels.iter().enumerate() {
            ui.horizontal(|ui| {
                let label = crate::rtl::label(ui, egui::RichText::new(tr(&lang, label)).color(t.text));
                if ui.add(egui::DragValue::new(&mut insets[i]).speed(1.0)).labelled_by(label.id).changed() {
                    insets_changed = true;
                }
            });
        }
        if insets_changed {
            let _ = app.run("data.options", json!({"insets": insets}));
        }
        let mut column_spacing = options.column_spacing;
        if ui.add(egui::DragValue::new(&mut column_spacing).speed(1.0).prefix(format!("{} ", tr(&lang, "Column spacing")))).changed() {
            let _ = app.run("data.options", json!({"columnSpacing": column_spacing}));
        }
        let mut row_spacing = options.row_spacing;
        if ui.add(egui::DragValue::new(&mut row_spacing).speed(1.0).prefix(format!("{} ", tr(&lang, "Row spacing")))).changed() {
            let _ = app.run("data.options", json!({"rowSpacing": row_spacing}));
        }
    }
    if let Some(fitting) = choice(
        ui,
        "dm_fitting",
        &options.fitting,
        &[
            ("fitProportionally", "Fit proportionally"),
            ("fillProportionally", "Fill proportionally"),
            ("fitContentToFrame", "Fit content to frame"),
            ("none", "None"),
        ],
        &lang,
        "Image Fitting",
    ) {
        let _ = app.run("data.options", json!({"fitting": fitting}));
    }
    let mut center = options.center;
    if ui.checkbox(&mut center, w(ui, &lang, "Center in frame")).changed() {
        let _ = app.run("data.options", json!({"center": center}));
    }
    let mut link_images = options.link_images;
    if ui.checkbox(&mut link_images, w(ui, &lang, "Link images")).changed() {
        let _ = app.run("data.options", json!({"linkImages": link_images}));
    }
    let mut limit = options.limit.unwrap_or(0);
    if ui.add(egui::DragValue::new(&mut limit).range(0..=100_000).prefix(format!("{} ", tr(&lang, "Limit")))).changed() {
        let value = if limit == 0 { Value::Null } else { json!(limit) };
        let _ = app.run("data.options", json!({"limit": value}));
    }
    ui.add_space(6.0);
    if ui.button(w(ui, &lang, "Create Merged Document…")).clicked() {
        let _ = app.run("data.merge", json!({}));
    }
}

fn select_source(app: &mut DesignApp) {
    let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("dataMerge")) else {
        app.status(tr(&app.ui.language, "Pass csv, rows, or bytes from the control channel."));
        return;
    };
    let _ = app.run("data.source.select", json!({"path": path}));
}

/// Render a localized, named combo and return only a changed selection.
fn choice(ui: &mut egui::Ui, id: &str, current: &str, options: &[(&str, &str)], lang: &str, name: &str) -> Option<String> {
    let label = options.iter().find(|(value, _)| *value == current).map(|(_, text)| tr(lang, text)).unwrap_or(current);
    let mut picked = None;
    let combo = egui::ComboBox::from_id_salt(id).selected_text(label).width(ui.available_width().min(200.0)).show_ui(ui, |ui| {
        for (value, text) in options {
            if ui.selectable_label(*value == current, tr(lang, text)).clicked() {
                picked = Some((*value).to_string());
            }
        }
    });
    let _ = crate::widgets::name_combo(combo, name.to_owned());
    picked.filter(|value| value != current)
}

fn kind_icon(ui: &mut egui::Ui, kind: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    let painter = ui.painter();
    match kind {
        "image" => {
            painter.rect_stroke(rect.shrink(1.5), 1.0, Stroke::new(1.0, color), egui::StrokeKind::Middle);
            painter.circle_stroke(rect.center() + vec2(-3.0, -2.0), 1.5, Stroke::new(1.0, color));
            let base = rect.left_bottom() + vec2(2.0, -3.0);
            painter.line_segment([base, rect.center() + vec2(-1.0, 2.0)], Stroke::new(1.0, color));
            painter.line_segment([rect.center() + vec2(-1.0, 2.0), rect.right_bottom() + vec2(-2.0, -5.0)], Stroke::new(1.0, color));
        }
        "qr" => {
            let origin = rect.min + vec2(2.0, 2.0);
            for (x, y) in [(0, 0), (2, 0), (0, 2), (2, 2), (1, 1)] {
                let at = origin + vec2(x as f32 * 4.0, y as f32 * 4.0);
                painter.rect_filled(egui::Rect::from_min_size(at, vec2(3.0, 3.0)), 0.0, color);
            }
        }
        _ => {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "T", semibold(13.0), color);
        }
    }
}

fn dim(ui: &mut egui::Ui, t: &Tokens, lang: &str, key: &str) {
    crate::rtl::label(ui, egui::RichText::new(tr(lang, key)).size(11.0).color(t.text_dim));
}

fn tr<'a>(lang: &str, s: &'a str) -> &'a str {
    crate::i18n::tr(lang, s)
}

fn w(ui: &egui::Ui, lang: &str, s: &str) -> egui::WidgetText {
    crate::rtl::widget(ui, tr(lang, s))
}

#[cfg(test)]
mod tests {
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    use super::*;

    /// Records and Arrangement each expose exactly one named combo, and the accessibility check passes.
    #[test]
    fn arrangement_combo_is_accessible_and_distinct_from_the_records_combo() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("data.source.select", json!({"bytes": designcraft_engine::cmd::base64_encode(b"Name\nAda\nGrace\n"), "name": "people.csv"})).unwrap();
        app.run("data.options", json!({"perPage": "multiple"})).unwrap();
        let mut h = Harness::builder().with_size(vec2(420.0, 700.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("datamerge_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                show(app, ui);
            },
            app,
        );
        h.run_steps(3);
        let combos = |h: &Harness<'_, DesignApp>, label: &str| {
            h.query_all_by_label(label).filter(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox).count()
        };
        assert_eq!(combos(&h, "Merge options"), 1, "records selection keeps its name");
        assert_eq!(combos(&h, "Arrangement"), 1, "the arrangement control has its own name");
        h.check_accessibility();
    }
}
