//! Modal dialogs. Each dialog keeps its fields in a JSON map so the control channel can set them
//! (`ui.dialog.set {field, value}`) and confirm them (`ui.dialog.confirm`) like a user would.

use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::theme::semibold;

mod style_cjk;

#[derive(Clone, Debug, Serialize)]
pub struct Dialog {
    pub id: String,
    pub fields: Map<String, Value>,
}

impl Dialog {
    pub fn new(id: &str, params: Value) -> Self {
        let mut fields = match params {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let defaults = match id {
            "newDocument" => {
                json!({"preset": "Letter", "width": "51p0", "height": "66p0", "pages": 1, "facingPages": true, "columns": 1, "gutter": "1p0",
                "marginTop": "3p0", "marginBottom": "3p0", "marginInside": "3p0", "marginOutside": "3p0", "bleed": "0p0", "primaryTextFrame": false})
            }
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
        if id == "frameSize" {
            for k in ["width", "height"] {
                if let Some(v) = fields.get(k).and_then(Value::as_f64) {
                    fields.insert(k.into(), json!(format_measure(v, Unit::Picas)));
                }
            }
        }
        Dialog { id: id.into(), fields }
    }
    fn s(&self, k: &str) -> String {
        match self.fields.get(k) {
            Some(Value::String(s)) => s.clone(),
            Some(v) => v.to_string(),
            None => String::new(),
        }
    }
    fn m(&self, k: &str) -> Option<f64> {
        match self.fields.get(k) {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => parse_measure(s, Unit::Picas).ok(),
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
    fn b(&self, k: &str) -> bool {
        self.fields.get(k).and_then(Value::as_bool).unwrap_or(false)
    }
    fn n(&self, k: &str) -> Option<f64> {
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
                    Err(e) => app.status(format!("User Dictionary: {e}")),
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
                let text = if recording == *id { "Press keys…".to_string() } else { cur };
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
    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Reset All to Defaults"))).clicked() {
        let _ = app.run("window.resetShortcuts", json!({}));
    }
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
            if ui.radio(range == "all", "All").clicked() {
                d.fields.insert("range".into(), json!("all"));
            }
            ui.horizontal(|ui| {
                if ui.radio(range == "range", "Range:").clicked() {
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
    let shown = json!({
        "columns": o.columns, "gutter": fm(o.gutter),
        "insetTop": fm(o.inset[0]), "insetLeft": fm(o.inset[1]), "insetBottom": fm(o.inset[2]), "insetRight": fm(o.inset[3]),
        "verticalJustification": serde_json::to_value(o.vertical_justification).unwrap_or(Value::Null),
        "columnRule": o.column_rule, "columnRuleWeight": format_measure(o.column_rule_weight, Unit::Points), "columnRuleColor": o.column_rule_color,
        "columnRuleTint": percent(o.column_rule_tint), "columnRuleTopInset": fm(o.column_rule_top_inset),
        "columnRuleBottomInset": fm(o.column_rule_bottom_inset), "columnRuleOffset": fm(o.column_rule_offset),
    });
    if let Value::Object(m) = &shown {
        for (k, v) in m {
            d.fields.entry(k.clone()).or_insert_with(|| v.clone());
        }
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
    egui::Grid::new("tfo").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Number of columns"));
        text_field(ui, d, "columns", 80.0);
        ui.end_row();
        crate::rtl::label(ui, tr("Gutter"));
        text_field(ui, d, "gutter", 80.0);
        ui.end_row();
    });
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Inset spacing")).font(semibold(12.0)));
    egui::Grid::new("tfo_inset").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (label, key) in [("Top", "insetTop"), ("Bottom", "insetBottom"), ("Left", "insetLeft"), ("Right", "insetRight")] {
            crate::rtl::label(ui, tr(label));
            text_field(ui, d, key, 80.0);
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    egui::Grid::new("tfo_vj").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, tr("Vertical justification"));
        let cur = d.s("verticalJustification");
        egui::ComboBox::from_id_salt("vj").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&lang, &cur))).show_ui(ui, |ui| {
            for v in ["top", "center", "bottom", "justify"] {
                if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&lang, v))).clicked() {
                    d.fields.insert("verticalJustification".into(), json!(v));
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    crate::rtl::label(ui, egui::RichText::new(tr("Column Rules")).font(semibold(12.0)));
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
    if edited(d, "verticalJustification") {
        p.insert("verticalJustification".into(), json!(d.s("verticalJustification")));
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
pub fn open_document_setup(app: &mut DesignApp) {
    let Ok(cur) = app.session.execute("layout.documentSetup", &json!({})) else { return };
    let units = app.session.active().map(|s| s.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let fm = |v: &Value| json!(format_measure(v.as_f64().unwrap_or(0.0), units));
    let mut f = json!({"intent": cur["intent"], "pages": cur["pages"], "startPage": cur["startPage"], "facingPages": cur["facingPages"],
        "binding": cur["binding"], "width": fm(&cur["width"]), "height": fm(&cur["height"])});
    for k in ["bleed", "slug"] {
        for (i, e) in ["Top", "Bottom", "Inside", "Outside"].iter().enumerate() {
            f[format!("{k}{e}")] = fm(&cur[k][i]);
        }
    }
    app.ui.dialog = Some(Dialog::new("documentSetup", f));
}

fn document_setup(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    egui::Grid::new("ds").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Intent:"));
        let cur = d.s("intent");
        egui::ComboBox::from_id_salt("ds_intent")
            .selected_text(match cur.as_str() {
                "web" => crate::i18n::tr(&app.ui.language, "Web"),
                "mobile" => crate::i18n::tr(&app.ui.language, "Mobile"),
                _ => crate::i18n::tr(&app.ui.language, "Print"),
            })
            .width(110.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("print", "Print"), ("web", "Web"), ("mobile", "Mobile")] {
                    if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        d.fields.insert("intent".into(), json!(v));
                    }
                }
            });
        ui.label("");
        ui.label("");
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Number of Pages:"));
        text_field(ui, d, "pages", 60.0);
        check(ui, d, "facingPages", crate::i18n::tr(&app.ui.language, "Facing Pages"));
        ui.label("");
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Start Page #:"));
        text_field(ui, d, "startPage", 60.0);
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Binding:"));
        let rtl = d.s("binding") == "rightToLeft";
        egui::ComboBox::from_id_salt("ds_binding")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if rtl { "Right to Left" } else { "Left to Right" })))
            .width(110.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("leftToRight", "Left to Right"), ("rightToLeft", "Right to Left")] {
                    if ui.selectable_label((v == "rightToLeft") == rtl, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked() {
                        d.fields.insert("binding".into(), json!(v));
                    }
                }
            });
        ui.end_row();
    });
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Page Size")).font(semibold(12.0)));
    let (w, h) = (d.m("width").unwrap_or(612.0), d.m("height").unwrap_or(792.0));
    let preset =
        designcraft_doc::build::PRESETS.iter().find(|p| (p.width - w).abs() < 0.5 && (p.height - h).abs() < 0.5).map_or("Custom", |p| p.name);
    egui::Grid::new("ds_size").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Size:"));
        egui::ComboBox::from_id_salt("ds_preset").selected_text(preset).width(140.0).show_ui(ui, |ui| {
            for p in designcraft_doc::build::PRESETS {
                if ui.selectable_label(p.name == preset, p.name).clicked() {
                    d.fields.insert("width".into(), json!(format_measure(p.width, p.units)));
                    d.fields.insert("height".into(), json!(format_measure(p.height, p.units)));
                }
            }
        });
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Orientation:"));
        ui.horizontal(|ui| {
            for (portrait, l) in [(true, "Portrait"), (false, "Landscape")] {
                if ui.selectable_label((h >= w) == portrait, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, l))).clicked()
                    && (h >= w) != portrait
                {
                    let (fw, fh) = (d.s("width"), d.s("height"));
                    d.fields.insert("width".into(), json!(fh));
                    d.fields.insert("height".into(), json!(fw));
                }
            }
        });
        ui.end_row();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Width:"));
        text_field(ui, d, "width", 80.0);
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Height:"));
        text_field(ui, d, "height", 80.0);
        ui.end_row();
    });
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Bleed and Slug")).font(semibold(12.0)));
    egui::Grid::new("ds_bleed").num_columns(5).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label("");
        for e in ["Top", "Bottom", "Inside", "Outside"] {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, e));
        }
        ui.end_row();
        for (k, l) in [("bleed", "Bleed:"), ("slug", "Slug:")] {
            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, l));
            for e in ["Top", "Bottom", "Inside", "Outside"] {
                text_field(ui, d, &format!("{k}{e}"), 60.0);
            }
            ui.end_row();
        }
    });
    check(ui, d, "adjustLayout", crate::i18n::tr(&app.ui.language, "Adjust Layout (objects follow the new page size)"));
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
                    check(ui, d, "snap.alignEdges", "Align to Object Edges");
                    check(ui, d, "snap.alignCenters", "Align to Object Centers");
                    check(ui, d, "snap.dimensions", "Smart Dimensions");
                    check(ui, d, "snap.spacing", "Smart Spacing");
                    ui.horizontal(|ui| {
                        ui.label("Snap to Zone");
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
                }
                _ => {
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
    let mut result: Option<bool> = None;
    let alert_title = d.s("title");
    let title = match d.id.as_str() {
        "alert" => crate::i18n::tr(&app.ui.language, &alert_title),
        "ruby" => crate::i18n::tr(&app.ui.language, "Ruby"),
        "newDocument" => crate::i18n::tr(&app.ui.language, "New Document"),
        "frameSize" => crate::i18n::tr(&app.ui.language, "Rectangle"),
        "goToPage" => crate::i18n::tr(&app.ui.language, "Go to Page"),
        "insertTable" => crate::i18n::tr(&app.ui.language, "Create Table"),
        "textFrameOptions" => crate::i18n::tr(&app.ui.language, "Text Frame Options"),
        "documentSetup" => crate::i18n::tr(&app.ui.language, "Document Setup"),
        "findChange" => crate::i18n::tr(&app.ui.language, "Find/Change"),
        "paragraphStyleOptions" => crate::i18n::tr(&app.ui.language, "Paragraph Style Options"),
        "characterStyleOptions" => crate::i18n::tr(&app.ui.language, "Character Style Options"),
        "footnoteOptions" => crate::i18n::tr(&app.ui.language, "Footnote Options"),
        "paragraphRules" => crate::i18n::tr(&app.ui.language, "Paragraph Rules"),
        "insertXref" => crate::i18n::tr(&app.ui.language, "New Cross-Reference"),
        "findFont" => crate::i18n::tr(&app.ui.language, "Find/Replace Font"),
        "polygonSettings" => crate::i18n::tr(&app.ui.language, "Polygon Settings"),
        "userDictionary" => crate::i18n::tr(&app.ui.language, "User Dictionary"),
        "newWorkspace" => crate::i18n::tr(&app.ui.language, "New Workspace"),
        "closeDocument" => crate::i18n::tr(&app.ui.language, "Unsaved Changes"),
        "menus" => crate::i18n::tr(&app.ui.language, "Menu Customization"),
        "importOptions" => crate::i18n::tr(&app.ui.language, "Import Options"),
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
        id => match id.strip_prefix("cmd:").and_then(designcraft_engine::find_command) {
            Some(c) => crate::i18n::tr(&app.ui.language, c.label.trim_end_matches('…')),
            None => crate::i18n::tr(&app.ui.language, "Dialog"),
        },
    };
    egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
        let frame_margin = egui::Frame::popup(ui.style()).total_margin().sum();
        let available = (ctx.content_rect().size() - frame_margin - egui::vec2(16.0, 16.0)).max(egui::Vec2::splat(1.0));
        let preferred_width: f32 = match d.id.as_str() {
            "alert" => 440.0,
            "closeDocument" => 380.0,
            "newDocument" if crate::i18n::is_rtl(&app.ui.language) => 380.0,
            _ => 640.0,
        };
        let width = preferred_width.min(available.x);
        ui.set_min_width(380.0_f32.min(width));
        ui.set_max_width(width);
        if crate::i18n::is_rtl(&app.ui.language) {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::rtl::label(ui, egui::RichText::new(title).font(semibold(16.0)));
                });
            });
        } else {
            crate::rtl::label(ui, egui::RichText::new(title).font(semibold(16.0)));
        }
        ui.add_space(10.0);
        // Keep the title and action buttons outside the scrollable body. Oversized
        // forms remain reachable at large UI scales, without shrinking their text.
        let footer_height = ui.spacing().interact_size.y.max(ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y);
        let body_height = (available.y - ui.min_size().y - footer_height - 12.0 - 2.0 * ui.spacing().item_spacing.y).max(1.0);
        egui::ScrollArea::both().id_salt(("dialog_body", &d.id)).min_scrolled_width(1.0).min_scrolled_height(1.0).max_width(width).max_height(body_height).show(ui, |ui| {
        match d.id.as_str() {
            "newDocument" => {
                ui.horizontal(|ui| {
                    ui.with_layout(if crate::i18n::is_rtl(&app.ui.language) { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) }, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Preset"));
                    let cur = d.s("preset");
                    egui::ComboBox::from_id_salt("preset").selected_text(&cur).width(180.0).show_ui(ui, |ui| {
                        for p in designcraft_doc::build::PRESETS {
                            if ui.selectable_label(cur == p.name, p.name).clicked() {
                                d.fields.insert("preset".into(), json!(p.name));
                                d.fields.insert("width".into(), json!(format_measure(p.width, p.units)));
                                d.fields.insert("height".into(), json!(format_measure(p.height, p.units)));
                                d.fields.insert("facingPages".into(), json!(p.intent == designcraft_doc::Intent::Print));
                            }
                        }
                    });
                    });
                });
                if crate::i18n::is_rtl(&app.ui.language) {
                    // egui Grid currently supports LTR only. Use explicit RTL rows;
                    // the numeric editors retain LTR alignment and logical values.
                    for (first, key, second, other, toggle) in [
                        ("Width", "width", "Height", "height", false),
                        ("Pages", "pages", "Facing Pages", "facingPages", true),
                        ("Columns", "columns", "Gutter", "gutter", false),
                        ("Top", "marginTop", "Bottom", "marginBottom", false),
                        ("Inside", "marginInside", "Outside", "marginOutside", false),
                        ("Bleed", "bleed", "Primary Text Frame", "primaryTextFrame", true),
                    ] {
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, first));
                                text_field(ui, &mut d, key, 80.0);
                                if toggle {
                                    check(ui, &mut d, other, crate::i18n::tr(&app.ui.language, second));
                                } else {
                                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, second));
                                    text_field(ui, &mut d, other, 80.0);
                                }
                            });
                        });
                    }
                } else {
                egui::Grid::new("nd").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Width"));
                    text_field(ui, &mut d, "width", 80.0);
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Height"));
                    text_field(ui, &mut d, "height", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Pages"));
                    text_field(ui, &mut d, "pages", 80.0);
                    ui.label("");
                    check(ui, &mut d, "facingPages", crate::i18n::tr(&app.ui.language, "Facing Pages"));
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Columns"));
                    text_field(ui, &mut d, "columns", 80.0);
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Gutter"));
                    text_field(ui, &mut d, "gutter", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Top"));
                    text_field(ui, &mut d, "marginTop", 80.0);
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Bottom"));
                    text_field(ui, &mut d, "marginBottom", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Inside"));
                    text_field(ui, &mut d, "marginInside", 80.0);
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Outside"));
                    text_field(ui, &mut d, "marginOutside", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Bleed"));
                    text_field(ui, &mut d, "bleed", 80.0);
                    ui.label("");
                    check(ui, &mut d, "primaryTextFrame", crate::i18n::tr(&app.ui.language, "Primary Text Frame"));
                    ui.end_row();
                });
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
                        if ui.selectable_label(!d.b("objectMode") && d.b("grep") == grep, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                            d.fields.insert("grep".into(), json!(grep));
                            d.fields.insert("objectMode".into(), json!(false));
                        }
                    }
                    if ui.selectable_label(d.b("objectMode"),crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Object"))).clicked() {
                        d.fields.insert("objectMode".into(), json!(true));
                    }
                });
                if d.b("objectMode") {
                    // Find/Change › Object: objects by fill and kind; change their fill or opacity.
                    let swatches: Vec<String> = app.session.active().map(|st| st.doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect()).unwrap_or_default();
                    egui::Grid::new("fco").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        for (key, label, any) in [("objFill", "Find fill:", "(any)"), ("objChangeFill", "Change fill to:", "(unchanged)")] {
                            crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                            let cur = d.s(key);
                            egui::ComboBox::from_id_salt(key).selected_text(if cur.is_empty() { any } else { cur.as_str() }).width(180.0).show_ui(ui, |ui| {
                                if ui.selectable_label(cur.is_empty(), crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, any))).clicked() {
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
                        egui::ComboBox::from_id_salt("objKind").selected_text(if cur.is_empty() { "(any)" } else { cur.as_str() }).show_ui(ui, |ui| {
                            for v in ["", "text", "graphic", "shape", "line", "group"] {
                                if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if v.is_empty() { "(any)" } else { v }))).clicked() {
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
                        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change All"))).clicked() && !d.s("objChangeFill").is_empty() {
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
                    egui::ComboBox::from_id_salt("fcscope").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &cur))).show_ui(ui, |ui| {
                        for v in ["document", "story", "selection"] {
                            if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, v))).clicked() {
                                d.fields.insert("scope".into(), json!(v));
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
                    let params0 = json!({"find": d.s("find"), "change": d.s("change"), "grep": d.b("grep"), "caseSensitive": d.b("caseSensitive"), "wholeWord": d.b("wholeWord"), "scope": d.s("scope")});
                    let params = || params0.clone();
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Find Next"))).clicked() {
                        let r = app.run("find.next", params());
                        let msg = match r {
                            Ok(Value::Null) => "No matches".to_string(),
                            Ok(_) => "Found".to_string(),
                            Err(e) => e,
                        };
                        d.fields.insert("status".into(), json!(msg));
                    }
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Change All"))).clicked() {
                        let msg = match app.run("find.change", params()) {
                            Ok(v) => format!("{} replacement(s) made", v["count"]),
                            Err(e) => e,
                        };
                        d.fields.insert("status".into(), json!(msg));
                    }
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Count"))).clicked() {
                        let msg = match app.run("find.find", params()) {
                            Ok(v) => format!("{} match(es)", v.as_array().map(|a| a.len()).unwrap_or(0)),
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
            "characterStyleOptions" => character_style_options(app, ui, &mut d),
            "footnoteOptions" => footnote_options(app, ui, &mut d),
            "paragraphRules" => {
                let current = d.fields.get("current").cloned().unwrap_or_default();
                paragraph_rules(app, ui, &mut d, "", &current);
            }
            "insertXref" => insert_xref(app, ui, &mut d),
            "findFont" => find_font(app, ui, &mut d),
            "colorPicker" => color_picker(ui, &mut d),
            "preferences" => preferences(app, ui, &mut d, body_height),
            "print" => print_dialog(app, ui, &mut d),
            "pdfImport" => {
                ui.label(egui::RichText::new(if d.b("async") { d.s("name") } else { d.s("path") }).size(11.0));
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page (1–{count}):").replace("{count}", &d.n("pages").unwrap_or(1.0).to_string()));
                    text_field(ui, &mut d, "page", 60.0);
                });
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Crop to:"));
                    let cur = d.s("crop");
                    let label = |v: &str| match v {
                        "trim" => crate::i18n::tr(&app.ui.language, "Trim"),
                        "bleed" => crate::i18n::tr(&app.ui.language, "Bleed"),
                        "art" => crate::i18n::tr(&app.ui.language, "Art"),
                        "media" => crate::i18n::tr(&app.ui.language, "Media"),
                        _ => crate::i18n::tr(&app.ui.language, "Crop"),
                    };
                    egui::ComboBox::from_id_salt("pdf_crop").selected_text(crate::rtl::widget(ui, label(&cur))).show_ui(ui, |ui| {
                        for v in ["crop", "trim", "bleed", "art", "media"] {
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
                    if ui.checkbox(&mut on, l["name"].as_str().unwrap_or("")).changed() {
                        l["visible"] = json!(on);
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
                        .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, designcraft_color::cms::Intent::parse(&cur).map_or("", |i| i.label()))))
                        .show_ui(ui, |ui| {
                            for i in designcraft_color::cms::Intent::ALL {
                                if ui.selectable_label(i.id() == cur, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, i.label()))).clicked() {
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
                        Err(e) => app.status(format!("Color Settings: {e}")),
                    }
                }
            }
            "keyboardShortcuts" => keyboard_shortcuts(app, ui, &mut d),
            "userDictionary" => user_dictionary(app, ui, &mut d),
            "menus" => {
                text_field(ui, &mut d, "query", 260.0);
                let q = d.s("query").to_lowercase();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (menu, label) in crate::menus::all_menu_items().into_iter().filter(|(m, l)| q.is_empty() || m.to_lowercase().contains(&q) || l.to_lowercase().contains(&q)) {
                        let key = crate::menus::menu_key(&menu, &label);
                        let mut visible = !app.ui.hidden_menu_items.contains(&key);
                        if ui.checkbox(&mut visible, crate::rtl::widget(ui, format!("{} › {}", crate::i18n::tr(&app.ui.language, &menu), crate::i18n::tr(&app.ui.language, &label)))).changed() {
                            let _ = app.run("window.hideMenuItem", json!({"item": key, "hidden": !visible}));
                        }
                    }
                });
            }
            "importOptions" => {
                ui.label(egui::RichText::new(d.s("path")).size(10.5));
                check(ui, &mut d, "removeStyles", crate::i18n::tr(&app.ui.language, "Remove Styles and Formatting from Text and Tables"));
                if !d.b("removeStyles") {
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Name Conflicts")).font(semibold(12.0)));
                    let conflicts: Vec<String> = d.fields.get("conflicts").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    ui.label(if conflicts.is_empty() { crate::i18n::tr(&app.ui.language, "No conflicts").to_string() } else { format!("{} conflict(s): {}", conflicts.len(), conflicts.join(", ")) });
                    let cur = d.s("styleConflicts");
                    ui.horizontal(|ui| {
                        for (v, l) in [("useExisting", "Use Document Style Definition"), ("redefine", "Redefine Document Style"), ("autoRename", "Auto Rename")] {
                            if ui.radio(cur == v, l).clicked() {
                                d.fields.insert("styleConflicts".into(), json!(v));
                            }
                        }
                    });
                    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Style Mapping")).font(semibold(12.0)));
                    let names: Vec<String> = app.session.active().map(|s| s.doc.styles.paragraph.iter().map(|p| p.name.clone()).collect()).unwrap_or_default();
                    let imported: Vec<String> = d.fields.get("styles").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        egui::Grid::new("imp_map").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                            for w in &imported {
                                ui.label(w);
                                let mapped = d.fields.get("map").and_then(|m| m.get(w)).and_then(Value::as_str).unwrap_or("").to_string();
                                let shown = if mapped.is_empty() { "(import)".to_string() } else { mapped.clone() };
                                egui::ComboBox::from_id_salt(("imp_map", w)).selected_text(crate::rtl::widget(ui, &shown)).width(170.0).show_ui(ui, |ui| {
                                    if ui.selectable_label(mapped.is_empty(),crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "(import)"))).clicked()
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
            "closeDocument" => {
                let text = crate::i18n::tr(&app.ui.language, "“{}” has changes that are not saved. Save them before closing?").replace("{}", &d.s("title"));
                crate::rtl::label(ui, text);
            }
            "newWorkspace" => {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Name:"));
                    text_field(ui, &mut d, "name", 220.0);
                });
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Saves which bars show and where the panels are.")).size(10.5));
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
                    egui::ComboBox::from_id_salt("ff_fit").selected_text(crate::rtl::widget(ui, label(&cur))).width(200.0).show_ui(ui, |ui| {
                        for v in ["none", "fillProportionally", "fitProportionally", "fitContentToFrame", "centerContent"] {
                            if ui.selectable_label(cur == v, crate::rtl::widget(ui, label(v))).clicked() {
                                d.fields.insert("fitting".into(), json!(v));
                            }
                        }
                    });
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
                    egui::ComboBox::from_id_salt("qr_type").selected_text(crate::rtl::widget(ui, label(&cur))).width(160.0).show_ui(ui, |ui| {
                        for v in ["url", "text", "sms", "email", "vcard"] {
                            if ui.selectable_label(cur == v, crate::rtl::widget(ui, label(v))).clicked() {
                                d.fields.insert("type".into(), json!(v));
                            }
                        }
                    });
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
            id if id.starts_with("cmd:") => command_form(app, ui, &mut d),
            "goToPage" => {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Page"));
                    text_field(ui, &mut d, "page", 80.0);
                });
            }
            "ruby" => {
                ui.horizontal(|ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Ruby:"));
                    text_field(ui, &mut d, "text", 200.0);
                });
                crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Set over the selected text; empty removes it.")).size(11.0));
            }
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
            "textFrameOptions" => {
                seed_text_frame_options(app, &mut d);
                text_frame_options_dialog(app, ui, &mut d);
            }
            "documentSetup" => document_setup(app, ui, &mut d),
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
        ui.add_space(12.0);
        let ok_label = if d.id == "closeDocument" { "  Save  " } else { "  OK  " };
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, ok_label)).color(egui::Color32::WHITE)))
                            .fill(crate::theme::Tokens::get(ui.ctx()).accent_strong),
                    )
                    .clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    result = Some(true);
                }
                // An alert only has OK.
                let cancel = d.id != "alert" && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Cancel"))).clicked();
                if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    result = Some(false);
                }
                if d.id == "closeDocument" && ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Don't Save"))).clicked() {
                    d.fields.insert("discard".into(), json!(true));
                    result = Some(true);
                }
            });
        });
    });
    app.ui.dialog = Some(d);
    match result {
        Some(true) => {
            let _ = confirm(app);
        }
        Some(false) => {
            app.ui.dialog = None;
            app.cancel_pdf_import();
        }
        None => {}
    }
}

/// Apply the open dialog.
pub fn confirm(app: &mut DesignApp) -> Result<Value, String> {
    let Some(d) = app.ui.dialog.take() else { return Err("no dialog open".into()) };
    if d.id != "pdfImport" || !d.b("async") {
        app.cancel_pdf_import();
    }
    match d.id.as_str() {
        "newDocument" => {
            let margins = json!({"top": d.m("marginTop").unwrap_or(36.0), "bottom": d.m("marginBottom").unwrap_or(36.0), "inside": d.m("marginInside").unwrap_or(36.0), "outside": d.m("marginOutside").unwrap_or(36.0)});
            app.run(
                "file.new",
                json!({"width": d.m("width"), "height": d.m("height"), "pages": d.n("pages").unwrap_or(1.0) as u64, "facingPages": d.b("facingPages"),
                    "columns": d.n("columns").unwrap_or(1.0) as u64, "gutter": d.m("gutter").unwrap_or(12.0), "margins": margins, "bleed": d.m("bleed").unwrap_or(0.0),
                    "primaryTextFrame": d.b("primaryTextFrame")}),
            )
        }
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
        "ruby" => app.run("type.ruby", json!({"text": d.s("text")})),
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
            let edges = |k: &str| json!(["Top", "Bottom", "Inside", "Outside"].map(|e| d.m(&format!("{k}{e}")).unwrap_or(0.0)));
            app.run(
                "layout.documentSetup",
                json!({"intent": d.s("intent"), "pages": d.n("pages").unwrap_or(1.0).max(1.0) as u64, "startPage": d.n("startPage").unwrap_or(1.0).max(1.0) as u64,
                    "facingPages": d.b("facingPages"), "binding": d.s("binding"), "width": d.m("width"), "height": d.m("height"), "bleed": edges("bleed"), "slug": edges("slug"), "adjustLayout": d.b("adjustLayout")}),
            )
        }
        "paragraphStyleOptions" => {
            let name = d.s("name");
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
            if d.fields.contains_key("x.tag") {
                let tag = d.s("x.tag");
                let tag = if tag == "[Automatic]" { String::new() } else { tag };
                app.run("style.exportTag", json!({"style": style, "tag": tag, "class": d.s("x.class")}))?;
            }
            Ok(r)
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
        "colorPicker" => {
            let hex = d.s("hex");
            app.run("object.color", json!({"color": hex, "target": d.s("target")}))
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
            app.open_file("file.place", json!({"path": d.s("path"), "pdfPage": d.n("page").unwrap_or(1.0).max(1.0) as u64, "pdfCrop": if crop.is_empty() { "crop".to_string() } else { crop }}))
        }
        "print" => {
            let pages = if d.s("range") == "all" { Value::Null } else { json!(d.s("pages")) };
            let printer = d.s("printer");
            let r = app.run(
                "file.print",
                json!({"printer": if printer.is_empty() { Value::Null } else { json!(printer) }, "copies": d.n("copies").unwrap_or(1.0).max(1.0) as u64,
                    "pages": pages, "spreads": d.b("spreads"), "marks": d.b("marks"), "bleed": d.b("bleed")}),
            )?;
            app.status(format!("Sent {} page(s) to {}", r["pages"], r["printer"].as_str().unwrap_or("the default printer")));
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
                    "recoveryMinutes": d.n("recoveryMinutes").unwrap_or(0.5),
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
            app.ui.story_editor_size = d.n("storyEditorSize").unwrap_or(14.0) as f32;
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
            if !d.fields.contains_key("horizontalUnits") {
                return Ok(Value::Null);
            }
            let mut doc = json!({"horizontalUnits": d.fields["horizontalUnits"], "verticalUnits": d.fields["verticalUnits"]});
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
        "newWorkspace" => app.run("window.newWorkspace", json!({"name": d.s("name")})),
        "closeDocument" => {
            // The document may have moved in the tab row (or gone) while the dialog was open.
            let uid = d.fields.get("uid").and_then(Value::as_u64);
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
            let (f, st) = (d.s("family"), d.s("style"));
            if f.is_empty() || d.s("toFamily").is_empty() {
                return Ok(Value::Null);
            }
            app.run("font.replace", json!({"family": f, "style": st, "toFamily": d.s("toFamily"), "toStyle": d.s("toStyle")}))
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
        "findChange" if d.b("objectMode") => Ok(Value::Null),
        "findChange" => app.run("find.change", json!({"find": d.s("find"), "change": d.s("change"), "grep": d.b("grep"), "caseSensitive": d.b("caseSensitive"), "wholeWord": d.b("wholeWord"), "scope": d.s("scope")})),
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

/// Paragraph Style Options: sections in a left list (like InDesign), fields on the right.
/// Edited values are stored as `p.<attr>` / `c.<attr>` fields and applied on OK.
/// Is `p` a pattern the GREP styles can use? (Checked as the user types.)
fn regex_ok(p: &str) -> Result<(), ()> {
    if p.is_empty() {
        return Ok(());
    }
    regex::Regex::new(p).map(|_| ()).map_err(|_| ())
}

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
                ("indents", "Indents and Spacing"),
                ("tabs", "Tabs"),
                ("rules", "Paragraph Rules"),
                ("hyph", "Hyphenation"),
                ("justify", "Justification"),
                ("nested", "Drop Caps and Nested Styles"),
                ("grep", "GREP Style"),
                ("color", "Character Color"),
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
                        if app.ui.language != "uk" {
                            count_value(ui, ns, &mut changed);
                        }
                        let kind = ns["until"]["kind"].as_str().unwrap_or("words").to_string();
                        egui::ComboBox::from_id_salt(("ns_until", i))
                            .selected_text(crate::rtl::widget(
                                ui,
                                if app.ui.language == "uk" {
                                    format!("{}:", crate::i18n::tr("uk", &kind))
                                } else {
                                    crate::i18n::tr(&app.ui.language, &kind).to_owned()
                                },
                            ))
                            .width(100.0)
                            .show_ui(ui, |ui| {
                                for k in
                                    ["sentences", "words", "characters", "letters", "digits", "tab", "forcedLineBreak", "emSpace", "enSpace", "chars"]
                                {
                                    if ui.selectable_label(k == kind, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, k))).clicked() {
                                        ns["until"] = if k == "chars" { json!({"kind": "chars", "chars": ":"}) } else { json!({"kind": k}) };
                                        changed = true;
                                    }
                                }
                            });
                        if app.ui.language == "uk" {
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
                            if app.ui.language == "uk" {
                                format!("{}:", crate::i18n::tr("uk", "lines"))
                            } else {
                                crate::i18n::tr(&app.ui.language, "for").to_owned()
                            },
                        );
                        let mut n = l["lines"].as_u64().unwrap_or(1) as u32;
                        if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
                            l["lines"] = json!(n);
                            changed = true;
                        }
                        if app.ui.language != "uk" {
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
                        let bad = regex_ok(&pat).is_err();
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
            "justify" => {
                egui::Grid::new("psj").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label("");
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Minimum"));
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Desired"));
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Maximum"));
                    ui.end_row();
                    for (label, base) in [("Word Spacing:", "wordSpace"), ("Letter Spacing:", "letterSpace"), ("Glyph Scaling:", "glyphScale")] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        for suffix in ["Min", "Desired", "Max"] {
                            let key = format!("{base}{suffix}");
                            let v = cur(d, &format!("p.{key}"), &pv[key.as_str()]).as_f64().map(|x| x * 100.0);
                            if let Some(n) = ui.scope(|ui| crate::widgets::number(ui, &format!("ps{key}"), v, "%", 60.0, 0)).inner {
                                d.fields.insert(format!("p.{key}"), json!(n / 100.0));
                            }
                        }
                        ui.end_row();
                    }
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Composer:"));
                    let single = cur(d, "p.composer", &pv["composer"]).as_str() == Some("singleLine");
                    if ui.selectable_label(!single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Paragraph Composer"))).clicked() {
                        d.fields.insert("p.composer".into(), json!("paragraph"));
                    }
                    if ui.selectable_label(single, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Single-line Composer"))).clicked() {
                        d.fields.insert("p.composer".into(), json!("singleLine"));
                    }
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Insert Kashidas:"));
                    let mut k = cur(d, "p.kashidas", &pv["kashidas"]).as_bool().unwrap_or(true);
                    if ui.checkbox(&mut k, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "In justified Arabic text"))).changed() {
                        d.fields.insert("p.kashidas".into(), json!(k));
                    }
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Korean Line Breaks:"));
                    let mut kb = cur(d, "p.koreanCharBreaks", &pv["koreanCharBreaks"]).as_bool().unwrap_or(false);
                    if ui
                        .checkbox(&mut kb, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Between syllables (not only at spaces)")))
                        .changed()
                    {
                        d.fields.insert("p.koreanCharBreaks".into(), json!(kb));
                    }
                    ui.end_row();
                });
            }
            "color" => character_color(&app.ui.language, ui, d, &CharFields { base: &cv, sparse: false }, &doc),
            s if style_cjk::is_section(s) => {
                style_cjk::section(&app.ui.language, ui, d, s, &CharFields { base: &cv, sparse: false }, Some(&pv), &doc)
            }
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
                crate::rtl::label(
                    ui,
                    egui::RichText::new(format!(
                        "{} {} {:.1} pt · {}",
                        cp.font_family,
                        cp.font_style,
                        cp.size,
                        crate::i18n::tr(&app.ui.language, pp.align.label())
                    ))
                    .color(crate::theme::Tokens::get(ui.ctx()).text_dim),
                );
            }
        });
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

/// The section list on the left of the style options dialogs.
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
            s if style_cjk::is_section(s) => style_cjk::section(&lang, ui, d, s, &cf, None, &doc),
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
                    cf.set(d, "fontFamily", json!(f));
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
    let (swatches, h_units, v_units) = app
        .session
        .active()
        .map(|st| {
            let sw: Vec<String> = st.doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect();
            (sw, st.doc.settings.horizontal_units, st.doc.settings.vertical_units)
        })
        .unwrap_or((Vec::new(), Unit::Points, Unit::Points));
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
            egui::ComboBox::from_id_salt(("rule_color", which)).selected_text(&color).width(130.0).show_ui(ui, |ui| {
                for w in &swatches {
                    let (c, g) = app.session.active().map_or((None, None), |st| crate::widgets::swatch_colors(&st.doc, w, 1.0));
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        crate::widgets::paint_chip(ui.painter(), r, c, g);
                        if ui.selectable_label(*w == color, w).clicked() {
                            d.fields.insert(key("color"), json!(w));
                        }
                    });
                }
            });
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
                &[("ascent", "Ascent"), ("capHeight", "Cap Height"), ("leading", "Leading"), ("xHeight", "x Height"), ("fixed", "Fixed")],
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

/// New Cross-Reference: link to a paragraph (paragraph styles on the left, their paragraphs on
/// the right) or a text anchor, with a cross-reference format.
fn insert_xref(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let Some(st) = app.session.active() else { return };
    let doc = &st.doc;
    let all = "[All Paragraphs]";
    ui.horizontal(|ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Link To:"));
        combo(ui, d, "linkTo", &[("paragraph", "Paragraph"), ("anchor", "Text Anchor")]);
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

/// Color Picker: saturation/brightness field and hue slider, with RGB, CMYK and hex fields.
fn color_picker(ui: &mut egui::Ui, d: &mut Dialog) {
    let hex = d.s("hex");
    let mut c = designcraft_color::Color::from_hex(&hex).map(|c| c.to_rgb()).map_or(egui::Color32::BLACK, |[r, g, b]| {
        egui::Color32::from_rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8)
    });
    let before = c;
    ui.horizontal_top(|ui| {
        egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque);
        ui.vertical(|ui| {
            let mut rgb = [c.r() as f32, c.g() as f32, c.b() as f32];
            egui::Grid::new("cp_rgb").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                for (k, l) in ["R", "G", "B"].iter().enumerate() {
                    ui.label(*l);
                    ui.add(egui::DragValue::new(&mut rgb[k]).range(0.0..=255.0));
                    ui.end_row();
                }
            });
            c = egui::Color32::from_rgb(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
            let col = designcraft_color::Color::rgb8(c.r(), c.g(), c.b());
            let [cc, m, y, k] = col.to_cmyk();
            ui.add_space(6.0);
            ui.label(format!("C {:.0}%  M {:.0}%  Y {:.0}%  K {:.0}%", cc * 100.0, m * 100.0, y * 100.0, k * 100.0));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("#");
                let mut h = format!("{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
                if ui.add(egui::TextEdit::singleline(&mut h).desired_width(70.0)).changed()
                    && let Some(n) = designcraft_color::Color::from_hex(&h)
                {
                    let [r, g, b] = n.to_rgb();
                    c = egui::Color32::from_rgb((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8);
                }
            });
        });
    });
    if c != before {
        d.fields.insert("hex".into(), json!(format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())));
    }
}

/// Find/Replace Font: the document's fonts (missing ones first, flagged) and a replacement.
fn find_font(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    // Listed once when the dialog opens (it scans every story).
    if !d.fields.contains_key("_fonts") {
        let list = app.session.execute("font.list", &json!({})).unwrap_or_default();
        d.fields.insert("_fonts".into(), list);
    }
    let fonts = d.fields.get("_fonts").and_then(Value::as_array).cloned().unwrap_or_default();
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
            if warn {
                text = text.color(egui::Color32::from_rgb(0xe5, 0x4b, 0x4b));
            }
            if ui.selectable_label(on, text).clicked() {
                d.fields.insert("family".into(), json!(fam));
                d.fields.insert("style".into(), json!(st));
            }
        }
    });
    ui.add_space(8.0);
    crate::rtl::label(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "Replace With")).font(semibold(12.0)));
    let db = crate::panels::fonts(app);
    egui::Grid::new("ff_to").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Family:"));
        if let Some(f) = crate::panels::font_combo(app, ui, "toFamily", &d.s("toFamily"), 170.0) {
            d.fields.insert("toFamily".into(), json!(f));
        }
        ui.end_row();
        let styles = db.styles(&d.s("toFamily"));
        let st_opts: Vec<(&str, &str)> = styles.iter().map(|s| (s.as_str(), s.as_str())).collect();
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Style:"));
        combo(ui, d, "toStyle", &st_opts);
        ui.end_row();
    });
    ui.label(crate::rtl::widget(
        ui,
        egui::RichText::new(crate::i18n::tr(&app.ui.language, "OK changes all: text and paragraph/character styles."))
            .color(crate::theme::Tokens::get(ui.ctx()).text_dim)
            .size(11.0),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn dialog_title_and_actions_fit_scaled_viewports() {
        for language in ["en", "ar"] {
            for size in [egui::vec2(1178.0, 814.0), egui::vec2(589.0, 407.0), egui::vec2(392.0, 271.0), egui::vec2(450.0, 280.0)] {
                for (command, title) in [("app.preferences", "Preferences"), ("app.newDocumentDialog", "New Document")] {
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
                    for expected in [title, "OK", "Cancel"] {
                        visible_label(&labels, screen, crate::i18n::tr(language, expected));
                    }
                }
            }
        }
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

    #[test]
    fn new_document_vertical_scroll_reaches_last_field() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("app.newDocumentDialog", json!({})).unwrap();
        let ctx = dialog_context("en");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(392.0, 271.0));
        for _ in 0..3 {
            dialog_frame(&mut app, &ctx, screen, vec![]);
        }
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        let body = visible_label(&labels, screen, "Width").center();
        let (_, rect, clip) = labels.iter().find(|(text, _, _)| text == "Primary Text Frame").unwrap();
        assert!(rect.bottom() > clip.bottom(), "last field starts vertically clipped");
        scroll_dialog(&mut app, &ctx, screen, body, egui::vec2(0.0, -600.0));
        let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
        for expected in ["Primary Text Frame", "New Document", "OK", "Cancel"] {
            visible_label(&labels, screen, expected);
        }
        click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Primary Text Frame").center());
        assert!(app.ui.dialog.as_ref().unwrap().b("primaryTextFrame"));
    }

    #[test]
    fn arabic_new_document_leftmost_fields_remain_reachable() {
        for size in [egui::vec2(392.0, 271.0), egui::vec2(450.0, 280.0)] {
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
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            for (_, value) in fields {
                visible_label(&labels, screen, value);
            }
            let body = visible_label(&labels, screen, "66p1").center();
            for x in [-600.0, 600.0] {
                scroll_dialog(&mut app, &ctx, screen, body, egui::vec2(x, 0.0));
                let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
                for (_, value) in fields {
                    visible_label(&labels, screen, value);
                }
            }
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "66p1").center());
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
            assert_eq!(app.ui.dialog.as_ref().unwrap().s("height"), "88p8", "leftmost numeric field accepts edits");
            scroll_dialog(&mut app, &ctx, screen, body, egui::vec2(0.0, -600.0));
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            let label = crate::i18n::tr("ar", "Primary Text Frame");
            click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, label).center());
            assert!(app.ui.dialog.as_ref().unwrap().b("primaryTextFrame"), "leftmost checkbox remains clickable after scrolling");
        }
    }

    #[test]
    fn resized_dialogs_cancel_and_reopen_without_applying_edits() {
        for (command, title, field) in [("app.preferences", "Preferences", "recoveryMinutes"), ("app.newDocumentDialog", "New Document", "pages")] {
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
                for expected in [title, "OK", "Cancel"] {
                    visible_label(&labels, screen, expected);
                }
            }
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(589.0, 407.0));
            let labels = dialog_frame(&mut app, &ctx, screen, vec![]);
            click_dialog(&mut app, &ctx, screen, visible_label(&labels, screen, "Cancel").center());
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
            assert!(doc.spreads.iter().all(|s| s.items.is_empty()), "undocumented fields stay excluded");
            assert!(app.ui.dialog.is_none());
        }
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
        // An unknown swatch is an error and the dialog stays open.
        let e = open(&mut app, &[("columnRuleColor", json!("Mauve"))]).unwrap_err();
        assert!(e.contains("Mauve") && app.ui.dialog.is_some(), "{e}");
        assert_eq!(opts(&app).column_rule_color, designcraft_color::swatch::BLACK);
        // Each OK is one undo step.
        for _ in 0..3 {
            app.run("edit.undo", json!({})).unwrap();
        }
        assert_eq!(opts(&app), before);
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
            for section in ["general", "chars", "color"] {
                app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"new": true, "section": section})));
                frame(&mut app);
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
}
