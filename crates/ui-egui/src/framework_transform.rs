//! The craft UI framework pieces the Properties panel's object sections share: the runtime's install, the app's
//! own artwork as a framework glyph, the length field every Transform, Text Wrap and guide row uses, and
//! Properties › Transform's X/Y/W/H rows.
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
use craft_behavior::{Codec, IconId, NotAValue};
use craft_ui_widgets::buttons::IconButton;
use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
use craft_ui_widgets::display::{Icon, Text};
use craft_ui_widgets::primitives::{Glyph, IconSpec};
use craft_ui_widgets::runtime::env::{LengthUnits, UnitKind};
use craft_ui_widgets::style::UiTokens;
use craft_ui_widgets::text::{NumberField, TextField};
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
        None => {
            let out = TextField::with_codec(label.to_owned(), Bind::given(value), MeasureCodec(unit)).id(id).enabled(enabled).show_at(ui, at);
            // A field left as it shows its value commits nothing: reading the shown text back would round the value
            // (13Q is 9.2126 pt, shown as 9.213).
            out.events
                .committed()
                .copied()
                .flatten()
                .filter(|v| v.is_finite() && value.is_none_or(|old| format_measure(*v, unit) != format_measure(old, unit)))
                .map(|v| range.as_ref().map_or(v, |range| v.clamp(*range.start(), *range.end())))
        }
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
