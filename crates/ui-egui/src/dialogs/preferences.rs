//! Preferences on the framework's form components: the section list and each section's options. The form edits the
//! same `Dialog` draft the native body (`preferences` in `dialogs.rs`) edits, and OK and Cancel are the host's, so
//! validation, the saved settings and Cancel's return to the language and colour theme the dialog opened with are the
//! existing controller's. Interface Language and Color Theme take effect at once, as the native window's do.

use craft_behavior::{EditEvents, Value as Bindable};
use craft_theme::{Rgba, Srgba};
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup, Swatch};
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Scroll, Show as _, ShowContainer as _};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::Field;
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::style::TypeRole;
use craft_ui_widgets::text::{HexField, NumberField, TextField};
use craft_ui_widgets::value_drag::Slider;
use designcraft_geom::Unit;
use egui::Ui;
use serde_json::{Value, json};

use super::Dialog;
use super::framework::Form;
use crate::DesignApp;

/// Edit › Preferences.
pub(crate) const FORM: Form = Form::new(show).width(640.0);

/// The sections with a document open: application options and the document's own settings (InDesign keeps those with
/// the document).
const DOCUMENT_SECTIONS: &[(&str, &str)] = &[
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
];

/// The sections with no document open: application options, and Units & Increments for new documents.
const APP_SECTIONS: &[(&str, &str)] = &[
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
];

/// The height both panes keep, so a lower section's options stay in view.
const PANE_HEIGHT: f32 = 440.0;

/// The value after a control's `events`, when they changed `before`.
fn edited<T: Bindable>(events: EditEvents<T>, before: T) -> Option<T> {
    let mut next = before.clone();
    for event in events {
        event.apply(&mut next);
    }
    (next != before).then_some(next)
}

/// The section list on the left and the chosen section's options on the right.
fn show(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let has_doc = d.fields.contains_key("horizontalUnits") && !d.b("defaults");
    let sections = if has_doc { DOCUMENT_SECTIONS } else { APP_SECTIONS };
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
    // A section the list doesn't have shows General, as the native body does.
    let shown = d.s("section");
    let current = sections.iter().map(|(id, _)| *id).find(|id| *id == shown).unwrap_or("general");
    Row::new().id_salt("preferences_form").fill_width().show(ui, |ui, _| {
        Stack::new().id_salt("preferences_sections").scroll(Scroll::vertical()).room_height(PANE_HEIGHT).show(ui, |ui, _| {
            let group = RadioGroup::new(tr("Preferences"), Bind::given(current)).id_salt("preferences_section_list").show(ui, |ui, _| {
                for (id, label) in sections {
                    Radio::new(*id).label(tr(label)).id_salt(("preferences_section", *id)).show(ui);
                }
            });
            if let Some(next) = edited(group.output.events, current) {
                d.fields.insert("section".into(), json!(next));
            }
        });
        Stack::new().id_salt(("preferences_options", current)).fill_width().scroll(Scroll::vertical()).room_height(PANE_HEIGHT).show(ui, |ui, _| {
            match current {
                "dictionary" => dictionary(ui, d, &lang),
                "spelling" => {
                    heading(ui, &tr("Dynamic Spelling"));
                    check(ui, d, "dynamicSpelling", &tr("Enable Dynamic Spelling (underline misspelled words)"));
                }
                "autocorrect" => autocorrect(ui, d, &lang),
                "notes" => {
                    heading(ui, &tr("Options"));
                    check(ui, d, "showNoteAnchors", &tr("Show Note Anchors in Layout View"));
                }
                "trackChanges" => {
                    heading(ui, &tr("Show"));
                    check(ui, d, "showAddedText", &tr("Added Text (highlighted)"));
                }
                "storyEditor" => {
                    heading(ui, &tr("Text Display Options"));
                    slider(ui, d, "storyEditorSize", &tr("Font Size:"), Range { default: 14.0, min: 8.0, max: 36.0, step: 1.0 }, " pt");
                }
                "files" => files(ui, d, &lang),
                "type" => type_options(ui, d, &lang),
                "composition" => composition(ui, d, &lang),
                "advancedType" => advanced_type(ui, d, &lang),
                "black" => black(ui, d, &lang),
                "interface" => interface(app, ui, d),
                "uiScaling" => {
                    heading(ui, &tr("UI Scaling"));
                    slider(ui, d, "uiScale", &tr("UI Size:"), Range { default: 100.0, min: 50.0, max: 200.0, step: 5.0 }, "%");
                    heading(ui, &tr("Panels"));
                    check(ui, d, "autoCollapseIconicPanels", &tr("Auto-Collapse Iconic Panels"));
                }
                "grids" => grids(ui, d, &lang),
                "guides" => guides(ui, d, &lang),
                "display" => {
                    heading(ui, &tr("Options"));
                    let choices = [("fast", "Fast"), ("typical", "Typical"), ("high", "High Quality")];
                    // An unknown quality shows as High Quality, as the native field does.
                    let cur = d.s("displayQuality");
                    let held = if matches!(cur.as_str(), "fast" | "typical") { cur } else { "high".to_owned() };
                    choose_text(ui, d, "displayQuality", &tr("Default View:"), &held, &choices, &lang);
                }
                "units" => units(ui, d, &lang),
                _ => general(app, ui, d),
            }
            if let Some(status) = d.fields.get("status").and_then(Value::as_str) {
                Text::new(status.to_owned()).shade(Shade::Muted).show(ui);
            }
        });
    });
    None
}

/// A heading over a group of options.
fn heading(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).role(TypeRole::BodyStrong).show(ui);
}

/// A check box over the boolean `key`.
fn check(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let on = d.b(key);
    let out = Checkbox::new(Bind::given(on)).label(label.to_owned()).id_salt(("preferences_check", key.to_owned())).show(ui);
    if let Some(next) = edited(out.events, on) {
        d.fields.insert(key.into(), Value::Bool(next));
    }
}

/// A text field named `label` over the text `key`: the draft holds what is typed, and OK reads and checks it.
fn text_field(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let current = d.s(key);
    let out =
        TextField::new(label.to_owned(), Bind::given(current.clone())).id_salt(("preferences_text", key.to_owned())).limit(None::<usize>).show(ui);
    if let Some(next) = edited(out.events, current) {
        d.fields.insert(key.into(), Value::String(next));
    }
}

/// [`text_field`] with its caption before it and `after` (a unit) after it, on one row.
fn labelled_text(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, after: Option<&str>) {
    Row::new().id_salt(("preferences_text_row", key.to_owned())).show(ui, |ui, _| {
        Field::new(label.to_owned()).id_salt(("preferences_text_field", key.to_owned())).show(ui, |ui, _| text_field(ui, d, key, label));
        if let Some(after) = after {
            Text::new(after.to_owned()).show(ui);
        }
    });
}

/// A multi-line text field of `lines` lines over the text `key`.
fn multiline(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, lines: usize) {
    let current = d.s(key);
    let out = TextField::new(label.to_owned(), Bind::given(current.clone()))
        .id_salt(("preferences_lines", key.to_owned()))
        .lines(lines)
        .max_lines(lines)
        .limit(None::<usize>)
        .show(ui);
    if let Some(next) = edited(out.events, current) {
        d.fields.insert(key.into(), Value::String(next));
    }
}

/// A select named `label` over the text `key`, among `choices` (value and caption, translated here), showing `held`.
fn choose_text(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, held: &str, choices: &[(&'static str, &str)], lang: &str) {
    let entries: Vec<Entry<&'static str>> = choices.iter().map(|(v, l)| Entry::new(*v, crate::i18n::tr(lang, l).to_owned())).collect();
    let current = choices.iter().map(|(v, _)| *v).find(|v| *v == held).unwrap_or(choices[0].0);
    Field::new(label.to_owned()).id_salt(("preferences_choice_field", key.to_owned())).show(ui, |ui, _| {
        let out = Select::new(label.to_owned(), Bind::given(current), &entries).id_salt(("preferences_choice", key.to_owned())).show(ui);
        if let Some(next) = edited(out.events, current) {
            d.fields.insert(key.into(), json!(next));
        }
    });
}

/// A boolean `key` shown as a choice between two captions (Appearance of Black), named `label`.
fn choose_bool(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, captions: [&str; 2], lang: &str) {
    let entries = [Entry::new(false, crate::i18n::tr(lang, captions[0]).to_owned()), Entry::new(true, crate::i18n::tr(lang, captions[1]).to_owned())];
    let current = d.b(key);
    Field::new(label.to_owned()).id_salt(("preferences_bool_field", key.to_owned())).show(ui, |ui, _| {
        let out = Select::new(label.to_owned(), Bind::given(current), &entries).id_salt(("preferences_bool", key.to_owned())).show(ui);
        if let Some(next) = edited(out.events, current) {
            d.fields.insert(key.into(), json!(next));
        }
    });
}

/// A number's default (while the draft has none), its range and its step.
#[derive(Clone, Copy)]
struct Range {
    default: f64,
    min: f64,
    max: f64,
    step: f64,
}

/// A slider named `label` over the number `key`, with its value and `suffix` after it.
fn slider(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, range: Range, suffix: &str) {
    let value = d.n(key).unwrap_or(range.default);
    Row::new().id_salt(("preferences_slider_row", key.to_owned())).show(ui, |ui, _| {
        Field::new(label.to_owned()).id_salt(("preferences_slider_field", key.to_owned())).show(ui, |ui, _| {
            let shown = format!("{value}{suffix}");
            let out = Slider::new(Bind::given(value), range.min..=range.max)
                .label(label.to_owned())
                .step(range.step)
                .value_text(shown)
                .id_salt(("preferences_slider", key.to_owned()))
                .show(ui);
            if let Some(next) = edited(out.events, value) {
                d.fields.insert(key.into(), json!(next));
            }
        });
        Text::new(format!("{value}{suffix}")).show(ui);
    });
}

/// A number field named `label` over the number `key`, written with at most `decimals`, with `suffix` after it.
fn number(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, range: Range, decimals: usize, suffix: Option<&str>) {
    let value = d.n(key).filter(|v| v.is_finite()).unwrap_or(range.default);
    Row::new().id_salt(("preferences_number_row", key.to_owned())).show(ui, |ui, _| {
        Field::new(label.to_owned()).id_salt(("preferences_number_field", key.to_owned())).show(ui, |ui, _| {
            let field = NumberField::new(label.to_owned(), Bind::given(Some(value)))
                .id_salt(("preferences_number", key.to_owned()))
                .range(range.min..=range.max)
                .step(range.step)
                .commit_decimals(decimals);
            let out = field.show(ui);
            if let Some(Some(next)) = edited(out.edit.events, Some(value)).filter(|v| v.is_some_and(f64::is_finite)) {
                // A count is a whole number ("20", not "20.0").
                let next = if decimals == 0 { next.round() } else { next };
                d.fields.insert(key.into(), json!(next));
            }
        });
        if let Some(suffix) = suffix {
            Text::new(suffix.to_owned()).show(ui);
        }
    });
}

/// A colour field over the `[r, g, b]` key: the colour, and its hex digits to type.
fn color(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let rgb: [u8; 3] = d.fields.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or([128, 128, 128]);
    let current = Rgba::rgb(rgb[0], rgb[1], rgb[2]);
    Row::new().id_salt(("preferences_color_row", key.to_owned())).show(ui, |ui, _| {
        let well = Srgba::rgb(f32::from(rgb[0]) / 255.0, f32::from(rgb[1]) / 255.0, f32::from(rgb[2]) / 255.0);
        Swatch::new(label.to_owned(), well).id_salt(("preferences_color_well", key.to_owned())).show(ui);
        Field::new(label.to_owned()).side(HexField::hash_label()).id_salt(("preferences_color_field", key.to_owned())).show(ui, |ui, _| {
            let out = HexField::hex(label.to_owned(), Bind::given(current)).id_salt(("preferences_color", key.to_owned())).show(ui);
            if let Some(next) = edited(out.events, current) {
                let [r, g, b, _] = next.0;
                d.fields.insert(key.into(), json!([r, g, b]));
            }
        });
    });
}

/// General: Interface Language (each language by its own name), then When Scaling, Transform and Page Numbering.
fn general(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
    heading(ui, &tr("Interface Language"));
    let languages = crate::i18n::picker_languages();
    let entries: Vec<Entry<&'static str>> = languages.iter().map(|(code, name)| Entry::new(*code, (*name).to_owned())).collect();
    let current = languages.iter().map(|(code, _)| *code).find(|code| *code == app.ui.language).unwrap_or("");
    let label = tr("Interface Language");
    let out = Select::new(label, Bind::given(current), &entries).id_salt("preferences_language").show(ui);
    // A pick takes effect at once, like any other app setting; Cancel goes back to the language it opened with.
    if let Some(code) = edited(out.events, current)
        && let Err(e) = app.run("app.language", json!({"lang": code}))
    {
        d.fields.insert("status".into(), json!(e));
    }
    heading(ui, &tr("When Scaling"));
    check(ui, d, "scaleStrokes", &tr("Include Stroke Weight"));
    heading(ui, &tr("Transform"));
    check(ui, d, "dimensionsIncludeStroke", &tr("Dimensions Include Stroke Weight"));
    check(ui, d, "transformationsAreTotals", &tr("Transformations are Totals"));
    heading(ui, &tr("Page Numbering"));
    check(ui, d, "absolutePageNumbers", &tr("Absolute Numbering (instead of Section Numbering)"));
}

/// Interface: the Color Theme (a swatch per theme; a click shows it at once, Cancel goes back to the theme Preferences
/// opened with) and the Tools panel's columns.
fn interface(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
    heading(ui, &tr("Appearance"));
    let current = d.s("brightness");
    let held = crate::theme::Brightness::ALL.iter().map(|b| b.id()).find(|id| *id == current).unwrap_or(crate::theme::Brightness::default().id());
    let mut picked = None;
    Row::new().id_salt("preferences_theme_row").show(ui, |ui, _| {
        Text::new(tr("Color Theme:")).show(ui);
        let group = RadioGroup::new(tr("Color Theme:"), Bind::given(held)).id_salt("preferences_theme").show(ui, |ui, _| {
            Row::new().id_salt("preferences_theme_swatches").show(ui, |ui, _| {
                for b in crate::theme::Brightness::ALL {
                    let panel = crate::theme::Tokens::for_brightness(b).panel;
                    let color = Srgba::rgb(f32::from(panel.r()) / 255.0, f32::from(panel.g()) / 255.0, f32::from(panel.b()) / 255.0);
                    let name = tr(b.label());
                    Swatch::new(name.clone(), color).member(b.id()).tooltip(name).id_salt(("preferences_theme_swatch", b.id())).show(ui);
                }
            });
        });
        picked = edited(group.output.events, held);
    });
    if let Some(id) = picked {
        d.fields.insert("brightness".into(), json!(id));
        if let Err(e) = app.run("window.brightness", json!({"brightness": id})) {
            d.fields.insert("status".into(), json!(e));
        }
    }
    heading(ui, &tr("Panels"));
    let columns = d.s("toolsColumns");
    choose_text(ui, d, "toolsColumns", &tr("Tools Panel:"), &columns, &[("single", "Single Column"), ("double", "Double Column")], &lang);
}

fn dictionary(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("User Dictionary (this document)"));
    Text::new(tr("Words, one per line:")).show(ui);
    multiline(ui, d, "userWords", &tr("Words, one per line:"), 8);
}

fn autocorrect(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Options"));
    check(ui, d, "autocorrect", &tr("Enable Autocorrect"));
    let label = tr("Misspelled word → Correction (one per line):");
    Text::new(label.clone()).show(ui);
    multiline(ui, d, "autocorrectText", &label, 8);
}

/// File Handling: how often recovery data is saved, how many recent items show, and links to placed text files.
fn files(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Document Recovery Data"));
    number(ui, d, "recoveryMinutes", &tr("Save recovery data every:"), Range { default: 0.5, min: 0.1, max: 60.0, step: 0.1 }, 1, Some("min"));
    let recent = Range { default: 20.0, min: 0.0, max: f64::from(designcraft_engine::MAX_RECENT_FILES), step: 1.0 };
    number(ui, d, "recentFilesMax", &tr("Number of Recent Items to Display:"), recent, 0, None);
    heading(ui, &tr("Links"));
    check(ui, d, "linkTextFiles", &tr("Create Links When Placing Text Files"));
}

fn type_options(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Type Options"));
    check(ui, d, "typographersQuotes", &tr("Use Typographer's Quotes"));
    check(ui, d, "showFontNamesInEnglish", &tr("Show Font Names in English"));
    check(ui, d, "cjkFeatures", &tr("Show CJK Features"));
    heading(ui, &tr("Smart Text Reflow"));
    check(ui, d, "smartTextReflow", &tr("Add and remove pages as the primary text frame's story grows and shrinks"));
}

fn composition(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Highlight"));
    check(ui, d, "highlightKeeps", &tr("Keep Violations"));
    check(ui, d, "highlightHj", &tr("H&J Violations"));
    check(ui, d, "highlightCustomTracking", &tr("Custom Tracking/Kerning"));
    check(ui, d, "highlightSubstitutedFonts", &tr("Substituted Fonts"));
    if d.fields.contains_key("glyphFallback") {
        heading(ui, &tr("Missing Glyphs"));
        check(ui, d, "glyphFallback", &tr("Draw Missing Glyphs from Fallback Fonts"));
    }
}

/// Advanced Type: the superscript and subscript size and position, in percent.
fn advanced_type(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Character Settings"));
    for (k, l) in [("superscript", "Superscript:"), ("subscript", "Subscript:")] {
        Row::new().id_salt(("preferences_advanced", k)).show(ui, |ui, _| {
            Text::new(tr(l)).show(ui);
            for f in ["Size", "Position"] {
                // Each field is named by its row and its column: "Superscript: Size".
                let name = format!("{} {}", tr(l), tr(f));
                labelled_text(ui, d, &format!("adv.{k}{f}"), &name, Some("%"));
            }
        });
    }
}

/// Appearance of Black: on screen and for output, all blacks accurately or as rich black; [Black] Overprint.
fn black(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Options for Black on RGB and Grayscale Devices"));
    choose_bool(ui, d, "richBlack", &tr("On Screen:"), ["Display All Blacks Accurately", "Display All Blacks as Rich Black"], lang);
    choose_bool(ui, d, "richBlackOutput", &tr("Printing / Exporting:"), ["Output All Blacks Accurately", "Output All Blacks as Rich Black"], lang);
    if d.fields.contains_key("overprintBlack") {
        heading(ui, &tr("[Black] Overprint"));
        check(ui, d, "overprintBlack", &tr("Overprint [Black] Swatch at 100%"));
    }
}

/// Grids: the baseline grid and the document grid.
fn grids(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Baseline Grid"));
    color(ui, d, "bg.color", &tr("Color:"));
    labelled_text(ui, d, "bg.start", &tr("Start:"), None);
    let relative = if d.s("bg.relativeTo") == "topMargin" { "topMargin" } else { "topOfPage" };
    choose_text(ui, d, "bg.relativeTo", &tr("Relative To:"), relative, &[("topOfPage", "Top of Page"), ("topMargin", "Top Margin")], lang);
    labelled_text(ui, d, "bg.increment", &tr("Increment Every:"), None);
    labelled_text(ui, d, "bg.viewThreshold", &tr("View Threshold:"), Some("%"));
    heading(ui, &tr("Document Grid"));
    color(ui, d, "grid.color", &tr("Color:"));
    labelled_text(ui, d, "grid.horizontal", &tr("Horizontal Gridline Every:"), None);
    labelled_text(ui, d, "grid.vertical", &tr("Vertical Gridline Every:"), None);
    labelled_text(ui, d, "grid.subdivisions", &tr("Subdivisions:"), None);
    check(ui, d, "grid.inBack", &tr("Grids in Back"));
}

/// Guides & Pasteboard: the guide colours, the pasteboard margins and the smart guides.
fn guides(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Color"));
    for (k, l) in [("marginColor", "Margins:"), ("columnColor", "Columns:"), ("bleedColor", "Bleed:"), ("slugColor", "Slug:")] {
        color(ui, d, k, &tr(l));
    }
    heading(ui, &tr("Pasteboard Options"));
    labelled_text(ui, d, "pasteboard.h", &tr("Horizontal Margins:"), None);
    labelled_text(ui, d, "pasteboard.v", &tr("Vertical Margins:"), None);
    check(ui, d, "snap.alignEdges", &tr("Align to Object Edges"));
    check(ui, d, "snap.alignCenters", &tr("Align to Object Centers"));
    check(ui, d, "snap.dimensions", &tr("Smart Dimensions"));
    check(ui, d, "snap.spacing", &tr("Smart Spacing"));
    number(ui, d, "snap.zone", &tr("Snap to Zone"), Range { default: 4.0, min: 0.0, max: 1000.0, step: 0.1 }, 2, None);
}

/// Units & Increments: the ruler origin and units, and the keyboard increments.
fn units(ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_owned();
    heading(ui, &tr("Ruler Units"));
    let origin = d.fields.get("rulerOrigin").and_then(Value::as_str).unwrap_or("spread").to_owned();
    choose_text(ui, d, "rulerOrigin", &tr("Origin:"), &origin, &[("spread", "Spread"), ("page", "Page"), ("spine", "Spine")], lang);
    let entries: Vec<Entry<Unit>> = Unit::ALL.iter().map(|u| Entry::new(*u, tr(u.label()))).collect();
    for (key, label) in [("horizontalUnits", "Horizontal:"), ("verticalUnits", "Vertical:")] {
        let current: Unit = d.fields.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let label = tr(label);
        Field::new(label.clone()).id_salt(("preferences_unit_field", key)).show(ui, |ui, _| {
            let out = Select::new(label.clone(), Bind::given(current), &entries).id_salt(("preferences_unit", key)).show(ui);
            if let Some(next) = edited(out.events, current) {
                d.fields.insert(key.into(), json!(next));
            }
        });
    }
    heading(ui, &tr("Keyboard Increments"));
    labelled_text(ui, d, "keyboardIncrement", &tr("Cursor Key:"), None);
    labelled_text(ui, d, "kerningIncrement", &tr("Kerning/Tracking:"), Some(&tr("/1000 em")));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Brightness;
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, vec2};
    use egui_kittest::{Harness, kittest::Queryable};

    type Window = Harness<'static, DesignApp>;

    /// An app with a document, its Preferences open on `section`.
    fn with_preferences(section: &str) -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = String::new();
        app.run("file.new", json!({})).unwrap();
        app.run("app.preferences", json!({})).unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("section".into(), json!(section));
        app
    }

    /// The app's dialog host with the framework installed, as the desktop shell runs it.
    fn open(mut app: DesignApp) -> Window {
        let ctx = egui::Context::default();
        app.framework_transform = false;
        app.framework_opacity = true;
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert!(app.framework_installed, "the framework form needs the installed runtime");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(1100.0, 900.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn event(h: &mut Window, event: Event) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn steps(h: &mut Window, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn key(h: &mut Window, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        steps(h, 3);
    }

    fn click_at(h: &mut Window, at: egui::Pos2) {
        event(h, Event::PointerMoved(at));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 3);
    }

    fn click(h: &mut Window, role: Role, label: &str) {
        let at = h.query_all_by_role_and_label(role, label).next().unwrap_or_else(|| panic!("no {role:?} named {label}")).rect().center();
        click_at(h, at);
    }

    /// Chooses `option` in the select named `label`: opens it, then clicks the listed entry.
    fn pick(h: &mut Window, label: &str, option: &str) {
        click(h, Role::ComboBox, label);
        let at = h.query_all_by_label(option).last().unwrap_or_else(|| panic!("{option} is listed")).rect().center();
        click_at(h, at);
    }

    /// Types `text` over what the field named `label` holds, by real pointer and key events, then Tab commits it.
    fn type_into(h: &mut Window, label: &str, text: &str) {
        click(h, Role::TextInput, label);
        assert!(h.get_by_role_and_label(Role::TextInput, label).is_focused(), "a click focuses {label}");
        for pressed in [true, false] {
            event(h, Event::Key { key: Key::A, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND });
        }
        event(h, Event::Text(text.into()));
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        key(h, Key::Tab);
    }

    fn shows(h: &Window, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    fn dialog(h: &Window) -> &Dialog {
        h.state().ui.dialog.as_ref().expect("Preferences is open")
    }

    #[test]
    fn preferences_has_a_framework_form() {
        assert!(super::super::framework::form("preferences").is_some(), "Preferences is registered with the host");
    }

    #[test]
    fn the_section_list_shows_the_chosen_sections_options() {
        let mut h = open(with_preferences("general"));
        assert!(shows(&h, "Interface Language"), "General comes first");
        for (_, label) in DOCUMENT_SECTIONS {
            assert_eq!(h.query_all_by_role_and_label(Role::RadioButton, label).count(), 1, "{label} is listed once");
        }
        click(&mut h, Role::RadioButton, "Units & Increments");
        assert_eq!(dialog(&h).s("section"), "units");
        assert!(shows(&h, "Ruler Units") && shows(&h, "Keyboard Increments"));
        assert!(!shows(&h, "When Scaling"), "only the chosen section shows");
        // Without a document the list is the application's, with Units & Increments for new documents.
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = String::new();
        crate::menus::activate(&mut app, "app.preferences", &Value::Null);
        let h = open(app);
        assert!(h.query_by_role_and_label(Role::RadioButton, "Grids").is_none(), "Grids belong to a document");
        assert!(h.query_by_role_and_label(Role::RadioButton, "Units & Increments").is_some());
    }

    #[test]
    fn a_check_box_edits_the_draft_and_ok_saves_it() {
        let mut h = open(with_preferences("general"));
        let before = h.state().session.prefs.scale_strokes;
        click(&mut h, Role::CheckBox, "Include Stroke Weight");
        assert_eq!(dialog(&h).b("scaleStrokes"), !before, "the draft holds the change");
        assert_eq!(h.state().session.prefs.scale_strokes, before, "nothing is saved before OK");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.prefs.scale_strokes, !before);
    }

    /// The framework successor of `preferences_interface_sets_the_color_theme_and_tools_panel`'s checks, which keep
    /// driving the native window: a Color Theme swatch shows its theme at once and Cancel goes back; OK keeps the theme
    /// and the Tools panel's columns.
    #[test]
    fn a_theme_swatch_shows_at_once_cancel_goes_back_and_ok_keeps_it_with_the_tools_columns() {
        let opened = with_preferences("interface").ui.brightness;
        let mut h = open(with_preferences("interface"));
        for label in ["Appearance", "Color Theme:", "Panels", "Tools Panel:"] {
            assert!(shows(&h, label), "{label}");
        }
        click(&mut h, Role::RadioButton, "Light");
        assert_eq!(h.state().ui.brightness, Brightness::Light, "a click shows the theme at once");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().ui.brightness, opened, "Cancel goes back to the theme Preferences opened with");

        let mut h = open(with_preferences("interface"));
        click(&mut h, Role::RadioButton, "Dark");
        pick(&mut h, "Tools Panel:", "Double Column");
        assert_eq!(dialog(&h).s("toolsColumns"), "double");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().ui.brightness, Brightness::Dark);
        assert!(h.state().ui.tools_double_column);
    }

    /// The framework successor of `preferences_first_setting_switches_interface_language`'s pick and Cancel, which
    /// keeps driving the native window.
    #[test]
    fn a_language_pick_applies_at_once_and_cancel_puts_back_the_one_it_opened_with() {
        let mut h = open(with_preferences("general"));
        pick(&mut h, "Interface Language", "Deutsch");
        assert_eq!(h.state().ui.language, "de", "a pick takes effect at once");
        assert_eq!(dialog(&h).id, "preferences", "the dialog stays open");
        assert!(shows(&h, crate::i18n::tr("de", "Interface Language")), "the form reads in German");
        click(&mut h, Role::Button, crate::i18n::tr("de", "Cancel").trim());
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().ui.language, "", "Cancel goes back to the language it opened with");
    }

    #[test]
    fn units_and_a_typed_increment_set_the_documents_rulers_and_a_bad_increment_is_refused() {
        let mut h = open(with_preferences("units"));
        pick(&mut h, "Horizontal:", "Inches");
        pick(&mut h, "Vertical:", "Inches");
        type_into(&mut h, "Cursor Key:", "0.5 pt");
        assert_eq!(dialog(&h).s("keyboardIncrement"), "0.5 pt");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let st = &h.state().session.active().unwrap().doc.settings;
        assert_eq!((st.horizontal_units, st.vertical_units, st.keyboard_increment), (Unit::Inches, Unit::Inches, 0.5));

        let mut h = open(with_preferences("units"));
        type_into(&mut h, "Cursor Key:", "wide");
        click(&mut h, Role::Button, "OK");
        assert_eq!(dialog(&h).id, "preferences", "text that isn't a measure keeps the dialog open");
        assert_eq!(h.state().session.active().unwrap().doc.settings.keyboard_increment, st_increment(), "the saved increment stays");
    }

    /// The document's keyboard increment when it is made: what a refused edit must leave.
    fn st_increment() -> f64 {
        let mut session = designcraft_engine::Session::new();
        session.execute("file.new", &json!({})).unwrap();
        session.active().unwrap().doc.settings.keyboard_increment
    }

    #[test]
    fn tab_moves_through_the_sections_controls() {
        let mut h = open(with_preferences("notes"));
        click(&mut h, Role::RadioButton, "Notes");
        key(&mut h, Key::Tab);
        let check = h.get_by_role_and_label(Role::CheckBox, "Show Note Anchors in Layout View");
        assert!(check.is_focused(), "Tab goes from the section list to the section's first option");
        let before = dialog(&h).b("showNoteAnchors");
        key(&mut h, Key::Space);
        assert_eq!(dialog(&h).b("showNoteAnchors"), !before, "Space toggles it in the draft");
    }
}
