use super::column_guides::{COLUMN_DRAG, ColumnDrag, column_guide_drag};
use super::guides::guide_drag;
use super::ime::{finish_pending_ime_delete, ime_event};
use super::rulers::zero_point_drag;
use super::scroll::{mods, navigation_space, pan, spring_zoom_input, spring_zoom_mode, wheel_alt};
use super::view::zoom_at;
use super::{
    CanvasLayout, Clicks, Cursor, DVec2, DesignApp, Item, Mods, Point, PointerEvent, PointerKind, Pos2, Rect, ScreenMode, SpreadRef, Stroke,
    StrokeKind, Tokens, ToolKey, Xf, json, pos2, vec2,
};

/// Whether the pointer this canvas's window last saw is in another of the app's windows: a
/// floating panel's OS window, or the other window of New Window.
pub(super) fn pointer_elsewhere(app: &DesignApp, ctx: &egui::Context) -> bool {
    let mut others = crate::panel_docking::native_viewports(app, ctx);
    if app.second_window && !ctx.embed_viewports() {
        others.extend([egui::ViewportId::ROOT, crate::second_window_viewport()]);
    }
    crate::cursors::pointer_elsewhere(ctx, &others)
}

/// The loaded text cursor's badge beside the system cursor: lines of text, a closed chain
/// (a click threads) or a broken one (a click unthreads). Drawn in code.
pub(super) fn draw_cursor_badge(painter: &egui::Painter, p: Pos2, c: Cursor, t: &Tokens) {
    if !matches!(c, Cursor::LoadedText | Cursor::ThreadLink | Cursor::Unthread) {
        return;
    }
    let r = Rect::from_min_size(p + vec2(12.0, 12.0), vec2(20.0, 16.0));
    painter.rect_filled(r, 3.0, t.measure_bg);
    let ink = Stroke::new(1.5, t.measure_text);
    let c0 = r.center();
    match c {
        Cursor::LoadedText => {
            for (dy, w) in [(-4.0, 12.0), (0.0, 12.0), (4.0, 8.0)] {
                painter.line_segment([pos2(c0.x - 6.0, c0.y + dy), pos2(c0.x - 6.0 + w, c0.y + dy)], ink);
            }
        }
        _ => {
            let gap = if c == Cursor::Unthread { 2.5 } else { -2.0 };
            for side in [-1.0, 1.0] {
                let link = Rect::from_center_size(pos2(c0.x + side * (3.5 + gap / 2.0), c0.y), vec2(9.0, 6.0));
                painter.rect_stroke(link, 3.0, ink, StrokeKind::Middle);
            }
            if c == Cursor::Unthread {
                painter.line_segment([pos2(c0.x + 2.0, c0.y - 6.0), pos2(c0.x - 2.0, c0.y + 6.0)], ink);
            }
        }
    }
}

/// A text field had keyboard focus when this frame began, per window: egui keeps one focus per
/// viewport, so a field focused in the main window must not mute the second window's canvas.
fn field_focused_id(ctx: &egui::Context) -> egui::Id {
    egui::Id::unique("canvas_field_focused").with(ctx.viewport_id())
}

/// Called before the panels draw. A panel field gives up focus on Enter or Tab while it draws,
/// before the canvas: without this the canvas would take that same key for the text.
pub fn begin_frame(ctx: &egui::Context) {
    let (id, focused) = (field_focused_id(ctx), ctx.text_edit_focused());
    ctx.data_mut(|d| d.insert_temp(id, focused));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let now = ctx.input(|i| i.modifiers);
        // The modifiers last frame ended with are the ones this frame's events began with.
        ctx.data_mut(|d| {
            let (_, end) = d.get_temp::<(egui::Modifiers, egui::Modifiers)>(modifiers_id()).unwrap_or((now, now));
            d.insert_temp(modifiers_id(), (end, now));
        });
    }
}

/// (held when this frame's events began, held after them).
#[cfg(not(target_arch = "wasm32"))]
fn modifiers_id() -> egui::Id {
    egui::Id::unique("canvas-frame-modifiers")
}

/// A canvas context menu asked for by a right-click; `open` until the popup is open after a frame.
#[derive(Clone, Copy, Debug)]
struct MenuRequest {
    kind: crate::menus::ContextMenu,
    open: bool,
}

/// The popup id of the canvas context menu in split-view pane `pane`.
fn context_menu_id(pane: u8) -> egui::Id {
    egui::Id::unique(("canvas_context_menu", pane))
}

/// Was a canvas context menu open (in either pane of this window) when this frame began?
/// Pane 0 shows its menu, which a click or Escape may close, before pane 1 handles input; that
/// click or key still belongs to the menu. Popups are per window (viewport), and so is input.
pub fn context_menu_open(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let key = egui::Id::unique("canvas_context_menu_open").with(ctx.viewport_id());
    if let Some((p, open)) = ctx.data(|d| d.get_temp::<(u64, bool)>(key))
        && p == pass
    {
        return open;
    }
    let open = (0..2).any(|pane| egui::Popup::is_id_open(ctx, context_menu_id(pane)));
    ctx.data_mut(|d| d.insert_temp(key, (pass, open)));
    open
}

/// Shows the context menu a right-click asked for. A right-click while the menu is open closes it
/// in that frame (it was a click outside the menu), so the request stays until a frame ends with
/// the menu open: it reopens in the next frame where the new click was.
pub(super) fn context_menu(app: &mut DesignApp, ui: &egui::Ui, resp: &egui::Response) {
    let id = context_menu_id(app.pane);
    let Some(req) = ui.data(|d| d.get_temp::<MenuRequest>(id)) else { return };
    egui::Popup::menu(resp)
        .id(id)
        .open_memory(req.open.then_some(egui::SetOpenCommand::Bool(true)))
        .at_pointer_fixed()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| crate::menus::context_menu(app, ui, req.kind));
    if req.open {
        if egui::Popup::is_id_open(ui.ctx(), id) {
            ui.data_mut(|d| d.insert_temp(id, MenuRequest { open: false, ..req }));
        } else {
            ui.ctx().request_repaint();
        }
    }
}

/// Prepares a right-click at canvas point `p`: picks the menu and the selection it acts on.
/// - Typing: a click in a text frame places the caret there as a left click would, except
///   inside the selected text, which stays selected; elsewhere the text is left as it is.
/// - On an object: it is selected first unless it already is, as the Selection tool would.
/// - On nothing: the selection stays.
fn context_target(app: &mut DesignApp, p: Point, zoom: f64) -> crate::menus::ContextMenu {
    use crate::menus::ContextMenu;
    let hit = item_at(app, p, zoom);
    if app.session.wants_text() {
        let text_frame = hit.filter(|(id, _)| app.session.active().and_then(|st| st.doc.item(*id)).is_some_and(Item::is_text_frame));
        if let Some((id, sp)) = text_frame {
            // A press in the selected text keeps it for dragging; end that press without a move.
            if app.run("text.placeCaret", json!({"frame": id.0, "point": [sp.x, sp.y]})).is_ok_and(|r| r["dragging"] == json!(true)) {
                let _ = app.run("text.release", json!({"frame": id.0, "point": [sp.x, sp.y], "moved": true}));
            }
        }
        return ContextMenu::Text;
    }
    let Some((id, _)) = hit else { return ContextMenu::Canvas };
    let direct = app.session.tool_id() == "directSelection";
    let Some((target, selected)) = app.session.active().map(|st| {
        let target = if direct { id } else { st.doc.top_level_of(id).unwrap_or(id) };
        (target, st.selection.contains(target))
    }) else {
        return ContextMenu::Canvas;
    };
    if !selected {
        let _ = app.run("selection.set", json!({"ids": [target.0], "content": direct}));
    }
    ContextMenu::Object
}

/// The frontmost item under canvas point `p` and that point in its spread, hit as the tools do.
fn item_at(app: &DesignApp, p: Point, zoom: f64) -> Option<(designcraft_doc::ItemId, Point)> {
    let st = app.session.active()?;
    let (sr, sp) = CanvasLayout::new(&st.doc, st.editing_parents).spread_at(p)?;
    let tol = 4.0 / zoom.max(1e-6);
    let id = match sr {
        SpreadRef::Doc(si) => st.doc.hit_item(si, sp, tol)?,
        _ => st.doc.spread(sr)?.items.iter().rev().find(|it| !it.locked && designcraft_doc::edit_hit(it, sp, tol))?.id,
    };
    Some((id, sp))
}

/// Wheel, pinch and Alt/Ctrl/Cmd + wheel over the canvas: zoom at the pointer, else scroll.
fn wheel_navigate(app: &mut DesignApp, ui: &egui::Ui, resp: &egui::Response, xf: &Xf) {
    let alt = wheel_alt(ui);
    if resp.hovered() {
        let (scroll, zoom_delta, m, hover) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.pointer.hover_pos()));
        if let Some(hp) = hover {
            if zoom_delta != 1.0 {
                zoom_at(app, hp, zoom_delta as f64);
            } else if m.command && scroll.y != 0.0 {
                zoom_at(app, hp, (1.0 + scroll.y as f64 * 0.003).clamp(0.5, 2.0));
            } else if alt && scroll != egui::Vec2::ZERO {
                // Alt + wheel zooms by as much as Ctrl/Cmd + wheel, which egui turns into `zoom_delta`.
                let speed = ui.ctx().options(|o| o.input_options.scroll_zoom_speed);
                zoom_at(app, hp, (speed * (scroll.x + scroll.y)).exp() as f64);
            } else if scroll != egui::Vec2::ZERO
                && let Some(v) = app.view_mut()
            {
                let scroll = xf.unrotate_delta(scroll);
                v.origin = Point::new(v.origin.x - scroll.x as f64 / v.zoom, v.origin.y - scroll.y as f64 / v.zoom);
            }
        }
    }
}

/// Where the primary button went down this pass, if egui no longer holds the press origin.
fn recent_primary_press(ui: &egui::Ui) -> Option<Pos2> {
    ui.input(|i| {
        i.events
            .iter()
            .rev()
            .find_map(|event| match event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } => Some(*pos),
                _ => None,
            })
            .filter(|pos| pos.is_finite())
    })
}

/// Power Zoom: whether a press held still with the Hand tool (`active`) has now lasted half a
/// second. Keeps the hold's state under `hold_id`, and clears it when the hold ends.
fn power_zoom_fires(ui: &egui::Ui, hold_id: egui::Id, active: bool, latest: Option<Pos2>) -> bool {
    if !active {
        ui.data_mut(|d| d.remove::<(f64, bool)>(hold_id));
        return false;
    }
    let (now, origin_pos) = ui.input(|i| (i.time, i.pointer.press_origin()));
    let (start, fired): (f64, bool) = ui.data(|d| d.get_temp(hold_id)).unwrap_or((now, false));
    let still = origin_pos.zip(latest).is_some_and(|(o, l)| (o - l).length() < 3.0);
    if !fired && still && now - start >= 0.5 {
        ui.data_mut(|d| d.insert_temp(hold_id, (start, true)));
        return true;
    }
    ui.data_mut(|d| d.insert_temp(hold_id, (start, fired)));
    if !fired {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    false
}

/// The zero point, ruler guide and column guide drags; whether one took this pass.
fn ruler_drags(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf, space: bool, menu_open: bool) -> bool {
    // The zero point and ruler guides need the rulers (hidden while the view is rotated).
    if !space && !menu_open && xf.rot == 0 && zero_point_drag(app, ui, resp, rect, xf) {
        return true;
    }
    // Hidden or locked guides can't stay selected.
    if (!app.ui.guides || app.ui.guides_locked) && app.session.active().is_some_and(|st| !st.selection.guides.is_empty()) {
        let _ = app.run("guide.deselect", json!({}));
    }
    if !space && !menu_open && xf.rot == 0 && guide_drag(app, ui, resp, rect, xf) {
        return true;
    }
    if !space && !menu_open && xf.rot == 0 && column_guide_drag(app, ui, resp, rect, xf) {
        return true;
    }
    false
}

/// Hand-tool panning (Space-drag, middle drag, or Alt held at a text press); whether it took this pass.
fn hand_pan(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, xf: &Xf, space: bool, wants_text: bool, menu_open: bool) -> bool {
    let tool_drag: bool = ui.data(|d| d.get_temp(egui::Id::unique(("canvas_pointer_down", app.pane)))).unwrap_or(false);
    let hand = space && !ui.input(|i| i.modifiers.command);
    if !tool_drag && ((hand && resp.dragged()) || resp.dragged_by(egui::PointerButton::Middle)) {
        let d = xf.unrotate_delta(resp.drag_delta());
        if let Some(v) = app.view_mut() {
            v.origin = Point::new(v.origin.x - d.x as f64 / v.zoom, v.origin.y - d.y as f64 / v.zoom);
        }
        return true;
    }
    // Editing text (Space types a space): Alt held at the press gives the Hand tool until the
    // button is released, and the caret stays. Alt pressed later in a text drag copies the text.
    let alt_hand_id = egui::Id::unique(("canvas_alt_hand", app.pane));
    let mut alt_hand: bool = ui.data(|d| d.get_temp(alt_hand_id)).unwrap_or(false);
    let (press, alt, primary_down) = ui.input(|i| (i.pointer.primary_pressed(), i.modifiers.alt, i.pointer.primary_down()));
    if press && alt && wants_text && !tool_drag && !menu_open && resp.hovered() {
        alt_hand = true;
    }
    if alt_hand {
        if resp.dragged() {
            pan(app, xf, resp.drag_delta());
        }
        ui.data_mut(|d| d.insert_temp(alt_hand_id, primary_down));
        return true;
    }
    false
}

/// A release ends the press run's last click; remembers where and when for the next press.
fn note_release(ui: &egui::Ui, clicks_id: egui::Id, latest: Option<Pos2>, now: f64) {
    if let Some(c) = ui.data(|d| d.get_temp::<Clicks>(clicks_id)) {
        let dist = ui.ctx().options(|o| o.input_options.max_click_dist);
        ui.data_mut(|d| d.insert_temp(clicks_id, release_clicks(c, latest.unwrap_or(c.pos), now, dist)));
    }
}

/// Give an open menu priority, then handle wheel/zoom/pan, spring Zoom, guide drags and primary pointer routing.
pub(super) fn handle_input(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect) {
    // While a context menu is open (in either pane), the click that closes it and the keys go to the menu.
    let menu_open = context_menu_open(ui.ctx());
    let Some(v) = app.view().copied() else { return };
    let xf = Xf::new(rect, &v);
    let wants_text = app.session.wants_text();
    let space = navigation_space(app, ui.ctx());
    wheel_navigate(app, ui, resp, &xf);
    let zoom = spring_zoom_mode(app, ui.ctx(), space);
    if spring_zoom_input(app, ui, resp, &xf, zoom) {
        return;
    }
    // Space-drag = hand (unless a tool drag is under way: then Space is a modifier, e.g. Live
    // Distribute while resizing). Middle-drag pans too, with any tool; tools only
    // see the primary button.
    // A column guide drag keeps the pointer until the button is up, whatever Space or a menu does
    // meanwhile.
    if ui.data(|d| d.get_temp::<ColumnDrag>(egui::Id::unique((COLUMN_DRAG, app.pane)))).is_some() && column_guide_drag(app, ui, resp, rect, &xf) {
        return;
    }
    if hand_pan(app, ui, resp, &xf, space, wants_text, menu_open) {
        return;
    }
    if ruler_drags(app, ui, resp, rect, &xf, space, menu_open) {
        return;
    }
    let m = ui.input(|i| mods(i, space));
    let vi = app.view_info();
    let pos = |p: Pos2| xf.to_canvas(p);
    let mut events: Vec<PointerEvent> = Vec::new();
    let down_id = egui::Id::unique(("canvas_pointer_down", app.pane));
    let mut down: bool = ui.data(|d| d.get_temp(down_id)).unwrap_or(false);
    let clicks_id = egui::Id::unique(("canvas_clicks", app.pane));
    let (pressed, released, origin, latest, now) =
        ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin(), i.pointer.latest_pos(), i.time));
    // A complete click can arrive in one pass: release clears egui's held origin.
    // Recover the actual primary press position, preserving the origin for held gestures.
    let origin = origin.or_else(|| recent_primary_press(ui));
    let in_view = |o: Pos2| if xf.rot == 0 { rect.contains(o) } else { resp.rect.contains(o) };
    if pressed
        && !space
        && !menu_open
        && let Some(o) = origin.filter(|o| in_view(*o) && resp.hovered())
    {
        // Count presses in a row here: egui reports double and triple clicks only on release and
        // stops at three, but a press must know (word, line, paragraph, story).
        let (delay, dist) = ui.ctx().options(|o| (o.input_options.max_double_click_delay, o.input_options.max_click_dist));
        let n = press_count(ui.data(|d| d.get_temp(clicks_id)), o, now, delay, dist);
        ui.data_mut(|d| d.insert_temp(clicks_id, Clicks { at: f64::NEG_INFINITY, pos: o, n }));
        events.push(PointerEvent { kind: PointerKind::Down, pos: pos(o), mods: m, clicks: n });
        down = true;
    }
    // The place cursor draws a frame from 3 points (its own threshold), before egui starts a
    // drag (at `max_click_dist`): follow the pointer from there, so a release between the two
    // places the frame that was previewed.
    let place_drag = app.session.tool_id() == "placeGun" && origin.zip(latest).is_some_and(|(o, p)| (o - p).length() >= 3.0);
    if down
        && ((resp.dragged() && resp.drag_delta() != egui::Vec2::ZERO) || place_drag)
        && let Some(p) = latest
    {
        events.push(PointerEvent { kind: PointerKind::Drag, pos: pos(p), mods: m, clicks: 1 });
    }
    // Power Zoom: holding the Hand tool still for half a second turns the press into one.
    let hold_id = egui::Id::unique(("canvas_hold", app.pane));
    if power_zoom_fires(ui, hold_id, down && !released && app.session.tool_id() == "hand" && !m.alt, latest) {
        let p = pos(latest.unwrap_or(rect.center()));
        events.push(PointerEvent { kind: PointerKind::Up, pos: p, mods: m, clicks: 1 });
        events.push(PointerEvent { kind: PointerKind::Down, pos: p, mods: Mods { alt: true, ..m }, clicks: 1 });
    }
    if down && released {
        note_release(ui, clicks_id, latest, now);
        let p = latest.unwrap_or(rect.center());
        events.push(PointerEvent { kind: PointerKind::Up, pos: pos(p), mods: m, clicks: 1 });
        down = false;
    } else if !down && let Some(p) = resp.hover_pos() {
        events.push(PointerEvent { kind: PointerKind::Move, pos: pos(p), mods: m, clicks: 1 });
    }
    ui.data_mut(|d| d.insert_temp(down_id, down));
    for e in events {
        if let Err(err) = app.session.pointer(&e, vi) {
            app.status(err.to_string());
        }
    }
    app.after_engine();
    // Right-click: tools only see the primary button, so it starts no gesture; it opens the menu.
    if resp.secondary_clicked()
        && !down
        && !space
        && app.ui.screen_mode != ScreenMode::Presentation
        && let Some(o) = resp.interact_pointer_pos().filter(|o| in_view(*o))
    {
        let kind = context_target(app, xf.to_canvas(o), xf.zoom);
        ui.data_mut(|d| d.insert_temp(context_menu_id(app.pane), MenuRequest { kind, open: true }));
        ui.ctx().request_repaint();
    }
    if resp.clicked() || resp.drag_started() {
        resp.request_focus();
    }
    // Arrow keys, Tab and Escape belong to the canvas (caret, nudge, tab character, deselect).
    // Without the lock egui moves focus to a neighbouring widget, and the next Space or Return
    // clicks it while typing still reaches the text.
    let lock = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true };
    ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, lock));
    // Keyboard for the active tool. A field focused when the frame began keeps this frame's keys.
    let field_id = field_focused_id(ui.ctx());
    let field = ui.ctx().text_edit_focused() || ui.data(|d| d.get_temp::<bool>(field_id)).unwrap_or(false);
    if menu_open || modal_open(app) || (field && !resp.has_focus()) || !keyboard_pane(app) {
        return;
    }
    // Keys of a frame that began composing are the IME's too (a Backspace that empties the marked
    // text must not delete the character before it).
    let composing_at_start = app.session.tool_composing();
    let evs = ui.input(|i| i.events.clone());
    // Native only: egui-winit sends a ⌘+V chord as a paste without its key press, so the modifiers
    // held at the paste tell Paste in Place from Paste. The web also gets the key press, and the
    // shortcut table has already run the command. A chord's modifiers may be released in the same
    // frame, hence the modifiers held at each event.
    #[cfg(not(target_arch = "wasm32"))]
    let mut held =
        ui.data(|d| d.get_temp::<(egui::Modifiers, egui::Modifiers)>(modifiers_id())).map_or_else(|| ui.input(|i| i.modifiers), |(start, _)| start);
    for e in evs {
        #[cfg(not(target_arch = "wasm32"))]
        let chord = {
            if let egui::Event::ModifiersChanged(m) = &e {
                held = *m;
            }
            if matches!(e, egui::Event::Paste(_)) { crate::menus::paste_chord_command(app, held) } else { None }
        };
        let composing = composing_at_start || app.session.tool_composing();
        match e {
            egui::Event::Ime(ime) if wants_text => ime_event(app, ui.ctx(), ime, vi),
            // Keys the IME passes through come as text only while nothing is marked.
            egui::Event::Text(t) if wants_text && !composing => {
                let _ = app.run("text.insert", json!({"text": t}));
            }
            // The IME owns the keyboard until it commits: no typing, clipboard or editing keys.
            egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Key { .. } if composing => {}
            egui::Event::Paste(t) if wants_text => {
                // Formatted when the system clipboard still holds what was copied here;
                // Shift (⇧⌘V) pastes without formatting.
                let plain = ui.input(|i| i.modifiers.shift);
                let id = if plain { "edit.pasteWithoutFormatting" } else { "edit.paste" };
                let _ = app.run(id, json!({"text": t}));
            }
            egui::Event::Copy | egui::Event::Cut if wants_text => {
                // `run` puts the copied text on the system clipboard.
                let id = if matches!(e, egui::Event::Cut) { "edit.cut" } else { "edit.copy" };
                let _ = app.run(id, json!({}));
            }
            // Paste in Place's chord (⌘⌥⇧V unless rebound) arrives as a paste too.
            #[cfg(not(target_arch = "wasm32"))]
            egui::Event::Paste(_) if chord.is_some() => {
                if let Some(id) = chord
                    && crate::menus::enabled(app, id)
                {
                    crate::menus::activate(app, id, &serde_json::Value::Null);
                }
            }
            // Without a native menu bar the copy and paste keys arrive only as these events.
            egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) if !app.native_menu => {
                let (id, params) = match e {
                    egui::Event::Copy => ("edit.copy", serde_json::Value::Null),
                    egui::Event::Cut => ("edit.cut", serde_json::Value::Null),
                    egui::Event::Paste(t) => ("edit.paste", json!({"text": t})),
                    _ => continue,
                };
                if crate::menus::enabled(app, id) {
                    crate::menus::activate(app, id, &params);
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => tool_key(app, key, modifiers, vi),
            _ => {}
        }
    }
    finish_pending_ime_delete(app, ui.ctx());
    let _ = DVec2::ZERO;
}

/// A key for the active tool (caret movement, nudging, Backspace); Escape it leaves alone
/// deselects.
fn tool_key(app: &mut DesignApp, key: egui::Key, modifiers: egui::Modifiers, vi: designcraft_engine::ViewInfo) {
    let k = match key {
        egui::Key::ArrowLeft => ToolKey::Left,
        egui::Key::ArrowRight => ToolKey::Right,
        egui::Key::ArrowUp => ToolKey::Up,
        egui::Key::ArrowDown => ToolKey::Down,
        egui::Key::Enter => ToolKey::Enter,
        egui::Key::Escape => ToolKey::Escape,
        egui::Key::Backspace => ToolKey::Backspace,
        egui::Key::Delete => ToolKey::Delete,
        egui::Key::Tab => ToolKey::Tab,
        egui::Key::Home => ToolKey::Home,
        egui::Key::End => ToolKey::End,
        _ => return,
    };
    let m = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command, ctrl: modifiers.ctrl, space: false };
    let handled = app.session.tool_key(k, m, vi).unwrap_or(false);
    if !handled && k == ToolKey::Escape {
        let _ = app.run("edit.deselectAll", json!({}));
    }
    app.after_engine();
}

/// The keys a floating panel's own OS window left over once the shortcuts took theirs: the
/// clipboard and the active tool's keys, as the canvas handles them while no text is edited.
/// `modifiers` are those held in that window; egui-winit sends ⌘C, ⌘X and ⌘V as `Copy`, `Cut`
/// and `Paste` without their key press.
pub(crate) fn forwarded_keys(app: &mut DesignApp, events: Vec<egui::Event>, modifiers: egui::Modifiers) {
    if !keyboard_pane(app) || modal_open(app) || app.session.wants_text() || app.session.tool_composing() {
        return;
    }
    let vi = app.view_info();
    for e in events {
        #[cfg(not(target_arch = "wasm32"))]
        let chord = if matches!(e, egui::Event::Paste(_)) { crate::menus::paste_chord_command(app, modifiers) } else { None };
        // The web keeps floating panels in the page, so nothing is forwarded there.
        #[cfg(target_arch = "wasm32")]
        let chord: Option<&str> = {
            let _ = modifiers;
            None
        };
        let (id, params) = match (e, chord) {
            (egui::Event::Paste(_), Some(id)) => (id, serde_json::Value::Null),
            (egui::Event::Copy, _) if !app.native_menu => ("edit.copy", serde_json::Value::Null),
            (egui::Event::Cut, _) if !app.native_menu => ("edit.cut", serde_json::Value::Null),
            (egui::Event::Paste(t), _) if !app.native_menu => ("edit.paste", json!({"text": t})),
            (egui::Event::Key { key, pressed: true, modifiers, .. }, _) => {
                tool_key(app, key, modifiers, vi);
                continue;
            }
            _ => continue,
        };
        if crate::menus::enabled(app, id) {
            crate::menus::activate(app, id, &params);
        }
    }
}

/// A dialog or Quick Apply has the keyboard. The canvas keeps egui's focus behind a modal, and it
/// runs before the modal draws, so it must not also act on the Esc or Return that closes it.
pub(super) fn modal_open(app: &DesignApp) -> bool {
    app.ui.dialog.is_some() || app.ui.palette.is_some()
}

/// Does the current pane take the keyboard? Both panes (split, or the second window) see the same
/// events, so typing, tool keys, the clipboard and the input method serve only the focused pane.
pub(super) fn keyboard_pane(app: &DesignApp) -> bool {
    app.pane == app.focus_pane
}

/// The count of a press at `pos` at time `now`: one more than the last press when it comes within
/// the double-click delay of the last release and near where that release was, else 1.
pub(super) fn press_count(last: Option<Clicks>, pos: Pos2, now: f64, delay: f64, dist: f32) -> u8 {
    match last {
        Some(c) if now - c.at <= delay && (pos - c.pos).length() <= dist => c.n.saturating_add(1),
        _ => 1,
    }
}

/// The record a release at `pos` leaves: a release away from its press ended a drag, which is
/// not a click, so the next press counts from one.
pub(super) fn release_clicks(press: Clicks, pos: Pos2, now: f64, dist: f32) -> Clicks {
    let n = if (pos - press.pos).length() <= dist { press.n } else { 0 };
    Clicks { at: now, pos, n }
}
