//! Data Merge panel. It reads the linked source and calls commands. Opening the panel does not merge.

use egui::{Color32, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::theme::{Tokens, semibold};

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language.clone();
    let snapshot = app.session.active().map(|st| {
        let source = st.doc.data_merge.sources.first();
        (
            (st.uid, st.identity_epoch),
            source.map(|s| s.name.clone()),
            source.map(|s| s.id),
            source.map(|s| s.rows.len()).unwrap_or(0),
            st.doc.data_merge.options.clone(),
            st.preview_record.unwrap_or(0),
        )
    });
    let Some((document, source_name, source_id, record_count, options, preview_record)) = snapshot else {
        dim(ui, &t, &lang, "No data source");
        return;
    };
    let controls = DataMergeControls {
        document,
        #[cfg(not(target_arch = "wasm32"))]
        sdk: app.framework_installed,
        #[cfg(target_arch = "wasm32")]
        sdk: false,
    };
    let fields = app.session.execute("data.fields", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();

    ui.horizontal(|ui| {
        let label = source_name.clone().unwrap_or_else(|| tr(&lang, "No data source").to_string());
        let color = if source_name.is_some() { t.text_strong } else { t.text_dim };
        crate::rtl::label(ui, egui::RichText::new(label).color(color));
    });
    ui.horizontal(|ui| {
        if controls.button(ui, "select-source", tr(&lang, "Select Data Source…"), tr(&lang, "Select Data Source…"), true, false) {
            select_source(app);
        }
        let has_source = source_name.is_some();
        if controls.button(ui, "update-source", tr(&lang, "Update"), tr(&lang, "Update"), has_source, false) {
            let _ = app.run("data.source.update", json!({}));
        }
        if controls.button(ui, "remove-source", tr(&lang, "Remove"), tr(&lang, "Remove"), has_source, false) {
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
            // Keep the short painted caption and the untouched raw data-field name, but
            // name the real action unambiguously for keyboard/accessibility consumers.
            ui.push_id(("datamerge-field", source_id, name.as_str(), kind.as_str()), |ui| {
                let caption = tr(&lang, caption);
                let action_name = format!("{caption}: {name}");
                if controls.button(ui, "insert-field", caption, &action_name, true, true) {
                    let params = if bind { json!({"field": name}) } else { json!({"field": name, "role": "text"}) };
                    let _ = app.run("data.placeholder.add", params);
                }
            });
        });
    }
    ui.separator();

    let mut preview_on = preview_record > 0;
    if controls.check(ui, "preview", tr(&lang, "Preview"), &mut preview_on) {
        if preview_on {
            let _ = app.run("data.preview", json!({"record": 1}));
        } else {
            let _ = app.run("data.preview.stop", json!({}));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let framework = app.framework_installed;
    let navigation_button = |ui: &mut egui::Ui, key: &str, label: &str| {
        #[cfg(not(target_arch = "wasm32"))]
        if framework {
            use craft_ui_widgets::contract::{Props as _, Show as _};
            return craft_ui_widgets::buttons::Button::new(tr(&lang, label).to_owned())
                .id(ui.make_persistent_id(("datamerge-preview", key)))
                .enabled(ui.is_enabled())
                .size(craft_theme::ControlSize::Sm)
                .show(ui)
                .clicked();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = key;
        ui.small_button(w(ui, &lang, label)).clicked()
    };
    let max_record = (record_count as u32).max(1);
    let shown = if preview_record == 0 { 1 } else { preview_record.min(max_record) };
    let mut jump = None;
    ui.add_enabled_ui(preview_record > 0, |ui| {
        // The SDK number field is wider than the old bare counter. Wrap the existing
        // navigation rather than clipping the last buttons in a narrow sidebar.
        ui.horizontal_wrapped(|ui| {
            if navigation_button(ui, "first", "First") {
                jump = Some(1);
            }
            if navigation_button(ui, "previous", "Previous") {
                jump = Some(shown.saturating_sub(1).max(1));
            }
            let record = controls.number(ui, "preview-record", tr(&lang, "Record"), shown as f64, Some(1.0..=max_record as f64), true, false);
            if let Some(rec) = record.value {
                jump = Some(rec as u32);
            }
            let label = crate::rtl::label(ui, egui::RichText::new(tr(&lang, "Record")).color(t.text));
            record.response.labelled_by(label.id);
            if navigation_button(ui, "next", "Next") {
                jump = Some(shown.saturating_add(1).min(max_record));
            }
            if navigation_button(ui, "last", "Last") {
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
    if let Some(records) = controls.choice(ui, "dm_records", &options.records, &record_options, &lang, records_name) {
        let _ = app.run("data.options", json!({"records": records}));
    }
    if options.records == "one" {
        if let Some(one) =
            controls.number(ui, "one-record", tr(&lang, "Record"), options.one.max(1) as f64, Some(1.0..=max_record as f64), true, true).value
        {
            let _ = app.run("data.options", json!({"one": one as u32}));
        }
    }
    if options.records == "range" {
        let mut range = options.range.clone();
        if controls.text(ui, "record-range", tr(&lang, "Range"), &mut range) {
            let _ = app.run("data.options", json!({"range": range}));
        }
    }
    crate::rtl::label(ui, egui::RichText::new(tr(&lang, "Records per page")).color(t.text));
    let per_page_name = tr(&lang, "Records per page");
    let per_page_options = [("single", "Single record"), ("multiple", "Multiple records")];
    if let Some(per_page) = controls.choice(ui, "dm_per_page", &options.per_page, &per_page_options, &lang, per_page_name) {
        let _ = app.run("data.options", json!({"perPage": per_page}));
    }
    if options.per_page == "multiple" {
        let arrange_options = [("rows", "Rows first"), ("columns", "Columns first")];
        let arrange_name = tr(&lang, "Arrangement");
        if let Some(arrange) = controls.choice(ui, "dm_arrange", &options.arrange, &arrange_options, &lang, arrange_name) {
            let _ = app.run("data.options", json!({"arrange": arrange}));
        }
        let labels = ["Top", "Right", "Bottom", "Left"];
        let mut insets = options.insets;
        let mut insets_changed = false;
        for (i, label) in labels.iter().enumerate() {
            ui.horizontal(|ui| {
                let label_response = crate::rtl::label(ui, egui::RichText::new(tr(&lang, label)).color(t.text));
                let number = controls.number(ui, label, tr(&lang, label), insets[i], None, false, false);
                number.response.labelled_by(label_response.id);
                if let Some(value) = number.value {
                    insets[i] = value;
                    insets_changed = true;
                }
            });
        }
        if insets_changed {
            let _ = app.run("data.options", json!({"insets": insets}));
        }
        if let Some(column_spacing) =
            controls.number(ui, "column-spacing", tr(&lang, "Column spacing"), options.column_spacing, None, false, true).value
        {
            let _ = app.run("data.options", json!({"columnSpacing": column_spacing}));
        }
        if let Some(row_spacing) = controls.number(ui, "row-spacing", tr(&lang, "Row spacing"), options.row_spacing, None, false, true).value {
            let _ = app.run("data.options", json!({"rowSpacing": row_spacing}));
        }
    }
    if let Some(fitting) = controls.choice(
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
    if controls.check(ui, "center", tr(&lang, "Center in frame"), &mut center) {
        let _ = app.run("data.options", json!({"center": center}));
    }
    let mut link_images = options.link_images;
    if controls.check(ui, "link-images", tr(&lang, "Link images"), &mut link_images) {
        let _ = app.run("data.options", json!({"linkImages": link_images}));
    }
    if let Some(limit) = controls.number(ui, "limit", tr(&lang, "Limit"), options.limit.unwrap_or(0) as f64, Some(0.0..=100_000.0), true, true).value
    {
        let limit = limit as u32;
        let value = if limit == 0 { Value::Null } else { json!(limit) };
        let _ = app.run("data.options", json!({"limit": value}));
    }
    ui.add_space(6.0);
    if controls.button(ui, "merge", tr(&lang, "Create Merged Document…"), tr(&lang, "Create Merged Document…"), true, false) {
        let _ = app.run("data.merge", json!({}));
    }
}

/// Local widget adapters only: every accepted value still calls the existing live engine
/// command below. No draft document, Apply/Cancel layer, IO protocol or Undo wrapper.
struct DataMergeControls {
    sdk: bool,
    document: (u64, u64),
}

struct MergeNumber {
    response: egui::Response,
    value: Option<f64>,
}

impl DataMergeControls {
    fn id(&self, ui: &egui::Ui, kind: &str, key: &str) -> egui::Id {
        ui.make_persistent_id(("datamerge-workflow", self.document, kind, key))
    }

    fn button(&self, ui: &mut egui::Ui, key: &str, caption: &str, name: &str, enabled: bool, small: bool) -> bool {
        let id = self.id(ui, "button", key);
        let enabled = enabled && ui.is_enabled();
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, Show as _};
            return craft_ui_widgets::buttons::Button::new(name.to_owned()).painted(caption.to_owned()).id(id).enabled(enabled).show(ui).clicked();
        }
        let response = ui
            .push_id(id, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if small {
                        ui.small_button(crate::rtl::widget(ui, caption))
                    } else {
                        ui.button(crate::rtl::widget(ui, caption))
                    }
                })
                .inner
            })
            .inner;
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
        response.clicked()
    }

    fn check(&self, ui: &mut egui::Ui, key: &str, name: &str, value: &mut bool) -> bool {
        let id = self.id(ui, "check", key);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
            let before = *value;
            craft_ui_widgets::choice::Checkbox::new(Bind::bound(value)).id(id).label(name.to_owned()).enabled(ui.is_enabled()).show(ui);
            return *value != before;
        }
        ui.push_id(id, |ui| ui.checkbox(value, crate::rtl::widget(ui, name))).inner.changed()
    }

    fn choice(&self, ui: &mut egui::Ui, key: &str, current: &str, options: &[(&str, &str)], lang: &str, name: &str) -> Option<String> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, Sizing};
            let id = self.id(ui, "choice", key);
            let current_index = options.iter().position(|(value, _)| *value == current).unwrap_or(usize::MAX);
            let entries: Vec<_> = options.iter().enumerate().map(|(i, (_, text))| Entry::new(i, tr(lang, text).to_owned())).collect();
            let width = ui.available_width().min(200.0);
            let out = ui
                .allocate_ui_with_layout(vec2(width, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    craft_ui_widgets::pickers::Select::new(tr(lang, name).to_owned(), Bind::given(current_index), &entries)
                        .id(id)
                        .enabled(ui.is_enabled())
                        .placeholder(current.to_owned())
                        .sizing(Sizing::FillWidth)
                        .show(ui)
                })
                .inner;
            // Arrow navigation is only a proposal. Escape cannot send data.options.
            return out.events.committed().and_then(|i| options.get(*i)).map(|(value, _)| (*value).to_owned()).filter(|value| value != current);
        }
        ui.push_id(self.id(ui, "choice", key), |ui| choice(ui, key, current, options, lang, name)).inner
    }

    fn text(&self, ui: &mut egui::Ui, key: &str, name: &str, value: &mut String) -> bool {
        let id = self.id(ui, "text", key);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _, Sizing};
            let out = craft_ui_widgets::text::TextField::new(name.to_owned(), Bind::given(value.clone()))
                .id(id)
                .enabled(ui.is_enabled())
                .limit(None)
                .sizing(Sizing::FillWidth)
                .show(ui);
            // Range text was already live, including invalid grammar. Only data.merge
            // parses record ranges; do not turn this panel into an invented Apply dialog.
            if let Some(next) = out.value().filter(|next| next.as_str() != value.as_str()) {
                *value = next.clone();
                return true;
            }
            return false;
        }
        let response = ui.add(egui::TextEdit::singleline(value).id(id));
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
        response.changed()
    }

    fn number(
        &self,
        ui: &mut egui::Ui,
        key: &str,
        name: &str,
        value: f64,
        range: Option<std::ops::RangeInclusive<f64>>,
        integer: bool,
        prefix: bool,
    ) -> MergeNumber {
        let id = self.id(ui, "number", key);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _, Sizing};
            return ui
                .horizontal(|ui| {
                    if prefix {
                        crate::rtl::label(ui, name);
                    }
                    let width = if key == "preview-record" { 56.0 } else { 110.0 };
                    let out = ui
                        .allocate_ui_with_layout(vec2(width, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let mut field = craft_ui_widgets::text::NumberField::new(name.to_owned(), Bind::given(Some(value)))
                                .id(id)
                                .enabled(ui.is_enabled())
                                .step(1.0)
                                .sizing(Sizing::FillWidth);
                            if let Some(range) = &range {
                                field = field.range(range.clone());
                            }
                            if integer {
                                field.fixed_decimals(0).show(ui)
                            } else {
                                field.show(ui)
                            }
                        })
                        .inner;
                    // Like the original live DragValue, successful typing/scrubbing is
                    // already an engine edit. Escape is not a new rollback command or an
                    // invented grouped Undo transaction; ignore Cancel-only events.
                    let next = out
                        .edit
                        .value()
                        .copied()
                        .flatten()
                        .filter(|v| v.is_finite())
                        .map(|v| {
                            let v = if integer { v.trunc() } else { v };
                            range.as_ref().map_or(v, |range| v.clamp(*range.start(), *range.end()))
                        })
                        .filter(|v| *v != value);
                    MergeNumber { response: out.edit.response, value: next }
                })
                .inner;
        }
        if integer {
            // Keep the real legacy u32 control, including its quarter-point drag speed,
            // integer conversion and live-typing behavior, in the native fallback.
            let mut next = value as u32;
            let mut field = egui::DragValue::new(&mut next);
            if let Some(range) = &range {
                field = field.range(*range.start() as u32..=*range.end() as u32);
            }
            if prefix {
                field = field.prefix(format!("{name} "));
            }
            let response = ui.push_id(id, |ui| ui.add(field)).inner;
            response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
            let next = next as f64;
            let changed = response.changed() && next != value;
            return MergeNumber { response, value: changed.then_some(next) };
        }
        let mut next = value;
        let mut field = egui::DragValue::new(&mut next).speed(1.0);
        if let Some(range) = &range {
            field = field.range(range.clone());
        }
        if prefix {
            field = field.prefix(format!("{name} "));
        }
        let response = ui.push_id(id, |ui| ui.add(field)).inner;
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(name.to_owned()));
        let changed = response.changed() && next.is_finite() && next != value;
        MergeNumber { response, value: changed.then_some(next) }
    }
}

fn select_source(app: &mut DesignApp) {
    let asked = app.ask_open("dataMerge", Bound::Document, |app, path| {
        let _ = app.run("data.source.select", json!({"path": path}));
        Ok(Value::Null)
    });
    if asked.is_err() {
        app.status(tr(&app.ui.language, "Pass csv, rows, or bytes from the control channel."));
    }
}

/// Render a localized, named combo and return only a changed selection.
fn choice(ui: &mut egui::Ui, id: &str, current: &str, options: &[(&str, &str)], lang: &str, name: &str) -> Option<String> {
    let label = options.iter().find(|(value, _)| *value == current).map(|(_, text)| tr(lang, text)).unwrap_or(current);
    let mut picked = None;
    let combo = craft_ui_legacy::ComboBox::from_id_salt(id).selected_text(label).width(ui.available_width().min(200.0)).show_ui(ui, |ui| {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "datamerge_preview_nav_port_tests.rs"]
mod datamerge_preview_nav_port_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_datamerge_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::{collections::HashSet, sync::Arc};

    const CSV: &str = "Name,@Photo,#Code\nAda,,one\nGrace,,two\nKatherine,,three\n";

    struct Window {
        app: DesignApp,
        controls: Option<Arc<Recorded>>,
        number_units: Option<craft_ui_widgets::runtime::env::Units>,
        enabled: bool,
        sizing: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct DocumentEffects {
        uid: u64,
        epoch: u64,
        revision: u64,
        undo: usize,
        redo: usize,
        document: Arc<designcraft_doc::Document>,
        saved: Arc<designcraft_doc::Document>,
        preview: Option<u32>,
        stash: Option<Arc<designcraft_doc::Document>>,
        path: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Effects {
        active: Option<usize>,
        documents: Vec<DocumentEffects>,
        journal: Vec<(String, Value)>,
        queued: usize,
        pending: bool,
    }

    fn select_csv(app: &mut DesignApp, csv: &str) {
        // Same real inline-source adapter as the preserved native navigation tests.
        // data.source.select accepts bytes/base64; csv/rows are data.fields/merge inputs.
        app.run("data.source.select", json!({"bytes": designcraft_engine::cmd::base64_encode(csv.as_bytes()), "name": "people.csv"})).unwrap();
        assert_eq!(app.session.active().unwrap().doc.data_merge.sources[0].rows.len(), 3);
        assert!(app.session.active().unwrap().doc.data_merge.sources[0].path.is_none());
    }

    fn app(pages: u32, facing: bool) -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
        app.run("file.new", json!({"pages": pages, "facingPages": facing})).unwrap();
        select_csv(&mut app, CSV);
        app
    }

    fn fixture(app: DesignApp, sdk: bool) -> Harness<'static, Window> {
        fixture_width(app, sdk, 560.0)
    }

    fn fixture_width(mut app: DesignApp, sdk: bool, width: f32) -> Harness<'static, Window> {
        // Pin both pilot inputs before installation, not the process environment. A
        // fallback fixture must not silently inherit the Transform pilot's runtime.
        app.framework_transform = false;
        app.framework_opacity = sdk;
        assert!(!app.framework_installed);
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        assert_eq!(app.framework_installed, sdk);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(width, 1400.0)).build_ui_state(
            move |ui, state: &mut Window| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    ready = true;
                    return;
                }
                let builder = if state.sizing { UiBuilder::new().sizing_pass() } else { UiBuilder::new() };
                ui.scope_builder(builder, |ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        // Observe only the same inherited formatting configuration that
                        // NumberField reads. Do not override the host locale or widget state.
                        if state.app.framework_installed {
                            state.number_units = Some(craft_ui_widgets::runtime::env::Env::of(ui).units());
                        }
                        state.controls = record(ui, egui::Id::NULL.with("datamerge-full-workflow-tests"), |ui| show(&mut state.app, ui)).1;
                    });
                });
            },
            Window { app, controls: None, number_units: None, enabled: true, sizing: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 4);
        h
    }

    fn step(h: &mut Harness<'_, Window>) {
        let mut raw = std::mem::take(h.input_mut());
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, Window>, frames: usize) {
        for _ in 0..frames {
            step(h);
        }
    }
    fn event(h: &mut Harness<'_, Window>, event: Event) {
        h.input_mut().events.push(event);
        step(h);
    }
    fn key_mod(h: &mut Harness<'_, Window>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        idle(h, 2);
    }
    fn key(h: &mut Harness<'_, Window>, key: Key) {
        key_mod(h, key, Modifiers::NONE);
    }
    fn control(h: &Harness<'_, Window>, component: &str, name: &str, at: usize) -> Element {
        h.state()
            .controls
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|element| {
                element.part.is_none() && element.component == component && element.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name)
            })
            .nth(at)
            .unwrap_or_else(|| panic!("missing real SDK {component} named {name:?} at {at}"))
            .clone()
    }
    fn click_rect(h: &mut Harness<'_, Window>, rect: egui::Rect) {
        let pos = rect.center();
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        idle(h, 3);
    }
    fn click(h: &mut Harness<'_, Window>, component: &str, name: &str) {
        let element = control(h, component, name, 0);
        if !element.facts.as_ref().unwrap().disabled {
            assert!(
                h.state().controls.as_ref().unwrap().hits.iter().any(|hit| hit.owner == element.id && hit.rect.intersects(element.rect)),
                "real SDK hit ownership"
            );
        }
        click_rect(h, element.rect);
    }
    fn native_click(h: &mut Harness<'_, Window>, name: &str) {
        let rect = h.get_by_label(name).rect();
        click_rect(h, rect);
    }
    fn tab_to(h: &mut Harness<'_, Window>, element: &Element) {
        let mut visited = HashSet::new();
        loop {
            let focused = h.ctx.memory(|m| m.focused());
            if focused == Some(element.id) {
                return;
            }
            assert!(visited.insert(focused), "natural Tab must reach the observed SDK element {:?}", element.facts);
            key(h, Key::Tab);
        }
    }
    fn choose(h: &mut Harness<'_, Window>, name: &str, value: &str, sdk: bool) {
        if sdk {
            click(h, "Select", name);
        } else {
            let rect = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, name).rect();
            click_rect(h, rect);
        }
        let rect = h.get_all_by_label(value).last().unwrap().rect();
        click_rect(h, rect);
    }
    fn replace(h: &mut Harness<'_, Window>, rect: egui::Rect, text: &str) {
        click_rect(h, rect);
        key_mod(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(text.to_owned()));
        idle(h, 2);
    }
    fn number(h: &mut Harness<'_, Window>, name: &str, at: usize, text: &str) {
        let element = control(h, "NumberField", name, at);
        assert_eq!(element.facts.as_ref().unwrap().role, "SpinButton");
        replace(h, element.rect, text);
        key(h, Key::Enter);
    }
    fn numeric_input(h: &Harness<'_, Window>, value: f64) -> String {
        // Match NumberField's plain-number codec: the inherited Units format, no
        // grouping, up to its default three decimals. This formats genuine input;
        // the independent engine/payload assertions below still require exact values.
        let units = h.state().number_units.as_ref().expect("the rendered SDK environment supplied its format");
        let text = units.number_ungrouped(value, 0..=3);
        eprintln!("Data Merge genuine numeric input: value={value:?}, text={text:?}, decimal={:?}", units.reader(None).decimal_separator());
        text
    }
    fn native_number(h: &mut Harness<'_, Window>, name: &str, at: usize, value: &str) {
        let rect =
            h.query_all_by_label(name).filter(|node| node.accesskit_node().role() == egui::accesskit::Role::SpinButton).nth(at).unwrap().rect();
        // A real DragValue click enters its numeric editor. In particular a long
        // prefix disappears in edit mode, so a second click at the old button's
        // centre can land outside the now-short editor and dismiss it again.
        click_rect(h, rect);
        key_mod(h, Key::A, Modifiers::COMMAND);
        event(h, Event::Text(value.to_owned()));
        key(h, Key::Enter);
    }
    fn range(h: &mut Harness<'_, Window>, value: &str, sdk: bool) {
        let rect =
            if sdk { control(h, "TextField", "Range", 0).rect } else { h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Range").rect() };
        replace(h, rect, value);
    }
    fn options(h: &Harness<'_, Window>) -> designcraft_doc::MergeOptions {
        h.state().app.session.active().unwrap().doc.data_merge.options.clone()
    }
    fn effects(h: &Harness<'_, Window>) -> Effects {
        let app = &h.state().app;
        Effects {
            active: app.session.active_index(),
            documents: app
                .session
                .documents()
                .iter()
                .map(|st| DocumentEffects {
                    uid: st.uid,
                    epoch: st.identity_epoch,
                    revision: st.revision,
                    undo: st.history.undo.len(),
                    redo: st.history.redo.len(),
                    document: st.doc.clone(),
                    saved: st.saved_doc.clone(),
                    preview: st.preview_record,
                    stash: st.preview_stash.clone(),
                    path: st.path.clone(),
                })
                .collect(),
            journal: app.session.journal.clone(),
            queued: app.file_dialogs.queued(),
            pending: app.file_dialogs_pending(),
        }
    }
    fn no_idle_replay(h: &mut Harness<'_, Window>) {
        let before = effects(h);
        idle(h, 6);
        assert_eq!(effects(h), before, "idle frames cannot replay a successful or refused action");
    }
    fn count(h: &Harness<'_, Window>, command: &str) -> usize {
        h.state().app.session.journal.iter().filter(|(id, _)| id == command).count()
    }
    fn payload(h: &Harness<'_, Window>, command: &str) -> Value {
        h.state().app.session.journal.iter().rev().find(|(id, _)| id == command).unwrap().1.clone()
    }
    fn text_target(h: &mut Harness<'_, Window>) -> u64 {
        let frame = h.state_mut().app.run("frame.create", json!({"rect": [10, 10, 120, 40], "content": "text", "text": ""})).unwrap();
        let story = frame["story"].as_u64().unwrap();
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        idle(h, 3);
        story
    }
    fn undo(h: &mut Harness<'_, Window>) {
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(h, 3);
    }

    #[test]
    fn ui223_datamerge_source_cancel_update_refusal_remove_and_undo() {
        let mut h = fixture(app(1, false), true);
        let before = effects(&h);
        click(&mut h, "Button", "Select Data Source…");
        assert_eq!(h.state().app.file_dialogs.queued(), 1);
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(effects(&h), before, "actual picker Cancel must not run data.source.select");
        click(&mut h, "Button", "Update");
        assert!(h.state().app.ui.status.contains("no file to update"), "inline sources retain the engine's Update refusal");
        assert_eq!(effects(&h), before);
        text_target(&mut h);
        click(&mut h, "Button", "Insert field: Name");
        let template = h.state().app.session.active().unwrap().doc.clone();
        let commands = count(&h, "data.source.remove");
        let history = h.state().app.session.active().unwrap().history.undo.len();
        click(&mut h, "Button", "Remove");
        assert!(h.state().app.session.active().unwrap().doc.data_merge.sources.is_empty());
        assert_eq!(
            h.state().app.session.active().unwrap().doc.data_merge.placeholders,
            template.data_merge.placeholders,
            "removing a source keeps placeholders"
        );
        assert_eq!(count(&h, "data.source.remove"), commands + 1);
        assert_eq!(payload(&h, "data.source.remove"), json!({}));
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), history + 1);
        no_idle_replay(&mut h);
        undo(&mut h);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), template.as_ref());
    }

    #[test]
    fn ui223_datamerge_field_insert_image_and_qr_bind_preserve_payload_and_undo() {
        let mut h = fixture(app(1, false), true);
        let story = text_target(&mut h);
        let history = h.state().app.session.active().unwrap().history.undo.len();
        click(&mut h, "Button", "Insert field: Name");
        assert_eq!(payload(&h, "data.placeholder.add"), json!({"field": "Name", "role": "text"}));
        assert_eq!(h.state().app.session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().text, "<<Name>>");
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), history + 1);
        no_idle_replay(&mut h);
        undo(&mut h);
        assert!(h.state().app.session.active().unwrap().doc.data_merge.placeholders.is_empty());
        let frame = h.state_mut().app.run("frame.create", json!({"rect": [20, 50, 60, 90], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        h.state_mut().app.run("selection.set", json!({"ids": [frame]})).unwrap();
        idle(&mut h, 3);
        for (field, role) in [("Photo", designcraft_doc::PlaceholderRole::Image), ("Code", designcraft_doc::PlaceholderRole::Qr)] {
            let before = h.state().app.session.active().unwrap().doc.clone();
            let history = h.state().app.session.active().unwrap().history.undo.len();
            click(&mut h, "Button", &format!("Bind to frame: {field}"));
            assert_eq!(payload(&h, "data.placeholder.add"), json!({"field": field}));
            let st = h.state().app.session.active().unwrap();
            assert_eq!(st.doc.data_merge.placeholders.len(), 1);
            assert_eq!(st.doc.data_merge.placeholders[0].role, role);
            assert_eq!(st.doc.data_merge.placeholders[0].anchor, designcraft_doc::PlaceholderAnchor::Item { id: designcraft_doc::ItemId(frame) });
            assert_eq!(st.doc.item(designcraft_doc::ItemId(frame)).unwrap().label, format!("<<{field}>>"));
            assert_eq!(st.history.undo.len(), history + 1);
            no_idle_replay(&mut h);
            undo(&mut h);
            assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), before.as_ref());
        }
    }

    #[test]
    fn ui223_datamerge_field_refusals_keep_source_document_and_history() {
        let mut h = fixture(app(1, false), true);
        let before = effects(&h);
        for name in ["Insert field: Name", "Bind to frame: Photo", "Bind to frame: Code"] {
            click(&mut h, "Button", name);
            assert!(!h.state().app.ui.status.is_empty());
            assert_eq!(effects(&h), before, "selection-less insertion/binding must preserve real engine state");
            no_idle_replay(&mut h);
        }
    }

    #[test]
    fn ui223_datamerge_preview_toggle_record_and_escape_keep_session_policy() {
        let mut h = fixture_width(app(1, false), true, 320.0);
        for name in ["First", "Previous", "Next", "Last"] {
            let navigation = control(&h, "Button", name, 0);
            assert!(navigation.rect.left() >= 0.0 && navigation.rect.right() <= 320.0, "the actual narrow-sidebar navigation must not clip: {name}");
        }
        let before = effects(&h);
        click(&mut h, "Checkbox", "Preview");
        let st = h.state().app.session.active().unwrap();
        assert_eq!(st.preview_record, Some(1));
        assert!(st.revision > before.documents[0].revision, "the preview applied (revisions are process-wide)");
        assert_eq!(st.history.undo.len(), before.documents[0].undo);
        assert_eq!(st.doc.as_ref(), before.documents[0].document.as_ref());
        assert_eq!(h.state().app.session.journal, before.journal);
        let before_number = effects(&h);
        number(&mut h, "Record", 0, "3");
        assert_eq!(h.state().app.session.active().unwrap().preview_record, Some(3));
        assert!(h.state().app.session.active().unwrap().revision > before_number.documents[0].revision, "the record preview applied");
        assert_eq!(h.state().app.session.journal, before_number.journal);
        no_idle_replay(&mut h);
        let before_bad = effects(&h);
        let number = control(&h, "NumberField", "Record", 0);
        replace(&mut h, number.rect, "not-a-number");
        key(&mut h, Key::Escape);
        assert_eq!(effects(&h), before_bad, "invalid numeric draft/Escape cannot apply a preview");
        click(&mut h, "Checkbox", "Preview");
        let st = h.state().app.session.active().unwrap();
        assert_eq!(st.preview_record, None);
        assert!(st.preview_stash.is_none());
        assert_eq!(st.doc.as_ref(), before.documents[0].document.as_ref());
        assert_eq!(st.history.undo.len(), before.documents[0].undo);
        assert_eq!(payload(&h, "data.preview.stop"), json!({}));
        assert_eq!(count(&h, "data.preview.stop"), 1, "Stop retains its existing noundo/journal contract");
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_natural_tab_dropdown_escape_and_commit_are_distinct() {
        let mut h = fixture(app(1, false), true);
        let select = control(&h, "Select", "Merge options", 0);
        tab_to(&mut h, &select);
        let before = effects(&h);
        key(&mut h, Key::Space);
        key(&mut h, Key::ArrowDown);
        assert_eq!(effects(&h), before, "navigating an open dropdown is not data.options");
        key(&mut h, Key::Escape);
        assert_eq!(effects(&h), before);
        key(&mut h, Key::Space);
        key(&mut h, Key::ArrowDown);
        key(&mut h, Key::Enter);
        assert_eq!(options(&h).records, "one");
        assert_eq!(payload(&h, "data.options"), json!({"records": "one"}));
        assert_eq!(h.state().app.session.active().unwrap().history.undo.len(), before.documents[0].undo + 1);
        no_idle_replay(&mut h);
        let center = control(&h, "Checkbox", "Center in frame", 0);
        tab_to(&mut h, &center);
        let old = options(&h).center;
        let count_before = count(&h, "data.options");
        key(&mut h, Key::Space);
        assert_eq!(options(&h).center, !old);
        assert_eq!(count(&h, "data.options"), count_before + 1);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_all_options_numbers_fitting_and_limit_remain_live() {
        let mut h = fixture(app(1, false), true);
        choose(&mut h, "Records per page", "Multiple records", true);
        assert_eq!(payload(&h, "data.options"), json!({"perPage": "multiple"}));
        choose(&mut h, "Arrangement", "Columns first", true);
        assert_eq!(options(&h).arrange, "columns");
        for (name, value) in [("Top", 12.5), ("Right", -2.0), ("Bottom", 3.0), ("Left", 4.0)] {
            let text = numeric_input(&h, value);
            number(&mut h, name, 0, &text);
        }
        assert_eq!(options(&h).insets, [12.5, -2.0, 3.0, 4.0]);
        assert_eq!(payload(&h, "data.options"), json!({"insets": [12.5, -2.0, 3.0, 4.0]}));
        let text = numeric_input(&h, -1.5);
        number(&mut h, "Column spacing", 0, &text);
        assert_eq!(payload(&h, "data.options"), json!({"columnSpacing": -1.5}));
        let text = numeric_input(&h, 2.75);
        number(&mut h, "Row spacing", 0, &text);
        assert_eq!(payload(&h, "data.options"), json!({"rowSpacing": 2.75}));
        for (name, value) in [
            ("Fill proportionally", "fillProportionally"),
            ("Fit content to frame", "fitContentToFrame"),
            ("None", "none"),
            ("Fit proportionally", "fitProportionally"),
        ] {
            choose(&mut h, "Image Fitting", name, true);
            assert_eq!(options(&h).fitting, value);
            assert_eq!(payload(&h, "data.options"), json!({"fitting": value}));
        }
        let center = options(&h).center;
        let link = options(&h).link_images;
        click(&mut h, "Checkbox", "Center in frame");
        assert_eq!(payload(&h, "data.options"), json!({"center": !center}));
        click(&mut h, "Checkbox", "Link images");
        assert_eq!(payload(&h, "data.options"), json!({"linkImages": !link}));
        number(&mut h, "Limit", 0, "100001");
        assert_eq!(options(&h).limit, Some(100_000));
        number(&mut h, "Limit", 0, "0");
        assert_eq!(options(&h).limit, None);
        assert_eq!(payload(&h, "data.options"), json!({"limit": null}));
        let top = control(&h, "NumberField", "Top", 0);
        let text = numeric_input(&h, 9.5);
        replace(&mut h, top.rect, &text);
        assert_eq!(options(&h).insets[0], 9.5, "valid numeric typing is already an engine edit before Enter");
        let live = effects(&h);
        key(&mut h, Key::Escape);
        assert_eq!(effects(&h), live, "Escape does not add a rollback command/Undo step to a live panel");
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_invalid_range_refuses_merge_then_recovers_in_order() {
        let mut h = fixture(app(1, false), true);
        let story = text_target(&mut h);
        click(&mut h, "Button", "Insert field: Name");
        choose(&mut h, "Merge options", "Range", true);
        range(&mut h, "9", true);
        assert_eq!(options(&h).range, "9", "range text is live; grammar is not prematurely refused by the widget");
        key(&mut h, Key::Escape);
        assert_eq!(options(&h).range, "9", "Escape does not invent a panel-wide rollback of live engine edits");
        let before = effects(&h);
        click(&mut h, "Button", "Create Merged Document…");
        assert!(!h.state().app.ui.status.is_empty());
        assert_eq!(effects(&h), before, "an out-of-range record must not add a document or history");
        no_idle_replay(&mut h);
        range(&mut h, "3,1-2", true);
        key(&mut h, Key::Enter);
        let template = h.state().app.session.active().unwrap().doc.clone();
        click(&mut h, "Button", "Create Merged Document…");
        let documents = h.state().app.session.documents();
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0].doc.as_ref(), template.as_ref());
        assert_eq!(h.state().app.session.active().unwrap().doc.page_count(), 3);
        assert_eq!(
            h.state().app.session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().text,
            "Katherine",
            "range order is engine-defined, not sorted by the panel"
        );
        assert_eq!(payload(&h, "data.merge"), json!({}));
        assert_eq!(count(&h, "data.merge"), 1);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_one_record_merge_preserves_template_saved_state_and_undo() {
        let mut h = fixture(app(1, false), true);
        let story = text_target(&mut h);
        let insert = control(&h, "Button", "Insert field: Name", 0);
        tab_to(&mut h, &insert);
        key(&mut h, Key::Enter);
        choose(&mut h, "Merge options", "One record", true);
        number(&mut h, "Record", 1, "2");
        assert_eq!(payload(&h, "data.options"), json!({"one": 2}));
        let before = effects(&h);
        let old_button = control(&h, "Button", "Create Merged Document…", 0);
        click(&mut h, "Button", "Create Merged Document…");
        let after = effects(&h);
        assert_eq!(after.documents.len(), 2);
        assert_eq!(after.documents[0], before.documents[0], "merge preserves the template's document, saved state, revision and undo/redo");
        let st = h.state().app.session.active().unwrap();
        assert_ne!(st.uid, before.documents[0].uid);
        assert_eq!(st.doc.page_count(), 1);
        assert_eq!(st.doc.story(designcraft_doc::StoryId(story)).unwrap().text, "Grace");
        assert!(st.doc.data_merge.placeholders.is_empty());
        assert!(st.history.undo.is_empty());
        assert!(st.path.is_none());
        assert_eq!(payload(&h, "data.merge"), json!({}));
        assert_ne!(control(&h, "Button", "Create Merged Document…", 0).id, old_button.id);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_multiple_records_tile_only_two_cards_in_memory() {
        let mut h = fixture(app(1, false), true);
        text_target(&mut h);
        click(&mut h, "Button", "Insert field: Name");
        choose(&mut h, "Records per page", "Multiple records", true);
        choose(&mut h, "Arrangement", "Columns first", true);
        number(&mut h, "Limit", 0, "2");
        let before = effects(&h);
        click(&mut h, "Button", "Create Merged Document…");
        assert_eq!(h.state().app.session.documents().len(), 2);
        assert_eq!(effects(&h).documents[0], before.documents[0]);
        let st = h.state().app.session.active().unwrap();
        assert_eq!(st.doc.page_count(), 1);
        let mut texts: Vec<_> = st.doc.stories.values().map(|story| story.text.as_str()).collect();
        texts.sort();
        assert_eq!(texts, ["Ada", "Grace"]);
        assert_eq!(count(&h, "data.merge"), 1);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_multiple_records_keeps_facing_and_page_count_refusals() {
        for (pages, facing) in [(1, true), (2, false)] {
            let mut app = app(pages, facing);
            app.run("data.options", json!({"perPage": "multiple"})).unwrap();
            let mut h = fixture(app, true);
            let before = effects(&h);
            click(&mut h, "Button", "Create Merged Document…");
            assert!(h.state().app.ui.status.contains("one page"));
            assert_eq!(effects(&h), before);
            no_idle_replay(&mut h);
            choose(&mut h, "Records per page", "Single record", true);
            choose(&mut h, "Merge options", "One record", true);
            click(&mut h, "Button", "Create Merged Document…");
            assert_eq!(h.state().app.session.documents().len(), 2);
            assert_eq!(h.state().app.session.active().unwrap().doc.page_count(), pages as usize);
            no_idle_replay(&mut h);
        }
    }

    #[test]
    fn ui223_datamerge_disabled_and_sizing_input_never_defers_workflow_actions() {
        let mut app = app(1, false);
        app.run("data.options", json!({"records": "range", "range": "1-3", "perPage": "multiple"})).unwrap();
        let mut h = fixture(app, true);
        let center = control(&h, "Checkbox", "Center in frame", 0);
        tab_to(&mut h, &center);
        let before = effects(&h);
        h.state_mut().enabled = false;
        idle(&mut h, 2);
        let controls: Vec<_> =
            h.state().controls.as_ref().unwrap().elements.iter().filter(|e| e.part.is_none() && e.facts.is_some()).cloned().collect();
        for element in controls {
            assert!(element.facts.as_ref().unwrap().disabled);
            click_rect(&mut h, element.rect);
        }
        key(&mut h, Key::Enter);
        key(&mut h, Key::Space);
        event(&mut h, Event::Text("99".into()));
        assert_eq!(effects(&h), before);
        h.state_mut().enabled = true;
        idle(&mut h, 2);
        let center = control(&h, "Checkbox", "Center in frame", 0);
        tab_to(&mut h, &center);
        h.state_mut().sizing = true;
        click_rect(&mut h, center.rect);
        key(&mut h, Key::Enter);
        key(&mut h, Key::Space);
        assert_eq!(effects(&h), before);
        h.state_mut().sizing = false;
        idle(&mut h, 6);
        assert_eq!(effects(&h), before, "disabled/sizing actions cannot fire after reenabling");
        click(&mut h, "Checkbox", "Center in frame");
        assert_eq!(options(&h).center, !before.documents[0].document.data_merge.options.center);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_held_action_cannot_follow_a_new_document_identity() {
        let mut h = fixture(app(1, false), true);
        let old = control(&h, "Checkbox", "Center in frame", 0);
        let old_doc = effects(&h).documents[0].clone();
        let pos = old.rect.center();
        event(&mut h, Event::PointerMoved(pos));
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.state_mut().app.run("file.new", json!({"pages": 1, "facingPages": false})).unwrap();
        select_csv(&mut h.state_mut().app, CSV);
        idle(&mut h, 2);
        let new = control(&h, "Checkbox", "Center in frame", 0);
        assert_ne!(new.id, old.id);
        assert!(new.rect.center().distance(pos) < 1.0, "same actual target geometry makes this an identity test, not a miss");
        let before_release = effects(&h);
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        idle(&mut h, 4);
        assert_eq!(effects(&h), before_release);
        assert_eq!(effects(&h).documents[0], old_doc);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_no_source_keeps_disabled_and_refusal_routes() {
        let mut app = app(1, false);
        app.run("data.source.remove", json!({})).unwrap();
        let mut h = fixture(app, true);
        let before = effects(&h);
        for name in ["Update", "Remove"] {
            assert!(control(&h, "Button", name, 0).facts.as_ref().unwrap().disabled);
            click(&mut h, "Button", name);
            assert_eq!(effects(&h), before);
        }
        for (component, name) in [("Checkbox", "Preview"), ("Button", "Create Merged Document…")] {
            click(&mut h, component, name);
            assert!(!h.state().app.ui.status.is_empty());
            assert_eq!(effects(&h), before, "preserve the engine refusal, not an invented disable/apply model");
        }
        click(&mut h, "Button", "Select Data Source…");
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(effects(&h), before);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_csv_and_rows_queries_use_real_adapter_without_attachment() {
        let mut h = fixture(app(1, false), true);
        let before = effects(&h);
        for params in
            [json!({"csv": "Résumé\nAda\nGrace\nKatherine\n"}), json!({"rows": [{"Résumé": "Ada"}, {"Résumé": "Grace"}, {"Résumé": "Katherine"}]})]
        {
            let fields = h.state_mut().app.run("data.fields", params).unwrap();
            assert_eq!(fields, json!([{"name": "Résumé", "kind": "text", "uses": 0}]));
            assert_eq!(effects(&h), before, "querying inline rows/csv does not silently replace the linked source");
        }
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_datamerge_native_fallback_retains_complete_live_workflow() {
        let mut app = app(1, false);
        select_csv(&mut app, "Résumé,@Photo,#Code\nAda,,one\nGrace,,two\nKatherine,,three\n");
        let mut h = fixture(app, false);
        assert!(!h.state().app.framework_installed);
        let before = effects(&h);
        native_click(&mut h, "Select Data Source…");
        h.state_mut().app.settle_dialogs();
        idle(&mut h, 3);
        assert_eq!(effects(&h), before);
        native_click(&mut h, "Update");
        assert_eq!(effects(&h), before);
        let story = text_target(&mut h);
        native_click(&mut h, "Insert field: Résumé");
        assert_eq!(payload(&h, "data.placeholder.add"), json!({"field": "Résumé", "role": "text"}));
        undo(&mut h);
        let frame = h.state_mut().app.run("frame.create", json!({"rect": [20, 50, 60, 90], "content": "graphic"})).unwrap()["id"].as_u64().unwrap();
        h.state_mut().app.run("selection.set", json!({"ids": [frame]})).unwrap();
        idle(&mut h, 3);
        for field in ["Photo", "Code"] {
            native_click(&mut h, &format!("Bind to frame: {field}"));
            assert_eq!(payload(&h, "data.placeholder.add"), json!({"field": field}));
            assert_eq!(h.state().app.session.active().unwrap().doc.data_merge.placeholders.len(), 1);
            undo(&mut h); // Never preview or merge an image placeholder in these bounded cases.
        }
        h.state_mut().app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        idle(&mut h, 3);
        native_click(&mut h, "Insert field: Résumé");
        native_click(&mut h, "Preview");
        native_number(&mut h, "Record", 0, "3");
        assert_eq!(h.state().app.session.active().unwrap().preview_record, Some(3));
        native_click(&mut h, "Preview");
        assert_eq!(h.state().app.session.active().unwrap().preview_record, None);
        choose(&mut h, "Merge options", "One record", false);
        native_number(&mut h, "Record", 1, "2");
        assert_eq!(options(&h).one, 2);
        choose(&mut h, "Merge options", "All records", false);
        assert_eq!(options(&h).records, "all");
        choose(&mut h, "Merge options", "Range", false);
        range(&mut h, "2", false);
        key(&mut h, Key::Enter);
        assert_eq!(options(&h).range, "2");
        choose(&mut h, "Records per page", "Multiple records", false);
        choose(&mut h, "Arrangement", "Columns first", false);
        choose(&mut h, "Image Fitting", "None", false);
        for name in ["Top", "Right", "Bottom", "Left", "Column spacing", "Row spacing"] {
            native_number(&mut h, name, 0, "1");
        }
        native_number(&mut h, "Limit", 0, "2");
        assert_eq!(options(&h).limit, Some(2));
        native_number(&mut h, "Limit", 0, "0");
        assert_eq!(options(&h).insets, [1.0; 4]);
        assert_eq!(options(&h).column_spacing, 1.0);
        assert_eq!(options(&h).row_spacing, 1.0);
        assert_eq!(options(&h).limit, None);
        let center = options(&h).center;
        native_click(&mut h, "Center in frame");
        assert_eq!(options(&h).center, !center);
        let link = options(&h).link_images;
        native_click(&mut h, "Link images");
        assert_eq!(options(&h).link_images, !link);
        choose(&mut h, "Records per page", "Single record", false);
        native_click(&mut h, "Preview");
        assert_eq!(h.state().app.session.active().unwrap().preview_record, Some(1));
        native_click(&mut h, "Preview");
        assert_eq!(h.state().app.session.active().unwrap().preview_record, None);
        native_click(&mut h, "Create Merged Document…");
        assert_eq!(h.state().app.session.documents().len(), 2);
        assert_eq!(h.state().app.session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().text, "Grace");
        assert_eq!(payload(&h, "data.merge"), json!({}));
        no_idle_replay(&mut h);
        let merged = h.state().app.session.active().unwrap().doc.clone();
        native_click(&mut h, "Remove");
        assert!(h.state().app.session.active().unwrap().doc.data_merge.sources.is_empty());
        let removed = effects(&h);
        native_click(&mut h, "Update");
        assert_eq!(effects(&h), removed, "native fallback preserves disabled Update");
        undo(&mut h);
        assert_eq!(h.state().app.session.active().unwrap().doc.as_ref(), merged.as_ref());
        no_idle_replay(&mut h);
    }
}
