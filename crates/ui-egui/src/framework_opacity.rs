//! Properties › Appearance's Opacity on the craft UI framework's own `NumberField` and `SliderField` — the
//! `DESIGNCRAFT_FRAMEWORK_OPACITY=1` pilot.
//!
//! The row keeps the rest of itself: its icon, the "Opacity" link that opens the Effects panel, and the
//! chevron beside the field. With the flag on, the number field where the percentage is typed and the
//! slider that chevron opens are the framework's own components over the same `object.opacity` command the
//! bespoke ones ran, so the document, the canvas and the undo history stay the engine's.
//!
//! What this slice does and does not do:
//!
//! - the field commits on the framework's `Commit` (Enter, Tab, focus loss, a step key, a spinner step) and
//!   on a `Set` (a reset): one `object.opacity`, one undo step;
//! - the slider's drag is the panel's existing live preview (`edit.preview` … `edit.commitPreview`): the
//!   canvas follows the drag and the release is one undo step, as the bespoke slider's drag was;
//! - a `Cancel` — Escape while a drag is running, or the popup going before the drag draws a gesture of its
//!   own — goes back through the engine's own `edit.cancelPreview`: the object has the opacity it had before
//!   the drag, no undo step is pushed, and the preview this row opened is closed, so nothing later commits
//!   it. Both paths are guarded by a marker that says the open preview is this row's, so a cancel from here
//!   never takes another panel's preview back;
//! - the value is a whole percent: the field steps by 1, snaps a typed value to it and rounds a committed
//!   one, which is the number the bespoke field showed (integer, `%`);
//! - a locked object's opacity stays editable, as the bespoke one is;
//! - native only, and it needs the framework's runtime on the context
//!   ([`crate::framework_transform::install`]); without it the row keeps the bespoke field and slider.

use crate::DesignApp;
use craft_behavior::EditEvent;
use craft_ui_widgets::contract::{Bind, Show as _};
use craft_ui_widgets::form::{SliderField, SliderFieldLayout};
use craft_ui_widgets::runtime::env::UnitKind;
use craft_ui_widgets::text::NumberField;
use egui::{Rect, vec2};
use serde_json::{Value, json};

/// The flag that puts the Opacity row on the framework: `DESIGNCRAFT_FRAMEWORK_OPACITY=1`.
pub(crate) const FLAG: &str = "DESIGNCRAFT_FRAMEWORK_OPACITY";

/// Whether the pilot is asked for in the environment.
pub(crate) fn enabled() -> bool {
    std::env::var(FLAG).is_ok_and(|v| matches!(v.trim(), "1" | "on" | "true" | "yes"))
}

/// Whether the framework draws the row: the pilot is on and its runtime is on this context. Without the
/// runtime a field paints but takes no input, so the bespoke field and slider stay.
pub(crate) fn available(app: &DesignApp) -> bool {
    app.framework_opacity && app.framework_installed
}

/// Where the row and the document meet: the document keeps a ratio (0..=1) and the row shows whole
/// percents (0..=100). These two are the only place the two scales meet — `percent_of` going in, and
/// [`committed`] (with `ratio`) coming out — so nothing double-scales: 0.4 shows as 40, and a committed 40
/// runs as 0.4.
fn percent_of(ratio: f64) -> f64 {
    (ratio * 100.0).clamp(0.0, 100.0).round()
}

/// `percent` (0..=100) as the ratio `object.opacity` takes, or `None` for a number that is not one.
fn ratio(percent: f64) -> Option<f64> {
    percent.is_finite().then(|| (percent / 100.0).clamp(0.0, 1.0))
}

/// The ratio to run for a percent a field committed, through [`ratio`]: the row's one way out.
pub(crate) fn committed(percent: f64) -> Option<f64> {
    ratio(percent)
}

/// What one pass of the pilot's slider reported, in percent.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Gesture {
    /// The drag moved: the value to show as the live preview.
    Update(f64),
    /// The gesture ended: the value to commit as one undo step.
    Commit(f64),
    /// A reset (a double click on the thumb): the value to apply at once.
    Set(f64),
    /// The gesture was cancelled: the preview goes back to what the object had.
    Cancel,
}

/// The framework's percent field over `opacity` (0..=1) at `at`, named `label`: the percentage it
/// committed this pass, when one did.
pub(crate) fn field(app: &mut DesignApp, ui: &mut egui::Ui, at: Rect, label: &str, opacity: f64) -> Option<f64> {
    let region = ui.scope_id().with("framework-opacity");
    let mut rect = at;
    // The value goes in and its edits come out as events (`Bind::given`); `percent_of` is the row's single
    // ratio-to-percent boundary, so the field is given the whole percent it shows.
    let percent = percent_of(opacity);
    let (out, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)));
        NumberField::new(label.to_string(), Bind::given(Some(percent)))
            .unit_kind(UnitKind::Percent)
            .range(0.0..=100.0)
            .step(1.0)
            .snap_typed(true)
            // The whole-percent field shows no decimals and commits whole percents: the display precision is
            // what the field's own write-back matcher compares at, so it must be the row's precision too.
            .fixed_decimals(0)
            .commit_decimals(0)
            .show(&mut child)
    });
    let mut whole = rect;
    let mut stops = Vec::new();
    if let Some(recorded) = recorded {
        // The component's own box and each of its Tab stops, so a test can say what "outside the whole
        // component" and "a part of it" mean without guessing from the field's inner rect.
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
    if std::env::var("DESIGNCRAFT_TAB_TRACE").is_ok() {
        eprintln!("TRACE field in={opacity} shown={:?} focused={:?} events={:?}", out.edit.shown, ui.ctx().memory(|m| m.focused()), out.edit.events);
    }
    app.framework_opacity_fields.push(("component".to_string(), whole));
    app.framework_opacity_component_stops = stops;
    let mut committed = None;
    for event in out.edit.events {
        match event {
            EditEvent::Commit { after, .. } | EditEvent::Set { after, .. } => committed = after,
            // A Begin, a live Update and a Cancel: the field applies a typed value when it is committed.
            _ => {}
        }
    }
    committed.filter(|v| v.is_finite())
}

/// The framework's slider and its own number field over `opacity` (0..=1) at `at`: what the pass reported,
/// the rect the framework laid out, and whether the slider is still held.
pub(crate) fn slider(ui: &mut egui::Ui, at: Rect, label: &str, opacity: f64) -> (Option<Gesture>, Rect, bool) {
    let region = ui.scope_id().with("framework-opacity-slider");
    let mut rect = at;
    let percent = percent_of(opacity);
    let (out, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::top_down(egui::Align::Min)));
        SliderField::new(label.to_string(), Bind::given(percent), 0.0..=100.0)
            .layout(SliderFieldLayout::Inline)
            .unit_kind(UnitKind::Percent)
            .step(1.0)
            .show(&mut child)
    });
    if let Some(recorded) = recorded {
        for element in &recorded.elements {
            let Some(facts) = &element.facts else { continue };
            if facts.role == "Slider" {
                rect = recorded.hits.iter().find(|hit| hit.owner == element.id && hit.part == element.part).map_or(element.rect, |hit| hit.rect);
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

/// The popup the chevron beside the field opens: the framework's slider field over the panel's own live
/// preview, so a drag shows on the page and its release is one undo step.
pub(crate) fn slider_popup(app: &mut DesignApp, resp: &egui::Response, opacity: f64) {
    let label = crate::i18n::tr(&app.ui.language, "Opacity");
    let mut shown = false;
    let (mut gesture, mut rect, mut held) = (None, None, false);
    egui::Popup::menu(resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        shown = true;
        let width = ui.available_width().max(150.0);
        let at = ui.allocate_exact_size(vec2(width, 46.0), egui::Sense::hover()).0;
        let (g, r, h) = slider(ui, at, &label, opacity);
        (gesture, rect, held) = (g, Some(r), h);
    });
    if let Some(rect) = rect {
        app.framework_opacity_fields.push(("slider".to_string(), rect));
    }
    // Whether the open preview is this row's drag: the marker is true only while the preview the row
    // opened is still open, and both cancel branches below are guarded by it.
    let owned = app.live_preview_open() && resp.ctx.data(|d| d.get_temp::<bool>(owned_marker()).unwrap_or(false));
    match gesture {
        Some(Gesture::Update(v)) => {
            if let Some(r) = ratio(v) {
                app.live_preview("Opacity", "object.opacity", json!({"opacity": r}));
                mark_owned(&resp.ctx, true);
            }
        }
        Some(Gesture::Commit(v)) => {
            // The release is one undo step, at the value the drag reached.
            if let Some(r) = ratio(v) {
                app.live_preview("Opacity", "object.opacity", json!({"opacity": r}));
            }
            app.end_live_preview();
            mark_owned(&resp.ctx, false);
        }
        Some(Gesture::Cancel) => {
            // Only this row's own drag may take a preview back: a Cancel from a popup whose drag never
            // started here leaves another panel's preview, its value and the history alone.
            if owned {
                app.cancel_live_preview();
            }
            mark_owned(&resp.ctx, false);
        }
        Some(Gesture::Set(v)) => {
            app.end_live_preview();
            mark_owned(&resp.ctx, false);
            if let Some(r) = ratio(v) {
                let _ = app.run("object.opacity", json!({"opacity": r}));
            }
        }
        None if held => app.hold_live_preview(),
        // The popup went before it drew a gesture of its own (Escape closed it, or a press outside did):
        // the drag's preview goes back through `edit.cancelPreview`, so the document is what it was and
        // no step is pushed.
        None if owned && !shown => {
            app.cancel_live_preview();
            mark_owned(&resp.ctx, false);
        }
        None => {}
    }
}

/// The egui key that says whether the open preview is this row's drag, so a cancel from here never touches
/// another panel's preview.
fn owned_marker() -> egui::Id {
    egui::Id::unique("framework-opacity-preview")
}

/// Records whether this row's drag opened the preview that is open now.
fn mark_owned(ctx: &egui::Context, owned: bool) {
    ctx.data_mut(|d| d.insert_temp(owned_marker(), owned));
}

/// The rects the pilot recorded in the last pass — the field (`opacity`), the chevron beside it (`more`, from
/// the row) and the slider while its popup is open (`slider`) — for the control channel's `ui.inspect` and
/// the tests.
pub(crate) fn inspect(app: &DesignApp) -> Value {
    Value::Object(
        app.framework_opacity_fields.iter().map(|(key, rect)| (key.clone(), json!([rect.left(), rect.top(), rect.width(), rect.height()]))).collect(),
    )
}
