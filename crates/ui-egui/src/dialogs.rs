//! Modal dialogs. Each dialog keeps its fields in a JSON map so the control channel can set them
//! (`ui.dialog.set {field, value}`) and confirm them (`ui.dialog.confirm`) like a user would.

use designcraft_doc::SpreadRef;
use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::DesignApp;
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
            "qrCode" => json!({"type": "url", "content": "https://", "color": "[Black]"}),
            "insertTable" => json!({"bodyRows": 4, "columns": 4, "headerRows": 0, "footerRows": 0}),
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
        if id == "frameSize" {
            for k in ["width", "height"] {
                if let Some(v) = fields.get(k).and_then(Value::as_f64) {
                    fields.insert(k.into(), json!(format_measure(v, Unit::Picas)));
                }
            }
        }
        Dialog { id: id.into(), fields, unit: Unit::Picas }
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
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => parse_measure(s, self.unit).ok(),
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
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(w).horizontal_align(egui::Align::Min)).changed() {
        d.fields.insert(key.into(), Value::String(s));
    }
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

/// Edit › Keyboard Shortcuts: every command and its shortcut; click a shortcut, then press the
/// new keys (Esc cancels, Backspace clears).
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

fn is_modifier_key(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(key, ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | SuperLeft | SuperRight)
}

fn keyboard_shortcuts(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let mut q = d.s("query");
    ui.add(
        egui::TextEdit::singleline(&mut q)
            .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search commands")))
            .desired_width(f32::INFINITY),
    );
    d.fields.insert("query".into(), json!(q));
    let recording = d.s("recording");
    // Capture the next key press for the command being recorded. Pressing a modifier
    // reports the modifier itself as a key; skip it and wait for the key it modifies.
    if !recording.is_empty() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } if !is_modifier_key(*key) => Some((*key, *modifiers)),
                _ => None,
            })
        });
        if let Some((key, m)) = pressed {
            let sc = match key {
                egui::Key::Escape => None,
                egui::Key::Backspace if !m.any() => Some(String::new()),
                _ => Some(crate::menus::shortcut_string(m, key)),
            };
            if let Some(sc) = sc {
                let msg = match app.run("window.setShortcut", json!({"id": recording, "shortcut": sc})) {
                    Ok(v) => match v["conflicts"].as_array().filter(|c| !c.is_empty()) {
                        Some(c) => format!("Also used by: {}", c.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")),
                        None => String::new(),
                    },
                    Err(e) => e,
                };
                d.fields.insert("message".into(), json!(msg));
            }
            d.fields.insert("recording".into(), json!(""));
        }
    }
    let ql = q.to_lowercase();
    let mut rows: Vec<(String, String, String)> = designcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.menu.is_empty() || c.shortcut.is_some())
        .map(|c| (c.id.to_string(), c.label.to_string(), c.menu.join(" › ")))
        .chain(crate::menus::UI_COMMANDS.iter().map(|c| (c.0.to_string(), c.1.to_string(), String::new())))
        .filter(|(id, l, _)| ql.is_empty() || l.to_lowercase().contains(&ql) || id.to_lowercase().contains(&ql))
        .collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1));
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        egui::Grid::new("shortcuts").num_columns(3).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
            for (id, label, menu) in &rows {
                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label.trim_end_matches('…')))
                    .on_hover_text(format!("{id}{}", if menu.is_empty() { String::new() } else { format!("  ({menu})") }));
                let cur = crate::menus::shortcut_of(app, id).map(|s| crate::menus::shortcut_text(&s)).unwrap_or_else(|| "—".into());
                let text = if recording == *id { crate::i18n::tr(&app.ui.language, "Press keys…").to_string() } else { cur };
                if ui.add(egui::Button::new(text).min_size(egui::vec2(110.0, 0.0))).clicked() {
                    d.fields.insert("recording".into(), json!(id));
                    d.fields.insert("message".into(), json!(""));
                }
                if app.ui.shortcuts.contains_key(id) {
                    if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Default"))).clicked() {
                        let _ = app.run("window.setShortcut", json!({"id": id, "shortcut": null}));
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    });
    let msg = d.s("message");
    if !msg.is_empty() {
        ui.label(egui::RichText::new(msg).color(egui::Color32::from_rgb(240, 180, 60)));
    }
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reset All to Defaults"))).clicked() {
            let _ = app.run("window.resetShortcuts", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Import InDesign Set…"))).clicked()
            && let Some(msg) = crate::shortcut_import::pick_and_import(app)
        {
            d.fields.insert("message".into(), json!(msg));
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
/// (`UiState.pdf_export`) and the active document. `pages` is per-job (not persisted); `flatten`
/// falls back to the global flattener preset when the persisted value is empty.
pub fn open_pdf_export(app: &mut DesignApp) {
    let mut fields = serde_json::to_value(&app.ui.pdf_export).unwrap_or_else(|_| json!({}));
    if let Value::Object(m) = &mut fields {
        let title = app.session.active().map(|st| st.doc.title.clone()).unwrap_or_default();
        m.insert("title".into(), json!(title));
        m.insert("pages".into(), json!("all"));
        if app.ui.pdf_export.flatten.is_empty() {
            m.insert("flatten".into(), json!(app.ui.flattener));
        }
    }
    app.ui.dialog = Some(Dialog::new("pdfExport", fields));
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
            egui::ComboBox::from_id_salt("printer").selected_text(&cur).width(220.0).show_ui(ui, |ui| {
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
const PDF_EXPORT_PRESETS: &[&str] = &["Desktop Printing", "Commercial Printing", "Screen and Email", "PDF/X-4", "PDF/A-2b", "Custom"];

/// Apply a preset's dependent fields. Flatten is only touched by "Screen and Email"; "Custom"
/// (and any unknown value) changes nothing.
fn apply_pdf_preset(d: &mut Dialog, preset: &str) {
    let (standard, compress, crop, bleed, page_info, flatten) = match preset {
        "Desktop Printing" => ("none", false, false, false, false, None),
        "Commercial Printing" => ("none", true, true, true, false, None),
        "Screen and Email" => ("none", true, false, false, false, Some("low")),
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
        egui::ComboBox::from_id_salt("pdf_preset").selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, &cur))).width(200.0).show_ui(
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
        translated_combo(lang, ui, d, "standard", &[("none", "None"), ("x4", "PDF/X-4"), ("a2b", "PDF/A-2b")]);
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
    let shown = json!({
        "columns": o.columns, "gutter": fm(o.gutter),
        "columnsKind": kind(serde_json::to_value(o.columns_kind).unwrap_or_default()), "columnWidth": fm(width),
        "balanceColumns": o.balance_columns, "ignoreWrap": o.ignore_wrap,
        "firstBaseline": kind(serde_json::to_value(o.first_baseline).unwrap_or_default()), "firstBaselineMin": fm(o.first_baseline_min),
        "insetTop": fm(o.inset[0]), "insetLeft": fm(o.inset[1]), "insetBottom": fm(o.inset[2]), "insetRight": fm(o.inset[3]),
        "verticalJustification": serde_json::to_value(o.vertical_justification).unwrap_or(Value::Null),
        "columnRule": o.column_rule, "columnRuleWeight": format_measure(o.column_rule_weight, Unit::Points), "columnRuleColor": o.column_rule_color,
        "columnRuleTint": percent(o.column_rule_tint), "columnRuleTopInset": fm(o.column_rule_top_inset),
        "columnRuleBottomInset": fm(o.column_rule_bottom_inset), "columnRuleOffset": fm(o.column_rule_offset),
        "autoSize": serde_json::to_value(o.auto_size).unwrap_or(Value::Null),
        "autoSizeReference": designcraft_engine::autosize::REFERENCES.get(usize::from(o.auto_size_ref)).copied().unwrap_or("topCenter"),
        "useMinimumHeight": o.auto_size_use_min_height, "minimumHeight": fm(o.auto_size_min_height),
        "useMinimumWidth": o.auto_size_use_min_width, "minimumWidth": fm(o.auto_size_min_width),
        "noLineBreaks": o.auto_size_no_line_breaks,
    });
    if let Value::Object(m) = &shown {
        for (k, v) in m {
            d.fields.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    d.fields.entry("insetLinked".to_string()).or_insert_with(|| json!(o.inset.iter().all(|v| (v - o.inset[0]).abs() < 1e-9)));
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
            &[("general", "General"), ("baseline", "Baseline Options"), ("autosize", "Auto-Size"), ("rules", "Column Rules")],
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
            }
            "autosize" => auto_size(&lang, ui, d),
            "rules" => column_rules(app, ui, d),
            _ => general(&lang, ui, d),
        });
    });
}

/// Text Frame Options › General: columns, inset spacing, vertical justification, text wrap.
fn general(lang: &str, ui: &mut egui::Ui, d: &mut Dialog) {
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    crate::rtl::label(ui, egui::RichText::new(tr("Columns")).font(semibold(12.0)));
    egui::Grid::new("tfo_cols").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Columns:"));
        translated_combo(lang, ui, d, "columnsKind", &[("fixedNumber", "Fixed Number"), ("fixedWidth", "Fixed Width")]);
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr_context(lang, "Number:", "count"));
        text_field(ui, d, "columns", 70.0);
        // Fixed Number: the width follows from the frame; Fixed Width: it's set here.
        crate::rtl::label(ui, tr("Width:"));
        ui.add_enabled_ui(d.s("columnsKind") == "fixedWidth", |ui| text_field(ui, d, "columnWidth", 70.0));
        ui.end_row();
        crate::rtl::label(ui, tr("Gutter:"));
        text_field(ui, d, "gutter", 70.0);
        ui.end_row();
    });
    check(ui, d, "balanceColumns", tr("Balance Columns"));
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Inset Spacing")).font(semibold(12.0)));
    // While linked, a side typed in sets all four ("Make all settings the same").
    let sides = ["insetTop", "insetLeft", "insetBottom", "insetRight"];
    let before: Vec<String> = sides.iter().map(|k| d.s(k)).collect();
    // Two rows, Top | Left and Bottom | Right, the link between the columns (fixed-width
    // captions; the second column starts where the first row's does).
    let mut col2_x: Option<f32> = None;
    for (r, row) in [[("Top:", "insetTop"), ("Left:", "insetLeft")], [("Bottom:", "insetBottom"), ("Right:", "insetRight")]].into_iter().enumerate() {
        ui.horizontal(|ui| {
            for (k, (label, key)) in row.into_iter().enumerate() {
                if k == 1 {
                    match col2_x {
                        Some(x) => ui.add_space((x - ui.cursor().min.x).max(0.0)),
                        None => col2_x = Some(ui.cursor().min.x),
                    }
                }
                let x0 = ui.cursor().min.x;
                crate::rtl::label(ui, tr(label));
                ui.add_space(((if k == 0 { 56.0 } else { 44.0 }) - (ui.cursor().min.x - x0)).max(0.0));
                text_field(ui, d, key, 70.0);
                if k == 0 && r == 0 {
                    let linked = d.b("insetLinked");
                    let tip = tr("Make all settings the same");
                    if crate::widgets::icon_toggle_sized(ui, if linked { "link" } else { "link-broken" }, linked, tip, egui::vec2(22.0, 21.0))
                        .clicked()
                    {
                        d.fields.insert("insetLinked".into(), json!(!linked));
                    }
                }
            }
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
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Vertical Justification")).font(semibold(12.0)));
    egui::Grid::new("tfo_vj").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Align:"));
        translated_combo(lang, ui, d, "verticalJustification", &[("top", "Top"), ("center", "Center"), ("bottom", "Bottom"), ("justify", "Justify")]);
        ui.end_row();
    });
    ui.add_space(6.0);
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
        let resp = ui.interact(dot.expand(2.0), ui.id().with((key, i)), egui::Sense::click());
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

fn confirm_text_frame_options(app: &mut DesignApp, d: &mut Dialog) -> Result<Value, String> {
    seed_text_frame_options(app, d);
    let d = &*d;
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
    if p.is_empty() {
        return Ok(Value::Null);
    }
    // Typing in a frame: the options apply to that frame.
    if let Some(st) = app.session.active()
        && st.selection.items.is_empty()
        && let Some(f) = st.selection.text.and_then(|t| t.frame)
    {
        p.insert("ids".into(), json!([f.0]));
    }
    app.run("object.textFrameOptions", Value::Object(p))
}

/// File › Document Setup, filled from the document.
/// The active document's ruler unit (picas without a document).
fn doc_unit(app: &DesignApp) -> Unit {
    app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas)
}

/// Layout › Create Guides…: guides go on the page in view (or the parent page being edited), not
/// on the first page, and the gutters are typed in the document's ruler unit.
pub fn open_create_guides(app: &mut DesignApp) {
    let unit = doc_unit(app);
    let Some((spread, page)) = crate::canvas::current_spread_page(app) else { return };
    let editing_parents = matches!(spread, SpreadRef::Parent(_));
    let gutter = json!(format_measure(12.0, unit));
    let mut d = Dialog::new(
        "createGuides",
        json!({"rows": 0, "columns": 0, "rowGutter": gutter, "columnGutter": gutter, "fitTo": "margins", "removeExisting": false,
            "spread": serde_json::to_value(spread).unwrap_or(json!(0)), "page": page,
            // A parent spread is edited as a whole (both pages of a facing spread), so the guides show on every page that uses it.
            "target": if editing_parents { "spread" } else { "page" },
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
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &tip));
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
    let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(w).horizontal_align(egui::Align::Min));
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
        let resp = ui.interact(half, ui.id().with((key, dir < 0.0)), egui::Sense::click());
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
    let resp = child.add(egui::TextEdit::singleline(&mut s).id(ui.id().with((key, "text"))).frame(egui::Frame::NONE).desired_width(text.width()));
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
        egui::ComboBox::from_id_salt("ds_intent")
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
        egui::ComboBox::from_id_salt("ds_binding")
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
            egui::ComboBox::from_id_salt("ds_preset").selected_text(crate::rtl::widget(ui, &preset)).width(260.0).show_ui(ui, |ui| {
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

/// Preferences: a section list and the section's options. Application options always; units and
/// increments when a document is open (InDesign keeps those with the document).
fn preferences(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog, max_height: f32) {
    let has_doc = d.fields.contains_key("horizontalUnits");
    let sections: &[(&str, &str)] = if has_doc {
        &[
            ("general", "General"),
            ("interface", "Interface"),
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
            ("type", "Type"),
            ("composition", "Composition"),
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
                        if ui.add(egui::DragValue::new(&mut v).range(0.0..=f64::from(designcraft_engine::MAX_RECENT_FILES)).speed(0.2)).changed() {
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
                        egui::ComboBox::from_id_salt("pref_black")
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
                        egui::ComboBox::from_id_salt("pref_black_out")
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
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "UI Scaling")).font(semibold(12.0)));
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "UI Size:"));
                        let mut v = d.n("uiScale").unwrap_or(100.0);
                        if ui.add(egui::Slider::new(&mut v, 50.0..=200.0).step_by(5.0).suffix("%")).changed() {
                            d.fields.insert("uiScale".into(), json!(v));
                        }
                    });
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
                        egui::ComboBox::from_id_salt("bg_rel")
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
                        egui::ComboBox::from_id_salt("pref_dq")
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
                        egui::ComboBox::from_id_salt("rulerOrigin")
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
                            egui::ComboBox::from_id_salt(key)
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
                    egui::ComboBox::from_id_salt("pref_language").width(180.0).height(400.0).selected_text(crate::rtl::widget(ui, current)).show_ui(
                        ui,
                        |ui| {
                            for (code, name) in crate::i18n::LANGUAGES {
                                if ui.selectable_label(*code == app.ui.language, crate::rtl::widget(ui, *name)).clicked() {
                                    // Applied by `show` right after this frame, which has `&mut DesignApp`.
                                    d.fields.insert("applyLanguage".into(), json!(code));
                                }
                            }
                        },
                    );
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

pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(mut d) = app.ui.dialog.clone() else { return };
    // Enter and Escape belong to an open popup (a menu, a name field) rather than the dialog.
    // Read before the dialog draws: a popup closed by this frame's Escape is still open here.
    let popup_open = egui::Popup::is_any_open(ctx);
    let mut result: Option<bool> = None;
    let alert_title = d.s("title");
    let title = match d.id.as_str() {
        "alert" => crate::i18n::tr(&app.ui.language, &alert_title),
        "ruby" => crate::i18n::tr(&app.ui.language, "Ruby"),
        "newDocument" => crate::i18n::tr(&app.ui.language, "New Document"),
        "frameSize" => crate::i18n::tr(&app.ui.language, "Rectangle"),
        "goToPage" => crate::i18n::tr(&app.ui.language, "Go to Page"),
        "insertTable" => crate::i18n::tr(&app.ui.language, "Create Table"),
        "cellOptions" => crate::i18n::tr(&app.ui.language, "Cell Options"),
        "createGuides" => crate::i18n::tr(&app.ui.language, "Create Guides"),
        "marginsAndColumns" => crate::i18n::tr(&app.ui.language, "Margins and Columns"),
        "textFrameOptions" => crate::i18n::tr(&app.ui.language, "Text Frame Options"),
        "colorTheme" => crate::i18n::tr(&app.ui.language, "Color Theme"),
        "documentSetup" => crate::i18n::tr(&app.ui.language, "Document Setup"),
        "numberingSection" => crate::i18n::tr(&app.ui.language, "Numbering & Section Options…").trim_end_matches('…'),
        "findChange" => crate::i18n::tr(&app.ui.language, "Find/Change"),
        "paragraphStyleOptions" => crate::i18n::tr(&app.ui.language, "Paragraph Style Options"),
        "deleteParagraphStyle" => crate::i18n::tr(&app.ui.language, "Delete Paragraph Style"),
        "characterStyleOptions" => crate::i18n::tr(&app.ui.language, "Character Style Options"),
        "deleteCharacterStyle" => crate::i18n::tr(&app.ui.language, "Delete Character Style"),
        "footnoteOptions" => crate::i18n::tr(&app.ui.language, "Footnote Options"),
        "paragraphRules" => crate::i18n::tr(&app.ui.language, "Paragraph Rules"),
        "paragraphLocal" => crate::i18n::tr(
            &app.ui.language,
            match d.s("section").as_str() {
                "span" => "Span Columns",
                "bullets" => "Bullets and Numbering",
                _ => "Paragraph Borders and Shading",
            },
        ),
        "justification" => crate::i18n::tr(&app.ui.language, "Justification"),
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
        "colorSettings" => crate::i18n::tr(&app.ui.language, "Color Settings"),
        "layerOptions" => crate::i18n::tr(&app.ui.language, "Object Layer Options"),
        "keyboardShortcuts" => crate::i18n::tr(&app.ui.language, "Keyboard Shortcuts"),
        "colorPicker" => crate::i18n::tr(&app.ui.language, "Color Picker"),
        "swatchOptions" => crate::i18n::tr(&app.ui.language, "Swatch Options"),
        "colorGroupOptions" => crate::i18n::tr(&app.ui.language, "Color Group Options"),
        "layerSettings" => crate::i18n::tr(&app.ui.language, "Layer Options"),
        crate::effects_dialog::ID => crate::i18n::tr(&app.ui.language, "Effects"),
        id => match id.strip_prefix("cmd:").and_then(designcraft_engine::find_command) {
            Some(c) => crate::i18n::tr(&app.ui.language, c.label.trim_end_matches('…')),
            None => crate::i18n::tr(&app.ui.language, "Dialog"),
        },
    };
    // No dimmed backdrop: the page behind a dialog is what its settings and colours are judged against.
    let host = egui::Id::new("dialog");
    let instance = host.with(&d.id);
    crate::modal_drag::modal(ctx, host, instance).backdrop_color(egui::Color32::TRANSPARENT).show(ctx, |ui| {
        let frame_margin = egui::Frame::popup(ui.style()).total_margin().sum();
        let available = (ctx.content_rect().size() - frame_margin - egui::vec2(16.0, 16.0)).max(egui::Vec2::splat(1.0));
        let preferred_width: f32 = match d.id.as_str() {
            "alert" => 440.0,
            "closeDocument" | "updateLink" | "deletePages" | "newWorkspace" | "pagesPanelOptions" => 380.0,
            "newDocument" => 960.0,
            "numberingSection" => 560.0,
            crate::effects_dialog::ID => 720.0,
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
                                    egui::ComboBox::from_id_salt(key)
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
                                egui::ComboBox::from_id_salt("objKind")
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
                            egui::Grid::new("fc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Find what:"));
                                text_field(ui, &mut d, "find", 260.0);
                                ui.end_row();
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Change to:"));
                                text_field(ui, &mut d, "change", 260.0);
                                ui.end_row();
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Search:"));
                                let cur = d.s("scope");
                                egui::ComboBox::from_id_salt("fcscope")
                                    .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &cur)))
                                    .show_ui(ui, |ui| {
                                        for v in ["document", "story", "selection"] {
                                            if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, v))).clicked()
                                                && d.set_field("scope", json!(v))
                                            {
                                                app.session.reset_find_scope();
                                            }
                                        }
                                    });
                                ui.end_row();
                            });
                            ui.horizontal(|ui| {
                                check(ui, &mut d, "caseSensitive", crate::i18n::tr(&app.ui.language, "Case sensitive"));
                                check(ui, &mut d, "wholeWord", crate::i18n::tr(&app.ui.language, "Whole word"));
                            });
                            ui.horizontal(|ui| {
                                let params0 = find_change_params(&d);
                                let params = || params0.clone();
                                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Find Next"))).clicked() {
                                    let r = app.run("find.next", params());
                                    let msg = match r {
                                        Ok(Value::Null) => crate::i18n::tr(&app.ui.language, "No matches").to_string(),
                                        Ok(_) => crate::i18n::tr(&app.ui.language, "Found").to_string(),
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
                    "insertXref" => insert_xref(app, ui, &mut d),
                    "findFont" => find_font(app, ui, &mut d),
                    "colorPicker" => color_picker(ui, &mut d),
                    "swatchOptions" => options::swatch_options(app, ui, &mut d),
                    "colorGroupOptions" => options::color_group_options(app, ui, &mut d),
                    "layerSettings" => options::layer_options(app, ui, &mut d),
                    crate::effects_dialog::ID => crate::effects_dialog::show(app, ui, &mut d),
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
                            egui::ComboBox::from_id_salt("pdf_crop").selected_text(crate::rtl::widget(ui, label(&cur))).show_ui(ui, |ui| {
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
                            }
                        }
                        d.fields.insert("layers".into(), json!(layers));
                    }
                    "colorSettings" => {
                        let profiles = d.fields.get("profiles").and_then(Value::as_array).cloned().unwrap_or_default();
                        egui::Grid::new("color_settings").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                            for (key, label, kind) in [("rgb", "RGB:", "rgb"), ("cmyk", "CMYK:", "cmyk")] {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                                let cur = d.s(key);
                                egui::ComboBox::from_id_salt(("ws", key)).selected_text(&cur).width(260.0).show_ui(ui, |ui| {
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
                            egui::ComboBox::from_id_salt("ws_intent")
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
                            && let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("icc"))
                        {
                            match app.session.execute("color.loadProfile", &json!({"path": path})) {
                                Ok(_) => {
                                    let f = app.session.execute("color.settings", &json!({})).unwrap_or_default();
                                    d.fields.insert("profiles".into(), f["profiles"].clone());
                                }
                                Err(e) => app.status_error("Color Settings", e),
                            }
                        }
                    }
                    "keyboardShortcuts" => keyboard_shortcuts(app, ui, &mut d),
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
                                        egui::ComboBox::from_id_salt(("imp_map", w))
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
                            egui::ComboBox::from_id_salt("ff_fit").selected_text(crate::rtl::widget(ui, label(&cur))).width(200.0).show_ui(
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
                            egui::ComboBox::from_id_salt("qr_type").selected_text(crate::rtl::widget(ui, label(&cur))).width(160.0).show_ui(
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
                    TOC => toc_dialog(app, ui, &mut d),
                    id if id.starts_with("cmd:") => command_form(app, ui, &mut d),
                    "goToPage" => {
                        ui.horizontal(|ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page"));
                            text_field(ui, &mut d, "page", 80.0);
                        });
                    }
                    "ruby" => style_cjk::ruby_dialog(app, ui, &mut d),
                    "insertTable" => {
                        crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Table Dimensions")).font(semibold(12.0)));
                        egui::Grid::new("ins_table").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Body Rows"));
                            text_field(ui, &mut d, "bodyRows", 60.0);
                            crate::rtl::label(ui, crate::i18n::tr_context(&app.ui.language, "Columns", "table"));
                            text_field(ui, &mut d, "columns", 60.0);
                            ui.end_row();
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Header Rows"));
                            text_field(ui, &mut d, "headerRows", 60.0);
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Footer Rows"));
                            text_field(ui, &mut d, "footerRows", 60.0);
                            ui.end_row();
                        });
                    }
                    "cellOptions" => cell_options(app, ui, &mut d),
                    "createGuides" => create_guides_dialog(app, ui, &mut d),
                    "marginsAndColumns" => margins_and_columns_dialog(app, ui, &mut d),
                    "textFrameOptions" => {
                        seed_text_frame_options(app, &mut d);
                        text_frame_options_dialog(app, ui, &mut d);
                    }
                    "colorTheme" => color_theme(app, ui, &mut d),
                    "documentSetup" => {
                        // Adjust Layout applies at once.
                        let adjust = document_setup(app, ui, &mut d);
                        if adjust {
                            result = Some(true);
                        }
                    }
                    "numberingSection" => crate::section_options::body(app, ui, &mut d),
                    "pdfExport" => pdf_export(app, ui, &mut d),
                    "alert" => {
                        let file = d.s("file");
                        if !file.is_empty() {
                            crate::rtl::label(ui, egui::RichText::new(crate::rtl::isolate(&file)).font(semibold(13.0)));
                            ui.add_space(4.0);
                        }
                        crate::rtl::label(ui, d.s("message"));
                    }
                    _ => {}
                }
            });
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
                style_options_preview(app, &mut d);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(ok_label).color(egui::Color32::WHITE)))
                            .fill(crate::theme::Tokens::get(ui.ctx()).accent_strong),
                    )
                    .clicked()
                    || (!popup_open && ui.input(|i| i.key_pressed(egui::Key::Enter)))
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
    // Preferences › General › Interface Language takes effect at once, like any other app setting.
    if let Some(lang) = d.fields.remove("applyLanguage")
        && let Err(e) = app.run("app.language", json!({"lang": lang}))
    {
        d.fields.insert("status".into(), json!(e));
    }
    if d.id == "newDocument" && result.is_none() {
        crate::new_document::preview(app, &mut d, ctx);
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
    crate::new_document::end_preview(app);
    if d.as_ref().is_some_and(|d| d.fields.contains_key(PREVIEWED)) {
        app.session.cancel_interaction();
    }
    app.cancel_pdf_import();
    if let Some(d) = d
        && d.id == "preferences"
        && let Some(lang) = d.fields.get("openedLanguage").and_then(Value::as_str)
        && lang != app.ui.language
    {
        let _ = app.run("app.language", json!({"lang": lang}));
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
    p
}

/// Apply the open dialog.
pub fn confirm(app: &mut DesignApp) -> Result<Value, String> {
    let Some(d) = app.ui.dialog.take() else { return Err("no dialog open".into()) };
    if matches!(d.id.as_str(), "paragraphStyleOptions" | "characterStyleOptions") && d.fields.contains_key(PREVIEWED) {
        // Restore the original before the ordinary command commits one undoable edit.
        app.session.cancel_interaction();
    }
    if d.id != "pdfImport" || !d.b("async") {
        app.cancel_pdf_import();
    }
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
            let h = d.m("height").unwrap_or(72.0);
            app.run(
                "frame.create",
                json!({"spread": d.fields.get("spread").cloned().unwrap_or(json!(0)), "shape": d.s("shape"), "content": d.s("content"), "rect": [x, y, x + w, y + h]}),
            )
        }
        "ruby" => app.run("type.ruby", style_cjk::ruby_params(&d)),
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
        "insertTable" => app.run(
            "table.insert",
            json!({"rows": d.n("bodyRows").unwrap_or(4.0).max(1.0) as u64, "cols": d.n("columns").unwrap_or(4.0).max(1.0) as u64,
                "headerRows": d.n("headerRows").unwrap_or(0.0).max(0.0) as u64, "footerRows": d.n("footerRows").unwrap_or(0.0).max(0.0) as u64}),
        ),
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
                app.run("style.character.create", params)
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
            let hidden: Vec<Value> = d.fields.get("layers").and_then(Value::as_array).map(|a| a.iter().filter(|l| l["visible"] == false).map(|l| l["name"].clone()).collect()).unwrap_or_default();
            app.run("object.layerOptions", json!({"hidden": hidden}))
        }
        "colorSettings" => {
            let r = app.run("color.settings", json!({"rgb": d.s("rgb"), "cmyk": d.s("cmyk"), "intent": d.s("intent"), "bpc": d.b("bpc")}))?;
            app.ui.color_settings = Some(designcraft_color::cms::active_settings());
            app.canvas.shown = None;
            Ok(r)
        }
        "pdfImport" => {
            let crop = d.s("crop");
            if d.b("async") {
                return app.confirm_pdf_import(d.n("page").unwrap_or(1.0).max(1.0) as u64, if crop.is_empty() { "crop".into() } else { crop });
            }
            if let Some(target) = d.fields.get("target").and_then(Value::as_u64) {
                app.activate_import_target(&crate::ImportRequest { purpose: "place".into(), target: Some(target) })?;
            }
            app.place_file(json!({"path": d.s("path"), "pdfPage": d.n("page").unwrap_or(1.0).max(1.0) as u64, "pdfCrop": if crop.is_empty() { "crop".to_string() } else { crop }}))
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
            app.status(crate::i18n::tr(&app.ui.language, "Sent {count} page(s) to {printer}").replace("{count}", &r["pages"].to_string()).replace("{printer}", printer));
            Ok(r)
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
            let pages = d.s("pages");
            if !pages.trim().is_empty() && pages != "all" {
                params["pages"] = json!(pages);
            }
            let title = d.s("title");
            if !title.is_empty() {
                params["title"] = json!(title);
            }
            let name = if title.is_empty() { "Export.pdf".to_string() } else { format!("{title}.pdf") };
            match app.services.pick_save.as_mut().and_then(|f| f(&name)) {
                Some(path) => {
                    params["path"] = json!(path);
                    crate::menus::export_pdf(app, &params)
                }
                // The user cancelled the save dialog: export nothing.
                None => Ok(Value::Null),
            }
        }
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
            app.run("prefs.set", prefs)?;
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
            if let Some(v) = d.n("uiScale") {
                app.run("window.uiScale", json!({"scale": v / 100.0}))?;
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
            let index = |app: &DesignApp| app.session.documents().iter().position(|doc| Some(doc.uid) == uid);
            let Some(i) = index(app) else { return Ok(Value::Null) };
            if !d.b("discard") {
                app.run("file.activate", json!({"index": i}))?;
                app.run("app.save", json!({}))?;
                // Still unsaved: the save was cancelled (no file chosen). Keep the document open.
                if app.session.documents().get(i).is_none_or(|doc| doc.is_dirty()) {
                    return Ok(Value::Null);
                }
            }
            match index(app) {
                Some(i) => app.run("file.close", json!({"index": i})),
                None => Ok(Value::Null),
            }
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
        "findChange" => {
            let result = if d.b("objectMode") { Ok(Value::Null) } else { app.run("find.change", find_change_params(&d)) };
            // Confirmation consumes retained bounds before closing, even on an error.
            app.session.reset_find_scope();
            result
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

/// The Justification spacing check of Paragraph Style Options, against what the section showed:
/// the style's own values, or a new style's Based On.
fn check_paragraph_style_justification(app: &DesignApp, d: &Dialog) -> Result<(), String> {
    let shown = if d.b("new") { d.s("basedOn") } else { d.s("name") };
    let base =
        app.session.active().map(|st| serde_json::to_value(st.doc.styles.resolve_para_style(&shown).0).unwrap_or_default()).unwrap_or_default();
    check_justification(d, "p.", &base)
}

fn confirm_paragraph_style_options(app: &mut DesignApp, d: &mut Dialog) -> Result<Value, String> {
    let name = d.s("name");
    if let Err(e) = check_paragraph_style_justification(app, d) {
        // The caller shows the reason in the dialog's status line.
        d.fields.insert("section".into(), json!("justify"));
        return Err(e);
    }
    let mut para = serde_json::Map::new();
    let mut chars = serde_json::Map::new();
    for (k, v) in &d.fields {
        if let Some(a) = k.strip_prefix("p.") {
            // `p.ruleAbove.weight`: one field of a rule (the command keeps the others).
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
    let based = d.s("basedOn");
    let rename = d.s("rename");
    let (r, style) = if d.b("new") {
        let mut params = json!({"name": if rename.trim().is_empty() { "Paragraph Style 1" } else { rename.trim() }, "para": para, "chars": chars});
        if !based.is_empty() && based != designcraft_doc::NO_PARA_STYLE {
            params["basedOn"] = json!(based);
        }
        let r = app.run("style.paragraph.create", params)?;
        let style = r["name"].as_str().unwrap_or_default().to_string();
        (r, style)
    } else {
        let mut params = json!({"name": name, "para": para, "chars": chars});
        if !based.is_empty() {
            params["basedOn"] = if based == designcraft_doc::NO_PARA_STYLE { Value::Null } else { json!(based) };
        }
        if !rename.is_empty() && rename != name {
            params["rename"] = json!(rename);
        }
        let style = if !rename.is_empty() && rename != name { rename } else { name };
        (app.run("style.paragraph.edit", params)?, style)
    };
    // The style now exists under `style`: if a later step fails, the dialog stays open on it and
    // a second OK edits it rather than creating another one.
    d.fields.insert("new".into(), json!(false));
    d.fields.insert("name".into(), json!(&style));
    d.fields.insert("rename".into(), json!(&style));
    if d.fields.contains_key("x.tag") {
        let tag = d.s("x.tag");
        let tag = if tag == "[Automatic]" { String::new() } else { tag };
        app.run("style.exportTag", json!({"style": style, "tag": tag, "class": d.s("x.class")}))?;
    }
    Ok(r)
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
    if let Some(based) = d.fields.get("basedOn") {
        params["basedOn"] = match based.as_str() {
            Some(designcraft_doc::NO_PARA_STYLE | designcraft_doc::NO_CHAR_STYLE) => Value::Null,
            _ => based.clone(),
        };
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
    let names: Vec<String> = doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| new || *n != name).collect();
    // A new style shows what it would inherit from its Based On.
    let (pp, cp) = doc.styles.resolve_para_style(if new { d.fields.get("basedOn").and_then(Value::as_str).unwrap_or("") } else { &name });
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
                }
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Tag:"));
                    let cur = d.s("x.tag");
                    egui::ComboBox::from_id_salt("export_tag")
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
            }
            "nested" => {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Drop Caps")).font(semibold(12.0)));
                egui::Grid::new("psdc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    for (label, key, max) in [
                        ("Lines:", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES),
                        ("Characters:", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS),
                    ] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        let v = cur(d, &format!("p.{key}"), &pv[key]).as_f64();
                        if let Some(n) = crate::widgets::number(ui, &format!("ps{key}"), v, "", 60.0, 0) {
                            d.fields.insert(format!("p.{key}"), json!((n.max(0.0) as u64).min(max as u64)));
                        }
                        ui.end_row();
                    }
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Character Style:"));
                    let style = cur(d, "p.dropCapStyle", &pv["dropCapStyle"]).as_str().unwrap_or("").to_string();
                    let style = if style.is_empty() { designcraft_doc::NO_CHAR_STYLE.to_string() } else { style };
                    egui::ComboBox::from_id_salt("ps_dropcap_style")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &style)))
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for c in &cnames {
                                if ui.selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, c))).clicked() {
                                    d.fields.insert("p.dropCapStyle".into(), json!(c));
                                }
                            }
                        });
                    ui.end_row();
                });
                for (label, key) in [("Align Left Edge", "dropCapAlignLeft"), ("Scale for Descenders", "dropCapScaleDescenders")] {
                    let mut on = cur(d, &format!("p.{key}"), &pv[key]).as_bool().unwrap_or(false);
                    if ui.checkbox(&mut on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).changed() {
                        d.fields.insert(format!("p.{key}"), json!(on));
                    }
                }
                ui.add_space(8.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Nested Styles")).font(semibold(12.0)));
                let mut list: Vec<Value> = cur(d, "p.nestedStyles", &pv["nestedStyles"]).as_array().cloned().unwrap_or_default();
                let mut changed = false;
                let mut remove = None;
                for (i, ns) in list.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let style = ns["style"].as_str().unwrap_or("[None]").to_string();
                        egui::ComboBox::from_id_salt(("ns_style", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &style)))
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for c in &cnames {
                                    if ui
                                        .selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, c)))
                                        .clicked()
                                    {
                                        ns["style"] = json!(c);
                                        changed = true;
                                    }
                                }
                            });
                        let through = ns["through"].as_bool().unwrap_or(true);
                        egui::ComboBox::from_id_salt(("ns_thr", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if through { "through" } else { "up to" })))
                            .width(70.0)
                            .show_ui(ui, |ui| {
                                for (v, l) in [(true, "through"), (false, "up to")] {
                                    if ui.selectable_label(v == through, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
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
                        if !crate::i18n::category_first(&app.ui.language) {
                            count_value(ui, ns, &mut changed);
                        }
                        let kind = ns["until"]["kind"].as_str().unwrap_or("words").to_string();
                        egui::ComboBox::from_id_salt(("ns_until", i))
                            .selected_text(crate::rtl::widget(
                                ui,
                                if crate::i18n::category_first(&app.ui.language) {
                                    format!("{}:", crate::i18n::tr(&app.ui.language, &kind))
                                } else {
                                    crate::i18n::tr(&app.ui.language, &kind).to_owned()
                                },
                            ))
                            .width(100.0)
                            .show_ui(ui, |ui| {
                                for k in [
                                    "sentences",
                                    "words",
                                    "characters",
                                    "letters",
                                    "digits",
                                    "tab",
                                    "forcedLineBreak",
                                    "emSpace",
                                    "enSpace",
                                    "dropcap",
                                    "chars",
                                ] {
                                    if ui.selectable_label(k == kind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, k))).clicked() {
                                        ns["until"] = if k == "chars" { json!({"kind": "chars", "chars": ":"}) } else { json!({"kind": k}) };
                                        changed = true;
                                    }
                                }
                            });
                        if crate::i18n::category_first(&app.ui.language) {
                            count_value(ui, ns, &mut changed);
                        }
                        if kind == "chars" {
                            let mut c = ns["until"]["chars"].as_str().unwrap_or("").to_string();
                            if ui.add(egui::TextEdit::singleline(&mut c).desired_width(40.0)).changed() {
                                ns["until"]["chars"] = json!(c);
                                changed = true;
                            }
                        }
                        if ui
                            .small_button("×")
                            .on_hover_ui(|ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Delete"));
                            })
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    list.remove(i);
                    changed = true;
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Nested Style"))).clicked() {
                    list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "through": true, "count": 1, "until": {"kind": "words"}}));
                    changed = true;
                }
                if changed {
                    d.fields.insert("p.nestedStyles".into(), Value::Array(list));
                }
                ui.add_space(8.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Nested Line Styles")).font(semibold(12.0)));
                let mut lines: Vec<Value> = cur(d, "p.nestedLineStyles", &pv["nestedLineStyles"]).as_array().cloned().unwrap_or_default();
                let mut changed = false;
                let mut remove = None;
                for (i, l) in lines.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let style = l["style"].as_str().unwrap_or("").to_string();
                        egui::ComboBox::from_id_salt(("nls_style", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &style)))
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for c in &cnames {
                                    if ui
                                        .selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, c)))
                                        .clicked()
                                    {
                                        l["style"] = json!(c);
                                        changed = true;
                                    }
                                }
                            });
                        crate::rtl::label(
                            ui,
                            if crate::i18n::category_first(&app.ui.language) {
                                format!("{}:", crate::i18n::tr(&app.ui.language, "lines"))
                            } else {
                                crate::i18n::tr(&app.ui.language, "for").to_owned()
                            },
                        );
                        let mut n = l["lines"].as_u64().unwrap_or(1) as u32;
                        if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
                            l["lines"] = json!(n);
                            changed = true;
                        }
                        if !crate::i18n::category_first(&app.ui.language) {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, if n == 1 { "line" } else { "lines" }));
                        }
                        if ui
                            .small_button("×")
                            .on_hover_ui(|ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Delete"));
                            })
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    lines.remove(i);
                    changed = true;
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New Line Style"))).clicked() {
                    lines.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "lines": 1}));
                    changed = true;
                }
                if changed {
                    d.fields.insert("p.nestedLineStyles".into(), Value::Array(lines));
                }
            }
            "grep" => {
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "GREP Styles")).font(semibold(12.0)));
                let mut list: Vec<Value> = cur(d, "p.grepStyles", &pv["grepStyles"]).as_array().cloned().unwrap_or_default();
                let mut changed = false;
                let mut remove = None;
                for (i, g) in list.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Apply Style:"));
                        let style = g["style"].as_str().unwrap_or("").to_string();
                        egui::ComboBox::from_id_salt(("gs_style", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &style)))
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for c in &cnames {
                                    if ui
                                        .selectable_label(*c == style, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, c)))
                                        .clicked()
                                    {
                                        g["style"] = json!(c);
                                        changed = true;
                                    }
                                }
                            });
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "To Text:"));
                        let mut pat = g["pattern"].as_str().unwrap_or("").to_string();
                        let bad = grep_error(&pat).is_some();
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut pat)
                                .desired_width(150.0)
                                .text_color_opt(bad.then_some(egui::Color32::from_rgb(240, 90, 90))),
                        );
                        if r.changed() {
                            g["pattern"] = json!(pat);
                            changed = true;
                        }
                        if ui
                            .small_button("×")
                            .on_hover_ui(|ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Delete"));
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
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "New GREP Style"))).clicked() {
                    list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "pattern": "\\d+"}));
                    changed = true;
                }
                if changed {
                    d.fields.insert("p.grepStyles".into(), Value::Array(list));
                }
            }
            "chars" => basic_character_formats(app, ui, d, &CharFields { base: &cv, sparse: false }),
            "indents" => {
                egui::Grid::new("psi").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Alignment:"));
                    let a: designcraft_doc::Align = serde_json::from_value(cur(d, "p.align", &pv["align"])).unwrap_or_default();
                    egui::ComboBox::from_id_salt("psalign")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, a.label())))
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for al in designcraft_doc::Align::ALL {
                                if ui.selectable_label(al == a, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, al.label()))).clicked() {
                                    d.fields.insert("p.align".into(), serde_json::to_value(al).unwrap_or_default());
                                }
                            }
                        });
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
                });
            }
            "tabs" => style_tabs(&app.ui.language, ui, d, &pv, &doc),
            "rules" => paragraph_rules(app, ui, d, "p.", &pv),
            "hyph" => {
                let mut h = cur(d, "p.hyphenate", &pv["hyphenate"]).as_bool().unwrap_or(true);
                if ui.checkbox(&mut h, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Hyphenate"))).changed() {
                    d.fields.insert("p.hyphenate".into(), json!(h));
                }
                egui::Grid::new("psh").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    for (label, key) in [
                        ("Words with at Least:", "hyphMinWord"),
                        ("After First:", "hyphAfterFirst"),
                        ("Before Last:", "hyphBeforeLast"),
                        ("Hyphen Limit:", "hyphLimit"),
                    ] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        let v = cur(d, &format!("p.{key}"), &pv[key]).as_f64();
                        if let Some(n) = crate::widgets::number(ui, &format!("ps{key}"), v, "", 60.0, 0) {
                            d.fields.insert(format!("p.{key}"), json!(n.max(0.0) as u64));
                        }
                        ui.end_row();
                    }
                });
            }
            "justify" => justification(&app.ui.language, ui, d, "p.", &pv),
            "color" => character_color(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }, &doc),
            "advanced" => advanced_character_formats(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }),
            "openType" => open_type_features(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }),
            "underline" => line_options(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }, &doc, "underline"),
            "strikethrough" => line_options(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }, &doc, "strikethrough"),
            "border" => border_shading(&app.ui.language, ui, d, &pv, &doc, true),
            "shading" => border_shading(&app.ui.language, ui, d, &pv, &doc, false),
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
                    if ui.add(egui::TextEdit::singleline(&mut rn).desired_width(220.0)).changed() {
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
                    egui::ComboBox::from_id_salt("psbased")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, &based)))
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for n in &names {
                                if ui.selectable_label(*n == based, crate::rtl::widget(ui, crate::i18n::style_name(&app.ui.language, n))).clicked() {
                                    d.fields.insert("basedOn".into(), json!(n));
                                }
                            }
                        });
                    ui.end_row();
                });
                ui.add_space(8.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Settings:")).font(semibold(12.0)));
                let summary = paragraph_style_summary(&app.ui.language, d, &style, &doc.styles);
                egui::ScrollArea::vertical().id_salt("paragraph_style_settings").max_height(220.0).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(summary).color(crate::theme::Tokens::get(ui.ctx()).text_dim)).wrap());
                });
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
    egui::ComboBox::from_id_salt("del_para_replace")
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
        if let (Some(l), Some(i)) = (text_commit(ui, "psleader", stop.as_ref().map_or("", |s| s.leader.as_str()), 36.0, stop.is_some()), sel) {
            result = Some(tabs::leader_value(&l, "tabs").map_err(|e| e.to_string()).and_then(|l| tabs::change(&mut list, i, None, Some(l), None)));
        }
        crate::rtl::label(ui, crate::i18n::tr(lang, "Align On:"));
        let on_char = stop.as_ref().is_some_and(|s| s.align == designcraft_doc::TabAlign::Char);
        if let (Some(c), Some(i)) = (text_commit(ui, "psalignon", stop.as_ref().map_or("", |s| s.align_on.as_str()), 18.0, on_char), sel) {
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
    let cur = para_value(d, pv, key);
    let shown = opts.iter().find(|o| cur == o.0).map_or("", |o| crate::i18n::tr(lang, o.1));
    egui::ComboBox::from_id_salt(("pschoice", key)).selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
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
    let mut changed = false;
    egui::Grid::new(("pssides", key)).num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        for (i, label) in ["Top:", "Left:", "Bottom:", "Right:"].into_iter().enumerate() {
            crate::rtl::label(ui, crate::i18n::tr(lang, label));
            if let Some(side) = sides.get_mut(i)
                && let Some(v) = field(ui, &format!("ps{key}{i}"), Some(*side))
            {
                *side = v;
                changed = true;
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    if changed {
        para_set(d, key, json!(sides));
    }
}

/// A swatch menu with a colour chip per swatch (hidden swatches left out); returns the swatch
/// picked.
fn swatch_menu(ui: &mut egui::Ui, doc: &designcraft_doc::Document, id: impl egui::AsIdSalt, current: &str, width: f32) -> Option<String> {
    let mut picked = None;
    egui::ComboBox::from_id_salt(id).selected_text(current).width(width).show_ui(ui, |ui| {
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
    egui::ComboBox::from_id_salt("psbordertype").selected_text(crate::rtl::widget(ui, shown)).width(160.0).show_ui(ui, |ui| {
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
/// bottom left); an edited corner stores all four.
fn para_corners(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, pv: &Value, key: &str, units: Unit) {
    use designcraft_geom::corners::{Corner, CornerShape};
    let mut corners: [Corner; 4] = serde_json::from_value(para_value(d, pv, key)).unwrap_or_default();
    let mut changed = false;
    egui::Grid::new(("pscorners", key)).num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
        for (i, label) in ["Top Left:", "Top Right:", "Bottom Right:", "Bottom Left:"].into_iter().enumerate() {
            let Some(c) = corners.get_mut(i) else { continue };
            crate::rtl::label(ui, crate::i18n::tr(lang, label));
            if let Some(v) = crate::widgets::measure(ui, &format!("ps{key}{i}"), Some(c.size), units, 70.0) {
                c.size = if v.is_finite() { v.clamp(0.0, 10_000.0) } else { 0.0 };
                changed = true;
            }
            egui::ComboBox::from_id_salt(("pscornershape", key, i))
                .selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, c.shape.label())))
                .width(130.0)
                .show_ui(ui, |ui| {
                    for shape in CornerShape::ALL {
                        if ui.selectable_label(c.shape == shape, crate::rtl::widget(ui, crate::i18n::tr(lang, shape.label()))).clicked() {
                            c.shape = shape;
                            changed = true;
                        }
                    }
                });
            ui.end_row();
        }
    });
    if changed {
        para_set(d, key, serde_json::to_value(corners).unwrap_or_default());
    }
}

/// The colour and tint rows of a border, its gap or shading (`keys`: colour, tint), in a grid.
fn para_color_tint(
    ui: &mut egui::Ui,
    d: &mut Dialog,
    pv: &Value,
    doc: &designcraft_doc::Document,
    keys: (&str, &str),
    labels: (&str, &str),
    enabled: bool,
) {
    let (color_key, tint_key) = keys;
    crate::rtl::label(ui, labels.0);
    let color = para_value(d, pv, color_key).as_str().unwrap_or("").to_string();
    if let Some(w) = ui.add_enabled_ui(enabled, |ui| swatch_menu(ui, doc, ("pscolor", color_key), &color, 160.0)).inner {
        para_set(d, color_key, json!(w));
    }
    ui.end_row();
    crate::rtl::label(ui, labels.1);
    let tint = para_value(d, pv, tint_key).as_f64().map(|t| t * 100.0);
    if let Some(v) = ui.add_enabled_ui(enabled, |ui| crate::widgets::number(ui, &format!("ps{tint_key}"), tint, "%", 60.0, 0)).inner {
        para_set(d, tint_key, json!(if v.is_finite() { (v / 100.0).clamp(0.0, 1.0) } else { 1.0 }));
    }
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
    egui::ScrollArea::vertical().id_salt(("psbsscroll", border)).max_height(330.0).show(ui, |ui| {
        ui.add_enabled_ui(on, |ui| {
            if border {
                head(ui, "Stroke");
                para_sides(lang, ui, d, pv, &key("Weights"), &|ui: &mut egui::Ui, id: &str, v: Option<f64>| {
                    crate::widgets::number(ui, id, v, " pt", 60.0, 2).map(|w| if w.is_finite() { w.clamp(0.0, 1000.0) } else { 0.0 })
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
                para_color_tint(ui, d, pv, doc, (&key("Color"), &key("Tint")), (color, tint), true);
                if border {
                    // The gap colour shows between the dashes, dots or stripes.
                    let solid = para_value(d, pv, "borderType").get("kind").and_then(Value::as_str).is_none_or(|k| k == "solid");
                    let (gap, gap_tint) = (crate::i18n::tr(lang, "Gap Color:"), crate::i18n::tr(lang, "Gap Tint:"));
                    para_color_tint(ui, d, pv, doc, ("borderGapColor", "borderGapTint"), (gap, gap_tint), !solid);
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
            head(ui, "Offsets");
            para_sides(lang, ui, d, pv, &key("Offsets"), &|ui: &mut egui::Ui, id: &str, v: Option<f64>| {
                crate::widgets::measure(ui, id, v, units, 60.0)
            });
            ui.add_space(4.0);
            egui::Grid::new(("psbsedges", border)).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                crate::rtl::label(ui, crate::i18n::tr(lang, "Top Edge:"));
                para_choice(
                    lang,
                    ui,
                    d,
                    pv,
                    &key("Top"),
                    &[("ascent", "Ascent"), ("baseline", "Baseline"), ("emBox", "Em Box Top"), ("leading", "Leading")],
                );
                ui.end_row();
                crate::rtl::label(ui, crate::i18n::tr(lang, "Bottom Edge:"));
                para_choice(lang, ui, d, pv, &key("Bottom"), &[("descent", "Descent"), ("baseline", "Baseline"), ("emBox", "Em Box Bottom")]);
                ui.end_row();
                crate::rtl::label(ui, crate::i18n::tr(lang, "Width:"));
                para_choice(lang, ui, d, pv, &key("Width"), &[("column", "Column"), ("text", "Text")]);
                ui.end_row();
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
        egui::ComboBox::from_id_salt("psspankind").selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
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
                egui::ComboBox::from_id_salt("psspann").selected_text(crate::rtl::widget(ui, shown)).width(80.0).show_ui(ui, |ui| {
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
    egui::ComboBox::from_id_salt(("pslistcs", key)).selected_text(crate::rtl::widget(ui, crate::i18n::style_name(lang, &cur))).width(200.0).show_ui(
        ui,
        |ui| {
            for n in std::iter::once(designcraft_doc::NO_CHAR_STYLE)
                .chain(styles.iter().map(String::as_str).filter(|n| *n != designcraft_doc::NO_CHAR_STYLE))
            {
                if ui.selectable_label(n == cur, crate::rtl::widget(ui, crate::i18n::style_name(lang, n))).clicked() {
                    para_set(d, key, json!(n));
                }
            }
        },
    );
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
    egui::ComboBox::from_id_salt("psbulletfont").selected_text(if font.is_empty() { inherit } else { font.as_str() }).width(200.0).show_ui(
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
        egui::ComboBox::from_id_salt("psbulletstyle").selected_text(style.as_str()).width(200.0).show_ui(ui, |ui| {
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
                    egui::ComboBox::from_id_salt("pslist").selected_text(name(&list)).width(200.0).show_ui(ui, |ui| {
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
                        egui::ComboBox::from_id_salt("psmode").selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
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
                            egui::ComboBox::from_id_salt("psrestartafter").selected_text(crate::rtl::widget(ui, label(after))).width(140.0).show_ui(
                                ui,
                                |ui| {
                                    for n in 0..level {
                                        if ui.selectable_label(n == after, crate::rtl::widget(ui, label(n))).clicked() {
                                            para_set(d, "restartAfterLevel", json!(n));
                                        }
                                    }
                                },
                            );
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
    app.ui.dialog = Some(Dialog::new(id, json!({"name": name})));
}

/// New Paragraph Style… / New Character Style…: the style options for a style made on OK, named
/// with the next free "Paragraph Style n" / "Character Style n".
pub fn open_new_style(app: &mut DesignApp, para: bool) {
    let Some(st) = app.session.active() else { return };
    let name = next_style_name(&st.doc.styles, para);
    let id = if para { "paragraphStyleOptions" } else { "characterStyleOptions" };
    app.ui.dialog = Some(Dialog::new(id, json!({"new": true, "rename": name})));
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
                egui::Grid::new("csg").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&lang, "Style Name:"));
                    text_field(ui, d, "rename", 220.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&lang, "Based On:"));
                    egui::ComboBox::from_id_salt("csbased")
                        .selected_text(crate::rtl::widget(ui, crate::i18n::style_name(&lang, &based)))
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            // Not itself, nor a style based on it.
                            for s in styles.character.iter().filter(|s| new || !styles.char_based_on_cycles(&name, &s.name)) {
                                if ui.selectable_label(s.name == based, crate::rtl::widget(ui, crate::i18n::style_name(&lang, &s.name))).clicked() {
                                    d.fields.insert("basedOn".into(), json!(s.name));
                                }
                            }
                        });
                    ui.end_row();
                });
                let status = d.s("status");
                if !status.is_empty() {
                    crate::rtl::label(ui, egui::RichText::new(status).color(crate::theme::Tokens::get(ui.ctx()).text_strong));
                }
                ui.add_space(8.0);
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&lang, "Style Settings:")).font(semibold(12.0)));
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
                let mut parts = vec![crate::i18n::style_name(&lang, &based).to_string()];
                parts.extend(own.as_object().into_iter().flatten().map(|(k, v)| format!("{}: {}", setting_label(&lang, k), setting_text(v))));
                ui.add(egui::Label::new(egui::RichText::new(parts.join(" + ")).color(crate::theme::Tokens::get(ui.ctx()).text_dim)).wrap());
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
    egui::ComboBox::from_id_salt(("cs", key)).selected_text(crate::rtl::widget(ui, shown)).width(200.0).show_ui(ui, |ui| {
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
    egui::Grid::new("psc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(lang, "Font Family:"));
        let fam = cf.get(d, "fontFamily").as_str().unwrap_or("").to_string();
        let shown = if fam.is_empty() { "—".to_string() } else { crate::panels::font_label(app, &menu, &fam) };
        egui::ComboBox::from_id_salt("psfam")
            .selected_text(shown)
            .width(200.0)
            .height(440.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                if cf.sparse && ui.selectable_label(fam.is_empty(), " ").clicked() {
                    cf.set(d, "fontFamily", Value::Null);
                }
                if let Some(f) = crate::panels::font_menu_body(app, ui, &menu, &fam, 200.0) {
                    cf.set(d, "fontFamily", json!(f.family));
                    if let Some(style) = f.style {
                        cf.set(d, "fontStyle", json!(style));
                    }
                }
            });
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
            egui::ComboBox::from_id_salt("pssty").selected_text(&sty).width(200.0).show_ui(ui, |ui| {
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
        crate::rtl::label(ui, crate::i18n::tr(lang, "Size:"));
        char_number(ui, d, cf, "size", " pt", 1.0, (0.1, 1296.0));
        ui.end_row();
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
        ui.end_row();
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
        ui.end_row();
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
        egui::ComboBox::from_id_salt(("cs", "digits")).selected_text(crate::rtl::widget(ui, &shown)).width(200.0).show_ui(ui, |ui| {
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
        egui::ComboBox::from_id_salt("csfig").selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, shown))).width(180.0).show_ui(ui, |ui| {
            for (id, label, ..) in otf::FIGURES {
                if ui.selectable_label(fig == *id, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() && otf::set_figures(&mut list, id)
                {
                    changed = true;
                }
            }
        });
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
    egui::ComboBox::from_id_salt("del_char_replace")
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

fn combo(ui: &mut egui::Ui, d: &mut Dialog, key: &str, opts: &[(&str, &str)]) {
    let cur = d.s(key);
    let shown = opts.iter().find(|o| o.0 == cur).map_or(cur.as_str(), |o| o.1).to_string();
    egui::ComboBox::from_id_salt(key).selected_text(crate::rtl::widget(ui, &shown)).width(170.0).show_ui(ui, |ui| {
        for (v, label) in opts {
            if ui.selectable_label(cur == *v, crate::rtl::widget(ui, *label)).clicked() {
                d.fields.insert(key.into(), json!(v));
            }
        }
    });
}

/// Translate enum captions without changing serialized values or user-defined names.
fn translated_combo(lang: &str, ui: &mut egui::Ui, d: &mut Dialog, key: &str, opts: &[(&str, &str)]) {
    let opts: Vec<_> = opts.iter().map(|(value, label)| (*value, crate::i18n::tr(lang, label))).collect();
    combo(ui, d, key, &opts);
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
        egui::ComboBox::from_id_salt("rule_which").selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, shown))).width(130.0).show_ui(
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
    ui.add_enabled_ui(on, |ui| {
        egui::Grid::new(("rules", which)).num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
            crate::rtl::label(ui, crate::i18n::tr(lang, "Weight:"));
            if let Some(v) = sized(ui, &mut |ui| crate::widgets::number(ui, &key("weight"), rule["weight"].as_f64(), " pt", 80.0, 2)) {
                d.fields.insert(key("weight"), json!(v.clamp(0.0, 1000.0)));
            }
            crate::rtl::label(ui, crate::i18n::tr(lang, "Color:"));
            let color = rule["color"].as_str().unwrap_or("").to_string();
            if let Some(w) = app.session.active().and_then(|st| swatch_menu(ui, &st.doc, ("rule_color", which), &color, 130.0)) {
                d.fields.insert(key("color"), json!(w));
            }
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(lang, "Tint:"));
            if let Some(v) = sized(ui, &mut |ui| crate::widgets::number(ui, &key("tint"), rule["tint"].as_f64().map(|t| t * 100.0), "%", 80.0, 0)) {
                d.fields.insert(key("tint"), json!((v / 100.0).clamp(0.0, 1.0)));
            }
            crate::rtl::label(ui, crate::i18n::tr(lang, "Width:"));
            let column = rule["columnWidth"].as_bool().unwrap_or(true);
            egui::ComboBox::from_id_salt(("rule_width", which))
                .selected_text(crate::rtl::widget(ui, crate::i18n::tr(lang, if column { "Column" } else { "Text" })))
                .width(130.0)
                .show_ui(ui, |ui| {
                    for (v, label) in [(true, "Column"), (false, "Text")] {
                        if ui.selectable_label(v == column, crate::rtl::widget(ui, crate::i18n::tr(lang, label))).clicked() {
                            d.fields.insert(key("columnWidth"), json!(v));
                        }
                    }
                });
            ui.end_row();
            crate::rtl::label(ui, crate::i18n::tr(lang, "Offset:"));
            if let Some(v) = sized(ui, &mut |ui| crate::widgets::measure(ui, &key("offset"), rule["offset"].as_f64(), v_units, 80.0)) {
                d.fields.insert(key("offset"), json!(v));
            }
            ui.end_row();
            for (label, field) in [("Left Indent:", "leftIndent"), ("Right Indent:", "rightIndent")] {
                crate::rtl::label(ui, crate::i18n::tr(lang, label));
                if let Some(v) = sized(ui, &mut |ui| crate::widgets::measure(ui, &key(field), rule[field].as_f64(), h_units, 80.0)) {
                    d.fields.insert(key(field), json!(v));
                }
            }
            ui.end_row();
        });
    });
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
    s.replace('\t', "^t").replace('\u{2003}', "^m").replace('\u{2002}', "^>")
}

fn toc_separator_typed(s: &str) -> String {
    s.replace("^t", "\t").replace("^m", "\u{2003}").replace("^>", "\u{2002}")
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
    let mut p = json!({"title": d.s("title"), "entries": entries, "replace": d.b("replace")});
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
fn toc_style_combo(ui: &mut egui::Ui, salt: &str, styles: &[String], auto: &str, cur: &mut String) -> bool {
    let shown = if cur.is_empty() { auto.to_string() } else { cur.clone() };
    let mut changed = false;
    egui::ComboBox::from_id_salt(("toc_style", salt)).selected_text(shown).width(200.0).show_ui(ui, |ui| {
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
    });
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
    let mut rows = toc_rows(d);
    let mut sel = d.n("selected").map_or(0, |n| n.max(0.0) as usize);
    let mut other = d.s("other");
    let mut title_style = d.s("titleStyle");
    egui::Grid::new("toc_title").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Title:"));
        text_field(ui, d, "title", 200.0);
        crate::rtl::label(ui, tr("Style:"));
        toc_style_combo(ui, "title", &styles, "TOC Title", &mut title_style);
        ui.end_row();
    });
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Styles in Table of Contents")).font(semibold(12.0)));
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
            crate::rtl::label(ui, tr("Entry Style:"));
            let auto = format!("TOC Level {}", r.level.clamp(1, 9));
            toc_style_combo(ui, "entry", &styles, &auto, &mut r.entry_style);
            ui.end_row();
            crate::rtl::label(ui, tr("Page Number:"));
            let choices = [("after", "After Entry"), ("before", "Before Entry"), ("none", "No Page Number")];
            let cur = choices.iter().find(|(v, _)| *v == r.page_number).map_or("After Entry", |(_, l)| l);
            egui::ComboBox::from_id_salt("toc_page_number").selected_text(tr(cur)).width(200.0).show_ui(ui, |ui| {
                for (v, l) in choices {
                    if ui.selectable_label(r.page_number == v, tr(l)).clicked() {
                        r.page_number = v.into();
                    }
                }
            });
            crate::rtl::label(ui, tr("Style:"));
            toc_style_combo(ui, "page_number", &char_styles, none, &mut r.page_number_style);
            ui.end_row();
            crate::rtl::label(ui, tr("Between Entry and Number:"));
            ui.add(egui::TextEdit::singleline(&mut r.separator).desired_width(80.0).horizontal_align(egui::Align::Min));
            crate::rtl::label(ui, tr("Style:"));
            toc_style_combo(ui, "separator", &char_styles, none, &mut r.separator_style);
            ui.end_row();
            crate::rtl::label(ui, "");
            ui.checkbox(&mut r.sort, tr("Sort Entries in Alphabetical Order"));
            ui.end_row();
            crate::rtl::label(ui, tr("Level:"));
            egui::ComboBox::from_id_salt("toc_level").selected_text(r.level.to_string()).width(60.0).show_ui(ui, |ui| {
                for l in 1..=9u8 {
                    if ui.selectable_label(r.level == l, l.to_string()).clicked() {
                        r.level = l;
                    }
                }
            });
            ui.end_row();
        });
    }
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Options")).font(semibold(12.0)));
    let has_toc = d.b("hasToc");
    ui.add_enabled_ui(has_toc, |ui| check(ui, d, "replace", &tr("Replace Existing Table of Contents")));
    if let Some(st) = d.fields.get("status").and_then(Value::as_str) {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(st).color(crate::theme::Tokens::get(ui.ctx()).text_dim));
    }
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

/// What changed the Color Picker's colour this frame.
#[derive(Clone, Copy, PartialEq)]
enum ColorEdit {
    None,
    /// The saturation/brightness field, the hue slider or the hex field: keeps the colour's model.
    Picked,
    /// Typed R/G/B values: the colour becomes RGB.
    Rgb,
    /// Typed C/M/Y/K values: the colour becomes CMYK.
    Cmyk,
}

/// Color Picker: saturation/brightness field and hue slider, with R/G/B, C/M/Y/K and hex fields.
///
/// `rgb` (0-255) and `cmyk` (percent) hold the values as typed, so a field being edited is never
/// rebuilt from the colour; the hex field keeps its text in `hexEdit` while it has focus and shows
/// the colour again when focus leaves. `model` is what OK applies: picking keeps a CMYK colour CMYK
/// (converted through the CMYK working space); typing R/G/B or C/M/Y/K values chooses that model.
fn color_picker(ui: &mut egui::Ui, d: &mut Dialog) {
    use designcraft_color::Color;
    let to8 = |v: f64| v.clamp(0.0, 255.0).round() as u8;
    let display_rgb = |c: Color| c.to_rgb().map(|v| (f64::from(v) * 255.0).clamp(0.0, 255.0).round());
    let cmyk_of = |rgb: [f64; 3]| {
        let c = Color::rgb((rgb[0] / 255.0) as f32, (rgb[1] / 255.0) as f32, (rgb[2] / 255.0) as f32);
        c.to_cmyk_managed(designcraft_color::cms::active_settings().intent).map(|v| f64::from(v) * 100.0)
    };
    let hex_rgb = |s: &str| match Color::from_hex(s) {
        Some(Color::Rgb { r, g, b }) => Some([r, g, b].map(|v| (f64::from(v) * 255.0).round())),
        _ => None,
    };
    let mut rgb = nums::<3>(d, "rgb").unwrap_or_else(|| hex_rgb(&d.s("hex")).unwrap_or([0.0; 3]));
    let mut cmyk = nums::<4>(d, "cmyk").unwrap_or_else(|| cmyk_of(rgb));
    let mut model = if d.s("model") == "cmyk" || (!d.fields.contains_key("model") && d.fields.contains_key("cmyk")) { "cmyk" } else { "rgb" };
    let mut edit = ColorEdit::None;
    let hex_id = egui::Id::new("color_picker_hex");
    ui.horizontal_top(|ui| {
        let mut c = egui::Color32::from_rgb(to8(rgb[0]), to8(rgb[1]), to8(rgb[2]));
        let shown = c;
        if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) && c != shown {
            rgb = [c.r(), c.g(), c.b()].map(f64::from);
            edit = ColorEdit::Picked;
        }
        ui.vertical(|ui| {
            egui::Grid::new("cp_values").num_columns(4).spacing([6.0, 4.0]).show(ui, |ui| {
                let mut rgb_rows = rgb.iter_mut().zip(["R", "G", "B"]);
                for (v, l) in cmyk.iter_mut().zip(["C", "M", "Y", "K"]) {
                    if let Some((rv, rl)) = rgb_rows.next() {
                        ui.label(rl);
                        if ui.add(egui::DragValue::new(rv).range(0.0..=255.0).max_decimals(1)).changed() {
                            edit = ColorEdit::Rgb;
                        }
                    } else {
                        ui.label("");
                        ui.label("");
                    }
                    ui.label(l);
                    if ui.add(egui::DragValue::new(v).range(0.0..=100.0).max_decimals(1).suffix("%")).changed() {
                        edit = ColorEdit::Cmyk;
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("#");
                let editing = ui.memory(|m| m.has_focus(hex_id));
                let mut h = if editing { d.s("hexEdit") } else { format!("{:02x}{:02x}{:02x}", to8(rgb[0]), to8(rgb[1]), to8(rgb[2])) };
                if ui.add(egui::TextEdit::singleline(&mut h).id(hex_id).desired_width(70.0)).changed()
                    && let Some(v) = hex_rgb(&h)
                {
                    rgb = v;
                    edit = ColorEdit::Picked;
                }
                d.fields.insert("hexEdit".into(), json!(h));
            });
        });
    });
    match edit {
        ColorEdit::None => {}
        ColorEdit::Picked => cmyk = cmyk_of(rgb),
        ColorEdit::Rgb => {
            model = "rgb";
            cmyk = cmyk_of(rgb);
        }
        ColorEdit::Cmyk => {
            model = "cmyk";
            let [c, m, y, k] = cmyk.map(|v| (v / 100.0) as f32);
            rgb = display_rgb(Color::cmyk(c, m, y, k));
        }
    }
    if edit != ColorEdit::None {
        d.fields.insert("edited".into(), json!(true));
    }
    d.fields.insert("rgb".into(), json!(rgb));
    d.fields.insert("cmyk".into(), json!(cmyk));
    d.fields.insert("model".into(), json!(model));
    d.fields.insert("hex".into(), json!(format!("#{:02x}{:02x}{:02x}", to8(rgb[0]), to8(rgb[1]), to8(rgb[2]))));
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
mod tests {
    use super::*;

    #[test]
    fn paragraph_style_settings_show_captured_color_and_pending_spacing_without_changing_the_document() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"Heading"})).unwrap();
        app.run("text.select", json!({"story":f["story"],"anchor":0,"focus":7})).unwrap();
        app.run("type.char", json!({"fontFamily":"Source Sans 3","fontStyle":"Bold","size":16,"fill":"[Paper]"})).unwrap();
        app.run("style.paragraph.create", json!({"name":"Header","fromSelection":true})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        let style = doc.styles.para("Header").unwrap();
        let mut d = Dialog::new("paragraphStyleOptions", json!({"name":"Header"}));
        let summary = paragraph_style_summary("", &d, style, &doc.styles);
        for value in ["Source Sans 3", "Bold", "16 pt", "[Paper]"] {
            assert!(summary.contains(value), "{summary}");
        }
        assert!(summary.len() < 250, "default attributes do not swamp the captured settings: {summary}");
        d.fields.insert("c.fill".into(), json!("[Registration]"));
        d.fields.insert("p.spaceAfter".into(), json!(4.5));
        let summary = paragraph_style_summary("", &d, style, &doc.styles);
        assert!(summary.contains("[Registration]") && !summary.contains("[Paper]"), "{summary}");
        assert!(summary.contains("4.5 pt"), "{summary}");
        assert_eq!(app.session.doc().unwrap().doc, doc);
    }

    #[test]
    fn paragraph_style_settings_compare_with_the_chosen_parent() {
        let mut styles = designcraft_doc::Styles::default();
        styles.paragraph.push(designcraft_doc::ParagraphStyle {
            name: "Parent".into(),
            chars: designcraft_doc::CharAttrs { size: Some(20.0), ..Default::default() },
            based_on: None,
            next_style: None,
            para: Default::default(),
            shortcut: String::new(),
        });
        let style = designcraft_doc::ParagraphStyle {
            name: "Child".into(),
            based_on: Some("Parent".into()),
            next_style: None,
            para: Default::default(),
            chars: Default::default(),
            shortcut: String::new(),
        };
        let d = Dialog::new("paragraphStyleOptions", json!({"name":"Child"}));
        assert_eq!(paragraph_style_summary("", &d, &style, &styles), "Parent");
    }

    #[test]
    fn step_and_repeat_dialog_previews_and_makes_the_copies_as_one_undo_step() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [72, 600, 540, 640]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        let open = |app: &mut DesignApp| {
            crate::menus::activate(app, "edit.stepAndRepeat", &Value::Null);
            assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(STEP_AND_REPEAT));
        };
        let set = |app: &mut DesignApp, k: &str, v: Value| {
            app.ui.dialog.as_mut().unwrap().fields.insert(k.into(), v);
        };
        let items = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].items.len();
        let undo = |app: &DesignApp| app.session.doc().unwrap().history.undo.len();
        let (n0, u0) = (items(&app), undo(&app));
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        open(&mut app);
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for expected in ["Step and Repeat", "Repeat", "Count:", "Create as a grid", "Offset", "Vertical:", "Horizontal:", "Preview", "OK", "Cancel"] {
            visible_label(&labels, screen, expected);
        }
        // Preview: the copies show, and change with the fields, without an undo step.
        set(&mut app, "count", json!("5"));
        set(&mut app, "vertical", json!("-0.75 in"));
        set(&mut app, "horizontal", json!("0"));
        set(&mut app, "preview", json!(true));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        assert_eq!((items(&app), undo(&app)), (n0 + 5, u0));
        set(&mut app, "count", json!("3"));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        assert_eq!(items(&app), n0 + 3);
        set(&mut app, "preview", json!(false));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        assert_eq!(items(&app), n0, "Preview off takes them away");
        set(&mut app, "preview", json!(true));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        assert_eq!(items(&app), n0 + 3);
        // Cancel puts the document back.
        cancel(&mut app);
        assert!(app.ui.dialog.is_none());
        assert_eq!((items(&app), undo(&app)), (n0, u0));
        // OK after a preview makes the copies once, as one undo step, 0.75 in apart.
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        open(&mut app);
        set(&mut app, "count", json!("2"));
        set(&mut app, "vertical", json!("-0.75 in"));
        set(&mut app, "horizontal", json!("0"));
        set(&mut app, "preview", json!(true));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let r = confirm(&mut app).unwrap();
        assert_eq!((items(&app), undo(&app)), (n0 + 2, u0 + 1));
        let last = r["ids"][1].as_u64().unwrap();
        let top = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(last)).unwrap().bounds().min_y();
        assert!((top - (600.0 - 108.0)).abs() < 1e-6, "{top}");
        // Create as a grid: Rows × Columns cells, the original being one.
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        open(&mut app);
        set(&mut app, "grid", json!(true));
        set(&mut app, "rows", json!("2"));
        set(&mut app, "columns", json!("3"));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        visible_label(&labels, screen, "Rows:");
        visible_label(&labels, screen, "Columns:");
        assert_eq!(confirm(&mut app).unwrap()["created"], 5);
        // A value that isn't a measure keeps the dialog open and says so.
        open(&mut app);
        set(&mut app, "vertical", json!("up"));
        assert!(confirm(&mut app).is_err());
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.s("status"), "Invalid value: up");
    }

    /// A three-page document in `units` with Document Setup open.
    fn document_setup_app(units: &str) -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 3})).unwrap();
        app.run("document.preferences", json!({"horizontalUnits": units, "verticalUnits": units})).unwrap();
        crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
        app
    }

    fn setup_fields(app: &mut DesignApp, fields: Value) {
        let d = app.ui.dialog.as_mut().unwrap();
        for (k, v) in fields.as_object().unwrap() {
            d.fields.insert(k.clone(), v.clone());
        }
    }

    #[test]
    fn document_setup_ok_applies_margins_bleed_and_size_in_one_step() {
        let mut app = document_setup_app("millimeters");
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.id, "documentSetup");
        assert_eq!((d.s("marginTop"), d.b("marginLinked")), ("12.7 mm".to_string(), true), "the document's margins, in its units");
        setup_fields(
            &mut app,
            json!({"marginTop": "20 mm", "marginBottom": "20 mm", "marginInside": "20 mm", "marginOutside": "20 mm", "bleedTop": "3 mm",
            "bleedBottom": "3 mm", "bleedInside": "3 mm", "bleedOutside": "3 mm", "width": "210 mm", "height": "297 mm"}),
        );
        confirm(&mut app).unwrap();
        assert!(app.ui.dialog.is_none());
        let mm = designcraft_geom::units::PT_PER_MM;
        let doc = &app.session.active().unwrap().doc;
        assert!((doc.settings.page_width - 210.0 * mm).abs() < 1e-6 && (doc.settings.bleed[2] - 3.0 * mm).abs() < 1e-6);
        assert!(doc.spreads.iter().flat_map(|s| &s.pages).all(|p| (p.margins.top - 20.0 * mm).abs() < 1e-6), "every page");
        app.run("edit.undo", json!({})).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert!((doc.settings.page_width - 612.0).abs() < 1e-6 && doc.settings.bleed[0] == 0.0, "one undo step");
    }

    #[test]
    fn document_setup_reads_a_bare_number_in_the_documents_units() {
        let mut app = document_setup_app("millimeters");
        setup_fields(&mut app, json!({"bleedTop": "5", "bleedBottom": "5", "bleedInside": "5", "bleedOutside": "5", "width": "210"}));
        confirm(&mut app).unwrap();
        let mm = designcraft_geom::units::PT_PER_MM;
        let settings = &app.session.active().unwrap().doc.settings;
        assert!(settings.bleed.iter().all(|b| (b - 5.0 * mm).abs() < 1e-6), "5 mm, not 5 picas: {:?}", settings.bleed);
        assert!((settings.page_width - 210.0 * mm).abs() < 1e-6, "{}", settings.page_width);
    }

    #[test]
    fn document_setup_keeps_each_pages_margins_unless_they_are_edited() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 3, "margins": 36})).unwrap();
        app.run("layout.marginsAndColumns", json!({"pages": [1], "margins": 90})).unwrap();
        let margins =
            |app: &DesignApp| app.session.active().unwrap().doc.spreads.iter().flat_map(|s| &s.pages).map(|p| p.margins.top).collect::<Vec<_>>();
        // Only the page count changes: every page keeps its own margins.
        crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
        setup_fields(&mut app, json!({"pages": "4"}));
        confirm(&mut app).unwrap();
        assert_eq!(margins(&app), [36.0, 90.0, 36.0, 36.0]);
        // Edited margins go to every page.
        crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
        setup_fields(&mut app, json!({"marginTop": "1p0"}));
        confirm(&mut app).unwrap();
        assert_eq!(margins(&app), [12.0; 4]);
    }

    #[test]
    fn document_setup_keeps_a_value_it_cannot_read_and_changes_nothing() {
        for (key, value) in [("width", "wide"), ("marginTop", "-"), ("pages", "0"), ("bleedTop", "x")] {
            let mut app = document_setup_app("points");
            setup_fields(&mut app, json!({key: value, "bleedInside": "9"}));
            assert!(confirm(&mut app).is_err(), "{key}");
            assert!(app.ui.dialog.is_some(), "{key}: the dialog stays open");
            assert_eq!(app.session.active().unwrap().doc.settings.bleed, [0.0; 4], "{key}: nothing changed");
        }
    }

    #[test]
    fn document_setup_links_edges_and_adjust_layout_applies() {
        let mut app = document_setup_app("points");
        let f = app.run("frame.create", json!({"rect": [36, 36, 576, 756], "content": "text"})).unwrap();
        crate::menus::activate(&mut app, "layout.documentSetup", &Value::Null);
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 900.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        // Typing in a linked margin sets all four.
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let top = labels.iter().filter(|(t, _, _)| t == "36 pt").map(|(_, r, _)| *r).next().expect("the top margin");
        click_dialog(&mut app, &ctx, screen, top.center());
        let select_all =
            egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
        dialog_frame(&mut app, &ctx, screen, vec![select_all, egui::Event::Text("72 pt".into())]);
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(["Top", "Bottom", "Inside", "Outside"].iter().all(|e| d.s(&format!("margin{e}")) == "72 pt"), "linked: {:?}", d.fields);
        // Adjust Layout applies at once, and the frame follows the margins.
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Adjust Layout").center());
        assert!(app.ui.dialog.is_none(), "applied");
        let doc = &app.session.active().unwrap().doc;
        let b = doc.item(designcraft_doc::ItemId(f["id"].as_u64().unwrap())).unwrap().bounds();
        assert!((b.y0 - 72.0).abs() < 1e-6 && doc.page(0).unwrap().margins.outside == 72.0, "{b:?}");
    }

    #[test]
    fn cell_options_dialog_draws_and_applies_only_changed_values_to_the_chosen_edges() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
        let tid = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        app.run("table.select", json!({"what": "table"})).unwrap();
        app.run("app.cellOptionsDialog", json!({})).unwrap();
        let ctx = dialog_context("");
        dialog_frame(&mut app, &ctx, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0)), vec![]);
        let d = app.ui.dialog.as_mut().unwrap();
        assert_eq!(d.id, "cellOptions");
        assert_eq!(d.fields["weight"], json!(1.0));
        d.fields.insert("edges".into(), json!(["top", "bottom", "left", "right"]));
        d.fields.insert("gapColor".into(), json!("[Paper]"));
        d.fields.insert("fill".into(), json!("[Black]"));
        d.fields.insert("fillTint".into(), json!(0.2));
        confirm(&mut app).unwrap();
        let st = app.session.active().unwrap();
        let t = st.doc.stories.values().find_map(|s| s.tables.get(&tid)).unwrap();
        let gap = |r: usize, c: usize, side: usize| t.cell(r, c).unwrap().strokes[side].gap_color.clone();
        assert_eq!([gap(0, 0, 0), gap(0, 0, 1), gap(1, 1, 2), gap(1, 1, 3)], ["[Paper]"; 4].map(String::from), "outer edges");
        assert_eq!([gap(0, 0, 2), gap(0, 0, 3)], ["[None]"; 2].map(String::from), "inner edges keep theirs");
        assert!(t.cells.iter().all(|c| c.fill == "[Black]" && (c.fill_tint - 0.2).abs() < 1e-6));
        assert!(t.cells.iter().all(|c| c.strokes.iter().all(|e| e.weight == 1.0)), "unchanged values are not applied");
    }

    #[test]
    fn cell_options_edge_changes_reseed_every_stroke_field_and_survive_a_bad_seed() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
        let tid = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        app.run("table.select", json!({"what": "table"})).unwrap();
        app.run("table.setCell", json!({"stroke": {"weight": 3, "edges": "innerHorizontal"}})).unwrap();
        app.run("app.cellOptionsDialog", json!({})).unwrap();
        let mut d = app.ui.dialog.take().unwrap();
        d.fields.insert("seed".into(), json!(5));
        let outer = [true, true, true, true, false, false];
        cell_options_edges_changed(&mut app, &mut d, &outer);
        assert_eq!((&d.fields["weight"], &d.fields["seed"]["weight"]), (&json!(1.0), &json!(1.0)));
        // A weight typed for the outer edges, then the inner horizontal edges (3 pt) chosen
        // and 1 pt typed: 1 pt differs from what those edges show, so OK applies it.
        d.fields.insert("weight".into(), json!(5.0));
        cell_options_edges_changed(&mut app, &mut d, &[false, false, false, false, true, false]);
        assert_eq!((&d.fields["weight"], &d.fields["seed"]["weight"]), (&json!(5.0), &json!(3.0)));
        d.fields.insert("weight".into(), json!(1.0));
        app.ui.dialog = Some(d);
        confirm(&mut app).unwrap();
        let st = app.session.active().unwrap();
        let t = st.doc.stories.values().find_map(|s| s.tables.get(&tid)).unwrap();
        assert_eq!([t.cell(0, 0).unwrap().strokes[2].weight, t.cell(1, 1).unwrap().strokes[0].weight], [1.0; 2]);
    }

    #[test]
    fn preferences_ok_saves_a_story_editor_size_that_loads_back() {
        // `ui.dialog.set` takes any value; a size that isn't finite is saved as `null`.
        let sizes = [(json!("nan"), 20.0), (json!("inf"), 20.0), (json!(null), 20.0), (json!(1e39), 36.0), (json!(3), 8.0), (json!(11), 11.0)];
        for (size, want) in sizes {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            app.ui.story_editor_size = 20.0;
            app.run("app.preferences", json!({})).unwrap();
            app.ui.dialog.as_mut().unwrap().fields.insert("storyEditorSize".into(), size);
            confirm(&mut app).unwrap();
            let saved = serde_json::to_vec(&app.ui).unwrap();
            assert_eq!(crate::UiState::from_preferences_json(&saved).unwrap().story_editor_size, want);
        }
    }

    #[test]
    fn preferences_ok_sets_only_the_ruler_units_it_is_given() {
        // `ui.dialog.open` takes any fields, so the vertical units can be missing.
        for (vertical, want) in [(None, "inches"), (Some("points"), "points")] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            app.run("document.preferences", json!({"verticalUnits": "inches"})).unwrap();
            let mut fields = json!({"horizontalUnits": "millimeters", "bg.relativeTo": "topOfPage"});
            if let Some(vertical) = vertical {
                fields["verticalUnits"] = json!(vertical);
            }
            app.ui.dialog = Some(Dialog::new("preferences", fields));
            confirm(&mut app).unwrap();
            let units = app.run("document.preferences", json!({})).unwrap();
            assert_eq!((units["horizontalUnits"].as_str(), units["verticalUnits"].as_str()), (Some("millimeters"), Some(want)));
        }
    }

    #[test]
    fn enter_and_escape_close_the_style_menu_not_numbering_and_section_options() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 2})).unwrap();
        crate::menus::activate(&mut app, "layout.section", &Value::Null);
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        for pressed in [egui::Key::Enter, egui::Key::Escape] {
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            // The section's Style menu (the first of the two).
            let style = labels.iter().filter(|(t, r, c)| t.trim() == "1, 2, 3, 4…" && c.contains_rect(*r)).map(|(_, r, _)| *r).next().unwrap();
            click_dialog(&mut app, &ctx, screen, style.center());
            dialog_frame(&mut app, &ctx, screen, vec![]);
            assert!(egui::Popup::is_any_open(&ctx), "the Style menu opened");
            dialog_frame(&mut app, &ctx, screen, vec![key(pressed)]);
            dialog_frame(&mut app, &ctx, screen, vec![]);
            assert!(app.ui.dialog.is_some(), "{pressed:?} with the menu open leaves the dialog open");
            assert!(app.session.active().unwrap().doc.sections.iter().all(|s| s.start_number != Some(0)));
            if egui::Popup::is_any_open(&ctx) {
                egui::Popup::close_all(&ctx);
                dialog_frame(&mut app, &ctx, screen, vec![]);
            }
        }
        // With no menu open, Escape closes the dialog.
        dialog_frame(&mut app, &ctx, screen, vec![key(egui::Key::Escape)]);
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn numbering_and_section_options_keep_their_actions_in_view() {
        for language in ["en", "ar"] {
            for size in [egui::vec2(1178.0, 814.0), egui::vec2(392.0, 271.0)] {
                let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
                app.ui.language = language.into();
                app.run("file.new", json!({"pages": 2})).unwrap();
                crate::menus::activate(&mut app, "layout.section", &Value::Null);
                let ctx = dialog_context(language);
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for _ in 0..3 {
                    dialog_frame(&mut app, &ctx, screen, vec![]);
                }
                let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
                let tr = |s| crate::i18n::tr(language, s);
                for expected in [tr("Numbering & Section Options…").trim_end_matches('…'), tr("OK"), tr("Cancel"), tr("Start Section")] {
                    visible_label(&labels, screen, expected);
                }
                if size.x > 1000.0 {
                    // Every option fits without scrolling, and choosing one then OK applies it.
                    visible_label(&labels, screen, &format!("{} {}", tr("Book Name:"), tr("N/A")));
                    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, tr("Automatic Page Numbering")).center());
                    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
                    click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, tr("OK")).center());
                    assert!(app.ui.dialog.is_none(), "{language}: OK closes the dialog");
                    let doc = &app.session.active().unwrap().doc;
                    assert!(doc.sections.iter().all(|s| s.start != 0 || s.start_number.is_none()), "{language}: page 1 numbers automatically");
                }
            }
        }
    }

    fn select_all_key() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { ctrl: true, command: true, ..Default::default() },
        }
    }

    /// The Color Picker over a new document's selected frame, a few frames in.
    fn picker(fields: Value) -> (DesignApp, egui::Context, egui::Rect, u64) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("selection.set", json!({"ids": [id]})).unwrap();
        app.ui.dialog = Some(Dialog::new("colorPicker", fields));
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 500.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        (app, ctx, screen, id)
    }

    /// The rightmost painted `text`: the picker's own field, right of egui's colour editor (which
    /// has R/G/B fields of its own).
    fn rightmost(labels: &[(String, egui::Rect, egui::Rect)], text: &str) -> egui::Pos2 {
        labels
            .iter()
            .filter(|(t, _, _)| t.trim() == text)
            .map(|(_, r, _)| r.center())
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap_or_else(|| panic!("{text} is painted"))
    }

    /// Types `text` one character per frame, with an idle frame after each.
    fn type_slowly(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, text: &str) {
        for ch in text.chars() {
            dialog_frame(app, ctx, screen, vec![egui::Event::Text(ch.into())]);
            dialog_frame(app, ctx, screen, vec![]);
        }
    }

    fn fill(app: &DesignApp, id: u64) -> designcraft_color::Color {
        let d = &app.session.active().unwrap().doc;
        d.resolve_color(&d.item(designcraft_doc::ItemId(id)).unwrap().fill.swatch, 1.0).unwrap()
    }

    #[test]
    fn the_hex_field_keeps_typed_text_and_commits_a_valid_code() {
        let (mut app, ctx, screen, _) = picker(json!({"target": "fill", "hex": "#ffffff"}));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "ffffff").center());
        dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
        type_slowly(&mut app, &ctx, screen, "#f5");
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.s("hexEdit").as_str(), d.s("hex").as_str()), ("#f5", "#ffffff"), "half-typed text stays; the colour waits");
        type_slowly(&mut app, &ctx, screen, "9019");
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.s("hexEdit").as_str(), d.s("hex").as_str()), ("#f59019", "#f59019"), "a 7-character #rrggbb commits");
        // Invalid text is replaced by the colour when focus leaves.
        dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
        type_slowly(&mut app, &ctx, screen, "zz");
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "#").center());
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        visible_label(&labels, screen, "f59019");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("hex"), "#f59019");
    }

    #[test]
    fn typed_channel_values_are_kept_while_typing() {
        let (mut app, ctx, screen, id) = picker(json!({"target": "fill", "hex": "#010203", "model": "rgb", "rgb": [1, 2, 3]}));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, rightmost(&labels, "1"));
        dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
        type_slowly(&mut app, &ctx, screen, "12.5");
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        assert!(rightmost(&labels, "12.5").x > rightmost(&labels, "2").x - 1.0, "the typed text stays in the R field");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("hex"), "#0d0203");
        confirm(&mut app).unwrap();
        let designcraft_color::Color::Rgb { r, .. } = fill(&app, id) else { panic!("not RGB") };
        assert!((r - 12.5 / 255.0).abs() < 1e-6, "{r}");
    }

    #[test]
    fn a_cmyk_colour_stays_cmyk_unless_rgb_values_are_typed() {
        let cmyk = json!({"target": "fill", "hex": "#ff8000", "model": "cmyk", "cmyk": [0, 50, 100, 0]});
        // Picked by hex: converted to CMYK.
        let (mut app, ctx, screen, id) = picker(cmyk.clone());
        let hex = app.ui.dialog.as_ref().unwrap().s("hex");
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, hex.trim_start_matches('#')).center());
        dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
        type_slowly(&mut app, &ctx, screen, "3366cc");
        confirm(&mut app).unwrap();
        assert!(matches!(fill(&app, id), designcraft_color::Color::Cmyk { .. }), "{:?}", fill(&app, id));
        // A typed C/M/Y/K percent is a percent: M 1 is 1 %.
        let (mut app, ctx, screen, id) = picker(cmyk.clone());
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, rightmost(&labels, "50"));
        dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
        type_slowly(&mut app, &ctx, screen, "1");
        confirm(&mut app).unwrap();
        let designcraft_color::Color::Cmyk { c, m, y, k } = fill(&app, id) else { panic!("not CMYK") };
        assert!(c == 0.0 && (m - 0.01).abs() < 1e-6 && (y - 1.0).abs() < 1e-6 && k == 0.0, "{c} {m} {y} {k}");
        // Typed R/G/B values: RGB.
        let (mut app, ctx, screen, id) = picker(cmyk);
        let r = app.ui.dialog.as_ref().unwrap().fields["rgb"][0].as_f64().unwrap();
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, rightmost(&labels, &r.to_string()));
        dialog_frame(&mut app, &ctx, screen, vec![select_all_key()]);
        type_slowly(&mut app, &ctx, screen, "10");
        confirm(&mut app).unwrap();
        assert!(matches!(fill(&app, id), designcraft_color::Color::Rgb { .. }), "{:?}", fill(&app, id));
    }

    #[test]
    fn ok_without_edits_leaves_the_fill_alone() {
        let (mut app, _, _, id) = picker(json!({"target": "fill", "hex": "#ff8000", "model": "cmyk", "cmyk": [0, 50, 100, 0]}));
        let before = app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill.clone();
        confirm(&mut app).unwrap();
        assert_eq!(app.session.active().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().fill, before);
    }

    #[test]
    fn dialogs_do_not_dim_the_page() {
        let (mut app, ctx, screen, _) = picker(json!({"target": "fill", "hex": "#ff8000"}));
        let time = ctx.input(|i| i.time) + 1.0 / 60.0;
        let output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), ..Default::default() }, |ui| show(&mut app, ui.ctx()));
        fn covers(shape: &egui::Shape, screen: egui::Rect) -> bool {
            match shape {
                egui::Shape::Rect(r) => r.rect.contains_rect(screen) && r.fill.a() > 0,
                egui::Shape::Vec(v) => v.iter().any(|s| covers(s, screen)),
                _ => false,
            }
        }
        assert!(app.ui.dialog.is_some());
        assert!(!output.shapes.iter().any(|s| covers(&s.shape, screen)), "nothing paints over the whole window");
    }

    fn draw(app: &mut DesignApp, ctx: &egui::Context) {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }

    /// A paragraph styled "Ruled Child", whose Rule Below comes from its parent "Ruled".
    fn ruled_paragraph() -> (DesignApp, Value) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let rule = json!({"on": true, "weight": 3.0, "color": "[Black]", "tint": 0.5, "columnWidth": false, "offset": 4.0, "leftIndent": 6.0, "rightIndent": 2.0});
        app.run("style.paragraph.create", json!({"name": "Ruled", "para": {"ruleBelow": rule}})).unwrap();
        app.run("style.paragraph.create", json!({"name": "Ruled Child", "basedOn": "Ruled"})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 400], "content": "text", "text": "One"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        app.run("style.paragraph.apply", json!({"name": "Ruled Child"})).unwrap();
        (app, rule)
    }

    fn undo_steps(app: &DesignApp) -> usize {
        app.session.doc().map_or(0, |st| st.history.undo.len())
    }

    #[test]
    fn paragraph_rules_dialog_shows_the_rule_and_applies_one_field() {
        let (mut app, rule) = ruled_paragraph();
        let ctx = egui::Context::default();
        app.run("app.paragraphRulesDialog", json!({"rule": "ruleBelow"})).unwrap();
        draw(&mut app, &ctx);
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!(d.id, "paragraphRules");
        let current = d.fields["current"].clone();
        assert_eq!(shown_rule(&d, "", "ruleBelow", &current), rule, "the inherited rule");
        assert_eq!(shown_rule(&d, "", "ruleAbove", &current)["on"], false);
        // Pick a colour, as the Color menu does.
        app.ui.dialog.as_mut().unwrap().fields.insert("ruleBelow.color".into(), json!("[Registration]"));
        draw(&mut app, &ctx);
        let before = undo_steps(&app);
        confirm(&mut app).unwrap();
        assert_eq!(undo_steps(&app), before + 1, "one undo step");
        let mut want = rule.clone();
        want["color"] = json!("[Registration]");
        assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["ruleBelow"], want);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["ruleBelow"], rule);
    }

    #[test]
    fn paragraph_style_options_edit_one_rule_field() {
        let (mut app, rule) = ruled_paragraph();
        let ctx = egui::Context::default();
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "rules", "rule": "ruleBelow"})));
        draw(&mut app, &ctx);
        let resolved = |app: &DesignApp| serde_json::to_value(app.session.doc().unwrap().doc.styles.resolve_para_style("Ruled Child").0).unwrap();
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!(shown_rule(&d, "p.", "ruleBelow", &resolved(&app)), rule, "inherited from the parent style");
        app.ui.dialog.as_mut().unwrap().fields.insert("p.ruleBelow.weight".into(), json!(1.5));
        draw(&mut app, &ctx);
        let before = undo_steps(&app);
        confirm(&mut app).unwrap();
        assert_eq!(undo_steps(&app), before + 1, "one undo step");
        let mut want = rule.clone();
        want["weight"] = json!(1.5);
        assert_eq!(resolved(&app)["ruleBelow"], want);
        assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["para"]["ruleBelow"], want, "the paragraph follows its style");
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(resolved(&app)["ruleBelow"], rule);
    }

    /// Opens Paragraph Style Options on "Ruled Child" at each of `sections`, sets `fields` and
    /// clicks OK; returns the resolved attributes of the paragraph styled with it.
    fn style_section_edit(sections: &[&str], fields: Value) -> Value {
        let (mut app, _) = ruled_paragraph();
        let ctx = egui::Context::default();
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child"})));
        for section in sections {
            app.ui.dialog.as_mut().unwrap().fields.insert("section".into(), json!(section));
            draw(&mut app, &ctx);
        }
        for (k, v) in fields.as_object().unwrap() {
            app.ui.dialog.as_mut().unwrap().fields.insert(k.clone(), v.clone());
        }
        draw(&mut app, &ctx);
        let before = undo_steps(&app);
        confirm(&mut app).unwrap();
        assert_eq!(undo_steps(&app), before + 1, "one undo step");
        let style = app.session.doc().unwrap().doc.styles.para("Ruled Child").cloned().unwrap();
        let own = serde_json::to_value(&style.para).unwrap();
        for (k, v) in fields.as_object().unwrap() {
            assert_eq!(own[k.trim_start_matches("p.")], *v, "the style sets {k}");
        }
        app.run("type.selectionAttrs", json!({})).unwrap()["para"].clone()
    }

    #[test]
    fn paragraph_style_options_keep_and_border_sections_reach_the_styled_paragraph() {
        let para = style_section_edit(
            &["keep", "border"],
            json!({"p.keepWithNext": 2, "p.keepLinesTogether": true, "p.keepAllLines": false, "p.keepFirst": 3,
                "p.startParagraph": "nextPage", "p.borderOn": true, "p.borderWeights": [2.0, 0.0, 2.0, 0.0]}),
        );
        assert_eq!(para["keepWithNext"], 2);
        assert_eq!(para["keepLinesTogether"], true);
        assert_eq!(para["keepAllLines"], false);
        assert_eq!(para["keepFirst"], 3);
        assert_eq!(para["startParagraph"], "nextPage");
        assert_eq!(para["borderOn"], true);
        assert_eq!(para["borderWeights"], json!([2.0, 0.0, 2.0, 0.0]));
        // Fields not edited keep the resolved value.
        assert_eq!(para["keepLast"], 2);
        assert_eq!(para["ruleBelow"]["weight"], 3.0);
    }

    #[test]
    fn paragraph_style_options_bullets_and_span_sections_reach_the_styled_paragraph() {
        let para = style_section_edit(
            &["bullets", "span"],
            json!({"p.listType": "numbers", "p.numberStyle": "upperRoman", "p.numberExpression": "^#)^t", "p.startAt": 3,
                "p.spanColumns": {"kind": "split", "columns": 3}}),
        );
        assert_eq!(para["listType"], "numbers");
        assert_eq!(para["numberStyle"], "upperRoman");
        assert_eq!(para["numberExpression"], "^#)^t");
        assert_eq!(para["startAt"], 3);
        assert_eq!(para["spanColumns"], json!({"kind": "split", "columns": 3}));
        let props: designcraft_doc::ParaProps = serde_json::from_value(para).unwrap();
        assert_eq!(props.number_label(3), "III)\t");
    }

    #[test]
    fn paragraph_style_options_keep_a_bad_grep_pattern_open_with_the_error() {
        let (mut app, _) = ruled_paragraph();
        let ctx = egui::Context::default();
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled", "section": "general"})));
        draw(&mut app, &ctx);
        app.ui.dialog.as_mut().unwrap().fields.insert("p.grepStyles".into(), json!([{"style": "[None]", "pattern": "(?<=x"}]));
        let e = confirm(&mut app).unwrap_err();
        assert!(e.contains("bad GREP"), "{e}");
        let d = app.ui.dialog.clone().expect("the dialog stays open");
        assert_eq!((d.s("section").as_str(), d.s("status").contains("bad GREP")), ("grep", true));
        assert!(app.session.doc().unwrap().doc.styles.para("Ruled").unwrap().para.grep_styles.is_none(), "nothing saved");
        draw(&mut app, &ctx);
    }

    /// A frame with two paragraphs of [Basic Paragraph]; the first has a local Word Spacing minimum.
    fn two_paragraphs() -> (DesignApp, Value) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 400], "content": "text", "text": "One\nTwo"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 0})).unwrap();
        app.run("type.para", json!({"attrs": {"wordSpaceMin": 0.7}})).unwrap();
        (app, r["story"].clone())
    }

    /// The resolved paragraph attribute `key` of the paragraph at byte `at`.
    fn para_attr(app: &mut DesignApp, story: &Value, at: usize, key: &str) -> Value {
        app.run("text.select", json!({"story": story, "anchor": at, "focus": at})).unwrap();
        app.run("type.selectionAttrs", json!({})).unwrap()["para"][key].clone()
    }

    #[test]
    fn justification_dialog_applies_to_a_local_paragraph_and_cancel_changes_nothing() {
        let (mut app, story) = two_paragraphs();
        let ctx = egui::Context::default();
        assert!(crate::menus::menu_enabled(&app, "app.justificationDialog"));
        app.run("app.justificationDialog", json!({})).unwrap();
        draw(&mut app, &ctx);
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!(d.id, "justification");
        assert_eq!(d.fields["current"]["wordSpaceMin"], 0.7);
        assert_eq!(d.fields["current"]["wordSpaceDesired"], 1.0);
        app.ui.dialog.as_mut().unwrap().fields.insert("wordSpaceDesired".into(), json!(1.1));
        draw(&mut app, &ctx);
        let before = undo_steps(&app);
        confirm(&mut app).unwrap();
        assert_eq!(undo_steps(&app), before + 1, "one undo step");
        assert_eq!(para_attr(&mut app, &story, 0, "wordSpaceDesired"), 1.1);
        assert_eq!(para_attr(&mut app, &story, 0, "wordSpaceMin"), 0.7, "unedited fields stay");
        assert_eq!(para_attr(&mut app, &story, 4, "wordSpaceDesired"), 1.0, "only the selected paragraph");
        let style = app.session.doc().unwrap().doc.styles.resolve_para_style(designcraft_doc::story::BASIC_PARAGRAPH).0.word_space_desired;
        assert_eq!(style, 1.0, "the paragraph style keeps its value");

        // Cancel (Escape) leaves the paragraph alone.
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        app.run("app.justificationDialog", json!({})).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("wordSpaceDesired".into(), json!(1.2));
        let before = undo_steps(&app);
        let escape = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events: vec![escape],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        });
        out.textures_delta.clear();
        assert!(app.ui.dialog.is_none(), "cancelled");
        assert_eq!(undo_steps(&app), before);
        assert_eq!(para_attr(&mut app, &story, 0, "wordSpaceDesired"), 1.1);

        app.run("edit.deselectAll", json!({})).unwrap();
        assert!(!crate::menus::menu_enabled(&app, "app.justificationDialog"), "nothing to justify");
        assert!(app.run("app.justificationDialog", json!({})).is_err());
    }

    #[test]
    fn justification_dialog_leaves_mixed_fields_alone_and_refuses_bad_values() {
        let (mut app, story) = two_paragraphs();
        let ctx = egui::Context::default();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 7})).unwrap();
        app.run("app.justificationDialog", json!({})).unwrap();
        draw(&mut app, &ctx);
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!(d.fields["current"]["wordSpaceMin"], Value::Null, "mixed shows empty");
        assert_eq!(d.fields["current"]["wordSpaceMax"], 1.33);

        // Out of range, and minimum above desired: an error, the dialog stays open, nothing changes.
        let before = undo_steps(&app);
        for (key, v) in [("wordSpaceMax", 20.0), ("glyphScaleMin", 0.2), ("letterSpaceDesired", f64::NAN)] {
            app.ui.dialog = Some(d.clone());
            app.ui.dialog.as_mut().unwrap().fields.insert(key.into(), json!(v));
            assert!(confirm(&mut app).is_err(), "{key} = {v}");
            assert!(app.ui.dialog.as_ref().is_some_and(|d| d.fields.contains_key("justifyError")), "{key}: the dialog shows why");
        }
        app.ui.dialog = Some(d.clone());
        app.ui.dialog.as_mut().unwrap().fields.insert("letterSpaceMin".into(), json!(0.1));
        assert!(confirm(&mut app).is_err(), "letter spacing minimum above desired");
        draw(&mut app, &ctx);
        assert_eq!(undo_steps(&app), before);

        app.ui.dialog = Some(d);
        app.ui.dialog.as_mut().unwrap().fields.insert("wordSpaceMax".into(), json!(1.5));
        confirm(&mut app).unwrap();
        assert_eq!(undo_steps(&app), before + 1);
        for (at, min) in [(0, 0.7), (4, 0.8)] {
            assert_eq!(para_attr(&mut app, &story, at, "wordSpaceMax"), 1.5);
            assert_eq!(para_attr(&mut app, &story, at, "wordSpaceMin"), min, "each paragraph keeps its own minimum");
        }
    }

    fn command_app(id: &str, fields: Value) -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.dialog = Some(Dialog::new(&format!("cmd:{id}"), fields));
        app
    }

    #[test]
    fn generic_new_document_preserves_typed_values_and_text_entry() {
        for pages in [json!(2), json!(" 2 ")] {
            let mut app = command_app(
                "file.new",
                json!({
                    "preset": "A4", "pages": pages, "facingPages": false, "gutter": 0,
                    "margins": {"top": 10, "bottom": 20, "inside": 30, "outside": 40},
                    "title": "  ", "status": "internal", "primaryTextFrame": true
                }),
            );
            confirm(&mut app).unwrap();
            let doc = &app.session.active().unwrap().doc;
            assert_eq!(doc.page_count(), 2);
            assert!(!doc.settings.facing_pages);
            assert!(doc.title.starts_with("Untitled-"));
            for page in doc.spreads.iter().flat_map(|s| &s.pages) {
                assert!((page.width - 595.2755905511812).abs() < 0.01);
                assert!((page.height - 841.8897637795277).abs() < 0.01);
                assert_eq!(page.columns.gutter, 0.0);
                assert_eq!(page.margins.top, 10.0);
                assert_eq!(page.margins.outside, 40.0);
            }
            // `primaryTextFrame` is documented since #776, so the dialog passes it on (`status` isn't).
            assert!(doc.spreads.iter().any(|s| !s.items.is_empty()), "documented fields reach the command");
            assert!(app.ui.dialog.is_none());
        }
    }

    #[test]
    fn numbering_commands_document_their_fields() {
        let names = |id: &str| command_fields(designcraft_engine::find_command(id).unwrap().params).into_iter().map(|f| f.key).collect::<Vec<_>>();
        assert_eq!(names("layout.section"), ["page", "startNumber", "style", "prefix", "includePrefix", "marker", "remove"]);
        assert_eq!(names("layout.chapterNumbering"), ["style", "start", "source"]);
    }

    #[test]
    fn generic_command_preserves_array_geometry() {
        let mut app = command_app("file.new", json!({}));
        confirm(&mut app).unwrap();
        app.ui.dialog = Some(Dialog::new("cmd:frame.create", json!({"rect": [10, 20, 110, 220]})));
        confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        let item = doc.spreads.iter().flat_map(|s| &s.items).next().unwrap();
        assert_eq!(item.bounds(), designcraft_geom::Rect::new(10.0, 20.0, 110.0, 220.0));
    }

    #[test]
    fn generic_explicit_null_uses_the_commands_existing_semantics() {
        // file.new accepts null width by using the selected preset's width.
        for width in [Value::Null, json!("null")] {
            let mut app = command_app("file.new", json!({"preset": "A4", "width": width, "title": "  Sample layout  "}));
            confirm(&mut app).unwrap();
            let doc = &app.session.active().unwrap().doc;
            assert_eq!(doc.title, "Sample layout");
            assert!((doc.settings.page_width - 595.2755905511812).abs() < 0.01);
        }
    }

    #[test]
    fn generic_invalid_number_retains_dialog_without_creating_document() {
        let mut app = command_app("file.new", json!({"width": 0}));
        assert!(confirm(&mut app).is_err());
        assert!(app.session.active().is_none());
        let dialog = app.ui.dialog.as_ref().unwrap();
        assert_eq!(dialog.fields["width"], 0);
        assert!(dialog.fields["status"].as_str().unwrap().contains("page size out of range"));
    }

    #[test]
    fn style_options_preview_updates_all_uses_cancels_and_commits_once() {
        use egui_kittest::{Harness, kittest::Queryable};
        for paragraph in [true, false] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            let f = app.run("frame.create", json!({"rect":[36,36,300,300],"content":"text","text":"First\nSecond"})).unwrap();
            app.run("text.select", json!({"story":f["story"],"anchor":0,"focus":12})).unwrap();
            let kind = if paragraph { "paragraph" } else { "character" };
            app.run(&format!("style.{kind}.create"), json!({"name":"Tutorial","chars":{"size":10}})).unwrap();
            app.run(&format!("style.{kind}.apply"), json!({"name":"Tutorial"})).unwrap();
            let before = app.session.doc().unwrap().doc.clone();
            let undo = app.session.doc().unwrap().history.undo.len();
            let journal = app.session.journal.len();
            open_style_options(&mut app, paragraph, "Tutorial");
            let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui_state(
                |ui, app: &mut DesignApp| {
                    let key = egui::Id::new("style_preview_test_fonts");
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
            h.get_by_label("Preview").click();
            h.run_steps(3);
            h.state_mut().ui.dialog.as_mut().unwrap().fields.insert("c.size".into(), json!(18));
            h.run_steps(3);
            let app = h.state_mut();
            assert_eq!(app.run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 18.0);
            assert_eq!(app.session.doc().unwrap().history.undo.len(), undo);
            assert_eq!(app.session.journal.len(), journal);
            // Repeated previews replace each other, never accumulate undo entries.
            app.ui.dialog.as_mut().unwrap().fields.insert("c.size".into(), json!(20));
            h.run_steps(3);
            assert_eq!(h.state_mut().run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 20.0);
            h.get_by_label("Preview").click();
            h.run_steps(3);
            assert_eq!(h.state().session.doc().unwrap().doc, before);
            h.get_by_label("Preview").click();
            h.run_steps(3);
            cancel(h.state_mut());
            assert_eq!(h.state().session.doc().unwrap().doc, before);
            assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo);
            open_style_options(h.state_mut(), paragraph, "Tutorial");
            let d = h.state_mut().ui.dialog.as_mut().unwrap();
            d.fields.insert("preview".into(), json!(true));
            d.fields.insert("c.size".into(), json!(24));
            h.run_steps(3);
            confirm(h.state_mut()).unwrap();
            assert_eq!(h.state().session.doc().unwrap().history.undo.len(), undo + 1);
            // Both paragraphs/character runs still use the globally edited definition.
            for pos in [0, 6] {
                h.state_mut().run("text.select", json!({"story":f["story"],"anchor":pos,"focus":pos+1})).unwrap();
                assert_eq!(h.state_mut().run("type.selectionAttrs", json!({})).unwrap()["chars"]["size"], 24.0);
            }
            h.state_mut().run("edit.undo", json!({})).unwrap();
            assert_eq!(h.state().session.doc().unwrap().doc, before);
        }
    }

    #[test]
    fn style_preview_refuses_justification_spacing_ok_would_refuse() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        let mut d =
            Dialog::new("paragraphStyleOptions", json!({"name": designcraft_doc::BASIC_PARAGRAPH, "preview": true, "p.wordSpaceDesired": 1.1}));
        style_options_preview(&mut app, &mut d);
        assert_ne!(app.session.doc().unwrap().doc, before, "a valid value previews");
        // Minimum above Desired: the page goes back and the dialog says why.
        d.fields.insert("p.wordSpaceMin".into(), json!(1.5));
        style_options_preview(&mut app, &mut d);
        assert_eq!(app.session.doc().unwrap().doc, before);
        assert!(d.s("status").contains("Word Spacing"), "{:?}", d.fields.get("status"));
        assert!(app.session.doc().unwrap().interaction.is_none());
    }

    #[test]
    fn invalid_style_preview_leaves_the_starting_document_and_can_be_cancelled() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.character.create", json!({"name":"Emphasis"})).unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        let mut d = Dialog::new("characterStyleOptions", json!({"name":"Emphasis","preview":true,"basedOn":"Emphasis"}));
        style_options_preview(&mut app, &mut d);
        assert!(d.fields.contains_key("status"));
        assert_eq!(app.session.doc().unwrap().doc, before);
        app.ui.dialog = Some(d);
        cancel(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, before);
        assert!(app.session.doc().unwrap().interaction.is_none());
    }

    #[test]
    fn style_preview_rejects_conflicting_scalar_and_nested_rule_fields_without_panicking() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        let mut dialog = Dialog::new(
            "paragraphStyleOptions",
            json!({
                "name": designcraft_doc::BASIC_PARAGRAPH, "preview": true,
                "p.ruleAbove": false, "p.ruleAbove.weight": 2
            }),
        );
        style_options_preview(&mut app, &mut dialog);
        assert!(dialog.fields.contains_key("status"));
        assert_eq!(app.session.doc().unwrap().doc, before);
        app.ui.dialog = Some(dialog);
        cancel(&mut app);
        assert!(app.session.doc().unwrap().interaction.is_none());
    }

    #[test]
    fn text_frame_options_set_columns_baseline_and_wrap_as_indesign_does() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        // A 300 pt frame, 2 columns, 12 pt gutter (picas in the dialog).
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        app.run("object.textFrameOptions", json!({"ids": [id.0], "columns": 2, "gutter": 12})).unwrap();
        let opts = |app: &DesignApp| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(&app, &mut d);
        // Fixed Number shows each column's width: (300 - 12) / 2 = 144 pt.
        assert_eq!((d.s("columnsKind"), d.s("columnWidth"), d.s("firstBaseline")), ("fixedNumber".into(), "12p0".into(), "ascent".into()));
        for (k, v) in [
            ("columnsKind", json!("fixedWidth")),
            ("columnWidth", json!("10p0")),
            ("firstBaseline", json!("capHeight")),
            ("firstBaselineMin", json!("0p6")),
            ("balanceColumns", json!(true)),
            ("ignoreWrap", json!(true)),
        ] {
            d.fields.insert(k.into(), v);
        }
        app.ui.dialog = Some(d);
        confirm(&mut app).unwrap();
        let o = opts(&app);
        assert_eq!(o.columns_kind, designcraft_doc::ColumnsKind::FixedWidth);
        assert_eq!((o.column_width, o.first_baseline_min, o.columns), (120.0, 6.0, 2));
        assert_eq!(o.first_baseline, designcraft_doc::FirstBaseline::CapHeight);
        assert!(o.balance_columns && o.ignore_wrap);
        // Fixed Number again: the width isn't sent.
        let mut d = Dialog::new("textFrameOptions", json!({}));
        seed_text_frame_options(&app, &mut d);
        assert_eq!(d.s("columnWidth"), "10p0", "Fixed Width shows its own width");
        d.fields.insert("columnsKind".into(), json!("fixedNumber"));
        app.ui.dialog = Some(d);
        confirm(&mut app).unwrap();
        assert_eq!((opts(&app).columns_kind, opts(&app).column_width), (designcraft_doc::ColumnsKind::FixedNumber, 120.0));
    }

    #[test]
    fn text_frame_options_change_only_what_was_edited() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [36, 36, 336, 236], "content": "text", "text": "Hello"})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        app.run("object.textFrameOptions", json!({"ids": [id.0], "columns": 2, "inset": [1, 2, 3, 4]})).unwrap();
        let opts = |app: &DesignApp| app.session.doc().unwrap().doc.item(id).unwrap().text_frame().unwrap().options.clone();
        let before = opts(&app);
        let open = |app: &mut DesignApp, fields: &[(&str, Value)]| {
            app.ui.dialog = Some(Dialog::new("textFrameOptions", json!({})));
            let mut d = app.ui.dialog.take().unwrap();
            seed_text_frame_options(app, &mut d);
            for (k, v) in fields {
                d.fields.insert((*k).into(), v.clone());
            }
            app.ui.dialog = Some(d);
            confirm(app)
        };
        // OK without edits changes nothing (no undo step either).
        assert_eq!(open(&mut app, &[]).unwrap(), Value::Null);
        assert_eq!(opts(&app), before);
        // The rule's weight is in points; unequal insets the user didn't touch stay.
        open(&mut app, &[("columnRule", json!(true)), ("columnRuleWeight", json!("6"))]).unwrap();
        let o = opts(&app);
        assert!(o.column_rule && o.column_rule_weight == 6.0, "{o:?}");
        assert_eq!((o.inset, o.columns, o.gutter), (before.inset, before.columns, before.gutter));
        // One of the rule's insets edited: the other rule settings stay.
        open(&mut app, &[("columnRuleBottomInset", json!("0p9")), ("columnRuleTint", json!("40"))]).unwrap();
        let o = opts(&app);
        assert_eq!((o.column_rule_bottom_inset, o.column_rule_top_inset, o.column_rule_offset, o.column_rule_weight), (9.0, 0.0, 0.0, 6.0));
        assert!((o.column_rule_tint - 0.4).abs() < 1e-6 && o.column_rule);
        // One inset side edited: the others keep their values.
        open(&mut app, &[("insetLeft", json!("1p0"))]).unwrap();
        assert_eq!(opts(&app).inset, [1.0, 12.0, 3.0, 4.0]);
        open(&mut app, &[("balanceColumns", json!(true))]).unwrap();
        let o = opts(&app);
        assert!(o.balance_columns && o.column_rule && o.columns == 2, "{o:?}");
        // An unknown swatch is an error and the dialog stays open.
        let e = open(&mut app, &[("columnRuleColor", json!("Mauve"))]).unwrap_err();
        assert!(e.contains("Mauve") && app.ui.dialog.is_some(), "{e}");
        assert_eq!(opts(&app).column_rule_color, designcraft_color::swatch::BLACK);
        // Each OK is one undo step.
        for _ in 0..4 {
            app.run("edit.undo", json!({})).unwrap();
        }
        assert_eq!(opts(&app), before);
    }

    #[test]
    fn toc_dialog_adds_a_style_and_ok_makes_the_table_of_contents() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.newSample", json!({})).unwrap();
        crate::menus::activate(&mut app, "toc.generate", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some(TOC));
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for expected in
            ["Table of Contents", "Title:", "Include Paragraph Styles:", "Other Styles:", "« Add", "Remove »", "Replace Existing Table of Contents"]
        {
            visible_label(&labels, screen, expected);
        }
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Headline").center());
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "« Add").center());
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for expected in ["Entry Style:", "TOC Level 1", "After Entry", "Between Entry and Number:", "Level:"] {
            visible_label(&labels, screen, expected);
        }
        let undo = app.session.doc().unwrap().history.undo.len();
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "OK").center());
        assert!(app.ui.dialog.is_none());
        let d = &app.session.doc().unwrap().doc;
        let toc = d.toc.clone().unwrap();
        assert_eq!(toc.entries, vec![designcraft_doc::TocEntry::new("Headline", 1)]);
        let text = &d.story(toc.story).unwrap().text;
        assert!(text.starts_with("Contents\n") && text.contains("\nNotes on the Grid\t"), "{text}");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
        // Reopened, the dialog shows the table of contents and replaces it.
        crate::menus::activate(&mut app, "toc.generate", &Value::Null);
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(d.b("replace"));
        assert_eq!(toc_rows(d).iter().map(|r| r.style.as_str()).collect::<Vec<_>>(), ["Headline"]);
    }

    #[test]
    fn margins_and_columns_link_arithmetic_preview_and_one_undo_step() {
        // Adobe's "Set up a modern magazine layout" (1:41–3:04): linked margins to 0.4375 in, then
        // 5 columns with a 0.1875 in gutter, unlinked, and "*2" typed after the outside margin.
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"preset": "A4", "pages": 3, "facingPages": true, "margins": 36})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0)));
        open_margins_and_columns(&mut app);
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
        let mut labels = vec![];
        for _ in 0..3 {
            labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        for expected in
            ["Margins and Columns", "Margins", "Top:", "Bottom:", "Inside:", "Outside:", "Columns", "Number:", "Gutter:", "Adjust Layout", "Preview"]
        {
            visible_label(&labels, screen, expected);
        }
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(d.b("link"), "four equal margins open linked");
        // Linked: one margin typed is all four.
        app.ui.dialog.as_mut().unwrap().fields.insert("marginTop".into(), json!("0.4375 in"));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(MARGIN_FIELDS.iter().all(|(k, _, _)| d.s(k) == "0.4375 in"), "{:?}", d.fields);
        // Preview: the page changes, with no undo step.
        let undo = app.session.doc().unwrap().history.undo.len();
        app.ui.dialog.as_mut().unwrap().fields.insert("preview".into(), json!(true));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let top = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].pages[0].margins.top;
        assert!((top(&app) - 31.5).abs() < 1e-6, "previewed: {}", top(&app));
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo);
        // Unlinked (the link button), the outside margin alone: "*2" typed after it is worked out
        // when the field is left.
        app.ui.dialog.as_mut().unwrap().fields.insert("link".into(), json!(false));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let key = |k| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let field = labels
            .iter()
            .filter(|(t, _, _)| t.trim() == "0.4375 in")
            .map(|(_, r, _)| *r)
            .max_by(|a, b| (a.max.x + a.max.y).total_cmp(&(b.max.x + b.max.y)))
            .unwrap();
        click_dialog(&mut app, &ctx, screen, field.center());
        dialog_frame(&mut app, &ctx, screen, vec![key(egui::Key::End), egui::Event::Text("*2".into())]);
        dialog_frame(&mut app, &ctx, screen, vec![key(egui::Key::Tab)]);
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let d = app.ui.dialog.as_ref().unwrap();
        assert!(
            !d.s("marginOutside").contains('*') && (d.m("marginOutside").unwrap() - 63.0).abs() < 1e-6,
            "worked out on leaving the field: {}",
            d.s("marginOutside")
        );
        let d = app.ui.dialog.as_mut().unwrap();
        d.fields.insert("link".into(), json!(false));
        for (k, v) in [("marginOutside", "0.4375 in*2"), ("columns", "5"), ("gutter", "0.1875 in"), ("target", "all")] {
            d.fields.insert(k.into(), json!(v));
        }
        dialog_frame(&mut app, &ctx, screen, vec![]);
        confirm(&mut app).unwrap();
        let pg = app.session.doc().unwrap().doc.spreads[1].pages[0].clone();
        assert_eq!((pg.margins.top, pg.margins.outside, pg.columns.count), (31.5, 63.0, 5));
        assert!((pg.columns.gutter - 13.5).abs() < 1e-6);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
        // Cancel after a preview puts the page back.
        open_margins_and_columns(&mut app);
        let d = app.ui.dialog.as_mut().unwrap();
        d.fields.insert("preview".into(), json!(true));
        d.fields.insert("columns".into(), json!("2"));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        cancel(&mut app);
        assert_eq!(app.session.doc().unwrap().doc.spreads[1].pages[0].columns.count, 5);
    }

    #[test]
    fn create_guides_reads_like_indesigns_and_previews() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 1, "facingPages": false})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0)));
        open_create_guides(&mut app);
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
        let mut labels = vec![];
        for _ in 0..3 {
            labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        for expected in
            ["Create Guides", "Rows", "Columns", "Options", "Fit Guides to:", "Margins", "Page", "Remove Existing Ruler Guides", "Preview"]
        {
            visible_label(&labels, screen, expected);
        }
        // Rows and Columns side by side.
        let (r, c) = (visible_label(&labels, screen, "Rows"), visible_label(&labels, screen, "Columns"));
        assert!((r.center().y - c.center().y).abs() < 2.0 && r.center().x < c.center().x);
        // Thirds, single guides, fitted to the page: clicking the Page radio.
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Page").center());
        let d = app.ui.dialog.as_mut().unwrap();
        assert_eq!(d.s("fitTo"), "page");
        d.fields.insert("rows".into(), json!("3"));
        d.fields.insert("rowGutter".into(), json!("0"));
        d.fields.insert("preview".into(), json!(true));
        dialog_frame(&mut app, &ctx, screen, vec![]);
        let guides = |app: &DesignApp| app.session.doc().unwrap().doc.spreads[0].pages[0].guides.len();
        assert_eq!(guides(&app), 2, "previewed");
        confirm(&mut app).unwrap();
        assert_eq!(guides(&app), 2, "made once");
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(guides(&app), 0, "one undo step");
    }

    fn dialog_context(language: &str) -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, language);
        crate::theme::apply(&ctx, &crate::theme::Tokens::for_brightness(crate::theme::Brightness::default()));
        ctx
    }

    fn dialog_frame(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, events: Vec<egui::Event>) -> Vec<(String, egui::Rect, egui::Rect)> {
        let time = ctx.input(|i| i.time) + 1.0 / 60.0;
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(time), events, ..Default::default() }, |ui| {
            show(app, ui.ctx());
        });
        output.textures_delta.clear();
        fn collect(shape: &egui::Shape, clip: egui::Rect, labels: &mut Vec<(String, egui::Rect, egui::Rect)>) {
            match shape {
                egui::Shape::Text(t) => labels.push((t.galley.job.text.clone(), t.visual_bounding_rect(), clip)),
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        collect(s, clip, labels);
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, shape.clip_rect, &mut labels);
        }
        labels
    }

    fn visible_label(labels: &[(String, egui::Rect, egui::Rect)], screen: egui::Rect, expected: &str) -> egui::Rect {
        let (_, rect, clip) = labels.iter().find(|(text, _, _)| text.trim() == expected).unwrap_or_else(|| panic!("{expected} is painted"));
        assert!(screen.contains_rect(*rect), "{expected} must stay on screen: {rect:?}");
        assert!(clip.contains_rect(*rect), "{expected} must not be clipped: {rect:?}, {clip:?}");
        *rect
    }

    fn click_dialog(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, pos: egui::Pos2) {
        for pressed in [true, false] {
            dialog_frame(
                app,
                ctx,
                screen,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
    }

    fn scroll_dialog(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, pos: egui::Pos2, delta: egui::Vec2) {
        dialog_frame(
            app,
            ctx,
            screen,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase: egui::TouchPhase::Move, modifiers: egui::Modifiers::NONE },
            ],
        );
        // Let smooth wheel scrolling and scrollbar visibility settle before clicking.
        for _ in 0..30 {
            dialog_frame(app, ctx, screen, vec![]);
        }
    }

    fn scope_control(app: &mut DesignApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
        let (request, _reply) = crate::control::ControlRequest::new(method, params);
        let crate::control::Outcome::Done(value) = crate::control::handle(app, ctx, &request) else { panic!("ordinary control response") };
        assert_eq!(value["ok"], true, "{value}");
        value["result"].clone()
    }

    fn scope_click(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, label: &str) {
        let labels = dialog_frame(app, ctx, screen, vec![]);
        let pos = visible_label(&labels, screen, label).center();
        click_dialog(app, ctx, screen, pos);
    }

    fn scope_dialog() -> (DesignApp, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
        scope_dialog_with_selection("red red | red", 7)
    }

    fn scope_dialog_with_selection(text: &str, end: usize) -> (DesignApp, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
        scope_dialog_with_tool(text, end, None)
    }

    fn scope_dialog_with_tool(
        text: &str,
        end: usize,
        tool: Option<&str>,
    ) -> (DesignApp, egui::Context, egui::Rect, designcraft_engine::doc::StoryId) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".into();
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [36, 36, 100, 100], "content": "text", "text": text})).unwrap();
        let sid = designcraft_engine::doc::StoryId(frame["story"].as_u64().unwrap());
        let ctx = dialog_context("en");
        if let Some(tool) = tool {
            scope_control(&mut app, &ctx, "ui.tool.select", json!({"tool": tool}));
        }
        app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": end})).unwrap();
        app.run("app.findChange", json!({})).unwrap();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
        for (field, value) in [("find", "red"), ("change", "unused")] {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
        }
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        scope_click(&mut app, &ctx, screen, "document");
        scope_click(&mut app, &ctx, screen, "selection");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("scope"), "selection");
        (app, ctx, screen, sid)
    }

    fn scope_selected(app: &DesignApp, sid: designcraft_engine::doc::StoryId, a: usize, b: usize) {
        let t = app.session.doc().unwrap().selection.text.unwrap();
        assert_eq!((t.story, t.range(), t.cell), (sid, a..b, None));
    }

    fn scope_state_unchanged(app: &DesignApp, before: &designcraft_engine::DocState) {
        let after = app.session.doc().unwrap();
        assert!(std::sync::Arc::ptr_eq(&before.doc, &after.doc));
        assert_eq!(after.uid, before.uid);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.selection, before.selection);
        assert_eq!(after.history.limit, before.history.limit);
        for (actual, expected) in [(&after.history.undo, &before.history.undo), (&after.history.redo, &before.history.redo)] {
            assert_eq!(actual.len(), expected.len());
            for (a, b) in actual.iter().zip(expected) {
                assert_eq!(a.label, b.label);
                assert_eq!(a.selection, b.selection);
                assert!(std::sync::Arc::ptr_eq(&a.doc, &b.doc));
            }
        }
    }

    #[test]
    fn find_scope_dialog_control_effective_noops_retain_bounds() {
        for (field, value) in [("objectMode", json!(false)), ("story", Value::Null)] {
            let (mut app, ctx, screen, sid) = scope_dialog();
            assert!(!app.ui.dialog.as_ref().unwrap().fields.contains_key(field));
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 3);
            let before = app.session.doc().unwrap().clone();
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
            assert_eq!(app.ui.dialog.as_ref().unwrap().fields[field], value);
            scope_state_unchanged(&app, &before);
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 4, 7);
            scope_click(&mut app, &ctx, screen, "Change All");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
            assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
        }
    }

    #[test]
    fn find_scope_dialog_explicit_story_ignored_scope_preserves_grep_cursor() {
        for physical in [false, true] {
            for (text, pattern, hits, changed) in [
                ("é", "é|(?:)", vec![(0, 2)], "X"),
                ("é🙂", "é|(?:)", vec![(0, 2), (6, 6)], "X🙂X"),
                ("é🙂", "(?:)", vec![(0, 0), (2, 2), (6, 6)], "XéX🙂X"),
            ] {
                let (mut app, ctx, screen, sid) = scope_dialog_with_selection(text, 0);
                for (field, value) in [("find", json!(pattern)), ("change", json!("X")), ("story", json!(sid.0))] {
                    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
                }
                scope_click(&mut app, &ctx, screen, "GREP");
                scope_click(&mut app, &ctx, screen, "Count");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} match(es)", hits.len()));
                for &(a, b) in &hits {
                    scope_click(&mut app, &ctx, screen, "Find Next");
                    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "Found");
                    scope_selected(&app, sid, a, b);
                    let before = app.session.doc().unwrap().clone();
                    let cur = app.ui.dialog.as_ref().unwrap().s("scope");
                    let next = if cur == "selection" { "document" } else { "selection" };
                    if physical {
                        scope_click(&mut app, &ctx, screen, &cur);
                        scope_click(&mut app, &ctx, screen, next);
                    } else {
                        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": next}));
                    }
                    assert_eq!(app.ui.dialog.as_ref().unwrap().s("scope"), next);
                    assert_eq!(app.ui.dialog.as_ref().unwrap().fields["story"], json!(sid.0));
                    scope_state_unchanged(&app, &before);
                    scope_click(&mut app, &ctx, screen, "Count");
                    assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} match(es)", hits.len()));
                }
                scope_click(&mut app, &ctx, screen, "Find Next");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
                // An ignored scope change must not revive an exhausted endpoint either.
                let before = app.session.doc().unwrap().clone();
                let cur = app.ui.dialog.as_ref().unwrap().s("scope");
                let next = if cur == "selection" { "document" } else { "selection" };
                if physical {
                    scope_click(&mut app, &ctx, screen, &cur);
                    scope_click(&mut app, &ctx, screen, next);
                } else {
                    scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": next}));
                }
                scope_click(&mut app, &ctx, screen, "Find Next");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
                scope_state_unchanged(&app, &before);
                scope_click(&mut app, &ctx, screen, "Change All");
                assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), format!("{} replacement(s) made", hits.len()));
                assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, changed);
            }
        }
    }

    #[test]
    fn find_scope_dialog_explicit_story_transitions_reset_cursor() {
        for remove_override in [false, true] {
            let (mut app, ctx, screen, sid) = scope_dialog_with_selection("é🙂", 0);
            let other = app.run("frame.create", json!({"rect": [180, 36, 100, 100], "content": "text", "text": "other"})).unwrap();
            app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
            for (field, value) in [("find", json!("(?:)")), ("story", json!(sid.0))] {
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
            }
            scope_click(&mut app, &ctx, screen, "GREP");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 0);
            let before = app.session.doc().unwrap().clone();
            let value = if remove_override { Value::Null } else { other["story"].clone() };
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": value}));
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": sid.0}));
            scope_state_unchanged(&app, &before);
            // Away and back still releases continuation even if no Find command intervenes.
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 0);
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 2, 2);
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "é🙂");
            let other_sid = designcraft_engine::doc::StoryId(other["story"].as_u64().unwrap());
            assert_eq!(app.session.doc().unwrap().doc.story(other_sid).unwrap().text, "other");
        }
    }

    #[test]
    fn find_scope_dialog_control_mode_changes_release_bounds() {
        for object_mode in [false, true] {
            let (mut app, ctx, _, sid) = scope_dialog();
            if object_mode {
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "objectMode", "value": true}));
            }
            // Direct Session consumers share the retained domain even while Object is shown.
            let p = json!({"find": "red", "change": "blue", "scope": "selection"});
            let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
            assert_eq!(count.as_array().unwrap().len(), 2);
            scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.next", "params": p}));
            scope_selected(&app, sid, 0, 3);
            let before = app.session.doc().unwrap().clone();
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "objectMode", "value": !object_mode}));
            assert_eq!(app.ui.dialog.as_ref().unwrap().b("objectMode"), !object_mode);
            scope_state_unchanged(&app, &before);
            let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
            assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
            let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
            assert_eq!(changed, json!({"count": 1}));
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
        }
    }

    #[test]
    fn find_scope_dialog_tool_replacement_releases_bounds() {
        for (tool, replacement) in [("page", "cmd:layout.pageSize"), ("rectangleFrame", "frameSize")] {
            let (mut app, ctx, screen, sid) = scope_dialog_with_tool("red red | red", 7, Some(tool));
            assert_eq!(app.session.tool_id(), tool);
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 3);
            let before = app.session.doc().unwrap().clone();
            let active = (app.session.active_index(), app.session.documents().len());
            let off = app.session.layout().xf(designcraft_engine::doc::SpreadRef::Doc(0)).translation();
            let x = off.x + before.doc.spreads[0].pages[0].x + 200.0;
            let y = off.y + 200.0;
            // The Page tool opens Page Size on a double-click (a click selects the page).
            let events = if tool == "page" {
                json!([{"kind": "doubleclick", "x": x, "y": y}])
            } else {
                json!([{"kind": "down", "x": x, "y": y}, {"kind": "up", "x": x, "y": y}])
            };
            // Real tool -> Action::Dialog -> after_engine; no injected UI request.
            scope_control(&mut app, &ctx, "ui.pointer", json!({"events": events}));
            assert_eq!(app.ui.dialog.as_ref().unwrap().id, replacement);
            assert!(app.session.ui_requests.is_empty());
            assert_eq!((app.session.active_index(), app.session.documents().len()), active);
            scope_state_unchanged(&app, &before);
            scope_click(&mut app, &ctx, screen, "Cancel");
            assert!(app.ui.dialog.is_none());
            assert_eq!((app.session.active_index(), app.session.documents().len()), active);
            scope_state_unchanged(&app, &before);
            let p = json!({"find": "red", "change": "blue", "scope": "selection"});
            let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
            assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
            scope_state_unchanged(&app, &before);
            let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
            assert_eq!(changed, json!({"count": 1}));
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
            assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
        }
    }

    #[test]
    fn find_scope_dialog_tool_without_replacement_retains_bounds() {
        for tool in ["page", "rectangleFrame"] {
            let (mut app, ctx, screen, sid) = scope_dialog_with_tool("red red | red", 7, Some(tool));
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 3);
            let before = app.session.doc().unwrap().clone();
            let fields = app.ui.dialog.as_ref().unwrap().fields.clone();
            scope_control(&mut app, &ctx, "ui.pointer", json!({"events": [{"kind": "move", "x": 200, "y": 200}]}));
            assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findChange");
            assert_eq!(app.ui.dialog.as_ref().unwrap().fields, fields);
            scope_state_unchanged(&app, &before);
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 4, 7);
            scope_click(&mut app, &ctx, screen, "Change All");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn find_scope_dialog_native_pdf_replacement_releases_bounds() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("three.pdf");
        std::fs::write(&path, crate::browser_import_tests::pdf()).unwrap();
        for dismissal in ["Cancel", "Escape", "control"] {
            let (mut app, ctx, screen, sid) = scope_dialog();
            let picked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let seen_pick = picked.clone();
            let seen_read = read.clone();
            let pick_path = path.to_string_lossy().to_string();
            let read_path = pick_path.clone();
            app.services.pick_open = Some(Box::new(move |purpose| {
                assert_eq!(purpose, "place");
                seen_pick.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some(pick_path.clone())
            }));
            app.services.read = Some(Box::new(move |path| {
                assert_eq!(path, read_path);
                seen_read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::fs::read(path).map_err(|e| e.to_string())
            }));
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 3);
            let before = app.session.doc().unwrap().clone();
            let active = (app.session.active_index(), app.session.documents().len());
            // Exercise engine.execute -> UiRequest::Pick -> native host PDF replacement.
            scope_control(&mut app, &ctx, "engine.execute", json!({"command": "file.place", "params": {}}));
            assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 1);
            let dialog = app.ui.dialog.as_ref().unwrap();
            assert_eq!(dialog.id, "pdfImport");
            assert_eq!(dialog.s("path"), path.to_string_lossy());
            assert_eq!(dialog.fields["pages"], 3);
            assert_eq!(dialog.s("page"), "1");
            assert!(!dialog.b("async"));
            assert_eq!(dialog.fields["target"], json!(before.uid));
            assert!(app.session.ui_requests.is_empty());
            assert!(app.pending_pdf.is_none());
            assert_eq!((app.session.active_index(), app.session.documents().len()), active);
            scope_state_unchanged(&app, &before);
            match dismissal {
                "Cancel" => scope_click(&mut app, &ctx, screen, "Cancel"),
                "Escape" => {
                    dialog_frame(
                        &mut app,
                        &ctx,
                        screen,
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                }
                "control" => {
                    scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
                }
                _ => panic!("known dismissal"),
            }
            assert!(app.ui.dialog.is_none());
            assert!(app.pending_pdf.is_none());
            assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!((app.session.active_index(), app.session.documents().len()), active);
            scope_state_unchanged(&app, &before);
            let p = json!({"find": "red", "change": "blue", "scope": "selection"});
            let count = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.find", "params": p}));
            assert_eq!(count, json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
            scope_state_unchanged(&app, &before);
            let changed = scope_control(&mut app, &ctx, "engine.execute", json!({"command": "find.change", "params": p}));
            assert_eq!(changed, json!({"count": 1}));
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
            assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn find_scope_dialog_native_picker_cancel_retains_bounds() {
        let (mut app, ctx, screen, sid) = scope_dialog();
        let picked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = picked.clone();
        app.services.pick_open = Some(Box::new(move |purpose| {
            assert_eq!(purpose, "place");
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            None
        }));
        let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen_read = read.clone();
        app.services.read = Some(Box::new(move |_| {
            seen_read.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("canceled picker must not read".into())
        }));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let before = app.session.doc().unwrap().clone();
        let active = (app.session.active_index(), app.session.documents().len());
        let fields = app.ui.dialog.as_ref().unwrap().fields.clone();
        scope_control(&mut app, &ctx, "engine.execute", json!({"command": "file.place", "params": {}}));
        assert_eq!(picked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(read.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "findChange");
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields, fields);
        assert_eq!((app.session.active_index(), app.session.documents().len()), active);
        scope_state_unchanged(&app, &before);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), before.history.undo.len() + 1);
    }

    #[test]
    fn find_scope_dialog_exact_baseline_buttons() {
        let (mut app, ctx, screen, sid) = scope_dialog_with_selection("red red", 3);
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red");
    }

    #[test]
    fn find_scope_dialog_grep_tab_alone_restarts_original_selection() {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_click(&mut app, &ctx, screen, "GREP");
        assert!(app.ui.dialog.as_ref().unwrap().b("grep"));
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("find"), "red");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        scope_click(&mut app, &ctx, screen, "Text");
        assert!(!app.ui.dialog.as_ref().unwrap().b("grep"));
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
    }

    #[test]
    fn find_scope_dialog_buttons_retain_nonempty_hits_and_change_all() {
        let (mut app, ctx, screen, sid) = scope_dialog();
        let history = app.session.doc().unwrap().history.undo.len();
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        scope_click(&mut app, &ctx, screen, "Find Next");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "No matches");
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
        assert_eq!(app.session.doc().unwrap().history.undo.len(), history + 1);
        scope_click(&mut app, &ctx, screen, "Cancel");
        assert!(app.ui.dialog.is_none());
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
        app.run("edit.redo", json!({})).unwrap();
        let native = app.run("file.serialize", json!({})).unwrap();
        app.run("file.openBytes", json!({"name": "scope.designcraft", "base64": native["base64"]})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
    }

    fn scope_confirm_case(action: &str) {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Count");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        match action {
            "OK" => scope_click(&mut app, &ctx, screen, "OK"),
            "Enter" => {
                dialog_frame(
                    &mut app,
                    &ctx,
                    screen,
                    vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            "control" => {
                assert_eq!(scope_control(&mut app, &ctx, "ui.dialog.confirm", json!({}))["count"], 2);
            }
            _ => panic!("known confirmation route"),
        }
        assert!(app.ui.dialog.is_none());
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue blue | red");
        // The closed dialog cannot leave its old seven-byte range for a later direct query.
        let matches = app.run("find.find", json!({"find": "blue", "scope": "selection"})).unwrap();
        assert_eq!(matches.as_array().unwrap().len(), 0);
    }

    #[test]
    fn find_scope_dialog_ok_consumes_retained_bounds() {
        scope_confirm_case("OK");
    }

    #[test]
    fn find_scope_dialog_enter_consumes_retained_bounds() {
        scope_confirm_case("Enter");
    }

    #[test]
    fn find_scope_dialog_control_confirm_consumes_retained_bounds() {
        scope_confirm_case("control");
    }

    #[test]
    fn find_scope_dialog_query_reselection_cancel_and_reopen() {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "grep", "value": true}));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "find", "value": "r.d"}));
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
        assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
        app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 7})).unwrap();
        app.run("find.next", json!({"find": "red", "scope": "selection"})).unwrap();
        scope_control(&mut app, &ctx, "ui.dialog.open", json!({"id": "findChange", "fields": {"find": "red", "scope": "selection"}}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
        app.run("text.select", json!({"story": sid.0, "anchor": 4, "focus": 7})).unwrap();
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 4, 7);
        app.run("app.findChange", json!({})).unwrap();
        for (field, value) in [("find", "red"), ("scope", "selection")] {
            scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": field, "value": value}));
        }
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
    }

    #[test]
    fn find_scope_dialog_word_reselection_limits_count_and_change_buttons() {
        let (mut app, ctx, screen, sid) = scope_dialog();
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
        scope_click(&mut app, &ctx, screen, "Find Next");
        scope_selected(&app, sid, 0, 3);
        let selected = app.session.doc().unwrap().selection.clone();
        let frame = selected.text.unwrap().frame.unwrap();
        let point = {
            let st = app.session.doc().unwrap();
            let cs = app.session.cache.get(&st.doc, sid, None);
            let (fi, x, baseline, ascent, _) = designcraft_compose::caret(&cs, 1).unwrap();
            let inner = designcraft_geom::Point::new(x, baseline - ascent * 0.5);
            assert_eq!(cs.frames[fi].frame, frame);
            assert_eq!(designcraft_compose::hit(&cs, fi, inner), Some(1));
            let loc = st.doc.find(frame).unwrap();
            st.doc.parent_xf(&loc) * st.doc.text_xf(st.doc.item(frame).unwrap()) * inner
        };
        let before = app.session.doc().unwrap().doc.clone();
        let revision = app.session.doc().unwrap().revision;
        app.run("text.selectWord", json!({"frame": frame.0, "point": [point.x, point.y]})).unwrap();
        assert_eq!(app.session.doc().unwrap().selection, selected);
        assert!(app.session.doc().unwrap().revision > revision, "a new revision");
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 match(es)");
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "1 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn find_scope_dialog_failed_native_drop_alert_releases_bounds() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("broken.designcraft");
        std::fs::write(&path, b"not a native document").unwrap();
        for dismissal in ["OK", "Escape", "control"] {
            let (mut app, ctx, screen, sid) = scope_dialog();
            scope_click(&mut app, &ctx, screen, "Count");
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "2 match(es)");
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 3);
            let before = app.session.doc().unwrap().doc.clone();
            let selected = app.session.doc().unwrap().selection.clone();
            let revision = app.session.doc().unwrap().revision;
            let history = app.session.doc().unwrap().history.undo.len();
            let uid = app.session.doc().unwrap().uid;
            let error = app.open_dropped(&path.to_string_lossy()).unwrap_err();
            let alert = app.ui.dialog.as_ref().unwrap();
            assert_eq!(alert.id, "alert");
            assert_eq!(alert.s("title"), "Can't Open the File");
            assert_eq!(alert.s("message"), error);
            assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
            assert_eq!(app.session.doc().unwrap().selection, selected);
            assert_eq!(app.session.doc().unwrap().revision, revision);
            assert_eq!(app.session.doc().unwrap().uid, uid);
            assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
            match dismissal {
                "OK" => scope_click(&mut app, &ctx, screen, "OK"),
                "Escape" => {
                    dialog_frame(
                        &mut app,
                        &ctx,
                        screen,
                        vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                }
                "control" => {
                    scope_control(&mut app, &ctx, "ui.dialog.cancel", json!({}));
                }
                _ => panic!("known dismissal"),
            }
            assert!(app.ui.dialog.is_none());
            let p = json!({"find": "red", "change": "blue", "scope": "selection"});
            assert_eq!(app.run("find.find", p.clone()).unwrap(), json!([{"story": sid.0, "start": 0, "end": 3, "text": "red"}]));
            assert_eq!(app.run("find.change", p).unwrap(), json!({"count": 1}));
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "blue red | red");
            assert_eq!(app.session.doc().unwrap().history.undo.len(), history + 1);
        }
    }

    #[test]
    fn find_scope_dialog_child_errors_and_explicit_parent() {
        let (mut app, ctx, screen, sid) = scope_dialog();
        app.run("text.select", json!({"story": sid.0, "anchor": 13, "focus": 13})).unwrap();
        app.run("table.insert", json!({"rows": 1, "cols": 1})).unwrap();
        app.run("text.insert", json!({"text": "red child", "raw": true})).unwrap();
        let child = app.session.doc().unwrap().selection.text.unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        let selection = app.session.doc().unwrap().selection.clone();
        let history = app.session.doc().unwrap().history.undo.len();
        for label in ["Count", "Find Next", "Change All"] {
            scope_click(&mut app, &ctx, screen, label);
            assert!(app.ui.dialog.as_ref().unwrap().s("status").contains("not supported"));
            assert!(std::sync::Arc::ptr_eq(&before, &app.session.doc().unwrap().doc));
            assert_eq!(app.session.doc().unwrap().selection, selection);
            assert_eq!(app.session.doc().unwrap().history.undo.len(), history);
        }
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "change", "value": "blue"}));
        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "story", "value": sid.0}));
        scope_click(&mut app, &ctx, screen, "Count");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "3 match(es)");
        scope_click(&mut app, &ctx, screen, "Change All");
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("status"), "3 replacement(s) made");
        assert_eq!(app.session.doc().unwrap().doc.text_story(sid, child.cell).unwrap().text, "red child");
        assert!(app.session.doc().unwrap().doc.story(sid).unwrap().text.starts_with("blue blue | blue"));
    }

    #[test]
    fn find_scope_dialog_error_confirmation_and_escape_release_context() {
        for escape in [false, true] {
            let (mut app, ctx, screen, sid) = scope_dialog();
            scope_click(&mut app, &ctx, screen, "Find Next");
            scope_selected(&app, sid, 0, 3);
            if escape {
                dialog_frame(
                    &mut app,
                    &ctx,
                    screen,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            } else {
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "find", "value": "["}));
                scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "grep", "value": true}));
                scope_click(&mut app, &ctx, screen, "OK");
            }
            assert!(app.ui.dialog.is_none());
            assert_eq!(app.session.doc().unwrap().doc.story(sid).unwrap().text, "red red | red");
            assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn find_scope_dialog_domain_and_object_changes_release_original_range() {
        for route in ["scope", "object", "menu", "command"] {
            let (mut app, ctx, screen, _) = scope_dialog();
            scope_click(&mut app, &ctx, screen, "Find Next");
            match route {
                "scope" => {
                    for scope in ["story", "selection"] {
                        scope_control(&mut app, &ctx, "ui.dialog.set", json!({"field": "scope", "value": scope}));
                    }
                }
                "object" => {
                    scope_click(&mut app, &ctx, screen, "Object");
                    scope_click(&mut app, &ctx, screen, "Text");
                }
                "menu" => crate::menus::activate(&mut app, "app.newDocumentDialog", &Value::Null),
                "command" => {
                    app.run("app.newDocumentDialog", json!({})).unwrap();
                }
                _ => panic!("known reset route"),
            }
            assert_eq!(app.run("find.find", json!({"find": "red", "scope": "selection"})).unwrap().as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn dialog_title_and_actions_fit_scaled_viewports() {
        for language in ["en", "ar"] {
            for size in [egui::vec2(1178.0, 814.0), egui::vec2(589.0, 407.0), egui::vec2(392.0, 271.0), egui::vec2(450.0, 280.0)] {
                for (command, title, ok, cancel) in
                    [("app.preferences", "Preferences", "OK", "Cancel"), ("app.newDocumentDialog", "New Document", "Create", "Close")]
                {
                    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
                    app.ui.language = language.into();
                    app.run("file.new", json!({})).unwrap();
                    app.run(command, json!({})).unwrap();
                    let ctx = dialog_context(language);
                    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                    for _ in 0..3 {
                        dialog_frame(&mut app, &ctx, screen, vec![]);
                    }
                    let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
                    for expected in [title, ok, cancel] {
                        visible_label(&labels, screen, crate::i18n::tr(language, expected));
                    }
                }
            }
        }
    }

    #[test]
    fn preferences_first_setting_switches_interface_language() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.preferences", json!({})).unwrap();
        let ctx = dialog_context("");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1178.0, 814.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let heading = visible_label(&labels, screen, "Interface Language");
        let combo = visible_label(&labels, screen, "English");
        let scaling = visible_label(&labels, screen, "When Scaling");
        assert!(heading.bottom() <= combo.top() && combo.bottom() <= scaling.top(), "the language comes first: {heading:?} {combo:?} {scaling:?}");
        let pane = labels.iter().filter(|(_, rect, _)| rect.left() >= heading.left() - 1.0 && rect.top() < heading.top());
        assert!(pane.clone().all(|(text, _, _)| text == "Preferences"), "nothing above it: {:?}", pane.collect::<Vec<_>>());

        // Choosing a language applies it at once; the dialog stays open.
        click_dialog(&mut app, &ctx, screen, combo.center());
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for (_, name) in crate::i18n::LANGUAGES {
            assert!(labels.iter().any(|(text, _, _)| text == name), "{name} is listed by its own name");
        }
        let deutsch = visible_label(&labels, screen, "Deutsch").center();
        click_dialog(&mut app, &ctx, screen, deutsch);
        assert_eq!(app.ui.language, "de");
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let heading = visible_label(&labels, screen, crate::i18n::tr("de", "Interface Language"));
        let combo = visible_label(&labels, screen, "Deutsch");

        // The names stay endonyms in the German interface.
        click_dialog(&mut app, &ctx, screen, combo.center());
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for name in ["English", "Français", "日本語", "Українська"] {
            assert!(labels.iter().any(|(text, _, _)| text == name), "{name}");
        }
        click_dialog(&mut app, &ctx, screen, heading.center());
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        assert!(!labels.iter().any(|(text, _, _)| text == "Français"), "the list closes");

        // The control channel's command and the combo show the same setting.
        app.run("app.language", json!({"lang": "ja"})).unwrap();
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        assert!(labels.iter().any(|(text, _, _)| text == "日本語"));
        assert!(labels.iter().any(|(text, _, _)| text == crate::i18n::tr("ja", "Interface Language")));

        // Cancel puts back the language the dialog opened with; OK keeps the choice.
        let cancel_button = visible_label(&labels, screen, crate::i18n::tr("ja", "Cancel")).center();
        click_dialog(&mut app, &ctx, screen, cancel_button);
        assert!(app.ui.dialog.is_none());
        assert_eq!(app.ui.language, "");
        app.run("app.preferences", json!({})).unwrap();
        app.run("app.language", json!({"lang": "it"})).unwrap();
        confirm(&mut app).unwrap();
        assert_eq!(app.ui.language, "it");
        app.run("app.preferences", json!({})).unwrap();
        app.run("app.language", json!({"lang": "uk"})).unwrap();
        cancel(&mut app);
        assert_eq!(app.ui.language, "it");
    }

    #[test]
    fn preferences_sidebar_scroll_keeps_selected_options_visible() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("app.preferences", json!({})).unwrap();
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(589.0, 407.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let sidebar = visible_label(&labels, screen, "General").center();
        assert!(
            !labels.iter().any(|(text, rect, clip)| text == "File Handling" && clip.contains_rect(*rect)),
            "last section starts below the sidebar"
        );
        scroll_dialog(&mut app, &ctx, screen, sidebar, egui::vec2(0.0, -600.0));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let files = visible_label(&labels, screen, "File Handling").center();
        click_dialog(&mut app, &ctx, screen, files);
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("section"), "files");
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for expected in ["Document Recovery Data", "Save recovery data every:", "OK", "Cancel"] {
            visible_label(&labels, screen, expected);
        }
    }

    #[test]
    fn preferences_horizontal_scroll_reaches_clipped_options() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.preferences", json!({})).unwrap();
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(450.0, 280.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let body = visible_label(&labels, screen, "When Scaling").center();
        // The last General option sits below this short viewport.
        scroll_dialog(&mut app, &ctx, screen, body, egui::vec2(0.0, -600.0));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let option = "Absolute Numbering (instead of Section Numbering)";
        let (_, rect, clip) = labels.iter().find(|(text, _, _)| text == option).unwrap();
        assert!(rect.right() > clip.right(), "rightmost option starts horizontally clipped: {rect:?}, {clip:?}");
        scroll_dialog(&mut app, &ctx, screen, body, egui::vec2(-600.0, 0.0));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for expected in [option, "Preferences", "OK", "Cancel"] {
            visible_label(&labels, screen, expected);
        }
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, option).center());
        assert!(app.ui.dialog.as_ref().unwrap().b("absolutePageNumbers"));
    }

    /// Scroll the dialog body down at `at` until `text` is painted unclipped on screen.
    fn scroll_to_label(app: &mut DesignApp, ctx: &egui::Context, screen: egui::Rect, at: egui::Pos2, text: &str) -> egui::Rect {
        for _ in 0..40 {
            let labels = dialog_frame(app, ctx, screen, vec![]);
            if let Some((_, rect, _)) = labels.iter().find(|(t, r, clip)| t.trim() == text && screen.contains_rect(*r) && clip.contains_rect(*r)) {
                return *rect;
            }
            scroll_dialog(app, ctx, screen, at, egui::vec2(0.0, -60.0));
        }
        panic!("{text} never scrolled into view");
    }

    #[test]
    fn new_document_vertical_scroll_reaches_every_field_and_preset() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.newDocumentDialog", json!({})).unwrap();
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(392.0, 271.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let body = visible_label(&labels, screen, "Width").center();
        let shown =
            |labels: &[(String, egui::Rect, egui::Rect)]| labels.iter().any(|(t, r, clip)| t == "Primary Text Frame" && clip.contains_rect(*r));
        assert!(!shown(&labels), "the details start vertically clipped");
        let ptf = scroll_to_label(&mut app, &ctx, screen, body, "Primary Text Frame");
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for expected in ["New Document", "Create", "Close"] {
            visible_label(&labels, screen, expected);
        }
        click_dialog(&mut app, &ctx, screen, ptf.center());
        assert!(app.ui.dialog.as_ref().unwrap().b("primaryTextFrame"), "the caption toggles its checkbox");
        // Below the details: the preset tabs and cards.
        scroll_to_label(&mut app, &ctx, screen, body, "[Default]");
    }

    #[test]
    fn arabic_new_document_fields_remain_reachable_and_editable() {
        for size in [egui::vec2(392.0, 271.0), egui::vec2(450.0, 280.0), egui::vec2(1178.0, 814.0)] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.ui.language = "ar".into();
            app.run("app.newDocumentDialog", json!({})).unwrap();
            let fields = [("height", "66p1"), ("gutter", "1p2"), ("marginBottom", "3p3"), ("marginOutside", "3p4")];
            for (field, value) in fields {
                app.ui.dialog.as_mut().unwrap().fields.insert(field.into(), json!(value));
            }
            let ctx = dialog_context("ar");
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for _ in 0..3 {
                dialog_frame(&mut app, &ctx, screen, vec![]);
            }
            let height = visible_label(&dialog_frame(&mut app, &ctx, screen, vec![]), screen, "66p1");
            click_dialog(&mut app, &ctx, screen, height.center());
            dialog_frame(
                &mut app,
                &ctx,
                screen,
                vec![
                    egui::Event::Key {
                        key: egui::Key::A,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers { ctrl: true, command: true, ..Default::default() },
                    },
                    egui::Event::Text("88p8".into()),
                ],
            );
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("height"), "88p8", "numeric fields accept edits");
            let caption = scroll_to_label(&mut app, &ctx, screen, height.center(), crate::i18n::tr("ar", "Primary Text Frame"));
            click_dialog(&mut app, &ctx, screen, caption.center());
            assert!(app.ui.dialog.as_ref().unwrap().b("primaryTextFrame"), "checkboxes remain clickable");
            for (_, value) in &fields[1..] {
                scroll_to_label(&mut app, &ctx, screen, height.center(), value);
            }
        }
    }

    #[test]
    fn resized_dialogs_cancel_and_reopen_without_applying_edits() {
        for (command, title, field, ok, cancel) in [
            ("app.preferences", "Preferences", "recoveryMinutes", "OK", "Cancel"),
            ("app.newDocumentDialog", "New Document", "pages", "Create", "Close"),
        ] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run(command, json!({})).unwrap();
            let original = app.ui.dialog.as_ref().unwrap().fields[field].clone();
            app.ui.dialog.as_mut().unwrap().fields.insert(field.into(), json!(42));
            let ctx = dialog_context("en");
            for size in [egui::vec2(1178.0, 814.0), egui::vec2(392.0, 271.0), egui::vec2(589.0, 407.0)] {
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for _ in 0..3 {
                    dialog_frame(&mut app, &ctx, screen, vec![]);
                }
                let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
                for expected in [title, ok, cancel] {
                    visible_label(&labels, screen, expected);
                }
            }
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(589.0, 407.0));
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, cancel).center());
            assert!(app.ui.dialog.is_none());
            assert!(app.session.active().is_none(), "cancelling must not create a document");
            app.run(command, json!({})).unwrap();
            assert_eq!(app.ui.dialog.as_ref().unwrap().fields[field], original, "cancelled edits must not survive reopening");
            for _ in 0..3 {
                dialog_frame(&mut app, &ctx, screen, vec![]);
            }
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            visible_label(&labels, screen, title);
            dialog_frame(
                &mut app,
                &ctx,
                screen,
                vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
            assert!(app.ui.dialog.is_none(), "Escape must also close the reopened dialog");
        }
    }

    #[test]
    fn paragraph_style_options_tabs_section_adds_a_stop_on_its_ruler() {
        use egui_kittest::kittest::Queryable;
        let (app, _) = ruled_paragraph();
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 900.0));
        h.state_mut().app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Ruled Child", "section": "tabs"})));
        h.run_steps(4);
        h.get_by_label("Clear All");
        // The ruler sits below the row of alignment buttons; its strip takes the click.
        let b = h.get_by_label("Right-aligned tab").rect();
        h.get_by_label("Right-aligned tab").click();
        h.run_steps(2);
        let left = h.get_by_label("Left-aligned tab").rect();
        let at = egui::pos2(left.min.x + crate::panels::tabs::PAD + 100.0, b.max.y + 14.0);
        crate::test_window::click_at(&mut h, at);
        let d = h.state().app.ui.dialog.clone().unwrap();
        let tabs = d.fields.get("p.tabs").cloned().unwrap_or_default();
        assert_eq!(tabs.as_array().map(Vec::len), Some(1), "{tabs}");
        assert_eq!(tabs[0]["align"], "right");
        confirm(&mut h.state_mut().app).unwrap();
        let app = &mut h.state_mut().app;
        let style = app.session.execute("type.tabs.get", &json!({"style": "Ruled Child"})).unwrap();
        assert_eq!(style["tabs"], tabs);
        assert_eq!(app.session.execute("type.tabs.get", &json!({})).unwrap()["tabs"], tabs, "the paragraph follows its style");
    }

    #[test]
    fn parses_command_params() {
        let f = command_fields("{name, type: custom|lastPageNumber|chapterNumber, text?, rule?: {on, weight}, flag?: bool} — creates");
        let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["name", "type", "text", "rule", "flag"]);
        assert_eq!(f[1].choices, ["custom", "lastPageNumber", "chapterNumber"]);
        assert!(!f[0].optional && f[2].optional);
        assert!(f[4].boolean);
        assert!(command_fields("{}").is_empty());
        assert!(command_fields("no params").is_empty());
        // A parenthetical after the key describes the value.
        let f = command_fields("{angle (degrees, CCW), ids?}");
        let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["angle", "ids"]);
        assert!(!f[0].optional && f[1].optional);
        assert_eq!(f[0].hint, "degrees, CCW");
        let f = command_fields("{scaleX? (%), ref?: 0..8, ids?: parent items (default: all)}");
        let keys: Vec<&str> = f.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["scaleX", "ref", "ids"]);
        assert!(f.iter().all(|f| f.optional));
        assert_eq!(f[0].hint, "%");
        assert_eq!(f[2].hint, "parent items (default: all)");
        // Every command with a "…" label parses without panicking.
        for c in designcraft_engine::command_specs() {
            let _ = command_fields(c.params);
        }
    }

    fn test_app() -> crate::DesignApp {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({"pages": 1})).unwrap();
        app
    }

    #[test]
    fn toc_dialog_keeps_the_character_styles_and_sorting_of_its_entries() {
        let mut app = test_app();
        app.session.execute("style.paragraph.create", &json!({"name": "Head"})).unwrap();
        app.session.execute("style.character.create", &json!({"name": "Number"})).unwrap();
        app.session.execute("style.character.create", &json!({"name": "Dots"})).unwrap();
        let sid =
            app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Pears\nApples"})).unwrap()["story"]
                .clone();
        app.session.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 8})).unwrap();
        app.session.execute("style.paragraph.apply", &json!({"name": "Head"})).unwrap();
        app.session.execute("layout.pages.insert", &json!({"count": 1})).unwrap();
        let entries = json!([{"style": "Head", "pageNumberStyle": "Number", "separatorStyle": "Dots", "sort": true}]);
        app.session.execute("toc.generate", &json!({"page": 2, "entries": entries})).unwrap();
        let before = app.session.active().unwrap().doc.toc.clone().unwrap();
        // The dialog shows the options, and OK (Replace Existing) writes them back unchanged.
        open_toc(&mut app);
        let d = app.ui.dialog.as_ref().unwrap();
        let row = &toc_rows(d)[0];
        assert_eq!((row.page_number_style.as_str(), row.separator_style.as_str(), row.sort), ("Number", "Dots", true));
        let chars: Vec<String> = serde_json::from_value(d.fields["charStyles"].clone()).unwrap();
        assert_eq!(chars, ["Number", "Dots"], "[None] is the menu's first choice, not a style");
        let p = toc_params(&app, d);
        assert_eq!(
            (&p["entries"][0]["pageNumberStyle"], &p["entries"][0]["separatorStyle"], &p["entries"][0]["sort"]),
            (&json!("Number"), &json!("Dots"), &json!(true))
        );
        confirm(&mut app).expect("OK regenerates the table of contents");
        let st = app.session.active().unwrap();
        assert_eq!(st.doc.toc.as_ref().unwrap().entries, before.entries);
        // [None] and unsorted are left out of the params.
        let mut d = Dialog::new(TOC, json!({"included": [{"style": "Head", "level": 1, "separator": "^t"}]}));
        d.fields.insert("replace".into(), json!(true));
        let p = toc_params(&app, &d);
        assert!(p["entries"][0].get("pageNumberStyle").is_none() && p["entries"][0].get("sort").is_none(), "{p}");
    }

    #[test]
    fn paragraph_style_options_dialog_saves_edits() {
        let mut app = test_app();
        let name = app.session.execute("style.paragraph.create", &json!({"name": "Body Copy"})).unwrap()["name"].as_str().unwrap().to_string();
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": name, "p.align": "center"})));
        confirm(&mut app).expect("OK saves the style edit");
        let st = app.session.active().unwrap();
        assert_eq!(st.doc.styles.para(&name).unwrap().para.align, Some(designcraft_doc::Align::Center));
    }

    #[test]
    fn delete_paragraph_style_dialog_removes_the_style_and_reassigns_text() {
        let mut app = test_app();
        let name = app.session.execute("style.paragraph.create", &json!({"name": "Caption"})).unwrap()["name"].as_str().unwrap().to_string();
        let sid = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap()["story"].as_u64().unwrap();
        app.session.execute("text.insert", &json!({"text": "Hello"})).unwrap();
        app.session.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        app.session.execute("style.paragraph.apply", &json!({"name": name})).unwrap();
        app.ui.dialog = Some(Dialog::new("deleteParagraphStyle", json!({"name": name, "replaceWith": designcraft_doc::BASIC_PARAGRAPH})));
        confirm(&mut app).expect("OK deletes the style");
        let st = app.session.active().unwrap();
        assert!(st.doc.styles.para(&name).is_none(), "style removed");
        let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).unwrap();
        assert_eq!(story.paras[0].style, designcraft_doc::BASIC_PARAGRAPH, "text reassigned to the replacement style");
    }

    #[test]
    fn delete_paragraph_style_dialog_defaults_replacement_to_basic_paragraph() {
        let mut app = test_app();
        let name = app.session.execute("style.paragraph.create", &json!({"name": "Caption"})).unwrap()["name"].as_str().unwrap().to_string();
        // No `replaceWith` set — the dialog's default field.
        app.ui.dialog = Some(Dialog::new("deleteParagraphStyle", json!({"name": name})));
        confirm(&mut app).expect("OK deletes the style with the default replacement");
        let st = app.session.active().unwrap();
        assert!(st.doc.styles.para(&name).is_none());
    }

    #[test]
    fn character_style_options_dialog_saves_edits() {
        let mut app = test_app();
        let name = app.session.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
        app.ui.dialog = Some(Dialog::new("characterStyleOptions", json!({"name": name, "c.tracking": 20.0})));
        confirm(&mut app).expect("OK saves the style edit");
        let st = app.session.active().unwrap();
        assert_eq!(st.doc.styles.char_style(&name).unwrap().chars.tracking, Some(20.0));
    }

    #[test]
    fn delete_character_style_dialog_removes_the_style_and_reassigns_text() {
        let mut app = test_app();
        let name = app.session.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
        let sid = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap()["story"].as_u64().unwrap();
        app.session.execute("text.insert", &json!({"text": "Hello"})).unwrap();
        app.session.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 5})).unwrap();
        app.session.execute("style.character.apply", &json!({"name": name})).unwrap();
        app.ui.dialog = Some(Dialog::new("deleteCharacterStyle", json!({"name": name, "replaceWith": designcraft_doc::NO_CHAR_STYLE})));
        confirm(&mut app).expect("OK deletes the style");
        let st = app.session.active().unwrap();
        assert!(st.doc.styles.char_style(&name).is_none(), "style removed");
        let story = st.doc.stories.get(&designcraft_doc::StoryId(sid)).unwrap();
        assert_eq!(story.chars[0].format.style, designcraft_doc::NO_CHAR_STYLE, "text reassigned to the replacement style");
    }

    #[test]
    fn delete_character_style_dialog_defaults_replacement_to_none() {
        let mut app = test_app();
        let name = app.session.execute("style.character.create", &json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
        // No `replaceWith` set — the dialog's default field.
        app.ui.dialog = Some(Dialog::new("deleteCharacterStyle", json!({"name": name})));
        confirm(&mut app).expect("OK deletes the style with the default replacement");
        let st = app.session.active().unwrap();
        assert!(st.doc.styles.char_style(&name).is_none());
    }

    #[test]
    fn pdf_export_defaults() {
        let d = Dialog::new("pdfExport", json!({}));
        let keys: Vec<&str> = d.fields.iter().map(|(k, _)| k.as_str()).collect();
        // serde_json::Map keeps keys sorted.
        let expected = [
            "bleed",
            "compressImages",
            "flatten",
            "marksBleed",
            "marksCrop",
            "marksOffset",
            "marksPageInfo",
            "marksWeight",
            "preset",
            "spreads",
            "standard",
            "tagged",
        ];
        assert_eq!(keys, expected);
        assert_eq!(d.fields["preset"].as_str(), Some("Desktop Printing"));
        assert_eq!(d.fields["standard"].as_str(), Some("none"));
        assert_eq!(d.fields["compressImages"].as_bool(), Some(false));
        assert_eq!(d.fields["flatten"].as_str(), Some(""));
        assert_eq!(d.fields["spreads"].as_bool(), Some(false));
        assert_eq!(d.fields["bleed"].as_bool(), Some(true));
        assert_eq!(d.fields["marksCrop"].as_bool(), Some(false));
        assert_eq!(d.fields["marksBleed"].as_bool(), Some(false));
        assert_eq!(d.fields["marksPageInfo"].as_bool(), Some(false));
        assert_eq!(d.fields["marksWeight"].as_str(), Some("0.25"));
        assert_eq!(d.fields["marksOffset"].as_str(), Some("6"));
        assert_eq!(d.fields["tagged"].as_bool(), Some(true));
    }

    #[test]
    fn pdf_export_override_keeps_override() {
        let d = Dialog::new("pdfExport", json!({"bleed": false}));
        assert_eq!(d.fields["bleed"].as_bool(), Some(false));
        assert_eq!(d.fields["preset"].as_str(), Some("Desktop Printing"));
        assert_eq!(d.fields["tagged"].as_bool(), Some(true));
    }

    #[test]
    fn pdf_export_preset_high_quality_print_keeps_flatten() {
        let mut d = Dialog::new("pdfExport", json!({}));
        d.fields.insert("flatten".into(), json!("medium"));
        apply_pdf_preset(&mut d, "Desktop Printing");
        assert_eq!(d.s("standard"), "none");
        assert!(!d.b("compressImages"));
        assert!(!d.b("marksCrop"));
        assert!(!d.b("marksBleed"));
        assert!(!d.b("marksPageInfo"));
        assert_eq!(d.s("flatten"), "medium", "a leave-unchanged preset must not touch flatten");
    }

    #[test]
    fn pdf_export_preset_press_quality_keeps_flatten() {
        let mut d = Dialog::new("pdfExport", json!({}));
        d.fields.insert("flatten".into(), json!("high"));
        apply_pdf_preset(&mut d, "Commercial Printing");
        assert_eq!(d.s("standard"), "none");
        assert!(d.b("compressImages"));
        assert!(d.b("marksCrop"));
        assert!(d.b("marksBleed"));
        assert!(!d.b("marksPageInfo"));
        assert_eq!(d.s("flatten"), "high");
    }

    #[test]
    fn pdf_export_preset_smallest_file_size_sets_flatten_low() {
        let mut d = Dialog::new("pdfExport", json!({}));
        apply_pdf_preset(&mut d, "Screen and Email");
        assert_eq!(d.s("standard"), "none");
        assert!(d.b("compressImages"));
        assert!(!d.b("marksCrop"));
        assert!(!d.b("marksBleed"));
        assert!(!d.b("marksPageInfo"));
        assert_eq!(d.s("flatten"), "low");
    }

    #[test]
    fn pdf_export_preset_pdf_x4_keeps_flatten() {
        let mut d = Dialog::new("pdfExport", json!({}));
        d.fields.insert("flatten".into(), json!("medium"));
        apply_pdf_preset(&mut d, "PDF/X-4");
        assert_eq!(d.s("standard"), "x4");
        assert!(!d.b("compressImages"));
        assert!(d.b("marksCrop"));
        assert!(d.b("marksBleed"));
        assert!(!d.b("marksPageInfo"));
        assert_eq!(d.s("flatten"), "medium");
    }

    #[test]
    fn pdf_export_preset_pdf_a2b_keeps_flatten() {
        let mut d = Dialog::new("pdfExport", json!({}));
        d.fields.insert("flatten".into(), json!("low"));
        apply_pdf_preset(&mut d, "PDF/A-2b");
        assert_eq!(d.s("standard"), "a2b");
        assert!(!d.b("compressImages"));
        assert!(!d.b("marksCrop"));
        assert!(!d.b("marksBleed"));
        assert!(!d.b("marksPageInfo"));
        assert_eq!(d.s("flatten"), "low");
    }

    #[test]
    fn open_pdf_export_seeds_fields_from_settings() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.pdf_export.standard = "x4".into();
        app.ui.pdf_export.tagged = false;
        app.ui.pdf_export.marks_crop = true;
        app.ui.pdf_export.flatten = String::new();
        app.ui.flattener = "medium".into();
        open_pdf_export(&mut app);
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.id, "pdfExport");
        assert_eq!(d.s("standard"), "x4");
        assert!(!d.b("tagged"));
        assert!(d.b("marksCrop"));
        assert_eq!(d.s("pages"), "all");
        assert_eq!(d.s("flatten"), "medium", "empty flatten falls back to the global flattener");
        assert_eq!(d.s("title"), "", "no document → empty title");

        // A non-empty persisted flatten is used verbatim, not the global fallback.
        app.ui.pdf_export.flatten = "low".into();
        app.ui.flattener = "medium".into();
        open_pdf_export(&mut app);
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.s("flatten"), "low");
    }

    #[test]
    fn pdf_export_preset_custom_changes_nothing() {
        let mut d = Dialog::new("pdfExport", json!({}));
        d.fields.insert("standard".into(), json!("x4"));
        d.fields.insert("compressImages".into(), json!(true));
        d.fields.insert("marksCrop".into(), json!(true));
        d.fields.insert("marksBleed".into(), json!(true));
        d.fields.insert("marksPageInfo".into(), json!(true));
        d.fields.insert("flatten".into(), json!("high"));
        let before = d.fields.clone();
        apply_pdf_preset(&mut d, "Custom");
        assert_eq!(d.fields, before);
    }

    #[test]
    fn confirm_pdf_export_writes_file_and_persists_settings() {
        use std::sync::{Arc, Mutex};
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let writes: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = writes.clone();
        app.services.write = Some(Box::new(move |path: &str, bytes: &[u8]| {
            sink.lock().unwrap().push((path.to_string(), bytes.to_vec()));
            Ok(())
        }));
        app.services.pick_save = Some(Box::new(|_name: &str| Some("/tmp/designcraft-test.pdf".to_string())));

        app.ui.dialog = Some(Dialog::new("pdfExport", json!({})));
        {
            let d = app.ui.dialog.as_mut().unwrap();
            d.fields.insert("standard".into(), json!("x4"));
            d.fields.insert("bleed".into(), json!(false));
            d.fields.insert("marksCrop".into(), json!(true));
            d.fields.insert("tagged".into(), json!(false));
            d.fields.insert("title".into(), json!("Report"));
        }
        confirm(&mut app).unwrap();

        let got = writes.lock().unwrap();
        assert_eq!(got.len(), 1, "confirming the dialog writes exactly one file");
        assert_eq!(got[0].0, "/tmp/designcraft-test.pdf");
        assert!(got[0].1.starts_with(b"%PDF"), "the written bytes are a PDF");
        drop(got);

        assert_eq!(app.ui.pdf_export.standard, "x4");
        assert!(!app.ui.pdf_export.bleed);
        assert!(app.ui.pdf_export.marks_crop);
        assert!(!app.ui.pdf_export.tagged);
    }

    #[test]
    fn update_link_asks_before_replacing_edits_made_in_the_layout() {
        let dir = std::env::temp_dir().join(format!("dc-update-link-dialog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, "One").unwrap();
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let r = app.session.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true, "rect": [72, 72, 300, 200]})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        let text = |app: &crate::DesignApp| app.session.doc().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().text.clone();

        // Not edited in the layout: updates without asking.
        std::fs::write(&path, "Two").unwrap();
        crate::panels::properties::update_links(&mut app, json!({"story": sid}));
        assert!(app.ui.dialog.is_none());
        assert_eq!(text(&app), "Two");

        // Edited in the layout: asks first; Cancel changes nothing.
        app.session.execute("story.setText", &json!({"story": sid, "text": "Mine"})).unwrap();
        std::fs::write(&path, "Three").unwrap();
        crate::panels::properties::update_links(&mut app, json!({"story": sid}));
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.id.as_str(), d.s("files").as_str()), ("updateLink", "note.txt"));
        app.ui.dialog = None;
        assert_eq!(text(&app), "Mine");

        // Window › Links › Update Link (all links) asks too; confirming updates.
        crate::menus::activate(&mut app, "links.update", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("updateLink"));
        confirm(&mut app).unwrap();
        assert!(app.ui.dialog.is_none());
        assert_eq!(text(&app), "Three");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn confirm_pdf_export_cancelled_save_writes_nothing() {
        use std::sync::{Arc, Mutex};
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let writes: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = writes.clone();
        app.services.write = Some(Box::new(move |path: &str, bytes: &[u8]| {
            sink.lock().unwrap().push((path.to_string(), bytes.to_vec()));
            Ok(())
        }));
        app.services.pick_save = Some(Box::new(|_name: &str| None));

        app.ui.dialog = Some(Dialog::new("pdfExport", json!({})));
        let r = confirm(&mut app).unwrap();
        assert_eq!(r, Value::Null);
        assert!(writes.lock().unwrap().is_empty(), "cancelling the save picker must not write");
    }

    use designcraft_doc::{CharAttrs, CharacterStyle};

    /// A document with character styles Base (bold) and Emphasis (italic) on the first two letters.
    fn styled_app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("style.character.create", json!({"name": "Base", "chars": {"fontStyle": "Bold"}})).unwrap();
        app.run("style.character.create", json!({"name": "Emphasis", "chars": {"fontStyle": "Italic"}})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Hi there"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": 2})).unwrap();
        app.run("style.character.apply", json!({"name": "Emphasis"})).unwrap();
        app
    }

    /// One control-channel request, as the control port or MCP would send it.
    fn ctl(app: &mut DesignApp, method: &str, params: Value) -> Value {
        let (req, _reply) = crate::control::ControlRequest::new(method, params);
        match crate::control::handle(app, &egui::Context::default(), &req) {
            crate::control::Outcome::Done(v) => v,
            crate::control::Outcome::Screenshot { .. } => Value::Null,
        }
    }

    fn char_style(app: &DesignApp, name: &str) -> Option<CharacterStyle> {
        app.session.active().and_then(|st| st.doc.styles.char_style(name).cloned())
    }

    #[test]
    fn character_style_options_set_exactly_the_edited_attributes() {
        let mut app = styled_app();
        assert_eq!(ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Emphasis"}}))["ok"], true);
        ctl(&mut app, "ui.dialog.set", json!({"field": "c.size", "value": 14}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "c.fill", "value": "[Paper]"}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "basedOn", "value": "Base"}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        assert!(app.ui.dialog.is_none());
        let st = char_style(&app, "Emphasis").unwrap();
        assert_eq!(st.based_on.as_deref(), Some("Base"));
        assert_eq!(st.chars, CharAttrs { font_style: Some("Italic".into()), size: Some(14.0), fill: Some("[Paper]".into()), ..Default::default() });
    }

    #[test]
    fn character_style_options_unset_cleared_fields_and_rename_every_use() {
        let mut app = styled_app();
        ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Emphasis"}}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "c.fontStyle", "value": null}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "rename", "value": "Strong"}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        assert!(char_style(&app, "Emphasis").is_none());
        assert!(char_style(&app, "Strong").unwrap().chars.is_empty());
        let st = app.session.active().unwrap();
        let story = st.doc.stories.values().next().unwrap();
        assert!(story.chars.iter().any(|r| r.format.style == "Strong"));
        // A taken name keeps the dialog open with the reason.
        ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Strong"}}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "rename", "value": "Base"}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], false);
        assert!(app.ui.dialog.as_ref().is_some_and(|d| !d.s("status").is_empty()));
        assert!(char_style(&app, "Strong").is_some());
    }

    #[test]
    fn new_character_style_opens_its_options_and_creates_the_style() {
        let mut app = styled_app();
        crate::menus::activate(&mut app, "style.character.create", &Value::Null);
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!((d.id.as_str(), d.b("new"), d.s("rename").as_str()), ("characterStyleOptions", true, "Character Style 1"));
        ctl(&mut app, "ui.dialog.set", json!({"field": "c.size", "value": 9}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        let st = char_style(&app, "Character Style 1").unwrap();
        assert_eq!((st.based_on, st.chars), (None, CharAttrs { size: Some(9.0), ..Default::default() }));
        crate::menus::activate(&mut app, "style.character.create", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("rename"), "Character Style 2");
    }

    #[test]
    fn new_paragraph_style_opens_its_options_and_creates_the_style() {
        let mut app = styled_app();
        crate::menus::activate(&mut app, "style.paragraph.create", &Value::Null);
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!((d.id.as_str(), d.b("new"), d.s("rename").as_str()), ("paragraphStyleOptions", true, "Paragraph Style 1"));
        ctl(&mut app, "ui.dialog.set", json!({"field": "c.size", "value": 20}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "basedOn", "value": designcraft_doc::BASIC_PARAGRAPH}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        let st = app.session.active().unwrap().doc.styles.para("Paragraph Style 1").cloned().unwrap();
        assert_eq!(st.based_on.as_deref(), Some(designcraft_doc::BASIC_PARAGRAPH));
        assert_eq!(st.chars, CharAttrs { size: Some(20.0), ..Default::default() });
    }

    #[test]
    fn style_rows_open_their_options_except_none() {
        let mut app = styled_app();
        open_style_options(&mut app, false, "Emphasis");
        let d = app.ui.dialog.take().unwrap();
        assert_eq!((d.id.as_str(), d.s("name").as_str()), ("characterStyleOptions", "Emphasis"));
        open_style_options(&mut app, false, designcraft_doc::NO_CHAR_STYLE);
        assert!(app.ui.dialog.is_none());
        open_style_options(&mut app, true, designcraft_doc::BASIC_PARAGRAPH);
        assert_eq!(app.ui.dialog.take().unwrap().id, "paragraphStyleOptions");
    }

    #[test]
    fn style_options_sections_draw() {
        let mut app = styled_app();
        let ctx = egui::Context::default();
        let frame = |app: &mut DesignApp| {
            for _ in 0..2 {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    app.logic(&ui.ctx().clone());
                    app.ui(ui);
                });
                out.textures_delta.clear();
            }
        };
        for lang in ["", "ar"] {
            app.run("app.language", json!({"lang": lang})).unwrap();
            for fields in [json!({"name": "Emphasis"}), json!({"new": true}), json!({"name": "Emphasis", "c.size": null, "c.underline": false})] {
                for section in ["general", "chars", "advanced", "color", "openType", "underline", "strikethrough"] {
                    let mut f = fields.clone();
                    f["section"] = json!(section);
                    app.ui.dialog = Some(Dialog::new("characterStyleOptions", f));
                    frame(&mut app);
                    assert!(app.ui.dialog.is_some(), "{section} stays open");
                }
            }
            for section in
                ["general", "chars", "advanced", "border", "shading", "keep", "span", "bullets", "color", "openType", "underline", "strikethrough"]
            {
                for fields in [
                    json!({"new": true}),
                    json!({"new": true, "p.listType": "bullets", "p.spanColumns": {"kind": "span", "columns": 0}, "p.borderOn": true}),
                    json!({"new": true, "p.listType": "numbers", "p.startAt": 1, "p.spanColumns": {"kind": "split", "columns": 2}, "p.keepLinesTogether": true}),
                ] {
                    let mut f = fields;
                    f["section"] = json!(section);
                    app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", f));
                    frame(&mut app);
                    assert!(app.ui.dialog.is_some(), "{section} stays open");
                }
            }
        }
    }

    #[test]
    fn shortcut_recorder_waits_for_the_key_after_its_modifiers() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mut d = Dialog::new("keyboardShortcuts", json!({"query": "", "recording": "app.palette"}));
        let ctx = egui::Context::default();
        let key = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        // Holding Ctrl reports Ctrl itself as a key press, then Ctrl+K arrives.
        for event in [key(egui::Key::ControlLeft, egui::Modifiers::COMMAND), key(egui::Key::K, egui::Modifiers::COMMAND)] {
            let input = egui::RawInput { events: vec![event], ..Default::default() };
            ctx.run_ui(input, |ui| keyboard_shortcuts(&mut app, ui, &mut d)).textures_delta.clear();
        }
        assert_eq!(crate::menus::shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+K"));
        assert_eq!(d.s("recording"), "", "recording ends with the recorded shortcut");
    }

    #[test]
    fn dragging_the_title_moves_the_dialog_and_dragging_the_body_does_not() {
        use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
        use egui_kittest::{Harness, kittest::Queryable};
        fn drag(h: &mut Harness<'static, DesignApp>, from: Pos2, to: Pos2) {
            h.event(Event::PointerMoved(from));
            h.run_steps(1);
            h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
            h.run_steps(1);
            for step in 1..=8 {
                h.event(Event::PointerMoved(from + (to - from) * (step as f32 / 8.0)));
                h.run_steps(1);
            }
            h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            h.run_steps(3);
        }
        let rect = |h: &Harness<'static, DesignApp>| h.ctx.memory(|m| m.area_rect(egui::Id::new("dialog"))).unwrap();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.ui.dialog = Some(Dialog::new("insertTable", Value::Null));
        let mut ready = false;
        let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(crate::theme::Brightness::default()));
                    ready = true;
                    return;
                }
                show(app, ui.ctx());
            },
            app,
        );
        h.run_steps(4);
        let centred = rect(&h);
        assert!((centred.center() - egui::pos2(600.0, 400.0)).length() < 1.0, "{centred:?}");
        assert!(centred.width() < 1000.0 && centred.height() < 700.0, "the area is the dialog, not the backdrop: {centred:?}");

        let title = h.get_by_label("Create Table").rect().center();
        drag(&mut h, title, title + vec2(100.0, 60.0));
        let moved = rect(&h);
        assert!((moved.min - centred.min - vec2(100.0, 60.0)).length() < 2.0, "{centred:?} -> {moved:?}");

        let body = egui::pos2(moved.right() - 40.0, moved.center().y);
        drag(&mut h, body, body + vec2(-150.0, 80.0));
        assert_eq!(rect(&h), moved, "a drag in the body leaves the dialog in place");

        // The title row stays inside the top edge.
        let title = h.get_by_label("Create Table").rect().center();
        drag(&mut h, title, title - vec2(0.0, 1000.0));
        assert!(rect(&h).top().abs() < 1.0, "{:?}", rect(&h));

        // Opened again, the dialog starts centred.
        h.state_mut().ui.dialog = None;
        h.run_steps(2);
        h.state_mut().ui.dialog = Some(Dialog::new("insertTable", Value::Null));
        h.run_steps(4);
        assert!((rect(&h).center() - centred.center()).length() < 1.0, "{:?}", rect(&h));

        // Past the left edge, about 100 px of the title row stays inside.
        let title = h.get_by_label("Create Table").rect().center();
        drag(&mut h, title, title - vec2(900.0, 0.0));
        assert!((rect(&h).right() - 100.0).abs() < 2.0, "{:?}", rect(&h));
    }

    #[test]
    fn the_new_document_window_drags_mostly_aside_and_keeps_its_preview() {
        use egui::{Event, Modifiers, PointerButton, vec2};
        use egui_kittest::{Harness, kittest::Queryable};
        let rect = |h: &Harness<'static, DesignApp>| h.ctx.memory(|m| m.area_rect(egui::Id::new("dialog"))).unwrap();
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"title": "A"})).unwrap();
        app.ui.new_document_preview = true;
        app.run("app.newDocumentDialog", json!({})).unwrap();
        let mut ready = false;
        let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(crate::theme::Brightness::default()));
                    ready = true;
                    return;
                }
                show(app, ui.ctx());
            },
            app,
        );
        h.run_steps(6);
        let preview = h.state().session.new_document_preview();
        assert!(preview.is_some(), "Preview is on");
        let centred = rect(&h);
        assert!(centred.width() > 900.0, "{centred:?}");
        let from = h.get_all_by_label("New Document").map(|n| n.rect().center()).min_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
        let to = from + vec2(1300.0, 0.0);
        h.event(Event::PointerMoved(from));
        h.run_steps(1);
        h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run_steps(1);
        for step in 1..=8 {
            h.event(Event::PointerMoved(from + (to - from) * (step as f32 / 8.0)));
            h.run_steps(1);
        }
        h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(3);
        // All but 100 px of the window's width is past the right edge, over the preview.
        assert!((rect(&h).left() - 1300.0).abs() < 2.0, "{:?}", rect(&h));
        let app = h.state();
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("newDocument"));
        assert_eq!(app.session.new_document_preview(), preview, "moving the window keeps the preview");
        assert_eq!(app.session.documents().len(), 2);
    }

    #[test]
    fn margins_dialog_reads_bare_numbers_in_the_ruler_unit() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("document.preferences", json!({"horizontalUnits": "millimeters"})).unwrap();
        crate::menus::activate(&mut app, "layout.marginsAndColumns", &Value::Null);
        for k in ["marginTop", "marginBottom", "marginInside", "marginOutside"] {
            ctl(&mut app, "ui.dialog.set", json!({"field": k, "value": "15"}));
        }
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        let top = app.session.active().unwrap().doc.page(0).unwrap().margins.top;
        assert!((top - 15.0 * 72.0 / 25.4).abs() < 0.01, "top margin was {top} pt");
    }
    fn facing_app(pages: u64) -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": pages, "facingPages": true})).unwrap();
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0)));
        app
    }

    fn guide_counts(app: &DesignApp) -> Vec<usize> {
        let doc = &app.session.active().unwrap().doc;
        (0..doc.page_count()).map(|abs| doc.page(abs).unwrap().guides.len()).collect()
    }

    #[test]
    fn create_guides_dialog_targets_the_page_in_view() {
        let mut app = facing_app(6);
        crate::canvas::go_to_page(&mut app, 4);
        crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
        ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "3"}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        assert_eq!(guide_counts(&app), vec![0, 0, 0, 0, 4, 0]);
        // "Both pages of the spread" covers the facing pair.
        crate::canvas::go_to_page(&mut app, 1);
        crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
        ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "2"}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "target", "value": "spread"}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        assert_eq!(guide_counts(&app), vec![0, 2, 2, 0, 4, 0]);
    }

    #[test]
    fn parent_dialogs_cover_both_pages_of_the_parent_spread_by_default() {
        let mut app = facing_app(4);
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        crate::menus::activate(&mut app, "layout.marginsAndColumns", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("target"), "spread");
        for k in ["marginTop", "marginBottom", "marginInside", "marginOutside"] {
            ctl(&mut app, "ui.dialog.set", json!({"field": k, "value": "20 pt"}));
        }
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        crate::menus::activate(&mut app, "layout.createGuides", &Value::Null);
        ctl(&mut app, "ui.dialog.set", json!({"field": "columns", "value": "2"}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        let doc = &app.session.active().unwrap().doc;
        for pg in &doc.parents[0].pages {
            assert_eq!((pg.margins.top, pg.margins.outside, pg.guides.len()), (20.0, 20.0, 2));
        }
        // Every document page shows the parent's guides.
        assert!((0..doc.page_count()).all(|abs| doc.inherited_guides(abs).len() == 2));
    }

    #[test]
    fn other_dialogs_keep_reading_bare_numbers_as_picas() {
        let mut app = facing_app(2);
        app.run("document.preferences", json!({"horizontalUnits": "millimeters"})).unwrap();
        app.ui.dialog = Some(Dialog::new("newDocument", json!({"width": "15"})));
        assert_eq!(app.ui.dialog.as_ref().unwrap().m("width"), Some(180.0));
    }
    #[test]
    fn character_style_options_set_the_digits() {
        let mut app = styled_app();
        ctl(&mut app, "ui.dialog.open", json!({"id": "characterStyleOptions", "fields": {"name": "Emphasis"}}));
        ctl(&mut app, "ui.dialog.set", json!({"field": "c.digits", "value": "farsi"}));
        assert_eq!(ctl(&mut app, "ui.dialog.confirm", json!({}))["ok"], true);
        assert_eq!(char_style(&app, "Emphasis").unwrap().chars.digits, Some(designcraft_doc::Digits::Farsi));
    }
}
