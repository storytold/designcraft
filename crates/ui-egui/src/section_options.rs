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
    // A new section starts from the one the page is in. As in InDesign, the window opened on a
    // page that doesn't start a section is New Section: Start Section checked, Start Page
    // Numbering at offering 1, Automatic Page Numbering chosen.
    let base = own.or_else(|| doc.section_of(page));
    let new = page != 0 && own.is_none();
    let fields = json!({
        "page": page + 1,
        "newSection": new,
        "startSection": true,
        "automatic": own.is_none_or(|s| s.start_number.is_none()),
        "startNumber": if new { "1".to_string() } else { doc.page_number(page).to_string() },
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

/// Apply the dialog: the section and the chapter numbering together (`layout.numberingOptions`,
/// one undo step). A refused value leaves both as they were.
pub(crate) fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let page = number(d, "page").ok_or("no page")?;
    let source: ChapterSource = d.fields.get("chapterSource").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let mut chapter = json!({"style": d.fields.get("chapterStyle").cloned().unwrap_or(json!("arabic")), "source": source});
    if source == ChapterSource::UserDefined {
        chapter["start"] = json!(number(d, "chapterStart").ok_or("Start Chapter Numbering at: a whole number from 1")?);
    }
    let params = if page != 1 && !d.b("startSection") {
        json!({"page": page, "remove": true, "chapter": chapter})
    } else {
        let start =
            if d.b("automatic") { Value::Null } else { json!(number(d, "startNumber").ok_or("Start Page Numbering at: a whole number from 1")?) };
        json!({"page": page, "startNumber": start, "style": d.fields.get("style").cloned().unwrap_or(json!("arabic")),
            "prefix": d.s("prefix"), "marker": d.s("marker"), "includePrefix": d.b("includePrefix"), "chapter": chapter})
    };
    app.run("layout.numberingOptions", params)
}

/// A row that runs right to left in a right-to-left interface.
fn row<R>(ui: &mut Ui, rtl: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let layout = if rtl { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 24.0), layout, add).inner
}

/// A caption ending at the field it names (`Section Prefix:` [   ]).
fn caption(ui: &mut Ui, rtl: bool, text: &str) -> egui::Response {
    let layout = if rtl { Layout::left_to_right(Align::Center) } else { Layout::right_to_left(Align::Center) };
    ui.allocate_ui_with_layout(vec2(LABEL_W, 22.0), layout, |ui| {
        ui.set_width(LABEL_W);
        crate::rtl::label(ui, text)
    })
    .inner
}

fn text(ui: &mut Ui, d: &mut Dialog, key: &str, width: f32, framework: bool, label: &str, labelled_by: Option<egui::Id>) -> bool {
    let mut s = d.s(key);
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        let out = craft_ui_widgets::text::TextField::new(label.to_owned(), Bind::given(s.clone()))
            .id_salt(("section_text", key))
            .limit(None::<usize>)
            .show(ui);
        for event in out.events {
            event.apply(&mut s);
        }
        let changed = s != d.s(key);
        if changed {
            d.fields.insert(key.into(), Value::String(s));
        }
        return changed;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, label);
    let response = ui.add(egui::TextEdit::singleline(&mut s).desired_width(width));
    let response = if let Some(label) = labelled_by { response.labelled_by(label) } else { response };
    let changed = response.changed();
    if changed {
        d.fields.insert(key.into(), Value::String(s));
    }
    changed
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, PartialEq)]
struct NumberDraft<M> {
    text: String,
    mode: M,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct NumberDraftCodec<M>(M);

#[cfg(not(target_arch = "wasm32"))]
impl<M: craft_behavior::Value> craft_behavior::Codec for NumberDraftCodec<M> {
    type Value = NumberDraft<M>;
    const DRAFTS_ARE_VALUES: bool = true;

    fn format(&self, value: &Self::Value) -> String {
        craft_behavior::Codec::format(&craft_behavior::PlainText, &value.text)
    }

    fn parse(&self, text: &str, start: &Self::Value) -> Result<Self::Value, craft_behavior::NotAValue> {
        // Focus and an unchanged blur are not a numbering choice. The SDK may parse
        // the initial text before any edit; keep that transaction's original mode.
        let mode = if text == start.text { start.mode.clone() } else { self.0.clone() };
        Ok(NumberDraft { text: text.to_owned(), mode })
    }

    fn applies_as_typed(&self, text: &str) -> bool {
        craft_behavior::Codec::applies_as_typed(&craft_behavior::PlainText, text)
    }

    fn caret_on_focus(&self, text: &str) -> craft_behavior::Caret {
        craft_behavior::Codec::caret_on_focus(&craft_behavior::PlainText, text)
    }

    fn sample(&self, value: &Self::Value) -> std::borrow::Cow<'_, str> {
        craft_behavior::Codec::sample(&craft_behavior::PlainText, &value.text)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn number_draft<M: craft_behavior::Value + serde::Serialize>(
    ui: &mut Ui,
    d: &mut Dialog,
    key: &str,
    label: &str,
    mode_key: &str,
    mode: M,
    explicit: M,
) {
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
    // Number and choice belong to the same gesture: Cancel must restore both,
    // even after a previous Update already selected explicit numbering.
    let mut draft = NumberDraft { text: d.s(key), mode };
    let before = draft.clone();
    let out = craft_ui_widgets::text::TextField::with_codec(label.to_owned(), Bind::given(draft.clone()), NumberDraftCodec(explicit))
        .id_salt(("section_text", key))
        .limit(None::<usize>)
        .show(ui);
    for event in out.events {
        event.apply(&mut draft);
    }
    if draft != before {
        d.fields.insert(key.into(), json!(draft.text));
        d.fields.insert(mode_key.into(), json!(draft.mode));
    }
}

fn number_text(ui: &mut Ui, d: &mut Dialog, key: &str, framework: bool, label: &str, labelled_by: Option<egui::Id>) {
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        match key {
            "startNumber" => {
                let mode = d.b("automatic");
                number_draft(ui, d, key, label, "automatic", mode, false);
            }
            "chapterStart" => {
                let mode = d.fields.get("chapterSource").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                number_draft(ui, d, key, label, "chapterSource", mode, ChapterSource::UserDefined);
            }
            _ => {}
        }
        return;
    }
    // The draft is editable even while automatic numbering is selected.
    if text(ui, d, key, FIELD_W, framework, label, labelled_by) {
        match key {
            "startNumber" => {
                d.fields.insert("automatic".into(), json!(false));
            }
            "chapterStart" => {
                d.fields.insert("chapterSource".into(), json!(ChapterSource::UserDefined));
            }
            _ => {}
        }
    }
}

fn style_menu(ui: &mut Ui, d: &mut Dialog, key: &str, framework: bool, label: &str, labelled_by: Option<egui::Id>) {
    let current: NumberStyle = d.fields.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
        let entries: Vec<_> = STYLES.into_iter().map(|(style, name)| Entry::new(style, name)).collect();
        let out = craft_ui_widgets::pickers::Select::new(label.to_owned(), Bind::given(current), &entries)
            .keeps_name(true)
            .id_salt(("section_style", key))
            .show(ui);
        let mut next = current;
        for event in out.events {
            event.apply(&mut next);
        }
        if next != current {
            d.fields.insert(key.into(), json!(next));
        }
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (framework, label);
    let shown = STYLES.iter().find(|(s, _)| *s == current).map_or(STYLES[0].1, |(_, l)| l);
    let response = craft_ui_legacy::ComboBox::from_id_salt(("section_style", key))
        .selected_text(shown)
        .width(150.0)
        .show_ui(ui, |ui| {
            for (style, label) in STYLES {
                if ui.selectable_label(style == current, label).clicked() {
                    d.fields.insert(key.into(), json!(style));
                }
            }
        })
        .response;
    if let Some(label) = labelled_by {
        response.labelled_by(label);
    }
}

fn checkbox(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str, framework: bool) {
    let mut on = d.b(key);
    #[cfg(not(target_arch = "wasm32"))]
    if framework {
        use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
        let out = craft_ui_widgets::choice::Checkbox::new(Bind::given(on)).label(label.to_owned()).id_salt(("section_check", key)).show(ui);
        for event in out.events {
            event.apply(&mut on);
        }
        if on != d.b(key) {
            d.fields.insert(key.into(), json!(on));
        }
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = framework;
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

/// The retained browser/uninstalled form; installed native windows take `show_framework`.
pub(crate) fn body(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    form(app, ui, d, false);
}

/// The app-owned numbering adapter: the SDK owns modal chrome, focus and popup
/// precedence; the existing controller alone validates and changes the document.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn show_framework(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(mut draft) = app.ui.dialog.clone().filter(|d| d.id == "numberingSection") else { return };
    // This empty seed UI only supplies a parent. Dialog mounts its own real modal
    // layer; it is not a second egui modal or a guessed framework popup ID.
    let shown = egui::Area::new(egui::Id::unique("numbering_section_host"))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| framework_dialog(app, ui, &mut draft));
    settle_framework(app, draft, shown.inner);
}

#[cfg(not(target_arch = "wasm32"))]
fn framework_dialog(app: &DesignApp, ui: &mut Ui, draft: &mut Dialog) -> craft_ui_panels::dialog::DialogOutput {
    use craft_ui_panels::dialog::Dialog as FrameworkDialog;
    use craft_ui_panels::sections::{DialogRole, FooterButton};
    use craft_ui_widgets::contract::{Bind, Props as _, ShowContainer as _};
    use craft_ui_widgets::runtime::overlay::Backdrop;

    let mut opened = true;
    let lang = app.ui.language.as_str();
    let tr = |s: &'static str| crate::i18n::tr(lang, s).to_owned();
    FrameworkDialog::new(tr("Numbering & Section Options"))
        .id_salt("numbering_section")
        .opened(Bind::bound(&mut opened))
        .enabled(ui.is_enabled())
        .backdrop(Backdrop::None)
        .closable(false)
        .button_place(craft_ui_panels::dialog::ButtonPlace::Side)
        .outer_gesture(false)
        .close_on_enter(true)
        .buttons([
            // Validation runs once outside the repeatable component body.
            // Refusal must not close/reopen the SDK modal and reset focus.
            FooterButton::new(tr("OK"), DialogRole::Accept).stays_open(true),
            FooterButton::new(tr("Cancel"), DialogRole::Cancel),
        ])
        .show(ui, |ui, _| framework_form(app, ui, draft))
        .output
}

#[cfg(not(target_arch = "wasm32"))]
fn settle_framework(app: &mut DesignApp, draft: Dialog, out: craft_ui_panels::dialog::DialogOutput) {
    app.ui.dialog = Some(draft);
    if out.picked == Some(0) {
        let _ = crate::dialogs::confirm(app);
    } else if out.closed.is_some() {
        crate::dialogs::cancel(app);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn framework_form(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    use craft_ui_panels::sections::GroupBox;
    use craft_ui_widgets::choice::{Radio, RadioGroup};
    use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _};
    use craft_ui_widgets::display::Text;
    use craft_ui_widgets::form::{Field, FieldGroup};
    use craft_ui_widgets::layout_components::{Row, Stack};

    let lang = app.ui.language.as_str();
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    let first_page = number(d, "page") == Some(1);
    Stack::new().id_salt("section_form").fill_width().show(ui, |ui, _| {
        // The first page cannot stop being a section, exactly as in the legacy form.
        ui.add_enabled_ui(!first_page, |ui| checkbox(ui, d, "startSection", tr("Start Section"), true));
        let section = first_page || d.b("startSection");
        GroupBox::new(tr("Page Numbering"), None).id_salt("section_page").enabled(section).show(ui, |ui, _| {
            FieldGroup::new(tr("Page Numbering")).show(ui, |ui, _| {
                let before = d.b("automatic");
                let shown = RadioGroup::new(tr("Page Numbering"), Bind::given(before)).id_salt("section_automatic").show(ui, |ui, _| {
                    Stack::new().show(ui, |ui, _| {
                        Radio::new(true).label(tr("Automatic Page Numbering")).show(ui);
                        Row::new().fill_width().show(ui, |ui, _| {
                            Radio::new(false).label(tr("Start Page Numbering at:")).show(ui);
                            number_text(ui, d, "startNumber", true, tr("Start Page Numbering at:"), None);
                        });
                    });
                });
                let mut next = before;
                for event in shown.output.events {
                    event.apply(&mut next);
                }
                if next != before {
                    d.fields.insert("automatic".into(), json!(next));
                }
                Field::new(tr("Section Prefix:")).id_salt("section_prefix_field").show(ui, |ui, _| {
                    text(ui, d, "prefix", FIELD_W, true, tr("Section Prefix:"), None);
                });
                Field::new(tr("Style:")).id_salt("section_style_field").show(ui, |ui, _| {
                    style_menu(ui, d, "style", true, tr("Page Numbering"), None);
                });
                Field::new(tr("Section Marker:")).id_salt("section_marker_field").show(ui, |ui, _| {
                    text(ui, d, "marker", FIELD_W, true, tr("Section Marker:"), None);
                });
                checkbox(ui, d, "includePrefix", tr("Include Prefix when Numbering Pages"), true);
            });
        });
        GroupBox::new(tr("Document Chapter Numbering"), None).id_salt("section_chapter").show(ui, |ui, _| {
            FieldGroup::new(tr("Document Chapter Numbering")).show(ui, |ui, _| {
                Field::new(tr("Style:")).id_salt("chapter_style_field").show(ui, |ui, _| {
                    style_menu(ui, d, "chapterStyle", true, tr("Document Chapter Numbering"), None);
                });
                let before: ChapterSource = d.fields.get("chapterSource").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                let book = d.fields.get("book").and_then(Value::as_str).map(str::to_owned);
                let shown =
                    RadioGroup::new(tr("Document Chapter Numbering"), Bind::given(before)).id_salt("section_chapter_source").show(ui, |ui, _| {
                        Stack::new().show(ui, |ui, _| {
                            Radio::new(ChapterSource::Automatic).label(tr("Automatic Chapter Numbering")).show(ui);
                            Row::new().fill_width().show(ui, |ui, _| {
                                Radio::new(ChapterSource::UserDefined).label(tr("Start Chapter Numbering at:")).show(ui);
                                number_text(ui, d, "chapterStart", true, tr("Start Chapter Numbering at:"), None);
                            });
                            Radio::new(ChapterSource::SameAsPrevious)
                                .label(tr("Same as Previous Document in the Book"))
                                .enabled(book.is_some())
                                .show(ui);
                        });
                    });
                let mut next = before;
                for event in shown.output.events {
                    event.apply(&mut next);
                }
                if next != before {
                    d.fields.insert("chapterSource".into(), json!(next));
                }
                let name = book.as_deref().map_or_else(|| tr("N/A").to_owned(), crate::rtl::isolate);
                Field::new(tr("Book Name:")).id_salt("section_book").value(name.clone()).show(ui, |ui, _| {
                    Text::new(name.clone()).show(ui);
                });
            });
        });
    });
}

fn form(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, framework: bool) {
    let lang = app.ui.language.as_str();
    let tr = |s: &'static str| crate::i18n::tr(lang, s);
    let rtl = crate::i18n::is_rtl(lang);
    ui.spacing_mut().item_spacing.y = 6.0;
    let first_page = number(d, "page") == Some(1);
    // Page 1 always starts a section.
    row(ui, rtl, |ui| {
        ui.add_enabled_ui(!first_page, |ui| checkbox(ui, d, "startSection", tr("Start Section"), framework));
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
                let label = ui.radio(!automatic, crate::rtl::widget(ui, tr("Start Page Numbering at:")));
                if label.clicked() {
                    d.fields.insert("automatic".into(), json!(false));
                }
                // As in InDesign, the number stays editable; typing one chooses Start Page Numbering at.
                number_text(ui, d, "startNumber", framework, tr("Start Page Numbering at:"), Some(label.id));
            });
            ui.add_space(4.0);
            group(ui, rtl, tr("Page Numbering"), |ui| {
                row(ui, rtl, |ui| {
                    let label = caption(ui, rtl, tr("Section Prefix:"));
                    let _ = text(ui, d, "prefix", FIELD_W, framework, tr("Section Prefix:"), Some(label.id));
                });
                row(ui, rtl, |ui| {
                    let label = caption(ui, rtl, tr("Style:"));
                    style_menu(ui, d, "style", framework, tr("Page Numbering"), Some(label.id));
                });
                row(ui, rtl, |ui| {
                    let label = caption(ui, rtl, tr("Section Marker:"));
                    let _ = text(ui, d, "marker", 130.0, framework, tr("Section Marker:"), Some(label.id));
                });
                row(ui, rtl, |ui| checkbox(ui, d, "includePrefix", tr("Include Prefix when Numbering Pages"), framework));
            });
        });
    });
    ui.add_space(10.0);
    let source: ChapterSource = d.fields.get("chapterSource").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let book = d.fields.get("book").and_then(Value::as_str).map(str::to_string);
    group(ui, rtl, tr("Document Chapter Numbering"), |ui| {
        row(ui, rtl, |ui| {
            let label = crate::rtl::label(ui, tr("Style:"));
            style_menu(ui, d, "chapterStyle", framework, tr("Document Chapter Numbering"), Some(label.id));
        });
        for (value, label, enabled) in [
            (ChapterSource::Automatic, "Automatic Chapter Numbering", true),
            (ChapterSource::UserDefined, "Start Chapter Numbering at:", true),
            (ChapterSource::SameAsPrevious, "Same as Previous Document in the Book", book.is_some()),
        ] {
            row(ui, rtl, |ui| {
                let label_response = ui
                    .add_enabled_ui(enabled, |ui| {
                        let response = ui.radio(source == value, crate::rtl::widget(ui, tr(label)));
                        if response.clicked() {
                            d.fields.insert("chapterSource".into(), json!(value));
                        }
                        response
                    })
                    .inner;
                if value == ChapterSource::UserDefined {
                    // Editable whichever is chosen, as in InDesign; typing a number chooses it.
                    number_text(ui, d, "chapterStart", framework, tr("Start Chapter Numbering at:"), Some(label_response.id));
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
        // One OK is one undo step.
        app.run("edit.undo", json!({})).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(
            ((0..4).map(|i| doc.page_name(i)).collect::<Vec<_>>(), doc.chapter_label()),
            (vec!["1".to_string(), "2".into(), "3".into(), "4".into()], "1".into())
        );
        app.run("edit.redo", json!({})).unwrap();
        // Reopened on that page, it shows the section.
        crate::menus::activate(&mut app, "layout.section", &Value::Null);
        set(&mut app, json!({"page": 3, "startSection": false}));
        crate::dialogs::confirm(&mut app).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.page_name(2), "3", "unchecking Start Section removes the section");
    }

    #[test]
    fn refused_values_keep_the_dialog_open_and_change_nothing() {
        for (key, value) in [
            ("prefix", json!("A+B")),
            ("prefix", json!("Appendix1")),
            ("startNumber", json!("0")),
            ("startNumber", json!("4294967295")),
            ("chapterStart", json!("x")),
            ("chapterStart", json!("100000")),
        ] {
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn framework_number_drafts_choose_explicit_modes_before_blur_and_apply_atomically() {
        use craft_ui_widgets::runtime::inspect::{Recorded, record};
        use egui::{Event, Key, Modifiers, PointerButton};
        use egui_kittest::Harness;
        use std::sync::Arc;

        type Window = (DesignApp, Option<Arc<Recorded>>, bool, bool, bool);
        type HistoryState = Vec<(String, Arc<designcraft_doc::Document>, designcraft_doc::Selection)>;
        #[derive(Clone)]
        struct Effects {
            revision: u64,
            undo: usize,
            redo: usize,
            document: Arc<designcraft_doc::Document>,
            saved: Arc<designcraft_doc::Document>,
            journal: Vec<(String, Value)>,
            undo_entries: HistoryState,
            redo_entries: HistoryState,
            history_limit: usize,
            selection: designcraft_doc::Selection,
            uid: u64,
            epoch: u64,
            saved_revision: u64,
            dirty: bool,
        }
        fn app_effects(app: &DesignApp) -> Effects {
            let st = app.session.active().unwrap();
            let history = |entries: &[designcraft_engine::HistoryEntry]| -> HistoryState {
                entries.iter().map(|entry| (entry.label.clone(), entry.doc.clone(), entry.selection.clone())).collect()
            };
            Effects {
                revision: st.revision,
                undo: st.history.undo.len(),
                redo: st.history.redo.len(),
                document: st.doc.clone(),
                saved: st.saved_doc.clone(),
                journal: app.session.journal.clone(),
                undo_entries: history(&st.history.undo),
                redo_entries: history(&st.history.redo),
                history_limit: st.history.limit,
                selection: st.selection.clone(),
                uid: st.uid,
                epoch: st.identity_epoch,
                saved_revision: st.saved_revision,
                dirty: st.is_dirty(),
            }
        }
        fn effects(h: &Harness<'_, Window>) -> Effects {
            app_effects(&h.state().0)
        }
        fn assert_app_quiet(app: &DesignApp, before: &Effects) {
            let now = app_effects(app);
            assert_eq!((now.revision, now.undo, now.redo), (before.revision, before.undo, before.redo));
            assert_eq!(now.document.as_ref(), before.document.as_ref());
            assert_eq!(now.saved.as_ref(), before.saved.as_ref());
            assert_eq!(now.journal, before.journal);
            assert_eq!(now.undo_entries, before.undo_entries);
            assert_eq!(now.redo_entries, before.redo_entries);
            assert_eq!(now.history_limit, before.history_limit);
            assert_eq!(now.selection, before.selection);
            assert_eq!((now.uid, now.epoch, now.saved_revision, now.dirty), (before.uid, before.epoch, before.saved_revision, before.dirty));
            assert!(Arc::ptr_eq(&now.document, &before.document), "quiet frames do not even replace the live allocation");
            assert!(Arc::ptr_eq(&now.saved, &before.saved), "quiet frames preserve saved allocation identity");
        }
        fn assert_quiet(h: &Harness<'_, Window>, before: &Effects) {
            assert_app_quiet(&h.state().0, before);
        }
        fn native_from_same_state(app: &DesignApp, draft: &Dialog) -> DesignApp {
            // Public accepted seams only: preserve the COMPLETE DocState, its local next_id,
            // saved/live allocations, identity, selection and history. Do not rebuild a document
            // from page numbers or normalize IDs. This empty fixture has no external book/story.
            assert_eq!(app.session.documents().len(), 1);
            assert!(app.session.book.is_none());
            assert!(app.session.active().unwrap().doc.settings.primary_story.is_none());
            let mut session = designcraft_engine::Session::new();
            session.prefs = app.session.prefs.clone();
            session.cache = app.session.cache.clone();
            session.add_document(app.session.active().unwrap().clone());
            session.journal = app.session.journal.clone();
            let mut native = DesignApp::new_for_panel_tests(session, crate::Services::default());
            native.ui.language = app.ui.language.clone();
            native.ui.dialog = Some(draft.clone());
            assert_app_quiet(&native, &app_effects(app));
            native
        }
        fn expected_confirm(app: &DesignApp, draft: &Dialog) -> DesignApp {
            let mut native = native_from_same_state(app, draft);
            // The SAME existing public app/controller used by SDK settlement, invoked without
            // synthesizing its engine payload or implementing a repagination solver in this test.
            crate::dialogs::confirm(&mut native).unwrap();
            assert!(native.ui.dialog.is_none());
            native
        }
        fn assert_native_outcome(h: &Harness<'_, Window>, native: &DesignApp, before: &Effects) -> Effects {
            let expected = app_effects(native);
            let actual = effects(h);
            // IDs, next_id, geometry, settings, styles and all other Document fields stay in this
            // equality. This helper is for genuine edits/Undo/Redo, not no-op confirmation.
            assert_eq!(
                actual.document.as_ref(),
                expected.document.as_ref(),
                "the entire SDK outcome must equal the explicit native/public-confirm outcome"
            );
            assert_eq!(actual.saved.as_ref(), expected.saved.as_ref());
            assert_eq!((actual.undo, actual.redo), (expected.undo, expected.redo));
            assert_eq!(actual.undo_entries, expected.undo_entries);
            assert_eq!(actual.redo_entries, expected.redo_entries);
            assert_eq!(actual.history_limit, expected.history_limit);
            assert_eq!(actual.selection, expected.selection);
            assert_eq!(
                (actual.uid, actual.epoch, actual.saved_revision, actual.dirty),
                (expected.uid, expected.epoch, expected.saved_revision, expected.dirty)
            );
            assert_eq!(actual.journal, expected.journal);
            assert_eq!(actual.saved.as_ref(), before.saved.as_ref());
            assert!(Arc::ptr_eq(&expected.saved, &before.saved), "native numbering/Undo/Redo never saves the document");
            assert!(Arc::ptr_eq(&actual.saved, &before.saved));
            assert_eq!(
                (actual.uid, actual.epoch, actual.saved_revision, actual.history_limit),
                (before.uid, before.epoch, before.saved_revision, before.history_limit)
            );
            // Revisions are globally unique tokens, NOT a per-document command count. The native
            // expectation itself consumes a token. Both real paths must allocate fresh tokens in
            // execution order; one command/Undo/journal entry is asserted independently below.
            assert!(expected.revision > before.revision, "the native command allocates a fresh revision");
            assert!(actual.revision > expected.revision, "the subsequent SDK/app command allocates its own fresh revision");
            actual
        }
        fn assert_noop_confirm(h: &Harness<'_, Window>, native: &DesignApp, before: &Effects) -> Effects {
            let expected = app_effects(native);
            assert_eq!(expected.journal.len(), before.journal.len() + 1, "a successful no-op still journals the command attempt");
            assert_eq!(expected.journal.last().unwrap().0, "layout.numberingOptions");
            let mut unchanged = before.clone();
            unchanged.journal = expected.journal.clone();
            // Owner revision, complete Doc/IDs, live+saved Arc identity, dirty/saved markers,
            // full selection and every Undo/Redo entry remain EXACT, not just their lengths.
            assert_app_quiet(native, &unchanged);
            assert_quiet(h, &unchanged);
            effects(h)
        }
        fn assert_one_confirm(after: &Effects, before: &Effects) {
            assert_eq!(after.undo, before.undo + 1, "one accepted command records one complete undo entry");
            assert_eq!(after.redo, 0, "a new edit clears redo");
            assert_eq!(after.journal.len(), before.journal.len() + 1);
            assert_eq!(after.journal.last().unwrap().0, "layout.numberingOptions");
            assert!(!Arc::ptr_eq(&after.document, &before.document), "native confirmation replaces the live allocation");
            assert!(after.dirty, "the confirmed allocation differs from the untouched saved document");
        }
        fn draft_with(draft: &Dialog, values: Value) -> Dialog {
            let mut expected = draft.clone();
            expected.fields.extend(values.as_object().unwrap().clone());
            expected
        }
        fn step(h: &mut Harness<'_, Window>) {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().0.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
        fn event(h: &mut Harness<'_, Window>, event: Event) {
            h.input_mut().events.push(event);
            step(h);
        }
        fn click_control(h: &mut Harness<'_, Window>, role: egui::accesskit::Role, name: &str) {
            let role = format!("{role:?}");
            let controls: Vec<_> = h
                .state()
                .1
                .as_ref()
                .unwrap()
                .elements
                .iter()
                .filter(|e| e.facts.as_ref().is_some_and(|f| f.role == role && f.name.as_deref() == Some(name)))
                .collect();
            assert_eq!(controls.len(), 1, "one accessible {role} named {name}, not a generic component-name suffix");
            let pos = controls[0].rect.center();
            event(h, Event::PointerMoved(pos));
            for pressed in [true, false] {
                event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            }
        }
        fn style_popup(h: &Harness<'_, Window>) -> bool {
            h.state()
                .1
                .as_ref()
                .unwrap()
                .elements
                .iter()
                .any(|e| e.facts.as_ref().is_some_and(|f| f.role == "ListBox" && f.name.as_deref() == Some("Page Numbering")))
        }
        fn tab_to_button(h: &mut Harness<'_, Window>, name: &str) {
            let controls: Vec<_> = h
                .state()
                .1
                .as_ref()
                .unwrap()
                .elements
                .iter()
                .filter(|e| e.facts.as_ref().is_some_and(|f| f.role == "Button" && f.name.as_deref() == Some(name)))
                .collect();
            assert_eq!(controls.len(), 1, "one actual installed footer button named {name}");
            let id = controls[0].id;
            let mut visited = std::collections::HashSet::new();
            while h.ctx.memory(|memory| memory.focused()) != Some(id) {
                assert!(visited.insert(h.ctx.memory(|memory| memory.focused())), "natural Tab must reach the real {name} button");
                for pressed in [true, false] {
                    event(h, Event::Key { key: Key::Tab, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
                }
            }
        }
        fn type_draft(h: &mut Harness<'_, Window>, name: &str, text: &str) {
            click_control(h, egui::accesskit::Role::TextInput, name);
            for pressed in [true, false] {
                event(h, Event::Key { key: Key::A, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND });
            }
            event(h, Event::Text(text.into()));
            step(h);
        }

        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("file.new", json!({"pages": 4})).unwrap();
        crate::menus::activate(&mut app, "layout.section", &Value::Null);
        // A new document starts its first section at an explicit number. Choose
        // Automatic through the existing dialog/controller before testing that
        // ordinary focus and canceled drafts preserve that chosen mode.
        set(&mut app, json!({"automatic": true}));
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.active().unwrap().doc.sections[0].start_number, None, "the fixture actually starts in Automatic mode");
        open(&mut app);
        let before = app.session.active().unwrap().doc.clone();
        let ctx = egui::Context::default();
        app.framework_opacity = true;
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(720.0, 900.0)).build_ui_state(
            move |ui, state: &mut Window| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ready = true;
                    return;
                }
                state.1 = record(ui, egui::Id::unique("numbering-draft-test"), |ui| {
                    let ctx = ui.ctx().clone();
                    if state.4 {
                        // The same real Dialog and settlement used by show_framework,
                        // under public disabled/sizing passes, with actual pointer/key input.
                        let builder = if state.3 { egui::UiBuilder::new().sizing_pass() } else { egui::UiBuilder::new() };
                        let mut d = state.0.ui.dialog.clone().unwrap();
                        let before_pass = app_effects(&state.0);
                        let before_fields = d.fields.clone();
                        ui.scope_builder(builder, |ui| {
                            ui.add_enabled_ui(state.2, |ui| {
                                let out = framework_dialog(&state.0, ui, &mut d);
                                settle_framework(&mut state.0, d.clone(), out);
                                if !state.2 || state.3 {
                                    assert_eq!(
                                        state.0.ui.dialog.as_ref().unwrap().fields,
                                        before_fields,
                                        "disabled/sizing is draft-inert DURING the actual callback"
                                    );
                                    assert_app_quiet(&state.0, &before_pass);
                                }
                            });
                        });
                    } else {
                        crate::dialogs::show(&mut state.0, &ctx);
                    }
                })
                .1;
            },
            (app, None, true, false, false),
        );
        h.input_mut().max_texture_side = Some(8192);
        for _ in 0..4 {
            step(&mut h);
        }
        let dialogs: Vec<_> = h
            .state()
            .1
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.facts.as_ref().is_some_and(|f| f.role == "Dialog" && f.name.as_deref() == Some("Numbering & Section Options")))
            .collect();
        assert_eq!(dialogs.len(), 1, "the real SDK986 Dialog, not the legacy egui shell");
        assert!(craft_ui_widgets::runtime::overlay::any_open(&h.ctx, craft_ui_widgets::runtime::overlay::OpenFilter::Modal));
        let initial = effects(&h);
        let initial_fields = h.state().0.ui.dialog.as_ref().unwrap().fields.clone();
        for _ in 0..5 {
            step(&mut h);
        }
        assert_quiet(&h, &initial);
        assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields, initial_fields, "ordinary idle preserves every draft");
        // Observe the same actual Dialog at its own geometry before disabling or measuring it.
        h.state_mut().4 = true;
        step(&mut h);
        let page_target = h
            .state()
            .1
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .find(|e| e.facts.as_ref().is_some_and(|f| f.role == "TextInput" && f.name.as_deref() == Some("Start Page Numbering at:")))
            .unwrap()
            .rect
            .center();
        for (enabled, sizing) in [(false, false), (true, true)] {
            h.state_mut().2 = enabled;
            h.state_mut().3 = sizing;
            step(&mut h);
            // Real pointer/key input goes through the ordinary hook; no manual focus or getter-only proof.
            event(&mut h, Event::PointerMoved(page_target));
            for pressed in [true, false] {
                event(&mut h, Event::PointerButton { pos: page_target, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            }
            event(&mut h, Event::Text("9".into()));
            for _ in 0..3 {
                step(&mut h);
            }
            assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields, initial_fields, "disabled/sizing leaves drafts unchanged");
            assert_quiet(&h, &initial);
        }
        h.state_mut().2 = true;
        h.state_mut().3 = false;
        h.state_mut().4 = false;
        step(&mut h);
        for source in [ChapterSource::Automatic, ChapterSource::SameAsPrevious] {
            let app = &mut h.state_mut().0;
            app.run("layout.chapterNumbering", json!({"source": source})).unwrap();
            open(app);
            let d = app.ui.dialog.as_ref().unwrap();
            assert!(d.b("automatic"), "Automatic is a precondition before any focus input: {:?}", d.fields);
            assert_eq!(d.fields["chapterSource"], json!(source), "the requested chapter source is established before focus input");
            let opening = d.clone();
            let unchanged = app.session.active().unwrap().doc.clone();
            let history = app.session.active().unwrap().history.undo.len();
            let quiet = effects(&h);
            let app = &h.state().0;
            let page_draft = app.ui.dialog.as_ref().unwrap().s("startNumber");
            let chapter_draft = app.ui.dialog.as_ref().unwrap().s("chapterStart");
            step(&mut h);
            click_control(&mut h, egui::accesskit::Role::TextInput, "Start Page Numbering at:");
            click_control(&mut h, egui::accesskit::Role::TextInput, "Start Chapter Numbering at:");
            click_control(&mut h, egui::accesskit::Role::TextInput, "Start Page Numbering at:");
            let d = h.state().0.ui.dialog.as_ref().unwrap();
            assert!(d.b("automatic"), "focusing and leaving the page field without typing keeps automatic numbering: {:?}", d.fields);
            assert_eq!(d.fields["chapterSource"], json!(source), "focus-only chapter blur keeps its existing numbering source");
            assert_eq!(d.s("startNumber"), page_draft);
            assert_eq!(d.s("chapterStart"), chapter_draft);
            assert_eq!(d.fields, opening.fields, "focus/blur preserves the complete opening draft");
            assert_eq!(h.state().0.session.active().unwrap().doc.as_ref(), unchanged.as_ref(), "focus alone never edits the document");
            assert_eq!(h.state().0.session.active().unwrap().history.undo.len(), history, "focus alone adds no undo step");
            assert_quiet(&h, &quiet);
            type_draft(&mut h, "Start Page Numbering at:", "9");
            assert!(!h.state().0.ui.dialog.as_ref().unwrap().b("automatic"), "Update selects explicit page numbering");
            // craft's dialogs, like InDesign's, cancel on one Escape: the typed draft and the mode it chose go with it.
            for pressed in [true, false] {
                event(&mut h, Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
            }
            assert!(h.state().0.ui.dialog.is_none(), "page Escape cancels the dialog with its draft");
            assert_quiet(&h, &quiet);
            open(&mut h.state_mut().0);
            let d = h.state().0.ui.dialog.as_ref().unwrap();
            assert!(d.b("automatic"), "reopened, the page mode is the one from before Update");
            assert_eq!(d.s("startNumber"), page_draft);
            assert_eq!(d.fields, opening.fields, "reopening after the page Escape shows the complete unchanged settings");
            step(&mut h);
            type_draft(&mut h, "Start Chapter Numbering at:", "8");
            assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields["chapterSource"], json!(ChapterSource::UserDefined));
            for pressed in [true, false] {
                event(&mut h, Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
            }
            assert!(h.state().0.ui.dialog.is_none(), "chapter Escape cancels the dialog with its draft");
            let app = &h.state().0;
            assert_eq!(app.session.active().unwrap().doc.as_ref(), unchanged.as_ref(), "canceled drafts never edit the document");
            assert_eq!(app.session.active().unwrap().history.undo.len(), history, "no undo step before dialog confirmation");
            assert_quiet(&h, &quiet);
            open(&mut h.state_mut().0);
            let d = h.state().0.ui.dialog.as_ref().unwrap();
            assert_eq!(d.fields["chapterSource"], json!(source), "reopened, the source is automatic or SameAsPrevious, not UserDefined");
            assert_eq!(d.s("chapterStart"), chapter_draft);
            assert_eq!(d.fields, opening.fields, "reopening after the chapter Escape shows the complete unchanged settings");
            step(&mut h);
            // The independent existing controller receives the PRE-INPUT opening DTO.
            // The shared guard must make accepting these unchanged settings a true model no-op.
            let native = expected_confirm(&h.state().0, &opening);
            assert_quiet(&h, &quiet);
            click_control(&mut h, egui::accesskit::Role::Button, "OK");
            assert!(h.state().0.ui.dialog.is_none(), "the real Accept closes the successfully confirmed dialog");
            assert_eq!(
                h.state().0.session.active().unwrap().doc.as_ref(),
                unchanged.as_ref(),
                "confirming canceled drafts preserves both numbering modes"
            );
            let confirmed = assert_noop_confirm(&h, &native, &quiet);
            assert_eq!(confirmed.document.sections, unchanged.sections);
            assert_eq!(confirmed.document.settings.chapter_source, source);
            for _ in 0..5 {
                step(&mut h);
            }
            assert_quiet(&h, &confirmed);
        }
        let app = &mut h.state_mut().0;
        app.run("layout.chapterNumbering", json!({"source": ChapterSource::Automatic})).unwrap();
        open(app);
        set(app, json!({"page": 3, "automatic": true, "chapterSource": "automatic"}));
        let history = app.session.active().unwrap().history.undo.len();
        let atomic_opening = app.ui.dialog.as_ref().unwrap().clone();
        let prior = effects(&h);
        step(&mut h);
        click_control(&mut h, egui::accesskit::Role::ComboBox, "Page Numbering");
        use craft_ui_widgets::runtime::overlay::{OpenFilter, any_open};
        assert!(style_popup(&h), "the actual accessible SDK style listbox opened inside the modal");
        let style_before = h.state().0.ui.dialog.as_ref().unwrap().fields["style"].clone();
        for pressed in [true, false] {
            event(&mut h, Event::Key { key: Key::ArrowDown, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        assert_quiet(&h, &prior);
        for pressed in [true, false] {
            event(&mut h, Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        assert!(!style_popup(&h), "Escape closes the style listbox while the SDK modal stays open");
        assert!(h.state().0.ui.dialog.is_some(), "the popup owns Escape before the host can cancel");
        assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields["style"], style_before, "popup Cancel restores the original draft style");
        assert_eq!(
            h.state().0.ui.dialog.as_ref().unwrap().fields,
            atomic_opening.fields,
            "popup navigation/Escape restores the complete pre-popup draft"
        );
        assert_quiet(&h, &prior);
        for _ in 0..3 {
            step(&mut h);
        }
        assert!(!style_popup(&h));
        assert!(any_open(&h.ctx, OpenFilter::Modal));
        assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields, atomic_opening.fields);
        assert_quiet(&h, &prior);
        type_draft(&mut h, "Start Page Numbering at:", "0");
        let d = h.state().0.ui.dialog.as_ref().unwrap();
        assert!(!d.b("automatic"), "typing selects explicit page numbering without waiting for blur");
        assert_eq!(d.s("startNumber"), "0", "the raw draft is not normalized before validation");
        type_draft(&mut h, "Start Chapter Numbering at:", "4");
        let d = h.state().0.ui.dialog.as_ref().unwrap();
        assert_eq!(d.s("chapterSource"), "userDefined", "typing selects explicit chapter numbering");
        assert_eq!(d.s("chapterStart"), "4");
        let invalid_page =
            draft_with(&atomic_opening, json!({"automatic": false, "startNumber": "0", "chapterSource": "userDefined", "chapterStart": "4"}));
        assert_eq!(d.fields, invalid_page.fields, "only the two intended mode/draft edits follow popup recovery");
        let mut refused_native = native_from_same_state(&h.state().0, &invalid_page);
        let page_error = crate::dialogs::confirm(&mut refused_native).unwrap_err();
        assert_app_quiet(&refused_native, &prior);
        assert_eq!(refused_native.ui.dialog.as_ref().unwrap().fields, invalid_page.fields);
        click_control(&mut h, egui::accesskit::Role::Button, "OK");
        assert!(h.state().0.ui.dialog.is_some(), "the real SDK Accept must retain the modal after validation refusal");
        assert!(any_open(&h.ctx, OpenFilter::Modal), "refusal keeps the SDK modal, not a close/reopen shim");
        assert_quiet(&h, &prior);
        assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().s("startNumber"), "0", "a refused draft remains editable");
        assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields, invalid_page.fields);
        assert_eq!(h.state().0.ui.status, page_error);
        assert_eq!(h.state().0.session.active().unwrap().doc.as_ref(), before.as_ref(), "refusal changes neither numbering part");
        assert_eq!(
            h.state().0.session.active().unwrap().doc.as_ref(),
            prior.document.as_ref(),
            "refusal changes neither numbering part at this checkpoint"
        );
        type_draft(&mut h, "Start Page Numbering at:", "7");
        assert_eq!(h.state().0.session.active().unwrap().doc.as_ref(), before.as_ref(), "drafts do not edit the document");
        assert_eq!(h.state().0.session.active().unwrap().doc.as_ref(), prior.document.as_ref(), "drafts do not edit the current complete document");
        assert_eq!(h.state().0.session.active().unwrap().history.undo.len(), history);
        assert_quiet(&h, &prior);
        // The opposite atomic refusal reaches the real engine parser: the page edit is valid,
        // while chapter 100000 exceeds the existing chapter limit. Neither part may leak out.
        type_draft(&mut h, "Start Chapter Numbering at:", "100000");
        let invalid_chapter =
            draft_with(&atomic_opening, json!({"automatic": false, "startNumber": "7", "chapterSource": "userDefined", "chapterStart": "100000"}));
        assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields, invalid_chapter.fields);
        let mut refused_native = native_from_same_state(&h.state().0, &invalid_chapter);
        let chapter_error = crate::dialogs::confirm(&mut refused_native).unwrap_err();
        assert_app_quiet(&refused_native, &prior);
        assert_eq!(refused_native.ui.dialog.as_ref().unwrap().fields, invalid_chapter.fields);
        click_control(&mut h, egui::accesskit::Role::Button, "OK");
        assert!(h.state().0.ui.dialog.is_some());
        assert!(any_open(&h.ctx, OpenFilter::Modal));
        assert_eq!(h.state().0.ui.dialog.as_ref().unwrap().fields, invalid_chapter.fields);
        assert_eq!(h.state().0.ui.status, chapter_error);
        assert_quiet(&h, &prior);
        type_draft(&mut h, "Start Chapter Numbering at:", "4");
        let intended =
            draft_with(&atomic_opening, json!({"automatic": false, "startNumber": "7", "chapterSource": "userDefined", "chapterStart": "4"}));
        assert_eq!(
            h.state().0.ui.dialog.as_ref().unwrap().fields,
            intended.fields,
            "ordinary recovery retains both explicit modes and only the intended drafts"
        );
        assert_quiet(&h, &prior);
        let mut native = expected_confirm(&h.state().0, &intended);
        assert_quiet(&h, &prior);
        for pressed in [true, false] {
            event(&mut h, Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        assert!(h.state().0.ui.dialog.is_none(), "Enter reaches the real SDK dialog Accept");
        let accepted = assert_native_outcome(&h, &native, &prior);
        assert_one_confirm(&accepted, &prior);
        for _ in 0..5 {
            step(&mut h);
        }
        assert_quiet(&h, &accepted);
        let app = &mut h.state_mut().0;
        assert_eq!(app.session.active().unwrap().doc.page_name(2), "7");
        assert_eq!(app.session.active().unwrap().doc.chapter_label(), "4");
        assert_eq!(app.session.active().unwrap().history.undo.len(), history + 1, "one OK is one transaction");
        native.run("edit.undo", json!({})).unwrap();
        app.run("edit.undo", json!({})).unwrap();
        let undone = assert_native_outcome(&h, &native, &accepted);
        assert_eq!(undone.document.as_ref(), before.as_ref(), "one undo restores both numbering choices");
        assert_eq!(
            undone.document.as_ref(),
            prior.document.as_ref(),
            "one undo restores BOTH numbering choices and the full immediate pre-apply document, IDs included"
        );
        assert_eq!((undone.undo, undone.redo), (history, 1));
        for _ in 0..5 {
            step(&mut h);
        }
        assert_quiet(&h, &undone);
        native.run("edit.redo", json!({})).unwrap();
        h.state_mut().0.run("edit.redo", json!({})).unwrap();
        let redone = assert_native_outcome(&h, &native, &undone);
        assert_eq!(
            redone.document.as_ref(),
            accepted.document.as_ref(),
            "one redo restores the complete atomic page+chapter result, not a recomputed approximation"
        );
        assert_eq!((redone.undo, redone.redo), (history + 1, 0));
        assert_eq!(redone.document.page_name(2), "7");
        assert_eq!(redone.document.chapter_label(), "4");
        for _ in 0..5 {
            step(&mut h);
        }
        assert_quiet(&h, &redone);
        // Return to the real pre-apply checkpoint for the original Cancel journeys, retaining
        // the populated redo entry so canceled drafts must preserve it in full as well.
        native.run("edit.undo", json!({})).unwrap();
        h.state_mut().0.run("edit.undo", json!({})).unwrap();
        let restored = assert_native_outcome(&h, &native, &redone);
        assert_eq!(restored.document.as_ref(), prior.document.as_ref());
        assert_eq!((restored.undo, restored.redo), (history, 1));
        open(&mut h.state_mut().0);
        step(&mut h);
        let canceled = effects(&h);
        let cancel_opening = h.state().0.ui.dialog.as_ref().unwrap().clone();
        type_draft(&mut h, "Start Chapter Numbering at:", "9");
        assert_eq!(
            h.state().0.ui.dialog.as_ref().unwrap().fields,
            draft_with(&cancel_opening, json!({"chapterSource": "userDefined", "chapterStart": "9"})).fields
        );
        assert_quiet(&h, &canceled);
        tab_to_button(&mut h, "Cancel");
        for pressed in [true, false] {
            event(&mut h, Event::Key { key: Key::Space, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        assert!(h.state().0.ui.dialog.is_none(), "natural Tab and Space activate the actual SDK Cancel");
        for _ in 0..5 {
            step(&mut h);
        }
        assert_quiet(&h, &canceled);
        open(&mut h.state_mut().0);
        step(&mut h);
        assert_eq!(
            h.state().0.ui.dialog.as_ref().unwrap().fields,
            cancel_opening.fields,
            "reopening after host Cancel discards every previous draft/mode edit"
        );
        assert_quiet(&h, &canceled);
        type_draft(&mut h, "Start Chapter Numbering at:", "9");
        assert_eq!(
            h.state().0.ui.dialog.as_ref().unwrap().fields,
            draft_with(&cancel_opening, json!({"chapterSource": "userDefined", "chapterStart": "9"})).fields
        );
        assert_quiet(&h, &canceled);
        for escape in 0..2 {
            for pressed in [true, false] {
                event(&mut h, Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
            }
            if escape == 0 {
                assert_eq!(
                    h.state().0.ui.dialog.as_ref().unwrap().fields,
                    cancel_opening.fields,
                    "the first Escape restores the complete field draft/mode without closing the host"
                );
                assert!(any_open(&h.ctx, OpenFilter::Modal));
                assert_quiet(&h, &canceled);
            }
        }
        assert!(h.state().0.ui.dialog.is_none(), "Escape cancels the draft, then the actual SDK dialog host");
        assert_eq!(h.state().0.session.active().unwrap().doc.as_ref(), before.as_ref(), "Cancel never edits either numbering part");
        assert_eq!(
            h.state().0.session.active().unwrap().doc.as_ref(),
            canceled.document.as_ref(),
            "Cancel never edits either numbering part or any complete-document field at the actual restored checkpoint"
        );
        for _ in 0..5 {
            step(&mut h);
        }
        assert_quiet(&h, &canceled);
    }

    #[cfg(not(target_arch = "wasm32"))]
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role};
    #[cfg(not(target_arch = "wasm32"))]
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    #[cfg(not(target_arch = "wasm32"))]
    fn section_app() -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("file.new", json!({"pages": 4})).unwrap();
        open(&mut app);
        // Explicit pending-dialog fixture, not a claim that file.new starts Automatic.
        // Later-page journeys move this DTO to a fresh section; no controller command
        // or document change happens until their actual installed footer accepts it.
        set(&mut app, json!({"automatic": true}));
        app
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn installed_harness(mut app: DesignApp, sdk: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        // The native dialog router gates on framework_installed; pin both pilot flags, independent of the environment.
        app.framework_transform = false;
        app.framework_opacity = sdk;
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert_eq!(app.framework_installed, sdk, "the fixture must use its requested installed or fallback branch");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(920.0, 1000.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        section_steps(&mut h, 3);
        if sdk {
            let _ = h.get_by_role_and_label(Role::Dialog, "Numbering & Section Options");
            assert!(craft_ui_widgets::runtime::overlay::any_open(&h.ctx, craft_ui_widgets::runtime::overlay::OpenFilter::Modal));
        }
        h
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_event(h: &mut Harness<'_, DesignApp>, event: Event) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_steps(h: &mut Harness<'_, DesignApp>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_key(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            section_event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        }
        section_event(h, Event::ModifiersChanged(Modifiers::NONE));
        section_steps(h, 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_click_at(h: &mut Harness<'_, DesignApp>, at: egui::Pos2) {
        section_event(h, Event::PointerMoved(at));
        for pressed in [true, false] {
            section_event(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        section_steps(h, 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_click(h: &mut Harness<'_, DesignApp>, role: Role, label: &str) {
        let at = h.get_by_role_and_label(role, label).rect().center();
        section_click_at(h, at);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_type(h: &mut Harness<'_, DesignApp>, label: &str, text: &str) {
        section_click(h, Role::TextInput, label);
        assert!(h.get_by_role_and_label(Role::TextInput, label).is_focused(), "ordinary pointer focus on {label}");
        section_key(h, Key::A, Modifiers::COMMAND);
        // Native text input can carry a multi-character IME/text commitment; this is a real host event,
        // not a direct dialog setter or SDK activation intent.
        section_event(h, Event::Text(text.to_string()));
        section_steps(h, 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_pick_style(h: &mut Harness<'_, DesignApp>, chapter: bool, choice: &str) {
        let label = if chapter { "Document Chapter Numbering" } else { "Page Numbering" };
        section_click(h, Role::ComboBox, label);
        let at = h.get_by_label(choice).rect().center();
        section_click_at(h, at);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn section_dialog<'a>(h: &'a Harness<'_, DesignApp>) -> &'a Dialog {
        h.state().ui.dialog.as_ref().unwrap()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn numbering_commands(h: &Harness<'_, DesignApp>) -> Vec<(String, Value)> {
        h.state().session.journal.iter().filter(|(id, _)| id == "layout.numberingOptions").cloned().collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn numbering_undo(h: &Harness<'_, DesignApp>) -> usize {
        h.state().session.doc().unwrap().history.undo.len()
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_editable_automatic_numbers_choose_manual_and_cancel_without_command() {
        let mut app = section_app();
        set(&mut app, json!({"page": 3})); // In-memory dialog fixture targeting page 3; no UI mutation shortcut below.
        let before = app.session.doc().unwrap().doc.clone();
        let selection = app.session.doc().unwrap().selection.clone();
        let mut h = installed_harness(app, true);
        let undo = numbering_undo(&h);
        for label in ["Start Page Numbering at:", "Start Chapter Numbering at:"] {
            assert!(!h.get_by_role_and_label(Role::TextInput, label).accesskit_node().is_disabled());
        }
        assert!(section_dialog(&h).b("automatic"));
        section_type(&mut h, "Start Page Numbering at:", "7");
        assert!(!section_dialog(&h).b("automatic"));
        assert_eq!(section_dialog(&h).s("startNumber"), "7");
        section_click(&mut h, Role::RadioButton, "Automatic Page Numbering");
        assert!(section_dialog(&h).b("automatic"));
        assert_eq!(section_dialog(&h).s("startNumber"), "7", "automatic does not erase the manual draft");
        section_type(&mut h, "Start Chapter Numbering at:", "4");
        assert_eq!(section_dialog(&h).s("chapterSource"), "userDefined");
        section_click(&mut h, Role::RadioButton, "Automatic Chapter Numbering");
        assert_eq!(section_dialog(&h).s("chapterSource"), "automatic");
        assert_eq!(section_dialog(&h).s("chapterStart"), "4");
        assert!(!h.get_by_role_and_label(Role::TextInput, "Start Chapter Numbering at:").accesskit_node().is_disabled());
        section_click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        assert_eq!(h.state().session.doc().unwrap().selection, selection);
        assert_eq!(numbering_undo(&h), undo);
        assert!(numbering_commands(&h).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_full_page_and_chapter_apply_is_one_exact_command_and_undo() {
        let mut app = section_app();
        set(&mut app, json!({"page": 3}));
        let before = app.session.doc().unwrap().doc.clone();
        let selection = app.session.doc().unwrap().selection.clone();
        let uid = app.session.doc().unwrap().uid;
        let mut h = installed_harness(app, true);
        let undo = numbering_undo(&h);
        section_type(&mut h, "Start Page Numbering at:", "7");
        section_type(&mut h, "Section Prefix:", "A-");
        section_pick_style(&mut h, false, "i, ii, iii, iv…");
        section_type(&mut h, "Section Marker:", "Appendix");
        section_click(&mut h, Role::CheckBox, "Include Prefix when Numbering Pages");
        section_type(&mut h, "Start Chapter Numbering at:", "4");
        section_pick_style(&mut h, true, "I, II, III, IV…");
        assert_eq!(h.state().session.doc().unwrap().doc, before, "all controls edit drafts until OK");
        assert!(numbering_commands(&h).is_empty());
        section_click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(
            numbering_commands(&h),
            vec![(
                "layout.numberingOptions".into(),
                json!({
                    "page": 3, "startNumber": 7, "style": "lowerRoman", "prefix": "A-", "marker": "Appendix", "includePrefix": true,
                    "chapter": {"style": "upperRoman", "source": "userDefined", "start": 4}
                })
            )]
        );
        let state = h.state().session.doc().unwrap();
        assert_eq!(state.uid, uid);
        assert_eq!(state.selection, selection);
        assert_eq!(state.doc.page_name(0), "1");
        assert_eq!(state.doc.page_name(2), "A-vii");
        assert_eq!(state.doc.page_name(3), "A-viii");
        assert_eq!(state.doc.chapter_label(), "IV");
        assert_eq!(state.doc.sections.iter().find(|s| s.start == 2).unwrap().marker, "Appendix");
        assert_eq!(numbering_undo(&h), undo + 1);
        section_steps(&mut h, 3);
        assert_eq!(numbering_commands(&h).len(), 1, "idle frames must not replay OK");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc, before, "one Undo restores section and chapter together");
        assert_eq!(numbering_undo(&h), undo);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_invalid_number_drafts_reject_without_partial_write() {
        for (label, invalid) in [
            ("Start Page Numbering at:", "0"),
            ("Start Page Numbering at:", "1.5"),
            ("Start Page Numbering at:", "4294967295"),
            ("Start Chapter Numbering at:", "x"),
            ("Start Chapter Numbering at:", "100000"),
        ] {
            let mut app = section_app();
            set(&mut app, json!({"page": 3}));
            let before = app.session.doc().unwrap().doc.clone();
            let mut h = installed_harness(app, true);
            let undo = numbering_undo(&h);
            section_type(&mut h, "Start Page Numbering at:", "5");
            section_type(&mut h, "Start Chapter Numbering at:", "2");
            section_type(&mut h, label, invalid);
            section_click(&mut h, Role::Button, "OK");
            assert!(h.state().ui.dialog.is_some(), "refused {label} {invalid} stays open");
            assert!(!h.state().ui.status.is_empty());
            assert_eq!(h.state().session.doc().unwrap().doc, before, "neither section nor chapter may be partly written");
            assert_eq!(numbering_undo(&h), undo);
            assert!(numbering_commands(&h).is_empty());
            assert_eq!(h.get_by_role_and_label(Role::TextInput, label).value().as_deref(), Some(invalid));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_first_page_lock_and_remove_disable_only_section_controls() {
        let mut first = installed_harness(section_app(), true);
        assert!(first.get_by_role_and_label(Role::CheckBox, "Start Section").accesskit_node().is_disabled());
        section_click(&mut first, Role::CheckBox, "Start Section");
        assert!(section_dialog(&first).b("startSection"));
        assert!(!first.get_by_role_and_label(Role::TextInput, "Start Page Numbering at:").accesskit_node().is_disabled());
        section_click(&mut first, Role::Button, "Cancel");
        assert!(numbering_commands(&first).is_empty());

        let mut app = section_app();
        app.run("layout.section", json!({"page": 3, "startNumber": 5, "prefix": "B-"})).unwrap();
        set(&mut app, json!({"page": 3, "automatic": false, "startNumber": "5", "prefix": "B-"}));
        let before = app.session.doc().unwrap().doc.clone();
        let mut h = installed_harness(app, true);
        let undo = numbering_undo(&h);
        section_click(&mut h, Role::CheckBox, "Start Section");
        assert!(!section_dialog(&h).b("startSection"));
        for (role, label) in [(Role::TextInput, "Start Page Numbering at:"), (Role::TextInput, "Section Prefix:"), (Role::ComboBox, "Page Numbering")]
        {
            assert!(h.get_by_role_and_label(role, label).accesskit_node().is_disabled(), "section-disabled {label}");
        }
        assert!(!h.get_by_role_and_label(Role::TextInput, "Start Chapter Numbering at:").accesskit_node().is_disabled());
        section_type(&mut h, "Start Chapter Numbering at:", "3");
        section_click(&mut h, Role::Button, "OK");
        assert_eq!(
            numbering_commands(&h),
            vec![(
                "layout.numberingOptions".into(),
                json!({
                    "page": 3, "remove": true, "chapter": {"style": "arabic", "source": "userDefined", "start": 3}
                })
            )]
        );
        assert!(h.state().session.doc().unwrap().doc.sections.iter().all(|s| s.start != 2));
        assert_eq!(h.state().session.doc().unwrap().doc.page_name(2), "3");
        assert_eq!(h.state().session.doc().unwrap().doc.chapter_label(), "3");
        assert_eq!(numbering_undo(&h), undo + 1);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc, before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_bookless_previous_choice_is_disabled_and_book_name_is_readonly() {
        let mut h = installed_harness(section_app(), true);
        let label = "Same as Previous Document in the Book";
        assert!(h.get_by_role_and_label(Role::RadioButton, label).accesskit_node().is_disabled());
        section_click(&mut h, Role::RadioButton, label);
        assert_eq!(section_dialog(&h).s("chapterSource"), "automatic");
        assert_eq!(h.get_by_role_and_label(Role::Group, "Book Name:").value().as_deref(), Some("N/A"));
        assert!(h.query_by_role_and_label(Role::TextInput, "Book Name:").is_none(), "the displayed book name is a readout, not an editor");
        section_click(&mut h, Role::Button, "Cancel");
        assert!(numbering_commands(&h).is_empty());

        // UI membership fixture only: no book file is written and no book command is asserted.
        let mut app = section_app();
        set(&mut app, json!({"book": "Field guide"}));
        let mut h = installed_harness(app, true);
        assert!(!h.get_by_role_and_label(Role::RadioButton, label).accesskit_node().is_disabled());
        let displayed_book = crate::rtl::isolate("Field guide");
        assert_eq!(h.get_by_role_and_label(Role::Group, "Book Name:").value().as_deref(), Some(displayed_book.as_str()));
        assert!(h.query_by_role_and_label(Role::TextInput, "Book Name:").is_none());
        section_click(&mut h, Role::RadioButton, label);
        assert_eq!(section_dialog(&h).s("chapterSource"), "sameAsPrevious");
        section_click(&mut h, Role::Button, "Cancel");
        assert!(numbering_commands(&h).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_natural_tab_steps_the_real_style_picker_without_document_edits() {
        let mut h = installed_harness(section_app(), true);
        let before = h.state().session.doc().unwrap().doc.clone();
        section_type(&mut h, "Section Prefix:", "A-");
        section_key(&mut h, Key::Tab, Modifiers::NONE);
        assert!(h.get_by_role_and_label(Role::ComboBox, "Page Numbering").is_focused(), "natural Tab from prefix reaches the actual style picker");
        section_key(&mut h, Key::ArrowDown, Modifiers::NONE);
        assert_eq!(section_dialog(&h).s("style"), "upperRoman");
        section_key(&mut h, Key::Tab, Modifiers::NONE);
        assert!(h.get_by_role_and_label(Role::TextInput, "Section Marker:").is_focused(), "Tab leaves the picker for the real marker field");
        section_event(&mut h, Event::Text("Keyboard marker".into()));
        section_steps(&mut h, 3);
        assert_eq!(section_dialog(&h).s("marker"), "Keyboard marker");
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        assert!(numbering_commands(&h).is_empty());
        section_click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_long_marker_retains_full_draft_and_fallback_cancel_remains_usable() {
        let long = "A long section marker ".repeat(60);
        assert!(long.chars().count() > 1024);
        let mut h = installed_harness(section_app(), true);
        section_type(&mut h, "Section Marker:", &long);
        assert_eq!(section_dialog(&h).s("marker"), long);
        assert_eq!(h.get_by_role_and_label(Role::TextInput, "Section Marker:").value().as_deref(), Some(long.as_str()));
        section_click(&mut h, Role::Button, "OK");
        assert_eq!(h.state().session.doc().unwrap().doc.sections[0].marker, long);
        assert_eq!(numbering_commands(&h).len(), 1);
        let mut fallback = installed_harness(section_app(), false);
        section_click(&mut fallback, Role::Button, "Cancel");
        assert!(fallback.state().ui.dialog.is_none());
        assert!(numbering_commands(&fallback).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "requires a genuine Arabic UI font; run explicitly only when that native font precondition is supplied"]
    fn sdk_section_rtl_long_marker_with_genuine_arabic_ui_font() {
        let mut app = section_app();
        app.ui.language = "ar".into();
        let mut h = installed_harness(app, true);
        let prefix = crate::i18n::tr("ar", "Section Prefix:");
        let marker = crate::i18n::tr("ar", "Section Marker:");
        let font = egui::FontId::proportional(13.0);
        let required = format!("{}{}فصل", prefix, marker);
        assert!(
            h.ctx.fonts_mut(|fonts| fonts.has_glyphs(&font, &required)),
            "genuine Arabic glyphs are a precondition, not replacement-box visual acceptance"
        );
        let long = "فصل ".repeat(48);
        section_type(&mut h, marker, &long);
        assert_eq!(section_dialog(&h).s("marker"), long);
        let rect = h.get_by_role_and_label(Role::TextInput, marker).rect();
        assert!(rect.is_positive() && rect.width() > 0.0);
        section_click(&mut h, Role::Button, crate::i18n::tr("ar", "Cancel"));
        assert!(numbering_commands(&h).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_automatic_ok_ignores_inactive_invalid_number_drafts() {
        let mut app = section_app();
        set(&mut app, json!({"page": 3}));
        let before = app.session.doc().unwrap().doc.clone();
        let mut h = installed_harness(app, true);
        let undo = numbering_undo(&h);
        section_type(&mut h, "Start Page Numbering at:", "invalid");
        section_click(&mut h, Role::RadioButton, "Automatic Page Numbering");
        section_type(&mut h, "Start Chapter Numbering at:", "invalid");
        section_click(&mut h, Role::RadioButton, "Automatic Chapter Numbering");
        assert!(section_dialog(&h).b("automatic"));
        assert_eq!(section_dialog(&h).s("chapterSource"), "automatic");
        assert_eq!(section_dialog(&h).s("startNumber"), "invalid");
        assert_eq!(section_dialog(&h).s("chapterStart"), "invalid");
        section_click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none(), "inactive invalid number drafts must not block automatic numbering");
        assert_eq!(
            numbering_commands(&h),
            vec![(
                "layout.numberingOptions".into(),
                json!({
                    "page": 3, "startNumber": null, "style": "arabic", "prefix": "", "marker": "", "includePrefix": false,
                    "chapter": {"style": "arabic", "source": "automatic"}
                })
            )]
        );
        assert_eq!(h.state().session.doc().unwrap().doc.page_name(2), "3");
        assert!(h.state().session.doc().unwrap().doc.sections.iter().find(|s| s.start == 2).unwrap().start_number.is_none());
        assert_eq!(numbering_undo(&h), undo + 1);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc, before);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sdk_section_keyboard_enter_confirms_and_field_escape_cancels_without_command() {
        let app = section_app();
        let before = app.session.doc().unwrap().doc.clone();
        let mut h = installed_harness(app, true);
        let undo = numbering_undo(&h);
        let opening_fields = section_dialog(&h).fields.clone();
        assert!(!opening_fields.is_empty());
        section_type(&mut h, "Section Marker:", "Draft to cancel");
        // craft's dialogs, like InDesign's, cancel on one Escape: the field's typing goes with the dialog.
        section_key(&mut h, Key::Escape, Modifiers::NONE);
        assert!(h.state().ui.dialog.is_none(), "one real Escape over a field draft cancels the SDK modal");
        assert!(!craft_ui_widgets::runtime::overlay::any_open(&h.ctx, craft_ui_widgets::runtime::overlay::OpenFilter::Modal));
        assert_eq!(h.state().session.doc().unwrap().doc, before);
        assert_eq!(numbering_undo(&h), undo);
        assert!(numbering_commands(&h).is_empty());

        let mut h = installed_harness(section_app(), true);
        section_type(&mut h, "Section Prefix:", "A-");
        section_key(&mut h, Key::Enter, Modifiers::NONE);
        assert!(h.state().ui.dialog.is_none(), "Enter uses the existing native SDK dialog Accept route");
        assert_eq!(numbering_commands(&h).len(), 1);
        assert_eq!(numbering_commands(&h)[0].1["prefix"], "A-");
        assert_eq!(h.state().session.doc().unwrap().doc.sections[0].prefix, "A-");
    }
}
