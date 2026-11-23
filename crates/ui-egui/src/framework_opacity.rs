//! Properties › Appearance's Opacity row on the craft UI framework: the percent field where the opacity is typed,
//! the chevron beside it, and the slider popover the chevron opens, over the same `object.opacity` command the native
//! row runs, so the document, the canvas and the undo history stay the engine's.
//!
//! - the field commits on the framework's `Commit` (Enter, Tab, focus loss, a step key) and on a `Set` (a reset): one
//!   `object.opacity`, one undo step. Tab from it goes on to the chevron, the row's next stop;
//! - the slider's drag is the panel's live preview (`edit.preview` … `edit.commitPreview`): the canvas follows the drag
//!   and the release is one undo step;
//! - a `Cancel` (Escape while a drag is running) or the popover closing under a drag goes back through the engine's own
//!   `edit.cancelPreview`: the object has the opacity it had before the drag and no undo step is pushed. Both paths are
//!   guarded by a marker that says the open preview is this row's, so a cancel from here never takes another panel's
//!   preview back;
//! - the value is a whole percent: the field steps by 1, snaps a typed value to it and rounds a committed one;
//! - a locked object's opacity stays editable, as the native one is.

use crate::DesignApp;
use crate::framework_transform::Artwork;
use crate::panels::properties::sub;
use crate::widgets::FIELD_H;
use craft_behavior::EditEvent;
use craft_ui_widgets::buttons::IconButton;
use craft_ui_widgets::contract::{Bind, PopupKind, Props as _, Show as _, ShowContainer as _, Trigger};
use craft_ui_widgets::form::{SliderField, SliderFieldLayout};
use craft_ui_widgets::popover::Popover;
use craft_ui_widgets::primitives::IconSpec;
use craft_ui_widgets::runtime::env::UnitKind;
use craft_ui_widgets::text::NumberField;
use egui::Rect;
use serde_json::{Value, json};

/// Whether the Opacity row is the framework's: always on the desktop, where the framework is the UI.
pub(crate) fn enabled() -> bool {
    true
}

/// Where the row and the document meet: the document keeps a ratio (0..=1) and the row shows whole percents
/// (0..=100). This and [`ratio`] are the only place the two scales meet, so nothing double-scales: 0.4 shows as 40,
/// and a committed 40 runs as 0.4.
fn percent_of(ratio: f64) -> f64 {
    (ratio * 100.0).clamp(0.0, 100.0).round()
}

/// `percent` (0..=100) as the ratio `object.opacity` takes, or `None` for a number that is not one.
fn ratio(percent: f64) -> Option<f64> {
    percent.is_finite().then(|| (percent / 100.0).clamp(0.0, 1.0))
}

/// What one pass of the slider reported, in percent.
#[derive(Clone, Copy, Debug)]
enum Gesture {
    /// The drag moved: the value to show as the live preview.
    Update(f64),
    /// The gesture ended: the value to commit as one undo step.
    Commit(f64),
    /// A reset (a double click on the thumb): the value to apply at once.
    Set(f64),
    /// The gesture was cancelled: the preview goes back to what the object had.
    Cancel,
}

/// The row's field and chevron in the panel's block `block`, named `label`: the ratio `object.opacity` takes when the
/// field committed a value this pass. The chevron's popover runs its own live preview.
pub(crate) fn row(app: &mut DesignApp, ui: &mut egui::Ui, block: Rect, label: &str, opacity: f64) -> Option<f64> {
    // The field and the chevron are two controls, two Tab stops, side by side with room between them.
    let typed = field(app, ui, sub(block, 96.0, 0.0, 49.0, FIELD_H), label, opacity);
    chevron(app, ui, sub(block, 147.0, 0.0, 18.0, FIELD_H), label, opacity);
    typed.and_then(ratio)
}

/// The framework's percent field over `opacity` (0..=1) at `at`, named `label`: the percentage it committed this
/// pass, when one did.
fn field(app: &mut DesignApp, ui: &mut egui::Ui, at: Rect, label: &str, opacity: f64) -> Option<f64> {
    let region = ui.id().with("framework-opacity");
    let mut rect = at;
    // The value goes in and its edits come out as events (`Bind::given`); `percent_of` is the row's single
    // ratio-to-percent boundary, so the field is given the whole percent it shows.
    let percent = percent_of(opacity);
    let (out, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| {
        NumberField::new(label.to_owned(), Bind::given(Some(percent)))
            .id(region.with("field"))
            .unit_kind(UnitKind::Percent)
            .range(0.0..=100.0)
            .step(1.0)
            .snap_typed(true)
            // The whole-percent field shows no decimals and commits whole percents: the display precision is what the
            // field's own write-back matcher compares at, so it must be the row's precision too.
            .fixed_decimals(0)
            .commit_decimals(0)
            .show_at(ui, at)
    });
    let mut whole = rect;
    let mut stops = Vec::new();
    if let Some(recorded) = recorded {
        // The component's own box and each of its Tab stops, so a test can say what "outside the whole component" and
        // "a part of it" mean without guessing from the field's inner rect.
        whole = recorded.elements.first().map_or(rect, |element| element.rect);
        stops = (0..recorded.elements.len()).filter_map(|i| recorded.stop_rect(i).map(|r| (i, r))).collect();
        for element in &recorded.elements {
            let Some(facts) = &element.facts else { continue };
            if facts.role == "SpinButton" {
                rect = recorded.hits.iter().find(|hit| hit.owner == element.id && hit.part == element.part).map_or(element.rect, |hit| hit.rect);
                break;
            }
        }
    }
    app.framework_opacity_fields.push(("opacity".to_string(), rect));
    app.framework_opacity_fields.push(("component".to_string(), whole));
    app.framework_opacity_component_stops = stops;
    out.edit.events.committed().copied().flatten().filter(|v| v.is_finite())
}

/// The framework's slider field over `opacity` (0..=1), named `label`: what the pass reported, the rect of its track as
/// the framework laid it out, and whether the slider is still held.
fn slider(ui: &mut egui::Ui, label: &str, opacity: f64) -> (Option<Gesture>, Option<Rect>, bool) {
    let region = ui.id().with("framework-opacity-slider");
    let percent = percent_of(opacity);
    let (out, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| {
        SliderField::new(label.to_owned(), Bind::given(percent), 0.0..=100.0)
            .id(region.with("slider"))
            .layout(SliderFieldLayout::Inline)
            .unit_kind(UnitKind::Percent)
            .step(1.0)
            .show(ui)
    });
    let mut rect = None;
    if let Some(recorded) = recorded {
        for element in &recorded.elements {
            let Some(facts) = &element.facts else { continue };
            if facts.role == "Slider" {
                rect =
                    Some(recorded.hits.iter().find(|hit| hit.owner == element.id && hit.part == element.part).map_or(element.rect, |hit| hit.rect));
                break;
            }
        }
    }
    let held = out.edit.response.is_pointer_button_down_on() || out.edit.response.dragged();
    let mut gesture = None;
    for event in out.edit.events {
        match event {
            EditEvent::Update { value, .. } => gesture = Some(Gesture::Update(value)),
            EditEvent::Commit { after, .. } => gesture = Some(Gesture::Commit(after)),
            EditEvent::Set { after, .. } => gesture = Some(Gesture::Set(after)),
            EditEvent::Cancel { .. } => gesture = Some(Gesture::Cancel),
            // A Begin, or an event a later SDK adds: the four above are the slider's gesture.
            _ => {}
        }
    }
    (gesture, rect, held)
}

/// The chevron beside the field at `at`, a framework button and the row's own Tab stop, and the slider popover it
/// opens and closes. The popover's slider runs the panel's live preview, so a drag shows on the page and its release
/// is one undo step.
fn chevron(app: &mut DesignApp, ui: &mut egui::Ui, at: Rect, label: &str, opacity: f64) {
    let id = ui.id().with("framework-opacity-more");
    let opener = IconButton::new(IconSpec::glyph(&Artwork("chevron-right"))).id(id).label(label.to_owned()).popup(PopupKind::Dialog).show_at(ui, at);
    app.framework_opacity_fields.push(("more".to_string(), opener.response.rect));
    // The open state is the popover's own, kept in its opener's slot: a click on the chevron opens and closes it, and
    // Escape, a press outside or the focus leaving close it.
    let shown = Popover::new(label.to_owned(), Bind::kept())
        .id(id.with("popover"))
        .overlay(|overlay| overlay.opened_by(&opener.response, Trigger::Click))
        .show(ui, |ui, _| slider(ui, label, opacity));
    // Whether the popover is shown this pass: its body runs only then.
    let body_ran = shown.inner.is_some();
    // The body's last run is the pass's real one: its gesture is the one to apply.
    let (gesture, rect, held) = shown.inner.unwrap_or((None, None, false));
    // Closed: it reported a close this pass, or it isn't shown. `PopoverOutput::open` is not used: under `Bind::kept()`
    // with an overlay opener it reads false while the popover is shown and its body runs.
    let closed = shown.output.closed.is_some() || !body_ran;
    if let Some(rect) = rect {
        app.framework_opacity_fields.push(("slider".to_string(), rect));
    }
    // Whether the open preview is this row's drag: the marker is true only while the preview the row opened is still
    // open, and both cancel branches below are guarded by it.
    let owned = app.live_preview_open() && ui.ctx().data(|d| d.get_temp::<bool>(owned_marker()).unwrap_or(false));
    match gesture {
        Some(Gesture::Update(v)) => {
            if let Some(r) = ratio(v) {
                app.live_preview("Opacity", "object.opacity", json!({"opacity": r}));
                mark_owned(ui.ctx(), true);
            }
        }
        Some(Gesture::Commit(v)) => {
            // The release is one undo step, at the value the drag reached.
            if let Some(r) = ratio(v) {
                app.live_preview("Opacity", "object.opacity", json!({"opacity": r}));
            }
            app.end_live_preview();
            mark_owned(ui.ctx(), false);
        }
        Some(Gesture::Cancel) => {
            // Only this row's own drag may take a preview back: a Cancel from a popover whose drag never started here
            // leaves another panel's preview, its value and the history alone.
            if owned {
                app.cancel_live_preview();
            }
            mark_owned(ui.ctx(), false);
        }
        Some(Gesture::Set(v)) => {
            app.end_live_preview();
            mark_owned(ui.ctx(), false);
            if let Some(r) = ratio(v) {
                let _ = app.run("object.opacity", json!({"opacity": r}));
            }
        }
        // The popover went with a drag of this row still open (Escape closed it, or a press outside did): the drag's
        // preview goes back through `edit.cancelPreview`, so the document is what it was and no step is pushed.
        None if owned && closed => {
            app.cancel_live_preview();
            mark_owned(ui.ctx(), false);
        }
        // A pass with no new value in the middle of the row's own drag (the pointer is still down) keeps the preview open:
        // the slider's response does not report the press the framework holds, so `owned` (a preview this row opened
        // and has not yet committed or cancelled) says the drag goes on.
        None if held || owned => app.hold_live_preview(),
        None => {}
    }
}

/// The egui key that says whether the open preview is this row's drag, so a cancel from here never touches another
/// panel's preview.
fn owned_marker() -> egui::Id {
    egui::Id::unique("framework-opacity-preview")
}

/// Records whether this row's drag opened the preview that is open now.
fn mark_owned(ctx: &egui::Context, owned: bool) {
    ctx.data_mut(|d| d.insert_temp(owned_marker(), owned));
}

/// The rects the row recorded in the last pass — the field (`opacity`), the field component's own box (`component`),
/// the chevron (`more`) and the slider's track while its popover is open (`slider`) — for the control channel's
/// `ui.inspect` and the tests.
pub(crate) fn inspect(app: &DesignApp) -> Value {
    Value::Object(
        app.framework_opacity_fields.iter().map(|(key, rect)| (key.clone(), json!([rect.left(), rect.top(), rect.width(), rect.height()]))).collect(),
    )
}
