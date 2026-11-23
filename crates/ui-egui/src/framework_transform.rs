//! Properties › Transform's X/Y/W/H on the craft UI framework's own `NumberField` — the
//! `DESIGNCRAFT_FRAMEWORK_TRANSFORM=1` pilot.
//!
//! The real panel keeps every other row it has. With the flag on, [`rows`] draws the four transform
//! fields as genuine craft `NumberField`s (a text part over the framework's number codec, with its own
//! range, step, unit and keys) where the bespoke `NumField`s sat, and reports what a field committed as
//! the params of the panel's own `transform.set` call. Nothing else changes: the document geometry is
//! still the engine command's, the reference point is still the panel's, the unit shown is still the
//! document's, and the undo step is still the one `transform.set` pushes.
//!
//! What this slice does and does not do:
//!
//! - a field commits on the framework's `Commit` (Enter, Tab, focus loss, a spinner step, the release of
//!   a label scrub) and on a one-pass `Set` (a reset): one gesture, one `transform.set`, one undo step;
//! - live `Update`s are **not** applied. The document does not move while a value is being typed or
//!   dragged, so there is no preview to fake and nothing to put back on a `Cancel`: the document still
//!   holds the value the gesture began with. The limit is that the canvas follows a drag only when it is
//!   released;
//! - a locked selection's fields are disabled. This is the only guard: `transform.set` itself moves a
//!   locked frame, and the bespoke fields are not disabled for one, so this is a deliberate change;
//! - the arrow/wheel step is the bespoke spinner's (1 pt, 1/16 in, 1 mm), converted to the shown unit;
//! - picas keep the bespoke fields: the framework reads decimal picas only, not `1p6`;
//! - the framework shows fewer decimals than the app's `format_measure` for mm/cm/in (display only);
//! - a document unit the framework has no kind for (ciceros, agates, Q, Ha, pixels) keeps the bespoke
//!   fields: [`available`] answers `false` rather than converting behind the person's back;
//! - the framework's runtime must be installed on the context once, outside a pass, before its first one
//!   ([`install`], called from the desktop shell's creation context). Without it the fields stay
//!   bespoke: a field painted onto a context with no runtime draws but takes no input.

use crate::DesignApp;
use crate::panels::SelInfo;
use craft_behavior::EditEvent;
use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
use craft_ui_widgets::runtime::env::{LengthUnits, UnitKind};
use craft_ui_widgets::text::NumberField;
use designcraft_geom::Unit;
use egui::{Rect, vec2};
use serde_json::{Map, Value, json};

/// Whether the transform fields are the framework's: always on the desktop, where the framework is the UI.
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
        Err(error) => log::warn!("the craft UI runtime was not installed; the Transform fields stay bespoke: {error}"),
    }
}

/// The framework's unit for a document unit, or `None` when it has no kind for it.
fn unit_kind(unit: Unit) -> Option<UnitKind> {
    Some(match unit {
        Unit::Points => UnitKind::Points,
        Unit::Inches | Unit::InchesDecimal => UnitKind::Inches,
        Unit::Millimeters => UnitKind::Millimetres,
        Unit::Centimeters => UnitKind::Centimetres,
        // Picas: the framework shows and reads decimal picas ("1.5 pica") and refuses the app's `1p6`
        // spelling, so a pica document keeps the bespoke fields until the framework has a compound codec.
        // A cicero (12 didot points), an agate, Q, Ha and pixels have no kind in the framework, and a
        // field showing the wrong one would write the wrong number: the bespoke field keeps them.
        Unit::Picas | Unit::Ciceros | Unit::Agates | Unit::Pixels | Unit::Q | Unit::Ha => return None,
    })
}

/// Whether the framework can show this document's transform fields: its runtime is on the context and
/// the document's unit is one the framework names.
pub(crate) fn available(app: &DesignApp, _i: &SelInfo) -> bool {
    app.framework_installed && unit_supported(crate::panels::properties::units(app))
}

/// Whether the framework can show a document whose ruler unit is `unit`: [`available`]'s rule for a unit.
pub(crate) fn unit_supported(unit: Unit) -> bool {
    unit_kind(unit).is_some()
}

/// The four transform fields of the panel's block `block`, reporting what one of them committed as the
/// params of `transform.set` (`{x?, y?, width?, height?}`, in points, about the reference point `rp`).
/// Empty when no field committed this pass.
pub(crate) fn rows(app: &mut DesignApp, ui: &mut egui::Ui, block: Rect, i: &SelInfo, rp: u8, constrain: bool) -> Map<String, Value> {
    let mut set = Map::new();
    let unit = crate::panels::properties::units(app);
    let Some(shown) = unit_kind(unit) else { return set };
    // The bespoke spinner's step (in points), in the shown unit the framework steps in.
    let step = crate::widgets::NumKind::Measure(unit).step() / unit.points();
    // Every length in the document is in points; the field holds points and shows the document's unit.
    let lengths = LengthUnits::new(UnitKind::Points, shown, 72.0);
    let r = i.page_rect;
    let (fx, fy) = ((rp % 3) as f64 / 2.0, (rp / 3) as f64 / 2.0);
    let values = [("x", r.x0 + r.width() * fx), ("y", r.y0 + r.height() * fy), ("width", r.width()), ("height", r.height())];
    let grid =
        [[("x", "X:", 40.0, 18.0, 59.0), ("width", "W:", 120.0, 19.0, 140.0)], [("y", "Y:", 40.0, 18.0, 59.0), ("height", "H:", 120.0, 19.0, 140.0)]];
    let enabled = !i.locked;
    app.framework_fields.clear();
    for (row, cells) in grid.into_iter().enumerate() {
        let yy = row as f32 * 25.0;
        for (key, caption, cx, cw, x) in cells {
            // The panel's own visible caption, where the bespoke row draws it.
            let cap = crate::panels::properties::sub(block, cx, yy, cw, crate::widgets::FIELD_H);
            crate::panels::properties::place(ui, cap, |ui| crate::widgets::caption_w(ui, caption, cw));
            let value = values.iter().find(|(k, _)| *k == key).map_or(0.0, |(_, v)| *v);
            let at = Rect::from_min_size(block.min + vec2(x, yy), vec2(54.0, crate::widgets::FIELD_H));
            let label = crate::i18n::tr(&app.ui.language, caption.trim_end_matches(':'));
            let Some(committed) = field(app, ui, at, key, label, value, lengths, step, enabled) else { continue };
            set.insert(key.into(), json!(committed));
            // Constrain proportions: the other side follows by the same ratio, as the bespoke link does.
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

/// One framework number field over `value` (points, shown in `lengths`), at `at`. Returns the value it
/// committed, in points.
///
/// The field is drawn inside the framework's own inspection record, so the rect kept for the tests and
/// the control channel is the one the framework laid out (the element whose node is the spin button),
/// never a guess about where a field's text sits.
// The row's nine inputs are the panel's own shape: the app and its ui, the cell's rect, key and
// accessible label, the value, the document's units, the step and whether the person may edit it.
#[allow(clippy::too_many_arguments)]
fn field(
    app: &mut DesignApp,
    ui: &mut egui::Ui,
    at: Rect,
    key: &'static str,
    label: &str,
    value: f64,
    lengths: LengthUnits,
    step: f64,
    enabled: bool,
) -> Option<f64> {
    let region = ui.scope_id().with(("framework-transform", key));
    let mut rect = at;
    let (out, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let field = NumberField::length(label.to_string(), Bind::given(Some(value)), lengths).step(step).enabled(enabled);
        field.show(&mut child)
    });
    if let Some(recorded) = recorded {
        for element in &recorded.elements {
            let Some(facts) = &element.facts else { continue };
            if facts.role == "SpinButton" {
                rect = recorded.hits.iter().find(|hit| hit.owner == element.id && hit.part == element.part).map_or(element.rect, |hit| hit.rect);
                break;
            }
        }
    }
    app.framework_fields.push((key.to_string(), rect));
    let mut committed = None;
    for event in out.edit.events {
        match event {
            EditEvent::Commit { after, .. } | EditEvent::Set { after, .. } => committed = after,
            // A Begin, a live Update and a Cancel: nothing is applied, so the document still holds the
            // value the gesture began with (see the module's note on live edits).
            _ => {}
        }
    }
    committed.filter(|v| v.is_finite())
}

/// The rects of the pilot's fields from the last pass, for the control channel's `ui.inspect`.
pub(crate) fn inspect(app: &DesignApp) -> Value {
    Value::Object(
        app.framework_fields.iter().map(|(key, rect)| (key.clone(), json!([rect.left(), rect.top(), rect.width(), rect.height()]))).collect(),
    )
}
