//! Controls over a style dialog's draft, on the craft framework: each returns the value the user committed in this
//! pass, and the caller writes it to the draft field it edits. Measures are kept in points and shown in the document's
//! unit, so a measure keeps its typed text in a `_text.<key>` draft field until it is committed.

use crate::dialogs::Dialog;
use craft_behavior::{EditEvent, Mixed};
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::Checkbox;
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::Field;
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::primitives::ContentPainterCx;
use craft_ui_widgets::runtime::env::UnitKind;
use craft_ui_widgets::style::TypeRole;
use craft_ui_widgets::text::{NumberField, TextField};
use designcraft_geom::{Unit, format_measure, units::parse_measure};
use egui::Ui;
use serde_json::{Value, json};

/// What a number field counts after its value.
#[derive(Clone, Debug)]
pub(crate) enum Counts {
    /// Nothing.
    Plain,
    /// A unit the framework knows, which typed text may also spell.
    Kind(UnitKind),
    /// A word after the number ("letters", "lines").
    Word(String),
}

/// `body` under the side label `label`, which names it.
pub(crate) fn field(ui: &mut Ui, label: &str, salt: &str, mut body: impl FnMut(&mut Ui)) {
    Field::new(label.to_owned()).id_salt(("style_field", salt.to_owned())).show(ui, |ui, _| body(ui));
}

/// `options` captioned in the interface language, as (value, caption).
pub(crate) fn captioned(lang: &str, options: &[(&str, &str)]) -> Vec<(String, String)> {
    options.iter().map(|(value, caption)| ((*value).to_owned(), crate::i18n::tr(lang, caption).to_string())).collect()
}

/// A bold heading over a group of controls.
pub(crate) fn heading(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).role(TypeRole::BodyStrong).show(ui);
}

/// A line of muted text.
pub(crate) fn note(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).shade(Shade::Muted).show(ui);
}

/// A line of text.
pub(crate) fn line(ui: &mut Ui, text: &str) {
    Text::new(text.to_owned()).show(ui);
}

/// A push button; whether it was clicked.
pub(crate) fn push(ui: &mut Ui, salt: &str, label: &str, enabled: bool) -> bool {
    Button::new(label.to_owned()).id_salt(("style_button", salt.to_owned())).enabled(enabled).show(ui).clicked()
}

/// A menu over `options` (value, caption); the value picked. A `current` none of them has shows as the placeholder.
pub(crate) fn pick(ui: &mut Ui, salt: &str, label: &str, current: &str, options: &[(String, String)], enabled: bool) -> Option<String> {
    let entries: Vec<Entry<String>> = options.iter().map(|(value, caption)| Entry::new(value.clone(), caption.clone())).collect();
    let mut menu = Select::new(label.to_owned(), Bind::given(current.to_owned()), &entries).id_salt(("style_pick", salt.to_owned())).enabled(enabled);
    if !options.iter().any(|(value, _)| value == current) {
        menu = menu.placeholder(current.to_owned());
    }
    menu.show(ui).events.committed().cloned().filter(|next| next != current)
}

/// A check box over `value`, `None` for a value that differs across what the dialog edits (it shows mixed, and a
/// click turns it on); the state clicked to.
pub(crate) fn toggle(ui: &mut Ui, salt: &str, label: &str, value: Option<bool>, enabled: bool) -> Option<bool> {
    let shown = value.map_or(Mixed::Mixed, Mixed::Same);
    let out = Checkbox::new(Bind::given(shown)).label(label.to_owned()).id_salt(("style_toggle", salt.to_owned())).enabled(enabled).show(ui);
    out.events.committed().copied().filter(|next| Some(*next) != value)
}

/// What a number field accepts and shows.
#[derive(Clone, Debug)]
pub(crate) struct Num {
    /// The least and the most.
    pub(crate) range: (f64, f64),
    /// Whole numbers only.
    pub(crate) whole: bool,
    /// What follows the number.
    pub(crate) counts: Counts,
}

impl Num {
    /// Whole numbers from `least` to `most`.
    pub(crate) fn whole(least: f64, most: f64) -> Self {
        Num { range: (least, most), whole: true, counts: Counts::Plain }
    }

    /// Numbers from `least` to `most`.
    pub(crate) fn decimal(least: f64, most: f64) -> Self {
        Num { range: (least, most), whole: false, counts: Counts::Plain }
    }

    /// The same with `counts` after the number.
    pub(crate) fn counting(mut self, counts: Counts) -> Self {
        self.counts = counts;
        self
    }
}

/// A number field; `Some(value)` when one was committed (`Some(None)` for an emptied field).
pub(crate) fn number(ui: &mut Ui, salt: &str, label: &str, value: Option<f64>, num: &Num, enabled: bool) -> Option<Option<f64>> {
    let mut input = NumberField::new(label.to_owned(), Bind::given(value.filter(|v| v.is_finite())))
        .id_salt(("style_number", salt.to_owned()))
        .range(num.range.0..=num.range.1)
        .step(1.0)
        .enabled(enabled);
    if num.whole {
        input = input.snap_typed(true).fixed_decimals(0).commit_decimals(0);
    }
    input = match &num.counts {
        Counts::Plain => input,
        Counts::Kind(kind) => input.unit_kind(*kind),
        Counts::Word(word) => input.unit(word.clone()),
    };
    input.show(ui).edit.events.committed().copied()
}

/// A whole number kept in `range` (a count of lines, letters, columns); the value committed.
pub(crate) fn count(ui: &mut Ui, salt: &str, label: &str, value: Option<f64>, range: (u32, u32), counts: &Counts, enabled: bool) -> Option<u32> {
    let num = Num::whole(f64::from(range.0), f64::from(range.1)).counting(counts.clone());
    let next = number(ui, salt, label, value, &num, enabled)?;
    Some(next.filter(|v| v.is_finite()).map_or(range.0, |v| v.round().clamp(f64::from(range.0), f64::from(range.1)) as u32))
}

/// A ratio shown as whole percents, kept in `range` (as ratios); the ratio committed.
pub(crate) fn percent(ui: &mut Ui, salt: &str, label: &str, ratio: Option<f64>, range: (f64, f64), enabled: bool) -> Option<Option<f64>> {
    let shown = ratio.map(|r| (r * 100.0).round());
    let num = Num::whole(range.0 * 100.0, range.1 * 100.0).counting(Counts::Kind(UnitKind::Percent));
    let next = number(ui, salt, label, shown, &num, enabled)?;
    Some(next.map(|p| p / 100.0))
}

/// A text field over the string draft field `key`, every edit written as it is typed.
pub(crate) fn text(ui: &mut Ui, d: &mut Dialog, key: &str, salt: &str, label: &str, enabled: bool) {
    let current = d.s(key);
    let out = TextField::new(label.to_owned(), Bind::given(current.clone())).id_salt(("style_text", salt.to_owned())).limit(None::<usize>).enabled(enabled).show(ui);
    if let Some(next) = out.events.landed().filter(|next| **next != current).cloned() {
        d.fields.insert(key.into(), json!(next));
    }
}

/// A short text that is committed, not typed through: its value when committed.
pub(crate) fn committed_text(ui: &mut Ui, salt: &str, label: &str, current: &str, enabled: bool) -> Option<String> {
    let out = TextField::new(label.to_owned(), Bind::given(current.to_owned())).id_salt(("style_text_commit", salt.to_owned())).limit(None::<usize>).enabled(enabled).show(ui);
    out.events.committed().cloned().filter(|next| next != current)
}

/// A measure in points shown in `unit`: the committed text is worked out (any unit spelling, typed arithmetic) and
/// returned in points. The text being typed waits in the `_text.<salt>` draft field.
pub(crate) fn measure(ui: &mut Ui, d: &mut Dialog, salt: &str, label: &str, points: Option<f64>, unit: Unit, enabled: bool) -> Option<f64> {
    let draft = format!("_text.{salt}");
    let shown = d.fields.get(&draft).and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| points.filter(|v| v.is_finite()).map(|v| format_measure(v, unit)).unwrap_or_default());
    let out = TextField::new(label.to_owned(), Bind::given(shown.clone())).id_salt(("style_measure", salt.to_owned())).limit(None::<usize>).enabled(enabled).show(ui);
    if let Some(next) = out.events.landed().filter(|next| **next != shown).cloned() {
        d.fields.insert(draft.clone(), json!(next));
    }
    match out.events.ended() {
        Some(EditEvent::Commit { after, .. } | EditEvent::Set { after, .. }) => {
            d.fields.remove(&draft);
            parse_measure(after, unit).ok().filter(|v| v.is_finite())
        }
        Some(EditEvent::Cancel { .. }) => {
            d.fields.remove(&draft);
            None
        }
        _ => None,
    }
}

/// What a swatch menu offers besides the document's swatches.
#[derive(Default)]
pub(crate) struct Extra<'a> {
    /// The entry for the text's own colour: its value and caption.
    pub(crate) text: Option<(&'a str, &'a str)>,
    /// A blank entry for an attribute that is not set: its value and caption.
    pub(crate) blank: Option<(&'a str, &'a str)>,
}

/// A menu of the document's swatches (hidden ones left out), each with its colour chip, after the entries `extra`
/// names. The value picked.
pub(crate) fn swatch_pick(ui: &mut Ui, salt: &str, label: &str, doc: &designcraft_doc::Document, current: &str, extra: &Extra<'_>, enabled: bool) -> Option<String> {
    let mut entries: Vec<Entry<String>> = Vec::new();
    for (value, caption) in extra.blank.iter().chain(extra.text.iter()) {
        entries.push(Entry::new((*value).to_owned(), (*caption).to_owned()));
    }
    entries.extend(doc.swatches.iter().filter(|w| !w.hidden).map(|w| Entry::new(w.name.clone(), w.name.clone())));
    let chips: Vec<(String, _)> =
        doc.swatches.iter().filter(|w| !w.hidden).map(|w| (w.name.clone(), crate::widgets::swatch_colors(doc, &w.name, 1.0))).collect();
    let mut menu = Select::new(label.to_owned(), Bind::given(current.to_owned()), &entries)
        .id_salt(("style_swatch", salt.to_owned()))
        .enabled(enabled)
        .picture(move |name: &String, cx: &ContentPainterCx, rect: egui::Rect| {
            if let Some((_, (color, gradient))) = chips.iter().find(|(n, _)| n == name) {
                crate::widgets::paint_chip(&cx.painter, rect, color.clone(), gradient.clone());
            }
        });
    if !entries.iter().any(|e| e.value().is_some_and(|v| v == current)) {
        menu = menu.placeholder(current.to_owned());
    }
    menu.show(ui).events.committed().cloned().filter(|next| next != current)
}
