//! Modal dialogs. Each dialog keeps its fields in a JSON map so the control channel can set them
//! (`ui.dialog.set {field, value}`) and confirm them (`ui.dialog.confirm`) like a user would.

use designcraft_geom::{Unit, format_measure, units::parse_measure};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::theme::semibold;

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
            "textFrameOptions" => json!({"columns": 1, "gutter": "1p0", "inset": "0p0", "verticalJustification": "top"}),
            "documentSetup" => json!({}),
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
        if id == "paragraphStyleOptions" {
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

fn keyboard_shortcuts(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let mut q = d.s("query");
    ui.add(
        egui::TextEdit::singleline(&mut q)
            .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search commands")))
            .desired_width(f32::INFINITY),
    );
    d.fields.insert("query".into(), json!(q));
    let recording = d.s("recording");
    // Capture the next key press for the command being recorded.
    if !recording.is_empty() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
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
fn preferences(app: &crate::DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
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
        // A fixed height: the separator would otherwise take the whole window.
        ui.set_min_height(240.0);
        ui.set_max_height(240.0);
        ui.vertical(|ui| {
            ui.set_width(150.0);
            for (id, label) in sections {
                if ui.selectable_label(cur == *id, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    d.fields.insert("section".into(), json!(id));
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_width(300.0);
            match cur.as_str() {
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
            }
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
    let title = match d.id.as_str() {
        "ruby" => crate::i18n::tr(&app.ui.language, "Ruby"),
        "newDocument" => crate::i18n::tr(&app.ui.language, "New Document"),
        "frameSize" => crate::i18n::tr(&app.ui.language, "Rectangle"),
        "goToPage" => crate::i18n::tr(&app.ui.language, "Go to Page"),
        "insertTable" => crate::i18n::tr(&app.ui.language, "Create Table"),
        "textFrameOptions" => crate::i18n::tr(&app.ui.language, "Text Frame Options"),
        "documentSetup" => crate::i18n::tr(&app.ui.language, "Document Setup"),
        "findChange" => crate::i18n::tr(&app.ui.language, "Find/Change"),
        "paragraphStyleOptions" => crate::i18n::tr(&app.ui.language, "Paragraph Style Options"),
        "footnoteOptions" => crate::i18n::tr(&app.ui.language, "Footnote Options"),
        "insertXref" => crate::i18n::tr(&app.ui.language, "New Cross-Reference"),
        "findFont" => crate::i18n::tr(&app.ui.language, "Find/Replace Font"),
        "polygonSettings" => crate::i18n::tr(&app.ui.language, "Polygon Settings"),
        "userDictionary" => crate::i18n::tr(&app.ui.language, "User Dictionary"),
        "newWorkspace" => crate::i18n::tr(&app.ui.language, "New Workspace"),
        "menus" => crate::i18n::tr(&app.ui.language, "Menu Customization"),
        "importOptions" => crate::i18n::tr(&app.ui.language, "Import Options"),
        "fittingOptions" => crate::i18n::tr(&app.ui.language, "Frame Fitting Options"),
        "qrCode" => crate::i18n::tr(&app.ui.language, "Generate QR Code"),
        "preferences" => crate::i18n::tr(&app.ui.language, "Preferences"),
        "print" => crate::i18n::tr(&app.ui.language, "Print"),
        "pdfImport" => crate::i18n::tr(&app.ui.language, "Place PDF"),
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
        ui.set_min_width(380.0);
        ui.set_max_width(if d.id == "newDocument" && crate::i18n::is_rtl(&app.ui.language) { 380.0 } else { 640.0 });
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
            "footnoteOptions" => footnote_options(app, ui, &mut d),
            "insertXref" => insert_xref(app, ui, &mut d),
            "findFont" => find_font(app, ui, &mut d),
            "colorPicker" => color_picker(ui, &mut d),
            "preferences" => preferences(app, ui, &mut d),
            "print" => print_dialog(app, ui, &mut d),
            "pdfImport" => {
                ui.label(egui::RichText::new(d.s("path")).size(11.0));
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
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Columns"));
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
                egui::Grid::new("tfo").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Number of columns"));
                    text_field(ui, &mut d, "columns", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Gutter"));
                    text_field(ui, &mut d, "gutter", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Inset spacing"));
                    text_field(ui, &mut d, "inset", 80.0);
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Vertical justification"));
                    let cur = d.s("verticalJustification");
                    egui::ComboBox::from_id_salt("vj").selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &cur))).show_ui(ui, |ui| {
                        for v in ["top", "center", "bottom", "justify"] {
                            if ui.selectable_label(cur == v, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, v))).clicked() {
                                d.fields.insert("verticalJustification".into(), json!(v));
                            }
                        }
                    });
                    ui.end_row();
                });
            }
            "documentSetup" => document_setup(app, ui, &mut d),
            _ => {}
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "  OK  ")).color(egui::Color32::WHITE)))
                            .fill(crate::theme::Tokens::get(ui.ctx()).accent_strong),
                    )
                    .clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    result = Some(true);
                }
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Cancel"))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    result = Some(false);
                }
            });
        });
    });
    app.ui.dialog = Some(d);
    match result {
        Some(true) => {
            let _ = confirm(app);
        }
        Some(false) => app.ui.dialog = None,
        None => {}
    }
}

/// Apply the open dialog.
pub fn confirm(app: &mut DesignApp) -> Result<Value, String> {
    let Some(d) = app.ui.dialog.take() else { return Err("no dialog open".into()) };
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
        "textFrameOptions" => app.run(
            "object.textFrameOptions",
            json!({"columns": d.n("columns").unwrap_or(1.0) as u64, "gutter": d.m("gutter").unwrap_or(12.0), "inset": d.m("inset").unwrap_or(0.0), "verticalJustification": d.s("verticalJustification")}),
        ),
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
                    para.insert(a.into(), v.clone());
                } else if let Some(a) = k.strip_prefix("c.") {
                    chars.insert(a.into(), v.clone());
                }
            }
            let mut params = json!({"name": name, "para": para, "chars": chars});
            let based = d.s("basedOn");
            if !based.is_empty() {
                params["basedOn"] = if based == "[No Paragraph Style]" { Value::Null } else { json!(based) };
            }
            let rename = d.s("rename");
            if !rename.is_empty() && rename != name {
                params["rename"] = json!(rename);
            }
            let r = app.run("style.paragraph.edit", params)?;
            if d.fields.contains_key("x.tag") {
                let tag = d.s("x.tag");
                let tag = if tag == "[Automatic]" { String::new() } else { tag };
                let style = if !rename.is_empty() && rename != name { rename } else { name };
                app.run("style.exportTag", json!({"style": style, "tag": tag, "class": d.s("x.class")}))?;
            }
            Ok(r)
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
            app.run("file.place", json!({"path": d.s("path"), "pdfPage": d.n("page").unwrap_or(1.0).max(1.0) as u64, "pdfCrop": if crop.is_empty() { "crop".to_string() } else { crop }}))
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
        "preferences" => {
            app.run(
                "prefs.set",
                json!({"scaleStrokes": d.b("scaleStrokes"), "dimensionsIncludeStroke": d.b("dimensionsIncludeStroke"), "transformationsAreTotals": d.b("transformationsAreTotals"), "absolutePageNumbers": d.b("absolutePageNumbers"), "highlightHj": d.b("highlightHj"), "highlightKeeps": d.b("highlightKeeps"), "highlightCustomTracking": d.b("highlightCustomTracking"), "highlightSubstitutedFonts": d.b("highlightSubstitutedFonts"), "richBlackOutput": d.b("richBlackOutput"), "typographersQuotes": d.b("typographersQuotes"), "smartTextReflow": d.b("smartTextReflow"),
                    "autocorrect": d.b("autocorrect"), "showAddedText": d.b("showAddedText"), "showNoteAnchors": d.b("showNoteAnchors"),
                    "recoveryMinutes": d.n("recoveryMinutes").unwrap_or(0.5),
                    "autocorrectList": d.s("autocorrectText").lines().filter_map(|l| {
                        let (a, b) = l.split_once('→').or_else(|| l.split_once("->"))?;
                        let (a, b) = (a.trim().to_lowercase(), b.trim().to_string());
                        (!a.is_empty() && !b.is_empty()).then(|| json!([a, b]))
                    }).collect::<Vec<_>>()}),
            )?;
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
            app.run("document.preferences", doc)
        }
        "newWorkspace" => app.run("window.newWorkspace", json!({"name": d.s("name")})),
        "importOptions" => app.run(
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
                    Some(Value::Bool(b)) => {
                        p.insert(f.key, json!(b));
                    }
                    _ => {}
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
    let Some(st) = app.session.active() else { return };
    let Some(style) = st.doc.styles.para(&name).cloned() else {
        ui.label(format!("No style named {name}"));
        return;
    };
    let names: Vec<String> = st.doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| *n != name).collect();
    let (pp, cp) = st.doc.styles.resolve_para_style(&name);
    let units = st.doc.settings.horizontal_units;
    let pv = serde_json::to_value(&pp).unwrap_or_default();
    let cv = serde_json::to_value(&cp).unwrap_or_default();
    let cur = |d: &Dialog, k: &str, base: &Value| d.fields.get(k).cloned().unwrap_or_else(|| base.clone());
    ui.set_min_width(560.0);
    ui.horizontal_top(|ui| {
        // A fixed height: the separator would otherwise stretch the dialog to the window.
        ui.set_min_height(380.0);
        ui.set_max_height(380.0);
        ui.vertical(|ui| {
            ui.set_width(170.0);
            for (id, label) in [
                ("general", "General"),
                ("chars", "Basic Character Formats"),
                ("indents", "Indents and Spacing"),
                ("hyph", "Hyphenation"),
                ("justify", "Justification"),
                ("nested", "Drop Caps and Nested Styles"),
                ("grep", "GREP Style"),
                ("color", "Character Color"),
                ("export", "Export Tagging"),
            ] {
                if ui.selectable_label(d.s("section") == id, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                    d.fields.insert("section".into(), json!(id));
                }
            }
        });
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
                        if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
                            ns["count"] = json!(n);
                            changed = true;
                        }
                        let kind = ns["until"]["kind"].as_str().unwrap_or("words").to_string();
                        egui::ComboBox::from_id_salt(("ns_until", i))
                            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, &kind)))
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
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "for"));
                        let mut n = l["lines"].as_u64().unwrap_or(1) as u32;
                        if ui.add(egui::DragValue::new(&mut n).range(1..=999)).changed() {
                            l["lines"] = json!(n);
                            changed = true;
                        }
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, if n == 1 { "line" } else { "lines" }));
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
            "chars" => {
                egui::Grid::new("psc").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Family:"));
                    let fam = cur(d, "c.fontFamily", &cv["fontFamily"]).as_str().unwrap_or("").to_string();
                    egui::ComboBox::from_id_salt("psfam").selected_text(&fam).width(200.0).show_ui(ui, |ui| {
                        for f in designcraft_fonts::FontDb::global().families() {
                            if ui.selectable_label(f == fam, &f).clicked() {
                                d.fields.insert("c.fontFamily".into(), json!(f));
                            }
                        }
                    });
                    ui.end_row();
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Style:"));
                    let sty = cur(d, "c.fontStyle", &cv["fontStyle"]).as_str().unwrap_or("").to_string();
                    egui::ComboBox::from_id_salt("pssty").selected_text(&sty).width(200.0).show_ui(ui, |ui| {
                        for s in designcraft_fonts::FontDb::global().styles(&fam) {
                            if ui.selectable_label(s == sty, &s).clicked() {
                                d.fields.insert("c.fontStyle".into(), json!(s));
                            }
                        }
                    });
                    ui.end_row();
                    for (label, key, suffix) in [("Size:", "size", " pt"), ("Tracking:", "tracking", "")] {
                        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, label));
                        let v = cur(d, &format!("c.{key}"), &cv[key]).as_f64();
                        if let Some(n) = crate::widgets::number(ui, &format!("ps{key}"), v, suffix, 80.0, 2) {
                            d.fields.insert(format!("c.{key}"), json!(n));
                        }
                        ui.end_row();
                    }
                    crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Leading:"));
                    let lv = match cur(d, "c.leading", &cv["leading"]) {
                        v if v["kind"] == "points" => v["value"].as_f64(),
                        _ => None,
                    };
                    if let Some(n) = crate::widgets::number(ui, "pslead", lv, " pt", 80.0, 2) {
                        d.fields.insert("c.leading".into(), json!({"kind": "points", "value": n}));
                    }
                    ui.end_row();
                });
            }
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
                });
            }
            "color" => {
                let cur_fill = cur(d, "c.fill", &cv["fill"]).as_str().unwrap_or("").to_string();
                let swatches: Vec<String> = st.doc.swatches.iter().map(|s| s.name.clone()).collect();
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for sw in swatches {
                        let (c, g) = crate::widgets::swatch_colors(&st.doc, &sw, 1.0);
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            crate::widgets::paint_chip(ui.painter(), r, c, g);
                            if ui.selectable_label(sw == cur_fill, &sw).clicked() {
                                d.fields.insert("c.fill".into(), json!(sw));
                            }
                        });
                    }
                });
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
        let part = part.trim();
        let (k, spec) = match part.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (part, ""),
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
    let db = designcraft_fonts::FontDb::global();
    let families = db.families();
    let fam_opts: Vec<(&str, &str)> = families.iter().map(|f| (f.as_str(), f.as_str())).collect();
    egui::Grid::new("ff_to").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        crate::rtl::label(ui, crate::i18n::tr(&app.ui.language, "Font Family:"));
        combo(ui, d, "toFamily", &fam_opts);
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
        // Every command with a "…" label parses without panicking.
        for c in designcraft_engine::command_specs() {
            let _ = command_fields(c.params);
        }
    }
}
