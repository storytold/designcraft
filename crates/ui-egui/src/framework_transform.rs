//! The craft UI framework pieces the Properties panel and its small panels share: the runtime's install, the app's
//! own artwork and lettering as framework glyphs, the number, length, choice, swatch and font fields their rows are
//! made of, and Properties › Transform's X/Y/W/H rows.
//!
//! A length is kept in points and shown in the document's ruler unit. Where the framework names that unit, the field
//! is its `NumberField` over the unit (its own range, step and keys); where it does not — picas, which it writes as
//! decimal picas instead of the app's `1p6`, and ciceros, agates, Q, Ha and pixels, which it has no kind for — the
//! field is the framework's `TextField` over the app's own spelling of a measure ([`MeasureCodec`]), so the person
//! sees and types exactly what the rest of the app writes and nothing is converted behind their back. Either way
//! a commit (Enter, Tab, focus loss, a step key on a number field) is one engine command and one undo step; live
//! updates are not applied, so the document does not move while a value is being typed.
//!
//! The framework's runtime must be installed on the context once, outside a pass, before its first one
//! ([`install`], called from the desktop shell's creation context). Without it the panel draws its native body.

use crate::DesignApp;
use crate::panels::SelInfo;
use craft_behavior::{Codec, IconId, Mixed, NotAValue};
use craft_theme::Srgba;
use craft_ui_widgets::buttons::{Button, ButtonOutput, IconButton};
use craft_ui_widgets::choice::Checkbox;
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _, Trigger};
use craft_ui_widgets::display::{Divider, Icon, Text};
use craft_ui_widgets::menu::{Menu, MenuItem};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::primitives::{Glyph, IconSpec};
use craft_ui_widgets::runtime::env::{LengthUnits, UnitKind};
use craft_ui_widgets::style::TypeRole;
use craft_ui_widgets::style::UiTokens;
use craft_ui_widgets::text::{NumberField, NumberPick, TextField};
use designcraft_geom::Unit;
use designcraft_geom::units::{format_measure, parse_measure};
use egui::{Rect, pos2, vec2};
use serde_json::{Map, Value, json};
use std::ops::RangeInclusive;

/// Whether the object sections are the framework's: always on the desktop, where the framework is the UI.
pub(crate) fn enabled() -> bool {
    true
}

/// Installs the framework's runtime on `ctx`, once, before its first pass.
///
/// It refuses to install inside a pass ([`craft_ui_widgets::InstallError::DuringPass`]), so a host calls this
/// where it made the context: the desktop shell's eframe creation context, or a test's own context. A
/// second call on the same context is not an error.
pub(crate) fn install(ctx: &egui::Context) {
    let spec = craft_ui_widgets::InstallSpec::new(craft_host::target::ShellInfo::default(), std::sync::Arc::new(craft_window_sdk::system::NoWindows));
    match craft_ui_widgets::install(ctx, spec) {
        Ok(()) | Err(craft_ui_widgets::InstallError::AlreadyInstalled) => {}
        Err(error) => log::warn!("the craft UI runtime was not installed; the Properties panel keeps its native controls: {error}"),
    }
}

/// The app's own artwork for the icon `name` (`crate::icons`), as a framework glyph: the panel's buttons and icons
/// keep the drawings the app has always shown.
#[derive(craft_ui::contract::MemoProps)]
pub(crate) struct Artwork(pub(crate) &'static str);

impl Glyph for Artwork {
    fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, color: egui::Color32, _mirrored: bool) {
        crate::icons::paint(painter, rect, self.0, color);
    }
}

/// An icon button at `at` showing the app's artwork `icon`, named and tipped `label`, in its on look while
/// `selected`: whether it was clicked this pass.
pub(crate) fn icon_button(ui: &mut egui::Ui, at: Rect, id: egui::Id, icon: &'static str, label: &str, selected: bool) -> bool {
    IconButton::new(IconSpec::glyph(&Artwork(icon)))
        .id(id)
        .label(label.to_owned())
        .tooltip(label.to_owned())
        .selected(selected)
        .show_at(ui, at)
        .clicked()
}

/// The app's artwork `icon` at `at`, named `label` (its tooltip) when it has one.
pub(crate) fn icon(ui: &mut egui::Ui, at: Rect, icon: &'static str, label: Option<&str>, enabled: bool) {
    let artwork = Artwork(icon);
    let shown = Icon::new(IconSpec::glyph(&artwork)).enabled(enabled);
    match label {
        Some(label) => shown.label(label.to_owned()).show_at(ui, at),
        None => shown.show_at(ui, at),
    };
}

/// The `•••` button at the end of its own row under a panel section, with the id `id`: whether it was clicked this
/// pass.
pub(crate) fn more_options(ui: &mut egui::Ui, language: &str, id: egui::Id) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), egui::Sense::hover());
    let at = Rect::from_min_max(pos2(row.max.x - 26.0, row.min.y), pos2(row.max.x - 4.0, row.max.y));
    let label = crate::i18n::tr(language, "More Options");
    IconButton::new(IconSpec::svg(IconId::new("more"))).id(id).label(label.to_owned()).tooltip(label.to_owned()).show_at(ui, at).clicked()
}

/// A few letters as a framework glyph (`TT`, `T¹`), for a button whose face the app has always lettered; `struck`
/// draws a line through them (Strikethrough's face, which a combining stroke does not draw in every font).
#[derive(craft_ui::contract::MemoProps)]
pub(crate) struct Letters {
    pub(crate) text: &'static str,
    pub(crate) struck: bool,
}

impl Glyph for Letters {
    fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, color: egui::Color32, _mirrored: bool) {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, self.text, egui::FontId::proportional(13.0), color);
        if self.struck {
            let c = rect.center();
            painter.line_segment([c + vec2(-6.0, 0.5), c + vec2(6.0, 0.5)], egui::Stroke::new(1.2, color));
        }
    }
}

/// A toggle-like button at `at` lettered `letters`, named and tipped `label`, in its on look while `selected`:
/// whether it was clicked this pass.
pub(crate) fn letters_button(ui: &mut egui::Ui, at: Rect, id: egui::Id, letters: Letters, label: &str, selected: bool) -> bool {
    IconButton::new(IconSpec::glyph(&letters)).id(id).label(label.to_owned()).tooltip(label.to_owned()).selected(selected).show_at(ui, at).clicked()
}

/// What a number field counts and how it reads typed text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Quantity {
    /// A length in points shown in this document unit.
    Length(Unit),
    /// A length in points shown in points (type size, leading, stroke weight); any length spelling reads.
    Points,
    /// A percentage.
    Percent,
    /// An angle in degrees.
    Degrees,
    /// A plain number written with at most `decimals` decimals (a count, tracking in 1/1000 em).
    Plain { decimals: usize },
}

/// A number field: its name, value, quantity and limits.
pub(crate) struct Number<'a> {
    label: &'a str,
    value: Option<f64>,
    quantity: Quantity,
    range: Option<RangeInclusive<f64>>,
    step: Option<f64>,
    presets: &'a [f64],
    ignore: Option<&'a str>,
    enabled: bool,
}

/// The presets row that empties a field that may be unset (`Number::ignore`).
const IGNORE: u32 = 0;

impl<'a> Number<'a> {
    /// A field named `label` over `value` (`None` shows the blank), counting `quantity`.
    pub(crate) fn new(label: &'a str, value: Option<f64>, quantity: Quantity) -> Self {
        Number { label, value, quantity, range: None, step: None, presets: &[], ignore: None, enabled: true }
    }

    /// The range a committed value stays in (in points for a length).
    pub(crate) fn range(mut self, min: f64, max: f64) -> Self {
        self.range = Some(min..=max);
        self
    }

    /// One arrow press or wheel notch, in the shown unit.
    pub(crate) fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    /// The values the presets list offers.
    pub(crate) fn presets(mut self, presets: &'a [f64]) -> Self {
        self.presets = presets;
        self
    }

    /// The value may be unset: the presets list offers `word` first, and picking it or emptying the field unsets it.
    pub(crate) fn ignore(mut self, word: &'a str) -> Self {
        self.ignore = Some(word);
        self
    }

    /// Whether the person may edit it.
    pub(crate) fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

/// A text field over the app's own spelling of a length in `unit`, at `at` or in the flow: the value it committed
/// (points). A field left as it shows its value commits nothing: reading the shown text back would round the value
/// (13Q is 9.2126 pt, shown as 9.213).
fn measure_text(
    ui: &mut egui::Ui,
    at: Option<Rect>,
    id: egui::Id,
    label: &str,
    value: Option<f64>,
    unit: Unit,
    enabled: bool,
) -> Option<Option<f64>> {
    let field = TextField::with_codec(label.to_owned(), Bind::given(value), MeasureCodec(unit)).id(id).enabled(enabled);
    let out = match at {
        Some(at) => field.show_at(ui, at),
        None => field.show(ui),
    };
    out.events.committed().copied().filter(|v| match (v, value) {
        (Some(v), Some(old)) => v.is_finite() && format_measure(*v, unit) != format_measure(old, unit),
        (Some(v), None) => v.is_finite(),
        (None, _) => false,
    })
}

/// The number field `n` at `at` (or in the flow when `None`), with the id `id`: `Some(Some(v))` when it committed
/// `v`, `Some(None)` when a field that may be unset (`Number::ignore`) was emptied or its Ignore row picked.
pub(crate) fn number_field(ui: &mut egui::Ui, at: Option<Rect>, id: egui::Id, n: Number<'_>) -> Option<Option<f64>> {
    let Number { label, value, quantity, range, step, presets, ignore, enabled } = n;
    let clamp = |v: f64| range.as_ref().map_or(v, |r| v.clamp(*r.start(), *r.end()));
    if let Quantity::Length(unit) = quantity
        && unit_kind(unit).is_none()
    {
        return match measure_text(ui, at, id, label, value, unit, enabled)? {
            Some(v) => Some(Some(clamp(v))),
            None => ignore.map(|_| None),
        };
    }
    let rows: Vec<Entry<NumberPick>> = ignore
        .map(|word| Entry::new(NumberPick::Choice(IGNORE), word.to_owned()))
        .into_iter()
        .chain(presets.iter().map(|&v| Entry::new(NumberPick::Number(v), preset_label(quantity, v))))
        .collect();
    let field = match quantity {
        Quantity::Length(unit) => {
            let shown = unit_kind(unit).unwrap_or(UnitKind::Points);
            NumberField::length(label.to_owned(), Bind::given(value), LengthUnits::new(UnitKind::Points, shown, 72.0))
                .step(crate::widgets::NumKind::Measure(unit).step() / unit.points())
        }
        Quantity::Points => NumberField::length(label.to_owned(), Bind::given(value), LengthUnits::new(UnitKind::Points, UnitKind::Points, 72.0)),
        Quantity::Percent => NumberField::new(label.to_owned(), Bind::given(value)).unit_kind(UnitKind::Percent),
        Quantity::Degrees => NumberField::new(label.to_owned(), Bind::given(value)).unit_kind(UnitKind::Degrees),
        Quantity::Plain { decimals } => NumberField::new(label.to_owned(), Bind::given(value)).commit_decimals(decimals),
    };
    let mut field = field.id(id).enabled(enabled);
    if let Some(range) = range.clone() {
        field = field.range(range);
    }
    if let Some(step) = step {
        field = field.step(step);
    }
    if !rows.is_empty() {
        field = field.presets(&rows);
    }
    let out = match at {
        Some(at) => field.show_at(ui, at),
        None => field.show(ui),
    };
    match (out.edit.events.committed().copied(), out.preset) {
        (_, Some(NumberPick::Choice(IGNORE))) if ignore.is_some() => Some(None),
        (Some(Some(v)), _) if v.is_finite() => Some(Some(v)),
        (Some(None), _) if ignore.is_some() => Some(None),
        (_, Some(NumberPick::Number(v))) => Some(Some(clamp(v))),
        _ => None,
    }
}

/// How a presets row writes `v`: as the field writes it.
fn preset_label(quantity: Quantity, v: f64) -> String {
    match quantity {
        Quantity::Length(unit) => format_measure(v, unit),
        Quantity::Points => crate::widgets::NumKind::Points { decimals: 3 }.format(v),
        Quantity::Percent => crate::widgets::NumKind::Number { suffix: "%", decimals: 2 }.format(v),
        Quantity::Degrees => crate::widgets::NumKind::Number { suffix: "°", decimals: 2 }.format(v),
        Quantity::Plain { decimals } => crate::widgets::NumKind::Number { suffix: "", decimals }.format(v),
    }
}

/// A number field after its icon, as the panels lay a field out: the icon at `icon_at` (named by the field's label,
/// its tooltip) and the field at `field_at`. What the field did, as [`number_field`].
pub(crate) fn icon_number(
    ui: &mut egui::Ui,
    icon_at: Rect,
    field_at: Rect,
    id: egui::Id,
    artwork: &'static str,
    n: Number<'_>,
) -> Option<Option<f64>> {
    icon(ui, icon_at, artwork, Some(n.label), n.enabled);
    number_field(ui, Some(field_at), id, n)
}

/// A select at `at` (or in the flow) named `label`, over `choices` (value and shown label), showing `current`
/// (`None` for none of them, with `placeholder` when given): the value picked this pass.
pub(crate) fn choice<T: craft_behavior::Value>(
    ui: &mut egui::Ui,
    at: Option<Rect>,
    id: egui::Id,
    label: &str,
    current: Option<T>,
    choices: &[(T, String)],
    placeholder: Option<&str>,
) -> Option<T> {
    let entries: Vec<Entry<T>> = choices.iter().map(|(value, shown)| Entry::new(value.clone(), shown.clone())).collect();
    select(ui, at, id, Picker { label, current, entries: &entries, placeholder }, false)
}

/// What a select over `entries` shows: its name, the current value (`None` for none of them) and the text it shows
/// then.
struct Picker<'e, 'a, T> {
    label: &'a str,
    current: Option<T>,
    entries: &'e [Entry<T>],
    placeholder: Option<&'a str>,
}

/// The select `picker` at `at` (or in the flow); `searchable` puts a filter field over a long list. The value
/// picked this pass.
fn select<T: craft_behavior::Value>(ui: &mut egui::Ui, at: Option<Rect>, id: egui::Id, picker: Picker<'_, '_, T>, searchable: bool) -> Option<T> {
    let Picker { label, current, entries, placeholder } = picker;
    let held = current.map_or(Mixed::Mixed, Mixed::Same);
    let mut field = Select::new(label.to_owned(), Bind::given(held), entries).id(id);
    if let Some(placeholder) = placeholder {
        field = field.placeholder(placeholder.to_owned());
    }
    if searchable {
        field = field.searchable();
    }
    let out = match at {
        Some(at) => field.show_at(ui, at),
        None => field.show(ui),
    };
    out.events.committed().cloned()
}

/// A swatch's colour as a framework icon: its colour, a gradient's first stop, or the empty swatch for None.
pub(crate) fn swatch_icon(doc: &designcraft_doc::Document, name: &str) -> IconSpec<'static> {
    let (color, gradient) = crate::widgets::swatch_colors(doc, name, 1.0);
    match color.or(gradient.map(|(first, _)| first)) {
        Some(c) => IconSpec::swatch(Srgba::rgb(f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0)),
        None => IconSpec::empty_swatch(),
    }
}

/// A select of the document's swatches, each with its colour, at `at` (or in the flow), named `label`, showing
/// `current` (`placeholder` when it is none of them): the swatch picked this pass.
pub(crate) fn swatch_select(
    ui: &mut egui::Ui,
    at: Option<Rect>,
    id: egui::Id,
    label: &str,
    doc: &designcraft_doc::Document,
    current: Option<&str>,
    placeholder: Option<&str>,
) -> Option<String> {
    let entries: Vec<Entry<String>> =
        doc.swatches.iter().map(|sw| Entry::new(sw.name.clone(), sw.name.clone()).icon(swatch_icon(doc, &sw.name))).collect();
    let current = current.filter(|c| doc.swatches.iter().any(|sw| sw.name == *c)).map(str::to_owned);
    select(ui, at, id, Picker { label, current, entries: &entries, placeholder }, false)
}

/// The font family select at `at` (or in the flow): the families in the font menus' order, by the names the menus
/// show, with a filter field. The family picked this pass.
pub(crate) fn font_family_select(app: &DesignApp, ui: &mut egui::Ui, at: Option<Rect>, id: egui::Id, current: &str) -> Option<String> {
    let english = app.session.prefs.show_font_names_in_english;
    let menu = crate::panels::font_menu(app);
    let entries: Vec<Entry<String>> = menu.iter().map(|f| Entry::new(f.family.clone(), f.label(english).to_owned())).collect();
    let label = crate::i18n::tr(&app.ui.language, "Font Family");
    let current = (!current.is_empty()).then(|| current.to_owned());
    select(ui, at, id, Picker { label, current, entries: &entries, placeholder: Some("\u{2014}") }, true)
}

/// The font style select of `family` at `at` (or in the flow), showing `current` (a style with axis settings shows
/// its axes, as the native field does): the style picked this pass.
pub(crate) fn font_style_select(app: &DesignApp, ui: &mut egui::Ui, at: Option<Rect>, id: egui::Id, family: &str, current: &str) -> Option<String> {
    let styles = crate::panels::fonts(app).styles(family);
    let choices: Vec<(String, String)> = styles.iter().map(|s| (s.clone(), s.clone())).collect();
    let held = styles.iter().find(|s| *s == current).cloned();
    let shown = crate::panels::style_shown(current);
    choice(ui, at, id, crate::i18n::tr(&app.ui.language, "Font Style"), held, &choices, Some(&shown))
}

/// A check box at `at` (or in the flow) labelled `label`: its new value when it was toggled this pass.
pub(crate) fn check(ui: &mut egui::Ui, at: Option<Rect>, id: egui::Id, label: &str, value: bool, enabled: bool) -> Option<bool> {
    let field = Checkbox::new(Bind::given(value)).id(id).label(label.to_owned()).enabled(enabled);
    let out = match at {
        Some(at) => field.show_at(ui, at),
        None => field.show(ui),
    };
    out.events.committed().copied().filter(|v| *v != value)
}

/// A push button at `at` (or in the flow) labelled `label`.
pub(crate) fn button(ui: &mut egui::Ui, at: Option<Rect>, id: egui::Id, label: &str, enabled: bool) -> ButtonOutput {
    let field = Button::new(label.to_owned()).id(id).enabled(enabled);
    match at {
        Some(at) => field.show_at(ui, at),
        None => field.show(ui),
    }
}

/// Text in the panel's caption look at `at` (or in the flow).
pub(crate) fn caption(ui: &mut egui::Ui, at: Option<Rect>, text: &str) {
    let text = Text::new(text.to_owned());
    match at {
        Some(at) => text.show_at(ui, at),
        None => text.show(ui),
    };
}

/// A section heading in a strong caption, as the panels and dialogs title a group of rows.
pub(crate) fn heading(ui: &mut egui::Ui, text: &str) {
    Text::new(text.to_owned()).role(TypeRole::BodyStrong).show(ui);
}

/// A menu row: its label, whether it shows checked (`None` for a command row), and whether it can run.
pub(crate) struct MenuRow<'a, K> {
    pub(crate) key: K,
    pub(crate) label: &'a str,
    pub(crate) checked: Option<bool>,
    pub(crate) enabled: bool,
    /// The shortcut text the row shows at its end.
    pub(crate) shortcut: Option<String>,
}

/// The menu `label` opened by a click on `opener`, its rows in groups set off by dividers: the key of the row run
/// this pass.
pub(crate) fn menu<K: Clone>(ui: &mut egui::Ui, id: egui::Id, label: &str, opener: &ButtonOutput, groups: &[Vec<MenuRow<'_, K>>]) -> Option<K> {
    Menu::new(label.to_owned())
        .id(id)
        .opened_by(opener, Trigger::Click)
        .show(ui, |ui, _| {
            let mut ran = None;
            for (g, rows) in groups.iter().enumerate() {
                if g > 0 {
                    Divider::horizontal().show(ui);
                }
                for (r, row) in rows.iter().enumerate() {
                    let mut item = MenuItem::new(row.label.to_owned()).id(id.with(("row", g, r))).enabled(row.enabled);
                    if let Some(on) = row.checked {
                        item = item.check(on);
                    }
                    if let Some(shortcut) = &row.shortcut {
                        item = item.shortcut(shortcut.clone());
                    }
                    if item.show(ui).ran {
                        ran = Some(row.key.clone());
                    }
                }
            }
            ran
        })
        .inner
        .flatten()
}

/// The framework's unit for a document unit, or `None` when its number field cannot show it as the app does.
fn unit_kind(unit: Unit) -> Option<UnitKind> {
    Some(match unit {
        Unit::Points => UnitKind::Points,
        Unit::Inches | Unit::InchesDecimal => UnitKind::Inches,
        Unit::Millimeters => UnitKind::Millimetres,
        Unit::Centimeters => UnitKind::Centimetres,
        // Picas: the framework shows and reads decimal picas ("1.5 pica") and refuses the app's `1p6`. A cicero (12
        // didot points), an agate, Q, Ha and pixels have no kind in the framework. These fields are text fields over
        // the app's own measure spelling instead.
        Unit::Picas | Unit::Ciceros | Unit::Agates | Unit::Pixels | Unit::Q | Unit::Ha => return None,
    })
}

/// A length in points as the app writes and reads it in a document unit: `format_measure` and `parse_measure`, the
/// spelling of every measurement field of the app (`3p6`, `2c4`, `12 pt + 1p`). Empty text is no value.
#[derive(Clone)]
struct MeasureCodec(Unit);

impl Codec for MeasureCodec {
    type Value = Option<f64>;

    fn format(&self, value: &Option<f64>) -> String {
        value.filter(|v| v.is_finite()).map(|v| format_measure(v, self.0)).unwrap_or_default()
    }

    fn parse(&self, text: &str, _start: &Option<f64>) -> Result<Option<f64>, NotAValue> {
        if text.trim().is_empty() {
            return Err(NotAValue);
        }
        parse_measure(text, self.0).map(Some).map_err(|_| NotAValue)
    }
}

/// What a length field edits: a value in points, the document unit it is shown in, and the range a value stays in.
pub(crate) struct Length {
    /// The value, in points; `None` for no value (several guides).
    pub(crate) value: Option<f64>,
    /// The document unit the field shows.
    pub(crate) unit: Unit,
    /// The range a committed value stays in, in points.
    pub(crate) range: Option<RangeInclusive<f64>>,
}

/// One length field at `at`, named `label`: what it committed this pass (points), and the rect of its text part as
/// the framework laid it out (the element whose node is the spin button or the text input; the component's box for
/// anything else), for the tests and the control channel.
pub(crate) fn length_field(ui: &mut egui::Ui, at: Rect, id: egui::Id, label: &str, length: Length, enabled: bool) -> (Option<f64>, Rect) {
    let Length { value, unit, range } = length;
    let (committed, recorded) = craft_ui_widgets::runtime::inspect::record(ui, id.with("record"), |ui| match unit_kind(unit) {
        Some(shown) => {
            // The arrow and wheel step the native field had (1 pt, 1/16 in, 1 mm), in the shown unit the framework
            // steps in.
            let step = crate::widgets::NumKind::Measure(unit).step() / unit.points();
            let mut field = NumberField::length(label.to_owned(), Bind::given(value), LengthUnits::new(UnitKind::Points, shown, 72.0))
                .id(id)
                .step(step)
                .enabled(enabled);
            if let Some(range) = range.clone() {
                field = field.range(range);
            }
            field.show_at(ui, at).edit.events.committed().copied().flatten()
        }
        None => measure_text(ui, Some(at), id, label, value, unit, enabled)
            .flatten()
            .map(|v| range.as_ref().map_or(v, |range| v.clamp(*range.start(), *range.end()))),
    });
    let mut rect = at;
    if let Some(recorded) = recorded {
        rect = recorded.elements.first().map_or(at, |element| element.rect);
        for element in &recorded.elements {
            let Some(facts) = &element.facts else { continue };
            if facts.role == "SpinButton" || facts.role == "TextInput" {
                rect = recorded.hits.iter().find(|hit| hit.owner == element.id && hit.part == element.part).map_or(element.rect, |hit| hit.rect);
                break;
            }
        }
    }
    (committed.filter(|v| v.is_finite()), rect)
}

/// The four transform fields of the panel's block `block` and their captions, reporting what one of them committed as
/// the params of `transform.set` (`{x?, y?, width?, height?}`, in points, about the reference point `rp`). Empty when
/// no field committed this pass. A locked selection's fields are disabled: `transform.set` itself moves a locked
/// frame, so the disabled field is the guard.
pub(crate) fn rows(app: &mut DesignApp, ui: &mut egui::Ui, block: Rect, i: &SelInfo, rp: u8, constrain: bool) -> Map<String, Value> {
    let mut set = Map::new();
    let unit = crate::panels::properties::units(app);
    let r = i.page_rect;
    let (fx, fy) = ((rp % 3) as f64 / 2.0, (rp / 3) as f64 / 2.0);
    let values = [("x", r.x0 + r.width() * fx), ("y", r.y0 + r.height() * fy), ("width", r.width()), ("height", r.height())];
    let grid =
        [[("x", "X:", 40.0, 18.0, 59.0), ("width", "W:", 120.0, 19.0, 140.0)], [("y", "Y:", 40.0, 18.0, 59.0), ("height", "H:", 120.0, 19.0, 140.0)]];
    app.framework_fields.clear();
    for (row, cells) in grid.into_iter().enumerate() {
        let yy = row as f32 * 25.0;
        for (key, caption, cx, cw, x) in cells {
            Text::new(caption).show_at(ui, crate::panels::properties::sub(block, cx, yy, cw, crate::widgets::FIELD_H));
            let value = values.iter().find(|(k, _)| *k == key).map_or(0.0, |(_, v)| *v);
            let at = Rect::from_min_size(block.min + vec2(x, yy), vec2(54.0, crate::widgets::FIELD_H));
            let label = crate::i18n::tr(&app.ui.language, caption.trim_end_matches(':')).to_owned();
            let id = ui.id().with(("framework-transform", key));
            let (committed, rect) = length_field(ui, at, id, &label, Length { value: Some(value), unit, range: None }, !i.locked);
            app.framework_fields.push((key.to_string(), rect));
            let Some(committed) = committed else { continue };
            set.insert(key.into(), json!(committed));
            // Constrain proportions: the other side follows by the same ratio, as the native link does.
            if constrain && matches!(key, "width" | "height") {
                let (other, from) = if key == "width" { ("height", r.height()) } else { ("width", r.width()) };
                if from > 1e-9 && value > 0.0 {
                    set.insert(other.into(), json!(from * (committed / value)));
                }
            }
        }
    }
    set
}

/// The rects of the transform fields from the last pass, for the control channel's `ui.inspect`.
pub(crate) fn inspect(app: &DesignApp) -> Value {
    Value::Object(
        app.framework_fields.iter().map(|(key, rect)| (key.clone(), json!([rect.left(), rect.top(), rect.width(), rect.height()]))).collect(),
    )
}
