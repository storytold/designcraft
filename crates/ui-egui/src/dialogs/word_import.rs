//! File › Place's Microsoft Word Import Options (and RTF Import Options), laid out as InDesign's:
//! Include (Table of Contents Text, Index Text, Footnotes, Endnotes), Options (Use Typographer's
//! Quotes) and Formatting: either Remove Styles and Formatting (Preserve Local Overrides, Convert
//! Tables To) or Preserve Styles and Formatting (Manual Page Breaks, Import Unused Styles,
//! Convert Bullets & Numbers to Text, Style Name Conflicts, Import Styles Automatically or
//! Customize Style Import).

use serde_json::{Value, json};

use super::Dialog;
use crate::DesignApp;
use crate::theme::semibold;

/// The fields a new dialog for `path` starts with: InDesign's defaults, and the styles the file
/// has (`info`, from `place.styles`).
pub fn fields(path: &str, info: &Value) -> Value {
    json!({
        "path": path,
        "styles": info["paragraph"],
        "charStyles": info["character"],
        "conflicts": info["conflicts"],
        "styleConflicts": "useExisting",
        "removeStyles": false,
        "preserveLocal": false,
        "tablesTabbed": false,
        "toc": true,
        "index": true,
        "footnotes": true,
        "endnotes": true,
        "quotes": true,
        "pageBreaks": "preserve",
        "unusedStyles": false,
        "bulletsToText": false,
        "customize": false,
        "map": {},
    })
}

/// The dialog's title: Microsoft Word Import Options (file) or RTF Import Options (file).
pub fn title(lang: &str, d: &Dialog) -> String {
    let path = d.s("path");
    let name = std::path::Path::new(&path).file_name().map_or(path.clone(), |n| n.to_string_lossy().to_string());
    let kind = if path.to_lowercase().ends_with(".rtf") { "RTF Import Options" } else { "Microsoft Word Import Options" };
    format!("{} ({name})", crate::i18n::tr(lang, kind))
}

/// The `file.place` params of the dialog.
pub fn params(d: &Dialog) -> Value {
    let map = if d.b("customize") { d.fields.get("map").cloned().unwrap_or(json!({})) } else { json!({}) };
    json!({
        "path": d.s("path"),
        "removeStyles": d.b("removeStyles"),
        "styleConflicts": d.s("styleConflicts"),
        "styleMap": map,
        "word": {
            "toc": d.b("toc"),
            "index": d.b("index"),
            "footnotes": d.b("footnotes"),
            "endnotes": d.b("endnotes"),
            "typographersQuotes": d.b("quotes"),
            "preserveLocalOverrides": d.b("preserveLocal"),
            "tablesAsTabbedText": d.b("tablesTabbed"),
            "pageBreaks": d.s("pageBreaks"),
            "unusedStyles": d.b("unusedStyles"),
            "bulletsToText": d.b("bulletsToText"),
        },
    })
}

fn check(ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: &str) {
    let mut b = d.b(key);
    if ui.checkbox(&mut b, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(key.into(), Value::Bool(b));
    }
}

/// A titled group in a thin frame, as InDesign's dialogs draw them.
fn group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.add_space(6.0);
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).inner_margin(egui::Margin::symmetric(10, 8)).corner_radius(2.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        crate::rtl::label(ui, egui::RichText::new(title).font(semibold(12.0)));
        ui.add_space(2.0);
        add(ui);
    });
}

/// A drop-down of `(value, label)` choices for the field `key`.
fn choice(ui: &mut egui::Ui, d: &mut Dialog, key: &str, options: &[(&str, &str)], tr: &dyn Fn(&str) -> String) {
    let cur = d.s(key);
    let shown = options.iter().find(|o| o.0 == cur).map_or(options[0].1, |o| o.1);
    craft_ui::ComboBox::from_id_salt(("word_import", key.to_string())).selected_text(crate::rtl::widget(ui, tr(shown))).width(230.0).show_ui(
        ui,
        |ui| {
            for (v, l) in options {
                if ui.selectable_label(cur == *v, crate::rtl::widget(ui, tr(l))).clicked() {
                    d.fields.insert(key.into(), json!(v));
                }
            }
        },
    );
}

pub fn show(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    ui.horizontal(|ui| {
        crate::rtl::label(ui, tr("Preset:"));
        // Saved presets aren't kept yet: the settings are always [Custom].
        ui.add_enabled_ui(false, |ui| {
            craft_ui::ComboBox::from_id_salt("word_import_preset").selected_text(tr("[Custom]")).width(200.0).show_ui(ui, |_| {});
        });
    });
    group(ui, &tr("Include"), |ui| {
        egui::Grid::new("word_import_include").num_columns(2).spacing([40.0, 4.0]).show(ui, |ui| {
            check(ui, d, "toc", &tr("Table of Contents Text"));
            check(ui, d, "footnotes", &tr("Footnotes"));
            ui.end_row();
            check(ui, d, "index", &tr("Index Text"));
            check(ui, d, "endnotes", &tr("Endnotes"));
            ui.end_row();
        });
    });
    group(ui, &tr("Options"), |ui| check(ui, d, "quotes", &tr("Use Typographer's Quotes")));
    group(ui, &tr("Formatting"), |ui| {
        let remove = d.b("removeStyles");
        if ui.radio(remove, crate::rtl::widget(ui, tr("Remove Styles and Formatting from Text and Tables"))).clicked() {
            d.fields.insert("removeStyles".into(), json!(true));
        }
        ui.indent("word_import_remove", |ui| {
            ui.add_enabled_ui(remove, |ui| {
                check(ui, d, "preserveLocal", &tr("Preserve Local Overrides"));
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, tr("Convert Tables To:"));
                    let cur = if d.b("tablesTabbed") { "Unformatted Tabbed Text" } else { "Unformatted Tables" };
                    craft_ui::ComboBox::from_id_salt("word_import_tables").selected_text(crate::rtl::widget(ui, tr(cur))).width(200.0).show_ui(
                        ui,
                        |ui| {
                            for (v, l) in [(false, "Unformatted Tables"), (true, "Unformatted Tabbed Text")] {
                                if ui.selectable_label(d.b("tablesTabbed") == v, crate::rtl::widget(ui, tr(l))).clicked() {
                                    d.fields.insert("tablesTabbed".into(), json!(v));
                                }
                            }
                        },
                    );
                });
            });
        });
        if ui.radio(!remove, crate::rtl::widget(ui, tr("Preserve Styles and Formatting from Text and Tables"))).clicked() {
            d.fields.insert("removeStyles".into(), json!(false));
        }
        ui.indent("word_import_preserve", |ui| {
            ui.add_enabled_ui(!remove, |ui| {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, tr("Manual Page Breaks:"));
                    choice(
                        ui,
                        d,
                        "pageBreaks",
                        &[("preserve", "Preserve Page Breaks"), ("column", "Convert to Column Breaks"), ("none", "No Breaks")],
                        &tr,
                    );
                });
                egui::Grid::new("word_import_preserve_opts").num_columns(2).spacing([40.0, 4.0]).show(ui, |ui| {
                    // Images in a Word file aren't read yet; tracked changes come in accepted.
                    ui.add_enabled_ui(false, |ui| {
                        let mut off = false;
                        ui.checkbox(&mut off, crate::rtl::widget(ui, tr("Import Inline Graphics")));
                    });
                    ui.add_enabled_ui(false, |ui| {
                        let mut off = false;
                        ui.checkbox(&mut off, crate::rtl::widget(ui, tr("Track Changes")));
                    });
                    ui.end_row();
                    check(ui, d, "unusedStyles", &tr("Import Unused Styles"));
                    check(ui, d, "bulletsToText", &tr("Convert Bullets & Numbers to Text"));
                    ui.end_row();
                });
                let conflicts: Vec<String> = d
                    .fields
                    .get("conflicts")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, tr("Style Name Conflicts:"));
                    let text = if conflicts.is_empty() {
                        tr("No conflicts")
                    } else {
                        tr("{count} conflict(s): {names}").replace("{count}", &conflicts.len().to_string()).replace("{names}", &conflicts.join(", "))
                    };
                    crate::rtl::label(ui, if conflicts.is_empty() { text } else { format!("⚠ {text}") });
                });
                let customize = d.b("customize");
                if ui.radio(!customize, crate::rtl::widget(ui, tr("Import Styles Automatically"))).clicked() {
                    d.fields.insert("customize".into(), json!(false));
                }
                ui.indent("word_import_auto", |ui| {
                    ui.add_enabled_ui(!customize, |ui| {
                        let opts = [
                            ("useExisting", "Use Document Style Definition"),
                            ("redefine", "Redefine Document Style"),
                            ("autoRename", "Auto Rename"),
                        ];
                        // One rule for both kinds of styles: the two drop-downs show the same.
                        for label in ["Paragraph Style Conflicts:", "Character Style Conflicts:"] {
                            ui.horizontal(|ui| {
                                crate::rtl::label(ui, tr(label));
                                ui.push_id(label, |ui| choice(ui, d, "styleConflicts", &opts, &tr));
                            });
                        }
                    });
                });
                if ui.radio(customize, crate::rtl::widget(ui, tr("Customize Style Import"))).clicked() {
                    d.fields.insert("customize".into(), json!(true));
                }
                if customize {
                    style_mapping(app, ui, d, &tr);
                }
            });
        });
    });
}

/// Customize Style Import: each of the file's paragraph styles imported, or mapped to a
/// document style.
fn style_mapping(app: &DesignApp, ui: &mut egui::Ui, d: &mut Dialog, tr: &dyn Fn(&str) -> String) {
    crate::rtl::label(ui, egui::RichText::new(tr("Style Mapping")).font(semibold(12.0)));
    let names: Vec<String> = app.session.active().map(|s| s.doc.styles.paragraph.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
    let imported: Vec<String> = d
        .fields
        .get("styles")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
        egui::Grid::new("imp_map").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for w in &imported {
                ui.label(w);
                let mapped = d.fields.get("map").and_then(|m| m.get(w)).and_then(Value::as_str).unwrap_or("").to_string();
                let shown = if mapped.is_empty() { tr("(import)") } else { mapped.clone() };
                craft_ui::ComboBox::from_id_salt(("imp_map", w)).selected_text(crate::rtl::widget(ui, &shown)).width(170.0).show_ui(ui, |ui| {
                    if ui.selectable_label(mapped.is_empty(), crate::rtl::widget(ui, tr("(import)"))).clicked()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(app: &mut DesignApp, ctx: &egui::Context) -> Vec<String> {
        let mut texts = Vec::new();
        for _ in 0..8 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1400.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
            out.textures_delta.clear();
            fn walk(s: egui::epaint::Shape, texts: &mut Vec<String>) {
                match s {
                    egui::epaint::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                    egui::epaint::Shape::Vec(v) => v.into_iter().for_each(|s| walk(s, texts)),
                    _ => {}
                }
            }
            texts.clear();
            for c in out.shapes {
                walk(c.shape, &mut texts);
            }
        }
        texts
    }

    /// The course's chapter 17: File › Place with Show Import Options on a Word/RTF file shows
    /// InDesign's Import Options; Remove Styles and Formatting with Use Typographer's Quotes
    /// places the plain text with curly quotes.
    #[test]
    fn word_import_options_are_laid_out_as_indesign_and_apply() {
        let dir = std::env::temp_dir().join(format!("dc-word-dialog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Cookbook_Text.rtf");
        std::fs::write(&path, r#"{\rtf1\ansi {\b "Egg" Recipes}\par\page Boil them.\par}"#).unwrap();
        let path = path.to_string_lossy().to_string();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let info = app.run("place.styles", json!({"path": path})).unwrap();
        app.ui.dialog = Some(Dialog::new("importOptions", fields(&path, &info)));
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "en");
        let t = texts(&mut app, &ctx);
        for l in [
            "RTF Import Options (Cookbook_Text.rtf)",
            "Include",
            "Table of Contents Text",
            "Endnotes",
            "Use Typographer's Quotes",
            "Remove Styles and Formatting from Text and Tables",
            "Convert Tables To:",
            "Preserve Styles and Formatting from Text and Tables",
            "Manual Page Breaks:",
            "Import Unused Styles",
            "Import Styles Automatically",
            "Customize Style Import",
        ] {
            assert!(t.iter().any(|x| x.trim() == l), "{l}: {t:?}");
        }
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("removeStyles".into(), json!(true));
            d.fields.insert("pageBreaks".into(), json!("none"));
            assert_eq!(params(d)["word"]["typographersQuotes"], true);
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let st = app.session.active().unwrap();
        let story = st.doc.stories.values().find(|s| s.text.contains("Recipes")).unwrap();
        assert!(story.text.contains("\u{201C}Egg\u{201D}"), "{:?}", story.text);
        assert!(!story.text.contains(designcraft_doc::PAGE_BREAK), "No Breaks");
        assert_eq!(story.char_format_of(story.text.find("Recipes").unwrap()).over.font_style, None, "styles and formatting removed");
    }
}
