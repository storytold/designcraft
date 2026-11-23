//! Framework forms of Export PDF (with its presets), EPUB Export Options (with the cover picker) and Print. They edit the
//! drafts the native bodies edit; OK and Cancel go through the host's `confirm` and `cancel`, so the files written and the
//! settings kept are what the native windows produced.

use super::find_forms::picker;
use super::frame_forms::{captioned, check, field, select, text};
use super::framework::Form;
use super::{Bound, Dialog, PDF_EXPORT_PRESETS, apply_pdf_preset, set_epub_cover};
use crate::DesignApp;
use craft_theme::variant::Tone;
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _, Sizing};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::{Row, Stack};
use egui::Ui;
use serde_json::{Value, json};

/// Export PDF.
pub(crate) const PDF_EXPORT: Form = Form::new(pdf_export);
/// EPUB Export Options.
pub(crate) const EPUB_EXPORT: Form = Form::new(epub_export);
/// Print.
pub(crate) const PRINT: Form = Form::new(print);

fn note(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
}

// ---------------------------------------------------------------------------------------------------------------------
// Export PDF

/// Export PDF: General, Compression, Marks and Bleeds, Advanced and Transparency Flattener. Picking a preset fills the
/// dependent fields; editing any other control flips the preset back to Custom.
fn pdf_export(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let presets: Vec<(String, String)> = PDF_EXPORT_PRESETS.iter().map(|p| ((*p).to_owned(), tr(p))).collect();
    let standards = captioned(&lang, &[("none", "None"), ("x1a", "PDF/X-1a:2003"), ("x4", "PDF/X-4"), ("a2b", "PDF/A-2b")]);
    let flatteners = captioned(&lang, &[("", "None"), ("high", "High Resolution"), ("medium", "Medium Resolution"), ("low", "Low Resolution")]);
    let layouts = app.session.active().map(|st| st.doc.layouts()).unwrap_or_default();
    let layout_options: Vec<(String, String)> =
        std::iter::once(("all".to_owned(), tr("All"))).chain(layouts.iter().map(|l| (l.name.clone(), l.name.clone()))).collect();
    // The fields as they were, to see a manual edit after the controls ran.
    let before = d.fields.clone();
    let mut preset_clicked = false;
    Stack::new().id_salt("pdf_export").fill_width().show(ui, |ui, _| {
        GroupBox::new(tr("General"), None).id_salt("pdf_export_general").show(ui, |ui, _| {
            FieldGroup::new(tr("General")).id_salt("pdf_export_general_fields").show(ui, |ui, _| {
                field(ui, &tr("Preset:"), "preset", |ui| {
                    if let Some(next) = picker(ui, "preset", &tr("Preset:"), &d.s("preset"), &presets, false) {
                        d.fields.insert("preset".into(), json!(next));
                        apply_pdf_preset(d, &next);
                        preset_clicked = true;
                    }
                });
                field(ui, &tr("Page Range:"), "pages", |ui| {
                    Row::new().id_salt("pdf_export_pages_row").fill_width().show(ui, |ui, _| {
                        text(ui, d, "pages", &tr("Page Range:"), true);
                        // Alternate layouts: one pick puts a layout's pages in the range.
                        if layouts.len() > 1
                            && let Some(next) = picker(ui, "layout", &tr("Layout\u{2026}"), &tr("Layout\u{2026}"), &layout_options, false)
                        {
                            let pages = match layouts.iter().find(|l| l.name == next) {
                                Some(l) => format!("{}-{}", l.start.saturating_add(1), l.end),
                                None => "all".to_owned(),
                            };
                            d.fields.insert("pages".into(), json!(pages));
                        }
                    });
                });
            });
            check(ui, d, "spreads", &tr("Spreads"), true);
        });
        GroupBox::new(tr("Compression"), None).id_salt("pdf_export_compression").show(ui, |ui, _| {
            check(ui, d, "compressImages", &tr("Compress images"), true);
        });
        GroupBox::new(tr("Marks and Bleeds"), None).id_salt("pdf_export_marks").show(ui, |ui, _| {
            check(ui, d, "bleed", &tr("Include document bleed"), true);
            check(ui, d, "marksCrop", &tr("Crop Marks"), true);
            check(ui, d, "marksBleed", &tr("Bleed Marks"), true);
            check(ui, d, "marksPageInfo", &tr("Page Information"), true);
            FieldGroup::new(tr("Marks and Bleeds")).id_salt("pdf_export_marks_fields").show(ui, |ui, _| {
                field(ui, &tr("Weight:"), "marks_weight", |ui| {
                    text(ui, d, "marksWeight", &tr("Weight:"), true);
                });
                field(ui, &tr("Offset:"), "marks_offset", |ui| {
                    text(ui, d, "marksOffset", &tr("Offset:"), true);
                });
            });
        });
        GroupBox::new(tr("Advanced"), None).id_salt("pdf_export_advanced").show(ui, |ui, _| {
            FieldGroup::new(tr("Advanced")).id_salt("pdf_export_advanced_fields").show(ui, |ui, _| {
                field(ui, &tr("Standard:"), "standard", |ui| select(ui, d, "standard", &tr("Standard:"), &standards, true));
            });
        });
        // Spreads with transparency are rasterized at the preset's resolution.
        GroupBox::new(tr("Transparency Flattener"), None).id_salt("pdf_export_flattener").show(ui, |ui, _| {
            FieldGroup::new(tr("Transparency Flattener")).id_salt("pdf_export_flattener_fields").show(ui, |ui, _| {
                field(ui, &tr("Preset:"), "flatten", |ui| select(ui, d, "flatten", &tr("Preset:"), &flatteners, true));
            });
            check(ui, d, "tagged", &tr("Tagged PDF"), true);
        });
    });
    // Any edit other than picking a preset means the settings are no longer that preset.
    if !preset_clicked && d.fields != before {
        d.fields.insert("preset".into(), json!("Custom"));
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// EPUB Export Options

fn epub_export(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let styles: Vec<(String, String)> = app
        .session
        .active()
        .map(|st| {
            st.doc.styles.paragraph.iter().map(|s| s.name.clone()).filter(|n| n != designcraft_doc::NO_PARA_STYLE).map(|n| (n.clone(), n)).collect()
        })
        .unwrap_or_default();
    let mut choose = false;
    Stack::new().id_salt("epub_export").fill_width().show(ui, |ui, _| {
        check(ui, d, "split", &tr("Split Document"), true);
        let split = d.b("split");
        let mode = if d.s("mode") == "exportTags" { "exportTags" } else { "style" };
        let shown = RadioGroup::new(tr("Split Document"), Bind::given(mode.to_owned())).id_salt("epub_split_mode").show(ui, |ui, _| {
            Stack::new().id_salt("epub_split_modes").show(ui, |ui, _| {
                Radio::new("style".to_owned()).label(tr("Single Paragraph Style")).id_salt("epub_split_style_radio").enabled(split).show(ui);
                Radio::new("exportTags".to_owned())
                    .label(tr("Based on Paragraph Style Export Tags"))
                    .id_salt("epub_split_tags_radio")
                    .enabled(split)
                    .show(ui);
            });
        });
        if let Some(next) = shown.output.events.landed().filter(|next| *next != mode).cloned() {
            d.fields.insert("mode".into(), json!(next));
        }
        field(ui, &tr("Paragraph Style:"), "split_style", |ui| {
            if let Some(next) = picker(ui, "split_style", &tr("Paragraph Style:"), &d.s("style"), &styles, false) {
                d.fields.insert("style".into(), json!(next));
                d.fields.insert("mode".into(), json!("style"));
            }
        });
        // The cover: the first page rendered, or an image file (PNG or JPEG) named here.
        field(ui, &tr("Cover Image:"), "cover", |ui| {
            Row::new().id_salt("epub_cover_row").fill_width().show(ui, |ui, _| {
                text(ui, d, "coverImage", &tr("Cover Image:"), true);
                choose |= Button::new(tr("Choose\u{2026}")).id_salt("epub_cover_choose").show(ui).clicked();
            });
        });
        note(ui, &tr("A PNG or JPEG file; left empty, the first page is the cover."));
    });
    if choose && let Err(e) = app.ask_open("coverImage", Bound::Document, set_epub_cover) {
        app.status_error("EPUB Export Options", e);
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Print

fn print(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let printers: Vec<(String, String)> = d
        .fields
        .get("printers")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|p| (p.to_owned(), p.to_owned()))).collect())
        .unwrap_or_default();
    let layouts: Vec<String> = app.session.active().map(|st| st.doc.layouts().into_iter().map(|l| l.name).collect()).unwrap_or_default();
    let ranges: Vec<(&str, &str)> =
        [("all", "All"), ("range", "Range:")].into_iter().chain((layouts.len() > 1).then_some(("layouts", "Layouts:"))).collect();
    // Alternate layouts: the ones to print, all unless chosen.
    let chosen: Vec<String> = d.fields.get("layouts").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_else(|| layouts.clone());
    let mut next_chosen = chosen.clone();
    Stack::new().id_salt("print").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Print")).id_salt("print_fields").show(ui, |ui, _| {
            field(ui, &tr("Printer:"), "printer", |ui| {
                if printers.is_empty() {
                    Text::new(tr("System default")).show(ui);
                } else if let Some(next) = picker(ui, "printer", &tr("Printer:"), &d.s("printer"), &printers, false) {
                    d.fields.insert("printer".into(), json!(next));
                }
            });
            field(ui, &tr("Copies:"), "copies", |ui| {
                text(ui, d, "copies", &tr("Copies:"), true);
            });
        });
        let before = d.s("range");
        let shown = RadioGroup::new(tr("Pages:"), Bind::given(before.clone())).id_salt("print_range").show(ui, |ui, _| {
            Stack::new().id_salt("print_ranges").show(ui, |ui, _| {
                for (value, caption) in &ranges {
                    Radio::new((*value).to_owned()).label(tr(*caption)).id_salt(("print_range_radio", *value)).show(ui);
                }
            });
        });
        if let Some(next) = shown.output.events.landed().filter(|next| **next != before).cloned() {
            d.fields.insert("range".into(), json!(next));
        }
        FieldGroup::new(tr("Pages:")).id_salt("print_pages").show(ui, |ui, _| {
            field(ui, &tr("Pages:"), "pages", |ui| {
                text(ui, d, "pages", &tr("Pages:"), true);
            });
        });
        if layouts.len() > 1 {
            let on = d.s("range") == "layouts";
            Row::new().id_salt("print_layouts").wrap(true).show(ui, |ui, _| {
                for name in &layouts {
                    let checked = chosen.contains(name);
                    let out = Checkbox::new(Bind::given(checked)).label(name.clone()).id_salt(("print_layout", name.clone())).enabled(on).show(ui);
                    if let Some(now) = out.events.committed().copied().filter(|now| *now != checked) {
                        if now {
                            next_chosen.push(name.clone());
                        } else {
                            next_chosen.retain(|x| x != name);
                        }
                    }
                }
            });
            d.fields.insert("layouts".into(), json!(layouts.iter().filter(|n| next_chosen.contains(n)).collect::<Vec<_>>()));
            check(ui, d, "separateJobs", &tr("Print each layout separately"), on);
        }
        check(ui, d, "spreads", &tr("Spreads"), true);
        check(ui, d, "marks", &tr("Printer's Marks"), true);
        check(ui, d, "bleed", &tr("Include Bleed"), true);
        let status = d.s("status");
        if !status.is_empty() {
            Text::new(status).tone(Tone::Bad).sizing(Sizing::FillWidth).show(ui);
        }
    });
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    use std::sync::{Arc, Mutex};

    const BUTTON: &[Role] = &[Role::Button];
    const CHECK: &[Role] = &[Role::CheckBox];
    const RADIO: &[Role] = &[Role::RadioButton];

    type Writes = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    /// An app whose file writes are collected and whose save and open dialogs answer `path`.
    fn app_with_files(path: &str) -> (DesignApp, Writes) {
        let mut app = kit::app(1, false);
        let writes: Writes = Arc::new(Mutex::new(Vec::new()));
        let sink = writes.clone();
        app.services.write = Some(Box::new(move |path: &str, bytes: &[u8]| {
            sink.lock().unwrap().push((path.to_owned(), bytes.to_vec()));
            Ok(())
        }));
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(path.to_owned())));
        (app, writes)
    }

    /// Regression: picking a preset fills its dependent fields without turning the preset to Custom, editing any other
    /// field does, and OK writes the PDF with the settings kept for next time.
    #[test]
    fn pdf_export_preset_fills_fields_then_an_edit_makes_it_custom_and_ok_writes_the_file() {
        let (mut app, writes) = app_with_files("/tmp/designcraft-form.pdf");
        crate::dialogs::open_pdf_export(&mut app);
        let mut h = kit::harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "Export PDF");
        kit::pick(&mut h, "Preset:", "PDF/X-4");
        let d = kit::draft(&h);
        assert_eq!((d.s("preset"), d.s("standard"), d.b("marksCrop")), ("PDF/X-4".to_owned(), "x4".to_owned(), true));
        kit::click(&mut h, CHECK, "Tagged PDF", 0);
        assert_eq!(kit::draft(&h).s("preset"), "Custom", "an edit leaves the preset");
        assert_eq!(kit::draft(&h).s("standard"), "x4");
        kit::click(&mut h, BUTTON, "OK", 0);
        h.state_mut().settle_dialogs();
        let got = writes.lock().unwrap();
        assert_eq!(got.len(), 1, "one file is written");
        assert!(got[0].1.starts_with(b"%PDF"));
        drop(got);
        let kept = &h.state().ui.pdf_export;
        assert_eq!((kept.standard.as_str(), kept.tagged), ("x4", false));
    }

    /// Regression: EPUB Export Options keeps Split Document and its mode in the same fields, Choose… puts the picked image
    /// into Cover Image, and OK stores the split as one undo step.
    #[test]
    fn epub_export_split_and_cover_picker() {
        let (mut app, _writes) = app_with_files("/tmp/designcraft-cover.png");
        app.session.execute("style.paragraph.create", &json!({"name": "Chapter"})).unwrap();
        crate::menus::activate(&mut app, "app.exportEpub", &Value::Null);
        let mut h = kit::harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "EPUB Export Options");
        kit::click(&mut h, CHECK, "Split Document", 0);
        kit::pick(&mut h, "Paragraph Style:", "Chapter");
        assert_eq!((kit::draft(&h).b("split"), kit::draft(&h).s("style")), (true, "Chapter".to_owned()));
        kit::click(&mut h, RADIO, "Based on Paragraph Style Export Tags", 0);
        assert_eq!(kit::draft(&h).s("mode"), "exportTags");
        kit::click(&mut h, BUTTON, "Choose\u{2026}", 0);
        h.state_mut().settle_dialogs();
        assert_eq!(kit::draft(&h).s("coverImage"), "/tmp/designcraft-cover.png", "the picked file is the cover");
        kit::type_and_commit(&mut h, "Cover Image:", 0, "cover.png");
        assert_eq!(kit::draft(&h).s("coverImage"), "cover.png", "the field can be typed over");
    }

    /// Regression: Print keeps the printer, copies, page range and flags in the draft and shows why a print failed.
    #[test]
    fn print_edits_the_draft_and_shows_a_refusal() {
        let mut app = kit::app(1, false);
        crate::dialogs::open_print(&mut app);
        let mut h = kit::harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "Print");
        kit::type_and_commit(&mut h, "Copies:", 0, "2");
        kit::click(&mut h, RADIO, "Range:", 0);
        kit::type_and_commit(&mut h, "Pages:", 0, "1-1");
        kit::click(&mut h, CHECK, "Printer's Marks", 0);
        let d = kit::draft(&h);
        assert_eq!((d.s("copies"), d.s("range"), d.s("pages"), d.b("marks")), ("2".to_owned(), "range".to_owned(), "1-1".to_owned(), true));
        h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("status".into(), json!("No printer is available"));
        kit::steps(&mut h, 3);
        let _ = h.get_by_label("No printer is available");
        kit::click(&mut h, BUTTON, "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
    }
}
