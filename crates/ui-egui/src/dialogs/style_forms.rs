//! The framework forms of the style dialogs: Paragraph Style Options and Character Style Options (sections beside a
//! page, the General page, the live preview), Delete Paragraph Style and Delete Character Style, and Load Styles.
//! Each edits the draft the native window edits and leaves OK, Cancel, the preview and every command to the existing
//! controller, so documents, undo and saved settings are what they were.

#[path = "cjk_forms.rs"]
pub(crate) mod cjk_forms;
#[path = "style_kit.rs"]
pub(crate) mod style_kit;
#[path = "tab_forms.rs"]
pub(crate) mod tab_forms;

use super::Form;
use crate::DesignApp;
use crate::dialogs::text_forms::{self, Attrs};
use crate::dialogs::{CharFields, Dialog, paragraph_forms};
use craft_ui_widgets::choice::{Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Props as _, Scroll, Show as _, ShowContainer as _};
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::{Row, Stack};
use egui::Ui;
use serde_json::{Value, json};
use style_kit::{field, heading, line, note, pick, push, text, toggle};

/// Paragraph Style Options, with its live preview.
pub(crate) const PARAGRAPH_STYLE_OPTIONS: Form = Form::new(paragraph_style_options).width(760.0);
/// Character Style Options, with its live preview.
pub(crate) const CHARACTER_STYLE_OPTIONS: Form = Form::new(character_style_options).width(760.0);
/// Delete Paragraph Style: what the text using it is reassigned to.
pub(crate) const DELETE_PARAGRAPH_STYLE: Form = Form::new(delete_paragraph_style).width(380.0);
/// Delete Character Style: what the text using it is reassigned to.
pub(crate) const DELETE_CHARACTER_STYLE: Form = Form::new(delete_character_style).width(380.0);
/// The Ruby dialog: the reading over the selected text and the four ruby panes.
pub(crate) const RUBY: Form = Form::new(cjk_forms::ruby).width(700.0);
/// Load Styles: which styles of the chosen file to bring in, and what to do on a name conflict.
pub(crate) const LOAD_STYLES: Form = Form::new(load_styles).width(480.0);

fn tr(app: &DesignApp, s: &str) -> String {
    crate::i18n::tr(&app.ui.language, s).to_string()
}

/// The Preview check box under the options, and the preview it turns on: the changed attributes go through the
/// same engine commands as OK, on the page, until Cancel or OK takes the preview back.
fn preview(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) {
    if d.b("new") {
        return;
    }
    let label = tr(app, "Preview");
    if let Some(on) = toggle(ui, "style_preview", &label, Some(d.b("preview")), true) {
        d.fields.insert("preview".into(), json!(on));
    }
    app.ui.style_preview = d.b("preview");
    crate::dialogs::style_options_preview(app, d);
}

/// The sections beside the page they show: a list of radio buttons that scrolls at the dialog's fixed height.
fn section_nav(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, sections: &[(&str, &str)], title: &str) {
    let before = d.s("section");
    Stack::new().id_salt("style_sections_box").scroll(Scroll::vertical()).room_height(380.0).show(ui, |ui, _| {
        let shown = RadioGroup::new(title.to_owned(), Bind::given(before.clone())).id_salt("style_sections").show(ui, |ui, _| {
            Stack::new().id_salt("style_section_list").show(ui, |ui, _| {
                for (id, caption) in sections {
                    Radio::new((*id).to_owned()).label(tr(app, caption)).id_salt(("style_section", (*id).to_owned())).show(ui);
                }
            });
        });
        if let Some(next) = shown.output.events.committed().cloned().filter(|next| *next != before) {
            d.fields.insert("section".into(), json!(next));
        }
    });
}

/// The summary of what OK would leave a style setting: its Based On, then each setting.
fn summary(ui: &mut Ui, text: &str) {
    Stack::new().id_salt("style_summary").fill_width().scroll(Scroll::vertical()).room_height(180.0).show(ui, |ui, _| note(ui, text));
}

/// A status line: why OK did not apply.
fn status_line(ui: &mut Ui, d: &Dialog) {
    let status = d.s("status");
    if !status.is_empty() {
        line(ui, &status);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Paragraph Style Options

fn paragraph_style_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let name = d.s("name");
    let new = d.b("new");
    let doc = app.session.active().map(|s| s.doc.clone())?;
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
            line(ui, &format!("No style named {name}"));
            return None;
        }
    };
    if new && !d.fields.contains_key("rename") {
        d.fields.insert("rename".into(), json!(crate::dialogs::next_style_name(&doc.styles, true)));
    }
    let mut style = style;
    let reset = !new && d.b("resetToBase");
    if reset {
        style.para = Default::default();
        style.chars = Default::default();
    }
    let names: Vec<String> = doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| new || *n != name).collect();
    // A new style shows what it would inherit from its Based On (as does one reset to it).
    let based_name = d.fields.get("basedOn").and_then(Value::as_str).map(str::to_string).or(style.based_on.clone()).unwrap_or_default();
    // An existing style shows its own settings over the Based On chosen here.
    let (pp, cp) = if new || reset { doc.styles.resolve_para_style(&based_name) } else { crate::dialogs::style_over(&doc.styles, &based_name, &style) };
    let units = doc.settings.horizontal_units;
    let pv = serde_json::to_value(&pp).unwrap_or_default();
    let cv = serde_json::to_value(&cp).unwrap_or_default();
    {
        let app: &DesignApp = app;
        let cjk = crate::cjk_features(app);
        let cnames: Vec<String> = doc.styles.character.iter().map(|c| c.name.clone()).collect();
        let cf = CharFields { base: &cv, sparse: false };
        let base = [
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
        ];
        let sections = crate::dialogs::style_cjk::sections(cjk, true, &base);
        let title = tr(app, "Paragraph Style Options");
        let section = d.s("section");
        let lang = app.ui.language.as_str();
        let attrs = Attrs::new("p.", &pv);
        Stack::new().id_salt("paragraph_style_options").fill_width().show(ui, |ui, _| {
            Row::new().id_salt("paragraph_style_layout").fill_width().show(ui, |ui, _| {
                section_nav(app, ui, d, &sections, &title);
                Stack::new().id_salt("paragraph_style_page").fill_width().show(ui, |ui, _| match section.as_str() {
                    "export" => export_tagging(app, ui, d, &name),
                    "nested" => paragraph_forms::nested_styles(lang, ui, d, &attrs, &cnames),
                    "grep" => paragraph_forms::grep_styles(lang, ui, d, &attrs, &cnames),
                    // The shared character sections (dialogs/char_sections.rs).
                    "chars" => crate::dialogs::char_sections::basic(app, ui, d, &cf),
                    "indents" => indents(lang, ui, d, &attrs, units),
                    "tabs" => tabs_page(app, ui, d, &pv, &doc),
                    "rules" => text_forms::rules_form(app, ui, d, "p.", &pv),
                    "hyph" => text_forms::hyphenation_form(lang, ui, d, "p.", &pv, units),
                    "justify" => text_forms::justification_form(lang, ui, d, "p.", &pv),
                    "color" => crate::dialogs::char_sections::color(app, ui, d, &cf, &doc),
                    "advanced" => crate::dialogs::char_sections::advanced(app, ui, d, &cf),
                    "openType" => crate::dialogs::char_sections::open_type(app, ui, d, &cf),
                    "underline" => crate::dialogs::char_sections::line(app, ui, d, &cf, &doc, "underline"),
                    "strikethrough" => crate::dialogs::char_sections::line(app, ui, d, &cf, &doc, "strikethrough"),
                    "border" | "shading" => paragraph_forms::border_shading(lang, ui, d, &attrs, &doc, section == "border"),
                    "keep" => paragraph_forms::keep_options(lang, ui, d, &attrs),
                    "span" => paragraph_forms::span_columns(lang, ui, d, &attrs, &doc),
                    "bullets" => {
                        let (char_styles, families, fonts, scope) = crate::dialogs::ListUi::new_parts(app);
                        let bullets = paragraph_forms::Bullets {
                            char_styles: &char_styles,
                            families: &families,
                            fonts: &fonts,
                            scope,
                            family: &cp.font_family,
                            style: &cp.font_style,
                        };
                        paragraph_forms::bullets_and_numbering(lang, ui, d, &attrs, &doc, &bullets);
                    }
                    s if crate::dialogs::style_cjk::is_section(s) => cjk_forms::page(app, ui, d, s, &cf, Some(&pv), &doc),
                    _ => paragraph_general(app, ui, d, &doc, &style, &names, &based_name),
                });
            });
            // Why OK didn't apply (a pattern that can't compile, a taken name).
            status_line(ui, d);
        });
    }
    preview(app, ui, d);
    None
}

/// Paragraph Style Options › General: name, Based On, Next Style, Reset To Base and the settings the style has.
fn paragraph_general(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, doc: &designcraft_doc::Document, style: &designcraft_doc::ParagraphStyle, names: &[String], based_name: &str) {
    let (name, new) = (d.s("name"), d.b("new"));
    let lang = app.ui.language.as_str();
    if !d.fields.contains_key("rename") {
        d.fields.insert("rename".into(), json!(name));
    }
    let based = d.fields.get("basedOn").and_then(Value::as_str).map(str::to_string).or(style.based_on.clone()).unwrap_or_else(|| "[No Paragraph Style]".into());
    let entries: Vec<(String, String)> = names.iter().map(|n| (n.clone(), crate::i18n::style_name(lang, n).to_string())).collect();
    // Next Style: the style of the paragraph that follows when Enter ends this one.
    let next = match d.fields.get("nextStyle") {
        Some(Value::String(n)) => n.clone(),
        Some(_) => String::new(),
        None => style.next_style.clone().filter(|n| *n != name).unwrap_or_default(),
    };
    let mut next_entries = vec![(String::new(), tr(app, "[Same style]"))];
    next_entries.extend(entries.iter().filter(|(n, _)| n != designcraft_doc::NO_PARA_STYLE).cloned());
    let (name_label, based_label, next_label) = (tr(app, "Style Name:"), tr(app, "Based On:"), tr(app, "Next Style:"));
    Stack::new().id_salt("paragraph_general").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "General")).id_salt("paragraph_general_fields").show(ui, |ui, _| {
            field(ui, &name_label, "rename", |ui| text(ui, d, "rename", "style_rename", &name_label, true));
            field(ui, &based_label, "basedOn", |ui| {
                if let Some(picked) = pick(ui, "style_based_on", &based_label, &based, &entries, true) {
                    d.fields.insert("basedOn".into(), json!(picked));
                }
            });
            field(ui, &next_label, "nextStyle", |ui| {
                if let Some(picked) = pick(ui, "style_next", &next_label, &next, &next_entries, true) {
                    d.fields.insert("nextStyle".into(), if picked.is_empty() { Value::Null } else { json!(picked) });
                }
            });
        });
        // Reset To Base: the style keeps nothing of its own (the edits made here stay).
        if !new && push(ui, "reset_to_base", &tr(app, "Reset To Base"), !based_name.is_empty() && based_name != designcraft_doc::NO_PARA_STYLE) {
            d.fields.insert("resetToBase".into(), json!(true));
        }
        heading(ui, &tr(app, "Style Settings:"));
        summary(ui, &crate::dialogs::paragraph_style_summary(lang, d, style, &doc.styles));
        if new && let Some(on) = toggle(ui, "style_apply", &tr(app, "Apply Style to Selection"), Some(d.b("apply")), true) {
            d.fields.insert("apply".into(), json!(on));
        }
    });
}

/// Indents and Spacing: alignment, the five distances, Align to Grid and Ignore Optical Margin.
fn indents(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, units: designcraft_geom::Unit) {
    use designcraft_doc::GridAlign;
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let aligns: Vec<(Value, String)> = designcraft_doc::Align::ALL.iter().filter_map(|al| Some((serde_json::to_value(al).ok()?, tr(al.label())))).collect();
    let grid: Vec<(Value, String)> = [(GridAlign::None, "None"), (GridAlign::AllLines, "All Lines"), (GridAlign::FirstLineOnly, "First Line Only")]
        .into_iter()
        .filter_map(|(value, caption)| Some((serde_json::to_value(value).ok()?, tr(caption))))
        .collect();
    Stack::new().id_salt("style_indents").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Indents and Spacing")).id_salt("style_indents_fields").show(ui, |ui, _| {
            text_forms::field(ui, &tr("Alignment:"), "align", |ui| text_forms::choice(ui, d, a, "align", &tr("Alignment:"), &aligns, true));
            for (caption, key) in [
                ("Left Indent:", "leftIndent"),
                ("First Line Indent:", "firstLineIndent"),
                ("Right Indent:", "rightIndent"),
                ("Space Before:", "spaceBefore"),
                ("Space After:", "spaceAfter"),
            ] {
                text_forms::field(ui, &tr(caption), key, |ui| text_forms::measure(ui, d, a, key, &tr(caption), units, true));
            }
            // The CJK Grid Settings section edits the same `p.gridAlign` field.
            text_forms::field(ui, &tr("Align to Grid:"), "gridAlign", |ui| text_forms::choice(ui, d, a, "gridAlign", &tr("Align to Grid:"), &grid, true));
        });
        text_forms::check(ui, d, a, "ignoreOpticalMargin", &tr("Ignore Optical Margin"), true);
    });
}

/// Export Tagging: the tag and class a style exports with, and whether it splits the document (EPUB and HTML).
fn export_tagging(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, name: &str) {
    let lang = app.ui.language.as_str();
    let et = app.session.active().and_then(|s| s.doc.styles.export_tag(name, false).cloned()).unwrap_or_default();
    if !d.fields.contains_key("x.tag") {
        d.fields.insert("x.tag".into(), json!(if et.tag.is_empty() { "[Automatic]" } else { et.tag.as_str() }));
        d.fields.insert("x.class".into(), json!(et.class));
        d.fields.insert("x.split".into(), json!(et.split));
    }
    let tags: Vec<(String, String)> = ["[Automatic]", "p", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "li", "figcaption", "aside", "div"]
        .into_iter()
        .map(|t| (t.to_owned(), crate::i18n::tr(lang, t).to_string()))
        .collect();
    let (tag_label, class_label) = (tr(app, "Tag:"), tr(app, "Class:"));
    Stack::new().id_salt("style_export").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "EPUB and HTML")).id_salt("style_export_fields").show(ui, |ui, _| {
            field(ui, &tag_label, "x_tag", |ui| {
                let current = d.s("x.tag");
                if let Some(next) = pick(ui, "style_export_tag", &tag_label, &current, &tags, true) {
                    d.fields.insert("x.tag".into(), json!(next));
                }
            });
            field(ui, &class_label, "x_class", |ui| text(ui, d, "x.class", "style_export_class", &class_label, true));
        });
        if let Some(on) = toggle(ui, "style_export_split", &tr(app, "Split Document (EPUB only)"), Some(d.b("x.split")), true) {
            d.fields.insert("x.split".into(), json!(on));
        }
    });
}

/// Tabs: the style's stops on a ruler as wide as the first page's column, edited in `p.tabs` (and the indents in
/// `p.leftIndent` and the others) until OK.
fn tabs_page(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document) {
    tab_forms::tabs(app, ui, d, pv, doc);
}

// ---------------------------------------------------------------------------------------------------------------------
// Character Style Options

fn character_style_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let name = d.s("name");
    let new = d.b("new");
    let doc = app.session.active().map(|st| st.doc.clone())?;
    let styles = &doc.styles;
    let style = match styles.char_style(&name) {
        _ if new => designcraft_doc::CharacterStyle { name: String::new(), based_on: None, chars: Default::default(), shortcut: String::new() },
        Some(s) if name != designcraft_doc::NO_CHAR_STYLE => s.clone(),
        _ => {
            line(ui, &format!("No style named {name}"));
            return None;
        }
    };
    if !d.fields.contains_key("rename") {
        d.fields.insert("rename".into(), json!(if new { crate::dialogs::next_style_name(styles, false) } else { name.clone() }));
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
    {
        let app: &DesignApp = app;
        let cf = CharFields { base: &base, sparse: true };
        let cjk = crate::cjk_features(app);
        let list = [
            ("general", "General"),
            ("chars", "Basic Character Formats"),
            ("advanced", "Advanced Character Formats"),
            ("color", "Character Color"),
            ("openType", "OpenType Features"),
            ("underline", "Underline Options"),
            ("strikethrough", "Strikethrough Options"),
        ];
        let sections = crate::dialogs::style_cjk::sections(cjk, false, &list);
        let title = tr(app, "Character Style Options");
        Stack::new().id_salt("character_style_options").fill_width().show(ui, |ui, _| {
            Row::new().id_salt("character_style_layout").fill_width().show(ui, |ui, _| {
                section_nav(app, ui, d, &sections, &title);
                Stack::new().id_salt("character_style_page").fill_width().show(ui, |ui, _| match d.s("section").as_str() {
                    // The shared character sections (dialogs/char_sections.rs, from the frame dialogs' branch).
                    "chars" => crate::dialogs::char_sections::basic(app, ui, d, &cf),
                    "advanced" => crate::dialogs::char_sections::advanced(app, ui, d, &cf),
                    "color" => crate::dialogs::char_sections::color(app, ui, d, &cf, &doc),
                    "openType" => crate::dialogs::char_sections::open_type(app, ui, d, &cf),
                    "underline" => crate::dialogs::char_sections::line(app, ui, d, &cf, &doc, "underline"),
                    "strikethrough" => crate::dialogs::char_sections::line(app, ui, d, &cf, &doc, "strikethrough"),
                    s if crate::dialogs::style_cjk::is_section(s) => cjk_forms::page(app, ui, d, s, &cf, None, &doc),
                    _ => {
                        character_general(app, ui, d, styles, &style, &based);
                        if new && let Some(on) = toggle(ui, "style_apply", &tr(app, "Apply Style to Selection"), Some(d.b("apply")), true) {
                            d.fields.insert("apply".into(), json!(on));
                        }
                    }
                });
            });
        });
    }
    preview(app, ui, d);
    None
}

/// Character Style Options › General: name, Based On, and a summary of the style's settings.
fn character_general(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, styles: &designcraft_doc::Styles, style: &designcraft_doc::CharacterStyle, based: &str) {
    let lang = app.ui.language.as_str();
    let (name, new) = (d.s("name"), d.b("new"));
    let entries: Vec<(String, String)> = styles
        .character
        .iter()
        // Not itself, nor a style based on it.
        .filter(|s| new || !styles.char_based_on_cycles(&name, &s.name))
        .map(|s| (s.name.clone(), crate::i18n::style_name(lang, &s.name).to_string()))
        .collect();
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
    let mut parts = vec![crate::i18n::style_name(lang, based).to_string()];
    parts.extend(own.as_object().into_iter().flatten().map(|(k, v)| format!("{}: {}", crate::dialogs::setting_label(lang, k), crate::dialogs::setting_text(v))));
    let (name_label, based_label) = (tr(app, "Style Name:"), tr(app, "Based On:"));
    Stack::new().id_salt("character_general").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "General")).id_salt("character_general_fields").show(ui, |ui, _| {
            field(ui, &name_label, "rename", |ui| text(ui, d, "rename", "style_rename", &name_label, true));
            field(ui, &based_label, "basedOn", |ui| {
                if let Some(picked) = pick(ui, "style_based_on", &based_label, based, &entries, true) {
                    d.fields.insert("basedOn".into(), json!(picked));
                }
            });
        });
        status_line(ui, d);
        heading(ui, &tr(app, "Style Settings:"));
        note(ui, &parts.join(" + "));
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Delete styles

/// Whether style `name` is in use, and what the text using it can be reassigned to.
fn delete_style(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog, paragraph: bool) -> Option<bool> {
    let name = d.s("name");
    let lang = app.ui.language.clone();
    let kind = if paragraph { "paragraph" } else { "character" };
    let question = crate::i18n::tr(&lang, if paragraph { "Delete the paragraph style \"{name}\"?" } else { "Delete the character style \"{name}\"?" })
        .replace("{name}", crate::i18n::style_name(&lang, &name));
    let st = app.session.active()?;
    let (in_use, names): (bool, Vec<String>) = if paragraph {
        (
            st.doc.stories.values().any(|s| s.paras.iter().any(|f| f.style == name)),
            st.doc.styles.paragraph.iter().map(|p| p.name.clone()).filter(|n| *n != name).collect(),
        )
    } else {
        (
            st.doc.stories.values().any(|s| s.chars.iter().any(|r| r.format.style == name)),
            std::iter::once(designcraft_doc::NO_CHAR_STYLE.to_owned()).chain(st.doc.styles.character.iter().map(|c| c.name.clone()).filter(|n| *n != name)).collect(),
        )
    };
    let default = if paragraph { designcraft_doc::BASIC_PARAGRAPH } else { designcraft_doc::NO_CHAR_STYLE };
    if !d.fields.contains_key("replaceWith") {
        d.fields.insert("replaceWith".into(), json!(default));
    }
    let entries: Vec<(String, String)> = names.iter().map(|n| (n.clone(), crate::i18n::style_name(&lang, n).to_string())).collect();
    let replace = d.s("replaceWith");
    let prompt = crate::i18n::tr(&lang, if in_use { "This style is in use. Text using it will be reassigned to:" } else { "Replace with:" }).to_string();
    Stack::new().id_salt(("delete_style", kind)).fill_width().show(ui, |ui, _| {
        line(ui, &question);
        note(ui, &prompt);
        if let Some(next) = pick(ui, &format!("delete_{kind}_replace"), &prompt, &replace, &entries, true) {
            d.fields.insert("replaceWith".into(), json!(next));
        }
    });
    None
}

fn delete_paragraph_style(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    delete_style(app, ui, d, true)
}

fn delete_character_style(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    delete_style(app, ui, d, false)
}

// ---------------------------------------------------------------------------------------------------------------------
// Load Styles

/// The path of the file the styles come from, Select All and Deselect All, the styles to bring in (each marked where
/// it differs from the document's) and, when a chosen one conflicts, what to do about the conflict.
fn load_styles(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    use craft_ui_widgets::choice::Checkbox;
    let (select_all, deselect_all) = (tr(app, "Select All"), tr(app, "Deselect All"));
    let lang = app.ui.language.clone();
    let rows_of = |d: &Dialog, key: &str| -> Vec<(String, bool, bool)> {
        d.fields
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter_map(|row| Some((row.get("name")?.as_str()?.to_owned(), row.get("on") != Some(&json!(false)), row.get("conflict") == Some(&json!(true)))))
            .collect()
    };
    let groups = [("paragraph", "Paragraph Styles"), ("character", "Character Styles")];
    let listed: Vec<(&str, &str, Vec<(String, bool, bool)>)> = groups.iter().map(|(key, title)| (*key, *title, rows_of(d, key))).collect();
    let conflicts = listed.iter().flat_map(|(_, _, rows)| rows).filter(|(_, on, conflict)| *on && *conflict).count();
    let mut set_all = None;
    let mut toggled: Vec<(&str, usize, bool)> = Vec::new();
    let differs = tr(app, "Differs from this document");
    let (conflicts_title, none_label) = (tr(app, "Style Name Conflicts"), tr(app, "No conflicts"));
    let policy = d.s("conflicts");
    let policies = [
        ("redefine", tr(app, "Redefine Document Style")),
        ("useExisting", tr(app, "Use Document Style Definition")),
        ("autoRename", tr(app, "Auto Rename")),
    ];
    let mut chosen = None;
    Stack::new().id_salt("load_styles").fill_width().show(ui, |ui, _| {
        note(ui, &d.s("path"));
        Row::new().id_salt("load_styles_buttons").show(ui, |ui, _| {
            if push(ui, "load_select_all", &select_all, true) {
                set_all = Some(true);
            }
            if push(ui, "load_deselect_all", &deselect_all, true) {
                set_all = Some(false);
            }
        });
        Stack::new().id_salt("load_styles_list").fill_width().scroll(Scroll::vertical()).room_height(240.0).show(ui, |ui, _| {
            for (key, title, rows) in &listed {
                if rows.is_empty() {
                    continue;
                }
                heading(ui, &tr(app, title));
                for (at, (name, on, conflict)) in rows.iter().enumerate() {
                    Row::new().id_salt(("load_style_row", format!("{key}{at}"))).show(ui, |ui, _| {
                        let out = Checkbox::new(Bind::given(*on)).label(crate::i18n::style_name(&lang, name).to_owned()).id_salt(("load_style", format!("{key}{at}"))).show(ui);
                        if let Some(next) = out.events.committed().copied().filter(|next| next != on) {
                            toggled.push((*key, at, next));
                        }
                        if *conflict {
                            note(ui, &differs);
                        }
                    });
                }
            }
        });
        heading(ui, &conflicts_title);
        if conflicts == 0 {
            line(ui, &none_label);
            return;
        }
        let shown = RadioGroup::new(conflicts_title.clone(), Bind::given(policy.clone())).id_salt("load_styles_conflicts").show(ui, |ui, _| {
            Row::new().show(ui, |ui, _| {
                for (value, caption) in &policies {
                    Radio::new((*value).to_owned()).label(caption.clone()).show(ui);
                }
            });
        });
        chosen = shown.output.events.committed().cloned().filter(|next| *next != policy);
    });
    if let Some(on) = set_all {
        for key in ["paragraph", "character"] {
            for row in d.fields.get_mut(key).and_then(Value::as_array_mut).into_iter().flatten().filter_map(Value::as_object_mut) {
                row.insert("on".into(), json!(on));
            }
        }
    }
    for (key, at, on) in toggled {
        if let Some(row) = d.fields.get_mut(key).and_then(Value::as_array_mut).and_then(|rows| rows.iter_mut().filter_map(Value::as_object_mut).nth(at)) {
            row.insert("on".into(), json!(on));
        }
    }
    if let Some(next) = chosen {
        d.fields.insert("conflicts".into(), json!(next));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    type Window = Harness<'static, DesignApp>;

    fn app_of() -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("file.new", json!({})).unwrap();
        app
    }

    /// The app's dialog host with the framework installed, as the desktop shell runs it.
    fn open(mut app: DesignApp) -> Window {
        let ctx = egui::Context::default();
        app.framework_transform = false;
        app.framework_opacity = true;
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert!(app.framework_installed, "the framework forms need the installed runtime");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(1280.0, 1000.0)).build_ui_state(
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

    /// Clicks the first control of `role` named `label`.
    fn click(h: &mut Window, role: Role, label: &str) {
        let at = h.query_all_by_role_and_label(role, label).next().unwrap_or_else(|| panic!("no {role:?} named {label}")).rect().center();
        click_at(h, at);
    }

    /// Chooses `option` in the menu named `label`.
    fn choose(h: &mut Window, label: &str, option: &str) {
        click(h, Role::ComboBox, label);
        let at = h.get_by_label(option).rect().center();
        click_at(h, at);
    }

    /// Types `text` over what the field named `label` of `role` holds, by real pointer and key events.
    fn type_into(h: &mut Window, role: Role, label: &str, text: &str) {
        click(h, role, label);
        for pressed in [true, false] {
            event(h, Event::Key { key: Key::A, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND });
        }
        event(h, Event::Text(text.into()));
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        steps(h, 3);
    }

    fn dialog(h: &Window) -> &Dialog {
        h.state().ui.dialog.as_ref().expect("the dialog is open")
    }

    fn shows(h: &Window, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    fn journal(h: &Window, command: &str) -> Vec<Value> {
        h.state().session.journal.iter().filter(|(id, _)| id == command).map(|(_, params)| params.clone()).collect()
    }

    fn para_style(h: &Window, name: &str) -> designcraft_doc::ParagraphStyle {
        h.state().session.active().unwrap().doc.styles.para(name).cloned().unwrap_or_else(|| panic!("no paragraph style {name}"))
    }

    /// A document with paragraph style `Body` applied to its text.
    fn with_body_style() -> DesignApp {
        let mut app = app_of();
        app.run("style.paragraph.create", json!({"name": "Body"})).unwrap();
        let story = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap()["story"].as_u64().unwrap();
        app.run("text.insert", json!({"text": "Hello"})).unwrap();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        app.run("style.paragraph.apply", json!({"name": "Body"})).unwrap();
        app
    }

    #[test]
    fn every_style_dialog_has_a_framework_form() {
        for id in ["paragraphStyleOptions", "characterStyleOptions", "deleteParagraphStyle", "deleteCharacterStyle", "loadStyles", "ruby"] {
            assert!(super::super::form(id).is_some(), "{id} is registered with the host");
        }
    }

    #[test]
    fn delete_paragraph_style_reassigns_text_to_the_chosen_style_and_cancel_keeps_it() {
        let setup = || {
            let mut app = with_body_style();
            app.run("style.paragraph.create", json!({"name": "Other"})).unwrap();
            app.ui.dialog = Some(Dialog::new("deleteParagraphStyle", json!({"name": "Body"})));
            app
        };
        let story_style = |h: &Window| h.state().session.active().unwrap().doc.stories.values().next().unwrap().paras[0].style.clone();
        let mut h = open(setup());
        assert!(shows(&h, "Delete the paragraph style \"Body\"?"));
        assert!(shows(&h, "This style is in use. Text using it will be reassigned to:"));
        choose(&mut h, "This style is in use. Text using it will be reassigned to:", "Other");
        assert_eq!(dialog(&h).s("replaceWith"), "Other");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().session.active().unwrap().doc.styles.para("Body").is_none(), "style removed");
        assert_eq!(story_style(&h), "Other", "text reassigned to the chosen style");

        let mut h = open(setup());
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().session.active().unwrap().doc.styles.para("Body").is_some());
        assert_eq!(story_style(&h), "Body");
    }

    #[test]
    fn delete_character_style_defaults_to_none_and_reassigns_to_the_chosen_style() {
        let setup = || {
            let mut app = app_of();
            app.run("style.character.create", json!({"name": "Key Term"})).unwrap();
            app.run("style.character.create", json!({"name": "Plain"})).unwrap();
            app.ui.dialog = Some(Dialog::new("deleteCharacterStyle", json!({"name": "Key Term"})));
            app
        };
        let mut h = open(setup());
        assert_eq!(dialog(&h).s("replaceWith"), designcraft_doc::NO_CHAR_STYLE, "the default replacement");
        assert!(shows(&h, "Replace with:"));
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().session.active().unwrap().doc.styles.char_style("Key Term").is_none());

        let mut h = open(setup());
        choose(&mut h, "Replace with:", "Plain");
        assert_eq!(dialog(&h).s("replaceWith"), "Plain");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().session.active().unwrap().doc.styles.char_style("Key Term").is_some(), "Cancel deletes nothing");
    }

    #[test]
    fn paragraph_style_general_renames_and_sets_the_next_style_in_one_command() {
        let mut app = with_body_style();
        app.run("style.paragraph.create", json!({"name": "Caption"})).unwrap();
        crate::dialogs::open_style_options(&mut app, true, "Body");
        let mut h = open(app);
        assert_eq!(dialog(&h).s("rename"), "Body");
        type_into(&mut h, Role::TextInput, "Style Name:", "Headline");
        assert_eq!(dialog(&h).s("rename"), "Headline");
        choose(&mut h, "Next Style:", "Caption");
        assert_eq!(dialog(&h).s("nextStyle"), "Caption");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let style = para_style(&h, "Headline");
        assert_eq!(style.next_style.as_deref(), Some("Caption"));
        assert!(h.state().session.active().unwrap().doc.styles.para("Body").is_none(), "renamed, not copied");
        assert_eq!(journal(&h, "style.paragraph.edit").len(), 1, "one edit command");
    }

    #[test]
    fn a_new_paragraph_style_is_made_with_the_typed_name_and_its_edits() {
        let mut app = app_of();
        crate::dialogs::open_new_style(&mut app, true);
        let mut h = open(app);
        assert_eq!(dialog(&h).s("rename"), "Paragraph Style 1");
        assert!(h.query_by_role_and_label(Role::CheckBox, "Preview").is_none(), "a new style has no preview");
        type_into(&mut h, Role::TextInput, "Style Name:", "Headline");
        click(&mut h, Role::RadioButton, "Keep Options");
        click(&mut h, Role::CheckBox, "Keep Lines Together");
        assert_eq!(dialog(&h).fields["p.keepLinesTogether"], json!(true));
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(para_style(&h, "Headline").para.keep_lines_together, Some(true));
        assert_eq!(journal(&h, "style.paragraph.create").len(), 1);
    }

    #[test]
    fn paragraph_style_indents_edit_a_measure_in_points_whatever_the_unit() {
        let mut app = with_body_style();
        crate::dialogs::open_style_options(&mut app, true, "Body");
        app.ui.dialog.as_mut().unwrap().fields.insert("section".into(), json!("indents"));
        let mut h = open(app);
        type_into(&mut h, Role::TextInput, "Left Indent:", "10pt");
        key(&mut h, Key::Tab);
        assert_eq!(dialog(&h).fields["p.leftIndent"], json!(10.0));
        click(&mut h, Role::Button, "OK");
        assert_eq!(para_style(&h, "Body").para.left_indent, Some(10.0));
    }

    #[test]
    fn preview_shows_the_edit_on_the_page_and_cancel_takes_it_back() {
        let mut app = with_body_style();
        crate::dialogs::open_style_options(&mut app, true, "Body");
        app.ui.dialog.as_mut().unwrap().fields.insert("section".into(), json!("indents"));
        let mut h = open(app);
        assert!(h.query_by_role_and_label(Role::CheckBox, "Preview").is_some());
        click(&mut h, Role::CheckBox, "Preview");
        assert!(dialog(&h).b("preview") && h.state().ui.style_preview, "the choice is remembered for the next dialog");
        type_into(&mut h, Role::TextInput, "Left Indent:", "10pt");
        key(&mut h, Key::Tab);
        assert_eq!(para_style(&h, "Body").para.left_indent, Some(10.0), "the page shows the edit while the dialog is open");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(para_style(&h, "Body").para.left_indent, None, "Cancel restores the document");
    }

    #[test]
    fn character_style_general_renames_the_style() {
        let mut app = app_of();
        app.run("style.character.create", json!({"name": "Key Term"})).unwrap();
        crate::dialogs::open_style_options(&mut app, false, "Key Term");
        let mut h = open(app);
        type_into(&mut h, Role::TextInput, "Style Name:", "Term");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let styles = &h.state().session.active().unwrap().doc.styles;
        assert!(styles.char_style("Term").is_some() && styles.char_style("Key Term").is_none());
    }

    fn load_dialog() -> DesignApp {
        let mut app = app_of();
        app.ui.dialog = Some(Dialog::new(
            "loadStyles",
            json!({
                "path": "/tmp/book.designcraft",
                "paragraph": [{"name": "Alpha", "on": true, "conflict": false}, {"name": "Beta", "on": true, "conflict": true}],
                "character": [{"name": "Gamma", "on": true}],
                "conflicts": "redefine",
            }),
        ));
        app
    }

    #[test]
    fn load_styles_toggles_styles_marks_conflicts_and_chooses_how_to_resolve_them() {
        let mut h = open(load_dialog());
        assert!(shows(&h, "/tmp/book.designcraft") && shows(&h, "Differs from this document"));
        let on = |h: &Window, key: &str, at: usize| dialog(h).fields[key][at]["on"] != json!(false);
        click(&mut h, Role::CheckBox, "Alpha");
        assert!(!on(&h, "paragraph", 0) && on(&h, "paragraph", 1) && on(&h, "character", 0));
        click(&mut h, Role::Button, "Deselect All");
        assert!(!on(&h, "paragraph", 0) && !on(&h, "paragraph", 1) && !on(&h, "character", 0));
        assert!(shows(&h, "No conflicts"), "a conflicting style that is not chosen conflicts with nothing");
        click(&mut h, Role::Button, "Select All");
        assert!(on(&h, "paragraph", 0) && on(&h, "paragraph", 1) && on(&h, "character", 0));
        click(&mut h, Role::RadioButton, "Auto Rename");
        assert_eq!(dialog(&h).s("conflicts"), "autoRename");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
    }

    #[test]
    fn the_tab_ruler_adds_a_stop_where_the_strip_is_clicked() {
        let mut app = with_body_style();
        crate::dialogs::open_style_options(&mut app, true, "Body");
        app.ui.dialog.as_mut().unwrap().fields.insert("section".into(), json!("tabs"));
        let width = app.session.active().unwrap().doc.page(0).and_then(|p| p.column_rects().first().map(|r| r.width())).unwrap();
        let mut h = open(app);
        let rect = h.get_by_role_and_label(Role::Group, "Tabs").rect();
        let at = egui::pos2(rect.min.x + PAD_FOR_TEST + 60.0, rect.min.y + 5.0);
        click_at(&mut h, at);
        let stops = dialog(&h).fields["p.tabs"].as_array().cloned().expect("a stop was added");
        assert_eq!(stops.len(), 1);
        let scale = (rect.width() - 2.0 * PAD_FOR_TEST) / width as f32;
        let expected = f64::from(((at.x - (rect.min.x + PAD_FOR_TEST)) / scale * 2.0).round() / 2.0);
        assert!((stops[0]["position"].as_f64().unwrap() - expected).abs() < 1.0, "{stops:?} at {expected}");
        click(&mut h, Role::Button, "OK");
        assert_eq!(para_style(&h, "Body").para.tabs.len(), 1, "OK keeps the stop in the style");
    }

    /// The ruler's inset of position 0 from its left edge.
    const PAD_FOR_TEST: f32 = 10.0;

    #[test]
    fn escape_cancels_each_style_dialog_without_a_command() {
        let makers: Vec<Box<dyn Fn() -> DesignApp>> = vec![
            Box::new(|| {
                let mut app = with_body_style();
                crate::dialogs::open_style_options(&mut app, true, "Body");
                app
            }),
            Box::new(|| {
                let mut app = app_of();
                app.run("style.character.create", json!({"name": "Key Term"})).unwrap();
                crate::dialogs::open_style_options(&mut app, false, "Key Term");
                app
            }),
            Box::new(|| {
                let mut app = with_body_style();
                app.ui.dialog = Some(Dialog::new("deleteParagraphStyle", json!({"name": "Body"})));
                app
            }),
            Box::new(load_dialog),
        ];
        for make in makers {
            let app = make();
            let id = app.ui.dialog.as_ref().unwrap().id.clone();
            let (commands, doc) = (app.session.journal.len(), app.session.active().unwrap().doc.clone());
            let mut h = open(app);
            key(&mut h, Key::Escape);
            assert!(h.state().ui.dialog.is_none(), "one Escape closes {id}");
            assert_eq!(h.state().session.journal.len(), commands, "{id}: Escape runs no command");
            assert_eq!(h.state().session.active().unwrap().doc, doc, "{id}: the document is unchanged");
        }
    }

    /// Successor, under the framework host, of `cjk_sections_draw`: every CJK section of both style dialogs and the Ruby
    /// panes draws and leaves its dialog open.
    #[test]
    fn cjk_sections_draw_in_both_style_dialogs() {
        let ids = [
            "cjk.autoTcy",
            "cjk.tcy",
            "cjk.rubyPlacement",
            "cjk.rubyFont",
            "cjk.rubyLonger",
            "cjk.rubyColor",
            "cjk.kenten",
            "cjk.kentenColor",
            "cjk.shatai",
            "cjk.composition",
            "cjk.grid",
            "cjk.warichu",
        ];
        for id in ids {
            for dialog_id in ["paragraphStyleOptions", "characterStyleOptions"] {
                let mut app = with_body_style();
                app.run("style.character.create", json!({"name": "Emphasis"})).unwrap();
                app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
                let name = if dialog_id == "paragraphStyleOptions" { "Body" } else { "Emphasis" };
                app.ui.dialog = Some(Dialog::new(dialog_id, json!({"name": name, "section": id})));
                let h = open(app);
                assert!(h.state().ui.dialog.is_some(), "{dialog_id} {id} stays open");
            }
        }
    }

    #[test]
    fn kenten_and_japanese_composition_reach_the_style() {
        let mut app = with_body_style();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        app.ui.dialog = Some(Dialog::new("paragraphStyleOptions", json!({"name": "Body", "section": "cjk.kenten"})));
        let mut h = open(app);
        choose(&mut h, "Kenten Type:", "Black Circle");
        assert_eq!((dialog(&h).fields["c.kenten"].clone(), dialog(&h).fields["c.kentenKind"].clone()), (json!(true), json!("blackCircle")));
        click(&mut h, Role::RadioButton, "Japanese Composition Settings");
        choose(&mut h, "Hanging Type:", "Standard");
        assert_eq!(dialog(&h).s("p.kinsokuHang"), "regular");
        click(&mut h, Role::Button, "OK");
        let style = para_style(&h, "Body");
        assert_eq!(style.chars.kenten, Some(true));
        assert_eq!(style.para.kinsoku_hang, Some(designcraft_doc::cjk::KinsokuHang::Regular));
    }

    /// Successor, under the framework host, of `ruby_dialog_applies_its_settings_with_type_ruby`.
    #[test]
    fn the_ruby_dialog_applies_its_reading_with_type_ruby() {
        let mut app = app_of();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "漢字かな"})).unwrap();
        app.run("text.select", json!({"story": r["story"], "anchor": 0, "focus": "漢字".len()})).unwrap();
        app.run("app.rubyDialog", json!({})).unwrap();
        let mut h = open(app);
        assert_eq!((dialog(&h).id.as_str(), dialog(&h).s("section").as_str()), ("ruby", "cjk.rubyPlacement"));
        type_into(&mut h, Role::TextInput, "Ruby:", "かんじ");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let a = h.state_mut().run("type.selectionAttrs", json!({})).unwrap();
        assert_eq!(a["chars"]["ruby"], "かんじ");
        assert_eq!(journal(&h, "type.ruby").len(), 1);
    }
}
