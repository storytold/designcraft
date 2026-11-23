//! Margins and Columns and Create Guides on the framework's form components. Each form edits the same `Dialog` draft
//! the native bodies (`margins_and_columns_dialog`, `create_guides_dialog`) edit, and the live preview is the same
//! `layout_dialog_preview`: Preview shows the change on the page without an undo step and OK makes it once.

use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::{Field, FieldGroup};
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::text::{TextField, link_toggle};
use designcraft_geom::format_measure;
use egui::Ui;
use serde_json::{Value, json};

use super::framework::Form;
use super::{Dialog, MARGIN_FIELDS, layout_dialog_preview, link_margins};
use crate::DesignApp;

/// Layout › Margins and Columns.
pub(crate) const MARGINS_AND_COLUMNS: Form = Form::new(margins_and_columns);
/// Layout › Create Guides.
pub(crate) const CREATE_GUIDES: Form = Form::new(create_guides);

/// What Create Guides fits the guides to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fit {
    Margins,
    Page,
}

/// Margins (Top and Bottom, Inside and Outside, with the link between them), Columns (Number, Gutter), Adjust Layout,
/// what the change applies to, and Preview beside the options. Measures are in the ruler unit and take arithmetic
/// ("0.4375 in*2").
fn margins_and_columns(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
    let facing = app.session.active().is_none_or(|s| s.doc.settings.facing_pages);
    let all_pages = !d.fields.contains_key("parent");
    // Linked when the dialog opens with four equal margins.
    if !d.fields.contains_key("link") {
        let first = d.s("marginTop");
        let same = MARGIN_FIELDS.iter().all(|(k, _, _)| d.s(k) == first);
        d.fields.insert("link".into(), json!(same));
    }
    link_margins(d);
    Stack::new().id_salt("margins_form").fill_width().show(ui, |ui, _| {
        Row::new().id_salt("margins_body").fill_width().show(ui, |ui, _| {
            Stack::new().id_salt("margins_options").show(ui, |ui, _| {
                GroupBox::new(tr("Margins"), None).id_salt("margins_group").show(ui, |ui, _| {
                    Row::new().id_salt("margins_sides").fill_width().show(ui, |ui, _| {
                        FieldGroup::new(tr("Margins")).id_salt("margins_vertical").show(ui, |ui, _| {
                            for (key, facing_label, single_label) in &MARGIN_FIELDS[..2] {
                                let label = tr(if facing { *facing_label } else { *single_label });
                                Field::new(label.clone()).id_salt(("margin_field", *key)).show(ui, |ui, _| measure_field(ui, d, key, &label));
                            }
                        });
                        let linked = d.b("link");
                        let out = link_toggle(tr("Make all settings the same"), Bind::given(linked)).id_salt("margins_link").show(ui);
                        let mut next = linked;
                        for event in out.events {
                            event.apply(&mut next);
                        }
                        if next != linked {
                            d.fields.insert("link".into(), json!(next));
                        }
                        FieldGroup::new(tr("Margins")).id_salt("margins_horizontal").show(ui, |ui, _| {
                            for (key, facing_label, single_label) in &MARGIN_FIELDS[2..] {
                                let label = tr(if facing { *facing_label } else { *single_label });
                                Field::new(label.clone()).id_salt(("margin_field", *key)).show(ui, |ui, _| measure_field(ui, d, key, &label));
                            }
                        });
                    });
                });
                GroupBox::new(tr("Columns"), None).id_salt("columns_group").show(ui, |ui, _| {
                    Row::new().id_salt("columns_fields").fill_width().show(ui, |ui, _| {
                        let number = crate::i18n::tr_context(&lang, "Number:", "count").to_owned();
                        Field::new(number.clone()).id_salt("columns_number_field").show(ui, |ui, _| text_field(ui, d, "columns", &number));
                        let gutter = tr("Gutter:");
                        Field::new(gutter.clone()).id_salt("columns_gutter_field").show(ui, |ui, _| measure_field(ui, d, "gutter", &gutter));
                    });
                });
                checkbox(ui, d, "adjustLayout", &tr("Adjust Layout"));
                target_row(ui, d, &lang, all_pages);
            });
            checkbox(ui, d, "preview", &tr("Preview"));
        });
        preview_and_status(app, ui, d, "layout.marginsAndColumns", "Margins and Columns");
    });
    None
}

/// Rows and Columns side by side (Number, Gutter), Options (Fit Guides to: Margins or Page, Remove Existing Ruler
/// Guides), what the guides apply to, and Preview.
fn create_guides(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s).to_owned();
    Stack::new().id_salt("guides_form").fill_width().show(ui, |ui, _| {
        Row::new().id_salt("guides_body").fill_width().show(ui, |ui, _| {
            Stack::new().id_salt("guides_options").show(ui, |ui, _| {
                Row::new().id_salt("guides_grid").show(ui, |ui, _| {
                    for (title, number, gutter, salt) in
                        [("Rows", "rows", "rowGutter", "guides_rows"), ("Columns", "columns", "columnGutter", "guides_columns")]
                    {
                        GroupBox::new(tr(title), None).id_salt(salt).fill(false).show(ui, |ui, _| {
                            FieldGroup::new(tr(title)).id_salt((salt, "fields")).show(ui, |ui, _| {
                                let count = crate::i18n::tr_context(&lang, "Number:", "count").to_owned();
                                Field::new(count.clone()).id_salt((salt, "number")).show(ui, |ui, _| text_field(ui, d, number, &count));
                                let gap = tr("Gutter:");
                                Field::new(gap.clone()).id_salt((salt, "gutter")).show(ui, |ui, _| measure_field(ui, d, gutter, &gap));
                            });
                        });
                    }
                });
                GroupBox::new(tr("Options"), None).id_salt("guides_options_group").show(ui, |ui, _| {
                    fit_to(ui, d, &tr("Fit Guides to:"), &tr("Margins"), &tr("Page"));
                    checkbox(ui, d, "removeExisting", &tr("Remove Existing Ruler Guides"));
                });
                target_row(ui, d, &lang, false);
            });
            checkbox(ui, d, "preview", &tr("Preview"));
        });
        preview_and_status(app, ui, d, "layout.createGuides", "Create Guides");
    });
    None
}

/// Runs the live preview over the draft, then shows its status line (the reason a measure was refused).
fn preview_and_status(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog, command: &str, label: &str) {
    layout_dialog_preview(app, d, command, label);
    if let Some(status) = d.fields.get("status").and_then(Value::as_str) {
        Text::new(status.to_owned()).shade(Shade::Muted).show(ui);
    }
}

/// A plain text field: the draft holds what is typed, and the command reads it when OK runs.
fn text_field(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let current = d.s(key);
    let out = TextField::new(label.to_owned(), Bind::given(current.clone())).id_salt(("layout_text", key)).limit(None::<usize>).show(ui);
    let mut next = current.clone();
    for event in out.events {
        event.apply(&mut next);
    }
    if next != current {
        d.fields.insert(key.into(), Value::String(next));
    }
}

/// A measure field: typed arithmetic ("0.4375 in*2") is worked out when the field commits, as InDesign shows it.
fn measure_field(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let current = d.s(key);
    let out = TextField::new(label.to_owned(), Bind::given(current.clone())).id_salt(("layout_measure", key)).limit(None::<usize>).show(ui);
    let committed = out.events.committed().is_some();
    let mut next = current.clone();
    for event in out.events {
        event.apply(&mut next);
    }
    if next != current {
        d.fields.insert(key.into(), Value::String(next));
    }
    if committed && let Some(value) = d.m(key).filter(|v| v.is_finite()) {
        d.fields.insert(key.into(), json!(format_measure(value, d.unit)));
    }
}

fn checkbox(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let on = d.b(key);
    let out = Checkbox::new(Bind::given(on)).label(label.to_owned()).id_salt(("layout_check", key)).show(ui);
    let mut next = on;
    for event in out.events {
        event.apply(&mut next);
    }
    if next != on {
        d.fields.insert(key.into(), json!(next));
    }
}

/// Fit Guides to: Margins or Page.
fn fit_to(ui: &mut Ui, d: &mut Dialog, caption: &str, margins: &str, page: &str) {
    let current = if d.s("fitTo") == "page" { Fit::Page } else { Fit::Margins };
    Row::new().id_salt("guides_fit").show(ui, |ui, _| {
        Text::new(caption.to_owned()).show(ui);
        let shown = RadioGroup::new(caption.to_owned(), Bind::given(current)).id_salt("guides_fit_group").show(ui, |ui, _| {
            Row::new().id_salt("guides_fit_choices").show(ui, |ui, _| {
                Radio::new(Fit::Margins).label(margins.to_owned()).show(ui);
                Radio::new(Fit::Page).label(page.to_owned()).show(ui);
            });
        });
        let mut next = current;
        for event in shown.output.events {
            event.apply(&mut next);
        }
        if next != current {
            d.fields.insert("fitTo".into(), json!(if next == Fit::Page { "page" } else { "margins" }));
        }
    });
}

/// Apply to: the page, its spread or every page, with the page it refers to spelled out so a wrong target is visible
/// before the dialog is confirmed.
fn target_row(ui: &mut Ui, d: &mut Dialog, lang: &str, all_pages: bool) {
    let tr = |s: &'static str| crate::i18n::tr(lang, s).to_owned();
    let parent = d.fields.contains_key("parent") || matches!(d.fields.get("spread"), Some(v) if v["kind"] == "parent");
    let spread = if parent { tr("Both pages of the parent spread") } else { tr("Both pages of the spread") };
    let mut entries = vec![Entry::new("page".to_owned(), tr("This page")), Entry::new("spread".to_owned(), spread)];
    if all_pages {
        entries.push(Entry::new("all".to_owned(), tr("All pages")));
    }
    let label = tr("Apply to:");
    let current = d.s("target");
    Field::new(label.clone()).id_salt("layout_target_field").show(ui, |ui, _| {
        let out = Select::new(label.clone(), Bind::given(current.clone()), &entries).id_salt("layout_target").show(ui);
        let mut next = current.clone();
        for event in out.events {
            event.apply(&mut next);
        }
        if next != current {
            d.fields.insert("target".into(), json!(next));
        }
    });
    let place = d.s("where");
    if !place.is_empty() {
        Text::new(format!("{} {place}", tr("This page is"))).shade(Shade::Muted).show(ui);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::accesskit::Role;

    const SIDES: [&str; 4] = ["marginTop", "marginBottom", "marginInside", "marginOutside"];

    /// A six-page facing document on its second page, with the dialog the menu command `command` opens.
    fn spread_app(command: &str) -> DesignApp {
        let mut app = kit::app(6, true);
        crate::canvas::go_to_page(&mut app, 1);
        kit::open(&mut app, command);
        app
    }

    /// The top margin of the page, rounded to the thousandth of a point so a measure that went through the dialog's text
    /// compares as the number typed.
    fn page_top_margin(h: &egui_kittest::Harness<'_, DesignApp>, page: usize) -> f64 {
        (h.state().session.doc().unwrap().doc.page(page).unwrap().margins.top * 1000.0).round() / 1000.0
    }

    fn guide_counts(h: &egui_kittest::Harness<'_, DesignApp>) -> Vec<usize> {
        let doc = &h.state().session.doc().unwrap().doc;
        (0..doc.page_count()).map(|page| doc.page(page).unwrap().guides.len()).collect()
    }

    /// Guards the Margins and Columns port: with the link on, a margin typed in the framework form is all four, a
    /// measure with arithmetic is worked out when the field commits, and neither touches the document before OK.
    #[test]
    fn margins_form_links_the_sides_and_works_out_typed_measures() {
        let mut h = kit::harness(spread_app("layout.marginsAndColumns"));
        let before = h.state().session.doc().unwrap().doc.clone();
        let undo = kit::undo_steps(&h);
        let linked = SIDES.iter().all(|key| kit::draft(&h).s(key) == kit::draft(&h).s("marginTop"));
        assert_eq!(kit::draft(&h).b("link"), linked, "linked when the dialog opens with four equal margins");
        if !kit::draft(&h).b("link") {
            kit::click(&mut h, &[Role::Button, Role::CheckBox, Role::Switch], "Make all settings the same", 0);
            assert!(kit::draft(&h).b("link"));
        }
        kit::type_and_commit(&mut h, "Top:", 0, "20 pt");
        for key in SIDES {
            assert_eq!(kit::draft(&h).m(key), Some(20.0), "{key} follows the linked margin");
        }
        kit::type_and_commit(&mut h, "Gutter:", 0, "0.5 in*2");
        let gutter = kit::draft(&h);
        assert_ne!(gutter.s("gutter"), "0.5 in*2", "the arithmetic is worked out when the field is left");
        assert!(gutter.m("gutter").is_some_and(|points| (points - 72.0).abs() < 1e-6), "{:?}", gutter.m("gutter"));
        kit::click(&mut h, &[Role::Button, Role::CheckBox, Role::Switch], "Make all settings the same", 0);
        assert!(!kit::draft(&h).b("link"));
        kit::type_and_commit(&mut h, "Top:", 0, "10 pt");
        assert_eq!(kit::draft(&h).m("marginTop"), Some(10.0));
        assert_eq!(kit::draft(&h).m("marginBottom"), Some(20.0), "unlinked, a margin moves alone");
        assert_eq!(h.state().session.doc().unwrap().doc, before, "the fields only edit the draft");
        assert_eq!(kit::undo_steps(&h), undo);
    }

    /// Guards the live preview of Margins and Columns: Preview shows the typed margins on the page without an undo step,
    /// switching it off puts the page back, and Cancel leaves the document as it was.
    #[test]
    fn margins_form_preview_shows_the_change_and_cancel_puts_the_page_back() {
        let mut h = kit::harness(spread_app("layout.marginsAndColumns"));
        let before = h.state().session.doc().unwrap().doc.clone();
        let undo = kit::undo_steps(&h);
        let start = page_top_margin(&h, 1);
        kit::type_and_commit(&mut h, "Top:", 0, "20 pt");
        assert_eq!(page_top_margin(&h, 1), start, "nothing shows before Preview is on");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        assert!(kit::draft(&h).b("preview"));
        assert_eq!(page_top_margin(&h, 1), 20.0, "Preview shows the margins");
        assert_eq!(kit::undo_steps(&h), undo, "a preview is no undo step");
        kit::type_and_commit(&mut h, "Top:", 0, "30 pt");
        assert_eq!(page_top_margin(&h, 1), 30.0, "the preview follows the field");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        assert_eq!(page_top_margin(&h, 1), start, "Preview off puts the page back");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        assert_eq!(page_top_margin(&h, 1), 30.0);
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.doc().unwrap().doc, before, "Cancel leaves the document as it was");
        assert_eq!(kit::undo_steps(&h), undo);
        assert!(kit::journal(&h, "layout.marginsAndColumns").is_empty());
    }

    /// Guards OK on Margins and Columns: with Preview on, OK makes the change once, as one undo step, with the same
    /// parameters the native form gave `layout.marginsAndColumns`; the previewed page is not applied twice.
    #[test]
    fn margins_form_ok_applies_once_with_a_preview_on() {
        let mut h = kit::harness(spread_app("layout.marginsAndColumns"));
        let before = h.state().session.doc().unwrap().doc.clone();
        let undo = kit::undo_steps(&h);
        kit::type_and_commit(&mut h, "Top:", 0, "20 pt");
        kit::pick(&mut h, "Apply to:", "Both pages of the spread");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let commands = kit::journal(&h, "layout.marginsAndColumns");
        assert_eq!(commands.len(), 1);
        let params = &commands[0].1;
        assert_eq!(params["pages"], json!([1, 2]), "both pages of the spread in view");
        assert_eq!(params["margins"]["top"].as_f64(), Some(20.0));
        assert_eq!((page_top_margin(&h, 1), page_top_margin(&h, 2)), (20.0, 20.0));
        assert_eq!(kit::undo_steps(&h), undo + 1, "one OK is one undo step");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc, before);
    }

    /// Guards Create Guides: Rows and Columns are separate groups, the fit and the target choose what the native form
    /// chose, Preview shows the guides without an undo step, and OK adds them once to the pages chosen.
    #[test]
    fn create_guides_form_previews_and_ok_adds_guides_to_the_chosen_pages() {
        let mut h = kit::harness(spread_app("layout.createGuides"));
        let before = h.state().session.doc().unwrap().doc.clone();
        let undo = kit::undo_steps(&h);
        assert_eq!(kit::draft(&h).s("target"), "spread");
        // The Rows group comes first, then Columns.
        kit::type_and_commit(&mut h, "Number:", 1, "2");
        let d = kit::draft(&h);
        assert_eq!((d.s("rows"), d.s("columns")), ("0".to_owned(), "2".to_owned()));
        kit::click(&mut h, &[Role::RadioButton], "Page", 0);
        assert_eq!(kit::draft(&h).s("fitTo"), "page");
        kit::click(&mut h, &[Role::RadioButton], "Margins", 0);
        assert_eq!(kit::draft(&h).s("fitTo"), "margins");
        assert_eq!(guide_counts(&h), vec![0; 6], "nothing shows before Preview is on");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        assert_eq!(guide_counts(&h), vec![0, 2, 2, 0, 0, 0], "both pages of the spread in view");
        assert_eq!(kit::undo_steps(&h), undo, "a preview is no undo step");
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert_eq!(h.state().session.doc().unwrap().doc, before, "Cancel takes the preview away");

        let mut h = kit::harness(spread_app("layout.createGuides"));
        let undo = kit::undo_steps(&h);
        kit::type_and_commit(&mut h, "Number:", 1, "2");
        kit::pick(&mut h, "Apply to:", "This page");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(kit::journal(&h, "layout.createGuides").len(), 1);
        assert_eq!(guide_counts(&h), vec![0, 2, 0, 0, 0, 0], "This page narrows the guides to the page in view");
        assert_eq!(kit::undo_steps(&h), undo + 1, "one OK is one undo step");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(guide_counts(&h), vec![0; 6]);
    }

    /// Guards the refusal path of the layout forms: a measure that does not parse keeps the dialog open with its reason
    /// shown, and the document is untouched.
    #[test]
    fn create_guides_form_refuses_a_measure_that_does_not_parse_and_stays_open() {
        let mut h = kit::harness(spread_app("layout.createGuides"));
        let before = h.state().session.doc().unwrap().doc.clone();
        kit::type_and_commit(&mut h, "Number:", 1, "2");
        kit::type_and_commit(&mut h, "Gutter:", 1, "wide");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_some(), "the dialog stays open to be corrected");
        assert!(!kit::draft(&h).s("status").is_empty());
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        assert!(kit::journal(&h, "layout.createGuides").is_empty());
    }
}
