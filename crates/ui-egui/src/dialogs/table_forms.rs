//! Insert Table, Convert Text to Table and Convert Table to Text on the framework's form components. Each form edits the
//! same `Dialog` draft the native bodies (`insert_table_dialog`, `convert_table_dialog`) edit, so OK runs the same
//! `table.insert`, `table.convertFromText` or `table.convertToText` with the same parameters.

use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::form::{Field, FieldGroup};
use craft_ui_widgets::layout_components::Stack;
use craft_ui_widgets::pickers::{ComboBox, Select};
use craft_ui_widgets::text::NumberField;
use designcraft_engine::cmd::table::{MAX_BODY_ROWS, MAX_COLUMNS, MAX_HEADER_FOOTER_ROWS};
use egui::Ui;
use serde_json::json;

use super::framework::Form;
use super::{Dialog, SEPARATORS, same_separators, table_style_field};
use crate::DesignApp;

/// Table › Insert Table.
pub(crate) const INSERT: Form = Form::new(insert_table);
/// Table › Convert Text to Table and Convert Table to Text.
pub(crate) const CONVERT: Form = Form::new(convert_table);

/// The names of the document's table styles.
fn table_styles(app: &DesignApp) -> Vec<String> {
    app.session.active().map(|st| st.doc.styles.table.iter().map(|s| s.name.clone()).collect()).unwrap_or_default()
}

/// Table Dimensions (body rows and columns, then header and footer rows, within `table.insert`'s limits) and the Table
/// Style to apply.
fn insert_table(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
    let styles = table_styles(app);
    let dimensions = [
        ("bodyRows", format!("{}:", tr("Body Rows")), 1.0, MAX_BODY_ROWS),
        ("columns", format!("{}:", crate::i18n::tr_context(&lang, "Columns", "table")), 1.0, MAX_COLUMNS),
        ("headerRows", format!("{}:", tr("Header Rows")), 0.0, MAX_HEADER_FOOTER_ROWS),
        ("footerRows", format!("{}:", tr("Footer Rows")), 0.0, MAX_HEADER_FOOTER_ROWS),
    ];
    let style_label = format!("{}:", tr("Table Style"));
    Stack::new().id_salt("insert_table_form").fill_width().show(ui, |ui, _| {
        GroupBox::new(tr("Table Dimensions"), None).id_salt("insert_table_dimensions").show(ui, |ui, _| {
            FieldGroup::new(tr("Table Dimensions")).id_salt("insert_table_fields").show(ui, |ui, _| {
                for (key, label, min, max) in &dimensions {
                    Field::new(label.clone()).id_salt(("insert_table_field", *key)).show(ui, |ui, _| {
                        count_field(ui, d, key, label, *min, *max as f64);
                    });
                }
            });
        });
        Field::new(style_label.clone()).id_salt("insert_table_style_field").show(ui, |ui, _| {
            style_select(ui, d, &styles, &style_label);
        });
    });
    None
}

/// Column Separator and Row Separator, and, converting text to a table, Number of Columns (it applies only while both
/// separators are the same) and Table Style.
fn convert_table(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| format!("{}:", crate::i18n::tr(&lang, s));
    let to_table = d.id == "convertTextToTable";
    let styles = table_styles(app);
    let title = super::dialog_title(app, d);
    Stack::new().id_salt(("convert_table_form", to_table)).fill_width().show(ui, |ui, _| {
        FieldGroup::new(title.clone()).id_salt(("convert_table_fields", to_table)).show(ui, |ui, _| {
            for (key, text, salt) in
                [("columnSeparator", "Column Separator", "convert_column_separator"), ("rowSeparator", "Row Separator", "convert_row_separator")]
            {
                let label = tr(text);
                Field::new(label.clone()).id_salt(salt).show(ui, |ui, _| separator_field(ui, d, &lang, key, &label));
            }
            if !to_table {
                return;
            }
            // Read after the separators above, so the field follows an edit made in this pass.
            let same = same_separators(d);
            let columns = tr("Number of Columns");
            Field::new(columns.clone()).id_salt("convert_table_columns_field").enabled(same).show(ui, |ui, _| {
                count_field(ui, d, "columns", &columns, 1.0, MAX_COLUMNS as f64);
            });
            let style = tr("Table Style");
            Field::new(style.clone()).id_salt("convert_table_style_field").show(ui, |ui, _| style_select(ui, d, &styles, &style));
        });
    });
    None
}

/// A whole number in `key`, from `min` to `max`: what the field commits is rounded and stored as the native field
/// stored it.
fn count_field(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, min: f64, max: f64) {
    let out = NumberField::new(label.to_owned(), Bind::given(d.n(key)))
        .id_salt(("table_count", key))
        .range(min..=max)
        .step(1.0)
        .snap_typed(true)
        .fixed_decimals(0)
        .commit_decimals(0)
        .show(ui);
    let landed = out.edit.events.landed().copied().flatten().filter(|v| v.is_finite());
    if let Some(value) = landed
        && d.n(key) != Some(value.round())
    {
        d.fields.insert(key.into(), json!(value.round()));
    }
}

/// The Table Style choice: a style the document has, or [Basic Table] while none is chosen. A style that is gone (set
/// over the control channel) stays shown, so the command that refuses it can be read against the draft.
fn style_select(ui: &mut Ui, d: &mut Dialog, styles: &[String], label: &str) {
    let current = table_style_field(d).unwrap_or_else(|| designcraft_doc::BASIC_TABLE.to_string());
    let entries: Vec<Entry<String>> = styles.iter().map(|name| Entry::new(name.clone(), name.clone())).collect();
    let mut select = Select::new(label.to_owned(), Bind::given(current.clone()), &entries).id_salt("table_style_select");
    if !styles.contains(&current) {
        select = select.placeholder(current.clone());
    }
    let out = select.show(ui);
    let mut next = current.clone();
    for event in out.events {
        event.apply(&mut next);
    }
    if next != current {
        d.fields.insert("tableStyle".into(), json!(next));
    }
}

/// A separator field: type any characters, or pick Tab, Comma or Paragraph from the list. Stored as `tab`, `comma`,
/// `paragraph` or the typed text.
fn separator_field(ui: &mut Ui, d: &mut Dialog, lang: &str, key: &str, label: &str) {
    let name = |value: &str| SEPARATORS.iter().find(|(stored, _)| *stored == value).map(|(_, caption)| crate::i18n::tr(lang, caption).to_owned());
    let stored = d.s(key);
    let shown = name(&stored).unwrap_or_else(|| stored.clone());
    let options: Vec<Entry<String>> = SEPARATORS
        .iter()
        .map(|(_, caption)| {
            let text = crate::i18n::tr(lang, caption).to_owned();
            Entry::new(text.clone(), text)
        })
        .collect();
    let out = ComboBox::new(label.to_owned(), Bind::given(shown.clone()), &options).id_salt(("convert_separator", key)).show(ui);
    let mut next = shown.clone();
    for event in out.edit.events {
        event.apply(&mut next);
    }
    if next != shown {
        let value =
            SEPARATORS.iter().find(|(_, caption)| crate::i18n::tr(lang, caption) == next).map_or(next.clone(), |(stored, _)| (*stored).to_string());
        d.fields.insert(key.into(), json!(value));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    use serde_json::Value;

    /// A document with a text frame whose caret is ready for Insert Table.
    fn frame_app() -> DesignApp {
        let mut app = kit::app(1, false);
        app.run("style.table.create", json!({"name": "Grid"})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 372, 372], "content": "text"})).unwrap();
        app
    }

    /// A text frame holding four paragraphs, all selected, and its story.
    fn text_app() -> (DesignApp, u64) {
        let mut app = kit::app(1, false);
        let text = "Name:\nAddress:\nCity:\nState:";
        let story = app.run("frame.create", json!({"rect": [72, 72, 372, 372], "content": "text", "text": text})).unwrap()["story"].clone();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": text.len()})).unwrap();
        (app, story.as_u64().unwrap())
    }

    fn table_counts(h: &mut egui_kittest::Harness<'_, DesignApp>) -> (usize, usize) {
        let table = h.state_mut().run("table.get", json!({})).unwrap();
        let len = |key: &str| table[key].as_array().map_or(0, Vec::len);
        (len("rows"), len("columns"))
    }

    /// Guards the Insert Table port: every field of the framework form lands in the draft, and only OK runs
    /// `table.insert`, once, with the parameters the native form gave it, as one undo step.
    #[test]
    fn insert_table_form_edits_the_draft_and_ok_inserts_the_table_once() {
        let mut app = frame_app();
        app.run("app.insertTableDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let undo = kit::undo_steps(&h);
        kit::type_and_commit(&mut h, "Body Rows:", 0, "6");
        kit::type_and_commit(&mut h, "Columns:", 0, "3");
        kit::type_and_commit(&mut h, "Header Rows:", 0, "1");
        kit::pick(&mut h, "Table Style:", "Grid");
        let d = kit::draft(&h);
        assert_eq!((d.n("bodyRows"), d.n("columns"), d.n("headerRows"), d.n("footerRows")), (Some(6.0), Some(3.0), Some(1.0), Some(0.0)));
        assert_eq!(d.s("tableStyle"), "Grid");
        assert!(kit::journal(&h, "table.insert").is_empty(), "the fields only edit the draft");
        assert_eq!(kit::undo_steps(&h), undo);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(
            kit::journal(&h, "table.insert"),
            vec![("table.insert".to_owned(), json!({"rows": 6, "cols": 3, "headerRows": 1, "footerRows": 0, "tableStyle": "Grid"}))]
        );
        assert_eq!(table_counts(&mut h), (7, 3), "six body rows and a header row");
        assert_eq!(kit::undo_steps(&h), undo + 1);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert!(h.state_mut().run("table.get", json!({})).is_err(), "one undo takes the table away");
    }

    /// Guards Cancel on the Insert Table port: nothing is inserted and nothing is left in the history.
    #[test]
    fn insert_table_form_cancel_inserts_nothing() {
        let mut app = frame_app();
        app.run("app.insertTableDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let before = h.state().session.doc().unwrap().doc.clone();
        let undo = kit::undo_steps(&h);
        kit::type_and_commit(&mut h, "Body Rows:", 0, "9");
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
        assert!(kit::journal(&h, "table.insert").is_empty());
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        assert_eq!(kit::undo_steps(&h), undo);
    }

    /// Guards Convert Text to Table: Number of Columns follows the separators as the native form did (dim until both are
    /// the same, which a typed name and a typed character both are), and OK runs `table.convertFromText` with them.
    #[test]
    fn convert_text_to_table_form_applies_number_of_columns_only_for_equal_separators() {
        let (mut app, _) = text_app();
        app.run("app.convertTextToTableDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        assert!(kit::disabled(&h, &[Role::SpinButton], "Number of Columns:", 0), "tab and paragraph differ");
        kit::type_and_commit(&mut h, "Column Separator:", 0, ",");
        kit::type_and_commit(&mut h, "Row Separator:", 0, "Comma");
        let d = kit::draft(&h);
        assert_eq!((d.s("columnSeparator"), d.s("rowSeparator")), (",".to_owned(), "comma".to_owned()), "a typed name is the stored one");
        assert!(!kit::disabled(&h, &[Role::SpinButton], "Number of Columns:", 0), "a comma and Comma are the same separator");
        kit::type_and_commit(&mut h, "Column Separator:", 0, "Paragraph");
        kit::type_and_commit(&mut h, "Row Separator:", 0, "Paragraph");
        kit::type_and_commit(&mut h, "Number of Columns:", 0, "2");
        assert!(kit::journal(&h, "table.convertFromText").is_empty());
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let commands = kit::journal(&h, "table.convertFromText");
        assert_eq!(commands.len(), 1);
        let params = &commands[0].1;
        assert_eq!(
            (&params["columnSeparator"], &params["rowSeparator"], &params["tableStyle"]),
            (&json!("paragraph"), &json!("paragraph"), &Value::Null)
        );
        assert_eq!(params["columns"].as_f64(), Some(2.0));
        assert_eq!(table_counts(&mut h), (2, 2));
    }

    /// Guards Convert Table to Text: it has only the two separators, and OK gives the table back as text with them.
    #[test]
    fn convert_table_to_text_form_runs_with_the_typed_separators() {
        let (mut app, story) = text_app();
        app.run("table.convertFromText", json!({"columnSeparator": "paragraph", "rowSeparator": "paragraph", "columns": 2, "tableStyle": null}))
            .unwrap();
        app.run("app.convertTableToTextDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        assert!(h.query_by_label("Number of Columns:").is_none(), "no column count when converting to text");
        kit::type_and_commit(&mut h, "Column Separator:", 0, ", ");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(
            kit::journal(&h, "table.convertToText"),
            vec![("table.convertToText".to_owned(), json!({"columnSeparator": ", ", "rowSeparator": "paragraph"}))]
        );
        let text = h.state().session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().text.clone();
        assert_eq!(text.trim_end(), "Name:, Address:\nCity:, State:");
    }
}
