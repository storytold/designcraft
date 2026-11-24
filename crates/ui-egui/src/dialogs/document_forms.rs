//! Framework forms of Color Settings, Document Setup (with Adjust Layout, which accepts at once) and File Info. They edit
//! the drafts the native bodies edit; OK and Cancel go through the host's `confirm` and `cancel`, so the colour settings,
//! the document and its undo steps are what the native windows made.

use super::find_forms::picker;
use super::frame_forms::{captioned, check, field, message, select, text, text_chars};
use super::framework::Form;
use super::{Bound, Dialog, FILE_INFO_FIELDS, SETUP_EDGES, load_color_profile};
use crate::DesignApp;
use craft_behavior::{ColumnWidth, IconId};
use craft_ui_panels::sections::{GroupBox, Section};
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::Segmented;
use craft_ui_widgets::contract::{Bind, Entry, Pad, Props as _, Show as _, ShowContainer as _, Sizing};
use craft_ui_widgets::display::{Icon, Shade, Text};
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout::{GridFit, empty_cell};
use craft_ui_widgets::layout_components::{Grid, Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::primitives::{Glyph, IconSpec};
use craft_ui_widgets::style::{SurfaceKind, UiTokens};
use craft_ui_widgets::text::{TextField, link_toggle};
use designcraft_geom::format_measure;
use egui::Ui;
use serde_json::{Value, json};

/// Color Settings.
pub(crate) const COLOR_SETTINGS: Form = Form::new(color_settings);
/// Document Setup.
pub(crate) const DOCUMENT_SETUP: Form = Form::new(document_setup);
/// File Info.
pub(crate) const FILE_INFO: Form = Form::new(file_info);

// ---------------------------------------------------------------------------------------------------------------------
// Color Settings

/// The one column the three pickers of Color Settings stand in, so RGB, CMYK and Intent are as wide as each other whatever
/// their entries' names are.
const PICKER_COLUMN: &[ColumnWidth] = &[ColumnWidth::Fixed(250.0)];

/// A picker over the draft string of `key` that fills the width of its cell: `select`, but sized by the cell, not by its
/// widest entry.
fn cell_select(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, options: &[(String, String)]) {
    let current = d.s(key);
    let entries: Vec<Entry<String>> = options.iter().map(|(value, caption)| Entry::new(value.clone(), caption.clone())).collect();
    let mut picker = Select::new(label.to_owned(), Bind::given(current.clone()), &entries)
        .keeps_name(true)
        .sizing(Sizing::FillWidth)
        .id_salt(("frame_form_select", key));
    if !options.iter().any(|(value, _)| *value == current) {
        picker = picker.placeholder(current.clone());
    }
    if let Some(next) = picker.show(ui).events.committed().cloned().filter(|next| *next != current) {
        d.fields.insert(key.into(), json!(next));
    }
}

/// `body` as the one cell of a fixed-width column.
fn in_column(ui: &mut Ui, salt: &str, mut body: impl FnMut(&mut Ui)) {
    Grid::new(PICKER_COLUMN).id_salt(("color_settings_column", salt.to_owned())).fit(GridFit::Fixed).show(ui, |ui, _| body(ui));
}

fn color_settings(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let profiles = d.fields.get("profiles").and_then(Value::as_array).cloned().unwrap_or_default();
    let named = |kind: &str| -> Vec<(String, String)> {
        profiles.iter().filter(|p| p["kind"] == kind).filter_map(|p| p["name"].as_str()).map(|name| (name.to_owned(), name.to_owned())).collect()
    };
    let (rgb, cmyk) = (named("rgb"), named("cmyk"));
    let intents: Vec<(String, String)> = designcraft_color::cms::Intent::ALL.iter().map(|i| (i.id().to_owned(), tr(i.label()))).collect();
    let mut load = false;
    Stack::new().id_salt("color_settings").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Color Settings")).id_salt("color_settings_fields").show(ui, |ui, _| {
            field(ui, &tr("RGB:"), "rgb", |ui| in_column(ui, "rgb", |ui| cell_select(ui, d, "rgb", &tr("RGB:"), &rgb)));
            field(ui, &tr("CMYK:"), "cmyk", |ui| in_column(ui, "cmyk", |ui| cell_select(ui, d, "cmyk", &tr("CMYK:"), &cmyk)));
            field(ui, &tr("Intent:"), "intent", |ui| in_column(ui, "intent", |ui| cell_select(ui, d, "intent", &tr("Intent:"), &intents)));
        });
        check(ui, d, "bpc", &tr("Use Black Point Compensation"), true);
        load |= Button::new(tr("Load Profile\u{2026}")).id_salt("color_settings_load").show(ui).clicked();
    });
    if load && let Err(e) = app.ask_open("icc", Bound::App, load_color_profile) {
        app.status_error("Color Settings", e);
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Document Setup

/// An edge field of a group: a linked group's other edges follow the one typed.
fn edge(ui: &mut Ui, d: &mut Dialog, group: &str, side: &str, label: &str) {
    let key = format!("{group}{side}");
    let before = d.s(&key);
    text(ui, d, &key, label, true);
    if d.s(&key) != before && d.b(&format!("{group}Linked")) {
        let value = d.fields.get(&key).cloned().unwrap_or_default();
        for e in SETUP_EDGES {
            d.fields.insert(format!("{group}{e}"), value.clone());
        }
    }
}

/// The link beside a group's four edges: keeps them equal; linking makes them the first's.
fn link(ui: &mut Ui, d: &mut Dialog, lang: &str, group: &str) {
    let key = format!("{group}Linked");
    let linked = d.b(&key);
    let out = link_toggle(crate::i18n::tr(lang, "Make all settings the same").to_string(), Bind::given(linked))
        .id_salt(("document_setup_link", group.to_owned()))
        .show(ui);
    if let Some(next) = out.events.committed().copied().filter(|next| *next != linked) {
        d.fields.insert(key, json!(next));
        if next && let Some(top) = d.fields.get(&format!("{group}Top")).cloned() {
            for e in SETUP_EDGES {
                d.fields.insert(format!("{group}{e}"), top.clone());
            }
        }
    }
}

/// One of the app's icons, by its name in `crate::icons`.
#[derive(craft_ui::contract::MemoProps)]
struct AppIcon(&'static str);

impl Glyph for AppIcon {
    fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: egui::Rect, color: egui::Color32, _mirrored: bool) {
        crate::icons::paint(painter, rect, self.0, color);
    }
}

static PORTRAIT: AppIcon = AppIcon("orient-portrait");
static LANDSCAPE: AppIcon = AppIcon("orient-landscape");

/// Bleed and Slug as InDesign lays them out: the row's name, the four edges and the link between them.
const BLEED_COLUMNS: &[ColumnWidth] = &[
    ColumnWidth::Content,
    ColumnWidth::Fixed(84.0),
    ColumnWidth::Fixed(84.0),
    ColumnWidth::Fixed(84.0),
    ColumnWidth::Fixed(84.0),
    ColumnWidth::Content,
];

/// File › Document Setup: intent, pages, start page, binding and the facing pages and primary text frame flags, then Page
/// Size, Margins, Bleed and Slug and Adjust Layout. Adjust Layout accepts the dialog at once with `adjustLayout` set, so the
/// objects follow the new size.
fn document_setup(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let units = super::setup_units(d);
    let intents = captioned(&lang, &[("print", "Print"), ("web", "Web"), ("mobile", "Mobile")]);
    let bindings = captioned(&lang, &[("leftToRight", "Left to Right"), ("rightToLeft", "Right to Left")]);
    let (width, height) = (super::setup_measure(d, "width").unwrap_or(612.0), super::setup_measure(d, "height").unwrap_or(792.0));
    let presets: Vec<(String, String)> = designcraft_doc::build::PRESETS.iter().map(|p| (p.name.to_owned(), tr(p.name))).collect();
    let current_preset = designcraft_doc::build::PRESETS
        .iter()
        .find(|p| {
            ((p.width - width).abs() < 0.5 && (p.height - height).abs() < 0.5) || ((p.width - height).abs() < 0.5 && (p.height - width).abs() < 0.5)
        })
        .map_or_else(|| tr("Custom"), |p| p.name.to_owned());
    let portrait = height >= width;
    let orientations = [
        Entry::new("portrait".to_owned(), tr("Portrait")).icon(IconSpec::glyph(&PORTRAIT)),
        Entry::new("landscape".to_owned(), tr("Landscape")).icon(IconSpec::glyph(&LANDSCAPE)),
    ];
    let facing = d.b("facingPages");
    let (inside, outside) = if facing { ("Inside:", "Outside:") } else { ("Left:", "Right:") };
    let mut adjust = false;
    Stack::new().id_salt("document_setup").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Document Setup")).id_salt("document_setup_fields").show(ui, |ui, _| {
            field(ui, &tr("Intent:"), "intent", |ui| select(ui, d, "intent", &tr("Intent:"), &intents, true));
            field(ui, &tr("Number of Pages:"), "pages", |ui| {
                text(ui, d, "pages", &tr("Number of Pages:"), true);
            });
            field(ui, &tr("Start Page #:"), "start_page", |ui| {
                text(ui, d, "startPage", &tr("Start Page #:"), true);
            });
            field(ui, &tr("Binding:"), "binding", |ui| select(ui, d, "binding", &tr("Binding:"), &bindings, true));
            // A check box named by its own label sits straight in the group, in the control column: inside a `Field` the
            // field's label names it and its own text is not drawn.
            check(ui, d, "facingPages", &tr("Facing Pages"), true);
            // Set when the document is made (New Document); shown here, as InDesign does.
            check(ui, d, "primaryTextFrame", &tr("Primary Text Frame"), false);
        });
        Stack::new().id_salt("document_setup_size_box").fill_width().surface(SurfaceKind::Frame).pad(Pad::Regular).show(ui, |ui, _| {
            FieldGroup::new(tr("Page Size:")).id_salt("document_setup_size").show(ui, |ui, _| {
                field(ui, &tr("Page Size:"), "preset", |ui| {
                    if let Some(next) = picker(ui, "preset", &tr("Page Size:"), &current_preset, &presets, false)
                        && let Some(p) = designcraft_doc::build::PRESETS.iter().find(|p| p.name == next)
                    {
                        // Keep the orientation; the preset's size in the document's units.
                        let (pw, ph) = if width > height { (p.width.max(p.height), p.width.min(p.height)) } else { (p.width, p.height) };
                        d.fields.insert("width".into(), json!(format_measure(pw, units)));
                        d.fields.insert("height".into(), json!(format_measure(ph, units)));
                    }
                });
                field(ui, &tr("Width:"), "width", |ui| {
                    text(ui, d, "width", &tr("Width:"), true);
                });
                field(ui, &tr("Height:"), "height", |ui| {
                    text(ui, d, "height", &tr("Height:"), true);
                });
                field(ui, &tr("Orientation:"), "orientation", |ui| {
                    let now = if portrait { "portrait" } else { "landscape" };
                    let out =
                        Segmented::new(tr("Orientation:"), Bind::given(now.to_owned()), &orientations).id_salt("document_setup_orientation").show(ui);
                    if out.events.committed().is_some_and(|next| next != now) {
                        let (w, h) = (d.s("width"), d.s("height"));
                        d.fields.insert("width".into(), json!(h));
                        d.fields.insert("height".into(), json!(w));
                    }
                });
            });
        });
        GroupBox::new(tr("Margins"), None).id_salt("document_setup_margins_box").show(ui, |ui, _| {
            // Two columns with the link between them: Top and Bottom, then Inside and Outside (Left and Right).
            Row::new().id_salt("document_setup_margins").fill_width().show(ui, |ui, _| {
                FieldGroup::new(tr("Margins")).id_salt("document_setup_margins_vertical").show(ui, |ui, _| {
                    for (caption, e) in [("Top:", "Top"), ("Bottom:", "Bottom")] {
                        field(ui, &tr(caption), &format!("margin{e}"), |ui| edge(ui, d, "margin", e, &tr(caption)));
                    }
                });
                link(ui, d, &lang, "margin");
                FieldGroup::new(tr("Margins")).id_salt("document_setup_margins_horizontal").show(ui, |ui, _| {
                    for (caption, e) in [(inside, "Inside"), (outside, "Outside")] {
                        field(ui, &tr(caption), &format!("margin{e}"), |ui| edge(ui, d, "margin", e, &tr(caption)));
                    }
                });
            });
        });
        Section::new(tr("Bleed and Slug")).id_salt("document_setup_bleed").show(ui, |ui, _| {
            Grid::new(BLEED_COLUMNS).id_salt("document_setup_bleed_grid").show(ui, |ui, _| {
                empty_cell(ui);
                for e in SETUP_EDGES {
                    Text::new(tr(e)).shade(Shade::Muted).show(ui);
                }
                empty_cell(ui);
                for (group, caption) in [("bleed", "Bleed:"), ("slug", "Slug:")] {
                    Text::new(tr(caption)).show(ui);
                    for e in SETUP_EDGES {
                        edge(ui, d, group, e, &format!("{} {}", tr(caption), tr(e)));
                    }
                    link(ui, d, &lang, group);
                }
            });
        });
        // Adjust Layout sets `adjustLayout` and accepts at once.
        Row::new().id_salt("document_setup_adjust").fill_width().show(ui, |ui, _| {
            Icon::new(IconSpec::svg(IconId::new("info"))).id_salt("document_setup_info").show(ui);
            message(ui, &tr("Adjust page elements to document changes"));
            adjust |= Button::new(tr("Adjust Layout")).id_salt("document_setup_adjust_button").show(ui).clicked();
        });
    });
    if adjust {
        d.fields.insert("adjustLayout".into(), json!(true));
        return Some(true);
    }
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// File Info

/// How many characters wide File Info's fields ask to be: a title, a URL or a notice fits without scrolling.
const FILE_INFO_CHARS: u16 = 40;

fn file_info(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    FieldGroup::new(crate::i18n::tr(&lang, "File Info").to_string()).id_salt("file_info").show(ui, |ui, _| {
        for (key, caption) in FILE_INFO_FIELDS {
            let label = crate::i18n::tr(&lang, caption).to_string();
            field(ui, &label, key, |ui| {
                if key == "description" {
                    let out = TextField::new(label.clone(), Bind::given(d.s(key)))
                        .lines(3)
                        .width_chars(FILE_INFO_CHARS)
                        .limit(None::<usize>)
                        .id_salt(("file_info_text", key))
                        .show(ui);
                    if let Some(next) = out.events.landed().filter(|next| **next != d.s(key)).cloned() {
                        d.fields.insert(key.into(), Value::String(next));
                    }
                } else {
                    text_chars(ui, d, key, &label, true, FILE_INFO_CHARS);
                }
            });
        }
    });
    None
}

#[cfg(test)]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    const BUTTON: &[Role] = &[Role::Button];
    const CHECK: &[Role] = &[Role::CheckBox];
    const RADIO: &[Role] = &[Role::RadioButton];

    /// Regression: Color Settings sends the intent and black point choice, and the profile pickers list what the dialog
    /// holds.
    #[test]
    fn color_settings_pick_intent_and_black_point_and_ok_runs_the_command() {
        let mut app = kit::app(1, false);
        kit::open(&mut app, "app.colorSettings");
        let mut h = kit::harness(app);
        // Opening the dialog read the settings through the same command: count only what the dialog sends.
        let opened = kit::journal(&h, "color.settings").len();
        let _ = h.get_by_role_and_label(Role::Dialog, "Color Settings");
        let intent = designcraft_color::cms::Intent::ALL[1];
        kit::pick(&mut h, "Intent:", &crate::i18n::tr("en", intent.label()));
        assert_eq!(kit::draft(&h).s("intent"), intent.id());
        let bpc = kit::draft(&h).b("bpc");
        kit::click(&mut h, CHECK, "Use Black Point Compensation", 0);
        assert_eq!(kit::draft(&h).b("bpc"), !bpc);
        kit::click(&mut h, BUTTON, "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let sent = kit::journal(&h, "color.settings").split_off(opened);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!((sent[0].1["intent"].as_str(), sent[0].1["bpc"].as_bool()), (Some(intent.id()), Some(!bpc)));
    }

    /// Regression: Document Setup takes a page size preset in the document's units, swaps the sides with the
    /// orientation, keeps linked margins equal, and OK sends one `layout.documentSetup` with the margins because they were
    /// edited.
    #[test]
    fn document_setup_preset_orientation_linked_margins_and_ok() {
        let mut app = kit::app(2, false);
        crate::dialogs::open_document_setup(&mut app);
        let mut h = kit::harness(app);
        // Opening the dialog read the setup through the same command: count only what the dialog sends.
        let opened = kit::journal(&h, "layout.documentSetup").len();
        let _ = h.get_by_role_and_label(Role::Dialog, "Document Setup");
        kit::pick(&mut h, "Page Size:", "A4");
        let (w, hh) = (kit::draft(&h).m("width").unwrap(), kit::draft(&h).m("height").unwrap());
        assert!((w - 595.28).abs() < 0.5 && (hh - 841.89).abs() < 0.5, "{w} x {hh}");
        kit::click(&mut h, RADIO, "Landscape", 0);
        assert!(kit::draft(&h).m("width").unwrap() > kit::draft(&h).m("height").unwrap(), "the sides swapped");
        assert!(kit::draft(&h).b("marginLinked"), "the page's margins start equal, so linked");
        kit::type_and_commit(&mut h, "Top:", 0, "10 pt");
        for side in SETUP_EDGES {
            assert_eq!(kit::draft(&h).m(&format!("margin{side}")), Some(10.0), "margin{side} follows the linked edge");
        }
        kit::click(&mut h, BUTTON, "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let sent = kit::journal(&h, "layout.documentSetup").split_off(opened);
        assert_eq!(sent.len(), 1, "{sent:?}");
        let p = &sent[0].1;
        assert_eq!(p["margins"]["top"], 10.0);
        assert_eq!(p["adjustLayout"], false);
        assert!(p["width"].as_f64().unwrap() > p["height"].as_f64().unwrap());
    }

    /// Regression: Adjust Layout accepts Document Setup at once, with the objects following the change.
    #[test]
    fn document_setup_adjust_layout_accepts_at_once() {
        let mut app = kit::app(1, false);
        crate::dialogs::open_document_setup(&mut app);
        let mut h = kit::harness(app);
        let opened = kit::journal(&h, "layout.documentSetup").len();
        kit::type_and_commit(&mut h, "Number of Pages:", 0, "3");
        kit::click(&mut h, BUTTON, "Adjust Layout", 0);
        assert!(h.state().ui.dialog.is_none(), "Adjust Layout closes the dialog");
        let sent = kit::journal(&h, "layout.documentSetup").split_off(opened);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!((sent[0].1["adjustLayout"].as_bool(), sent[0].1["pages"].as_u64()), (Some(true), Some(3)));
    }

    /// Regression: Document Setup's flags are check boxes with their own names (Primary Text Frame is dimmed), and the Bleed
    /// and Slug grid edits the bleed and the slug apart.
    #[test]
    fn document_setup_flags_and_the_bleed_and_slug_grid() {
        let mut app = kit::app(1, false);
        crate::dialogs::open_document_setup(&mut app);
        let mut h = kit::harness(app);
        assert!(kit::disabled(&h, CHECK, "Primary Text Frame", 0), "set when the document is made");
        let facing = kit::draft(&h).b("facingPages");
        kit::click(&mut h, CHECK, "Facing Pages", 0);
        assert_eq!(kit::draft(&h).b("facingPages"), !facing);
        kit::type_and_commit(&mut h, "Bleed: Top", 0, "9 pt");
        kit::type_and_commit(&mut h, "Slug: Top", 0, "4 pt");
        let draft = kit::draft(&h);
        assert_eq!((draft.m("bleedTop"), draft.m("slugTop")), (Some(9.0), Some(4.0)));
    }

    /// Regression: File Info sends the typed fields, and the keywords as a list only when they were edited.
    #[test]
    fn file_info_ok_sends_the_fields_and_parsed_keywords() {
        let mut app = kit::app(1, false);
        crate::dialogs::open_file_info(&mut app);
        let mut h = kit::harness(app);
        let _ = h.get_by_role_and_label(Role::Dialog, "File Info");
        kit::type_and_commit(&mut h, "Author:", 0, "Jane");
        kit::type_and_commit(&mut h, "Keywords:", 0, "one; two");
        kit::click(&mut h, BUTTON, "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let sent = kit::journal(&h, "file.info");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].1["author"], "Jane");
        assert_eq!(sent[0].1["keywords"], json!(["one", "two"]));
    }
}
