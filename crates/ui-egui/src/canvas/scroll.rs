#[cfg(test)]
use super::json;
#[cfg(test)]
use super::view::set_zoom;
use super::{CanvasLayout, DesignApp, Mods, Point, PointerEvent, PointerKind, Rect, SCROLLBAR, Sense, Tokens, Xf, pos2, vec2};
#[cfg(test)]
use egui::Pos2;

/// Shortest scrollbar thumb.
const MIN_THUMB: f32 = 18.0;

/// How long egui (0.37 `WheelState`) keeps a scroll action's latched modifiers after the last
/// wheel event: 150 ms, or a second while fingers rest on a trackpad. The Alt latch below ends
/// with the action, exactly as egui ends Ctrl/Cmd, so the rest of an ended motion scrolls for
/// both keys and a later plain scroll is never taken for a zoom.
const SCROLL_ACTION_TIMEOUT: f64 = 0.150;

const RESTING_FINGER_TIMEOUT: f64 = 1.0;

/// Whether the wheel motion now arriving was made with Alt down. As egui does for Ctrl/Cmd, a
/// motion keeps its key to its end: the wheel is smoothed over a few frames, and a trackpad swipe
/// coasts on after the key is let go.
pub(super) fn wheel_alt(ui: &egui::Ui) -> bool {
    let id = egui::Id::unique("canvas_wheel_alt");
    // Alt, whether a trackpad swipe is under way, and the time of the last wheel event.
    let (mut alt, mut swipe, mut last): (bool, bool, f64) = ui.data(|d| d.get_temp(id)).unwrap_or((false, false, f64::NEG_INFINITY));
    ui.input(|i| {
        for e in &i.events {
            let egui::Event::MouseWheel { phase, modifiers, source, .. } = e else { continue };
            // egui ends the action (and drops its latched modifiers) when a new event arrives
            // after this pause; the Alt latch ends there too.
            let timeout = if swipe && *source == egui::MouseWheelSource::Trackpad { RESTING_FINGER_TIMEOUT } else { SCROLL_ACTION_TIMEOUT };
            if timeout < i.time - last {
                alt = false;
                swipe = false;
            }
            last = i.time;
            (alt, swipe) = match phase {
                egui::TouchPhase::Start => (modifiers.alt, true),
                egui::TouchPhase::Move => (modifiers.alt || (swipe && alt), swipe),
                egui::TouchPhase::End | egui::TouchPhase::Cancel => (false, false),
            };
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (alt, swipe, last)));
    alt
}

pub(super) fn mods(i: &egui::InputState, space: bool) -> Mods {
    Mods { shift: i.modifiers.shift, alt: i.modifiers.alt, cmd: i.modifiers.command, ctrl: i.modifiers.ctrl, space }
}

/// Held Space belongs to navigation only when neither the document nor a text field is typing.
pub(super) fn navigation_space(app: &mut DesignApp, ctx: &egui::Context) -> bool {
    ctx.input(|i| i.focused && i.key_down(egui::Key::Space)) && !app.session.wants_text() && !ctx.text_edit_focused()
}

/// Allow temporary Zoom only for Cmd+Space with no pointer press and no busy tool.
pub(super) fn spring_zoom_mode(app: &DesignApp, ctx: &egui::Context, space: bool) -> bool {
    let down = ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("canvas_pointer_down", app.pane)))).unwrap_or(false);
    space && ctx.input(|i| i.modifiers.command) && !down && !app.session.tool_busy()
}

#[derive(Clone, Default)]
struct SpringZoom {
    tool: designcraft_tools::ZoomTool,
    down: bool,
}

/// Lend the Zoom tool the gesture; the selected tool and document selection stay intact.
pub(super) fn spring_zoom_input(app: &mut DesignApp, ui: &egui::Ui, resp: &egui::Response, xf: &Xf, active: bool) -> bool {
    let id = egui::Id::unique(("canvas_spring_zoom", app.pane));
    let mut zoom: SpringZoom = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    if !ui.input(|i| i.focused) {
        ui.data_mut(|d| d.remove::<SpringZoom>(id));
        return false;
    }
    if !zoom.down && (!active || ui.input(|i| i.pointer.middle_down())) {
        return false;
    }
    let (pointer, m) = ui.input(|i| (i.pointer.clone(), mods(i, true)));
    let vi = app.view_info();
    let mut events = Vec::new();
    let canvas = if xf.rot == 0 { app.canvas_rect } else { Some(resp.rect) };
    if pointer.primary_pressed()
        && resp.hovered()
        && let Some(p) = pointer.press_origin().filter(|p| canvas.is_some_and(|r| r.contains(*p)))
    {
        events.push(PointerEvent { kind: PointerKind::Down, pos: xf.to_canvas(p), mods: m, clicks: 1 });
        zoom.down = true;
        resp.request_focus();
    }
    if zoom.down
        && let Some(p) = pointer.interact_pos()
    {
        if resp.dragged() && resp.drag_delta() != egui::Vec2::ZERO {
            events.push(PointerEvent { kind: PointerKind::Drag, pos: xf.to_canvas(p), mods: m, clicks: 1 });
        }
        if pointer.primary_released() {
            events.push(PointerEvent { kind: PointerKind::Up, pos: xf.to_canvas(p), mods: m, clicks: 1 });
            zoom.down = false;
        }
    }
    for e in events {
        if let Err(err) = app.session.pointer_with_tool(&mut zoom.tool, &e, vi) {
            app.status(err.to_string());
        }
    }
    app.after_engine();
    // Keep ownership until pointer release even if the shortcut was released first, so an Up
    // cannot leak into the previous tool and edit the document.
    ui.data_mut(|d| d.insert_temp(id, zoom));
    true
}

/// Move the view's content by a screen-space distance `d` (rotated views move in screen axes).
pub(super) fn pan(app: &mut DesignApp, xf: &Xf, d: egui::Vec2) {
    let d = xf.unrotate_delta(d);
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(v.origin.x - d.x as f64 / v.zoom, v.origin.y - d.y as f64 / v.zoom);
    }
}

/// A scrollbar thumb on a track `track` pixels long, for `visible` (the view) within `content`
/// (the pasteboard), both spans along the bar's axis in screen pixels: the thumb's start and
/// length on the track, and the content pixels one thumb pixel moves (0 when nothing scrolls).
fn thumb_span(track: f32, content: (f32, f32), visible: (f32, f32)) -> (f32, f32, f32) {
    let track = track.max(0.0);
    let (lo, hi) = (content.0.min(visible.0), content.1.max(visible.1));
    let (total, shown) = (hi - lo, visible.1 - visible.0);
    let scrollable = total - shown;
    if !total.is_finite() || !shown.is_finite() || total <= 0.0 || scrollable <= 0.0 {
        return (0.0, track, 0.0);
    }
    let len = (track * shown / total).max(MIN_THUMB.min(track));
    let free = track - len;
    if free <= 0.0 {
        return (0.0, track, 0.0);
    }
    ((visible.0 - lo) / scrollable * free, len, scrollable / free)
}

/// The track and thumb of the vertical (or horizontal) scrollbar of the view `screen` inside
/// `outer`, and the content pixels per thumb pixel.
fn scrollbar_rects(outer: Rect, screen: Rect, content: Rect, vertical: bool) -> (Rect, Rect, f32) {
    if vertical {
        let track = Rect::from_min_max(pos2(outer.max.x - SCROLLBAR, screen.min.y), pos2(outer.max.x, screen.max.y));
        let (start, len, k) = thumb_span(track.height(), (content.min.y, content.max.y), (screen.min.y, screen.max.y));
        (track, Rect::from_min_size(pos2(track.min.x, track.min.y + start), vec2(SCROLLBAR, len)), k)
    } else {
        let track = Rect::from_min_max(pos2(screen.min.x, outer.max.y - SCROLLBAR), pos2(screen.max.x, outer.max.y));
        let (start, len, k) = thumb_span(track.width(), (content.min.x, content.max.x), (screen.min.x, screen.max.x));
        (track, Rect::from_min_size(pos2(track.min.x + start, track.min.y), vec2(len, SCROLLBAR)), k)
    }
}

/// How far a scrollbar interaction pans the view along its axis: a thumb drag follows the pointer,
/// a click on the track beside the thumb pages by one view.
fn scrollbar_move(resp: &egui::Response, grabbed: bool, vertical: bool, thumb: Rect, page: f32, k: f32) -> f32 {
    if grabbed {
        let d = resp.drag_delta();
        return -(if vertical { d.y } else { d.x }) * k;
    }
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (p, a, b) = if vertical { (p.y, thumb.min.y, thumb.max.y) } else { (p.x, thumb.min.x, thumb.max.x) };
        if p < a {
            return page;
        } else if p > b {
            return -page;
        }
    }
    0.0
}

/// The view's scrollbars, on the right and bottom edges of `outer`: they span the pasteboard;
/// dragging a thumb pans, clicking the track pages by one view.
pub(super) fn scrollbars(app: &mut DesignApp, ui: &mut egui::Ui, outer: Rect, screen: Rect, rect: Rect, t: &Tokens) {
    let content = |app: &DesignApp| {
        let st = app.session.active()?;
        let v = app.view()?;
        let xf = Xf::new(rect, v);
        Some((xf, xf.rect(CanvasLayout::new(&st.doc, st.editing_parents).pasteboard)))
    };
    let Some((xf, cb)) = content(app) else { return };
    let press = ui.input(|i| i.pointer.press_origin());
    let mut hot = [false; 2];
    for (n, vertical) in [true, false].into_iter().enumerate() {
        let (track, thumb, k) = scrollbar_rects(outer, screen, cb, vertical);
        let id = egui::Id::unique(("canvas_scrollbar", app.pane, vertical));
        let resp = ui.interact(track, id, Sense::click_and_drag());
        let grab_id = id.with("grab");
        if resp.drag_started() {
            let on_thumb = press.is_some_and(|p| thumb.contains(p));
            ui.data_mut(|d| d.insert_temp(grab_id, on_thumb));
        }
        let grabbed = resp.dragged() && ui.data(|d| d.get_temp::<bool>(grab_id)).unwrap_or(false);
        if !resp.dragged() {
            ui.data_mut(|d| d.remove::<bool>(grab_id));
        }
        let page = if vertical { screen.size().y } else { screen.size().x };
        let moved = scrollbar_move(&resp, grabbed, vertical, thumb, page, k);
        if moved != 0.0 && moved.is_finite() {
            pan(app, &xf, if vertical { vec2(0.0, moved) } else { vec2(moved, 0.0) });
        }
        if let Some(slot) = hot.get_mut(n) {
            *slot = grabbed || resp.hovered();
        }
    }
    // Painted from the view as it is after this frame's input.
    let Some((_, cb)) = content(app) else { return };
    let painter = ui.painter_at(outer);
    let right = Rect::from_min_max(pos2(outer.max.x - SCROLLBAR, outer.min.y), outer.max);
    let bottom = Rect::from_min_max(pos2(outer.min.x, outer.max.y - SCROLLBAR), outer.max);
    painter.rect_filled(right, 0.0, t.panel_darker);
    painter.rect_filled(bottom, 0.0, t.panel_darker);
    for (n, vertical) in [true, false].into_iter().enumerate() {
        let (_, thumb, _) = scrollbar_rects(outer, screen, cb, vertical);
        let alpha = if hot.get(n).copied().unwrap_or(false) { 0.6 } else { 0.35 };
        painter.rect_filled(thumb.shrink(2.0), SCROLLBAR / 2.0 - 2.0, t.text.gamma_multiply(alpha));
    }
}

#[cfg(test)]
mod scrollbar_tests {
    use egui::{Event, Modifiers, PointerButton};
    use egui_kittest::Harness;

    use super::*;
    use crate::test_window::{Window, open, wheel};

    fn window() -> Harness<'static, Window> {
        let mut h = open(DesignApp::new(designcraft_engine::Session::new(), crate::Services::default()), vec2(1440.0, 900.0));
        // Zoomed in so both axes scroll.
        set_zoom(&mut h.state_mut().app, 2.0);
        h.run_steps(3);
        h
    }

    fn origin(h: &Harness<'static, Window>) -> Point {
        h.state().app.view().map(|v| v.origin).unwrap_or_default()
    }

    /// The scrollbar's track and thumb as the canvas lays them out for the current view, and
    /// the content pixels per thumb pixel.
    fn bar(h: &Harness<'static, Window>, vertical: bool) -> (Rect, Rect, f32) {
        let app = &h.state().app;
        let rect = app.canvas_rect.unwrap();
        let st = app.session.active().unwrap();
        let xf = Xf::new(rect, app.view().unwrap());
        let cb = xf.rect(CanvasLayout::new(&st.doc, st.editing_parents).pasteboard);
        let outer = Rect::from_min_max(rect.min, rect.max + vec2(SCROLLBAR, SCROLLBAR));
        scrollbar_rects(outer, rect, cb, vertical)
    }

    /// A primary-button drag from `a` to `b` in a few moves, with `mods` held throughout.
    fn drag(h: &mut Harness<'static, Window>, a: Pos2, b: Pos2, mods: Modifiers) {
        h.event(Event::ModifiersChanged(mods));
        h.hover_at(a);
        h.step();
        h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: mods });
        h.step();
        for n in 1..=4 {
            h.hover_at(a + (b - a) * n as f32 / 4.0);
            h.step();
        }
        h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: mods });
        h.event(Event::ModifiersChanged(Modifiers::NONE));
        h.run_steps(3);
    }

    /// Scrollbars sit immediately outside the view, have non-full-size thumbs and use a positive content scale.
    #[test]
    fn document_views_have_scrollbars_beside_the_view() {
        let h = window();
        let rect = h.state().app.canvas_rect.unwrap();
        for vertical in [true, false] {
            let r = h.ctx.read_response(egui::Id::unique(("canvas_scrollbar", 0u8, vertical))).expect("scrollbar").rect;
            let (track, thumb, k) = bar(&h, vertical);
            assert_eq!(r, track);
            assert!(k > 0.0, "the zoomed-in view scrolls");
            if vertical {
                assert_eq!((r.min.x, r.width(), r.min.y, r.max.y), (rect.max.x, SCROLLBAR, rect.min.y, rect.max.y));
                assert!(thumb.height() < track.height());
            } else {
                assert_eq!((r.min.y, r.height(), r.min.x, r.max.x), (rect.max.y, SCROLLBAR, rect.min.x, rect.max.x));
                assert!(thumb.width() < track.width());
            }
        }
    }

    #[test]
    fn dragging_a_thumb_pans_and_clicking_the_track_pages() {
        let mut h = window();
        let zoom = h.state().app.view().unwrap().zoom;
        // Vertical thumb: down 40 px moves the view down by 40 thumb pixels' worth of content.
        let (_, thumb, k) = bar(&h, true);
        let before = origin(&h);
        drag(&mut h, thumb.center(), thumb.center() + vec2(0.0, 40.0), Modifiers::NONE);
        let after = origin(&h);
        let want = 40.0 * k as f64 / zoom;
        assert!((after.y - before.y - want).abs() < want * 0.2, "{before:?} → {after:?}, want +{want}");
        assert_eq!(after.x, before.x);
        // Horizontal thumb: left 30 px.
        let (_, thumb, k) = bar(&h, false);
        drag(&mut h, thumb.center(), thumb.center() - vec2(30.0, 0.0), Modifiers::NONE);
        let next = origin(&h);
        let want = 30.0 * k as f64 / zoom;
        assert!((after.x - next.x - want).abs() < want * 0.2, "{after:?} → {next:?}, want -{want}");
        assert_eq!(next.y, after.y);
        // A click on the vertical track above the thumb pages up by one view.
        let (track, thumb, _) = bar(&h, true);
        assert!(thumb.min.y - track.min.y > 4.0);
        crate::test_window::click_at(&mut h, pos2(track.center().x, track.min.y + 2.0));
        let paged = origin(&h);
        let view = h.state().app.canvas_rect.unwrap().height() as f64 / zoom;
        assert!((next.y - paged.y - view).abs() < 0.01, "{next:?} → {paged:?}, want -{view}");
    }

    /// Wheel scrolling moves both thumbs and keeps the painted scrollbar rect in sync.
    #[test]
    fn scrolling_the_view_moves_the_thumbs() {
        let mut h = window();
        let (_, v0, _) = bar(&h, true);
        let (_, h0, _) = bar(&h, false);
        let c = h.state().app.canvas_rect.unwrap().center();
        wheel(&mut h, c, vec2(-50.0, -80.0));
        let (_, v1, _) = bar(&h, true);
        let (_, h1, _) = bar(&h, false);
        assert!(v1.min.y > v0.min.y && h1.min.x > h0.min.x, "{v0:?} → {v1:?}, {h0:?} → {h1:?}");
        let r = h.ctx.read_response(egui::Id::unique(("canvas_scrollbar", 0u8, true))).expect("scrollbar");
        assert_eq!(r.rect, bar(&h, true).0);
    }

    #[test]
    fn alt_drag_while_editing_text_pans_and_keeps_the_caret() {
        let mut h = window();
        let app = &mut h.state_mut().app;
        let rect = app.canvas_rect.unwrap();
        let xf = Xf::new(rect, app.view().unwrap());
        // A text frame filling the visible part of the page, with the caret in it.
        let (a, b) = (xf.to_canvas(rect.min + vec2(20.0, 20.0)), xf.to_canvas(rect.max - vec2(20.0, 20.0)));
        let r = app.run("frame.create", json!({"rect": [a.x, a.y, b.x, b.y], "content": "text", "text": "Hello brave world"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        app.run("tool.select", json!({"tool": "type"})).unwrap();
        app.run("text.select", json!({"story": sid, "anchor": 3, "focus": 3})).unwrap();
        assert!(app.session.wants_text());
        h.run_steps(2);
        let caret = h.state().app.session.doc().unwrap().selection.text;
        let before = origin(&h);
        let zoom = h.state().app.view().unwrap().zoom;
        let from = rect.center();
        drag(&mut h, from, from + vec2(60.0, 40.0), Modifiers::ALT);
        let after = origin(&h);
        assert!(after.x < before.x && after.y < before.y, "{before:?} → {after:?}");
        assert!((before.x - after.x - 60.0 / zoom).abs() < 15.0 / zoom, "{before:?} → {after:?}");
        assert_eq!(h.state().app.session.doc().unwrap().selection.text, caret);
        assert!(h.state_mut().app.session.wants_text(), "still editing text");
        // Without Alt the same drag selects text and leaves the view alone.
        drag(&mut h, from, from + vec2(60.0, 40.0), Modifiers::NONE);
        assert_eq!(origin(&h), after);
    }
}
