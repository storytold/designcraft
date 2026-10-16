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
    if crate::widgets::native_checkbox(ui, &mut preview_on, w(ui, &lang, "Preview")).changed() {
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
            if ui.add(egui::DragValue::new(&mut rec).range(1..=max_record)).changed() {
                jump = Some(rec);
            }
            crate::rtl::label(ui, egui::RichText::new(tr(&lang, "Record")).color(t.text));
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
    if let Some(records) = choice(ui, "dm_records", &options.records, &[("all", "All records"), ("one", "One record"), ("range", "Range")], &lang) {
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
    if let Some(per_page) = choice(ui, "dm_per_page", &options.per_page, &[("single", "Single record"), ("multiple", "Multiple records")], &lang) {
        let _ = app.run("data.options", json!({"perPage": per_page}));
    }
    if options.per_page == "multiple" {
        if let Some(arrange) = choice(ui, "dm_arrange", &options.arrange, &[("rows", "Rows first"), ("columns", "Columns first")], &lang) {
            let _ = app.run("data.options", json!({"arrange": arrange}));
        }
        let labels = ["Top", "Right", "Bottom", "Left"];
        let mut insets = options.insets;
        let mut insets_changed = false;
        for (i, label) in labels.iter().enumerate() {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, egui::RichText::new(tr(&lang, label)).color(t.text));
                if ui.add(egui::DragValue::new(&mut insets[i]).speed(1.0)).changed() {
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
    ) {
        let _ = app.run("data.options", json!({"fitting": fitting}));
    }
    let mut center = options.center;
    if crate::widgets::native_checkbox(ui, &mut center, w(ui, &lang, "Center in frame")).changed() {
        let _ = app.run("data.options", json!({"center": center}));
    }
    let mut link_images = options.link_images;
    if crate::widgets::native_checkbox(ui, &mut link_images, w(ui, &lang, "Link images")).changed() {
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

fn choice(ui: &mut egui::Ui, id: &str, current: &str, options: &[(&str, &str)], lang: &str) -> Option<String> {
    let label = options.iter().find(|(value, _)| *value == current).map(|(_, text)| tr(lang, text)).unwrap_or(current);
    let mut picked = None;
    egui::ComboBox::from_id_salt(id).selected_text(label).width(ui.available_width().min(200.0)).show_ui(ui, |ui| {
        for (value, text) in options {
            if ui.selectable_label(*value == current, tr(lang, text)).clicked() {
                picked = Some((*value).to_string());
            }
        }
    });
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
