//! Framework forms of Menu Customization, Import Options, Place Contact Sheet and Place PDF. They edit the drafts the
//! native bodies edit; OK and Cancel go through the host's `confirm` and `cancel`, so the placed files, the document and
//! its undo steps are what the native windows made.

use super::Dialog;
use super::find_forms::picker;
use super::frame_forms::{check, field, text};
use super::framework::Form;
use crate::DesignApp;
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Props as _, Scroll, Show as _, ShowContainer as _, Sizing};
use craft_ui_widgets::display::Text;
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::style::TypeRole;
use egui::Ui;
use serde_json::{Value, json};

/// Menu Customization: which menu items show.
pub(crate) const MENUS: Form = Form::new(menus);
/// Import Options: styles and formatting of a file placed into text.
pub(crate) const IMPORT_OPTIONS: Form = Form::new(import_options);
/// Place Contact Sheet: images on new pages.
pub(crate) const CONTACT_SHEET: Form = Form::new(contact_sheet);
/// Place PDF: the page and the crop of a PDF.
pub(crate) const PDF_IMPORT: Form = Form::new(pdf_import);

fn line(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).sizing(Sizing::FillWidth).show(ui);
}

fn heading(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).role(TypeRole::BodyStrong).show(ui);
}

// ---------------------------------------------------------------------------------------------------------------------
// Menu Customization

fn menus(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let mut toggled: Vec<(String, bool)> = Vec::new();
    Stack::new().id_salt("menus").fill_width().show(ui, |ui, _| {
        text(ui, d, "query", &crate::i18n::tr(&lang, "Search"), true);
        let query = d.s("query").to_lowercase();
        Stack::new().id_salt("menus_items").fill_width().scroll(Scroll::vertical()).room_height(320.0).show(ui, |ui, _| {
            for (menu, label) in crate::menus::all_menu_items()
                .into_iter()
                .filter(|(m, l)| query.is_empty() || m.to_lowercase().contains(&query) || l.to_lowercase().contains(&query))
            {
                let key = crate::menus::menu_key(&menu, &label);
                let visible = !app.ui.hidden_menu_items.contains(&key);
                let caption = format!("{} \u{203a} {}", crate::i18n::tr(&lang, &menu), crate::i18n::tr(&lang, &label));
                let out = Checkbox::new(Bind::given(visible)).label(caption).id_salt(("menus_item", key.clone())).show(ui);
                if let Some(now) = out.events.committed().copied().filter(|now| *now != visible) {
                    toggled.push((key, !now));
                }
            }
        });
    });
    for (item, hidden) in toggled {
        let _ = app.run("window.hideMenuItem", json!({"item": item, "hidden": hidden}));
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Import Options

fn import_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let conflicts: Vec<String> = d
        .fields
        .get("conflicts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let names: Vec<String> = app.session.active().map(|s| s.doc.styles.paragraph.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
    let imported: Vec<String> = d
        .fields
        .get("styles")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let options: Vec<(String, String)> =
        std::iter::once((String::new(), tr("(import)"))).chain(names.iter().map(|n| (n.clone(), n.clone()))).collect();
    let resolutions = [("useExisting", "Use Document Style Definition"), ("redefine", "Redefine Document Style"), ("autoRename", "Auto Rename")];
    Stack::new().id_salt("import_options").fill_width().show(ui, |ui, _| {
        line(ui, &d.s("path"));
        check(ui, d, "removeStyles", &tr("Remove Styles and Formatting from Text and Tables"), true);
        if d.b("removeStyles") {
            return;
        }
        heading(ui, &tr("Style Name Conflicts"));
        line(
            ui,
            &if conflicts.is_empty() {
                tr("No conflicts")
            } else {
                tr("{count} conflict(s): {names}").replace("{count}", &conflicts.len().to_string()).replace("{names}", &conflicts.join(", "))
            },
        );
        let before = d.s("styleConflicts");
        let shown = RadioGroup::new(tr("Style Name Conflicts"), Bind::given(before.clone())).id_salt("import_conflicts").show(ui, |ui, _| {
            Row::new().id_salt("import_conflicts_row").show(ui, |ui, _| {
                for (value, caption) in resolutions {
                    Radio::new(value.to_owned()).label(tr(caption)).id_salt(("import_conflict", value)).show(ui);
                }
            });
        });
        if let Some(next) = shown.output.events.landed().filter(|next| **next != before).cloned() {
            d.fields.insert("styleConflicts".into(), json!(next));
        }
        heading(ui, &tr("Style Mapping"));
        Stack::new().id_salt("import_mapping").fill_width().scroll(Scroll::vertical()).room_height(180.0).show(ui, |ui, _| {
            FieldGroup::new(tr("Style Mapping")).id_salt("import_mapping_fields").show(ui, |ui, _| {
                for style in &imported {
                    field(ui, style, &format!("map:{style}"), |ui| {
                        let mapped = d.fields.get("map").and_then(|m| m.get(style)).and_then(Value::as_str).unwrap_or("").to_owned();
                        if let Some(next) = picker(ui, &format!("map:{style}"), style, &mapped, &options, false)
                            && let Some(map) = d.fields.get_mut("map").and_then(Value::as_object_mut)
                        {
                            if next.is_empty() {
                                map.remove(style);
                            } else {
                                map.insert(style.clone(), json!(next));
                            }
                        }
                    });
                }
            });
        });
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Place Contact Sheet

fn contact_sheet(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    Stack::new().id_salt("contact_sheet").fill_width().show(ui, |ui, _| {
        let status = d.s("status");
        if !status.is_empty() {
            line(ui, &status);
        }
        line(ui, &tr("Place loaded images on new pages, in file order."));
        FieldGroup::new(tr("Place Contact Sheet")).id_salt("contact_sheet_fields").show(ui, |ui, _| {
            for (caption, key) in [("Columns", "columns"), ("Rows", "rows"), ("Gutter", "gutter"), ("Page margin", "margin")] {
                field(ui, &tr(caption), key, |ui| {
                    text(ui, d, key, &tr(caption), true);
                });
            }
        });
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Place PDF

fn pdf_import(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let crops: Vec<(String, String)> = designcraft_doc::PdfCrop::ALL
        .map(designcraft_doc::PdfCrop::name)
        .into_iter()
        .map(|name| {
            let caption = match name {
                "contentVisible" => "Bounding Box (Visible Layers Only)",
                "contentAll" => "Bounding Box (All Layers)",
                "trim" => "Trim",
                "bleed" => "Bleed",
                "art" => "Art",
                "media" => "Media",
                _ => "Crop",
            };
            (name.to_owned(), tr(caption))
        })
        .collect();
    let page_label = tr("Page (1\u{2013}{count}):").replace("{count}", &d.n("pages").unwrap_or(1.0).to_string());
    Stack::new().id_salt("pdf_import").fill_width().show(ui, |ui, _| {
        line(ui, &if d.b("async") { d.s("name") } else { d.s("path") });
        // The reason OK refused a page, which the native window kept to itself.
        let status = d.s("status");
        if !status.is_empty() {
            line(ui, &status);
        }
        FieldGroup::new(tr("Place PDF")).id_salt("pdf_import_fields").show(ui, |ui, _| {
            field(ui, &page_label, "page", |ui| {
                text(ui, d, "page", &page_label, true);
            });
            field(ui, &tr("Crop to:"), "crop", |ui| {
                let current = if d.s("crop").is_empty() { "crop".to_owned() } else { d.s("crop") };
                if let Some(next) = picker(ui, "crop", &tr("Crop to:"), &current, &crops, false) {
                    d.fields.insert("crop".into(), json!(next));
                }
            });
        });
    });
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::{Key, Modifiers, accesskit::Role};
    use egui_kittest::kittest::Queryable;

    const BUTTON: &[Role] = &[Role::Button];
    const CHECK: &[Role] = &[Role::CheckBox];
    const RADIO: &[Role] = &[Role::RadioButton];

    fn open(id: &str, fields: Value) -> egui_kittest::Harness<'static, DesignApp> {
        let mut app = kit::app(1, false);
        app.ui.dialog = Some(Dialog::new(id, fields));
        kit::harness(app)
    }

    /// Regression: unticking a menu item hides it through `window.hideMenuItem`, and the search narrows the list.
    #[test]
    fn menus_search_and_untick_hide_the_item() {
        let (menu, label) = crate::menus::all_menu_items().into_iter().next().expect("the menus have items");
        let mut h = open("menus", json!({}));
        let _ = h.get_by_role_and_label(Role::Dialog, "Menu Customization");
        kit::type_and_commit(&mut h, "Search", 0, &label);
        let name = format!("{menu} \u{203a} {label}");
        kit::click(&mut h, CHECK, &name, 0);
        let key = crate::menus::menu_key(&menu, &label);
        let sent = kit::journal(&h, "window.hideMenuItem");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!((sent[0].1["item"].as_str(), sent[0].1["hidden"].as_bool()), (Some(key.as_str()), Some(true)));
        assert!(h.state().ui.hidden_menu_items.contains(&key));
        kit::click(&mut h, CHECK, &name, 0);
        assert!(!h.state().ui.hidden_menu_items.contains(&key), "ticking it again shows it");
    }

    fn import_fields() -> Value {
        json!({"path": "/tmp/story.docx", "removeStyles": false, "styleConflicts": "useExisting", "conflicts": ["Body"], "styles": ["Heading 1"], "map": {}})
    }

    /// Regression: Import Options keeps the conflict choice and the style mapping in the same fields, and hides both when
    /// the styles are removed.
    #[test]
    fn import_options_choose_conflicts_and_map_styles() {
        let mut h = open("importOptions", import_fields());
        let _ = h.get_by_role_and_label(Role::Dialog, "Import Options");
        let _ = h.get_by_label("1 conflict(s): Body");
        kit::click(&mut h, RADIO, "Redefine Document Style", 0);
        assert_eq!(kit::draft(&h).s("styleConflicts"), "redefine");
        kit::pick(&mut h, "Heading 1", designcraft_doc::BASIC_PARAGRAPH);
        assert_eq!(kit::draft(&h).fields["map"]["Heading 1"], designcraft_doc::BASIC_PARAGRAPH);
        kit::pick(&mut h, "Heading 1", "(import)");
        assert!(kit::draft(&h).fields["map"].get("Heading 1").is_none(), "(import) takes the mapping away");
        kit::click(&mut h, CHECK, "Remove Styles and Formatting from Text and Tables", 0);
        assert!(kit::draft(&h).b("removeStyles"));
        assert!(h.query_by_label("Style Name Conflicts").is_none(), "the style options go with the choice");
        kit::click(&mut h, BUTTON, "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
    }

    /// Regression: Place Contact Sheet refuses a columns count that is no whole number, keeps what was typed and says why.
    #[test]
    fn contact_sheet_refuses_a_bad_count_and_keeps_the_dialog() {
        let mut h = open("contactSheet", json!({}));
        let _ = h.get_by_role_and_label(Role::Dialog, "Place Contact Sheet");
        kit::type_and_commit(&mut h, "Columns", 0, "x");
        kit::type_and_commit(&mut h, "Gutter", 0, "9pt");
        kit::click(&mut h, BUTTON, "OK", 0);
        let d = kit::draft(&h);
        assert_eq!((d.id.as_str(), d.s("columns"), d.s("gutter")), ("contactSheet", "x".to_owned(), "9pt".to_owned()));
        let _ = h.get_by_label("Columns must be a whole number");
    }

    /// Regression: Place PDF takes the crop by name, and a page that is not a single number is refused with the reason
    /// while the dialog stays open.
    #[test]
    fn pdf_import_picks_the_crop_and_refuses_a_page_range() {
        let mut h = open("pdfImport", json!({"path": "/tmp/a.pdf", "pages": 3, "page": "1", "crop": "crop"}));
        let _ = h.get_by_role_and_label(Role::Dialog, "Place PDF");
        kit::pick(&mut h, "Crop to:", "Trim");
        assert_eq!(kit::draft(&h).s("crop"), "trim");
        kit::type_and_commit(&mut h, "Page (1\u{2013}3):", 0, "2-3");
        kit::click(&mut h, BUTTON, "OK", 0);
        assert_eq!(kit::draft(&h).id, "pdfImport", "the dialog stays");
        let _ = h.get_by_label("Enter a single page number; one page is placed at a time");
        kit::key(&mut h, Key::Escape, Modifiers::NONE);
        assert!(h.state().ui.dialog.is_none(), "Escape cancels");
    }
}
