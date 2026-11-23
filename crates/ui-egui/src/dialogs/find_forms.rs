//! Framework forms of Find/Change (Text, GREP and Object, with the Find and Change Format Settings over it), Find/Replace
//! Font and User Dictionary. They edit the drafts and run the commands the native bodies do: the buttons that act at once
//! (Find Next, Change, Change All, Count, Add, Remove) call the same engine commands, and OK, Cancel and Done go through
//! the host's `confirm` and `cancel`.

use super::frame_forms::{field, message, text};
use super::{CHANGE_FORMAT, CharFields, Dialog, FIND_FORMAT, FORMAT_EDIT, FORMAT_SECTIONS};
use super::{char_sections, framework::Form};
use crate::DesignApp;
use craft_ui_panels::dialog::Dialog as FrameworkDialog;
use craft_ui_panels::sections::{DialogRole, FooterButton};
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::{Radio, RadioGroup, Segmented};
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Scroll, Show as _, ShowContainer as _, Sizing};
use craft_ui_widgets::display::Text;
use craft_ui_widgets::form::{Field, FieldGroup, Help};
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::runtime::overlay::Backdrop;
use craft_ui_widgets::style::TypeRole;
use egui::Ui;
use serde_json::{Map, Value, json};

/// Find/Change.
pub(crate) const FIND_CHANGE: Form = Form::new(find_change);
/// Find/Replace Font.
pub(crate) const FIND_FONT: Form = Form::new(find_font);
/// User Dictionary.
pub(crate) const USER_DICTIONARY: Form = Form::new(user_dictionary);

/// A picker over `current` from `options` (value, caption), with the value picked in this pass. A value none of them has
/// shows as the placeholder.
pub(super) fn picker(ui: &mut Ui, salt: &str, label: &str, current: &str, options: &[(String, String)], searchable: bool) -> Option<String> {
    let entries: Vec<Entry<String>> = options.iter().map(|(value, caption)| Entry::new(value.clone(), caption.clone())).collect();
    let mut picker =
        Select::new(label.to_owned(), Bind::given(current.to_owned()), &entries).keeps_name(true).id_salt(("find_picker", salt.to_owned()));
    if searchable {
        picker = picker.searchable();
    }
    if !options.iter().any(|(value, _)| value == current) {
        picker = picker.placeholder(current.to_owned());
    }
    picker.show(ui).events.committed().cloned().filter(|next| next != current)
}

/// A line of wrapping text.
fn line(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).sizing(Sizing::FillWidth).show(ui);
}

/// A bold heading.
fn heading(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).role(TypeRole::BodyStrong).show(ui);
}

// ---------------------------------------------------------------------------------------------------------------------
// Find/Change

fn find_change(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let modes = [Entry::new("text".to_owned(), tr("Text")), Entry::new("grep".to_owned(), tr("GREP")), Entry::new("object".to_owned(), tr("Object"))];
    Stack::new().id_salt("find_change").fill_width().show(ui, |ui, _| {
        let mode = if d.b("objectMode") {
            "object"
        } else if d.b("grep") {
            "grep"
        } else {
            "text"
        };
        let out = Segmented::new(tr("Find/Change"), Bind::given(mode.to_owned()), &modes).id_salt("find_change_mode").show(ui);
        if let Some(next) = out.events.committed().cloned().filter(|next| next != mode) {
            let object = next == "object";
            if d.set_field("objectMode", json!(object)) {
                app.session.reset_find_scope();
            }
            if !object {
                d.fields.insert("grep".into(), json!(next == "grep"));
            }
        }
        if d.b("objectMode") {
            object_page(app, ui, d, &lang);
        } else {
            text_page(app, ui, d, &lang);
        }
        if d.fields.get(FORMAT_EDIT).is_some_and(Value::is_object) {
            format_settings(app, ui, d);
        }
    });
    None
}

/// Find/Change › Object: objects by fill and kind; change their fill.
fn object_page(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let swatches: Vec<String> =
        app.session.active().map(|st| st.doc.swatches.iter().filter(|w| !w.hidden).map(|w| w.name.clone()).collect()).unwrap_or_default();
    let fills = |any: &str| -> Vec<(String, String)> {
        std::iter::once((String::new(), tr(any))).chain(swatches.iter().map(|name| (name.clone(), name.clone()))).collect()
    };
    let (find_fills, change_fills) = (fills("(any)"), fills("(unchanged)"));
    let kinds: Vec<(String, String)> = ["", "text", "graphic", "shape", "line", "group"]
        .iter()
        .map(|kind| ((*kind).to_owned(), tr(if kind.is_empty() { "(any)" } else { *kind })))
        .collect();
    FieldGroup::new(tr("Object")).id_salt("find_objects").show(ui, |ui, _| {
        field(ui, &tr("Find fill:"), "find_fill", |ui| {
            if let Some(next) = picker(ui, "objFill", &tr("Find fill:"), &d.s("objFill"), &find_fills, false) {
                d.fields.insert("objFill".into(), json!(next));
            }
        });
        field(ui, &tr("Change fill to:"), "change_fill", |ui| {
            if let Some(next) = picker(ui, "objChangeFill", &tr("Change fill to:"), &d.s("objChangeFill"), &change_fills, false) {
                d.fields.insert("objChangeFill".into(), json!(next));
            }
        });
        field(ui, &tr("Kind:"), "kind", |ui| {
            if let Some(next) = picker(ui, "objKind", &tr("Kind:"), &d.s("objKind"), &kinds, false) {
                d.fields.insert("objKind".into(), json!(next));
            }
        });
    });
    let mut criteria = json!({});
    if !d.s("objFill").is_empty() {
        criteria["fill"] = json!(d.s("objFill"));
    }
    if !d.s("objKind").is_empty() {
        criteria["kind"] = json!(d.s("objKind"));
    }
    Row::new().id_salt("find_objects_actions").show(ui, |ui, _| {
        if Button::new(tr("Find All")).id_salt("find_all_objects").show(ui).clicked() {
            let found = app.run("find.objects", criteria.clone()).ok().and_then(|r| r["ids"].as_array().map(Vec::len)).unwrap_or(0);
            d.fields.insert("status".into(), json!(format!("{found} found")));
        }
        if Button::new(tr("Change All")).id_salt("change_all_objects").show(ui).clicked() && !d.s("objChangeFill").is_empty() {
            let mut params = criteria.clone();
            params["change"] = json!({"fill": d.s("objChangeFill")});
            let changed = app.run("find.changeObjects", params).ok().and_then(|r| r["changed"].as_u64()).unwrap_or(0);
            d.fields.insert("status".into(), json!(format!("{changed} changed")));
        }
    });
    let status = d.s("status");
    if !status.is_empty() {
        line(ui, &status);
    }
}

/// Find/Change › Text and GREP.
fn text_page(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog, lang: &str) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let (find_formatted, change_formatted) = (super::format_set(d, FIND_FORMAT), super::format_set(d, CHANGE_FORMAT));
    let scopes: Vec<(String, String)> = [("document", "Document"), ("story", "Story"), ("selection", "Selection")]
        .iter()
        .map(|(value, caption)| ((*value).to_owned(), tr(caption)))
        .collect();
    let find_help = find_formatted.then(|| tr("With a Find Format, only text that has it is found; with nothing in Find what, all text that has it"));
    let change_help = change_formatted.then(|| tr("With a Change Format and nothing in Change to, the found text stays and is only formatted"));
    FieldGroup::new(tr("Find/Change")).id_salt("find_text").show(ui, |ui, _| {
        for (caption, key, help) in [("Find what:", "find", &find_help), ("Change to:", "change", &change_help)] {
            let mut f = Field::new(tr(caption)).id_salt(("find_field", key));
            if let Some(help) = help {
                f = f.help(Help::Text(help.clone().into()));
            }
            f.show(ui, |ui, _| {
                text(ui, d, key, &tr(caption), true);
            });
        }
        field(ui, &tr("Search:"), "scope", |ui| {
            if let Some(next) = picker(ui, "scope", &tr("Search:"), &d.s("scope"), &scopes, false)
                && d.set_field("scope", json!(next))
            {
                app.session.reset_find_scope();
            }
        });
    });
    Row::new().id_salt("find_options").show(ui, |ui, _| {
        super::frame_forms::check(ui, d, "caseSensitive", &tr("Case sensitive"), true);
        super::frame_forms::check(ui, d, "wholeWord", &tr("Whole word"), true);
    });
    format_box(ui, d, lang, true);
    format_box(ui, d, lang, false);
    let params = super::find_change_params(d);
    // Change and Change/Find act on the text Find Next found.
    let mut found = params.clone();
    found["found"] = json!(true);
    let selected = app.session.active().and_then(|st| st.selection.text).is_some_and(|t| !t.is_caret());
    let next_status = |app: &DesignApp, r: Result<Value, String>| match r {
        Ok(Value::Null) => crate::i18n::tr(&app.ui.language, "No matches").to_string(),
        Ok(_) => crate::i18n::tr(&app.ui.language, "Found").to_string(),
        Err(e) => e,
    };
    let changed = |app: &DesignApp, r: Result<Value, String>| match r {
        Ok(v) => crate::i18n::tr(&app.ui.language, "{count} replacement(s) made").replace("{count}", &v["count"].to_string()),
        Err(e) => e,
    };
    let mut status = None;
    Row::new().id_salt("find_actions").show(ui, |ui, _| {
        if Button::new(tr("Find Next")).id_salt("find_next").show(ui).clicked() {
            let r = app.run("find.next", params.clone());
            status = Some(next_status(app, r));
        }
        if Button::new(tr("Change")).id_salt("find_change").enabled(selected).show(ui).clicked() {
            let r = app.run("find.change", found.clone());
            status = Some(changed(app, r));
        }
        if Button::new(tr("Change All")).id_salt("find_change_all").show(ui).clicked() {
            let r = app.run("find.change", params.clone());
            status = Some(changed(app, r));
        }
        if Button::new(tr("Change/Find")).id_salt("find_change_find").enabled(selected).show(ui).clicked() {
            let r = app.run("find.change", found.clone()).and_then(|_| app.run("find.next", params.clone()));
            status = Some(next_status(app, r));
        }
        if Button::new(tr("Count")).id_salt("find_count").show(ui).clicked() {
            let r = match app.run("find.find", params.clone()) {
                Ok(v) => crate::i18n::tr(&app.ui.language, "{count} match(es)").replace("{count}", &v.as_array().map_or(0, |a| a.len()).to_string()),
                Err(e) => e,
            };
            status = Some(r);
        }
    });
    if let Some(status) = status {
        d.fields.insert("status".into(), json!(status));
    }
    let status = d.s("status");
    if !status.is_empty() {
        message(ui, &status);
    }
}

/// Find/Change's Find Format (`find`) or Change Format: what the found text must have or what Change All also gives the
/// changed text, with a button that opens its settings and one that clears it.
fn format_box(ui: &mut Ui, d: &mut Dialog, lang: &str, find: bool) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let (key, label, specify) = if find {
        (FIND_FORMAT, "Find Format:", "Specify attributes to find")
    } else {
        (CHANGE_FORMAT, "Change Format:", "Specify attributes to change")
    };
    let summary = super::format_summary(lang, d.fields.get(key));
    Stack::new().id_salt(("find_format_box", key)).fill_width().show(ui, |ui, _| {
        Text::new(tr(label)).show(ui);
        Row::new().id_salt(("find_format_row", key)).fill_width().show(ui, |ui, _| {
            Text::new(if summary.is_empty() { "\u{2014}".to_owned() } else { summary.clone() }).sizing(Sizing::FillWidth).show(ui);
            if Button::new(tr(specify)).id_salt(("find_format_specify", key)).show(ui).clicked() {
                let current = d.fields.get(key).cloned().unwrap_or_default();
                let mut edit = Map::new();
                edit.insert("for".into(), json!(if find { "find" } else { "change" }));
                edit.insert("section".into(), json!("style"));
                for name in ["charStyle", "paraStyle"] {
                    if let Some(v) = current.get(name) {
                        edit.insert(name.into(), v.clone());
                    }
                }
                for (k, v) in current.get("chars").and_then(Value::as_object).into_iter().flatten() {
                    edit.insert(format!("c.{k}"), v.clone());
                }
                d.fields.insert(FORMAT_EDIT.into(), Value::Object(edit));
            }
            if Button::new(tr("Clear specified attributes")).id_salt(("find_format_clear", key)).show(ui).clicked() {
                d.fields.remove(key);
            }
        });
    });
}

/// Find or Change Format Settings, over Find/Change: Style Options (the character and paragraph style the found text has
/// or the changed text gets, [Any Style] for none), then the character sections of Character Style Options with every
/// field blank (any value, or left as it is). OK makes it the Find or Change Format, Cancel drops it.
fn format_settings(app: &DesignApp, ui: &mut Ui, d: &mut Dialog) {
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let Some(doc) = app.session.active().map(|st| st.doc.clone()) else {
        d.fields.remove(FORMAT_EDIT);
        return;
    };
    let mut fd = Dialog {
        id: "changeFormat".into(),
        fields: d.fields.get(FORMAT_EDIT).and_then(Value::as_object).cloned().unwrap_or_default(),
        unit: d.unit,
        vertical_unit: None,
        contents: None,
    };
    if fd.s("section").is_empty() {
        fd.fields.insert("section".into(), json!("style"));
    }
    let find = fd.s("for") == "find";
    let base = json!({});
    let cf = CharFields { base: &base, sparse: true };
    let mut opened = true;
    let out = FrameworkDialog::new(tr(if find { "Find Format Settings" } else { "Change Format Settings" }))
        .id_salt("find_format_settings")
        .opened(Bind::bound(&mut opened))
        .enabled(ui.is_enabled())
        .backdrop(Backdrop::None)
        .closable(false)
        .outer_gesture(false)
        .close_on_enter(true)
        .buttons([FooterButton::new(tr("OK"), DialogRole::Accept).stays_open(true), FooterButton::new(tr("Cancel"), DialogRole::Cancel)])
        .show(ui, |ui, _| {
            Row::new().id_salt("find_format_layout").fill_width().show(ui, |ui, _| {
                let before = fd.s("section");
                let shown = RadioGroup::new(tr("Format Settings"), Bind::given(before.clone())).id_salt("find_format_sections").show(ui, |ui, _| {
                    Stack::new().id_salt("find_format_section_list").show(ui, |ui, _| {
                        for (id, caption) in FORMAT_SECTIONS {
                            Radio::new((*id).to_owned()).label(tr(*caption)).id_salt(("find_format_section", *id)).show(ui);
                        }
                    });
                });
                if let Some(next) = shown.output.events.landed().filter(|next| **next != before).cloned() {
                    fd.fields.insert("section".into(), json!(next));
                }
                Stack::new().id_salt("find_format_page").fill_width().show(ui, |ui, _| match fd.s("section").as_str() {
                    "chars" => char_sections::basic(app, ui, &mut fd, &cf),
                    "advanced" => char_sections::advanced(app, ui, &mut fd, &cf),
                    "color" => char_sections::color(app, ui, &mut fd, &cf, &doc),
                    "openType" => char_sections::open_type(app, ui, &mut fd, &cf),
                    "underline" => char_sections::line(app, ui, &mut fd, &cf, &doc, "underline"),
                    "strikethrough" => char_sections::line(app, ui, &mut fd, &cf, &doc, "strikethrough"),
                    _ => style_options(lang, ui, &mut fd, &doc.styles),
                });
            });
        })
        .output;
    if out.picked == Some(0) {
        d.fields.remove(FORMAT_EDIT);
        let mut format = Map::new();
        for key in ["charStyle", "paraStyle"] {
            if let Some(v) = fd.fields.get(key).filter(|v| v.is_string()) {
                format.insert(key.into(), v.clone());
            }
        }
        let chars: Map<String, Value> =
            fd.fields.iter().filter(|(_, v)| !v.is_null()).filter_map(|(k, v)| Some((k.strip_prefix("c.")?.to_string(), v.clone()))).collect();
        if !chars.is_empty() {
            format.insert("chars".into(), Value::Object(chars));
        }
        let key = if find { FIND_FORMAT } else { CHANGE_FORMAT };
        if format.is_empty() {
            d.fields.remove(key);
        } else {
            d.fields.insert(key.into(), Value::Object(format));
        }
    } else if out.closed.is_some() {
        d.fields.remove(FORMAT_EDIT);
    } else {
        d.fields.insert(FORMAT_EDIT.into(), Value::Object(fd.fields));
    }
}

/// Format Settings › Style Options: the character and paragraph style ([Any Style]: any, or as it is).
fn style_options(lang: &str, ui: &mut Ui, fd: &mut Dialog, styles: &designcraft_doc::Styles) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let any = tr("[Any Style]");
    let named = |names: Vec<&str>| -> Vec<(String, String)> {
        std::iter::once((String::new(), any.clone()))
            .chain(names.into_iter().map(|n| (n.to_owned(), crate::i18n::style_name(lang, n).to_string())))
            .collect()
    };
    let chars = named(styles.character.iter().map(|s| s.name.as_str()).collect());
    let paras = named(styles.paragraph.iter().map(|s| s.name.as_str()).collect());
    heading(ui, &tr("Style Options"));
    FieldGroup::new(tr("Style Options")).id_salt("find_format_styles").show(ui, |ui, _| {
        for (key, label, options) in [("charStyle", "Character Style:", &chars), ("paraStyle", "Paragraph Style:", &paras)] {
            field(ui, &tr(label), key, |ui| {
                let current = fd.fields.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
                if let Some(next) = picker(ui, key, &tr(label), &current, options, false) {
                    if next.is_empty() {
                        fd.fields.remove(key);
                    } else {
                        fd.fields.insert(key.into(), json!(next));
                    }
                }
            });
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Find/Replace Font

/// The family a replacement keeps its style in: the style of the same name, else Regular, else the first.
fn replacement_style(styles: &[String], preferred: &str) -> Option<String> {
    styles
        .iter()
        .find(|s| s.eq_ignore_ascii_case(preferred))
        .or_else(|| styles.iter().find(|s| s.eq_ignore_ascii_case("Regular")))
        .or_else(|| styles.first())
        .cloned()
}

fn find_font(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
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
    let key = format!("{}\n{}", d.s("family"), d.s("style"));
    if let Some(font) = fonts.iter().find(|f| f["family"] == d.s("family") && f["style"] == d.s("style"))
        && d.s("_fontKey") != key
    {
        d.fields.insert("_fontKey".into(), json!(key));
        d.fields.insert("toFamily".into(), font["replacementFamily"].clone());
        d.fields.insert("toStyle".into(), font["replacementStyle"].clone());
    }
    if d.s("toFamily").is_empty() {
        d.fields.insert("toFamily".into(), json!(designcraft_fonts::FALLBACK_FAMILY));
    }
    let db = crate::panels::fonts(app);
    let styles = db.styles(&d.s("toFamily"));
    if !styles.iter().any(|style| *style == d.s("toStyle"))
        && let Some(style) = replacement_style(&styles, &d.s("style"))
    {
        d.fields.insert("toStyle".into(), json!(style));
    }
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let missing = |f: &Value| f["missing"] == true || f["styleMissing"] == true;
    let english = app.session.prefs.show_font_names_in_english;
    let families: Vec<(String, String)> = crate::panels::font_menu(app).iter().map(|f| (f.family.clone(), f.label(english).to_owned())).collect();
    let style_options: Vec<(String, String)> = styles.iter().map(|s| (s.clone(), s.clone())).collect();
    let missing_count = fonts.iter().filter(|f| missing(f)).count();
    let at = fonts.iter().position(|f| f["family"] == d.s("family") && f["style"] == d.s("style")).unwrap_or(0);
    let rendered = fonts.get(at).filter(|f| f["family"] == d.s("family") && f["style"] == d.s("style")).map(|font| {
        format!(
            "{}: {} {}",
            tr("Currently Rendered With"),
            font["resolvedFamily"].as_str().unwrap_or(""),
            font["resolvedStyle"].as_str().unwrap_or("")
        )
    });
    Stack::new().id_salt("find_font").fill_width().show(ui, |ui, _| {
        if d.b("onOpen") && fonts.iter().any(missing) {
            line(ui, &tr("This document uses unavailable fonts. Choose replacements, or select Done to continue with fallback fonts."));
        }
        if !d.s("error").is_empty() {
            line(ui, &d.s("error"));
        }
        line(
            &mut *ui,
            &tr("Fonts in Document: {count}    Missing: {missing}")
                .replace("{count}", &fonts.len().to_string())
                .replace("{missing}", &missing_count.to_string()),
        );
        let before = at;
        let shown = RadioGroup::new(tr("Fonts in Document"), Bind::given(before)).id_salt("find_font_list").show(ui, |ui, _| {
            Stack::new().id_salt("find_font_rows").fill_width().scroll(Scroll::vertical()).room_height(180.0).show(ui, |ui, _| {
                for (i, f) in fonts.iter().enumerate() {
                    let label = format!(
                        "{}{} {}",
                        if missing(f) { "\u{26A0} " } else { "" },
                        f["family"].as_str().unwrap_or(""),
                        f["style"].as_str().unwrap_or("")
                    );
                    Radio::new(i).label(label).id_salt(("find_font_row", i)).show(ui);
                }
            });
        });
        if let Some(next) = shown.output.events.landed().copied().filter(|next| *next != before)
            && let Some(font) = fonts.get(next)
        {
            d.fields.insert("family".into(), font["family"].clone());
            d.fields.insert("style".into(), font["style"].clone());
        }
        if let Some(rendered) = &rendered {
            line(ui, rendered);
        }
        heading(ui, &tr("Replace With"));
        FieldGroup::new(tr("Replace With")).id_salt("find_font_replace").show(ui, |ui, _| {
            field(ui, &tr("Font Family:"), "to_family", |ui| {
                if let Some(family) = picker(ui, "toFamily", &tr("Font Family:"), &d.s("toFamily"), &families, true) {
                    let styles = crate::panels::fonts(app).styles(&family);
                    let style = if styles.iter().any(|s| s == "Regular") { Some("Regular".to_owned()) } else { styles.first().cloned() };
                    d.fields.insert("toFamily".into(), json!(family));
                    d.fields.insert("toStyle".into(), json!(style.unwrap_or_default()));
                }
            });
            field(ui, &tr("Font Style:"), "to_style", |ui| {
                if let Some(style) = picker(ui, "toStyle", &tr("Font Style:"), &d.s("toStyle"), &style_options, false) {
                    d.fields.insert("toStyle".into(), json!(style));
                }
            });
        });
        message(ui, &tr("Replace All updates this font throughout text and styles. Done keeps remaining fallback fonts."));
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// User Dictionary

fn user_dictionary(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let words = app.session.execute("hyphenation.list", &json!({})).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let (mut add, mut remove) = (false, None);
    Stack::new().id_salt("user_dictionary").fill_width().show(ui, |ui, _| {
        heading(ui, &tr("Hyphenation Exceptions"));
        line(ui, &tr("Type a word with ~ at each allowed break (ex~am~ple); without ~ the word is never hyphenated."));
        Row::new().id_salt("user_dictionary_add").fill_width().show(ui, |ui, _| {
            text(ui, d, "word", &tr("Word"), true);
            add |= Button::new(tr("Add")).id_salt("user_dictionary_add_button").show(ui).clicked();
        });
        Stack::new().id_salt("user_dictionary_words").fill_width().scroll(Scroll::vertical()).room_height(180.0).show(ui, |ui, _| {
            for (at, word) in words.iter().enumerate() {
                let word = word.as_str().unwrap_or("").to_owned();
                Row::new().id_salt(("user_dictionary_word", at)).fill_width().show(ui, |ui, _| {
                    Text::new(word.clone()).sizing(Sizing::FillWidth).show(ui);
                    if Button::new(tr("Remove")).id_salt(("user_dictionary_remove", at)).size(craft_theme::ControlSize::Sm).show(ui).clicked() {
                        remove = Some(word.clone());
                    }
                });
            }
        });
    });
    if add {
        let word = d.s("word");
        if !word.trim().is_empty() {
            match app.run("hyphenation.addException", json!({"word": word.trim()})) {
                Ok(_) => {
                    d.fields.insert("word".into(), json!(""));
                }
                Err(e) => app.status_error("User Dictionary", e),
            }
        }
    }
    if let Some(word) = remove {
        let _ = app.run("hyphenation.removeException", json!({"word": word}));
    }
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::{Key, Modifiers, accesskit::Role};
    use egui_kittest::kittest::{NodeT, Queryable};

    const BUTTON: &[Role] = &[Role::Button];
    const CHECK: &[Role] = &[Role::CheckBox];
    const RADIO: &[Role] = &[Role::RadioButton];

    fn open(app: DesignApp, id: &str, fields: Value) -> egui_kittest::Harness<'static, DesignApp> {
        let mut app = app;
        app.ui.dialog = Some(Dialog::new(id, fields));
        kit::harness(app)
    }

    fn app_with_text() -> DesignApp {
        let mut app = kit::app(1, false);
        app.run("frame.create", json!({"rect": [72, 72, 372, 372], "content": "text", "text": "Hello world"})).unwrap();
        app
    }

    /// Regression: Find/Change in the framework host runs the same find and change commands as the native window, and its
    /// Text/GREP/Object switch keeps `grep` and `objectMode` as before.
    #[test]
    fn find_change_runs_count_and_change_all_and_switches_modes() {
        let mut h = open(app_with_text(), "findChange", json!({}));
        let _ = h.get_by_role_and_label(Role::Dialog, "Find/Change");
        kit::type_and_commit(&mut h, "Find what:", 0, "world");
        kit::type_and_commit(&mut h, "Change to:", 0, "there");
        assert_eq!((kit::draft(&h).s("find"), kit::draft(&h).s("change")), ("world".to_owned(), "there".to_owned()));
        kit::click(&mut h, BUTTON, "Count", 0);
        assert_eq!(kit::draft(&h).s("status"), "1 match(es)");
        kit::click(&mut h, BUTTON, "Change All", 0);
        assert_eq!(kit::draft(&h).s("status"), "1 replacement(s) made");
        let sent = kit::journal(&h, "find.change");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!((sent[0].1["find"].as_str(), sent[0].1["change"].as_str()), (Some("world"), Some("there")));
        let left = h.state_mut().run("find.find", json!({"find": "world", "scope": "document"})).unwrap();
        assert_eq!(left.as_array().map(Vec::len), Some(0), "the text was changed");
        kit::click(&mut h, RADIO, "GREP", 0);
        assert!(kit::draft(&h).b("grep") && !kit::draft(&h).b("objectMode"));
        kit::click(&mut h, RADIO, "Object", 0);
        assert!(kit::draft(&h).b("objectMode"));
        kit::click(&mut h, BUTTON, "Find All", 0);
        assert!(kit::draft(&h).s("status").ends_with("found"), "{}", kit::draft(&h).s("status"));
    }

    /// Regression: Find Format Settings opens over Find/Change; OK makes its choices the Find Format, and Escape drops
    /// them while Find/Change itself stays open.
    #[test]
    fn find_format_settings_ok_sets_the_format_and_escape_keeps_find_change_open() {
        let mut app = app_with_text();
        app.run("style.character.create", json!({"name": "Emph"})).unwrap();
        let mut h = open(app, "findChange", json!({}));
        kit::click(&mut h, BUTTON, "Specify attributes to find", 0);
        assert!(kit::draft(&h).fields.get(FORMAT_EDIT).is_some_and(Value::is_object), "the settings are open");
        let _ = h.get_by_role_and_label(Role::Dialog, "Find Format Settings");
        kit::pick(&mut h, "Character Style:", "Emph");
        kit::click(&mut h, RADIO, "Basic Character Formats", 0);
        // A character attribute cycles unset, on, off, unset.
        kit::click(&mut h, CHECK, "Underline", 0);
        assert_eq!(kit::draft(&h).fields[FORMAT_EDIT]["c.underline"], true);
        kit::click(&mut h, CHECK, "Underline", 0);
        assert_eq!(kit::draft(&h).fields[FORMAT_EDIT]["c.underline"], false);
        kit::click(&mut h, CHECK, "Underline", 0);
        assert!(kit::draft(&h).fields[FORMAT_EDIT]["c.underline"].is_null());
        kit::click(&mut h, CHECK, "Ligatures", 0);
        kit::type_and_commit(&mut h, "Size:", 0, "14");
        assert_eq!(kit::draft(&h).fields[FORMAT_EDIT]["c.size"], 14.0);
        kit::key(&mut h, Key::Enter, Modifiers::NONE);
        let d = kit::draft(&h);
        assert!(d.fields.get(FORMAT_EDIT).is_none(), "OK closes the settings");
        assert_eq!(d.fields[FIND_FORMAT], json!({"charStyle": "Emph", "chars": {"ligatures": true, "size": 14.0}}));

        kit::click(&mut h, BUTTON, "Specify attributes to find", 0);
        kit::pick(&mut h, "Character Style:", "[Any Style]");
        kit::key(&mut h, Key::Escape, Modifiers::NONE);
        let d = kit::draft(&h);
        assert!(d.fields.get(FORMAT_EDIT).is_none(), "Escape closes the settings");
        assert_eq!(d.fields[FIND_FORMAT]["charStyle"], "Emph", "and drops the edit");
        assert_eq!(d.id, "findChange", "Find/Change stays open");
    }

    /// Regression: Find/Replace Font lists the document's fonts, selects one, and Done closes without a command.
    #[test]
    fn find_font_lists_the_fonts_and_done_closes_without_replacing() {
        let mut h = open(app_with_text(), "findFont", json!({}));
        let _ = h.get_by_role_and_label(Role::Dialog, "Find/Replace Font");
        let d = kit::draft(&h);
        assert!(!d.s("family").is_empty() && !d.s("toFamily").is_empty(), "a font is selected and a replacement chosen");
        let label = format!("{} {}", d.s("family"), d.s("style"));
        let listed = h.query_all_by_role(Role::RadioButton).any(|node| node.accesskit_node().label().is_some_and(|name| name.ends_with(&label)));
        assert!(listed, "the selected font is a row of the list");
        kit::click(&mut h, BUTTON, "Done", 0);
        assert!(h.state().ui.dialog.is_none());
        assert!(kit::journal(&h, "font.replace").is_empty());
    }

    /// Regression: Replace All sends the chosen font and replacement to `font.replace` and keeps the dialog open.
    #[test]
    fn find_font_replace_all_sends_the_draft_and_stays_open() {
        let mut h = open(app_with_text(), "findFont", json!({}));
        let d = kit::draft(&h).clone();
        kit::click(&mut h, BUTTON, "Replace All", 0);
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("findFont"), "the dialog stays for more replacements");
        let sent = kit::journal(&h, "font.replace");
        let failed = !kit::draft(&h).s("error").is_empty();
        assert!(failed || sent.len() == 1, "ran once or reported why not: {sent:?}");
        if let Some((_, params)) = sent.first() {
            assert_eq!((params["family"].as_str(), params["toFamily"].as_str()), (Some(d.s("family").as_str()), Some(d.s("toFamily").as_str())));
        }
    }

    /// Regression: User Dictionary adds a typed exception, lists it, and removes it, each at once.
    #[test]
    fn user_dictionary_adds_lists_and_removes_exceptions() {
        let mut h = open(kit::app(1, false), "userDictionary", json!({}));
        let _ = h.get_by_role_and_label(Role::Dialog, "User Dictionary");
        kit::type_and_commit(&mut h, "Word", 0, "ex~am~ple");
        kit::click(&mut h, BUTTON, "Add", 0);
        let sent = kit::journal(&h, "hyphenation.addException");
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].1["word"], "ex~am~ple");
        assert_eq!(kit::draft(&h).s("word"), "", "the field is cleared");
        let _ = h.get_by_label("ex~am~ple");
        kit::click(&mut h, BUTTON, "Remove", 0);
        assert_eq!(kit::journal(&h, "hyphenation.removeException").len(), 1);
        assert!(h.query_by_label("ex~am~ple").is_none());
    }

    /// Regression: the character pages of the format settings write their attributes as `c.<name>` and leave the rest
    /// unset, which is what makes a Change Format sparse.
    #[test]
    fn format_settings_character_pages_set_only_what_was_touched() {
        let mut h = open(app_with_text(), "findChange", json!({}));
        kit::click(&mut h, BUTTON, "Specify attributes to change", 0);
        let language = crate::panels::properties::LANGUAGES[0];
        kit::click(&mut h, RADIO, "Advanced Character Formats", 0);
        kit::type_and_commit(&mut h, "Horizontal Scale:", 0, "120");
        kit::pick(&mut h, "Language:", language);
        kit::click(&mut h, RADIO, "Character Color", 0);
        kit::click(&mut h, RADIO, "Stroke", 0);
        kit::pick(&mut h, "Color:", "[Black]");
        kit::click(&mut h, RADIO, "OpenType Features", 0);
        kit::click(&mut h, CHECK, "Fractions", 0);
        kit::click(&mut h, RADIO, "Underline Options", 0);
        kit::click(&mut h, CHECK, "Underline On", 0);
        kit::type_and_commit(&mut h, "Weight:", 0, "2");
        kit::key(&mut h, Key::Enter, Modifiers::NONE);
        let format = &kit::draft(&h).fields[CHANGE_FORMAT];
        assert_eq!(
            format["chars"],
            json!({"hScale": 1.2, "language": language, "stroke": "[Black]", "otfFeatures": ["frac"], "underline": true, "underlineWeight": 2.0}),
            "{format}"
        );
        assert!(kit::draft(&h).fields.get(FIND_FORMAT).is_none(), "the Find Format is untouched");
    }
}
