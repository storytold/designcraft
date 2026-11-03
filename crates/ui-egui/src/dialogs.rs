//! Modal dialogs. Each dialog keeps its fields in a JSON map so the control channel can set them
//! (`ui.dialog.set {field, value}`) and confirm them (`ui.dialog.confirm`) like a user would.

use designcraft_doc::SpreadRef;
use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::{Bound, with_path};
use crate::theme::semibold;

mod options;
mod style_cjk;
pub use options::{open_color_group_options, open_layer_options, open_swatch_options};

#[derive(Clone, Debug, Serialize)]
pub struct Dialog {
    pub id: String,
    pub fields: Map<String, Value>,
    /// The document's ruler unit: what a bare number such as `12` means in a measurement field.
    /// Set from the active document when the dialog is shown or confirmed (picas without one).
    #[serde(skip)]
    pub unit: Unit,
    /// The vertical ruler unit, for a dialog with a height field (the Rectangle dialog): kept
    /// from when it opened, so a bare `50` means what it did then. `None` falls back to `unit`.
    #[serde(skip)]
    pub vertical_unit: Option<Unit>,
}

impl Dialog {
    pub fn new(id: &str, params: Value) -> Self {
        let mut fields = match params {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let defaults = match id {
            "newDocument" => crate::new_document::defaults(),
            "goToPage" => json!({"page": "1"}),
            "epubExport" => json!({"split": false, "mode": "style", "style": designcraft_doc::BASIC_PARAGRAPH}),
            "qrCode" => json!({"type": "url", "content": "https://", "color": "[Black]"}),
            "insertTable" => json!({"bodyRows": 4, "columns": 4, "headerRows": 0, "footerRows": 0}),
            "convertTextToTable" => json!({"columnSeparator": "tab", "rowSeparator": "paragraph", "columns": 1}),
            "convertTableToText" => json!({"columnSeparator": "tab", "rowSeparator": "paragraph"}),
            "insertXref" => json!({"linkTo": "paragraph", "style": "", "target": "", "format": ""}),
            "findChange" => json!({"find": "", "change": "", "grep": false, "caseSensitive": false, "wholeWord": false, "scope": "document"}),
            "documentSetup" => json!({}),
            "pdfExport" => {
                json!({
                    "preset": "Desktop Printing",
                    "standard": "none",
                    "compressImages": false,
                    "flatten": "",
                    "spreads": false,
                    "bleed": true,
                    "marksCrop": false,
                    "marksBleed": false,
                    "marksPageInfo": false,
                    "marksWeight": "0.25",
                    "marksOffset": "6",
                    "tagged": true
                })
            }
            _ => json!({}),
        };
        if let Value::Object(d) = defaults {
            for (k, v) in d {
                fields.entry(k).or_insert(v);
            }
        }
        if id == "footnoteOptions" {
            // Flatten the rule and show special characters as InDesign metacharacters.
            if let Some(Value::Object(r)) = fields.remove("rule") {
                for (k, v) in r {
                    fields.insert(format!("rule.{k}"), v);
                }
            }
            let sep = fields.get("separator").and_then(Value::as_str).unwrap_or("").to_string();
            fields.insert("separator".into(), json!(sep.replace('\t', "^t").replace('\u{2003}', "^m").replace('\u{2002}', "^>")));
            fields.entry("tab".to_string()).or_insert(json!("numbering"));
        }
        if id == "paragraphStyleOptions" || id == "characterStyleOptions" {
            fields.entry("section".to_string()).or_insert(json!("general"));
        }
        if id == "ruby" {
            fields.entry("section".to_string()).or_insert(json!("cjk.rubyPlacement"));
        }
        Dialog { id: id.into(), fields, unit: Unit::Picas, vertical_unit: None }
    }
    /// Set a field and report a genuine Find domain/mode transition to the caller.
    pub(crate) fn set_field(&mut self, key: &str, value: Value) -> bool {
        let before =
            (self.id == "findChange" && matches!(key, "scope" | "story" | "objectMode")).then(|| (self.b("objectMode"), find_change_params(self)));
        self.fields.insert(key.into(), value);
        before.is_some_and(|(object_mode, params)| {
            object_mode != self.b("objectMode") || !designcraft_engine::cmd::same_find_domain(&params, &find_change_params(self))
        })
    }

    pub(crate) fn s(&self, k: &str) -> String {
        match self.fields.get(k) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }
    fn m(&self, k: &str) -> Option<f64> {
        self.m_in(k, self.unit)
    }
    /// A measure where a bare number is in `unit`.
    fn m_in(&self, k: &str, unit: Unit) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => parse_measure(s, unit).ok(),
            _ => None,
        }
    }
    /// A measure in points ("6", "6 pt", "0p6", "2 mm").
    fn pt(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => parse_measure(s, Unit::Points).ok(),
            _ => None,
        }
    }
    pub(crate) fn b(&self, k: &str) -> bool {
        self.fields.get(k).and_then(Value::as_bool).unwrap_or(false)
    }
    pub(crate) fn n(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

fn text_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str, w: f32) {
    let mut s = d.s(key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(w).align(egui::Align2::LEFT_TOP)).changed() {
        d.fields.insert(key.into(), Value::String(s));
    }
}

/// [`text_field`] with the visible caption's text as its accessible name (a text field with no
/// label, hint or `labelled_by` is an unnamed input).
fn text_field_named(ui: &mut egui::Ui, d: &mut Dialog, key: &str, w: f32, name: &str) {
    let mut s = d.s(key);
    let resp = ui.add(egui::TextEdit::singleline(&mut s).desired_width(w).align(egui::Align2::LEFT_TOP)).accessible_name(name.to_owned());
    if resp.changed() {
        d.fields.insert(key.into(), Value::String(s));
    }
}

/// Insert Pages…, after page `abs` and with that page's parent, as InDesign's.
pub(crate) fn open_insert_pages(app: &mut DesignApp, abs: usize) {
    let Some(st) = app.session.active() else { return };
    let abs = abs.min(st.doc.page_count().saturating_sub(1));
    let parent = st.doc.page(abs).and_then(|p| p.parent).map_or_else(String::new, |id| id.0.to_string());
    app.ui.dialog = Some(Dialog::new("insertPages", json!({"count": 1, "insert": "after", "page": abs, "parent": parent})));
}

/// Insert Pages, as InDesign's: how many, where (after or before a page, at the start or the end
/// of the document) and with which parent.
fn insert_pages_dialog(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let Some(st) = app.session.active() else { return };
    let pages: Vec<String> = (0..st.doc.page_count()).map(|i| app.session.page_label(i)).collect();
    let mut parents = vec![(String::new(), crate::i18n::tr(&lang, "[None]").to_string())];
    parents.extend(st.doc.parents.iter().map(|p| (p.id.0.to_string(), p.parent.as_ref().map(|i| i.label()).unwrap_or_default())));
    egui::Grid::new("insert_pages").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        let name = crate::rtl::label(ui, crate::i18n::tr(&lang, "Pages:"));
        let mut n = d.n("count").unwrap_or(1.0).clamp(1.0, 9999.0) as u32;
        if ui.add(egui::DragValue::new(&mut n).range(1..=9999)).labelled_by(name.id).changed() {
            d.fields.insert("count".into(), json!(n));
        }
        ui.end_row();
        let name = crate::rtl::label(ui, crate::i18n::tr(&lang, "Insert:"));
        ui.horizontal(|ui| {
            let wheres = [("after", "After Page"), ("before", "Before Page"), ("start", "At Start of Document"), ("end", "At End of Document")];
            translated_combo(&lang, ui, d, "insert", &wheres).labelled_by(name.id);
            // The page, by name: At Start and At End need none.
            ui.add_enabled_ui(matches!(d.s("insert").as_str(), "after" | "before"), |ui| {
                let cur = (d.n("page").unwrap_or(0.0).max(0.0) as usize).min(pages.len().saturating_sub(1));
                craft_ui::ComboBox::from_id_salt("insert_page")
                    .selected_text(pages.get(cur).cloned().unwrap_or_default())
                    .width(64.0)
                    .show_ui(ui, |ui| {
                        for (i, name) in pages.iter().enumerate() {
                            if ui.selectable_label(i == cur, name).clicked() {
                                d.fields.insert("page".into(), json!(i));
                            }
                        }
                    })
                    .response
                    .accessible_name(crate::i18n::tr(&lang, "Page").to_owned());
            });
        });
        ui.end_row();
        let name = crate::rtl::label(ui, crate::i18n::tr(&lang, "Parent:"));
        let opts: Vec<(&str, &str)> = parents.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        combo(ui, d, "parent", &opts).labelled_by(name.id);
        ui.end_row();
    });
}
/// The temporary-data key the Printing Instructions dialog sets, each frame, while one of its
/// multi-line fields has focus: Enter then makes a new line there, and the footer doesn't take it
/// for OK. No other dialog changes how Enter works.
const MULTILINE_FOCUS: &str = "dialog_multiline_focus";

pub(crate) fn set_multiline_focus(ctx: &egui::Context) {
    ctx.data_mut(|m| m.insert_temp(egui::Id::unique(MULTILINE_FOCUS), true));
}

/// Whether Enter in dialog `id` goes to a multi-line field this frame. The flag is read (and cleared)
/// for every dialog, but only the Printing Instructions dialog honours it.
fn take_multiline_focus(ctx: &egui::Context, id: &str) -> bool {
    let set = ctx.data_mut(|m| m.remove_temp::<bool>(egui::Id::unique(MULTILINE_FOCUS))).unwrap_or(false);
    set && id == crate::package_dialog::INSTRUCTIONS
}

/// Separators the Convert dialogs offer by name; anything else typed in the field is the
/// separator's own characters.
const SEPARATORS: [(&str, &str); 3] = [("tab", "Tab"), ("comma", "Comma"), ("paragraph", "Paragraph")];

/// The characters a separator field stands for (`table.convertFromText` resolves it the same
/// way), so "comma" and a typed "," are the same separator.
fn resolved_separator(v: &str) -> &str {
    match v {
        "tab" => "\t",
        "comma" => ",",
        "paragraph" => "\n",
        other => other,
    }
}

/// Whether the Convert dialog's two separators are the same (Number of Columns then applies).
fn same_separators(d: &Dialog) -> bool {
    resolved_separator(&d.s("columnSeparator")) == resolved_separator(&d.s("rowSeparator"))
}

/// A separator field, as InDesign's: type any characters, or pick Tab, Comma or Paragraph from
/// its menu. Stored as `tab`/`comma`/`paragraph` or the typed text.
fn separator_field(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, key: &str) {
    let cur = d.s(key);
    let name = |v: &str| SEPARATORS.iter().find(|s| s.0 == v).map(|s| crate::i18n::tr(lang, s.1).to_string());
    let mut shown = name(&cur).unwrap_or_else(|| cur.clone());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if ui.add(egui::TextEdit::singleline(&mut shown).id_salt(("sep", key)).desired_width(150.0)).changed() {
            let v = SEPARATORS.iter().find(|s| crate::i18n::tr(lang, s.1) == shown).map_or(shown.clone(), |s| s.0.to_string());
            d.fields.insert(key.into(), json!(v));
        }
        ui.menu_button("▾", |ui| {
            for (v, label) in SEPARATORS {
                if ui.selectable_label(cur == v, crate::i18n::tr(lang, label)).clicked() {
                    d.fields.insert(key.into(), json!(v));
                    ui.close();
                }
            }
        });
    });
}

/// Table › Convert Text to Table and Convert Table to Text, laid out as InDesign's: Column and
/// Row Separator; converting text to a table also has Number of Columns (only when both
/// separators are the same: the cells then fill rows of that many) and Table Style.
fn convert_table_dialog(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = &app.ui.language;
    let to_table = d.id == "convertTextToTable";
    let label = |ui: &mut egui::Ui, text: &str, on: bool| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled(on, egui::Label::new(crate::rtl::widget(ui, format!("{}:", crate::i18n::tr(lang, text)))));
        });
    };
    egui::Grid::new(("convert_table", to_table)).num_columns(2).min_col_width(120.0).spacing([8.0, 6.0]).show(ui, |ui| {
        label(ui, "Column Separator", true);
        separator_field(lang, ui, d, "columnSeparator");
        ui.end_row();
        label(ui, "Row Separator", true);
        separator_field(lang, ui, d, "rowSeparator");
        ui.end_row();
        if !to_table {
            return;
        }
        let same = same_separators(d);
        label(ui, "Number of Columns", same);
        let max = designcraft_engine::cmd::table::MAX_COLUMNS as f64;
        let field = crate::widgets::NumField::number("columns", d.n("columns"), "", 0).width(70.0).range(1.0, max).spinner().enabled(same);
        if let Some(v) = field.show(ui) {
            d.fields.insert("columns".into(), json!(v.round()));
        }
        ui.end_row();
        label(ui, "Table Style", true);
        let names: Vec<String> = app.session.active().map(|st| st.doc.styles.table.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
        let cur = table_style_field(d).unwrap_or_else(|| designcraft_doc::BASIC_TABLE.to_string());
        craft_ui::ComboBox::from_id_salt("convert_table_style").selected_text(&cur).width(170.0).show_ui(ui, |ui| {
            for n in &names {
                if ui.selectable_label(*n == cur, n).clicked() {
                    d.fields.insert("tableStyle".into(), json!(n));
                }
            }
        });
        ui.end_row();
    });
}

/// The dialog's chosen table style: none when unset, null or empty (a new table's [Basic Table]).
fn table_style_field(d: &Dialog) -> Option<String> {
    d.fields.get("tableStyle").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// The dialog's `tableStyle` as the table commands take it: null for none, the style's name, or
/// a value that is neither (set through `ui.dialog.set`) as it is, for the command to refuse.
fn table_style_value(d: &Dialog) -> Value {
    match d.fields.get("tableStyle") {
        None | Some(Value::String(_)) => table_style_field(d).map_or(Value::Null, Value::String),
        Some(v) => v.clone(),
    }
}

/// Table › Insert Table, laid out as InDesign's: Table Dimensions (body rows and columns, then
/// header and footer rows, each a number with a spinner, labels on the right edge of their
/// column) and the Table Style to apply.
fn insert_table_dialog(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = &app.ui.language;
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Table Dimensions")).font(semibold(12.0)));
    use designcraft_engine::cmd::table::{MAX_BODY_ROWS, MAX_COLUMNS, MAX_HEADER_FOOTER_ROWS};
    // `table.insert`'s limits, so the table made is the one the fields show.
    let rows = [
        ("bodyRows", crate::i18n::tr(lang, "Body Rows"), 1.0, MAX_BODY_ROWS),
        ("columns", crate::i18n::tr_context(lang, "Columns", "table"), 1.0, MAX_COLUMNS),
        ("headerRows", crate::i18n::tr(lang, "Header Rows"), 0.0, MAX_HEADER_FOOTER_ROWS),
        ("footerRows", crate::i18n::tr(lang, "Footer Rows"), 0.0, MAX_HEADER_FOOTER_ROWS),
    ];
    // Body rows and columns, then (after a gap) header and footer rows; one label width for both.
    for (g, pair) in rows.chunks(2).enumerate() {
        if g > 0 {
            ui.add_space(8.0);
        }
        egui::Grid::new(("ins_table", g)).num_columns(2).min_col_width(100.0).spacing([8.0, 6.0]).show(ui, |ui| {
            for (key, label, min, max) in pair {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| crate::rtl::label(ui, format!("{label}:")));
                let field = crate::widgets::NumField::number(key, d.n(key), "", 0).width(70.0).range(*min, *max as f64).spinner();
                if let Some(v) = field.show(ui) {
                    d.fields.insert((*key).into(), json!(v.round()));
                }
                ui.end_row();
            }
        });
    }
    ui.add_space(8.0);
    let names: Vec<String> = app.session.active().map(|st| st.doc.styles.table.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
    let cur = table_style_field(d).unwrap_or_else(|| designcraft_doc::BASIC_TABLE.to_string());
    ui.horizontal(|ui| {
        crate::rtl::label(ui, format!("{}:", crate::i18n::tr(lang, "Table Style")));
        craft_ui::ComboBox::from_id_salt("ins_table_style").selected_text(&cur).width(160.0).show_ui(ui, |ui| {
            for n in &names {
                if ui.selectable_label(*n == cur, n).clicked() {
                    d.fields.insert("tableStyle".into(), json!(n));
                }
            }
        });
    });
}

/// Extract once when opened. Cancel is read-only; confirmation adds exactly these colours.
pub(crate) fn open_color_theme(app: &mut DesignApp, params: Value) {
    match app.run("color.extractTheme", params) {
        Ok(mut theme) => {
            theme["name"] = json!("Color Theme");
            app.ui.dialog = Some(Dialog::new("colorTheme", theme));
        }
        Err(message) => app.ui.dialog = Some(Dialog::new("alert", json!({"message": message}))),
    }
}

fn color_theme(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name:"));
    text_field(ui, d, "name", 280.0);
    ui.add_space(8.0);
    if let Some(colors) = d.fields.get("colors").and_then(Value::as_array) {
        ui.horizontal(|ui| {
            for hex in colors.iter().filter_map(Value::as_str).take(5) {
                ui.vertical(|ui| {
                    if let Ok(color) = egui::Color32::from_hex(hex) {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(54.0, 40.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, color);
                    }
                    ui.label(hex);
                });
            }
        });
    }
    if let Some(status) = d.fields.get("status").and_then(Value::as_str) {
        crate::rtl::label(ui, status);
    }
}

#[cfg(test)]
mod color_theme_tests {
    use super::*;

    /// Enter in a multi-line field makes a new line in the Printing Instructions only; every other
    /// dialog keeps Enter as OK, and the flag does not outlive the frame.
    #[test]
    fn enter_in_a_multiline_field_is_local_to_the_printing_instructions() {
        let ctx = egui::Context::default();
        set_multiline_focus(&ctx);
        assert!(take_multiline_focus(&ctx, crate::package_dialog::INSTRUCTIONS));
        assert!(!take_multiline_focus(&ctx, crate::package_dialog::INSTRUCTIONS), "cleared once read");
        set_multiline_focus(&ctx);
        assert!(!take_multiline_focus(&ctx, "insertPages"));
    }

    /// A page past the end shows as the last page in the dialog, and Before Page puts the new
    /// page before that one rather than at the end.
    #[test]
    fn insert_pages_before_a_page_past_the_end_goes_before_the_last() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 4, "facingPages": false})).unwrap();
        let ids = |app: &DesignApp| {
            let d = &app.session.doc().unwrap().doc;
            (0..d.page_count()).map(|i| d.page(i).unwrap().id).collect::<Vec<_>>()
        };
        let old = ids(&app);
        app.ui.dialog = Some(Dialog::new("insertPages", json!({"count": 1, "insert": "before", "page": 100, "parent": ""})));
        confirm(&mut app).unwrap();
        let now = ids(&app);
        assert_eq!(now.len(), 5);
        assert_eq!((now[..3].to_vec(), now[4]), (old[..3].to_vec(), old[3]), "the new page is fourth, before the old last page");
    }

    /// The course's chapter 25: Insert Pages as InDesign's. It opens after the page in view with
    /// that page's parent; At Start of Document with B-Cover puts a B page first, and Before Page
    /// with [None] puts a page without a parent before the page chosen.
    #[test]
    fn insert_pages_goes_before_a_page_or_at_the_start_with_the_chosen_parent() {
        use egui::accesskit::Role;
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        // Facing: [1] [2 3] [4].
        app.run("file.new", json!({"pages": 4})).unwrap();
        let b = app.run("layout.parents.new", json!({"prefix": "B", "name": "Cover"})).unwrap()["id"].as_u64().unwrap();
        let ids = |app: &DesignApp| -> Vec<u64> {
            let d = &app.session.doc().unwrap().doc;
            (0..d.page_count()).map(|i| d.page(i).unwrap().id.0).collect()
        };
        let old = ids(&app);
        crate::menus::activate(&mut app, "layout.pages.insert", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "insertPages");
        let mut h = Harness::builder().with_size(egui::vec2(1000.0, 700.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let key = egui::Id::unique("insert_pages_test_fonts");
                if ui.data(|d| d.get_temp::<bool>(key)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(key, true));
                    return;
                }
                show(app, ui.ctx());
            },
            app,
        );
        h.run_steps(3);
        for l in ["Pages:", "Insert:", "Parent:"] {
            assert!(h.query_by_label(l).is_some(), "{l}");
        }
        // The dropdown showing `shown`, then its item `choice`.
        let pick = |h: &mut Harness<'_, DesignApp>, shown: &str, choice: &str| {
            h.get_by(|n| n.role() == Role::ComboBox && n.value().as_deref() == Some(shown)).click();
            h.run_steps(2);
            h.get_by_label(choice).click();
            h.run_steps(2);
        };
        // After page 1 with A, at first; then At Start of Document, with B-Cover.
        pick(&mut h, "After Page", "At Start of Document");
        pick(&mut h, "A-Parent", "B-Cover");
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        assert!(h.state().ui.dialog.is_none());
        let now = ids(h.state());
        assert_eq!(now[1..], old[..], "the new page is first");
        let parent = |app: &DesignApp, i: usize| app.session.doc().unwrap().doc.page(i).unwrap().parent.map(|p| p.0);
        assert_eq!(parent(h.state(), 0), Some(b));
        // Before page 4, with [None].
        crate::dialogs::open_insert_pages(h.state_mut(), 0);
        h.run_steps(3);
        pick(&mut h, "After Page", "Before Page");
        pick(&mut h, "1", "4");
        pick(&mut h, "B-Cover", "[None]");
        h.get_by_label("  OK  ").click();
        h.run_steps(3);
        let then = ids(h.state());
        assert_eq!(then.len(), 6);
        assert_eq!(then[..3], now[..3], "the pages before it stay");
        assert_eq!(then[4..], now[3..], "page 4 and those after it follow the new page");
        assert_eq!(parent(h.state(), 3), None);
    }

    /// The course's chapter 18: Text Frame Options with Preview on shows the columns and the
    /// inset on the page while the dialog is open; Cancel puts the frame back, OK sets them as one
    /// undo step. The Paragraph Spacing Limit is set from the dialog too.
    #[test]
    fn text_frame_options_preview_shows_the_change_until_ok_or_cancel() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        let opts = |app: &DesignApp| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
        let open = |app: &mut DesignApp| {
            let mut d = Dialog::new("textFrameOptions", json!({}));
            seed_text_frame_options(app, &mut d);
            d
        };
        let mut d = open(&mut app);
        d.fields.insert("columns".into(), json!("3"));
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 3, "previewed on the page");
        d.fields.insert("insetTop".into(), json!("1p0"));
        text_frame_preview(&mut app, &mut d);
        assert_eq!((opts(&app).columns, opts(&app).inset[0]), (3, 12.0), "redone when a field changes");
        d.fields.insert("preview".into(), json!(false));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 1, "Preview off: the page as it was");
        // A value OK would refuse: the page goes back while the status says why.
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 3);
        let gutter = d.fields.get("gutter").cloned().unwrap_or_default();
        d.fields.insert("gutter".into(), json!("wide"));
        text_frame_preview(&mut app, &mut d);
        assert_eq!(opts(&app).columns, 1, "the page as it was");
        assert!(!d.fields.contains_key(PREVIEWED) && !d.s("status").is_empty());
        d.fields.insert("gutter".into(), gutter);
        d.fields.remove("status");
        // Cancel after a preview: back as it was.
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        app.ui.dialog = Some(d);
        cancel(&mut app);
        assert_eq!(opts(&app).columns, 1);
        // OK after a preview: set once, one undo step.
        let mut d = open(&mut app);
        d.fields.insert("columns".into(), json!("2"));
        d.fields.insert("verticalJustification".into(), json!("justify"));
        d.fields.insert("paragraphSpacingLimit".into(), json!("2p0"));
        d.fields.insert("preview".into(), json!(true));
        text_frame_preview(&mut app, &mut d);
        app.ui.dialog = Some(d);
        confirm(&mut app).unwrap();
        let o = opts(&app);
        assert_eq!((o.columns, o.vj_paragraph_spacing_limit), (2, 24.0));
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(opts(&app).columns, 1, "one undo step");
    }

    #[test]
    fn toc_right_indent_separator_survives_the_dialog_and_update() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.newSample", json!({})).unwrap();
        open_toc(&mut app);
        app.ui.dialog.as_mut().unwrap().fields.insert(
            "included".into(),
            json!([
                {"style": "Headline", "level": 1, "separator": " ^y "}
            ]),
        );
        confirm(&mut app).unwrap();
        if app.session.tool_id() == "tocPlace" {
            app.run("toc.place", json!({"x":36,"y":36})).unwrap();
        }
        let separator = format!(" {} ", designcraft_doc::story::RIGHT_INDENT_TAB);
        let d = &app.session.doc().unwrap().doc;
        let toc = d.toc.as_ref().unwrap();
        assert_eq!(toc.entries[0].separator.as_deref(), Some(separator.as_str()));
        assert!(d.story(toc.story).unwrap().text.contains(&separator));
        app.run("toc.update", json!({})).unwrap();
        open_toc(&mut app);
        assert_eq!(toc_rows(app.ui.dialog.as_ref().unwrap())[0].separator, " ^y ");
        for text in ["^y", "^t^y", "^m^>", "unknown ^q and trailing ^", "a\t\u{E006}\u{2003}\u{2002}z"] {
            assert_eq!(toc_separator_typed(&toc_separator_shown(text)), text);
        }
        // The picker after a literal caret: the caret stays, and the code is the code.
        for (field, typed) in [("^", "^\u{E006}"), ("^^", "^\u{E006}"), ("a", "a\u{E006}"), ("^^^", "^^\u{E006}")] {
            let mut f = field.to_string();
            toc_separator_append(&mut f, "^y");
            assert_eq!(toc_separator_typed(&f), typed, "{field:?} → {f:?}");
        }
    }

    #[test]
    fn color_theme_cancel_is_read_only_and_confirm_adds_the_preview() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let before = app.session.active().unwrap().doc.clone();
        let theme = json!({"name":"Fashion", "colors":["#102030", "#f0a020"]});
        app.ui.dialog = Some(Dialog::new("colorTheme", theme.clone()));
        cancel(&mut app);
        assert_eq!(app.session.active().unwrap().doc, before);
        app.ui.dialog = Some(Dialog::new("colorTheme", theme));
        confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.color_groups.last().unwrap().name, "Fashion");
        assert_eq!(doc.color_groups.last().unwrap().swatches.len(), 2);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.active().unwrap().doc, before);
        app.ui.dialog = Some(Dialog::new("colorTheme", json!({"name":" ","colors":["#102030"]})));
        assert!(confirm(&mut app).is_err());
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "colorTheme");
        assert_eq!(app.session.active().unwrap().doc, before);
    }
}

/// Edit › Spelling › User Dictionary: added words and hyphenation exceptions (applied at once).
fn user_dictionary(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Hyphenation Exceptions")).font(semibold(12.0)));
    ui.label(crate::rtl::widget(
        ui,
        egui::RichText::new(crate::i18n::tr(
            &app.ui.language,
            "Type a word with ~ at each allowed break (ex~am~ple); without ~ the word is never hyphenated.",
        ))
        .size(10.5),
    ));
    ui.horizontal(|ui| {
        text_field(ui, d, "word", 200.0);
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Add"))).clicked() {
            let w = d.s("word");
            if !w.trim().is_empty() {
                match app.run("hyphenation.addException", json!({"word": w.trim()})) {
                    Ok(_) => {
                        d.fields.insert("word".into(), json!(""));
                    }
                    Err(e) => app.status_error("User Dictionary", e),
                }
            }
        }
    });
    let list = app.session.execute("hyphenation.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
        for w in list {
            let w = w.as_str().unwrap_or("").to_string();
            ui.horizontal(|ui| {
                ui.label(&w);
                if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Remove"))).clicked() {
                    let _ = app.run("hyphenation.removeException", json!({"word": w}));
                }
            });
        }
    });
}

/// File › Print, with the printers the system knows.
pub fn open_print(app: &mut DesignApp) {
    let p = app.session.execute("file.printers", &json!({})).unwrap_or_default();
    let printer = p["default"].as_str().map(str::to_string).or_else(|| p["printers"][0].as_str().map(str::to_string)).unwrap_or_default();
    app.ui.dialog = Some(Dialog::new(
        "print",
        json!({"printers": p["printers"], "printer": printer, "copies": 1, "range": "all", "pages": "1", "spreads": false, "marks": false, "bleed": false}),
    ));
}

/// File › Export PDF…: the "Export PDF" options dialog, seeded from the persisted settings
/// (`UiState.pdf_export`). `pages` is per-job (not persisted); `flatten` falls back to the global
/// flattener preset when the persisted value is empty. `title` starts empty: a title given to the
/// dialog is the PDF's title and names the file, and without one the file is named after the
/// document and the PDF takes its title from File Info.
pub fn open_pdf_export(app: &mut DesignApp) {
    let mut fields = serde_json::to_value(&app.ui.pdf_export).unwrap_or_else(|_| json!({}));
    if let Value::Object(m) = &mut fields {
        m.insert("pages".into(), json!("all"));
        if app.ui.pdf_export.flatten.is_empty() {
            m.insert("flatten".into(), json!(app.ui.flattener));
        }
    }
    app.ui.dialog = Some(Dialog::new("pdfExport", fields));
}

/// File › Export EPUB…: the EPUB Export Options dialog, seeded from the document's Split
/// Document setting.
pub fn open_epub_export(app: &mut DesignApp) {
    use designcraft_doc::EpubSplit;
    let split = app.session.active().map(|st| st.doc.settings.epub_split.clone()).unwrap_or_default();
    let fields = match split {
        EpubSplit::Off => json!({}),
        EpubSplit::Style(name) => json!({"split": true, "mode": "style", "style": name}),
        EpubSplit::ExportTags => json!({"split": true, "mode": "exportTags"}),
    };
    app.ui.dialog = Some(Dialog::new("epubExport", fields));
}

/// The Split Document setting the EPUB Export Options dialog's fields describe.
fn epub_export_split(d: &Dialog) -> designcraft_doc::EpubSplit {
    use designcraft_doc::EpubSplit;
    if !d.b("split") {
        EpubSplit::Off
    } else if d.s("mode") == "exportTags" {
        EpubSplit::ExportTags
    } else {
        EpubSplit::Style(d.s("style"))
    }
}

fn epub_export(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let styles: Vec<String> = app
        .session
        .active()
        .map(|st| st.doc.styles.paragraph.iter().map(|s| s.name.clone()).filter(|n| n != designcraft_doc::NO_PARA_STYLE).collect())
        .unwrap_or_default();
    check(ui, d, "split", crate::i18n::tr(lang, "Split Document"));
    ui.add_enabled_ui(d.b("split"), |ui| {
        ui.indent("epub_split", |ui| {
            let mode = d.s("mode");
            ui.horizontal(|ui| {
                if ui.radio(mode != "exportTags", crate::rtl::widget(ui, crate::i18n::tr(lang, "Single Paragraph Style"))).clicked() {
                    d.fields.insert("mode".into(), json!("style"));
                }
                let cur = d.s("style");
                craft_ui::ComboBox::from_id_salt("epub_split_style").selected_text(crate::rtl::widget(ui, &cur)).width(180.0).show_ui(ui, |ui| {
                    for name in &styles {
                        if ui.selectable_label(cur == *name, crate::rtl::widget(ui, name)).clicked() {
                            d.fields.insert("style".into(), json!(name));
                            d.fields.insert("mode".into(), json!("style"));
                        }
                    }
                });
            });
            if ui.radio(mode == "exportTags", crate::rtl::widget(ui, crate::i18n::tr(lang, "Based on Paragraph Style Export Tags"))).clicked() {
                d.fields.insert("mode".into(), json!("exportTags"));
            }
        });
    });
}

fn print_dialog(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let printers: Vec<String> = d
        .fields
        .get("printers")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    egui::Grid::new("print").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Printer:"));
        let cur = d.s("printer");
        if printers.is_empty() {
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "System default")).italics());
        } else {
            craft_ui::ComboBox::from_id_salt("printer").selected_text(&cur).width(220.0).show_ui(ui, |ui| {
                for p in &printers {
                    if ui.selectable_label(*p == cur, p).clicked() {
                        d.fields.insert("printer".into(), json!(p));
                    }
                }
            });
        }
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Copies:"));
        text_field(ui, d, "copies", 50.0);
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Pages:"));
        ui.vertical(|ui| {
            let range = d.s("range");
            if ui.radio(range == "all", crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "All"))).clicked() {
                d.fields.insert("range".into(), json!("all"));
            }
            ui.horizontal(|ui| {
                if ui.radio(range == "range", crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Range:"))).clicked() {
                    d.fields.insert("range".into(), json!("range"));
                }
                text_field(ui, d, "pages", 120.0);
            });
        });
        ui.end_row();
        ui.label("");
        ui.vertical(|ui| {
            check(ui, d, "spreads", crate::i18n::tr(&app.ui.language, "Spreads"));
            check(ui, d, "marks", crate::i18n::tr(&app.ui.language, "Printer's Marks"));
            check(ui, d, "bleed", crate::i18n::tr(&app.ui.language, "Include Bleed"));
        });
        ui.end_row();
    });
}

/// The presets offered by the Export PDF dialog, in display order. The stored value is the
/// English name; the caption is translated.
const PDF_EXPORT_PRESETS: &[&str] =
    &["Desktop Printing", "Commercial Printing", "Screen and Email", "PDF/X-1a:2003", "PDF/X-4", "PDF/A-2b", "Custom"];

/// The `file.exportPdf` options the Export PDF dialog's fields stand for (pages and title aside).
fn pdf_export_params(d: &Dialog) -> Value {
    let mut params = json!({
        "standard": d.s("standard"),
        "compressImages": d.b("compressImages"),
        "spreads": d.b("spreads"),
        "bleed": d.b("bleed"),
        "tagged": d.b("tagged"),
        "marks": {
            "crop": d.b("marksCrop"),
            "bleed": d.b("marksBleed"),
            "pageInfo": d.b("marksPageInfo"),
            "weight": d.pt("marksWeight").unwrap_or(0.25),
            "offset": d.pt("marksOffset").unwrap_or(6.0),
        },
    });
    let flatten = d.s("flatten");
    if !flatten.is_empty() {
        params["flatten"] = json!(flatten);
    }
    params
}

/// The PDF presets a document can be exported with, by name (Custom aside).
pub(crate) fn pdf_presets() -> impl Iterator<Item = &'static str> {
    PDF_EXPORT_PRESETS.iter().copied().filter(|p| *p != "Custom")
}

/// The `file.exportPdf` options of PDF preset `preset`, as the Export PDF dialog sets them.
pub(crate) fn pdf_preset_params(preset: &str) -> Value {
    let mut d = Dialog::new("pdfExport", json!({}));
    apply_pdf_preset(&mut d, preset);
    pdf_export_params(&d)
}

/// Apply a preset's dependent fields. Flatten is only touched by "Screen and Email" and
/// "PDF/X-1a:2003" (High Resolution); "Custom" (and any unknown value) changes nothing.
fn apply_pdf_preset(d: &mut Dialog, preset: &str) {
    let (standard, compress, crop, bleed, page_info, flatten) = match preset {
        "Desktop Printing" => ("none", false, false, false, false, None),
        "Commercial Printing" => ("none", true, true, true, false, None),
        "Screen and Email" => ("none", true, false, false, false, Some("low")),
        "PDF/X-1a:2003" => ("x1a", false, true, true, false, Some("high")),
        "PDF/X-4" => ("x4", false, true, true, false, None),
        "PDF/A-2b" => ("a2b", false, false, false, false, None),
        _ => return,
    };
    d.fields.insert("standard".into(), json!(standard));
    d.fields.insert("compressImages".into(), json!(compress));
    d.fields.insert("marksCrop".into(), json!(crop));
    d.fields.insert("marksBleed".into(), json!(bleed));
    d.fields.insert("marksPageInfo".into(), json!(page_info));
    if let Some(f) = flatten {
        d.fields.insert("flatten".into(), json!(f));
    }
}

/// File › Export PDF: the "Export PDF" options (General / Compression / Marks and Bleeds /
/// Advanced). Selecting a preset fills the dependent fields; editing any other control flips the
/// preset back to "Custom".
fn pdf_export(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let head = |ui: &mut egui::Ui, t: &str| {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, t)).font(semibold(12.0)));
    };
    // Snapshot the non-preset fields so a manual edit can be detected after drawing.
    let before = d.fields.clone();
    let mut preset_clicked = false;

    head(ui, "General");
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Preset:"));
        let cur = d.s("preset");
        craft_ui::ComboBox::from_id_salt("pdf_preset").selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, &cur))).width(200.0).show_ui(
            ui,
            |ui| {
                for p in PDF_EXPORT_PRESETS {
                    if ui.selectable_label(cur == *p, crate::rtl::widget(ui, crate::i18n::tr(lang, p))).clicked() {
                        d.fields.insert("preset".into(), json!(*p));
                        preset_clicked = true;
                    }
                }
            },
        );
    });
    if preset_clicked {
        let p = d.s("preset");
        apply_pdf_preset(d, &p);
    }
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Page Range:"));
        text_field(ui, d, "pages", 160.0);
    });
    check(ui, d, "spreads", crate::i18n::tr(lang, "Spreads"));

    head(ui, "Compression");
    check(ui, d, "compressImages", crate::i18n::tr(lang, "Compress images"));

    head(ui, "Marks and Bleeds");
    check(ui, d, "bleed", crate::i18n::tr(lang, "Include document bleed"));
    check(ui, d, "marksCrop", crate::i18n::tr(lang, "Crop Marks"));
    check(ui, d, "marksBleed", crate::i18n::tr(lang, "Bleed Marks"));
    check(ui, d, "marksPageInfo", crate::i18n::tr(lang, "Page Information"));
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Weight:"));
        text_field(ui, d, "marksWeight", 60.0);
        ui.add_space(8.0);
        crate::rtl::label(ui, crate::i18n::tr(lang, "Offset:"));
        text_field(ui, d, "marksOffset", 60.0);
    });

    head(ui, "Advanced");
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Standard:"));
        translated_combo(lang, ui, d, "standard", &[("none", "None"), ("x1a", "PDF/X-1a:2003"), ("x4", "PDF/X-4"), ("a2b", "PDF/A-2b")]);
    });
    // Spreads with transparency are rasterized at the preset's resolution; in PDF/X-1a it is the
    // resolution craft-prepress rasterizes complex transparent regions at.
    head(ui, "Transparency Flattener");
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Preset:"));
        translated_combo(
            lang,
            ui,
            d,
            "flatten",
            &[("", "None"), ("high", "High Resolution"), ("medium", "Medium Resolution"), ("low", "Low Resolution")],
        );
    });
    check(ui, d, "tagged", crate::i18n::tr(lang, "Tagged PDF"));

    // Any edit other than picking a preset means the settings are no longer that preset.
    if !preset_clicked && d.fields != before {
        d.fields.insert("preset".into(), json!("Custom"));
    }
}

/// The selected text frame's width (its own, before rotation).
fn selected_frame_width(app: &DesignApp) -> Option<f64> {
    let st = app.session.active()?;
    let fid = st.selection.items.first().copied().or_else(|| st.selection.text.and_then(|t| t.frame))?;
    Some(st.doc.item(fid)?.inner_bounds().width())
}

/// The options of the selected text frame (the first selected item, or the frame of the caret).
fn selected_frame_options(app: &DesignApp) -> Option<designcraft_doc::TextFrameOptions> {
    let st = app.session.active()?;
    let fid = st.selection.items.first().copied().or_else(|| st.selection.text.and_then(|t| t.frame))?;
    st.doc.item(fid)?.text_frame().map(|t| t.options.clone())
}

/// Object › Text Frame Options shows the selected frame's options. `_shown` keeps what it showed,
/// so OK sends only the fields the user changed. Fields given when the dialog opened count as
/// changed.
fn seed_text_frame_options(app: &DesignApp, d: &mut Dialog) {
    if d.fields.contains_key("_shown") {
        return;
    }
    let o = selected_frame_options(app).unwrap_or_default();
    let units = app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let fm = |v: f64| json!(format_measure(v, units));
    // Fixed Number: the width each column gets in the frame (shown, as in InDesign).
    let inner = selected_frame_width(app).map(|w| w - o.inset[1] - o.inset[3]).unwrap_or(0.0);
    let n = o.columns.max(1) as f64;
    let width = match o.columns_kind {
        designcraft_doc::ColumnsKind::FixedWidth if o.column_width > 0.0 => o.column_width,
        _ => ((inner - o.gutter * (n - 1.0)) / n).max(0.0),
    };
    let kind = |v: Value| v.as_str().unwrap_or_default().to_string();
    // A frame without a custom baseline grid shows the document grid's start and increment.
    let grid = o.baseline_grid.unwrap_or_else(|| {
        let g = app.session.active().map(|s| s.doc.settings.baseline_grid.clone()).unwrap_or_default();
        let relative_to = match g.relative_to {
            designcraft_doc::GridRelative::TopOfPage => designcraft_doc::FrameGridRelative::TopOfPage,
            designcraft_doc::GridRelative::TopMargin => designcraft_doc::FrameGridRelative::TopMargin,
        };
        designcraft_doc::FrameBaselineGrid { start: g.start, increment: g.increment, relative_to }
    });
    let shown = json!({
        "columns": o.columns, "gutter": fm(o.gutter),
        "columnsKind": kind(serde_json::to_value(o.columns_kind).unwrap_or_default()), "columnWidth": fm(width),
        "balanceColumns": o.balance_columns, "ignoreWrap": o.ignore_wrap,
        "firstBaseline": kind(serde_json::to_value(o.first_baseline).unwrap_or_default()), "firstBaselineMin": fm(o.first_baseline_min),
        "insetTop": fm(o.inset[0]), "insetLeft": fm(o.inset[1]), "insetBottom": fm(o.inset[2]), "insetRight": fm(o.inset[3]),
        "verticalJustification": serde_json::to_value(o.vertical_justification).unwrap_or(Value::Null),
        "paragraphSpacingLimit": fm(o.vj_paragraph_spacing_limit),
        "columnRule": o.column_rule, "columnRuleWeight": format_measure(o.column_rule_weight, Unit::Points), "columnRuleColor": o.column_rule_color,
        "columnRuleTint": percent(o.column_rule_tint), "columnRuleTopInset": fm(o.column_rule_top_inset),
        "columnRuleBottomInset": fm(o.column_rule_bottom_inset), "columnRuleOffset": fm(o.column_rule_offset),
        "autoSize": serde_json::to_value(o.auto_size).unwrap_or(Value::Null),
        "autoSizeReference": designcraft_engine::autosize::REFERENCES.get(usize::from(o.auto_size_ref)).copied().unwrap_or("topCenter"),
        "useMinimumHeight": o.auto_size_use_min_height, "minimumHeight": fm(o.auto_size_min_height),
        "useMinimumWidth": o.auto_size_use_min_width, "minimumWidth": fm(o.auto_size_min_width),
        "noLineBreaks": o.auto_size_no_line_breaks,
        "customGrid": o.baseline_grid.is_some(), "gridStart": fm(grid.start), "gridIncrement": fm(grid.increment),
        "gridRelativeTo": grid.relative_to.name(),
    });
    if let Value::Object(m) = &shown {
        for (k, v) in m {
            d.fields.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    d.fields.entry("insetLinked".to_string()).or_insert_with(|| json!(o.inset.iter().all(|v| (v - o.inset[0]).abs() < 1e-9)));
    // The frame's width: Fixed Number shows each column's width from it as the fields change.
    if let Some(w) = selected_frame_width(app) {
        d.fields.insert("_frameWidth".into(), json!(w));
    }
    d.fields.insert("_shown".into(), shown);
}

/// A 0..1 tint as a percentage ("40", "12.5").
fn percent(t: f32) -> String {
    let s = format!("{:.2}", f64::from(t) * 100.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn text_frame_options_dialog(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    if d.s("section").is_empty() {
        d.fields.insert("section".into(), json!("general"));
    }
    ui.set_min_width(560.0);
    ui.horizontal_top(|ui| {
        // A fixed height: the separator would otherwise stretch the dialog to the window.
        ui.set_min_height(330.0);
        ui.set_max_height(330.0);
        section_list(
            &lang,
            ui,
            d,
            false,
            &[("general", "General"), ("rules", "Column Rules"), ("baseline", "Baseline Options"), ("autosize", "Auto-Size")],
        );
        ui.separator();
        ui.vertical(|ui| match d.s("section").as_str() {
            "baseline" => {
                crate::rtl::label(ui, egui::RichText::new(tr("First Baseline")).font(semibold(12.0)));
                egui::Grid::new("tfo_fb").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, tr("Offset:"));
                    translated_combo(
                        &lang,
                        ui,
                        d,
                        "firstBaseline",
                        &[("ascent", "Ascent"), ("capHeight", "Cap Height"), ("leading", "Leading"), ("xHeight", "x Height"), ("fixed", "Fixed")],
                    );
                    crate::rtl::label(ui, tr("Min:"));
                    text_field(ui, d, "firstBaselineMin", 70.0);
                    ui.end_row();
                });
                ui.add_space(8.0);
                crate::rtl::label(ui, egui::RichText::new(tr("Baseline Grid")).font(semibold(12.0)));
                check(ui, d, "customGrid", tr("Use Custom Baseline Grid"));
                ui.add_enabled_ui(d.b("customGrid"), |ui| {
                    egui::Grid::new("tfo_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        crate::rtl::label(ui, tr("Start:"));
                        text_field(ui, d, "gridStart", 70.0);
                        ui.end_row();
                        crate::rtl::label(ui, tr("Relative To:"));
                        translated_combo(
                            &lang,
                            ui,
                            d,
                            "gridRelativeTo",
                            &[("page", "Top of Page"), ("margin", "Top Margin"), ("frame", "Top of Frame"), ("inset", "Top Inset")],
                        );
                        ui.end_row();
                        crate::rtl::label(ui, tr("Increment Every:"));
                        text_field(ui, d, "gridIncrement", 70.0);
                        ui.end_row();
                    });
                });
            }
            "autosize" => auto_size(&lang, ui, d),
            "rules" => column_rules(app, ui, d),
            _ => general(&lang, ui, d),
        });
    });
    ui.add_space(6.0);
    check(ui, d, "preview", tr("Preview"));
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

/// A measure field of the dialog with InDesign's spinner (stored as text in the document's unit).
fn spin_measure(ui: &mut egui::Ui, d: &mut Dialog, key: &str, w: f32, enabled: bool) {
    let id = format!("tfo_{key}");
    if let Some(v) = crate::widgets::NumField::measure(&id, d.m(key), d.unit).width(w).spinner().range(0.0, 15552.0).enabled(enabled).show(ui) {
        d.fields.insert(key.into(), json!(format_measure(v, d.unit)));
    }
}

/// A titled group in a thin frame, as InDesign's Text Frame Options draws them.
fn framed(ui: &mut egui::Ui, title: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.add_space(4.0);
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).inner_margin(egui::Margin::symmetric(10, 8)).corner_radius(2.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(title) = title {
            crate::rtl::label(ui, egui::RichText::new(title).font(semibold(12.0)));
        }
        add(ui);
    });
}

/// A caption right-aligned in `w` (the dialog's fields line up after their captions).
fn caption_at(ui: &mut egui::Ui, text: &str, w: f32) {
    ui.allocate_ui_with_layout(egui::vec2(w, crate::widgets::FIELD_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.set_min_width(w);
        ui.add(egui::Label::new(crate::rtl::widget(ui, text)).wrap_mode(egui::TextWrapMode::Extend));
    });
}

/// Preview on: the options show on the page (redone when a field changes) without an undo step;
/// off, the page goes back.
fn text_frame_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let params = match text_frame_params(app, d) {
        Ok(p) => Value::Object(p),
        Err(e) => {
            // Values OK would refuse don't stay on the page: it goes back to how it was.
            if shown.is_some() {
                app.session.cancel_interaction();
                d.fields.remove(PREVIEWED);
            }
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    // Nothing edited (yet, or any more): the page as it was.
    if params.as_object().is_some_and(|m| m.keys().all(|k| k == "ids")) {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
        }
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction("Text Frame Options")
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview("object.textFrameOptions", params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// Text Frame Options › General, as InDesign's: Columns (kind, number and width, gutter and
/// maximum, Balance Columns), Inset Spacing (top and bottom, the link, left and right), Vertical
/// Justification (align, paragraph spacing limit) and Ignore Text Wrap.
fn general(lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    let (cap, field, cap2) = (64.0, 110.0, 70.0);
    framed(ui, None, |ui| {
        ui.horizontal(|ui| {
            caption_at(ui, tr("Columns:"), cap);
            translated_combo(lang, ui, d, "columnsKind", &[("fixedNumber", "Fixed Number"), ("fixedWidth", "Fixed Width")]);
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            caption_at(ui, crate::i18n::tr_context(lang, "Number:", "count"), cap);
            let n = d.n("columns");
            if let Some(v) = crate::widgets::NumField::number("tfo_columns", n, "", 0).width(field).spinner().range(1.0, 40.0).show(ui) {
                d.fields.insert("columns".into(), json!(v.round().to_string()));
            }
            // Fixed Number: the width follows from the frame; Fixed Width: it's set here.
            caption_at(ui, tr("Width:"), cap2);
            if d.s("columnsKind") == "fixedWidth" {
                spin_measure(ui, d, "columnWidth", field, true);
            } else {
                // Fixed Number: each column's width, from the frame, the insets and the gutter.
                let frame = d.fields.get("_frameWidth").and_then(Value::as_f64);
                let n = d.n("columns").unwrap_or(1.0).max(1.0);
                let width = frame.map(|w| {
                    let inner = w - d.m("insetLeft").unwrap_or(0.0) - d.m("insetRight").unwrap_or(0.0);
                    ((inner - d.m("gutter").unwrap_or(0.0) * (n - 1.0)) / n).max(0.0)
                });
                let _ = crate::widgets::NumField::measure("tfo_auto_width", width, d.unit).width(field).spinner().enabled(false).show(ui);
            }
        });
        ui.horizontal(|ui| {
            caption_at(ui, tr("Gutter:"), cap);
            spin_measure(ui, d, "gutter", field, true);
            // Maximum is for Flexible Width columns, which aren't in the document model yet.
            caption_at(ui, tr("Maximum:"), cap2);
            ui.add_enabled_ui(false, |ui| {
                let _ = crate::widgets::NumField::number("tfo_max", None, "", 0).width(field).spinner().enabled(false).show(ui);
            });
        });
        check(ui, d, "balanceColumns", tr("Balance Columns"));
    });
    framed(ui, Some(tr("Inset Spacing")), |ui| {
        // While linked, a side typed in sets all four ("Make all settings the same").
        let sides = ["insetTop", "insetLeft", "insetBottom", "insetRight"];
        let before: Vec<String> = sides.iter().map(|k| d.s(k)).collect();
        for (r, [(l1, k1), (l2, k2)]) in
            [[("Top:", "insetTop"), ("Left:", "insetLeft")], [("Bottom:", "insetBottom"), ("Right:", "insetRight")]].into_iter().enumerate()
        {
            ui.horizontal(|ui| {
                caption_at(ui, tr(l1), cap);
                spin_measure(ui, d, k1, field, true);
                ui.add_space(8.0);
                if r == 0 {
                    let linked = d.b("insetLinked");
                    let tip = tr("Make all settings the same");
                    if crate::widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, tip, egui::vec2(22.0, 21.0))
                        .clicked()
                    {
                        d.fields.insert("insetLinked".into(), json!(!linked));
                    }
                } else {
                    ui.add_space(22.0 + ui.spacing().item_spacing.x);
                }
                caption_at(ui, tr(l2), cap2 - 30.0);
                spin_measure(ui, d, k2, field, true);
            });
        }
        if d.b("insetLinked")
            && let Some(k) = sides.iter().zip(&before).find(|(k, b)| d.s(k) != **b).map(|(k, _)| *k)
        {
            let v = d.fields.get(k).cloned().unwrap_or_default();
            for side in sides {
                d.fields.insert(side.into(), v.clone());
            }
        }
    });
    framed(ui, Some(tr("Vertical Justification")), |ui| {
        ui.horizontal(|ui| {
            caption_at(ui, tr("Align:"), 170.0);
            translated_combo(
                lang,
                ui,
                d,
                "verticalJustification",
                &[("top", "Top"), ("center", "Center"), ("bottom", "Bottom"), ("justify", "Justify")],
            );
        });
        ui.horizontal(|ui| {
            caption_at(ui, tr("Paragraph Spacing Limit:"), 170.0);
            let justify = d.s("verticalJustification") == "justify";
            spin_measure(ui, d, "paragraphSpacingLimit", field, justify);
        });
    });
    ui.add_space(4.0);
    check(ui, d, "ignoreWrap", tr("Ignore Text Wrap"));
}

/// Text Frame Options › Column Rules.
fn column_rules(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &'static str| crate::i18n::tr(&app.ui.language, s);
    check(ui, d, "columnRule", tr("Insert Column Rule"));
    let swatches: Vec<String> = app
        .session
        .active()
        .map(|s| {
            s.doc
                .swatches
                .iter()
                .filter(|w| !matches!(w.value, designcraft_color::swatch::SwatchValue::Gradient { .. }))
                .map(|w| w.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let opts: Vec<(&str, &str)> = swatches.iter().map(|n| (n.as_str(), n.as_str())).collect();
    egui::Grid::new("tfo_rule").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Weight"));
        text_field(ui, d, "columnRuleWeight", 70.0);
        ui.end_row();
        crate::rtl::label(ui, tr("Color"));
        combo(ui, d, "columnRuleColor", &opts);
        ui.end_row();
        for (label, key) in [
            ("Tint %", "columnRuleTint"),
            ("Top Inset", "columnRuleTopInset"),
            ("Bottom Inset", "columnRuleBottomInset"),
            ("Horizontal Offset", "columnRuleOffset"),
        ] {
            crate::rtl::label(ui, tr(label));
            text_field(ui, d, key, 70.0);
            ui.end_row();
        }
    });
}

/// Text Frame Options › Auto-Size.
fn auto_size(lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    crate::rtl::label(ui, egui::RichText::new(tr("Auto-Size")).font(semibold(12.0)));
    egui::Grid::new("tfo_autosize").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Auto-Sizing"));
        translated_combo(
            lang,
            ui,
            d,
            "autoSize",
            &[
                ("off", "Off"),
                ("heightOnly", "Height Only"),
                ("widthOnly", "Width Only"),
                ("heightAndWidth", "Height and Width"),
                ("heightAndWidthProportionally", "Height and Width (Keep Proportions)"),
            ],
        );
        ui.end_row();
        crate::rtl::label(ui, tr("Reference Point"));
        reference_point(ui, d, "autoSizeReference");
        ui.end_row();
        check(ui, d, "useMinimumHeight", tr("Minimum Height"));
        text_field(ui, d, "minimumHeight", 70.0);
        ui.end_row();
        check(ui, d, "useMinimumWidth", tr("Minimum Width"));
        text_field(ui, d, "minimumWidth", 70.0);
        ui.end_row();
    });
    check(ui, d, "noLineBreaks", tr("No Line Breaks"));
}

/// A 3×3 reference point picker; `d[key]` holds one of `AUTO_SIZE_REFERENCES`.
fn reference_point(ui: &mut egui::Ui, d: &mut Dialog, key: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let cur = d.s(key);
    let (r, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
    ui.painter().rect_stroke(r.shrink(5.0), 0.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    for (i, name) in designcraft_engine::autosize::REFERENCES.iter().enumerate() {
        let c = r.min + egui::vec2(5.0 + 18.0 * (i % 3) as f32, 5.0 + 18.0 * (i / 3) as f32);
        let dot = egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0));
        let resp = ui.interact(dot.expand(2.0), ui.scope_id().with((key, i)), egui::Sense::click());
        if resp.clicked() {
            d.fields.insert(key.into(), json!(name));
        }
        if cur == *name {
            ui.painter().rect_filled(dot, 0.0, t.accent);
        } else {
            ui.painter().rect_filled(dot, 0.0, t.panel);
            ui.painter().rect_stroke(dot, 0.0, egui::Stroke::new(1.0, if resp.hovered() { t.accent } else { t.icon }), egui::StrokeKind::Inside);
        }
    }
}

/// Whether the user changed `key` from what the dialog showed.
fn edited(d: &Dialog, key: &str) -> bool {
    let shown = match d.fields.get("_shown").and_then(|s| s.get(key)) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => return d.fields.contains_key(key),
    };
    d.fields.contains_key(key) && d.s(key).trim() != shown.trim()
}

/// The custom baseline grid of Text Frame Options: `None` if nothing about it was edited, else off
/// (null) or the fields the user changed.
fn text_frame_baseline_grid(d: &Dialog, bad: &dyn Fn(&str) -> String) -> Result<Option<Value>, String> {
    let grid_keys = ["customGrid", "gridStart", "gridIncrement", "gridRelativeTo"];
    if !grid_keys.iter().any(|k| edited(d, k)) {
        return Ok(None);
    }
    if !d.b("customGrid") {
        return Ok(Some(Value::Null));
    }
    let all = edited(d, "customGrid");
    let mut g = Map::new();
    if all || edited(d, "gridStart") {
        g.insert("start".into(), json!(d.m("gridStart").ok_or_else(|| bad("the baseline grid start is not a measure"))?));
    }
    if all || edited(d, "gridIncrement") {
        g.insert(
            "increment".into(),
            json!(d.m("gridIncrement").filter(|v| *v > 0.0).ok_or_else(|| bad("the baseline grid increment must be a measure greater than zero"))?),
        );
    }
    if all || edited(d, "gridRelativeTo") {
        g.insert("relativeTo".into(), json!(d.s("gridRelativeTo")));
    }
    Ok(Some(Value::Object(g)))
}

fn confirm_text_frame_options(app: &mut DesignApp, d: &mut Dialog) -> Result<Value, String> {
    seed_text_frame_options(app, d);
    // The preview goes; the options are set once, as one undo step.
    if d.fields.remove(PREVIEWED).is_some() {
        app.session.cancel_interaction();
    }
    let p = text_frame_params(app, d)?;
    if p.keys().all(|k| k == "ids") {
        return Ok(Value::Null);
    }
    app.run("object.textFrameOptions", Value::Object(p))
}

/// The `object.textFrameOptions` params of what the dialog changed.
fn text_frame_params(app: &DesignApp, d: &Dialog) -> Result<Map<String, Value>, String> {
    let mut p = Map::new();
    let bad = |what: &str| format!("Text Frame Options: {what}");
    if edited(d, "columns") {
        p.insert("columns".into(), json!(d.n("columns").ok_or_else(|| bad("the number of columns is not a number"))?.max(1.0) as u64));
    }
    if edited(d, "gutter") {
        p.insert("gutter".into(), json!(d.m("gutter").ok_or_else(|| bad("the gutter is not a measure"))?));
    }
    let sides = ["insetTop", "insetLeft", "insetBottom", "insetRight"];
    if edited(d, "inset") {
        p.insert("inset".into(), json!(d.m("inset").ok_or_else(|| bad("the inset is not a measure"))?));
    } else if sides.iter().any(|k| edited(d, k)) {
        // Sides left as shown are null: each selected frame keeps its own.
        let mut inset = [None; 4];
        for (v, k) in inset.iter_mut().zip(sides) {
            if edited(d, k) {
                *v = Some(d.m(k).ok_or_else(|| bad("an inset is not a measure"))?);
            }
        }
        p.insert("inset".into(), json!(inset));
    }
    if edited(d, "balanceColumns") {
        p.insert("balanceColumns".into(), json!(d.b("balanceColumns")));
    }
    if edited(d, "verticalJustification") {
        p.insert("verticalJustification".into(), json!(d.s("verticalJustification")));
    }
    for key in ["columnsKind", "firstBaseline"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.s(key)));
        }
    }
    // The width counts only for Fixed Width columns (Fixed Number shows it).
    if d.s("columnsKind") == "fixedWidth" && (edited(d, "columnWidth") || edited(d, "columnsKind")) {
        p.insert("columnWidth".into(), json!(d.m("columnWidth").ok_or_else(|| bad("the column width is not a measure"))?));
    }
    if edited(d, "firstBaselineMin") {
        p.insert("firstBaselineMin".into(), json!(d.m("firstBaselineMin").ok_or_else(|| bad("the first baseline minimum is not a measure"))?));
    }
    for key in ["balanceColumns", "ignoreWrap"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.b(key)));
        }
    }
    if edited(d, "columnRule") {
        p.insert("columnRule".into(), json!(d.b("columnRule")));
    }
    if edited(d, "columnRuleWeight") {
        p.insert("columnRuleWeight".into(), json!(d.pt("columnRuleWeight").ok_or_else(|| bad("the column rule weight is not a measure"))?));
    }
    if edited(d, "columnRuleColor") {
        p.insert("columnRuleColor".into(), json!(d.s("columnRuleColor")));
    }
    if edited(d, "columnRuleTint") {
        p.insert("columnRuleTint".into(), json!(d.n("columnRuleTint").ok_or_else(|| bad("the column rule tint is not a number"))? / 100.0));
    }
    for (key, what) in [
        ("columnRuleTopInset", "the column rule top inset is not a measure"),
        ("columnRuleBottomInset", "the column rule bottom inset is not a measure"),
        ("columnRuleOffset", "the column rule offset is not a measure"),
    ] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.m(key).ok_or_else(|| bad(what))?));
        }
    }
    for key in ["autoSize", "autoSizeReference"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.s(key)));
        }
    }
    for key in ["useMinimumHeight", "useMinimumWidth", "noLineBreaks"] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.b(key)));
        }
    }
    for (key, what) in [("minimumHeight", "the minimum height is not a measure"), ("minimumWidth", "the minimum width is not a measure")] {
        if edited(d, key) {
            p.insert(key.into(), json!(d.m(key).filter(|v| *v >= 0.0).ok_or_else(|| bad(what))?));
        }
    }
    // The custom baseline grid: off, or the fields the user changed (all of them when it is
    // turned on, since it shows the document grid's).
    if let Some(grid) = text_frame_baseline_grid(d, &bad)? {
        p.insert("baselineGrid".into(), grid);
    }
    if edited(d, "paragraphSpacingLimit") {
        p.insert(
            "paragraphSpacingLimit".into(),
            json!(d.m("paragraphSpacingLimit").filter(|v| *v >= 0.0).ok_or_else(|| bad("the paragraph spacing limit is not a measure"))?),
        );
    }
    // Typing in a frame: the options apply to that frame.
    if let Some(st) = app.session.active()
        && st.selection.items.is_empty()
        && let Some(f) = st.selection.text.and_then(|t| t.frame)
    {
        p.insert("ids".into(), json!([f.0]));
    }
    Ok(p)
}

/// File › Document Setup, filled from the document.
/// The active document's ruler unit (picas without a document).
fn doc_unit(app: &DesignApp) -> Unit {
    app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas)
}

/// The document's vertical ruler unit: what a bare number means in a height field.
fn doc_vertical_unit(app: &DesignApp) -> Unit {
    app.session.active().map(|s| s.doc.settings.vertical_units).unwrap_or(Unit::Picas)
}

/// The Rectangle dialog a click with a frame tool opens: the width (points in `params`) shown in
/// the horizontal ruler unit, the height in the vertical one, as Preferences › Units &
/// Increments and the rulers' menus set them (#1964).
pub fn frame_size_dialog(app: &DesignApp, params: Value) -> Dialog {
    let mut d = Dialog::new("frameSize", params);
    for (k, unit) in [("width", doc_unit(app)), ("height", doc_vertical_unit(app))] {
        if let Some(v) = d.fields.get(k).and_then(Value::as_f64) {
            d.fields.insert(k.into(), json!(format_measure(v, unit)));
        }
    }
    d.unit = doc_unit(app);
    d.vertical_unit = Some(doc_vertical_unit(app));
    d
}

/// Layout › Create Guides…: guides go on every page of the spread in view (or of the parent
/// spread being edited), as InDesign's do; "This page" narrows them to
/// the page in view. The gutters are typed in the document's ruler unit.
pub fn open_create_guides(app: &mut DesignApp) {
    let unit = doc_unit(app);
    let Some((spread, page)) = crate::canvas::current_spread_page(app) else { return };
    let gutter = json!(format_measure(12.0, unit));
    let mut d = Dialog::new(
        "createGuides",
        json!({"rows": 0, "columns": 0, "rowGutter": gutter, "columnGutter": gutter, "fitTo": "margins", "removeExisting": false,
            "spread": serde_json::to_value(spread).unwrap_or(json!(0)), "page": page,
            // InDesign puts the guides on both pages of a facing spread (and a parent spread is
            // edited as a whole, so they show on every page that uses it).
            "target": "spread",
            "where": target_description(app, spread, page)}),
    );
    d.unit = unit;
    app.ui.dialog = Some(d);
}

/// "Page 4" / "Pages 4–5" / "Parent A-Parent": what a dialog's target choice refers to.
fn target_description(app: &DesignApp, spread: SpreadRef, page: usize) -> String {
    let Some(st) = app.session.active() else { return String::new() };
    match spread {
        SpreadRef::Doc(si) => {
            let first = st.doc.first_page_of_spread(si);
            format!("page {}", app.session.page_label(first + page))
        }
        SpreadRef::Parent(pi) => {
            let name = st.doc.parents.get(pi).and_then(|p| p.parent.as_ref()).map(|i| i.label()).unwrap_or_default();
            format!("parent {name}")
        }
    }
}

/// Layout › Margins and Columns…: the margins and columns of the page in view (or of the parent
/// spread while parents are edited), shown and typed in the document's ruler unit.
pub fn open_margins_and_columns(app: &mut DesignApp) {
    let unit = doc_unit(app);
    let parent = crate::canvas::current_parent_page(app);
    let cur = crate::canvas::current_page(app).unwrap_or(0);
    let Some(st) = app.session.active() else { return };
    let page = match parent {
        Some((pi, qi)) => st.doc.parents.get(pi).and_then(|s| s.pages.get(qi)),
        None => st.doc.page(cur),
    };
    let Some(pg) = page else { return };
    let fm = |v: f64| json!(format_measure(v, unit));
    // The pages of the spread in view: both pages of a facing spread.
    let spread_pages: Vec<usize> = match parent {
        Some((pi, _)) => (0..st.doc.parents.get(pi).map_or(0, |s| s.pages.len())).collect(),
        None => st.doc.page_loc(cur).map_or_else(Vec::new, |(si, _)| {
            let first = st.doc.first_page_of_spread(si);
            (first..first + st.doc.spreads.get(si).map_or(0, |s| s.pages.len())).collect()
        }),
    };
    let where_ = match parent {
        Some((pi, qi)) => target_description(app, SpreadRef::Parent(pi), qi),
        None => format!("page {}", app.session.page_label(cur)),
    };
    let mut f = json!({
        "columns": pg.columns.count, "gutter": fm(pg.columns.gutter),
        "marginTop": fm(pg.margins.top), "marginBottom": fm(pg.margins.bottom),
        "marginInside": fm(pg.margins.inside), "marginOutside": fm(pg.margins.outside),
        "adjustLayout": false, "page": cur, "spreadPages": spread_pages,
        "target": if parent.is_some() { "spread" } else { "page" }, "where": where_,
    });
    if let Some((pi, qi)) = parent {
        f["parent"] = json!(pi);
        f["parentPage"] = json!(qi);
    }
    let mut d = Dialog::new("marginsAndColumns", f);
    d.unit = unit;
    app.ui.dialog = Some(d);
}

/// The margin fields of Margins and Columns: (key, label on a facing page, label on a single one).
const MARGIN_FIELDS: [(&str, &str, &str); 4] = [
    ("marginTop", "Top:", "Top:"),
    ("marginBottom", "Bottom:", "Bottom:"),
    ("marginInside", "Inside:", "Left:"),
    ("marginOutside", "Outside:", "Right:"),
];

/// Layout › Margins and Columns, laid out as InDesign's: Margins (Top and Bottom, Inside and
/// Outside, with the link button between them: Make all settings the same), Columns (Number,
/// Gutter), Adjust Layout, and Preview beside the options. Measures are in the ruler unit and
/// take arithmetic ("0.4375 in*2").
fn margins_and_columns_dialog(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let facing = app.session.active().is_none_or(|s| s.doc.settings.facing_pages);
    // Linked when the dialog opens with four equal margins, as InDesign's.
    if !d.fields.contains_key("link") {
        let first = d.s("marginTop");
        let same = MARGIN_FIELDS.iter().all(|(k, _, _)| d.s(k) == first);
        d.fields.insert("link".into(), json!(same));
    }
    link_margins(d);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            group_box(ui, &tr("Margins"), |ui| {
                ui.horizontal_top(|ui| {
                    egui::Grid::new("mac_tb").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
                        for (k, l, _) in &MARGIN_FIELDS[..2] {
                            caption_right(ui, &tr(l), 72.0);
                            measure_field(ui, d, k, 76.0);
                            ui.end_row();
                        }
                    });
                    // The link button, between the two rows.
                    let linked = d.b("link");
                    let tip = tr("Make all settings the same");
                    let resp = ui
                        .vertical(|ui| {
                            ui.add_space(14.0);
                            crate::icons::button(ui, if linked { "link" } else { "link-broken" }, 20.0, linked, &tip)
                        })
                        .inner;
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &tip));
                    if resp.clicked() {
                        d.fields.insert("link".into(), json!(!linked));
                    }
                    egui::Grid::new("mac_io").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
                        for (k, l, single) in &MARGIN_FIELDS[2..] {
                            caption_right(ui, &tr(if facing { l } else { single }), 72.0);
                            measure_field(ui, d, k, 76.0);
                            ui.end_row();
                        }
                    });
                });
            });
            ui.add_space(8.0);
            group_box(ui, &tr("Columns"), |ui| {
                ui.horizontal(|ui| {
                    caption_right(ui, crate::i18n::tr_context(&lang, "Number:", "count"), 72.0);
                    text_field(ui, d, "columns", 76.0);
                    ui.add_space(28.0);
                    caption_right(ui, &tr("Gutter:"), 72.0);
                    measure_field(ui, d, "gutter", 76.0);
                });
            });
            ui.add_space(6.0);
            check(ui, d, "adjustLayout", &tr("Adjust Layout"));
            ui.add_space(4.0);
            let all_pages = !d.fields.contains_key("parent");
            target_row(app, ui, d, all_pages);
        });
        ui.add_space(24.0);
        check(ui, d, "preview", &tr("Preview"));
    });
    link_margins(d);
    layout_dialog_preview(app, d, "layout.marginsAndColumns", "Margins and Columns");
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.label(egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

/// Linked margins: a margin changed since the dialog last looked (typed, or set over the control
/// channel) is all four.
fn link_margins(d: &mut Dialog) {
    let now: Vec<String> = MARGIN_FIELDS.iter().map(|(k, _, _)| d.s(k)).collect();
    let seen: Vec<String> = d.fields.get("_margins").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_else(|| now.clone());
    let mut now = now;
    if d.b("link")
        && let Some(i) = now.iter().zip(&seen).position(|(a, b)| a != b)
    {
        let v = now[i].clone();
        for (k, _, _) in &MARGIN_FIELDS {
            d.fields.insert((*k).into(), json!(v));
        }
        now = vec![v; MARGIN_FIELDS.len()];
    }
    d.fields.insert("_margins".into(), json!(now));
}

/// Layout › Create Guides, laid out as InDesign's: Rows and Columns side by side (Number,
/// Gutter), Options (Fit Guides to: Margins or Page, Remove Existing Ruler Guides), and Preview.
fn create_guides_dialog(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.horizontal_top(|ui| {
                for (title, n, g, id) in [("Rows", "rows", "rowGutter", "cg_rows"), ("Columns", "columns", "columnGutter", "cg_cols")] {
                    group_box(ui, &tr(title), |ui| {
                        egui::Grid::new(id).num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
                            caption_right(ui, crate::i18n::tr_context(&lang, "Number:", "count"), 72.0);
                            text_field(ui, d, n, 70.0);
                            ui.end_row();
                            caption_right(ui, &tr("Gutter:"), 72.0);
                            measure_field(ui, d, g, 70.0);
                            ui.end_row();
                        });
                    });
                    ui.add_space(8.0);
                }
            });
            ui.add_space(8.0);
            group_box(ui, &tr("Options"), |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::rtl::widget(ui, tr("Fit Guides to:")));
                    let page = d.s("fitTo") == "page";
                    if ui.radio(!page, crate::rtl::widget(ui, tr("Margins"))).clicked() {
                        d.fields.insert("fitTo".into(), json!("margins"));
                    }
                    if ui.radio(page, crate::rtl::widget(ui, tr("Page"))).clicked() {
                        d.fields.insert("fitTo".into(), json!("page"));
                    }
                });
                check(ui, d, "removeExisting", &tr("Remove Existing Ruler Guides"));
            });
            ui.add_space(4.0);
            target_row(app, ui, d, false);
        });
        ui.add_space(24.0);
        check(ui, d, "preview", &tr("Preview"));
    });
    layout_dialog_preview(app, d, "layout.createGuides", "Create Guides");
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.label(egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

/// A measure field: typed arithmetic ("0.4375 in*2") is worked out when the field is left, as
/// InDesign shows it ("click in another field to see the value multiplied by two").
fn measure_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str, w: f32) {
    let mut s = d.s(key);
    let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(w).align(egui::Align2::LEFT_TOP));
    if r.changed() {
        d.fields.insert(key.into(), Value::String(s));
    }
    if r.lost_focus()
        && let Some(v) = d.m(key).filter(|v| v.is_finite())
    {
        d.fields.insert(key.into(), json!(format_measure(v, d.unit)));
    }
}

/// A titled group, as InDesign's dialogs draw them: the title, then the contents in a thin frame.
fn group_box(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.vertical(|ui| {
        crate::rtl::label(ui, egui::RichText::new(title).font(semibold(12.0)));
        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).inner_margin(egui::Margin::same(8)).corner_radius(2.0).show(ui, add);
    });
}

/// A field caption (the grid lines the fields up).
fn caption_right(ui: &mut egui::Ui, text: &str, _w: f32) {
    ui.add(egui::Label::new(crate::rtl::widget(ui, text)).wrap_mode(egui::TextWrapMode::Extend));
}

/// The command params of the Margins and Columns or Create Guides dialog. A measure that does
/// not parse is an error to show, never a silent 0.
fn layout_dialog_params(d: &Dialog) -> Result<Value, String> {
    let measure =
        |key: &str, what: &str| d.m(key).filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("Enter a valid {what}, e.g. 12 or 12 mm."));
    if d.id == "createGuides" {
        let (row_gutter, column_gutter) = (measure("rowGutter", "gutter")?, measure("columnGutter", "gutter")?);
        let mut p = json!({"rows": d.n("rows").unwrap_or(0.0).max(0.0) as u64, "columns": d.n("columns").unwrap_or(0.0).max(0.0) as u64,
            "rowGutter": row_gutter, "columnGutter": column_gutter, "fitTo": d.s("fitTo"),
            "removeExisting": d.b("removeExisting"), "spread": d.fields.get("spread").cloned().unwrap_or(json!(0))});
        // Without `page` the command covers every page of the spread.
        if d.s("target") != "spread" {
            p["page"] = d.fields.get("page").cloned().unwrap_or(json!(0));
        }
        return Ok(p);
    }
    let mut v = Map::new();
    for (k, key) in [("top", "marginTop"), ("bottom", "marginBottom"), ("inside", "marginInside"), ("outside", "marginOutside")] {
        v.insert(k.into(), json!(measure(key, &format!("{k} value"))?));
    }
    let gutter = measure("gutter", "gutter value")?;
    let mut p =
        json!({"margins": v, "columns": d.n("columns").unwrap_or(1.0).max(1.0) as u64, "gutter": gutter, "adjustLayout": d.b("adjustLayout")});
    let spread_pages: Vec<u64> =
        d.fields.get("spreadPages").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    if let Some(pi) = d.fields.get("parent").and_then(Value::as_u64) {
        p["parent"] = json!(pi);
        p["pages"] =
            if d.s("target") == "page" { json!([d.fields.get("parentPage").and_then(Value::as_u64).unwrap_or(0)]) } else { json!(spread_pages) };
    } else {
        match d.s("target").as_str() {
            "all" => {}
            "spread" if !spread_pages.is_empty() => p["pages"] = json!(spread_pages),
            _ => p["pages"] = json!([d.fields.get("page").and_then(Value::as_u64).unwrap_or(0)]),
        }
    }
    Ok(p)
}

/// Preview on: the change shows on the page (redone when a field changes) without an undo step;
/// off, the page goes back.
fn layout_dialog_preview(app: &mut DesignApp, d: &mut Dialog, cmd: &str, label: &str) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let params = match layout_dialog_params(d) {
        Ok(p) => p,
        Err(e) => {
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction(label)
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview(cmd, params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// "Apply to:" choice of the Create Guides and Margins and Columns dialogs, with the page it
/// refers to spelled out so a wrong target is visible before the dialog is confirmed.
fn target_row(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, all_pages: bool) {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    let parent = d.fields.contains_key("parent") || matches!(d.fields.get("spread"), Some(v) if v["kind"] == "parent");
    let spread_label = if parent { tr("Both pages of the parent spread") } else { tr("Both pages of the spread") };
    let mut opts = vec![("page", tr("This page")), ("spread", spread_label)];
    if all_pages {
        opts.push(("all", tr("All pages")));
    }
    ui.horizontal(|ui| {
        ui.label(crate::rtl::widget(ui, tr("Apply to:")));
        combo(ui, d, "target", &opts);
    });
    let w = d.s("where");
    if !w.is_empty() {
        ui.label(egui::RichText::new(format!("{} {w}", tr("This page is"))).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

pub fn open_document_setup(app: &mut DesignApp) {
    let Ok(cur) = app.session.execute("layout.documentSetup", &json!({})) else { return };
    let units = app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let fm = |v: &Value| json!(format_measure(v.as_f64().unwrap_or(0.0), units));
    let mut f = json!({"intent": cur["intent"], "pages": cur["pages"], "startPage": cur["startPage"], "facingPages": cur["facingPages"],
        "binding": cur["binding"], "width": fm(&cur["width"]), "height": fm(&cur["height"]), "units": units,
        "primaryTextFrame": cur["primaryTextFrame"]});
    for (k, values) in [
        ("margin", ["top", "bottom", "inside", "outside"].map(|e| cur["margins"][e].clone())),
        ("bleed", [0, 1, 2, 3].map(|i| cur["bleed"][i].clone())),
        ("slug", [0, 1, 2, 3].map(|i| cur["slug"][i].clone())),
    ] {
        for (e, v) in SETUP_EDGES.iter().zip(&values) {
            f[format!("{k}{e}")] = fm(v);
        }
        let first = values[0].as_f64();
        f[format!("{k}Linked")] = json!(values.iter().all(|v| v.as_f64() == first));
    }
    // The margins as shown: OK applies them to every page only when they were edited.
    f["marginsShown"] = json!(SETUP_EDGES.map(|e| f[format!("margin{e}")].clone()));
    let mut d = Dialog::new("documentSetup", f);
    d.unit = units;
    app.ui.dialog = Some(d);
}

/// The fields of File › File Info, in the order the dialog shows them, with their labels.
const FILE_INFO_FIELDS: [(&str, &str); 6] = [
    ("documentTitle", "Document Title:"),
    ("author", "Author:"),
    ("description", "Description:"),
    ("keywords", "Keywords:"),
    ("copyrightNotice", "Copyright Notice:"),
    ("copyrightInfoUrl", "Copyright Info URL:"),
];

/// File › File Info, filled from the document. Keywords show as one line, separated by semicolons.
pub fn open_file_info(app: &mut DesignApp) {
    let Ok(cur) = app.session.execute("file.info.get", &json!({})) else { return };
    let mut f = Map::new();
    for (key, _) in FILE_INFO_FIELDS {
        let v = match &cur[key] {
            Value::Array(words) => words.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; "),
            v => v.as_str().unwrap_or_default().to_string(),
        };
        f.insert(key.into(), Value::String(v));
    }
    // The keywords as shown and as stored: OK sends them only when they were edited, and then a
    // stored keyword holding a semicolon or comma that is still there whole stays one keyword.
    let shown = f.get("keywords").cloned().unwrap_or_default();
    f.insert("keywordsShown".into(), shown);
    f.insert("keywordsStored".into(), cur["keywords"].clone());
    // The document the fields belong to, should another one become active before OK.
    f.insert("uid".into(), json!(app.session.active().map(|doc| doc.uid)));
    app.ui.dialog = Some(Dialog::new("fileInfo", Value::Object(f)));
}

fn file_info_dialog(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    egui::Grid::new("file_info").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (key, label) in FILE_INFO_FIELDS {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
            if key == "description" {
                let mut s = d.s(key);
                if ui.add(egui::TextEdit::multiline(&mut s).desired_rows(3).desired_width(320.0)).changed() {
                    d.fields.insert(key.into(), Value::String(s));
                }
            } else {
                text_field(ui, d, key, 320.0);
            }
            ui.end_row();
        }
    });
}

/// File Info's OK: every field the dialog holds, as typed (an empty one clears it); the keywords
/// only when they were edited. A field that is not text (a keyword list, or `null` from
/// `ui.dialog.open`) goes to `file.info` as it is. The fields go to the document the dialog was
/// opened for, even when another one became active meanwhile, and the active document stays
/// active; when the dialog's document has closed, nothing is stored.
fn confirm_file_info(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let keywords_edited = d.fields.get("keywordsShown").and_then(Value::as_str) != Some(d.s("keywords").as_str());
    let stored: Vec<String> = match d.fields.get("keywordsStored") {
        Some(Value::Array(words)) => words.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let p: Map<String, Value> = FILE_INFO_FIELDS
        .iter()
        .filter(|(key, _)| d.fields.contains_key(*key))
        .filter(|(key, _)| *key != "keywords" || keywords_edited)
        .map(|(key, _)| {
            let v = match d.fields.get(*key) {
                Some(Value::String(s)) if *key == "keywords" => json!(designcraft_doc::file_info::parse_keywords_keeping(s, &stored)),
                Some(v) => v.clone(),
                None => Value::Null,
            };
            (key.to_string(), v)
        })
        .collect();
    let uid = d.fields.get("uid").and_then(Value::as_u64);
    let active = app.session.active().map(|doc| doc.uid);
    if uid.is_none() || uid == active {
        return app.run("file.info", Value::Object(p));
    }
    let index = |app: &DesignApp, uid: Option<u64>| app.session.documents().iter().position(|doc| Some(doc.uid) == uid);
    let Some(i) = index(app, uid) else { return Ok(Value::Null) };
    // Switching documents ends the active document's Data Merge preview; it is shown again after.
    let preview = app.session.active().and_then(|doc| doc.preview_record);
    app.run("file.activate", json!({"index": i}))?;
    let r = app.run("file.info", Value::Object(p));
    if let Some(back) = index(app, active) {
        app.run("file.activate", json!({"index": back}))?;
        if let Some(record) = preview {
            app.run("data.preview", json!({"record": record}))?;
        }
    }
    r
}

/// The edges of Document Setup's margins, bleed and slug (field suffixes).
const SETUP_EDGES: [&str; 4] = ["Top", "Bottom", "Inside", "Outside"];

/// The units Document Setup shows its measures in (the document's horizontal units).
fn setup_units(d: &Dialog) -> Unit {
    d.fields.get("units").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(Unit::Picas)
}

/// A Document Setup measure in points, read as every dialog reads its measures.
fn setup_measure(d: &Dialog, key: &str) -> Option<f64> {
    d.m(key).filter(|v| v.is_finite())
}

/// A measure field with up/down arrows (a point, or a millimetre in metric units, a step); true
/// when its value changed.
fn setup_spin(ui: &mut egui::Ui, d: &mut Dialog, key: &str, width: f32, min: f64) -> bool {
    let t = crate::theme::Tokens::get(ui.ctx());
    let h = 22.0;
    let (r, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    ui.painter().rect(r, 2.0, t.input, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    let seg = egui::Rect::from_min_size(r.min, egui::vec2(18.0, h));
    ui.painter().line_segment([seg.right_top(), seg.right_bottom()], egui::Stroke::new(1.0, t.field_border));
    let units = setup_units(d);
    let step = if matches!(units, Unit::Millimeters | Unit::Centimeters) { designcraft_geom::units::PT_PER_MM } else { 1.0 };
    let mut changed = false;
    for (dir, half) in [
        (1.0, egui::Rect::from_min_max(seg.min, egui::pos2(seg.max.x, seg.center().y))),
        (-1.0, egui::Rect::from_min_max(egui::pos2(seg.min.x, seg.center().y), seg.max)),
    ] {
        let resp = ui.interact(half, ui.scope_id().with((key, dir < 0.0)), egui::Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(half.shrink(1.0), 1.0, t.hover);
        }
        let c = half.center();
        let y = 2.5 * dir as f32;
        ui.painter().add(egui::Shape::line(
            vec![egui::pos2(c.x - 3.5, c.y + y / 2.0), egui::pos2(c.x, c.y - y / 2.0), egui::pos2(c.x + 3.5, c.y + y / 2.0)],
            egui::Stroke::new(1.2, t.icon),
        ));
        if resp.clicked() {
            let v = (setup_measure(d, key).unwrap_or(min) + dir * step).clamp(min, 15552.0);
            d.fields.insert(key.into(), json!(format_measure(v, units)));
            changed = true;
        }
    }
    let text = egui::Rect::from_min_max(egui::pos2(seg.right() + 5.0, r.top() + 1.0), egui::pos2(r.right() - 3.0, r.bottom() - 1.0));
    let mut s = d.s(key);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let resp =
        child.add(egui::TextEdit::singleline(&mut s).id(ui.scope_id().with((key, "text"))).frame(egui::Frame::NONE).desired_width(text.width()));
    if resp.changed() {
        d.fields.insert(key.into(), Value::String(s));
        changed = true;
    }
    changed
}

/// A caption ending at the field it names (`Width:` [ ]), `width` wide.
fn setup_caption(ui: &mut egui::Ui, text: &str, width: f32) {
    let rtl = ui.layout().prefer_right_to_left();
    let layout = if rtl { egui::Layout::left_to_right(egui::Align::Center) } else { egui::Layout::right_to_left(egui::Align::Center) };
    ui.allocate_ui_with_layout(egui::vec2(width, 22.0), layout, |ui| {
        ui.set_width(width);
        crate::rtl::label(ui, text);
    });
}

/// A row that runs right to left in a right-to-left interface.
fn setup_row<R>(ui: &mut egui::Ui, rtl: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let layout = if rtl { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 24.0), layout, add).inner
}

/// An outlined group of options.
fn setup_group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).corner_radius(3.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        add(ui);
    });
}

/// The link beside a group's four edges: keeps them equal (linking makes them the first's).
fn setup_link(ui: &mut egui::Ui, lang: &str, d: &mut Dialog, group: &str, changed: Option<String>) {
    let key = format!("{group}Linked");
    let linked = d.b(&key);
    let mut source = if linked { changed } else { None };
    let icon = if linked { "link" } else { "link-broken" };
    if crate::widgets::icon_toggle_sized(ui, icon, linked, crate::i18n::tr(lang, "Make all settings the same"), egui::vec2(26.0, 26.0)).clicked() {
        d.fields.insert(key, json!(!linked));
        if !linked {
            source = Some(format!("{group}Top"));
        }
    }
    if let Some(v) = source.and_then(|k| d.fields.get(&k).cloned()) {
        for e in SETUP_EDGES {
            d.fields.insert(format!("{group}{e}"), v.clone());
        }
    }
}

/// File › Document Setup, laid out like InDesign's: intent, pages, start page #, facing pages,
/// then Page Size (preset, width, height, orientation), Margins, Bleed and Slug (each edge, linked or
/// not), and Adjust Layout. Returns true when Adjust Layout was clicked (apply now, objects follow).
fn document_setup(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let lang = app.ui.language.as_str();
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    let rtl = crate::i18n::is_rtl(lang);
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.y = 6.0;
    let label_w = 130.0;
    setup_row(ui, rtl, |ui| {
        setup_caption(ui, tr("Intent:"), label_w);
        let cur = d.s("intent");
        craft_ui::ComboBox::from_id_salt("ds_intent")
            .selected_text(crate::rtl::widget(
                ui,
                tr(match cur.as_str() {
                    "web" => "Web",
                    "mobile" => "Mobile",
                    _ => "Print",
                }),
            ))
            .width(150.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("print", "Print"), ("web", "Web"), ("mobile", "Mobile")] {
                    if ui.selectable_label(cur == v, crate::rtl::widget(ui, tr(l))).clicked() {
                        d.fields.insert("intent".into(), json!(v));
                    }
                }
            });
    });
    setup_row(ui, rtl, |ui| {
        setup_caption(ui, tr("Number of Pages:"), label_w);
        text_field(ui, d, "pages", 90.0);
        ui.add_space(40.0);
        check(ui, d, "facingPages", tr("Facing Pages"));
    });
    setup_row(ui, rtl, |ui| {
        setup_caption(ui, tr("Start Page #:"), label_w);
        text_field(ui, d, "startPage", 90.0);
        ui.add_space(40.0);
        // Set when the document is made (New Document); shown here, as InDesign does.
        ui.add_enabled_ui(false, |ui| check(ui, d, "primaryTextFrame", tr("Primary Text Frame")));
    });
    setup_row(ui, rtl, |ui| {
        setup_caption(ui, tr("Binding:"), label_w);
        let right_to_left = d.s("binding") == "rightToLeft";
        craft_ui::ComboBox::from_id_salt("ds_binding")
            .selected_text(crate::rtl::widget(ui, tr(if right_to_left { "Right to Left" } else { "Left to Right" })))
            .width(150.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("leftToRight", "Left to Right"), ("rightToLeft", "Right to Left")] {
                    if ui.selectable_label((v == "rightToLeft") == right_to_left, crate::rtl::widget(ui, tr(l))).clicked() {
                        d.fields.insert("binding".into(), json!(v));
                    }
                }
            });
    });
    ui.add_space(4.0);
    // Page Size.
    let (w, h) = (setup_measure(d, "width").unwrap_or(612.0), setup_measure(d, "height").unwrap_or(792.0));
    let preset = designcraft_doc::build::PRESETS
        .iter()
        .find(|p| ((p.width - w).abs() < 0.5 && (p.height - h).abs() < 0.5) || ((p.width - h).abs() < 0.5 && (p.height - w).abs() < 0.5))
        .map_or_else(|| tr("Custom").to_string(), |p| tr(p.name).to_string());
    setup_group(ui, |ui| {
        setup_row(ui, rtl, |ui| {
            crate::rtl::label(ui, egui::RichText::new(tr("Page Size:")).color(t.text_strong));
            craft_ui::ComboBox::from_id_salt("ds_preset").selected_text(crate::rtl::widget(ui, &preset)).width(260.0).show_ui(ui, |ui| {
                for p in designcraft_doc::build::PRESETS {
                    if ui.selectable_label(tr(p.name) == preset, crate::rtl::widget(ui, tr(p.name))).clicked() {
                        // Keep the orientation; the preset's size in the document's units.
                        let units = setup_units(d);
                        let (pw, ph) = if w > h { (p.width.max(p.height), p.width.min(p.height)) } else { (p.width, p.height) };
                        d.fields.insert("width".into(), json!(format_measure(pw, units)));
                        d.fields.insert("height".into(), json!(format_measure(ph, units)));
                    }
                }
            });
        });
        setup_row(ui, rtl, |ui| {
            setup_caption(ui, tr("Width:"), 70.0);
            setup_spin(ui, d, "width", 130.0, 1.0);
            ui.add_space(30.0);
            crate::rtl::label(ui, tr("Orientation:"));
            for (icon, tip, portrait) in [("orient-portrait", "Portrait", true), ("orient-landscape", "Landscape", false)] {
                let on = (h >= w) == portrait;
                if crate::widgets::icon_toggle_sized(ui, icon, on, tr(tip), egui::vec2(26.0, 26.0)).clicked() && !on {
                    let (fw, fh) = (d.s("width"), d.s("height"));
                    d.fields.insert("width".into(), json!(fh));
                    d.fields.insert("height".into(), json!(fw));
                }
            }
        });
        setup_row(ui, rtl, |ui| {
            setup_caption(ui, tr("Height:"), 70.0);
            setup_spin(ui, d, "height", 130.0, 1.0);
        });
    });
    ui.add_space(4.0);
    // Margins: two columns beside a link.
    setup_group(ui, |ui| {
        setup_row(ui, rtl, |ui| {
            crate::rtl::label(ui, egui::RichText::new(tr("Margins")).color(t.text_strong));
        });
        let facing = d.b("facingPages");
        let (inside, outside) = if facing { ("Inside:", "Outside:") } else { ("Left:", "Right:") };
        let mut changed = None;
        setup_row(ui, rtl, |ui| {
            ui.vertical(|ui| {
                for (a, b) in [("Top:", "Top"), ("Bottom:", "Bottom")] {
                    setup_row(ui, rtl, |ui| {
                        setup_caption(ui, tr(a), 70.0);
                        if setup_spin(ui, d, &format!("margin{b}"), 120.0, 0.0) {
                            changed = Some(format!("margin{b}"));
                        }
                    });
                }
            });
            ui.add_space(4.0);
            setup_link(ui, lang, d, "margin", changed.take());
            ui.add_space(4.0);
            ui.vertical(|ui| {
                for (a, b) in [(inside, "Inside"), (outside, "Outside")] {
                    setup_row(ui, rtl, |ui| {
                        setup_caption(ui, tr(a), 70.0);
                        if setup_spin(ui, d, &format!("margin{b}"), 120.0, 0.0) {
                            changed = Some(format!("margin{b}"));
                        }
                    });
                }
            });
        });
        if changed.is_some() {
            setup_link_follow(d, "margin", changed);
        }
    });
    ui.add_space(4.0);
    // Bleed and Slug: a column per edge, a row (and a link) each.
    egui::CollapsingHeader::new(crate::rtl::widget(ui, egui::RichText::new(tr("Bleed and Slug")).color(t.text_strong)))
        .id_salt("ds_bleed_slug")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("ds_bleed").num_columns(6).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("");
                for e in SETUP_EDGES {
                    ui.add_sized([96.0, 18.0], egui::Label::new(crate::rtl::widget(ui, egui::RichText::new(tr(e)).color(t.text_strong))));
                }
                ui.end_row();
                for (k, l) in [("bleed", "Bleed:"), ("slug", "Slug:")] {
                    crate::rtl::label(ui, tr(l));
                    let mut changed = None;
                    for e in SETUP_EDGES {
                        let key = format!("{k}{e}");
                        let mut v = d.s(&key);
                        if ui.add_sized([96.0, 22.0], egui::TextEdit::singleline(&mut v)).changed() {
                            d.fields.insert(key.clone(), Value::String(v));
                            changed = Some(key);
                        }
                    }
                    setup_link(ui, lang, d, k, changed);
                    ui.end_row();
                }
            });
        });
    ui.add_space(6.0);
    // Adjust Layout: apply with objects following the new page size and margins.
    let mut adjust = false;
    setup_group(ui, |ui| {
        setup_row(ui, rtl, |ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            ui.painter().circle_stroke(r.center(), 8.0, egui::Stroke::new(1.2, t.icon));
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(12.0), t.icon);
            crate::rtl::label(ui, tr("Adjust page elements to document changes"));
            let layout = if rtl { egui::Layout::left_to_right(egui::Align::Center) } else { egui::Layout::right_to_left(egui::Align::Center) };
            ui.with_layout(layout, |ui| {
                let button = egui::Button::new(crate::rtl::widget(ui, tr("Adjust Layout"))).corner_radius(12.0).min_size(egui::vec2(120.0, 24.0));
                if ui.add(button).on_hover_text(tr("Apply the changes; objects follow the new page size and margins")).clicked() {
                    adjust = true;
                }
            });
        });
    });
    if adjust {
        d.fields.insert("adjustLayout".into(), json!(true));
    }
    adjust
}

/// A linked group's edges follow the one that `changed`.
fn setup_link_follow(d: &mut Dialog, group: &str, changed: Option<String>) {
    if !d.b(&format!("{group}Linked")) {
        return;
    }
    if let Some(v) = changed.and_then(|k| d.fields.get(&k).cloned()) {
        for e in SETUP_EDGES {
            d.fields.insert(format!("{group}{e}"), v.clone());
        }
    }
}

/// Document Setup's OK: `layout.documentSetup` with what was typed; a value that can't be read is
/// refused before anything changes. The margins (the first page's, shown) go to every page only
/// when they were edited, so pages keep their own margins otherwise.
fn confirm_document_setup(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let measure = |key: &str| setup_measure(d, key).ok_or_else(|| format!("{key}: not a measure"));
    let edges = |group: &str| -> Result<Vec<f64>, String> { SETUP_EDGES.iter().map(|e| measure(&format!("{group}{e}"))).collect() };
    let count = |key: &str| d.n(key).filter(|n| n.is_finite() && *n >= 1.0).map(|n| n as u64).ok_or_else(|| format!("{key}: a whole number from 1"));
    let margins = edges("margin")?;
    let shown = d.fields.get("marginsShown").and_then(Value::as_array);
    let edited = SETUP_EDGES.iter().enumerate().any(|(i, e)| shown.and_then(|s| s.get(i)) != d.fields.get(&format!("margin{e}")));
    let mut p = json!({"intent": d.s("intent"), "pages": count("pages")?, "startPage": count("startPage")?, "facingPages": d.b("facingPages"),
        "binding": d.s("binding"), "width": measure("width")?, "height": measure("height")?,
        "bleed": edges("bleed")?, "slug": edges("slug")?, "adjustLayout": d.b("adjustLayout")});
    if edited {
        p["margins"] = json!({"top": margins[0], "bottom": margins[1], "inside": margins[2], "outside": margins[3]});
    }
    app.run("layout.documentSetup", p)
}

/// Preferences: a section list and the section's options. Application options always; with a
/// document open, its own settings (InDesign keeps those with the document); with none, Units &
/// Increments for new documents.
fn preferences(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog, max_height: f32) {
    let has_doc = d.fields.contains_key("horizontalUnits") && !d.b("defaults");
    let sections: &[(&str, &str)] = if has_doc {
        &[
            ("general", "General"),
            ("interface", "Interface"),
            ("uiScaling", "UI Scaling"),
            ("type", "Type"),
            ("advancedType", "Advanced Type"),
            ("composition", "Composition"),
            ("units", "Units & Increments"),
            ("grids", "Grids"),
            ("guides", "Guides & Pasteboard"),
            ("dictionary", "Dictionary"),
            ("spelling", "Spelling"),
            ("autocorrect", "Autocorrect"),
            ("notes", "Notes"),
            ("trackChanges", "Track Changes"),
            ("storyEditor", "Story Editor Display"),
            ("display", "Display Performance"),
            ("black", "Appearance of Black"),
            ("files", "File Handling"),
        ]
    } else {
        &[
            ("general", "General"),
            ("interface", "Interface"),
            ("uiScaling", "UI Scaling"),
            ("type", "Type"),
            ("composition", "Composition"),
            ("units", "Units & Increments"),
            ("spelling", "Spelling"),
            ("autocorrect", "Autocorrect"),
            ("notes", "Notes"),
            ("trackChanges", "Track Changes"),
            ("storyEditor", "Story Editor Display"),
            ("display", "Display Performance"),
            ("files", "File Handling"),
        ]
    };
    let cur = d.s("section");
    ui.horizontal_top(|ui| {
        // Bound both panes independently so selecting a lower section keeps its options visible.
        let height = max_height.min(440.0);
        ui.set_min_height(240.0_f32.min(height));
        ui.set_max_height(height);
        ui.vertical(|ui| {
            ui.set_width(150.0);
            egui::ScrollArea::vertical().id_salt("preferences_sections").min_scrolled_height(1.0).max_height(height).show(ui, |ui| {
                for (id, label) in sections {
                    if ui.selectable_label(cur == *id, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                        d.fields.insert("section".into(), json!(id));
                    }
                }
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_width(300.0);
            egui::ScrollArea::vertical().id_salt(("preferences_options", &cur)).min_scrolled_height(1.0).max_height(height).show(ui, |ui| match cur
                .as_str()
            {
                "dictionary" => {
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "User Dictionary (this document)")).font(semibold(12.0)),
                    ));
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Words, one per line:"));
                    let mut t = d.s("userWords");
                    if egui::ScrollArea::vertical()
                        .max_height(170.0)
                        .show(ui, |ui| ui.add(egui::TextEdit::multiline(&mut t).desired_rows(8).desired_width(280.0)))
                        .inner
                        .changed()
                    {
                        d.fields.insert("userWords".into(), json!(t));
                    }
                }
                "spelling" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Dynamic Spelling")).font(semibold(12.0)));
                    check(ui, d, "dynamicSpelling", crate::i18n::tr(&app.ui.language, "Enable Dynamic Spelling (underline misspelled words)"));
                }
                "autocorrect" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Options")).font(semibold(12.0)));
                    check(ui, d, "autocorrect", crate::i18n::tr(&app.ui.language, "Enable Autocorrect"));
                    ui.add_space(6.0);
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Misspelled word → Correction (one per line):"));
                    let mut t = d.s("autocorrectText");
                    if egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| ui.add(egui::TextEdit::multiline(&mut t).desired_rows(8).desired_width(280.0)))
                        .inner
                        .changed()
                    {
                        d.fields.insert("autocorrectText".into(), json!(t));
                    }
                }
                "notes" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Options")).font(semibold(12.0)));
                    check(ui, d, "showNoteAnchors", crate::i18n::tr(&app.ui.language, "Show Note Anchors in Layout View"));
                }
                "trackChanges" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Show")).font(semibold(12.0)));
                    check(ui, d, "showAddedText", crate::i18n::tr(&app.ui.language, "Added Text (highlighted)"));
                }
                "storyEditor" => {
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Text Display Options")).font(semibold(12.0)),
                    ));
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Size:"));
                        let mut v = d.n("storyEditorSize").unwrap_or(14.0);
                        if ui.add(egui::Slider::new(&mut v, 8.0..=36.0).step_by(1.0).suffix(" pt")).changed() {
                            d.fields.insert("storyEditorSize".into(), json!(v));
                        }
                    });
                }
                "files" => {
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Document Recovery Data")).font(semibold(12.0)),
                    ));
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Save recovery data every:"));
                        let mut v = d.n("recoveryMinutes").unwrap_or(0.5);
                        if ui.add(egui::DragValue::new(&mut v).range(0.1..=60.0).speed(0.1).suffix(" min")).changed() {
                            d.fields.insert("recoveryMinutes".into(), json!(v));
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Number of Recent Items to Display:"));
                        let mut v = d.n("recentFilesMax").unwrap_or(20.0);
                        // A count: a whole number ("20", not "20.0").
                        let field =
                            egui::DragValue::new(&mut v).range(0.0..=f64::from(designcraft_engine::MAX_RECENT_FILES)).speed(0.2).fixed_decimals(0);
                        if ui.add(field).changed() {
                            d.fields.insert("recentFilesMax".into(), json!(v.round()));
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Links")).font(semibold(12.0))));
                    check(ui, d, "linkTextFiles", crate::i18n::tr(&app.ui.language, "Create Links When Placing Text Files"));
                }
                "type" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Type Options")).font(semibold(12.0)));
                    check(ui, d, "typographersQuotes", crate::i18n::tr(&app.ui.language, "Use Typographer's Quotes"));
                    check(ui, d, "showFontNamesInEnglish", crate::i18n::tr(&app.ui.language, "Show Font Names in English"));
                    check(ui, d, "cjkFeatures", crate::i18n::tr(&app.ui.language, "Show CJK Features"));
                    ui.add_space(6.0);
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Smart Text Reflow")).font(semibold(12.0)),
                    ));
                    check(
                        ui,
                        d,
                        "smartTextReflow",
                        crate::i18n::tr(&app.ui.language, "Add and remove pages as the primary text frame's story grows and shrinks"),
                    );
                }
                "composition" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Highlight")).font(semibold(12.0)));
                    check(ui, d, "highlightKeeps", crate::i18n::tr(&app.ui.language, "Keep Violations"));
                    check(ui, d, "highlightHj", crate::i18n::tr(&app.ui.language, "H&J Violations"));
                    check(ui, d, "highlightCustomTracking", crate::i18n::tr(&app.ui.language, "Custom Tracking/Kerning"));
                    check(ui, d, "highlightSubstitutedFonts", crate::i18n::tr(&app.ui.language, "Substituted Fonts"));
                    if d.fields.contains_key("glyphFallback") {
                        ui.add_space(6.0);
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Missing Glyphs")).font(semibold(12.0)));
                        check(ui, d, "glyphFallback", crate::i18n::tr(&app.ui.language, "Draw Missing Glyphs from Fallback Fonts"));
                    }
                }
                "advancedType" => {
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Character Settings")).font(semibold(12.0)),
                    ));
                    egui::Grid::new("pref_adv").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("");
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Size"));
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Position"));
                        ui.end_row();
                        for (k, l) in [("superscript", "Superscript:"), ("subscript", "Subscript:")] {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
                            for f in ["Size", "Position"] {
                                ui.horizontal(|ui| {
                                    text_field(ui, d, &format!("adv.{k}{f}"), 50.0);
                                    ui.label("%");
                                });
                            }
                            ui.end_row();
                        }
                    });
                }
                "black" => {
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Options for Black on RGB and Grayscale Devices")).font(semibold(12.0)),
                    ));
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "On Screen:"));
                        let rich = d.b("richBlack");
                        craft_ui::ComboBox::from_id_salt("pref_black")
                            .selected_text(crate::rtl::widget(
                                ui,
                                crate::i18n::tr(
                                    &app.ui.language,
                                    if rich { "Display All Blacks as Rich Black" } else { "Display All Blacks Accurately" },
                                ),
                            ))
                            .width(240.0)
                            .show_ui(ui, |ui| {
                                for (v, l) in [(false, "Display All Blacks Accurately"), (true, "Display All Blacks as Rich Black")] {
                                    if ui.selectable_label(v == rich, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                                        d.fields.insert("richBlack".into(), json!(v));
                                    }
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Printing / Exporting:"));
                        let rich = d.b("richBlackOutput");
                        craft_ui::ComboBox::from_id_salt("pref_black_out")
                            .selected_text(crate::rtl::widget(
                                ui,
                                crate::i18n::tr(
                                    &app.ui.language,
                                    if rich { "Output All Blacks as Rich Black" } else { "Output All Blacks Accurately" },
                                ),
                            ))
                            .width(240.0)
                            .show_ui(ui, |ui| {
                                for (v, l) in [(false, "Output All Blacks Accurately"), (true, "Output All Blacks as Rich Black")] {
                                    if ui.selectable_label(v == rich, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                                        d.fields.insert("richBlackOutput".into(), json!(v));
                                    }
                                }
                            });
                    });
                    if d.fields.contains_key("overprintBlack") {
                        ui.add_space(6.0);
                        ui.label(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "[Black] Overprint")).font(semibold(12.0)),
                        ));
                        check(ui, d, "overprintBlack", crate::i18n::tr(&app.ui.language, "Overprint [Black] Swatch at 100%"));
                    }
                }
                "interface" => {
                    let lang = app.ui.language.clone();
                    let t = crate::theme::Tokens::get(ui.ctx());
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Appearance")).font(semibold(12.0)));
                    // Color Theme: a swatch per theme, as InDesign's. A click shows it at once;
                    // Cancel goes back to the theme Preferences opened with.
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&lang, "Color Theme:"));
                        let cur = d.s("brightness");
                        for b in crate::theme::Brightness::ALL {
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                            let on = cur == b.id();
                            let swatch = r.shrink(4.0);
                            ui.painter().rect_filled(swatch, 1.0, crate::theme::Tokens::for_brightness(b).panel);
                            ui.painter().rect_stroke(swatch, 1.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
                            if on || resp.hovered() {
                                let ring = if on { t.accent_strong } else { t.field_border };
                                ui.painter().rect_stroke(r.shrink(1.0), 2.0, egui::Stroke::new(2.0, ring), egui::StrokeKind::Inside);
                            }
                            let name = crate::i18n::tr(&lang, b.label()).to_string();
                            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, &name));
                            if resp.on_hover_text(&name).clicked() && !on {
                                d.fields.insert("brightness".into(), json!(b.id()));
                                d.fields.insert("applyBrightness".into(), json!(b.id()));
                            }
                        }
                    });
                    ui.add_space(8.0);
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Panels")).font(semibold(12.0)));
                    ui.horizontal(|ui| {
                        let name = crate::rtl::label(ui, crate::i18n::tr(&lang, "Tools Panel:"));
                        translated_combo(&lang, ui, d, "toolsColumns", &[("single", "Single Column"), ("double", "Double Column")])
                            .labelled_by(name.id);
                    });
                }
                "uiScaling" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "UI Scaling")).font(semibold(12.0)));
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "UI Size:"));
                        let mut v = d.n("uiScale").unwrap_or(100.0);
                        if ui.add(egui::Slider::new(&mut v, 50.0..=200.0).step_by(5.0).suffix("%")).changed() {
                            d.fields.insert("uiScale".into(), json!(v));
                        }
                    });
                    ui.add_space(8.0);
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Panels")).font(semibold(12.0)));
                    check(ui, d, "autoCollapseIconicPanels", crate::i18n::tr(&app.ui.language, "Auto-Collapse Iconic Panels"));
                }
                "grids" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Baseline Grid")).font(semibold(12.0)));
                    egui::Grid::new("pref_bg").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Color:"));
                        color_field(ui, d, "bg.color");
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Start:"));
                        text_field(ui, d, "bg.start", 80.0);
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Relative To:"));
                        let cur = d.s("bg.relativeTo");
                        craft_ui::ComboBox::from_id_salt("bg_rel")
                            .selected_text(crate::rtl::widget(
                                ui,
                                crate::i18n::tr(&app.ui.language, if cur == "topMargin" { "Top Margin" } else { "Top of Page" }),
                            ))
                            .show_ui(ui, |ui| {
                                for (v, l) in [("topOfPage", "Top of Page"), ("topMargin", "Top Margin")] {
                                    if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                                        d.fields.insert("bg.relativeTo".into(), json!(v));
                                    }
                                }
                            });
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Increment Every:"));
                        text_field(ui, d, "bg.increment", 80.0);
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "View Threshold:"));
                        ui.horizontal(|ui| {
                            text_field(ui, d, "bg.viewThreshold", 50.0);
                            ui.label("%");
                        });
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Document Grid")).font(semibold(12.0)));
                    egui::Grid::new("pref_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Color:"));
                        color_field(ui, d, "grid.color");
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Horizontal Gridline Every:"));
                        text_field(ui, d, "grid.horizontal", 80.0);
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Vertical Gridline Every:"));
                        text_field(ui, d, "grid.vertical", 80.0);
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Subdivisions:"));
                        text_field(ui, d, "grid.subdivisions", 50.0);
                        ui.end_row();
                    });
                    check(ui, d, "grid.inBack", crate::i18n::tr(&app.ui.language, "Grids in Back"));
                }
                "guides" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Color")).font(semibold(12.0)));
                    egui::Grid::new("pref_guides").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        for (k, l) in [("marginColor", "Margins:"), ("columnColor", "Columns:"), ("bleedColor", "Bleed:"), ("slugColor", "Slug:")] {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
                            color_field(ui, d, k);
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Pasteboard Options")).font(semibold(12.0)),
                    ));
                    egui::Grid::new("pref_pb").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Horizontal Margins:"));
                        text_field(ui, d, "pasteboard.h", 80.0);
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Vertical Margins:"));
                        text_field(ui, d, "pasteboard.v", 80.0);
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    check(ui, d, "snap.alignEdges", crate::i18n::tr(&app.ui.language, "Align to Object Edges"));
                    check(ui, d, "snap.alignCenters", crate::i18n::tr(&app.ui.language, "Align to Object Centers"));
                    check(ui, d, "snap.dimensions", crate::i18n::tr(&app.ui.language, "Smart Dimensions"));
                    check(ui, d, "snap.spacing", crate::i18n::tr(&app.ui.language, "Smart Spacing"));
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Snap to Zone"));
                        let mut zone = d.n("snap.zone").filter(|z| z.is_finite()).unwrap_or(4.0);
                        if ui.add(egui::DragValue::new(&mut zone).speed(0.1)).changed() {
                            d.fields.insert("snap.zone".into(), json!(zone));
                        }
                    });
                }
                "display" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Options")).font(semibold(12.0)));
                    let cur = d.s("displayQuality");
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Default View:"));
                        craft_ui::ComboBox::from_id_salt("pref_dq")
                            .selected_text(match cur.as_str() {
                                "fast" => crate::i18n::tr(&app.ui.language, "Fast"),
                                "typical" => crate::i18n::tr(&app.ui.language, "Typical"),
                                _ => crate::i18n::tr(&app.ui.language, "High Quality"),
                            })
                            .show_ui(ui, |ui| {
                                for (v, l) in [("fast", "Fast"), ("typical", "Typical"), ("high", "High Quality")] {
                                    if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                                        d.fields.insert("displayQuality".into(), json!(v));
                                    }
                                }
                            });
                    });
                }
                "units" => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Ruler Units")).font(semibold(12.0)));
                    egui::Grid::new("pref_units").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Origin:"));
                        let cur = d.fields.get("rulerOrigin").and_then(Value::as_str).unwrap_or("spread").to_string();
                        const ORIGINS: [(&str, &str); 3] = [("spread", "Spread"), ("page", "Page"), ("spine", "Spine")];
                        let shown = ORIGINS.iter().find(|o| o.0 == cur).map_or("Spread", |o| o.1);
                        craft_ui::ComboBox::from_id_salt("rulerOrigin")
                            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, shown)))
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for (v, l) in ORIGINS {
                                    if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                                        d.fields.insert("rulerOrigin".into(), json!(v));
                                    }
                                }
                            });
                        ui.end_row();
                        for (key, label) in [("horizontalUnits", "Horizontal:"), ("verticalUnits", "Vertical:")] {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                            let cur: Unit = d.fields.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                            craft_ui::ComboBox::from_id_salt(key)
                                .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, cur.label())))
                                .width(140.0)
                                .show_ui(ui, |ui| {
                                    for u in Unit::ALL {
                                        if ui
                                            .selectable_label(u == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, u.label())))
                                            .clicked()
                                        {
                                            d.fields.insert(key.into(), json!(u));
                                        }
                                    }
                                });
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(crate::rtl::widget(
                        ui,
                        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Keyboard Increments")).font(semibold(12.0)),
                    ));
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Cursor Key:"));
                        text_field(ui, d, "keyboardIncrement", 80.0);
                    });
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Kerning/Tracking:"));
                        text_field(ui, d, "kerningIncrement", 60.0);
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "/1000 em"));
                    });
                }
                _ => {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Interface Language")).font(semibold(12.0)));
                    // Each language by its own name, so the list reads the same in every interface language.
                    let current = crate::i18n::LANGUAGES.iter().find(|(code, _)| *code == app.ui.language).map_or("English", |(_, name)| *name);
                    craft_ui::ComboBox::from_id_salt("pref_language")
                        .width(180.0)
                        .height(400.0)
                        .selected_text(crate::rtl::widget(ui, current))
                        .show_ui(ui, |ui| {
                            for (code, name) in crate::i18n::picker_languages() {
                                if ui.selectable_label(code == app.ui.language, crate::rtl::widget(ui, name)).clicked() {
                                    // Applied by `show` right after this frame, which has `&mut DesignApp`.
                                    d.fields.insert("applyLanguage".into(), json!(code));
                                }
                            }
                        });
                    ui.add_space(6.0);
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "When Scaling")).font(semibold(12.0)));
                    check(ui, d, "scaleStrokes", crate::i18n::tr(&app.ui.language, "Include Stroke Weight"));
                    ui.add_space(6.0);
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Transform")).font(semibold(12.0)));
                    check(ui, d, "dimensionsIncludeStroke", crate::i18n::tr(&app.ui.language, "Dimensions Include Stroke Weight"));
                    check(ui, d, "transformationsAreTotals", crate::i18n::tr(&app.ui.language, "Transformations are Totals"));
                    ui.add_space(6.0);
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page Numbering")).font(semibold(12.0)));
                    check(ui, d, "absolutePageNumbers", crate::i18n::tr(&app.ui.language, "Absolute Numbering (instead of Section Numbering)"));
                }
            });
        });
    });
}

/// An `[r, g, b]` field edited with a colour button.
fn color_field(ui: &mut egui::Ui, d: &mut Dialog, key: &str) {
    let v: [u8; 3] = d.fields.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or([128, 128, 128]);
    let mut c = v;
    if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
        d.fields.insert(key.into(), json!(c));
    }
}

fn check(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str) {
    let mut b = d.b(key);
    if ui.checkbox(&mut b, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(key.into(), Value::Bool(b));
    }
}

/// An alert's message. Import warnings (`lines`: `{text, item}`) list one per line, scrolling
/// when there are many, with Show beside each that names a frame (sets `show`).
fn alert_message(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lines: Vec<(String, Option<u64>)> = d
        .fields
        .get("lines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|l| (l.get("text").and_then(Value::as_str).unwrap_or_default().to_string(), l.get("item").and_then(Value::as_u64)))
        .collect();
    if lines.iter().all(|(_, item)| item.is_none()) {
        crate::rtl::label(ui, d.s("message"));
        return;
    }
    let show = crate::i18n::tr(&app.ui.language, "Show");
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        for (text, item) in &lines {
            ui.horizontal_wrapped(|ui| {
                if let Some(item) = item
                    && ui.small_button(crate::rtl::widget(ui, show)).clicked()
                {
                    d.fields.insert("show".into(), json!(item));
                }
                crate::rtl::label(ui, text.as_str());
            });
        }
    });
}

/// Draw the active localized dialog; open popups own Enter/Escape, and result handling drives confirm or cancel.
pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let shortcuts = app.ui.dialog.as_ref().is_some_and(|d| d.id == crate::shortcuts_dialog::DIALOG_ID);
    if shortcuts {
        crate::shortcuts_dialog::show(app, ctx);
        return;
    }
    if app.shortcut_dialog.is_some() {
        // Another dialog took its place: its edits are cancelled.
        let _ = crate::shortcuts_dialog::close(app, false);
    }
    let Some(mut d) = app.ui.dialog.clone() else { return };
    // Enter and Escape belong to an open popup (a menu, a name field) or to Find/Change's Change
    // Format Settings rather than the dialog. Read before the dialog draws: a popup closed by this
    // frame's Escape is still open here.
    let popup_open = egui::Popup::is_any_open(ctx) || d.fields.get(FORMAT_EDIT).is_some_and(Value::is_object);
    let mut result: Option<bool> = None;
    let alert_title = d.s("title");
    let title = match d.id.as_str() {
        "alert" => crate::i18n::tr(&app.ui.language, &alert_title),
        "ruby" => crate::i18n::tr(&app.ui.language, "Ruby"),
        "newDocument" => crate::i18n::tr(&app.ui.language, "New Document"),
        "frameSize" => crate::i18n::tr(&app.ui.language, "Rectangle"),
        "goToPage" => crate::i18n::tr(&app.ui.language, "Go to Page"),
        "insertPages" => crate::i18n::tr(&app.ui.language, "Insert Pages"),
        "insertTable" => crate::i18n::tr(&app.ui.language, "Insert Table…").trim_end_matches('…').trim_end(),
        // The menu items' translations (every language has them), without the ellipsis.
        "convertTextToTable" => crate::i18n::tr(&app.ui.language, "Convert Text to Table…").trim_end_matches('…').trim_end(),
        "convertTableToText" => crate::i18n::tr(&app.ui.language, "Convert Table to Text…").trim_end_matches('…').trim_end(),
        "cellOptions" => crate::i18n::tr(&app.ui.language, "Cell Options"),
        "createGuides" => crate::i18n::tr(&app.ui.language, "Create Guides"),
        "marginsAndColumns" => crate::i18n::tr(&app.ui.language, "Margins and Columns"),
        "textFrameOptions" => crate::i18n::tr(&app.ui.language, "Text Frame Options"),
        "colorTheme" => crate::i18n::tr(&app.ui.language, "Color Theme"),
        "documentSetup" => crate::i18n::tr(&app.ui.language, "Document Setup"),
        "fileInfo" => crate::i18n::tr(&app.ui.language, "File Info"),
        // On a page that doesn't start a section yet, InDesign titles it New Section.
        "numberingSection" if d.b("newSection") => crate::i18n::tr(&app.ui.language, "New Section"),
        "numberingSection" => crate::i18n::tr(&app.ui.language, "Numbering & Section Options…").trim_end_matches('…'),
        "findChange" => crate::i18n::tr(&app.ui.language, "Find/Change"),
        "paragraphStyleOptions" if d.b("new") => crate::i18n::tr(&app.ui.language, "New Paragraph Style"),
        "paragraphStyleOptions" => crate::i18n::tr(&app.ui.language, "Paragraph Style Options"),
        "deleteParagraphStyle" => crate::i18n::tr(&app.ui.language, "Delete Paragraph Style"),
        "characterStyleOptions" if d.b("new") => crate::i18n::tr(&app.ui.language, "New Character Style"),
        "characterStyleOptions" => crate::i18n::tr(&app.ui.language, "Character Style Options"),
        "deleteCharacterStyle" => crate::i18n::tr(&app.ui.language, "Delete Character Style"),
        "footnoteOptions" => crate::i18n::tr(&app.ui.language, "Footnote Options"),
        "paragraphRules" => crate::i18n::tr(&app.ui.language, "Paragraph Rules"),
        "paragraphLocal" => crate::i18n::tr(
            &app.ui.language,
            match d.s("section").as_str() {
                "span" => "Span Columns",
                "bullets" => "Bullets and Numbering",
                "keep" => "Keep Options",
                "nested" => "Drop Caps and Nested Styles",
                "grep" => "GREP Styles",
                _ => "Paragraph Borders and Shading",
            },
        ),
        "justification" => crate::i18n::tr(&app.ui.language, "Justification"),
        "hyphenation" => crate::i18n::tr(&app.ui.language, "Hyphenation Settings"),
        "insertXref" => crate::i18n::tr(&app.ui.language, "New Cross-Reference"),
        "findFont" => crate::i18n::tr(&app.ui.language, "Find/Replace Font"),
        "polygonSettings" => crate::i18n::tr(&app.ui.language, "Polygon Settings"),
        "userDictionary" => crate::i18n::tr(&app.ui.language, "User Dictionary"),
        "newWorkspace" => crate::i18n::tr(&app.ui.language, "New Workspace"),
        "closeDocument" => crate::i18n::tr(&app.ui.language, "Unsaved Changes"),
        "deletePages" => crate::i18n::tr(&app.ui.language, "Warning"),
        "pagesPanelOptions" => crate::i18n::tr(&app.ui.language, "Panel Options"),
        "updateLink" => crate::i18n::tr(&app.ui.language, "Update Link"),
        "menus" => crate::i18n::tr(&app.ui.language, "Menu Customization"),
        "importOptions" => crate::i18n::tr(&app.ui.language, "Import Options"),
        "loadStyles" => crate::i18n::tr(&app.ui.language, "Load Styles"),
        "fittingOptions" => crate::i18n::tr(&app.ui.language, "Frame Fitting Options"),
        "qrCode" => crate::i18n::tr(&app.ui.language, "Generate QR Code"),
        "preferences" => crate::i18n::tr(&app.ui.language, "Preferences"),
        "print" => crate::i18n::tr(&app.ui.language, "Print"),
        "pdfImport" => crate::i18n::tr(&app.ui.language, "Place PDF"),
        "pdfExport" => crate::i18n::tr(&app.ui.language, "Export PDF"),
        "epubExport" => crate::i18n::tr(&app.ui.language, "EPUB Export Options"),
        "colorSettings" => crate::i18n::tr(&app.ui.language, "Color Settings"),
        "layerOptions" => crate::i18n::tr(&app.ui.language, "Object Layer Options"),
        "colorPicker" => crate::i18n::tr(&app.ui.language, "Color Picker"),
        "swatchOptions" => crate::i18n::tr(&app.ui.language, "Swatch Options"),
        "colorGroupOptions" => crate::i18n::tr(&app.ui.language, "Color Group Options"),
        "layerSettings" => crate::i18n::tr(&app.ui.language, "Layer Options"),
        crate::effects_dialog::ID => crate::i18n::tr(&app.ui.language, "Effects"),
        crate::package_dialog::ID => crate::i18n::tr(&app.ui.language, "Package"),
        crate::package_dialog::INSTRUCTIONS => crate::i18n::tr(&app.ui.language, "Printing Instructions"),
        id => match id.strip_prefix("cmd:").and_then(designcraft_engine::find_command) {
            Some(c) => crate::i18n::tr(&app.ui.language, c.label.trim_end_matches('…')),
            None => crate::i18n::tr(&app.ui.language, "Dialog"),
        },
    };
    // No dimmed backdrop: the page behind a dialog is what its settings and colours are judged against.
    let host = egui::Id::unique("dialog");
    let instance = host.with(&d.id);
    crate::modal_drag::modal(ctx, host, instance).backdrop_color(egui::Color32::TRANSPARENT).show(ctx, |ui| {
        let frame_margin = egui::Frame::popup(ui.style()).total_margin().sum();
        let available = (ctx.content_rect().size() - frame_margin - egui::vec2(16.0, 16.0)).max(egui::Vec2::splat(1.0));
        let preferred_width: f32 = match d.id.as_str() {
            "alert" => 440.0,
            "closeDocument" | "updateLink" | "deletePages" | "newWorkspace" | "pagesPanelOptions" | "insertTable" | "convertTableToText" => 380.0,
            "insertPages" => 440.0,
            "convertTextToTable" => 420.0,
            "newDocument" => 960.0,
            "hyphenation" => 560.0,
            "numberingSection" => 560.0,
            crate::effects_dialog::ID => 720.0,
            "colorPicker" => 580.0,
            crate::package_dialog::ID => 720.0,
            crate::package_dialog::INSTRUCTIONS => 520.0,
            _ => 640.0,
        };
        let width = preferred_width.min(available.x);
        ui.set_min_width(380.0_f32.min(width));
        ui.set_max_width(width);
        let title_top = ui.cursor().top();
        if crate::i18n::is_rtl(&app.ui.language) {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::rtl::label(ui, egui::RichText::new(title).font(semibold(16.0)));
                });
            });
        } else {
            crate::rtl::label(ui, egui::RichText::new(title).font(semibold(16.0)));
        }
        let margin = egui::Frame::popup(ui.style()).inner_margin;
        let title_row = egui::Rect::from_min_max(
            egui::pos2(ui.max_rect().left() - margin.leftf(), title_top - margin.topf()),
            egui::pos2(ui.max_rect().right() + margin.rightf(), ui.min_rect().bottom()),
        );
        crate::modal_drag::title_drag(ui, instance, title_row);
        ui.add_space(10.0);
        // Keep the title and action buttons outside the scrollable body. Oversized
        // forms remain reachable at large UI scales, without shrinking their text.
        // Numbering & Section Options keeps OK and Cancel in a column beside the options, as
        // InDesign does; other dialogs have them in a footer.
        let side_actions = d.id == "numberingSection";
        let footer_height = if side_actions {
            0.0
        } else {
            ui.spacing().interact_size.y.max(ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y)
        };
        let body_height = (available.y - ui.min_size().y - footer_height - 12.0 - 2.0 * ui.spacing().item_spacing.y).max(1.0);
        let mut body_width = width;
        if side_actions {
            // The column is beside the options, not in the layout: keep the window wide enough.
            ui.set_min_width(width);
            body_width = (width - crate::section_options::ACTIONS_W - 12.0).max(1.0);
            let top = ui.cursor().min;
            // On the right in every language, like the other dialogs' OK.
            let column = egui::Rect::from_min_size(
                egui::pos2(top.x + width - crate::section_options::ACTIONS_W, top.y),
                egui::vec2(crate::section_options::ACTIONS_W, 60.0),
            );
            let mut actions = ui.new_child(egui::UiBuilder::new().max_rect(column).layout(egui::Layout::top_down_justified(egui::Align::Center)));
            if let Some(ok) = crate::section_options::actions(app, &mut actions) {
                result = Some(ok);
            }
            if !popup_open && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                result = Some(true);
            }
            if !popup_open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                result = Some(false);
            }
        }
        // A centred window only offers its body last frame's height: let this one grow to its
        // options' height (the screen still caps it).
        let min_body = if side_actions { body_height.min(crate::section_options::BODY_H) } else { 1.0 };
        egui::ScrollArea::both()
            .id_salt(("dialog_body", &d.id))
            .min_scrolled_width(1.0)
            .min_scrolled_height(min_body)
            .max_width(body_width)
            .max_height(body_height)
            .show(ui, |ui| {
                match d.id.as_str() {
                    "newDocument" => {
                        // A double-clicked preset creates the document at once.
                        let create = crate::new_document::body(app, ui, &mut d, width, body_height);
                        if create {
                            result = Some(true);
                        }
                    }
                    "frameSize" => {
                        egui::Grid::new("fs").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Width"));
                            text_field(ui, &mut d, "width", 90.0);
                            ui.end_row();
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Height"));
                            text_field(ui, &mut d, "height", 90.0);
                            ui.end_row();
                        });
                    }
                    "findChange" => {
                        ui.horizontal(|ui| {
                            for (label, grep) in [("Text", false), ("GREP", true)] {
                                if ui
                                    .selectable_label(
                                        !d.b("objectMode") && d.b("grep") == grep,
                                        crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label)),
                                    )
                                    .clicked()
                                {
                                    if d.set_field("objectMode", json!(false)) {
                                        app.session.reset_find_scope();
                                    }
                                    d.fields.insert("grep".into(), json!(grep));
                                }
                            }
                            if ui.selectable_label(d.b("objectMode"), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Object"))).clicked()
                                && d.set_field("objectMode", json!(true))
                            {
                                app.session.reset_find_scope();
                            }
                        });
                        if d.b("objectMode") {
                            // Find/Change › Object: objects by fill and kind; change their fill or opacity.
                            let swatches: Vec<String> = app
                                .session
                                .active()
                                .map(|st| st.doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect())
                                .unwrap_or_default();
                            egui::Grid::new("fco").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                                for (key, label, any) in [("objFill", "Find fill:", "(any)"), ("objChangeFill", "Change fill to:", "(unchanged)")] {
                                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                                    let cur = d.s(key);
                                    craft_ui::ComboBox::from_id_salt(key)
                                        .selected_text(if cur.is_empty() { crate::i18n::tr(&app.ui.language, any) } else { cur.as_str() })
                                        .width(180.0)
                                        .show_ui(ui, |ui| {
                                            if ui
                                                .selectable_label(cur.is_empty(), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, any)))
                                                .clicked()
                                            {
                                                d.fields.insert(key.into(), json!(""));
                                            }
                                            for w in &swatches {
                                                if ui.selectable_label(*w == cur, w).clicked() {
                                                    d.fields.insert(key.into(), json!(w));
                                                }
                                            }
                                        });
                                    ui.end_row();
                                }
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Kind:"));
                                let cur = d.s("objKind");
                                craft_ui::ComboBox::from_id_salt("objKind")
                                    .selected_text(if cur.is_empty() { crate::i18n::tr(&app.ui.language, "(any)") } else { cur.as_str() })
                                    .show_ui(ui, |ui| {
                                        for v in ["", "text", "graphic", "shape", "line", "group"] {
                                            if ui
                                                .selectable_label(
                                                    cur == v,
                                                    crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if v.is_empty() { "(any)" } else { v })),
                                                )
                                                .clicked()
                                            {
                                                d.fields.insert("objKind".into(), json!(v));
                                            }
                                        }
                                    });
                                ui.end_row();
                            });
                            let mut crit = json!({});
                            if !d.s("objFill").is_empty() {
                                crit["fill"] = json!(d.s("objFill"));
                            }
                            if !d.s("objKind").is_empty() {
                                crit["kind"] = json!(d.s("objKind"));
                            }
                            ui.horizontal(|ui| {
                                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Find All"))).clicked() {
                                    let n = app.run("find.objects", crit.clone()).ok().and_then(|r| r["ids"].as_array().map(Vec::len)).unwrap_or(0);
                                    d.fields.insert("status".into(), json!(format!("{n} found")));
                                }
                                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change All"))).clicked()
                                    && !d.s("objChangeFill").is_empty()
                                {
                                    let mut p = crit.clone();
                                    p["change"] = json!({"fill": d.s("objChangeFill")});
                                    let n = app.run("find.changeObjects", p).ok().and_then(|r| r["changed"].as_u64()).unwrap_or(0);
                                    d.fields.insert("status".into(), json!(format!("{n} changed")));
                                }
                            });
                            ui.label(d.s("status"));
                        } else {
                            let (find_formatted, formatted) = (format_set(&d, FIND_FORMAT), format_set(&d, CHANGE_FORMAT));
                            egui::Grid::new("fc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Find what:"));
                                    // InDesign's ⓘ: a Find Format narrows what is found.
                                    if find_formatted {
                                        info_icon(
                                            ui,
                                            crate::i18n::tr(
                                                &app.ui.language,
                                                "With a Find Format, only text that has it is found; with nothing in Find what, all text that has it",
                                            ),
                                        );
                                    }
                                });
                                text_field_named(ui, &mut d, "find", 260.0, crate::i18n::tr(&app.ui.language, "Find what:"));
                                ui.end_row();
                                ui.horizontal(|ui| {
                                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Change to:"));
                                    // InDesign's ⓘ: with a Change Format, an empty Change to keeps the text.
                                    if formatted {
                                        info_icon(
                                            ui,
                                            crate::i18n::tr(
                                                &app.ui.language,
                                                "With a Change Format and nothing in Change to, the found text stays and is only formatted",
                                            ),
                                        );
                                    }
                                });
                                text_field_named(ui, &mut d, "change", 260.0, crate::i18n::tr(&app.ui.language, "Change to:"));
                                ui.end_row();
                                let search_label = crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Search:"));
                                let cur = d.s("scope");
                                // InDesign's names for the scopes.
                                let scope_name = |v: &str| match v {
                                    "story" => "Story",
                                    "selection" => "Selection",
                                    _ => "Document",
                                };
                                craft_ui::ComboBox::from_id_salt("fcscope")
                                    .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, scope_name(&cur))))
                                    .show_ui(ui, |ui| {
                                        for v in ["document", "story", "selection"] {
                                            if ui
                                                .selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, scope_name(v))))
                                                .clicked()
                                                && d.set_field("scope", json!(v))
                                            {
                                                app.session.reset_find_scope();
                                            }
                                        }
                                    })
                                    .response
                                    .labelled_by(search_label.id);
                                ui.end_row();
                            });
                            ui.horizontal(|ui| {
                                check(ui, &mut d, "caseSensitive", crate::i18n::tr(&app.ui.language, "Case sensitive"));
                                check(ui, &mut d, "wholeWord", crate::i18n::tr(&app.ui.language, "Whole word"));
                            });
                            format_box(&app.ui.language, ui, &mut d, true);
                            format_box(&app.ui.language, ui, &mut d, false);
                            ui.horizontal(|ui| {
                                let params0 = find_change_params(&d);
                                let params = || params0.clone();
                                // Change and Change/Find act on the text Find Next found.
                                let mut found = params0.clone();
                                found["found"] = json!(true);
                                let selected = app.session.active().and_then(|st| st.selection.text).is_some_and(|t| !t.is_caret());
                                let next_status = |app: &mut DesignApp, r: Result<Value, String>| match r {
                                    Ok(Value::Null) => crate::i18n::tr(&app.ui.language, "No matches").to_string(),
                                    Ok(_) => crate::i18n::tr(&app.ui.language, "Found").to_string(),
                                    Err(e) => e,
                                };
                                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Find Next"))).clicked() {
                                    let r = app.run("find.next", params());
                                    let msg = next_status(app, r);
                                    d.fields.insert("status".into(), json!(msg));
                                }
                                if ui
                                    .add_enabled(selected, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change"))))
                                    .clicked()
                                {
                                    let msg = match app.run("find.change", found.clone()) {
                                        Ok(v) => crate::i18n::tr(&app.ui.language, "{count} replacement(s) made")
                                            .replace("{count}", &v["count"].to_string()),
                                        Err(e) => e,
                                    };
                                    d.fields.insert("status".into(), json!(msg));
                                }
                                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change All"))).clicked() {
                                    let msg = match app.run("find.change", params()) {
                                        Ok(v) => crate::i18n::tr(&app.ui.language, "{count} replacement(s) made")
                                            .replace("{count}", &v["count"].to_string()),
                                        Err(e) => e,
                                    };
                                    d.fields.insert("status".into(), json!(msg));
                                }
                                if ui
                                    .add_enabled(
                                        selected,
                                        egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change/Find"))),
                                    )
                                    .clicked()
                                {
                                    let r = app.run("find.change", found.clone()).and_then(|_| app.run("find.next", params()));
                                    let msg = next_status(app, r);
                                    d.fields.insert("status".into(), json!(msg));
                                }
                                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Count"))).clicked() {
                                    let msg = match app.run("find.find", params()) {
                                        Ok(v) => crate::i18n::tr(&app.ui.language, "{count} match(es)")
                                            .replace("{count}", &v.as_array().map_or(0, |a| a.len()).to_string()),
                                        Err(e) => e,
                                    };
                                    d.fields.insert("status".into(), json!(msg));
                                }
                            });
                            if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
                                ui.label(egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
                            }
                        }
                    }
                    "paragraphStyleOptions" => paragraph_style_options(app, ui, &mut d),
                    "deleteParagraphStyle" => delete_paragraph_style(app, ui, &mut d),
                    "characterStyleOptions" => character_style_options(app, ui, &mut d),
                    "deleteCharacterStyle" => delete_character_style(app, ui, &mut d),
                    "footnoteOptions" => footnote_options(app, ui, &mut d),
                    "paragraphRules" => {
                        let current = d.fields.get("current").cloned().unwrap_or_default();
                        paragraph_rules(app, ui, &mut d, "", &current);
                    }
                    "paragraphLocal" => paragraph_local(app, ui, &mut d),
                    "justification" => {
                        let current = d.fields.get("current").cloned().unwrap_or_default();
                        justification(&app.ui.language, ui, &mut d, "", &current);
                    }
                    "hyphenation" => {
                        let current = d.fields.get("current").cloned().unwrap_or_default();
                        let units = app.session.active().map_or(Unit::Picas, |st| st.doc.settings.horizontal_units);
                        let preview = crate::i18n::tr(&app.ui.language, "Preview").to_string();
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| hyphenation(&app.ui.language, ui, &mut d, "", &current, units));
                            ui.add_space(24.0);
                            check(ui, &mut d, "preview", &preview);
                        });
                        hyphenation_preview(app, &mut d);
                        if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
                            ui.add_space(6.0);
                            crate::rtl::label(ui, egui::RichText::new(st).color(ui.visuals().error_fg_color));
                        }
                    }
                    "insertXref" => insert_xref(app, ui, &mut d),
                    "findFont" => find_font(app, ui, &mut d),
                    "colorPicker" => color_picker(app, ui, &mut d),
                    "swatchOptions" => options::swatch_options(app, ui, &mut d),
                    "colorGroupOptions" => options::color_group_options(app, ui, &mut d),
                    "layerSettings" => options::layer_options(app, ui, &mut d),
                    crate::effects_dialog::ID => crate::effects_dialog::show(app, ui, &mut d),
                    crate::package_dialog::ID => crate::package_dialog::show(app, ui, &mut d),
                    crate::package_dialog::INSTRUCTIONS => crate::package_dialog::show_instructions(app, ui, &mut d),
                    "preferences" => preferences(app, ui, &mut d, body_height),
                    "print" => print_dialog(app, ui, &mut d),
                    "pdfImport" => {
                        ui.label(egui::RichText::new(if d.b("async") { d.s("name") } else { d.s("path") }).size(11.0));
                        ui.horizontal(|ui| {
                            crate::rtl::label(
                                ui,
                                crate::i18n::tr(&app.ui.language, "Page (1–{count}):").replace("{count}", &d.n("pages").unwrap_or(1.0).to_string()),
                            );
                            text_field(ui, &mut d, "page", 60.0);
                        });
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Crop to:"));
                            let cur = d.s("crop");
                            let label = |v: &str| match v {
                                "contentVisible" => crate::i18n::tr(&app.ui.language, "Bounding Box (Visible Layers Only)"),
                                "contentAll" => crate::i18n::tr(&app.ui.language, "Bounding Box (All Layers)"),
                                "trim" => crate::i18n::tr(&app.ui.language, "Trim"),
                                "bleed" => crate::i18n::tr(&app.ui.language, "Bleed"),
                                "art" => crate::i18n::tr(&app.ui.language, "Art"),
                                "media" => crate::i18n::tr(&app.ui.language, "Media"),
                                _ => crate::i18n::tr(&app.ui.language, "Crop"),
                            };
                            craft_ui::ComboBox::from_id_salt("pdf_crop").selected_text(crate::rtl::widget(ui, label(&cur))).show_ui(ui, |ui| {
                                for v in designcraft_doc::PdfCrop::ALL.map(designcraft_doc::PdfCrop::name) {
                                    if ui.selectable_label(label(&cur) == label(v), label(v)).clicked() {
                                        d.fields.insert("crop".into(), json!(v));
                                    }
                                }
                            });
                        });
                    }
                    "layerOptions" => {
                        let mut layers = d.fields.get("layers").and_then(Value::as_array).cloned().unwrap_or_default();
                        if layers.is_empty() {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "This PDF has no layers."));
                        }
                        for l in &mut layers {
                            let mut on = l["visible"].as_bool().unwrap_or(true);
                            if ui.checkbox(&mut on, l["name"].as_str().unwrap_or("")).changed()
                                && let Some(o) = l.as_object_mut()
                            {
                                o.insert("visible".into(), json!(on));
                                d.fields.insert("useDefaults".into(), json!(false));
                            }
                        }
                        if ui.button(crate::i18n::tr(&app.ui.language, "Use File Defaults")).clicked() {
                            for layer in &mut layers {
                                layer["visible"] = layer["default"].clone();
                            }
                            d.fields.insert("useDefaults".into(), json!(true));
                        }
                        d.fields.insert("layers".into(), json!(layers));
                    }
                    "colorSettings" => {
                        let profiles = d.fields.get("profiles").and_then(Value::as_array).cloned().unwrap_or_default();
                        egui::Grid::new("color_settings").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            for (key, label, kind) in [("rgb", "RGB:", "rgb"), ("cmyk", "CMYK:", "cmyk")] {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                                let cur = d.s(key);
                                craft_ui::ComboBox::from_id_salt(("ws", key)).selected_text(&cur).width(260.0).show_ui(ui, |ui| {
                                    for pr in profiles.iter().filter(|p| p["kind"] == kind) {
                                        let name = pr["name"].as_str().unwrap_or("");
                                        if ui.selectable_label(name == cur, name).clicked() {
                                            d.fields.insert(key.into(), json!(name));
                                        }
                                    }
                                });
                                ui.end_row();
                            }
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Intent:"));
                            let cur = d.s("intent");
                            craft_ui::ComboBox::from_id_salt("ws_intent")
                                .selected_text(crate::rtl::widget(
                                    ui,
                                    crate::i18n::tr(&app.ui.language, designcraft_color::cms::Intent::parse(&cur).map_or("", |i| i.label())),
                                ))
                                .show_ui(ui, |ui| {
                                    for i in designcraft_color::cms::Intent::ALL {
                                        if ui
                                            .selectable_label(i.id() == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, i.label())))
                                            .clicked()
                                        {
                                            d.fields.insert("intent".into(), json!(i.id()));
                                        }
                                    }
                                });
                            ui.end_row();
                        });
                        check(ui, &mut d, "bpc", crate::i18n::tr(&app.ui.language, "Use Black Point Compensation"));
                        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Load Profile…"))).clicked()
                            && let Err(e) = app.ask_open("icc", Bound::App, load_color_profile)
                        {
                            app.status_error("Color Settings", e);
                        }
                    }
                    "userDictionary" => user_dictionary(app, ui, &mut d),
                    "menus" => {
                        text_field(ui, &mut d, "query", 260.0);
                        let q = d.s("query").to_lowercase();
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (menu, label) in crate::menus::all_menu_items()
                                .into_iter()
                                .filter(|(m, l)| q.is_empty() || m.to_lowercase().contains(&q) || l.to_lowercase().contains(&q))
                            {
                                let key = crate::menus::menu_key(&menu, &label);
                                let mut visible = !app.ui.hidden_menu_items.contains(&key);
                                if ui
                                    .checkbox(
                                        &mut visible,
                                        crate::rtl::widget(
                                            ui,
                                            format!("{} › {}", crate::i18n::tr(&app.ui.language, &menu), crate::i18n::tr(&app.ui.language, &label)),
                                        ),
                                    )
                                    .changed()
                                {
                                    let _ = app.run("window.hideMenuItem", json!({"item": key, "hidden": !visible}));
                                }
                            }
                        });
                    }
                    "importOptions" => {
                        ui.label(egui::RichText::new(d.s("path")).size(10.5));
                        check(ui, &mut d, "removeStyles", crate::i18n::tr(&app.ui.language, "Remove Styles and Formatting from Text and Tables"));
                        if !d.b("removeStyles") {
                            crate::rtl::label(
                                ui,
                                egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Name Conflicts")).font(semibold(12.0)),
                            );
                            let conflicts: Vec<String> = d
                                .fields
                                .get("conflicts")
                                .and_then(Value::as_array)
                                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                                .unwrap_or_default();
                            ui.label(if conflicts.is_empty() {
                                crate::i18n::tr(&app.ui.language, "No conflicts").to_string()
                            } else {
                                crate::i18n::tr(&app.ui.language, "{count} conflict(s): {names}")
                                    .replace("{count}", &conflicts.len().to_string())
                                    .replace("{names}", &conflicts.join(", "))
                            });
                            let cur = d.s("styleConflicts");
                            ui.horizontal(|ui| {
                                for (v, l) in [
                                    ("useExisting", "Use Document Style Definition"),
                                    ("redefine", "Redefine Document Style"),
                                    ("autoRename", "Auto Rename"),
                                ] {
                                    if ui.radio(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                                        d.fields.insert("styleConflicts".into(), json!(v));
                                    }
                                }
                            });
                            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Mapping")).font(semibold(12.0)));
                            let names: Vec<String> =
                                app.session.active().map(|s| s.doc.styles.paragraph.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
                            let imported: Vec<String> = d
                                .fields
                                .get("styles")
                                .and_then(Value::as_array)
                                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                                .unwrap_or_default();
                            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                egui::Grid::new("imp_map").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                    for w in &imported {
                                        ui.label(w);
                                        let mapped = d.fields.get("map").and_then(|m| m.get(w)).and_then(Value::as_str).unwrap_or("").to_string();
                                        let shown = if mapped.is_empty() {
                                            crate::i18n::tr(&app.ui.language, "(import)").to_string()
                                        } else {
                                            mapped.clone()
                                        };
                                        craft_ui::ComboBox::from_id_salt(("imp_map", w))
                                            .selected_text(crate::rtl::widget(ui, &shown))
                                            .width(170.0)
                                            .show_ui(ui, |ui| {
                                                if ui
                                                    .selectable_label(
                                                        mapped.is_empty(),
                                                        crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "(import)")),
                                                    )
                                                    .clicked()
                                                    && let Some(m) = d.fields.get_mut("map").and_then(Value::as_object_mut)
                                                {
                                                    m.remove(w);
                                                }
                                                for n in &names {
                                                    if ui.selectable_label(*n == mapped, n).clicked()
                                                        && let Some(m) = d.fields.get_mut("map").and_then(Value::as_object_mut)
                                                    {
                                                        m.insert(w.clone(), json!(n));
                                                    }
                                                }
                                            });
                                        ui.end_row();
                                    }
                                });
                            });
                        }
                    }
                    "loadStyles" => load_styles(app, ui, &mut d),
                    "closeDocument" => {
                        let text = crate::i18n::tr(&app.ui.language, "“{}” has changes that are not saved. Save them before closing?")
                            .replace("{}", &d.s("title"));
                        crate::rtl::label(ui, text);
                    }
                    "deletePages" => {
                        let several = d.fields.get("pages").and_then(Value::as_array).is_some_and(|p| p.len() > 1);
                        crate::rtl::label(
                            ui,
                            crate::i18n::tr(
                                &app.ui.language,
                                if several {
                                    "The affected pages contain objects. Delete the pages anyway?"
                                } else {
                                    "The affected page contains objects. Delete the page anyway?"
                                },
                            ),
                        );
                        ui.add_space(8.0);
                        check(ui, &mut d, "dontShowAgain", crate::i18n::tr(&app.ui.language, "Don't show again"));
                    }
                    "pagesPanelOptions" => {
                        // As InDesign's: Pages, Parents, Panel Layout.
                        use crate::panels::pages::{IconSize, PanelResize};
                        let lang = app.ui.language.clone();
                        let sizes: Vec<(&str, &str)> = IconSize::ALL.iter().map(|s| (s.key(), crate::i18n::tr(&lang, s.label()))).collect();
                        let group = |ui: &mut egui::Ui, title: &str| {
                            ui.add_space(4.0);
                            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, title)).strong());
                        };
                        group(ui, "Pages");
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&lang, "Size:"));
                            combo(ui, &mut d, "pageSize", &sizes);
                        });
                        // Extra Small pages have no room for a thumbnail.
                        ui.add_enabled_ui(d.s("pageSize") != "extraSmall", |ui| {
                            check(ui, &mut d, "pageThumbnails", crate::i18n::tr(&lang, "Show Thumbnails"))
                        });
                        group(ui, "Parents");
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&lang, "Size:"));
                            combo(ui, &mut d, "parentSize", &sizes);
                        });
                        ui.add_enabled_ui(d.s("parentSize") != "extraSmall", |ui| {
                            check(ui, &mut d, "parentThumbnails", crate::i18n::tr(&lang, "Show Thumbnails"))
                        });
                        check(ui, &mut d, "parentsVertical", crate::i18n::tr(&lang, "Show Vertically"));
                        group(ui, "Panel Layout");
                        for (v, label) in [("pagesOnTop", "Pages on Top"), ("parentsOnTop", "Parents on Top")] {
                            if ui.radio(d.s("layout") == v, crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).clicked() {
                                d.fields.insert("layout".into(), json!(v));
                            }
                        }
                        let resizes: Vec<(&str, &str)> = PanelResize::ALL.iter().map(|r| (r.key(), crate::i18n::tr(&lang, r.label()))).collect();
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&lang, "Resize:"));
                            combo(ui, &mut d, "resize", &resizes);
                        });
                    }
                    "updateLink" => {
                        let text = crate::i18n::tr(
                            &app.ui.language,
                            "“{}” has been edited in this layout. Updating replaces those edits with the text from the file.",
                        )
                        .replace("{}", &d.s("files"));
                        crate::rtl::label(ui, text);
                    }
                    "newWorkspace" => {
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name:"));
                            text_field(ui, &mut d, "name", 220.0);
                        });
                        ui.add_space(6.0);
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Capture")).font(semibold(12.0)));
                        check(ui, &mut d, "panelLocations", crate::i18n::tr(&app.ui.language, "Panel Locations"));
                        check(ui, &mut d, "menuCustomization", crate::i18n::tr(&app.ui.language, "Menu Customization"));
                    }
                    "fittingOptions" => {
                        check(ui, &mut d, "autoFit", crate::i18n::tr(&app.ui.language, "Auto-Fit"));
                        ui.add_space(4.0);
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Content Fitting")).font(semibold(12.0)));
                        let cur = d.s("fitting");
                        let label = |v: &str| match v {
                            "fillProportionally" => crate::i18n::tr(&app.ui.language, "Fill Frame Proportionally"),
                            "fitProportionally" => crate::i18n::tr(&app.ui.language, "Fit Content Proportionally"),
                            "fitContentToFrame" => crate::i18n::tr(&app.ui.language, "Fit Content to Frame"),
                            "centerContent" => crate::i18n::tr(&app.ui.language, "Center Content"),
                            _ => crate::i18n::tr(&app.ui.language, "None"),
                        };
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Fitting:"));
                            craft_ui::ComboBox::from_id_salt("ff_fit").selected_text(crate::rtl::widget(ui, label(&cur))).width(200.0).show_ui(
                                ui,
                                |ui| {
                                    for v in ["none", "fillProportionally", "fitProportionally", "fitContentToFrame", "centerContent"] {
                                        if ui.selectable_label(cur == v, crate::rtl::widget(ui, label(v))).clicked() {
                                            d.fields.insert("fitting".into(), json!(v));
                                        }
                                    }
                                },
                            );
                        });
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Align From:"));
                            let a = d.n("align").unwrap_or(4.0) as u8;
                            egui::Grid::new("ff_align").spacing([2.0, 2.0]).show(ui, |ui| {
                                for row in 0..3u8 {
                                    for col in 0..3u8 {
                                        let i = row * 3 + col;
                                        let (r, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                                        let c = ui.visuals().text_color();
                                        if a == i {
                                            ui.painter().rect_filled(r.shrink(2.0), 1.0, c);
                                        } else {
                                            ui.painter().rect_stroke(r.shrink(3.0), 1.0, egui::Stroke::new(1.0, c), egui::StrokeKind::Inside);
                                        }
                                        if resp.clicked() {
                                            d.fields.insert("align".into(), json!(i));
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Crop Amount")).font(semibold(12.0)));
                        egui::Grid::new("ff_crop").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                            for (k, l) in [("cropTop", "Top:"), ("cropLeft", "Left:")] {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
                                text_field(ui, &mut d, k, 70.0);
                            }
                            ui.end_row();
                            for (k, l) in [("cropBottom", "Bottom:"), ("cropRight", "Right:")] {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
                                text_field(ui, &mut d, k, 70.0);
                            }
                            ui.end_row();
                        });
                    }
                    "qrCode" => {
                        let cur = d.s("type");
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Type:"));
                            let label = |v: &str| match v {
                                "text" => crate::i18n::tr(&app.ui.language, "Plain Text"),
                                "sms" => crate::i18n::tr(&app.ui.language, "Text Message"),
                                "email" => crate::i18n::tr(&app.ui.language, "Email"),
                                "vcard" => crate::i18n::tr(&app.ui.language, "Business Card"),
                                _ => crate::i18n::tr(&app.ui.language, "Web Hyperlink"),
                            };
                            craft_ui::ComboBox::from_id_salt("qr_type").selected_text(crate::rtl::widget(ui, label(&cur))).width(160.0).show_ui(
                                ui,
                                |ui| {
                                    for v in ["url", "text", "sms", "email", "vcard"] {
                                        if ui.selectable_label(cur == v, crate::rtl::widget(ui, label(v))).clicked() {
                                            d.fields.insert("type".into(), json!(v));
                                        }
                                    }
                                },
                            );
                        });
                        let fields: &[(&str, &str)] = match cur.as_str() {
                            "sms" => &[("number", "Cell Number:"), ("message", "Message:")],
                            "email" => &[("to", "Email Address:"), ("subject", "Subject:"), ("body", "Message:")],
                            "vcard" => &[("name", "Name:"), ("org", "Organization:"), ("phone", "Phone:"), ("email", "Email:"), ("url", "URL:")],
                            "text" => &[("content", "Text:")],
                            _ => &[("content", "URL:")],
                        };
                        egui::Grid::new("qr_fields").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            for (k, l) in fields {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
                                text_field(ui, &mut d, k, 260.0);
                                ui.end_row();
                            }
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Color:"));
                            text_field(ui, &mut d, "color", 160.0);
                            ui.end_row();
                        });
                    }
                    "polygonSettings" => {
                        egui::Grid::new("poly").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Number of Sides:"));
                            text_field(ui, &mut d, "sides", 60.0);
                            ui.end_row();
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Star Inset:"));
                            ui.horizontal(|ui| {
                                text_field(ui, &mut d, "starInset", 60.0);
                                ui.label("%");
                            });
                            ui.end_row();
                        });
                    }
                    STEP_AND_REPEAT => step_and_repeat_dialog(app, ui, &mut d),
                    CORNER_OPTIONS => corner_options_dialog(app, ui, &mut d),
                    TOC => toc_dialog(app, ui, &mut d),
                    id if id.starts_with("cmd:") => command_form(app, ui, &mut d),
                    "insertPages" => insert_pages_dialog(app, ui, &mut d),
                    "goToPage" => {
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page"));
                            text_field(ui, &mut d, "page", 80.0);
                        });
                    }
                    "ruby" => style_cjk::ruby_dialog(app, ui, &mut d),
                    "insertTable" => insert_table_dialog(app, ui, &mut d),
                    "convertTextToTable" | "convertTableToText" => convert_table_dialog(app, ui, &mut d),
                    "cellOptions" => cell_options(app, ui, &mut d),
                    "createGuides" => create_guides_dialog(app, ui, &mut d),
                    "marginsAndColumns" => margins_and_columns_dialog(app, ui, &mut d),
                    "textFrameOptions" => {
                        seed_text_frame_options(app, &mut d);
                        text_frame_options_dialog(app, ui, &mut d);
                        text_frame_preview(app, &mut d);
                    }
                    "colorTheme" => color_theme(app, ui, &mut d),
                    "documentSetup" => {
                        // Adjust Layout applies at once.
                        let adjust = document_setup(app, ui, &mut d);
                        if adjust {
                            result = Some(true);
                        }
                    }
                    "fileInfo" => file_info_dialog(app, ui, &mut d),
                    "numberingSection" => crate::section_options::body(app, ui, &mut d),
                    "pdfExport" => pdf_export(app, ui, &mut d),
                    "epubExport" => epub_export(app, ui, &mut d),
                    "alert" => {
                        let file = d.s("file");
                        if !file.is_empty() {
                            crate::rtl::label(ui, egui::RichText::new(crate::rtl::isolate(&file)).font(semibold(13.0)));
                            ui.add_space(4.0);
                        }
                        alert_message(app, ui, &mut d);
                    }
                    _ => {}
                }
            });
        if d.id == "colorPicker" {
            match d.fields.remove("_close").as_ref().and_then(Value::as_str) {
                Some("ok") => result = Some(true),
                Some("cancel") => result = Some(false),
                _ => {}
            }
            return;
        }
        if side_actions {
            return;
        }
        ui.add_space(12.0);
        let ok_label = match d.id.as_str() {
            "colorTheme" => crate::i18n::tr(&app.ui.language, "Add to Swatches").to_string(),
            "closeDocument" => crate::i18n::tr(&app.ui.language, "  Save  ").to_string(),
            "updateLink" => crate::i18n::tr(&app.ui.language, "  Update  ").to_string(),
            "findFont" => crate::i18n::tr(&app.ui.language, "Replace All").to_string(),
            "newDocument" => format!("  {}  ", crate::i18n::tr(&app.ui.language, "Create")),
            crate::package_dialog::ID => crate::i18n::tr(&app.ui.language, "Package…").to_string(),
            crate::package_dialog::INSTRUCTIONS => crate::i18n::tr(&app.ui.language, "Continue").to_string(),
            _ => crate::i18n::tr(&app.ui.language, "  OK  ").to_string(),
        };
        let cancel_label = match d.id.as_str() {
            "newDocument" => "Close",
            "findFont" => "Done",
            _ => "Cancel",
        };
        ui.horizontal(|ui| {
            if matches!(d.id.as_str(), "paragraphStyleOptions" | "characterStyleOptions") && !d.b("new") {
                check(ui, &mut d, "preview", crate::i18n::tr(&app.ui.language, "Preview"));
                app.ui.style_preview = d.b("preview");
                style_options_preview(app, &mut d);
            }
            // A multi-line field with focus (set by the dialog this frame) takes Enter as a new line.
            let multiline = take_multiline_focus(ui.ctx(), &d.id);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(ok_label).color(egui::Color32::WHITE)))
                            .fill(crate::theme::Tokens::get(ui.ctx()).accent_strong),
                    )
                    .clicked()
                    || (!popup_open && !multiline && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    result = Some(true);
                }
                // An alert only has OK.
                let cancel = d.id != "alert" && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, cancel_label))).clicked();
                if cancel || (!popup_open && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
                    result = Some(false);
                }
                if d.id == "newDocument" {
                    ui.add_space(8.0);
                    check(ui, &mut d, "preview", crate::i18n::tr(&app.ui.language, "Preview"));
                    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
                        ui.add_space(8.0);
                        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
                    }
                }
                if d.id == "closeDocument" && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Don't Save"))).clicked() {
                    d.fields.insert("discard".into(), json!(true));
                    result = Some(true);
                }
            });
        });
    });
    // Show on an import warning: close the warnings (the status line keeps them) and select the
    // frame the warning is about, in view, so it can be fixed.
    if let Some(item) = d.fields.remove("show").and_then(|v| v.as_u64()) {
        app.ui.dialog = None;
        if app.run("selection.set", json!({"ids": [item]})).is_ok() {
            let _ = app.run("view.fitSelection", json!({}));
        }
        return;
    }
    // Preferences › General › Interface Language takes effect at once, like any other app setting.
    if let Some(lang) = d.fields.remove("applyLanguage")
        && let Err(e) = app.run("app.language", json!({"lang": lang}))
    {
        d.fields.insert("status".into(), json!(e));
    }
    // So does Preferences › Interface › Color Theme, as InDesign's.
    if let Some(b) = d.fields.remove("applyBrightness")
        && let Err(e) = app.run("window.brightness", json!({"brightness": b}))
    {
        d.fields.insert("status".into(), json!(e));
    }
    if d.id == "newDocument" && result.is_none() {
        crate::new_document::preview(app, &mut d, ctx);
    }
    if d.id == "findChange" && result.is_none() && d.fields.get(FORMAT_EDIT).is_some_and(Value::is_object) {
        format_settings(app, ctx, &mut d);
    }
    app.ui.dialog = Some(d);
    match result {
        Some(true) => {
            let _ = confirm(app);
        }
        Some(false) => cancel(app),
        None => {}
    }
}

/// Cell Options keys (`table.getCellStroke` names) and whether each belongs to the stroke.
const CELL_OPTION_KEYS: [(&str, bool); 8] =
    [("weight", true), ("type", true), ("color", true), ("tint", true), ("gapColor", true), ("gapTint", true), ("fill", false), ("fillTint", false)];

/// Table › Cell Options › Strokes and Fills… for the target cells. The fields start from the
/// cells' values (`seed`); OK applies the fields that differ from it.
pub fn open_cell_options(app: &mut DesignApp) -> Result<Value, String> {
    let edges = crate::panels::cell_stroke::edges_param(&[true; 6]);
    let cur = app.session.execute("table.getCellStroke", &json!({"edges": edges})).map_err(|e| e.to_string())?;
    let mut fields = Map::new();
    for (k, _) in CELL_OPTION_KEYS {
        fields.insert(k.into(), cur[k].clone());
    }
    fields.insert("seed".into(), Value::Object(fields.clone()));
    fields.insert("edges".into(), edges);
    app.ui.dialog = Some(Dialog::new("cellOptions", Value::Object(fields)));
    Ok(Value::Null)
}

/// Cell Options › Strokes and Fills: the edge proxy and stroke fields, then the cell fill.
fn cell_options(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    use crate::panels::cell_stroke;
    let lang = app.ui.language.clone();
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Cell Stroke")).font(semibold(12.0)));
    let mut edges = cell_stroke::edges_from(d.fields.get("edges").unwrap_or(&Value::Null));
    if cell_stroke::edge_proxy(ui, "cell_options", &mut edges, &lang) {
        cell_options_edges_changed(app, d, &edges);
    }
    ui.add_space(6.0);
    let values = Value::Object(d.fields.clone());
    let any = edges.iter().any(|on| *on);
    if let Some((k, v)) = ui.add_enabled_ui(any, |ui| cell_stroke::stroke_fields(app, ui, "cell_options", &values)).inner {
        d.fields.insert(k.into(), v);
    }
    ui.add_space(10.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Cell Fill")).font(semibold(12.0)));
    egui::Grid::new("cell_fill").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&lang, "Color:"));
        let mut picked = None;
        crate::panels::swatch_picker(app, ui, "cell_fill_color", values["fill"].as_str().map(str::to_string), |_, n| picked = Some(n));
        if let Some(n) = picked {
            d.fields.insert("fill".into(), json!(n));
        }
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&lang, "Tint:"));
        let tint = values["fillTint"].as_f64().map(|t| (t * 100.0).round());
        if let Some(t) = crate::widgets::NumField::number("cell_fill_tint", tint, "%", 0).width(60.0).range(0.0, 100.0).show(ui) {
            d.fields.insert("fillTint".into(), json!(t / 100.0));
        }
        ui.end_row();
    });
}

/// Cell Options with a new edge choice: the stroke fields the user hasn't changed show the
/// chosen edges' values, and every stroke field's seed becomes those values, so OK applies
/// exactly the fields that differ from what the chosen edges show.
fn cell_options_edges_changed(app: &mut DesignApp, d: &mut Dialog, edges: &crate::panels::cell_stroke::Edges) {
    d.fields.insert("edges".into(), crate::panels::cell_stroke::edges_param(edges));
    let cur = crate::panels::cell_stroke::current(app, edges);
    let mut seed = d.fields.get("seed").and_then(Value::as_object).cloned().unwrap_or_default();
    for (k, stroke) in CELL_OPTION_KEYS {
        if !stroke {
            continue;
        }
        let v = cur.get(k).cloned().unwrap_or(Value::Null);
        // Null shows mixed values; the user can't type it.
        let field = d.fields.get(k).filter(|f| !f.is_null());
        if field.is_none() || field == seed.get(k) {
            d.fields.insert(k.into(), v.clone());
        }
        seed.insert(k.into(), v);
    }
    d.fields.insert("seed".into(), Value::Object(seed));
}

/// Apply the Cell Options fields that differ from the cells' values, as one `table.setCell`.
fn confirm_cell_options(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let seed = d.fields.get("seed").cloned().unwrap_or(Value::Null);
    let mut stroke = Map::new();
    let mut params = Map::new();
    for (k, is_stroke) in CELL_OPTION_KEYS {
        let Some(v) = d.fields.get(k).filter(|v| !v.is_null() && seed.get(k) != Some(*v)) else { continue };
        match (is_stroke, k) {
            (true, _) => {
                stroke.insert(k.into(), v.clone());
            }
            (false, "fillTint") => {
                params.insert("tint".into(), v.clone());
            }
            _ => {
                params.insert(k.into(), v.clone());
            }
        }
    }
    if !stroke.is_empty() {
        stroke.insert("edges".into(), d.fields.get("edges").cloned().unwrap_or(json!("all")));
        params.insert("stroke".into(), Value::Object(stroke));
    }
    if params.is_empty() {
        return Ok(Value::Null);
    }
    app.run("table.setCell", Value::Object(params))
}

/// Delete absolute pages `pages` as the Pages panel and Layout › Pages › Delete Page do: when
/// they hold objects, InDesign's warning asks first, unless it was told not to.
pub fn delete_pages(app: &mut DesignApp, pages: Vec<usize>) -> Result<Value, String> {
    let has_items = app.session.active().is_some_and(|st| pages.iter().any(|&abs| st.doc.page_has_items(abs)));
    if has_items && !app.ui.skip_delete_pages_warning {
        app.ui.dialog = Some(Dialog::new("deletePages", json!({"pages": pages})));
        return Ok(Value::Null);
    }
    app.run("layout.pages.delete", json!({"pages": pages}))
}

/// Close the open dialog without applying it (Cancel, Esc, the control channel): a previewing
/// dialog puts the document back as it was, and Preferences puts back the interface language it
/// opened with, since choosing one previews it at once.
pub fn cancel(app: &mut DesignApp) {
    if app.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange") {
        app.session.reset_find_scope();
    }
    let d = app.ui.dialog.take();
    if d.as_ref().is_some_and(|d| d.id == crate::shortcuts_dialog::DIALOG_ID) {
        let _ = crate::shortcuts_dialog::close(app, false);
    }
    crate::new_document::end_preview(app);
    if d.as_ref().is_some_and(|d| d.fields.contains_key(PREVIEWED)) {
        app.session.cancel_interaction();
    }
    app.cancel_pdf_import();
    if let Some(d) = d
        && d.id == "preferences"
    {
        if let Some(lang) = d.fields.get("openedLanguage").and_then(Value::as_str)
            && lang != app.ui.language
        {
            let _ = app.run("app.language", json!({"lang": lang}));
        }
        // The Color Theme shown while choosing goes back too.
        if let Some(b) = d.fields.get("openedBrightness").and_then(Value::as_str)
            && b != app.ui.brightness.id()
        {
            let _ = app.run("window.brightness", json!({"brightness": b}));
        }
    }
}

fn find_change_params(d: &Dialog) -> Value {
    let mut p = json!({
        "find": d.s("find"), "change": d.s("change"), "grep": d.b("grep"),
        "caseSensitive": d.b("caseSensitive"), "wholeWord": d.b("wholeWord"), "scope": d.s("scope"),
    });
    if let Some(story) = d.fields.get("story") {
        p["story"] = story.clone();
    }
    // Find Format: only text with it is found (with nothing in Find what, all text with it).
    if let Some(f) = d.fields.get(FIND_FORMAT).and_then(Value::as_object).filter(|f| !f.is_empty()) {
        p["findFormat"] = Value::Object(f.clone());
    }
    // Change Format: find.change applies it to the changed text (the other commands ignore it).
    if let Some(f) = d.fields.get(CHANGE_FORMAT) {
        for key in ["charStyle", "paraStyle"] {
            if let Some(v) = f.get(key).filter(|v| v.is_string()) {
                p[key] = v.clone();
            }
        }
        if let Some(chars) = f.get("chars").filter(|c| c.as_object().is_some_and(|o| !o.is_empty())) {
            p["attrs"] = chars.clone();
        }
    }
    p
}

/// Find/Change's Find Format and Change Format ({charStyle?, paraStyle?, chars?}), as their
/// settings set them; absent when there is none.
const FIND_FORMAT: &str = "findFormat";
const CHANGE_FORMAT: &str = "changeFormat";
/// The fields of Find or Change Format Settings while it is open (`for`: find|change, its
/// section, `charStyle`, `paraStyle` and `c.<attr>`), until OK makes them the format.
const FORMAT_EDIT: &str = "formatEdit";

/// Whether the dialog has a Find (`FIND_FORMAT`) or Change Format.
fn format_set(d: &Dialog, key: &str) -> bool {
    d.fields.get(key).and_then(Value::as_object).is_some_and(|f| !f.is_empty())
}

/// The sections of Find and Change Format Settings: Style Options, then those of Character Style
/// Options.
const FORMAT_SECTIONS: &[(&str, &str)] = &[
    ("style", "Style Options"),
    ("chars", "Basic Character Formats"),
    ("advanced", "Advanced Character Formats"),
    ("color", "Character Color"),
    ("openType", "OpenType Features"),
    ("underline", "Underline Options"),
    ("strikethrough", "Strikethrough Options"),
];

/// A small ⓘ whose tooltip is `tip`.
fn info_icon(ui: &mut egui::Ui, tip: &str) {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    crate::icons::paint(ui.painter(), r, "info", crate::theme::Tokens::get(ui.ctx()).icon);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tip));
    resp.on_hover_text(tip);
}

/// What a Find or Change Format box shows: "Character Style: eggs + …".
fn format_summary(lang: &str, format: Option<&Value>) -> String {
    let Some(f) = format else { return String::new() };
    let mut parts = Vec::new();
    for (key, label) in [("charStyle", "Character Style:"), ("paraStyle", "Paragraph Style:")] {
        if let Some(name) = f.get(key).and_then(Value::as_str) {
            parts.push(format!("{} {}", crate::i18n::tr(lang, label), crate::i18n::style_name(lang, name)));
        }
    }
    parts.extend(
        f.get("chars").and_then(Value::as_object).into_iter().flatten().map(|(k, v)| format!("{}: {}", setting_label(lang, k), setting_text(v))),
    );
    parts.join(" + ")
}

/// Find/Change's Find Format (`find`) or Change Format, as InDesign's: a box with what the found
/// text must have or what Change All also gives the changed text, a button that opens its
/// settings and one that clears it.
fn format_box(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, find: bool) {
    let (key, label, specify) = if find {
        (FIND_FORMAT, "Find Format:", "Specify attributes to find")
    } else {
        (CHANGE_FORMAT, "Change Format:", "Specify attributes to change")
    };
    ui.add_space(6.0);
    crate::rtl::label(ui, crate::i18n::tr(lang, label));
    ui.horizontal_top(|ui| {
        let t = crate::theme::Tokens::get(ui.ctx());
        egui::Frame::new().fill(t.input).stroke(egui::Stroke::new(1.0, t.input_border)).inner_margin(6.0).show(ui, |ui| {
            ui.set_width(340.0);
            ui.set_min_height(40.0);
            let summary = format_summary(lang, d.fields.get(key));
            ui.add(egui::Label::new(egui::RichText::new(summary).color(t.text)).wrap());
        });
        ui.vertical(|ui| {
            if crate::icons::button(ui, "find-format", 22.0, false, crate::i18n::tr(lang, specify)).clicked() {
                let f = d.fields.get(key).cloned().unwrap_or_default();
                let mut edit = Map::new();
                edit.insert("for".into(), json!(if find { "find" } else { "change" }));
                edit.insert("section".into(), json!("style"));
                for key in ["charStyle", "paraStyle"] {
                    if let Some(v) = f.get(key) {
                        edit.insert(key.into(), v.clone());
                    }
                }
                for (k, v) in f.get("chars").and_then(Value::as_object).into_iter().flatten() {
                    edit.insert(format!("c.{k}"), v.clone());
                }
                d.fields.insert(FORMAT_EDIT.into(), Value::Object(edit));
            }
            if crate::icons::button(ui, "trash", 22.0, false, crate::i18n::tr(lang, "Clear specified attributes")).clicked() {
                d.fields.remove(key);
            }
        });
    });
}

/// Find/Change › Find or Change Format Settings, over Find/Change as InDesign's: Style Options
/// (the character and paragraph style the found text has or the changed text gets, [Any Style]
/// for none), then the character sections of Character Style Options with every field blank (any
/// value, or left as it is). OK makes it the Find or Change Format.
fn format_settings(app: &mut DesignApp, ctx: &egui::Context, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let Some(doc) = app.session.active().map(|st| st.doc.clone()) else {
        d.fields.remove(FORMAT_EDIT);
        return;
    };
    let mut fd = Dialog {
        id: "changeFormat".into(),
        fields: d.fields.get(FORMAT_EDIT).and_then(Value::as_object).cloned().unwrap_or_default(),
        unit: d.unit,
        vertical_unit: None,
    };
    let base = json!({});
    let cf = CharFields { base: &base, sparse: true };
    let find = fd.s("for") == "find";
    let popup_open = egui::Popup::is_any_open(ctx);
    let mut result = None;
    let host = egui::Id::unique("format_settings");
    crate::modal_drag::modal(ctx, host, host).backdrop_color(egui::Color32::TRANSPARENT).show(ctx, |ui| {
        let title_top = ui.cursor().top();
        let title = if find { "Find Format Settings" } else { "Change Format Settings" };
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, title)).font(semibold(16.0)));
        let margin = egui::Frame::popup(ui.style()).inner_margin;
        let title_row = egui::Rect::from_min_max(
            egui::pos2(ui.max_rect().left() - margin.leftf(), title_top - margin.topf()),
            egui::pos2(ui.max_rect().right() + margin.rightf(), ui.min_rect().bottom()),
        );
        crate::modal_drag::title_drag(ui, host, title_row);
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            // A fixed height: the separator would otherwise stretch the dialog to the window.
            ui.set_min_height(380.0);
            ui.set_max_height(380.0);
            section_list(&lang, ui, &mut fd, false, FORMAT_SECTIONS);
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(440.0);
                match fd.s("section").as_str() {
                    "chars" => basic_character_formats(app, ui, &mut fd, &cf),
                    "advanced" => advanced_character_formats(&lang, ui, &mut fd, &cf),
                    "color" => character_color(&lang, ui, &mut fd, &cf, &doc),
                    "openType" => open_type_features(&lang, ui, &mut fd, &cf),
                    "underline" => line_options(&lang, ui, &mut fd, &cf, &doc, "underline"),
                    "strikethrough" => line_options(&lang, ui, &mut fd, &cf, &doc, "strikethrough"),
                    _ => format_style_options(&lang, ui, &mut fd, &doc.styles),
                }
            });
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&lang, "  OK  ")).color(egui::Color32::WHITE)))
                        .fill(crate::theme::Tokens::get(ui.ctx()).accent_strong),
                )
                .clicked()
                || (!popup_open && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                result = Some(true);
            }
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Cancel"))).clicked()
                || (!popup_open && ui.input(|i| i.key_pressed(egui::Key::Escape)))
            {
                result = Some(false);
            }
        });
    });
    match result {
        Some(true) => {
            d.fields.remove(FORMAT_EDIT);
            let mut f = Map::new();
            for key in ["charStyle", "paraStyle"] {
                if let Some(v) = fd.fields.get(key).filter(|v| v.is_string()) {
                    f.insert(key.into(), v.clone());
                }
            }
            let chars: Map<String, Value> =
                fd.fields.iter().filter(|(_, v)| !v.is_null()).filter_map(|(k, v)| Some((k.strip_prefix("c.")?.to_string(), v.clone()))).collect();
            if !chars.is_empty() {
                f.insert("chars".into(), Value::Object(chars));
            }
            let key = if find { FIND_FORMAT } else { CHANGE_FORMAT };
            if f.is_empty() {
                d.fields.remove(key);
            } else {
                d.fields.insert(key.into(), Value::Object(f));
            }
        }
        Some(false) => {
            d.fields.remove(FORMAT_EDIT);
        }
        None => {
            d.fields.insert(FORMAT_EDIT.into(), Value::Object(fd.fields));
        }
    }
}

/// Find or Change Format Settings › Style Options: the character and paragraph style the found
/// text has or the changed text gets ([Any Style]: any, or as it is).
fn format_style_options(lang: &str, ui: &mut egui::Ui, fd: &mut Dialog, styles: &designcraft_doc::Styles) {
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Style Options")).font(semibold(13.0)));
    ui.add_space(6.0);
    let any = crate::i18n::tr(lang, "[Any Style]");
    let chars: Vec<&str> = styles.character.iter().map(|s| s.name.as_str()).collect();
    let paras: Vec<&str> = styles.paragraph.iter().map(|s| s.name.as_str()).collect();
    egui::Grid::new("format_style_options").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (key, label, names) in [("charStyle", "Character Style:", &chars), ("paraStyle", "Paragraph Style:", &paras)] {
            crate::rtl::label(ui, crate::i18n::tr(lang, label));
            let cur = fd.fields.get(key).and_then(Value::as_str).map(str::to_string);
            let shown = cur.as_deref().map_or_else(|| any.to_string(), |n| crate::i18n::style_name(lang, n).to_string());
            craft_ui::ComboBox::from_id_salt(("format_style", key)).selected_text(crate::rtl::widget(ui, shown)).width(240.0).show_ui(ui, |ui| {
                if ui.selectable_label(cur.is_none(), crate::rtl::widget(ui, any)).clicked() {
                    fd.fields.remove(key);
                }
                for n in names.iter() {
                    if ui.selectable_label(cur.as_deref() == Some(*n), crate::rtl::widget(ui, crate::i18n::style_name(lang, n))).clicked() {
                        fd.fields.insert(key.into(), json!(n));
                    }
                }
            });
            ui.end_row();
        }
    });
}

/// Apply the open dialog.
pub fn confirm(app: &mut DesignApp) -> Result<Value, String> {
    let Some(d) = app.ui.dialog.take() else { return Err("no dialog open".into()) };
    if d.id == crate::shortcuts_dialog::DIALOG_ID {
        return crate::shortcuts_dialog::close(app, true).map(|()| Value::Null);
    }
    if matches!(d.id.as_str(), "paragraphStyleOptions" | "characterStyleOptions") && d.fields.contains_key(PREVIEWED) {
        // Restore the original before the ordinary command commits one undoable edit.
        app.session.cancel_interaction();
    }
    if d.id != "pdfImport" || !d.b("async") {
        app.cancel_pdf_import();
    }
    confirm_part1(app, d)
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part1(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "colorTheme" => {
            let result = app.run("swatch.addColorTheme", json!({"name": d.s("name"), "colors": d.fields.get("colors")}));
            if let Err(error) = &result {
                let mut d = d;
                d.fields.insert("status".into(), json!(error));
                app.ui.dialog = Some(d);
            }
            result
        }
        "createGuides" | "marginsAndColumns" => {
            // The preview goes; the change is made once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let cmd = if d.id == "createGuides" { "layout.createGuides" } else { "layout.marginsAndColumns" };
            let r = layout_dialog_params(&d).and_then(|p| app.run(cmd, p));
            if let Err(e) = &r {
                // A value that does not parse, or a refused one, keeps the dialog open to be fixed.
                let mut d = d.clone();
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        "newDocument" => crate::new_document::create(app, d),
        "frameSize" => {
            let x = d.fields.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let y = d.fields.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            let w = d.m("width").unwrap_or(72.0);
            // In the units the dialog showed, not ones changed while it was open.
            let h = d.m_in("height", d.vertical_unit.unwrap_or(d.unit)).unwrap_or(72.0);
            app.run(
                "frame.create",
                json!({"spread": d.fields.get("spread").cloned().unwrap_or(json!(0)), "shape": d.s("shape"), "content": d.s("content"), "rect": [x, y, x + w, y + h]}),
            )
        }
        "ruby" => app.run("type.ruby", style_cjk::ruby_params(&d)),
        "insertPages" => {
            let n = app.session.active().map_or(1, |st| st.doc.page_count());
            // The page the menu shows: a page past the end is the last.
            let page = (d.n("page").unwrap_or(0.0).max(0.0) as usize).min(n.saturating_sub(1));
            let mut p = json!({"count": d.n("count").unwrap_or(1.0).clamp(1.0, 9999.0) as u64});
            match d.s("insert").as_str() {
                "before" => p["before"] = json!(page),
                "start" => p["before"] = json!(0),
                "end" => p["after"] = json!(n.saturating_sub(1)),
                _ => p["after"] = json!(page),
            }
            // A parent's id, or [None].
            p["parent"] = d.s("parent").parse::<u64>().map_or(Value::Null, |id| json!(id));
            app.run("layout.pages.insert", p)
        }
        "goToPage" => {
            // A page name ("iv", "A-3"), a number, or "+n" for an absolute position.
            let typed = match d.fields.get("page") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => "1".into(),
            };
            match app.session.resolve_page(&typed) {
                Some(abs) => {
                    crate::canvas::go_to_page(app, abs);
                    Ok(Value::Null)
                }
                None => Err(format!("no page \"{typed}\"")),
            }
        }
        "convertTextToTable" | "convertTableToText" => {
            let r = if d.id == "convertTextToTable" {
                let same = same_separators(&d);
                let style = table_style_value(&d);
                app.run(
                    "table.convertFromText",
                    json!({"columnSeparator": d.s("columnSeparator"), "rowSeparator": d.s("rowSeparator"),
                        "columns": if same { d.fields.get("columns").cloned().unwrap_or(json!(1)) } else { Value::Null }, "tableStyle": style}),
                )
            } else {
                app.run("table.convertToText", json!({"columnSeparator": d.s("columnSeparator"), "rowSeparator": d.s("rowSeparator")}))
            };
            // On an error (an emptied separator field) the dialog stays open to be corrected.
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "insertTable" => {
            // The table and its style in one step; a style that is gone leaves the document alone,
            // and the dialog stays open to be corrected.
            let style = table_style_value(&d);
            let r = app.run(
                "table.insert",
                json!({"rows": d.n("bodyRows").unwrap_or(4.0).max(1.0) as u64, "cols": d.n("columns").unwrap_or(4.0).max(1.0) as u64,
                    "headerRows": d.n("headerRows").unwrap_or(0.0).max(0.0) as u64, "footerRows": d.n("footerRows").unwrap_or(0.0).max(0.0) as u64,
                    "tableStyle": style}),
            );
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "numberingSection" => {
            // On an error the dialog stays open to be corrected.
            let r = crate::section_options::confirm(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "cellOptions" => {
            // On an error the dialog stays open to be corrected.
            let r = confirm_cell_options(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "textFrameOptions" => {
            // On an error the dialog stays open to be corrected.
            let mut d = d;
            let r = confirm_text_frame_options(app, &mut d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "documentSetup" => {
            // On an error the dialog stays open to be corrected.
            let r = confirm_document_setup(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "fileInfo" => {
            let r = confirm_file_info(app, &d);
            if let Err(e) = &r {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "paragraphStyleOptions" => {
            let mut d = d;
            let r = confirm_paragraph_style_options(app, &mut d);
            if let Err(e) = &r {
                // Keep the dialog open with the reason, on the section it concerns.
                if e.contains("bad GREP") {
                    d.fields.insert("section".into(), json!("grep"));
                }
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        _ => confirm_part2(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part2(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "deleteParagraphStyle" => {
            let name = d.s("name");
            let replace = d.s("replaceWith");
            app.run(
                "style.paragraph.delete",
                json!({"name": name, "replaceWith": if replace.is_empty() { json!(designcraft_doc::BASIC_PARAGRAPH) } else { json!(replace) }}),
            )
        }
        "paragraphLocal" => {
            let attrs: Map<String, Value> = d.fields.iter().filter_map(|(k, v)| k.strip_prefix("p.").map(|a| (a.to_string(), v.clone()))).collect();
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs}))
        }
        "hyphenation" => {
            // The preview goes; the edits are made once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let attrs = hyphenation_edits(&d);
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs})).inspect_err(|e| {
                // The dialog stays open, with the reason, to be corrected.
                app.status(e.clone());
                let mut d = d;
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            })
        }
        "justification" => {
            let current = d.fields.get("current").cloned().unwrap_or_default();
            if let Err(e) = check_justification(&d, "", &current) {
                // The dialog stays open to be corrected.
                app.status(e.clone());
                let mut d = d;
                d.fields.insert("justifyError".into(), json!(e));
                app.ui.dialog = Some(d);
                return Err(e);
            }
            let attrs: Map<String, Value> = JUSTIFY_ATTRS.iter().filter_map(|k| d.fields.get(*k).map(|v| (k.to_string(), v.clone()))).collect();
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs}))
        }
        "paragraphRules" => {
            let attrs = rule_edits(&d, "");
            if attrs.is_empty() {
                return Ok(Value::Null);
            }
            app.run("type.para", json!({"attrs": attrs}))
        }
        "characterStyleOptions" => {
            let name = d.s("name");
            let mut chars = Map::new();
            for (k, v) in &d.fields {
                if let Some(a) = k.strip_prefix("c.") {
                    chars.insert(a.into(), v.clone());
                }
            }
            let rename = d.s("rename").trim().to_string();
            let based = d.fields.get("basedOn").and_then(Value::as_str).map(str::to_string);
            let r = if d.b("new") {
                // A new style sets only what was filled in.
                chars.retain(|_, v| !v.is_null());
                let mut params = json!({"name": if rename.is_empty() { "Character Style 1" } else { rename.as_str() }, "chars": chars});
                if let Some(b) = based.filter(|b| b != designcraft_doc::NO_CHAR_STYLE) {
                    params["basedOn"] = json!(b);
                }
                // Apply Style to Selection.
                app.run("style.character.create", params).and_then(|r| {
                    if d.b("apply") && app.session.active().is_some_and(|st| st.selection.text.is_some()) {
                        app.run("style.character.apply", json!({"name": r["name"]}))?;
                    }
                    Ok(r)
                })
            } else {
                let mut params = json!({"name": name, "chars": chars});
                if let Some(b) = based {
                    params["basedOn"] = if b == designcraft_doc::NO_CHAR_STYLE { Value::Null } else { json!(b) };
                }
                if !rename.is_empty() && rename != name {
                    params["rename"] = json!(rename);
                }
                app.run("style.character.edit", params)
            };
            if let Err(e) = &r {
                // Keep the dialog open with the reason (a taken name, a based-on loop).
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                d.fields.insert("section".into(), json!("general"));
                app.ui.dialog = Some(d);
            }
            r
        }
        "deleteCharacterStyle" => {
            let name = d.s("name");
            let replace = d.s("replaceWith");
            app.run(
                "style.character.delete",
                json!({"name": name, "replaceWith": if replace.is_empty() { json!(designcraft_doc::NO_CHAR_STYLE) } else { json!(replace) }}),
            )
        }
        "swatchOptions" | "colorGroupOptions" | "layerSettings" => {
            let r = match d.id.as_str() {
                "swatchOptions" => options::confirm_swatch_options(app, &d),
                "colorGroupOptions" => options::confirm_color_group_options(app, &d),
                _ => options::confirm_layer_options(app, &d),
            };
            if let Err(e) = &r {
                // Keep the dialog open with the reason (a taken or empty name).
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        _ => confirm_part3(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part3(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        // Nothing edited: OK leaves the fill or stroke as it was (its swatch and tint).
        "colorPicker" if !d.b("edited") => Ok(Value::Null),
        "colorPicker" => {
            let color = if d.s("model") == "cmyk" {
                let [c, m, y, k] = nums::<4>(&d, "cmyk").ok_or("no colour")?.map(|v| v / 100.0);
                json!({"model": "cmyk", "c": c, "m": m, "y": y, "k": k})
            } else {
                let [r, g, b] = nums::<3>(&d, "rgb").ok_or("no colour")?.map(|v| v / 255.0);
                json!({"model": "rgb", "r": r, "g": g, "b": b})
            };
            app.run("object.color", json!({"color": color, "target": d.s("target"), "text": d.b("text")}))
        }
        "layerOptions" => {
            let hidden: Vec<Value> = d
                .fields
                .get("layers")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter(|l| l["visible"] == false).map(|l| l["name"].clone()).collect())
                .unwrap_or_default();
            // A dialog opened without the menu's fields has no `useDefaults` at all: confirming it
            // with no layer unticked resets file defaults, as it did before all-on existed, rather
            // than making every layer explicitly visible. Only an explicit `false` means all-on.
            let use_defaults = d.fields.get("useDefaults").and_then(Value::as_bool).unwrap_or(hidden.is_empty());
            let hidden = if use_defaults { Vec::new() } else { hidden };
            app.run("object.layerOptions", json!({"hidden": hidden, "useDefaults": use_defaults}))
        }
        "colorSettings" => {
            let r = app.run("color.settings", json!({"rgb": d.s("rgb"), "cmyk": d.s("cmyk"), "intent": d.s("intent"), "bpc": d.b("bpc")}))?;
            app.ui.color_settings = Some(designcraft_color::cms::active_settings());
            app.canvas.shown = None;
            Ok(r)
        }
        "pdfImport" => {
            let crop = d.s("crop");
            // One page number, 1 or more; a blank field is page 1. A range or list ("4-5", "2,5"), a
            // fraction or a negative is refused rather than placing some other page.
            let raw = if d.fields.get("page").is_none_or(Value::is_null) { String::new() } else { d.s("page") };
            let page = match raw.trim() {
                "" => 1,
                t => match t.parse::<u64>() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        let e = "Enter a single page number; one page is placed at a time".to_string();
                        let mut d = d;
                        d.fields.insert("status".into(), json!(e));
                        app.ui.dialog = Some(d);
                        return Err(e);
                    }
                },
            };
            if d.b("async") {
                return app.confirm_pdf_import(page, if crop.is_empty() { "crop".into() } else { crop });
            }
            if let Some(target) = d.fields.get("target").and_then(Value::as_u64) {
                app.activate_import_target(&crate::ImportRequest { purpose: "place".into(), target: Some(target) })?;
            }
            app.place_file(json!({"path": d.s("path"), "pdfPage": page, "pdfCrop": if crop.is_empty() { "crop".to_string() } else { crop }}))
        }
        "print" => {
            let pages = if d.s("range") == "all" { Value::Null } else { json!(d.s("pages")) };
            let printer = d.s("printer");
            let r = app.run(
                "file.print",
                json!({"printer": if printer.is_empty() { Value::Null } else { json!(printer) }, "copies": d.n("copies").unwrap_or(1.0).max(1.0) as u64,
                    "pages": pages, "spreads": d.b("spreads"), "marks": d.b("marks"), "bleed": d.b("bleed")}),
            )?;
            let printer = r["printer"].as_str().unwrap_or_else(|| crate::i18n::tr(&app.ui.language, "the default printer"));
            app.status(
                crate::i18n::tr(&app.ui.language, "Sent {count} page(s) to {printer}")
                    .replace("{count}", &r["pages"].to_string())
                    .replace("{printer}", printer),
            );
            Ok(r)
        }
        "epubExport" => {
            let split = epub_export_split(&d);
            let stored = app.session.active().map(|st| st.doc.settings.epub_split.clone());
            if stored.as_ref() != Some(&split) {
                let v = serde_json::to_value(&split).map_err(|e| e.to_string())?;
                app.run("document.epubSplit", json!({"split": v}))?;
            }
            crate::menus::export_epub(app, &json!({}))
        }
        "pdfExport" => {
            // Remember the last-used options; `pages` and `title` are per-job and not persisted.
            app.ui.pdf_export.preset = d.s("preset");
            app.ui.pdf_export.standard = d.s("standard");
            app.ui.pdf_export.compress_images = d.b("compressImages");
            app.ui.pdf_export.flatten = d.s("flatten");
            app.ui.pdf_export.spreads = d.b("spreads");
            app.ui.pdf_export.bleed = d.b("bleed");
            app.ui.pdf_export.marks_crop = d.b("marksCrop");
            app.ui.pdf_export.marks_bleed = d.b("marksBleed");
            app.ui.pdf_export.marks_page_info = d.b("marksPageInfo");
            app.ui.pdf_export.marks_weight = d.s("marksWeight");
            app.ui.pdf_export.marks_offset = d.s("marksOffset");
            app.ui.pdf_export.tagged = d.b("tagged");

            let mut params = pdf_export_params(&d);
            let pages = d.s("pages");
            if !pages.trim().is_empty() && pages != "all" {
                params["pages"] = json!(pages);
            }
            let title = d.s("title");
            // Without a title the document's name names the file, and the PDF's title comes from
            // File Info.
            let doc_name = app.session.active().map(|st| st.doc.title.clone()).unwrap_or_default();
            if !title.is_empty() {
                params["title"] = json!(title);
            }
            let stem = if title.is_empty() { doc_name } else { title };
            let name = if stem.is_empty() { "Export.pdf".to_string() } else { format!("{stem}.pdf") };
            app.ask_save(&name, Bound::Document, move |app, path| crate::menus::export_pdf(app, &with_path(&params, "path", path)))
        }
        _ => confirm_part4(app, d),
    }
}

/// Color Settings › Load Profile…: load the chosen profile and list it in the dialog if it is
/// still open.
fn load_color_profile(app: &mut DesignApp, path: String) -> Result<Value, String> {
    if let Err(e) = app.session.execute("color.loadProfile", &json!({"path": path})) {
        app.status_error("Color Settings", e);
        return Ok(Value::Null);
    }
    // Replayed in load order at start-up, so the latest load of a path comes last.
    app.ui.color_profiles.retain(|p| p != &path);
    app.ui.color_profiles.push(path);
    let f = app.session.execute("color.settings", &json!({})).unwrap_or_default();
    if let Some(d) = app.ui.dialog.as_mut().filter(|d| d.id == "colorSettings") {
        d.fields.insert("profiles".into(), f["profiles"].clone());
    }
    Ok(Value::Null)
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part4(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "preferences" => {
            let mut prefs = json!({"scaleStrokes": d.b("scaleStrokes"), "dimensionsIncludeStroke": d.b("dimensionsIncludeStroke"), "transformationsAreTotals": d.b("transformationsAreTotals"), "absolutePageNumbers": d.b("absolutePageNumbers"), "highlightHj": d.b("highlightHj"), "highlightKeeps": d.b("highlightKeeps"), "highlightCustomTracking": d.b("highlightCustomTracking"), "highlightSubstitutedFonts": d.b("highlightSubstitutedFonts"), "richBlackOutput": d.b("richBlackOutput"), "typographersQuotes": d.b("typographersQuotes"), "showFontNamesInEnglish": d.b("showFontNamesInEnglish"), "smartTextReflow": d.b("smartTextReflow"),
                    "autocorrect": d.b("autocorrect"), "showAddedText": d.b("showAddedText"), "showNoteAnchors": d.b("showNoteAnchors"),
                    "recoveryMinutes": d.n("recoveryMinutes").unwrap_or(0.5), "linkTextFiles": d.b("linkTextFiles"),
                    "recentFilesMax": d.n("recentFilesMax").unwrap_or(20.0).clamp(0.0, f64::from(designcraft_engine::MAX_RECENT_FILES)) as u32,
                    "autocorrectList": d.s("autocorrectText").lines().filter_map(|l| {
                        let (a, b) = l.split_once('→').or_else(|| l.split_once("->"))?;
                        let (a, b) = (a.trim().to_lowercase(), b.trim().to_string());
                        (!a.is_empty() && !b.is_empty()).then(|| json!([a, b]))
                    }).collect::<Vec<_>>()});
            if d.b("cjkFeatures") != d.b("cjkFeatures.initial") {
                prefs["cjkFeatures"] = json!(d.b("cjkFeatures"));
            }
            // No document open: the Units & Increments new documents start with, saved with the
            // rest in one `prefs.set` (which checks everything before it changes anything). A value
            // that doesn't read, or that it refuses, keeps the dialog open to be fixed, with no
            // preference changed.
            let saved = default_units_of(&d).and_then(|du| {
                if let Some(du) = du {
                    prefs["defaultUnits"] = du;
                }
                app.run("prefs.set", prefs)
            });
            if let Err(e) = saved {
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.status(e.clone());
                app.ui.dialog = Some(d);
                return Err(e);
            }
            app.ui.dynamic_spelling = d.b("dynamicSpelling");
            if d.fields.contains_key("userWords") {
                let words: Vec<String> = d.s("userWords").lines().map(str::to_string).collect();
                app.run("spelling.setWords", json!({"words": words}))?;
            }
            // The slider's range. A size that isn't finite is saved as `null`, and ui.json then fails to load.
            if let Some(size) = d.n("storyEditorSize").filter(|v| v.is_finite()) {
                app.ui.story_editor_size = size.clamp(8.0, 36.0) as f32;
            }
            // Highlight options change the screen view only.
            app.canvas.shown = None;
            let view = match d.s("displayQuality").as_str() {
                "fast" => "view.fastDisplay",
                "typical" => "view.typicalDisplay",
                _ => "view.highQualityDisplay",
            };
            app.run(view, json!({}))?;
            app.run("window.richBlack", json!({"on": d.b("richBlack")}))?;
            // Interface: the theme (already showing, unless set without a click) and the Tools panel.
            if let Some(b) = d.fields.get("brightness").and_then(Value::as_str).filter(|b| *b != app.ui.brightness.id()) {
                app.run("window.brightness", json!({"brightness": b}))?;
            }
            if d.fields.contains_key("toolsColumns") {
                app.run("window.toolsDoubleColumn", json!({"on": d.s("toolsColumns") == "double"}))?;
            }
            if let Some(v) = d.n("uiScale") {
                app.run("window.uiScale", json!({"scale": v / 100.0}))?;
            }
            if let Some(on) = d.fields.get("autoCollapseIconicPanels").and_then(Value::as_bool) {
                app.ui.dock_auto_collapse = on;
            }
            if d.fields.contains_key("snap.alignEdges") {
                let mut snap = json!({
                    "alignEdges": d.b("snap.alignEdges"),
                    "alignCenters": d.b("snap.alignCenters"),
                    "smartDimensions": d.b("snap.dimensions"),
                    "smartSpacing": d.b("snap.spacing"),
                });
                if let Some(zone) = d.n("snap.zone") {
                    snap["zone"] = json!(zone);
                }
                app.run("view.snapPreferences", snap)?;
            }
            if d.b("defaults") {
                // The default units went with the preferences above.
                return Ok(Value::Null);
            }
            let Some(horizontal) = d.fields.get("horizontalUnits") else { return Ok(Value::Null) };
            let mut doc = json!({"horizontalUnits": horizontal});
            // Fields can come from `ui.dialog.open`; rulers it doesn't name keep their units.
            if let Some(vertical) = d.fields.get("verticalUnits") {
                doc["verticalUnits"] = vertical.clone();
            }
            if let Some(o) = d.fields.get("rulerOrigin").filter(|o| o.is_string()) {
                doc["rulerOrigin"] = o.clone();
            }
            let mut bg = json!({"relativeTo": d.s("bg.relativeTo"), "color": d.fields.get("bg.color").cloned().unwrap_or(json!([140, 205, 230]))});
            if let Some(v) = d.m("bg.start") {
                bg["start"] = json!(v.max(0.0));
            }
            if let Some(v) = d.m("bg.increment").filter(|v| *v > 0.0) {
                bg["increment"] = json!(v);
            }
            if let Some(v) = d.n("bg.viewThreshold") {
                bg["viewThreshold"] = json!((v / 100.0).clamp(0.05, 40.0));
            }
            doc["baselineGrid"] = bg;
            let mut grid = json!({"inBack": d.b("grid.inBack"), "color": d.fields.get("grid.color").cloned().unwrap_or(json!([200, 200, 200]))});
            for k in ["horizontal", "vertical"] {
                if let Some(v) = d.m(&format!("grid.{k}")).filter(|v| *v > 0.0) {
                    grid[k] = json!(v);
                }
            }
            if let Some(v) = d.n("grid.subdivisions") {
                grid["subdivisions"] = json!(v.clamp(1.0, 100.0) as u32);
            }
            doc["grid"] = grid;
            for k in ["marginColor", "columnColor", "bleedColor", "slugColor"] {
                if let Some(v) = d.fields.get(k) {
                    doc[k] = v.clone();
                }
            }
            if let (Some(h), Some(v)) = (d.m("pasteboard.h"), d.m("pasteboard.v")) {
                doc["pasteboard"] = json!([h.max(0.0), v.max(0.0)]);
            }
            if let Some(v) = d.fields.get("keyboardIncrement").and_then(Value::as_str).and_then(|s| parse_measure(s, Unit::Points).ok()) {
                doc["keyboardIncrement"] = json!(v.max(0.001));
            }
            if let Some(v) = d.n("kerningIncrement").filter(|v| v.is_finite()) {
                doc["kerningIncrement"] = json!(v.clamp(1.0, 1000.0));
            }
            let mut adv = json!({});
            for k in ["superscriptSize", "superscriptPosition", "subscriptSize", "subscriptPosition"] {
                if let Some(v) = d.n(&format!("adv.{k}")) {
                    adv[k] = json!(if k.ends_with("Size") { v.clamp(1.0, 200.0) } else { v.clamp(-500.0, 500.0) });
                }
            }
            doc["advancedType"] = adv;
            doc["overprintBlack"] = json!(d.b("overprintBlack"));
            doc["glyphFallback"] = json!(d.b("glyphFallback"));
            app.run("document.preferences", doc)
        }
        _ => confirm_part5(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part5(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "newWorkspace" => app.run(
            "window.newWorkspace",
            json!({"name": d.s("name"), "panelLocations": d.b("panelLocations"), "menuCustomization": d.b("menuCustomization")}),
        ),
        "deletePages" => {
            if d.b("dontShowAgain") {
                app.ui.skip_delete_pages_warning = true;
            }
            app.run("layout.pages.delete", json!({"pages": d.fields.get("pages").cloned().unwrap_or_else(|| json!([]))}))
        }
        "pagesPanelOptions" => {
            use crate::panels::pages::{IconSize, PanelResize};
            let o = &mut app.ui.pages_panel;
            o.page_size = IconSize::from_key(&d.s("pageSize")).unwrap_or(o.page_size);
            o.page_thumbnails = d.b("pageThumbnails");
            o.parent_size = IconSize::from_key(&d.s("parentSize")).unwrap_or(o.parent_size);
            o.parent_thumbnails = d.b("parentThumbnails");
            o.parents_vertical = d.b("parentsVertical");
            o.pages_on_top = d.s("layout") == "pagesOnTop";
            o.resize = PanelResize::from_key(&d.s("resize")).unwrap_or(o.resize);
            Ok(Value::Null)
        }
        "updateLink" => crate::panels::properties::run_update_links(app, d.fields.get("target").cloned().unwrap_or_else(|| json!({}))),
        "closeDocument" => {
            // The document may have moved in the tab row (or gone) while the dialog was open.
            let uid = d.fields.get("uid").and_then(Value::as_u64);
            if d.b("quitting") {
                return app.confirm_quit_document(uid.ok_or("Missing document in quit confirmation")?, d.b("discard"));
            }
            let index = move |app: &DesignApp| app.session.documents().iter().position(|doc| Some(doc.uid) == uid);
            let Some(i) = index(app) else { return Ok(Value::Null) };
            let discard = d.b("discard");
            // Closes the document once it is saved (or at once, for Don't Save). Still unsaved, the
            // save failed: the document stays open.
            let close = move |app: &mut DesignApp| match index(app) {
                Some(i) if discard || app.session.documents().get(i).is_some_and(|doc| !doc.is_dirty()) => app.run("file.close", json!({"index": i})),
                _ => Ok(Value::Null),
            };
            if discard {
                return close(app);
            }
            app.run("file.activate", json!({"index": i}))?;
            crate::menus::save_then(app, close)
        }
        "loadStyles" => {
            let checked = |key: &str| -> Vec<&str> {
                let rows = d.fields.get(key).and_then(Value::as_array).into_iter().flatten();
                rows.filter(|row| row["on"] != false).filter_map(|row| row["name"].as_str()).collect()
            };
            app.run(
                "style.load",
                json!({"path": d.s("path"), "paragraph": checked("paragraph"), "character": checked("character"), "conflicts": d.s("conflicts")}),
            )
        }
        "importOptions" => app.open_file(
            "file.place",
            json!({"path": d.s("path"), "removeStyles": d.b("removeStyles"), "styleConflicts": d.s("styleConflicts"), "styleMap": d.fields.get("map").cloned().unwrap_or(json!({}))}),
        ),
        "fittingOptions" => {
            let m = |k: &str| d.fields.get(k).and_then(Value::as_str).and_then(|s| parse_measure(s, Unit::Points).ok()).unwrap_or(0.0);
            app.run(
                "object.fittingOptions",
                json!({"autoFit": d.b("autoFit"), "fitting": d.s("fitting"), "align": d.n("align").unwrap_or(4.0) as u64,
                    "crop": [m("cropTop"), m("cropLeft"), m("cropBottom"), m("cropRight")]}),
            )
        }
        "qrCode" => {
            let mut p = Value::Object(d.fields.clone());
            // Nothing selected: a 2-inch code at the top left of the page in view.
            if let Some(st) = app.session.active().filter(|s| s.selection.items.is_empty()) {
                let abs = crate::canvas::current_page(app).unwrap_or(0);
                let (si, pi) = st.doc.page_loc(abs).unwrap_or((0, 0));
                let x = st.doc.spreads.get(si).and_then(|sp| sp.pages.get(pi)).map_or(0.0, |pg| pg.x);
                p["rect"] = json!([x + 36.0, 36.0, x + 180.0, 180.0]);
                p["spread"] = json!(si);
            }
            app.run("object.qrCode", p)
        }
        "polygonSettings" => app.run("tool.polygonSettings", json!({"sides": d.n("sides").unwrap_or(6.0) as u64, "starInset": d.n("starInset").unwrap_or(0.0)})),
        "findFont" => {
            let result = app.run("font.replace", json!({"family": d.s("family"), "style": d.s("style"), "toFamily": d.s("toFamily"), "toStyle": d.s("toStyle")}));
            let mut d = d;
            match &result {
                Ok(_) => {
                    d.fields.remove("_fonts");
                    d.fields.remove("family");
                    d.fields.remove("style");
                    d.fields.remove("error");
                }
                Err(error) => { d.fields.insert("error".into(), json!(error)); }
            }
            app.ui.dialog = Some(d);
            result
        }
        _ => confirm_part6(app, d),
    }
}

/// One slice of the dialogs' Apply; later slices take the ids this one doesn't.
fn confirm_part6(app: &mut DesignApp, d: Dialog) -> Result<Value, String> {
    match d.id.as_str() {
        "insertXref" => {
            let format = d.s("format");
            let target = d.s("target");
            let params = match target.split_once(':') {
                Some(("a", id)) => json!({"anchor": id.parse::<u64>().unwrap_or(0), "format": format}),
                Some((sid, pi)) => json!({"story": sid.parse::<u64>().unwrap_or(0), "para": pi.parse::<u64>().unwrap_or(0), "format": format}),
                None => {
                    let mut d = d.clone();
                    d.fields.insert("status".into(), json!("Choose a destination paragraph or text anchor."));
                    app.ui.dialog = Some(d);
                    return Err("no destination chosen".into());
                }
            };
            app.run("xref.insert", params)
        }
        "footnoteOptions" => {
            let pt = |k: &str| d.pt(k).map_or(Value::Null, |v| json!(v));
            let mut p = json!({
                "style": d.s("style"), "startAt": d.n("startAt").unwrap_or(1.0).max(0.0) as u32, "restart": d.s("restart"),
                "prefix": d.s("prefix"), "suffix": d.s("suffix"), "affixIn": d.s("affixIn"), "refPosition": d.s("refPosition"),
                "refCharStyle": d.s("refCharStyle"), "paraStyle": d.s("paraStyle"),
                "separator": d.s("separator").replace("^t", "\t").replace("^m", "\u{2003}").replace("^>", "\u{2002}"),
                "spaceBefore": pt("spaceBefore"), "spaceBetween": pt("spaceBetween"), "firstBaseline": d.s("firstBaseline"),
                "firstBaselineMin": pt("firstBaselineMin"), "spanColumns": d.b("spanColumns"),
                "rule": {"on": d.b("rule.on"), "weight": pt("rule.weight"), "color": d.s("rule.color"), "width": pt("rule.width"),
                    "offset": pt("rule.offset"), "leftIndent": pt("rule.leftIndent")},
            });
            // Unparsable measures keep their current values.
            if let Some(o) = p.as_object_mut() {
                o.retain(|_, v| !v.is_null());
            }
            if let Some(r) = p.get_mut("rule").and_then(Value::as_object_mut) {
                r.retain(|_, v| !v.is_null());
            }
            app.run("footnote.options", p)
        }
        "alert" => Ok(Value::Null),
        crate::effects_dialog::ID => crate::effects_dialog::confirm(app, &d),
        crate::package_dialog::ID | crate::package_dialog::INSTRUCTIONS => {
            let r = if d.id == crate::package_dialog::ID {
                crate::package_dialog::confirm(app, &d)
            } else {
                crate::package_dialog::confirm_instructions(app, &d)
            };
            // Packaging failed: the dialog stays open with its options and instructions, and the
            // status line says why.
            if let Err(e) = &r
                && app.ui.dialog.is_none()
            {
                app.status(e.clone());
                app.ui.dialog = Some(d);
            }
            r
        }
        "findChange" => {
            let result = if d.b("objectMode") { Ok(Value::Null) } else { app.run("find.change", find_change_params(&d)) };
            // Confirmation consumes retained bounds before closing, even on an error.
            app.session.reset_find_scope();
            result
        }
        CORNER_OPTIONS => {
            // The preview goes; the corners change once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let mut d = d;
            seed_corner_options(app, &mut d);
            link_corners(&mut d);
            let r = corner_options_params(&d).and_then(|p| app.run("object.cornerOptions", p));
            if let Err(e) = &r {
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        STEP_AND_REPEAT => {
            // The preview goes; the copies are made once, as one undo step.
            if d.fields.contains_key(PREVIEWED) {
                app.session.cancel_interaction();
            }
            let r = step_and_repeat_params(app, &d).and_then(|p| app.run("edit.stepAndRepeat", p));
            if let Err(e) = &r {
                let mut d = d.clone();
                d.fields.remove(PREVIEWED);
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        TOC => {
            let r = app.run("toc.generate", toc_params(app, &d));
            if let Err(e) = &r {
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        id if id.starts_with("cmd:") => {
            let cid = &id[4..];
            let doc = designcraft_engine::find_command(cid).map_or("", |c| c.params);
            let mut p = Map::new();
            for f in command_fields(doc) {
                match d.fields.get(&f.key) {
                    Some(Value::String(v)) if !v.trim().is_empty() => {
                        let v = v.trim();
                        // Numbers, booleans, arrays and objects as JSON; anything else is a string.
                        let parsed = serde_json::from_str::<Value>(v).ok().filter(|x| !x.is_string());
                        p.insert(f.key, parsed.unwrap_or_else(|| json!(v)));
                    }
                    Some(Value::String(_)) | None => {}
                    Some(value) => {
                        // Control/MCP callers can supply JSON directly. Preserve its type,
                        // including explicit null, for the command to interpret.
                        p.insert(f.key, value.clone());
                    }
                }
            }
            let r = app.run(cid, Value::Object(p));
            if let Err(e) = &r {
                // Keep the dialog open with the error.
                let mut d = d.clone();
                d.fields.insert("status".into(), json!(e));
                app.ui.dialog = Some(d);
            }
            r
        }
        other => Err(format!("unknown dialog {other}")),
    }
}

/// Why `p` is not a pattern the GREP styles can use, if it isn't. (Checked as the user types.)
fn grep_error(p: &str) -> Option<String> {
    if p.is_empty() {
        return None;
    }
    designcraft_compose::grep::Grep::new(p, Default::default()).err().map(|e| e.to_string())
}

/// What paragraph style `style` resolves to when based on `based` (the Based On chosen in its
/// dialog, which may not be its own yet): `based`'s chain with the style's own settings over it.
fn style_over(
    styles: &designcraft_doc::Styles,
    based: &str,
    style: &designcraft_doc::ParagraphStyle,
) -> (designcraft_doc::ParaProps, designcraft_doc::CharProps) {
    styles.resolve_para(&designcraft_doc::ParaFormat {
        style: based.into(),
        para: style.para.clone(),
        chars: style.chars.clone(),
        ..Default::default()
    })
}

/// The Justification spacing check of Paragraph Style Options, against what the section showed:
/// the style's own values, or a new style's Based On.
fn check_paragraph_style_justification(app: &DesignApp, d: &Dialog) -> Result<(), String> {
    // After Reset To Base the style starts from its Based On (the one chosen here, if any).
    let base_of = |st: &designcraft_engine::DocState| {
        let chosen = d.s("basedOn");
        if chosen.is_empty() { st.doc.styles.para(&d.s("name")).and_then(|s| s.based_on.clone()).unwrap_or_default() } else { chosen }
    };
    let base = app
        .session
        .active()
        .map(|st| {
            let pp = if d.b("new") {
                st.doc.styles.resolve_para_style(&d.s("basedOn")).0
            } else if d.b("resetToBase") {
                st.doc.styles.resolve_para_style(&base_of(st)).0
            } else {
                // The style's own settings over the Based On chosen in the dialog.
                match st.doc.styles.para(&d.s("name")) {
                    Some(style) => style_over(&st.doc.styles, &base_of(st), style).0,
                    None => st.doc.styles.resolve_para_style(&d.s("name")).0,
                }
            };
            serde_json::to_value(pp).unwrap_or_default()
        })
        .unwrap_or_default();
    check_justification(d, "p.", &base)
}

/// The `p.*` and `c.*` dialog fields as the paragraph and character attribute maps a style
/// command takes. `p.ruleAbove.weight` is one field of a rule (the command keeps the others).
fn style_option_maps(d: &Dialog) -> (serde_json::Map<String, Value>, serde_json::Map<String, Value>) {
    let mut para = serde_json::Map::new();
    let mut chars = serde_json::Map::new();
    for (k, v) in &d.fields {
        if let Some(a) = k.strip_prefix("p.") {
            match a.split_once('.') {
                Some((attr, field)) => {
                    if let Value::Object(m) = para.entry(attr).or_insert_with(|| json!({})) {
                        m.insert(field.into(), v.clone());
                    }
                }
                None => {
                    para.insert(a.into(), v.clone());
                }
            }
        } else if let Some(a) = k.strip_prefix("c.") {
            chars.insert(a.into(), v.clone());
        }
    }
    (para, chars)
}

fn confirm_paragraph_style_options(app: &mut DesignApp, d: &mut Dialog) -> Result<Value, String> {
    let name = d.s("name");
    if let Err(e) = check_paragraph_style_justification(app, d) {
        // The caller shows the reason in the dialog's status line.
        d.fields.insert("section".into(), json!("justify"));
        return Err(e);
    }
    let (para, chars) = style_option_maps(d);
    let based = d.s("basedOn");
    let rename = d.s("rename").trim().to_string();
    let next = d.fields.get("nextStyle").cloned();
    let (r, style) = if d.b("new") {
        let mut params = json!({"name": if rename.is_empty() { "Paragraph Style 1" } else { rename.as_str() }, "para": para, "chars": chars});
        if !based.is_empty() && based != designcraft_doc::NO_PARA_STYLE {
            params["basedOn"] = json!(based);
        }
        if let Some(Value::String(n)) = &next {
            params["nextStyle"] = json!(n);
        }
        let r = app.run("style.paragraph.create", params)?;
        let style = r["name"].as_str().unwrap_or_default().to_string();
        (r, style)
    } else {
        let mut params = json!({"name": name, "para": para, "chars": chars});
        if let Some(n) = next {
            params["nextStyle"] = n;
        }
        if d.b("resetToBase") {
            params["resetToBase"] = json!(true);
        }
        if !based.is_empty() {
            params["basedOn"] = if based == designcraft_doc::NO_PARA_STYLE { Value::Null } else { json!(based) };
        }
        let renaming = !rename.is_empty() && rename != name;
        if renaming {
            params["rename"] = json!(rename);
        }
        let r = app.run("style.paragraph.edit", params).inspect_err(|_| {
            // A refused name is shown next to the name field.
            if renaming {
                d.fields.insert("section".into(), json!("general"));
            }
        })?;
        (r, if renaming { rename } else { name })
    };
    // The style now exists under `style`: if a later step fails, the dialog stays open on it and
    // a second OK edits it rather than creating another one.
    let created = d.b("new");
    d.fields.insert("new".into(), json!(false));
    d.fields.insert("name".into(), json!(&style));
    d.fields.insert("rename".into(), json!(&style));
    // Apply Style to Selection.
    if created && d.b("apply") && app.session.active().is_some_and(|st| st.selection.text.is_some()) {
        app.run("style.paragraph.apply", json!({"name": style}))?;
    }
    if d.fields.contains_key("x.tag") {
        let tag = d.s("x.tag");
        let tag = if tag == "[Automatic]" { String::new() } else { tag };
        app.run("style.exportTag", json!({"style": style, "tag": tag, "class": d.s("x.class"), "split": d.b("x.split")}))?;
    }
    Ok(r)
}

/// Copies the dialog's `c.*` (and, for a paragraph style, `p.*`) fields into the preview params.
fn fill_style_preview_fields(params: &mut Value, d: &Dialog, paragraph: bool) {
    for (key, value) in &d.fields {
        if let Some(attr) = key.strip_prefix("c.") {
            params["chars"][attr] = value.clone();
        } else if paragraph && let Some(attr) = key.strip_prefix("p.") {
            if let Some((attr, field)) = attr.split_once('.') {
                if params["para"][attr].is_null() {
                    params["para"][attr] = json!({});
                }
                if let Some(fields) = params["para"][attr].as_object_mut() {
                    fields.insert(field.into(), value.clone());
                }
            } else {
                params["para"][attr] = value.clone();
            }
        }
    }
}

/// Preview the changed style attributes through the same engine commands as OK. Renaming and
/// export tagging have no visual effect and wait for OK, so the dialog can keep looking up the
/// original style while the preview is visible. The interaction restores its starting document
/// before each update, including after a field is cleared or Preview is switched off.
fn style_options_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") || d.b("new") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let paragraph = d.id == "paragraphStyleOptions";
    // Values OK would refuse don't reach the page either: the page goes back and the status
    // line says why.
    if paragraph && let Err(e) = check_paragraph_style_justification(app, d) {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
        }
        d.fields.insert("status".into(), json!(e));
        return;
    }
    let mut params = json!({"name": d.s("name"), "chars": {}});
    if paragraph {
        params["para"] = json!({});
    }
    fill_style_preview_fields(&mut params, d, paragraph);
    if let Some(based) = d.fields.get("basedOn") {
        params["basedOn"] = match based.as_str() {
            Some(designcraft_doc::NO_PARA_STYLE | designcraft_doc::NO_CHAR_STYLE) => Value::Null,
            _ => based.clone(),
        };
    }
    // Reset To Base shows on the page too.
    if paragraph && d.b("resetToBase") {
        params["resetToBase"] = json!(true);
    }
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(error) = app.session.begin_interaction("Style Options")
    {
        d.fields.insert("status".into(), json!(error.to_string()));
        return;
    }
    let command = if paragraph { "style.paragraph.edit" } else { "style.character.edit" };
    match app.session.preview(command, params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(error) => d.fields.insert("status".into(), json!(error.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// Paragraph Style Options: sections in a left list (like InDesign), fields on the right.
/// Edited values are stored as `p.<attr>` / `c.<attr>` fields and applied on OK.
fn paragraph_style_options(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let name = d.s("name");
    let new = d.b("new");
    let Some(doc) = app.session.active().map(|s| s.doc.clone()) else { return };
    let style = match doc.styles.para(&name) {
        _ if new => designcraft_doc::ParagraphStyle {
            name: String::new(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: Default::default(),
            shortcut: String::new(),
        },
        Some(s) => s.clone(),
        None => {
            ui.label(format!("No style named {name}"));
            return;
        }
    };
    if new && !d.fields.contains_key("rename") {
        d.fields.insert("rename".into(), json!(next_style_name(&doc.styles, true)));
    }
    let mut style = style;
    let reset = !new && d.b("resetToBase");
    if reset {
        style.para = Default::default();
        style.chars = Default::default();
    }
    let names: Vec<String> = doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| new || *n != name).collect();
    // A new style shows what it would inherit from its Based On (as does one reset to it).
    let based_name = d.fields.get("basedOn").and_then(Value::as_str).map(str::to_string).or(style.based_on.clone()).unwrap_or_default();
    // An existing style shows its own settings over the Based On chosen here.
    let (pp, cp) = if new || reset { doc.styles.resolve_para_style(&based_name) } else { style_over(&doc.styles, &based_name, &style) };
    let units = doc.settings.horizontal_units;
    let pv = serde_json::to_value(&pp).unwrap_or_default();
    let cv = serde_json::to_value(&cp).unwrap_or_default();
    let cur = |d: &Dialog, k: &str, base: &Value| d.fields.get(k).cloned().unwrap_or_else(|| base.clone());
    ui.set_min_width(560.0);
    ui.horizontal_top(|ui| {
        // A fixed height: the separator would otherwise stretch the dialog to the window.
        ui.set_min_height(380.0);
        ui.set_max_height(380.0);
        section_list(
            &app.ui.language,
            ui,
            d,
            crate::cjk_features(app),
            &[
                ("general", "General"),
                ("chars", "Basic Character Formats"),
                ("advanced", "Advanced Character Formats"),
                ("indents", "Indents and Spacing"),
                ("tabs", "Tabs"),
                ("rules", "Paragraph Rules"),
                ("border", "Paragraph Border"),
                ("shading", "Paragraph Shading"),
                ("keep", "Keep Options"),
                ("hyph", "Hyphenation"),
                ("justify", "Justification"),
                ("span", "Span Columns"),
                ("nested", "Drop Caps and Nested Styles"),
                ("grep", "GREP Style"),
                ("bullets", "Bullets and Numbering"),
                ("color", "Character Color"),
                ("openType", "OpenType Features"),
                ("underline", "Underline Options"),
                ("strikethrough", "Strikethrough Options"),
                ("export", "Export Tagging"),
            ],
        );
        ui.separator();
        let cnames: Vec<String> = app.session.active().map(|s| s.doc.styles.character.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        ui.vertical(|ui| match d.s("section").as_str() {
            "export" => {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "EPUB and HTML")).font(semibold(12.0)));
                let et = app.session.active().and_then(|s| s.doc.styles.export_tag(&name, false).cloned()).unwrap_or_default();
                if !d.fields.contains_key("x.tag") {
                    d.fields.insert("x.tag".into(), json!(if et.tag.is_empty() { "[Automatic]" } else { et.tag.as_str() }));
                    d.fields.insert("x.class".into(), json!(et.class));
                    d.fields.insert("x.split".into(), json!(et.split));
                }
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Tag:"));
                    let cur = d.s("x.tag");
                    craft_ui::ComboBox::from_id_salt("export_tag")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &cur)))
                        .show_ui(ui, |ui| {
                            for t in ["[Automatic]", "p", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "li", "figcaption", "aside", "div"]
                            {
                                if ui.selectable_label(cur == t, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, t))).clicked() {
                                    d.fields.insert("x.tag".into(), json!(t));
                                }
                            }
                        });
                });
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Class:"));
                    text_field(ui, d, "x.class", 160.0);
                });
                check(ui, d, "x.split", crate::i18n::tr(&app.ui.language, "Split Document (EPUB only)"));
            }
            "nested" => drop_caps_and_nested_styles(&app.ui.language, ui, d, &pv, &cnames),
            "grep" => grep_styles(&app.ui.language, ui, d, &pv, &cnames),
            "chars" => basic_character_formats(app, ui, d, &CharFields { base: &cv, sparse: false }),
            "indents" => {
                egui::Grid::new("psi").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    let alignment_label = crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Alignment:"));
                    let a: designcraft_doc::Align = serde_json::from_value(cur(d, "p.align", &pv["align"])).unwrap_or_default();
                    craft_ui::ComboBox::from_id_salt("psalign")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, a.label())))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for al in designcraft_doc::Align::ALL {
                                if ui.selectable_label(al == a, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, al.label()))).clicked() {
                                    d.fields.insert("p.align".into(), serde_json::to_value(al).unwrap_or_default());
                                }
                            }
                        })
                        .response
                        .labelled_by(alignment_label.id);
                    ui.end_row();
                    for (label, key) in [
                        ("Left Indent:", "leftIndent"),
                        ("First Line Indent:", "firstLineIndent"),
                        ("Right Indent:", "rightIndent"),
                        ("Space Before:", "spaceBefore"),
                        ("Space After:", "spaceAfter"),
                    ] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        let v = cur(d, &format!("p.{key}"), &pv[key]).as_f64();
                        if let Some(n) = crate::widgets::measure(ui, &format!("ps{key}"), v, units, 80.0) {
                            d.fields.insert(format!("p.{key}"), json!(n));
                        }
                        ui.end_row();
                    }
                    // The CJK Grid Settings section edits the same `p.gridAlign` field.
                    use designcraft_doc::GridAlign;
                    const GRID_ALIGN: [(GridAlign, &str); 3] =
                        [(GridAlign::None, "None"), (GridAlign::AllLines, "All Lines"), (GridAlign::FirstLineOnly, "First Line Only")];
                    let grid_label = crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Align to Grid:"));
                    let g: GridAlign = serde_json::from_value(cur(d, "p.gridAlign", &pv["gridAlign"])).unwrap_or_default();
                    let shown = GRID_ALIGN.iter().find(|o| o.0 == g).map_or("", |o| o.1);
                    craft_ui::ComboBox::from_id_salt("psgridalign")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, shown)))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for (v, label) in GRID_ALIGN {
                                if ui.selectable_label(v == g, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked()
                                    && let Ok(v) = serde_json::to_value(v)
                                {
                                    d.fields.insert("p.gridAlign".into(), v);
                                }
                            }
                        })
                        .response
                        .labelled_by(grid_label.id);
                    ui.end_row();
                });
                ui.add_space(6.0);
                let mut ignore = cur(d, "p.ignoreOpticalMargin", &pv["ignoreOpticalMargin"]).as_bool().unwrap_or(false);
                if ui.checkbox(&mut ignore, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Ignore Optical Margin"))).changed() {
                    d.fields.insert("p.ignoreOpticalMargin".into(), json!(ignore));
                }
            }
            "tabs" => style_tabs(&app.ui.language, ui, d, &pv, &doc),
            "rules" => paragraph_rules(app, ui, d, "p.", &pv),
            "hyph" => hyphenation(&app.ui.language, ui, d, "p.", &pv, units),
            "justify" => justification(&app.ui.language, ui, d, "p.", &pv),
            "color" => character_color(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }, &doc),
            "advanced" => advanced_character_formats(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }),
            "openType" => open_type_features(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }),
            "underline" => line_options(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }, &doc, "underline"),
            "strikethrough" => line_options(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }, &doc, "strikethrough"),
            "border" | "shading" => {
                let border = d.s("section") == "border";
                let title = if border { "Paragraph Border" } else { "Paragraph Shading" };
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, title)).size(14.0));
                ui.add_space(6.0);
                border_shading(&app.ui.language, ui, d, &pv, &doc, border);
            }
            "keep" => keep_options(&app.ui.language, ui, d, &pv),
            "span" => span_columns(&app.ui.language, ui, d, &pv, &doc),
            "bullets" => {
                let (char_styles, families, fonts, scope) = ListUi::new_parts(app);
                let lu =
                    ListUi { char_styles: &char_styles, families: &families, fonts: &fonts, scope, family: &cp.font_family, style: &cp.font_style };
                bullets_and_numbering(&app.ui.language, ui, d, &pv, &doc, &lu);
            }
            s if style_cjk::is_section(s) => style_cjk::section_ui(app, ui, d, s, &CharFields { base: &cv, sparse: false }, Some(&pv), &doc),
            _ => {
                egui::Grid::new("psg").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Style Name:"));
                    if !d.fields.contains_key("rename") {
                        d.fields.insert("rename".into(), json!(name));
                    }
                    let mut rn = d.s("rename");
                    let name_field = ui
                        .add(egui::TextEdit::singleline(&mut rn).desired_width(220.0))
                        .accessible_name(crate::i18n::tr(&app.ui.language, "Style Name").to_owned());
                    if name_field.changed() {
                        d.fields.insert("rename".into(), json!(rn));
                    }
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Based On:"));
                    let based = d
                        .fields
                        .get("basedOn")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or(style.based_on.clone())
                        .unwrap_or_else(|| "[No Paragraph Style]".into());
                    let combo = craft_ui::ComboBox::from_id_salt("psbased")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &based)))
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for n in &names {
                                if ui.selectable_label(*n == based, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                                    d.fields.insert("basedOn".into(), json!(n));
                                }
                            }
                        });
                    let _ = crate::widgets::name_combo(combo, crate::i18n::tr(&app.ui.language, "Based On"));
                    ui.end_row();
                    // Next Style: the style of the paragraph that follows when Enter ends this one.
                    let next_name = crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Next Style:"));
                    let next = match d.fields.get("nextStyle") {
                        Some(Value::String(n)) => Some(n.clone()),
                        Some(_) => None,
                        None => style.next_style.clone().filter(|n| *n != name),
                    };
                    let same = crate::i18n::tr(&app.ui.language, "[Same style]").to_string();
                    let shown = next.as_deref().map_or(same.clone(), |n| crate::i18n::style_name(&app.ui.language, n).to_string());
                    craft_ui::ComboBox::from_id_salt("psnext")
                        .selected_text(crate::rtl::widget(ui, shown))
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(next.is_none(), crate::rtl::widget(ui, &same)).clicked() {
                                d.fields.insert("nextStyle".into(), Value::Null);
                            }
                            for n in names.iter().filter(|n| *n != designcraft_doc::NO_PARA_STYLE) {
                                if ui
                                    .selectable_label(
                                        next.as_deref() == Some(n),
                                        crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n)),
                                    )
                                    .clicked()
                                {
                                    d.fields.insert("nextStyle".into(), json!(n));
                                }
                            }
                        })
                        .response
                        .labelled_by(next_name.id);
                    ui.end_row();
                });
                // Reset To Base: the style keeps nothing of its own (the edits made here stay).
                if !new
                    && ui
                        .add_enabled(
                            !based_name.is_empty() && based_name != designcraft_doc::NO_PARA_STYLE,
                            egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reset To Base"))),
                        )
                        .clicked()
                {
                    d.fields.insert("resetToBase".into(), json!(true));
                }
                ui.add_space(8.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Settings:")).font(semibold(12.0)));
                let summary = paragraph_style_summary(&app.ui.language, d, &style, &doc.styles);
                egui::ScrollArea::vertical().id_salt("paragraph_style_settings").max_height(180.0).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(summary).color(crate::theme::Tokens::get(ui.ctx()).text_dim)).wrap());
                });
                if new {
                    ui.add_space(8.0);
                    check(ui, d, "apply", crate::i18n::tr(&app.ui.language, "Apply Style to Selection"));
                }
            }
        });
    });
    // Why OK didn't apply (a pattern that can't compile, a taken name).
    let status = d.s("status");
    if !status.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(status).color(crate::theme::Tokens::get(ui.ctx()).text_strong));
    }
}

/// Show what the style changes from its parent, including edits waiting for OK. Fully captured
/// styles contain every default, so comparing resolved values keeps the summary readable.
fn paragraph_style_summary(lang: &str, d: &Dialog, style: &designcraft_doc::ParagraphStyle, styles: &designcraft_doc::Styles) -> String {
    let based = d.fields.get("basedOn").and_then(Value::as_str).or(style.based_on.as_deref()).unwrap_or(designcraft_doc::NO_PARA_STYLE);
    let (base_para, base_chars) = styles.resolve_para_style(based);
    let (mut para, mut chars) = (base_para.clone(), base_chars.clone());
    para.apply(&style.para);
    chars.apply(&style.chars);
    for (key, value) in &d.fields {
        if let Some(key) = key.strip_prefix("c.") {
            let mut delta = designcraft_doc::CharAttrs::default();
            if delta.set_json(key, value).is_ok() {
                chars.apply(&delta);
            }
        } else if let Some(key) = key.strip_prefix("p.") {
            let (key, value) = key.split_once('.').map_or((key, value.clone()), |(attr, field)| (attr, json!({field:value})));
            let mut delta = designcraft_doc::ParaAttrs::default();
            if delta.set_json_over(key, &value, &para).is_ok() {
                para.apply(&delta);
            }
        }
    }
    let mut char_delta = base_chars.delta(&chars);
    if base_chars.font_family != chars.font_family {
        char_delta.font_style = Some(chars.font_style.clone());
    }
    let mut parts = vec![crate::i18n::style_name(lang, based).to_string()];
    for value in [serde_json::to_value(base_para.delta(&para)), serde_json::to_value(char_delta)] {
        if let Ok(Value::Object(attrs)) = value {
            for (key, value) in attrs {
                let suffix = if matches!(
                    key.as_str(),
                    "size" | "spaceBefore" | "spaceAfter" | "leftIndent" | "rightIndent" | "firstLineIndent" | "lastLineIndent"
                ) {
                    " pt"
                } else {
                    ""
                };
                parts.push(format!("{}: {}{suffix}", setting_label(lang, &key), setting_text(&value)));
            }
        }
    }
    parts.join(" + ")
}

fn delete_paragraph_style(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let name = d.s("name");
    ui.set_min_width(380.0);
    crate::rtl::label(
        ui,
        egui::RichText::new(
            crate::i18n::tr(&app.ui.language, "Delete the paragraph style \"{name}\"?")
                .replace("{name}", crate::i18n::style_name(&app.ui.language, &name)),
        )
        .size(12.5),
    );
    ui.add_space(8.0);
    let Some(st) = app.session.active() else { return };
    let in_use = st.doc.stories.values().any(|s| s.paras.iter().any(|f| f.style == name));
    let names: Vec<String> = st.doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| *n != name).collect();
    if in_use {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "This style is in use. Text using it will be reassigned to:"))
                .size(11.0)
                .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
        );
    } else {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Replace with:")).size(11.0).color(crate::theme::Tokens::get(ui.ctx()).text_dim),
        );
    }
    ui.add_space(4.0);
    if !d.fields.contains_key("replaceWith") {
        d.fields.insert("replaceWith".into(), json!(designcraft_doc::BASIC_PARAGRAPH));
    }
    let replace = d.s("replaceWith");
    craft_ui::ComboBox::from_id_salt("del_para_replace")
        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &replace)))
        .width(260.0)
        .show_ui(ui, |ui| {
            for n in &names {
                if ui.selectable_label(*n == replace, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                    d.fields.insert("replaceWith".into(), json!(n));
                }
            }
        });
}

/// Paragraph Style Options › Tabs: the style's stops on a ruler as wide as the first page's
/// column, edited in `p.tabs` (and the indents in `p.leftIndent`…) until OK.
fn style_tabs(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document) {
    use crate::panels::tabs::{Ruler, RulerEdit, align_buttons, ruler, text_commit};
    use designcraft_engine::cmd::tabs;
    let cur = |d: &Dialog, k: &str| d.fields.get(k).cloned().unwrap_or_else(|| pv[k.trim_start_matches("p.")].clone());
    let mut list: Vec<designcraft_doc::TabStop> = serde_json::from_value(cur(d, "p.tabs")).unwrap_or_default();
    tabs::sort(&mut list);
    let num = |d: &Dialog, k: &str| cur(d, k).as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
    let (left, first, right) = (num(d, "p.leftIndent"), num(d, "p.firstLineIndent"), num(d, "p.rightIndent"));
    let width = doc.page(0).and_then(|p| p.column_rects().first().map(|r| r.width())).filter(|w| *w > 1.0).unwrap_or(468.0);
    let unit = doc.settings.horizontal_units;
    let mut sel = d.fields.get("tabsSelected").and_then(Value::as_u64).map(|i| i as usize).filter(|i| *i < list.len());
    let mut align = d.fields.get("tabsAlign").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let stop = sel.and_then(|i| list.get(i)).cloned();
    // Each control's edit, applied to the list below.
    let mut result: Option<Result<Option<usize>, String>> = None;
    let mut indent: Option<(&str, f64)> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if let Some(a) = align_buttons(ui, lang, stop.as_ref().map_or(align, |s| s.align)) {
            align = a;
            if let Some(i) = sel {
                result = Some(tabs::change(&mut list, i, Some(a), None, None));
            }
        }
        ui.add_space(6.0);
        crate::rtl::label(ui, crate::i18n::tr(lang, "X:"));
        if let Some(x) = crate::widgets::measure(ui, "pstabx", stop.as_ref().map(|s| s.position), unit, 72.0) {
            result = Some(match sel {
                Some(i) => tabs::move_to(&mut list, i, x),
                None => tabs::add(&mut list, designcraft_doc::TabStop { position: x, align, leader: String::new(), align_on: String::new() }),
            });
        }
        crate::rtl::label(ui, crate::i18n::tr(lang, "Leader:"));
        let leader_name = crate::i18n::tr(lang, "Leader:");
        let leader = text_commit(ui, "psleader", stop.as_ref().map_or("", |s| s.leader.as_str()), 36.0, stop.is_some(), leader_name);
        if let (Some(l), Some(i)) = (leader, sel) {
            result = Some(tabs::leader_value(&l, "tabs").map_err(|e| e.to_string()).and_then(|l| tabs::change(&mut list, i, None, Some(l), None)));
        }
        crate::rtl::label(ui, crate::i18n::tr(lang, "Align On:"));
        let on_char = stop.as_ref().is_some_and(|s| s.align == designcraft_doc::TabAlign::Char);
        let align_name = crate::i18n::tr(lang, "Align On:");
        let align_on = text_commit(ui, "psalignon", stop.as_ref().map_or("", |s| s.align_on.as_str()), 18.0, on_char, align_name);
        if let (Some(c), Some(i)) = (align_on, sel) {
            result = Some(tabs::align_on_value(&c, "tabs").map_err(|e| e.to_string()).and_then(|c| tabs::change(&mut list, i, None, None, Some(c))));
        }
        ui.add_space(6.0);
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(lang, "Clear All"))).clicked() {
            list.clear();
            result = Some(Ok(None));
        }
    });
    ui.add_space(6.0);
    let r = Ruler { tabs: &list, left, first, right, width, unit, selected: sel, scale: None };
    match ruler(ui, "style", &r) {
        Some(RulerEdit::Select(i)) => sel = Some(i),
        Some(RulerEdit::Add(x)) => {
            result = Some(tabs::add(&mut list, designcraft_doc::TabStop { position: x, align, leader: String::new(), align_on: String::new() }))
        }
        Some(RulerEdit::Move(i, x)) => result = Some(tabs::move_to(&mut list, i, x)),
        Some(RulerEdit::Remove(i)) => result = Some(tabs::remove(&mut list, i)),
        Some(RulerEdit::Indent(k, x)) => indent = Some((k, x)),
        None => {}
    }
    match result {
        Some(Ok(i)) => {
            sel = i;
            d.fields.insert("p.tabs".into(), serde_json::to_value(&list).unwrap_or_default());
            d.fields.remove("status");
        }
        Some(Err(e)) => {
            d.fields.insert("status".into(), json!(e));
        }
        None => {}
    }
    if let Some((k, x)) = indent {
        d.fields.insert(format!("p.{k}"), json!(x));
    }
    d.fields.insert("tabsSelected".into(), sel.map_or(Value::Null, |i| json!(i)));
    d.fields.insert("tabsAlign".into(), serde_json::to_value(align).unwrap_or_default());
}

/// A paragraph attribute as Paragraph Style Options shows it: the dialog's edit (`p.<key>`), else
/// the style's resolved value in `pv`.
fn para_value(d: &Dialog, pv: &Value, key: &str) -> Value {
    d.fields.get(&format!("p.{key}")).cloned().unwrap_or_else(|| pv.get(key).cloned().unwrap_or(Value::Null))
}

/// Edit paragraph attribute `key`. The value is the whole attribute: `confirm` reads a dotted key
/// as one field of a rule.
fn para_set(d: &mut Dialog, key: &str, v: Value) {
    d.fields.insert(format!("p.{key}"), v);
}

/// A count attribute (lines, columns), rounded and kept in `range`.
fn para_count(ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, range: (u32, u32)) {
    let v = para_value(d, pv, key).as_f64();
    if let Some(n) = crate::widgets::number(ui, &format!("ps{key}"), v, "", 50.0, 0) {
        let n = if n.is_finite() { n.round().clamp(f64::from(range.0), f64::from(range.1)) as u32 } else { range.0 };
        para_set(d, key, json!(n));
    }
}

fn para_check(ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, label: &str) {
    let mut on = para_value(d, pv, key).as_bool().unwrap_or(false);
    if ui.checkbox(&mut on, crate::rtl::widget(ui, label)).changed() {
        para_set(d, key, json!(on));
    }
}

fn para_measure(ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, unit: Unit) {
    let v = para_value(d, pv, key).as_f64();
    if let Some(n) = crate::widgets::measure(ui, &format!("ps{key}"), v, unit, 80.0) {
        para_set(d, key, json!(n));
    }
}

/// A paragraph attribute chosen from `opts` (serialized value, label).
fn para_choice(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, opts: &[(&str, &str)]) {
    para_choice_width(lang, ui, d, pv, key, opts, 200.0);
}

/// [`para_choice`] `width` wide.
fn para_choice_width(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, opts: &[(&str, &str)], width: f32) {
    let cur = para_value(d, pv, key);
    let shown = opts.iter().find(|o| cur == o.0).map_or("", |o| crate::i18n::tr(lang, o.1));
    craft_ui::ComboBox::from_id_salt(("pschoice", key)).selected_text(crate::rtl::widget(ui, shown)).width(width).show_ui(ui, |ui| {
        for (v, label) in opts {
            if ui.selectable_label(cur == *v, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                para_set(d, key, json!(v));
            }
        }
    });
}

/// Four per-side values in the order the attribute holds them: top, left, bottom, right. An
/// edited side stores the whole array.
fn para_sides(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, field: &dyn Fn(&mut egui::Ui, &str, Option<f64>) -> Option<f64>) {
    let mut sides: [f64; 4] = serde_json::from_value(para_value(d, pv, key)).unwrap_or_default();
    let linked = linked(d, key, sides.iter().all(|v| (v - sides[0]).abs() < 1e-9));
    let mut changed = false;
    // As InDesign's: Top and Bottom, the link, Left and Right.
    egui::Grid::new(("pssides", key)).num_columns(5).spacing([8.0, 6.0]).show(ui, |ui| {
        for (i, label) in ["Top:", "Left:", "Bottom:", "Right:"].into_iter().enumerate() {
            crate::rtl::label(ui, crate::i18n::tr(lang, label));
            if let Some(v) = field(ui, &format!("ps{key}{i}"), sides.get(i).copied()) {
                // Linked, a side typed in sets all four.
                if linked {
                    sides = [v; 4];
                } else if let Some(side) = sides.get_mut(i) {
                    *side = v;
                }
                changed = true;
            }
            match i {
                // Linked again, the four take Top's value.
                0 if link_toggle(lang, ui, d, key, linked) => {
                    sides = [sides[0]; 4];
                    changed = true;
                }
                0 => {}
                2 => {
                    ui.label("");
                }
                _ => ui.end_row(),
            }
        }
    });
    if changed {
        para_set(d, key, json!(sides));
    }
}

/// Whether the four values of `key` are linked ("Make all settings the same"): as the user left
/// the link, else when the four are the same, as InDesign starts it.
fn linked(d: &Dialog, key: &str, same: bool) -> bool {
    d.fields.get(&format!("_linked.{key}")).and_then(Value::as_bool).unwrap_or(same)
}

/// The link button between a border's or shading's four sides or corners; true when it was
/// clicked to link them, and the caller makes the four the same.
fn link_toggle(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, key: &str, linked: bool) -> bool {
    let tip = crate::i18n::tr(lang, "Make all settings the same");
    let clicked = crate::widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, tip, egui::vec2(22.0, 21.0)).clicked();
    if clicked {
        d.fields.insert(format!("_linked.{key}"), json!(!linked));
    }
    clicked && !linked
}

/// A swatch menu with a colour chip per swatch (hidden swatches left out); returns the swatch
/// picked.
fn swatch_menu(ui: &mut egui::Ui, doc: &designcraft_doc::Document, id: impl egui::AsIdSalt, current: &str, width: f32) -> Option<String> {
    swatch_menu_with(ui, doc, id, current, width, None)
}

/// [`swatch_menu`] with, given its translated label, Text Color first (a paragraph rule's
/// colour: the text's own), as InDesign's Paragraph Rules › Color.
fn swatch_menu_with(
    ui: &mut egui::Ui,
    doc: &designcraft_doc::Document,
    id: impl egui::AsIdSalt,
    current: &str,
    width: f32,
    text_color: Option<&str>,
) -> Option<String> {
    let mut picked = None;
    let shown = match text_color {
        Some(label) if current == designcraft_doc::TEXT_COLOR => label,
        _ => current,
    };
    craft_ui::ComboBox::from_id_salt(id).selected_text(shown).width(width).show_ui(ui, |ui| {
        if let Some(label) = text_color
            && ui.selectable_label(current == designcraft_doc::TEXT_COLOR, label).clicked()
        {
            picked = Some(designcraft_doc::TEXT_COLOR.to_string());
        }
        for w in doc.swatches.iter().filter(|w| !w.hidden) {
            let (c, g) = crate::widgets::swatch_colors(doc, &w.name, 1.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                crate::widgets::paint_chip(ui.painter(), r, c, g);
                if ui.selectable_label(w.name == current, &w.name).clicked() {
                    picked = Some(w.name.clone());
                }
            });
        }
    });
    picked
}

/// Built-in stroke types of the paragraph border's Type menu (serialized `StrokeType`s).
const BORDER_TYPES: [&str; 11] =
    ["solid", "dashed", "dotted", "thickThin", "thinThick", "thinThin", "thickThick", "thinThickThin", "thickThinThick", "wavy", "hashed"];

/// The border's stroke type: a built-in type or one of the document's stroke styles.
fn border_type_menu(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document) {
    use designcraft_doc::StrokeType;
    let cur: StrokeType = serde_json::from_value(para_value(d, pv, "borderType")).unwrap_or_default();
    let shown = match &cur {
        StrokeType::Style { name } => name.clone(),
        k => crate::i18n::tr(lang, k.label()).to_string(),
    };
    craft_ui::ComboBox::from_id_salt("psbordertype").selected_text(crate::rtl::widget(ui, shown)).width(160.0).show_ui(ui, |ui| {
        for k in BORDER_TYPES {
            let ty = if k == "dashed" { json!({"kind": k, "pattern": []}) } else { json!({"kind": k}) };
            let Ok(t) = serde_json::from_value::<StrokeType>(ty.clone()) else { continue };
            let on = std::mem::discriminant(&t) == std::mem::discriminant(&cur);
            if ui.selectable_label(on, crate::rtl::widget(ui, crate::i18n::tr(lang, t.label()))).clicked() && !on {
                para_set(d, "borderType", ty);
            }
        }
        for st in &doc.stroke_styles {
            let on = matches!(&cur, StrokeType::Style { name } if *name == st.name);
            if ui.selectable_label(on, &st.name).clicked() {
                para_set(d, "borderType", json!({"kind": "style", "name": st.name}));
            }
        }
    });
}

/// Corner size and shape of a border or shading (`key` holds top left, top right, bottom right,
/// bottom left), as InDesign lays them out: Top Left and Top Right over Bottom Left and Bottom
/// Right, each a corner icon, a size and a shape menu showing the shapes' glyphs, with the link in
/// the middle. An edited corner stores all four.
fn para_corners(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, units: Unit) {
    use designcraft_geom::corners::{Corner, CornerShape};
    let mut corners: [Corner; 4] = serde_json::from_value(para_value(d, pv, key)).unwrap_or_default();
    let linked = linked(d, key, corners.iter().all(|c| *c == corners[0]));
    let mut changed = None;
    let mut relinked = false;
    let mut corner = |ui: &mut egui::Ui, i: usize, icon: &str, tip: &str, changed: &mut Option<usize>| {
        let Some(c) = corners.get_mut(i) else { return };
        let tip = crate::i18n::tr(lang, tip);
        let (r, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        crate::icons::paint(ui.painter(), r, icon, crate::theme::Tokens::get(ui.ctx()).icon);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tip));
        resp.on_hover_text(tip);
        let id = format!("ps{key}{i}");
        if let Some(v) = crate::widgets::NumField::measure(&id, Some(c.size), units).width(84.0).spinner().range(0.0, 10_000.0).show(ui) {
            c.size = if v.is_finite() { v.clamp(0.0, 10_000.0) } else { 0.0 };
            *changed = Some(i);
        }
        let shape_label = crate::i18n::tr(lang, "Corner Shape");
        let shown = c.shape;
        let resp = crate::panels::properties::corner_shape_button(ui, shown, 52.0);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, shape_label));
        egui::Popup::menu(&resp).show(|ui| {
            for shape in CornerShape::ALL {
                let row = crate::panels::properties::preview_row(ui, c.shape == shape, crate::i18n::tr(lang, shape.label()), 22.0, |p, r, ink| {
                    crate::panels::properties::paint_corner_shape(p, r, shape, ink)
                });
                if row.clicked() {
                    c.shape = shape;
                    *changed = Some(i);
                    ui.close();
                }
            }
        });
    };
    egui::Grid::new(("pscorners", key)).num_columns(3).spacing([14.0, 6.0]).show(ui, |ui| {
        for (row, [(li, licon, ltip), (ri, ricon, rtip)]) in [
            [(0, "corner-tl", "Top Left Corner"), (1, "corner-tr", "Top Right Corner")],
            [(3, "corner-bl", "Bottom Left Corner"), (2, "corner-br", "Bottom Right Corner")],
        ]
        .into_iter()
        .enumerate()
        {
            ui.horizontal(|ui| corner(ui, li, licon, ltip, &mut changed));
            // Linked again, the four take Top Left's size and shape.
            if row == 0 {
                if link_toggle(lang, ui, d, key, linked) {
                    relinked = true;
                }
            } else {
                ui.label("");
            }
            ui.horizontal(|ui| corner(ui, ri, ricon, rtip, &mut changed));
            ui.end_row();
        }
    });
    if relinked {
        corners = [corners[0]; 4];
        changed = Some(0);
    }
    if let Some(i) = changed {
        // Linked, the corner changed sets all four.
        if linked {
            corners = [corners[i]; 4];
        }
        para_set(d, key, serde_json::to_value(corners).unwrap_or_default());
    }
}

/// Tint presets (%), as InDesign's tint menus list them.
const TINTS: &[f64] = &[0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];

/// The colour and tint rows of a border, its gap or shading (`keys`: colour, tint, overprint), in
/// a grid; Overprint follows the tint, as InDesign's.
#[allow(clippy::too_many_arguments)]
fn para_color_tint(
    lang: &str,
    ui: &mut egui::Ui,
    d: &mut Dialog,
    pv: &Value,
    doc: &designcraft_doc::Document,
    keys: (&str, &str, &str),
    labels: (&str, &str),
    enabled: bool,
) {
    let (color_key, tint_key, overprint_key) = keys;
    crate::rtl::label(ui, labels.0);
    let color = para_value(d, pv, color_key).as_str().unwrap_or("").to_string();
    if let Some(w) = ui.add_enabled_ui(enabled, |ui| swatch_menu(ui, doc, ("pscolor", color_key), &color, 160.0)).inner {
        para_set(d, color_key, json!(w));
    }
    ui.end_row();
    crate::rtl::label(ui, labels.1);
    let tint = para_value(d, pv, tint_key).as_f64().map(|t| (t * 1000.0).round() / 10.0);
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            let id = format!("ps{tint_key}");
            let field = crate::widgets::NumField::number(&id, tint, "%", 0).width(96.0).spinner().presets(TINTS).step(10.0).range(0.0, 100.0);
            if let Some(v) = field.show(ui) {
                para_set(d, tint_key, json!(if v.is_finite() { (v / 100.0).clamp(0.0, 1.0) } else { 1.0 }));
            }
            ui.add_space(8.0);
            para_check(ui, d, pv, overprint_key, crate::i18n::tr(lang, "Overprint"));
        });
    });
    ui.end_row();
}

/// Paragraph Style Options › Paragraph Border / Paragraph Shading (and the Paragraph Borders and
/// Shading dialog): on or off, the stroke (border) or colour (shading), corners, offsets, the
/// edges and width the box follows, and the options for split and consecutive paragraphs.
fn border_shading(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document, border: bool) {
    let key = |field: &str| format!("{}{field}", if border { "border" } else { "shading" });
    let on_key = key("On");
    para_check(ui, d, pv, &on_key, crate::i18n::tr(lang, if border { "Border" } else { "Shading" }));
    let on = para_value(d, pv, &on_key).as_bool().unwrap_or(false);
    let units = doc.settings.horizontal_units;
    let head = |ui: &mut egui::Ui, t: &str| {
        ui.add_space(4.0);
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, t)).font(semibold(12.0)));
    };
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt(("psbsscroll", border)).max_height(400.0).show(ui, |ui| {
        ui.add_enabled_ui(on, |ui| {
            if border {
                head(ui, "Stroke");
                para_sides(lang, ui, d, pv, &key("Weights"), &|ui: &mut egui::Ui, id: &str, v: Option<f64>| {
                    crate::widgets::NumField::number(id, v, " pt", 2)
                        .width(84.0)
                        .spinner()
                        .range(0.0, 1000.0)
                        .show(ui)
                        .map(|w| if w.is_finite() { w.clamp(0.0, 1000.0) } else { 0.0 })
                });
                ui.add_space(4.0);
            } else {
                head(ui, "Color");
            }
            egui::Grid::new(("psbs", border)).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                if border {
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Type:"));
                    border_type_menu(lang, ui, d, pv, doc);
                    ui.end_row();
                }
                let (color, tint) = (crate::i18n::tr(lang, "Color:"), crate::i18n::tr(lang, "Tint:"));
                para_color_tint(lang, ui, d, pv, doc, (&key("Color"), &key("Tint"), &key("Overprint")), (color, tint), true);
                if border {
                    // The gap colour shows between the dashes, dots or stripes.
                    let solid = para_value(d, pv, "borderType").get("kind").and_then(Value::as_str).is_none_or(|k| k == "solid");
                    let (gap, gap_tint) = (crate::i18n::tr(lang, "Gap Color:"), crate::i18n::tr(lang, "Gap Tint:"));
                    para_color_tint(lang, ui, d, pv, doc, ("borderGapColor", "borderGapTint", "borderGapOverprint"), (gap, gap_tint), !solid);
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Cap:"));
                    para_choice(lang, ui, d, pv, "borderCap", &[("butt", "Butt"), ("round", "Round"), ("projecting", "Projecting")]);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Join:"));
                    para_choice(lang, ui, d, pv, "borderJoin", &[("miter", "Miter"), ("round", "Round"), ("bevel", "Bevel")]);
                    ui.end_row();
                }
            });
            head(ui, "Corner Size and Shape");
            para_corners(lang, ui, d, pv, &key("Corners"), units);
            // The offsets, and beside them the edges and width the box follows, as InDesign's.
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    head(ui, "Offsets");
                    para_sides(lang, ui, d, pv, &key("Offsets"), &|ui: &mut egui::Ui, id: &str, v: Option<f64>| {
                        crate::widgets::spin_measure(ui, id, v, units, 72.0)
                    });
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    head(ui, "");
                    egui::Grid::new(("psbsedges", border)).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        crate::rtl::label(ui, crate::i18n::tr(lang, "Top Edge:"));
                        para_choice_width(
                            lang,
                            ui,
                            d,
                            pv,
                            &key("Top"),
                            &[("ascent", "Ascent"), ("baseline", "Baseline"), ("emBox", "Em Box Top"), ("leading", "Leading")],
                            92.0,
                        );
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(lang, "Bottom Edge:"));
                        para_choice_width(
                            lang,
                            ui,
                            d,
                            pv,
                            &key("Bottom"),
                            &[("descent", "Descent"), ("baseline", "Baseline"), ("emBox", "Em Box Bottom")],
                            92.0,
                        );
                        ui.end_row();
                        crate::rtl::label(ui, crate::i18n::tr(lang, "Width:"));
                        para_choice_width(lang, ui, d, pv, &key("Width"), &[("column", "Column"), ("text", "Text")], 92.0);
                        ui.end_row();
                    });
                });
            });
            ui.add_space(4.0);
            if border {
                para_check(ui, d, pv, "borderDisplayIfSplits", crate::i18n::tr(lang, "Display Border if Paragraph Splits Across Frames/Columns"));
                para_check(ui, d, pv, "borderMerge", crate::i18n::tr(lang, "Merge Consecutive Borders and Shading"));
            } else {
                para_check(ui, d, pv, "shadingClip", crate::i18n::tr(lang, "Clip to Frame"));
                para_check(ui, d, pv, "shadingNonprinting", crate::i18n::tr(lang, "Do not Print or Export"));
            }
        });
    });
}

/// The value of dialog field `k`, or `base` when the dialog has not set it.
fn field_or(d: &Dialog, k: &str, base: &Value) -> Value {
    d.fields.get(k).cloned().unwrap_or_else(|| base.clone())
}

/// Drop Caps and Nested Styles: the drop cap, nested styles and nested line styles (`p.` fields).
fn drop_caps_and_nested_styles(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    drop_cap_fields(lang, ui, d, pv, cnames);
    ui.add_space(8.0);
    nested_style_rows(lang, ui, d, pv, cnames);
    ui.add_space(8.0);
    nested_line_style_rows(lang, ui, d, pv, cnames);
}

/// The drop cap's lines, characters, character style and alignment options (`p.` fields).
fn drop_cap_fields(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Drop Caps")).font(semibold(12.0)));
    egui::Grid::new("psdc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (label, key, max) in [
            ("Lines:", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES),
            ("Characters:", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS),
        ] {
            crate::rtl::label(ui, crate::i18n::tr(lang, label));
            let v = field_or(d, &format!("p.{key}"), &pv[key]).as_f64();
            if let Some(n) = crate::widgets::number(ui, &format!("ps{key}"), v, "", 60.0, 0) {
                d.fields.insert(format!("p.{key}"), json!((n.max(0.0) as u64).min(max as u64)));
            }
            ui.end_row();
        }
        crate::rtl::label(ui, crate::i18n::tr(lang, "Character Style:"));
        let style = field_or(d, "p.dropCapStyle", &pv["dropCapStyle"]).as_str().unwrap_or("").to_string();
        let style = if style.is_empty() { designcraft_doc::NO_CHAR_STYLE.to_string() } else { style };
        craft_ui::ComboBox::from_id_salt("ps_dropcap_style")
            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(lang, &style)))
            .width(160.0)
            .show_ui(ui, |ui| {
                for c in cnames {
                    if ui.selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(lang, c))).clicked() {
                        d.fields.insert("p.dropCapStyle".into(), json!(c));
                    }
                }
            });
        ui.end_row();
    });
    for (label, key) in [("Align Left Edge", "dropCapAlignLeft"), ("Scale for Descenders", "dropCapScaleDescenders")] {
        let mut on = field_or(d, &format!("p.{key}"), &pv[key]).as_bool().unwrap_or(false);
        if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).changed() {
            d.fields.insert(format!("p.{key}"), json!(on));
        }
    }
}

/// The nested styles list (`p.nestedStyles`), with its add button.
fn nested_style_rows(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Nested Styles")).font(semibold(12.0)));
    let mut list: Vec<Value> = field_or(d, "p.nestedStyles", &pv["nestedStyles"]).as_array().cloned().unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    for (i, ns) in list.iter_mut().enumerate() {
        let (row_changed, delete) = ui.horizontal(|ui| nested_style_row(lang, ui, ns, i, cnames)).inner;
        changed |= row_changed;
        if delete {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(lang, "New Nested Style"))).clicked() {
        list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "through": true, "count": 1, "until": {"kind": "words"}}));
        changed = true;
    }
    if changed {
        d.fields.insert("p.nestedStyles".into(), Value::Array(list));
    }
}

/// One row of the nested styles list. Returns whether the row changed and whether its delete
/// button was pressed.
fn nested_style_row(lang: &str, ui: &mut egui::Ui, ns: &mut Value, i: usize, cnames: &[String]) -> (bool, bool) {
    let mut changed = false;
    let style = ns["style"].as_str().unwrap_or("[None]").to_string();
    craft_ui::ComboBox::from_id_salt(("ns_style", i))
        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(lang, &style)))
        .width(110.0)
        .show_ui(ui, |ui| {
            for c in cnames {
                if ui.selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(lang, c))).clicked() {
                    ns["style"] = json!(c);
                    changed = true;
                }
            }
        });
    let through = ns["through"].as_bool().unwrap_or(true);
    craft_ui::ComboBox::from_id_salt(("ns_thr", i))
        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, if through { "through" } else { "up to" })))
        .width(70.0)
        .show_ui(ui, |ui| {
            for (v, l) in [(true, "through"), (false, "up to")] {
                if ui.selectable_label(v == through, crate::rtl::widget(ui, crate::i18n::tr(lang, l))).clicked() {
                    ns["through"] = json!(v);
                    changed = true;
                }
            }
        });
    let mut n = ns["count"].as_u64().unwrap_or(1) as u32;
    let mut count_value = |ui: &mut egui::Ui, ns: &mut Value, changed: &mut bool| {
        if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
            ns["count"] = json!(n);
            *changed = true;
        }
    };
    if !crate::i18n::category_first(lang) {
        count_value(ui, ns, &mut changed);
    }
    let kind = ns["until"]["kind"].as_str().unwrap_or("words").to_string();
    craft_ui::ComboBox::from_id_salt(("ns_until", i))
        .selected_text(crate::rtl::widget(
            ui,
            if crate::i18n::category_first(lang) { format!("{}:", crate::i18n::tr(lang, &kind)) } else { crate::i18n::tr(lang, &kind).to_owned() },
        ))
        .width(100.0)
        .show_ui(ui, |ui| {
            for k in ["sentences", "words", "characters", "letters", "digits", "tab", "forcedLineBreak", "emSpace", "enSpace", "dropcap", "chars"] {
                if ui.selectable_label(k == kind, crate::rtl::widget(ui, crate::i18n::tr(lang, k))).clicked() {
                    ns["until"] = if k == "chars" { json!({"kind": "chars", "chars": ":"}) } else { json!({"kind": k}) };
                    changed = true;
                }
            }
        });
    if crate::i18n::category_first(lang) {
        count_value(ui, ns, &mut changed);
    }
    if kind == "chars" {
        let mut c = ns["until"]["chars"].as_str().unwrap_or("").to_string();
        if ui.add(egui::TextEdit::singleline(&mut c).desired_width(40.0)).changed() {
            ns["until"]["chars"] = json!(c);
            changed = true;
        }
    }
    let delete = ui
        .small_button("×")
        .on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(lang, "Delete"));
        })
        .clicked();
    (changed, delete)
}

/// The nested line styles list (`p.nestedLineStyles`), with its add button.
fn nested_line_style_rows(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Nested Line Styles")).font(semibold(12.0)));
    let mut lines: Vec<Value> = field_or(d, "p.nestedLineStyles", &pv["nestedLineStyles"]).as_array().cloned().unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    for (i, l) in lines.iter_mut().enumerate() {
        let (row_changed, delete) = ui.horizontal(|ui| nested_line_style_row(lang, ui, l, i, cnames)).inner;
        changed |= row_changed;
        if delete {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        lines.remove(i);
        changed = true;
    }
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(lang, "New Line Style"))).clicked() {
        lines.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "lines": 1}));
        changed = true;
    }
    if changed {
        d.fields.insert("p.nestedLineStyles".into(), Value::Array(lines));
    }
}

/// One row of the nested line styles list. Returns whether the row changed and whether its delete
/// button was pressed.
fn nested_line_style_row(lang: &str, ui: &mut egui::Ui, l: &mut Value, i: usize, cnames: &[String]) -> (bool, bool) {
    let mut changed = false;
    let style = l["style"].as_str().unwrap_or("").to_string();
    craft_ui::ComboBox::from_id_salt(("nls_style", i))
        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(lang, &style)))
        .width(130.0)
        .show_ui(ui, |ui| {
            for c in cnames {
                if ui.selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(lang, c))).clicked() {
                    l["style"] = json!(c);
                    changed = true;
                }
            }
        });
    crate::rtl::label(
        ui,
        if crate::i18n::category_first(lang) { format!("{}:", crate::i18n::tr(lang, "lines")) } else { crate::i18n::tr(lang, "for").to_owned() },
    );
    let mut n = l["lines"].as_u64().unwrap_or(1) as u32;
    if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
        l["lines"] = json!(n);
        changed = true;
    }
    if !crate::i18n::category_first(lang) {
        crate::rtl::label(ui, crate::i18n::tr(lang, if n == 1 { "line" } else { "lines" }));
    }
    let delete = ui
        .small_button("×")
        .on_hover_ui(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(lang, "Delete"));
        })
        .clicked();
    (changed, delete)
}

/// GREP Styles: a character style applied to text matching each pattern (`p.` fields).
fn grep_styles(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    let cur = |d: &Dialog, k: &str, base: &Value| d.fields.get(k).cloned().unwrap_or_else(|| base.clone());
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "GREP Styles")).font(semibold(12.0)));
    let mut list: Vec<Value> = cur(d, "p.grepStyles", &pv["grepStyles"]).as_array().cloned().unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    for (i, g) in list.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            crate::rtl::label(ui, crate::i18n::tr(lang, "Apply Style:"));
            let style = g["style"].as_str().unwrap_or("").to_string();
            craft_ui::ComboBox::from_id_salt(("gs_style", i))
                .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(lang, &style)))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for c in cnames {
                        if ui.selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(lang, c))).clicked() {
                            g["style"] = json!(c);
                            changed = true;
                        }
                    }
                });
            crate::rtl::label(ui, crate::i18n::tr(lang, "To Text:"));
            let mut pat = g["pattern"].as_str().unwrap_or("").to_string();
            let bad = grep_error(&pat).is_some();
            let r =
                ui.add(egui::TextEdit::singleline(&mut pat).desired_width(150.0).text_color_opt(bad.then_some(egui::Color32::from_rgb(240, 90, 90))));
            if r.changed() {
                g["pattern"] = json!(pat);
                changed = true;
            }
            if ui
                .small_button("×")
                .on_hover_ui(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Delete"));
                })
                .clicked()
            {
                remove = Some(i);
            }
        });
        if let Some(e) = grep_error(g["pattern"].as_str().unwrap_or("")) {
            crate::rtl::label(ui, egui::RichText::new(e).size(11.0).color(egui::Color32::from_rgb(240, 90, 90)));
        }
    }
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(lang, "New GREP Style"))).clicked() {
        list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "pattern": "\\d+"}));
        changed = true;
    }
    if changed {
        d.fields.insert("p.grepStyles".into(), Value::Array(list));
    }
}

/// Paragraph Style Options › Keep Options: lines kept with the next paragraph, the paragraph's
/// lines kept together (all of them, or its first and last lines), and where it starts.
fn keep_options(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value) {
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Keep with Next:"));
        para_count(ui, d, pv, "keepWithNext", (0, 5));
        crate::rtl::label(ui, crate::i18n::tr(lang, "lines"));
    });
    ui.add_space(4.0);
    para_check(ui, d, pv, "keepLinesTogether", crate::i18n::tr(lang, "Keep Lines Together"));
    let together = para_value(d, pv, "keepLinesTogether").as_bool().unwrap_or(false);
    ui.add_enabled_ui(together, |ui| {
        ui.indent("pskeepall", |ui| {
            let all = para_value(d, pv, "keepAllLines").as_bool().unwrap_or(true);
            if ui.radio(all, crate::rtl::widget(ui, crate::i18n::tr(lang, "All Lines in Paragraph"))).clicked() {
                para_set(d, "keepAllLines", json!(true));
            }
            if ui.radio(!all, crate::rtl::widget(ui, crate::i18n::tr(lang, "At Start/End of Paragraph"))).clicked() {
                para_set(d, "keepAllLines", json!(false));
            }
            ui.add_enabled_ui(!all, |ui| {
                egui::Grid::new("pskeepends").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                    for (label, key) in [("Start:", "keepFirst"), ("End:", "keepLast")] {
                        crate::rtl::label(ui, crate::i18n::tr(lang, label));
                        para_count(ui, d, pv, key, (1, 50));
                        crate::rtl::label(ui, crate::i18n::tr(lang, "lines"));
                        ui.end_row();
                    }
                });
            });
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Start Paragraph:"));
        para_choice(
            lang,
            ui,
            d,
            pv,
            "startParagraph",
            &[
                ("anywhere", "Anywhere"),
                ("nextColumn", "In Next Column"),
                ("nextFrame", "In Next Frame"),
                ("nextPage", "On Next Page"),
                ("nextOddPage", "On Next Odd Page"),
                ("nextEvenPage", "On Next Even Page"),
            ],
        );
    });
}

/// Paragraph Style Options › Span Columns (and the Span Columns dialog): one column, spanning
/// columns (0 = all), or split into sub-columns with inside and outside gutters; the least space
/// before and after a spanning or split paragraph.
fn span_columns(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document) {
    use designcraft_doc::SpanColumns;
    let cur: SpanColumns = serde_json::from_value(para_value(d, pv, "spanColumns")).unwrap_or_default();
    let same = |a: &SpanColumns, b: &SpanColumns| std::mem::discriminant(a) == std::mem::discriminant(b);
    let units = doc.settings.horizontal_units;
    let mut next = None;
    egui::Grid::new("psspan").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Paragraph Layout:"));
        let kinds = [("Single Column", SpanColumns::Single), ("Span Columns", SpanColumns::Span(0)), ("Split Column", SpanColumns::Split(2))];
        // "Span Columns" names both the section and this option, which some languages word differently.
        let kind_label = |k: &'static str| -> &'static str { crate::i18n::tr_context(lang, k, "paragraphLayout") };
        let shown = kinds.iter().find(|k| same(&k.1, &cur)).map_or("", |k| kind_label(k.0));
        craft_ui::ComboBox::from_id_salt("psspankind").selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
            for (label, v) in kinds {
                if ui.selectable_label(same(&v, &cur), crate::rtl::widget(ui, kind_label(label))).clicked() && !same(&v, &cur) {
                    next = Some(v);
                }
            }
        });
        ui.end_row();
        match cur {
            SpanColumns::Single => {}
            SpanColumns::Span(n) => {
                crate::rtl::label(ui, crate::i18n::tr(lang, "Columns:"));
                let shown = if n == 0 { crate::i18n::tr(lang, "All").to_string() } else { n.to_string() };
                craft_ui::ComboBox::from_id_salt("psspann").selected_text(crate::rtl::widget(ui, shown)).width(80.0).show_ui(ui, |ui| {
                    if ui.selectable_label(n == 0, crate::rtl::widget(ui, crate::i18n::tr(lang, "All"))).clicked() {
                        next = Some(SpanColumns::Span(0));
                    }
                    for c in 2..=20u32 {
                        if ui.selectable_label(n == c, c.to_string()).clicked() {
                            next = Some(SpanColumns::Span(c));
                        }
                    }
                });
                ui.end_row();
            }
            SpanColumns::Split(n) => {
                crate::rtl::label(ui, crate::i18n::tr(lang, "Columns:"));
                if let Some(c) = crate::widgets::number(ui, "pssplitn", Some(f64::from(n)), "", 50.0, 0) {
                    next = Some(SpanColumns::Split(if c.is_finite() { c.round().clamp(2.0, 20.0) as u32 } else { 2 }));
                }
                ui.end_row();
            }
        }
        let v_units = doc.settings.vertical_units;
        let spaces = match cur {
            SpanColumns::Single => None,
            SpanColumns::Span(_) => Some(("Space Before Span:", "Space After Span:")),
            SpanColumns::Split(_) => Some(("Space Before Split:", "Space After Split:")),
        };
        if let Some((before, after)) = spaces {
            for (label, key) in [(before, "spanSpaceBefore"), (after, "spanSpaceAfter")] {
                crate::rtl::label(ui, crate::i18n::tr(lang, label));
                para_measure(ui, d, pv, key, v_units);
                ui.end_row();
            }
        }
        if matches!(cur, SpanColumns::Split(_)) {
            for (label, key) in [("Inside Gutter:", "splitInsideGutter"), ("Outside Gutter:", "splitOutsideGutter")] {
                crate::rtl::label(ui, crate::i18n::tr(lang, label));
                para_measure(ui, d, pv, key, units);
                ui.end_row();
            }
        }
    });
    if let Some(v) = next {
        para_set(d, "spanColumns", serde_json::to_value(v).unwrap_or_default());
    }
}

/// Number styles as the list's Format menu shows them (`NumberStyle` values).
const NUMBER_STYLES: [(&str, &str); 9] = [
    ("arabic", "1, 2, 3, 4..."),
    ("upperRoman", "I, II, III, IV..."),
    ("lowerRoman", "i, ii, iii, iv..."),
    ("upperLetters", "A, B, C, D..."),
    ("lowerLetters", "a, b, c, d..."),
    ("arabicLeadingZero", "01, 02, 03..."),
    ("arabicThreeDigits", "001, 002, 003..."),
    ("arabicFourDigits", "0001, 0002, 0003..."),
    ("symbols", "*, †, ‡, §..."),
];

/// A list label text (Text After, Number), shown with its tab as `^t`; labels expand
/// metacharacters, so the typed text is stored as it is.
fn list_text(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, label: &str, key: &str) {
    crate::rtl::label(ui, crate::i18n::tr(lang, label));
    let mut s = para_value(d, pv, key).as_str().unwrap_or("").replace('\t', "^t");
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(160.0)).changed() {
        para_set(d, key, json!(s));
    }
    ui.end_row();
}

/// What the Bullets and Numbering section offers besides the paragraph's attributes: the
/// document's character styles, the font families for the bullet, the fonts, and the paragraph's
/// font (the bullet's when it has none of its own).
struct ListUi<'a> {
    char_styles: &'a [String],
    families: &'a [String],
    fonts: &'a designcraft_fonts::ScopedFonts<'static>,
    scope: u32,
    family: &'a str,
    style: &'a str,
}

impl ListUi<'_> {
    fn new_parts(app: &DesignApp) -> (Vec<String>, Vec<String>, designcraft_fonts::ScopedFonts<'static>, u32) {
        let styles = app.session.active().map(|s| s.doc.styles.character.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        let families = crate::panels::font_menu(app).into_iter().map(|f| f.family).collect();
        (styles, families, crate::panels::fonts(app), crate::panels::font_scope(app))
    }
}

/// A character style for a bullet or number (`key`), [None] first.
fn list_char_style(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, styles: &[String]) {
    let cur = para_value(d, pv, key).as_str().unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_string();
    craft_ui::ComboBox::from_id_salt(("pslistcs", key))
        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(lang, &cur)))
        .width(200.0)
        .show_ui(ui, |ui| {
            for n in std::iter::once(designcraft_doc::NO_CHAR_STYLE)
                .chain(styles.iter().map(String::as_str).filter(|n| *n != designcraft_doc::NO_CHAR_STYLE))
            {
                if ui.selectable_label(n == cur, crate::rtl::widget(ui, crate::i18n::style_name(lang, n))).clicked() {
                    para_set(d, key, json!(n));
                }
            }
        });
}

/// The bullet character: common bullets, a field, and the font's glyphs in a grid to pick from.
fn bullet_char(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, lu: &ListUi<'_>) {
    crate::rtl::label(ui, crate::i18n::tr(lang, "Bullet Character:"));
    ui.horizontal(|ui| {
        let cur = para_value(d, pv, "bulletChar").as_str().unwrap_or("").to_string();
        for b in ["\u{2022}", "\u{2013}", "\u{2014}", "\u{00BB}", "*"] {
            if ui.add_sized([24.0, 20.0], egui::Button::selectable(cur == b, b)).clicked() {
                para_set(d, "bulletChar", json!(b));
            }
        }
        let mut s = cur;
        if ui.add(egui::TextEdit::singleline(&mut s).desired_width(36.0)).changed() {
            para_set(d, "bulletChar", json!(s));
        }
        let open = d.b("bulletGlyphs");
        if ui.selectable_label(open, crate::rtl::widget(ui, crate::i18n::tr(lang, "Glyphs…"))).clicked() {
            d.fields.insert("bulletGlyphs".into(), json!(!open));
        }
    });
    ui.end_row();
    if d.b("bulletGlyphs") {
        ui.label("");
        ui.vertical(|ui| {
            ui.set_width(300.0);
            let font = para_value(d, pv, "bulletFont").as_str().unwrap_or("").to_string();
            let (family, style) = if font.is_empty() {
                (lu.family.to_string(), lu.style.to_string())
            } else {
                let st = para_value(d, pv, "bulletFontStyle").as_str().unwrap_or("").to_string();
                let st = if st.is_empty() { lu.fonts.styles(&font).into_iter().next().unwrap_or_else(|| "Regular".into()) } else { st };
                (font, st)
            };
            let mut q = d.s("bulletGlyphQuery");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut q)
                        .hint_text(crate::rtl::widget(ui, crate::i18n::tr(lang, "Search: character or U+code")))
                        .desired_width(300.0),
                )
                .changed()
            {
                d.fields.insert("bulletGlyphQuery".into(), json!(q));
            }
            if let (Some(c), _) = crate::panels::glyphs::grid(ui, lu.fonts, lu.scope, &family, &style, &q, 160.0) {
                para_set(d, "bulletChar", json!(c.to_string()));
            }
        });
        ui.end_row();
    }
}

/// The bullet's font family and style ("" = the paragraph's).
fn bullet_font(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, lu: &ListUi<'_>) {
    let font = para_value(d, pv, "bulletFont").as_str().unwrap_or("").to_string();
    let inherit = crate::i18n::tr(lang, "(Paragraph Font)");
    crate::rtl::label(ui, crate::i18n::tr(lang, "Font Family:"));
    craft_ui::ComboBox::from_id_salt("psbulletfont").selected_text(if font.is_empty() { inherit } else { font.as_str() }).width(200.0).show_ui(
        ui,
        |ui| {
            if ui.selectable_label(font.is_empty(), inherit).clicked() {
                para_set(d, "bulletFont", json!(""));
                para_set(d, "bulletFontStyle", json!(""));
            }
            for f in lu.families {
                if ui.selectable_label(*f == font, f).clicked() {
                    para_set(d, "bulletFont", json!(f));
                    para_set(d, "bulletFontStyle", json!(lu.fonts.styles(f).into_iter().next().unwrap_or_default()));
                }
            }
        },
    );
    ui.end_row();
    crate::rtl::label(ui, crate::i18n::tr(lang, "Font Style:"));
    let style = para_value(d, pv, "bulletFontStyle").as_str().unwrap_or("").to_string();
    ui.add_enabled_ui(!font.is_empty(), |ui| {
        craft_ui::ComboBox::from_id_salt("psbulletstyle").selected_text(style.as_str()).width(200.0).show_ui(ui, |ui| {
            for st in lu.fonts.styles(&font) {
                if ui.selectable_label(st == style, &st).clicked() {
                    para_set(d, "bulletFontStyle", json!(st));
                }
            }
        });
    });
    ui.end_row();
}

/// Paragraph Style Options › Bullets and Numbering (and the Bullets and Numbering dialog): the
/// list type and level; the bullet, its font and the text after it, or the list, number style,
/// number text and where numbering starts and restarts; the label's character style; then where
/// the label goes (alignment, indents and the tab position of the text after it).
fn bullets_and_numbering(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document, lu: &ListUi<'_>) {
    let units = doc.settings.horizontal_units;
    let kind = para_value(d, pv, "listType").as_str().unwrap_or("none").to_string();
    let level = para_value(d, pv, "listLevel").as_u64().unwrap_or(1).clamp(1, designcraft_doc::LIST_LEVELS as u64) as u32;
    egui::ScrollArea::vertical().id_salt("psbnscroll").max_height(340.0).show(ui, |ui| {
        egui::Grid::new("psbn").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(lang, "List Type:"));
            para_choice(lang, ui, d, pv, "listType", &[("none", "None"), ("bullets", "Bullets"), ("numbers", "Numbers")]);
            ui.end_row();
            if kind != "none" {
                crate::rtl::label(ui, crate::i18n::tr(lang, "Level:"));
                para_count(ui, d, pv, "listLevel", (1, designcraft_doc::LIST_LEVELS as u32));
                ui.end_row();
            }
            match kind.as_str() {
                "bullets" => {
                    bullet_char(lang, ui, d, pv, lu);
                    bullet_font(lang, ui, d, pv, lu);
                    list_text(lang, ui, d, pv, "Text After:", "listSeparator");
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Character Style:"));
                    list_char_style(lang, ui, d, pv, "bulletCharStyle", lu.char_styles);
                    ui.end_row();
                }
                "numbers" => {
                    crate::rtl::label(ui, crate::i18n::tr(lang, "List:"));
                    let list = para_value(d, pv, "listName").as_str().unwrap_or("").to_string();
                    // "" is the story's own list.
                    let name = |n: &str| if n.is_empty() { crate::i18n::tr(lang, "[Default]").to_string() } else { n.to_string() };
                    craft_ui::ComboBox::from_id_salt("pslist").selected_text(name(&list)).width(200.0).show_ui(ui, |ui| {
                        for n in std::iter::once("").chain(doc.settings.lists.iter().map(|l| l.name.as_str())) {
                            if ui.selectable_label(n == list, name(n)).clicked() {
                                para_set(d, "listName", json!(n));
                            }
                        }
                    });
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Format:"));
                    para_choice(lang, ui, d, pv, "numberStyle", &NUMBER_STYLES);
                    ui.end_row();
                    list_text(lang, ui, d, pv, "Number:", "numberExpression");
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Character Style:"));
                    list_char_style(lang, ui, d, pv, "numberCharStyle", lu.char_styles);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(lang, "Mode:"));
                    let start = para_value(d, pv, "startAt").as_u64();
                    ui.horizontal(|ui| {
                        let shown = crate::i18n::tr(lang, if start.is_some() { "Start At" } else { "Continue from Previous Number" });
                        craft_ui::ComboBox::from_id_salt("psmode").selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
                            if ui
                                .selectable_label(start.is_none(), crate::rtl::widget(ui, crate::i18n::tr(lang, "Continue from Previous Number")))
                                .clicked()
                            {
                                para_set(d, "startAt", Value::Null);
                            }
                            if ui.selectable_label(start.is_some(), crate::rtl::widget(ui, crate::i18n::tr(lang, "Start At"))).clicked()
                                && start.is_none()
                            {
                                para_set(d, "startAt", json!(1));
                            }
                        });
                        if let Some(n) = start
                            && let Some(v) = crate::widgets::number(ui, "psstartat", Some(n as f64), "", 60.0, 0)
                        {
                            para_set(d, "startAt", json!(if v.is_finite() { v.round().clamp(0.0, 1_000_000.0) as u32 } else { 1 }));
                        }
                    });
                    ui.end_row();
                    // Restart Numbers at This Level After: a higher level (any, or one of them).
                    ui.label("");
                    ui.add_enabled_ui(level > 1, |ui| {
                        ui.horizontal(|ui| {
                            para_check(ui, d, pv, "restartNumbers", crate::i18n::tr(lang, "Restart Numbers at This Level After:"));
                            let after = para_value(d, pv, "restartAfterLevel").as_u64().unwrap_or(0) as u32;
                            let label = |n: u32| if n == 0 { crate::i18n::tr(lang, "Any Previous Level").to_string() } else { n.to_string() };
                            craft_ui::ComboBox::from_id_salt("psrestartafter")
                                .selected_text(crate::rtl::widget(ui, label(after)))
                                .width(140.0)
                                .show_ui(ui, |ui| {
                                    for n in 0..level {
                                        if ui.selectable_label(n == after, crate::rtl::widget(ui, label(n))).clicked() {
                                            para_set(d, "restartAfterLevel", json!(n));
                                        }
                                    }
                                });
                        });
                    });
                    ui.end_row();
                }
                _ => {}
            }
            if kind != "none" {
                crate::rtl::label(ui, crate::i18n::tr(lang, "Alignment:"));
                let key = if kind == "bullets" { "bulletAlign" } else { "numberAlign" };
                para_choice(lang, ui, d, pv, key, &[("left", "Left"), ("center", "Center"), ("right", "Right")]);
                ui.end_row();
                for (label, key) in [("Left Indent:", "leftIndent"), ("First Line Indent:", "firstLineIndent")] {
                    crate::rtl::label(ui, crate::i18n::tr(lang, label));
                    para_measure(ui, d, pv, key, units);
                    ui.end_row();
                }
                // Tab Position: none, or a stop for the text after the label.
                crate::rtl::label(ui, crate::i18n::tr(lang, "Tab Position:"));
                ui.horizontal(|ui| {
                    let tab = para_value(d, pv, "listTab").as_f64();
                    let mut on = tab.is_some();
                    if ui.checkbox(&mut on, "").changed() {
                        let left = para_value(d, pv, "leftIndent").as_f64().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(18.0);
                        para_set(d, "listTab", if on { json!(left) } else { Value::Null });
                    }
                    if let Some(t) = tab
                        && let Some(v) = crate::widgets::measure(ui, "pslisttab", Some(t), units, 80.0)
                    {
                        para_set(d, "listTab", json!(if v.is_finite() { v.max(0.0) } else { 0.0 }));
                    }
                });
                ui.end_row();
            }
        });
    });
}

/// Paragraph Borders and Shading, Span Columns, and Bullets and Numbering for the selected
/// paragraphs: the Paragraph Style Options sections over the paragraphs' attributes (`current`);
/// OK applies the edits (`p.<key>`) as local formatting.
fn paragraph_local(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let Some(doc) = app.session.active().map(|s| s.doc.clone()) else { return };
    let pv = d.fields.get("current").cloned().unwrap_or_default();
    let lang = app.ui.language.clone();
    ui.set_min_width(480.0);
    match d.s("section").as_str() {
        "span" => span_columns(&lang, ui, d, &pv, &doc),
        "keep" => keep_options(&lang, ui, d, &pv),
        "nested" | "grep" => {
            let cnames: Vec<String> = doc.styles.character.iter().map(|c| c.name.clone()).collect();
            if d.s("section") == "nested" {
                drop_caps_and_nested_styles(&lang, ui, d, &pv, &cnames);
            } else {
                grep_styles(&lang, ui, d, &pv, &cnames);
            }
        }
        "bullets" => {
            let (char_styles, families, fonts, scope) = ListUi::new_parts(app);
            let (family, style) = (d.s("family"), d.s("style"));
            let lu = ListUi { char_styles: &char_styles, families: &families, fonts: &fonts, scope, family: &family, style: &style };
            bullets_and_numbering(&lang, ui, d, &pv, &doc, &lu);
        }
        section => {
            let border = section != "shading";
            ui.horizontal(|ui| {
                for (tab, label) in [("border", "Border"), ("shading", "Shading")] {
                    if ui.selectable_label(section == tab, crate::rtl::widget(ui, crate::i18n::tr(&lang, label))).clicked() {
                        d.fields.insert("section".into(), json!(tab));
                    }
                }
            });
            ui.separator();
            border_shading(&lang, ui, d, &pv, &doc, border);
        }
    }
}

fn section_list(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cjk: bool, sections: &[(&str, &str)]) {
    let sections = style_cjk::sections(cjk, d.id == "paragraphStyleOptions", sections);
    ui.vertical(|ui| {
        ui.set_width(170.0);
        // The dialog has a fixed height; a long list scrolls.
        egui::ScrollArea::vertical().id_salt("style_sections").auto_shrink([false, true]).show(ui, |ui| {
            for (id, label) in &sections {
                if ui.selectable_label(d.s("section") == *id, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                    d.fields.insert("section".into(), json!(id));
                }
            }
        });
    });
}

/// Preferences with no document open: the Units & Increments new documents start with, as
/// `prefs.set`'s `defaultUnits` (None with a document open). An increment that doesn't read is an
/// error naming the field.
fn default_units_of(d: &Dialog) -> Result<Option<Value>, String> {
    if !d.b("defaults") {
        return Ok(None);
    }
    let mut du = json!({});
    for k in ["horizontalUnits", "verticalUnits", "rulerOrigin"] {
        if let Some(v) = d.fields.get(k).filter(|v| v.is_string()) {
            du[k] = v.clone();
        }
    }
    if let Some(t) = d.fields.get("keyboardIncrement").and_then(Value::as_str) {
        du["keyboardIncrement"] = json!(parse_measure(t, Unit::Points).map_err(|_| format!("Cursor Key: `{t}` is not a measurement"))?);
    }
    if let Some(v) = d.fields.get("kerningIncrement").filter(|v| !v.is_null()) {
        let n = d.n("kerningIncrement").filter(|v| v.is_finite());
        du["kerningIncrement"] = json!(n.ok_or_else(|| format!("Kerning/Tracking: `{}` is not a number", v.as_str().unwrap_or_default()))?);
    }
    Ok(Some(du))
}

/// "Paragraph Style n" / "Character Style n": the first name no style has.
fn next_style_name(styles: &designcraft_doc::Styles, para: bool) -> String {
    let (base, count) = if para { ("Paragraph Style", styles.paragraph.len()) } else { ("Character Style", styles.character.len()) };
    let taken = |n: &str| if para { styles.para(n).is_some() } else { styles.char_style(n).is_some() };
    // Among count + 1 numbers at least one is free.
    (1..=count + 1).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap_or_else(|| format!("{base} {}", count + 1))
}

/// Paragraph or Character Style Options for an existing style (a double-click on it in the styles
/// panel, or Edit in its menu). [None] and [No Paragraph Style] have no options.
pub fn open_style_options(app: &mut DesignApp, para: bool, name: &str) {
    if name == if para { designcraft_doc::NO_PARA_STYLE } else { designcraft_doc::NO_CHAR_STYLE } {
        return;
    }
    let id = if para { "paragraphStyleOptions" } else { "characterStyleOptions" };
    app.ui.dialog = Some(Dialog::new(id, json!({"name": name, "preview": app.ui.style_preview})));
}

/// New Paragraph Style… / New Character Style…: the style options for a style made on OK, named
/// with the next free "Paragraph Style n" / "Character Style n".
pub fn open_new_style(app: &mut DesignApp, para: bool) {
    let Some(st) = app.session.active() else { return };
    let name = next_style_name(&st.doc.styles, para);
    let has_text = st.selection.text.is_some();
    let (bp, bc) = st.doc.styles.resolve_para_style(designcraft_doc::NO_PARA_STYLE);
    let id = if para { "paragraphStyleOptions" } else { "characterStyleOptions" };
    let mut fields = json!({"new": true, "rename": name});
    // New Paragraph Style with text selected starts from it, as InDesign's: Based On [No Paragraph
    // Style] and every setting of the text that differs, applied to the selection on OK.
    if para
        && has_text
        && let Some(sel) = app.session.execute("type.selectionAttrs", &json!({})).ok().filter(|v| !v.is_null())
    {
        for (prefix, now, base) in
            [("p.", &sel["para"], serde_json::to_value(bp).unwrap_or_default()), ("c.", &sel["chars"], serde_json::to_value(bc).unwrap_or_default())]
        {
            for (k, v) in now.as_object().into_iter().flatten() {
                if !v.is_null() && base.get(k) != Some(v) && base.get(k).is_some() {
                    fields[format!("{prefix}{k}")] = v.clone();
                }
            }
        }
        fields["apply"] = json!(true);
    }
    // New Character Style with text selected starts from what the text has of its own over its
    // paragraph ("[None] + Bold + color"), applied to the selection on OK, as InDesign's.
    if !para
        && has_text
        && let Some(sel) = app.session.execute("type.selectionAttrs", &json!({})).ok().filter(|v| !v.is_null())
    {
        for (k, v) in sel["ownChars"].as_object().into_iter().flatten().filter(|(_, v)| !v.is_null()) {
            fields[format!("c.{k}")] = v.clone();
        }
        fields["apply"] = json!(true);
    }
    app.ui.dialog = Some(Dialog::new(id, fields));
}

/// The General section of the Character Style Options dialog: name, Based On, and a summary of
/// the style's settings.
fn character_style_general(
    lang: &str,
    ui: &mut egui::Ui,
    d: &mut Dialog,
    styles: &designcraft_doc::Styles,
    style: &designcraft_doc::CharacterStyle,
    based: &str,
) {
    let (name, new) = (d.s("name"), d.b("new"));
    egui::Grid::new("csg").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Style Name:"));
        text_field_named(ui, d, "rename", 220.0, crate::i18n::tr(lang, "Style Name"));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Based On:"));
        let combo = craft_ui::ComboBox::from_id_salt("csbased")
            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(lang, based)))
            .width(220.0)
            .show_ui(ui, |ui| {
                // Not itself, nor a style based on it.
                for s in styles.character.iter().filter(|s| new || !styles.char_based_on_cycles(&name, &s.name)) {
                    if ui.selectable_label(s.name == based, crate::rtl::widget(ui, crate::i18n::style_name(lang, &s.name))).clicked() {
                        d.fields.insert("basedOn".into(), json!(s.name));
                    }
                }
            });
        let _ = crate::widgets::name_combo(combo, crate::i18n::tr(lang, "Based On"));
        ui.end_row();
    });
    let status = d.s("status");
    if !status.is_empty() {
        crate::rtl::label(ui, egui::RichText::new(status).color(crate::theme::Tokens::get(ui.ctx()).text_strong));
    }
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Style Settings:")).font(semibold(12.0)));
    // What OK would leave the style setting, after its Based On.
    let mut own = serde_json::to_value(&style.chars).unwrap_or_default();
    if let Some(o) = own.as_object_mut() {
        for (k, v) in d.fields.iter().filter_map(|(k, v)| Some((k.strip_prefix("c.")?, v))) {
            if v.is_null() {
                o.remove(k);
            } else {
                o.insert(k.to_string(), v.clone());
            }
        }
    }
    let mut parts = vec![crate::i18n::style_name(lang, based).to_string()];
    parts.extend(own.as_object().into_iter().flatten().map(|(k, v)| format!("{}: {}", setting_label(lang, k), setting_text(v))));
    ui.add(egui::Label::new(egui::RichText::new(parts.join(" + ")).color(crate::theme::Tokens::get(ui.ctx()).text_dim)).wrap());
}

/// Character Style Options: General, then the character sections it shares with Paragraph Style
/// Options. A character style sets only some attributes: the fields show those it and its Based
/// On set and leave the rest blank. Edits are `c.<attr>` fields (`null` = no longer set), applied
/// on OK; `new` makes the style instead of editing `name`.
fn character_style_options(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let name = d.s("name");
    let new = d.b("new");
    let Some(doc) = app.session.active().map(|st| st.doc.clone()) else { return };
    let styles = &doc.styles;
    let style = match styles.char_style(&name) {
        _ if new => designcraft_doc::CharacterStyle { name: String::new(), based_on: None, chars: Default::default(), shortcut: String::new() },
        Some(s) if name != designcraft_doc::NO_CHAR_STYLE => s.clone(),
        _ => {
            crate::rtl::label(ui, format!("No style named {name}"));
            return;
        }
    };
    if !d.fields.contains_key("rename") {
        d.fields.insert("rename".into(), json!(if new { next_style_name(styles, false) } else { name.clone() }));
    }
    let based = d
        .fields
        .get("basedOn")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(style.based_on.clone())
        .unwrap_or_else(|| designcraft_doc::NO_CHAR_STYLE.into());
    // The fields show the style's own attributes over those its Based On gives it.
    let mut shown = styles.char_style_attrs(&based);
    shown.merge(&style.chars);
    let base = serde_json::to_value(&shown).unwrap_or_default();
    let cf = CharFields { base: &base, sparse: true };
    let lang = app.ui.language.clone();
    ui.set_min_width(560.0);
    ui.horizontal_top(|ui| {
        // A fixed height: the separator would otherwise stretch the dialog to the window.
        ui.set_min_height(380.0);
        ui.set_max_height(380.0);
        section_list(
            &lang,
            ui,
            d,
            crate::cjk_features(app),
            &[
                ("general", "General"),
                ("chars", "Basic Character Formats"),
                ("advanced", "Advanced Character Formats"),
                ("color", "Character Color"),
                ("openType", "OpenType Features"),
                ("underline", "Underline Options"),
                ("strikethrough", "Strikethrough Options"),
            ],
        );
        ui.separator();
        ui.vertical(|ui| match d.s("section").as_str() {
            "chars" => basic_character_formats(app, ui, d, &cf),
            "advanced" => advanced_character_formats(&lang, ui, d, &cf),
            "color" => character_color(&lang, ui, d, &cf, &doc),
            "openType" => open_type_features(&lang, ui, d, &cf),
            "underline" => line_options(&lang, ui, d, &cf, &doc, "underline"),
            "strikethrough" => line_options(&lang, ui, d, &cf, &doc, "strikethrough"),
            s if style_cjk::is_section(s) => style_cjk::section_ui(app, ui, d, s, &cf, None, &doc),
            _ => {
                character_style_general(&lang, ui, d, styles, &style, &based);
                if new {
                    ui.add_space(8.0);
                    check(ui, d, "apply", crate::i18n::tr(&lang, "Apply Style to Selection"));
                }
            }
        });
    });
}

/// An attribute's name as Style Settings shows it: the field's label where the name reads poorly.
fn setting_label(lang: &str, key: &str) -> String {
    match key {
        "hScale" => crate::i18n::tr(lang, "Horizontal Scale:").trim_end_matches([':', '：']).to_string(),
        "vScale" => crate::i18n::tr(lang, "Vertical Scale:").trim_end_matches([':', '：']).to_string(),
        "capitalization" => crate::i18n::tr(lang, "Case:").trim_end_matches([':', '：']).to_string(),
        "otfFeatures" => crate::i18n::tr(lang, "OpenType Features").to_string(),
        _ => crate::i18n::tr(lang, &humanize(key)).to_string(),
    }
}

/// An attribute value as Style Settings shows it.
fn setting_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |x| format!("{}", (x * 1000.0).round() / 1000.0)),
        Value::Bool(b) => if *b { "on" } else { "off" }.into(),
        Value::Array(a) => a.iter().map(setting_text).collect::<Vec<_>>().join(", "),
        Value::Object(o) => o.get("value").or_else(|| o.get("kind")).map(setting_text).unwrap_or_default(),
        Value::Null => String::new(),
    }
}

/// Where the character sections of the style options dialogs get the values they show: `base`
/// holds them by attribute name (resolved values for a paragraph style; for a character style the
/// attributes it sets, the others absent). `sparse` (a character style) lets a field be blank:
/// emptying it, or picking the blank entry, unsets the attribute.
struct CharFields<'a> {
    base: &'a Value,
    sparse: bool,
}

impl CharFields<'_> {
    /// The value shown for attribute `key`: the dialog's edit, else the base value.
    fn get(&self, d: &Dialog, key: &str) -> Value {
        d.fields.get(&format!("c.{key}")).cloned().unwrap_or_else(|| self.base.get(key).cloned().unwrap_or(Value::Null))
    }
    fn set(&self, d: &mut Dialog, key: &str, v: Value) {
        d.fields.insert(format!("c.{key}"), v);
    }
}

/// A number attribute shown multiplied by `scale` (100 for a fraction shown as a percentage).
fn char_number(ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, key: &str, suffix: &str, scale: f64, range: (f64, f64)) {
    let v = cf.get(d, key).as_f64().map(|x| x * scale);
    let field = crate::widgets::NumField::number(key, v, suffix, 2).width(80.0).range(range.0, range.1);
    let edit = if cf.sparse { field.show_or_clear(ui) } else { field.show(ui).map(Some) };
    if let Some(n) = edit {
        cf.set(d, key, n.map_or(Value::Null, |n| json!(n / scale)));
    }
}

/// An attribute chosen from `opts` (value, label); a character style also has a blank entry.
fn char_choice(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, key: &str, opts: &[(Value, &str)]) {
    let cur = cf.get(d, key);
    let shown = opts.iter().find(|o| o.0 == cur).map_or("", |o| crate::i18n::tr(lang, o.1));
    craft_ui::ComboBox::from_id_salt(("cs", key)).selected_text(crate::rtl::widget(ui, shown)).width(140.0).show_ui(ui, |ui| {
        if cf.sparse && ui.selectable_label(cur.is_null(), " ").clicked() {
            cf.set(d, key, Value::Null);
        }
        for (v, label) in opts {
            if ui.selectable_label(*v == cur, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                cf.set(d, key, v.clone());
            }
        }
    });
}

/// An on/off attribute; for a character style a third, mixed state leaves it unset (clicks go
/// unset → on → off → unset).
fn char_check(ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, key: &str, label: &str) {
    let cur = cf.get(d, key);
    let mut on = cur.as_bool().unwrap_or(false);
    let unset = cf.sparse && cur.is_null();
    if ui.add(egui::Checkbox::new(&mut on, crate::rtl::widget(ui, label)).indeterminate(unset)).clicked() {
        let next = match cur.as_bool() {
            _ if !cf.sparse => json!(on),
            None => json!(true),
            Some(true) => json!(false),
            Some(false) => Value::Null,
        };
        cf.set(d, key, next);
    }
}

/// A swatch attribute from a menu; `text_color` names the "" entry (the text's own colour).
fn char_swatch(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document, key: &str, text_color: Option<&str>) {
    let mut opts: Vec<(Value, &str)> = text_color.map(|l| (json!(""), l)).into_iter().collect();
    opts.extend(doc.swatches.iter().map(|w| (json!(w.name), w.name.as_str())));
    char_choice(lang, ui, d, cf, key, &opts);
}

/// Basic Character Formats: font, size, leading, kerning, tracking, case, position and the
/// underline, strikethrough, ligatures and no-break switches.
fn basic_character_formats(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    let fonts = crate::panels::fonts(app);
    let menu = crate::panels::font_menu(app);
    // Two columns, as InDesign's: the font, then Size | Leading, Kerning | Tracking, Case | Position.
    egui::Grid::new("psc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Font Family:"));
        let fam = cf.get(d, "fontFamily").as_str().unwrap_or("").to_string();
        let shown = if fam.is_empty() { "—".to_string() } else { crate::panels::font_label(app, &menu, &fam) };
        let picked = craft_ui::ComboBox::from_id_salt("psfam")
            .selected_text(shown)
            .width(200.0)
            .height(440.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui_with_header(
                ui,
                |ui| {
                    if cf.sparse && ui.selectable_label(fam.is_empty(), " ").clicked() {
                        cf.set(d, "fontFamily", Value::Null);
                    }
                    crate::panels::font_menu_header(app, ui, &menu, &fam, 200.0)
                },
                crate::panels::font_menu_list,
            )
            .inner
            .flatten();
        if let Some(f) = picked {
            cf.set(d, "fontFamily", json!(f.family));
            if let Some(style) = f.style {
                cf.set(d, "fontStyle", json!(style));
            }
        }
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Font Style:"));
        let sty = cf.get(d, "fontStyle").as_str().unwrap_or("").to_string();
        if cf.sparse && fam.is_empty() {
            // No family to list styles of: type the style ("Italic" in whatever font the text has).
            let mut s = sty.clone();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(200.0)).changed() {
                cf.set(d, "fontStyle", if s.trim().is_empty() { Value::Null } else { json!(s) });
            }
        } else {
            craft_ui::ComboBox::from_id_salt("pssty").selected_text(&sty).width(200.0).show_ui(ui, |ui| {
                if cf.sparse && ui.selectable_label(sty.is_empty(), " ").clicked() {
                    cf.set(d, "fontStyle", Value::Null);
                }
                for s in fonts.styles(&fam) {
                    if ui.selectable_label(s == sty, &s).clicked() {
                        cf.set(d, "fontStyle", json!(s));
                    }
                }
            });
        }
        ui.end_row();
    });
    egui::Grid::new("psc2").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Size:"));
        char_number(ui, d, cf, "size", " pt", 1.0, (0.1, 1296.0));
        crate::rtl::label(ui, crate::i18n::tr(lang, "Leading:"));
        let lv = match cf.get(d, "leading") {
            v if v["kind"] == "points" => v["value"].as_f64(),
            _ => None,
        };
        let field = crate::widgets::NumField::number("pslead", lv, " pt", 2).width(80.0).range(0.0, 5000.0);
        let edit = if cf.sparse { field.show_or_clear(ui) } else { field.show(ui).map(Some) };
        if let Some(n) = edit {
            cf.set(d, "leading", n.map_or(Value::Null, |n| json!({"kind": "points", "value": n})));
        }
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Kerning:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
            "kerning",
            &[(json!({"kind": "metrics"}), "Metrics"), (json!({"kind": "optical"}), "Optical"), (json!({"kind": "none"}), "0")],
        );
        crate::rtl::label(ui, crate::i18n::tr(lang, "Tracking:"));
        char_number(ui, d, cf, "tracking", "", 1.0, (-1000.0, 10000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Case:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
            "capitalization",
            &[
                (json!("normal"), "Normal"),
                (json!("allCaps"), "All Caps"),
                (json!("smallCaps"), "Small Caps"),
                (json!("openTypeAllSmallCaps"), "OpenType All Small Caps"),
            ],
        );
        crate::rtl::label(ui, crate::i18n::tr(lang, "Position:"));
        char_choice(
            lang,
            ui,
            d,
            cf,
            "position",
            &[
                (json!("normal"), "Normal"),
                (json!("superscript"), "Superscript"),
                (json!("subscript"), "Subscript"),
                (json!("otSuperscript"), "OpenType Superscript"),
                (json!("otSubscript"), "OpenType Subscript"),
                (json!("otNumerator"), "OpenType Numerator"),
                (json!("otDenominator"), "OpenType Denominator"),
            ],
        );
        ui.end_row();
    });
    ui.add_space(4.0);
    egui::Grid::new("pscb").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
        char_check(ui, d, cf, "underline", crate::i18n::tr(lang, "Underline"));
        char_check(ui, d, cf, "ligatures", crate::i18n::tr(lang, "Ligatures"));
        ui.end_row();
        char_check(ui, d, cf, "strikethrough", crate::i18n::tr(lang, "Strikethrough"));
        char_check(ui, d, cf, "noBreak", crate::i18n::tr(lang, "No Break"));
        ui.end_row();
    });
}

/// Advanced Character Formats: scaling, baseline shift, skew and language.
fn advanced_character_formats(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    egui::Grid::new("csa").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Horizontal Scale:"));
        char_number(ui, d, cf, "hScale", "%", 100.0, (1.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Vertical Scale:"));
        char_number(ui, d, cf, "vScale", "%", 100.0, (1.0, 1000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Baseline Shift:"));
        char_number(ui, d, cf, "baselineShift", " pt", 1.0, (-5000.0, 5000.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Skew:"));
        char_number(ui, d, cf, "skew", "°", 1.0, (-85.0, 85.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Language:"));
        let opts: Vec<(Value, &str)> = crate::panels::properties::LANGUAGES.iter().map(|l| (json!(l), *l)).collect();
        char_choice(lang, ui, d, cf, "language", &opts);
        ui.end_row();
        // Digits (World-Ready): Farsi draws 0-9 as Persian ۰-۹.
        crate::rtl::label(ui, crate::i18n::tr(lang, "Digits:"));
        let labels: Vec<String> = crate::panels::properties::DIGIT_OPTIONS.iter().map(|o| crate::panels::properties::digit_label(lang, *o)).collect();
        let cur = cf.get(d, "digits");
        let shown = crate::panels::properties::DIGIT_OPTIONS.iter().position(|o| json!(o.0) == cur).map_or(String::new(), |i| labels[i].clone());
        craft_ui::ComboBox::from_id_salt(("cs", "digits")).selected_text(crate::rtl::widget(ui, &shown)).width(200.0).show_ui(ui, |ui| {
            if cf.sparse && ui.selectable_label(cur.is_null(), " ").clicked() {
                cf.set(d, "digits", Value::Null);
            }
            for (o, label) in crate::panels::properties::DIGIT_OPTIONS.iter().zip(&labels) {
                if ui.selectable_label(json!(o.0) == cur, crate::rtl::widget(ui, label)).clicked() {
                    cf.set(d, "digits", json!(o.0));
                }
            }
        });
        ui.end_row();
    });
}

/// Character Color: the fill or stroke swatch, its tint and the stroke weight.
fn character_color(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document) {
    let stroke = d.s("colorTarget") == "stroke";
    ui.horizontal(|ui| {
        for (target, label) in [("fill", "Fill"), ("stroke", "Stroke")] {
            if ui.selectable_label((target == "stroke") == stroke, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                d.fields.insert("colorTarget".into(), json!(target));
            }
        }
    });
    let key = if stroke { "stroke" } else { "fill" };
    let cur = cf.get(d, key).as_str().unwrap_or("").to_string();
    egui::ScrollArea::vertical().id_salt(("cscolor", key)).max_height(220.0).show(ui, |ui| {
        for sw in &doc.swatches {
            let (c, g) = crate::widgets::swatch_colors(doc, &sw.name, 1.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                crate::widgets::paint_chip(ui.painter(), r, c, g);
                if ui.selectable_label(sw.name == cur, &sw.name).clicked() {
                    // A character style's chosen swatch, clicked again, is no longer set.
                    cf.set(d, key, if cf.sparse && sw.name == cur { Value::Null } else { json!(sw.name) });
                }
            });
        }
    });
    egui::Grid::new("cscg").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Tint:"));
        char_number(ui, d, cf, if stroke { "strokeTint" } else { "fillTint" }, "%", 100.0, (0.0, 100.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Weight:"));
        char_number(ui, d, cf, "strokeWeight", " pt", 1.0, (0.0, 800.0));
        ui.end_row();
        ui.label("");
        char_check(ui, d, cf, "strokeBehindFill", crate::i18n::tr(lang, "Stroke Behind Fill"));
        ui.end_row();
    });
}

/// OpenType Features: the feature switches, figure style and stylistic sets (one `otfFeatures`
/// list: changing any feature sets the whole list).
fn open_type_features(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields) {
    use designcraft_doc::otf;
    let mut list: Vec<String> =
        cf.get(d, "otfFeatures").as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let mut changed = false;
    egui::Grid::new("csot").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
        for (i, (tag, label, _)) in otf::TOGGLES.iter().enumerate() {
            let mut on = otf::is_on(&list, tag);
            if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).changed() {
                otf::set(&mut list, tag, on);
                changed = true;
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Figure Style:"));
        let fig = otf::figures(&list);
        let shown = otf::FIGURES.iter().find(|f| f.0 == fig).map_or("", |f| f.1);
        craft_ui::ComboBox::from_id_salt("csfig").selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, shown))).width(180.0).show_ui(
            ui,
            |ui| {
                for (id, label, ..) in otf::FIGURES {
                    if ui.selectable_label(fig == *id, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked()
                        && otf::set_figures(&mut list, id)
                    {
                        changed = true;
                    }
                }
            },
        );
    });
    ui.add_space(4.0);
    crate::rtl::label(ui, crate::i18n::tr(lang, "Stylistic Sets"));
    let mask = otf::stylistic_sets(&list);
    egui::Grid::new("csss").num_columns(10).spacing([2.0, 2.0]).show(ui, |ui| {
        for n in 1..=20u32 {
            let bit = 1u32 << (n - 1);
            if ui.add_sized([28.0, 20.0], egui::Button::selectable(mask & bit != 0, format!("{n}"))).clicked() {
                otf::set_stylistic_sets(&mut list, mask ^ bit);
                changed = true;
            }
            if n % 10 == 0 {
                ui.end_row();
            }
        }
    });
    if changed {
        cf.set(d, "otfFeatures", json!(list));
    }
}

/// Underline Options / Strikethrough Options (`key` is `underline` or `strikethrough`).
fn line_options(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, cf: &CharFields, doc: &designcraft_doc::Document, key: &str) {
    char_check(ui, d, cf, key, crate::i18n::tr(lang, if key == "underline" { "Underline On" } else { "Strikethrough On" }));
    ui.add_space(4.0);
    egui::Grid::new(("csline", key)).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Weight:"));
        char_number(ui, d, cf, &format!("{key}Weight"), " pt", 1.0, (0.0, 800.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Offset:"));
        char_number(ui, d, cf, &format!("{key}Offset"), " pt", 1.0, (-800.0, 800.0));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Color:"));
        char_swatch(lang, ui, d, cf, doc, &format!("{key}Color"), Some("(Text Color)"));
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(lang, "Tint:"));
        char_number(ui, d, cf, &format!("{key}Tint"), "%", 100.0, (0.0, 100.0));
        ui.end_row();
    });
}

fn delete_character_style(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let name = d.s("name");
    ui.set_min_width(380.0);
    crate::rtl::label(
        ui,
        egui::RichText::new(
            crate::i18n::tr(&app.ui.language, "Delete the character style \"{name}\"?")
                .replace("{name}", crate::i18n::style_name(&app.ui.language, &name)),
        )
        .size(12.5),
    );
    ui.add_space(8.0);
    let Some(st) = app.session.active() else { return };
    let in_use = st.doc.stories.values().any(|s| s.chars.iter().any(|r| r.format.style == name));
    let names: Vec<String> = st.doc.styles.character.iter().map(|c| c.name.clone()).filter(|n| *n != name).collect();
    if in_use {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "This style is in use. Text using it will be reassigned to:"))
                .size(11.0)
                .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
        );
    } else {
        crate::rtl::label(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Replace with:")).size(11.0).color(crate::theme::Tokens::get(ui.ctx()).text_dim),
        );
    }
    ui.add_space(4.0);
    if !d.fields.contains_key("replaceWith") {
        d.fields.insert("replaceWith".into(), json!(designcraft_doc::NO_CHAR_STYLE));
    }
    let replace = d.s("replaceWith");
    craft_ui::ComboBox::from_id_salt("del_char_replace")
        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &replace)))
        .width(260.0)
        .show_ui(ui, |ui| {
            for n in [designcraft_doc::NO_CHAR_STYLE].into_iter().chain(names.iter().map(String::as_str)) {
                if ui.selectable_label(n == replace, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                    d.fields.insert("replaceWith".into(), json!(n));
                }
            }
        });
}

fn combo(ui: &mut egui::Ui, d: &mut Dialog, key: &str, opts: &[(&str, &str)]) -> egui::Response {
    let cur = d.s(key);
    let shown = opts.iter().find(|o| o.0 == cur).map_or(cur.as_str(), |o| o.1).to_string();
    craft_ui::ComboBox::from_id_salt(key)
        .selected_text(crate::rtl::widget(ui, &shown))
        .width(170.0)
        .show_ui(ui, |ui| {
            for (v, label) in opts {
                if ui.selectable_label(cur == *v, crate::rtl::widget(ui, *label)).clicked() {
                    d.fields.insert(key.into(), json!(v));
                }
            }
        })
        .response
}

/// Translate enum captions without changing serialized values or user-defined names.
fn translated_combo(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, key: &str, opts: &[(&str, &str)]) -> egui::Response {
    let opts: Vec<_> = opts.iter().map(|(value, label)| (*value, crate::i18n::tr(lang, label))).collect();
    combo(ui, d, key, &opts)
}

/// Style name picker (paragraph or character styles of the active document).
fn style_combo(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, key: &str, character: bool) {
    let names: Vec<String> = app
        .session
        .active()
        .map(|st| {
            if character {
                st.doc.styles.character.iter().map(|s| s.name.clone()).collect()
            } else {
                st.doc.styles.paragraph.iter().map(|s| s.name.clone()).collect()
            }
        })
        .unwrap_or_default();
    let mut opts: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), crate::i18n::style_name(&app.ui.language, n))).collect();
    if character && !names.iter().any(|n| n == designcraft_doc::NO_CHAR_STYLE) {
        opts.insert(0, (designcraft_doc::NO_CHAR_STYLE, crate::i18n::style_name(&app.ui.language, designcraft_doc::NO_CHAR_STYLE)));
    }
    combo(ui, d, key, &opts);
}

const RULES: [(&str, &str); 2] = [("ruleAbove", "Rule Above"), ("ruleBelow", "Rule Below")];

/// A rule as the dialog shows it: `current[which]` (the resolved rule, inherited values included)
/// with the fields edited in the dialog (`{prefix}{which}.{field}`) on top.
fn shown_rule(d: &Dialog, prefix: &str, which: &str, current: &Value) -> Value {
    let mut r = match current.get(which) {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    let edited = format!("{prefix}{which}.");
    for (k, v) in &d.fields {
        if let Some(field) = k.strip_prefix(&edited) {
            r.insert(field.into(), v.clone());
        }
    }
    Value::Object(r)
}

/// The paragraph attributes of the Justification controls.
pub const JUSTIFY_ATTRS: [&str; 12] = [
    "wordSpaceMin",
    "wordSpaceDesired",
    "wordSpaceMax",
    "letterSpaceMin",
    "letterSpaceDesired",
    "letterSpaceMax",
    "glyphScaleMin",
    "glyphScaleDesired",
    "glyphScaleMax",
    "composer",
    "kashidas",
    "koreanCharBreaks",
];

/// The spacing rows of the Justification controls: (label, attribute stem, allowed percent range).
const JUSTIFY_ROWS: [(&str, &str, (f64, f64)); 3] = [
    ("Word Spacing", "wordSpace", (0.0, 1000.0)),
    ("Letter Spacing", "letterSpace", (-100.0, 500.0)),
    ("Glyph Scaling", "glyphScale", (50.0, 200.0)),
];

/// Checks the edited spacing values: each within its range, and minimum ≤ desired ≤ maximum
/// (values left mixed are not compared).
fn check_justification(d: &Dialog, prefix: &str, current: &Value) -> Result<(), String> {
    for (label, stem, (lo, hi)) in JUSTIFY_ROWS {
        let mut edited = false;
        let mut known = Vec::new();
        for suffix in ["Min", "Desired", "Max"] {
            let key = format!("{stem}{suffix}");
            let v = match d.fields.get(&format!("{prefix}{key}")) {
                Some(v) => {
                    edited = true;
                    let pct =
                        v.as_f64().map(|x| x * 100.0).filter(|x| x.is_finite()).ok_or_else(|| format!("Justification: {label} is not a number"))?;
                    // The fields show whole percents; compare what was shown.
                    if pct.round() < lo || pct.round() > hi {
                        return Err(format!("Justification: {label} must be between {lo}% and {hi}%"));
                    }
                    Some(pct)
                }
                None => current[key.as_str()].as_f64().map(|x| x * 100.0),
            };
            known.extend(v);
        }
        if edited && known.windows(2).any(|w| w[0] > w[1] + 1e-9) {
            return Err(format!("Justification: {label} needs Minimum ≤ Desired ≤ Maximum"));
        }
    }
    Ok(())
}

/// Paragraph attributes the Hyphenation Settings dialog sets.
pub const HYPH_ATTRS: [&str; 10] = [
    "hyphenate",
    "hyphMinWord",
    "hyphAfterFirst",
    "hyphBeforeLast",
    "hyphLimit",
    "hyphZone",
    "hyphWeight",
    "hyphCapitalized",
    "hyphLastWord",
    "hyphAcrossColumn",
];

/// InDesign's Hyphenation Settings: Hyphenate; Words with at Least, After First and Before Last
/// (letters), Hyphen Limit (hyphens) and Hyphenation Zone; the Better Spacing ↔ Fewer Hyphens
/// slider (0–10, as InDesign's HyphenWeight); Hyphenate Capitalized Words, Last Word and Across
/// Column. Edited fields are stored as `{prefix}{key}`; the others show `current` (a mixed value
/// is null and shows blank). The Hyphenation dialog for the selected paragraphs and Paragraph
/// Style Options › Hyphenation both use it.
fn hyphenation(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, prefix: &str, current: &Value, units: Unit) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let cur = |d: &Dialog, k: &str| d.fields.get(&format!("{prefix}{k}")).cloned().unwrap_or_else(|| current[k].clone());
    // Labels right-aligned against their fields, as InDesign's.
    let label_right = |ui: &mut egui::Ui, text: &str| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(crate::rtl::widget(ui, text)).wrap_mode(egui::TextWrapMode::Extend))
        });
    };
    let checkbox = |ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str| {
        let v = cur(d, key);
        // Mixed shows indeterminate and starts off, so the first click turns it on for all.
        let mut on = v.as_bool().unwrap_or(false);
        if ui.add(egui::Checkbox::new(&mut on, crate::rtl::widget(ui, tr(label))).indeterminate(v.is_null())).changed() {
            d.fields.insert(format!("{prefix}{key}"), json!(on));
        }
    };
    checkbox(ui, d, "hyphenate", "Hyphenate");
    ui.add_space(4.0);
    egui::Grid::new(("hyph", prefix)).num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
        // InDesign's ranges.
        for (label, key, unit, max) in [
            ("Words with at Least:", "hyphMinWord", "letters", 25.0),
            ("After First:", "hyphAfterFirst", "letters", 15.0),
            ("Before Last:", "hyphBeforeLast", "letters", 15.0),
            ("Hyphen Limit:", "hyphLimit", "hyphens", 25.0),
        ] {
            label_right(ui, &tr(label));
            let min = if key == "hyphLimit" { 0.0 } else { 1.0 };
            if let Some(n) = crate::widgets::spin_number(ui, &format!("hy{prefix}{key}"), cur(d, key).as_f64(), "", 60.0, 0) {
                d.fields.insert(format!("{prefix}{key}"), json!(n.round().clamp(min, max) as u64));
            }
            crate::rtl::label(ui, tr(unit));
            ui.end_row();
        }
        label_right(ui, &tr("Hyphenation Zone:"));
        if let Some(n) = crate::widgets::spin_measure(ui, &format!("hy{prefix}zone"), cur(d, "hyphZone").as_f64(), units, 60.0) {
            d.fields.insert(format!("{prefix}hyphZone"), json!(if n.is_finite() { n.clamp(0.0, 8640.0) } else { 36.0 }));
        }
        ui.end_row();
    });
    ui.add_space(6.0);
    // Better Spacing ↔ Fewer Hyphens, in InDesign's eleven steps.
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Better Spacing"));
        let w = cur(d, "hyphWeight").as_f64();
        let mut step = w.map_or(5.0, |w| (w * 10.0).round()).clamp(0.0, 10.0);
        // Mixed across the selection: the slider shows dimmed, and a click or drag on it, even
        // one that leaves the knob in place, sets every paragraph to that step.
        let mixed = w.is_none();
        // The ticks go under the knob: their place in the paint order is taken first.
        let ticks = ui.painter().add(egui::Shape::Noop);
        let r = ui
            .scope(|ui| {
                ui.spacing_mut().slider_width = 180.0;
                if mixed {
                    ui.multiply_opacity(0.4);
                }
                ui.add(egui::Slider::new(&mut step, 0.0..=10.0).step_by(1.0).show_value(false).handle_shape(egui::style::HandleShape::Circle))
            })
            .inner;
        // The tick marks under the track.
        let t = crate::theme::Tokens::get(ui.ctx());
        let inner = r.rect.shrink2(egui::vec2(r.rect.height() / 2.0, 0.0));
        let y = r.rect.center().y;
        let marks: Vec<egui::Shape> = (0..=10)
            .map(|i| {
                let x = inner.left() + inner.width() * i as f32 / 10.0;
                egui::Shape::line_segment([egui::pos2(x, y - 8.0), egui::pos2(x, y + 8.0)], egui::Stroke::new(1.0, t.text_dim))
            })
            .collect();
        ui.painter().set(ticks, egui::Shape::Vec(marks));
        if r.changed() || (mixed && (r.clicked() || r.drag_stopped())) {
            d.fields.insert(format!("{prefix}hyphWeight"), json!(step / 10.0));
        }
        crate::rtl::label(ui, tr("Fewer Hyphens"));
    });
    ui.add_space(6.0);
    egui::Grid::new(("hyphc", prefix)).num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
        checkbox(ui, d, "hyphCapitalized", "Hyphenate Capitalized Words");
        checkbox(ui, d, "hyphLastWord", "Hyphenate Last Word");
        ui.end_row();
        checkbox(ui, d, "hyphAcrossColumn", "Hyphenate Across Column");
        ui.end_row();
    });
}

/// The Hyphenation dialog's edits, as `type.para` attrs.
fn hyphenation_edits(d: &Dialog) -> Map<String, Value> {
    HYPH_ATTRS.iter().filter_map(|k| d.fields.get(*k).map(|v| (k.to_string(), v.clone()))).collect()
}

/// Preview on: the edits show on the page (redone when a field changes) without an undo step;
/// off, the paragraphs go back.
fn hyphenation_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
        }
        return;
    }
    let params = json!({"attrs": hyphenation_edits(d)});
    if shown.as_ref() == Some(&params) || d.fields.get(PREVIEW_FAILED) == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction("Hyphenation")
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview("type.para", params.clone()) {
        Ok(_) => {
            d.fields.remove("status");
            d.fields.remove(PREVIEW_FAILED);
            d.fields.insert(PREVIEWED.into(), params);
        }
        // Rejected settings: the paragraphs go back, and the reason shows until they change.
        Err(e) => {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.insert("status".into(), json!(e.to_string()));
            d.fields.insert(PREVIEW_FAILED.into(), params);
        }
    }
}

/// The hyphenation settings a preview was refused for (not tried again until they change).
const PREVIEW_FAILED: &str = "previewFailed";

/// The Justification controls (the Justification dialog, and the Paragraph Style Options section).
/// `current` holds the resolved paragraph attributes, `null` where the selected paragraphs differ
/// (the field shows empty); each edit is stored as `{prefix}{attr}`, so OK changes only the fields
/// edited.
fn justification(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, prefix: &str, current: &Value) {
    let cur = |d: &Dialog, k: &str| d.fields.get(&format!("{prefix}{k}")).cloned().unwrap_or_else(|| current[k].clone());
    egui::Grid::new("psj").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label("");
        crate::rtl::label(ui, crate::i18n::tr(lang, "Minimum"));
        crate::rtl::label(ui, crate::i18n::tr(lang, "Desired"));
        crate::rtl::label(ui, crate::i18n::tr(lang, "Maximum"));
        ui.end_row();
        for (label, base, _) in JUSTIFY_ROWS {
            crate::rtl::label(ui, crate::i18n::tr(lang, &format!("{label}:")));
            for suffix in ["Min", "Desired", "Max"] {
                let key = format!("{base}{suffix}");
                let v = cur(d, &key).as_f64().map(|x| x * 100.0);
                if let Some(n) = ui.scope(|ui| crate::widgets::number(ui, &format!("ps{key}"), v, "%", 60.0, 0)).inner {
                    d.fields.insert(format!("{prefix}{key}"), json!(n / 100.0));
                }
            }
            ui.end_row();
        }
        crate::rtl::label(ui, crate::i18n::tr(lang, "Composer:"));
        let composer = cur(d, "composer");
        // Mixed (null) selects neither; a Japanese composer keeps its kind.
        let (single, [paragraph, single_line]) = crate::panels::properties::composer_choice(composer.as_str());
        let known = !composer.is_null();
        if ui.selectable_label(known && !single, crate::rtl::widget(ui, crate::i18n::tr(lang, "Paragraph Composer"))).clicked() {
            d.fields.insert(format!("{prefix}composer"), json!(paragraph));
        }
        if ui.selectable_label(known && single, crate::rtl::widget(ui, crate::i18n::tr(lang, "Single-line Composer"))).clicked() {
            d.fields.insert(format!("{prefix}composer"), json!(single_line));
        }
        ui.end_row();
        for (label, key, text, default) in [
            ("Insert Kashidas:", "kashidas", "In justified Arabic text", true),
            ("Korean Line Breaks:", "koreanCharBreaks", "Between syllables (not only at spaces)", false),
        ] {
            crate::rtl::label(ui, crate::i18n::tr(lang, label));
            let v = cur(d, key);
            let mut on = v.as_bool().unwrap_or(default);
            if ui.add(egui::Checkbox::new(&mut on, crate::rtl::widget(ui, crate::i18n::tr(lang, text))).indeterminate(v.is_null())).changed() {
                d.fields.insert(format!("{prefix}{key}"), json!(on));
            }
            ui.end_row();
        }
    });
    if let Some(e) = d.fields.get("justifyError").and_then(Value::as_str) {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(e).color(ui.visuals().error_fg_color));
    }
}

/// The rule fields edited in the dialog, as `type.para` attrs: `{ruleAbove?: {…}, ruleBelow?: {…}}`.
fn rule_edits(d: &Dialog, prefix: &str) -> Map<String, Value> {
    let mut out = Map::new();
    for (which, _) in RULES {
        let edited = format!("{prefix}{which}.");
        let fields: Map<String, Value> = d.fields.iter().filter_map(|(k, v)| k.strip_prefix(&edited).map(|f| (f.to_string(), v.clone()))).collect();
        if !fields.is_empty() {
            out.insert(which.into(), Value::Object(fields));
        }
    }
    out
}

/// Paragraph Rules (the dialog, and the Paragraph Style Options section): Rule Above or Rule Below,
/// chosen at the top. `current` holds the resolved `ruleAbove` / `ruleBelow`; each edit is stored as
/// `{prefix}{rule}.{field}`, so OK changes only the fields edited.
fn paragraph_rules(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, prefix: &str, current: &Value) {
    let lang = app.ui.language.as_str();
    let (h_units, v_units) =
        app.session.active().map(|st| (st.doc.settings.horizontal_units, st.doc.settings.vertical_units)).unwrap_or((Unit::Points, Unit::Points));
    let which = if d.s("rule") == "ruleBelow" { "ruleBelow" } else { "ruleAbove" };
    let rule = shown_rule(d, prefix, which, current);
    let key = |field: &str| format!("{prefix}{which}.{field}");
    // A number field takes the rest of the row; keep its grid column to the field's width.
    let sized = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui) -> Option<f64>| {
        ui.allocate_ui_with_layout(egui::vec2(80.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| add(ui)).inner
    };
    ui.horizontal(|ui| {
        let shown = RULES.iter().find(|r| r.0 == which).map_or(which, |r| r.1);
        craft_ui::ComboBox::from_id_salt("rule_which").selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, shown))).width(130.0).show_ui(
            ui,
            |ui| {
                for (v, label) in RULES {
                    if ui.selectable_label(v == which, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                        d.fields.insert("rule".into(), json!(v));
                    }
                }
            },
        );
        ui.add_space(12.0);
        let mut on = rule["on"].as_bool().unwrap_or(false);
        if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(lang, "Rule On"))).changed() {
            d.fields.insert(key("on"), json!(on));
        }
    });
    ui.add_space(6.0);
    let on = rule["on"].as_bool().unwrap_or(false);
    let kind: designcraft_doc::StrokeType = serde_json::from_value(rule["type"].clone()).unwrap_or_default();
    let doc = app.session.active().map(|st| &st.doc);
    // As InDesign's: Weight and Type, Color and Tint, Gap Color and Gap Tint, Width and Offset,
    // Left and Right Indent, then Keep In Frame (rule above).
    ui.add_enabled_ui(on, |ui| {
        egui::Grid::new(("rules", which)).num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(lang, "Weight:"));
            if let Some(v) = sized(ui, &mut |ui| {
                crate::widgets::NumField::number(&key("weight"), rule["weight"].as_f64(), " pt", 2).width(80.0).spinner().range(0.0, 1000.0).show(ui)
            }) {
                d.fields.insert(key("weight"), json!(v.clamp(0.0, 1000.0)));
            }
            crate::rtl::label(ui, crate::i18n::tr(lang, "Type:"));
            let custom: Vec<String> = doc.map_or_else(Vec::new, |doc| doc.stroke_styles.iter().map(|s| s.name.clone()).collect());
            let shown = match &kind {
                designcraft_doc::StrokeType::Style { name } => name.clone(),
                k => crate::i18n::tr(lang, k.label()).to_string(),
            };
            craft_ui::ComboBox::from_id_salt(("rule_type", which)).selected_text(crate::rtl::widget(ui, shown)).width(130.0).show_ui(ui, |ui| {
                for k in rule_types().into_iter().chain(custom.into_iter().map(|name| designcraft_doc::StrokeType::Style { name })) {
                    let label = match &k {
                        designcraft_doc::StrokeType::Style { name } => name.clone(),
                        k => crate::i18n::tr(lang, k.label()).to_string(),
                    };
                    if ui.selectable_label(k == kind, crate::rtl::widget(ui, label)).clicked() {
                        d.fields.insert(key("type"), serde_json::to_value(&k).unwrap_or_default());
                    }
                }
            });
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(lang, "Color:"));
            let color = rule["color"].as_str().unwrap_or("").to_string();
            let text_color = crate::i18n::tr(lang, "Text Color");
            if let Some(w) = doc.and_then(|doc| swatch_menu_with(ui, doc, ("rule_color", which), &color, 130.0, Some(text_color))) {
                d.fields.insert(key("color"), json!(w));
            }
            crate::rtl::label(ui, crate::i18n::tr(lang, "Tint:"));
            if let Some(v) = sized(ui, &mut |ui| crate::widgets::number(ui, &key("tint"), rule["tint"].as_f64().map(|t| t * 100.0), "%", 80.0, 0)) {
                d.fields.insert(key("tint"), json!((v / 100.0).clamp(0.0, 1.0)));
            }
            ui.end_row();
            // A solid rule has no gaps.
            let gaps = !matches!(kind, designcraft_doc::StrokeType::Solid);
            crate::rtl::label(ui, crate::i18n::tr(lang, "Gap Color:"));
            let gap = rule["gapColor"].as_str().unwrap_or(designcraft_color::swatch::NONE).to_string();
            ui.add_enabled_ui(gaps, |ui| {
                if let Some(w) = doc.and_then(|doc| swatch_menu(ui, doc, ("rule_gap", which), &gap, 130.0)) {
                    d.fields.insert(key("gapColor"), json!(w));
                }
            });
            crate::rtl::label(ui, crate::i18n::tr(lang, "Gap Tint:"));
            ui.add_enabled_ui(gaps && gap != designcraft_color::swatch::NONE, |ui| {
                // A full gap tint is left out of the rule's JSON: it reads 100%.
                let gap_tint = rule["gapTint"].as_f64().unwrap_or(1.0) * 100.0;
                if let Some(v) = sized(ui, &mut |ui| crate::widgets::number(ui, &key("gapTint"), Some(gap_tint), "%", 80.0, 0)) {
                    d.fields.insert(key("gapTint"), json!((v / 100.0).clamp(0.0, 1.0)));
                }
            });
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(lang, "Width:"));
            let column = rule["columnWidth"].as_bool().unwrap_or(true);
            craft_ui::ComboBox::from_id_salt(("rule_width", which))
                .selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, if column { "Column" } else { "Text" })))
                .width(130.0)
                .show_ui(ui, |ui| {
                    for (v, label) in [(true, "Column"), (false, "Text")] {
                        if ui.selectable_label(v == column, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                            d.fields.insert(key("columnWidth"), json!(v));
                        }
                    }
                });
            crate::rtl::label(ui, crate::i18n::tr(lang, "Offset:"));
            if let Some(v) = sized(ui, &mut |ui| {
                crate::widgets::NumField::measure(&key("offset"), rule["offset"].as_f64(), v_units).width(80.0).spinner().show(ui)
            }) {
                d.fields.insert(key("offset"), json!(v));
            }
            ui.end_row();
            for (label, field) in [("Left Indent:", "leftIndent"), ("Right Indent:", "rightIndent")] {
                crate::rtl::label(ui, crate::i18n::tr(lang, label));
                if let Some(v) =
                    sized(ui, &mut |ui| crate::widgets::NumField::measure(&key(field), rule[field].as_f64(), h_units).width(80.0).spinner().show(ui))
                {
                    d.fields.insert(key(field), json!(v));
                }
            }
            ui.end_row();
        });
        if which == "ruleAbove" {
            let mut keep = rule["keepInFrame"].as_bool().unwrap_or(false);
            if ui.checkbox(&mut keep, crate::rtl::widget(ui, crate::i18n::tr(lang, "Keep In Frame"))).changed() {
                d.fields.insert(key("keepInFrame"), json!(keep));
            }
        }
    });
}

/// The built-in stroke types a paragraph rule can have (Paragraph Rules › Type).
fn rule_types() -> Vec<designcraft_doc::StrokeType> {
    use designcraft_doc::StrokeType as T;
    vec![
        T::Solid,
        T::ThickThick,
        T::ThickThin,
        T::ThickThinThick,
        T::ThinThick,
        T::ThinThickThin,
        T::ThinThin,
        T::Dashed { pattern: vec![12.0, 4.0] },
        T::Dotted,
        T::Hashed,
        T::Wavy,
    ]
}

/// Document Footnote Options: Numbering and Formatting / Layout tabs.
fn footnote_options(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for (tab, label) in [("numbering", "Numbering and Formatting"), ("layout", "Layout")] {
            if ui.selectable_label(d.s("tab") == tab, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                d.fields.insert("tab".into(), json!(tab));
            }
        }
    });
    ui.separator();
    let head = |ui: &mut egui::Ui, t: &str| {
        ui.add_space(4.0);
        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, t)).font(semibold(12.0)));
    };
    if d.s("tab") == "layout" {
        head(ui, "Spacing Options");
        egui::Grid::new("fn_sp").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Minimum Space Before First Footnote:"));
            text_field(ui, d, "spaceBefore", 70.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Space Between Footnotes:"));
            text_field(ui, d, "spaceBetween", 70.0);
            ui.end_row();
        });
        head(ui, "First Baseline");
        egui::Grid::new("fn_fb").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Offset:"));
            translated_combo(
                &app.ui.language,
                ui,
                d,
                "firstBaseline",
                &[
                    ("ascent", "Ascent"),
                    ("capHeight", "Cap Height"),
                    ("leading", "Leading"),
                    ("xHeight", "x Height"),
                    ("emboxHeight", "Em Box Height"),
                    ("fixed", "Fixed"),
                ],
            );
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Min:"));
            text_field(ui, d, "firstBaselineMin", 70.0);
            ui.end_row();
        });
        check(ui, d, "spanColumns", crate::i18n::tr(&app.ui.language, "Span Footnotes Across Columns"));
        head(ui, "Rule Above");
        check(ui, d, "rule.on", crate::i18n::tr(&app.ui.language, "Rule On"));
        egui::Grid::new("fn_rule").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Weight:"));
            text_field(ui, d, "rule.weight", 60.0);
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Color:"));
            let swatches: Vec<String> = app.session.active().map(|st| st.doc.swatches.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
            let opts: Vec<(&str, &str)> = swatches.iter().map(|n| (n.as_str(), n.as_str())).collect();
            combo(ui, d, "rule.color", &opts);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Width:"));
            text_field(ui, d, "rule.width", 60.0);
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Offset:"));
            text_field(ui, d, "rule.offset", 60.0);
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Left Indent:"));
            text_field(ui, d, "rule.leftIndent", 60.0);
            ui.end_row();
        });
        return;
    }
    head(ui, "Numbering");
    egui::Grid::new("fn_num").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Style:"));
        translated_combo(
            &app.ui.language,
            ui,
            d,
            "style",
            &[
                ("arabic", "1, 2, 3, 4..."),
                ("upperRoman", "I, II, III, IV..."),
                ("lowerRoman", "i, ii, iii, iv..."),
                ("upperLetters", "A, B, C, D..."),
                ("lowerLetters", "a, b, c, d..."),
                ("arabicLeadingZero", "01, 02, 03..."),
                ("symbols", "*, †, ‡, §..."),
                ("kanji", "一, 二, 三, 四..."),
            ],
        );
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Start at:"));
        text_field(ui, d, "startAt", 60.0);
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Restart Numbering Every:"));
        translated_combo(
            &app.ui.language,
            ui,
            d,
            "restart",
            &[("never", "Never (continuous)"), ("page", "Page"), ("spread", "Spread"), ("section", "Section")],
        );
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Show Prefix/Suffix in:"));
        translated_combo(
            &app.ui.language,
            ui,
            d,
            "affixIn",
            &[("none", "None"), ("reference", "Footnote Reference"), ("text", "Footnote Text"), ("both", "Both")],
        );
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Prefix:"));
        text_field(ui, d, "prefix", 60.0);
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Suffix:"));
        text_field(ui, d, "suffix", 60.0);
        ui.end_row();
    });
    head(ui, "Footnote Reference Number in Text");
    egui::Grid::new("fn_ref").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Position:"));
        translated_combo(
            &app.ui.language,
            ui,
            d,
            "refPosition",
            &[
                ("superscript", "Apply Superscript"),
                ("subscript", "Apply Subscript"),
                ("normal", "Apply Normal"),
                ("otSuperscript", "OpenType Superscript"),
            ],
        );
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Character Style:"));
        style_combo(app, ui, d, "refCharStyle", true);
        ui.end_row();
    });
    head(ui, "Footnote Formatting");
    egui::Grid::new("fn_fmt").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Paragraph Style:"));
        style_combo(app, ui, d, "paraStyle", false);
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Separator:"));
        text_field(ui, d, "separator", 60.0);
        ui.end_row();
    });
}

/// One parameter of a command, parsed from its params documentation
/// (`{name, type?: a|b|c, count?, flag?: bool, …}`).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandField {
    pub key: String,
    pub optional: bool,
    /// Enumerated values (`a|b|c`).
    pub choices: Vec<String>,
    pub boolean: bool,
    /// The documented value spec (shown as a hint).
    pub hint: String,
}

/// Parameters of a command from its documentation string: the top-level keys of the first `{…}`.
pub fn command_fields(doc: &str) -> Vec<CommandField> {
    let Some(start) = doc.find('{') else { return vec![] };
    let mut depth = 0i32;
    let mut end = doc.len();
    for (i, c) in doc[start..].char_indices() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &doc[start + 1..end];
    let mut parts = Vec::new();
    let (mut depth, mut from) = (0i32, 0usize);
    for (i, c) in body.char_indices() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&body[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[from..]);
    let ident = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !s.starts_with(|c: char| c.is_ascii_digit());
    let mut out = Vec::new();
    for part in parts {
        // `key`, `key?`, `key: spec` or `key (spec)`: the key is the leading word.
        let part = part.trim();
        let k_end = part.find(|c: char| c.is_whitespace() || c == ':' || c == '(').unwrap_or(part.len());
        let (k, rest) = part.split_at(k_end);
        let rest = rest.trim_start();
        let spec = match rest.strip_prefix(':') {
            Some(v) => v.trim(),
            None => rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')).unwrap_or(rest).trim(),
        };
        let optional = k.ends_with('?');
        let key = k.trim_end_matches('?');
        if !ident(key) || out.iter().any(|f: &CommandField| f.key == key) {
            continue;
        }
        let word = spec.split_whitespace().next().unwrap_or("");
        let choices: Vec<String> =
            if word.contains('|') && word.split('|').all(ident) { word.split('|').map(str::to_string).collect() } else { vec![] };
        let boolean = spec.starts_with("bool") || spec.starts_with("true|false") || spec.starts_with("true") && spec.len() <= 5;
        out.push(CommandField { key: key.into(), optional, choices: if boolean { vec![] } else { choices }, boolean, hint: spec.into() });
    }
    out
}

fn humanize(key: &str) -> String {
    let mut s = String::new();
    for (i, c) in key.chars().enumerate() {
        if i == 0 {
            s.extend(c.to_uppercase());
        } else if c.is_uppercase() {
            s.push(' ');
            s.extend(c.to_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

/// A form for any command, from its parameter documentation.
fn command_form(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let doc = designcraft_engine::find_command(&d.id[4..]).map_or("", |c| c.params);
    let fields = command_fields(doc);
    let dim = crate::theme::Tokens::get(ui.ctx()).text_dim;
    egui::Grid::new("cmdform").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for f in &fields {
            ui.label(crate::rtl::widget(
                ui,
                format!("{}{}", crate::i18n::tr(&app.ui.language, &humanize(&f.key)), if f.optional { "" } else { " *" }),
            ));
            if f.boolean {
                check(ui, d, &f.key, "");
            } else if !f.choices.is_empty() {
                let mut opts: Vec<(&str, &str)> = vec![("", "—")];
                let labels: Vec<_> = f.choices.iter().map(|c| humanize(c)).collect();
                opts.extend(f.choices.iter().zip(&labels).map(|(value, label)| (value.as_str(), crate::i18n::tr(&app.ui.language, label))));
                combo(ui, d, &f.key, &opts);
            } else {
                let mut s = d.s(&f.key);
                let hint = if f.hint.is_empty() { String::new() } else { f.hint.clone() };
                if ui.add(egui::TextEdit::singleline(&mut s).hint_text(egui::RichText::new(hint).color(dim)).desired_width(240.0)).changed() {
                    d.fields.insert(f.key.clone(), Value::String(s));
                }
            }
            ui.end_row();
        }
    });
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.label(egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

const STEP_AND_REPEAT: &str = "cmd:edit.stepAndRepeat";
pub(crate) const CORNER_OPTIONS: &str = "cmd:object.cornerOptions";
const TOC: &str = "cmd:toc.generate";

/// One included style of the Table of Contents dialog, as `toc.generate` takes it, with the
/// separator in the dialog's notation (`^t` a tab).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct TocRow {
    style: String,
    level: u8,
    entry_style: String,
    page_number: String,
    separator: String,
    /// Character styles of the page number and the separator; "" is [None].
    page_number_style: String,
    separator_style: String,
    sort: bool,
}

fn toc_separator_shown(s: &str) -> String {
    designcraft_doc::story::toc_separator_notation(s)
}

fn toc_separator_typed(s: &str) -> String {
    designcraft_doc::story::toc_separator_text(s)
}

/// Appends the special-character `code` (such as `^y`) to the separator field's notation. A
/// trailing unescaped caret is a literal caret: it's escaped first, so it doesn't swallow the code.
fn toc_separator_append(field: &mut String, code: &str) {
    if field.chars().rev().take_while(|&c| c == '^').count() % 2 == 1 {
        field.push('^');
    }
    field.push_str(code);
}

fn toc_rows(d: &Dialog) -> Vec<TocRow> {
    d.fields.get("included").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// Layout › Table of Contents…: prefilled from the document's table of contents when it has one.
pub fn open_toc(app: &mut DesignApp) {
    let Some(st) = app.session.active() else { return };
    let d = &st.doc;
    let styles: Vec<&str> = d.styles.paragraph.iter().map(|p| p.name.as_str()).filter(|n| *n != designcraft_doc::NO_PARA_STYLE).collect();
    let char_styles: Vec<&str> = d.styles.character.iter().map(|c| c.name.as_str()).filter(|n| *n != designcraft_doc::NO_CHAR_STYLE).collect();
    let toc = d.toc.as_ref().filter(|t| d.story(t.story).is_some());
    let rows: Vec<TocRow> = toc
        .map(|t| {
            t.entries
                .iter()
                .map(|e| TocRow {
                    style: e.style.clone(),
                    level: e.level,
                    entry_style: e.entry_style.clone().unwrap_or_default(),
                    page_number: match (t.page_numbers, e.page_number) {
                        (false, _) | (_, designcraft_doc::TocPageNumber::None) => "none",
                        (_, designcraft_doc::TocPageNumber::Before) => "before",
                        _ => "after",
                    }
                    .into(),
                    separator: toc_separator_shown(e.separator.as_deref().unwrap_or("\t")),
                    page_number_style: e.page_number_style.clone().unwrap_or_default(),
                    separator_style: e.separator_style.clone().unwrap_or_default(),
                    sort: e.sort,
                })
                .collect()
        })
        .unwrap_or_default();
    let fields = json!({
        "title": toc.map_or("Contents", |t| t.title.as_str()),
        "titleStyle": toc.and_then(|t| t.title_style.clone()).unwrap_or_default(),
        "included": rows,
        "selected": 0,
        "styles": styles,
        "charStyles": char_styles,
        "hasToc": toc.is_some(),
        "replace": toc.is_some(),
        "createBookmarks": toc.is_none_or(|t| t.create_bookmarks),
    });
    app.ui.dialog = Some(Dialog::new(TOC, fields));
}

/// The dialog's fields as `toc.generate` params. A new frame goes on the page in view.
fn toc_params(app: &DesignApp, d: &Dialog) -> Value {
    let entries: Vec<Value> = toc_rows(d)
        .iter()
        .map(|r| {
            let mut e = json!({"style": r.style, "level": r.level, "separator": toc_separator_typed(&r.separator)});
            if !r.entry_style.is_empty() {
                e["entryStyle"] = json!(r.entry_style);
            }
            if !r.page_number.is_empty() {
                e["pageNumber"] = json!(r.page_number);
            }
            if !r.page_number_style.is_empty() {
                e["pageNumberStyle"] = json!(r.page_number_style);
            }
            if !r.separator_style.is_empty() {
                e["separatorStyle"] = json!(r.separator_style);
            }
            if r.sort {
                e["sort"] = json!(true);
            }
            e
        })
        .collect();
    let mut p = json!({"title": d.s("title"), "entries": entries, "replace": d.b("replace"), "createBookmarks": d.b("createBookmarks")});
    let title_style = d.s("titleStyle");
    if !title_style.is_empty() {
        p["titleStyle"] = json!(title_style);
    }
    if let Some(page) = crate::canvas::current_page(app) {
        p["page"] = json!(page + 1);
    }
    p
}

/// A style menu whose first choice, stored as "", is `auto` (the generated paragraph style, or
/// [None] for a character style).
fn toc_style_combo(ui: &mut egui::Ui, salt: &str, styles: &[String], auto: &str, cur: &mut String, name: egui::Id) -> bool {
    let shown = if cur.is_empty() { auto.to_string() } else { cur.clone() };
    let mut changed = false;
    craft_ui::ComboBox::from_id_salt(("toc_style", salt))
        .selected_text(shown)
        .width(200.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_empty(), auto).clicked() {
                cur.clear();
                changed = true;
            }
            for n in styles.iter().filter(|n| *n != auto) {
                if ui.selectable_label(cur == n, n.as_str()).clicked() {
                    *cur = n.clone();
                    changed = true;
                }
            }
        })
        .response
        .labelled_by(name);
    changed
}

/// Layout › Table of Contents: the title and its style, the paragraph styles to include (Add /
/// Remove between the two lists) and, for the selected one, its entry style, page number
/// position and separator with their character styles, alphabetical sorting and level.
fn toc_dialog(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let styles: Vec<String> = d.fields.get("styles").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let char_styles: Vec<String> = d.fields.get("charStyles").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut expanded = d.fields.get("moreOptions").and_then(Value::as_bool).unwrap_or(true);
    let mut rows = toc_rows(d);
    let mut sel = d.n("selected").map_or(0, |n| n.max(0.0) as usize);
    let mut other = d.s("other");
    let mut title_style = d.s("titleStyle");
    // The dialog body's width, less the two labels and the style selector (200 pt) beside the title.
    let title_room = ui.available_width() - 300.0;
    egui::Grid::new("toc_title").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Title:"));
        let mut title = d.s("title");
        // 200 pt, reserved: a desired width alone is squeezed by the grid to a few characters.
        // Narrower when the dialog is, so the style selector beside it stays in view.
        let w = 200.0_f32.min(title_room.max(80.0));
        let field = ui.add_sized([w, ui.spacing().interact_size.y], egui::TextEdit::singleline(&mut title).align(egui::Align2::LEFT_TOP));
        if field.accessible_name(tr("Title:")).changed() {
            d.fields.insert("title".into(), json!(title));
        }
        let name = crate::rtl::label(ui, tr("Style:")).id;
        toc_style_combo(ui, "title", &styles, "TOC Title", &mut title_style, name);
        ui.end_row();
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Styles in Table of Contents")).font(semibold(12.0)));
        let toggle = ui.button(tr(if expanded { "Fewer Options" } else { "More Options" }));
        // Enter on the focused toggle toggles it (egui activates a focused button with Enter):
        // the footer drawn after it mustn't take that Enter for OK too.
        if toggle.has_focus() {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        }
        if toggle.clicked() {
            expanded = !expanded;
        }
    });
    let others: Vec<&String> = styles.iter().filter(|n| !rows.iter().any(|r| &r.style == *n)).collect();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            crate::rtl::label(ui, tr("Include Paragraph Styles:"));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(200.0);
                egui::ScrollArea::vertical().id_salt("toc_included").max_height(150.0).min_scrolled_height(150.0).show(ui, |ui| {
                    for (i, r) in rows.iter().enumerate() {
                        let indent = "  ".repeat(usize::from(r.level.saturating_sub(1)));
                        if ui.selectable_label(i == sel, format!("{indent}{}", r.style)).clicked() {
                            sel = i;
                        }
                    }
                });
            });
        });
        ui.vertical(|ui| {
            ui.add_space(40.0);
            if ui.button(tr("« Add")).clicked() && others.iter().any(|n| **n == other) {
                let level = u8::try_from(rows.len() + 1).unwrap_or(9).min(9);
                rows.push(TocRow { style: other.clone(), level, page_number: "after".into(), separator: "^t".into(), ..Default::default() });
                sel = rows.len() - 1;
                other.clear();
            }
            if ui.button(tr("Remove »")).clicked() && sel < rows.len() {
                other = rows.remove(sel).style;
                sel = sel.min(rows.len().saturating_sub(1));
            }
        });
        ui.vertical(|ui| {
            crate::rtl::label(ui, tr("Other Styles:"));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(200.0);
                egui::ScrollArea::vertical().id_salt("toc_other").max_height(150.0).min_scrolled_height(150.0).show(ui, |ui| {
                    for n in styles.iter().filter(|n| !rows.iter().any(|r| &r.style == *n)) {
                        if ui.selectable_label(*n == other, n.as_str()).clicked() {
                            other = n.clone();
                        }
                    }
                });
            });
        });
    });
    if let Some(r) = rows.get_mut(sel) {
        ui.add_space(8.0);
        crate::rtl::label(ui, egui::RichText::new(format!("{} {}", tr("Style:"), r.style)).font(semibold(12.0)));
        let none = designcraft_doc::NO_CHAR_STYLE;
        egui::Grid::new("toc_entry").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            let name = crate::rtl::label(ui, tr("Entry Style:")).id;
            let auto = format!("TOC Level {}", r.level.clamp(1, 9));
            toc_style_combo(ui, "entry", &styles, &auto, &mut r.entry_style, name);
            ui.end_row();
            if !expanded {
                return;
            }
            let page_number = crate::rtl::label(ui, tr("Page Number:")).id;
            let choices = [("after", "After Entry"), ("before", "Before Entry"), ("none", "No Page Number")];
            let cur = choices.iter().find(|(v, _)| *v == r.page_number).map_or("After Entry", |(_, l)| l);
            craft_ui::ComboBox::from_id_salt("toc_page_number")
                .selected_text(tr(cur))
                .width(200.0)
                .show_ui(ui, |ui| {
                    for (v, l) in choices {
                        if ui.selectable_label(r.page_number == v, tr(l)).clicked() {
                            r.page_number = v.into();
                        }
                    }
                })
                .response
                .labelled_by(page_number);
            let name = crate::rtl::label(ui, tr("Style:")).id;
            toc_style_combo(ui, "page_number", &char_styles, none, &mut r.page_number_style, name);
            ui.end_row();
            let between = crate::rtl::label(ui, tr("Between Entry and Number:")).id;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut r.separator).desired_width(80.0).align(egui::Align2::LEFT_TOP)).labelled_by(between);
                ui.menu_button("▸", |ui| {
                    for (label, code) in [("Tab Character", "^t"), ("Right Indent Tab", "^y"), ("Em Space", "^m"), ("En Space", "^>")] {
                        if ui.button(tr(label)).clicked() {
                            toc_separator_append(&mut r.separator, code);
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(tr("Insert Special Character"));
            });
            let name = crate::rtl::label(ui, tr("Style:")).id;
            toc_style_combo(ui, "separator", &char_styles, none, &mut r.separator_style, name);
            ui.end_row();
            crate::rtl::label(ui, "");
            ui.checkbox(&mut r.sort, tr("Sort Entries in Alphabetical Order"));
            ui.end_row();
            let level = crate::rtl::label(ui, tr("Level:")).id;
            craft_ui::ComboBox::from_id_salt("toc_level")
                .selected_text(r.level.to_string())
                .width(60.0)
                .show_ui(ui, |ui| {
                    for l in 1..=9u8 {
                        if ui.selectable_label(r.level == l, l.to_string()).clicked() {
                            r.level = l;
                        }
                    }
                })
                .response
                .labelled_by(level);
            ui.end_row();
        });
    }
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Options")).font(semibold(12.0)));
    check(ui, d, "createBookmarks", &tr("Create PDF Bookmarks"));
    let has_toc = d.b("hasToc");
    ui.add_enabled_ui(has_toc, |ui| check(ui, d, "replace", &tr("Replace Existing Table of Contents")));
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
    d.fields.insert("moreOptions".into(), json!(expanded));
    d.fields.insert("included".into(), json!(rows));
    d.fields.insert("selected".into(), json!(sel));
    d.fields.insert("other".into(), json!(other));
    d.fields.insert("titleStyle".into(), json!(title_style));
}
/// The params a previewing dialog shows on the page (its interaction is open while it is set).
const PREVIEWED: &str = "_previewed";

/// Edit › Step and Repeat, as in InDesign: Repeat (Count, or Rows and Columns with Create as a
/// grid), Offset (Vertical, Horizontal, in the document's ruler units) and Preview, which shows
/// the copies on the page while the dialog is open.
fn step_and_repeat_dialog(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let (hu, vu) = app.session.active().map_or((Unit::Picas, Unit::Picas), |s| (s.doc.settings.horizontal_units, s.doc.settings.vertical_units));
    for (k, v) in [
        ("count", json!("1")),
        ("rows", json!("1")),
        ("columns", json!("1")),
        ("vertical", json!(format_measure(12.0, vu))),
        ("horizontal", json!(format_measure(12.0, hu))),
    ] {
        d.fields.entry(k).or_insert(v);
    }
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            crate::rtl::label(ui, egui::RichText::new(tr("Repeat")).font(semibold(12.0)));
            egui::Grid::new("sr_repeat").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                let grid = d.b("grid");
                let fields: &[(&str, &str)] = if grid { &[("rows", "Rows:"), ("columns", "Columns:")] } else { &[("count", "Count:")] };
                for (k, l) in fields {
                    // A grid's columns, not a text frame's.
                    crate::rtl::label(ui, crate::i18n::tr_context(&lang, l, "grid"));
                    text_field(ui, d, k, 70.0);
                    ui.end_row();
                }
                crate::rtl::label(ui, "");
                check(ui, d, "grid", &tr("Create as a grid"));
                ui.end_row();
            });
            ui.add_space(10.0);
            crate::rtl::label(ui, egui::RichText::new(tr("Offset")).font(semibold(12.0)));
            ui.horizontal(|ui| {
                for (k, l) in [("vertical", "Vertical:"), ("horizontal", "Horizontal:")] {
                    crate::rtl::label(ui, tr(l));
                    text_field(ui, d, k, 80.0);
                    ui.add_space(12.0);
                }
            });
        });
        ui.add_space(32.0);
        check(ui, d, "preview", &tr("Preview"));
    });
    step_and_repeat_preview(app, d);
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

/// `edit.stepAndRepeat`'s params from the dialog: whole counts, offsets in the ruler units
/// (or any unit typed: "-0.75 in", "3p"). Empty fields are 1 copy and 1p0.
fn step_and_repeat_params(app: &DesignApp, d: &Dialog) -> Result<Value, String> {
    let (hu, vu) = app.session.active().map_or((Unit::Picas, Unit::Picas), |s| (s.doc.settings.horizontal_units, s.doc.settings.vertical_units));
    let invalid = |s: &str| format!("{}: {}", crate::i18n::tr(&app.ui.language, "Invalid value"), s.trim());
    // Fields not shown yet (a dialog confirmed over the control channel) take the defaults.
    let whole = |k: &str| {
        let s = d.s(k);
        if s.trim().is_empty() { Ok(1) } else { s.trim().parse::<u64>().map_err(|_| invalid(&s)) }
    };
    let measure = |k: &str, u: Unit| {
        let s = d.s(k);
        if s.trim().is_empty() {
            return Ok(12.0);
        }
        parse_measure(&s, u).ok().filter(|v| v.is_finite()).ok_or_else(|| invalid(&s))
    };
    let (dx, dy) = (measure("horizontal", hu)?, measure("vertical", vu)?);
    Ok(if d.b("grid") {
        json!({"rows": whole("rows")?, "columns": whole("columns")?, "dx": dx, "dy": dy})
    } else {
        json!({"count": whole("count")?, "dx": dx, "dy": dy})
    })
}

/// Preview on: the copies show on the page (redone when a field changes); off: they go.
fn step_and_repeat_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let params = match step_and_repeat_params(app, d) {
        Ok(p) => p,
        Err(e) => {
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction("Step and Repeat")
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview("edit.stepAndRepeat", params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// The corners of Object › Corner Options, in `object.cornerOptions` path order (top-left,
/// top-right, bottom-right, bottom-left), with the tip InDesign gives each.
const CORNERS: [(&str, &str); 4] = [("0", "Top Left:"), ("1", "Top Right:"), ("2", "Bottom Right:"), ("3", "Bottom Left:")];

/// The Corner Options dialog's fields from the first selected object's corners, once: each
/// corner's size (in the ruler units) and shape, linked when all four are the same.
fn seed_corner_options(app: &DesignApp, d: &mut Dialog) {
    use designcraft_geom::corners::Corner;
    let Some(st) = app.session.active() else { return };
    d.unit = st.doc.settings.horizontal_units;
    if d.fields.contains_key("_seeded") {
        return;
    }
    let corners: [Corner; 4] = st.selection.items.iter().find_map(|id| st.doc.item(*id)).map(|it| it.corners.corners).unwrap_or_default();
    for ((k, _), c) in CORNERS.iter().zip(corners) {
        d.fields.entry(format!("size{k}")).or_insert(json!(format_measure(c.size, d.unit)));
        d.fields.entry(format!("shape{k}")).or_insert(serde_json::to_value(c.shape).unwrap_or(json!("none")));
    }
    let first = corners[0];
    d.fields.entry("link").or_insert(json!(corners.iter().all(|c| *c == first)));
    d.fields.insert("_seed".into(), json!(corner_fields(d)));
    d.fields.insert("_seeded".into(), json!(true));
}

/// Each corner's size and shape fields, in `CORNERS` order.
fn corner_fields(d: &Dialog) -> Vec<(String, String)> {
    CORNERS.iter().map(|(k, _)| (d.s(&format!("size{k}")), d.s(&format!("shape{k}")))).collect()
}

/// The corners edited since the dialog opened: changed from the first object's, or set by a
/// linked edit (all four, even one that was already the value).
fn edited_corners(d: &Dialog) -> [bool; 4] {
    let touched: [bool; 4] = d.fields.get("_touched").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let seed: Vec<(String, String)> = d.fields.get("_seed").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let now = corner_fields(d);
    std::array::from_fn(|i| touched[i] || seed.get(i) != now.get(i))
}

/// Linked corners: a corner changed since the dialog last looked (size or shape, typed or set
/// over the control channel) is all four.
fn link_corners(d: &mut Dialog) {
    let now = corner_fields(d);
    let seen: Vec<(String, String)> = d.fields.get("_corners").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_else(|| now.clone());
    let mut now = now;
    let mut touched: [bool; 4] = d.fields.get("_touched").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    for (t, (a, b)) in touched.iter_mut().zip(now.iter().zip(&seen)) {
        *t |= a != b;
    }
    if d.b("link")
        && let Some(i) = now.iter().zip(&seen).position(|(a, b)| a != b)
        && let Some(changed) = now.get(i).cloned()
    {
        for (k, _) in &CORNERS {
            d.fields.insert(format!("size{k}"), json!(changed.0));
            d.fields.insert(format!("shape{k}"), json!(changed.1));
        }
        now = vec![changed; CORNERS.len()];
        touched = [true; 4];
    }
    d.fields.insert("_corners".into(), json!(now));
    d.fields.insert("_touched".into(), json!(touched));
}

/// `object.cornerOptions`' params from the dialog: the corners edited, the others null (each
/// selected object keeps its own). A size that does not parse, or a shape the dialog doesn't
/// offer, is an error to show.
fn corner_options_params(d: &Dialog) -> Result<Value, String> {
    use designcraft_geom::corners::CornerShape;
    let edited = edited_corners(d);
    let mut corners = Vec::with_capacity(CORNERS.len());
    for ((k, _), edited) in CORNERS.iter().zip(edited) {
        let key = format!("size{k}");
        let size =
            d.m(&key).filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| format!("Enter a valid corner size, e.g. 12 or 3 mm: {}", d.s(&key)))?;
        let shape: CornerShape = match d.fields.get(&format!("shape{k}")) {
            None => CornerShape::default(),
            Some(v) => serde_json::from_value(v.clone()).map_err(|_| format!("Choose a corner shape: {v} isn't one"))?,
        };
        corners.push(if edited { json!({"shape": shape, "size": size}) } else { Value::Null });
    }
    Ok(json!({"corners": corners}))
}

/// A corner's icon: the corner of a small square, with a dot at the corner it sets.
fn paint_corner_icon(ui: &mut egui::Ui, corner: usize, tip: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    resp.on_hover_text(tip);
    let b = r.shrink(3.0);
    ui.painter().rect_stroke(b, 0.0, egui::Stroke::new(1.0, t.icon), egui::StrokeKind::Inside);
    let at = match corner {
        0 => b.left_top(),
        1 => b.right_top(),
        2 => b.right_bottom(),
        _ => b.left_bottom(),
    };
    ui.painter().rect_filled(egui::Rect::from_center_size(at, egui::vec2(4.0, 4.0)), 0.0, t.accent);
}

/// Object › Corner Options, as InDesign's: under "Corner Size and Shape:", each corner's size and
/// shape (top corners above, bottom ones below, as on the frame), the link button that makes all
/// four the same, and Preview.
fn corner_options_dialog(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    use designcraft_geom::corners::CornerShape;
    seed_corner_options(app, d);
    link_corners(d);
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    crate::rtl::label(ui, tr("Corner Size and Shape:"));
    ui.add_space(8.0);
    let corner = |ui: &mut egui::Ui, d: &mut Dialog, i: usize| {
        let Some((k, tip)) = CORNERS.get(i) else { return };
        ui.horizontal(|ui| {
            paint_corner_icon(ui, i, &tr(tip));
            measure_field(ui, d, &format!("size{k}"), 70.0);
            let key = format!("shape{k}");
            let shape: CornerShape = d.fields.get(&key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            let resp = crate::panels::properties::corner_shape_button(ui, shape, 46.0);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, format!("{} {}", tr(tip), tr(shape.label()))));
            egui::Popup::menu(&resp).show(|ui| {
                ui.set_min_width(150.0);
                for s in CornerShape::ALL {
                    if ui.selectable_label(s == shape, crate::rtl::widget(ui, tr(s.label()))).clicked() {
                        d.fields.insert(key.clone(), json!(s));
                        ui.close();
                    }
                }
            });
        });
    };
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            corner(ui, d, 0);
            ui.add_space(6.0);
            corner(ui, d, 3);
        });
        ui.add_space(8.0);
        let linked = d.b("link");
        let tip = tr("Make all settings the same");
        let resp = ui
            .vertical(|ui| {
                ui.add_space(12.0);
                crate::icons::button(ui, if linked { "link" } else { "link-broken" }, 20.0, linked, &tip)
            })
            .inner;
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &tip));
        if resp.clicked() {
            d.fields.insert("link".into(), json!(!linked));
        }
        ui.add_space(8.0);
        ui.vertical(|ui| {
            corner(ui, d, 1);
            ui.add_space(6.0);
            corner(ui, d, 2);
        });
    });
    link_corners(d);
    ui.add_space(10.0);
    check(ui, d, "preview", &tr("Preview"));
    corner_options_preview(app, d);
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
}

/// Preview on: the corners show on the frame (redone when a field changes); off: they go back.
fn corner_options_preview(app: &mut DesignApp, d: &mut Dialog) {
    let shown = d.fields.get(PREVIEWED).cloned();
    if !d.b("preview") {
        if shown.is_some() {
            app.session.cancel_interaction();
            d.fields.remove(PREVIEWED);
            d.fields.remove("status");
        }
        return;
    }
    let params = match corner_options_params(d) {
        Ok(p) => p,
        Err(e) => {
            d.fields.insert("status".into(), json!(e));
            return;
        }
    };
    if shown.as_ref() == Some(&params) {
        return;
    }
    if shown.is_none()
        && let Err(e) = app.session.begin_interaction("Corner Options")
    {
        d.fields.insert("status".into(), json!(e.to_string()));
        return;
    }
    match app.session.preview("object.cornerOptions", params.clone()) {
        Ok(()) => d.fields.remove("status"),
        Err(e) => d.fields.insert("status".into(), json!(e.to_string())),
    };
    d.fields.insert(PREVIEWED.into(), params);
}

/// New Cross-Reference: link to a paragraph (paragraph styles on the left, their paragraphs on
/// the right) or a text anchor, with a cross-reference format.
fn insert_xref(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let Some(st) = app.session.active() else { return };
    let doc = &st.doc;
    let all = "[All Paragraphs]";
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Link To:"));
        translated_combo(&app.ui.language, ui, d, "linkTo", &[("paragraph", "Paragraph"), ("anchor", "Text Anchor")]);
    });
    ui.add_space(6.0);
    let dim = crate::theme::Tokens::get(ui.ctx()).text_dim;
    if d.s("linkTo") == "anchor" {
        egui::ScrollArea::vertical().id_salt("xr_anchors").max_height(220.0).max_width(460.0).show(ui, |ui| {
            let mut any = false;
            for st in doc.stories.values() {
                for a in &st.anchors {
                    any = true;
                    let key = format!("a:{}", a.id);
                    if ui.selectable_label(d.s("target") == key, &a.name).clicked() {
                        d.fields.insert("target".into(), json!(key));
                    }
                }
            }
            if !any {
                ui.label(crate::rtl::widget(
                    ui,
                    egui::RichText::new(crate::i18n::tr(&app.ui.language, "No text anchors in this document.")).color(dim),
                ));
            }
        });
    } else {
        let mut styles: Vec<String> = vec![all.to_string()];
        styles.extend(doc.styles.paragraph.iter().map(|s| s.name.clone()));
        let cur_style = if d.s("style").is_empty() { all.to_string() } else { d.s("style") };
        ui.horizontal_top(|ui| {
            let col = |w: f32| (egui::vec2(w, 240.0), egui::Layout::top_down(egui::Align::Min));
            let (sz, lay) = col(180.0);
            ui.allocate_ui_with_layout(sz, lay, |ui| {
                egui::ScrollArea::vertical().id_salt("xr_styles").auto_shrink([false, false]).show(ui, |ui| {
                    for name in &styles {
                        if ui.selectable_label(*name == cur_style, name).clicked() {
                            d.fields.insert("style".into(), json!(name));
                        }
                    }
                })
            });
            ui.add_space(8.0);
            let (sz, lay) = col(330.0);
            ui.allocate_ui_with_layout(sz, lay, |ui| {
                egui::ScrollArea::vertical().id_salt("xr_paras").auto_shrink([false, false]).show(ui, |ui| {
                    for st in doc.stories.values().filter(|s| !s.frames.is_empty()) {
                        for (pi, r) in st.para_ranges().into_iter().enumerate() {
                            if cur_style != all && st.paras[pi].style != cur_style {
                                continue;
                            }
                            let text = designcraft_doc::xref::clean(&st.text[r]);
                            if text.is_empty() {
                                continue;
                            }
                            let short: String = text.chars().take(52).collect();
                            let key = format!("{}:{pi}", st.id.0);
                            if ui.selectable_label(d.s("target") == key, short).clicked() {
                                d.fields.insert("target".into(), json!(key));
                            }
                        }
                    }
                })
            });
        });
    }
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Cross-Reference Format")).font(semibold(12.0)));
    let names: Vec<String> = doc.xref_formats.iter().map(|f| f.name.clone()).collect();
    if d.s("format").is_empty() {
        d.fields.insert("format".into(), json!(names.first().cloned().unwrap_or_default()));
    }
    let opts: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), n.as_str())).collect();
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Format:"));
        combo(ui, d, "format", &opts);
    });
    if let Some(m) = d.fields.get("status").and_then(Value::as_str) {
        ui.label(egui::RichText::new(m).color(dim));
    }
}

/// `N` numbers from a dialog field holding a JSON array.
fn nums<const N: usize>(d: &Dialog, k: &str) -> Option<[f64; N]> {
    let a = d.fields.get(k)?.as_array()?;
    if a.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (o, v) in out.iter_mut().zip(a) {
        *o = v.as_f64().filter(|x| x.is_finite())?;
    }
    Some(out)
}

const EYEDROPPER_DRAG: &str = "color_picker_eyedropper_dragging";

/// The Color Picker's eyedropper at `at`, as InDesign's: dragged onto the page, it takes the colour
/// shown under the pointer when it is let go. The colour taken, if any.
fn eyedropper(app: &mut DesignApp, ui: &mut egui::Ui, at: egui::Pos2, tip: String, t: &crate::theme::Tokens) -> Option<[u8; 3]> {
    let er = egui::Rect::from_center_size(at, egui::vec2(21.0, 21.0));
    let resp = ui.interact(er, egui::Id::unique("color_picker_eyedropper"), egui::Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &tip));
    if resp.hovered() || resp.dragged() {
        ui.painter().rect_filled(er, 3.0, t.hover);
    }
    crate::icons::paint(ui.painter(), egui::Rect::from_center_size(er.center(), egui::vec2(15.0, 15.0)), "tool-eyedropper", t.icon);
    // Esc during the drag cancels just the drag (the picker's own Esc/Enter handling stands down
    // while `EYEDROPPER_DRAG` is set), and the release takes nothing.
    let cancel_id = egui::Id::unique("color_picker_eyedropper_cancelled");
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            ui.data_mut(|d| d.insert_temp(cancel_id, true));
        }
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    }
    ui.data_mut(|d| d.insert_temp(egui::Id::unique(EYEDROPPER_DRAG), resp.dragged()));
    let cancelled = resp.drag_stopped() && ui.data_mut(|d| d.remove_temp::<bool>(cancel_id)).unwrap_or(false);
    // Let go over the picker itself it takes nothing, though a page may lie under it. (Its modal
    // backdrop shares the picker's layer and covers the screen: the picker is where it draws.)
    let mut taken = None;
    if resp.drag_stopped()
        && !cancelled
        && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
        && !ui.clip_rect().contains(pos)
    {
        taken = crate::canvas::color_at(app, ui.ctx(), pos);
    }
    resp.on_hover_text(tip);
    taken
}

/// Closes the picker for a click on OK or Cancel, else for Enter (OK) or Esc (Cancel) unless an
/// eyedropper drag is on.
fn picker_close(ui: &mut egui::Ui, d: &mut Dialog, ok: bool, cancel: bool) {
    let dragging = ui.data(|d| d.get_temp::<bool>(egui::Id::unique(EYEDROPPER_DRAG))).unwrap_or(false);
    let (enter, esc) = if dragging {
        (false, false)
    } else {
        (
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)),
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)),
        )
    };
    if ok || cancel {
        d.fields.insert("_close".into(), json!(if cancel { "cancel" } else { "ok" }));
    } else if enter || esc {
        d.fields.insert("_close".into(), json!(if enter { "ok" } else { "cancel" }));
    }
}

/// The Color Picker, laid out as InDesign's: the colour space view (a field of two components,
/// a slider for the third: the one whose radio button is on, R at first), the new colour over the
/// one it replaces, OK / Cancel / Add CMYK Swatch at the top right, H/S/B beside L/a/b, R/G/B
/// beside C/M/Y/K, and the hex code. Its buttons replace the dialog's own.
fn color_picker(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    use picker_layout as at;
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let old = d.s("hex");
    d.fields.entry("old").or_insert_with(|| json!(old));
    let comp = PickComp::from_key(&d.s("component"));
    let mut p = picked_of(d);
    let before = p;
    d.fields.entry("rgb").or_insert_with(|| json!(p.rgb.map(|x| x * 255.0)));
    if !d.fields.contains_key("cmyk") {
        d.fields.insert(
            "cmyk".into(),
            json!(
                designcraft_color::Color::Rgb { r: p.rgb[0], g: p.rgb[1], b: p.rgb[2] }
                    .to_cmyk_managed(designcraft_color::cms::active_settings().intent)
                    .map(|x| x * 100.0)
            ),
        );
    }
    let mut exact_cmyk = None;
    let t = crate::theme::Tokens::get(ui.ctx());
    let (area, _) = ui.allocate_exact_size(at::SIZE, egui::Sense::hover());
    let o = area.min.to_vec2();
    let put = |r: egui::Rect| r.translate(o);
    let font = egui::FontId::proportional(11.5);
    let text = |ui: &egui::Ui, pos: egui::Pos2, align: egui::Align2, s: &str| {
        ui.painter().text(pos, align, s, egui::FontId::proportional(11.5), t.text_strong);
    };
    // The colour space view (a label, so screen readers hear which).
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(area.min, egui::vec2(260.0, 24.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.label(
                egui::RichText::new(tr(if comp.lab() { "Lab Color Space View" } else { "RGB Color Space View" })).size(11.5).color(t.text_strong),
            );
        },
    );
    let fr = put(at::field());
    let fresp = ui.interact(fr, ui.scope_id().with("cp_field"), egui::Sense::click_and_drag());
    let (tv, _, _) = p.coords(comp);
    let cells = if matches!(comp, PickComp::R | PickComp::G | PickComp::Bl | PickComp::H) { 2 } else { 24 };
    gradient_mesh(ui.painter(), fr, cells, cells, |x, y| p.at(comp, tv, x, y).rgb);
    fresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Color field"));
    if let Some(pos) = fresp.interact_pointer_pos().filter(|_| fresp.is_pointer_button_down_on() || fresp.clicked()) {
        let u = ((pos.x - fr.left()) / fr.width()).clamp(0.0, 1.0);
        let v = ((fr.bottom() - pos.y) / fr.height()).clamp(0.0, 1.0);
        p = p.at(comp, tv, u, v);
    }
    // The marker: a cross with an open centre, white over black. Fixed colours: it sits on the
    // colour field, whatever the theme.
    let (_, u, v) = p.coords(comp);
    let m = egui::pos2(fr.left() + u * fr.width(), fr.bottom() - v * fr.height());
    let painter = ui.painter().with_clip_rect(fr);
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let (a, b) = (m + egui::vec2(dx * 3.0, dy * 3.0), m + egui::vec2(dx * 10.0, dy * 10.0));
        let side = egui::vec2(dy, dx) * 1.2;
        painter.line_segment([a + side, b + side], egui::Stroke::new(1.0, egui::Color32::BLACK));
        painter.line_segment([a - side, b - side], egui::Stroke::new(1.0, egui::Color32::BLACK));
        painter.line_segment([a, b], egui::Stroke::new(1.5, egui::Color32::WHITE));
    }
    // The slider, with a marker on each side (white with a dark edge, readable on any colour).
    let sr = put(at::slider());
    let sresp = ui.interact(sr.expand2(egui::vec2(6.0, 0.0)), ui.scope_id().with("cp_slider"), egui::Sense::click_and_drag());
    gradient_mesh(ui.painter(), sr, 1, 48, |_, y| p.slider_at(comp, y));
    sresp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Color slider"));
    if let Some(pos) = sresp.interact_pointer_pos().filter(|_| sresp.is_pointer_button_down_on() || sresp.clicked()) {
        let tn = ((sr.bottom() - pos.y) / sr.height()).clamp(0.0, 1.0);
        let (_, u, v) = p.coords(comp);
        p = p.at(comp, tn, u, v);
    }
    let (tn, _, _) = p.coords(comp);
    let y = sr.bottom() - tn * sr.height();
    for (x, dir) in [(sr.left(), 1.0), (sr.right(), -1.0)] {
        let pts = vec![
            egui::pos2(x - 5.0 * dir, y - 4.5),
            egui::pos2(x, y - 4.5),
            egui::pos2(x + 3.5 * dir, y),
            egui::pos2(x, y + 4.5),
            egui::pos2(x - 5.0 * dir, y + 4.5),
        ];
        ui.painter().add(egui::Shape::convex_polygon(pts, egui::Color32::WHITE, egui::Stroke::new(1.0, egui::Color32::from_gray(40))));
    }
    // The new colour over the one it replaces.
    let pr = put(at::swatch());
    let old = designcraft_color::Color::from_hex(&d.s("old")).map_or([0.0; 3], |c| c.to_rgb_uncalibrated());
    ui.painter().rect_filled(egui::Rect::from_min_max(pr.min, egui::pos2(pr.max.x, pr.center().y)), 0.0, color32(p.rgb));
    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(pr.min.x, pr.center().y), pr.max), 0.0, color32(old));
    ui.painter().rect_stroke(pr, 0.0, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    // OK, Cancel, Add CMYK Swatch: rounded, OK filled.
    let pill = |ui: &mut egui::Ui, i: usize, label: &str, primary: bool| -> bool {
        let r = put(at::button(i));
        let resp = ui.interact(r, ui.scope_id().with(("cp_button", i)), egui::Sense::click());
        let (fill, fg, stroke) = if primary {
            (t.accent_strong, egui::Color32::WHITE, egui::Stroke::NONE)
        } else {
            (if resp.hovered() { t.hover } else { egui::Color32::TRANSPARENT }, t.text_strong, egui::Stroke::new(1.0, t.text_dim))
        };
        ui.painter().rect(r, r.height() / 2.0, fill, stroke, egui::StrokeKind::Inside);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), fg);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        resp.clicked()
    };
    // The picker draws its own buttons, so the generic footer's Enter/Esc handling is skipped.
    // The keys are consumed here so nothing else reacts to them as well, and a click on a pill
    // wins over a key pressed in the same frame, so the dialog closes once.
    // The footer's padded OK key, which every catalog has.
    let ok = pill(ui, 0, tr("  OK  ").trim(), true);
    let cancel = pill(ui, 1, &tr("Cancel"), false);
    picker_close(ui, d, ok, cancel);
    // The C/M/Y/K the fields show: the percentages typed or given, else the colour converted.
    let cmyk = nums::<4>(d, "cmyk").map(|v| v.map(|x| x as f32)).unwrap_or_else(|| {
        designcraft_color::Color::Rgb { r: p.rgb[0], g: p.rgb[1], b: p.rgb[2] }
            .to_cmyk_managed(designcraft_color::cms::active_settings().intent)
            .map(|v| v * 100.0)
    });
    if pill(ui, 2, &tr("Add CMYK Swatch"), false) {
        let [c, m, y, k] = cmyk.map(|v| v.round());
        match app.run("swatch.create", json!({"color": {"c": c, "m": m, "y": y, "k": k}})) {
            Ok(r) => app.status(format!("{} {}", tr("Added swatch"), r["name"].as_str().unwrap_or(""))),
            Err(e) => app.status(e),
        }
    }
    // The values: a radio button, a label, the field, a unit.
    let mut comp_now = comp;
    let mut edited: Option<Picked> = None;
    // (column x of the radio, row, component, label, key, value, range, unit)
    let rows: [(f32, usize, Option<PickComp>, &str, &str, f32, (f32, f32), &str); 13] = [
        (at::LEFT, 0, Some(PickComp::H), "H:", "h", p.hsb[0], (0.0, 360.0), "°"),
        (at::LEFT, 1, Some(PickComp::S), "S:", "s", p.hsb[1] * 100.0, (0.0, 100.0), "%"),
        (at::LEFT, 2, Some(PickComp::Br), "B:", "b", p.hsb[2] * 100.0, (0.0, 100.0), "%"),
        (at::RIGHT, 0, Some(PickComp::L), "L:", "l", p.lab[0], (0.0, 100.0), ""),
        (at::RIGHT, 1, Some(PickComp::A), "a:", "la", p.lab[1], (-128.0, 127.0), ""),
        (at::RIGHT, 2, Some(PickComp::Lb), "b:", "lb", p.lab[2], (-128.0, 127.0), ""),
        (at::LEFT, 3, Some(PickComp::R), "R:", "r", p.rgb[0] * 255.0, (0.0, 255.0), ""),
        (at::LEFT, 4, Some(PickComp::G), "G:", "g", p.rgb[1] * 255.0, (0.0, 255.0), ""),
        (at::LEFT, 5, Some(PickComp::Bl), "B:", "bl", p.rgb[2] * 255.0, (0.0, 255.0), ""),
        (at::RIGHT, 3, None, "C:", "c", cmyk[0], (0.0, 100.0), "%"),
        (at::RIGHT, 4, None, "M:", "m", cmyk[1], (0.0, 100.0), "%"),
        (at::RIGHT, 5, None, "Y:", "y", cmyk[2], (0.0, 100.0), "%"),
        (at::RIGHT, 6, None, "K:", "k", cmyk[3], (0.0, 100.0), "%"),
    ];
    for (x, row, c, label, key, value, range, unit) in rows {
        let cy = area.min.y + at::ROWS[row];
        let x = area.min.x + x;
        match c {
            Some(c) => {
                let rr = egui::Rect::from_center_size(egui::pos2(x, cy), egui::vec2(14.0, 14.0));
                let resp = ui.interact(rr, ui.scope_id().with(("cp_radio", key)), egui::Sense::click());
                ui.painter().circle(rr.center(), 6.0, t.input, egui::Stroke::new(1.0, t.text_dim));
                if comp_now == c {
                    ui.painter().circle_filled(rr.center(), 2.6, t.text_strong);
                }
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, comp_now == c, label));
                if resp.clicked() {
                    comp_now = c;
                }
                text(ui, egui::pos2(x + 11.0, cy), egui::Align2::LEFT_CENTER, label);
            }
            // C/M/Y/K have no radio button: their labels end at the field.
            None => text(ui, egui::pos2(x + 28.0, cy), egui::Align2::RIGHT_CENTER, label),
        }
        let fr = egui::Rect::from_min_size(egui::pos2(x + 35.0, cy - 10.5), egui::vec2(44.0, 21.0));
        if let Some(v) = picker_field(ui, d, fr, key, label.trim_end_matches(':'), value, range) {
            if matches!(key, "r" | "g" | "bl") {
                d.fields.insert("model".into(), json!("rgb"));
            }
            if matches!(key, "c" | "m" | "y" | "k") {
                d.fields.insert("model".into(), json!("cmyk"));
            }
            edited = Some(match key {
                "h" => Picked::from_hsb([v, p.hsb[1], p.hsb[2]]),
                "s" => Picked::from_hsb([p.hsb[0], v / 100.0, p.hsb[2]]),
                "b" => Picked::from_hsb([p.hsb[0], p.hsb[1], v / 100.0]),
                "l" => Picked::from_lab([v, p.lab[1], p.lab[2]], p.hsb[0]),
                "la" => Picked::from_lab([p.lab[0], v, p.lab[2]], p.hsb[0]),
                "lb" => Picked::from_lab([p.lab[0], p.lab[1], v], p.hsb[0]),
                "r" => Picked::from_rgb([v / 255.0, p.rgb[1], p.rgb[2]], p.hsb[0]),
                "g" => Picked::from_rgb([p.rgb[0], v / 255.0, p.rgb[2]], p.hsb[0]),
                "bl" => Picked::from_rgb([p.rgb[0], p.rgb[1], v / 255.0], p.hsb[0]),
                _ => {
                    let mut c = cmyk;
                    c[["c", "m", "y", "k"].iter().position(|k| *k == key).unwrap_or(0)] = v;
                    exact_cmyk = Some(c);
                    let [c, m, y, k] = c.map(|x| x / 100.0);
                    Picked::from_rgb(designcraft_color::Color::Cmyk { c, m, y, k }.to_rgb(), p.hsb[0])
                }
            });
        }
        if !unit.is_empty() {
            text(ui, egui::pos2(fr.right() + 8.0, cy), egui::Align2::LEFT_CENTER, unit);
        }
    }
    if let Some(e) = edited {
        p = e;
    }
    if comp_now != comp {
        d.fields.insert("component".into(), json!(comp_now.key()));
    }
    // The hex code, and the eyedropper beside it.
    let cy = area.min.y + at::ROWS[7];
    text(ui, egui::pos2(area.min.x + 321.0, cy), egui::Align2::LEFT_CENTER, "#:");
    let hr = egui::Rect::from_min_size(egui::pos2(area.min.x + 341.0, cy - 10.5), egui::vec2(64.0, 21.0));
    let id = egui::Id::unique("color_picker_hex");
    let editing = ui.memory(|m| m.has_focus(id));
    let mut h = if editing { d.s("hexEdit") } else { p.hex().trim_start_matches('#').to_string() };
    if ui
        .put(hr, egui::TextEdit::singleline(&mut h).id(id).font(font).align(egui::Align2::LEFT_CENTER).margin(egui::Margin::symmetric(4, 2)))
        .accessible_name("Hex".to_owned())
        .changed()
        && (h.len() == 6 || (h.len() == 7 && h.starts_with('#')))
        && let Some(c) = designcraft_color::Color::from_hex(&h)
    {
        p = Picked::from_rgb(c.to_rgb_uncalibrated(), p.hsb[0]);
    }
    d.fields.insert("hexEdit".into(), json!(h));
    if let Some(c) = eyedropper(app, ui, egui::pos2(hr.right() + 16.0, cy), tr("Eyedropper: drag it onto the page"), &t) {
        p = Picked::from_rgb(c.map(|v| f32::from(v) / 255.0), p.hsb[0]);
    }
    if p != before {
        store_picked(d, &p);
    }
    // Typed C/M/Y/K are kept as typed, even when the colour shown stays the same (at 100% K, a
    // device conversion is black whatever C, M and Y are).
    if let Some(c) = exact_cmyk {
        d.fields.insert("cmyk".into(), json!(c));
        d.fields.insert("edited".into(), json!(true));
    }
}

/// Find/Replace Font: the document's fonts (missing ones first, flagged) and a replacement.
/// Load Styles: a checkbox for each style of the picked file, and what happens to a style this
/// document defines differently under the same name.
fn load_styles(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let dim = crate::theme::Tokens::get(ui.ctx()).text_dim;
    ui.label(egui::RichText::new(d.s("path")).size(10.5));
    ui.horizontal(|ui| {
        for (label, on) in [("Select All", true), ("Deselect All", false)] {
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                for key in ["paragraph", "character"] {
                    for row in d.fields.get_mut(key).and_then(Value::as_array_mut).into_iter().flatten().filter_map(Value::as_object_mut) {
                        row.insert("on".into(), json!(on));
                    }
                }
            }
        }
    });
    let mut conflicts = 0;
    egui::ScrollArea::vertical().id_salt("load_styles").max_height(240.0).show(ui, |ui| {
        for (key, title) in [("paragraph", "Paragraph Styles"), ("character", "Character Styles")] {
            let Some(rows) = d.fields.get_mut(key).and_then(Value::as_array_mut).filter(|rows| !rows.is_empty()) else { continue };
            crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, title)).font(semibold(12.0)));
            for row in rows.iter_mut().filter_map(Value::as_object_mut) {
                let Some(name) = row.get("name").and_then(Value::as_str).map(str::to_string) else { continue };
                let (mut on, conflict) = (row.get("on") != Some(&json!(false)), row.get("conflict") == Some(&json!(true)));
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::style_name(lang, &name))).changed() {
                        row.insert("on".into(), json!(on));
                    }
                    if conflict {
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Differs from this document")).size(10.5).color(dim));
                    }
                });
                conflicts += usize::from(on && conflict);
            }
        }
    });
    ui.add_space(4.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(lang, "Style Name Conflicts")).font(semibold(12.0)));
    if conflicts == 0 {
        crate::rtl::label(ui, crate::i18n::tr(lang, "No conflicts"));
        return;
    }
    let cur = d.s("conflicts");
    ui.horizontal(|ui| {
        for (v, l) in [("redefine", "Redefine Document Style"), ("useExisting", "Use Document Style Definition"), ("autoRename", "Auto Rename")] {
            if ui.radio(cur == v, crate::rtl::widget(ui, crate::i18n::tr(lang, l))).clicked() {
                d.fields.insert("conflicts".into(), json!(v));
            }
        }
    });
}

fn find_font(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    // Listed once when the dialog opens (it scans every story).
    if !d.fields.contains_key("_fonts") {
        let list = app.session.execute("font.list", &json!({})).unwrap_or_default();
        d.fields.insert("_fonts".into(), list);
    }
    let fonts = d.fields.get("_fonts").and_then(Value::as_array).cloned().unwrap_or_default();
    if !fonts.iter().any(|font| font["family"] == d.s("family") && font["style"] == d.s("style"))
        && let Some(font) = fonts.first()
    {
        d.fields.insert("family".into(), font["family"].clone());
        d.fields.insert("style".into(), font["style"].clone());
    }
    if d.b("onOpen") && fonts.iter().any(|f| f["missing"] == true || f["styleMissing"] == true) {
        crate::rtl::label(
            ui,
            crate::i18n::tr(
                &app.ui.language,
                "This document uses unavailable fonts. Choose replacements, or select Done to continue with fallback fonts.",
            ),
        );
    }
    if !d.s("error").is_empty() {
        crate::rtl::label(ui, d.s("error"));
    }
    let missing = fonts.iter().filter(|f| f["missing"] == true || f["styleMissing"] == true).count();
    crate::rtl::label(
        ui,
        crate::i18n::tr(&app.ui.language, "Fonts in Document: {count}    Missing: {missing}")
            .replace("{count}", &fonts.len().to_string())
            .replace("{missing}", &missing.to_string()),
    );
    egui::ScrollArea::vertical().id_salt("ff_list").max_height(180.0).show(ui, |ui| {
        for f in &fonts {
            let (fam, st) = (f["family"].as_str().unwrap_or(""), f["style"].as_str().unwrap_or(""));
            let warn = f["missing"] == true || f["styleMissing"] == true;
            let label = format!("{}{fam} {st}", if warn { "\u{26A0} " } else { "" });
            let on = d.s("family") == fam && d.s("style") == st;
            let mut text = egui::RichText::new(label);
            if warn && !on {
                text = text.color(crate::theme::Tokens::get(ui.ctx()).text_strong);
            }
            if ui.selectable_label(on, text).clicked() {
                d.fields.insert("family".into(), json!(fam));
                d.fields.insert("style".into(), json!(st));
            }
        }
    });
    let key = format!("{}\n{}", d.s("family"), d.s("style"));
    if let Some(font) = fonts.iter().find(|f| f["family"] == d.s("family") && f["style"] == d.s("style")) {
        if d.s("_fontKey") != key {
            d.fields.insert("_fontKey".into(), json!(key));
            d.fields.insert("toFamily".into(), font["replacementFamily"].clone());
            d.fields.insert("toStyle".into(), font["replacementStyle"].clone());
        }
        crate::rtl::label(
            ui,
            format!(
                "{}: {} {}",
                crate::i18n::tr(&app.ui.language, "Currently Rendered With"),
                font["resolvedFamily"].as_str().unwrap_or(""),
                font["resolvedStyle"].as_str().unwrap_or("")
            ),
        );
    }
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Replace With")).font(semibold(12.0)));
    let db = crate::panels::fonts(app);
    if d.s("toFamily").is_empty() {
        d.fields.insert("toFamily".into(), json!(designcraft_fonts::FALLBACK_FAMILY));
    }
    egui::Grid::new("ff_to").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Family:"));
        if let Some(f) = crate::panels::font_combo(app, ui, "toFamily", &d.s("toFamily"), 170.0) {
            let style = crate::panels::pick_style(app, &f);
            d.fields.insert("toFamily".into(), json!(f.family));
            d.fields.insert("toStyle".into(), json!(style));
        }
        ui.end_row();
        let styles = db.styles(&d.s("toFamily"));
        if !styles.iter().any(|style| style == &d.s("toStyle")) {
            let preferred = d.s("style");
            let style = styles
                .iter()
                .find(|s| s.eq_ignore_ascii_case(&preferred))
                .or_else(|| styles.iter().find(|s| s.eq_ignore_ascii_case("Regular")))
                .or_else(|| styles.first());
            if let Some(style) = style {
                d.fields.insert("toStyle".into(), json!(style));
            }
        }
        let st_opts: Vec<(&str, &str)> = styles.iter().map(|s| (s.as_str(), s.as_str())).collect();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Style:"));
        combo(ui, d, "toStyle", &st_opts);
        ui.end_row();
    });
    ui.label(crate::rtl::widget(
        ui,
        egui::RichText::new(crate::i18n::tr(
            &app.ui.language,
            "Replace All updates this font throughout text and styles. Done keeps remaining fallback fonts.",
        ))
        .color(crate::theme::Tokens::get(ui.ctx()).text_dim)
        .size(11.0),
    ));
}

#[cfg(test)]
mod tests;

/// The Color Picker's components: the one picked runs along the slider, the other two of its
/// space across the field (InDesign's, as Photoshop's: R puts B across and G up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickComp {
    H,
    S,
    Br,
    L,
    A,
    Lb,
    R,
    G,
    Bl,
}

impl PickComp {
    const ALL: [PickComp; 9] =
        [PickComp::H, PickComp::S, PickComp::Br, PickComp::L, PickComp::A, PickComp::Lb, PickComp::R, PickComp::G, PickComp::Bl];

    fn key(self) -> &'static str {
        match self {
            PickComp::H => "h",
            PickComp::S => "s",
            PickComp::Br => "b",
            PickComp::L => "l",
            PickComp::A => "la",
            PickComp::Lb => "lb",
            PickComp::R => "r",
            PickComp::G => "g",
            PickComp::Bl => "bl",
        }
    }

    fn from_key(k: &str) -> PickComp {
        PickComp::ALL.into_iter().find(|c| c.key() == k).unwrap_or(PickComp::R)
    }

    fn lab(self) -> bool {
        matches!(self, PickComp::L | PickComp::A | PickComp::Lb)
    }
}

/// The colour the picker shows, in each of its spaces (HSB keeps its hue through greys).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Picked {
    pub rgb: [f32; 3],
    pub hsb: [f32; 3],
    pub lab: [f32; 3],
}

impl Picked {
    pub fn from_rgb(rgb: [f32; 3], hue: f32) -> Picked {
        let rgb = rgb.map(|v| v.clamp(0.0, 1.0));
        let c = designcraft_color::Color::Rgb { r: rgb[0], g: rgb[1], b: rgb[2] };
        let mut hsb = c.to_hsb();
        if hsb[1] <= 1e-4 || hsb[2] <= 1e-4 {
            hsb[0] = hue;
        }
        let l = designcraft_color::cms::lab::srgb_to_lab(rgb);
        Picked { rgb, hsb, lab: [l.l, l.a, l.b] }
    }

    fn from_hsb(hsb: [f32; 3]) -> Picked {
        let hsb = [hsb[0].rem_euclid(360.0), hsb[1].clamp(0.0, 1.0), hsb[2].clamp(0.0, 1.0)];
        let mut p = Picked::from_rgb(designcraft_color::Color::from_hsb(hsb[0], hsb[1], hsb[2]).to_rgb_uncalibrated(), hsb[0]);
        p.hsb = hsb;
        p
    }

    fn from_lab(lab: [f32; 3], hue: f32) -> Picked {
        let lab = [lab[0].clamp(0.0, 100.0), lab[1].clamp(-128.0, 127.0), lab[2].clamp(-128.0, 127.0)];
        let mut p = Picked::from_rgb(designcraft_color::cms::lab::lab_to_srgb(designcraft_color::cms::Lab::new(lab[0], lab[1], lab[2])), hue);
        p.lab = lab;
        p
    }

    /// (slider, across, up), each 0..1, for component `c`.
    pub fn coords(&self, c: PickComp) -> (f32, f32, f32) {
        let [r, g, b] = self.rgb;
        let [h, s, v] = self.hsb;
        let [l, la, lb] = self.lab;
        let (a, bb) = ((la + 128.0) / 255.0, (lb + 128.0) / 255.0);
        match c {
            PickComp::R => (r, b, g),
            PickComp::G => (g, b, r),
            PickComp::Bl => (b, r, g),
            PickComp::H => (h / 360.0, s, v),
            PickComp::S => (s, h / 360.0, v),
            PickComp::Br => (v, h / 360.0, s),
            PickComp::L => (l / 100.0, a, bb),
            PickComp::A => (a, bb, l / 100.0),
            PickComp::Lb => (bb, a, l / 100.0),
        }
    }

    /// The colour at (slider `t`, across `u`, up `v`) for component `c`.
    pub fn at(&self, c: PickComp, t: f32, u: f32, v: f32) -> Picked {
        let hue = self.hsb[0];
        let ab = |x: f32| x * 255.0 - 128.0;
        match c {
            PickComp::R => Picked::from_rgb([t, v, u], hue),
            PickComp::G => Picked::from_rgb([v, t, u], hue),
            PickComp::Bl => Picked::from_rgb([u, v, t], hue),
            PickComp::H => Picked::from_hsb([t * 360.0, u, v]),
            PickComp::S => Picked::from_hsb([u * 360.0, t, v]),
            PickComp::Br => Picked::from_hsb([u * 360.0, v, t]),
            PickComp::L => Picked::from_lab([t * 100.0, ab(u), ab(v)], hue),
            PickComp::A => Picked::from_lab([v * 100.0, ab(t), ab(u)], hue),
            PickComp::Lb => Picked::from_lab([v * 100.0, ab(u), ab(t)], hue),
        }
    }

    /// The slider's colour at `t`: the hue slider shows pure hues, the others this colour with
    /// the component at `t`.
    fn slider_at(&self, c: PickComp, t: f32) -> [f32; 3] {
        if c == PickComp::H {
            return designcraft_color::Color::from_hsb(t * 360.0, 1.0, 1.0).to_rgb_uncalibrated();
        }
        let (_, u, v) = self.coords(c);
        self.at(c, t, u, v).rgb
    }

    pub fn hex(&self) -> String {
        let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", q(self.rgb[0]), q(self.rgb[1]), q(self.rgb[2]))
    }
}

fn color32(rgb: [f32; 3]) -> egui::Color32 {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(q(rgb[0]), q(rgb[1]), q(rgb[2]))
}

/// A `cols` × `rows` grid of quads over `r`, coloured at each vertex by `f(x, y)` (0..1, y up).
fn gradient_mesh(painter: &egui::Painter, r: egui::Rect, cols: usize, rows: usize, f: impl Fn(f32, f32) -> [f32; 3]) {
    let mut mesh = egui::Mesh::default();
    for j in 0..=rows {
        for i in 0..=cols {
            let (x, y) = (i as f32 / cols as f32, j as f32 / rows as f32);
            mesh.colored_vertex(egui::pos2(r.left() + x * r.width(), r.bottom() - y * r.height()), color32(f(x, y)));
        }
    }
    let w = (cols + 1) as u32;
    for j in 0..rows as u32 {
        for i in 0..cols as u32 {
            let a = j * w + i;
            mesh.add_triangle(a, a + 1, a + w);
            mesh.add_triangle(a + 1, a + w + 1, a + w);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The colour in the dialog's fields: `hex`, with the HSB hue and the Lab values kept while
/// they round-trip (`_hsb`, `_lab`).
fn picked_of(d: &Dialog) -> Picked {
    let mut rgb = designcraft_color::Color::from_hex(&d.s("hex")).map_or([0.0; 3], |c| c.to_rgb_uncalibrated());
    if let Some(raw) = nums::<3>(d, "rgb") {
        let raw = raw.map(|x| (x / 255.0) as f32);
        if Picked::from_rgb(raw, 0.0).hex() == Picked::from_rgb(rgb, 0.0).hex() {
            rgb = raw;
        }
    }
    let hue = nums::<3>(d, "_hsb").map_or(0.0, |h| h[0] as f32);
    let mut p = Picked::from_rgb(rgb, hue);
    // The exact values the picker last set, while they still give this colour.
    if let Some(h) = nums::<3>(d, "_hsb").map(|h| h.map(|v| v as f32))
        && Picked::from_hsb(h).hex() == p.hex()
    {
        p.hsb = h;
    }
    if let Some(l) = nums::<3>(d, "_lab").map(|l| l.map(|v| v as f32))
        && Picked::from_lab(l, p.hsb[0]).hex() == p.hex()
    {
        p.lab = l;
    }
    p
}

fn store_picked(d: &mut Dialog, p: &Picked) {
    d.fields.insert("edited".into(), json!(true));
    d.fields.insert("rgb".into(), json!(p.rgb.map(|x| x * 255.0)));
    d.fields.insert(
        "cmyk".into(),
        json!(
            designcraft_color::Color::Rgb { r: p.rgb[0], g: p.rgb[1], b: p.rgb[2] }
                .to_cmyk_managed(designcraft_color::cms::active_settings().intent)
                .map(|x| x * 100.0)
        ),
    );
    d.fields.insert("hex".into(), json!(p.hex()));
    d.fields.insert("_hsb".into(), json!(p.hsb));
    d.fields.insert("_lab".into(), json!(p.lab));
}

/// The Color Picker's place in the dialog, as InDesign's (points from the body's top-left).
mod picker_layout {
    use egui::{Rect, pos2, vec2};
    // InDesign's proportions at this app's density (its 30 px fields are 21 pt here).
    pub const SIZE: egui::Vec2 = vec2(566.0, 334.0);
    pub fn field() -> Rect {
        Rect::from_min_size(pos2(2.0, 31.0), vec2(265.0, 287.0))
    }
    pub fn slider() -> Rect {
        Rect::from_min_size(pos2(285.0, 32.0), vec2(19.0, 282.0))
    }
    pub fn swatch() -> Rect {
        Rect::from_min_size(pos2(319.0, 0.0), vec2(71.0, 90.0))
    }
    /// OK, Cancel, Add … Swatch.
    pub fn button(i: usize) -> Rect {
        Rect::from_min_size(pos2(433.0, [0.0, 39.0, 78.0][i.min(2)]), vec2(132.0, 25.0))
    }
    /// Row centres: H/L, S/a, B/b, (gap), R/C, G/M, B/Y, K, #.
    pub const ROWS: [f32; 8] = [129.0, 157.0, 185.0, 224.0, 252.0, 280.0, 308.0, 320.0];
    /// The radio buttons' columns.
    pub const LEFT: f32 = 326.0;
    pub const RIGHT: f32 = 457.0;
}

/// A value field, named `name` (its caption): the text typed stays while it has the focus; a
/// number that parses applies.
fn picker_field(ui: &mut egui::Ui, d: &mut Dialog, r: egui::Rect, key: &str, name: &str, value: f32, range: (f32, f32)) -> Option<f32> {
    let id = egui::Id::unique(("color_picker_field", key));
    let editing = ui.memory(|m| m.has_focus(id));
    let shown = format!("{}", value.round() as i64);
    let edit_key = format!("_edit_{key}");
    let mut text = if editing { d.fields.get(&edit_key).and_then(Value::as_str).map(str::to_string).unwrap_or(shown) } else { shown };
    let resp = ui
        .put(
            r,
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .font(egui::FontId::proportional(11.5))
                .align(egui::Align2::LEFT_CENTER)
                .margin(egui::Margin::symmetric(4, 2)),
        )
        .accessible_name(name.to_owned());
    let mut out = None;
    if resp.changed()
        && let Ok(v) = text.trim().parse::<f32>()
        && v.is_finite()
    {
        out = Some(v.clamp(range.0, range.1));
    }
    d.fields.insert(edit_key, json!(text));
    out
}
