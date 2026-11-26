//! Data Merge panel. It reads the linked source and calls commands. Opening the panel does not merge.

use egui::{Color32, Stroke, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::theme::semibold;
use std::ops::RangeInclusive;

/// The Data Merge panel: the craft framework's controls.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    framework::show(app, ui);
    super::interactive::bottom_inset(ui);
}

/// Local widget adapters only: every accepted value still calls the existing live engine
/// command below. No draft document, Apply/Cancel layer, IO protocol or Undo wrapper.
#[derive(Clone, Copy)]
struct DataMergeControls {
    document: (u64, u64),
    /// Whether the controls take input; the framework's also follow the parent Ui.
    enabled: bool,
    /// Whether number fields are as wide as their value, not a number cell: the record number in the navigation row.
    narrow_numbers: bool,
}

struct MergeNumber {
    value: Option<f64>,
}

/// A number field's rule: the values it takes, whether it steps in whole numbers, and whether its caption stands
/// before the field rather than inside it.
struct NumberSpec {
    range: Option<RangeInclusive<f64>>,
    integer: bool,
    prefix: bool,
}

impl NumberSpec {
    /// A count, a record number or a limit: whole numbers, with the caption before the field.
    fn count(range: RangeInclusive<f64>) -> NumberSpec {
        NumberSpec { range: Some(range), integer: true, prefix: true }
    }

    /// The record number in the navigation row: a whole number, with its caption after the field.
    fn record(range: RangeInclusive<f64>) -> NumberSpec {
        NumberSpec { range: Some(range), integer: true, prefix: false }
    }

    /// A measure in points, with the caption before the field: insets and spacing.
    fn measure() -> NumberSpec {
        NumberSpec { range: None, integer: false, prefix: true }
    }
}

impl DataMergeControls {
    fn id(&self, ui: &egui::Ui, kind: &str, key: &str) -> egui::Id {
        ui.make_persistent_id(("datamerge-workflow", self.document, kind, key))
    }

    fn button(&self, ui: &mut egui::Ui, key: &str, caption: &str, name: &str, enabled: bool) -> bool {
        self.face(ui, key, (caption, name), enabled, (craft_theme::ControlSize::Md, None))
    }

    /// A small button, for the source actions and the record navigation that stand in one row of the narrow panel.
    /// `tip` names a button whose caption is only a glyph.
    fn compact_button(&self, ui: &mut egui::Ui, key: &str, caption_and_name: (&str, &str), enabled: bool, tip: Option<&str>) -> bool {
        self.face(ui, key, caption_and_name, enabled, (craft_theme::ControlSize::Sm, tip))
    }

    /// An icon button of the app's glyph `glyph` (the status bar's first, prev, next and last arrows), named and tipped
    /// `name`: a record move of the navigation row.
    fn nav_button(&self, ui: &mut egui::Ui, key: &str, glyph: &'static str, name: &str) -> bool {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        let id = self.id(ui, "button", key);
        let enabled = self.enabled && ui.is_enabled();
        let face = crate::panels::tabs::TabGlyph(glyph);
        craft_ui_widgets::buttons::IconButton::new(craft_ui_widgets::primitives::IconSpec::glyph(&face))
            .label(name.to_owned())
            .tooltip(name.to_owned())
            .id(id)
            .enabled(enabled)
            .show(ui)
            .clicked()
    }

    /// A button named `name` that paints `caption`, at `size`, with a tooltip.
    fn face(
        &self,
        ui: &mut egui::Ui,
        key: &str,
        (caption, name): (&str, &str),
        enabled: bool,
        (size, tip): (craft_theme::ControlSize, Option<&str>),
    ) -> bool {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        let id = self.id(ui, "button", key);
        let enabled = enabled && self.enabled && ui.is_enabled();
        let button = craft_ui_widgets::buttons::Button::new(name.to_owned()).painted(caption.to_owned()).id(id).enabled(enabled).size(size);
        let button = match tip {
            Some(tip) => button.tooltip(tip.to_owned()),
            None => button,
        };
        button.show(ui).clicked()
    }

    fn check(&self, ui: &mut egui::Ui, key: &str, name: &str, value: &mut bool) -> bool {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        let id = self.id(ui, "check", key);
        let before = *value;
        craft_ui_widgets::choice::Checkbox::new(Bind::bound(value)).id(id).label(name.to_owned()).enabled(self.enabled && ui.is_enabled()).show(ui);
        *value != before
    }

    fn choice(&self, ui: &mut egui::Ui, key: &str, current: &str, options: &[(&str, &str)], lang: &str, name: &str) -> Option<String> {
        use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
        let id = self.id(ui, "choice", key);
        let current_index = options.iter().position(|(value, _)| *value == current).unwrap_or(usize::MAX);
        let entries: Vec<_> = options.iter().enumerate().map(|(i, (_, text))| Entry::new(i, tr(lang, text).to_owned())).collect();
        let out = craft_ui_widgets::pickers::Select::new(tr(lang, name).to_owned(), Bind::given(current_index), &entries)
            .id(id)
            .enabled(self.enabled && ui.is_enabled())
            .placeholder(current.to_owned())
            .show(ui);
        // Arrow navigation is only a proposal. Escape cannot send data.options.
        out.events.committed().and_then(|i| options.get(*i)).map(|(value, _)| (*value).to_owned()).filter(|value| value != current)
    }

    fn text(&self, ui: &mut egui::Ui, key: &str, name: &str, value: &mut String) -> bool {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _, Sizing};
        let id = self.id(ui, "text", key);
        let out = craft_ui_widgets::text::TextField::new(name.to_owned(), Bind::given(value.clone()))
            .id(id)
            .enabled(self.enabled && ui.is_enabled())
            .limit(None)
            .sizing(Sizing::FillWidth)
            .show(ui);
        // Range text was already live, including invalid grammar. Only data.merge
        // parses record ranges; do not turn this panel into an invented Apply dialog.
        if let Some(next) = out.value().filter(|next| next.as_str() != value.as_str()) {
            *value = next.clone();
            return true;
        }
        false
    }

    fn number(&self, ui: &mut egui::Ui, key: &str, name: &str, value: f64, spec: NumberSpec) -> MergeNumber {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _};
        let NumberSpec { range, integer, prefix } = spec;
        let id = self.id(ui, "number", key);
        let mut shown = None;
        craft_ui_widgets::layout_components::Row::new().id_salt(("datamerge-number", key)).show(ui, |ui, _| {
            if prefix {
                craft_ui_widgets::display::Text::new(name.to_owned()).show(ui);
            }
            let mut field = craft_ui_widgets::text::NumberField::new(name.to_owned(), Bind::given(Some(value)))
                .id(id)
                .enabled(self.enabled && ui.is_enabled())
                .step(1.0);
            if self.narrow_numbers {
                field = field.sizing(craft_ui_widgets::contract::Sizing::Content);
            }
            if let Some(range) = &range {
                field = field.range(range.clone());
            }
            let out = if integer { field.fixed_decimals(0).show(ui) } else { field.show(ui) };
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
            shown = Some(MergeNumber { value: next });
        });
        shown.unwrap_or(MergeNumber { value: None })
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

/// A data field's kind as a 16-point glyph in `rect`: a picture, a QR code, or a T for text.
fn paint_kind(painter: &egui::Painter, rect: egui::Rect, kind: &str, color: Color32) {
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

fn tr<'a>(lang: &str, s: &'a str) -> &'a str {
    crate::i18n::tr(lang, s)
}

/// The Data Merge panel on the craft framework: the live commands through the control adapters, laid out with
/// framework rows and text. Every control's id carries the document's identity.
mod framework {
    use super::{DataMergeControls, NumberSpec, paint_kind, select_source, tr};
    use crate::DesignApp;
    use crate::theme::Tokens;
    use craft_ui_widgets::contract::{Props as _, Show as _, ShowContainer as _, Sizing};
    use craft_ui_widgets::display::{Divider, Picture, Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::ThumbAspect;
    use craft_ui_widgets::style::TypeRole;
    use serde_json::{Value, json};

    fn note(ui: &mut egui::Ui, text: &str) {
        Text::new(text.to_owned()).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
    }

    fn heading(ui: &mut egui::Ui, text: &str) {
        Text::new(text.to_owned()).role(TypeRole::BodyStrong).show(ui);
    }

    pub(super) fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
        let lang = app.ui.language.clone();
        let icon = Tokens::get(ui.ctx()).icon;
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
            note(ui, tr(&lang, "No data source"));
            return;
        };
        let controls = DataMergeControls { document, enabled: true, narrow_numbers: false };
        let fields = app.session.execute("data.fields", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let max_record = (record_count as u32).max(1);
        let shown = if preview_record == 0 { 1 } else { preview_record.min(max_record) };
        // What the pass asked for, run once it is drawn.
        let mut commands: Vec<(&'static str, Value)> = Vec::new();
        let (mut select, mut jump) = (false, None);
        Stack::new().id_salt(("datamerge", document)).fill_width().show(ui, |ui, _| {
            match &source_name {
                Some(name) => Text::new(name.clone()).role(TypeRole::BodyStrong).show(ui),
                None => Text::new(tr(&lang, "No data source").to_owned()).shade(Shade::Muted).show(ui),
            };
            // Small buttons keep the three source actions on one row; it only wraps where even they do not fit.
            Row::new().id_salt("source").wrap(true).show(ui, |ui, _| {
                let select_name = tr(&lang, "Select Data Source…");
                select = controls.compact_button(ui, "select-source", (select_name, select_name), true, None);
                let has_source = source_name.is_some();
                let (update, remove) = (tr(&lang, "Update"), tr(&lang, "Remove"));
                if controls.compact_button(ui, "update-source", (update, update), has_source, None) {
                    commands.push(("data.source.update", json!({})));
                }
                if controls.compact_button(ui, "remove-source", (remove, remove), has_source, None) {
                    commands.push(("data.source.remove", json!({})));
                }
            });
            Divider::horizontal().show(ui);
            if fields.is_empty() {
                note(ui, tr(&lang, "Pass csv, rows, or bytes from the control channel."));
            }
            for field in &fields {
                let name = field["name"].as_str().unwrap_or("").to_string();
                let kind = field["kind"].as_str().unwrap_or("text").to_string();
                let uses = field["uses"].as_u64().unwrap_or(0);
                Row::new().id_salt(("datamerge-field", source_id, name.as_str(), kind.as_str())).fill_width().show(ui, |ui, _| {
                    let painted = kind.clone();
                    Picture::painter("", ThumbAspect::SQUARE, move |cx, rect| {
                        paint_kind(&cx.painter, egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0)), &painted, icon);
                    })
                    .id_salt("kind")
                    .show(ui);
                    Text::new(name.clone()).show(ui);
                    Text::new(uses.to_string()).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
                    let bind = kind == "image" || kind == "qr";
                    // The short caption shows; the action is named with the untouched field name.
                    let caption = tr(&lang, if bind { "Bind to frame" } else { "Insert field" });
                    if controls.button(ui, "insert-field", caption, &format!("{caption}: {name}"), true) {
                        let params = if bind { json!({"field": name}) } else { json!({"field": name, "role": "text"}) };
                        commands.push(("data.placeholder.add", params));
                    }
                });
            }
            Divider::horizontal().show(ui);
            let mut preview_on = preview_record > 0;
            if controls.check(ui, "preview", tr(&lang, "Preview"), &mut preview_on) {
                commands.push(if preview_on { ("data.preview", json!({"record": 1})) } else { ("data.preview.stop", json!({})) });
            }
            // The record navigation takes input only while a record is previewed. It is one row, First Previous [n]
            // Record Next Last, inside the narrow sidebar: the four moves are icon buttons with the arrows the status
            // bar's spread navigation draws, named and tipped in full, and the number is as wide as its value. Text
            // pills did not fit and cut Last off. It wraps only where even that does not fit.
            let navigation = DataMergeControls { enabled: preview_record > 0, narrow_numbers: true, ..controls };
            Row::new().id_salt("preview-navigation").wrap(true).show(ui, |ui, _| {
                for (key, glyph, label, to) in [("first", "first", "First", 1), ("previous", "prev", "Previous", shown.saturating_sub(1).max(1))] {
                    let name = tr(&lang, label);
                    if navigation.nav_button(ui, key, glyph, name) {
                        jump = Some(to);
                    }
                }
                let record = navigation.number(ui, "preview-record", tr(&lang, "Record"), shown as f64, NumberSpec::record(1.0..=max_record as f64));
                if let Some(rec) = record.value {
                    jump = Some(rec as u32);
                }
                Text::new(tr(&lang, "Record").to_owned()).show(ui);
                for (key, glyph, label, to) in
                    [("next", "next", "Next", shown.saturating_add(1).min(max_record)), ("last", "last", "Last", max_record)]
                {
                    let name = tr(&lang, label);
                    if navigation.nav_button(ui, key, glyph, name) {
                        jump = Some(to);
                    }
                }
            });
            Divider::horizontal().show(ui);
            heading(ui, tr(&lang, "Merge options"));
            let record_options = [("all", "All records"), ("one", "One record"), ("range", "Range")];
            if let Some(records) = controls.choice(ui, "dm_records", &options.records, &record_options, &lang, tr(&lang, "Merge options")) {
                commands.push(("data.options", json!({"records": records})));
            }
            if options.records == "one"
                && let Some(one) = controls
                    .number(ui, "one-record", tr(&lang, "Record"), options.one.max(1) as f64, NumberSpec::count(1.0..=max_record as f64))
                    .value
            {
                commands.push(("data.options", json!({"one": one as u32})));
            }
            if options.records == "range" {
                let mut range = options.range.clone();
                if controls.text(ui, "record-range", tr(&lang, "Range"), &mut range) {
                    commands.push(("data.options", json!({"range": range})));
                }
            }
            Text::new(tr(&lang, "Records per page").to_owned()).show(ui);
            let per_page_options = [("single", "Single record"), ("multiple", "Multiple records")];
            if let Some(per_page) = controls.choice(ui, "dm_per_page", &options.per_page, &per_page_options, &lang, tr(&lang, "Records per page")) {
                commands.push(("data.options", json!({"perPage": per_page})));
            }
            if options.per_page == "multiple" {
                let arrange_options = [("rows", "Rows first"), ("columns", "Columns first")];
                if let Some(arrange) = controls.choice(ui, "dm_arrange", &options.arrange, &arrange_options, &lang, tr(&lang, "Arrangement")) {
                    commands.push(("data.options", json!({"arrange": arrange})));
                }
                let mut insets = options.insets;
                let mut insets_changed = false;
                for (i, label) in ["Top", "Right", "Bottom", "Left"].iter().enumerate() {
                    if let Some(value) = controls.number(ui, label, tr(&lang, label), insets[i], NumberSpec::measure()).value {
                        insets[i] = value;
                        insets_changed = true;
                    }
                }
                if insets_changed {
                    commands.push(("data.options", json!({"insets": insets})));
                }
                if let Some(spacing) =
                    controls.number(ui, "column-spacing", tr(&lang, "Column spacing"), options.column_spacing, NumberSpec::measure()).value
                {
                    commands.push(("data.options", json!({"columnSpacing": spacing})));
                }
                let row_spacing = controls.number(ui, "row-spacing", tr(&lang, "Row spacing"), options.row_spacing, NumberSpec::measure());
                if let Some(spacing) = row_spacing.value {
                    commands.push(("data.options", json!({"rowSpacing": spacing})));
                }
            }
            let fitting_options = [
                ("fitProportionally", "Fit proportionally"),
                ("fillProportionally", "Fill proportionally"),
                ("fitContentToFrame", "Fit content to frame"),
                ("none", "None"),
            ];
            if let Some(fitting) = controls.choice(ui, "dm_fitting", &options.fitting, &fitting_options, &lang, "Image Fitting") {
                commands.push(("data.options", json!({"fitting": fitting})));
            }
            let mut center = options.center;
            if controls.check(ui, "center", tr(&lang, "Center in frame"), &mut center) {
                commands.push(("data.options", json!({"center": center})));
            }
            let mut link_images = options.link_images;
            if controls.check(ui, "link-images", tr(&lang, "Link images"), &mut link_images) {
                commands.push(("data.options", json!({"linkImages": link_images})));
            }
            let limit = options.limit.unwrap_or(0) as f64;
            if let Some(limit) = controls.number(ui, "limit", tr(&lang, "Limit"), limit, NumberSpec::count(0.0..=100_000.0)).value {
                let limit = limit as u32;
                commands.push(("data.options", json!({"limit": if limit == 0 { Value::Null } else { json!(limit) }})));
            }
            if controls.button(ui, "merge", tr(&lang, "Create Merged Document…"), tr(&lang, "Create Merged Document…"), true) {
                commands.push(("data.merge", json!({})));
            }
        });
        if select {
            select_source(app);
        }
        for (id, params) in commands {
            let _ = app.run(id, params);
        }
        if let Some(rec) = jump.filter(|rec| *rec != preview_record) {
            let _ = app.run("data.preview", json!({"record": rec}));
        }
    }
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
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(420.0, 700.0)).build_ui_state(
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

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "datamerge_preview_nav_port_tests.rs"]
mod datamerge_preview_nav_port_tests;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod ui223_datamerge_workflow_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded, record};
    use egui::{Event, Key, Modifiers, PointerButton, UiBuilder};
    use egui_kittest::{Harness, kittest::Queryable};
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

    fn fixture(app: DesignApp) -> Harness<'static, Window> {
        fixture_width(app, 560.0)
    }

    fn fixture_width(mut app: DesignApp, width: f32) -> Harness<'static, Window> {
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
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
                        state.number_units = Some(craft_ui_widgets::runtime::env::Env::of(ui).units());
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
    fn choose(h: &mut Harness<'_, Window>, name: &str, value: &str) {
        click(h, "Select", name);
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
    fn range(h: &mut Harness<'_, Window>, value: &str) {
        let rect = control(h, "TextField", "Range", 0).rect;
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
        let mut h = fixture(app(1, false));
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
        let mut h = fixture(app(1, false));
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
        let mut h = fixture(app(1, false));
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
        let mut h = fixture_width(app(1, false), 320.0);
        for name in ["First", "Previous", "Next", "Last"] {
            // The record moves are icon buttons, named in full.
            let navigation = control(&h, "IconButton", name, 0);
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
        let mut h = fixture(app(1, false));
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
        let mut h = fixture(app(1, false));
        choose(&mut h, "Records per page", "Multiple records");
        assert_eq!(payload(&h, "data.options"), json!({"perPage": "multiple"}));
        choose(&mut h, "Arrangement", "Columns first");
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
            choose(&mut h, "Image Fitting", name);
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
        let mut h = fixture(app(1, false));
        let story = text_target(&mut h);
        click(&mut h, "Button", "Insert field: Name");
        choose(&mut h, "Merge options", "Range");
        range(&mut h, "9");
        assert_eq!(options(&h).range, "9", "range text is live; grammar is not prematurely refused by the widget");
        key(&mut h, Key::Escape);
        assert_eq!(options(&h).range, "9", "Escape does not invent a panel-wide rollback of live engine edits");
        let before = effects(&h);
        click(&mut h, "Button", "Create Merged Document…");
        assert!(!h.state().app.ui.status.is_empty());
        assert_eq!(effects(&h), before, "an out-of-range record must not add a document or history");
        no_idle_replay(&mut h);
        range(&mut h, "3,1-2");
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
        let mut h = fixture(app(1, false));
        let story = text_target(&mut h);
        let insert = control(&h, "Button", "Insert field: Name", 0);
        tab_to(&mut h, &insert);
        key(&mut h, Key::Enter);
        choose(&mut h, "Merge options", "One record");
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
        let mut h = fixture(app(1, false));
        text_target(&mut h);
        click(&mut h, "Button", "Insert field: Name");
        choose(&mut h, "Records per page", "Multiple records");
        choose(&mut h, "Arrangement", "Columns first");
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
            let mut h = fixture(app);
            let before = effects(&h);
            click(&mut h, "Button", "Create Merged Document…");
            assert!(h.state().app.ui.status.contains("one page"));
            assert_eq!(effects(&h), before);
            no_idle_replay(&mut h);
            choose(&mut h, "Records per page", "Single record");
            choose(&mut h, "Merge options", "One record");
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
        let mut h = fixture(app);
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
        let mut h = fixture(app(1, false));
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
        let mut h = fixture(app);
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
        let mut h = fixture(app(1, false));
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
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod successor_tests;
