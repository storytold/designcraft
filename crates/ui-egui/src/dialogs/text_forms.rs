//! The framework forms of the text dialogs: Footnote Options, Paragraph Rules, Justification, Hyphenation Settings and New
//! Cross-Reference. Each form edits the draft the native body edits (the same `{prefix}{attribute}` fields over the same
//! `current` values) and leaves OK, Cancel, the live preview and every command to the existing controller, so documents,
//! undo and saved settings are what they were.
//!
//! The controls over the draft (`Attrs`, `number`, `measure`, `check`, `select_attr`, …) are shared with the paragraph
//! forms of `paragraph_forms`, and the Justification, Hyphenation and Paragraph Rules bodies take the prefix and the
//! resolved values the Paragraph Style Options sections use, so those sections can show the same bodies.
//!
//! A measure is a text field whose committed text is resolved with `parse_measure` and written back with
//! `format_measure` in the document's unit (the framework's number field reads decimal picas only and has no kind for
//! every unit the document offers), as the native spinner fields did.

use super::Dialog;
use super::framework::Form;
use crate::DesignApp;
use craft_behavior::{DisplayOrder, EditEvent, ItemSource, Mixed, Selection};
use craft_theme::variant::Tone;
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::choice::{Checkbox, Segmented};
use craft_ui_widgets::contract::{Bind, Entry, Props as _, SelectMode, Show as _, ShowCollection as _, ShowContainer as _, Sizing};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::{Field, FieldGroup};
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::list::{List, ListRow};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::text::{NumberField, TextField};
use craft_ui_widgets::value_drag::Slider;
use designcraft_geom::{Unit, format_measure, units::parse_measure};
use egui::Ui;
use serde_json::{Value, json};

/// Document › Footnote Options.
pub(crate) const FOOTNOTE_OPTIONS: Form = Form::new(footnote_options);
/// Type › Paragraph Rules.
pub(crate) const PARAGRAPH_RULES: Form = Form::new(paragraph_rules);
/// Type › Justification.
pub(crate) const JUSTIFICATION: Form = Form::new(justification);
/// Type › Hyphenation Settings, with its live preview.
pub(crate) const HYPHENATION: Form = Form::new(hyphenation);
/// Type › Insert Cross-Reference.
pub(crate) const INSERT_XREF: Form = Form::new(insert_xref);

// ---------------------------------------------------------------------------------------------------------------------
// Controls over the draft

/// Where a form reads and writes a group of attributes: an edit is stored in the draft as `{prefix}{key}`, and an
/// attribute not edited shows `base[key]` (`null` where the selected paragraphs differ).
#[derive(Clone, Copy)]
pub(super) struct Attrs<'a> {
    prefix: &'a str,
    base: &'a Value,
}

impl<'a> Attrs<'a> {
    pub(super) fn new(prefix: &'a str, base: &'a Value) -> Self {
        Attrs { prefix, base }
    }

    /// The draft field of attribute `key`.
    pub(super) fn field(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    /// The value shown for `key`: the edit, else the base value.
    pub(super) fn get(&self, d: &Dialog, key: &str) -> Value {
        d.fields.get(&self.field(key)).cloned().unwrap_or_else(|| self.base.get(key).cloned().unwrap_or(Value::Null))
    }

    /// Edits `key`; the value is the whole attribute.
    pub(super) fn set(&self, d: &mut Dialog, key: &str, value: Value) {
        d.fields.insert(self.field(key), value);
    }
}

/// The text a draft value shows in a text field (`Dialog::s`'s reading of it).
fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// What a number field holds: its range, the factor from the stored value to the shown one (100 for a fraction shown as
/// a percentage), whether it takes whole numbers only, and its unit text.
#[derive(Clone)]
pub(super) struct Spec {
    range: (f64, f64),
    scale: f64,
    whole: bool,
    unit: Option<String>,
    /// Emptying the field is an edit (an attribute that may be unset).
    clearable: bool,
}

impl Spec {
    /// Whole numbers from `least` to `most`.
    pub(super) fn whole(least: f64, most: f64) -> Self {
        Spec { range: (least, most), scale: 1.0, whole: true, unit: None, clearable: false }
    }

    /// Numbers from `least` to `most`.
    pub(super) fn real(least: f64, most: f64) -> Self {
        Spec { range: (least, most), scale: 1.0, whole: false, unit: None, clearable: false }
    }

    /// Shown `scale` times the stored value.
    pub(super) fn scaled(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Whether the field takes whole numbers only.
    pub(super) fn is_whole(&self) -> bool {
        self.whole
    }

    /// An emptied field reports an edit (`Some(None)`) instead of none.
    pub(super) fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// With `unit` after the number.
    pub(super) fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }
}

/// `body` under the side label `label`, which names it.
pub(super) fn field(ui: &mut Ui, label: &str, salt: &str, mut body: impl FnMut(&mut Ui)) {
    Field::new(label.to_owned()).id_salt(("text_form_field", salt.to_owned())).show(ui, |ui, _| body(ui));
}

/// A number field over `shown` (in stored units): the value stored after an edit that committed, kept inside the range.
/// The outer `Option` is the edit; the inner one is kept for callers that match `Some(Some(value))`.
pub(super) fn number_edit(ui: &mut Ui, salt: &str, label: &str, shown: Option<f64>, spec: &Spec, enabled: bool) -> Option<Option<f64>> {
    let scaled = shown.map(|v| v * spec.scale);
    let mut field = NumberField::new(label.to_owned(), Bind::given(scaled))
        .id_salt(("text_form_number", salt.to_owned()))
        .range(spec.range.0..=spec.range.1)
        .step(1.0)
        .enabled(enabled);
    if spec.whole {
        field = field.snap_typed(true).fixed_decimals(0).commit_decimals(0);
    }
    if let Some(unit) = &spec.unit {
        field = field.unit(unit.clone());
    }
    let landed = field.show(ui).edit.events.landed().copied()?;
    match landed {
        Some(value) if value.is_finite() => {
            let value = value.clamp(spec.range.0, spec.range.1);
            // Adding 0.0 turns a rounded -0 into 0.
            let value = if spec.whole { value.round() + 0.0 } else { value };
            scaled.is_none_or(|before| (before - value).abs() > 1e-9).then_some(Some(value / spec.scale))
        }
        None if spec.clearable && scaled.is_some() => Some(None),
        _ => None,
    }
}

/// A number attribute: an edit is stored as the attribute's own number (an integer for a whole count).
pub(super) fn number(ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, label: &str, spec: &Spec, enabled: bool) {
    let shown = a.get(d, key).as_f64().filter(|v| v.is_finite());
    if let Some(edit) = number_edit(ui, &a.field(key), label, shown, spec, enabled) {
        let value = match edit {
            None => Value::Null,
            Some(value) if spec.whole && spec.scale == 1.0 => json!(value.max(0.0) as u64),
            Some(value) => json!(value),
        };
        a.set(d, key, value);
    }
}

/// A measure field over `shown` (points), in `unit`: the points stored after an edit that committed, kept inside
/// `range`. A text that is no measure leaves the value as it was.
pub(super) fn measure_edit(ui: &mut Ui, salt: &str, label: &str, shown: Option<f64>, unit: Unit, range: (f64, f64), enabled: bool) -> Option<f64> {
    let text = shown.map(|v| format_measure(v, unit)).unwrap_or_default();
    let out = TextField::new(label.to_owned(), Bind::given(text))
        .id_salt(("text_form_measure", salt.to_owned()))
        .limit(None::<usize>)
        .enabled(enabled)
        .show(ui);
    let Some(EditEvent::Commit { after, .. } | EditEvent::Set { after, .. }) = out.events.ended() else { return None };
    let value = parse_measure(after, unit).ok().filter(|v| v.is_finite())?.clamp(range.0, range.1);
    shown.is_none_or(|before| (before - value).abs() > 1e-9).then_some(value)
}

/// A measure attribute, in points.
pub(super) fn measure(ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, label: &str, unit: Unit, enabled: bool) {
    let shown = a.get(d, key).as_f64().filter(|v| v.is_finite());
    if let Some(points) = measure_edit(ui, &a.field(key), label, shown, unit, (f64::MIN, f64::MAX), enabled) {
        a.set(d, key, json!(points));
    }
}

/// A text attribute, written to the draft as it is typed.
pub(super) fn text(ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, label: &str, enabled: bool) {
    let current = text_of(&a.get(d, key));
    let out = TextField::new(label.to_owned(), Bind::given(current.clone()))
        .id_salt(("text_form_text", a.field(key)))
        .limit(None::<usize>)
        .enabled(enabled)
        .show(ui);
    if let Some(next) = out.events.landed().filter(|next| **next != current).cloned() {
        a.set(d, key, json!(next));
    }
}

/// A flag attribute: a missing value is off.
pub(super) fn check(ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, label: &str, enabled: bool) {
    let current = a.get(d, key).as_bool().unwrap_or(false);
    let out = Checkbox::new(Bind::given(current)).label(label.to_owned()).id_salt(("text_form_check", a.field(key))).enabled(enabled).show(ui);
    if let Some(on) = out.events.committed().copied().filter(|on| *on != current) {
        a.set(d, key, json!(on));
    }
}

/// A flag attribute of paragraphs that may differ (`null`): it shows as mixed, and the first click sets it on (off where
/// the flag is on without a value, `default`).
pub(super) fn check_mixed(ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, label: &str, default: bool, enabled: bool) {
    let current = a.get(d, key);
    let mixed = current.is_null();
    let value = if mixed { Mixed::Mixed } else { Mixed::Same(current.as_bool().unwrap_or(false)) };
    let out = Checkbox::new(Bind::given(value)).label(label.to_owned()).id_salt(("text_form_check", a.field(key))).enabled(enabled).show(ui);
    if let Some(on) = out.events.committed().copied() {
        let on = if mixed && default { false } else { on };
        if mixed || Some(on) != current.as_bool() {
            a.set(d, key, json!(on));
        }
    }
}

/// A picker over `captions`, with entry `at` picked: the entry picked in this pass. `placeholder` shows while none is.
pub(super) fn select_at(
    ui: &mut Ui,
    salt: &str,
    label: &str,
    captions: &[String],
    at: Option<usize>,
    placeholder: Option<String>,
    enabled: bool,
) -> Option<usize> {
    let entries: Vec<Entry<usize>> = captions.iter().enumerate().map(|(i, caption)| Entry::new(i, caption.clone())).collect();
    let value = at.map_or(Mixed::Mixed, Mixed::Same);
    let mut picker =
        Select::new(label.to_owned(), Bind::given(value), &entries).keeps_name(true).id_salt(("text_form_select", salt.to_owned())).enabled(enabled);
    if let Some(placeholder) = placeholder.filter(|text| !text.is_empty()) {
        picker = picker.placeholder(placeholder);
    }
    picker.show(ui).events.committed().copied().filter(|picked| Some(*picked) != at)
}

/// A picker over a draft attribute, from `options` as (stored value, caption). A value none of them has shows as the
/// picker's placeholder; a mixed one (`null`) shows blank until a choice is made. The value picked in this pass.
pub(super) fn select_attr(ui: &mut Ui, d: &Dialog, a: &Attrs, key: &str, label: &str, options: &[(Value, String)], enabled: bool) -> Option<Value> {
    let current = a.get(d, key);
    let at = options.iter().position(|(value, _)| *value == current);
    let captions: Vec<String> = options.iter().map(|(_, caption)| caption.clone()).collect();
    let placeholder = if at.is_none() { current.as_str().map(str::to_owned) } else { None };
    let picked = select_at(ui, &a.field(key), label, &captions, at, placeholder, enabled)?;
    options.get(picked).map(|(value, _)| value.clone())
}

/// [`select_attr`] over string values, written to the draft.
pub(super) fn choice(ui: &mut Ui, d: &mut Dialog, a: &Attrs, key: &str, label: &str, options: &[(Value, String)], enabled: bool) {
    if let Some(value) = select_attr(ui, d, a, key, label, options, enabled) {
        a.set(d, key, value);
    }
}

/// The swatches a colour menu offers: the document's, after the text's own colour where the attribute has one, each shown
/// with its colour chip.
pub(super) struct Swatches<'a> {
    options: Vec<(Value, String)>,
    doc: Option<&'a designcraft_doc::Document>,
}

impl<'a> Swatches<'a> {
    /// The visible swatches of `doc`; `text_color` names the entry for the text's own colour when there is one.
    pub(super) fn new(doc: Option<&'a designcraft_doc::Document>, text_color: Option<String>) -> Self {
        let mut options = Vec::new();
        if let Some(caption) = text_color {
            options.push((json!(designcraft_doc::TEXT_COLOR), caption));
        }
        options.extend(doc.into_iter().flat_map(|doc| doc.swatches.iter().filter(|w| !w.hidden).map(|w| (json!(w.name), w.name.clone()))));
        Swatches { options, doc }
    }
}

impl<'a> Swatches<'a> {
    /// The visible swatches of `doc` after `first`, entries of the menu's own (a blank one, the text's own colour).
    pub(super) fn with_entries(doc: Option<&'a designcraft_doc::Document>, first: Vec<(Value, String)>) -> Self {
        let mut menu = Swatches::new(doc, None);
        menu.options.splice(0..0, first);
        menu
    }
}

/// A picker over the swatches of `menu` for the draft attribute `key`: the value picked in this pass.
pub(super) fn swatch_choice(ui: &mut Ui, d: &Dialog, a: &Attrs, key: &str, label: &str, menu: &Swatches, enabled: bool) -> Option<Value> {
    swatch_value(ui, &a.field(key), label, menu, &a.get(d, key), enabled)
}

/// A picker over the swatches of `menu` for `current`: the value picked in this pass.
pub(super) fn swatch_value(ui: &mut Ui, salt: &str, label: &str, menu: &Swatches, current: &Value, enabled: bool) -> Option<Value> {
    let current = current.clone();
    let at = menu.options.iter().position(|(value, _)| *value == current);
    let entries: Vec<Entry<usize>> = menu.options.iter().enumerate().map(|(i, (_, caption))| Entry::new(i, caption.clone())).collect();
    let chips: Vec<Option<(Option<egui::Color32>, Option<(egui::Color32, egui::Color32)>)>> = menu
        .options
        .iter()
        .map(|(value, _)| {
            let name = value.as_str().filter(|name| *name != designcraft_doc::TEXT_COLOR)?;
            menu.doc.map(|doc| crate::widgets::swatch_colors(doc, name, 1.0))
        })
        .collect();
    let mut picker = Select::new(label.to_owned(), Bind::given(at.map_or(Mixed::Mixed, Mixed::Same)), &entries)
        .keeps_name(true)
        .id_salt(("text_form_swatch", salt.to_owned()))
        .enabled(enabled)
        .picture(move |index: &usize, cx: &craft_ui_widgets::primitives::ContentPainterCx, rect: egui::Rect| {
            if let Some(Some((color, gradient))) = chips.get(*index) {
                crate::widgets::paint_chip(&cx.painter, rect, *color, *gradient);
            }
        });
    if let Some(name) = current.as_str().filter(|name| at.is_none() && !name.is_empty()) {
        picker = picker.placeholder(name.to_owned());
    }
    let picked = picker.show(ui).events.committed().copied().filter(|picked| Some(*picked) != at)?;
    menu.options.get(picked).map(|(value, _)| value.clone())
}

/// `options` as (stored value, caption in the interface language).
pub(super) fn captioned(lang: &str, options: &[(&str, &str)]) -> Vec<(Value, String)> {
    options.iter().map(|(value, caption)| (json!(value), crate::i18n::tr(lang, caption).to_string())).collect()
}

/// A strip of tabs over `current`: the tab picked in this pass.
pub(super) fn tabs(ui: &mut Ui, salt: &str, label: &str, current: &str, options: &[(&str, String)]) -> Option<String> {
    let entries: Vec<Entry<String>> = options.iter().map(|(id, caption)| Entry::new((*id).to_owned(), caption.clone())).collect();
    let out = Segmented::new(label.to_owned(), Bind::given(current.to_owned()), &entries).id_salt(("text_form_tabs", salt.to_owned())).show(ui);
    out.events.committed().cloned().filter(|next| next != current)
}

/// A message under a form: in the bad tone for a refusal, muted otherwise.
pub(super) fn message(ui: &mut Ui, text: &str, bad: bool) {
    let text = Text::new(text.to_owned());
    if bad {
        text.tone(Tone::Bad).show(ui)
    } else {
        text.shade(Shade::Muted).show(ui)
    };
}

/// A line of text.
pub(super) fn line(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).show(ui);
}

/// A bold heading over a group of controls.
pub(super) fn heading(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).role(craft_ui_widgets::style::TypeRole::BodyStrong).show(ui);
}

/// A push button; whether it was clicked.
pub(super) fn push(ui: &mut Ui, salt: &str, label: &str, enabled: bool) -> bool {
    craft_ui_widgets::buttons::Button::new(label.to_owned()).id_salt(("text_form_button", salt.to_owned())).enabled(enabled).show(ui).clicked()
}

/// A picker over `options` as (value, caption) for the current `value`; the value picked in this pass. A value none of them
/// has shows as the placeholder.
pub(super) fn pick(ui: &mut Ui, salt: &str, label: &str, value: &str, options: &[(String, String)], enabled: bool) -> Option<String> {
    let captions: Vec<String> = options.iter().map(|(_, caption)| caption.clone()).collect();
    let at = options.iter().position(|(v, _)| v == value);
    let placeholder = at.is_none().then(|| value.to_owned());
    let picked = select_at(ui, salt, label, &captions, at, placeholder, enabled)?;
    options.get(picked).map(|(v, _)| v.clone())
}

/// A check box over a flag that may be unset or mixed (`None`: it shows mixed, and a click sets it on): the state clicked to.
pub(super) fn toggle(ui: &mut Ui, salt: &str, label: &str, value: Option<bool>, enabled: bool) -> Option<bool> {
    let shown = value.map_or(Mixed::Mixed, Mixed::Same);
    let out = Checkbox::new(Bind::given(shown)).label(label.to_owned()).id_salt(("text_form_toggle", salt.to_owned())).enabled(enabled).show(ui);
    out.events.committed().copied().filter(|next| Some(*next) != value)
}

/// A short text that is committed, not typed through: its value when committed.
pub(super) fn committed_text(ui: &mut Ui, salt: &str, label: &str, current: &str, enabled: bool) -> Option<String> {
    let out = TextField::new(label.to_owned(), Bind::given(current.to_owned()))
        .id_salt(("text_form_commit", salt.to_owned()))
        .limit(None::<usize>)
        .enabled(enabled)
        .show(ui);
    out.events.committed().cloned().filter(|next| next != current)
}

/// The rows of a pick list: their captions.
struct Rows(Vec<String>);

impl DisplayOrder<usize> for Rows {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn key(&self, at: usize) -> usize {
        at
    }

    fn index_of(&self, key: &usize) -> Option<usize> {
        (*key < self.0.len()).then_some(*key)
    }

    fn text(&self, at: usize) -> Option<std::borrow::Cow<'_, str>> {
        self.0.get(at).map(|caption| std::borrow::Cow::Borrowed(caption.as_str()))
    }
}

impl ItemSource<usize> for Rows {}

/// A list of `items` of which one is picked, at most `max_rows` tall: the row picked in this pass.
pub(super) fn pick_list(ui: &mut Ui, salt: &str, label: &str, items: &[String], current: Option<usize>, max_rows: usize) -> Option<usize> {
    let rows = Rows(items.to_vec());
    let mut selected = current.map_or_else(Selection::default, Selection::single);
    let _shown = List::<usize>::new(label.to_owned())
        .id_salt(("text_form_list", salt.to_owned()))
        .select(SelectMode::Single)
        .arrows_select(true)
        .max_rows(u16::try_from(max_rows).unwrap_or(u16::MAX))
        .sizing(Sizing::FillWidth)
        .selection(Bind::bound(&mut selected))
        .show_items(ui, &rows, &mut |at, _| ListRow::new(rows.0.get(*at).cloned().unwrap_or_default()));
    selected.items.first().copied().filter(|at| Some(*at) != current)
}

// ---------------------------------------------------------------------------------------------------------------------
// Footnote Options

/// The names of the document's styles, character (with [No Character Style] first) or paragraph, as (name, caption).
fn style_options(app: &DesignApp, character: bool) -> Vec<(Value, String)> {
    let lang = app.ui.language.as_str();
    let mut names: Vec<String> = app
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
    if character && !names.iter().any(|n| n == designcraft_doc::NO_CHAR_STYLE) {
        names.insert(0, designcraft_doc::NO_CHAR_STYLE.to_owned());
    }
    names.into_iter().map(|name| (json!(name), crate::i18n::style_name(lang, &name).to_string())).collect()
}

fn footnote_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let none = Value::Null;
    let a = Attrs::new("", &none);
    let tab = if d.s("tab") == "layout" { "layout" } else { "numbering" };
    let tab_list = [("numbering", tr("Numbering and Formatting")), ("layout", tr("Layout"))];
    let styles = captioned(
        lang,
        &[
            ("arabic", "1, 2, 3, 4..."),
            ("upperRoman", "I, II, III, IV..."),
            ("lowerRoman", "i, ii, iii, iv..."),
            ("upperLetters", "A, B, C, D..."),
            ("lowerLetters", "a, b, c, d..."),
            ("arabicLeadingZero", "01, 02, 03..."),
            ("symbols", "*, †, ‡, §..."),
            ("kanji", "一, 二, 三, 四..."),
        ],
    );
    let restarts = captioned(lang, &[("never", "Never (continuous)"), ("page", "Page"), ("spread", "Spread"), ("section", "Section")]);
    let affixes = captioned(lang, &[("none", "None"), ("reference", "Footnote Reference"), ("text", "Footnote Text"), ("both", "Both")]);
    let positions = captioned(
        lang,
        &[
            ("superscript", "Apply Superscript"),
            ("subscript", "Apply Subscript"),
            ("normal", "Apply Normal"),
            ("otSuperscript", "OpenType Superscript"),
        ],
    );
    let baselines = captioned(
        lang,
        &[
            ("ascent", "Ascent"),
            ("capHeight", "Cap Height"),
            ("leading", "Leading"),
            ("xHeight", "x Height"),
            ("emboxHeight", "Em Box Height"),
            ("fixed", "Fixed"),
        ],
    );
    let char_styles = style_options(app, true);
    let para_styles = style_options(app, false);
    let swatches: Vec<(Value, String)> =
        app.session.active().map(|st| st.doc.swatches.iter().map(|s| (json!(s.name), s.name.clone())).collect()).unwrap_or_default();
    Stack::new().id_salt("footnote_options").fill_width().show(ui, |ui, _| {
        if let Some(next) = tabs(ui, "footnote", &tr("Footnote Options"), tab, &tab_list) {
            d.fields.insert("tab".into(), json!(next));
        }
        if tab == "layout" {
            GroupBox::new(tr("Spacing Options"), None).id_salt("footnote_spacing_box").show(ui, |ui, _| {
                FieldGroup::new(tr("Spacing Options")).id_salt("footnote_spacing").show(ui, |ui, _| {
                    for (key, caption) in [("spaceBefore", "Minimum Space Before First Footnote:"), ("spaceBetween", "Space Between Footnotes:")] {
                        field(ui, &tr(caption), key, |ui| text(ui, d, &a, key, &tr(caption), true));
                    }
                });
            });
            GroupBox::new(tr("First Baseline"), None).id_salt("footnote_baseline_box").show(ui, |ui, _| {
                FieldGroup::new(tr("First Baseline")).id_salt("footnote_baseline").show(ui, |ui, _| {
                    field(ui, &tr("Offset:"), "firstBaseline", |ui| choice(ui, d, &a, "firstBaseline", &tr("Offset:"), &baselines, true));
                    field(ui, &tr("Min:"), "firstBaselineMin", |ui| text(ui, d, &a, "firstBaselineMin", &tr("Min:"), true));
                });
            });
            check(ui, d, &a, "spanColumns", &tr("Span Footnotes Across Columns"), true);
            GroupBox::new(tr("Rule Above"), None).id_salt("footnote_rule_box").show(ui, |ui, _| {
                check(ui, d, &a, "rule.on", &tr("Rule On"), true);
                FieldGroup::new(tr("Rule Above")).id_salt("footnote_rule").show(ui, |ui, _| {
                    field(ui, &tr("Weight:"), "rule.weight", |ui| text(ui, d, &a, "rule.weight", &tr("Weight:"), true));
                    field(ui, &tr("Color:"), "rule.color", |ui| choice(ui, d, &a, "rule.color", &tr("Color:"), &swatches, true));
                    for (key, caption) in [("rule.width", "Width:"), ("rule.offset", "Offset:"), ("rule.leftIndent", "Left Indent:")] {
                        field(ui, &tr(caption), key, |ui| text(ui, d, &a, key, &tr(caption), true));
                    }
                });
            });
            return;
        }
        GroupBox::new(tr("Numbering"), None).id_salt("footnote_numbering_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Numbering")).id_salt("footnote_numbering").show(ui, |ui, _| {
                field(ui, &tr("Style:"), "style", |ui| choice(ui, d, &a, "style", &tr("Style:"), &styles, true));
                field(ui, &tr("Start at:"), "startAt", |ui| text(ui, d, &a, "startAt", &tr("Start at:"), true));
                field(ui, &tr("Restart Numbering Every:"), "restart", |ui| {
                    choice(ui, d, &a, "restart", &tr("Restart Numbering Every:"), &restarts, true)
                });
                field(ui, &tr("Show Prefix/Suffix in:"), "affixIn", |ui| choice(ui, d, &a, "affixIn", &tr("Show Prefix/Suffix in:"), &affixes, true));
                for (key, caption) in [("prefix", "Prefix:"), ("suffix", "Suffix:")] {
                    field(ui, &tr(caption), key, |ui| text(ui, d, &a, key, &tr(caption), true));
                }
            });
        });
        GroupBox::new(tr("Footnote Reference Number in Text"), None).id_salt("footnote_reference_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Footnote Reference Number in Text")).id_salt("footnote_reference").show(ui, |ui, _| {
                field(ui, &tr("Position:"), "refPosition", |ui| choice(ui, d, &a, "refPosition", &tr("Position:"), &positions, true));
                field(ui, &tr("Character Style:"), "refCharStyle", |ui| {
                    choice(ui, d, &a, "refCharStyle", &tr("Character Style:"), &char_styles, true)
                });
            });
        });
        GroupBox::new(tr("Footnote Formatting"), None).id_salt("footnote_formatting_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Footnote Formatting")).id_salt("footnote_formatting").show(ui, |ui, _| {
                field(ui, &tr("Paragraph Style:"), "paraStyle", |ui| choice(ui, d, &a, "paraStyle", &tr("Paragraph Style:"), &para_styles, true));
                field(ui, &tr("Separator:"), "separator", |ui| text(ui, d, &a, "separator", &tr("Separator:"), true));
            });
        });
    });
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Paragraph Rules

fn paragraph_rules(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let current = d.fields.get("current").cloned().unwrap_or_default();
    rules_form(app, ui, d, "", &current);
    None
}

/// Paragraph Rules (the dialog, and the Paragraph Style Options section): Rule Above or Rule Below, chosen at the top.
/// `current` holds the resolved `ruleAbove` and `ruleBelow`; each edit is stored as `{prefix}{rule}.{field}`, so OK changes
/// only the fields edited.
pub(crate) fn rules_form(app: &DesignApp, ui: &mut Ui, d: &mut Dialog, prefix: &str, current: &Value) {
    use designcraft_doc::StrokeType;
    let lang = app.ui.language.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let (h_units, v_units) =
        app.session.active().map(|st| (st.doc.settings.horizontal_units, st.doc.settings.vertical_units)).unwrap_or((Unit::Points, Unit::Points));
    let which = if d.s("rule") == "ruleBelow" { "ruleBelow" } else { "ruleAbove" };
    let base = current.get(which).cloned().unwrap_or(Value::Null);
    let rule_prefix = format!("{prefix}{which}.");
    let a = Attrs::new(&rule_prefix, &base);
    let on = a.get(d, "on").as_bool().unwrap_or(false);
    let kind: StrokeType = serde_json::from_value(a.get(d, "type")).unwrap_or_default();
    let doc = app.session.active().map(|st| &st.doc);
    let rules: Vec<(Value, String)> = super::RULES.iter().map(|(value, caption)| (json!(value), tr(caption))).collect();
    // The built-in types, then the document's own stroke styles.
    let mut types: Vec<StrokeType> = super::rule_types();
    types.extend(doc.map(|doc| doc.stroke_styles.iter().map(|s| StrokeType::Style { name: s.name.clone() }).collect::<Vec<_>>()).unwrap_or_default());
    let type_captions: Vec<String> = types
        .iter()
        .map(|k| match k {
            StrokeType::Style { name } => name.clone(),
            k => tr(k.label()),
        })
        .collect();
    let type_at = types
        .iter()
        .position(|k| std::mem::discriminant(k) == std::mem::discriminant(&kind) && !matches!(k, StrokeType::Style { .. }))
        .or_else(|| types.iter().position(|k| *k == kind));
    let colors = Swatches::new(doc.map(|d| &**d), Some(tr("Text Color")));
    let swatches = Swatches::new(doc.map(|d| &**d), None);
    let gaps = !matches!(kind, StrokeType::Solid);
    let gap_color = a.get(d, "gapColor").as_str().unwrap_or(designcraft_color::swatch::NONE).to_owned();
    let widths = vec![(json!(true), tr("Column")), (json!(false), tr("Text"))];
    let tint = |value: Value| value.as_f64().map(|t| (t * 100.0).round() / 100.0);
    Stack::new().id_salt(("paragraph_rules", prefix.to_owned())).fill_width().show(ui, |ui, _| {
        Row::new().id_salt("paragraph_rules_which").show(ui, |ui, _| {
            let at = rules.iter().position(|(value, _)| *value == json!(which));
            let captions: Vec<String> = rules.iter().map(|(_, caption)| caption.clone()).collect();
            if let Some(picked) = select_at(ui, "rule_which", &tr("Rule:"), &captions, at, None, true)
                && let Some((value, _)) = rules.get(picked)
            {
                d.fields.insert("rule".into(), value.clone());
            }
            check(ui, d, &a, "on", &tr("Rule On"), true);
        });
        FieldGroup::new(tr("Paragraph Rules")).id_salt(("paragraph_rules_fields", which)).show(ui, |ui, _| {
            field(ui, &tr("Weight:"), "weight", |ui| number(ui, d, &a, "weight", &tr("Weight:"), &Spec::real(0.0, 1000.0).unit("pt"), on));
            field(ui, &tr("Type:"), "type", |ui| {
                if let Some(picked) = select_at(ui, &a.field("type"), &tr("Type:"), &type_captions, type_at, None, on)
                    && let Some(kind) = types.get(picked)
                {
                    a.set(d, "type", serde_json::to_value(kind).unwrap_or_default());
                }
            });
            field(ui, &tr("Color:"), "color", |ui| {
                if let Some(picked) = swatch_choice(ui, d, &a, "color", &tr("Color:"), &colors, on) {
                    a.set(d, "color", picked);
                }
            });
            field(ui, &tr("Tint:"), "tint", |ui| {
                let shown = tint(a.get(d, "tint"));
                if let Some(Some(v)) = number_edit(ui, &a.field("tint"), &tr("Tint:"), shown, &Spec::whole(0.0, 100.0).scaled(100.0).unit("%"), on) {
                    a.set(d, "tint", json!(v.clamp(0.0, 1.0)));
                }
            });
            // A solid rule has no gaps.
            field(ui, &tr("Gap Color:"), "gapColor", |ui| {
                if let Some(picked) = swatch_choice(ui, d, &a, "gapColor", &tr("Gap Color:"), &swatches, on && gaps) {
                    a.set(d, "gapColor", picked);
                }
            });
            field(ui, &tr("Gap Tint:"), "gapTint", |ui| {
                // A full gap tint is left out of the rule's JSON: it reads 100%.
                let shown = Some(a.get(d, "gapTint").as_f64().unwrap_or(1.0));
                let enabled = on && gaps && gap_color != designcraft_color::swatch::NONE;
                if let Some(Some(v)) =
                    number_edit(ui, &a.field("gapTint"), &tr("Gap Tint:"), shown, &Spec::whole(0.0, 100.0).scaled(100.0).unit("%"), enabled)
                {
                    a.set(d, "gapTint", json!(v.clamp(0.0, 1.0)));
                }
            });
            field(ui, &tr("Width:"), "columnWidth", |ui| {
                let column = a.get(d, "columnWidth").as_bool().unwrap_or(true);
                let at = Some(usize::from(!column));
                let captions: Vec<String> = widths.iter().map(|(_, caption)| caption.clone()).collect();
                if let Some(picked) = select_at(ui, &a.field("columnWidth"), &tr("Width:"), &captions, at, None, on) {
                    a.set(d, "columnWidth", json!(picked == 0));
                }
            });
            field(ui, &tr("Offset:"), "offset", |ui| measure(ui, d, &a, "offset", &tr("Offset:"), v_units, on));
            for (caption, key) in [("Left Indent:", "leftIndent"), ("Right Indent:", "rightIndent")] {
                field(ui, &tr(caption), key, |ui| measure(ui, d, &a, key, &tr(caption), h_units, on));
            }
        });
        if which == "ruleAbove" {
            check(ui, d, &a, "keepInFrame", &tr("Keep In Frame"), on);
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Justification

fn justification(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let current = d.fields.get("current").cloned().unwrap_or_default();
    justification_form(&app.ui.language, ui, d, "", &current);
    None
}

/// The Justification controls (the Justification dialog, and the Paragraph Style Options section). `current` holds the
/// resolved paragraph attributes, `null` where the selected paragraphs differ (the field shows empty); each edit is stored
/// as `{prefix}{attribute}`, so OK changes only the fields edited.
pub(crate) fn justification_form(lang: &str, ui: &mut Ui, d: &mut Dialog, prefix: &str, current: &Value) {
    let a = Attrs::new(prefix, current);
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let single_words: Vec<(Value, String)> = super::SINGLE_WORD_CHOICES.iter().map(|(value, caption)| (json!(value), tr(caption))).collect();
    let (single, [paragraph, single_line]) = crate::panels::properties::composer_choice(a.get(d, "composer").as_str());
    let composers = [(paragraph, tr("Paragraph Composer")), (single_line, tr("Single-line Composer"))];
    Stack::new().id_salt(("justification_form", prefix.to_owned())).fill_width().show(ui, |ui, _| {
        for (label, stem, (least, most)) in super::JUSTIFY_ROWS {
            GroupBox::new(tr(label), None).id_salt(("justification_group", stem)).show(ui, |ui, _| {
                FieldGroup::new(tr(label)).id_salt(("justification_fields", stem)).show(ui, |ui, _| {
                    for (suffix, caption) in [("Min", "Minimum"), ("Desired", "Desired"), ("Max", "Maximum")] {
                        let key = format!("{stem}{suffix}");
                        let name = format!("{}:", tr(caption));
                        let spec = Spec::whole(least, most).scaled(100.0).unit("%");
                        field(ui, &name, &key, |ui| number(ui, d, &a, &key, &name, &spec, true));
                    }
                });
            });
        }
        FieldGroup::new(tr("Justification")).id_salt(("justification_options", prefix.to_owned())).show(ui, |ui, _| {
            field(ui, &tr("Auto Leading:"), "autoLeading", |ui| {
                let spec = Spec::whole(super::AUTO_LEADING_RANGE.0, super::AUTO_LEADING_RANGE.1).scaled(100.0).unit("%");
                number(ui, d, &a, "autoLeading", &tr("Auto Leading:"), &spec, true);
            });
            field(ui, &tr("Single Word Justification:"), "singleWordJustify", |ui| {
                // Mixed (null) shows blank until a choice is made.
                if let Some(picked) = select_attr(ui, d, &a, "singleWordJustify", &tr("Single Word Justification:"), &single_words, true) {
                    // The paragraphs' own choice is no edit, so OK adds no undo step for it.
                    let key = a.field("singleWordJustify");
                    if current["singleWordJustify"] == picked {
                        d.fields.remove(&key);
                    } else {
                        d.fields.insert(key, picked);
                    }
                }
            });
            field(ui, &tr("Composer:"), "composer", |ui| {
                // Mixed (null) selects neither; a Japanese composer keeps its kind.
                let known = !a.get(d, "composer").is_null();
                let entries: Vec<Entry<String>> = composers.iter().map(|(id, caption)| Entry::new((*id).to_owned(), caption.clone())).collect();
                let value = if known { Mixed::Same(composers[usize::from(single)].0.to_owned()) } else { Mixed::Mixed };
                let out =
                    Segmented::new(tr("Composer:"), Bind::given(value), &entries).id_salt(("justification_composer", prefix.to_owned())).show(ui);
                if let Some(next) = out.events.committed().cloned() {
                    a.set(d, "composer", json!(next));
                }
            });
        });
        for (caption, key, label, default) in [
            ("Insert Kashidas:", "kashidas", "In justified Arabic text", true),
            ("Korean Line Breaks:", "koreanCharBreaks", "Between syllables (not only at spaces)", false),
        ] {
            Row::new().id_salt(("justification_flag", key)).show(ui, |ui, _| {
                Text::new(tr(caption)).show(ui);
                check_mixed(ui, d, &a, key, &tr(label), default, true);
            });
        }
        if let Some(error) = d.fields.get("justifyError").and_then(Value::as_str).map(str::to_owned) {
            message(ui, &error, true);
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Hyphenation Settings

fn hyphenation(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let current = d.fields.get("current").cloned().unwrap_or_default();
    let units = app.session.active().map_or(Unit::Picas, |st| st.doc.settings.horizontal_units);
    let lang = app.ui.language.clone();
    let none = Value::Null;
    let preview = Attrs::new("", &none);
    Stack::new().id_salt("hyphenation").fill_width().show(ui, |ui, _| {
        Row::new().id_salt("hyphenation_body").fill_width().show(ui, |ui, _| {
            Stack::new().id_salt("hyphenation_options").show(ui, |ui, _| hyphenation_form(&lang, ui, d, "", &current, units));
            check(ui, d, &preview, "preview", &crate::i18n::tr(&lang, "Preview"), true);
        });
        super::hyphenation_preview(app, d);
        if let Some(status) = d.fields.get("status").and_then(Value::as_str).map(str::to_owned) {
            message(ui, &status, true);
        }
    });
    None
}

/// Hyphenation Settings: Hyphenate; Words with at Least, After First and Before Last (letters), Hyphen Limit (hyphens) and
/// Hyphenation Zone; the Better Spacing to Fewer Hyphens slider (0 to 10, as InDesign's hyphen weight); Hyphenate
/// Capitalized Words, Last Word and Across Column. Edited fields are stored as `{prefix}{key}`; the others show `current` (a
/// mixed value is `null` and shows blank). The Hyphenation dialog and Paragraph Style Options › Hyphenation both use it.
pub(crate) fn hyphenation_form(lang: &str, ui: &mut Ui, d: &mut Dialog, prefix: &str, current: &Value, units: Unit) {
    let a = Attrs::new(prefix, current);
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    Stack::new().id_salt(("hyphenation_form", prefix.to_owned())).show(ui, |ui, _| {
        check_mixed(ui, d, &a, "hyphenate", &tr("Hyphenate"), false, true);
        FieldGroup::new(tr("Hyphenation Settings")).id_salt(("hyphenation_fields", prefix.to_owned())).show(ui, |ui, _| {
            // InDesign's ranges.
            for (caption, key, unit, most) in [
                ("Words with at Least:", "hyphMinWord", "letters", 25.0),
                ("After First:", "hyphAfterFirst", "letters", 15.0),
                ("Before Last:", "hyphBeforeLast", "letters", 15.0),
                ("Hyphen Limit:", "hyphLimit", "hyphens", 25.0),
            ] {
                let least = if key == "hyphLimit" { 0.0 } else { 1.0 };
                field(ui, &tr(caption), key, |ui| number(ui, d, &a, key, &tr(caption), &Spec::whole(least, most).unit(tr(unit)), true));
            }
            field(ui, &tr("Hyphenation Zone:"), "hyphZone", |ui| {
                let shown = a.get(d, "hyphZone").as_f64().filter(|v| v.is_finite());
                if let Some(points) = measure_edit(ui, &a.field("hyphZone"), &tr("Hyphenation Zone:"), shown, units, (0.0, 8640.0), true) {
                    a.set(d, "hyphZone", json!(points));
                }
            });
        });
        // Better Spacing to Fewer Hyphens, in InDesign's eleven steps. Mixed across the selection: no knob until a click or
        // a drag sets every paragraph to that step.
        Row::new().id_salt(("hyphenation_weight", prefix.to_owned())).show(ui, |ui, _| {
            Text::new(tr("Better Spacing")).show(ui);
            let weight = a.get(d, "hyphWeight").as_f64();
            let step = weight.map(|w| (w * 10.0).round().clamp(0.0, 10.0));
            let value = step.map_or(Mixed::Mixed, Mixed::Same);
            let out = Slider::new(Bind::given(value), 0.0..=10.0)
                .step(1.0)
                .ticks(10)
                .label(tr("Hyphenation Weight"))
                .id_salt(("hyphenation_slider", prefix.to_owned()))
                .show(ui);
            if let Some(next) = out.events.landed().copied().filter(|next| Some(*next) != step) {
                a.set(d, "hyphWeight", json!(next / 10.0));
            }
            Text::new(tr("Fewer Hyphens")).show(ui);
        });
        Row::new().id_salt(("hyphenation_flags", prefix.to_owned())).show(ui, |ui, _| {
            check_mixed(ui, d, &a, "hyphCapitalized", &tr("Hyphenate Capitalized Words"), false, true);
            check_mixed(ui, d, &a, "hyphLastWord", &tr("Hyphenate Last Word"), false, true);
        });
        check_mixed(ui, d, &a, "hyphAcrossColumn", &tr("Hyphenate Across Column"), false, true);
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// New Cross-Reference

fn insert_xref(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    let tr = |s: &str| crate::i18n::tr(lang, s).to_string();
    let doc = app.session.active()?.doc.clone();
    let all = "[All Paragraphs]";
    let none = Value::Null;
    let a = Attrs::new("", &none);
    let links = captioned(lang, &[("paragraph", "Paragraph"), ("anchor", "Text Anchor")]);
    let anchors: Vec<(String, String)> =
        doc.stories.values().flat_map(|st| st.anchors.iter().map(|anchor| (format!("a:{}", anchor.id), anchor.name.clone()))).collect();
    let mut styles: Vec<String> = vec![all.to_owned()];
    styles.extend(doc.styles.paragraph.iter().map(|s| s.name.clone()));
    let cur_style = if d.s("style").is_empty() { all.to_owned() } else { d.s("style") };
    // The paragraphs of the chosen style: their destination keys and the text shown for each.
    let mut paragraphs: Vec<(String, String)> = Vec::new();
    for st in doc.stories.values().filter(|s| !s.frames.is_empty()) {
        for (pi, r) in st.para_ranges().into_iter().enumerate() {
            if cur_style != all && st.paras[pi].style != cur_style {
                continue;
            }
            let text = designcraft_doc::xref::clean(&st.text[r]);
            if text.is_empty() {
                continue;
            }
            paragraphs.push((format!("{}:{pi}", st.id.0), text.chars().take(52).collect()));
        }
    }
    let formats: Vec<(Value, String)> = doc.xref_formats.iter().map(|f| (json!(f.name), f.name.clone())).collect();
    if d.s("format").is_empty() {
        d.fields.insert("format".into(), json!(doc.xref_formats.first().map(|f| f.name.clone()).unwrap_or_default()));
    }
    Stack::new().id_salt("insert_xref").fill_width().show(ui, |ui, _| {
        field(ui, &tr("Link To:"), "linkTo", |ui| choice(ui, d, &a, "linkTo", &tr("Link To:"), &links, true));
        if d.s("linkTo") == "anchor" {
            if anchors.is_empty() {
                message(ui, &tr("No text anchors in this document."), false);
            } else {
                let captions: Vec<String> = anchors.iter().map(|(_, name)| name.clone()).collect();
                let at = anchors.iter().position(|(key, _)| *key == d.s("target"));
                if let Some(picked) = pick_list(ui, "xref_anchors", &tr("Text Anchors"), &captions, at, 10) {
                    d.fields.insert("target".into(), json!(anchors[picked].0));
                }
            }
        } else {
            Row::new().id_salt("xref_paragraphs").fill_width().show(ui, |ui, _| {
                let at = styles.iter().position(|name| *name == cur_style);
                if let Some(picked) = pick_list(ui, "xref_styles", &tr("Paragraph Styles"), &styles, at, 10) {
                    d.fields.insert("style".into(), json!(styles[picked]));
                }
                if paragraphs.is_empty() {
                    return;
                }
                let captions: Vec<String> = paragraphs.iter().map(|(_, text)| text.clone()).collect();
                let at = paragraphs.iter().position(|(key, _)| *key == d.s("target"));
                if let Some(picked) = pick_list(ui, "xref_paragraph_list", &tr("Paragraphs"), &captions, at, 10) {
                    d.fields.insert("target".into(), json!(paragraphs[picked].0));
                }
            });
        }
        GroupBox::new(tr("Cross-Reference Format"), None).id_salt("xref_format_box").show(ui, |ui, _| {
            FieldGroup::new(tr("Cross-Reference Format")).id_salt("xref_format_fields").show(ui, |ui, _| {
                field(ui, &tr("Format:"), "format", |ui| choice(ui, d, &a, "format", &tr("Format:"), &formats, true));
            });
        });
        if let Some(status) = d.fields.get("status").and_then(Value::as_str).map(str::to_owned) {
            message(ui, &status, false);
        }
    });
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::Harness;

    /// The resolved Words with at Least of the paragraph `at` of the story.
    fn min_word(h: &Harness<'_, DesignApp>, story: u64, at: usize) -> u32 {
        let doc = &h.state().session.doc().unwrap().doc;
        doc.styles.resolve_para(&doc.story(designcraft_doc::StoryId(story)).unwrap().paras[at]).0.hyph_min_word
    }

    /// Guards the Justification port: a percent typed in the framework form is stored as the fraction the attribute holds,
    /// the paragraphs' own Single Word Justification is no edit, and only OK runs `type.para`, once, with the fields edited.
    #[test]
    fn justification_form_stores_edits_and_ok_applies_them_once() {
        let (mut app, story) = kit::two_paragraphs();
        app.run("app.justificationDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let undo = kit::undo_steps(&h);
        // Word Spacing comes first, then Letter and Glyph Scaling.
        kit::type_and_commit(&mut h, "Maximum:", 0, "150");
        assert_eq!(kit::draft(&h).n("wordSpaceMax"), Some(1.5));
        kit::pick(&mut h, "Single Word Justification:", "Align Center");
        assert_eq!(kit::draft(&h).s("singleWordJustify"), "center");
        kit::pick(&mut h, "Single Word Justification:", "Full Justify");
        assert!(!kit::draft(&h).fields.contains_key("singleWordJustify"), "the paragraph's own choice is no edit");
        assert!(kit::journal(&h, "type.para").len() == 1, "only the setup ran a paragraph command");
        assert_eq!(kit::undo_steps(&h), undo);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let commands = kit::journal(&h, "type.para");
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[1].1, json!({"attrs": {"wordSpaceMax": 1.5}}));
        assert_eq!(kit::undo_steps(&h), undo + 1, "one undo step");
        assert_eq!(kit::para_attr(&mut h, story, 0, "wordSpaceMax"), json!(1.5));
        assert_eq!(kit::para_attr(&mut h, story, 0, "wordSpaceMin"), json!(0.7), "the minimum stays");
    }

    /// Guards Cancel on the Justification port, and the refusal of a minimum above the desired value: the dialog stays open
    /// with its reason shown and nothing changes.
    #[test]
    fn justification_form_cancel_and_refusal_change_nothing() {
        let (mut app, _) = kit::two_paragraphs();
        app.run("app.justificationDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let before = h.state().session.doc().unwrap().doc.clone();
        let undo = kit::undo_steps(&h);
        kit::type_and_commit(&mut h, "Minimum:", 1, "200");
        assert_eq!(kit::draft(&h).n("letterSpaceMin"), Some(2.0));
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_some(), "a letter spacing minimum above its desired value is refused");
        assert!(!kit::draft(&h).s("justifyError").is_empty());
        assert_eq!(kit::undo_steps(&h), undo);
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.doc().unwrap().doc, before);
    }

    /// Guards the Hyphenation port and its live preview: Preview shows the typed setting on the page without an undo step,
    /// Cancel puts the paragraph back, and OK sets it once, as one undo step.
    #[test]
    fn hyphenation_form_previews_cancels_and_applies_once() {
        let (mut app, story) = kit::two_paragraphs();
        app.run("app.hyphenationDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let before = h.state().session.doc().unwrap().doc.clone();
        let undo = kit::undo_steps(&h);
        assert_eq!(min_word(&h, story, 0), 5);
        kit::type_and_commit(&mut h, "Words with at Least:", 0, "7");
        assert_eq!(kit::draft(&h).n("hyphMinWord"), Some(7.0));
        assert_eq!(min_word(&h, story, 0), 5, "nothing shows before Preview is on");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        assert_eq!(min_word(&h, story, 0), 7, "Preview shows the setting");
        assert_eq!(kit::undo_steps(&h), undo, "a preview is no undo step");
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.doc().unwrap().doc, before, "Cancel puts the paragraph back");

        let (mut app, story) = kit::two_paragraphs();
        app.run("app.hyphenationDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let undo = kit::undo_steps(&h);
        kit::type_and_commit(&mut h, "Words with at Least:", 0, "7");
        kit::click(&mut h, &[Role::CheckBox], "Preview", 0);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(min_word(&h, story, 0), 7);
        assert_eq!(min_word(&h, story, 1), 5, "only the selected paragraph");
        assert_eq!(kit::undo_steps(&h), undo + 1, "one OK is one undo step");
    }

    /// Guards Paragraph Rules: the form shows the rule the dialog opened on, and OK sends only the fields edited, for that
    /// rule, in one command.
    #[test]
    fn paragraph_rules_form_applies_only_the_edited_fields() {
        let (mut app, story) = kit::two_paragraphs();
        let rule = json!({"on": true, "weight": 3.0, "color": "[Black]", "tint": 0.5, "columnWidth": false, "offset": 4.0, "leftIndent": 6.0, "rightIndent": 2.0});
        app.run("type.para", json!({"attrs": {"ruleBelow": rule}})).unwrap();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        app.run("app.paragraphRulesDialog", json!({"rule": "ruleBelow"})).unwrap();
        let mut h = kit::harness(app);
        let undo = kit::undo_steps(&h);
        assert!(!kit::disabled(&h, &[Role::SpinButton], "Weight:", 0), "the rule is on");
        kit::type_and_commit(&mut h, "Weight:", 0, "5");
        kit::pick(&mut h, "Color:", "[Registration]");
        assert_eq!(kit::undo_steps(&h), undo);
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let commands = kit::journal(&h, "type.para");
        assert_eq!(commands.last().unwrap().1, json!({"attrs": {"ruleBelow": {"weight": 5.0, "color": "[Registration]"}}}));
        assert_eq!(kit::undo_steps(&h), undo + 1, "one undo step");
        let below = kit::para_attr(&mut h, story, 0, "ruleBelow");
        assert_eq!((below["weight"].as_f64(), below["color"].as_str(), below["offset"].as_f64()), (Some(5.0), Some("[Registration]"), Some(4.0)));
    }

    /// Guards Footnote Options: both tabs edit the draft the document's options came in, and OK runs `footnote.options` once
    /// with the edits, which change the document and undo as one step.
    #[test]
    fn footnote_options_form_edits_both_tabs_and_ok_applies_once() {
        let mut app = kit::app(1, false);
        app.run("app.footnoteOptionsDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let undo = kit::undo_steps(&h);
        kit::type_into(&mut h, "Prefix:", 0, "[");
        kit::pick(&mut h, "Restart Numbering Every:", "Page");
        assert_eq!((kit::draft(&h).s("prefix"), kit::draft(&h).s("restart")), ("[".to_owned(), "page".to_owned()));
        kit::click(&mut h, &[Role::RadioButton, Role::Button, Role::Tab], "Layout", 0);
        assert_eq!(kit::draft(&h).s("tab"), "layout");
        kit::type_into(&mut h, "Space Between Footnotes:", 0, "6");
        kit::click(&mut h, &[Role::CheckBox], "Span Footnotes Across Columns", 0);
        assert!(kit::draft(&h).b("spanColumns"));
        assert!(kit::journal(&h, "footnote.options").is_empty(), "the fields only edit the draft");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let commands = kit::journal(&h, "footnote.options");
        assert_eq!(commands.len(), 1);
        let params = &commands[0].1;
        assert_eq!((params["prefix"].as_str(), params["restart"].as_str(), params["spanColumns"].as_bool()), (Some("["), Some("page"), Some(true)));
        assert_eq!(params["spaceBetween"].as_f64(), Some(6.0));
        let options = &h.state().session.doc().unwrap().doc.footnote_options;
        assert_eq!(options.prefix, "[");
        assert_eq!(kit::undo_steps(&h), undo + 1, "one undo step");
    }

    /// Guards New Cross-Reference: the destination is picked from the framework lists, OK runs `xref.insert` with it, and
    /// OK without one keeps the dialog open with the reason.
    #[test]
    fn insert_xref_form_picks_a_paragraph_and_ok_inserts_the_reference() {
        let (mut app, story) = kit::two_paragraphs();
        app.run("app.insertXrefDialog", json!({})).unwrap();
        let mut h = kit::harness(app);
        let undo = kit::undo_steps(&h);
        assert!(!kit::draft(&h).s("format").is_empty(), "the first format is chosen");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_some(), "no destination chosen");
        assert!(!kit::draft(&h).s("status").is_empty());
        assert_eq!(kit::undo_steps(&h), undo);
        kit::click_first(&mut h, "Two");
        assert_eq!(kit::draft(&h).s("target"), format!("{story}:1"));
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let commands = kit::journal(&h, "xref.insert");
        assert_eq!(commands.len(), 1);
        assert_eq!((commands[0].1["story"].as_u64(), commands[0].1["para"].as_u64()), (Some(story), Some(1)));
        assert_eq!(kit::undo_steps(&h), undo + 1, "one undo step");
    }
}
