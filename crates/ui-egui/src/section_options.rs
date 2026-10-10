//! Layout › Numbering & Section Options, laid out like InDesign's: Start Section and the page
//! numbering of the section that starts at the current page, then the document's chapter
//! numbering. OK applies `layout.section` and `layout.chapterNumbering`.

use designcraft_doc::{ChapterSource, NumberStyle};
use egui::{Align, Layout, Ui, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;
use crate::theme::Tokens;

/// The page numbering styles and how the Style menus show them.
const STYLES: [(NumberStyle, &str); 8] = [
    (NumberStyle::Arabic, "1, 2, 3, 4…"),
    (NumberStyle::UpperRoman, "I, II, III, IV…"),
    (NumberStyle::LowerRoman, "i, ii, iii, iv…"),
    (NumberStyle::UpperLetters, "A, B, C, D…"),
    (NumberStyle::LowerLetters, "a, b, c, d…"),
    (NumberStyle::ArabicLeadingZero, "01, 02, 03…"),
    (NumberStyle::ArabicThreeDigits, "001, 002, 003…"),
    (NumberStyle::ArabicFourDigits, "0001, 0002, 0003…"),
];
/// The OK and Cancel column beside the options.
pub(crate) const ACTIONS_W: f32 = 104.0;
/// The options' height, for the window to grow to.
pub(crate) const BODY_H: f32 = 470.0;
const LABEL_W: f32 = 104.0;
const FIELD_W: f32 = 84.0;

/// Open the dialog for the section at the page in view.
pub(crate) fn open(app: &mut DesignApp) {
    let Ok(chapter) = designcraft_engine::cmd::chapter_numbering(&app.session) else { return };
    let current = crate::canvas::current_page(app).unwrap_or(0);
    let Some(st) = app.session.active() else { return };
    let doc = &st.doc;
    let page = current.min(doc.page_count().saturating_sub(1));
    let own = doc.sections.iter().find(|s| s.start == page);
    // A new section starts from the one the page is in.
    let base = own.or_else(|| doc.section_of(page));
    let fields = json!({
        "page": page + 1,
        "startSection": page == 0 || own.is_some(),
        "automatic": own.is_none_or(|s| s.start_number.is_none()),
        "startNumber": doc.page_number(page).to_string(),
        "prefix": base.map(|s| s.prefix.clone()).unwrap_or_default(),
        "style": base.map(|s| s.style).unwrap_or_default(),
        "marker": base.map(|s| s.marker.clone()).unwrap_or_default(),
        "includePrefix": base.is_some_and(|s| s.include_prefix),
        "chapterStyle": chapter.get("style").cloned().unwrap_or(json!("arabic")),
        "chapterSource": chapter.get("source").cloned().unwrap_or(json!("automatic")),
        "chapterStart": chapter.get("chapterNumber").and_then(Value::as_u64).unwrap_or(1).to_string(),
        "book": chapter.get("book").cloned().unwrap_or(Value::Null),
    });
    app.ui.dialog = Some(Dialog::new("numberingSection", fields));
}

/// A whole number typed in `key` (1 or more).
fn number(d: &Dialog, key: &str) -> Option<u64> {
    d.n(key).filter(|v| v.is_finite() && *v >= 1.0 && v.fract() == 0.0).map(|v| v as u64)
}

/// Apply the dialog: the section, then the chapter numbering. Refused values leave both as they were.
pub(crate) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let page = number(d, "page").ok_or("no page")?;
    let source: ChapterSource = d.fields.get("chapterSource").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let mut chapter = json!({"style": d.fields.get("chapterStyle").cloned().unwrap_or(json!("arabic")), "source": source});
    if source == ChapterSource::UserDefined {
        chapter["start"] = json!(number(d, "chapterStart").ok_or("Start Chapter Numbering at: a whole number from 1")?);
    }
    if page != 1 && !d.b("startSection") {
        app.run("layout.section", json!({"page": page, "remove": true}))?;
    } else {
        let start =
            if d.b("automatic") { Value::Null } else { json!(number(d, "startNumber").ok_or("Start Page Numbering at: a whole number from 1")?) };
        app.run(
            "layout.section",
            json!({"page": page, "startNumber": start, "style": d.fields.get("style").cloned().unwrap_or(json!("arabic")),
                "prefix": d.s("prefix"), "marker": d.s("marker"), "includePrefix": d.b("includePrefix")}),
        )?;
    }
    app.run("layout.chapterNumbering", chapter)
}

/// A row that runs right to left in a right-to-left interface.
fn row<R>(ui: &mut Ui, rtl: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = if rtl { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 24.0), layout, add).inner
}

/// A caption ending at the field it names (`Section Prefix:` [   ]).
fn caption(ui: &mut Ui, rtl: bool, text: &str) {
    let layout = if rtl { Layout::left_to_right(Align::Center) } else { Layout::right_to_left(Align::Center) };
    ui.allocate_ui_with_layout(vec2(LABEL_W, 22.0), layout, |ui| {
        ui.set_width(LABEL_W);
        crate::rtl::label(ui, text);
    });
}

fn text(ui: &mut Ui, d: &mut Dialog, key: &str, width: f32) {
    let mut s = d.s(key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(width)).changed() {
        d.fields.insert(key.into(), Value::String(s));
    }
}

fn style_menu(ui: &mut Ui, d: &mut Dialog, key: &str) {
    let current: NumberStyle = d.fields.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let shown = STYLES.iter().find(|(s, _)| *s == current).map_or(STYLES[0].1, |(_, l)| l);
    egui::ComboBox::from_id_salt(("section_style", key)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
        for (style, label) in STYLES {
            if ui.selectable_label(style == current, label).clicked() {
                d.fields.insert(key.into(), json!(style));
            }
        }
    });
}

fn checkbox(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let mut on = d.b(key);
    if ui.checkbox(&mut on, crate::rtl::widget(ui, label)).changed() {
        d.fields.insert(key.into(), json!(on));
    }
}

/// A titled, outlined group of options.
fn group(ui: &mut Ui, rtl: bool, title: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    row(ui, rtl, |ui| {
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(title).color(t.text_strong));
    });
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.divider)).corner_radius(3.0).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        add(ui);
    });
}

/// The options (beside the OK/Cancel column).
pub(crate) fn body(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    let rtl = crate::i18n::is_rtl(lang);
    ui.spacing_mut().item_spacing.y = 6.0;
    let first_page = number(d, "page") == Some(1);
    // Page 1 always starts a section.
    row(ui, rtl, |ui| {
        ui.add_enabled_ui(!first_page, |ui| checkbox(ui, d, "startSection", tr("Start Section")));
    });
    let section = first_page || d.b("startSection");
    ui.add_enabled_ui(section, |ui| {
        let (start, end) = if rtl { (0, 18) } else { (18, 0) };
        egui::Frame::NONE.inner_margin(egui::Margin { left: start, right: end, top: 0, bottom: 0 }).show(ui, |ui| {
            let automatic = d.b("automatic");
            row(ui, rtl, |ui| {
                if ui.radio(automatic, crate::rtl::widget(ui, tr("Automatic Page Numbering"))).clicked() {
                    d.fields.insert("automatic".into(), json!(true));
                }
            });
            row(ui, rtl, |ui| {
                if ui.radio(!automatic, crate::rtl::widget(ui, tr("Start Page Numbering at:"))).clicked() {
                    d.fields.insert("automatic".into(), json!(false));
                }
                ui.add_enabled_ui(!automatic, |ui| text(ui, d, "startNumber", FIELD_W));
            });
            ui.add_space(4.0);
            group(ui, rtl, tr("Page Numbering"), |ui| {
                row(ui, rtl, |ui| {
                    caption(ui, rtl, tr("Section Prefix:"));
                    text(ui, d, "prefix", FIELD_W);
                });
                row(ui, rtl, |ui| {
                    caption(ui, rtl, tr("Style:"));
                    style_menu(ui, d, "style");
                });
                row(ui, rtl, |ui| {
                    caption(ui, rtl, tr("Section Marker:"));
                    text(ui, d, "marker", 130.0);
                });
                row(ui, rtl, |ui| checkbox(ui, d, "includePrefix", tr("Include Prefix when Numbering Pages")));
            });
        });
    });
    ui.add_space(10.0);
    let source: ChapterSource = d.fields.get("chapterSource").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let book = d.fields.get("book").and_then(Value::as_str).map(str::to_string);
    group(ui, rtl, tr("Document Chapter Numbering"), |ui| {
        row(ui, rtl, |ui| {
            crate::rtl::label(ui, tr("Style:"));
            style_menu(ui, d, "chapterStyle");
        });
        for (value, label, enabled) in [
            (ChapterSource::Automatic, "Automatic Chapter Numbering", true),
            (ChapterSource::UserDefined, "Start Chapter Numbering at:", true),
            (ChapterSource::SameAsPrevious, "Same as Previous Document in the Book", book.is_some()),
        ] {
            row(ui, rtl, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if ui.radio(source == value, crate::rtl::widget(ui, tr(label))).clicked() {
                        d.fields.insert("chapterSource".into(), json!(value));
                    }
                });
                if value == ChapterSource::UserDefined {
                    ui.add_enabled_ui(source == value, |ui| text(ui, d, "chapterStart", FIELD_W));
                }
            });
        }
        row(ui, rtl, |ui| {
            let name = book.as_deref().map_or_else(|| tr("N/A").to_string(), crate::rtl::isolate);
            crate::rtl::label(ui, format!("{} {name}", tr("Book Name:")));
        });
    });
}

/// OK and Cancel, stacked in their column → Some(true) for OK, Some(false) for Cancel.
pub(crate) fn actions(app: &DesignApp, ui: &mut Ui) -> Option<bool> {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ACTIONS_W - 8.0, 26.0);
    let mut out = None;
    let ok = egui::Button::new(crate::rtl::widget(ui, egui::RichText::new(crate::i18n::tr(&app.ui.language, "OK")).color(egui::Color32::WHITE)))
        .fill(t.accent_strong)
        .corner_radius(13.0)
        .min_size(size);
    if ui.add(ok).clicked() {
        out = Some(true);
    }
    ui.add_space(4.0);
    if ui.add(egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Cancel"))).corner_radius(13.0).min_size(size)).clicked() {
        out = Some(false);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A four-page document with Numbering & Section Options open (on page 1).
    fn open_app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 4})).unwrap();
        crate::menus::activate(&mut app, "layout.section", &Value::Null);
        app
    }

    fn set(app: &mut DesignApp, fields: Value) {
        let d = app.ui.dialog.as_mut().unwrap();
        for (k, v) in fields.as_object().unwrap() {
            d.fields.insert(k.clone(), v.clone());
        }
    }

    #[test]
    fn the_menu_opens_the_page_section_and_ok_applies_both_parts() {
        let mut app = open_app();
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.id, "numberingSection");
        assert_eq!(
            (d.n("page"), d.b("startSection"), d.s("chapterSource"), d.fields["book"].clone()),
            (Some(1.0), true, "automatic".into(), Value::Null)
        );
        // A section from page 3 numbered A-v, A-vi; the document is chapter IV.
        set(
            &mut app,
            json!({"page": 3, "startSection": true, "automatic": false, "startNumber": "5", "style": "lowerRoman", "prefix": "A-",
                "includePrefix": true, "chapterSource": "userDefined", "chapterStart": "4", "chapterStyle": "upperRoman"}),
        );
        crate::dialogs::confirm(&mut app).unwrap();
        assert!(app.ui.dialog.is_none());
        let doc = &app.session.active().unwrap().doc;
        assert_eq!((0..4).map(|i| doc.page_name(i)).collect::<Vec<_>>(), ["1", "2", "A-v", "A-vi"]);
        assert_eq!(doc.chapter_label(), "IV");
        // Reopened on that page, it shows the section.
        crate::menus::activate(&mut app, "layout.section", &Value::Null);
        set(&mut app, json!({"page": 3, "startSection": false}));
        crate::dialogs::confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.page_name(2), "3", "unchecking Start Section removes the section");
    }

    #[test]
    fn refused_values_keep_the_dialog_open_and_change_nothing() {
        for (key, value) in [("prefix", json!("A+B")), ("prefix", json!("Appendix1")), ("startNumber", json!("0")), ("chapterStart", json!("x"))] {
            let mut app = open_app();
            set(
                &mut app,
                json!({"page": 3, "startSection": true, "automatic": false, "startNumber": "5", "chapterSource": "userDefined", "chapterStart": "2"}),
            );
            set(&mut app, json!({key: value}));
            assert!(crate::dialogs::confirm(&mut app).is_err(), "{key}: {value}");
            assert!(app.ui.dialog.is_some(), "the dialog stays open to be corrected ({key})");
            let doc = &app.session.active().unwrap().doc;
            assert!(doc.sections.iter().all(|s| s.start != 2), "no section was made ({key})");
            assert_eq!(doc.settings.chapter_source, ChapterSource::Automatic, "the chapter numbering is untouched ({key})");
        }
    }
}
