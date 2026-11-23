//! The paragraph sections of Paragraph Style Options on the craft framework: the pages a paragraph style, and the
//! dialogs over the selected paragraphs, show beside the character sections. Each page draws the body only and edits
//! `{prefix}<attr>` draft fields: the value shown is the draft's edit, else the resolved attribute in `current`.

use super::style_kit::{Counts, Extra, Num, captioned, count, field, measure, number, percent, pick, push, text, toggle};
use crate::DesignApp;
use crate::dialogs::{Dialog, JUSTIFY_ROWS, RULES, SINGLE_WORD_CHOICES};
use craft_theme::variant::Tone;
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::choice::{Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Props as _, Scroll, Show as _, ShowContainer as _};
use craft_ui_widgets::display::Text;
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::runtime::env::UnitKind;
use craft_ui_widgets::style::TypeRole;
use craft_ui_widgets::value_drag::Slider;
use designcraft_geom::Unit;
use egui::Ui;
use serde_json::{Value, json};

/// The attribute `key` of the paragraph as the dialog shows it: the draft's edit, else the resolved value in `pv`.
pub(crate) fn val(d: &Dialog, prefix: &str, pv: &Value, key: &str) -> Value {
    d.fields.get(&format!("{prefix}{key}")).cloned().unwrap_or_else(|| pv.get(key).cloned().unwrap_or(Value::Null))
}

/// Edits attribute `key`.
pub(crate) fn set(d: &mut Dialog, prefix: &str, key: &str, value: Value) {
    d.fields.insert(format!("{prefix}{key}"), value);
}

fn tr(app: &DesignApp, s: &str) -> String {
    crate::i18n::tr(&app.ui.language, s).to_string()
}

/// Where a page's attributes live: the draft prefix and the resolved values.
#[derive(Clone, Copy)]
struct At<'a> {
    prefix: &'a str,
    pv: &'a Value,
}

impl At<'_> {
    fn val(&self, d: &Dialog, key: &str) -> Value {
        val(d, self.prefix, self.pv, key)
    }

    fn set(&self, d: &mut Dialog, key: &str, value: Value) {
        set(d, self.prefix, key, value);
    }

    fn salt(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }
}

/// A menu over attribute `key`, a string chosen from `options` (value, caption).
fn attr_pick(ui: &mut Ui, d: &mut Dialog, at: &At, key: &str, label: &str, options: &[(String, String)], enabled: bool) {
    let current = at.val(d, key).as_str().unwrap_or("").to_owned();
    if let Some(next) = pick(ui, &at.salt(key), label, &current, options, enabled) {
        at.set(d, key, json!(next));
    }
}

/// A check box over attribute `key`.
fn attr_toggle(ui: &mut Ui, d: &mut Dialog, at: &At, key: &str, label: &str, enabled: bool) {
    let current = at.val(d, key).as_bool();
    if let Some(on) = toggle(ui, &at.salt(key), label, current, enabled) {
        at.set(d, key, json!(on));
    }
}

/// A measure attribute, in points, shown in `unit`.
fn attr_measure(ui: &mut Ui, d: &mut Dialog, at: &At, key: &str, label: &str, unit: Unit, enabled: bool) {
    let current = at.val(d, key).as_f64();
    if let Some(points) = measure(ui, d, &at.salt(key), label, current, unit, enabled) {
        at.set(d, key, json!(points));
    }
}

/// A count attribute (lines, letters, columns) kept in `range`.
fn attr_count(ui: &mut Ui, d: &mut Dialog, at: &At, key: &str, label: &str, range: (u32, u32), counts: &Counts) {
    let current = at.val(d, key).as_f64();
    if let Some(n) = count(ui, &at.salt(key), label, current, range, counts, true) {
        at.set(d, key, json!(n));
    }
}

/// Indents and Spacing: alignment, the five distances, Align to Grid and Ignore Optical Margin.
pub(crate) fn indents(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, units: Unit) {
    let at = At { prefix: "p.", pv };
    use designcraft_doc::GridAlign;
    let lang = app.ui.language.as_str();
    let aligns: Vec<(String, String)> = designcraft_doc::Align::ALL
        .iter()
        .filter_map(|al| Some((serde_json::to_value(al).ok()?.as_str()?.to_owned(), crate::i18n::tr(lang, al.label()).to_string())))
        .collect();
    let grid: Vec<(String, String)> = [(GridAlign::None, "None"), (GridAlign::AllLines, "All Lines"), (GridAlign::FirstLineOnly, "First Line Only")]
        .into_iter()
        .filter_map(|(v, label)| Some((serde_json::to_value(v).ok()?.as_str()?.to_owned(), crate::i18n::tr(lang, label).to_string())))
        .collect();
    let ignore = tr(app, "Ignore Optical Margin");
    Stack::new().id_salt("para_indents").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "Indents and Spacing")).id_salt("para_indents_fields").show(ui, |ui, _| {
            let label = tr(app, "Alignment:");
            field(ui, &label, "align", |ui| attr_pick(ui, d, &at, "align", &label, &aligns, true));
            for (label, key) in [
                ("Left Indent:", "leftIndent"),
                ("First Line Indent:", "firstLineIndent"),
                ("Right Indent:", "rightIndent"),
                ("Space Before:", "spaceBefore"),
                ("Space After:", "spaceAfter"),
            ] {
                let label = tr(app, label);
                field(ui, &label, key, |ui| attr_measure(ui, d, &at, key, &label, units, true));
            }
            // The CJK Grid Settings section edits the same `p.gridAlign` field.
            let label = tr(app, "Align to Grid:");
            field(ui, &label, "gridAlign", |ui| attr_pick(ui, d, &at, "gridAlign", &label, &grid, true));
        });
        attr_toggle(ui, d, &at, "ignoreOpticalMargin", &ignore, true);
    });
}

/// Keep Options: lines kept with the next paragraph, the paragraph's lines kept together (all of them, or its first
/// and last), and where it starts.
pub(crate) fn keep(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value) {
    let at = At { prefix: "p.", pv };
    let lang = app.ui.language.as_str();
    let (with_next, together_label, lines) = (tr(app, "Keep with Next:"), tr(app, "Keep Lines Together"), tr(app, "lines"));
    let (all_label, ends_label) = (tr(app, "All Lines in Paragraph"), tr(app, "At Start/End of Paragraph"));
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
    let start_label = tr(app, "Start Paragraph:");
    Stack::new().id_salt("para_keep").fill_width().show(ui, |ui, _| {
        field(ui, &with_next, "keepWithNext", |ui| attr_count(ui, d, &at, "keepWithNext", &with_next, (0, 5), &Counts::Word(lines.clone())));
        attr_toggle(ui, d, &at, "keepLinesTogether", &together_label, true);
        let together = at.val(d, "keepLinesTogether").as_bool().unwrap_or(false);
        let all = at.val(d, "keepAllLines").as_bool().unwrap_or(true);
        Stack::new().id_salt("para_keep_lines").fill_width().enabled(together).indent(1).show(ui, |ui, _| {
            let shown = RadioGroup::new(together_label.clone(), Bind::given(all)).id_salt("para_keep_all").show(ui, |ui, _| {
                Stack::new().show(ui, |ui, _| {
                    Radio::new(true).label(all_label.clone()).show(ui);
                    Radio::new(false).label(ends_label.clone()).show(ui);
                });
            });
            if let Some(next) = shown.output.events.committed().copied().filter(|next| *next != all) {
                set(d, "p.", "keepAllLines", json!(next));
            }
            Stack::new().id_salt("para_keep_ends").fill_width().enabled(!all).show(ui, |ui, _| {
                for (label, key) in [("Start:", "keepFirst"), ("End:", "keepLast")] {
                    let label = tr(app, label);
                    field(ui, &label, key, |ui| attr_count(ui, d, &at, key, &label, (1, 50), &Counts::Word(lines.clone())));
                }
            });
        });
        field(ui, &start_label, "startParagraph", |ui| attr_pick(ui, d, &at, "startParagraph", &start_label, &starts, true));
    });
}

/// Span Columns: one column, spanning columns (0 = all), or split into sub-columns with inside and outside gutters;
/// the least space before and after a spanning or split paragraph.
pub(crate) fn span(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document) {
    let at = At { prefix: "p.", pv };
    use designcraft_doc::SpanColumns;
    let lang = app.ui.language.as_str();
    let cur: SpanColumns = serde_json::from_value(at.val(d, "spanColumns")).unwrap_or_default();
    let (units, v_units) = (doc.settings.horizontal_units, doc.settings.vertical_units);
    let kind_of = |c: &SpanColumns| match c {
        SpanColumns::Single => "single",
        SpanColumns::Span(_) => "span",
        SpanColumns::Split(_) => "split",
    };
    // "Span Columns" names both the section and this option, which some languages word differently.
    let kinds: Vec<(String, String)> = [("single", "Single Column"), ("span", "Span Columns"), ("split", "Split Column")]
        .into_iter()
        .map(|(value, caption)| (value.to_owned(), crate::i18n::tr_context(lang, caption, "paragraphLayout").to_string()))
        .collect();
    let spans: Vec<(String, String)> =
        std::iter::once(("0".to_owned(), tr(app, "All"))).chain((2..=20u32).map(|c| (c.to_string(), c.to_string()))).collect();
    let (layout, columns) = (tr(app, "Paragraph Layout:"), tr(app, "Columns:"));
    let mut next = None;
    Stack::new().id_salt("para_span").fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "Span Columns")).id_salt("para_span_fields").show(ui, |ui, _| {
            field(ui, &layout, "kind", |ui| {
                if let Some(picked) = pick(ui, "p.spanKind", &layout, kind_of(&cur), &kinds, true) {
                    next = Some(match picked.as_str() {
                        "span" => SpanColumns::Span(0),
                        "split" => SpanColumns::Split(2),
                        _ => SpanColumns::Single,
                    });
                }
            });
            match cur {
                SpanColumns::Single => {}
                SpanColumns::Span(n) => field(ui, &columns, "span_columns", |ui| {
                    if let Some(picked) = pick(ui, "p.spanColumns", &columns, &n.to_string(), &spans, true) {
                        next = picked.parse().ok().map(SpanColumns::Span);
                    }
                }),
                SpanColumns::Split(n) => field(ui, &columns, "split_columns", |ui| {
                    if let Some(c) = count(ui, "p.splitColumns", &columns, Some(f64::from(n)), (2, 20), &Counts::Plain, true) {
                        next = Some(SpanColumns::Split(c));
                    }
                }),
            }
            let spaces = match cur {
                SpanColumns::Single => None,
                SpanColumns::Span(_) => Some(("Space Before Span:", "Space After Span:")),
                SpanColumns::Split(_) => Some(("Space Before Split:", "Space After Split:")),
            };
            if let Some((before, after)) = spaces {
                for (label, key) in [(before, "spanSpaceBefore"), (after, "spanSpaceAfter")] {
                    let label = tr(app, label);
                    field(ui, &label, key, |ui| attr_measure(ui, d, &at, key, &label, v_units, true));
                }
            }
            if matches!(cur, SpanColumns::Split(_)) {
                for (label, key) in [("Inside Gutter:", "splitInsideGutter"), ("Outside Gutter:", "splitOutsideGutter")] {
                    let label = tr(app, label);
                    field(ui, &label, key, |ui| attr_measure(ui, d, &at, key, &label, units, true));
                }
            }
        });
    });
    if let Some(v) = next {
        set(d, "p.", "spanColumns", serde_json::to_value(v).unwrap_or_default());
    }
}

/// A character style name's menu entries: `names` captioned, as (name, caption).
fn style_entries(app: &DesignApp, names: &[String]) -> Vec<(String, String)> {
    names.iter().map(|n| (n.clone(), crate::i18n::style_name(&app.ui.language, n).to_string())).collect()
}

/// The × of a list row: whether it was clicked.
fn delete_button(ui: &mut Ui, app: &DesignApp, salt: &str) -> bool {
    use craft_ui_widgets::buttons::IconButton;
    use craft_ui_widgets::primitives::IconSpec;
    IconButton::new(IconSpec::svg(std::borrow::Cow::Borrowed("trash").into()))
        .id_salt(("para_delete", salt.to_owned()))
        .label(tr(app, "Delete"))
        .show(ui)
        .clicked()
}

/// Drop Caps and Nested Styles: the drop cap, nested styles and nested line styles (`p.` fields).
pub(crate) fn nested(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    Stack::new().id_salt("para_nested").fill_width().show(ui, |ui, _| {
        drop_cap(app, ui, d, pv, cnames);
        nested_styles(app, ui, d, pv, cnames);
        nested_line_styles(app, ui, d, pv, cnames);
    });
}

/// The drop cap's lines, characters, character style and alignment options.
fn drop_cap(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    let at = At { prefix: "p.", pv };
    let styles = style_entries(app, cnames);
    GroupBox::new(tr(app, "Drop Caps"), None).id_salt("para_drop_cap_box").show(ui, |ui, _| {
        FieldGroup::new(tr(app, "Drop Caps")).id_salt("para_drop_cap").show(ui, |ui, _| {
            for (label, key, max) in [
                ("Lines:", "dropCapLines", designcraft_compose::MAX_DROP_CAP_LINES),
                ("Characters:", "dropCapChars", designcraft_compose::MAX_DROP_CAP_CHARS),
            ] {
                let label = tr(app, label);
                field(ui, &label, key, |ui| attr_count(ui, d, &at, key, &label, (0, max as u32), &Counts::Plain));
            }
            let label = tr(app, "Character Style:");
            field(ui, &label, "dropCapStyle", |ui| {
                let current = at.val(d, "dropCapStyle").as_str().filter(|s| !s.is_empty()).unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_owned();
                if let Some(next) = pick(ui, "p.dropCapStyle", &label, &current, &styles, true) {
                    set(d, "p.", "dropCapStyle", json!(next));
                }
            });
        });
        for (label, key) in [("Align Left Edge", "dropCapAlignLeft"), ("Scale for Descenders", "dropCapScaleDescenders")] {
            attr_toggle(ui, d, &at, key, &tr(app, label), true);
        }
    });
}

/// The nested styles list (`p.nestedStyles`), with its add button.
fn nested_styles(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    let at = At { prefix: "p.", pv };
    let lang = app.ui.language.as_str();
    let styles = style_entries(app, cnames);
    let throughs = [("through".to_owned(), tr(app, "through")), ("upTo".to_owned(), tr(app, "up to"))];
    let kinds: Vec<(String, String)> = ["sentences", "words", "characters", "letters", "digits", "tab", "forcedLineBreak", "emSpace", "enSpace", "dropcap", "chars"]
        .into_iter()
        .map(|k| (k.to_owned(), crate::i18n::tr(lang, k).to_string()))
        .collect();
    let mut list: Vec<Value> = at.val(d, "nestedStyles").as_array().cloned().unwrap_or_default();
    let (mut changed, mut remove) = (false, None);
    GroupBox::new(tr(app, "Nested Styles"), None).id_salt("para_nested_box").show(ui, |ui, _| {
        for (i, ns) in list.iter_mut().enumerate() {
            Row::new().id_salt(("para_nested_row", i)).fill_width().show(ui, |ui, _| {
                let style = ns["style"].as_str().unwrap_or("[None]").to_owned();
                if let Some(next) = pick(ui, &format!("ns_style{i}"), &tr(app, "Nested Style"), &style, &styles, true) {
                    ns["style"] = json!(next);
                    changed = true;
                }
                let through = if ns["through"].as_bool().unwrap_or(true) { "through" } else { "upTo" };
                if let Some(next) = pick(ui, &format!("ns_through{i}"), &tr(app, "Through or up to"), through, &throughs, true) {
                    ns["through"] = json!(next == "through");
                    changed = true;
                }
                let kind = ns["until"]["kind"].as_str().unwrap_or("words").to_owned();
                // A count before its kind, or after it where the language names the category first.
                let mut count_field = |ui: &mut Ui, ns: &mut Value, changed: &mut bool| {
                    let current = ns["count"].as_u64().map(|n| n as f64);
                    if let Some(n) = count(ui, &format!("ns_count{i}"), &tr(app, "Count"), current, (1, 999), &Counts::Plain, true) {
                        ns["count"] = json!(n);
                        *changed = true;
                    }
                };
                if !crate::i18n::category_first(lang) {
                    count_field(ui, ns, &mut changed);
                }
                if let Some(next) = pick(ui, &format!("ns_until{i}"), &tr(app, "Until"), &kind, &kinds, true) {
                    ns["until"] = if next == "chars" { json!({"kind": "chars", "chars": ":"}) } else { json!({"kind": next}) };
                    changed = true;
                }
                if crate::i18n::category_first(lang) {
                    count_field(ui, ns, &mut changed);
                }
                if kind == "chars" {
                    let current = ns["until"]["chars"].as_str().unwrap_or("").to_owned();
                    if let Some(next) = super::style_kit::committed_text(ui, &format!("ns_chars{i}"), &tr(app, "Characters"), &current, true) {
                        ns["until"]["chars"] = json!(next);
                        changed = true;
                    }
                }
                if delete_button(ui, app, &format!("ns{i}")) {
                    remove = Some(i);
                }
            });
        }
        if push(ui, "new_nested_style", &tr(app, "New Nested Style"), true) {
            list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "through": true, "count": 1, "until": {"kind": "words"}}));
            changed = true;
        }
    });
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if changed {
        set(d, "p.", "nestedStyles", Value::Array(list));
    }
}

/// The nested line styles list (`p.nestedLineStyles`), with its add button.
fn nested_line_styles(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    let at = At { prefix: "p.", pv };
    let lang = app.ui.language.as_str();
    let styles = style_entries(app, cnames);
    let mut list: Vec<Value> = at.val(d, "nestedLineStyles").as_array().cloned().unwrap_or_default();
    let (mut changed, mut remove) = (false, None);
    GroupBox::new(tr(app, "Nested Line Styles"), None).id_salt("para_nested_lines_box").show(ui, |ui, _| {
        for (i, entry) in list.iter_mut().enumerate() {
            Row::new().id_salt(("para_nested_line_row", i)).fill_width().show(ui, |ui, _| {
                let style = entry["style"].as_str().unwrap_or("").to_owned();
                if let Some(next) = pick(ui, &format!("nls_style{i}"), &tr(app, "Line Style"), &style, &styles, true) {
                    entry["style"] = json!(next);
                    changed = true;
                }
                let lines = entry["lines"].as_u64().unwrap_or(1) as u32;
                let word = if crate::i18n::category_first(lang) { tr(app, "lines") } else { tr(app, if lines == 1 { "line" } else { "lines" }) };
                if let Some(n) = count(ui, &format!("nls_lines{i}"), &tr(app, "Lines"), Some(f64::from(lines)), (1, 999), &Counts::Word(word), true) {
                    entry["lines"] = json!(n);
                    changed = true;
                }
                if delete_button(ui, app, &format!("nls{i}")) {
                    remove = Some(i);
                }
            });
        }
        if push(ui, "new_line_style", &tr(app, "New Line Style"), true) {
            list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "lines": 1}));
            changed = true;
        }
    });
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if changed {
        set(d, "p.", "nestedLineStyles", Value::Array(list));
    }
}

/// GREP Styles: a character style applied to text matching each pattern (`p.` fields).
pub(crate) fn grep(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, cnames: &[String]) {
    let at = At { prefix: "p.", pv };
    use craft_ui_widgets::text::TextField;
    let styles = style_entries(app, cnames);
    let mut list: Vec<Value> = at.val(d, "grepStyles").as_array().cloned().unwrap_or_default();
    let (mut changed, mut remove) = (false, None);
    GroupBox::new(tr(app, "GREP Styles"), None).id_salt("para_grep_box").show(ui, |ui, _| {
        for (i, g) in list.iter_mut().enumerate() {
            Row::new().id_salt(("para_grep_row", i)).fill_width().show(ui, |ui, _| {
                let label = tr(app, "Apply Style:");
                field(ui, &label, &format!("gs_style{i}"), |ui| {
                    let style = g["style"].as_str().unwrap_or("").to_owned();
                    if let Some(next) = pick(ui, &format!("gs_style{i}"), &label, &style, &styles, true) {
                        g["style"] = json!(next);
                        changed = true;
                    }
                });
                let label = tr(app, "To Text:");
                field(ui, &label, &format!("gs_pattern{i}"), |ui| {
                    let pattern = g["pattern"].as_str().unwrap_or("").to_owned();
                    let out = TextField::new(label.clone(), Bind::given(pattern.clone())).id_salt(("para_grep_pattern", i)).limit(None::<usize>).show(ui);
                    if let Some(next) = out.events.landed().filter(|next| **next != pattern).cloned() {
                        g["pattern"] = json!(next);
                        changed = true;
                    }
                });
                if delete_button(ui, app, &format!("gs{i}")) {
                    remove = Some(i);
                }
            });
            if let Some(e) = crate::dialogs::grep_error(g["pattern"].as_str().unwrap_or("")) {
                Text::new(e).role(TypeRole::Caption).tone(Tone::Bad).show(ui);
            }
        }
        if push(ui, "new_grep_style", &tr(app, "New GREP Style"), true) {
            list.push(json!({"style": cnames.get(1).cloned().unwrap_or_default(), "pattern": "\\d+"}));
            changed = true;
        }
    });
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if changed {
        set(d, "p.", "grepStyles", Value::Array(list));
    }
}

/// Hyphenation: the switches, the letter counts and zone, the spacing/hyphens balance, the capitalized-word,
/// last-word and across-column switches. Edits are `{prefix}<attr>`; a value that differs across the paragraphs
/// shows mixed.
pub(crate) fn hyphenation(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, prefix: &str, current: &Value, units: Unit) {
    let cur = |d: &Dialog, k: &str| val(d, prefix, current, k);
    let flag = |ui: &mut Ui, d: &mut Dialog, key: &str, label: &str| {
        let label = tr(app, label);
        let v = cur(d, key).as_bool();
        if let Some(on) = toggle(ui, &format!("hy{prefix}{key}"), &label, v, true) {
            set(d, prefix, key, json!(on));
        }
    };
    Stack::new().id_salt(("hyphenation", prefix.to_owned())).fill_width().show(ui, |ui, _| {
        flag(ui, d, "hyphenate", "Hyphenate");
        FieldGroup::new(tr(app, "Hyphenation Settings")).id_salt(("hyphenation_fields", prefix.to_owned())).show(ui, |ui, _| {
            // InDesign's ranges.
            for (label, key, unit, max) in [
                ("Words with at Least:", "hyphMinWord", "letters", 25),
                ("After First:", "hyphAfterFirst", "letters", 15),
                ("Before Last:", "hyphBeforeLast", "letters", 15),
                ("Hyphen Limit:", "hyphLimit", "hyphens", 25),
            ] {
                let label = tr(app, label);
                field(ui, &label, key, |ui| {
                    let least = if key == "hyphLimit" { 0 } else { 1 };
                    let v = cur(d, key).as_f64();
                    if let Some(n) = count(ui, &format!("hy{prefix}{key}"), &label, v, (least, max), &Counts::Word(tr(app, unit)), true) {
                        set(d, prefix, key, json!(n));
                    }
                });
            }
            let label = tr(app, "Hyphenation Zone:");
            field(ui, &label, "hyphZone", |ui| {
                let v = cur(d, "hyphZone").as_f64();
                if let Some(n) = measure(ui, d, &format!("hy{prefix}zone"), &label, v, units, true) {
                    set(d, prefix, "hyphZone", json!(n.clamp(0.0, 8640.0)));
                }
            });
        });
        // Better Spacing ↔ Fewer Hyphens, in InDesign's eleven steps; mixed across the selection, a click or drag sets
        // every paragraph to that step.
        let (better, fewer) = (tr(app, "Better Spacing"), tr(app, "Fewer Hyphens"));
        let weight = cur(d, "hyphWeight").as_f64();
        Row::new().id_salt(("hyphenation_weight", prefix.to_owned())).fill_width().show(ui, |ui, _| {
            Text::new(better.clone()).show(ui);
            let shown = weight.map_or(craft_behavior::Mixed::Mixed, |w| craft_behavior::Mixed::Same((w * 10.0).round().clamp(0.0, 10.0)));
            let out = Slider::new(Bind::given(shown), 0.0..=10.0)
                .step(1.0)
                .ticks(10)
                .label(format!("{better} / {fewer}"))
                .id_salt(("hyphenation_slider", prefix.to_owned()))
                .show(ui);
            if let Some(step) = out.events.committed().copied() {
                set(d, prefix, "hyphWeight", json!(step / 10.0));
            }
            Text::new(fewer.clone()).show(ui);
        });
        flag(ui, d, "hyphCapitalized", "Hyphenate Capitalized Words");
        flag(ui, d, "hyphLastWord", "Hyphenate Last Word");
        flag(ui, d, "hyphAcrossColumn", "Hyphenate Across Column");
    });
}

/// Justification: the word, letter and glyph spacing limits, Auto Leading, Single Word Justification, the composer
/// and the Arabic and Korean options. Edits are `{prefix}<attr>` ratios; what OK refuses shows below.
pub(crate) fn justification(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, prefix: &str, current: &Value) {
    let cur = |d: &Dialog, k: &str| val(d, prefix, current, k);
    let singles: Vec<(String, String)> = SINGLE_WORD_CHOICES.iter().map(|(v, label)| ((*v).to_owned(), tr(app, label))).collect();
    let single_label = tr(app, "Single Word Justification:");
    let composer_label = tr(app, "Composer:");
    let (paragraph, single_line) = (tr(app, "Paragraph Composer"), tr(app, "Single-line Composer"));
    Stack::new().id_salt(("justification", prefix.to_owned())).fill_width().show(ui, |ui, _| {
        FieldGroup::new(tr(app, "Justification")).id_salt(("justification_fields", prefix.to_owned())).show(ui, |ui, _| {
            Row::new().id_salt(("justification_heads", prefix.to_owned())).fill_width().show(ui, |ui, _| {
                for head in ["Minimum", "Desired", "Maximum"] {
                    Text::new(tr(app, head)).show(ui);
                }
            });
            for (label, base, _) in JUSTIFY_ROWS {
                let label = tr(app, &format!("{label}:"));
                field(ui, &label, base, |ui| {
                    Row::new().id_salt(("justification_row", format!("{prefix}{base}"))).fill_width().show(ui, |ui, _| {
                        for (suffix, head) in [("Min", "Minimum"), ("Desired", "Desired"), ("Max", "Maximum")] {
                            let key = format!("{base}{suffix}");
                            let name = format!("{} {}", label.trim_end_matches(':'), tr(app, head));
                            // Typed values outside the limits are refused when OK is pressed, as before.
                            if let Some(ratio) = percent(ui, &format!("psj{prefix}{key}"), &name, cur(d, &key).as_f64(), (-1000.0, 1000.0), true).flatten() {
                                set(d, prefix, &key, json!(ratio));
                            }
                        }
                    });
                });
            }
            let label = tr(app, "Auto Leading:");
            field(ui, &label, "autoLeading", |ui| {
                if let Some(ratio) = percent(ui, &format!("psj{prefix}autoLeading"), &label, cur(d, "autoLeading").as_f64(), (-1000.0, 1000.0), true).flatten() {
                    set(d, prefix, "autoLeading", json!(crate::dialogs::auto_leading_typed(ratio * 100.0)));
                }
            });
            field(ui, &single_label, "singleWordJustify", |ui| {
                // Mixed (null) shows blank until a choice is made.
                let single = cur(d, "singleWordJustify").as_str().unwrap_or("").to_owned();
                if let Some(next) = pick(ui, &format!("single{prefix}"), &single_label, &single, &singles, true) {
                    // The paragraphs' own choice is no edit, so OK adds no undo step for it.
                    let key = format!("{prefix}singleWordJustify");
                    if current["singleWordJustify"].as_str() == Some(next.as_str()) {
                        d.fields.remove(&key);
                    } else {
                        d.fields.insert(key, json!(next));
                    }
                }
            });
            field(ui, &composer_label, "composer", |ui| {
                let composer = cur(d, "composer");
                // Mixed (null) selects neither; a Japanese composer keeps its kind.
                let (single, [para, single_kind]) = crate::panels::properties::composer_choice(composer.as_str());
                let shown = if composer.is_null() { "" } else if single { "single" } else { "paragraph" };
                let group = RadioGroup::new(composer_label.clone(), Bind::given(shown.to_owned())).id_salt(("composer", prefix.to_owned())).show(ui, |ui, _| {
                    Row::new().show(ui, |ui, _| {
                        Radio::new("paragraph".to_owned()).label(paragraph.clone()).show(ui);
                        Radio::new("single".to_owned()).label(single_line.clone()).show(ui);
                    });
                });
                if let Some(next) = group.output.events.committed().cloned() {
                    set(d, prefix, "composer", json!(if next == "single" { single_kind } else { para }));
                }
            });
            for (label, key, text, default) in [
                ("Insert Kashidas:", "kashidas", "In justified Arabic text", true),
                ("Korean Line Breaks:", "koreanCharBreaks", "Between syllables (not only at spaces)", false),
            ] {
                let (label, text) = (tr(app, label), tr(app, text));
                field(ui, &label, key, |ui| {
                    let v = cur(d, key);
                    let shown = if v.is_null() { None } else { Some(v.as_bool().unwrap_or(default)) };
                    if let Some(on) = toggle(ui, &format!("{prefix}{key}"), &text, shown, true) {
                        set(d, prefix, key, json!(on));
                    }
                });
            }
        });
        if let Some(e) = d.fields.get("justifyError").and_then(Value::as_str) {
            Text::new(e.to_owned()).tone(Tone::Bad).show(ui);
        }
    });
}

/// Paragraph Rules: Rule Above or Rule Below chosen at the top. `current` holds the resolved `ruleAbove` and
/// `ruleBelow`; each edit is stored as `{prefix}{rule}.{field}`, so OK changes only the fields edited.
pub(crate) fn rules(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, prefix: &str, current: &Value) {
    use designcraft_doc::StrokeType;
    let lang = app.ui.language.as_str();
    let (h_units, v_units) = app
        .session
        .active()
        .map(|st| (st.doc.settings.horizontal_units, st.doc.settings.vertical_units))
        .unwrap_or((Unit::Points, Unit::Points));
    let which = if d.s("rule") == "ruleBelow" { "ruleBelow" } else { "ruleAbove" };
    let rule = crate::dialogs::shown_rule(d, prefix, which, current);
    let key = |field: &str| format!("{prefix}{which}.{field}");
    let doc = app.session.active().map(|st| st.doc.clone());
    let on = rule["on"].as_bool().unwrap_or(false);
    let kind: StrokeType = serde_json::from_value(rule["type"].clone()).unwrap_or_default();
    let type_name = |k: &StrokeType| match k {
        StrokeType::Style { name } => name.clone(),
        k => crate::i18n::tr(lang, k.label()).to_string(),
    };
    let mut types: Vec<StrokeType> = crate::dialogs::rule_types();
    types.extend(doc.iter().flat_map(|doc| doc.stroke_styles.iter().map(|s| StrokeType::Style { name: s.name.clone() })));
    let type_entries: Vec<(String, String)> = types.iter().filter_map(|k| Some((serde_json::to_string(k).ok()?, type_name(k)))).collect();
    let whichs: Vec<(String, String)> = RULES.iter().map(|(v, label)| ((*v).to_owned(), tr(app, label))).collect();
    let widths = [("column".to_owned(), tr(app, "Column")), ("text".to_owned(), tr(app, "Text"))];
    let gaps = !matches!(kind, StrokeType::Solid);
    let gap = rule["gapColor"].as_str().unwrap_or(designcraft_color::swatch::NONE).to_owned();
    Stack::new().id_salt(("paragraph_rules", prefix.to_owned())).fill_width().show(ui, |ui, _| {
        Row::new().id_salt(("rule_head", prefix.to_owned())).fill_width().show(ui, |ui, _| {
            let label = tr(app, "Rule");
            if let Some(next) = pick(ui, &format!("rule_which{prefix}"), &label, which, &whichs, true) {
                d.fields.insert("rule".into(), json!(next));
            }
            if let Some(next) = toggle(ui, &key("on"), &tr(app, "Rule On"), Some(on), true) {
                d.fields.insert(key("on"), json!(next));
            }
        });
        Stack::new().id_salt(("rule_body", format!("{prefix}{which}"))).fill_width().enabled(on).show(ui, |ui, _| {
            FieldGroup::new(tr(app, "Paragraph Rules")).id_salt(("rule_fields", format!("{prefix}{which}"))).show(ui, |ui, _| {
                let label = tr(app, "Weight:");
                field(ui, &label, "weight", |ui| {
                    let num = Num::decimal(0.0, 1000.0).counting(Counts::Kind(UnitKind::Points));
                    if let Some(Some(v)) = number(ui, &key("weight"), &label, rule["weight"].as_f64(), &num, true) {
                        d.fields.insert(key("weight"), json!(v.clamp(0.0, 1000.0)));
                    }
                });
                let label = tr(app, "Type:");
                field(ui, &label, "type", |ui| {
                    let current_type = serde_json::to_string(&kind).unwrap_or_default();
                    if let Some(next) = pick(ui, &format!("rule_type{which}"), &label, &current_type, &type_entries, true)
                        && let Ok(v) = serde_json::from_str::<Value>(&next)
                    {
                        d.fields.insert(key("type"), v);
                    }
                });
                let label = tr(app, "Color:");
                field(ui, &label, "color", |ui| {
                    if let Some(doc) = &doc {
                        let color = rule["color"].as_str().unwrap_or("").to_owned();
                        if let Some(w) = super::style_kit::swatch_pick(ui, &format!("rule_color{which}"), &label, doc, &color, &Extra { text: Some((designcraft_doc::TEXT_COLOR, &tr(app, "Text Color"))), blank: None }, true) {
                            d.fields.insert(key("color"), json!(w));
                        }
                    }
                });
                let label = tr(app, "Tint:");
                field(ui, &label, "tint", |ui| {
                    if let Some(Some(r)) = percent(ui, &key("tint"), &label, rule["tint"].as_f64(), (0.0, 1.0), true) {
                        d.fields.insert(key("tint"), json!(r.clamp(0.0, 1.0)));
                    }
                });
                // A solid rule has no gaps.
                let label = tr(app, "Gap Color:");
                field(ui, &label, "gapColor", |ui| {
                    if let Some(doc) = &doc
                        && let Some(w) = super::style_kit::swatch_pick(ui, &format!("rule_gap{which}"), &label, doc, &gap, &Extra::default(), gaps)
                    {
                        d.fields.insert(key("gapColor"), json!(w));
                    }
                });
                let label = tr(app, "Gap Tint:");
                field(ui, &label, "gapTint", |ui| {
                    // A full gap tint is left out of the rule's JSON: it reads 100%.
                    let tint = rule["gapTint"].as_f64().unwrap_or(1.0);
                    if let Some(Some(r)) = percent(ui, &key("gapTint"), &label, Some(tint), (0.0, 1.0), gaps && gap != designcraft_color::swatch::NONE) {
                        d.fields.insert(key("gapTint"), json!(r.clamp(0.0, 1.0)));
                    }
                });
                let label = tr(app, "Width:");
                field(ui, &label, "columnWidth", |ui| {
                    let current_width = if rule["columnWidth"].as_bool().unwrap_or(true) { "column" } else { "text" };
                    if let Some(next) = pick(ui, &format!("rule_width{which}"), &label, current_width, &widths, true) {
                        d.fields.insert(key("columnWidth"), json!(next == "column"));
                    }
                });
                for (label, field_key, unit) in [("Offset:", "offset", v_units), ("Left Indent:", "leftIndent", h_units), ("Right Indent:", "rightIndent", h_units)] {
                    let label = tr(app, label);
                    field(ui, &label, field_key, |ui| {
                        if let Some(v) = measure(ui, d, &key(field_key), &label, rule[field_key].as_f64(), unit, true) {
                            d.fields.insert(key(field_key), json!(v));
                        }
                    });
                }
            });
            if which == "ruleAbove" {
                let keep = rule["keepInFrame"].as_bool().unwrap_or(false);
                if let Some(next) = toggle(ui, &key("keepInFrame"), &tr(app, "Keep In Frame"), Some(keep), true) {
                    d.fields.insert(key("keepInFrame"), json!(next));
                }
            }
        });
    });
}

/// A list label text (Text After, Number), shown with its tab as `^t`; labels expand metacharacters, so the typed
/// text is stored as it is.
fn list_text(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, label: &str, key: &str) {
    use craft_ui_widgets::text::TextField;
    let label = tr(app, label);
    let current = at.val(d, key).as_str().unwrap_or("").replace('\t', "^t");
    field(ui, &label, key, |ui| {
        let out = TextField::new(label.clone(), Bind::given(current.clone())).id_salt(("para_list_text", at.salt(key))).limit(None::<usize>).show(ui);
        if let Some(next) = out.events.landed().filter(|next| **next != current).cloned() {
            at.set(d, key, json!(next));
        }
    });
}

/// A character style for a bullet or number (`key`), [None] first.
fn list_char_style(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, key: &str, styles: &[String]) {
    let label = tr(app, "Character Style:");
    let names: Vec<String> =
        std::iter::once(designcraft_doc::NO_CHAR_STYLE.to_owned()).chain(styles.iter().filter(|n| *n != designcraft_doc::NO_CHAR_STYLE).cloned()).collect();
    let entries = style_entries(app, &names);
    field(ui, &label, key, |ui| {
        let current = at.val(d, key).as_str().unwrap_or(designcraft_doc::NO_CHAR_STYLE).to_owned();
        if let Some(next) = pick(ui, &at.salt(key), &label, &current, &entries, true) {
            at.set(d, key, json!(next));
        }
    });
}

/// The bullet character: common bullets, a field, and the font's glyphs in a grid to pick from.
fn bullet_char(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, lu: &crate::dialogs::ListUi<'_>) {
    use craft_ui_widgets::buttons::ToggleButton;
    use craft_ui_widgets::text::TextField;
    let label = tr(app, "Bullet Character:");
    let glyphs = tr(app, "Glyphs…");
    let current = at.val(d, "bulletChar").as_str().unwrap_or("").to_owned();
    field(ui, &label, "bulletChar", |ui| {
        Row::new().id_salt(("para_bullets", at.salt("chars"))).fill_width().show(ui, |ui, _| {
            for b in ["\u{2022}", "\u{2013}", "\u{2014}", "\u{00BB}", "*"] {
                let out = ToggleButton::new(b.to_owned(), Bind::given(current == b)).id_salt(("para_bullet", b.to_owned())).show(ui);
                if out.events.committed().copied() == Some(true) {
                    at.set(d, "bulletChar", json!(b));
                }
            }
            let out = TextField::new(label.clone(), Bind::given(current.clone())).id_salt(("para_bullet_char", at.salt("field"))).limit(None::<usize>).show(ui);
            if let Some(next) = out.events.landed().filter(|next| **next != current).cloned() {
                at.set(d, "bulletChar", json!(next));
            }
            let open = d.b("bulletGlyphs");
            let out = ToggleButton::new(glyphs.clone(), Bind::given(open)).id_salt(("para_bullet_glyphs", at.salt("open"))).show(ui);
            if let Some(next) = out.events.committed().copied() {
                d.fields.insert("bulletGlyphs".into(), json!(next));
            }
        });
    });
    if d.b("bulletGlyphs") {
        let font = at.val(d, "bulletFont").as_str().unwrap_or("").to_owned();
        let (family, style) = if font.is_empty() {
            (lu.family.to_string(), lu.style.to_string())
        } else {
            let st = at.val(d, "bulletFontStyle").as_str().unwrap_or("").to_owned();
            let st = if st.is_empty() { lu.fonts.styles(&font).into_iter().next().unwrap_or_else(|| "Regular".into()) } else { st };
            (font, st)
        };
        text(ui, d, "bulletGlyphQuery", "bullet_glyph_query", &tr(app, "Search: character or U+code"), true);
        let query = d.s("bulletGlyphQuery");
        // The glyph grid is one painted texture of the Glyphs panel; the framework has no equivalent yet.
        if let (Some(c), _) = crate::panels::glyphs::grid(ui, lu.fonts, lu.scope, &family, &style, &query, 160.0) {
            at.set(d, "bulletChar", json!(c.to_string()));
        }
    }
}

/// The bullet's font family and style ("" = the paragraph's).
fn bullet_font(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, lu: &crate::dialogs::ListUi<'_>) {
    let font = at.val(d, "bulletFont").as_str().unwrap_or("").to_owned();
    let mut families = vec![(String::new(), tr(app, "(Paragraph Font)"))];
    families.extend(lu.families.iter().map(|f| (f.clone(), f.clone())));
    let styles: Vec<(String, String)> = lu.fonts.styles(&font).into_iter().map(|s| (s.clone(), s)).collect();
    let (family_label, style_label) = (tr(app, "Font Family:"), tr(app, "Font Style:"));
    field(ui, &family_label, "bulletFont", |ui| {
        if let Some(next) = pick(ui, &at.salt("bulletFont"), &family_label, &font, &families, true) {
            at.set(d, "bulletFont", json!(next));
            let first = if next.is_empty() { String::new() } else { lu.fonts.styles(&next).into_iter().next().unwrap_or_default() };
            at.set(d, "bulletFontStyle", json!(first));
        }
    });
    field(ui, &style_label, "bulletFontStyle", |ui| {
        let current = at.val(d, "bulletFontStyle").as_str().unwrap_or("").to_owned();
        if let Some(next) = pick(ui, &at.salt("bulletFontStyle"), &style_label, &current, &styles, !font.is_empty()) {
            at.set(d, "bulletFontStyle", json!(next));
        }
    });
}

/// Bullets and Numbering: the list type and level; the bullet, its font and the text after it, or the list, number
/// style, number text and where numbering starts and restarts; the label's character style; then where the label goes
/// (alignment, indents and the tab position of the text after it).
pub(crate) fn bullets(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document, lu: &crate::dialogs::ListUi<'_>) {
    let at = At { prefix: "p.", pv };
    let units = doc.settings.horizontal_units;
    let kind = at.val(d, "listType").as_str().unwrap_or("none").to_owned();
    let level = at.val(d, "listLevel").as_u64().unwrap_or(1).clamp(1, designcraft_doc::LIST_LEVELS as u64) as u32;
    let types = captioned(&app.ui.language, &[("none", "None"), ("bullets", "Bullets"), ("numbers", "Numbers")]);
    let formats: Vec<(String, String)> = crate::dialogs::NUMBER_STYLES.iter().map(|(v, label)| ((*v).to_owned(), (*label).to_owned())).collect();
    let aligns = captioned(&app.ui.language, &[("left", "Left"), ("center", "Center"), ("right", "Right")]);
    let lists: Vec<(String, String)> = std::iter::once((String::new(), tr(app, "[Default]")))
        .chain(doc.settings.lists.iter().map(|l| (l.name.clone(), l.name.clone())))
        .collect();
    let modes = [("continue".to_owned(), tr(app, "Continue from Previous Number")), ("start".to_owned(), tr(app, "Start At"))];
    let any_level = tr(app, "Any Previous Level");
    let restart_levels: Vec<(String, String)> = (0..level).map(|n| (n.to_string(), if n == 0 { any_level.clone() } else { n.to_string() })).collect();
    Stack::new().id_salt("para_bullets_page").fill_width().scroll(Scroll::vertical()).room_height(340.0).show(ui, |ui, _| {
        FieldGroup::new(tr(app, "Bullets and Numbering")).id_salt("para_bullets_fields").show(ui, |ui, _| {
            let label = tr(app, "List Type:");
            field(ui, &label, "listType", |ui| attr_pick(ui, d, &at, "listType", &label, &types, true));
            if kind != "none" {
                let label = tr(app, "Level:");
                field(ui, &label, "listLevel", |ui| attr_count(ui, d, &at, "listLevel", &label, (1, designcraft_doc::LIST_LEVELS as u32), &Counts::Plain));
            }
            match kind.as_str() {
                "bullets" => {
                    bullet_char(app, ui, d, &at, lu);
                    bullet_font(app, ui, d, &at, lu);
                    list_text(app, ui, d, &at, "Text After:", "listSeparator");
                    list_char_style(app, ui, d, &at, "bulletCharStyle", lu.char_styles);
                }
                "numbers" => {
                    let label = tr(app, "List:");
                    field(ui, &label, "listName", |ui| attr_pick(ui, d, &at, "listName", &label, &lists, true));
                    let label = tr(app, "Format:");
                    field(ui, &label, "numberStyle", |ui| attr_pick(ui, d, &at, "numberStyle", &label, &formats, true));
                    list_text(app, ui, d, &at, "Number:", "numberExpression");
                    list_char_style(app, ui, d, &at, "numberCharStyle", lu.char_styles);
                    let label = tr(app, "Mode:");
                    let start = at.val(d, "startAt").as_u64();
                    field(ui, &label, "startAt", |ui| {
                        Row::new().id_salt("para_number_mode").fill_width().show(ui, |ui, _| {
                            let shown = if start.is_some() { "start" } else { "continue" };
                            if let Some(next) = pick(ui, "p.startMode", &label, shown, &modes, true) {
                                if next == "start" && start.is_none() {
                                    at.set(d, "startAt", json!(1));
                                } else if next == "continue" {
                                    at.set(d, "startAt", Value::Null);
                                }
                            }
                            if let Some(n) = start
                                && let Some(v) = count(ui, "p.startAtNumber", &tr(app, "Start At"), Some(n as f64), (0, 1_000_000), &Counts::Plain, true)
                            {
                                at.set(d, "startAt", json!(v));
                            }
                        });
                    });
                    // Restart Numbers at This Level After: a higher level (any, or one of them).
                    Row::new().id_salt("para_number_restart").fill_width().enabled(level > 1).show(ui, |ui, _| {
                        attr_toggle(ui, d, &at, "restartNumbers", &tr(app, "Restart Numbers at This Level After:"), true);
                        let after = at.val(d, "restartAfterLevel").as_u64().unwrap_or(0).to_string();
                        if let Some(next) = pick(ui, "p.restartAfterLevel", &tr(app, "Restart After Level"), &after, &restart_levels, true) {
                            at.set(d, "restartAfterLevel", json!(next.parse::<u32>().unwrap_or(0)));
                        }
                    });
                }
                _ => {}
            }
            if kind != "none" {
                let key = if kind == "bullets" { "bulletAlign" } else { "numberAlign" };
                let label = tr(app, "Alignment:");
                field(ui, &label, key, |ui| attr_pick(ui, d, &at, key, &label, &aligns, true));
                for (label, key) in [("Left Indent:", "leftIndent"), ("First Line Indent:", "firstLineIndent")] {
                    let label = tr(app, label);
                    field(ui, &label, key, |ui| attr_measure(ui, d, &at, key, &label, units, true));
                }
                // Tab Position: none, or a stop for the text after the label.
                let label = tr(app, "Tab Position:");
                field(ui, &label, "listTab", |ui| {
                    Row::new().id_salt("para_list_tab").fill_width().show(ui, |ui, _| {
                        let tab = at.val(d, "listTab").as_f64();
                        if let Some(on) = toggle(ui, "p.listTabOn", &label, Some(tab.is_some()), true) {
                            let left = at.val(d, "leftIndent").as_f64().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(18.0);
                            at.set(d, "listTab", if on { json!(left) } else { Value::Null });
                        }
                        if let Some(t) = tab
                            && let Some(v) = measure(ui, d, "p.listTab", &tr(app, "Tab Position"), Some(t), units, true)
                        {
                            at.set(d, "listTab", json!(v.max(0.0)));
                        }
                    });
                });
            }
        });
    });
}

/// Whether the four values of a group are linked, as the link left them.
fn linked_state(d: &Dialog, key: &str, same: bool) -> bool {
    crate::dialogs::linked(d, key, same)
}

/// The link toggle between a group's four sides or corners; the state it was clicked to.
fn link_button(ui: &mut Ui, app: &DesignApp, d: &mut Dialog, key: &str, linked: bool) -> Option<bool> {
    use craft_ui_widgets::text::link_toggle;
    let out = link_toggle(tr(app, "Make all settings the same"), Bind::given(linked)).id_salt(("para_link", key.to_owned())).show(ui);
    let next = out.events.committed().copied().filter(|next| *next != linked)?;
    d.fields.insert(format!("_linked.{key}"), json!(next));
    Some(next)
}

/// What a side field edits.
#[derive(Clone, Copy)]
enum SideKind {
    /// A stroke weight, in points.
    Weight,
    /// A distance in the document's unit.
    Offset(Unit),
}

/// Four per-side values in the order the attribute holds them: top, left, bottom, right. An edited side stores the
/// whole array.
fn sides(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, key: &str, kind: SideKind, group: &str) {
    let mut values: [f64; 4] = serde_json::from_value(at.val(d, key)).unwrap_or_default();
    let linked = linked_state(d, key, values.iter().all(|v| (v - values[0]).abs() < 1e-9));
    let mut changed = false;
    FieldGroup::new(group.to_owned()).id_salt(("para_sides", at.salt(key))).show(ui, |ui, _| {
        for (i, label) in ["Top:", "Left:", "Bottom:", "Right:"].into_iter().enumerate() {
            let label = tr(app, label);
            field(ui, &label, &format!("{key}{i}"), |ui| {
                Row::new().id_salt(("para_side", format!("{}{i}", at.salt(key)))).show(ui, |ui, _| {
                    let salt = format!("{}{i}", at.salt(key));
                    let edited = match kind {
                        SideKind::Weight => {
                            let num = Num::decimal(0.0, 1000.0).counting(Counts::Kind(UnitKind::Points));
                            number(ui, &salt, &label, Some(values[i]), &num, true).map(|v| v.filter(|v| v.is_finite()).map_or(0.0, |v| v.clamp(0.0, 1000.0)))
                        }
                        SideKind::Offset(unit) => measure(ui, d, &salt, &label, Some(values[i]), unit, true),
                    };
                    if let Some(v) = edited {
                        // Linked, a side typed in sets all four.
                        if linked {
                            values = [v; 4];
                        } else {
                            values[i] = v;
                        }
                        changed = true;
                    }
                    // Linked again, the four take Top's value.
                    if i == 0 && link_button(ui, app, d, key, linked) == Some(true) {
                        values = [values[0]; 4];
                        changed = true;
                    }
                });
            });
        }
    });
    if changed {
        at.set(d, key, json!(values));
    }
}

/// Built-in stroke types of the paragraph border's Type menu (serialized `StrokeType`s).
const BORDER_TYPES: [&str; 11] =
    ["solid", "dashed", "dotted", "thickThin", "thinThick", "thinThin", "thickThick", "thinThickThin", "thickThinThick", "wavy", "hashed"];

/// The border's stroke type: a built-in type or one of the document's stroke styles.
fn border_type(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, doc: &designcraft_doc::Document) {
    use designcraft_doc::StrokeType;
    let lang = app.ui.language.as_str();
    let cur: StrokeType = serde_json::from_value(at.val(d, "borderType")).unwrap_or_default();
    let builtin = |k: &str| -> Option<(Value, StrokeType)> {
        let ty = if k == "dashed" { json!({"kind": k, "pattern": []}) } else { json!({"kind": k}) };
        let t = serde_json::from_value::<StrokeType>(ty.clone()).ok()?;
        Some((ty, t))
    };
    let mut entries: Vec<(String, String)> = BORDER_TYPES.iter().filter_map(|k| builtin(k).map(|(_, t)| ((*k).to_owned(), crate::i18n::tr(lang, t.label()).to_string()))).collect();
    entries.extend(doc.stroke_styles.iter().map(|st| (format!("style:{}", st.name), st.name.clone())));
    let shown = match &cur {
        StrokeType::Style { name } => format!("style:{name}"),
        k => BORDER_TYPES.iter().find(|b| builtin(b).is_some_and(|(_, t)| std::mem::discriminant(&t) == std::mem::discriminant(k))).map_or(String::new(), |b| (*b).to_owned()),
    };
    let label = tr(app, "Type:");
    field(ui, &label, "borderType", |ui| {
        if let Some(next) = pick(ui, "p.borderType", &label, &shown, &entries, true) {
            let ty = match next.strip_prefix("style:") {
                Some(name) => json!({"kind": "style", "name": name}),
                None => builtin(&next).map_or(Value::Null, |(ty, _)| ty),
            };
            if !ty.is_null() {
                at.set(d, "borderType", ty);
            }
        }
    });
}

/// Corner size and shape of a border or shading (`key` holds top left, top right, bottom right, bottom left): Top
/// Left and Top Right over Bottom Left and Bottom Right, each a size and a shape menu showing the shapes' glyphs, with
/// the link between. An edited corner stores all four.
fn corners(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, key: &str, units: Unit) {
    use designcraft_geom::corners::{Corner, CornerShape};
    let mut values: [Corner; 4] = serde_json::from_value(at.val(d, key)).unwrap_or_default();
    let linked = linked_state(d, key, values.iter().all(|c| *c == values[0]));
    let (mut changed, mut relinked) = (None, false);
    let shapes: Vec<craft_ui_widgets::contract::Entry<usize>> =
        CornerShape::ALL.iter().enumerate().map(|(i, s)| craft_ui_widgets::contract::Entry::new(i, tr(app, s.label()))).collect();
    GroupBox::new(tr(app, "Corner Size and Shape"), None).id_salt(("para_corners_box", at.salt(key))).show(ui, |ui, _| {
        for (row, pair) in [[(0, "Top Left Corner"), (1, "Top Right Corner")], [(3, "Bottom Left Corner"), (2, "Bottom Right Corner")]].into_iter().enumerate() {
            Row::new().id_salt(("para_corner_row", format!("{}{row}", at.salt(key)))).fill_width().show(ui, |ui, _| {
                for (slot, (i, label)) in pair.into_iter().enumerate() {
                    let label = tr(app, label);
                    let c = &mut values[i];
                    if let Some(v) = measure(ui, d, &format!("{}{i}", at.salt(key)), &label, Some(c.size), units, true) {
                        c.size = v.clamp(0.0, 10_000.0);
                        changed = Some(i);
                    }
                    let current = CornerShape::ALL.iter().position(|s| *s == c.shape).unwrap_or(0);
                    let out = craft_ui_widgets::pickers::Select::new(format!("{label} {}", tr(app, "Corner Shape")), Bind::given(current), &shapes)
                        .id_salt(("para_corner_shape", format!("{}{i}", at.salt(key))))
                        .picture(|index: &usize, cx: &craft_ui_widgets::primitives::ContentPainterCx, rect: egui::Rect| {
                            let ink = crate::theme::Tokens::get(cx.painter.ctx()).icon;
                            crate::panels::properties::paint_corner_shape(&cx.painter, rect, CornerShape::ALL[*index], ink);
                        })
                        .show(ui);
                    if let Some(next) = out.events.committed().copied().filter(|next| *next != current) {
                        c.shape = CornerShape::ALL[next];
                        changed = Some(i);
                    }
                    // Linked again, the four take Top Left's size and shape.
                    if row == 0 && slot == 0 && link_button(ui, app, d, key, linked) == Some(true) {
                        relinked = true;
                    }
                }
            });
        }
    });
    if relinked {
        values = [values[0]; 4];
        changed = Some(0);
    }
    if let Some(i) = changed {
        // Linked, the corner changed sets all four.
        if linked {
            values = [values[i]; 4];
        }
        at.set(d, key, serde_json::to_value(values).unwrap_or_default());
    }
}

/// The colour and tint rows of a border, its gap or shading (`keys`: colour, tint, overprint); Overprint follows the
/// tint, as InDesign's.
fn color_tint(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, at: &At, doc: &designcraft_doc::Document, rows: [&str; 5], enabled: bool) {
    let [color_key, tint_key, overprint_key, color_caption, tint_caption] = rows;
    let (color_label, tint_label) = (tr(app, color_caption), tr(app, tint_caption));
    field(ui, &color_label, color_key, |ui| {
        let color = at.val(d, color_key).as_str().unwrap_or("").to_owned();
        if let Some(w) = super::style_kit::swatch_pick(ui, &at.salt(color_key), &color_label, doc, &color, &Extra::default(), enabled) {
            at.set(d, color_key, json!(w));
        }
    });
    field(ui, &tint_label, tint_key, |ui| {
        Row::new().id_salt(("para_tint_row", at.salt(tint_key))).fill_width().enabled(enabled).show(ui, |ui, _| {
            let tint = at.val(d, tint_key).as_f64().map(|t| (t * 1000.0).round() / 10.0);
            let num = Num::decimal(0.0, 100.0).counting(Counts::Kind(UnitKind::Percent));
            if let Some(v) = number(ui, &at.salt(tint_key), &tint_label, tint, &num, true) {
                at.set(d, tint_key, json!(v.filter(|v| v.is_finite()).map_or(1.0, |v| (v / 100.0).clamp(0.0, 1.0))));
            }
            attr_toggle(ui, d, at, overprint_key, &tr(app, "Overprint"), true);
        });
    });
}

/// Paragraph Border / Paragraph Shading (and the Paragraph Borders and Shading dialog): on or off, the stroke (border)
/// or colour (shading), corners, offsets, the edges and width the box follows, and the options for split and
/// consecutive paragraphs.
pub(crate) fn border_shading(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, pv: &Value, doc: &designcraft_doc::Document, border: bool) {
    let at = At { prefix: "p.", pv };
    let lang = app.ui.language.as_str();
    let key = |field: &str| format!("{}{field}", if border { "border" } else { "shading" });
    let on_key = key("On");
    let on = at.val(d, &on_key).as_bool().unwrap_or(false);
    let units = doc.settings.horizontal_units;
    let tops = captioned(lang, &[("ascent", "Ascent"), ("baseline", "Baseline"), ("emBox", "Em Box Top"), ("leading", "Leading")]);
    let bottoms = captioned(lang, &[("descent", "Descent"), ("baseline", "Baseline"), ("emBox", "Em Box Bottom")]);
    let widths = captioned(lang, &[("column", "Column"), ("text", "Text")]);
    let caps = captioned(lang, &[("butt", "Butt"), ("round", "Round"), ("projecting", "Projecting")]);
    let joins = captioned(lang, &[("miter", "Miter"), ("round", "Round"), ("bevel", "Bevel")]);
    let solid = at.val(d, "borderType").get("kind").and_then(Value::as_str).is_none_or(|k| k == "solid");
    Stack::new().id_salt(("para_border_shading", border)).fill_width().show(ui, |ui, _| {
        attr_toggle(ui, d, &at, &on_key, &tr(app, if border { "Border" } else { "Shading" }), true);
        Stack::new().id_salt(("para_bs_scroll", border)).fill_width().scroll(Scroll::vertical()).room_height(400.0).enabled(on).show(ui, |ui, _| {
            if border {
                sides(app, ui, d, &at, &key("Weights"), SideKind::Weight, &tr(app, "Stroke"));
            }
            FieldGroup::new(tr(app, if border { "Stroke" } else { "Color" })).id_salt(("para_bs_color", border)).show(ui, |ui, _| {
                if border {
                    border_type(app, ui, d, &at, doc);
                }
                color_tint(app, ui, d, &at, doc, [&key("Color"), &key("Tint"), &key("Overprint"), "Color:", "Tint:"], true);
                if border {
                    // The gap colour shows between the dashes, dots or stripes.
                    color_tint(app, ui, d, &at, doc, ["borderGapColor", "borderGapTint", "borderGapOverprint", "Gap Color:", "Gap Tint:"], !solid);
                    let label = tr(app, "Cap:");
                    field(ui, &label, "borderCap", |ui| attr_pick(ui, d, &at, "borderCap", &label, &caps, true));
                    let label = tr(app, "Join:");
                    field(ui, &label, "borderJoin", |ui| attr_pick(ui, d, &at, "borderJoin", &label, &joins, true));
                }
            });
            corners(app, ui, d, &at, &key("Corners"), units);
            Row::new().id_salt(("para_bs_offsets", border)).fill_width().show(ui, |ui, _| {
                sides(app, ui, d, &at, &key("Offsets"), SideKind::Offset(units), &tr(app, "Offsets"));
                FieldGroup::new(tr(app, "Edges")).id_salt(("para_bs_edges", border)).show(ui, |ui, _| {
                    let label = tr(app, "Top Edge:");
                    field(ui, &label, "top", |ui| attr_pick(ui, d, &at, &key("Top"), &label, &tops, true));
                    let label = tr(app, "Bottom Edge:");
                    field(ui, &label, "bottom", |ui| attr_pick(ui, d, &at, &key("Bottom"), &label, &bottoms, true));
                    let label = tr(app, "Width:");
                    field(ui, &label, "width", |ui| attr_pick(ui, d, &at, &key("Width"), &label, &widths, true));
                });
            });
            if border {
                attr_toggle(ui, d, &at, "borderDisplayIfSplits", &tr(app, "Display Border if Paragraph Splits Across Frames/Columns"), true);
                attr_toggle(ui, d, &at, "borderMerge", &tr(app, "Merge Consecutive Borders and Shading"), true);
            } else {
                attr_toggle(ui, d, &at, "shadingClip", &tr(app, "Clip to Frame"), true);
                attr_toggle(ui, d, &at, "shadingNonprinting", &tr(app, "Do not Print or Export"), true);
            }
        });
    });
}
