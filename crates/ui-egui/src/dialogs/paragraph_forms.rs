//! The framework form of Paragraph Borders and Shading, Span Columns, Keep Options, Drop Caps and Nested Styles, GREP Styles
//! and Bullets and Numbering for the selected paragraphs: the Paragraph Style Options sections over the paragraphs'
//! attributes (`current`). Edits are stored as `p.<attribute>` in the draft, as the native sections store them, and OK
//! applies them as local formatting with the existing controller.

use super::Dialog;
use super::framework::Form;
use super::text_forms::{
    Attrs, Spec, Swatches, captioned, check, choice, field, measure, measure_edit, message, number, number_edit, select_at, swatch_choice, tabs, text,
};
use crate::DesignApp;
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::display::Text;
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::text::{TextField, link_toggle};
use designcraft_geom::Unit;
use egui::Ui;
use serde_json::{Value, json};

/// Paragraph Borders and Shading, Span Columns, Keep Options, Nested Styles, GREP Styles and Bullets and Numbering.
pub(crate) const PARAGRAPH_LOCAL: Form = Form::new(paragraph_local);

/// The Border and Shading tabs.
const SHADING_TABS: [(&str, &str); 2] = [("border", "Border"), ("shading", "Shading")];

fn paragraph_local(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let doc = app.session.active()?.doc.clone();
    let pv = d.fields.get("current").cloned().unwrap_or_default();
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    let section = d.s("section");
    let a = Attrs::new("p.", &pv);
    let char_styles: Vec<String> = doc.styles.character.iter().map(|c| c.name.clone()).collect();
    // What the Bullets and Numbering section offers besides the attributes: the families, the fonts and the paragraph's font.
    let (list_styles, families, fonts, scope) =
        if section == "bullets" { super::ListUi::new_parts(app) } else { (Vec::new(), Vec::new(), crate::panels::fonts(app), 0) };
    let (family, style) = (d.s("family"), d.s("style"));
    let bullets = Bullets { char_styles: &list_styles, families: &families, fonts: &fonts, scope, family: &family, style: &style };
    Stack::new().id_salt("paragraph_local").fill_width().show(ui, |ui, _| match section.as_str() {
        "span" => span_columns(lang, ui, d, &a, &doc),
        "keep" => keep_options(lang, ui, d, &a),
        "nested" => nested_styles(lang, ui, d, &a, &char_styles),
        "grep" => grep_styles(lang, ui, d, &a, &char_styles),
        "bullets" => bullets_and_numbering(lang, ui, d, &a, &doc, &bullets),
        other => {
            let tab_list: Vec<(&str, String)> = SHADING_TABS.iter().map(|(id, caption)| (*id, crate::i18n::tr(lang, caption).to_string())).collect();
            let current = if other == "shading" { "shading" } else { "border" };
            if let Some(next) = tabs(ui, "paragraph_local", &crate::i18n::tr(lang, "Paragraph Borders and Shading"), current, &tab_list) {
                d.fields.insert("section".into(), json!(next));
            }
            border_shading(lang, ui, d, &a, &doc, current == "border");
        }
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Keep Options

/// Lines kept with the next paragraph, the paragraph's lines kept together (all of them, or its first and last lines), and
/// where it starts.
pub(super) fn keep_options(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let starts = captioned(
        lang,
        &[
            ("anywhere", "Anywhere"),
            ("nextColumn", "In Next Column"),
            ("nextFrame", "In Next Frame"),
            ("nextPage", "On Next Page"),
            ("nextOddPage", "On Next Odd Page"),
            ("nextEvenPage", "On Next Even Page"),
        ],
    );
    Stack::new().id_salt("keep_options").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Keep Options")).id_salt("keep_options_fields").show(ui, |ui, _| {
            field(ui, &tr("Keep with Next:"), "keepWithNext", |ui| {
                number(ui, d, a, "keepWithNext", &tr("Keep with Next:"), &Spec::whole(0.0, 5.0).unit(tr("lines")), true);
            });
        });
        check(ui, d, a, "keepLinesTogether", &tr("Keep Lines Together"), true);
        let together = a.get(d, "keepLinesTogether").as_bool().unwrap_or(false);
        let all = a.get(d, "keepAllLines").as_bool().unwrap_or(true);
        let shown = RadioGroup::new(tr("Keep Lines Together"), Bind::given(all)).id_salt("keep_all_lines").enabled(together).show(ui, |ui, _| {
            Stack::new().id_salt("keep_all_lines_choices").show(ui, |ui, _| {
                Radio::new(true).label(tr("All Lines in Paragraph")).show(ui);
                Radio::new(false).label(tr("At Start/End of Paragraph")).show(ui);
            });
        });
        let mut next = all;
        for event in shown.output.events {
            event.apply(&mut next);
        }
        if next != all {
            a.set(d, "keepAllLines", json!(next));
        }
        FieldGroup::new(tr("Keep Lines Together")).id_salt("keep_ends_fields").show(ui, |ui, _| {
            for (caption, key) in [("Start:", "keepFirst"), ("End:", "keepLast")] {
                field(ui, &tr(caption), key, |ui| number(ui, d, a, key, &tr(caption), &Spec::whole(1.0, 50.0).unit(tr("lines")), together && !all));
            }
            field(ui, &tr("Start Paragraph:"), "startParagraph", |ui| choice(ui, d, a, "startParagraph", &tr("Start Paragraph:"), &starts, true));
        });
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Span Columns

/// One column, spanning columns (0 = all), or split into sub-columns with inside and outside gutters; the least space
/// before and after a spanning or split paragraph.
pub(super) fn span_columns(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, doc: &designcraft_doc::Document) {
    use designcraft_doc::SpanColumns;
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let current: SpanColumns = serde_json::from_value(a.get(d, "spanColumns")).unwrap_or_default();
    let same = |x: &SpanColumns, y: &SpanColumns| std::mem::discriminant(x) == std::mem::discriminant(y);
    let (units, v_units) = (doc.settings.horizontal_units, doc.settings.vertical_units);
    // "Span Columns" names both the section and this option, which some languages word differently.
    let kinds = [("Single Column", SpanColumns::Single), ("Span Columns", SpanColumns::Span(0)), ("Split Column", SpanColumns::Split(2))];
    let kind_captions: Vec<String> = kinds.iter().map(|(caption, _)| crate::i18n::tr_context(lang, caption, "paragraphLayout").to_owned()).collect();
    let kind_at = kinds.iter().position(|(_, kind)| same(kind, &current));
    let mut next: Option<SpanColumns> = None;
    Stack::new().id_salt("span_columns").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Paragraph Layout")).id_salt("span_columns_fields").show(ui, |ui, _| {
            field(ui, &tr("Paragraph Layout:"), "spanColumns", |ui| {
                if let Some(picked) = select_at(ui, "span_kind", &tr("Paragraph Layout:"), &kind_captions, kind_at, None, true)
                    && !same(&kinds[picked].1, &current)
                {
                    next = Some(kinds[picked].1);
                }
            });
            match current {
                SpanColumns::Single => {}
                SpanColumns::Span(n) => field(ui, &tr("Columns:"), "span_n", |ui| {
                    let mut captions = vec![tr("All")];
                    captions.extend((2..=20u32).map(|c| c.to_string()));
                    let at = if n == 0 { Some(0) } else { (2..=20u32).position(|c| c == n).map(|p| p + 1) };
                    if let Some(picked) = select_at(ui, "span_n", &tr("Columns:"), &captions, at, Some(n.to_string()), true) {
                        next = Some(SpanColumns::Span(if picked == 0 { 0 } else { picked as u32 + 1 }));
                    }
                }),
                SpanColumns::Split(n) => field(ui, &tr("Columns:"), "split_n", |ui| {
                    if let Some(Some(v)) = number_edit(ui, "split_n", &tr("Columns:"), Some(f64::from(n)), &Spec::whole(2.0, 20.0), true) {
                        next = Some(SpanColumns::Split(v as u32));
                    }
                }),
            }
            let spaces = match current {
                SpanColumns::Single => None,
                SpanColumns::Span(_) => Some(("Space Before Span:", "Space After Span:")),
                SpanColumns::Split(_) => Some(("Space Before Split:", "Space After Split:")),
            };
            if let Some((before, after)) = spaces {
                for (caption, key) in [(before, "spanSpaceBefore"), (after, "spanSpaceAfter")] {
                    field(ui, &tr(caption), key, |ui| measure(ui, d, a, key, &tr(caption), v_units, true));
                }
            }
            if matches!(current, SpanColumns::Split(_)) {
                for (caption, key) in [("Inside Gutter:", "splitInsideGutter"), ("Outside Gutter:", "splitOutsideGutter")] {
                    field(ui, &tr(caption), key, |ui| measure(ui, d, a, key, &tr(caption), units, true));
                }
            }
        });
    });
    if let Some(kind) = next {
        a.set(d, "spanColumns", serde_json::to_value(kind).unwrap_or_default());
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Drop Caps and Nested Styles

/// The drop cap, nested styles and nested line styles.
pub(super) fn nested_styles(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, char_styles: &[String]) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let style = a.get(d, "dropCapStyle").as_str().filter(|s| !s.is_empty()).unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_owned();
    let captions: Vec<String> = char_styles.iter().map(|c| crate::i18n::style_name(lang, c).to_string()).collect();
    let at = char_styles.iter().position(|c| *c == style);
    Stack::new().id_salt("nested_styles").fill_width().show(ui, |ui, _| {
        GroupBox::new(tr("Drop Caps"), None).id_salt("drop_caps_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Drop Caps")).id_salt("drop_caps_fields").show(ui, |ui, _| {
                for (caption, key, most) in [
                    ("Lines:", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES),
                    ("Characters:", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS),
                ] {
                    field(ui, &tr(caption), key, |ui| number(ui, d, a, key, &tr(caption), &Spec::whole(0.0, f64::from(most)), true));
                }
                field(ui, &tr("Character Style:"), "dropCapStyle", |ui| {
                    let placeholder = Some(crate::i18n::style_name(lang, &style).to_string());
                    if let Some(picked) = select_at(ui, "drop_cap_style", &tr("Character Style:"), &captions, at, placeholder, true) {
                        a.set(d, "dropCapStyle", json!(char_styles[picked]));
                    }
                });
            });
            check(ui, d, a, "dropCapAlignLeft", &tr("Align Left Edge"), true);
            check(ui, d, a, "dropCapScaleDescenders", &tr("Scale for Descenders"), true);
        });
        style_rows(lang, ui, d, a, char_styles, Rows::Nested);
        style_rows(lang, ui, d, a, char_styles, Rows::Lines);
    });
}

/// Which list of character-style rows a section shows.
#[derive(Clone, Copy)]
enum Rows {
    Nested,
    Lines,
}

/// The nested styles (`p.nestedStyles`) or nested line styles (`p.nestedLineStyles`) list, with its add button.
fn style_rows(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, char_styles: &[String], kind: Rows) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let (key, title, add, salt) = match kind {
        Rows::Nested => ("nestedStyles", "Nested Styles", "New Nested Style", "nested_style"),
        Rows::Lines => ("nestedLineStyles", "Nested Line Styles", "New Line Style", "nested_line_style"),
    };
    let mut list: Vec<Value> = a.get(d, key).as_array().cloned().unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    GroupBox::new(tr(title), None).id_salt((salt, "box")).show(ui, |ui, _| {
        for (i, row) in list.iter_mut().enumerate() {
            Row::new().id_salt((salt, "row", i)).show(ui, |ui, _| {
                let (row_changed, delete) = match kind {
                    Rows::Nested => nested_style_row(lang, ui, row, i, char_styles),
                    Rows::Lines => line_style_row(lang, ui, row, i, char_styles),
                };
                changed |= row_changed;
                if delete {
                    remove = Some(i);
                }
            });
        }
        if Button::new(tr(add)).id_salt((salt, "new")).show(ui).clicked() {
            let style = char_styles.get(1).cloned().unwrap_or_default();
            list.push(match kind {
                Rows::Nested => json!({"style": style, "through": true, "count": 1, "until": {"kind": "words"}}),
                Rows::Lines => json!({"style": style, "lines": 1}),
            });
            changed = true;
        }
    });
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if changed {
        a.set(d, key, Value::Array(list));
    }
}

/// A character style picker over `value["style"]`: whether it changed.
fn style_picker(lang: &str, ui: &mut Ui, salt: (&str, usize), label: &str, value: &mut Value, char_styles: &[String]) -> bool {
    let style = value["style"].as_str().unwrap_or("").to_owned();
    let captions: Vec<String> = char_styles.iter().map(|c| crate::i18n::style_name(lang, c).to_string()).collect();
    let at = char_styles.iter().position(|c| *c == style);
    let placeholder = Some(crate::i18n::style_name(lang, &style).to_string());
    let salt = format!("{}{}", salt.0, salt.1);
    match select_at(ui, &salt, label, &captions, at, placeholder, true) {
        Some(picked) => {
            value["style"] = json!(char_styles[picked]);
            true
        }
        None => false,
    }
}

/// One row of the nested styles list: the character style, through or up to, a count and what it counts. Whether the row
/// changed and whether its delete button was pressed.
fn nested_style_row(lang: &str, ui: &mut Ui, ns: &mut Value, i: usize, char_styles: &[String]) -> (bool, bool) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let mut changed = style_picker(lang, ui, ("ns_style", i), &tr("Character Style:"), ns, char_styles);
    let through = ns["through"].as_bool().unwrap_or(true);
    let throughs = [tr("through"), tr("up to")];
    if let Some(picked) = select_at(ui, &format!("ns_through{i}"), &tr("through"), &throughs, Some(usize::from(!through)), None, true) {
        ns["through"] = json!(picked == 0);
        changed = true;
    }
    let first = crate::i18n::category_first(lang);
    let counted = |ui: &mut Ui, ns: &mut Value, changed: &mut bool| {
        let shown = ns["count"].as_u64().unwrap_or(1) as f64;
        if let Some(Some(v)) = number_edit(ui, &format!("ns_count{i}"), &tr("Count"), Some(shown), &Spec::whole(1.0, 999.0), true) {
            ns["count"] = json!(v as u32);
            *changed = true;
        }
    };
    if !first {
        counted(ui, ns, &mut changed);
    }
    let kind = ns["until"]["kind"].as_str().unwrap_or("words").to_owned();
    let kinds = ["sentences", "words", "characters", "letters", "digits", "tab", "forcedLineBreak", "emSpace", "enSpace", "dropcap", "chars"];
    let captions: Vec<String> = kinds.iter().map(|k| tr(k)).collect();
    let at = kinds.iter().position(|k| *k == kind);
    if let Some(picked) = select_at(ui, &format!("ns_until{i}"), &tr("Until"), &captions, at, Some(kind.clone()), true) {
        ns["until"] = if kinds[picked] == "chars" { json!({"kind": "chars", "chars": ":"}) } else { json!({"kind": kinds[picked]}) };
        changed = true;
    }
    if first {
        counted(ui, ns, &mut changed);
    }
    if kind == "chars" {
        let chars = ns["until"]["chars"].as_str().unwrap_or("").to_owned();
        let out = TextField::new(tr("Characters"), Bind::given(chars.clone())).id_salt(("ns_chars", i)).limit(None::<usize>).show(ui);
        if let Some(next) = out.events.landed().filter(|next| **next != chars).cloned() {
            ns["until"]["chars"] = json!(next);
            changed = true;
        }
    }
    let delete = Button::new(tr("Delete")).id_salt(("ns_delete", i)).show(ui).clicked();
    (changed, delete)
}

/// One row of the nested line styles list: the character style and how many lines. Whether the row changed and whether its
/// delete button was pressed.
fn line_style_row(lang: &str, ui: &mut Ui, line: &mut Value, i: usize, char_styles: &[String]) -> (bool, bool) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let mut changed = style_picker(lang, ui, ("nls_style", i), &tr("Character Style:"), line, char_styles);
    let first = crate::i18n::category_first(lang);
    Text::new(if first { format!("{}:", tr("lines")) } else { tr("for") }).show(ui);
    let lines = line["lines"].as_u64().unwrap_or(1);
    if let Some(Some(v)) = number_edit(ui, &format!("nls_lines{i}"), &tr("Lines"), Some(lines as f64), &Spec::whole(1.0, 999.0), true) {
        line["lines"] = json!(v as u32);
        changed = true;
    }
    if !first {
        Text::new(tr(if lines == 1 { "line" } else { "lines" })).show(ui);
    }
    let delete = Button::new(tr("Delete")).id_salt(("nls_delete", i)).show(ui).clicked();
    (changed, delete)
}

// ---------------------------------------------------------------------------------------------------------------------
// GREP Styles

/// A character style applied to text matching each pattern.
pub(super) fn grep_styles(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, char_styles: &[String]) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let mut list: Vec<Value> = a.get(d, "grepStyles").as_array().cloned().unwrap_or_default();
    let mut changed = false;
    let mut remove = None;
    Stack::new().id_salt("grep_styles").fill_width().show(ui, |ui, _| {
        GroupBox::new(tr("GREP Styles"), None).id_salt("grep_styles_box").show(ui, |ui, _| {
            for (i, grep) in list.iter_mut().enumerate() {
                Row::new().id_salt(("grep_row", i)).show(ui, |ui, _| {
                    Text::new(tr("Apply Style:")).show(ui);
                    changed |= style_picker(lang, ui, ("gs_style", i), &tr("Apply Style:"), grep, char_styles);
                    Text::new(tr("To Text:")).show(ui);
                    let pattern = grep["pattern"].as_str().unwrap_or("").to_owned();
                    let out = TextField::new(tr("To Text:"), Bind::given(pattern.clone())).id_salt(("gs_pattern", i)).limit(None::<usize>).show(ui);
                    if let Some(next) = out.events.landed().filter(|next| **next != pattern).cloned() {
                        grep["pattern"] = json!(next);
                        changed = true;
                    }
                    if Button::new(tr("Delete")).id_salt(("gs_delete", i)).show(ui).clicked() {
                        remove = Some(i);
                    }
                });
                if let Some(error) = super::grep_error(grep["pattern"].as_str().unwrap_or("")) {
                    message(ui, &error, true);
                }
            }
            if Button::new(tr("New GREP Style")).id_salt("gs_new").show(ui).clicked() {
                list.push(json!({"style": char_styles.get(1).cloned().unwrap_or_default(), "pattern": "\\d+"}));
                changed = true;
            }
        });
    });
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if changed {
        a.set(d, "grepStyles", Value::Array(list));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Bullets and Numbering

/// What the Bullets and Numbering section offers besides the paragraph's attributes: the document's character styles, the
/// font families for the bullet, the fonts, and the paragraph's font (the bullet's when it has none of its own).
pub(super) struct Bullets<'a> {
    pub(super) char_styles: &'a [String],
    pub(super) families: &'a [String],
    pub(super) fonts: &'a designcraft_fonts::ScopedFonts<'static>,
    pub(super) scope: u32,
    pub(super) family: &'a str,
    pub(super) style: &'a str,
}

/// The most glyphs the bullet's glyph picker shows.
const GLYPHS_SHOWN: usize = 96;

/// The list type and level; the bullet, its font and the text after it, or the list, number style, number text and where
/// numbering starts and restarts; the label's character style; then where the label goes (alignment, indents and the tab
/// position of the text after it).
pub(super) fn bullets_and_numbering(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, doc: &designcraft_doc::Document, bullets: &Bullets) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let units = doc.settings.horizontal_units;
    let kind = a.get(d, "listType").as_str().unwrap_or("none").to_owned();
    let level = a.get(d, "listLevel").as_u64().unwrap_or(1).clamp(1, designcraft_doc::LIST_LEVELS as u64) as u32;
    let types = captioned(lang, &[("none", "None"), ("bullets", "Bullets"), ("numbers", "Numbers")]);
    let numbers: Vec<(Value, String)> = super::NUMBER_STYLES.iter().map(|(value, caption)| (json!(value), tr(caption))).collect();
    let aligns = captioned(lang, &[("left", "Left"), ("center", "Center"), ("right", "Right")]);
    let list_names: Vec<(Value, String)> =
        std::iter::once((json!(""), tr("[Default]"))).chain(doc.settings.lists.iter().map(|l| (json!(l.name), l.name.clone()))).collect();
    let char_styles: Vec<(Value, String)> = std::iter::once(designcraft_doc::NO_CHAR_STYLE)
        .chain(bullets.char_styles.iter().map(String::as_str).filter(|n| *n != designcraft_doc::NO_CHAR_STYLE))
        .map(|name| (json!(name), crate::i18n::style_name(lang, name).to_string()))
        .collect();
    Stack::new().id_salt("bullets_numbering").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr("Bullets and Numbering")).id_salt("bullets_fields").show(ui, |ui, _| {
            field(ui, &tr("List Type:"), "listType", |ui| choice(ui, d, a, "listType", &tr("List Type:"), &types, true));
            if kind != "none" {
                field(ui, &tr("Level:"), "listLevel", |ui| {
                    number(ui, d, a, "listLevel", &tr("Level:"), &Spec::whole(1.0, designcraft_doc::LIST_LEVELS as f64), true);
                });
            }
            match kind.as_str() {
                "bullets" => {
                    field(ui, &tr("Bullet Character:"), "bulletChar", |ui| text(ui, d, a, "bulletChar", &tr("Bullet Character:"), true));
                    bullet_font(lang, ui, d, a, bullets);
                    field(ui, &tr("Text After:"), "listSeparator", |ui| list_text(ui, d, a, "listSeparator", &tr("Text After:")));
                    field(ui, &tr("Character Style:"), "bulletCharStyle", |ui| {
                        choice(ui, d, a, "bulletCharStyle", &tr("Character Style:"), &char_styles, true);
                    });
                }
                "numbers" => {
                    field(ui, &tr("List:"), "listName", |ui| choice(ui, d, a, "listName", &tr("List:"), &list_names, true));
                    field(ui, &tr("Format:"), "numberStyle", |ui| choice(ui, d, a, "numberStyle", &tr("Format:"), &numbers, true));
                    field(ui, &tr("Number:"), "numberExpression", |ui| list_text(ui, d, a, "numberExpression", &tr("Number:")));
                    field(ui, &tr("Character Style:"), "numberCharStyle", |ui| {
                        choice(ui, d, a, "numberCharStyle", &tr("Character Style:"), &char_styles, true);
                    });
                    numbering_mode(lang, ui, d, a);
                    Row::new().id_salt("numbering_restart").show(ui, |ui, _| {
                        check(ui, d, a, "restartNumbers", &tr("Restart Numbers at This Level After:"), level > 1);
                        let after = a.get(d, "restartAfterLevel").as_u64().unwrap_or(0) as u32;
                        let captions: Vec<String> = (0..level).map(|n| if n == 0 { tr("Any Previous Level") } else { n.to_string() }).collect();
                        if let Some(picked) = select_at(
                            ui,
                            "restart_after",
                            &tr("Restart Numbers at This Level After:"),
                            &captions,
                            Some(after as usize),
                            None,
                            level > 1,
                        ) {
                            a.set(d, "restartAfterLevel", json!(picked as u32));
                        }
                    });
                }
                _ => {}
            }
            if kind != "none" {
                let align = if kind == "bullets" { "bulletAlign" } else { "numberAlign" };
                field(ui, &tr("Alignment:"), align, |ui| choice(ui, d, a, align, &tr("Alignment:"), &aligns, true));
                for (caption, key) in [("Left Indent:", "leftIndent"), ("First Line Indent:", "firstLineIndent")] {
                    field(ui, &tr(caption), key, |ui| measure(ui, d, a, key, &tr(caption), units, true));
                }
                tab_position(lang, ui, d, a, units);
            }
        });
        if kind == "bullets" {
            bullet_chooser(lang, ui, d, a, bullets);
        }
    });
}

/// A list label text (Text After, Number), shown with its tab as `^t`; labels expand metacharacters, so the typed text is
/// stored as it is.
fn list_text(ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, label: &str) {
    let shown = a.get(d, key).as_str().unwrap_or("").replace('\t', "^t");
    let out = TextField::new(label.to_owned(), Bind::given(shown.clone())).id_salt(("list_text", key)).limit(None::<usize>).show(ui);
    if let Some(next) = out.events.landed().filter(|next| **next != shown).cloned() {
        a.set(d, key, json!(next));
    }
}

/// Mode: continue from the previous number, or start at a number.
fn numbering_mode(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let start = a.get(d, "startAt").as_u64();
    let captions = [tr("Continue from Previous Number"), tr("Start At")];
    field(ui, &tr("Mode:"), "startAtMode", |ui| {
        if let Some(picked) = select_at(ui, "start_mode", &tr("Mode:"), &captions, Some(usize::from(start.is_some())), None, true)
            && (picked == 0) != start.is_none()
        {
            a.set(d, "startAt", if picked == 0 { Value::Null } else { json!(1) });
        }
    });
    if let Some(n) = start {
        field(ui, &tr("Start At"), "startAt", |ui| {
            if let Some(Some(v)) = number_edit(ui, "start_at", &tr("Start At"), Some(n as f64), &Spec::whole(0.0, 1_000_000.0), true) {
                a.set(d, "startAt", json!(v as u32));
            }
        });
    }
}

/// Tab Position: none, or a stop for the text after the label.
fn tab_position(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, units: Unit) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let tab = a.get(d, "listTab").as_f64();
    Row::new().id_salt("list_tab_position").show(ui, |ui, _| {
        let out = Checkbox::new(Bind::given(tab.is_some())).label(tr("Tab Position:")).id_salt("list_tab_on").show(ui);
        if let Some(on) = out.events.committed().copied().filter(|on| *on != tab.is_some()) {
            let left = a.get(d, "leftIndent").as_f64().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(18.0);
            a.set(d, "listTab", if on { json!(left) } else { Value::Null });
        }
        if tab.is_some()
            && let Some(points) = measure_edit(ui, "list_tab", &tr("Tab Position:"), tab, units, (0.0, f64::MAX), true)
        {
            a.set(d, "listTab", json!(points));
        }
    });
}

/// The bullet's font family and style ("" = the paragraph's).
fn bullet_font(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, bullets: &Bullets) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let font = a.get(d, "bulletFont").as_str().unwrap_or("").to_owned();
    let inherit = tr("(Paragraph Font)");
    let mut captions = vec![inherit];
    captions.extend(bullets.families.iter().cloned());
    let at = if font.is_empty() { Some(0) } else { bullets.families.iter().position(|f| *f == font).map(|p| p + 1) };
    field(ui, &tr("Font Family:"), "bulletFont", |ui| {
        if let Some(picked) = select_at(ui, "bullet_font", &tr("Font Family:"), &captions, at, Some(font.clone()), true) {
            let family = if picked == 0 { String::new() } else { bullets.families[picked - 1].clone() };
            let style = if picked == 0 { String::new() } else { bullets.fonts.styles(&family).into_iter().next().unwrap_or_default() };
            a.set(d, "bulletFont", json!(family));
            a.set(d, "bulletFontStyle", json!(style));
        }
    });
    let style = a.get(d, "bulletFontStyle").as_str().unwrap_or("").to_owned();
    let styles = bullets.fonts.styles(&font);
    let at = styles.iter().position(|s| *s == style);
    field(ui, &tr("Font Style:"), "bulletFontStyle", |ui| {
        if let Some(picked) = select_at(ui, "bullet_font_style", &tr("Font Style:"), &styles, at, Some(style.clone()), !font.is_empty()) {
            a.set(d, "bulletFontStyle", json!(styles[picked]));
        }
    });
}

/// The common bullets, and the font's glyphs to pick from: the glyphs matching the search (a character or `U+code`), the
/// first few of them.
fn bullet_chooser(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, bullets: &Bullets) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    GroupBox::new(tr("Common Bullets"), None).id_salt("bullet_chooser_box").show(ui, |ui, _| {
        Row::new().id_salt("bullet_common").show(ui, |ui, _| {
            for bullet in ["\u{2022}", "\u{2013}", "\u{2014}", "\u{00BB}", "*"] {
                if Button::new(bullet).id_salt(("bullet_common", bullet)).show(ui).clicked() {
                    a.set(d, "bulletChar", json!(bullet));
                }
            }
            let open = d.b("bulletGlyphs");
            let out = Checkbox::new(Bind::given(open)).label(tr("Glyphs…")).id_salt("bullet_glyphs_open").show(ui);
            if let Some(on) = out.events.committed().copied().filter(|on| *on != open) {
                d.fields.insert("bulletGlyphs".into(), json!(on));
            }
        });
        if !d.b("bulletGlyphs") {
            return;
        }
        let font = a.get(d, "bulletFont").as_str().unwrap_or("").to_owned();
        let (family, style) = if font.is_empty() {
            (bullets.family.to_owned(), bullets.style.to_owned())
        } else {
            let style = a.get(d, "bulletFontStyle").as_str().unwrap_or("").to_owned();
            let style = if style.is_empty() { bullets.fonts.styles(&font).into_iter().next().unwrap_or_else(|| "Regular".into()) } else { style };
            (font, style)
        };
        let query = d.s("bulletGlyphQuery");
        let out =
            TextField::new(tr("Search: character or U+code"), Bind::given(query.clone())).id_salt("bullet_glyph_query").limit(None::<usize>).show(ui);
        if let Some(next) = out.events.landed().filter(|next| **next != query).cloned() {
            d.fields.insert("bulletGlyphQuery".into(), json!(next));
        }
        for glyph in glyphs(bullets.fonts, &family, &style, d.s("bulletGlyphQuery").trim()) {
            if Button::new(glyph.to_string()).id_salt(("bullet_glyph", glyph as u32)).show(ui).clicked() {
                a.set(d, "bulletChar", json!(glyph.to_string()));
            }
        }
    });
}

/// The first [`GLYPHS_SHOWN`] characters of the face that match `query`, in code point order: all of them for an empty
/// query, else those it contains or names as `U+code`.
fn glyphs(fonts: &designcraft_fonts::ScopedFonts<'static>, family: &str, style: &str, query: &str) -> Vec<char> {
    let code = query.strip_prefix("U+").or_else(|| query.strip_prefix("u+")).and_then(|hex| u32::from_str_radix(hex, 16).ok());
    let mut chars: Vec<char> = fonts.face(family, style).chars().into_iter().map(|(c, _)| c).filter(|c| !c.is_control()).collect();
    chars.sort();
    chars.dedup();
    if !query.is_empty() {
        chars.retain(|c| Some(*c as u32) == code || query.contains(*c) || format!("{:04X}", *c as u32).contains(&query.to_ascii_uppercase()));
    }
    chars.truncate(GLYPHS_SHOWN);
    chars
}

// ---------------------------------------------------------------------------------------------------------------------
// Paragraph Border and Paragraph Shading

/// Built-in stroke types of the paragraph border's Type menu (serialized `StrokeType`s).
const BORDER_TYPES: [&str; 11] =
    ["solid", "dashed", "dotted", "thickThin", "thinThick", "thinThin", "thickThick", "thinThickThin", "thickThinThick", "wavy", "hashed"];

/// The values the four sides of an attribute can hold.
#[derive(Clone, Copy)]
enum Side {
    /// A stroke weight in points.
    Weight,
    /// A distance in the document's unit.
    Offset(Unit),
}

/// Four per-side values in the order the attribute holds them: top, left, bottom, right. Top and Bottom, the link
/// ("Make all settings the same"), then Left and Right. An edited side stores the whole array.
fn sides(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, legend: &str, kind: Side) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let mut values: [f64; 4] = serde_json::from_value(a.get(d, key)).unwrap_or_default();
    let linked = super::linked(d, key, values.iter().all(|v| (v - values[0]).abs() < 1e-9));
    let mut changed = false;
    let edit = |ui: &mut Ui, values: &mut [f64; 4], i: usize, caption: &str| {
        let label = caption.to_owned();
        let salt = format!("{}{i}", a.field(key));
        let value = match kind {
            Side::Weight => number_edit(ui, &salt, &label, Some(values[i]), &Spec::real(0.0, 1000.0).unit("pt"), true).flatten(),
            Side::Offset(unit) => measure_edit(ui, &salt, &label, Some(values[i]), unit, (f64::MIN, f64::MAX), true),
        };
        let Some(value) = value else { return false };
        // Linked, a side typed in sets all four.
        if linked {
            *values = [value; 4];
        } else {
            values[i] = value;
        }
        true
    };
    GroupBox::new(legend.to_owned(), None).id_salt(("paragraph_sides_box", key.to_owned())).show(ui, |ui, _| {
        Row::new().id_salt(("paragraph_sides", key.to_owned())).fill_width().show(ui, |ui, _| {
            for (column, indices) in [[0, 2], [1, 3]].into_iter().enumerate() {
                if column == 1 {
                    let out =
                        link_toggle(tr("Make all settings the same"), Bind::given(linked)).id_salt(("paragraph_sides_link", key.to_owned())).show(ui);
                    if let Some(on) = out.events.committed().copied().filter(|on| *on != linked) {
                        d.fields.insert(format!("_linked.{key}"), json!(on));
                        // Linked again, the four take Top's value.
                        if on {
                            values = [values[0]; 4];
                            changed = true;
                        }
                    }
                }
                FieldGroup::new(legend.to_owned()).id_salt(("paragraph_sides_fields", key.to_owned(), column)).show(ui, |ui, _| {
                    for i in indices {
                        let caption = tr(["Top:", "Left:", "Bottom:", "Right:"][i]);
                        field(ui, &caption, &format!("{key}{i}"), |ui| changed |= edit(ui, &mut values, i, &caption));
                    }
                });
            }
        });
    });
    if changed {
        a.set(d, key, json!(values));
    }
}

/// Corner size and shape of a border or shading (`key` holds top left, top right, bottom right, bottom left): Top Left and
/// Top Right over Bottom Left and Bottom Right, each a size and a shape, with the link between the columns. An edited corner
/// stores all four.
fn corners(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, units: Unit) {
    use designcraft_geom::corners::{Corner, CornerShape};
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let mut values: [Corner; 4] = serde_json::from_value(a.get(d, key)).unwrap_or_default();
    let linked = super::linked(d, key, values.iter().all(|c| *c == values[0]));
    let mut changed = None;
    let shapes: Vec<String> = CornerShape::ALL.iter().map(|shape| tr(shape.label())).collect();
    GroupBox::new(tr("Corner Size and Shape"), None).id_salt(("paragraph_corners_box", key.to_owned())).show(ui, |ui, _| {
        Row::new().id_salt(("paragraph_corners", key.to_owned())).fill_width().show(ui, |ui, _| {
            for (column, corner_rows) in
                [[(0, "Top Left Corner"), (3, "Bottom Left Corner")], [(1, "Top Right Corner"), (2, "Bottom Right Corner")]].into_iter().enumerate()
            {
                if column == 1 {
                    let out = link_toggle(tr("Make all settings the same"), Bind::given(linked))
                        .id_salt(("paragraph_corners_link", key.to_owned()))
                        .show(ui);
                    if let Some(on) = out.events.committed().copied().filter(|on| *on != linked) {
                        d.fields.insert(format!("_linked.{key}"), json!(on));
                        // Linked again, the four take Top Left's size and shape.
                        if on {
                            values = [values[0]; 4];
                            changed = Some(0);
                        }
                    }
                }
                FieldGroup::new(tr("Corner Size and Shape")).id_salt(("paragraph_corner_fields", key.to_owned(), column)).show(ui, |ui, _| {
                    for (i, caption) in corner_rows {
                        let caption = tr(caption);
                        field(ui, &caption, &format!("{key}{i}"), |ui| {
                            Row::new().id_salt(("paragraph_corner", key.to_owned(), i)).show(ui, |ui, _| {
                                let salt = format!("{}{i}", a.field(key));
                                if let Some(size) = measure_edit(ui, &salt, &caption, Some(values[i].size), units, (0.0, 10_000.0), true) {
                                    values[i].size = size;
                                    changed = Some(i);
                                }
                                let at = CornerShape::ALL.iter().position(|shape| *shape == values[i].shape);
                                let label = format!("{} {}", caption, tr("Corner Shape"));
                                if let Some(picked) = select_at(ui, &format!("{salt}shape"), &label, &shapes, at, None, true) {
                                    values[i].shape = CornerShape::ALL[picked];
                                    changed = Some(i);
                                }
                            });
                        });
                    }
                });
            }
        });
    });
    if let Some(i) = changed {
        // Linked, the corner changed sets all four.
        if linked {
            values = [values[i]; 4];
        }
        a.set(d, key, serde_json::to_value(values).unwrap_or_default());
    }
}

/// The colour and tint rows of a border, its gap or shading (`keys`: colour, tint, overprint; `labels` likewise); Overprint
/// follows the tint.
fn color_tint(ui: &mut Ui, d: &mut Dialog, a: &Attrs, swatches: &Swatches, keys: (&str, &str, &str), labels: (&str, &str, &str), enabled: bool) {
    let (color_key, tint_key, overprint_key) = keys;
    field(ui, labels.0, color_key, |ui| {
        if let Some(picked) = swatch_choice(ui, d, a, color_key, labels.0, swatches, enabled) {
            a.set(d, color_key, picked);
        }
    });
    field(ui, labels.1, tint_key, |ui| {
        let tint = a.get(d, tint_key).as_f64().map(|t| (t * 1000.0).round() / 1000.0);
        let spec = Spec::whole(0.0, 100.0).scaled(100.0).unit("%");
        if let Some(edit) = number_edit(ui, &a.field(tint_key), labels.1, tint, &spec, enabled) {
            a.set(d, tint_key, json!(edit.map_or(1.0, |v| v.clamp(0.0, 1.0))));
        }
    });
    check(ui, d, a, overprint_key, labels.2, enabled);
}

/// The border's stroke type: a built-in type or one of the document's stroke styles.
fn border_type(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, doc: &designcraft_doc::Document, label: &str) {
    use designcraft_doc::StrokeType;
    let current: StrokeType = serde_json::from_value(a.get(d, "borderType")).unwrap_or_default();
    let mut options: Vec<(Value, String)> = Vec::new();
    let mut at = None;
    for kind in BORDER_TYPES {
        let value = if kind == "dashed" { json!({"kind": kind, "pattern": []}) } else { json!({"kind": kind}) };
        let Ok(ty) = serde_json::from_value::<StrokeType>(value.clone()) else { continue };
        if std::mem::discriminant(&ty) == std::mem::discriminant(&current) {
            at = Some(options.len());
        }
        options.push((value, crate::i18n::tr(lang, ty.label()).to_string()));
    }
    for style in &doc.stroke_styles {
        if matches!(&current, StrokeType::Style { name } if *name == style.name) {
            at = Some(options.len());
        }
        options.push((json!({"kind": "style", "name": style.name}), style.name.clone()));
    }
    let captions: Vec<String> = options.iter().map(|(_, caption)| caption.clone()).collect();
    field(ui, label, "borderType", |ui| {
        if let Some(picked) = select_at(ui, "border_type", label, &captions, at, None, true)
            && at != Some(picked)
        {
            a.set(d, "borderType", options[picked].0.clone());
        }
    });
}

/// On or off, the stroke (border) or colour (shading), corners, offsets, the edges and width the box follows, and the options
/// for split and consecutive paragraphs.
pub(super) fn border_shading(lang: &str, ui: &mut Ui, d: &mut Dialog, a: &Attrs, doc: &designcraft_doc::Document, border: bool) {
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let key = |name: &str| format!("{}{name}", if border { "border" } else { "shading" });
    let on_key = key("On");
    let on = a.get(d, &on_key).as_bool().unwrap_or(false);
    let units = doc.settings.horizontal_units;
    let swatches = Swatches::new(Some(doc), None);
    let solid = a.get(d, "borderType").get("kind").and_then(Value::as_str).is_none_or(|k| k == "solid");
    let caps = captioned(lang, &[("butt", "Butt"), ("round", "Round"), ("projecting", "Projecting")]);
    let joins = captioned(lang, &[("miter", "Miter"), ("round", "Round"), ("bevel", "Bevel")]);
    let tops = captioned(lang, &[("ascent", "Ascent"), ("baseline", "Baseline"), ("emBox", "Em Box Top"), ("leading", "Leading")]);
    let bottoms = captioned(lang, &[("descent", "Descent"), ("baseline", "Baseline"), ("emBox", "Em Box Bottom")]);
    let widths = captioned(lang, &[("column", "Column"), ("text", "Text")]);
    check(ui, d, a, &on_key, &tr(if border { "Border" } else { "Shading" }), true);
    Stack::new().id_salt(("paragraph_border_shading", border)).fill_width().enabled(on).show(ui, |ui, _| {
        if border {
            sides(lang, ui, d, a, &key("Weights"), &tr("Stroke"), Side::Weight);
        }
        GroupBox::new(tr(if border { "Stroke" } else { "Color" }), None).id_salt(("paragraph_color_box", border)).show(ui, |ui, _| {
            FieldGroup::new(tr("Color")).id_salt(("paragraph_color_fields", border)).show(ui, |ui, _| {
                if border {
                    border_type(lang, ui, d, a, doc, &tr("Type:"));
                }
                color_tint(
                    ui,
                    d,
                    a,
                    &swatches,
                    (&key("Color"), &key("Tint"), &key("Overprint")),
                    (&tr("Color:"), &tr("Tint:"), &tr("Overprint")),
                    true,
                );
                if border {
                    // The gap colour shows between the dashes, dots or stripes.
                    color_tint(
                        ui,
                        d,
                        a,
                        &swatches,
                        ("borderGapColor", "borderGapTint", "borderGapOverprint"),
                        (&tr("Gap Color:"), &tr("Gap Tint:"), &tr("Overprint Gap")),
                        !solid,
                    );
                    field(ui, &tr("Cap:"), "borderCap", |ui| choice(ui, d, a, "borderCap", &tr("Cap:"), &caps, true));
                    field(ui, &tr("Join:"), "borderJoin", |ui| choice(ui, d, a, "borderJoin", &tr("Join:"), &joins, true));
                }
            });
        });
        corners(lang, ui, d, a, &key("Corners"), units);
        // The offsets, and beside them the edges and width the box follows, as InDesign's.
        Row::new().id_salt(("paragraph_offsets_row", border)).fill_width().show(ui, |ui, _| {
            sides(lang, ui, d, a, &key("Offsets"), &tr("Offsets"), Side::Offset(units));
            FieldGroup::new(tr("Offsets")).id_salt(("paragraph_edges", border)).show(ui, |ui, _| {
                field(ui, &tr("Top Edge:"), &key("Top"), |ui| choice(ui, d, a, &key("Top"), &tr("Top Edge:"), &tops, true));
                field(ui, &tr("Bottom Edge:"), &key("Bottom"), |ui| choice(ui, d, a, &key("Bottom"), &tr("Bottom Edge:"), &bottoms, true));
                field(ui, &tr("Width:"), &key("Width"), |ui| choice(ui, d, a, &key("Width"), &tr("Width:"), &widths, true));
            });
        });
        if border {
            check(ui, d, a, "borderDisplayIfSplits", &tr("Display Border if Paragraph Splits Across Frames/Columns"), true);
            check(ui, d, a, "borderMerge", &tr("Merge Consecutive Borders and Shading"), true);
        } else {
            check(ui, d, a, "shadingClip", &tr("Clip to Frame"), true);
            check(ui, d, a, "shadingNonprinting", &tr("Do not Print or Export"), true);
        }
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::Queryable};

    /// The two-paragraph document with the dialog the menu command `command` opens, and a character style to pick.
    fn open(command: &str) -> (Harness<'static, DesignApp>, u64) {
        let (mut app, story) = kit::two_paragraphs();
        app.run("style.character.create", json!({"name": "Emph"})).unwrap();
        app.run(command, json!({})).unwrap();
        (kit::harness(app), story)
    }

    /// The attributes the last `type.para` carried.
    fn last_attrs(h: &Harness<'_, DesignApp>) -> Value {
        kit::journal(h, "type.para").last().unwrap().1["attrs"].clone()
    }

    /// Guards Keep Options: the lines fields follow their switches as the native section did, every edit stays in the draft
    /// until OK, and OK sends exactly the attributes edited, once.
    #[test]
    fn keep_options_form_enables_fields_with_their_switches_and_ok_sends_the_edits() {
        let (mut h, story) = open("app.keepOptionsDialog");
        assert_eq!(kit::draft(&h).s("section"), "keep");
        let undo = kit::undo_steps(&h);
        assert!(kit::disabled(&h, &[Role::SpinButton], "Start:", 0), "Keep Lines Together is off");
        kit::type_and_commit(&mut h, "Keep with Next:", 0, "2");
        kit::click(&mut h, &[Role::CheckBox], "Keep Lines Together", 0);
        // A paragraph keeps its first and last lines by default (`keepAllLines` is false until chosen).
        assert!(!kit::disabled(&h, &[Role::SpinButton], "Start:", 0), "At Start/End of Paragraph is the default choice");
        kit::click(&mut h, &[Role::RadioButton], "All Lines in Paragraph", 0);
        assert!(kit::disabled(&h, &[Role::SpinButton], "Start:", 0), "All Lines in Paragraph is chosen");
        kit::click(&mut h, &[Role::RadioButton], "At Start/End of Paragraph", 0);
        assert!(!kit::disabled(&h, &[Role::SpinButton], "Start:", 0));
        kit::type_and_commit(&mut h, "Start:", 0, "2");
        assert_eq!(kit::undo_steps(&h), undo);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(last_attrs(&h), json!({"keepWithNext": 2, "keepLinesTogether": true, "keepAllLines": false, "keepFirst": 2}));
        assert_eq!(kit::undo_steps(&h), undo + 1, "one undo step");
        assert_eq!(kit::para_attr(&mut h, story, 0, "keepWithNext"), json!(2));
    }

    /// Guards Span Columns: the layout choice shows its own fields, the choice and a typed measure are stored as the
    /// attributes the command takes, and Cancel sends nothing.
    #[test]
    fn span_columns_form_stores_the_layout_and_its_spacing() {
        let (mut h, _) = open("app.spanColumnsDialog");
        let undo = kit::undo_steps(&h);
        assert!(h.query_by_label("Space Before Span:").is_none(), "a single column has no spans");
        kit::pick(&mut h, "Paragraph Layout:", "Span Columns");
        assert_eq!(kit::draft(&h).fields["p.spanColumns"], json!({"kind": "span", "columns": 0}));
        kit::type_and_commit(&mut h, "Space Before Span:", 0, "6 pt");
        assert_eq!(kit::draft(&h).fields["p.spanSpaceBefore"].as_f64(), Some(6.0));
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert_eq!(last_attrs(&h), json!({"spanColumns": {"kind": "span", "columns": 0}, "spanSpaceBefore": 6.0}));
        assert_eq!(kit::undo_steps(&h), undo + 1);

        let (mut h, _) = open("app.spanColumnsDialog");
        let before = h.state().session.doc().unwrap().doc.clone();
        kit::pick(&mut h, "Paragraph Layout:", "Split Column");
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.doc().unwrap().doc, before);
    }

    /// Guards Nested Styles: a new row takes the first character style, its count is edited in place, and OK stores the
    /// whole list; Delete takes a row out of it.
    #[test]
    fn nested_styles_form_adds_edits_and_removes_rows() {
        let (mut h, story) = open("app.nestedStylesDialog");
        let undo = kit::undo_steps(&h);
        kit::click(&mut h, &[Role::Button], "New Nested Style", 0);
        assert_eq!(kit::draft(&h).fields["p.nestedStyles"].as_array().map(Vec::len), Some(1));
        kit::type_and_commit(&mut h, "Count", 0, "3");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(last_attrs(&h), json!({"nestedStyles": [{"style": "Emph", "through": true, "count": 3, "until": {"kind": "words"}}]}));
        assert_eq!(kit::undo_steps(&h), undo + 1);
        assert_eq!(kit::para_attr(&mut h, story, 0, "nestedStyles")[0]["count"], json!(3));

        let (mut h, _) = open("app.nestedStylesDialog");
        kit::click(&mut h, &[Role::Button], "New Nested Style", 0);
        kit::click(&mut h, &[Role::Button], "Delete", 0);
        assert_eq!(kit::draft(&h).fields["p.nestedStyles"], json!([]));
    }

    /// Guards GREP Styles: the pattern typed in a row is stored with its character style, and a row can be removed.
    #[test]
    fn grep_styles_form_stores_the_pattern_with_its_style() {
        let (mut h, _) = open("app.grepStylesDialog");
        let undo = kit::undo_steps(&h);
        kit::click(&mut h, &[Role::Button], "New GREP Style", 0);
        kit::type_into(&mut h, "To Text:", 0, "\\d{3}");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(last_attrs(&h), json!({"grepStyles": [{"style": "Emph", "pattern": "\\d{3}"}]}));
        assert_eq!(kit::undo_steps(&h), undo + 1);

        let (mut h, _) = open("app.grepStylesDialog");
        kit::click(&mut h, &[Role::Button], "New GREP Style", 0);
        kit::click(&mut h, &[Role::Button], "Delete", 0);
        assert_eq!(kit::draft(&h).fields["p.grepStyles"], json!([]));
    }

    /// Guards Bullets and Numbering: choosing Bullets shows the bullet's fields, a common bullet and the tab position are
    /// stored, and OK sends them as one paragraph edit.
    #[test]
    fn bullets_form_stores_the_list_type_bullet_and_tab_position() {
        let (mut h, _) = open("app.bulletsNumberingDialog");
        let undo = kit::undo_steps(&h);
        assert!(h.query_by_label("Bullet Character:").is_none(), "no list, no bullet");
        kit::pick(&mut h, "List Type:", "Bullets");
        assert_eq!(kit::draft(&h).s("p.listType"), "bullets");
        kit::click_first(&mut h, "\u{2013}");
        assert_eq!(kit::draft(&h).s("p.bulletChar"), "\u{2013}");
        kit::click(&mut h, &[Role::CheckBox], "Tab Position:", 0);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(last_attrs(&h), json!({"listType": "bullets", "bulletChar": "\u{2013}", "listTab": 18.0}));
        assert_eq!(kit::undo_steps(&h), undo + 1);
    }

    /// Guards Paragraph Borders and Shading: the fields wait for the switch, a stroke weight typed with the sides linked is
    /// all four, the tabs switch the section, and OK sends the edits of both.
    #[test]
    fn border_and_shading_form_follow_their_switches_and_link_the_sides() {
        let (mut h, _) = open("app.paragraphBordersDialog");
        let undo = kit::undo_steps(&h);
        assert_eq!(kit::draft(&h).s("section"), "border");
        assert!(kit::disabled(&h, &[Role::SpinButton], "Top:", 0), "Border is off");
        kit::click(&mut h, &[Role::CheckBox], "Border", 0);
        assert!(!kit::disabled(&h, &[Role::SpinButton], "Top:", 0));
        kit::type_and_commit(&mut h, "Top:", 0, "3");
        assert_eq!(kit::draft(&h).fields["p.borderWeights"], json!([3.0, 3.0, 3.0, 3.0]), "linked, one side is all four");
        kit::click(&mut h, &[Role::RadioButton, Role::Button, Role::Tab], "Shading", 0);
        assert_eq!(kit::draft(&h).s("section"), "shading");
        kit::click(&mut h, &[Role::CheckBox], "Shading", 0);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(last_attrs(&h), json!({"borderOn": true, "borderWeights": [3.0, 3.0, 3.0, 3.0], "shadingOn": true}));
        assert_eq!(kit::undo_steps(&h), undo + 1);
    }
}
