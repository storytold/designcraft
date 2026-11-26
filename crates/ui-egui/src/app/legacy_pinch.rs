//! Windows precision touchpads and some Linux Wayland compositors report a pinch as Ctrl + wheel
//! in fractions of a notch or in points, rather than as egui's own `Event::Zoom`. Since Ctrl/Cmd +
//! wheel now pans sideways (`super::input::configure_wheel`), those events are turned back into
//! a pinch before egui folds them into a horizontal scroll. A whole-notch Ctrl + wheel from a
//! mouse is left alone and pans sideways.

use egui::{Context, Event, Id, MouseWheelUnit, RawInput, Vec2};

/// A Ctrl + wheel event this soon (seconds) after a pinch step still belongs to the pinch.
const PINCH_GAP: f64 = 0.25;

fn pinch_id() -> Id {
    Id::unique("designcraft-wheel-legacy-pinch")
}

/// Turn fractional Ctrl + wheel from a precision touchpad into `Event::Zoom` events.
pub(super) fn fold_legacy_pinch(ctx: &Context, raw: &mut RawInput) {
    // A native zoom event already carries the pinch; folding the wheel beside it would zoom twice.
    // Some backends also send the same gesture as Ctrl + wheel in that frame: drop that duplicate,
    // or egui smooths it into a sideways pan after the zoom frame and moves the point under the
    // pointer. Ordinary wheel input without Ctrl stays.
    if raw.events.iter().any(|event| matches!(event, Event::Zoom(_))) {
        raw.events.retain(|event| !matches!(event, Event::MouseWheel { modifiers, .. } if modifiers.ctrl));
        ctx.data_mut(|d| d.remove::<f64>(pinch_id()));
        return;
    }
    let (line_scroll_speed, scroll_zoom_speed) = ctx.options(|o| (o.input_options.line_scroll_speed, o.input_options.scroll_zoom_speed));
    let notch = if line_scroll_speed.is_finite() && line_scroll_speed > 0.0 { line_scroll_speed } else { 40.0 };
    let speed = if scroll_zoom_speed.is_finite() && scroll_zoom_speed > 0.0 { scroll_zoom_speed } else { 1.0 / 200.0 };
    let now = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
    let last: Option<f64> = ctx.data(|d| d.get_temp(pinch_id()));

    // (delta, is a high-resolution Point event).
    let ctrl_wheel = |event: &Event| match event {
        Event::MouseWheel { unit: MouseWheelUnit::Line, delta, modifiers, .. } if modifiers.ctrl => Some((*delta, false)),
        Event::MouseWheel { unit: MouseWheelUnit::Point, delta, modifiers, .. } if modifiers.ctrl => Some((*delta, true)),
        _ => None,
    };
    let fraction = |d: Vec2| d.x.fract() != 0.0 || d.y.fract() != 0.0;
    let pinching = last.is_some_and(|t| (0.0..PINCH_GAP).contains(&(now - t)))
        || raw.events.iter().filter_map(ctrl_wheel).any(|(delta, high_res)| high_res || fraction(delta));
    if !pinching {
        return;
    }

    for event in &mut raw.events {
        let Some((delta, high_res)) = ctrl_wheel(event) else { continue };
        // Point deltas are already screen points; Line deltas are notches egui scales by the line
        // scroll speed. Both reach the same exponential Ctrl-scroll zoom egui used before.
        let points = if high_res { delta } else { delta * notch };
        let factor = (speed * (points.x + points.y)).exp();
        *event = Event::Zoom(if factor.is_finite() && factor > 0.0 { factor } else { 1.0 });
        // Only a converted wheel step keeps the pinch window open; an empty frame must not.
        ctx.data_mut(|d| d.insert_temp(pinch_id(), now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Modifiers, TouchPhase};

    fn configure(ctx: &Context) {
        super::super::input::configure_wheel(ctx);
    }

    fn wheel(unit: MouseWheelUnit, delta: Vec2, modifiers: Modifiers) -> Event {
        Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers, source: egui::MouseWheelSource::Unknown }
    }

    fn raw(time: f64, events: Vec<Event>) -> RawInput {
        RawInput { time: Some(time), events, ..Default::default() }
    }

    #[test]
    fn ctrl_point_wheel_is_a_pinch() {
        let ctx = Context::default();
        configure(&ctx);
        let mut input = raw(1.0, vec![wheel(MouseWheelUnit::Point, Vec2::new(2.0, 0.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut input);
        assert!(matches!(input.events.as_slice(), [Event::Zoom(f)] if *f > 1.0));
    }

    #[test]
    fn ctrl_line_whole_notch_stays_a_sideways_scroll() {
        let ctx = Context::default();
        configure(&ctx);
        let mut input = raw(1.0, vec![wheel(MouseWheelUnit::Line, Vec2::new(0.0, 1.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut input);
        assert!(matches!(input.events.as_slice(), [Event::MouseWheel { .. }]));
    }

    #[test]
    fn a_notch_right_after_a_pinch_belongs_to_it() {
        let ctx = Context::default();
        configure(&ctx);
        let mut first = raw(1.0, vec![wheel(MouseWheelUnit::Point, Vec2::new(1.0, 0.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut first);
        let mut second = raw(1.1, vec![wheel(MouseWheelUnit::Line, Vec2::new(0.0, 1.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut second);
        assert!(matches!(second.events.as_slice(), [Event::Zoom(_)]));
    }

    #[test]
    fn a_notch_after_the_gap_is_a_sideways_scroll() {
        let ctx = Context::default();
        configure(&ctx);
        let mut first = raw(1.0, vec![wheel(MouseWheelUnit::Point, Vec2::new(1.0, 0.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut first);
        let mut later = raw(1.3, vec![wheel(MouseWheelUnit::Line, Vec2::new(0.0, 1.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut later);
        assert!(matches!(later.events.as_slice(), [Event::MouseWheel { .. }]));
    }

    #[test]
    fn an_empty_frame_does_not_extend_the_pinch_window() {
        let ctx = Context::default();
        configure(&ctx);
        let mut first = raw(1.0, vec![wheel(MouseWheelUnit::Point, Vec2::new(1.0, 0.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut first);
        // Frames with no wheel events must not refresh the marker, or the window never closes.
        for time in [1.1, 1.2, 1.3] {
            let mut empty = raw(time, Vec::new());
            fold_legacy_pinch(&ctx, &mut empty);
        }
        let mut later = raw(1.3, vec![wheel(MouseWheelUnit::Line, Vec2::new(0.0, 1.0), Modifiers::CTRL)]);
        fold_legacy_pinch(&ctx, &mut later);
        assert!(matches!(later.events.as_slice(), [Event::MouseWheel { .. }]));
    }

    #[test]
    fn a_frame_with_a_zoom_event_drops_its_ctrl_wheel() {
        let ctx = Context::default();
        configure(&ctx);
        let mut input = raw(
            1.0,
            vec![
                Event::Zoom(1.2),
                wheel(MouseWheelUnit::Line, Vec2::new(0.0, 1.0), Modifiers::CTRL),
                wheel(MouseWheelUnit::Line, Vec2::new(0.0, 1.0), Modifiers::NONE),
            ],
        );
        fold_legacy_pinch(&ctx, &mut input);
        // The duplicate Ctrl + wheel is gone, so there is no smoothing tail to pan afterwards;
        // the ordinary wheel stays.
        assert!(matches!(
            input.events.as_slice(),
            [Event::Zoom(_), Event::MouseWheel { modifiers, .. }] if !modifiers.ctrl
        ));
    }

    #[test]
    fn wheel_without_ctrl_is_not_folded() {
        let ctx = Context::default();
        configure(&ctx);
        let mut input = raw(1.0, vec![wheel(MouseWheelUnit::Point, Vec2::new(2.0, 0.0), Modifiers::NONE)]);
        fold_legacy_pinch(&ctx, &mut input);
        assert!(matches!(input.events.as_slice(), [Event::MouseWheel { .. }]));
    }
}
