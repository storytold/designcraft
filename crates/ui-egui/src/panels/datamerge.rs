//! Data Merge panel. It reads linked sources and calls commands. Opening the panel does not merge.

use designcraft_doc::SourceStatus;
use egui::{Color32, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::{Tokens, semibold};

struct FieldRow {
    name: String,
    kind: String,
    uses: u64,
}

struct SourceSnap {
    id: u64,
    name: String,
    enabled: bool,
    status: String,
    records: u64,
    match_mode: String,
    rules: Vec<(String, String, String)>,
    sort: Vec<(String, String)>,
    field_names: Vec<String>,
    fields: Vec<FieldRow>,
}

struct GridSnap {
    id: u64,
    rows: u32,
    columns: u32,
    gutter: f64,
    offset: u32,
    advance: u32,
    origin: String,
    arrange: String,
}

struct PanelSnap {
    sources: Vec<SourceSnap>,
    options: designcraft_doc::MergeOptions,
    preview_record: u32,
    preview_len: u64,
    grid_ids: Vec<u64>,
    grid: Option<GridSnap>,
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let Some(snap) = snapshot(app) else {
        dim(ui, &t, &lang, "No data source");
        return;
    };
    let mem = ui.id().with("dm_source");
    let remembered = ui.ctx().data(|d| d.get_temp::<u64>(mem));
    let selected = remembered.filter(|id| snap.sources.iter().any(|s| s.id == *id)).or_else(|| snap.sources.first().map(|s| s.id));

    if snap.sources.is_empty() {
        dim(ui, &t, &lang, "No data source");
    }
    for source in &snap.sources {
        let count = tr(&lang, "{count} records").replace("{count}", &source.records.to_string());
        let status_color = match source.status.as_str() {
            "Modified" => t.accent,
            "Missing" => t.text_dim,
            _ => t.text,
        };
        ui.horizontal(|ui| {
            let mut enabled = source.enabled;
            if ui.checkbox(&mut enabled, "").on_hover_text(tr(&lang, "Enabled")).changed() {
                let _ = app.run("data.source.enabled", json!({"id": source.id, "enabled": enabled}));
            }
            if ui.selectable_label(selected == Some(source.id), egui::RichText::new(&source.name).color(t.text_strong)).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(mem, source.id));
            }
            crate::rtl::label(ui, egui::RichText::new(tr(&lang, &source.status)).color(status_color));
            crate::rtl::label(ui, egui::RichText::new(count).size(11.0).color(t.text_dim));
        });
    }
    ui.horizontal(|ui| {
        if ui.button(w(ui, &lang, "Select Data Source…")).clicked() {
            select_source(app);
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new(w(ui, &lang, "Update"))).clicked()
            && let Some(id) = selected
        {
            let _ = app.run("data.source.update", json!({"id": id}));
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new(w(ui, &lang, "Remove"))).clicked()
            && let Some(id) = selected
        {
            let _ = app.run("data.source.remove", json!({"id": id}));
        }
    });
    ui.separator();

    if let Some(source) = snap.sources.iter().find(|s| Some(s.id) == selected) {
        filter_editor(app, ui, &lang, &t, source);
        sort_editor(app, ui, &lang, &t, source);
    }
    if snap.sources.is_empty() {
        dim(ui, &t, &lang, "Pass csv, rows, json, or bytes from the control channel.");
    }
    for source in &snap.sources {
        if snap.sources.len() > 1 {
            crate::rtl::label(ui, egui::RichText::new(&source.name).strong().color(t.text_strong));
        }
        for field in &source.fields {
            field_row(app, ui, &lang, &t, source.id, field);
        }
    }
    ui.separator();

    let preview_len = snap.preview_len;
    let max_record = u32::try_from(preview_len).unwrap_or(u32::MAX).max(1);
    let mut preview_on = snap.preview_record > 0;
    ui.add_enabled_ui(preview_len > 0 || preview_on, |ui| {
        if ui.checkbox(&mut preview_on, w(ui, &lang, "Preview")).changed() {
            if preview_on {
                let _ = app.run("data.preview", json!({"record": 1}));
            } else {
                let _ = app.run("data.preview.stop", json!({}));
            }
        }
    });
    let shown = if snap.preview_record == 0 { 1 } else { snap.preview_record.min(max_record) };
    let mut jump = None;
    ui.add_enabled_ui(snap.preview_record > 0 && preview_len > 0, |ui| {
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
    if let Some(rec) = jump.filter(|rec| *rec != snap.preview_record) {
        let _ = app.run("data.preview", json!({"record": rec}));
    }
    ui.separator();

    merge_options(app, ui, &lang, &t, &snap.options, max_record, &snap.grid_ids);
    if !snap.grid_ids.is_empty() {
        ui.add_space(6.0);
        if ui.button(w(ui, &lang, "Create Grid")).clicked() {
            create_grid(app);
        }
        if let Some(grid) = &snap.grid {
            grid_editor(app, ui, &lang, &t, grid);
        }
    }
    ui.add_space(6.0);
    if ui.button(w(ui, &lang, "Create Merged Document…")).clicked()
        && let Ok(v) = app.run("data.merge", json!({}))
    {
        let pages = v.get("pages").and_then(Value::as_u64).unwrap_or(0);
        app.status(tr(&lang, "Created a merged document ({pages} pages).").replace("{pages}", &pages.to_string()));
    }
}

fn snapshot(app: &mut DesignApp) -> Option<PanelSnap> {
    let st = app.session.active()?;
    let mut sources = Vec::new();
    for src in &st.doc.data_merge.sources {
        let status = match src.status {
            SourceStatus::Ok => "Up to date",
            SourceStatus::Missing => "Missing",
            SourceStatus::Modified => "Modified",
        };
        sources.push(SourceSnap {
            id: src.id,
            name: src.name.clone(),
            enabled: src.enabled,
            status: status.to_string(),
            records: u64::try_from(src.rows.len()).unwrap_or(0),
            match_mode: src.filter.match_mode.clone(),
            rules: src.filter.rules.iter().map(|r| (r.field.clone(), r.op.clone(), r.value.clone())).collect(),
            sort: src.sort.iter().map(|k| (k.field.clone(), k.direction.clone())).collect(),
            field_names: src.fields.iter().map(|f| f.name.clone()).collect(),
            fields: src.fields.iter().map(|f| FieldRow { name: f.name.clone(), kind: f.kind.as_str().to_string(), uses: 0 }).collect(),
        });
    }
    let grid = st.selection.items.iter().find_map(|id| {
        let it = st.doc.item(*id)?;
        let g = it.data_grid.as_ref()?;
        Some(GridSnap {
            id: id.0,
            rows: g.rows,
            columns: g.columns,
            gutter: g.gutter,
            offset: g.record_offset,
            advance: g.record_advance,
            origin: g.origin.clone(),
            arrange: g.arrange.clone(),
        })
    });
    let grid_ids: Vec<u64> = st.doc.spreads.iter().flat_map(|sp| sp.items.iter()).filter(|it| it.data_grid.is_some()).map(|it| it.id.0).collect();
    let options = st.doc.data_merge.options.clone();
    let preview_record = st.preview_record.unwrap_or(0);
    let queried = app.session.execute("data.fields", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let preview_len = queried.first().and_then(|f| f.get("preview")).and_then(Value::as_u64).unwrap_or(0);
    for src in &mut sources {
        let rows: Vec<FieldRow> = queried
            .iter()
            .filter(|field| field.get("sourceId").and_then(Value::as_u64) == Some(src.id))
            .map(|field| FieldRow {
                name: field.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                kind: field.get("kind").and_then(Value::as_str).unwrap_or("text").to_string(),
                uses: field.get("uses").and_then(Value::as_u64).unwrap_or(0),
            })
            .collect();
        if let Some(field) = queried.iter().find(|field| field.get("sourceId").and_then(Value::as_u64) == Some(src.id)) {
            src.records = field.get("records").and_then(Value::as_u64).unwrap_or(src.records);
        }
        if !rows.is_empty() {
            src.fields = rows;
        }
    }
    Some(PanelSnap { sources, options, preview_record, preview_len, grid_ids, grid })
}

fn filter_editor(app: &mut DesignApp, ui: &mut egui::Ui, lang: &str, t: &Tokens, source: &SourceSnap) {
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Filter")).strong().color(t.text_strong));
    if let Some(mode) = choice(ui, "dm_match", &source.match_mode, &[("all", "Match all"), ("any", "Match any")], lang) {
        apply_filter(app, source.id, &mode, &source.rules);
    }
    for (i, (field, op, value)) in source.rules.iter().enumerate() {
        ui.horizontal(|ui| {
            let names: Vec<&str> = source.field_names.iter().map(String::as_str).collect();
            if let Some(next) = raw_choice(ui, &format!("dm_rule_field_{i}"), field, &names) {
                let mut rules = source.rules.clone();
                if let Some(slot) = rules.get_mut(i) {
                    slot.0 = next;
                    apply_filter(app, source.id, &source.match_mode, &rules);
                }
            }
        });
        ui.horizontal(|ui| {
            if let Some(next) = choice(
                ui,
                &format!("dm_rule_op_{i}"),
                op,
                &[
                    ("equals", "Equals"),
                    ("notEquals", "Does not equal"),
                    ("contains", "Contains"),
                    ("startsWith", "Starts with"),
                    ("empty", "Is empty"),
                    ("notEmpty", "Is not empty"),
                ],
                lang,
            ) {
                let mut rules = source.rules.clone();
                if let Some(slot) = rules.get_mut(i) {
                    slot.1 = next;
                    apply_filter(app, source.id, &source.match_mode, &rules);
                }
            }
            if op != "empty" && op != "notEmpty" {
                let mut text = value.clone();
                if ui.text_edit_singleline(&mut text).changed() {
                    let mut rules = source.rules.clone();
                    if let Some(slot) = rules.get_mut(i) {
                        slot.2 = text;
                        apply_filter(app, source.id, &source.match_mode, &rules);
                    }
                }
            }
            if ui.small_button(w(ui, lang, "Remove rule")).clicked() {
                let rules: Vec<_> = source.rules.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, r)| r.clone()).collect();
                apply_filter(app, source.id, &source.match_mode, &rules);
            }
        });
    }
    if ui.add_enabled(!source.field_names.is_empty(), egui::Button::new(w(ui, lang, "Add rule"))).clicked()
        && let Some(field) = source.field_names.first()
    {
        let mut rules = source.rules.clone();
        rules.push((field.clone(), "equals".to_string(), String::new()));
        apply_filter(app, source.id, &source.match_mode, &rules);
    }
}

fn sort_editor(app: &mut DesignApp, ui: &mut egui::Ui, lang: &str, t: &Tokens, source: &SourceSnap) {
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Sort")).strong().color(t.text_strong));
    for (i, (field, direction)) in source.sort.iter().enumerate() {
        ui.horizontal(|ui| {
            let names: Vec<&str> = source.field_names.iter().map(String::as_str).collect();
            if let Some(next) = raw_choice(ui, &format!("dm_sort_field_{i}"), field, &names) {
                let mut keys = source.sort.clone();
                if let Some(slot) = keys.get_mut(i) {
                    slot.0 = next;
                    apply_sort(app, source.id, &keys);
                }
            }
            if let Some(next) = choice(ui, &format!("dm_sort_dir_{i}"), direction, &[("asc", "Ascending"), ("desc", "Descending")], lang) {
                let mut keys = source.sort.clone();
                if let Some(slot) = keys.get_mut(i) {
                    slot.1 = next;
                    apply_sort(app, source.id, &keys);
                }
            }
            if ui.small_button(w(ui, lang, "Remove rule")).clicked() {
                let keys: Vec<_> = source.sort.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, r)| r.clone()).collect();
                apply_sort(app, source.id, &keys);
            }
        });
    }
    if ui.add_enabled(!source.field_names.is_empty(), egui::Button::new(w(ui, lang, "Add sort field"))).clicked()
        && let Some(field) = source.field_names.first()
    {
        let mut keys = source.sort.clone();
        keys.push((field.clone(), "asc".to_string()));
        apply_sort(app, source.id, &keys);
    }
}

fn field_row(app: &mut DesignApp, ui: &mut egui::Ui, lang: &str, t: &Tokens, source_id: u64, field: &FieldRow) {
    ui.horizontal(|ui| {
        kind_icon(ui, &field.kind, t.icon);
        crate::rtl::label(ui, egui::RichText::new(&field.name).color(t.text));
        crate::rtl::label(ui, egui::RichText::new(field.uses.to_string()).size(11.0).color(t.text_dim));
        let bind = field.kind == "image" || field.kind == "qr";
        let caption = if bind { "Bind to frame" } else { "Insert field" };
        if ui.small_button(w(ui, lang, caption)).clicked() {
            let params = if bind {
                json!({"field": field.name, "source": source_id})
            } else {
                json!({"field": field.name, "source": source_id, "role": "text"})
            };
            let _ = app.run("data.placeholder.add", params);
        }
    });
}

fn merge_options(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    lang: &str,
    t: &Tokens,
    options: &designcraft_doc::MergeOptions,
    max_record: u32,
    grid_ids: &[u64],
) {
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Merge options")).strong().color(t.text_strong));
    if let Some(records) = choice(ui, "dm_records", &options.records, &[("all", "All records"), ("one", "One record"), ("range", "Range")], lang) {
        let _ = app.run("data.options", json!({"records": records}));
    }
    if options.records == "one" {
        let mut one = options.one.max(1);
        if ui.add(egui::DragValue::new(&mut one).range(1..=max_record).prefix(format!("{} ", tr(lang, "Record")))).changed() {
            let _ = app.run("data.options", json!({"one": one}));
        }
    }
    if options.records == "range" {
        let mut range = options.range.clone();
        if ui.text_edit_singleline(&mut range).changed() {
            let _ = app.run("data.options", json!({"range": range}));
        }
    }
    let layout = if grid_ids.is_empty() { options.per_page.as_str() } else { "grid" };
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Records per page")).color(t.text));
    if let Some(next) = choice(ui, "dm_per_page", layout, &[("single", "Single record"), ("multiple", "Multiple records"), ("grid", "Grid")], lang) {
        select_layout(app, &next, &options.per_page, grid_ids);
    }
    if grid_ids.is_empty() && options.per_page == "multiple" {
        if let Some(arrange) = choice(ui, "dm_arrange", &options.arrange, &[("rows", "Rows first"), ("columns", "Columns first")], lang) {
            let _ = app.run("data.options", json!({"arrange": arrange}));
        }
        let labels = ["Top", "Right", "Bottom", "Left"];
        let mut insets = options.insets;
        let mut insets_changed = false;
        for (i, label) in labels.iter().enumerate() {
            ui.horizontal(|ui| {
                crate::rtl::label(ui, egui::RichText::new(tr(lang, label)).color(t.text));
                if ui.add(egui::DragValue::new(&mut insets[i]).speed(1.0)).changed() {
                    insets_changed = true;
                }
            });
        }
        if insets_changed {
            let _ = app.run("data.options", json!({"insets": insets}));
        }
        let mut column_spacing = options.column_spacing;
        if ui.add(egui::DragValue::new(&mut column_spacing).speed(1.0).prefix(format!("{} ", tr(lang, "Column spacing")))).changed() {
            let _ = app.run("data.options", json!({"columnSpacing": column_spacing}));
        }
        let mut row_spacing = options.row_spacing;
        if ui.add(egui::DragValue::new(&mut row_spacing).speed(1.0).prefix(format!("{} ", tr(lang, "Row spacing")))).changed() {
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
        lang,
    ) {
        let _ = app.run("data.options", json!({"fitting": fitting}));
    }
    let mut center = options.center;
    if ui.checkbox(&mut center, w(ui, lang, "Center in frame")).changed() {
        let _ = app.run("data.options", json!({"center": center}));
    }
    let mut link_images = options.link_images;
    if ui.checkbox(&mut link_images, w(ui, lang, "Link images")).changed() {
        let _ = app.run("data.options", json!({"linkImages": link_images}));
    }
    let mut limit = options.limit.unwrap_or(0);
    if ui.add(egui::DragValue::new(&mut limit).range(0..=100_000).prefix(format!("{} ", tr(lang, "Limit")))).changed() {
        let value = if limit == 0 { Value::Null } else { json!(limit) };
        let _ = app.run("data.options", json!({"limit": value}));
    }
}

fn grid_editor(app: &mut DesignApp, ui: &mut egui::Ui, lang: &str, t: &Tokens, grid: &GridSnap) {
    let mut rows = grid.rows.max(1);
    if ui.add(egui::DragValue::new(&mut rows).range(1..=500).prefix(format!("{} ", tr(lang, "Rows")))).changed() {
        set_grid(app, grid.id, "rows", json!(rows));
    }
    let mut columns = grid.columns.max(1);
    if ui.add(egui::DragValue::new(&mut columns).range(1..=500).prefix(format!("{} ", tr(lang, "Columns")))).changed() {
        set_grid(app, grid.id, "columns", json!(columns));
    }
    let mut gutter = grid.gutter;
    if ui.add(egui::DragValue::new(&mut gutter).range(0.0..=10_000.0).speed(1.0).prefix(format!("{} ", tr(lang, "Gutter")))).changed() {
        set_grid(app, grid.id, "gutter", json!(gutter));
    }
    let mut offset = grid.offset;
    if ui.add(egui::DragValue::new(&mut offset).range(0..=100_000).prefix(format!("{} ", tr(lang, "Record offset")))).changed() {
        set_grid(app, grid.id, "recordOffset", json!(offset));
    }
    let mut advance = grid.advance;
    if ui.add(egui::DragValue::new(&mut advance).range(0..=100_000).prefix(format!("{} ", tr(lang, "Record advance")))).changed() {
        set_grid(app, grid.id, "recordAdvance", json!(advance));
    }
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Origin")).color(t.text));
    if let Some(origin) = choice(
        ui,
        "dm_grid_origin",
        &grid.origin,
        &[("topLeft", "Top left"), ("topRight", "Top right"), ("bottomLeft", "Bottom left"), ("bottomRight", "Bottom right")],
        lang,
    ) {
        set_grid(app, grid.id, "origin", json!(origin));
    }
    crate::rtl::label(ui, egui::RichText::new(tr(lang, "Arrange")).color(t.text));
    if let Some(arrange) = choice(ui, "dm_grid_arrange", &grid.arrange, &[("rows", "Rows"), ("columns", "Columns")], lang) {
        set_grid(app, grid.id, "arrange", json!(arrange));
    }
}

fn apply_filter(app: &mut DesignApp, id: u64, mode: &str, rules: &[(String, String, String)]) {
    let rules: Vec<Value> = rules.iter().map(|(field, op, value)| json!({"field": field, "op": op, "value": value})).collect();
    let _ = app.run("data.source.filter", json!({"id": id, "match": mode, "rules": rules}));
}

fn apply_sort(app: &mut DesignApp, id: u64, keys: &[(String, String)]) {
    let fields: Vec<Value> = keys.iter().map(|(field, direction)| json!({"field": field, "direction": direction})).collect();
    let _ = app.run("data.source.sort", json!({"id": id, "fields": fields}));
}

fn set_grid(app: &mut DesignApp, id: u64, key: &str, value: Value) {
    let mut params = json!({"id": id});
    if let Some(obj) = params.as_object_mut() {
        obj.insert(key.to_string(), value);
    }
    let _ = app.run("data.grid.set", params);
}

fn select_layout(app: &mut DesignApp, next: &str, per_page: &str, grid_ids: &[u64]) {
    match next {
        "grid" => create_grid(app),
        "multiple" => {
            let _ = app.run("data.options", json!({"perPage": "multiple"}));
        }
        "single" => {
            for id in grid_ids {
                let _ = app.run("data.grid.release", json!({"id": id}));
            }
            if per_page != "single" {
                let _ = app.run("data.options", json!({"perPage": "single"}));
            }
        }
        _ => {}
    }
}

fn create_grid(app: &mut DesignApp) {
    let abs = crate::canvas::current_page(app).unwrap_or(0);
    let params = {
        let Some(st) = app.session.active() else { return };
        let Some(page) = st.doc.page(abs) else { return };
        let rect = page.margin_rect();
        let Some((spread, _)) = st.doc.page_loc(abs) else { return };
        json!({"rect": [rect.x0, rect.y0, rect.x1, rect.y1], "spread": spread})
    };
    let _ = app.run("data.grid.create", params);
}

fn select_source(app: &mut DesignApp) {
    let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("dataMerge")) else {
        app.status(tr(&app.ui.language, "Pass csv, rows, json, or bytes from the control channel."));
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

fn raw_choice(ui: &mut egui::Ui, id: &str, current: &str, options: &[&str]) -> Option<String> {
    let mut picked = None;
    egui::ComboBox::from_id_salt(id).selected_text(current).width(ui.available_width().min(160.0)).show_ui(ui, |ui| {
        for value in options {
            if ui.selectable_label(*value == current, *value).clicked() {
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
