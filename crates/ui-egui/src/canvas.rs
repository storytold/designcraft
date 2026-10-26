//! The document canvas: pasteboard, spreads (rendered by `designcraft-render`), guides, frame edges,
//! selection, text ports and threads, the text caret, rulers, and pointer/keyboard input.

use designcraft_compose as compose;
use designcraft_doc::{Content, Document, GuideSel, Item, SpreadRef};
use designcraft_engine::doc::Selection;
use designcraft_engine::{ImeMutation, ImeTextContext};
use designcraft_geom::{Affine, Point, Rect as DRect, Vec2 as DVec2};
use designcraft_tools::{CanvasLayout, Cursor, Mods, Overlay, PointerEvent, PointerKind, SpreadSlot, ToolKey};
use egui::ime_context::{CommittedText, ImeMutationStamp, ImeObservation, ImeTarget};
use egui::output::IMESurroundingText;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::{DesignApp, ScreenMode, View, theme::Tokens};

pub const RULER: f32 = 15.0;
/// Width of the view's scrollbars.
pub const SCROLLBAR: f32 = 10.0;
/// Shortest scrollbar thumb.
const MIN_THUMB: f32 = 18.0;

/// Canvas ↔ screen transform.
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    /// Top left of the (unrotated) view rectangle.
    pub min: Pos2,
    pub origin: Point,
    pub zoom: f64,
    /// Quarter turns clockwise about `pivot` (the view centre).
    pub rot: u8,
    pub pivot: Pos2,
}

/// Rotate `p` by `q` quarter turns clockwise about `c` (screen space, y down).
fn quarter(p: Pos2, c: Pos2, q: u8) -> Pos2 {
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    let (x, y) = match q % 4 {
        0 => (dx, dy),
        1 => (-dy, dx),
        2 => (-dx, -dy),
        _ => (dy, -dx),
    };
    pos2(c.x + x, c.y + y)
}

/// The unrotated view rectangle for a screen rectangle and a rotation (width and height swap
/// for quarter turns).
pub fn view_rect(screen: Rect, rot: u8) -> Rect {
    if rot % 2 == 1 { Rect::from_center_size(screen.center(), vec2(screen.height(), screen.width())) } else { screen }
}

impl Xf {
    pub fn new(rect: Rect, v: &View) -> Self {
        Xf { min: rect.min, origin: v.origin, zoom: v.zoom, rot: v.rotation % 4, pivot: rect.center() }
    }
    pub fn to_screen(&self, p: Point) -> Pos2 {
        let s = pos2(self.min.x + ((p.x - self.origin.x) * self.zoom) as f32, self.min.y + ((p.y - self.origin.y) * self.zoom) as f32);
        if self.rot == 0 { s } else { quarter(s, self.pivot, self.rot) }
    }
    /// A screen position in the unrotated view.
    pub fn unrotate(&self, p: Pos2) -> Pos2 {
        if self.rot == 0 { p } else { quarter(p, self.pivot, 4 - self.rot) }
    }
    /// A screen-space movement in the unrotated view.
    pub fn unrotate_delta(&self, d: egui::Vec2) -> egui::Vec2 {
        self.unrotate(self.pivot + d) - self.pivot
    }
    pub fn to_canvas(&self, p: Pos2) -> Point {
        let p = self.unrotate(p);
        Point::new(self.origin.x + (p.x - self.min.x) as f64 / self.zoom, self.origin.y + (p.y - self.min.y) as f64 / self.zoom)
    }
    pub fn rect(&self, r: DRect) -> Rect {
        Rect::from_two_pos(self.to_screen(Point::new(r.x0, r.y0)), self.to_screen(Point::new(r.x1, r.y1)))
    }
    /// Canvas → screen affine (for vello rendering: pixels = points × ppp).
    pub fn affine(&self, ppp: f64, area_min: Pos2) -> Affine {
        let unrotated = Affine::translate((self.min.x as f64, self.min.y as f64))
            * Affine::scale(self.zoom)
            * Affine::translate((-self.origin.x, -self.origin.y));
        let (cx, cy) = (self.pivot.x as f64, self.pivot.y as f64);
        let rot = Affine::translate((cx, cy)) * Affine::rotate(self.rot as f64 * std::f64::consts::FRAC_PI_2) * Affine::translate((-cx, -cy));
        Affine::scale(ppp) * Affine::translate((-(area_min.x as f64), -(area_min.y as f64))) * rot * unrotated
    }
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Fit the current spread (or page) in the canvas.
pub fn fit(app: &mut DesignApp, rect: Rect, what: &str) {
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let cur = current_slot(app, &layout).unwrap_or(0);
    let Some(slot) = layout.slots.get(cur) else { return };
    let mut b = slot.bounds;
    if what == "page"
        && let Some(sp) = st.doc.spread(slot.spread)
    {
        let center = app.view().map(|v| v.origin.x + rect.width() as f64 / 2.0 / v.zoom).unwrap_or(0.0);
        let c = slot.to_spread(Point::new(center, slot.bounds.center().y));
        let pi = sp.page_at_x(c.x).unwrap_or(0);
        b = slot.xf.transform_rect_bbox(sp.pages[pi].bounds());
    }
    if what == "all" {
        b = layout.slots.iter().map(|s| s.bounds).reduce(|a, b| a.union(b)).unwrap_or(b);
    }
    if what == "selection" {
        // Fit Selection in Window: the selected objects (on their spread's slot).
        let sel: Vec<designcraft_geom::Rect> = st
            .selection
            .items
            .iter()
            .filter_map(|id| {
                let loc = st.doc.find(*id)?;
                let slot = layout.slots.iter().find(|s| s.spread == loc.spread)?;
                Some(slot.xf.transform_rect_bbox(st.doc.item(*id)?.bounds()))
            })
            .collect();
        let Some(u) = sel.into_iter().reduce(|a, b| a.union(b)) else { return };
        b = u.inflate(u.width().max(u.height()) * 0.05 + 1.0, u.width().max(u.height()) * 0.05 + 1.0);
    }
    fit_bounds(app, rect, b);
}

/// Zoom and scroll so canvas rectangle `b` fills the canvas `rect`.
fn fit_bounds(app: &mut DesignApp, rect: Rect, b: DRect) {
    let pad = 40.0;
    let zoom =
        ((rect.width() as f64 - 2.0 * pad) / b.width().max(1e-6)).min((rect.height() as f64 - 2.0 * pad) / b.height().max(1e-6)).clamp(0.05, 40.0);
    let origin = Point::new(b.center().x - rect.width() as f64 / 2.0 / zoom, b.center().y - rect.height() as f64 / 2.0 / zoom);
    if let Some(v) = app.view_mut() {
        *v = View { zoom, origin, fitted: true, rotation: v.rotation };
    }
}

/// Fit spread `r` (a document or parent spread on the canvas as it is laid out now) in the canvas.
/// Before the canvas has a size, the next frame fits whatever spread is current.
pub fn show_spread(app: &mut DesignApp, r: SpreadRef) {
    let Some(st) = app.session.active() else { return };
    let Some(b) = CanvasLayout::new(&st.doc, st.editing_parents).slot(r).map(|s| s.bounds) else { return };
    match app.canvas_rect {
        Some(rect) => fit_bounds(app, rect, b),
        None => {
            if let Some(v) = app.view_mut() {
                v.fitted = false;
            }
        }
    }
}

/// Index of the slot nearest the view centre.
pub fn current_slot(app: &DesignApp, layout: &CanvasLayout) -> Option<usize> {
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
    layout
        .slots
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let da = (a.1.bounds.center().y - c.y).abs();
            let db = (b.1.bounds.center().y - c.y).abs();
            da.total_cmp(&db)
        })
        .map(|(i, _)| i)
}

/// The spread under the view centre (a parent spread while parents are edited).
pub fn current_spread(app: &DesignApp) -> Option<SpreadRef> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    Some(layout.slots.get(current_slot(app, &layout)?)?.spread)
}

/// The spread nearest the view centre and the centre in that spread's coordinates.
pub fn view_centre(app: &DesignApp) -> Option<(SpreadRef, Point)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let slot = layout.slots.get(current_slot(app, &layout)?)?;
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
    Some((slot.spread, slot.to_spread(c)))
}

/// Where a file dropped on the window lands: the spread under the pointer, or under the view
/// centre when the pointer isn't over the canvas, and that point in spread coordinates.
pub fn drop_point(app: &DesignApp, screen: Option<Pos2>) -> Option<(SpreadRef, Point)> {
    let rect = app.canvas_rect?;
    let Some(pos) = screen.filter(|p| rect.contains(*p)) else { return view_centre(app) };
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    layout.spread_at(Xf::new(rect, app.view()?).to_canvas(pos))
}

/// Absolute page index under the view centre.
pub fn current_page(app: &DesignApp) -> Option<usize> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let i = current_slot(app, &layout)?;
    let slot = layout.slots.get(i)?;
    let SpreadRef::Doc(si) = slot.spread else { return Some(0) };
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = slot.to_spread(Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, slot.bounds.center().y));
    let pi = st.doc.spreads[si].page_at_x(c.x).unwrap_or(0);
    Some(st.doc.first_page_of_spread(si) + pi)
}

/// The spread (document or parent) under the view centre and the page index within it: where
/// commands that act "on the page I am working on" should go.
pub fn current_spread_page(app: &DesignApp) -> Option<(SpreadRef, usize)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let i = current_slot(app, &layout)?;
    let slot = layout.slots.get(i)?;
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = slot.to_spread(Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, slot.bounds.center().y));
    let pg = st.doc.spread(slot.spread)?.page_at_x(c.x).unwrap_or(0);
    Some((slot.spread, pg))
}

/// While parent spreads are being edited: (parent spread index, page index within it) under the
/// view centre. `None` when document pages are shown.
pub fn current_parent_page(app: &DesignApp) -> Option<(usize, usize)> {
    let st = app.session.active()?;
    if !st.editing_parents {
        return None;
    }
    let layout = CanvasLayout::new(&st.doc, true);
    let i = current_slot(app, &layout)?;
    let slot = layout.slots.get(i)?;
    let SpreadRef::Parent(pi) = slot.spread else { return None };
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = slot.to_spread(Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, slot.bounds.center().y));
    let qi = st.doc.parents.get(pi)?.page_at_x(c.x).unwrap_or(0);
    Some((pi, qi))
}

/// Scroll so absolute page `abs` is centred (keeps zoom). A jump Go Back can return from.
pub fn go_to_page(app: &mut DesignApp, abs: usize) {
    note_jump(app, false);
    centre_page(app, abs);
    note_jump(app, true);
}

fn centre_page(app: &mut DesignApp, abs: usize) {
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, false);
    let Some(pr) = layout.page_rect(&st.doc, abs) else { return };
    let Some(rect) = app.canvas_rect else { return };
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(pr.center().x - rect.width() as f64 / 2.0 / v.zoom, pr.center().y - rect.height() as f64 / 2.0 / v.zoom);
    }
}

/// The document's pages by id, in page order.
fn page_ids(doc: &Document) -> Vec<designcraft_doc::ids::PageId> {
    doc.spreads.iter().flat_map(|s| s.pages.iter().map(|p| p.id)).collect()
}

/// The page in view and the view centre's offset from that page's centre. Parent pages have none,
/// nor has a view still waiting to be fitted (leaving parent pages for a document page).
pub(crate) fn page_in_view(app: &DesignApp) -> Option<(designcraft_doc::ids::PageId, DVec2)> {
    let st = app.session.active().filter(|st| !st.editing_parents)?;
    let (v, r) = (app.view().filter(|v| v.fitted)?, app.canvas_rect?);
    let abs = current_page(app)?;
    let id = st.doc.page(abs)?.id;
    let pr = CanvasLayout::new(&st.doc, false).page_rect(&st.doc, abs)?;
    let c = Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
    Some((id, c - pr.center()))
}

/// Layout ▸ Go Back / Go Forward record jumps (Go to Page, the page and spread commands, the Pages
/// panel), not scrolling: where the view is when a jump starts, and where the jump took it.
fn note_jump(app: &mut DesignApp, to: bool) {
    let Some((id, offset)) = page_in_view(app) else { return };
    if let Some(h) = app.page_history_mut() {
        if to { h.visit(id, offset) } else { h.leave(id, offset) }
    }
}

/// Whether Layout ▸ Go Back (`step` < 0) or Go Forward has a page to go to from the view as it is.
pub fn can_go_through_history(app: &DesignApp, step: isize) -> bool {
    let Some(st) = app.session.active().filter(|st| !st.editing_parents) else { return false };
    // A view still waiting to be fitted is refitted next frame, which would replace the destination.
    if !app.view().is_some_and(|v| v.fitted) {
        return false;
    }
    let ids = page_ids(&st.doc);
    let Some(mut h) = app.page_history().cloned() else { return false };
    if let Some((id, offset)) = page_in_view(app) {
        h.leave(id, offset);
    }
    h.can_go(step, |id| ids.contains(&id))
}

/// Layout ▸ Go Back (`step` < 0) or Go Forward: show the page viewed before or after this one, where
/// the view was on it (zoom kept).
pub fn go_through_history(app: &mut DesignApp, step: isize) {
    if !app.view().is_some_and(|v| v.fitted) {
        return;
    }
    note_jump(app, false);
    let Some(ids) = app.session.active().filter(|st| !st.editing_parents).map(|st| page_ids(&st.doc)) else { return };
    let Some((id, offset)) = app.page_history_mut().and_then(|h| h.go(step, |id| ids.contains(&id))) else { return };
    let Some(st) = app.session.active() else { return };
    let Some(pr) = ids.iter().position(|p| *p == id).and_then(|abs| CanvasLayout::new(&st.doc, false).page_rect(&st.doc, abs)) else { return };
    let Some(rect) = app.canvas_rect else { return };
    let c = pr.center() + offset;
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(c.x - rect.width() as f64 / 2.0 / v.zoom, c.y - rect.height() as f64 / 2.0 / v.zoom);
    }
}

/// Presentation: the spread in view, centred and as large as `rect` allows.
fn present_fit(app: &mut DesignApp, rect: Rect) {
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let Some(b) = current_slot(app, &layout).and_then(|i| layout.slots.get(i)).map(|s| s.bounds) else { return };
    if b.width() <= 0.0 || b.height() <= 0.0 {
        return;
    }
    let zoom = (rect.width() as f64 / b.width()).min(rect.height() as f64 / b.height()).clamp(0.05, 40.0);
    let origin = Point::new(b.center().x - rect.width() as f64 / 2.0 / zoom, b.center().y - rect.height() as f64 / 2.0 / zoom);
    if let Some(v) = app.view_mut() {
        v.zoom = zoom;
        v.origin = origin;
    }
}

/// Centre the spread `delta` spreads after (or before) the one in view (keeps zoom); stops at the
/// first and last spread. Parent spreads while they are being edited.
pub fn step_spread(app: &mut DesignApp, delta: isize) {
    // At the first or last spread only the view moves, within the spread: no jump for Go Back.
    let jump = spread_step(app, delta).is_some_and(|(cur, to)| cur != to);
    if jump {
        note_jump(app, false);
    }
    centre_spread(app, delta);
    if jump {
        note_jump(app, true);
    }
}

/// The slot in view and the one `delta` spreads after (or before) it, stopping at the ends.
fn spread_step(app: &DesignApp, delta: isize) -> Option<(usize, usize)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let cur = current_slot(app, &layout)?;
    Some((cur, cur.saturating_add_signed(delta).min(layout.slots.len().saturating_sub(1))))
}

fn centre_spread(app: &mut DesignApp, delta: isize) {
    let Some((_, to)) = spread_step(app, delta) else { return };
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let Some(b) = layout.slots.get(to).map(|s| s.bounds) else { return };
    let Some(rect) = app.canvas_rect else { return };
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(b.center().x - rect.width() as f64 / 2.0 / v.zoom, b.center().y - rect.height() as f64 / 2.0 / v.zoom);
    }
}

pub fn zoom_at(app: &mut DesignApp, screen: Pos2, factor: f64) {
    let Some(rect) = app.canvas_rect else { return };
    let Some(v) = app.view_mut() else { return };
    let xf = Xf::new(rect, v);
    let c = xf.to_canvas(screen);
    let screen = xf.unrotate(screen);
    let nz = (v.zoom * factor).clamp(0.05, 40.0);
    v.zoom = nz;
    v.origin = Point::new(c.x - (screen.x - rect.min.x) as f64 / nz, c.y - (screen.y - rect.min.y) as f64 / nz);
}

pub fn set_zoom(app: &mut DesignApp, z: f64) {
    let Some(rect) = app.canvas_rect else { return };
    let cur = app.view().map(|v| v.zoom).unwrap_or(1.0);
    zoom_at(app, rect.center(), z / cur);
}

/// How long egui (0.37 `WheelState`) keeps a scroll action's latched modifiers after the last
/// wheel event: 150 ms, or a second while fingers rest on a trackpad. The Alt latch below ends
/// with the action, exactly as egui ends Ctrl/Cmd, so the rest of an ended motion scrolls for
/// both keys and a later plain scroll is never taken for a zoom.
const SCROLL_ACTION_TIMEOUT: f64 = 0.150;
const RESTING_FINGER_TIMEOUT: f64 = 1.0;

/// Whether the wheel motion now arriving was made with Alt down. As egui does for Ctrl/Cmd, a
/// motion keeps its key to its end: the wheel is smoothed over a few frames, and a trackpad swipe
/// coasts on after the key is let go.
fn wheel_alt(ui: &egui::Ui) -> bool {
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

pub fn apply_view_request(app: &mut DesignApp, p: &Value) {
    if let Some(r) = p.get("showSpread").and_then(|v| serde_json::from_value::<SpreadRef>(v.clone()).ok()) {
        show_spread(app, r);
    }
    // Power Zoom (Hand tool, Alt-press): zoom out to the spread, aim a view-sized rectangle, zoom
    // back in where it was released.
    if let Some(phase) = p.get("powerZoom").and_then(Value::as_str) {
        let at = p.get("at").and_then(Value::as_array).map(|a| Point::new(a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)));
        match phase {
            "start" => {
                if let (Some(v), Some(rect)) = (app.view().copied(), app.canvas_rect) {
                    let centre = Xf::new(rect, &v).to_canvas(rect.center());
                    app.power_zoom = Some((v.zoom, at.unwrap_or(centre)));
                    fit(app, rect, "spread");
                }
            }
            "move" => {
                if let (Some(pz), Some(a)) = (&mut app.power_zoom, at) {
                    pz.1 = a;
                }
            }
            _ => {
                if let (Some((z, c)), Some(rect)) = (app.power_zoom.take(), app.canvas_rect)
                    && let Some(v) = app.view_mut()
                {
                    let c = at.unwrap_or(c);
                    v.zoom = z;
                    v.origin = Point::new(c.x - rect.width() as f64 / 2.0 / z, c.y - rect.height() as f64 / 2.0 / z);
                }
            }
        }
    }
    if let Some(d) = p.get("pan").and_then(Value::as_array) {
        let (dx, dy) = (d.first().and_then(Value::as_f64).unwrap_or(0.0), d.get(1).and_then(Value::as_f64).unwrap_or(0.0));
        if let Some(v) = app.view_mut() {
            v.origin = Point::new(v.origin.x - dx / v.zoom, v.origin.y - dy / v.zoom);
        }
    }
    if let (Some(a), Some(rect)) = (p.get("zoomAt").and_then(Value::as_array), app.canvas_rect)
        && let Some(v) = app.view()
    {
        let c = Point::new(a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
        let xf = Xf::new(rect, v);
        let s = xf.to_screen(c);
        zoom_at(app, s, p.get("factor").and_then(Value::as_f64).unwrap_or(2.0));
    }
}

fn mods(i: &egui::InputState, space: bool) -> Mods {
    Mods { shift: i.modifiers.shift, alt: i.modifiers.alt, cmd: i.modifiers.command, ctrl: i.modifiers.ctrl, space }
}

/// Held Space belongs to navigation only when neither the document nor a text field is typing.
fn navigation_space(app: &mut DesignApp, ctx: &egui::Context) -> bool {
    ctx.input(|i| i.focused && i.key_down(egui::Key::Space)) && !app.session.wants_text() && !ctx.text_edit_focused()
}

/// Allow temporary Zoom only for Cmd+Space with no pointer press and no busy tool.
fn spring_zoom_mode(app: &DesignApp, ctx: &egui::Context, space: bool) -> bool {
    let down = ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("canvas_pointer_down", app.pane)))).unwrap_or(false);
    space && ctx.input(|i| i.modifiers.command) && !down && !app.session.tool_busy()
}

#[derive(Clone, Default)]
struct SpringZoom {
    tool: designcraft_tools::ZoomTool,
    down: bool,
}

/// Lend the Zoom tool the gesture; the selected tool and document selection stay intact.
fn spring_zoom_input(app: &mut DesignApp, ui: &egui::Ui, resp: &egui::Response, xf: &Xf, active: bool) -> bool {
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

/// Lay out the canvas, scrollbars and rulers, presentation mode, fit/view state, input handling and painting.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let outer = ui.available_rect_before_wrap();
    let rot = app.view().map_or(0, |v| v.rotation % 4);
    let presenting = app.ui.screen_mode == ScreenMode::Presentation;
    // Scrollbars sit outside the view (right and bottom); rulers and the view share the rest.
    let bars = !presenting && app.session.active().is_some();
    let full = if bars { Rect::from_min_max(outer.min, outer.max - vec2(SCROLLBAR, SCROLLBAR)) } else { outer };
    // Rulers measure the unrotated layout, so they hide while the view is rotated.
    let rulers = app.ui.rulers && !presenting && rot == 0;
    let screen = if rulers { Rect::from_min_max(full.min + vec2(RULER, RULER), full.max) } else { full };
    let rect = view_rect(screen, rot);
    app.canvas_rect = Some(rect);
    let resp = ui.allocate_rect(full, Sense::click_and_drag());
    if presenting {
        // One spread at a time, as large as the screen allows; a click shows the next one,
        // Shift-click the previous one. Tools and selection don't see the input.
        present_fit(app, rect);
        if resp.clicked() {
            // The release that made the click says whether Shift was down.
            let shift = ui.input(|i| {
                i.modifiers.shift
                    || i.events.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: false, modifiers, .. } if modifiers.shift))
            });
            step_spread(app, if shift { -1 } else { 1 });
            present_fit(app, rect);
        }
    } else {
        if !app.view().is_some_and(|v| v.fitted) {
            fit(app, rect, "spread");
        }
        handle_input(app, ui, &resp, rect);
    }
    if bars {
        scrollbars(app, ui, outer, screen, rect, &t);
    }
    let Some(st) = app.session.active() else { return };
    let Some(&v) = app.view() else { return };
    let xf = Xf::new(rect, &v);
    let doc = st.doc.clone();
    let mut layout = CanvasLayout::new(&doc, st.editing_parents);
    if presenting && let Some(cur) = current_slot(app, &layout) {
        layout.slots = layout.slots.get(cur).copied().into_iter().collect();
    }
    let painter = ui.painter_at(screen);
    // Preview, Bleed, Slug and Presentation are all preview modes: guides, frame edges and the
    // other non-printing marks go, and only the shown area of each page keeps its content. They
    // differ in how much of the page shows, which `shown_area` gives.
    let preview = app.ui.screen_mode != ScreenMode::Normal;
    let bg = if app.ui.screen_mode == ScreenMode::Presentation { Color32::BLACK } else { t.pasteboard };
    painter.rect_filled(screen, 0.0, bg);
    // Page shadow: a hard 1.5 pt black offset on the right and bottom (InDesign 2026).
    for slot in &layout.slots {
        let r = xf.rect(slot.bounds);
        painter.rect_filled(r.translate(vec2(1.5, 1.5)), 0.0, Color32::BLACK);
    }
    // Paper under the rendered content: a page scrolled or jumped to shows its paper colour,
    // not the shadow's black, until its render arrives.
    let [r, g, b, _] = doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).unwrap_or(designcraft_color::Color::WHITE).to_rgba8(1.0);
    let paper = Color32::from_rgb(r, g, b);
    for slot in &layout.slots {
        for page in doc.spread(slot.spread).map(|sp| sp.pages.as_slice()).unwrap_or_default() {
            painter.rect_filled(xf.rect(slot.xf.transform_rect_bbox(page.bounds())), 0.0, paper);
        }
    }
    // Rendered content.
    render_texture(app, ui.ctx(), rect, &xf, &layout, preview);
    if let (Some(tex), Some(sh)) = (&app.canvas.texture, app.canvas.shown) {
        // Place the texture where its region is in the current view (shifted while panning,
        // scaled while a zoom re-render is pending).
        // The texture's corners in canvas space, placed through the view (rotation included).
        let (w, h) = (sh.size.0 as f64 / sh.zoom, sh.size.1 as f64 / sh.zoom);
        let o = sh.origin;
        let corners = [
            (o, pos2(0.0, 0.0)),
            (Point::new(o.x + w, o.y), pos2(1.0, 0.0)),
            (Point::new(o.x + w, o.y + h), pos2(1.0, 1.0)),
            (Point::new(o.x, o.y + h), pos2(0.0, 1.0)),
        ];
        let mut mesh = egui::Mesh::with_texture(tex.id());
        for (c, uv) in corners {
            mesh.vertices.push(egui::epaint::Vertex { pos: xf.to_screen(c), uv, color: Color32::WHITE });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        if preview {
            // Preview: only the shown area of each spread keeps its content.
            for slot in &layout.slots {
                let r = shown_area(app, &doc, &xf, slot);
                painter.with_clip_rect(r.intersect(screen)).add(egui::Shape::mesh(mesh.clone()));
            }
        } else {
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    crate::separations::paint(app, ui.ctx(), &painter, &xf, &layout, screen);
    let hair = 1.0 / ui.ctx().pixels_per_point();
    for slot in &layout.slots {
        painter.rect_stroke(xf.rect(slot.bounds), 0.0, Stroke::new(hair, Color32::from_rgb(0x4a, 0x4a, 0x4a)), StrokeKind::Outside);
    }
    if !preview {
        let drag = ui.data(|d| d.get_temp::<GuideDrag>(egui::Id::unique(GUIDE_DRAG)));
        let press = ui.data(|d| d.get_temp::<GuidePress>(egui::Id::unique(GUIDE_PRESS)));
        let moving = drag.as_ref().zip(press).and_then(|(g, press)| moving_guides(app, &xf, rect, g, press));
        draw_guides(app, &painter, &xf, &doc, &layout, &t, moving.as_ref());
        if let Some(g) = drag
            && let Some(p) = g.at
        {
            let col = Color32::from_rgb(74, 227, 255);
            let spot = guide_spot(app, &xf, &g);
            let line = spot.as_ref().map_or(p, |s| s.screen);
            let (a, b) = match g.orientation {
                designcraft_doc::Orientation::Horizontal => (pos2(rect.min.x, line.y), pos2(rect.max.x, line.y)),
                designcraft_doc::Orientation::Vertical => (pos2(line.x, rect.min.y), pos2(line.x, rect.max.y)),
            };
            // What caught it, as for a moved object.
            for mark in spot.iter().flat_map(|s| &s.marks) {
                if let Overlay::Guide { a, b } = mark {
                    painter.line_segment([xf.to_screen(*a), xf.to_screen(*b)], Stroke::new(1.0, t.smart_guide));
                }
            }
            // A new guide from the rulers with the Page tool is a liquid guide: dashed.
            if g.from.is_none() && app.session.tool_id() == "page" {
                dashed(&painter, a, b, Stroke::new(1.0, col), 6.0, 3.0);
            } else {
                painter.line_segment([a, b], Stroke::new(1.0, col));
            }
            // Where it is, beside the pointer (#1282).
            if let Some(text) = guide_readout(app, &xf, &g) {
                let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), t.text);
                let at = readout_at(p, galley.size(), rect);
                let bg = Rect::from_min_size(at, galley.size()).expand(3.0);
                painter.rect_filled(bg, 2.0, t.panel);
                painter.rect_stroke(bg, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
                painter.galley(at, galley, t.text);
            }
        }
        draw_frames(app, &painter, &xf, &doc, &layout);
        if app.ui.hidden_characters {
            draw_hidden_characters(app, &painter, &xf, &doc, &layout);
        }
        if app.ui.flattener_preview {
            draw_flattener_preview(&painter, &xf, &doc, &layout);
        }
        if app.ui.dynamic_spelling && !preview {
            draw_dynamic_spelling(app, ui.ctx(), &painter, &xf, &doc, &layout);
        }
    }
    if presenting {
        return;
    }
    let sel_rect = draw_selection(app, &painter, &xf, &doc, &layout);
    // Where the pointer moved on to another of the app's windows, this window's last pointer
    // position is stale: no hover feedback or tool cursor there.
    let elsewhere = pointer_elsewhere(app, ui.ctx());
    if !preview {
        // Content grabbers: the selected frame's and the hovered one's (Selection tool).
        let hover = ui.ctx().pointer_hover_pos().filter(|p| screen.contains(*p) && !elsewhere).map(|p| xf.to_canvas(p));
        let vi = app.view_info();
        for c in app.session.content_grabbers(hover, vi) {
            let c = xf.to_screen(c);
            painter.circle_stroke(c, 8.0, Stroke::new(1.5, Color32::from_white_alpha(170)));
            painter.circle_stroke(c, 4.0, Stroke::new(1.5, Color32::from_white_alpha(170)));
        }
    }
    draw_tool_overlays(app, &painter, &xf);
    ime_canvas_pass(app, ui.ctx(), &xf);
    if rulers {
        draw_rulers(app, ui, full, rect, &xf, &doc, &layout, &t);
        ruler_units_menus(app, &resp, full, rect, &doc);
    }
    context_menu(app, ui, &resp);
    if let Some(r) = sel_rect
        && !preview
        && r.intersects(screen)
    {
        crate::taskbar::show(app, &ui.ctx().clone(), r, screen);
    }
    // Cursor.
    if let Some(p) = resp.hover_pos().filter(|p| screen.contains(*p) && !elsewhere) {
        let wants_text = app.session.wants_text();
        let space = navigation_space(app, ui.ctx());
        let zoom = spring_zoom_mode(app, ui.ctx(), space);
        let middle = ui.input(|i| i.pointer.middle_down());
        let alt_hand: bool = ui.data(|d| d.get_temp(egui::Id::unique(("canvas_alt_hand", app.pane)))).unwrap_or(false);
        let tool_drag: bool = ui.data(|d| d.get_temp(egui::Id::unique(("canvas_pointer_down", app.pane)))).unwrap_or(false);
        let alt_text = wants_text && !tool_drag && ui.input(|i| i.modifiers.alt);
        let c = if middle || alt_hand {
            Cursor::HandGrab
        } else if zoom {
            if ui.input(|i| i.modifiers.alt) { Cursor::ZoomOut } else { Cursor::ZoomIn }
        } else if (space && !ui.input(|i| i.modifiers.command)) || alt_text {
            Cursor::Hand
        } else {
            app.session.cursor(xf.to_canvas(p), ui.input(|i| mods(i, false)), app.view_info())
        };
        // A column guide that a press would drag, or one being dragged: a resize cursor across it.
        let dragging = ui.data(|d| d.get_temp::<ColumnDrag>(egui::Id::unique((COLUMN_DRAG, app.pane)))).map(|g| g.spread);
        let column = dragging.or_else(|| {
            let menu_open = context_menu_open(ui.ctx());
            (!space && !zoom && !middle && !alt_hand && !tool_drag && !menu_open && xf.rot == 0)
                .then(|| column_guide_at(app, &xf, p).map(|h| h.0))
                .flatten()
        });
        let c = match column {
            Some(r) if spread_x_across(app, r) => Cursor::ResizeH,
            Some(_) => Cursor::ResizeV,
            None => c,
        };
        let icon = crate::cursors::show(ui.ctx(), &mut app.cursors, c, p);
        ui.ctx().set_cursor_icon(icon);
        draw_cursor_badge(&painter, p, c, &t);
    }
    // A second viewport has no panels, dialogs or Quick Apply after its canvas: observe here,
    // once, as the end of its pass. The root viewport observes at the end of `DesignApp::ui`.
    if ui.ctx().viewport_id() != egui::ViewportId::ROOT {
        ime_observe(app, ui.ctx());
    }
}

/// Whether the pointer this canvas's window last saw is in another of the app's windows: a
/// floating panel's OS window, or the other window of New Window.
fn pointer_elsewhere(app: &DesignApp, ctx: &egui::Context) -> bool {
    let mut others = crate::panel_docking::native_viewports(app, ctx);
    if app.second_window && !ctx.embed_viewports() {
        others.extend([egui::ViewportId::ROOT, crate::second_window_viewport()]);
    }
    crate::cursors::pointer_elsewhere(ctx, &others)
}

/// The loaded text cursor's badge beside the system cursor: lines of text, a closed chain
/// (a click threads) or a broken one (a click unthreads). Drawn in code.
fn draw_cursor_badge(painter: &egui::Painter, p: Pos2, c: Cursor, t: &Tokens) {
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

/// Render options for the canvas texture.
fn screen_options(app: &DesignApp, layout: &CanvasLayout, preview: bool, ppp: f64) -> designcraft_render::RenderOptions {
    designcraft_render::RenderOptions {
        printing_only: preview,
        greek_below_px: 3.0 * ppp,
        highlight_missing_fonts: !preview && app.session.prefs.highlight_substituted_fonts,
        highlight_hj: !preview && app.session.prefs.highlight_hj,
        condition_indicators: !preview,
        note_indicators: !preview && app.session.prefs.show_note_anchors,
        change_markup: !preview && app.session.prefs.show_added_text,
        tag_markers: !preview && app.ui.tag_markers,
        highlight_keeps: !preview && app.session.prefs.highlight_keeps,
        highlight_custom_tracking: !preview && app.session.prefs.highlight_custom_tracking,
        quality: app.ui.display_quality,
        rich_black: app.ui.rich_black,
        overprint_preview: app.ui.overprint_preview,
        blend_space_view: !app.ui.proof_colors,
        document_grid: if app.ui.document_grid && !preview { grid_areas(layout) } else { vec![] },
        ..Default::default()
    }
}

/// The area each spread's document grid covers, in spread coordinates: the full-width band of
/// the pasteboard nearest that spread, the band in which `CanvasLayout::spread_at` (and so
/// snapping) picks it.
fn grid_areas(layout: &CanvasLayout) -> Vec<(designcraft_doc::SpreadRef, DRect)> {
    let pb = layout.pasteboard;
    let slots = &layout.slots;
    slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let lo = i.checked_sub(1).and_then(|j| slots.get(j)).map_or(pb.y0, |prev| (prev.bounds.y1 + slot.bounds.y0) / 2.0);
            let hi = slots.get(i + 1).map_or(pb.y1, |next| (slot.bounds.y1 + next.bounds.y0) / 2.0);
            (slot.spread, slot.xf.inverse().transform_rect_bbox(DRect::new(pb.x0, lo, pb.x1, hi)))
        })
        .collect()
}

/// The display options that change how the canvas texture looks without touching the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayKey {
    quality: designcraft_render::DisplayQuality,
    rich_black: bool,
    overprint_preview: bool,
    proof_colors: bool,
    tag_markers: bool,
    document_grid: bool,
    highlight_substituted_fonts: bool,
    highlight_hj: bool,
    show_note_anchors: bool,
    show_added_text: bool,
    highlight_keeps: bool,
    highlight_custom_tracking: bool,
}

fn display_key(app: &DesignApp) -> DisplayKey {
    let (ui, prefs) = (&app.ui, &app.session.prefs);
    DisplayKey {
        quality: ui.display_quality,
        rich_black: ui.rich_black,
        overprint_preview: ui.overprint_preview,
        proof_colors: ui.proof_colors,
        tag_markers: ui.tag_markers,
        document_grid: ui.document_grid,
        highlight_substituted_fonts: prefs.highlight_substituted_fonts,
        highlight_hj: prefs.highlight_hj,
        show_note_anchors: prefs.show_note_anchors,
        show_added_text: prefs.show_added_text,
        highlight_keeps: prefs.highlight_keeps,
        highlight_custom_tracking: prefs.highlight_custom_tracking,
    }
}

/// When a display option changed, drop the texture and any render in flight: they were drawn with the old options.
fn invalidate_on_display_change(app: &mut DesignApp) {
    let key = display_key(app);
    if app.canvas.display_key != Some(key) {
        app.canvas.display_key = Some(key);
        app.canvas.shown = None;
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
    }
}

/// Keep the canvas texture current. Panning inside the overscanned region just shifts the
/// texture; zooming shows the old texture scaled until the background render arrives; document
/// edits render synchronously so editing never lags.
fn render_texture(app: &mut DesignApp, ctx: &egui::Context, rect: Rect, xf: &Xf, layout: &CanvasLayout, preview: bool) {
    invalidate_on_display_change(app);
    let Some(st) = app.session.active() else { return };
    let ppp = ctx.pixels_per_point() as f64;
    let doc_key = (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, preview);
    let editing_parents = st.editing_parents;
    // Overscan: 25% of the viewport on every side, within the GPU's maximum texture size.
    let max_side = ctx.input(|i| i.max_texture_side) as f32 / ppp as f32;
    let (ow, oh) =
        ((rect.width() * 0.25).min((max_side - rect.width()) / 2.0).max(0.0), (rect.height() * 0.25).min((max_side - rect.height()) / 2.0).max(0.0));
    let want = |origin: Point| crate::Shown {
        doc: doc_key,
        editing_parents,
        origin: Point::new(origin.x - ow as f64 / xf.zoom, origin.y - oh as f64 / xf.zoom),
        zoom: xf.zoom,
        size: (rect.width() + 2.0 * ow, rect.height() + 2.0 * oh),
    };
    let target = want(xf.origin);
    // Does the current texture cover the viewport at this zoom and document state?
    let covers = |s: &crate::Shown| {
        s.doc == doc_key && s.editing_parents == editing_parents && (s.zoom - xf.zoom).abs() < 1e-9 && {
            let x0 = (xf.origin.x - s.origin.x) * s.zoom;
            let y0 = (xf.origin.y - s.origin.y) * s.zoom;
            x0 >= -0.5 && y0 >= -0.5 && x0 + rect.width() as f64 <= s.size.0 as f64 + 0.5 && y0 + rect.height() as f64 <= s.size.1 as f64 + 0.5
        }
    };
    // Collect finished background renders.
    #[cfg(not(target_arch = "wasm32"))]
    if let (Some(w), Some((tok, shown))) = (app.canvas.worker.as_ref(), app.canvas.pending)
        && let Some(done) = w.poll()
        && done.token == tok
    {
        upload(app, ctx, done.image);
        app.canvas.shown = Some(shown);
        app.canvas.shown_doc = app.canvas.pending_doc.take();
        app.canvas.pending = None;
        app.perf.render_ms = done.ms;
    }
    if app.canvas.shown.is_some_and(|s| covers(&s)) {
        return;
    }
    let doc_changed = app.canvas.shown.is_none_or(|s| s.doc != doc_key);
    let mode_changed = app.canvas.shown.is_some_and(|s| s.editing_parents != editing_parents);
    let zoom_same = app.canvas.shown.is_some_and(|s| (s.zoom - xf.zoom).abs() < 1e-9);
    let job = |app: &DesignApp| {
        let st = app.session.active()?;
        let placed: Vec<designcraft_render::Placed> =
            layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
        let w = (target.size.0 as f64 * ppp).round().max(1.0) as u32;
        let h = (target.size.1 as f64 * ppp).round().max(1.0) as u32;
        let view = Affine::scale(ppp) * Affine::scale(xf.zoom) * Affine::translate((-target.origin.x, -target.origin.y));
        let opts = screen_options(app, layout, preview, ppp);
        Some((st.doc.clone(), placed, w, h, view, opts))
    };
    // Synchronous path: document edits (keep editing crisp), first frame, or no worker.
    #[cfg(not(target_arch = "wasm32"))]
    if !app.canvas.worker_started {
        app.canvas.worker_started = true;
        app.canvas.worker = render_worker_spawn(ctx);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let has_worker = app.canvas.worker.is_some();
    #[cfg(target_arch = "wasm32")]
    let has_worker = false;
    // An edit with the view unchanged: repaint just the regions it changed.
    if doc_changed && zoom_same && patch_texture(app, layout, ppp, doc_key, preview) {
        return;
    }
    // A mode switch changes every spread's meaning, even if the document has no damage.
    // Show the new canvas immediately, including when fitting it changes the zoom.
    if !has_worker || app.canvas.shown.is_none() || mode_changed || (doc_changed && zoom_same) {
        let t0 = crate::now_ms();
        let Some((doc, placed, w, h, view, opts)) = job(app) else { return };
        let img = app.canvas.renderer.render(&doc, &app.session.cache, &placed, w, h, view, &opts);
        upload(app, ctx, img);
        app.canvas.shown = Some(target);
        app.canvas.shown_doc = Some(doc);
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
        app.perf.render_ms = crate::now_ms() - t0;
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Already rendering this view?
        if app.canvas.pending.is_some_and(|(_, p)| {
            p.doc == doc_key && p.editing_parents == editing_parents && (p.zoom - xf.zoom).abs() < 1e-9 && {
                let x0 = (xf.origin.x - p.origin.x) * p.zoom;
                let y0 = (xf.origin.y - p.origin.y) * p.zoom;
                x0 >= 0.0 && y0 >= 0.0 && x0 + rect.width() as f64 <= p.size.0 as f64 && y0 + rect.height() as f64 <= p.size.1 as f64
            }
        }) {
            return;
        }
        let Some((doc, placed, w, h, view, opts)) = job(app) else { return };
        app.canvas.pending_doc = Some(doc.clone());
        app.canvas.token += 1;
        let token = app.canvas.token;
        if let Some(wk) = app.canvas.worker.as_ref() {
            wk.submit(crate::render_worker::Job { token, doc, cache: app.session.cache.clone(), placed, w, h, view, opts });
        }
        app.canvas.pending = Some((token, target));
    }
}

/// Repaint only the damaged regions of the current texture after an edit (typing, nudging).
/// Returns false when a full render is needed (view moved, too much changed, or damage unknown).
fn patch_texture(app: &mut DesignApp, layout: &CanvasLayout, ppp: f64, doc_key: (u64, u64, u64, bool), preview: bool) -> bool {
    let (Some(sh), Some(old), Some(st)) = (app.canvas.shown, app.canvas.shown_doc.clone(), app.session.active()) else { return false };
    if sh.doc.0 != doc_key.0 || sh.doc.3 != doc_key.3 || sh.editing_parents != st.editing_parents {
        return false;
    }
    let (tw, th) = ((sh.size.0 as f64 * ppp).round() as i64, (sh.size.1 as f64 * ppp).round() as i64);
    // Patches are in full-resolution pixels: a texture `upload` shrank to the GPU's limit
    // (a viewport wider than `max_texture_side`) takes full renders instead.
    let full = [usize::try_from(tw).unwrap_or(0), usize::try_from(th).unwrap_or(0)];
    if app.canvas.texture.as_ref().is_none_or(|t| t.size() != full) {
        return false;
    }
    let new = st.doc.clone();
    let Some(regions) = designcraft_render::damage::damage_with(&old, &new, Some(&app.session.cache)) else { return false };
    let t0 = crate::now_ms();
    let k = sh.zoom * ppp;
    let mut px: Option<(i64, i64, i64, i64)> = None;
    for (r, rect) in &regions {
        let c = layout.xf(*r).transform_rect_bbox(*rect);
        let x0 = ((c.x0 - sh.origin.x) * k).floor() as i64 - 1;
        let y0 = ((c.y0 - sh.origin.y) * k).floor() as i64 - 1;
        let x1 = ((c.x1 - sh.origin.x) * k).ceil() as i64 + 1;
        let y1 = ((c.y1 - sh.origin.y) * k).ceil() as i64 + 1;
        px = Some(px.map_or((x0, y0, x1, y1), |p| (p.0.min(x0), p.1.min(y0), p.2.max(x1), p.3.max(y1))));
    }
    let done = |app: &mut DesignApp| {
        app.canvas.shown = Some(crate::Shown { doc: doc_key, ..sh });
        app.canvas.shown_doc = Some(new.clone());
        // A background render of the old document must not overwrite this.
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
    };
    let Some((x0, y0, x1, y1)) = px.map(|p| (p.0.max(0), p.1.max(0), p.2.min(tw), p.3.min(th))) else {
        done(app);
        return true;
    };
    if x1 <= x0 || y1 <= y0 {
        done(app);
        return true;
    }
    // Big changes: a full render costs about the same.
    if ((x1 - x0) * (y1 - y0)) as f64 > 0.5 * (tw * th) as f64 {
        return false;
    }
    let placed: Vec<designcraft_render::Placed> = layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
    let view = Affine::translate((-(x0 as f64), -(y0 as f64)))
        * Affine::scale(ppp)
        * Affine::scale(sh.zoom)
        * Affine::translate((-sh.origin.x, -sh.origin.y));
    let opts = screen_options(app, layout, preview, ppp);
    let img = app.canvas.patcher.render(&new, &app.session.cache, &placed, (x1 - x0) as u32, (y1 - y0) as u32, view, &opts);
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    if let Some(tex) = app.canvas.texture.as_mut() {
        tex.set_partial([x0 as usize, y0 as usize], ci, egui::TextureOptions::LINEAR);
    }
    done(app);
    app.canvas.patches += 1;
    app.perf.render_ms = crate::now_ms() - t0;
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn render_worker_spawn(ctx: &egui::Context) -> Option<crate::render_worker::Worker> {
    if std::env::var_os("DESIGNCRAFT_SYNC_RENDER").is_some() {
        return None;
    }
    crate::render_worker::Worker::spawn(ctx.clone())
}

fn upload(app: &mut DesignApp, ctx: &egui::Context, mut img: designcraft_render::Rendered) {
    if app.ui.proof_colors {
        designcraft_render::proof_view(&mut img, &app.ui.proof_setup);
    }
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    let ci = crate::widgets::fit_texture(ci, ctx.input(|i| i.max_texture_side));
    match &mut app.canvas.texture {
        Some(tex) => tex.set(ci, egui::TextureOptions::LINEAR),
        None => app.canvas.texture = Some(ctx.load_texture("canvas", ci, egui::TextureOptions::LINEAR)),
    }
}

fn dashed(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke, dash: f32, gap: f32) {
    painter.extend(egui::Shape::dashed_line(&[a, b], stroke, dash, gap));
}

/// The most baseline grid lines drawn on a page: one per point of the tallest page (216 in).
const MAX_BASELINES: usize = 216 * 72;

/// Baseline grid lines inside a page, measured from its top: `start + n * increment` for n >= 0 (the bottom
/// edge is not a line), `start` from [`BaselineGrid::origin`](designcraft_doc::BaselineGrid::origin). Lines
/// above the page are skipped without walking them, however far up the start is.
fn baseline_grid_lines(start: f64, increment: f64, height: f64) -> impl Iterator<Item = f64> {
    let step = increment.max(1.0);
    let first = if start < 0.0 { start.rem_euclid(step) } else { start };
    (0..MAX_BASELINES).map(move |n| first + n as f64 * step).take_while(move |y| *y < height)
}

/// What a screen mode shows of a spread, in screen coordinates: the trim, or out to the bleed or
/// the slug box. Slug shows every printing element, so the bleed as well — and a slug narrower
/// than the bleed does not pull the edge back in.
fn shown_area(app: &DesignApp, doc: &Document, xf: &Xf, slot: &SpreadSlot) -> Rect {
    let trim = xf.rect(slot.bounds);
    let boxes: &[[f64; 4]] = match app.ui.screen_mode {
        ScreenMode::Bleed => &[doc.settings.bleed],
        ScreenMode::Slug => &[doc.settings.bleed, doc.settings.slug],
        _ => &[],
    };
    let Some(sp) = doc.spread(slot.spread) else { return trim };
    boxes.iter().filter_map(|o| sp.outset_rect(*o)).map(|r| xf.rect(slot.xf.transform_rect_bbox(r))).fold(trim, |a, b| a.union(b))
}

/// `moving`: guides being dragged, drawn where they would be dropped instead of where they are.
fn draw_guides(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout, t: &Tokens, moving: Option<&MovingGuides>) {
    let selection: std::collections::HashSet<GuideSel> =
        app.session.active().map(|st| st.selection.guides.iter().copied().collect()).unwrap_or_default();
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        if app.ui.guides {
            draw_bleed_and_slug(painter, xf, doc, slot, sp);
        }
        let pasteboard = slot.xf.inverse().transform_rect_bbox(layout.pasteboard);
        for (pi, p) in sp.pages.iter().enumerate() {
            let pr = p.bounds();
            draw_parent_overlay(painter, xf, doc, slot, pi, p, pr);
            if !app.ui.guides {
                continue;
            }
            draw_page_grids(painter, xf, doc, slot, p, pr, app.ui.baseline_grid);
            draw_page_guides(painter, xf, doc, slot, sp, pi, p, pr, pasteboard, moving, &selection);
        }
    }
    let _ = t;
}

/// The bleed and slug outlines of a spread, in their own colours.
fn draw_bleed_and_slug(painter: &egui::Painter, xf: &Xf, doc: &Document, slot: &SpreadSlot, sp: &designcraft_doc::Spread) {
    let s = &doc.settings;
    let rect = |r: DRect| xf.rect(slot.xf.transform_rect_bbox(r));
    // Bleed, then slug: one rectangle each around the spread (the inside offset only applies
    // at the spread's outer edges).
    for (r, color) in [(sp.bleed_rect(s.bleed), s.bleed_color), (sp.slug_rect(s.slug), s.slug_color)] {
        if let Some(r) = r {
            painter.rect_stroke(rect(r), 0.0, Stroke::new(hair(painter), c32(color)), StrokeKind::Middle);
        }
    }
}

/// Page tool › Show Parent Page Overlay: the parent page's size over the page.
fn draw_parent_overlay(painter: &egui::Painter, xf: &Xf, doc: &Document, slot: &SpreadSlot, pi: usize, p: &designcraft_doc::Page, pr: DRect) {
    let rect = |r: DRect| xf.rect(slot.xf.transform_rect_bbox(r));
    if p.show_parent_overlay
        && let SpreadRef::Doc(si) = slot.spread
        && let Some((ppi, ppg)) = doc.parent_page_for(doc.first_page_of_spread(si) + pi)
        && let Some(pp) = doc.parents.get(ppi).and_then(|ps| ps.pages.get(ppg))
    {
        let r = rect(DRect::new(pr.x0, pr.y0, pr.x0 + pp.width, pr.y0 + pp.height));
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(110, 110, 230, 28));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(110, 110, 230)), StrokeKind::Middle);
    }
}

/// A page's baseline grid (when `baseline` is on and the view is zoomed in far enough), margins
/// and columns.
fn draw_page_grids(painter: &egui::Painter, xf: &Xf, doc: &Document, slot: &SpreadSlot, p: &designcraft_doc::Page, pr: DRect, baseline: bool) {
    let s = &doc.settings;
    let pt = |q: Point| xf.to_screen(slot.xf * q);
    let rect = |r: DRect| xf.rect(slot.xf.transform_rect_bbox(r));
    // Baseline grid.
    if baseline && xf.zoom >= s.baseline_grid.view_threshold {
        let g = &s.baseline_grid;
        for y in baseline_grid_lines(g.origin(p), g.increment, p.height) {
            painter.line_segment(
                [pt(Point::new(pr.x0, pr.y0 + y)), pt(Point::new(pr.x1, pr.y0 + y))],
                Stroke::new(hair(painter), c32(g.color).gamma_multiply(0.8)),
            );
        }
    }
    // Margins (magenta) and columns (violet).
    painter.rect_stroke(rect(p.margin_rect()), 0.0, Stroke::new(hair(painter), c32(s.margin_color)), StrokeKind::Middle);
    let cols = p.column_rects();
    if cols.len() > 1 {
        for (i, c) in cols.iter().enumerate() {
            let col = Stroke::new(hair(painter), c32(s.column_color));
            if i > 0 {
                painter.line_segment([pt(Point::new(c.x0, c.y0)), pt(Point::new(c.x0, c.y1))], col);
            }
            if i + 1 < cols.len() {
                painter.line_segment([pt(Point::new(c.x1, c.y0)), pt(Point::new(c.x1, c.y1))], col);
            }
        }
    }
}

/// The page's own guides, then those it shows from its parent page. `moving` draws dragged
/// guides where they would be dropped.
fn draw_page_guides(
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    slot: &SpreadSlot,
    sp: &designcraft_doc::Spread,
    pi: usize,
    p: &designcraft_doc::Page,
    pr: DRect,
    pasteboard: DRect,
    moving: Option<&MovingGuides>,
    selection: &std::collections::HashSet<GuideSel>,
) {
    let own = p.guides.len();
    for (gi, mut g) in p.guides.iter().cloned().chain(doc.parent_guides(p)).enumerate().filter(|(_, g)| g.visible_in(doc)) {
        let key = GuideSel { spread: slot.spread, page: pi, index: gi };
        // Selected guides are drawn thicker, in their layer's color.
        let selected = gi < own && selection.contains(&key);
        if gi < own
            && let Some(m) = moving
            && m.guides.contains(&key)
        {
            g.position += match g.orientation {
                designcraft_doc::Orientation::Vertical => m.dx,
                designcraft_doc::Orientation::Horizontal => m.dy,
            };
        }
        draw_guide(painter, xf, doc, slot, sp, pr, pasteboard, &g, selected);
    }
}

/// One guide as a line: across the pasteboard for a spread guide, across its page otherwise.
fn draw_guide(
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    slot: &SpreadSlot,
    sp: &designcraft_doc::Spread,
    pr: DRect,
    pasteboard: DRect,
    g: &designcraft_doc::Guide,
    selected: bool,
) {
    let pt = |q: Point| xf.to_screen(slot.xf * q);
    // Spread guides cross the pasteboard; page guides their page.
    let span = if g.spread { pasteboard } else { pr };
    let (a, bb) = match g.orientation {
        designcraft_doc::Orientation::Horizontal => (Point::new(span.x0, g.position), Point::new(span.x1, g.position)),
        designcraft_doc::Orientation::Vertical => {
            let span = if g.spread { sp.bounds().inflate(0.0, 36.0) } else { pr };
            (Point::new(g.position, span.y0), Point::new(g.position, span.y1))
        }
    };
    let stroke = if selected {
        let color = g.layer.and_then(|l| doc.layer(l)).map_or(Color32::from_rgb(79, 153, 255), |l| c32(l.color));
        Stroke::new(2.0, color)
    } else {
        Stroke::new(hair(painter), Color32::from_rgb(74, 227, 255))
    };
    if g.liquid {
        // Liquid guides are dashed.
        painter.extend(egui::Shape::dashed_line(&[pt(a), pt(bb)], stroke, 6.0, 3.0));
    } else {
        painter.line_segment([pt(a), pt(bb)], stroke);
    }
}

/// Spread-space → canvas transform for an item at `loc`.
fn item_canvas_xf(doc: &Document, layout: &CanvasLayout, id: designcraft_doc::ItemId) -> Option<(Affine, &'static str)> {
    let loc = doc.find(id)?;
    Some((layout.xf(loc.spread) * doc.parent_xf(&loc), ""))
}

fn path_screen(it: &Item, xf: &Xf, a: Affine) -> Vec<Vec<Pos2>> {
    let bp = it.path.to_bezpath();
    let full = a * it.xf;
    let mut out = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    designcraft_geom::kurbo::flatten(&bp, 0.25 / xf.zoom.max(0.01), |el| {
        use designcraft_geom::PathEl::*;
        match el {
            MoveTo(p) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                cur.push(xf.to_screen(full * p));
            }
            LineTo(p) => cur.push(xf.to_screen(full * p)),
            ClosePath => {
                if let Some(f) = cur.first().copied() {
                    cur.push(f);
                }
            }
            _ => {}
        }
    });
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

fn draw_frames(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
    if !app.ui.frame_edges {
        return;
    }
    let ported = ported_frames(app, doc);
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        let a = slot.xf;
        // Parent items on document pages, and base parents' items on a parent spread: dotted edges.
        for (pi, page) in sp.pages.iter().enumerate() {
            for (ppi, ppg) in doc.shown_parent_pages(slot.spread, pi) {
                let Some(parent) = doc.parents.get(ppi) else { continue };
                let Some(ppage) = parent.pages.get(ppg) else { continue };
                let dx = page.x - ppage.x;
                for it in &parent.items {
                    if (parent.pages.len() > 1 && parent.page_at_x(it.bounds().center().x) != Some(ppg)) || page.overridden.contains(&it.id) {
                        continue;
                    }
                    let col = doc.layer(it.layer).map(|l| c32(l.color)).unwrap_or(Color32::LIGHT_BLUE);
                    for poly in path_screen(it, xf, a * Affine::translate((dx, 0.0))) {
                        for w in poly.windows(2) {
                            dashed(painter, w[0], w[1], Stroke::new(hair(painter), col.gamma_multiply(0.8)), 2.0, 2.0);
                        }
                    }
                }
            }
        }
        for it in &sp.items {
            draw_item_edges(painter, xf, doc, it, a);
        }
        // Every overset frame, selected or not and inside groups too, shows its red mark at the
        // out port (InDesign shows it only on the selected frame; a report in Preflight is easy
        // to miss while laying out).
        for (it, c) in overset_marks(app, doc, layout, &sp.items, xf.zoom, &ported) {
            let c = xf.to_screen(c);
            let s = designcraft_tools::thread::PORT_SIZE_PX as f32;
            painter.rect_filled(Rect::from_center_size(c, vec2(s, s)), 0.0, Color32::WHITE);
            painter.rect_stroke(Rect::from_center_size(c, vec2(s, s)), 0.0, Stroke::new(hair(painter), layer_color(doc, it)), StrokeKind::Inside);
            let red = Color32::from_rgb(230, 20, 20);
            painter.line_segment([c - vec2(2.5, 0.0), c + vec2(2.5, 0.0)], Stroke::new(1.5, red));
            painter.line_segment([c - vec2(0.0, 2.5), c + vec2(0.0, 2.5)], Stroke::new(1.5, red));
        }
        // Tagged frames in their tag's colour.
        if app.ui.tagged_frames {
            for top in &sp.items {
                top.walk(&mut |it: &Item| {
                    if it.xml_tag.is_empty() {
                        return;
                    }
                    let Some(t) = doc.xml.tags.iter().find(|t| t.name == it.xml_tag) else { return };
                    let col = Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
                    for poly in path_screen(it, xf, a) {
                        for w in poly.windows(2) {
                            painter.line_segment([w[0], w[1]], Stroke::new(2.5, col));
                        }
                    }
                });
            }
        }
    }
}

fn draw_item_edges(painter: &egui::Painter, xf: &Xf, doc: &Document, it: &Item, a: Affine) {
    if it.hidden || doc.layer(it.layer).is_some_and(|l| !l.visible) {
        return;
    }
    let col = doc.layer(it.layer).map(|l| c32(l.color)).unwrap_or(Color32::LIGHT_BLUE);
    if let Content::Group { items } = &it.content {
        for c in items {
            draw_item_edges(painter, xf, doc, c, a * it.xf);
        }
        return;
    }
    // Frames without a stroke show their edge; shapes with strokes are visible already.
    let show = it.stroke.is_none() || matches!(it.content, Content::Text(_) | Content::Graphic(_));
    if show {
        for poly in path_screen(it, xf, a) {
            painter.add(egui::Shape::line(poly, Stroke::new(hair(painter), col.gamma_multiply(0.75))));
        }
    }
    // Empty graphic frame: the X.
    if it.object_style == designcraft_doc::BASIC_GRAPHICS_FRAME && matches!(it.content, Content::Unassigned) {
        let r = it.inner_bounds();
        let m = a * it.xf;
        let p = |x: f64, y: f64| xf.to_screen(m * Point::new(x, y));
        painter.line_segment([p(r.x0, r.y0), p(r.x1, r.y1)], Stroke::new(hair(painter), col.gamma_multiply(0.75)));
        painter.line_segment([p(r.x1, r.y0), p(r.x0, r.y1)], Stroke::new(hair(painter), col.gamma_multiply(0.75)));
    }
}

/// Layer colour of the first selected item: live-corner marks are outlined in it.
fn selection_color(app: &DesignApp) -> Color32 {
    let fallback = Color32::from_rgb(79, 153, 255);
    let Some(st) = app.session.active() else { return fallback };
    st.selection.items.first().and_then(|id| st.doc.item(*id)).map_or(fallback, |it| layer_color(&st.doc, it))
}

fn layer_color(doc: &Document, it: &Item) -> Color32 {
    doc.layer(it.layer).map(|l| c32(l.color)).unwrap_or(Color32::from_rgb(79, 153, 255))
}

/// Draw selection chrome; returns the selection's screen bounds (for the Contextual Task Bar).
fn draw_selection(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) -> Option<Rect> {
    let st = app.session.active()?;
    let sel: &Selection = &st.selection;
    let mut text_rect = None;
    // Text selection / caret.
    if let Some(ts) = sel.text {
        draw_text_selection(app, painter, xf, doc, layout, ts, sel.cells);
        let fid = ts.frame.or_else(|| {
            let cs = app.session.cache.get(doc, ts.story, None);
            compose::caret(&cs, ts.focus).and_then(|c| cs.frames.get(c.0)).map(|f| f.frame)
        });
        if let Some(fid) = fid
            && let (Some(it), Some((a, _))) = (doc.item(fid), item_canvas_xf(doc, layout, fid))
        {
            text_rect = Some(xf.rect(a.transform_rect_bbox(it.bounds())));
        }
    }
    let mut union: Option<Rect> = None;
    let mut color = Color32::from_rgb(79, 153, 255);
    for id in &sel.items {
        let (Some(it), Some((a, _))) = (doc.item(*id), item_canvas_xf(doc, layout, *id)) else { continue };
        color = layer_color(doc, it);
        for poly in path_screen(it, xf, a) {
            painter.add(egui::Shape::line(poly, Stroke::new(1.0, color)));
        }
        if sel.items.len() == 1 && sel.text.is_none() {
            // InDesign draws a 1 pt bounding box around the selection.
            painter.rect_stroke(xf.rect(a.transform_rect_bbox(it.bounds())), 0.0, Stroke::new(1.0, color), StrokeKind::Middle);
        } else if sel.key == Some(*id) {
            // The key object for Align: a thicker bounding box, as in InDesign.
            painter.rect_stroke(xf.rect(a.transform_rect_bbox(it.bounds())), 0.0, Stroke::new(3.0, color), StrokeKind::Middle);
        }
        let b = xf.rect(a.transform_rect_bbox(it.bounds()));
        union = Some(union.map_or(b, |u| u.union(b)));
        // Text frame ports, also of the frames inside a selected group (so an overset frame in a
        // group shows its red mark).
        let mut frames = Vec::new();
        text_frames_in(it, 0, &mut frames);
        for (frame, story) in frames {
            draw_ports(app, painter, xf, doc, layout, frame, story, color);
        }
        if let Content::Graphic(g) = &it.content
            && sel.content
        {
            let gb = xf.rect((a * it.xf).transform_rect_bbox(g.xf.transform_rect_bbox(DRect::new(0.0, 0.0, g.size.0, g.size.1))));
            let brown = Color32::from_rgb(196, 111, 43);
            painter.rect_stroke(gb, 0.0, Stroke::new(1.0, brown), StrokeKind::Middle);
            // Its own handles resize the graphic (InDesign's brown content bounding box).
            if sel.items.len() == 1 {
                for h in designcraft_tools::select::handles(DRect::new(gb.min.x as f64, gb.min.y as f64, gb.max.x as f64, gb.max.y as f64)) {
                    let r = Rect::from_center_size(pos2(h.x as f32, h.y as f32), vec2(6.5, 6.5));
                    painter.rect_filled(r, 0.0, Color32::WHITE);
                    painter.rect_stroke(r, 0.0, Stroke::new(hair(painter), brown), StrokeKind::Inside);
                }
            }
        }
    }
    let direct = matches!(app.session.tool_id(), "directSelection" | "pen" | "addAnchor" | "deleteAnchor" | "convertDirection");
    if direct {
        for id in &sel.items {
            let (Some(it), Some((a, _))) = (doc.item(*id), item_canvas_xf(doc, layout, *id)) else { continue };
            let m = a * it.xf;
            let col = layer_color(doc, it);
            for sp in &it.path.subpaths {
                for an in &sp.anchors {
                    let p = xf.to_screen(m * an.p);
                    for (h, has) in [(an.h_in, an.has_in()), (an.h_out, an.has_out())] {
                        if has {
                            let hp = xf.to_screen(m * h);
                            painter.line_segment([p, hp], Stroke::new(1.0, col));
                            painter.circle_filled(hp, 2.5, col);
                        }
                    }
                    let r = Rect::from_center_size(p, vec2(5.0, 5.0));
                    painter.rect_filled(r, 0.0, Color32::WHITE);
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, col), StrokeKind::Inside);
                }
            }
        }
    }
    if let Some(u) = union
        && sel.text.is_none()
        && !direct
    {
        if sel.items.len() > 1 {
            painter.rect_stroke(u, 0.0, Stroke::new(1.0, color), StrokeKind::Middle);
        }
        let hs = 6.5;
        // Centre point. The Selection tool draws the live-corner widget as an overlay.
        painter.rect_filled(Rect::from_center_size(u.center(), vec2(3.5, 3.5)), 0.0, color);
        // A selected placed graphic has its own handles instead of the frame's.
        let graphic_selected =
            sel.content && sel.items.len() == 1 && doc.item(sel.items[0]).is_some_and(|i| matches!(i.content, Content::Graphic(_)));
        for h in designcraft_tools::select::handles(DRect::new(u.min.x as f64, u.min.y as f64, u.max.x as f64, u.max.y as f64))
            .into_iter()
            .filter(|_| !graphic_selected)
        {
            let r = Rect::from_center_size(pos2(h.x as f32, h.y as f32), vec2(hs, hs));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(r, 0.0, Stroke::new(hair(painter), color), StrokeKind::Inside);
        }
    }
    // Threads.
    if app.ui.text_threads || sel.items.iter().any(|i| doc.item(*i).is_some_and(|x| x.is_text_frame())) {
        for id in &sel.items {
            let Some(sid) = doc.item(*id).and_then(|i| i.text_frame()).map(|t| t.story) else { continue };
            let Some(story) = doc.story(sid) else { continue };
            for w in story.frames.windows(2) {
                let (Some(a), Some(b)) = (port_pos(doc, layout, w[0], true), port_pos(doc, layout, w[1], false)) else { continue };
                let col = doc.item(w[0]).map(|i| layer_color(doc, i)).unwrap_or(color);
                painter.line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(1.0, col));
            }
        }
    }
    text_rect.or(union)
}

/// How deep a selected group is walked for the text frames whose ports are drawn.
const PORT_GROUP_DEPTH: usize = 32;

/// The text frames whose ports `draw_selection` draws: the selected text frames and the text
/// frames inside selected groups.
fn ported_frames(app: &DesignApp, doc: &Document) -> std::collections::HashSet<designcraft_doc::ItemId> {
    let mut frames = Vec::new();
    if let Some(st) = app.session.active() {
        for it in st.selection.items.iter().filter_map(|id| doc.item(*id)) {
            text_frames_in(it, 0, &mut frames);
        }
    }
    frames.into_iter().map(|(f, _)| f.id).collect()
}

/// Canvas centres of the always-on overset marks among `items`: the out port of every visible
/// frame that ends an overset story, except the frames in `ported`, whose out port `draw_ports`
/// already marks.
fn overset_marks<'a>(
    app: &DesignApp,
    doc: &Document,
    layout: &CanvasLayout,
    items: &'a [std::sync::Arc<Item>],
    zoom: f64,
    ported: &std::collections::HashSet<designcraft_doc::ItemId>,
) -> Vec<(&'a Item, Point)> {
    let mut frames = Vec::new();
    overset_frames(app, doc, items, 0, &mut frames);
    frames
        .into_iter()
        .filter(|it| !ported.contains(&it.id))
        .filter_map(|it| Some((it, designcraft_tools::thread::port_center(doc, layout, it.id, true, zoom)?)))
        .collect()
}

/// The visible text frames among `items` (groups walked) that end an overset story.
fn overset_frames<'a>(app: &DesignApp, doc: &Document, items: &'a [std::sync::Arc<Item>], depth: usize, out: &mut Vec<&'a Item>) {
    if depth > PORT_GROUP_DEPTH {
        return;
    }
    for it in items {
        if it.hidden || doc.layer(it.layer).is_some_and(|l| !l.visible) {
            continue;
        }
        match &it.content {
            Content::Text(tf) => {
                let Some(story) = doc.story(tf.story) else { continue };
                if story.frames.last() == Some(&it.id) && app.session.cache.get(doc, tf.story, None).is_overset() {
                    out.push(it);
                }
            }
            Content::Group { items } => overset_frames(app, doc, items, depth + 1, out),
            _ => {}
        }
    }
}

/// `it` when it is a text frame, or every text frame nested in it when it is a group, each with
/// its story.
fn text_frames_in<'a>(it: &'a Item, depth: usize, out: &mut Vec<(&'a Item, designcraft_doc::StoryId)>) {
    match &it.content {
        Content::Text(tf) => out.push((it, tf.story)),
        Content::Group { items } if depth < PORT_GROUP_DEPTH => {
            for child in items {
                text_frames_in(child, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// Port location (canvas): out port at bottom-right, in port at top-left.
fn port_pos(doc: &Document, layout: &CanvasLayout, id: designcraft_doc::ItemId, out: bool) -> Option<Point> {
    let it = doc.item(id)?;
    let (a, _) = item_canvas_xf(doc, layout, id)?;
    let r = it.inner_bounds();
    let p = if out { Point::new(r.x1 - 10.0, r.y1) } else { Point::new(r.x0 + 10.0, r.y0) };
    Some(a * it.xf * p)
}

#[allow(clippy::too_many_arguments)]
fn draw_ports(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    it: &Item,
    sid: designcraft_doc::StoryId,
    color: Color32,
) {
    let Some(story) = doc.story(sid) else { return };
    let pos = story.frames.iter().position(|f| *f == it.id).unwrap_or(0);
    let is_last = pos + 1 == story.frames.len();
    let cs = app.session.cache.get(doc, sid, None);
    let overset = is_last && cs.is_overset();
    let s = designcraft_tools::thread::PORT_SIZE_PX as f32;
    // In port on the left edge below the top-left corner; out port on the right edge above the
    // bottom-right corner. The Selection tool hit-tests the same places.
    let port = |out: bool| designcraft_tools::thread::port_center(doc, layout, it.id, out, xf.zoom).map(|c| xf.to_screen(c));
    let (Some(inp), Some(outp)) = (port(false), port(true)) else { return };
    for (c, has_link, is_out) in [(inp, pos > 0, false), (outp, !is_last, true)] {
        let pr = Rect::from_center_size(c, vec2(s, s));
        painter.rect_filled(pr, 0.0, Color32::WHITE);
        painter.rect_stroke(pr, 0.0, Stroke::new(hair(painter), color), StrokeKind::Inside);
        if is_out && overset {
            let red = Color32::from_rgb(230, 20, 20);
            painter.line_segment([c - vec2(2.5, 0.0), c + vec2(2.5, 0.0)], Stroke::new(1.5, red));
            painter.line_segment([c - vec2(0.0, 2.5), c + vec2(0.0, 2.5)], Stroke::new(1.5, red));
        } else if has_link {
            painter.add(egui::Shape::convex_polygon(vec![c + vec2(-2.0, -2.5), c + vec2(2.5, 0.0), c + vec2(-2.0, 2.5)], color, Stroke::NONE));
        }
    }
}

fn draw_text_selection(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    ts: designcraft_doc::TextSel,
    cells: Option<designcraft_doc::TableSel>,
) {
    let cs = app.session.cache.get(doc, ts.story, None);
    let range = ts.range();
    let tint = Color32::from_rgba_unmultiplied(80, 140, 255, 110);
    // Selected table cells: tint every selected cell.
    if let Some(sel) = cells.filter(|c| c.story == ts.story) {
        let selected = doc.story(sel.story).and_then(|s| s.tables.get(&sel.table)).map(|t| t.selected_owners(sel.range)).unwrap_or_default();
        for ft in &cs.frames {
            let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { continue };
            let m = a * doc.text_xf(it);
            for t in ft.tables.iter().filter(|t| t.table == sel.table) {
                for c in t.cells.iter().filter(|c| selected.binary_search(&(c.row, c.col)).is_ok()) {
                    let q = [
                        xf.to_screen(m * Point::new(c.rect.x0, c.rect.y0)),
                        xf.to_screen(m * Point::new(c.rect.x1, c.rect.y0)),
                        xf.to_screen(m * Point::new(c.rect.x1, c.rect.y1)),
                        xf.to_screen(m * Point::new(c.rect.x0, c.rect.y1)),
                    ];
                    painter.add(egui::Shape::convex_polygon(q.to_vec(), Color32::from_rgba_unmultiplied(80, 140, 255, 90), Stroke::NONE));
                }
            }
        }
        return;
    }
    if let Some(cell) = ts.cell {
        draw_cell_selection(app, painter, xf, doc, layout, &cs, ts, cell);
        return;
    }
    let mut runs: Vec<(usize, Affine, Vec<[Point; 4]>, Vec<(usize, usize)>)> = Vec::new();
    for (fi, ft) in cs.frames.iter().enumerate() {
        let Some((a, _)) = item_canvas_xf(doc, layout, ft.frame) else { continue };
        let Some(it) = doc.item(ft.frame) else { continue };
        let m = a * doc.text_xf(it);
        // Show the frame edge while typing.
        for poly in path_screen(it, xf, a) {
            painter.add(egui::Shape::line(poly, Stroke::new(1.0, layer_color(doc, it).gamma_multiply(0.6))));
        }
        if range.is_empty() {
            continue;
        }
        // Type on a path: the highlight bends with the text.
        if let Some(map) = compose::PathTextMap::new(it, ft) {
            for l in &ft.lines {
                let (s, e) = (range.start.max(l.range.start), range.end.min(l.range.end));
                if s >= e {
                    continue;
                }
                for q in compose::highlight_quads(l, s, e, false) {
                    let (x0, x1) = (q[0].x.min(q[1].x), q[0].x.max(q[1].x));
                    let (y0, y1) = (q[0].y.min(q[2].y), q[0].y.max(q[2].y));
                    // Short slices, each turned with the path.
                    let n = ((x1 - x0) / 2.0).ceil().clamp(1.0, 2000.0) as usize;
                    for k in 0..n {
                        let (a, b) = (x0 + (x1 - x0) * k as f64 / n as f64, x0 + (x1 - x0) * (k + 1) as f64 / n as f64);
                        let poly =
                            [Point::new(a, y0), Point::new(b, y0), Point::new(b, y1), Point::new(a, y1)].map(|p| xf.to_screen(m * map.to_path(p)));
                        painter.add(egui::Shape::convex_polygon(poly.to_vec(), tint, Stroke::NONE));
                    }
                }
            }
            continue;
        }
        let mut quads = Vec::new();
        let mut glyphs = Vec::new();
        for (li, l) in ft.lines.iter().enumerate() {
            let s = range.start.max(l.range.start);
            let e = range.end.min(l.range.end);
            if s > e || (s == e && !(l.range.end < range.end && e == l.range.end)) {
                continue;
            }
            let past = e == l.range.end && range.end > l.range.end;
            quads.extend(compose::highlight_quads(l, s, e, past));
            // A selected drop cap is highlighted down to its baseline.
            if let Some(dc) = l.drop_cap {
                let sel = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < dc.end && g.byte >= s && g.byte < e);
                let (a, b) = sel.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), g| (a.min(g.x), b.max(g.x + g.adv)));
                if a < b {
                    let r = dc.rect;
                    quads.push([Point::new(a, r.y0), Point::new(b, r.y0), Point::new(b, r.y1), Point::new(a, r.y1)]);
                }
            }
            for (gi, g) in l.glyphs.iter().enumerate() {
                if g.visible && g.len > 0 && g.byte >= s && g.byte < e {
                    glyphs.push((li, gi));
                }
            }
        }
        if !quads.is_empty() {
            runs.push((fi, m, quads, glyphs));
        }
    }
    if !runs.is_empty() {
        draw_inverse_highlight(app, painter, xf, doc, &cs, ts.story, &runs);
    }
    if ts.is_caret()
        && let Some((fi, x, bl, asc, desc)) = compose::caret(&cs, ts.focus)
        && let Some(ft) = cs.frames.get(fi)
        && let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame))
    {
        let m = a * doc.text_xf(it);
        // Type on a path: the caret stands across the path where it is along it.
        let on = |p: Point| compose::PathTextMap::new(it, ft).map_or(p, |map| map.to_path(p));
        let p0 = xf.to_screen(m * on(Point::new(x, bl - asc)));
        let p1 = xf.to_screen(m * on(Point::new(x, bl + desc)));
        let blink = (painter.ctx().input(|i| i.time) * 1.6) as i64 % 2 == 0;
        if blink {
            painter.line_segment([p0, p1], Stroke::new(1.0, Color32::BLACK));
        }
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
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

/// Caret / text highlight inside a table cell (the cell's lines are offset by the cell origin).
#[allow(clippy::too_many_arguments)]
fn draw_cell_selection(
    _app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    cs: &compose::ComposedStory,
    ts: designcraft_doc::TextSel,
    cell: designcraft_doc::CellAddr,
) {
    // Footnote text or a cell: its composed story and where it sits in the frame.
    let found = match cell.footnote_id() {
        Some(id) => compose::find_note(cs, id).map(|(fi, n)| (fi, n.origin, n.text.clone())),
        None => compose::find_cell(cs, cell.table, cell.row, cell.col).map(|(fi, _, pc)| (fi, pc.origin, pc.text.clone())),
    };
    let Some((fi, origin, text)) = found else { return };
    let Some(ft) = cs.frames.get(fi) else { return };
    let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { return };
    let m = a * doc.text_xf(it) * designcraft_geom::Affine::translate(origin.to_vec2());
    let range = ts.range();
    if !range.is_empty()
        && let Some(cft) = text.frames.first()
    {
        for l in &cft.lines {
            let s = range.start.max(l.range.start);
            let e = range.end.min(l.range.end);
            if s > e || (s == e && !(l.range.end < range.end && e == l.range.end)) {
                continue;
            }
            let past = e == l.range.end && range.end > l.range.end;
            for q in compose::highlight_quads(l, s, e, past) {
                let poly = q.map(|p| xf.to_screen(m * p));
                painter.add(egui::Shape::convex_polygon(poly.to_vec(), Color32::from_rgba_unmultiplied(80, 140, 255, 110), Stroke::NONE));
            }
        }
    }
    if ts.is_caret()
        && let Some((_, x, bl, asc, desc)) = match cell.footnote_id() {
            Some(id) => compose::note_caret(cs, id, ts.focus),
            None => compose::cell_caret(cs, cell.table, cell.row, cell.col, ts.focus),
        }
    {
        let m = a * doc.text_xf(it);
        let (p0, p1) = (xf.to_screen(m * Point::new(x, bl - asc)), xf.to_screen(m * Point::new(x, bl + desc)));
        let blink = (painter.ctx().input(|i| i.time) * 1.6) as i64 % 2 == 0;
        if blink {
            painter.line_segment([p0, p1], Stroke::new(1.0, Color32::BLACK));
        }
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
    }
}

/// InDesign draws selected text inverted: a black highlight with the selected glyphs in the
/// inverse of their colour (white for black text). The highlight and glyph outlines are
/// rasterised with `vello_cpu` into a small screen-space texture over the selection.
fn draw_inverse_highlight(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    cs: &compose::ComposedStory,
    story: designcraft_doc::StoryId,
    runs: &[(usize, Affine, Vec<[Point; 4]>, Vec<(usize, usize)>)],
) {
    use designcraft_render::vello_cpu::{self, kurbo, peniko};
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let to_screen = |m: Affine, p: Point| xf.to_screen(m * p);
    let mut bbox = Rect::NOTHING;
    for (_, m, quads, _) in runs {
        for q in quads {
            for p in q {
                bbox.extend_with(to_screen(*m, *p));
            }
        }
    }
    let bbox = bbox.intersect(painter.clip_rect());
    if !bbox.is_positive() {
        return;
    }
    // Snap to device pixels so the texture maps 1:1.
    let bbox = Rect::from_min_max(
        pos2((bbox.min.x * ppp).floor() / ppp, (bbox.min.y * ppp).floor() / ppp),
        pos2((bbox.max.x * ppp).ceil() / ppp, (bbox.max.y * ppp).ceil() / ppp),
    );
    let (w, h) = (((bbox.width() * ppp).round() as u32).clamp(1, 8192), ((bbox.height() * ppp).round() as u32).clamp(1, 8192));
    let Some(st) = app.session.active() else { return };
    let key = egui::Id::unique(("inverse_sel", st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize, story.0))
        .with((runs.iter().map(|r| r.2.len() + r.3.len() * 7919).sum::<usize>(), w, h))
        .with((bbox.min.x.to_bits(), bbox.min.y.to_bits(), xf.zoom.to_bits(), st.selection.text.map(|t| (t.anchor, t.focus))));
    let cache_id = egui::Id::unique("inverse_sel_tex");
    let cached: Option<(egui::Id, egui::TextureHandle)> = ctx.data(|d| d.get_temp(cache_id));
    let tex = match cached {
        Some((k, t)) if k == key => t,
        _ => {
            let view = xf.affine(ppp as f64, bbox.min);
            let mut rc = vello_cpu::RenderContext::new_with(w as u16, h as u16, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
            rc.set_paint(peniko::Color::from_rgba8(0, 0, 0, 255));
            for (_, m, quads, _) in runs {
                rc.set_transform(view * *m);
                for q in quads {
                    let mut bp = kurbo::BezPath::new();
                    bp.move_to(q[0]);
                    for p in &q[1..] {
                        bp.line_to(*p);
                    }
                    bp.close_path();
                    rc.fill_path(&bp);
                }
            }
            let db = designcraft_fonts::FontDb::global();
            for (fi, m, _, glyphs) in runs {
                let Some(ft) = cs.frames.get(*fi) else { continue };
                for &(li, gi) in glyphs {
                    let Some(l) = ft.lines.get(li) else { continue };
                    let Some(g) = l.glyphs.get(gi) else { continue };
                    let Some(style) = cs.styles.get(g.style as usize) else { continue };
                    let Some(c) = doc.resolve_color(&style.fill, style.fill_tint) else { continue };
                    let [r, gg, b, _] = c.to_rgba8(1.0);
                    rc.set_paint(peniko::Color::from_rgba8(255 - r, 255 - gg, 255 - b, 255));
                    let outline = db.outline(&g.face, g.gid);
                    let skew =
                        if style.skew != 0.0 { Affine::new([1.0, 0.0, -style.skew.to_radians().tan(), 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
                    let shatai = style.shatai_xf(g, l.baseline).unwrap_or(Affine::IDENTITY);
                    let ga = shatai * Affine::translate((g.x, l.baseline + g.y)) * skew * Affine::scale_non_uniform(g.sx, g.sy);
                    rc.set_transform(view * *m * ga);
                    rc.fill_path(&outline);
                }
            }
            rc.flush();
            let mut pm = vello_cpu::Pixmap::new(w as u16, h as u16);
            let mut res = vello_cpu::Resources::new();
            rc.render(&mut pm, &mut res);
            let ci = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data_as_u8_slice());
            let t = crate::widgets::load_texture(ctx, "inverse_selection", ci, egui::TextureOptions::NEAREST);
            ctx.data_mut(|d| d.insert_temp(cache_id, (key, t.clone())));
            t
        }
    };
    painter.image(tex.id(), bbox, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

fn draw_tool_overlays(app: &mut DesignApp, painter: &egui::Painter, xf: &Xf) {
    if let (Some((z, c)), Some(rect)) = (app.power_zoom, app.canvas_rect) {
        // The view that releasing returns to.
        let (w, h) = (rect.width() as f64 / z, rect.height() as f64 / z);
        let r = xf.rect(designcraft_geom::Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0));
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, Color32::from_rgb(230, 40, 40)), egui::StrokeKind::Middle);
    }
    let tok = Tokens::get(painter.ctx());
    let ov = app.session.overlays(app.view_info());
    let doc = app.session.active().map(|st| st.doc.clone());
    let layout = app.session.layout();
    let sel_color = selection_color(app);
    for o in ov {
        match o {
            Overlay::Marquee(r) => {
                let r = xf.rect(r);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(80, 140, 255, 30));
                for (a, b) in [
                    (r.left_top(), r.right_top()),
                    (r.right_top(), r.right_bottom()),
                    (r.right_bottom(), r.left_bottom()),
                    (r.left_bottom(), r.left_top()),
                ] {
                    dashed(painter, a, b, Stroke::new(1.0, Color32::from_gray(200)), 3.0, 3.0);
                }
            }
            Overlay::Measure { p, text } => {
                let s = xf.to_screen(p) + vec2(14.0, 14.0);
                let g = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), tok.measure_text);
                let r = Rect::from_min_size(s, g.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, 3.0, tok.measure_bg);
                painter.galley(r.min + vec2(5.0, 3.0), g, tok.measure_text);
            }
            Overlay::Line { a, b, color, dashed: d } => {
                let st = Stroke::new(1.0, c32(color));
                if d {
                    dashed(painter, xf.to_screen(a), xf.to_screen(b), st, 3.0, 3.0);
                } else {
                    painter.line_segment([xf.to_screen(a), xf.to_screen(b)], st);
                }
            }
            Overlay::Guide { a, b } => {
                painter.line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(1.0, tok.smart_guide));
            }
            Overlay::Gap { a, b, label } => {
                let (a, b) = (xf.to_screen(a), xf.to_screen(b));
                painter.line_segment([a, b], Stroke::new(1.0, tok.smart_guide));
                let s = a + (b - a) * 0.5;
                let g = painter.layout_no_wrap(label, egui::FontId::proportional(11.0), tok.measure_text);
                let r = Rect::from_min_size(s, g.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, 3.0, tok.measure_bg);
                painter.galley(r.min + vec2(5.0, 3.0), g, tok.measure_text);
            }
            Overlay::Path { path, color, dashed: d } => draw_path_overlay(painter, xf, &path, c32(color), d),
            Overlay::ContentGhost(id) => {
                if let Some(doc) = &doc {
                    draw_content_ghost(painter, xf, doc, &layout, id);
                }
            }
            Overlay::LiveCornerWidget(p) => {
                let r = Rect::from_center_size(xf.to_screen(p), vec2(6.0, 6.0));
                painter.rect_filled(r, 0.0, tok.live_corner);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, sel_color), StrokeKind::Inside);
            }
            Overlay::LiveCornerDiamond(p) => {
                let c = xf.to_screen(p);
                let h = 4.5;
                let pts = vec![c + vec2(0.0, -h), c + vec2(h, 0.0), c + vec2(0.0, h), c + vec2(-h, 0.0)];
                painter.add(egui::Shape::convex_polygon(pts, tok.live_corner, Stroke::new(1.0, sel_color)));
            }
            Overlay::PageHandles(r) => {
                // The Page tool's selected page: outline and eight handles, as InDesign draws them.
                let r = xf.rect(r);
                let col = Color32::from_rgb(79, 153, 255);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, col), StrokeKind::Middle);
                for i in (0..9).filter(|i| *i != 4) {
                    let c = pos2(r.min.x + r.width() * (i % 3) as f32 / 2.0, r.min.y + r.height() * (i / 3) as f32 / 2.0);
                    let h = Rect::from_center_size(c, vec2(7.0, 7.0));
                    painter.rect_filled(h, 0.0, Color32::WHITE);
                    painter.rect_stroke(h, 0.0, Stroke::new(1.0, col), StrokeKind::Inside);
                }
            }
            Overlay::Tag { p, text } => {
                let text = crate::i18n::tr(&app.ui.language, &text).to_owned();
                let s = xf.to_screen(p);
                let g = painter.layout_no_wrap(text, egui::FontId::proportional(10.0), tok.measure_text);
                let r = Rect::from_min_size(s - vec2(0.0, g.size().y + 4.0), g.size() + vec2(8.0, 4.0));
                painter.rect_filled(r, 2.0, tok.measure_bg);
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_rgb(79, 153, 255)), StrokeKind::Inside);
                painter.galley(r.min + vec2(4.0, 2.0), g, tok.measure_text);
            }
            Overlay::Tip { p, text } => {
                let text = crate::i18n::tr(&app.ui.language, &text).to_owned();
                let s = xf.to_screen(p) + vec2(14.0, 14.0);
                let g = painter.layout(text, egui::FontId::proportional(11.0), tok.measure_text, 260.0);
                let r = Rect::from_min_size(s, g.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, 3.0, tok.measure_bg);
                painter.galley(r.min + vec2(5.0, 3.0), g, tok.measure_text);
            }
            Overlay::LiquidPins { object, page, pins, resize } => draw_liquid_pins(painter, xf.rect(object), xf.rect(page), pins, resize),
        }
    }
}

/// A path overlay (canvas coordinates; the Pencil's stroke while drawing, the Smooth and Erase
/// drags) as screen polylines, its curves flattened to about a quarter of a screen pixel.
fn draw_path_overlay(painter: &egui::Painter, xf: &Xf, path: &designcraft_geom::BezPath, color: Color32, dashed_line: bool) {
    use designcraft_geom::PathEl;
    let stroke = Stroke::new(1.0, color);
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    designcraft_geom::kurbo::flatten(path, 0.25 / xf.zoom.max(1e-9), |el| match el {
        PathEl::MoveTo(p) => runs.push(vec![xf.to_screen(p)]),
        PathEl::LineTo(p) => {
            if let Some(run) = runs.last_mut() {
                run.push(xf.to_screen(p));
            }
        }
        PathEl::ClosePath => {
            if let Some(run) = runs.last_mut()
                && let Some(&first) = run.first()
            {
                run.push(first);
            }
        }
        // Flattening yields only moves, lines and closes.
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
    });
    for run in runs.into_iter().filter(|r| r.len() >= 2) {
        if dashed_line {
            painter.extend(egui::Shape::dashed_line(&run, stroke, 3.0, 3.0));
        } else {
            painter.add(egui::Shape::line(run, stroke));
        }
    }
}

/// A frame's whole content drawn faintly over the canvas, the part outside the frame included,
/// with its bounds outlined (while the content moves inside the frame).
fn draw_content_ghost(painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout, id: designcraft_doc::ItemId) {
    let Some(it) = doc.item(id) else { return };
    let Some((a, _)) = item_canvas_xf(doc, layout, id) else { return };
    let outline = Stroke::new(1.0, Color32::from_rgb(196, 111, 43));
    let Content::Graphic(g) = &it.content else {
        for c in it.children() {
            let r = xf.rect((a * it.xf).transform_rect_bbox(c.bounds()));
            painter.rect_stroke(r, 0.0, outline, StrokeKind::Middle);
        }
        return;
    };
    let m = a * it.xf * g.xf;
    let (w, h) = g.size;
    let corners = [Point::new(0.0, 0.0), Point::new(w, 0.0), Point::new(w, h), Point::new(0.0, h)].map(|p| xf.to_screen(m * p));
    if corners.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return;
    }
    if let Some(tex) = ghost_texture(painter.ctx(), doc, g) {
        let mut mesh = egui::Mesh::with_texture(tex.id());
        let tint = Color32::from_white_alpha(90);
        for (p, uv) in corners.iter().zip([pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)]) {
            mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv, color: tint });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.add(egui::Shape::closed_line(corners.to_vec(), outline));
}

/// A screen-resolution proxy of a placed graphic for the ghost, cached per asset.
fn ghost_texture(ctx: &egui::Context, doc: &Document, g: &designcraft_doc::Graphic) -> Option<egui::TextureHandle> {
    let asset = doc.assets.get(&g.asset)?;
    let key = egui::Id::unique(("content_ghost", std::sync::Arc::as_ptr(&asset.data) as usize, asset.data.len(), asset.page));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(t);
    }
    // A 72 ppi proxy is enough for a faint preview.
    let pm = designcraft_render::images::mip(&asset.data, asset.page, g.size.0, 1.0)?;
    let (pw, ph) = (pm.width() as usize, pm.height() as usize);
    if pw == 0 || ph == 0 {
        return None;
    }
    let ci = egui::ColorImage::from_rgba_premultiplied([pw, ph], pm.data_as_u8_slice());
    let max = ctx.input(|i| i.max_texture_side).min(4096);
    let t = ctx.load_texture("content_ghost", crate::widgets::fit_texture(ci, max), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, t.clone()));
    Some(t)
}

/// An object's liquid controls on an object-based page, as InDesign draws them in orange: a
/// dashed line across the object for width and height with a padlock (closed: locked), and a pin
/// outside each edge (pinned: a line to the page edge).
fn draw_liquid_pins(painter: &egui::Painter, o: Rect, page: Rect, pins: [bool; 4], resize: [bool; 2]) {
    let col = Color32::from_rgb(178, 88, 18);
    let st = Stroke::new(1.5, col);
    let c = o.center();
    let mids = [pos2(c.x, o.min.y), pos2(o.max.x, c.y), pos2(c.x, o.max.y), pos2(o.min.x, c.y)];
    // The width and height lines between the edges' middles.
    for (a, b) in [(mids[3], mids[1]), (mids[0], mids[2])] {
        dashed(painter, a, b, Stroke::new(1.0, col), 3.0, 3.0);
    }
    for m in mids {
        painter.circle_filled(m, 3.5, col);
    }
    let (po, lo) = (designcraft_tools::PIN_OFFSET_PX as f32, designcraft_tools::LOCK_OFFSET_PX as f32);
    let outward = [vec2(0.0, -1.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(-1.0, 0.0)];
    let edge = [pos2(c.x, page.min.y), pos2(page.max.x, c.y), pos2(c.x, page.max.y), pos2(page.min.x, c.y)];
    for i in 0..4 {
        if pins[i] {
            painter.line_segment([mids[i], edge[i]], st);
            painter.circle_filled(edge[i], 4.0, col);
        } else {
            painter.circle_stroke(mids[i] + outward[i] * po, 4.0, st);
        }
    }
    // Padlocks: the width lock right of the centre, the height lock above it.
    for (at, locked) in [(c + vec2(lo, 0.0), !resize[0]), (c + vec2(0.0, -lo), !resize[1])] {
        let body = Rect::from_center_size(at + vec2(0.0, 2.0), vec2(8.0, 6.0));
        painter.rect_filled(body.expand(1.0), 1.0, Color32::WHITE);
        painter.rect_filled(body, 1.0, col);
        // The shackle: over the body when locked, swung open when not.
        let base = if locked { body.center().x } else { body.center().x + 3.0 };
        let pts: Vec<_> = (0..=8)
            .map(|k| {
                let a = std::f32::consts::PI * k as f32 / 8.0;
                pos2(base - 2.6 * a.cos(), body.min.y - 3.0 * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
    }
}

/// Paint ruler bands and ticks, the zero-point crosshair, guide previews, and ticks restarted at each page origin.
#[allow(clippy::too_many_arguments)]
fn draw_rulers(app: &DesignApp, ui: &egui::Ui, full: Rect, rect: Rect, xf: &Xf, doc: &Document, layout: &CanvasLayout, t: &Tokens) {
    let painter = ui.painter_at(full);
    let (top, left) = ruler_rects(full, rect);
    painter.rect_filled(top, 0.0, t.ruler);
    painter.rect_filled(left, 0.0, t.ruler);
    painter.rect_filled(Rect::from_min_max(full.min, rect.min), 0.0, t.ruler);
    painter.line_segment([pos2(rect.min.x, rect.min.y - 0.25), pos2(full.max.x, rect.min.y - 0.25)], Stroke::new(0.5, t.ruler_tick));
    painter.line_segment([pos2(rect.min.x - 0.25, rect.min.y), pos2(rect.min.x - 0.25, full.max.y)], Stroke::new(0.5, t.ruler_tick));
    // Zero-point crosshair box.
    let corner = Rect::from_min_max(full.min, rect.min);
    let zc = corner.center();
    painter.line_segment([zc - vec2(4.0, 0.0), zc + vec2(4.0, 0.0)], Stroke::new(1.0, t.ruler_tick));
    painter.line_segment([zc - vec2(0.0, 4.0), zc + vec2(0.0, 4.0)], Stroke::new(1.0, t.ruler_tick));
    // While the zero point is dragged out of the corner, the rulers measure from where it would
    // land (kept on the pasteboard, as the engine keeps it) and crosshair lines show the spot.
    // Cmd-dragged, the lines are the two guides it makes; locked, a plain drag does nothing.
    let dragged = ui.data(|d| d.get_temp::<Pos2>(egui::Id::unique((ZERO_POINT_DRAG, app.pane)))).filter(|p| !corner.contains(*p));
    let guides = ui.input(|i| i.modifiers.command);
    let moving = dragged.filter(|_| !guides && !doc.settings.zero_point_locked).and_then(|p| zero_point_at(app, xf, p));
    let cross = if guides { dragged } else { moving.map(|(_, c)| xf.to_screen(c)) };
    if let Some(p) = cross {
        let m = Stroke::new(1.0, t.text_dim);
        dashed(&painter, pos2(p.x, rect.min.y), pos2(p.x, full.max.y), m, 3.0, 3.0);
        dashed(&painter, pos2(rect.min.x, p.y), pos2(full.max.x, p.y), m, 3.0, 3.0);
    }
    let zp = moving.map_or_else(|| doc.zero_point(), |(zp, _)| zp);
    let pieces = ruler_pieces(app, doc, layout, zp);
    let Some(&(_, _, origin)) = pieces.first() else { return };
    let font = egui::FontId::proportional(9.0);
    let tick = Stroke::new(1.0, t.ruler_tick);
    // Horizontal: one run of ticks per piece (a page origin restarts at every page).
    let unit = doc.settings.horizontal_units;
    let (major, sub) = unit.ruler_ticks(xf.zoom);
    let (v0, v1) = (xf.to_canvas(top.min).x, xf.to_canvas(pos2(top.max.x, 0.0)).x);
    for &(from, to, o) in &pieces {
        // The piece and the visible ruler, measured from the piece's origin.
        let (s0, s1) = (from - o.x, to - o.x);
        let (c0, c1) = ((v0 - o.x).max(s0), (v1 - o.x).min(s1));
        if !(c0.is_finite() && c1.is_finite() && c0 <= c1) {
            continue;
        }
        let mut k = (c0 / major).floor() as i64;
        while (k as f64) * major <= c1 {
            let x0 = k as f64 * major;
            for s in 0..sub {
                let x = x0 + major * s as f64 / sub as f64;
                if x < s0 || x >= s1 {
                    continue;
                }
                let sx = xf.to_screen(Point::new(o.x + x, 0.0)).x;
                let len = if s == 0 {
                    RULER
                } else if sub % 2 == 0 && s == sub / 2 {
                    7.0
                } else {
                    4.0
                };
                painter.line_segment([pos2(sx, top.max.y - len), pos2(sx, top.max.y)], tick);
                if s == 0 {
                    let v = unit.from_pt(x);
                    painter.text(pos2(sx + 2.0, top.min.y + 1.0), egui::Align2::LEFT_TOP, fmt_tick(v), font.clone(), t.ruler_text);
                }
            }
            k += 1;
        }
    }
    // Vertical.
    let unit = doc.settings.vertical_units;
    let (major, sub) = unit.ruler_ticks(xf.zoom);
    let c0 = xf.to_canvas(pos2(0.0, left.min.y)).y - origin.y;
    let c1 = xf.to_canvas(pos2(0.0, left.max.y)).y - origin.y;
    let mut k = (c0 / major).floor() as i64;
    while (k as f64) * major <= c1 {
        let y0 = k as f64 * major;
        for s in 0..sub {
            let y = y0 + major * s as f64 / sub as f64;
            let sy = xf.to_screen(Point::new(0.0, origin.y + y)).y;
            let len = if s == 0 {
                RULER
            } else if sub % 2 == 0 && s == sub / 2 {
                7.0
            } else {
                4.0
            };
            painter.line_segment([pos2(left.max.x - len, sy), pos2(left.max.x, sy)], tick);
            if s == 0 {
                // Stacked digits like InDesign's vertical ruler.
                let label = fmt_tick(unit.from_pt(y));
                for (j, ch) in label.chars().enumerate() {
                    painter.text(pos2(left.min.x + 3.0, sy + 2.0 + j as f32 * 8.5), egui::Align2::LEFT_TOP, ch, font.clone(), t.ruler_text);
                }
            }
        }
        k += 1;
    }
    // Pointer position markers.
    if let Some(p) = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p) && !pointer_elsewhere(app, ui.ctx())) {
        let m = Stroke::new(1.0, t.text_dim);
        dashed(&painter, pos2(p.x, top.min.y), pos2(p.x, top.max.y), m, 1.5, 1.5);
        dashed(&painter, pos2(left.min.x, p.y), pos2(left.max.x, p.y), m, 1.5, 1.5);
    }
}

/// The horizontal ruler of the spread in view in pieces `(from x, to x, origin)`, canvas
/// coordinates: the ruler origin (one per page for a page origin) moved by zero point `zp`.
fn ruler_pieces(app: &DesignApp, doc: &Document, layout: &CanvasLayout, zp: [f64; 2]) -> Vec<(f64, f64, Point)> {
    let Some(slot) = current_slot(app, layout).and_then(|i| layout.slots.get(i)) else { return Vec::new() };
    ruler_pieces_of(doc, slot, zp)
}

/// [`ruler_pieces`] for the spread in `slot`.
fn ruler_pieces_of(doc: &Document, slot: &SpreadSlot, zp: [f64; 2]) -> Vec<(f64, f64, Point)> {
    let x = |v: f64, y: f64| if v.is_finite() { slot.to_canvas(Point::new(v, y)).x } else { v };
    doc.ruler_pieces(slot.spread)
        .into_iter()
        .map(|(from, to, b)| (x(from, b.y), x(to, b.y), slot.to_canvas(Point::new(b.x + zp[0], b.y + zp[1]))))
        .collect()
}

/// The horizontal (top) and vertical (left) rulers around the view `rect` in `full`.
fn ruler_rects(full: Rect, rect: Rect) -> (Rect, Rect) {
    (
        Rect::from_min_max(pos2(rect.min.x, full.min.y), pos2(full.max.x, rect.min.y)),
        Rect::from_min_max(pos2(full.min.x, rect.min.y), pos2(rect.min.x, full.max.y)),
    )
}

/// Right-clicking a ruler opens its menu, as InDesign's: units for that ruler, Hide Rulers and
/// (the horizontal ruler) Delete All Guides on Spread. Right-clicking the corner where they meet
/// opens Lock Zero Point and the units for both rulers at once.
fn ruler_units_menus(app: &mut DesignApp, resp: &egui::Response, full: Rect, rect: Rect, doc: &Document) {
    let (top, left) = ruler_rects(full, rect);
    let corner = Rect::from_min_max(full.min, rect.min);
    let at = resp.interact_pointer_pos();
    let (h, v) = (doc.settings.horizontal_units, doc.settings.vertical_units);
    let locked = doc.settings.zero_point_locked;
    // (where, which units it sets, the unit checked)
    let menus = [(top, RulerMenu::Horizontal, Some(h)), (left, RulerMenu::Vertical, Some(v)), (corner, RulerMenu::Both, (h == v).then_some(h))];
    for (area, which, cur) in menus {
        let open = if resp.secondary_clicked() {
            Some(egui::SetOpenCommand::Bool(at.is_some_and(|p| area.contains(p))))
        } else if resp.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        egui::Popup::context_menu(resp)
            .id(resp.id.with(("ruler_units", which as u8)))
            .open_memory(open)
            .show(|ui| ruler_units_menu(app, ui, which, cur, locked));
    }
}

#[derive(Clone, Copy, PartialEq)]
enum RulerMenu {
    Horizontal,
    Vertical,
    Both,
}

/// The units a ruler's menu offers, in InDesign's order.
const RULER_UNITS: [designcraft_geom::Unit; 9] = {
    use designcraft_geom::Unit::*;
    [Points, Picas, Inches, InchesDecimal, Millimeters, Centimeters, Ciceros, Agates, Pixels]
};

/// A ruler's menu: the units (the current one checked), then Hide Rulers and, on the horizontal
/// ruler, Delete All Guides on Spread. The corner's menu (`Both`) starts with Lock Zero Point
/// (`locked` = the zero point is locked). Picking a unit sets `horizontalUnits`,
/// `verticalUnits` or both through `document.preferences`.
fn ruler_units_menu(app: &mut DesignApp, ui: &mut egui::Ui, which: RulerMenu, cur: Option<designcraft_geom::Unit>, locked: bool) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    if which == RulerMenu::Both {
        let label = tr(if locked { "Unlock Zero Point" } else { "Lock Zero Point" });
        if ui.button(crate::rtl::widget(ui, format!("   {label}"))).clicked() {
            let _ = app.run("view.lockZeroPoint", json!({"on": !locked}));
            ui.close();
        }
        ui.separator();
    }
    // A document in Q or Ha keeps its unit on the list.
    let extra = cur.filter(|u| !RULER_UNITS.contains(u));
    for u in RULER_UNITS.into_iter().chain(extra) {
        let label = tr(u.label());
        let text = if Some(u) == cur { format!("✓ {label}") } else { format!("   {label}") };
        if ui.button(crate::rtl::widget(ui, text)).clicked() {
            let p = match which {
                RulerMenu::Horizontal => json!({"horizontalUnits": u}),
                RulerMenu::Vertical => json!({"verticalUnits": u}),
                RulerMenu::Both => json!({"horizontalUnits": u, "verticalUnits": u}),
            };
            let _ = app.run("document.preferences", p);
            ui.close();
        }
    }
    ui.separator();
    let hide = format!("   {}", tr("Hide Rulers"));
    if ui
        .add(egui::Button::new(crate::rtl::widget(ui, hide)).shortcut_text(crate::menus::shortcut_of(app, "view.rulers").unwrap_or_default()))
        .clicked()
    {
        let _ = app.run("view.rulers", json!({}));
        ui.close();
    }
    if which == RulerMenu::Horizontal && ui.button(crate::rtl::widget(ui, format!("   {}", tr("Delete All Guides on Spread")))).clicked() {
        crate::menus::activate(app, "app.deleteAllGuides", &Value::Null);
        ui.close();
    }
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
fn context_menu(app: &mut DesignApp, ui: &egui::Ui, resp: &egui::Response) {
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

pub(crate) fn fmt_tick(v: f64) -> String {
    let r = v.round();
    if (v - r).abs() < 1e-6 { format!("{}", r as i64) } else { format!("{v:.1}") }
}

/// Move the view's content by a screen-space distance `d` (rotated views move in screen axes).
fn pan(app: &mut DesignApp, xf: &Xf, d: egui::Vec2) {
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
fn scrollbars(app: &mut DesignApp, ui: &mut egui::Ui, outer: Rect, screen: Rect, rect: Rect, t: &Tokens) {
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

/// Give an open menu priority, then handle wheel/zoom/pan, spring Zoom, guide drags and primary pointer routing.
fn handle_input(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect) {
    // While a context menu is open (in either pane), the click that closes it and the keys go to the menu.
    let menu_open = context_menu_open(ui.ctx());
    let Some(v) = app.view().copied() else { return };
    let xf = Xf::new(rect, &v);
    let wants_text = app.session.wants_text();
    let space = navigation_space(app, ui.ctx());
    // Scroll / zoom.
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
    let tool_drag: bool = ui.data(|d| d.get_temp(egui::Id::unique(("canvas_pointer_down", app.pane)))).unwrap_or(false);
    let hand = space && !ui.input(|i| i.modifiers.command);
    if !tool_drag && ((hand && resp.dragged()) || resp.dragged_by(egui::PointerButton::Middle)) {
        let d = xf.unrotate_delta(resp.drag_delta());
        if let Some(v) = app.view_mut() {
            v.origin = Point::new(v.origin.x - d.x as f64 / v.zoom, v.origin.y - d.y as f64 / v.zoom);
        }
        return;
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
            pan(app, &xf, resp.drag_delta());
        }
        ui.data_mut(|d| d.insert_temp(alt_hand_id, primary_down));
        return;
    }
    // The zero point and ruler guides need the rulers (hidden while the view is rotated).
    if !space && !menu_open && xf.rot == 0 && zero_point_drag(app, ui, resp, rect, &xf) {
        return;
    }
    // Hidden or locked guides can't stay selected.
    if (!app.ui.guides || app.ui.guides_locked) && app.session.active().is_some_and(|st| !st.selection.guides.is_empty()) {
        let _ = app.run("guide.deselect", json!({}));
    }
    if !space && !menu_open && xf.rot == 0 && guide_drag(app, ui, resp, rect, &xf) {
        return;
    }
    if !space && !menu_open && xf.rot == 0 && column_guide_drag(app, ui, resp, rect, &xf) {
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
    let origin = origin.or_else(|| {
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
    });
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
    if down
        && resp.dragged()
        && resp.drag_delta() != egui::Vec2::ZERO
        && let Some(p) = latest
    {
        events.push(PointerEvent { kind: PointerKind::Drag, pos: pos(p), mods: m, clicks: 1 });
    }
    // Power Zoom: holding the Hand tool still for half a second turns the press into one.
    let hold_id = egui::Id::unique(("canvas_hold", app.pane));
    if down && !released && app.session.tool_id() == "hand" && !m.alt {
        let (now, origin_pos) = ui.input(|i| (i.time, i.pointer.press_origin()));
        let (start, fired): (f64, bool) = ui.data(|d| d.get_temp(hold_id)).unwrap_or((now, false));
        let still = origin_pos.zip(latest).is_some_and(|(o, l)| (o - l).length() < 3.0);
        if !fired && still && now - start >= 0.5 {
            let p = pos(latest.unwrap_or(rect.center()));
            events.push(PointerEvent { kind: PointerKind::Up, pos: p, mods: m, clicks: 1 });
            events.push(PointerEvent { kind: PointerKind::Down, pos: p, mods: Mods { alt: true, ..m }, clicks: 1 });
            ui.data_mut(|d| d.insert_temp(hold_id, (start, true)));
        } else {
            ui.data_mut(|d| d.insert_temp(hold_id, (start, fired)));
            if !fired {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    } else {
        ui.data_mut(|d| d.remove::<(f64, bool)>(hold_id));
    }
    if down && released {
        if let Some(c) = ui.data(|d| d.get_temp::<Clicks>(clicks_id)) {
            let dist = ui.ctx().options(|o| o.input_options.max_click_dist);
            ui.data_mut(|d| d.insert_temp(clicks_id, release_clicks(c, latest.unwrap_or(c.pos), now, dist)));
        }
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
fn modal_open(app: &DesignApp) -> bool {
    app.ui.dialog.is_some() || app.ui.palette.is_some()
}

/// Does the current pane take the keyboard? Both panes (split, or the second window) see the same
/// events, so typing, tool keys, the clipboard and the input method serve only the focused pane.
fn keyboard_pane(app: &DesignApp) -> bool {
    app.pane == app.focus_pane
}

/// Has the composition these events belong to already ended outside the IME in this batch?
///
/// A pointer press and a document switch end the composition and type its marked text where the
/// caret then is, but the system IME still sends the Commit (or a delete) for the copy it holds.
/// Applying those at the new caret would type the text twice, put it in another story or document,
/// or delete text there, so they are dropped and the IME is told to drop its copy.
fn ime_composition_ended(app: &DesignApp) -> bool {
    app.ime_marked.is_some() && !app.session.tool_composing()
}

/// Did the composition move to another owner (another story, cell or document state) since it
/// began? A late commit then belongs to the old one, not the caret that is there now.
fn ime_composition_moved(app: &DesignApp) -> bool {
    let Some(recorded) = app.ime_target else { return false };
    app.session.ime_text_context().is_some_and(|context| ime_target(&context) != recorded)
}

/// Drop events for a composition that already ended or moved, keeping what was typed.
fn drop_stale_ime_events(app: &mut DesignApp) {
    app.ime_marked = None;
    app.ime_target = None;
    app.pending_ime_delete = None;
    app.ime_discard = true;
    // The host must interrupt the system IME's copy in this pass too, as for a click away.
    app.ime_interrupt = true;
}

/// One input method event for the text caret: marked text previews in the story, a commit types.
fn ime_event(app: &mut DesignApp, ctx: &egui::Context, e: egui::ImeEvent, vi: designcraft_engine::ViewInfo) {
    let r = match e {
        egui::ImeEvent::Preedit { text, active_range_chars } => {
            if text.is_empty() {
                app.ime_marked = None;
                app.ime_target = None;
            } else {
                // The composition's owner at its first marked text: a commit may only land there.
                if app.ime_marked.is_none() {
                    app.ime_target = app.session.ime_text_context().map(|c| ime_target(&c));
                }
                app.ime_marked = Some(text.clone());
            }
            app.session.tool_preedit(&text, active_range_chars, vi).map_err(|e| e.to_string())
        }
        // A composition that already ended or moved in this batch owns nothing any more.
        egui::ImeEvent::Commit(_) | egui::ImeEvent::DeleteSurrounding { .. } | egui::ImeEvent::DeleteSurroundingBytes { .. }
            if app.ime_interrupt || ime_composition_ended(app) || ime_composition_moved(app) =>
        {
            // Dropping a stale delete clears the marked target. Keep the pass's interrupt latch
            // authoritative so the commit following that delete cannot become a fresh insertion.
            drop_stale_ime_events(app);
            Ok(())
        }
        // A bare line break confirms the composition (Enter with some IMEs): the marked text, not
        // a new paragraph.
        egui::ImeEvent::Commit(t) if t == "\n" || t == "\r" => {
            finish_pending_ime_delete(app, ctx);
            keep_marked_text(app);
            Ok(())
        }
        egui::ImeEvent::Commit(t) => ime_commit(app, ctx, &t, vi),
        // The web text agent's corrections and mobile Backspace: characters around the caret go.
        // These are held for the Commit in the same batch so both mutations are one undo step.
        egui::ImeEvent::DeleteSurrounding { before_chars, after_chars } => {
            app.pending_ime_delete = Some((before_chars, after_chars, false));
            Ok(())
        }
        egui::ImeEvent::DeleteSurroundingBytes { before_bytes, after_bytes } => {
            app.pending_ime_delete = Some((before_bytes, after_bytes, true));
            Ok(())
        }
        // `Enabled` and `Disabled` carry nothing: the IME is on while the observation sets `ime`.
        _ => Ok(()),
    };
    if let Err(e) = r {
        app.status(e);
    }
    app.after_engine();
}

/// The bounded committed snapshot and stable target just before an IME mutation.
struct ImeBefore {
    target: ImeTarget,
    stamp: ImeMutationStamp,
    snapshot: IMESurroundingText,
    document_uid: u64,
    document_epoch: u64,
    story: designcraft_doc::StoryId,
    cell: Option<designcraft_doc::CellAddr>,
    anchor: usize,
    focus: usize,
}

/// Build a stable owner from the document uid, story and cell identity, plus the document epoch.
fn ime_target(context: &ImeTextContext<'_>) -> ImeTarget {
    let owner = egui::Id::unique(("designcraft-ime", context.document_uid, context.story.0, context.cell.map(|c| (c.table, c.row, c.col))));
    ImeTarget::new(owner, context.document_epoch)
}

/// Capture the bounded committed surrounding text, mutation stamp, target and selection identity before an IME edit.
fn capture_ime_before(app: &DesignApp) -> Option<ImeBefore> {
    let context = app.session.ime_text_context()?;
    let snapshot = IMESurroundingText::from_text(context.text, context.focus, context.anchor, None);
    let stamp = ImeMutationStamp::from_surrounding_text(snapshot.clone());
    Some(ImeBefore {
        target: ime_target(&context),
        stamp,
        snapshot,
        document_uid: context.document_uid,
        document_epoch: context.document_epoch,
        story: context.story,
        cell: context.cell,
        anchor: context.anchor,
        focus: context.focus,
    })
}

/// Fold a pending delete-surrounding plus commit into one engine operation, clear marked/target state and record the change.
fn ime_commit(app: &mut DesignApp, ctx: &egui::Context, text: &str, vi: designcraft_engine::ViewInfo) -> Result<(), String> {
    let pending = app.pending_ime_delete.take();
    let before = capture_ime_before(app);
    let r = if let Some((before_bytes, after_bytes, bytes)) = pending {
        app.session.ime_commit_after_delete(text, vi, before_bytes, after_bytes, bytes)
    } else {
        app.session.ime_commit(text, vi)
    };
    app.ime_marked = None;
    app.ime_target = None;
    let r = r.map_err(|e| e.to_string());
    if let Ok(mutation) = &r {
        note_ime_change(app, ctx, before, mutation.as_ref(), pending);
    }
    r.map(|_| ())
}

/// Apply a pending IME delete unless its composition ended or moved; record the mutation and drop stale marked text.
fn finish_pending_ime_delete(app: &mut DesignApp, ctx: &egui::Context) {
    let Some((before_bytes, after_bytes, bytes)) = app.pending_ime_delete.take() else { return };
    if ime_composition_ended(app) || ime_composition_moved(app) {
        // The composition ended earlier in this batch: its delete belongs to the copy the system
        // IME still holds, not to the caret that is there now.
        drop_stale_ime_events(app);
        return;
    }
    let before = capture_ime_before(app);
    match app.session.ime_delete_surrounding(before_bytes, after_bytes, bytes) {
        Ok(mutation) => {
            // The delete drops the composition (its marked text is not typed): the IME's copy goes.
            if !app.session.tool_composing() && app.ime_marked.take().is_some() {
                app.ime_discard = true;
            }
            if !app.session.tool_composing() {
                app.ime_target = None;
            }
            note_ime_change(app, ctx, before, mutation.as_ref(), Some((before_bytes, after_bytes, bytes)));
        }
        Err(e) => app.status(e.to_string()),
    }
    app.after_engine();
}

/// Record the origin of a successful IME mutation.
///
/// A receipt is earned only when the engine reported the exact mutation it applied and the bounded
/// committed snapshot agrees with it. Smart quotes, autocorrect, tracked changes and any other
/// application transformation therefore report `Other`, as do failed, stale and no-op mutations.
fn note_ime_change(
    app: &mut DesignApp,
    ctx: &egui::Context,
    before: Option<ImeBefore>,
    mutation: Option<&ImeMutation>,
    delete: Option<(usize, usize, bool)>,
) {
    let Some(before) = before else { return };
    let Some(after) = app.session.ime_text_context() else {
        let viewport = ctx.viewport_id();
        app.ime_tracker.note_application_change(viewport, ctx.cumulative_frame_nr_for(viewport), before.target);
        return;
    };
    if ime_target(&after) != before.target {
        return;
    }
    let after_snapshot = IMESurroundingText::from_text(after.text, after.focus, after.anchor, None);
    if &after_snapshot == before.stamp.surrounding_text() {
        return;
    }
    let after_stamp = ImeMutationStamp::from_surrounding_text(after_snapshot.clone());
    let viewport = ctx.viewport_id();
    let frame_nr = ctx.cumulative_frame_nr_for(viewport);
    if mutation.is_some_and(|m| ime_change_is_exact(&before, &after_snapshot, m, delete)) {
        app.ime_tracker.note_input_method_apply(viewport, frame_nr, before.target, before.stamp, after_stamp);
    } else {
        app.ime_tracker.note_application_change(viewport, frame_nr, before.target);
    }
}

/// Does the bounded post-mutation snapshot agree with the mutation the engine applied?
///
/// The expected result is built from the reported ranges and typed text — absolute byte offsets in
/// the whole story, not the truncated outgoing window — so a delete that reaches past the window
/// still matches. Every visible byte of the after window must agree with it, but a mutation the
/// after window does not overlap at all (a delete longer than the window) rests on the engine's
/// report alone, which is exact because the engine computed it from the whole story. A rewrite,
/// tracked changes or a replaced story is not a receipt.
fn ime_change_is_exact(before: &ImeBefore, after: &IMESurroundingText, m: &ImeMutation, delete: Option<(usize, usize, bool)>) -> bool {
    // The same target, selection and text the receipt was captured around.
    if m.document_uid != before.document_uid
        || m.document_epoch != before.document_epoch
        || m.story != before.story
        || m.cell != before.cell
        || m.anchor_before != before.anchor
        || m.focus_before != before.focus
    {
        return false;
    }
    let (Some(before_payload), Some(after_payload)) = (before.snapshot.available(), after.available()) else { return false };
    let before_text = before_payload.text.as_str();
    let after_text = after_payload.text.as_str();
    let selection = m.anchor_before.min(m.focus_before)..m.anchor_before.max(m.focus_before);
    // The reported deletion adjoins the selection, is inside the text, and exists only for a
    // delete-surrounding event. Its exact extent is the engine's: it computes the ranges from the
    // whole story with the requested counts.
    if m.deleted_before.end != selection.start
        || m.deleted_after.start != selection.end
        || m.deleted_before.start > selection.start
        || m.deleted_after.end > m.text_len_before
    {
        return false;
    }
    if delete.is_none() && (!m.deleted_before.is_empty() || !m.deleted_after.is_empty()) {
        return false;
    }
    // The story changed by exactly the removed bytes, the replaced selection and the typed text:
    // tracked changes and an inserted paragraph return (a table anchor) break this identity.
    let replaced = if m.inserted.is_empty() { 0 } else { selection.len() };
    let expected_len = m
        .text_len_before
        .saturating_sub(m.deleted_before.len())
        .saturating_sub(m.deleted_after.len())
        .saturating_sub(replaced)
        .saturating_add(m.inserted.len());
    if m.text_len_after != expected_len {
        return false;
    }
    // A commit collapses the caret after the typed text; a delete keeps the selection, shifted.
    let (want_anchor, want_focus) = if m.inserted.is_empty() {
        (m.anchor_before.saturating_sub(m.deleted_before.len()), m.focus_before.saturating_sub(m.deleted_before.len()))
    } else {
        let at = m.deleted_before.start + m.inserted.len();
        (at, at)
    };
    if m.anchor_after != want_anchor || m.focus_after != want_focus {
        return false;
    }
    // The text around the caret, rebuilt from the reported mutation. A snapshot's window starts
    // before its selection's start by the selection's own offset in that window.
    let before_win = selection.start.saturating_sub(before_payload.cursor.min(before_payload.anchor));
    let after_selection = m.anchor_after.min(m.focus_after)..m.anchor_after.max(m.focus_after);
    let after_win = after_selection.start.saturating_sub(after_payload.cursor.min(after_payload.anchor));
    if after_win.saturating_add(after_payload.cursor) != m.focus_after || after_win.saturating_add(after_payload.anchor) != m.anchor_after {
        return false;
    }
    let before_end = before_win.saturating_add(before_text.len());
    let after_end = after_win.saturating_add(after_text.len());
    if before_end > m.text_len_before || after_end > m.text_len_after {
        return false;
    }
    let kept = if m.inserted.is_empty() { selection.len() } else { 0 };
    let inserted_post = m.deleted_before.start + kept;
    let tail_post = inserted_post + m.inserted.len();
    let compared = (|| -> Option<usize> {
        let mut n = 0usize;
        n += ime_compare_kept(before_text, before_win..before_end, 0..m.deleted_before.start, 0, after_text, after_win..after_end)?;
        if kept > 0 {
            n += ime_compare_kept(before_text, before_win..before_end, selection.clone(), m.deleted_before.start, after_text, after_win..after_end)?;
        }
        n += ime_compare_window(after_text, after_win..after_end, &(inserted_post..tail_post), &m.inserted)?;
        n += ime_compare_kept(
            before_text,
            before_win..before_end,
            m.deleted_after.end..m.text_len_before,
            tail_post,
            after_text,
            after_win..after_end,
        )?;
        Some(n)
    })();
    compared.is_some()
}

/// Compare one unchanged piece of the story: the pre-mutation bytes `pre` (absolute) appear at
/// post-mutation offset `post_start`. Returns the number of bytes compared, or `None` on a
/// mismatch. Bytes outside the bounded before window cannot be checked and are skipped.
fn ime_compare_kept(
    before_text: &str,
    before_win: std::ops::Range<usize>,
    pre: std::ops::Range<usize>,
    post_start: usize,
    after_text: &str,
    after_win: std::ops::Range<usize>,
) -> Option<usize> {
    let from = pre.start.max(before_win.start);
    let to = pre.end.min(before_win.end);
    if from >= to {
        return Some(0);
    }
    let expected = before_text.get(from - before_win.start..to - before_win.start)?;
    let post = post_start + (from - pre.start)..post_start + (to - pre.start);
    ime_compare_window(after_text, after_win, &post, expected)
}

/// Compare `expected`, which begins at absolute post-mutation offset `post.start`, with what the
/// after window shows of it. Returns the number of bytes compared; `None` when they differ.
fn ime_compare_window(after_text: &str, after_win: std::ops::Range<usize>, post: &std::ops::Range<usize>, expected: &str) -> Option<usize> {
    let from = post.start.max(after_win.start);
    let to = post.end.min(after_win.end);
    if from >= to {
        return Some(0);
    }
    let actual = after_text.get(from - after_win.start..to - after_win.start)?;
    let skip = from - post.start;
    let want = expected.get(skip..skip + (to - from))?;
    (actual == want).then_some(to - from)
}

/// End a composition outside the IME (a click away, the canvas losing the keyboard): the marked
/// text stays as typed, and the system IME is told to drop its copy.
pub(crate) fn keep_marked_text(app: &mut DesignApp) {
    if app.ime_marked.take().is_some() {
        app.ime_discard = true;
    }
    app.ime_target = None;
    if app.session.tool_composing() {
        if let Err(e) = app.session.end_composition() {
            app.status(e.to_string());
        }
        app.after_engine();
    }
}

/// What the canvas found for this viewport pass, observed once at the end of the pass.
pub(crate) struct CanvasIme {
    /// The viewport it was found in: the second window's canvas runs inside the root's pass and
    /// must not consume the root's observation.
    viewport: egui::ViewportId,
    /// The caret's screen rectangle, when the canvas owns the text caret this pass.
    caret: Option<Rect>,
}

/// While the text caret is in the canvas, let the system IME compose (egui-winit allows it only in
/// frames that set `ime`; the web build focuses its hidden text input then) and keep the candidate
/// window at the caret, through zoom, scrolling, view rotation and vertical text.
///
/// The caret is only recorded here: the observation waits for the end of the viewport's pass, so
/// a command or a field that takes the keyboard in a later section of the UI is included.
fn ime_canvas_pass(app: &mut DesignApp, ctx: &egui::Context, xf: &Xf) {
    if !keyboard_pane(app) {
        // Both panes see the same viewport and events. The focused pane owns this viewport's one
        // observation; observing for the other would reset the context every split-window frame.
        return;
    }
    // A text field, dialog or Quick Apply has the keyboard: egui's own text editing serves the IME.
    let ours = app.session.wants_text() && !ctx.text_edit_focused() && !modal_open(app);
    let caret = if ours { app.session.tool_ime_caret(app.view_info()) } else { None };
    let caret = caret.map(|(a, b)| Rect::from_two_pos(xf.to_screen(a), xf.to_screen(b)).expand(1.0));
    app.canvas_ime = caret.is_some();
    app.ime_canvas = Some(CanvasIme { viewport: ctx.viewport_id(), caret });
}

/// Observe the IME once for this viewport pass, after every command and focus change it applied.
pub(crate) fn ime_observe(app: &mut DesignApp, ctx: &egui::Context) {
    let viewport = ctx.viewport_id();
    if app.ime_canvas.as_ref().is_none_or(|canvas| canvas.viewport != viewport) {
        return;
    }
    let Some(canvas) = app.ime_canvas.take() else { return };
    // A text field, dialog or Quick Apply that took the keyboard later in the pass owns the IME.
    let ours = canvas.caret.is_some() && !ctx.text_edit_focused() && !modal_open(app);
    let Some(r) = canvas.caret.filter(|_| ours) else {
        // The IME goes away with its marked text: keep that text as typed, like a click away.
        keep_marked_text(app);
        app.ime_tracker.observe(viewport, None);
        return;
    };
    // The composition ended outside the IME (a click, a command): the IME drops what it has marked.
    let interrupt = app.ime_interrupt || (app.ime_marked.is_some() && !app.session.tool_composing());
    app.ime_interrupt = false;
    if interrupt {
        app.ime_marked = None;
        app.ime_target = None;
        app.ime_discard = true;
    }
    app.canvas_ime = true;
    // One observation per viewport pass, after this pass's events and commands. The committed
    // text excludes any active pre-edit, and the tracker copies only the bounded IME window.
    let update = match app.session.ime_text_context() {
        Some(context) => {
            let observation = ImeObservation::new(ctx.cumulative_frame_nr_for(viewport), ctx.cumulative_pass_nr_for(viewport), ime_target(&context))
                .purpose(egui::IMEPurpose::Normal)
                .committed(Some(CommittedText::new(context.text, context.focus, context.anchor)));
            app.ime_tracker.observe(viewport, Some(observation))
        }
        None => app.ime_tracker.observe(viewport, None),
    };
    let Some(update) = update else { return };
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput {
            owner: update.owner,
            context_generation: update.context_generation,
            purpose: update.purpose,
            rect: r,
            cursor_rect: r,
            should_interrupt_composition: interrupt,
            reset_request: None,
            change_cause: update.change_cause,
            surrounding_text: update.surrounding_text,
        });
    });
}

/// View → Show Hidden Characters: ¶ paragraph ends, » tabs, · spaces, ¬ forced line breaks, # end of story.
fn draw_hidden_characters(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
    for story in doc.stories.values() {
        let cs = app.session.cache.get(doc, story.id, None);
        for ft in &cs.frames {
            let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { continue };
            let m = a * doc.text_xf(it);
            let col = layer_color(doc, it);
            for l in &ft.lines {
                let size = (l.ascent * 0.75 * xf.zoom).clamp(6.0, 40.0) as f32;
                let font = egui::FontId::proportional(size);
                for (x, mark, centered) in hidden_marks(&story.text, l) {
                    let p = xf.to_screen(m * Point::new(x, l.baseline));
                    let align = if centered { egui::Align2::CENTER_BOTTOM } else { egui::Align2::LEFT_BOTTOM };
                    painter.text(p, align, mark, font.clone(), col);
                }
            }
        }
    }
}

/// The hidden-character marks on line `l` of a story with text `text`: frame-space x, mark, and
/// whether the mark is centered on x (spaces) or starts at it.
fn hidden_marks(text: &str, l: &compose::Line) -> Vec<(f64, &'static str, bool)> {
    let mut out = Vec::new();
    for g in &l.glyphs {
        if g.len == 0 {
            continue;
        }
        let Some(ch) = text.get(g.byte..).and_then(|t| t.chars().next()) else { continue };
        let mark = match ch {
            ' ' => "·",
            '\t' => "»",
            '\u{2028}' => "¬",
            '\u{A0}' => "°",
            _ => continue,
        };
        if ch == ' ' {
            out.push((g.x + g.adv / 2.0, mark, true));
        } else {
            out.push((g.x, mark, false));
        }
    }
    if l.last_in_para {
        // The spaces trimmed off the line's end sit past `end_x`; the paragraph mark follows them.
        let end = l.glyphs.iter().filter(|g| g.len > 0 && !g.visible && g.x >= l.end_x - 1e-9).map(|g| g.x + g.adv).fold(l.end_x, f64::max);
        let mark = if l.range.end >= text.len() { "#" } else { "¶" };
        out.push((end + 1.0, mark, false));
    }
    out
}

/// View › Flattener Preview: red over every object that involves transparency (opacity, blend
/// modes, effects, knockout) — what flattening or export rasterising affects.
fn draw_flattener_preview(painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
    fn transparent(it: &Item) -> bool {
        it.opacity < 1.0 || it.blend != Default::default() || it.effects.any() || it.knockout || it.children().iter().any(|c| transparent(c))
    }
    let red = Color32::from_rgba_unmultiplied(230, 30, 30, 110);
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        for it in sp.items.iter().filter(|it| !it.hidden && transparent(it)) {
            let r = it.bounds().inflate(designcraft_render::effect_outset(it), designcraft_render::effect_outset(it));
            painter.rect_filled(xf.rect(slot.xf.transform_rect_bbox(r)), 0.0, red);
        }
    }
}

/// Edit › Spelling › Dynamic Spelling: red squiggles under misspelled words (checked once per
/// story version).
fn draw_dynamic_spelling(app: &DesignApp, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
    for story in doc.stories.values() {
        let key = egui::Id::unique(("dyn_spell", std::sync::Arc::as_ptr(story) as usize, doc.user_words.len()));
        let bad: std::sync::Arc<Vec<std::ops::Range<usize>>> = match ctx.data(|d| d.get_temp(key)) {
            Some(v) => v,
            None => {
                let v = std::sync::Arc::new(designcraft_engine::cmd::spelling::misspellings(doc, story, &doc.user_words));
                ctx.data_mut(|d| d.insert_temp(key, v.clone()));
                v
            }
        };
        if bad.is_empty() {
            continue;
        }
        let cs = app.session.cache.get(doc, story.id, None);
        let red = Stroke::new(1.0, Color32::from_rgb(230, 30, 30));
        for ft in &cs.frames {
            let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { continue };
            let m = a * doc.text_xf(it);
            for l in &ft.lines {
                for r in bad.iter().filter(|r| r.start < l.range.end && r.end > l.range.start) {
                    let gs: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte >= r.start && g.byte < r.end).collect();
                    let (Some(x0), Some(x1)) = (gs.iter().map(|g| g.x).reduce(f64::min), gs.iter().map(|g| g.x + g.adv).reduce(f64::max)) else {
                        continue;
                    };
                    let y = l.baseline + l.descent * 0.5;
                    let (p0, p1) = (xf.to_screen(m * Point::new(x0, y)), xf.to_screen(m * Point::new(x1, y)));
                    // A zigzag 2 px high, 4 px per wave.
                    let n = ((p1.x - p0.x) / 2.0).max(1.0) as usize;
                    let pts: Vec<Pos2> =
                        (0..=n).map(|k| pos2(p0.x + (p1.x - p0.x) * k as f32 / n as f32, p0.y + if k % 2 == 0 { 0.0 } else { 2.0 })).collect();
                    painter.add(egui::Shape::line(pts, red));
                }
            }
        }
    }
}

/// Plain text of the current text selection (story markers resolved to readable characters).
pub fn selected_text(app: &DesignApp) -> Option<String> {
    let st = app.session.active()?;
    let t = st.selection.text?;
    let story = st.doc.text_story(t.story, t.cell)?;
    let s = story.slice(t.range());
    if s.is_empty() {
        return None;
    }
    Some(s.replace(designcraft_doc::story::FORCED_LINE_BREAK, "\n"))
}

/// One device pixel, the width InDesign uses for guides and frame edges.
fn hair(painter: &egui::Painter) -> f32 {
    1.0 / painter.ctx().pixels_per_point()
}

const ZERO_POINT_DRAG: &str = "canvas_zero_point_drag";

/// The ruler corner: drag from it and the zero point lands where the pointer is released; a click
/// that stays in the corner leaves it; a double-click resets it. Locked, the corner ignores both.
/// Cmd-dragged (Ctrl elsewhere), it makes a horizontal and a vertical guide instead.
/// Holds the pointer position while dragging; returns true while it owns the pointer.
fn zero_point_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
    let id = egui::Id::unique((ZERO_POINT_DRAG, app.pane));
    let corner = Rect::from_min_max(resp.rect.min, rect.min);
    let (pressed, released, down, origin, latest, dbl, command) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
            i.modifiers.command,
        )
    });
    let mut at: Option<Pos2> = ui.data(|d| d.get_temp(id));
    if at.is_none() {
        if !pressed || !resp.hovered() || app.session.active().is_none() || corner.width() <= 0.0 || corner.height() <= 0.0 {
            return false;
        }
        at = origin.filter(|o| corner.contains(*o));
    }
    let Some(mut p) = at else { return false };
    if let Some(l) = latest.filter(|l| l.x.is_finite() && l.y.is_finite()) {
        p = l;
    }
    if !released && down {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        ui.data_mut(|d| d.insert_temp(id, p));
        return true;
    }
    ui.data_mut(|d| d.remove::<Pos2>(id));
    if !released {
        // The release happened where we couldn't see it (outside the window).
        return false;
    }
    let locked = app.session.active().is_none_or(|st| st.doc.settings.zero_point_locked);
    if command {
        if !corner.contains(p) && rect.contains(p) {
            corner_guides(app, xf, p);
        }
    } else if !locked {
        if dbl {
            let _ = app.run("view.zeroPoint", json!({"reset": true}));
        } else if !corner.contains(p)
            && let Some((zp, _)) = zero_point_at(app, xf, p)
        {
            let _ = app.run("view.zeroPoint", json!({ "at": zp }));
        }
    }
    true
}

/// A screen point as a zero point (its offset from the ruler origin of the spread in view, kept
/// on the pasteboard as the engine keeps it), and where that zero point is on the canvas.
fn zero_point_at(app: &DesignApp, xf: &Xf, p: Pos2) -> Option<([f64; 2], Point)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let slot = layout.slots.get(current_slot(app, &layout)?)?;
    let sp = slot.to_spread(xf.to_canvas(p));
    let base = st.doc.ruler_base(slot.spread, sp.x)?;
    let zp = st.doc.clamp_zero_point([sp.x - base.x, sp.y - base.y])?;
    Some((zp, slot.to_canvas(Point::new(base.x + zp[0], base.y + zp[1]))))
}

/// Cmd-drag from the ruler corner: a horizontal and a vertical spread guide through the drop
/// point, on the spread under it.
fn corner_guides(app: &mut DesignApp, xf: &Xf, p: Pos2) {
    if !app.ui.guides || app.ui.guides_locked {
        return;
    }
    let Some((r, sp)) = app.session.active().and_then(|st| CanvasLayout::new(&st.doc, st.editing_parents).spread_at(xf.to_canvas(p))) else {
        return;
    };
    if !(sp.x.is_finite() && sp.y.is_finite()) {
        return;
    }
    let _ = app.run("guide.add", json!({"spread": r, "orientation": "both", "point": [sp.x, sp.y], "spreadGuide": true}));
}

const GUIDE_DRAG: &str = "canvas_guide_drag";

/// A ruler guide being dragged: out of a ruler (`from` None) or an existing one.
#[derive(Clone, Debug)]
struct GuideDrag {
    orientation: designcraft_doc::Orientation,
    from: Option<(designcraft_doc::SpreadRef, usize, usize)>,
    /// Pointer position (screen).
    at: Option<Pos2>,
    /// Modifiers held at the pointer's last move: Shift snaps to the ruler's ticks, Command
    /// suspends snapping.
    mods: egui::Modifiers,
}

const GUIDE_PRESS: &str = "canvas_guide_press";

/// A press on an existing guide: where it was (screen) and whether Shift was down. Released
/// where it was pressed, it is a click, which selects the guide.
#[derive(Clone, Copy, Debug)]
struct GuidePress {
    origin: Pos2,
    shift: bool,
}

/// How far (screen pixels) the pointer may move between press and release for a click.
const GUIDE_CLICK_SLOP: f32 = 3.0;

/// Where a dragged guide is, as the rulers measure it (#1282): `X: …` for a vertical guide, `Y: …`
/// for a horizontal one, from the zero point (and the page origin, when rulers restart per page)
/// in the document's ruler units. `None` with no document or rulers.
fn guide_readout(app: &DesignApp, xf: &Xf, g: &GuideDrag) -> Option<String> {
    let (label, value, unit) = guide_measure(app, xf, g)?;
    Some(format!("{label}: {}", designcraft_geom::units::format_measure(value, unit)))
}

/// Where a pointer readout of `size` goes for a pointer at `p`: below right of it, on the other
/// side where that would leave the canvas `area` (near its right or bottom edge), and kept inside
/// it (with the label's 3-point frame) however small the canvas is.
fn readout_at(p: Pos2, size: egui::Vec2, area: Rect) -> Pos2 {
    const GAP: f32 = 12.0;
    const PAD: f32 = 3.0;
    let x = if p.x + GAP + size.x + PAD > area.max.x { p.x - GAP - size.x } else { p.x + GAP };
    let y = if p.y + GAP + size.y + PAD > area.max.y { p.y - GAP - size.y } else { p.y + GAP };
    let clamp = |v: f32, lo: f32, hi: f32| if hi < lo { lo } else { v.clamp(lo, hi) };
    pos2(clamp(x, area.min.x + PAD, area.max.x - PAD - size.x), clamp(y, area.min.y + PAD, area.max.y - PAD - size.y))
}

/// [`guide_readout`]'s parts: the axis label, the value (points) and its unit.
fn guide_measure(app: &DesignApp, xf: &Xf, g: &GuideDrag) -> Option<(&'static str, f64, designcraft_geom::Unit)> {
    let p = g.at?;
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    // Measured where the guide is dropped (`guide_drag`): its snapped spot, on the spread under
    // the pointer, as the rulers measure that spread, not the one in the middle of the view.
    let (c, slot) = match guide_spot(app, xf, g) {
        Some(spot) => (layout.to_canvas(spot.spread, spot.sp), layout.slot(spot.spread)?),
        None => (xf.to_canvas(p), current_slot(app, &layout).and_then(|i| layout.slots.get(i))?),
    };
    // An existing guide moves only on its own spread (`guide_drag`): over another, dropping it
    // leaves it where it was, so there is no place to show.
    if g.from.is_some_and(|(r, _, _)| r != slot.spread) {
        return None;
    }
    let pieces = ruler_pieces_of(&st.doc, slot, st.doc.zero_point());
    let first = pieces.first()?;
    let (label, value, unit) = match g.orientation {
        designcraft_doc::Orientation::Vertical => {
            // The horizontal ruler restarts at each page: the piece under the guide measures it.
            let o = pieces.iter().find(|(from, to, _)| *from <= c.x && c.x < *to).unwrap_or(first).2;
            ("X", c.x - o.x, st.doc.settings.horizontal_units)
        }
        designcraft_doc::Orientation::Horizontal => ("Y", c.y - first.2.y, st.doc.settings.vertical_units),
    };
    value.is_finite().then_some((label, value, unit))
}

/// Where a dragged ruler guide is (#1221).
struct GuideSpot {
    /// The spread under the pointer.
    spread: SpreadRef,
    /// The pointer in that spread's coordinates.
    pointer: Point,
    /// [`Self::pointer`] with the guide's axis snapped.
    sp: Point,
    /// [`Self::sp`] on screen, where the preview line goes.
    screen: Pos2,
    /// Alignment marks (canvas coordinates) when an object or page edge or center caught it.
    marks: Vec<Overlay>,
}

/// Where the guide of drag `g` is: its axis (x for a vertical guide, y for a horizontal one)
/// snapped as a moved object's is, to ruler guides, margins, columns, bleed, the document grid and
/// smart-guide targets (not the baseline grid, which would catch a horizontal guide almost
/// anywhere). Shift snaps to the ruler's ticks instead and Command suspends snapping. The guides
/// being dragged are not targets. An existing guide over another spread, where it
/// can't be dropped, stays at the pointer. The preview, the readout and the drop all read it.
/// `None` with no spread under the pointer.
fn guide_spot(app: &DesignApp, xf: &Xf, g: &GuideDrag) -> Option<GuideSpot> {
    let p = g.at?;
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let (r, pointer) = layout.spread_at(xf.to_canvas(p))?;
    let spot = |sp: Point, marks: Vec<Overlay>| GuideSpot { spread: r, pointer, sp, screen: xf.to_screen(layout.to_canvas(r, sp)), marks };
    if g.mods.command || g.from.is_some_and(|(from, _, _)| from != r) {
        return Some(GuideSpot { spread: r, pointer, sp: pointer, screen: p, marks: Vec::new() });
    }
    let vertical = matches!(g.orientation, designcraft_doc::Orientation::Vertical);
    let place = |v: f64| if vertical { Point::new(v, pointer.y) } else { Point::new(pointer.x, v) };
    if g.mods.shift {
        let slot = layout.slot(r)?;
        let at = ruler_tick_of(&st.doc, slot, g.orientation, xf.zoom, pointer).map_or(pointer, place);
        return Some(spot(at, Vec::new()));
    }
    let spread = st.doc.spread(r)?;
    // Not targets: the dragged guide, and the others of a selected group, which move with it.
    let exclude: Vec<GuideSel> = match g.from {
        Some((sr, pi, gi)) if drags_group(app, GuideSel { spread: sr, page: pi, index: gi }) => st.selection.guides.to_vec(),
        Some((sr, pi, gi)) => vec![GuideSel { spread: sr, page: pi, index: gi }],
        None => Vec::new(),
    };
    // How far the guide reaches across its axis: its page, or the whole spread.
    let page = match g.from {
        Some((_, pi, gi)) => spread.pages.get(pi).filter(|pg| pg.guides.get(gi).is_some_and(|guide| !guide.spread)),
        None => spread.pages.iter().find(|pg| pg.bounds().contains(pointer)),
    };
    let reach = page.map_or_else(|| spread.bounds(), |pg| pg.bounds());
    let (at, rect, x_edges, y_edges) = if vertical {
        (pointer.x, DRect::new(pointer.x, reach.y0, pointer.x, reach.y1), [true; 3], [false; 3])
    } else {
        (pointer.y, DRect::new(reach.x0, pointer.y, reach.x1, pointer.y), [false; 3], [true; 3])
    };
    let cx = designcraft_tools::ToolContext {
        doc: &st.doc,
        selection: &st.selection,
        cache: &app.session.cache,
        layout: &layout,
        zoom: xf.zoom.max(1e-6),
        layer: st.active_layer,
        snap: app.ui.snap_view(),
        unit: app.ui.units,
        stroke_in_front: false,
    };
    let s = designcraft_tools::snap::snap(
        &cx,
        designcraft_tools::SnapRequest {
            spread: r,
            gesture: designcraft_tools::Gesture::Guide,
            rect,
            x_edges,
            y_edges,
            exclude: &[],
            exclude_guides: &exclude,
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer,
        },
    );
    let snapped = at + if vertical { s.delta.x } else { s.delta.y };
    Some(if snapped.is_finite() { spot(place(snapped), s.guides) } else { spot(pointer, Vec::new()) })
}

/// The nearest ruler tick to spread point `sp` along a guide of `orientation`'s axis, in spread
/// coordinates: the horizontal ruler's (the piece under the point) where that axis runs across the
/// screen, else the vertical ruler's. `None` when the ruler has no ticks there.
fn ruler_tick_of(doc: &Document, slot: &SpreadSlot, orientation: designcraft_doc::Orientation, zoom: f64, sp: Point) -> Option<f64> {
    let vertical = matches!(orientation, designcraft_doc::Orientation::Vertical);
    // A quarter-turned spread runs its x axis down the screen.
    let across = vertical == slot.rotation.is_multiple_of(2);
    let c = slot.to_canvas(sp);
    let pieces = ruler_pieces_of(doc, slot, doc.zero_point());
    let step = |unit: designcraft_geom::Unit| {
        let (major, sub) = unit.ruler_ticks(zoom);
        let step = major / f64::from(sub.max(1));
        (step.is_finite() && step > 0.0).then_some(step)
    };
    let tick = |v: f64, o: f64, step: f64| o + ((v - o) / step).round() * step;
    let c = if across {
        let &(_, _, o) = pieces.iter().find(|(from, to, _)| *from <= c.x && c.x < *to).or(pieces.first())?;
        Point::new(tick(c.x, o.x, step(doc.settings.horizontal_units)?), c.y)
    } else {
        let o = pieces.first()?.2;
        Point::new(c.x, tick(c.y, o.y, step(doc.settings.vertical_units)?))
    };
    let s = slot.to_spread(c);
    Some(if vertical { s.x } else { s.y }).filter(|v| v.is_finite())
}

/// The guide under a screen point (within 3 px), as (spread, page, index, orientation).
fn guide_at(app: &DesignApp, xf: &Xf, p: Pos2) -> Option<(designcraft_doc::SpreadRef, usize, usize, designcraft_doc::Orientation)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let cp = xf.to_canvas(p);
    let tol = 3.0 / xf.zoom;
    for slot in &layout.slots {
        let sp = st.doc.spread(slot.spread)?;
        let local = slot.to_spread(cp);
        for (pi, pg) in sp.pages.iter().enumerate() {
            let pr = if pg.guides.iter().any(|g| g.spread) { slot.xf.inverse().transform_rect_bbox(layout.pasteboard) } else { pg.bounds() };
            for (gi, g) in pg.guides.iter().enumerate() {
                if !g.editable_in(&st.doc) {
                    continue;
                }
                let r = if g.spread { pr } else { pg.bounds() };
                let hit = match g.orientation {
                    designcraft_doc::Orientation::Horizontal => (local.y - g.position).abs() <= tol && local.x >= r.x0 && local.x <= r.x1,
                    designcraft_doc::Orientation::Vertical => (local.x - g.position).abs() <= tol && local.y >= r.y0 - tol && local.y <= r.y1 + tol,
                };
                if hit && !g.locked {
                    return Some((slot.spread, pi, gi, g.orientation));
                }
            }
        }
    }
    None
}

/// A primary press at `o` on a ruler starts a new guide; on a guide it selects and grabs it.
fn begin_guide_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf, o: Pos2) -> Option<GuideDrag> {
    let full = resp.rect;
    if full.contains(o) && !rect.contains(o) {
        // In a ruler (not the corner).
        let orientation = if o.y < rect.min.y && o.x >= rect.min.x {
            Some(designcraft_doc::Orientation::Horizontal)
        } else if o.x < rect.min.x && o.y >= rect.min.y {
            Some(designcraft_doc::Orientation::Vertical)
        } else {
            None
        };
        if let Some(orientation) = orientation {
            return Some(GuideDrag { orientation, from: None, at: Some(o), mods: egui::Modifiers::NONE });
        }
    } else if rect.contains(o)
        && matches!(app.session.tool_id(), "selection" | "directSelection")
        && let Some((r, pi, gi, orientation)) = guide_at(app, xf, o)
    {
        let shift = ui.input(|i| i.modifiers.shift);
        // A press on a guide selects it (Shift: on release, as a click adds or removes it).
        let selected = app.session.active().is_some_and(|st| st.selection.guides.contains(&GuideSel { spread: r, page: pi, index: gi }));
        if !shift && !selected {
            let _ = app.run("guide.select", json!({"spread": r, "page": pi, "index": gi}));
        }
        ui.data_mut(|d| d.insert_temp(egui::Id::unique(GUIDE_PRESS), GuidePress { origin: o, shift }));
        return Some(GuideDrag { orientation, from: Some((r, pi, gi)), at: Some(o), mods: egui::Modifiers::NONE });
    }
    None
}

/// Ruler guides: drag one out of a ruler, drag an existing one (Selection tools) to move it, drop
/// it back on a ruler to delete it. A press near an unlocked guide takes it even over an object:
/// the guide is selected and the object is not. Returns true from the press to the release, so
/// the tools never see that press.
fn guide_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
    let id = egui::Id::unique(GUIDE_DRAG);
    let (pressed, released, origin, latest) =
        ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin(), i.pointer.latest_pos()));
    let mut drag: Option<GuideDrag> = ui.data(|d| d.get_temp(id));
    if drag.is_none() && pressed && resp.hovered() && app.ui.guides && !app.ui.guides_locked {
        let Some(o) = origin else { return false };
        drag = begin_guide_drag(app, ui, resp, rect, xf, o);
    }
    let Some(mut g) = drag else { return false };
    if let Some(p) = latest {
        g.at = Some(p);
    }
    g.mods = ui.input(|i| i.modifiers);
    if !released {
        ui.ctx().set_cursor_icon(match g.orientation {
            designcraft_doc::Orientation::Horizontal => egui::CursorIcon::ResizeVertical,
            designcraft_doc::Orientation::Vertical => egui::CursorIcon::ResizeHorizontal,
        });
        ui.data_mut(|d| d.insert_temp(id, g));
        return true;
    }
    ui.data_mut(|d| d.remove::<GuideDrag>(id));
    let press: Option<GuidePress> = ui.data_mut(|d| {
        let press = d.get_temp(egui::Id::unique(GUIDE_PRESS));
        d.remove::<GuidePress>(egui::Id::unique(GUIDE_PRESS));
        press
    });
    let Some(p) = g.at else { return true };
    if let (Some((r, pi, gi)), Some(press)) = (g.from, press)
        && (p - press.origin).length() <= GUIDE_CLICK_SLOP
    {
        // A click: the guide is selected (at the press); Shift adds it or removes it.
        if press.shift {
            let _ = app.run("guide.select", json!({"spread": r, "page": pi, "index": gi, "toggle": true}));
        }
        return true;
    }
    // Dragging one of several selected guides moves (or deletes) them all.
    let group = g.from.is_some_and(|(spread, page, index)| drags_group(app, GuideSel { spread, page, index }));
    let on_canvas = rect.contains(p);
    // Placed where the preview showed it (snapped); the page under the pointer owns a new guide.
    let target = guide_spot(app, xf, &g).and_then(|spot| {
        let spread = app.session.active()?.doc.spread(spot.spread)?;
        let on_page = spread.pages.iter().position(|pg| pg.bounds().contains(spot.pointer));
        Some((spot.spread, spot.sp, on_page))
    });
    let pos_of = |sp: Point| match g.orientation {
        designcraft_doc::Orientation::Horizontal => sp.y,
        designcraft_doc::Orientation::Vertical => sp.x,
    };
    let orient = match g.orientation {
        designcraft_doc::Orientation::Horizontal => "horizontal",
        designcraft_doc::Orientation::Vertical => "vertical",
    };
    match (g.from, on_canvas, target) {
        // Dropped back on a ruler: delete.
        (Some(_), false, _) if group => {
            let _ = app.run("guide.delete", json!({}));
        }
        (Some((r, pi, gi)), false, _) => {
            let _ = app.run("guide.delete", json!({"spread": r, "page": pi, "index": gi}));
        }
        (Some((r, _, _)), true, Some((tr, _, _))) if tr == r && group => {
            if let Some((dx, dy)) = press.and_then(|press| guide_drag_delta(app, xf, &g, press.origin)) {
                let _ = app.run("guide.moveBy", json!({"dx": dx, "dy": dy}));
            }
        }
        (Some((r, pi, gi)), true, Some((tr, sp, _))) if tr == r => {
            let _ = app.run("guide.move", json!({"spread": r, "page": pi, "index": gi, "position": pos_of(sp)}));
        }
        (None, true, Some((r, sp, on_page))) => {
            // The Page tool draws liquid guides from the rulers.
            let liquid = app.session.tool_id() == "page" && on_page.is_some();
            let mut params =
                json!({"spread": r, "orientation": orient, "position": pos_of(sp), "at": sp.x, "spreadGuide": on_page.is_none(), "liquid": liquid});
            if let Some(pi) = on_page {
                params["page"] = json!(pi);
            }
            let _ = app.run("guide.add", params);
        }
        _ => {}
    }
    true
}

/// Whether dragging guide `from` takes the other selected guides along: it is one of several
/// selected.
fn drags_group(app: &DesignApp, from: GuideSel) -> bool {
    app.session.active().is_some_and(|st| st.selection.guides.len() > 1 && st.selection.guides.contains(&from))
}

/// How far a drag of the existing guide `g.from`, pressed at `origin` (screen), moves guides, in
/// spread coordinates: the dragged guide goes to its [`guide_spot`], the other selected guides as
/// far along their own axis as it moved. `None` while the pointer is not over the guide's own
/// spread, where a drop leaves it in place.
fn guide_drag_delta(app: &DesignApp, xf: &Xf, g: &GuideDrag, origin: Pos2) -> Option<(f64, f64)> {
    let (r, pi, gi) = g.from?;
    let spot = guide_spot(app, xf, g)?;
    if spot.spread != r {
        return None;
    }
    let sp = spot.sp;
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let start = layout.slot(r)?.to_spread(xf.to_canvas(origin));
    let at = st.doc.spread(r)?.pages.get(pi)?.guides.get(gi)?.position;
    let (dx, dy) = match g.orientation {
        designcraft_doc::Orientation::Vertical => (sp.x - at, sp.y - start.y),
        designcraft_doc::Orientation::Horizontal => (sp.x - start.x, sp.y - at),
    };
    (dx.is_finite() && dy.is_finite()).then_some((dx, dy))
}

/// Guides being dragged on the canvas and how far they have moved (spread coordinates).
struct MovingGuides {
    guides: std::collections::HashSet<GuideSel>,
    dx: f64,
    dy: f64,
}

/// What a drag of an existing guide moves, for the preview: the dragged guide, or every selected
/// guide when it is one of several selected. `None` before the drag leaves the click distance,
/// over a ruler (a drop there deletes) and off the guide's spread.
fn moving_guides(app: &DesignApp, xf: &Xf, rect: Rect, g: &GuideDrag, press: GuidePress) -> Option<MovingGuides> {
    let (spread, page, index) = g.from?;
    let p = g.at?;
    if !rect.contains(p) || (p - press.origin).length() <= GUIDE_CLICK_SLOP {
        return None;
    }
    let (dx, dy) = guide_drag_delta(app, xf, g, press.origin)?;
    let from = GuideSel { spread, page, index };
    let guides = if drags_group(app, from) { app.session.active()?.selection.guides.iter().copied().collect() } else { [from].into_iter().collect() };
    Some(MovingGuides { guides, dx, dy })
}

/// An object is under screen point `p` where the Selection tool would pick it (a document
/// spread's, or a parent spread's while editing parents): it takes the press before the guides
/// behind it.
fn over_item(app: &DesignApp, xf: &Xf, p: Pos2) -> bool {
    let Some(st) = app.session.active() else { return false };
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let cx = designcraft_tools::ToolContext {
        doc: &st.doc,
        selection: &st.selection,
        cache: &app.session.cache,
        layout: &layout,
        zoom: xf.zoom.max(1e-6),
        layer: st.active_layer,
        snap: app.ui.snap_view(),
        unit: app.ui.units,
        stroke_in_front: false,
    };
    cx.hit(xf.to_canvas(p)).is_some()
}

const COLUMN_DRAG: &str = "canvas_column_drag";

/// A column guide being dragged with a Selection tool: its gutter follows the pointer.
#[derive(Clone, Debug)]
struct ColumnDrag {
    /// The document the drag's interaction belongs to.
    doc: u64,
    spread: SpreadRef,
    page: usize,
    gutter: usize,
    /// The pointer's spread x minus the gutter's left edge at the press: the gutter keeps that
    /// distance from the pointer.
    grab: f64,
    /// The left edge last previewed.
    last: Option<f64>,
}

/// The interior column guide under screen point `p` (within 3 px) that a press there would drag,
/// as (spread, page, gutter, the gutter's left edge in spread x): a Selection tool, guides shown
/// (not in Preview or Presentation), column guides unlocked, and no object or ruler guide that
/// takes the press there.
fn column_guide_at(app: &DesignApp, xf: &Xf, p: Pos2) -> Option<(SpreadRef, usize, usize, f64)> {
    let preview = matches!(app.ui.screen_mode, ScreenMode::Preview | ScreenMode::Presentation);
    if !app.ui.guides || preview || !matches!(app.session.tool_id(), "selection" | "directSelection") {
        return None;
    }
    let st = app.session.active()?;
    if st.doc.settings.column_guides_locked {
        return None;
    }
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let (r, sp) = layout.spread_at(xf.to_canvas(p))?;
    let tol = 3.0 / xf.zoom.max(1e-9);
    let hit = st.doc.spread(r)?.pages.iter().enumerate().find_map(|(pi, pg)| {
        let m = pg.margin_rect();
        if sp.y < m.y0 - tol || sp.y > m.y1 + tol {
            return None;
        }
        pg.column_rects().windows(2).enumerate().find_map(|(gi, w)| match w {
            // Columns that cannot move (gutters wider than the margin box) offer no guide.
            [a, b]
                if ((sp.x - a.x1).abs() <= tol || (sp.x - b.x0).abs() <= tol)
                    && pg.columns.with_gutter_moved(m.width(), gi, a.x1 - m.x0).is_some() =>
            {
                Some((r, pi, gi, a.x1))
            }
            _ => None,
        })
    })?;
    // A ruler guide there takes the press first (see `guide_drag`).
    let ruler_guide = !app.ui.guides_locked && guide_at(app, xf, p).is_some();
    (!ruler_guide && !over_item(app, xf, p)).then_some(hit)
}

/// Whether spread `r`'s x axis runs across the screen (the spread view is not turned a quarter).
fn spread_x_across(app: &DesignApp, r: SpreadRef) -> bool {
    app.session.active().is_none_or(|st| {
        let [a, b, ..] = CanvasLayout::new(&st.doc, st.editing_parents).xf(r).as_coeffs();
        a.abs() >= b.abs()
    })
}

/// Canvas x `x` moved to the nearest tick mark of the horizontal ruler (its finest increment).
fn ruler_tick_x(app: &DesignApp, doc: &Document, layout: &CanvasLayout, zoom: f64, x: f64) -> Option<f64> {
    let pieces = ruler_pieces(app, doc, layout, doc.zero_point());
    let &(_, _, o) = pieces.iter().find(|(from, to, _)| *from <= x && x < *to).or(pieces.first())?;
    let (major, sub) = doc.settings.horizontal_units.ruler_ticks(zoom);
    let step = major / f64::from(sub.max(1));
    (step.is_finite() && step > 0.0).then(|| o.x + ((x - o.x) / step).round() * step).filter(|v| v.is_finite())
}

/// Where a dragged gutter's left edge goes for spread x `want`: Shift snaps it to the ruler's tick
/// marks; otherwise its edges snap to the document grid, guides and objects as those snaps are
/// set. Measured against the document as it was when the drag began.
fn snap_gutter(app: &DesignApp, g: &ColumnDrag, want: f64, shift: bool, zoom: f64) -> f64 {
    let Some(st) = app.session.active() else { return want };
    let doc = st.interaction.as_ref().map_or_else(|| st.doc.clone(), |i| i.doc.clone());
    let layout = CanvasLayout::new(&doc, st.editing_parents);
    let Some(slot) = layout.slots.iter().find(|s| s.spread == g.spread) else { return want };
    let Some(pg) = doc.spread(g.spread).and_then(|sp| sp.pages.get(g.page)) else { return want };
    let m = pg.margin_rect();
    if shift {
        if !spread_x_across(app, g.spread) {
            return want;
        }
        let c = slot.to_canvas(Point::new(want, m.y0));
        return ruler_tick_x(app, &doc, &layout, zoom, c.x).map_or(want, |x| slot.to_spread(Point::new(x, c.y)).x);
    }
    let cols = pg.column_rects();
    let width = match (cols.get(g.gutter), cols.get(g.gutter + 1)) {
        (Some(a), Some(b)) => (b.x0 - a.x1).max(0.0),
        _ => 0.0,
    };
    let cx = designcraft_tools::ToolContext {
        doc: &doc,
        selection: &st.selection,
        cache: &app.session.cache,
        layout: &layout,
        zoom: zoom.max(1e-6),
        layer: st.active_layer,
        // A line has no size or spacing to match.
        snap: designcraft_tools::SnapView { smart_dimensions: false, smart_spacing: false, ..app.ui.snap_view() },
        unit: app.ui.units,
        stroke_in_front: false,
    };
    let rect = DRect::new(want, m.y0, want + width, m.y1);
    let s = designcraft_tools::snap::snap(
        &cx,
        designcraft_tools::SnapRequest {
            spread: g.spread,
            gesture: designcraft_tools::Gesture::Move,
            rect,
            x_edges: [true, false, true],
            y_edges: [false; 3],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: Point::new(want, m.center().y),
        },
    );
    let x = want + s.delta.x;
    if x.is_finite() { x } else { want }
}

/// Column guides: with a Selection tool, drag an interior column guide (not over an object) to
/// move its gutter, both guides together; Escape puts it back. One undo step per drag. Returns
/// true while it owns the pointer.
fn column_guide_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
    let id = egui::Id::unique((COLUMN_DRAG, app.pane));
    let (pressed, released, origin, latest, shift, escape) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            // A release this frame, or one missed while something else had the pointer.
            i.pointer.primary_released() || !i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
            i.modifiers.shift,
            i.key_pressed(egui::Key::Escape),
        )
    });
    let mut drag: Option<ColumnDrag> = ui.data(|d| d.get_temp(id));
    if drag.is_none()
        && pressed
        && resp.hovered()
        && let Some(o) = origin.filter(|o| rect.contains(*o))
        && let Some((spread, page, gutter, left)) = column_guide_at(app, xf, o)
        // Another gesture or a dialog's preview owns the document until it ends.
        && let Some((doc, slot_x)) = app
            .session
            .active()
            .filter(|st| st.interaction.is_none())
            .map(|st| (st.uid, CanvasLayout::new(&st.doc, st.editing_parents).to_spread(spread, xf.to_canvas(o)).x))
        && app.session.begin_interaction("Move Column Guide").is_ok()
    {
        drag = Some(ColumnDrag { doc, spread, page, gutter, grab: slot_x - left, last: None });
    }
    let Some(mut g) = drag else { return false };
    // Another document became active: the drag ends in its own, as it began.
    if app.session.active().is_none_or(|st| st.uid != g.doc) {
        app.session.cancel_interaction_of(g.doc);
        ui.data_mut(|d| d.remove::<ColumnDrag>(id));
        return false;
    }
    if escape {
        app.session.cancel_interaction();
        ui.data_mut(|d| d.remove::<ColumnDrag>(id));
        return true;
    }
    if let Some(p) = latest
        && let Some(x) = app.session.active().map(|st| CanvasLayout::new(&st.doc, st.editing_parents).to_spread(g.spread, xf.to_canvas(p)).x)
        && x.is_finite()
    {
        let want = snap_gutter(app, &g, x - g.grab, shift, xf.zoom);
        if g.last != Some(want) && (g.last.is_some() || (p - origin.unwrap_or(p)).length() > 0.0) {
            let params = json!({"spread": g.spread, "page": g.page, "gutter": g.gutter, "position": want});
            if let Err(e) = app.session.preview("layout.moveColumnGuide", params) {
                app.status(e.to_string());
            }
            g.last = Some(want);
        }
    }
    if released {
        ui.data_mut(|d| d.remove::<ColumnDrag>(id));
        if let Err(e) = app.session.commit_interaction() {
            app.status(e.to_string());
        }
        app.after_engine();
        return true;
    }
    ui.data_mut(|d| d.insert_temp(id, g));
    true
}

/// Presses in a row on the canvas: the last press's count, and where and when it was released
/// (`at` is −∞ while the button is down).
#[derive(Clone, Copy, Debug)]
struct Clicks {
    at: f64,
    pos: Pos2,
    n: u8,
}

/// The count of a press at `pos` at time `now`: one more than the last press when it comes within
/// the double-click delay of the last release and near where that release was, else 1.
fn press_count(last: Option<Clicks>, pos: Pos2, now: f64, delay: f64, dist: f32) -> u8 {
    match last {
        Some(c) if now - c.at <= delay && (pos - c.pos).length() <= dist => c.n.saturating_add(1),
        _ => 1,
    }
}

/// The record a release at `pos` leaves: a release away from its press ended a drag, which is
/// not a click, so the next press counts from one.
fn release_clicks(press: Clicks, pos: Pos2, now: f64, dist: f32) -> Clicks {
    let n = if (pos - press.pos).length() <= dist { press.n } else { 0 };
    Clicks { at: now, pos, n }
}

#[cfg(test)]
#[path = "parent_canvas_tests.rs"]
mod parent_canvas_tests;

#[cfg(test)]
#[path = "guide_selection_tests.rs"]
mod guide_selection_tests;

#[cfg(test)]
mod tests {
    use designcraft_geom::Unit;
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    #[test]
    fn a_press_after_a_drag_is_not_a_double_click() {
        let (delay, dist) = (0.3, 6.0);
        let press = |last, x: f32, now| press_count(last, pos2(x, 0.0), now, delay, dist);
        // Click, click: a double click.
        let n = press(None, 0.0, 0.0);
        let up = release_clicks(Clicks { at: f64::NEG_INFINITY, pos: pos2(0.0, 0.0), n }, pos2(0.0, 0.0), 0.1, dist);
        assert_eq!(press(Some(up), 1.0, 0.2), 2);
        // A drag from 0 to 100, then a fast press where it started or where it ended: a single click.
        let up = release_clicks(Clicks { at: f64::NEG_INFINITY, pos: pos2(0.0, 0.0), n: 1 }, pos2(100.0, 0.0), 0.1, dist);
        assert_eq!(press(Some(up), 0.0, 0.2), 1);
        assert_eq!(press(Some(up), 100.0, 0.2), 1);
        // Too slow: a single click.
        let up = release_clicks(Clicks { at: f64::NEG_INFINITY, pos: pos2(0.0, 0.0), n: 1 }, pos2(0.0, 0.0), 0.1, dist);
        assert_eq!(press(Some(up), 0.0, 1.0), 1);
    }

    /// While a page's render is on its way the page shows paper, not the black of its shadow.
    #[test]
    fn pages_show_paper_under_their_render() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({"pages": 3, "facingPages": true})).unwrap();
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| show(&mut app, ui));
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        let st = app.session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, false);
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        // The paper swatch on screen (white, as the colour settings show it).
        let [r, g, b, _] = st.doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).unwrap().to_rgba8(1.0);
        let paper: Vec<Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(s) if s.fill == Color32::from_rgb(r, g, b) => Some(s.rect),
                _ => None,
            })
            .collect();
        // Each page on its own (a spread's pages may differ in size).
        assert_eq!(paper.len(), st.doc.page_count());
        for abs in 0..st.doc.page_count() {
            let want = xf.rect(layout.page_rect(&st.doc, abs).unwrap());
            assert!(paper.iter().any(|r| (r.min - want.min).length() < 0.01 && (r.max - want.max).length() < 0.01), "page {abs}");
        }
    }

    /// Bleed and Slug are preview modes: like Preview they hide the guides, and what the page
    /// shows widens to the bleed or the slug box. The slug guide is drawn in Normal, in its own
    /// colour, beside the bleed guide.
    #[test]
    fn bleed_and_slug_are_preview_modes_that_show_their_area() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({"bleed": 9, "slug": 18})).unwrap();
        let strokes = |app: &mut DesignApp, want: [u8; 3]| -> usize {
            let ctx = egui::Context::default();
            let mut shapes = Vec::new();
            for _ in 0..2 {
                let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| show(app, ui));
                out.textures_delta.clear();
                shapes = out.shapes;
            }
            let want = c32(want);
            shapes.iter().filter(|c| matches!(&c.shape, egui::Shape::Rect(s) if s.stroke.color == want)).count()
        };
        let s = app.session.active().unwrap().doc.settings.clone();
        // Normal draws every guide, the slug's among them.
        app.ui.screen_mode = ScreenMode::Normal;
        assert!(strokes(&mut app, s.margin_color) > 0, "Normal draws the margin guide");
        assert!(strokes(&mut app, s.bleed_color) > 0, "Normal draws the bleed guide");
        assert!(strokes(&mut app, s.slug_color) > 0, "Normal draws the slug guide");
        // Every preview mode hides them.
        for mode in [ScreenMode::Preview, ScreenMode::Bleed, ScreenMode::Slug, ScreenMode::Presentation] {
            app.ui.screen_mode = mode;
            assert_eq!(strokes(&mut app, s.margin_color), 0, "{mode:?} hides the margin guide");
            assert_eq!(strokes(&mut app, s.slug_color), 0, "{mode:?} hides the slug guide");
        }
    }

    /// Each preview mode's shown area: Preview stops at the trim, Bleed reaches the bleed box and
    /// Slug the slug box. A slug narrower than the bleed never pulls the edge back in.
    #[test]
    fn shown_area_widens_to_the_bleed_and_slug_boxes() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({"bleed": 9, "slug": 18})).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        let layout = CanvasLayout::new(&doc, false);
        let xf = Xf::new(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0)), &View::default());
        let slot = &layout.slots[0];
        let area = |app: &mut DesignApp, mode| {
            app.ui.screen_mode = mode;
            shown_area(app, &doc, &xf, slot)
        };
        let trim = xf.rect(slot.bounds);
        assert_eq!(area(&mut app, ScreenMode::Preview), trim, "Preview stops at the trim");
        assert_eq!(area(&mut app, ScreenMode::Presentation), trim, "Presentation stops at the trim");
        let bleed = area(&mut app, ScreenMode::Bleed);
        let slug = area(&mut app, ScreenMode::Slug);
        assert!(bleed.contains_rect(trim) && bleed != trim, "Bleed reaches past the trim");
        assert!(slug.contains_rect(bleed) && slug != bleed, "Slug reaches past the bleed");
        // 9 pt of bleed and 18 pt of slug at this zoom, on every edge.
        let grew = |r: Rect, pt: f64| {
            let d = (pt * xf.zoom) as f32;
            (r.min - trim.min + vec2(d, d)).length() < 0.01 && (r.max - trim.max - vec2(d, d)).length() < 0.01
        };
        assert!(grew(bleed, 9.0), "Bleed grows by the bleed: {bleed:?}");
        assert!(grew(slug, 18.0), "Slug grows by the slug: {slug:?}");
        // A slug inside the bleed: Slug still shows every printing element, so the bleed too.
        app.run("file.new", serde_json::json!({"bleed": 18, "slug": 9})).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        app.ui.screen_mode = ScreenMode::Slug;
        assert!(grew(shown_area(&app, &doc, &xf, slot), 18.0), "the wider of the two boxes wins");
        // No bleed and no slug: nothing to widen to.
        app.run("file.new", serde_json::json!({})).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        for mode in [ScreenMode::Bleed, ScreenMode::Slug] {
            app.ui.screen_mode = mode;
            assert_eq!(shown_area(&app, &doc, &xf, slot), trim, "{mode:?} without a bleed or slug");
        }
    }

    /// Build the four-fps app harness used by canvas tests.
    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        harness_with_step(app, 1.0 / 4.0)
    }

    /// A window whose frames last `step_dt` seconds. Wheel motion is tested at a realistic frame
    /// rate: egui ends a scroll action after 150 ms without wheel events, so quarter-second
    /// frames would split every motion into separate actions.
    fn harness_with_step(app: DesignApp, step_dt: f32) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(step_dt).build_ui_state(
            |ui, app: &mut DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        h
    }

    #[test]
    fn display_option_change_drops_the_render_in_flight() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let shown = crate::Shown { doc: (1, 2, 3, false), origin: Point::new(0.0, 0.0), zoom: 1.0, size: (10.0, 10.0), editing_parents: false };
        invalidate_on_display_change(&mut app);
        app.canvas.shown = Some(shown);
        app.canvas.pending = Some((7, shown));
        // Nothing changed: the texture and the request stay.
        invalidate_on_display_change(&mut app);
        assert!(app.canvas.shown.is_some() && app.canvas.pending.is_some());
        // A display option changed while the render ran: its result must not be accepted.
        app.ui.display_quality = designcraft_render::DisplayQuality::Fast;
        invalidate_on_display_change(&mut app);
        assert!(app.canvas.shown.is_none() && app.canvas.pending.is_none() && app.canvas.pending_doc.is_none());
        // The document grid is drawn into the texture too.
        app.canvas.shown = Some(shown);
        app.ui.document_grid = !app.ui.document_grid;
        invalidate_on_display_change(&mut app);
        assert!(app.canvas.shown.is_none());
    }

    /// A primary-button drag from `a` to `b` over a few frames.
    fn drag(h: &mut Harness<'_, DesignApp>, a: Pos2, b: Pos2) {
        h.hover_at(a);
        h.step();
        h.event(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        h.step();
        for k in 1..=4 {
            h.hover_at(a + (b - a) * (k as f32 / 4.0));
            h.step();
        }
        h.event(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
        h.run_steps(3);
    }

    #[test]
    fn the_page_tool_draws_liquid_guides_from_the_rulers() {
        // Chris Cicchino's Page Tool and Liquid Layout video (11:00): with the Page tool, a guide
        // dragged from the ruler is a liquid guide (dashed).
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.session.set_tool("page");
        let mut h = harness(app);
        let r = h.state().canvas_rect.unwrap();
        let guides = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.spreads[0].pages[0].guides.clone();
        drag(&mut h, pos2(r.center().x, r.min.y - 8.0), r.center());
        let g = guides(&h);
        assert_eq!(g.len(), 1, "{g:?}");
        assert!(g[0].liquid && g[0].orientation == designcraft_doc::Orientation::Horizontal);
        // With the Selection tool it's a ruler guide.
        h.state_mut().session.set_tool("selection");
        drag(&mut h, pos2(r.center().x, r.min.y - 8.0), r.center() + vec2(0.0, 40.0));
        let g = guides(&h);
        assert_eq!(g.len(), 2);
        assert!(!g[1].liquid);
    }

    /// Dragging a column guide with the Selection tool moves its gutter (one undo step); with
    /// column guides locked the same drag leaves the columns alone.
    #[test]
    fn the_selection_tool_drags_a_column_guide() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        // Two columns across the 540 pt margin box: the gutter's left edge is at x = 300.
        app.run("layout.marginsAndColumns", json!({"parent": "A", "columns": 2, "gutter": 12})).unwrap();
        let mut h = harness(app);
        let page = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.spreads[0].pages[0].clone();
        let screen = |h: &Harness<'_, DesignApp>, x: f64| {
            let app = h.state();
            let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
            let layout = CanvasLayout::new(&app.session.active().unwrap().doc, false);
            let slot = layout.slots.iter().find(|s| s.spread == SpreadRef::Doc(0)).unwrap();
            (xf.to_screen(slot.xf * Point::new(x, 400.0)), xf.zoom as f32)
        };
        let (at, zoom) = screen(&h, 300.0);
        drag(&mut h, at, at + vec2(72.0 * zoom, 0.0));
        assert_eq!(page(&h).columns.positions, None, "column guides are locked in a new document");
        h.hover_at(at);
        h.run_steps(2);
        assert_ne!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);

        h.state_mut().run("view.lockColumnGuides", json!({"on": false})).unwrap();
        h.run_steps(2);
        let (at, zoom) = screen(&h, 300.0);
        drag(&mut h, at, at + vec2(72.0 * zoom, 0.0));
        let pg = page(&h);
        let cols = pg.column_rects();
        assert!((cols[0].x1 - 372.0).abs() < 1.0 && (cols[1].x0 - cols[0].x1 - 12.0).abs() < 1e-9, "{cols:?}");
        assert!(pg.local_margins, "the dragged page's columns are its own");
        // Over the moved guide: a resize cursor.
        h.hover_at(screen(&h, 372.0).0);
        h.run_steps(2);
        assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(page(&h).columns.positions, None, "one undo step");

        // Escape during the drag puts the gutter back.
        h.hover_at(at);
        h.step();
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        h.step();
        h.hover_at(at + vec2(40.0, 0.0));
        h.step();
        assert!(page(&h).columns.positions.is_some(), "the drag previews");
        h.event(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
        h.step();
        h.event(egui::Event::PointerButton {
            pos: at + vec2(40.0, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(3);
        assert_eq!(page(&h).columns.positions, None);
        assert!(!page(&h).local_margins);

        // Space pressed during the drag and the button released under it: the drag ends with the
        // release and the gutter stops following the pointer.
        let key = |pressed| egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        h.hover_at(at);
        h.step();
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
        h.step();
        h.hover_at(at + vec2(40.0, 0.0));
        h.step();
        h.event(key(true));
        h.step();
        h.event(egui::Event::PointerButton {
            pos: at + vec2(40.0, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.step();
        h.event(key(false));
        h.run_steps(2);
        let moved = page(&h).columns.positions;
        assert!(moved.is_some());
        h.hover_at(at + vec2(120.0, 0.0));
        h.run_steps(3);
        assert_eq!(page(&h).columns.positions, moved, "the gutter stays where the drag ended");
        assert!(h.state().session.active().unwrap().interaction.is_none());
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(page(&h).columns.positions, None, "the drag was one undo step");
    }

    /// A column guide is not picked where the screen mode hides guides, where an object on the
    /// parent being edited lies over it, or where its columns cannot move.
    #[test]
    fn column_guides_are_picked_only_where_a_drag_can_move_them() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 1})).unwrap();
        app.run("layout.marginsAndColumns", json!({"parent": "A", "columns": 2, "gutter": 12})).unwrap();
        app.run("view.lockColumnGuides", json!({"on": false})).unwrap();
        let mut h = harness(app);
        let hit = |h: &Harness<'_, DesignApp>, r: SpreadRef, x: f64| {
            let app = h.state();
            let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
            let st = app.session.active().unwrap();
            let layout = CanvasLayout::new(&st.doc, st.editing_parents);
            let slot = layout.slots.iter().find(|s| s.spread == r).unwrap();
            column_guide_at(app, &xf, xf.to_screen(slot.xf * Point::new(x, 400.0)))
        };
        assert!(hit(&h, SpreadRef::Doc(0), 300.0).is_some());
        h.state_mut().ui.screen_mode = ScreenMode::Preview;
        assert!(hit(&h, SpreadRef::Doc(0), 300.0).is_none(), "Preview hides the guides");
        h.state_mut().ui.screen_mode = ScreenMode::Normal;

        // Editing parent A: its right page (the one page 0 follows) and a frame over its gutter.
        h.state_mut().run("layout.parents.edit", json!({"on": true, "parent": "A-Parent"})).unwrap();
        h.run_steps(2);
        let parent = SpreadRef::Parent(0);
        let x = h.state().session.active().unwrap().doc.parents[0].pages[1].column_rects()[0].x1;
        assert!(hit(&h, parent, x).is_some());
        h.state_mut()
            .run("frame.create", json!({"spread": {"kind": "parent", "index": 0}, "rect": [x - 20.0, 300.0, x + 20.0, 500.0], "content": "text"}))
            .unwrap();
        h.run_steps(2);
        assert!(hit(&h, parent, x).is_none(), "the parent's frame takes the press");
        h.state_mut().run("layout.parents.edit", json!({"on": false})).unwrap();
        h.run_steps(2);

        // Gutters wider than the margin box: the even split cannot be moved.
        h.state_mut().run("layout.marginsAndColumns", json!({"pages": [0], "columns": 2, "gutter": 1000})).unwrap();
        h.run_steps(2);
        let x = h.state().session.active().unwrap().doc.spreads[0].pages[0].column_rects()[0].x1;
        assert!(hit(&h, SpreadRef::Doc(0), x).is_none());
    }

    fn right_click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
        h.hover_at(p);
        h.step();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() });
        }
        h.run_steps(3);
    }

    fn new_document() -> Harness<'static, DesignApp> {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        harness(app)
    }

    /// [`new_document`] at 60 frames a second, the cadence a wheel motion really arrives at.
    fn new_document_60fps() -> Harness<'static, DesignApp> {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        harness_with_step(app, 1.0 / 60.0)
    }

    fn held(modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::ModifiersChanged(modifiers)
    }

    /// Construct a point-unit vertical mouse-wheel event with an unknown source.
    fn wheel(phase: egui::TouchPhase, up: f32, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, up), phase, modifiers, source: egui::MouseWheelSource::Unknown }
    }

    // Plays `events` with the pointer at `at`, a frame each, and lets the motion settle.
    fn play(h: &mut Harness<'_, DesignApp>, at: Pos2, events: &[egui::Event]) -> View {
        h.hover_at(at);
        for event in events {
            h.event(event.clone());
            h.step();
        }
        h.run_steps(30);
        *h.state().view().unwrap()
    }

    /// Alt + wheel scrolled the view like the plain wheel; it zooms at the pointer (#470).
    #[test]
    fn alt_wheel_zooms_at_the_pointer() {
        use egui::{Modifiers, TouchPhase::Move};
        let mut h = new_document();
        let rect = h.state().canvas_rect.unwrap();
        let at = rect.center() + vec2(150.0, 90.0);
        let before = *h.state().view().unwrap();
        let under = Xf::new(rect, &before).to_canvas(at);
        let zoomed = play(&mut h, at, &[held(Modifiers::ALT), wheel(Move, 100.0, Modifiers::ALT)]);
        assert!(zoomed.zoom > before.zoom * 1.2, "wheel up zooms in: {} -> {}", before.zoom, zoomed.zoom);
        let still = Xf::new(rect, &zoomed).to_canvas(at);
        assert!((still.x - under.x).abs() < 0.01 && (still.y - under.y).abs() < 0.01, "the point under the pointer stays: {under:?} -> {still:?}");
        let back = play(&mut h, at, &[held(Modifiers::ALT), wheel(Move, -100.0, Modifiers::ALT)]);
        assert!((back.zoom - before.zoom).abs() < 1e-6, "wheel down zooms out: {} -> {}", zoomed.zoom, back.zoom);
        // The plain wheel still scrolls.
        let scrolled = play(&mut h, at, &[held(Modifiers::NONE), wheel(Move, 100.0, Modifiers::NONE)]);
        assert!(scrolled.zoom == back.zoom && scrolled.origin.y < back.origin.y, "{back:?} -> {scrolled:?}");
    }

    /// Alt zooms exactly as Ctrl/Cmd does, also when the key is let go while the wheel's motion is
    /// still arriving: the rest of it zooms too and does not scroll the page.
    #[test]
    fn alt_wheel_zooms_like_ctrl_wheel_to_the_end_of_the_motion() {
        use egui::{
            Modifiers,
            TouchPhase::{End, Move, Start},
        };
        const NONE: Modifiers = Modifiers::NONE;
        // Each motion in steps; the views are compared after every step.
        let motions: [fn(Modifiers) -> Vec<Vec<egui::Event>>; 4] = [
            |key| vec![vec![held(key), wheel(Move, 100.0, key)]],
            // A wheel turned fast, the key let go a frame later.
            |key| vec![vec![held(key), wheel(Move, 400.0, key), held(NONE)]],
            // A trackpad swipe that coasts on after the key is let go; once it has ended, the wheel scrolls.
            |key| {
                vec![
                    vec![held(key), wheel(Start, 0.0, key), wheel(Move, 50.0, key), held(NONE), wheel(Move, 200.0, NONE)],
                    vec![wheel(End, 0.0, NONE), wheel(Move, 100.0, NONE)],
                ]
            },
            |key| vec![vec![held(key | Modifiers::SHIFT), wheel(Move, 100.0, key | Modifiers::SHIFT)]],
        ];
        for (i, motion) in motions.iter().enumerate() {
            let (mut with_alt, mut with_command) = (new_document_60fps(), new_document_60fps());
            let at = with_alt.state().canvas_rect.unwrap().center() + vec2(150.0, 90.0);
            let start = *with_alt.state().view().unwrap();
            for (alt, command) in motion(Modifiers::ALT).iter().zip(motion(Modifiers::COMMAND)) {
                let (alt, command) = (play(&mut with_alt, at, alt), play(&mut with_command, at, &command));
                assert!(command.zoom > start.zoom, "motion {i} zooms");
                assert_eq!(alt, command, "motion {i}");
            }
        }
    }

    fn units(h: &Harness<'_, DesignApp>) -> (Unit, Unit) {
        let s = &h.state().session.doc().unwrap().doc.settings;
        (s.horizontal_units, s.vertical_units)
    }

    fn zoom_app(tool: &str) -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.select_tool(tool);
        app.ui.rulers = false;
        app
    }

    /// Run one app frame with the supplied modifiers, install fonts, draw the canvas when ready and return the cursor icon.
    fn canvas_frame(app: &mut DesignApp, ctx: &egui::Context, m: egui::Modifiers, mut events: Vec<egui::Event>) -> egui::CursorIcon {
        events.insert(0, egui::Event::ModifiersChanged(m));
        let mut output = ctx.run_ui(
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, ..Default::default() },
            |ui| {
                app.logic(ui.ctx());
                // Fonts are installed by the first real UI pass, just as in the full application.
                app.ui_setup(ui.ctx());
                if app.fonts_ready {
                    show(app, ui);
                }
            },
        );
        output.textures_delta.clear();
        output.platform_output.cursor_icon
    }

    /// The cursor icon the canvas sets for `c` on this platform (bitmap and painted cursors
    /// report their own icon).
    fn shown(c: Cursor) -> egui::CursorIcon {
        crate::cursors::show(&egui::Context::default(), &mut crate::cursors::Images::default(), c, Pos2::ZERO)
    }

    fn space_key(pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers }
    }

    fn press(pos: Pos2, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers }
    }

    fn hover(app: &mut DesignApp, ctx: &egui::Context, p: Pos2) {
        for _ in 0..3 {
            canvas_frame(app, ctx, egui::Modifiers::NONE, vec![egui::Event::PointerMoved(p)]);
        }
    }

    const CMD: egui::Modifiers = egui::Modifiers { command: true, mac_cmd: true, ..egui::Modifiers::NONE };

    #[test]
    fn spring_zoom_click_is_anchored_view_only_and_release_restores_previous_tool() {
        for tool in ["rectangleFrame", "directSelection", "type", "hand", "zoom"] {
            let mut app = zoom_app(tool);
            if tool == "rectangleFrame" {
                let frame = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"Hello"})).unwrap();
                app.run("text.select", json!({"story":frame["story"],"anchor":5})).unwrap();
            }
            let ctx = egui::Context::default();
            let p = pos2(300.0, 250.0);
            hover(&mut app, &ctx, p);
            let before = *app.view().unwrap();
            let doc = app.session.doc().unwrap().doc.clone();
            let selection = app.session.doc().unwrap().selection.clone();
            let history = app.run("document.history", json!({})).unwrap();
            let cursor = canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD)]);
            assert_eq!(cursor, shown(Cursor::ZoomIn), "{tool}");
            canvas_frame(&mut app, &ctx, CMD, vec![press(p, true, CMD)]);
            canvas_frame(&mut app, &ctx, CMD, vec![press(p, false, CMD)]);
            let after = *app.view().unwrap();
            assert_eq!(after.zoom, before.zoom * 2.0, "{tool}");
            let anchor = Xf::new(app.canvas_rect.unwrap(), &before).to_canvas(p);
            assert!((Xf::new(app.canvas_rect.unwrap(), &after).to_canvas(p) - anchor).hypot() < 1e-9);
            let cursor = canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![space_key(false, egui::Modifiers::NONE)]);
            assert_eq!(app.session.tool_id(), tool);
            let expected = app.session.cursor(anchor, Mods::default(), app.view_info());
            assert_eq!(cursor, shown(expected), "{tool}");
            assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
            assert_eq!(selection, app.session.doc().unwrap().selection);
            assert_eq!(history, app.run("document.history", json!({})).unwrap());
        }
    }

    #[test]
    fn spring_zoom_alt_clicks_out_and_modifier_release_switches_back_to_hand() {
        let mut app = zoom_app("rectangleFrame");
        let ctx = egui::Context::default();
        let p = pos2(300.0, 250.0);
        hover(&mut app, &ctx, p);
        let z = app.view().unwrap().zoom;
        let alt = egui::Modifiers { alt: true, ..CMD };
        assert_eq!(canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD)]), shown(Cursor::ZoomIn));
        assert_eq!(canvas_frame(&mut app, &ctx, alt, vec![]), shown(Cursor::ZoomOut));
        canvas_frame(&mut app, &ctx, alt, vec![press(p, true, alt)]);
        canvas_frame(&mut app, &ctx, alt, vec![press(p, false, alt)]);
        assert_eq!(app.view().unwrap().zoom, z * 0.5);
        assert_eq!(canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![]), shown(Cursor::Hand));
        let before = *app.view().unwrap();
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p, true, egui::Modifiers::NONE)]);
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![egui::Event::PointerMoved(p + vec2(80.0, 0.0))]);
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p + vec2(80.0, 0.0), false, egui::Modifiers::NONE)]);
        assert_eq!(app.view().unwrap().zoom, before.zoom);
        assert!(app.view().unwrap().origin.x < before.origin.x);
        assert_eq!(app.session.tool_id(), "rectangleFrame");
    }

    #[test]
    fn spring_zoom_scrubs_and_owns_pointer_release_after_shortcut_release() {
        let mut app = zoom_app("rectangleFrame");
        let ctx = egui::Context::default();
        let p = pos2(300.0, 250.0);
        hover(&mut app, &ctx, p);
        let z = app.view().unwrap().zoom;
        let doc = app.session.doc().unwrap().doc.clone();
        // Ctrl is the platform command modifier on Windows/Linux, using the same input path.
        let cmd = egui::Modifiers { command: true, ctrl: true, ..egui::Modifiers::NONE };
        canvas_frame(&mut app, &ctx, cmd, vec![space_key(true, cmd), press(p, true, cmd)]);
        let end = p + vec2(150.0, 0.0);
        canvas_frame(&mut app, &ctx, cmd, vec![egui::Event::PointerMoved(end)]);
        assert!((app.view().unwrap().zoom - z * std::f64::consts::E).abs() < 1e-9);
        let scrubbed = app.view().unwrap().zoom;
        assert_eq!(canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![space_key(false, egui::Modifiers::NONE)]), shown(Cursor::Crosshair));
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(end, false, egui::Modifiers::NONE)]);
        assert_eq!(app.view().unwrap().zoom, scrubbed, "no extra click zoom at the end of a scrub");
        assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
        // The original tool still draws after the temporary gesture.
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p, true, egui::Modifiers::NONE)]);
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![egui::Event::PointerMoved(p + vec2(50.0, 50.0))]);
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p + vec2(50.0, 50.0), false, egui::Modifiers::NONE)]);
        assert!(!std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
        assert_eq!(app.session.tool_id(), "rectangleFrame");
    }

    #[test]
    fn spring_zoom_is_suppressed_while_typing_in_a_text_frame() {
        let mut app = zoom_app("type");
        let frame = app.run("frame.create", json!({"rect":[36,36,300,200],"content":"text","text":"Hello"})).unwrap();
        app.run("text.select", json!({"story":frame["story"],"anchor":5})).unwrap();
        assert!(app.session.wants_text());
        let ctx = egui::Context::default();
        let p = pos2(300.0, 250.0);
        hover(&mut app, &ctx, p);
        let before = *app.view().unwrap();
        let alt = egui::Modifiers { alt: true, ..CMD };
        for m in [CMD, alt] {
            app.run("text.select", json!({"story":frame["story"],"anchor":5})).unwrap();
            assert!(app.session.wants_text());
            assert!(!matches!(canvas_frame(&mut app, &ctx, m, vec![space_key(true, m)]), egui::CursorIcon::ZoomIn | egui::CursorIcon::ZoomOut));
            canvas_frame(&mut app, &ctx, m, vec![press(p, true, m)]);
            canvas_frame(&mut app, &ctx, m, vec![press(p, false, m)]);
            assert_eq!(app.view().unwrap().zoom, before.zoom);
            assert_eq!(app.view().unwrap().origin, before.origin);
            canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![space_key(false, egui::Modifiers::NONE)]);
        }
    }

    #[test]
    fn spring_zoom_is_suppressed_while_a_text_field_has_focus() {
        let mut app = zoom_app("rectangleFrame");
        let ctx = egui::Context::default();
        let mut text = String::new();
        let p = pos2(300.0, 250.0);
        hover(&mut app, &ctx, p);
        let before = *app.view().unwrap();
        for m in [egui::Modifiers::NONE, CMD, egui::Modifiers { alt: true, ..CMD }] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    events: vec![egui::Event::ModifiersChanged(m), space_key(true, m), egui::Event::Text("x".into()), egui::Event::PointerMoved(p)],
                    ..Default::default()
                },
                |ui| {
                    ui.text_edit_singleline(&mut text).request_focus();
                    show(&mut app, ui);
                },
            );
            output.textures_delta.clear();
            assert!(ctx.text_edit_focused());
            assert!(!matches!(output.platform_output.cursor_icon, egui::CursorIcon::ZoomIn | egui::CursorIcon::ZoomOut | egui::CursorIcon::Grab));
            assert_eq!(app.view().unwrap().zoom, before.zoom);
            assert_eq!(app.session.tool_id(), "rectangleFrame");
        }
        assert!(!text.is_empty(), "typing still reaches the field");
    }

    #[test]
    fn spring_zoom_does_not_interrupt_an_existing_tool_drag() {
        let mut app = zoom_app("rectangleFrame");
        let ctx = egui::Context::default();
        let p = pos2(300.0, 250.0);
        hover(&mut app, &ctx, p);
        let before = *app.view().unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        canvas_frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(p, true, egui::Modifiers::NONE)]);
        let end = p + vec2(100.0, 100.0);
        canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD), egui::Event::PointerMoved(end)]);
        canvas_frame(&mut app, &ctx, CMD, vec![press(end, false, CMD)]);
        assert_eq!(app.view().unwrap().zoom, before.zoom);
        assert_eq!(app.view().unwrap().origin, before.origin);
        assert!(!std::sync::Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
    }

    #[test]
    fn spring_zoom_does_not_zoom_on_the_rulers() {
        let mut app = zoom_app("rectangleFrame");
        app.ui.rulers = true;
        let ctx = egui::Context::default();
        hover(&mut app, &ctx, pos2(300.0, 250.0));
        let rect = app.canvas_rect.unwrap();
        let p = pos2(rect.min.x - RULER / 2.0, rect.center().y);
        hover(&mut app, &ctx, p);
        let before = *app.view().unwrap();
        canvas_frame(&mut app, &ctx, CMD, vec![space_key(true, CMD)]);
        canvas_frame(&mut app, &ctx, CMD, vec![press(p, true, CMD)]);
        canvas_frame(&mut app, &ctx, CMD, vec![press(p, false, CMD)]);
        assert_eq!(app.view().unwrap().zoom, before.zoom);
        assert_eq!(app.view().unwrap().origin, before.origin);
    }

    #[test]
    fn parent_guides_on_a_page_are_not_picked_there() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        // 100 pt into A's right page, which page 1 (a right page at x = 0) shows.
        let parent_guide = json!({"spread": {"kind": "parent", "index": 0}, "page": 1, "orientation": "vertical", "position": 712});
        app.run("guide.add", parent_guide).unwrap();
        let mut h = harness(app);
        let at = |app: &DesignApp| {
            let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
            let layout = CanvasLayout::new(&app.session.active().unwrap().doc, false);
            let slot = layout.slots.iter().find(|s| s.spread == SpreadRef::Doc(0)).unwrap();
            guide_at(app, &xf, xf.to_screen(slot.xf * Point::new(100.0, 300.0)))
        };
        assert_eq!(at(h.state()), None, "the parent's guide is edited on the parent");
        h.state_mut().run("guide.add", json!({"orientation": "vertical", "position": 100})).unwrap();
        h.run_steps(2);
        assert!(matches!(at(h.state()), Some((SpreadRef::Doc(0), 0, 0, _))), "the page's own guide is");
    }

    #[test]
    fn mouse_double_clicking_a_frame_handle_fits_with_one_undo_step() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = designcraft_doc::ItemId(
            app.run("frame.create", json!({"rect": [100, 100, 400, 300], "content": "text", "text": "Fit this text"})).unwrap()["id"]
                .as_u64()
                .unwrap(),
        );
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        let original = app.session.doc().unwrap().doc.item(id).unwrap().bounds();
        // 60 frames a second: the default 0.25 s step puts two clicks past egui's double-click delay.
        let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        let app = h.state();
        let st = app.session.doc().unwrap();
        let layout = CanvasLayout::new(&st.doc, false);
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        let p = xf.to_screen(layout.xf(SpreadRef::Doc(0)) * Point::new(original.x1, original.y1));
        h.hover_at(p);
        h.step();
        // Separate frames model the actual mouse, including egui's release-time double click.
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
                h.step();
            }
        }
        h.run_steps(2);
        let fitted = h.state().session.doc().unwrap().doc.item(id).unwrap().bounds();
        assert!(fitted.width() < original.width() && fitted.height() < original.height(), "{fitted:?}");
        assert_eq!(h.state().session.tool_id(), "selection");
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.doc().unwrap().doc.item(id).unwrap().bounds(), original);
    }

    const ART: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50" width="100pt" height="50pt"><path d="M0,0 C30,0 70,50 100,50" fill="none" stroke="#000" stroke-width="2"/></svg>"##;

    #[test]
    fn pasting_svg_from_another_app_places_editable_paths_in_the_middle_of_the_view() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut h = harness(app);
        h.run_steps(2);
        let undo = h.state().session.doc().unwrap().history.undo.len();
        h.event(egui::Event::Paste(ART.into()));
        h.run_steps(2);
        let app = h.state();
        let st = app.session.doc().unwrap();
        assert_eq!(st.history.undo.len(), undo + 1, "one undo step");
        let id = *st.selection.items.first().expect("the pasted path is selected");
        let it = st.doc.item(id).unwrap();
        assert_eq!(it.shape, designcraft_doc::Shape::Path);
        assert_eq!(it.path.subpaths[0].anchors.len(), 2);
        assert!((it.stroke.weight - 2.0).abs() < 1e-9);
        // Centred where text from another app would go: the middle of the view.
        let (_, centre) = view_centre(app).unwrap();
        assert!((it.bounds().center() - centre).hypot() < 0.01, "{:?} vs {centre:?}", it.bounds());
    }

    #[test]
    fn the_paste_in_place_shortcut_pastes_in_place_and_plain_paste_offsets() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let id = app.run("frame.create", json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let original = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(id)).unwrap().bounds();
        app.run("edit.copy", json!({})).unwrap();
        let clip = app.object_clip_text.clone().expect("the copy's placeholder text");
        let mut h = harness(app);
        let paste = |h: &mut Harness<'static, DesignApp>, modifiers: egui::Modifiers| {
            // As egui-winit delivers ⌃⌥⇧V: the chord's paste and the V release, no V press; here the
            // modifiers are released within the same frame.
            let release = egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers };
            let none = egui::Event::ModifiersChanged(egui::Modifiers::NONE);
            h.input_mut().events.extend([egui::Event::ModifiersChanged(modifiers), egui::Event::Paste(clip.clone()), release, none]);
            h.run_steps(2);
            let st = h.state().session.doc().unwrap();
            let pasted = *st.selection.items.first().expect("the copy is selected");
            assert_ne!(pasted.0, id);
            st.doc.item(pasted).unwrap().bounds()
        };
        let linux = egui::Modifiers { alt: true, ctrl: true, shift: true, mac_cmd: false, command: true };
        assert_eq!(paste(&mut h, linux), original, "Paste in Place");
        let moved = paste(&mut h, egui::Modifiers::COMMAND);
        assert!((moved.x0 - original.x0 - 12.0).abs() < 1e-9 && (moved.y0 - original.y0 - 12.0).abs() < 1e-9, "{moved:?}");
        // The modifiers went down in an earlier frame than the paste.
        h.event_modifiers(egui::Event::Paste(clip.clone()), linux);
        h.run_steps(2);
        let st = h.state().session.doc().unwrap();
        let pasted = *st.selection.items.first().expect("the copy is selected");
        assert_eq!(st.doc.item(pasted).unwrap().bounds(), original);
    }

    #[test]
    fn edit_paste_from_the_menu_pastes_svg_on_the_system_clipboard_as_paths() {
        let services = crate::Services { clipboard_text: Some(Box::new(|| Some(ART.to_string()))), ..Default::default() };
        let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
        app.native_menu = true;
        app.run("file.new", json!({})).unwrap();
        crate::menus::activate(&mut app, "edit.paste", &Value::Null);
        let st = app.session.doc().unwrap();
        let id = *st.selection.items.first().expect("the pasted path is selected");
        assert_eq!(st.doc.item(id).unwrap().shape, designcraft_doc::Shape::Path);
        assert!(st.selection.text.is_none(), "paths, not a text frame of markup");
        // With an insertion point, the markup is text like any other.
        let story = app.run("frame.create", json!({"rect": [10, 10, 300, 300], "content": "text"})).unwrap()["story"].clone();
        app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
        let items = app.session.doc().unwrap().doc.spreads[0].items.len();
        crate::menus::activate(&mut app, "edit.paste", &Value::Null);
        assert_eq!(app.session.doc().unwrap().doc.spreads[0].items.len(), items, "no paths added");
    }

    #[test]
    fn selected_merged_owners_paint_the_same_row_and_column_targets() {
        for column in [true, false] {
            for merged in [false, true] {
                let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
                app.run("file.new", json!({"pages": 1})).unwrap();
                app.run("frame.create", json!({"rect": [36, 36, 300, 300], "content": "text"})).unwrap();
                app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
                if merged {
                    let range = if column { json!({"rows": [0, 0], "cols": [0, 1]}) } else { json!({"rows": [0, 1], "cols": [0, 0]}) };
                    app.run("table.select", range).unwrap();
                    app.run("table.merge", json!({})).unwrap();
                }
                app.run("table.select", json!({"rows": [1, 1], "cols": [1, 1]})).unwrap();
                app.run(if column { "table.selectColumn" } else { "table.selectRow" }, json!({})).unwrap();
                let st = app.session.doc().unwrap();
                let selection = st.selection.clone();
                let ts = selection.text.unwrap();
                let selected = selection.cells.unwrap();
                assert_eq!(
                    selected.range,
                    if column { designcraft_doc::CellRange::new(0, 1, 1, 1) } else { designcraft_doc::CellRange::new(1, 0, 1, 1) }
                );
                let layout = CanvasLayout::new(&st.doc, st.editing_parents);
                let cs = app.session.cache.get(&st.doc, ts.story, None);
                let ft = &cs.frames[0];
                let table = ft.tables.iter().find(|t| t.table == selected.table).unwrap();
                let (a, _) = item_canvas_xf(&st.doc, &layout, ft.frame).unwrap();
                let m = a * st.doc.text_xf(st.doc.item(ft.frame).unwrap());
                let xf = Xf { min: pos2(0.0, 0.0), origin: Point::new(0.0, 0.0), zoom: 1.0, rot: 0, pivot: pos2(0.0, 0.0) };
                let owners = if merged {
                    [(0, 0), (1, 1)]
                } else if column {
                    [(0, 1), (1, 1)]
                } else {
                    [(1, 0), (1, 1)]
                };
                let expected: Vec<_> = owners
                    .into_iter()
                    .map(|(r, c)| {
                        let rect = table.cells.iter().find(|cell| cell.row == r && cell.col == c).unwrap().rect;
                        vec![
                            xf.to_screen(m * Point::new(rect.x0, rect.y0)),
                            xf.to_screen(m * Point::new(rect.x1, rect.y0)),
                            xf.to_screen(m * Point::new(rect.x1, rect.y1)),
                            xf.to_screen(m * Point::new(rect.x0, rect.y1)),
                        ]
                    })
                    .collect();
                let ctx = egui::Context::default();
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    draw_text_selection(&app, ui.painter(), &xf, &st.doc, &layout, ts, Some(selected));
                });
                output.textures_delta.clear();
                let tint = Color32::from_rgba_unmultiplied(80, 140, 255, 90);
                let painted: Vec<_> = output
                    .shapes
                    .into_iter()
                    .filter_map(|s| match s.shape {
                        egui::Shape::Path(path) if path.fill == tint => {
                            assert!(path.closed);
                            assert_eq!(path.stroke, egui::epaint::PathStroke::NONE);
                            Some(path.points)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(painted, expected, "only the two touched owners are tinted; column={column}, merged={merged}");
                assert_eq!(app.session.doc().unwrap().selection, selection);
            }
        }
    }

    /// A selected group lists the text frames inside it for ports.
    #[test]
    fn grouped_text_frames_get_ports() {
        let mut s = designcraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let a = s.execute("frame.create", &json!({"rect": [10, 10, 110, 60], "content": "text"})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [10, 80, 110, 130]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a, b]})).unwrap();
        let g = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
        let doc = &s.doc().unwrap().doc;
        let group = doc.item(designcraft_doc::ItemId(g)).unwrap();
        let mut out = Vec::new();
        text_frames_in(group, 0, &mut out);
        assert_eq!(out.iter().map(|(it, _)| it.id.0).collect::<Vec<_>>(), vec![a], "one text frame in the group");
        // A plain text frame lists itself.
        out.clear();
        text_frames_in(out_frame(doc, a), 0, &mut out);
        assert_eq!(out.len(), 1);
    }

    /// A selected group paints both ports of the overset text frame inside it, on top of the
    /// group's own selection marks.
    #[test]
    fn a_selected_group_paints_both_ports_of_its_text_frame() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let s = &mut app.session;
        let text = "far too much text for a frame this small, over and over again ".repeat(8);
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 140], "content": "text", "text": text})).unwrap()["id"].as_u64().unwrap();
        let b = s.execute("frame.create", &json!({"rect": [100, 200, 200, 260]})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [a, b]})).unwrap();
        let g = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
        s.execute("transform.rotate", &json!({"angle": 30, "ids": [g]})).unwrap();
        s.execute("selection.set", &json!({"ids": [g]})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        let layout = CanvasLayout::new(&doc, false);
        let xf = Xf { min: pos2(0.0, 0.0), origin: Point::new(0.0, 0.0), zoom: 1.0, rot: 0, pivot: pos2(0.0, 0.0) };
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_selection(&app, ui.painter(), &xf, &doc, &layout);
        });
        output.textures_delta.clear();
        let white: Vec<Rect> = output
            .shapes
            .into_iter()
            .filter_map(|s| match s.shape {
                egui::Shape::Rect(r) if r.fill == Color32::WHITE => Some(r.rect),
                _ => None,
            })
            .collect();
        for out in [false, true] {
            let c = xf.to_screen(designcraft_tools::thread::port_center(&doc, &layout, designcraft_doc::ItemId(a), out, xf.zoom).unwrap());
            let port = white.iter().position(|r| (r.center() - c).length() < 0.01).unwrap_or_else(|| panic!("port (out: {out}) at {c:?}"));
            assert!(!white[port + 1..].iter().any(|r| r.contains(c)), "nothing painted later covers the port (out: {out})");
        }
    }

    fn out_frame(doc: &Document, id: u64) -> &Item {
        doc.item(designcraft_doc::ItemId(id)).unwrap()
    }

    /// An overset frame inside a group is listed for the red mark even when nothing is
    /// selected; a frame with room, or a hidden one, is not.
    #[test]
    fn overset_frames_are_found_inside_groups() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let s = &mut app.session;
        let frame_with = |s: &mut designcraft_engine::Session, rect: [f64; 4], text: &str| {
            s.execute("frame.create", &json!({"rect": rect, "content": "text", "text": text})).unwrap()["id"].as_u64().unwrap()
        };
        let tiny = frame_with(s, [10.0, 10.0, 40.0, 16.0], "far too much text for a frame this small, over and over again");
        let roomy = frame_with(s, [10.0, 100.0, 300.0, 300.0], "fits");
        s.execute("selection.set", &json!({"ids": [tiny, roomy]})).unwrap();
        s.execute("object.group", &json!({})).unwrap();
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        let mut out = Vec::new();
        overset_frames(&app, &doc, &doc.spreads[0].items, 0, &mut out);
        assert_eq!(out.iter().map(|it| it.id.0).collect::<Vec<_>>(), vec![tiny]);
        // Hidden: no mark.
        app.session.execute("object.hide", &json!({"ids": [tiny]})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        out.clear();
        overset_frames(&app, &doc, &doc.spreads[0].items, 0, &mut out);
        assert!(out.is_empty());
    }

    /// The always-on overset mark sits at the frame's out port, a step along its own right edge
    /// (so a rotated frame gets it inside its edge, not straight up from the corner), and a
    /// selected overset frame gets only the mark its ports draw.
    #[test]
    fn overset_mark_sits_on_the_out_port_once() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let s = &mut app.session;
        let id = s
            .execute("frame.create", &json!({"rect": [100, 100, 200, 160], "content": "text", "text": "far too much text for a frame this small, over and over again ".repeat(8)}))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        s.execute("transform.rotate", &json!({"angle": 90, "ids": [id]})).unwrap();
        s.execute("selection.set", &json!({"ids": []})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        let layout = CanvasLayout::new(&doc, false);
        let zoom = 1.0;
        let marks = overset_marks(&app, &doc, &layout, &doc.spreads[0].items, zoom, &ported_frames(&app, &doc));
        assert_eq!(marks.len(), 1);
        let it = out_frame(&doc, id);
        let (a, _) = item_canvas_xf(&doc, &layout, it.id).unwrap();
        let m = a * it.xf;
        let r = it.inner_bounds();
        let corner = m * Point::new(r.x1, r.y1);
        let edge = (m * Point::new(r.x1, r.y0)) - corner;
        let d = marks[0].1 - corner;
        assert!((d.x * edge.y - d.y * edge.x).abs() < 1e-6, "the mark lies on the frame's right edge: {d:?} along {edge:?}");
        assert!((d.hypot() - 12.0 / zoom).abs() < 1e-6, "12 screen px from the corner: {d:?}");
        // Selected: the ports draw the mark, so no always-on mark is added.
        app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let marks = overset_marks(&app, &doc, &layout, &doc.spreads[0].items, zoom, &ported_frames(&app, &doc));
        assert!(marks.is_empty());
    }

    #[test]
    fn right_clicking_a_ruler_sets_that_rulers_units() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut h = harness(app);
        let rect = h.state().canvas_rect.unwrap();
        assert_eq!(units(&h), (Unit::Picas, Unit::Picas));
        // Vertical ruler: the current unit is checked; picking one sets only the vertical units.
        right_click(&mut h, pos2(rect.min.x - RULER / 2.0, rect.center().y));
        h.get_by_label("✓ Picas");
        h.get_by_label("   Millimeters").click();
        h.run_steps(3);
        assert_eq!(units(&h), (Unit::Picas, Unit::Millimeters));
        // Horizontal ruler.
        right_click(&mut h, pos2(rect.center().x, rect.min.y - RULER / 2.0));
        h.get_by_label("✓ Picas");
        h.get_by_label("   Inches").click();
        h.run_steps(3);
        assert_eq!(units(&h), (Unit::Inches, Unit::Millimeters));
        // The corner where the rulers meet sets both, as InDesign's: nothing is checked while
        // they differ.
        let corner = rect.min - vec2(RULER / 2.0, RULER / 2.0);
        right_click(&mut h, corner);
        assert!(h.query_by_label("✓ Inches").is_none() && h.query_by_label("✓ Millimeters").is_none());
        let _ = h.get_by_label_contains("Hide Rulers");
        let _ = h.get_by_label("   Lock Zero Point");
        assert!(h.query_by_label("   Delete All Guides on Spread").is_none(), "only on the horizontal ruler");
        h.get_by_label("   Points").click();
        h.run_steps(3);
        assert_eq!(units(&h), (Unit::Points, Unit::Points));
        // InDesign's list: no Q or Ha.
        right_click(&mut h, corner);
        h.get_by_label("✓ Points");
        assert!(h.query_by_label("   Q").is_none() && h.query_by_label("   Ha").is_none());
        let _ = h.get_by_label("   Inches Decimal");
        // The horizontal ruler's menu also deletes the spread's guides; Hide Rulers hides them.
        right_click(&mut h, pos2(rect.center().x, rect.min.y - RULER / 2.0));
        let _ = h.get_by_label("   Delete All Guides on Spread");
        h.get_by_label_contains("Hide Rulers").click();
        h.run_steps(3);
        assert!(!h.state().ui.rulers);
        // The page opens no units menu.
        right_click(&mut h, rect.center());
        assert!(h.query_by_label("   Points").is_none());
    }

    /// The live-corner widget draws on a selected rectangle frame; clicking it draws the diamonds.
    #[test]
    fn live_corner_widget_and_diamonds_draw() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [100, 100, 300, 250], "content": "graphic"})).unwrap();
        let id = app.session.doc().unwrap().doc.spreads[0].items[0].id;
        app.run("selection.set", json!({"ids": [id.0]})).unwrap();
        let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_pixels_per_point(1.0).wgpu().build_ui_state(
            |ui, app: &mut DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        let screen = |h: &Harness<'_, DesignApp>, x: f64, y: f64| {
            let app = h.state();
            let layout = CanvasLayout::new(&app.session.doc().unwrap().doc, false);
            let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
            xf.to_screen(layout.to_canvas(SpreadRef::Doc(0), Point::new(x, y)))
        };
        let widget = screen(&h, 300.0, 100.0) + vec2(0.0, 11.5);
        let diamond = screen(&h, 100.0, 100.0) + vec2(12.0, 0.0);
        let yellow = |h: &mut Harness<'_, DesignApp>, p: Pos2| -> Option<bool> {
            let img = match h.render() {
                Ok(img) => img,
                Err(e) => {
                    eprintln!("skipped: no offscreen renderer ({e})");
                    return None;
                }
            };
            let px = img.get_pixel(p.x.round() as u32, p.y.round() as u32).0;
            Some(px[0] > 200 && px[1] > 180 && px[2] < 100)
        };
        let Some(w) = yellow(&mut h, widget) else { return };
        assert!(w, "no widget at {widget:?}");
        assert_eq!(yellow(&mut h, diamond), Some(false));
        h.hover_at(widget);
        h.step();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: widget, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        }
        h.run_steps(3);
        assert_eq!(yellow(&mut h, diamond), Some(true), "no diamond at {diamond:?}");
        assert_eq!(yellow(&mut h, widget), Some(false));
    }

    /// A harness whose frames are 1 ms apart, so two clicks make a double-click.
    fn fast_harness(app: DesignApp) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(0.001).build_ui_state(
            |ui, app: &mut DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        h
    }

    fn primary(h: &mut Harness<'_, DesignApp>, pos: Pos2, pressed: bool) {
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
        h.run_steps(2);
    }

    fn corner_drag(h: &mut Harness<'_, DesignApp>, from: Pos2, to: Pos2) {
        h.hover_at(from);
        h.step();
        primary(h, from, true);
        h.event(egui::Event::PointerMoved(from + (to - from) / 2.0));
        h.run_steps(2);
        h.event(egui::Event::PointerMoved(to));
        h.run_steps(2);
        primary(h, to, false);
    }

    fn corner_click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
        h.hover_at(p);
        h.step();
        primary(h, p, true);
        primary(h, p, false);
    }

    /// Where the rulers measure from (canvas coordinates): the document's zero point on the
    /// spread in view (its first page, for a page origin).
    fn ruler_zero(app: &DesignApp, doc: &Document, layout: &CanvasLayout) -> Option<Point> {
        ruler_pieces(app, doc, layout, doc.zero_point()).first().map(|p| p.2)
    }

    fn zero_point(h: &Harness<'_, DesignApp>) -> [f64; 2] {
        h.state().session.doc().unwrap().doc.settings.zero_point
    }

    #[test]
    fn the_ruler_corner_drags_resets_and_locks_the_zero_point() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 3})).unwrap();
        let mut h = fast_harness(app);
        let rect = h.state().canvas_rect.unwrap();
        let corner = rect.min - vec2(RULER / 2.0, RULER / 2.0);
        // A click that stays in the corner leaves the zero point.
        corner_click(&mut h, corner);
        assert_eq!(zero_point(&h), [0.0, 0.0]);
        // Dragged out, it lands where released, measured from the spread in view.
        let dest = rect.min + vec2(80.0, 50.0);
        let expected = {
            let app = h.state();
            let st = app.session.active().unwrap();
            let layout = CanvasLayout::new(&st.doc, false);
            let slot = layout.slots[current_slot(app, &layout).unwrap()];
            let sp = slot.to_spread(Xf::new(rect, app.view().unwrap()).to_canvas(dest));
            let b = st.doc.spread(slot.spread).unwrap().bounds();
            [sp.x - b.x0, sp.y - b.y0]
        };
        corner_drag(&mut h, corner, dest);
        let zp = zero_point(&h);
        assert!((zp[0] - expected[0]).abs() < 0.5 && (zp[1] - expected[1]).abs() < 0.5, "{zp:?} vs {expected:?}");
        assert!(zp[0].abs() > 1.0 && zp[1].abs() > 1.0, "moved: {zp:?}");
        // The rulers measure from the point it was dropped on.
        {
            let app = h.state();
            let st = app.session.active().unwrap();
            let layout = CanvasLayout::new(&st.doc, false);
            let o = ruler_zero(app, &st.doc, &layout).unwrap();
            let on_screen = Xf::new(rect, app.view().unwrap()).to_screen(o);
            assert!((on_screen - dest).length() < 0.5, "{on_screen:?} vs {dest:?}");
        }
        // A double-click resets it.
        corner_click(&mut h, corner);
        corner_click(&mut h, corner);
        assert_eq!(zero_point(&h), [0.0, 0.0]);
        // Locked from the corner's context menu, the corner ignores drags and double-clicks.
        right_click(&mut h, corner);
        h.get_by_label("   Lock Zero Point").click();
        h.run_steps(3);
        assert!(h.state().session.doc().unwrap().doc.settings.zero_point_locked);
        corner_drag(&mut h, corner, dest);
        assert_eq!(zero_point(&h), [0.0, 0.0]);
        // Unlocked again from the same menu.
        right_click(&mut h, corner);
        h.get_by_label("   Unlock Zero Point").click();
        h.run_steps(3);
        corner_drag(&mut h, corner, dest);
        assert_ne!(zero_point(&h), [0.0, 0.0]);
        h.state_mut().run("view.lockZeroPoint", json!({"on": true})).unwrap();
        corner_click(&mut h, corner);
        corner_click(&mut h, corner);
        assert_ne!(zero_point(&h), [0.0, 0.0], "a locked zero point isn't reset");
    }

    #[test]
    fn cmd_dragging_the_ruler_corner_makes_a_guide_each_way() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut h = harness(app);
        let rect = h.state().canvas_rect.unwrap();
        let corner = rect.min - vec2(RULER / 2.0, RULER / 2.0);
        let dest = rect.min + vec2(120.0, 90.0);
        let (r, at) = {
            let app = h.state();
            let st = app.session.active().unwrap();
            CanvasLayout::new(&st.doc, false).spread_at(Xf::new(rect, app.view().unwrap()).to_canvas(dest)).unwrap()
        };
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
        h.step();
        corner_drag(&mut h, corner, dest);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.step();
        let doc = h.state().session.doc().unwrap().doc.clone();
        let guides: Vec<_> = doc.spread(r).unwrap().pages.iter().flat_map(|p| p.guides.clone()).collect();
        assert_eq!(guides.len(), 2, "{guides:?}");
        let pos = |o: designcraft_doc::Orientation| guides.iter().find(|g| g.orientation == o).map(|g| g.position).unwrap();
        assert!((pos(designcraft_doc::Orientation::Horizontal) - at.y).abs() < 0.5);
        assert!((pos(designcraft_doc::Orientation::Vertical) - at.x).abs() < 0.5);
        assert!(guides.iter().all(|g| g.spread));
        assert_eq!(doc.settings.zero_point, [0.0, 0.0], "the zero point stays");
    }

    /// A guide being dragged out of a ruler says where it is, as the rulers measure it (#1282):
    /// from the zero point, in the ruler units, and the label follows the pointer while it moves.
    #[test]
    fn a_dragged_guide_shows_where_it_is() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("view.zeroPoint", json!({"at": [100, 50]})).unwrap();
        let mut h = harness(app);
        let rect = h.state().canvas_rect.unwrap();
        let dest = rect.min + vec2(160.0, 110.0);
        let (xf, at) = {
            let app = h.state();
            let xf = Xf::new(rect, app.view().unwrap());
            let st = app.session.active().unwrap();
            (xf, CanvasLayout::new(&st.doc, false).spread_at(xf.to_canvas(dest)).unwrap().1)
        };
        let units = h.state().session.doc().unwrap().doc.settings.horizontal_units;
        for (orientation, label, want) in
            [(designcraft_doc::Orientation::Vertical, "X", at.x - 100.0), (designcraft_doc::Orientation::Horizontal, "Y", at.y - 50.0)]
        {
            let g = GuideDrag { orientation, from: None, at: Some(dest), mods: egui::Modifiers::NONE };
            let (l, v, _) = guide_measure(h.state(), &xf, &g).unwrap();
            assert_eq!(l, label);
            assert!((v - want).abs() < 1e-6, "{label}: {v} vs {want}");
            let text = guide_readout(h.state(), &xf, &g).unwrap();
            assert_eq!(text, format!("{label}: {}", designcraft_geom::units::format_measure(v, units)));
        }
        // While a guide is actually being dragged out of the top ruler, the readout is there.
        let ruler = pos2(rect.min.x + 200.0, rect.min.y - RULER / 2.0);
        h.hover_at(ruler);
        h.step();
        primary(&mut h, ruler, true);
        h.event(egui::Event::PointerMoved(dest));
        h.run_steps(2);
        let g = h.ctx.data(|d| d.get_temp::<GuideDrag>(egui::Id::unique(GUIDE_DRAG))).expect("dragging");
        assert_eq!(g.orientation, designcraft_doc::Orientation::Horizontal);
        assert!(guide_readout(h.state(), &xf, &g).is_some_and(|t| t.starts_with("Y: ")));
        primary(&mut h, dest, false);
    }

    /// Over a spread other than the one in the middle of the view, the readout measures that
    /// spread, the one the guide is dropped on: it reads the position the guide gets.
    #[test]
    fn the_guide_readout_measures_the_spread_under_the_pointer() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 5})).unwrap();
        app.run("view.zeroPoint", json!({"at": [100, 50]})).unwrap();
        let h = harness(app);
        let rect = h.state().canvas_rect.unwrap();
        let xf = Xf::new(rect, h.state().view().unwrap());
        let st = h.state().session.active().unwrap();
        let layout = CanvasLayout::new(&st.doc, false);
        let current = current_slot(h.state(), &layout).unwrap();
        // A point inside the last spread, well away from the one in view.
        let last = layout.slots.last().unwrap();
        assert_ne!(layout.slots.iter().position(|s| s.spread == last.spread), Some(current), "the last spread isn't the one in view");
        let inside = Point::new(last.bounds.center().x, last.bounds.y0 + (last.bounds.y1 - last.bounds.y0) * 0.3);
        let (r, sp) = layout.spread_at(inside).unwrap();
        assert_eq!(r, last.spread);
        let g = GuideDrag {
            orientation: designcraft_doc::Orientation::Horizontal,
            from: None,
            at: Some(xf.to_screen(inside)),
            mods: egui::Modifiers::NONE,
        };
        let (label, v, _) = guide_measure(h.state(), &xf, &g).unwrap();
        assert_eq!(label, "Y");
        // The guide would be stored at `sp.y` on that spread, which its ruler reads from the zero point.
        // (To screen and back in f32: a few hundred-thousandths of a point.)
        assert!((v - (sp.y - 50.0)).abs() < 1e-3, "{v} vs {}", sp.y - 50.0);
        // An existing guide of another spread can't be dropped there (it only moves on its own
        // spread): no readout over this one, a readout over its own.
        let first = layout.slots[current].spread;
        let moving = GuideDrag {
            orientation: designcraft_doc::Orientation::Horizontal,
            from: Some((first, 0, 0)),
            at: Some(xf.to_screen(inside)),
            mods: egui::Modifiers::NONE,
        };
        assert!(guide_measure(h.state(), &xf, &moving).is_none(), "an existing guide over another spread");
        let own = layout.slots[current].bounds;
        let over_own = Point::new(own.center().x, own.y0 + (own.y1 - own.y0) * 0.3);
        let moving_here = GuideDrag { at: Some(xf.to_screen(over_own)), ..moving };
        assert!(guide_measure(h.state(), &xf, &moving_here).is_some(), "over its own spread");
    }

    /// The guide readout stays on the canvas: below right of the pointer in the open, flipped
    /// left or up near the right or bottom edge, and inside the canvas in every corner.
    #[test]
    fn the_guide_readout_stays_inside_the_canvas() {
        let area = Rect::from_min_max(pos2(50.0, 40.0), pos2(850.0, 640.0));
        let size = vec2(80.0, 14.0);
        let inside = |at: Pos2| {
            let r = Rect::from_min_size(at, size).expand(3.0);
            r.min.x >= area.min.x - 1e-3 && r.min.y >= area.min.y - 1e-3 && r.max.x <= area.max.x + 1e-3 && r.max.y <= area.max.y + 1e-3
        };
        assert_eq!(readout_at(pos2(400.0, 300.0), size, area), pos2(412.0, 312.0), "in the open: below right");
        let right = readout_at(pos2(845.0, 300.0), size, area);
        assert!(right.x + size.x < 845.0 && inside(right), "near the right edge: left of the pointer, {right:?}");
        let bottom = readout_at(pos2(400.0, 636.0), size, area);
        assert!(bottom.y + size.y < 636.0 && inside(bottom), "near the bottom: above it, {bottom:?}");
        for p in [area.min, pos2(area.max.x, area.min.y), area.max, pos2(area.min.x, area.max.y), pos2(-500.0, 2000.0)] {
            assert!(inside(readout_at(p, size, area)), "{p:?}");
        }
        // A canvas narrower than the label: pinned to its top left, not past it.
        let tiny = Rect::from_min_max(pos2(0.0, 0.0), pos2(40.0, 10.0));
        assert_eq!(readout_at(pos2(20.0, 5.0), size, tiny), pos2(3.0, 3.0));
    }

    #[test]
    fn rulers_and_xy_fields_measure_from_the_zero_point_on_every_spread() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        // Spread 0: page 1; spread 1: pages 2 and 3.
        app.run("file.new", json!({"pages": 3})).unwrap();
        app.run("view.zeroPoint", json!({"at": [100, 50]})).unwrap();
        app.session.prefs.dimensions_include_stroke = false;
        let mut h = harness(app);
        let rect = h.state().canvas_rect.unwrap();
        for (spread, x0) in [(0usize, 136.0), (1, 736.0)] {
            let app = h.state_mut();
            let id = app.run("frame.create", json!({"spread": spread, "rect": [x0, 70, x0 + 50.0, 120]})).unwrap()["id"].clone();
            app.run("selection.set", json!({"ids": [id]})).unwrap();
            // X/Y (Control, Properties and Info panels).
            let r = crate::panels::sel_info(app).unwrap().page_rect;
            assert!((r.x0 - (x0 - 100.0)).abs() < 1e-6 && (r.y0 - 20.0).abs() < 1e-6, "spread {spread}: {r:?}");
            // The rulers, with that spread in view.
            let layout = CanvasLayout::new(&app.session.active().unwrap().doc, false);
            let slot = layout.slots[spread];
            let v = app.view_mut().unwrap();
            let zoom = v.zoom;
            let c = slot.bounds.center();
            v.origin = Point::new(c.x - rect.width() as f64 / 2.0 / zoom, c.y - rect.height() as f64 / 2.0 / zoom);
            h.run_steps(2);
            let app = h.state();
            let doc = app.session.active().unwrap().doc.clone();
            assert_eq!(current_slot(app, &layout), Some(spread));
            let o = ruler_zero(app, &doc, &layout).unwrap();
            assert!((o.x - slot.bounds.x0 - 100.0).abs() < 1e-6 && (o.y - slot.bounds.y0 - 50.0).abs() < 1e-6, "spread {spread}: {o:?}");
        }
    }

    /// Screen position of spread 0's point (x, y).
    fn screen_of(h: &Harness<'_, DesignApp>, x: f64, y: f64) -> Pos2 {
        let app = h.state();
        let st = app.session.active().unwrap();
        let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
        xf.to_screen(CanvasLayout::new(&st.doc, st.editing_parents).xf(SpreadRef::Doc(0)) * Point::new(x, y))
    }

    /// A menu row: a button that isn't a toggle, named by its label, then its shortcut if it has
    /// one. Toggle buttons with the same name (the Task Bar's icons) don't match, nor do buttons
    /// right of the canvas (the Properties panel's Arrange).
    fn menu_row<'h>(h: &'h Harness<'_, DesignApp>, label: &'h str) -> Option<egui_kittest::Node<'h>> {
        let right = f64::from(h.state().canvas_rect.map_or(f32::INFINITY, |r| r.max.x));
        h.query_by(move |n| {
            n.role() == egui::accesskit::Role::Button
                && n.toggled().is_none()
                && n.bounding_box().is_none_or(|b| b.x0 < right)
                && n.label().is_some_and(|l| l == label || l.strip_prefix(label).and_then(|r| r.strip_prefix(' ')).is_some_and(|r| !r.contains(' ')))
        })
    }

    fn enabled(h: &Harness<'_, DesignApp>, label: &str) -> bool {
        use egui_kittest::kittest::NodeT;
        !menu_row(h, label).unwrap_or_else(|| panic!("no menu row {label}")).accesskit_node().is_disabled()
    }

    fn spread_items(h: &Harness<'_, DesignApp>) -> Vec<u64> {
        h.state().session.doc().unwrap().doc.spreads[0].items.iter().map(|i| i.id.0).collect()
    }

    fn selected(h: &Harness<'_, DesignApp>) -> Vec<u64> {
        h.state().session.doc().unwrap().selection.items.iter().map(|i| i.0).collect()
    }

    #[test]
    fn right_clicking_an_object_selects_it_and_its_menu_arranges_it() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let b = app.run("frame.create", json!({"rect": [150, 150, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        let mut h = harness(app);
        assert_eq!(selected(&h), [b]);
        let on_a = screen_of(&h, 60.0, 60.0);
        right_click(&mut h, on_a);
        assert_eq!(selected(&h), [a], "the object under the pointer is selected first");
        // An empty frame's menu: Content, but no Fitting (nothing to fit) and no text frame entries.
        assert!(menu_row(&h, "Content").is_some());
        assert!(menu_row(&h, "Fitting").is_none() && menu_row(&h, "Text Frame Options…").is_none());
        // Paste is available with nothing copied here: the system clipboard may hold another app's text.
        assert!(enabled(&h, "Copy") && enabled(&h, "Paste"));
        menu_row(&h, "Arrange").unwrap().click();
        h.run_steps(3);
        menu_row(&h, "Bring to Front").unwrap().click();
        h.run_steps(3);
        assert_eq!(spread_items(&h), [b, a]);
        assert!(menu_row(&h, "Arrange").is_none(), "choosing an entry closes the menu");
    }

    #[test]
    fn right_clicking_empty_canvas_offers_paste_after_a_copy() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        app.run("edit.copy", json!({})).unwrap();
        app.select_tool("rectangleFrame");
        let mut h = harness(app);
        let empty = screen_of(&h, 400.0, 500.0);
        right_click(&mut h, empty);
        assert_eq!(selected(&h), [a], "a right-click on nothing keeps the selection");
        assert!(menu_row(&h, "Arrange").is_none(), "the page menu, not the object menu");
        assert!(enabled(&h, "Paste in Place"));
        assert!(menu_row(&h, "✓ Show/Hide Rulers").is_some(), "toggles show their state");
        // Escape closes the menu and does nothing else: the tool would deselect, the shortcuts
        // would switch to the Selection tool.
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(menu_row(&h, "Paste in Place").is_none());
        assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
        right_click(&mut h, empty);
        menu_row(&h, "Paste").unwrap().click();
        h.run_steps(3);
        assert_eq!(spread_items(&h).len(), 2);
    }

    #[test]
    fn right_clicking_another_object_while_the_menu_is_open_reopens_it_there() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let b = app.run("frame.create", json!({"rect": [36, 600, 200, 760]})).unwrap()["id"].as_u64().unwrap();
        let mut h = harness(app);
        // The menu opens down from A, with the pointer on its first row (no submenu open); B is below it.
        let (on_a, on_b) = (screen_of(&h, 60.0, 60.0), screen_of(&h, 50.0, 700.0));
        right_click(&mut h, on_a);
        assert_eq!(selected(&h), [a]);
        assert!(menu_row(&h, "Arrange").is_some());
        right_click(&mut h, on_b);
        assert_eq!(selected(&h), [b]);
        assert!(menu_row(&h, "Arrange").is_some(), "the menu reopens at the new click");
    }

    #[test]
    fn a_context_menu_in_one_pane_keeps_the_keys_from_the_other() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 100, 100]})).unwrap()["id"].as_u64().unwrap();
        app.select_tool("rectangleFrame");
        app.split = true;
        let mut h = harness(app);
        // The focused pane is the left one; this point is near its right edge, away from the frame.
        let left = h.state().canvas_rect.unwrap();
        right_click(&mut h, pos2(left.max.x - 40.0, left.center().y));
        assert!(menu_row(&h, "Paste in Place").is_some());
        // Escape closes the left pane's menu before the right pane handles the keys.
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(menu_row(&h, "Paste in Place").is_none());
        assert_eq!((h.state().session.tool_id(), selected(&h)), ("rectangleFrame", vec![a]));
    }

    #[test]
    fn right_clicking_text_keeps_the_selected_text_or_places_the_caret() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 400, 200], "content": "text"})).unwrap();
        app.select_tool("type");
        app.run("text.insert", json!({"text": "Hello world"})).unwrap();
        let sid = *app.session.doc().unwrap().doc.stories.keys().next().unwrap();
        app.run("text.select", json!({"story": sid.0, "anchor": 0, "focus": 5})).unwrap();
        let mut h = harness(app);
        let text_sel = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().selection.text.as_ref().map(|t| (t.anchor, t.focus));
        // Inside "Hello": the selection stays.
        let inside = screen_of(&h, 45.0, 44.0);
        right_click(&mut h, inside);
        assert_eq!(text_sel(&h), Some((0, 5)));
        assert!(menu_row(&h, "Paste without Formatting").is_some(), "the text menu");
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        // Past the end of the line: the caret goes there.
        let past_end = screen_of(&h, 300.0, 44.0);
        right_click(&mut h, past_end);
        assert_eq!(text_sel(&h), Some((11, 11)));
        assert!(menu_row(&h, "Paste without Formatting").is_some());
    }

    #[test]
    fn rows_hidden_in_the_menu_bar_are_hidden_in_context_menus() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap();
        app.ui.hidden_menu_items.push(crate::menus::menu_key("Object/Arrange", "Bring to Front"));
        let mut h = harness(app);
        let on_frame = screen_of(&h, 60.0, 60.0);
        right_click(&mut h, on_frame);
        menu_row(&h, "Arrange").unwrap().click();
        h.run_steps(3);
        assert!(menu_row(&h, "Bring to Front").is_none());
        assert!(menu_row(&h, "Send to Back").is_some());
        assert!(h.query_by_label("Show All Menu Items").is_some());
    }

    #[test]
    fn a_secondary_drag_neither_moves_marquees_nor_draws() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let a = app.run("frame.create", json!({"rect": [36, 36, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        let mut h = harness(app);
        // A secondary-button drag between spread points.
        let drag = |h: &mut Harness<'_, DesignApp>, from: (f64, f64), to: (f64, f64)| {
            let (from, to) = (screen_of(h, from.0, from.1), screen_of(h, to.0, to.1));
            h.hover_at(from);
            h.step();
            h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Secondary, pressed: true, modifiers: Default::default() });
            h.step();
            for t in [0.25, 0.5, 0.75, 1.0] {
                h.event(egui::Event::PointerMoved(from + (to - from) * t));
                h.step();
            }
            h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Secondary, pressed: false, modifiers: Default::default() });
            h.run_steps(3);
        };
        let xf = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.item(designcraft_doc::ItemId(a)).unwrap().xf;
        let before = xf(&h);
        // Selection tool: on the object (no move), and from empty page across it (no marquee).
        drag(&mut h, (100.0, 100.0), (300.0, 300.0));
        assert_eq!(xf(&h), before);
        h.state_mut().run("edit.deselectAll", json!({})).unwrap();
        drag(&mut h, (400.0, 400.0), (20.0, 20.0));
        assert!(selected(&h).is_empty());
        // Rectangle Frame tool: no frame.
        h.state_mut().select_tool("rectangleFrame");
        drag(&mut h, (300.0, 300.0), (500.0, 500.0));
        assert_eq!(spread_items(&h), [a]);
    }

    /// A text frame holding `text`, the Type tool, and a harness stepping at 60 fps (clicks in a
    /// row must land within the double-click delay).
    fn typing(text: &str) -> (Harness<'static, DesignApp>, u64) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "text": text})).unwrap();
        let id = r["id"].as_u64().unwrap();
        app.run("tool.select", json!({"tool": "type"})).unwrap();
        let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut DesignApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        (h, id)
    }

    /// The screen point over the first line of frame `id` where a click puts the caret at a byte
    /// in `bytes`.
    fn screen_at(h: &mut Harness<'_, DesignApp>, id: u64, bytes: std::ops::Range<usize>) -> Pos2 {
        let mut x = 74.0;
        let p = loop {
            assert!(x < 400.0, "no point on the first line hits {bytes:?}");
            h.state_mut().run("text.placeCaret", json!({"frame": id, "point": [x, 80.0]})).unwrap();
            if h.state().session.doc().unwrap().selection.text.is_some_and(|t| bytes.contains(&t.focus)) {
                break Point::new(x, 80.0);
            }
            x += 1.0;
        };
        h.state_mut().run("edit.deselectAll", json!({})).unwrap();
        let app = h.state();
        let c = app.session.layout().to_canvas(SpreadRef::Doc(0), p);
        Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(c)
    }

    fn click(h: &mut Harness<'_, DesignApp>, p: Pos2) {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        }
    }

    fn selected_range(h: &Harness<'_, DesignApp>) -> std::ops::Range<usize> {
        h.state().session.doc().unwrap().selection.text.map(|t| t.range()).unwrap_or_default()
    }

    #[test]
    fn clicks_in_a_row_select_word_line_paragraph_with_return_and_story() {
        let text = "One two. Three four! Five six.\nSecond para here.";
        let (mut h, id) = typing(text);
        // Inside "Three": double-click selects the word, then line and paragraph with return.
        let p = screen_at(&mut h, id, 10..14);
        h.hover_at(p);
        h.step();
        let mut got = vec![];
        for _ in 0..5 {
            click(&mut h, p);
            let r = selected_range(&h);
            got.push(text[r].to_string());
        }
        assert_eq!(got[1..], ["Three", "One two. Three four! Five six.", "One two. Three four! Five six.\n", text], "{got:?}");
        assert!(got[0].is_empty(), "one click places the caret: {got:?}");
    }

    #[test]
    fn esc_that_closes_a_dialog_keeps_the_type_tool_and_its_text_selection() {
        let (mut h, id) = typing("One two three");
        // A click in the text gives the canvas the keyboard focus, as when typing.
        let p = screen_at(&mut h, id, 5..6);
        h.hover_at(p);
        h.step();
        for _ in 0..2 {
            click(&mut h, p);
        }
        h.run_steps(4);
        let words = selected_range(&h);
        assert_eq!(words, 4..7, "the double-click selects \"two\"");
        let app = h.state_mut();
        let name = app.run("style.character.create", json!({"name": "Key Term"})).unwrap()["name"].as_str().unwrap().to_string();
        app.ui.dialog = Some(crate::dialogs::Dialog::new("characterStyleOptions", json!({"name": name})));
        h.run_steps(3);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.state().ui.dialog.is_none(), "Esc closes the dialog");
        assert_eq!((h.state().session.tool_id(), selected_range(&h)), ("type", words));
        // With no dialog open, Esc in the text still leaves it for the Selection tool.
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert_eq!(h.state().session.tool_id(), "selection");
    }

    #[test]
    fn slow_clicks_do_not_add_up() {
        let (mut h, id) = typing("One two. Three four!");
        let p = screen_at(&mut h, id, 10..14);
        h.hover_at(p);
        h.step();
        for _ in 0..3 {
            click(&mut h, p);
            h.run_steps(30);
        }
        assert!(selected_range(&h).is_empty(), "three clicks half a second apart leave a caret: {:?}", selected_range(&h));
    }

    /// A viewport wider than the GPU's largest texture: the canvas texture is sampled down, and an
    /// edit repaints it whole. A region repainted in view pixels would land in the wrong place,
    /// or outside the texture (a wgpu validation error). A view that fits is still patched.
    #[test]
    fn an_edit_repaints_a_canvas_texture_sampled_down_to_the_gpu_limit_whole() {
        for (width, sampled_down) in [(1440.0, false), (2600.0, true)] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            let frame = app.run("frame.create", json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Text"})).unwrap();
            let mut h = Harness::builder().with_size(vec2(width, 900.0)).build_ui_state(
                |ui, app: &mut DesignApp| {
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx);
                    app.ui(ui);
                },
                app,
            );
            h.run_steps(6);
            let limit = h.ctx.input(|i| i.max_texture_side);
            let size = h.state().canvas.texture.as_ref().map(|t| t.size()).unwrap();
            assert!(size[0] <= limit && size[1] <= limit, "{size:?}");
            let shown = h.state().canvas.shown.unwrap().size;
            assert_eq!(shown.0 > limit as f32, sampled_down, "view {shown:?}, limit {limit}");
            let patches = h.state().canvas.patches;
            // A small edit: one more character in the frame.
            h.state_mut().run("text.insert", json!({"story": frame["story"], "at": 4, "text": "s"})).unwrap();
            h.run_steps(6);
            assert_eq!(h.state().canvas.patches == patches, sampled_down, "width {width}");
            assert_eq!(h.state().canvas.texture.as_ref().map(|t| t.size()), Some(size));
        }
    }

    /// ⌥E then I types "í" on a Mac: the input method composes the dead key's accent, which
    /// it does only for a window that turned it on. The canvas never did, so ⌥E typed "e".
    #[test]
    fn dead_keys_compose_through_the_input_method() {
        let (mut h, id) = typing("T");
        h.state_mut().run("text.placeCaret", json!({"frame": id, "point": [390.0, 80.0]})).unwrap();
        h.run_steps(2);
        assert!(h.output().platform_output.ime.is_some(), "a text caret turns the input method on");
        h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: "´".into(), active_range_chars: None }));
        h.run_steps(2);
        let text = |h: &Harness<'_, DesignApp>| h.state().session.doc().unwrap().doc.stories.values().map(|s| s.text.clone()).collect::<String>();
        assert_eq!(text(&h), "T´", "the accent being composed shows in the story");
        assert!(h.state().session.tool_composing(), "and is marked as composed, not typed");
        h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None }));
        h.event(egui::Event::Ime(egui::ImeEvent::Commit("í".into())));
        h.run_steps(2);
        assert_eq!(text(&h), "Tí");
        assert!(!h.state().session.tool_composing());
        // Moving the caret mid-composition keeps the accent as typed and tells the input method
        // to drop its copy.
        h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: "˜".into(), active_range_chars: None }));
        h.run_steps(2);
        h.state_mut().run("text.move", json!({"dir": "left"})).unwrap();
        h.step();
        assert_eq!(text(&h), "Tí˜");
        assert!(!h.state().session.tool_composing());
        assert!(h.output().platform_output.ime.as_ref().is_some_and(|i| i.should_interrupt_composition));
        assert!(h.state_mut().take_ime_discard());
        // Another tool takes no text: the input method is off.
        h.state_mut().run("tool.select", json!({"tool": "selection"})).unwrap();
        h.run_steps(2);
        assert!(h.output().platform_output.ime.is_none());
    }

    #[test]
    fn tools_show_their_own_cursors_over_the_page() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut h = harness(app);
        let p = h.state().canvas_rect.unwrap().center();
        let mut shown = |tool: &str| {
            h.state_mut().run("tool.select", json!({"tool": tool})).unwrap();
            h.hover_at(p);
            h.run_steps(2);
            let out = &h.output().platform_output;
            (out.cursor_icon, out.cursor_image.clone())
        };
        let (pen, direct, text, selection) = (shown("pen"), shown("directSelection"), shown("type"), shown("selection"));
        let ppp = h.ctx.pixels_per_point();
        let images = &mut h.state_mut().cursors;
        match crate::cursors::mode() {
            crate::cursors::Mode::Os => {
                assert_eq!(pen.1, images.get(Cursor::Pen, ppp), "Pen");
                assert_eq!(direct.1, images.get(Cursor::ArrowHollow, ppp), "Direct Selection");
                assert_eq!(selection.1, images.get(Cursor::Arrow, ppp), "Selection");
                assert_eq!(text, (egui::CursorIcon::Text, None), "the I-beam stays a system cursor");
            }
            crate::cursors::Mode::Painted => {
                assert_eq!((pen.0, direct.0, selection.0), (egui::CursorIcon::None, egui::CursorIcon::None, egui::CursorIcon::None));
            }
            crate::cursors::Mode::System => assert_eq!(pen, (egui::CursorIcon::Crosshair, None)),
        }
    }

    #[test]
    fn document_grid_is_drawn_where_points_snap_to_it() {
        use designcraft_tools::{SnapView, ToolContext};
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        let layout = CanvasLayout::new(&doc, false);
        let sr = SpreadRef::Doc(0);
        // Where the grid snaps a point in the first page (72 pt / 8 subdivisions = 9 pt).
        let page = doc.spreads[0].pages[0].bounds();
        let sel = Selection::default();
        let cx = ToolContext {
            doc: &doc,
            selection: &sel,
            cache: &app.session.cache,
            layout: &layout,
            zoom: 1.0,
            layer: doc.layers[0].id,
            snap: SnapView { snap_to_document_grid: true, zone_px: 20.0, ..SnapView::OFF },
            unit: Unit::Points,
            stroke_in_front: false,
        };
        let (snapped, _) = designcraft_tools::snap::snap_point(&cx, sr, Point::new(page.x0 + 31.3, page.y0 + 47.9));
        assert_eq!((snapped.x - page.x0, snapped.y - page.y0), (27.0, 45.0));
        // Render 40 × 40 pt around it at 4 px/pt.
        let scale = 4.0;
        let corner = layout.xf(sr) * Point::new(snapped.x - 20.0, snapped.y - 20.0);
        let view = Affine::scale(scale) * Affine::translate((-corner.x, -corner.y));
        let placed: Vec<_> = layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
        let render = |app: &DesignApp| {
            let opts = screen_options(app, &layout, false, 1.0);
            designcraft_render::Renderer::new().render(&doc, &app.session.cache, &placed, 160, 160, view, &opts)
        };
        // The snapped point is at pixel (80, 80); halfway to the next line is 18 px further.
        let darkest = |img: &designcraft_render::Rendered, xs: [u32; 2], ys: [u32; 2]| {
            (xs[0]..xs[1]).flat_map(|x| (ys[0]..ys[1]).map(move |y| (x, y))).map(|(x, y)| img.pixel(x, y)[0]).min().unwrap()
        };
        let off = render(&app);
        assert_eq!(darkest(&off, [78, 82], [60, 64]), 255, "no grid while it is hidden");
        app.run("view.documentGrid", json!({})).unwrap();
        let on = render(&app);
        assert!(darkest(&on, [79, 81], [60, 64]) < 250, "a vertical line at the snapped x");
        assert!(darkest(&on, [60, 64], [79, 81]) < 250, "a horizontal line at the snapped y");
        assert_eq!(darkest(&on, [96, 100], [96, 100]), 255, "nothing between lines");
        // Preview hides it.
        app.ui.screen_mode = ScreenMode::Preview;
        assert!(screen_options(&app, &layout, true, 1.0).document_grid.is_empty());
    }

    /// egui moves keyboard focus on a plain arrow key or Tab, and Space/Return then clicks the
    /// focused widget (e.g. the status bar's spread buttons) while typing still reaches the text.
    #[test]
    fn arrow_keys_and_tab_keep_keyboard_focus_on_the_canvas() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 3})).unwrap();
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
        app.select_tool("type");
        let mut h = harness(app);
        let p = {
            let app = h.state();
            let st = app.session.active().unwrap();
            let layout = CanvasLayout::new(&st.doc, st.editing_parents);
            let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
            xf.to_screen(layout.xf(SpreadRef::Doc(0)) * Point::new(100.0, 60.0))
        };
        h.hover_at(p);
        h.step();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        }
        h.run_steps(2);
        let focus = h.ctx.memory(|m| m.focused());
        assert!(focus.is_some(), "a click on the canvas takes keyboard focus");
        let view = *h.state().view().unwrap();
        for key in [egui::Key::ArrowDown, egui::Key::Space, egui::Key::ArrowRight, egui::Key::Enter, egui::Key::Tab, egui::Key::Escape] {
            h.key_press(key);
            if key == egui::Key::Space {
                h.event(egui::Event::Text(" ".into()));
            }
            h.run_steps(2);
            assert_eq!(h.ctx.memory(|m| m.focused()), focus, "{key:?} keeps keyboard focus on the canvas");
        }
        let v = *h.state().view().unwrap();
        assert_eq!((v.origin, v.zoom), (view.origin, view.zoom), "the view stays where it was");
        let text = h.state_mut().session.execute("story.get", &json!({"story": f["story"]})).unwrap()["text"].clone();
        assert_eq!(text, "Hello \n\t", "Space, Return and Tab reach the text");
    }

    #[test]
    fn rulers_open_no_units_menu_without_a_document() {
        let mut h = harness(DesignApp::new(designcraft_engine::Session::new(), crate::Services::default()));
        let Some(rect) = h.state().canvas_rect else { return };
        right_click(&mut h, pos2(rect.center().x, rect.min.y - RULER / 2.0));
        assert!(h.query_by_label("   Points").is_none());
    }

    /// One frame of the whole app with `events`; returns what it asks of the system IME.
    fn ime_frame(app: &mut DesignApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Option<egui::output::IMEOutput> {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        out.platform_output.ime
    }

    /// An edit patches the canvas texture in place only while that texture is at full resolution:
    /// past the GPU's texture limit `upload` shrinks it, and a full-resolution patch written into
    /// it is a renderer validation error.
    #[test]
    fn edits_patch_the_canvas_only_at_full_resolution() {
        for (max_side, patches) in [(None, true), (Some(1024), false)] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            let ctx = egui::Context::default();
            let frame = |app: &mut DesignApp| {
                // Wider than the limit (egui's font atlas needs at least 1024).
                let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1900.0, 900.0))), ..Default::default() };
                if let Some(m) = max_side {
                    raw.max_texture_side = Some(m);
                }
                let mut out = ctx.run_ui(raw, |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
                out.textures_delta.clear();
            };
            app.run("file.new", json!({})).unwrap();
            for _ in 0..3 {
                frame(&mut app);
            }
            app.run("frame.create", json!({"rect": [36, 36, 60, 60]})).unwrap();
            frame(&mut app);
            let size = app.canvas.texture.as_ref().unwrap().size();
            assert!(max_side.is_none_or(|m| size[0] <= m && size[1] <= m), "{size:?}");
            assert_eq!(app.canvas.patches > 0, patches, "limit {max_side:?}");
        }
    }

    /// The Type tool's caret at the end of a new text frame holding `text`, the app's first frames
    /// drawn; returns the story id.
    fn typing_app(app: &mut DesignApp, ctx: &egui::Context, text: &str) -> Value {
        app.run("file.new", json!({})).unwrap();
        let f = app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": text})).unwrap();
        let sid = f["story"].clone();
        app.session.execute("text.select", &json!({"story": sid, "anchor": text.len(), "focus": text.len()})).unwrap();
        app.select_tool("type");
        for _ in 0..4 {
            ime_frame(app, ctx, vec![]);
        }
        sid
    }

    fn story_text(app: &mut DesignApp, sid: &Value) -> String {
        app.session.execute("story.get", &json!({"story": sid})).unwrap()["text"].as_str().unwrap().to_string()
    }

    /// Spread point `p` on screen.
    fn on_screen(app: &DesignApp, p: Point) -> Pos2 {
        let st = app.session.active().unwrap();
        let c = CanvasLayout::new(&st.doc, st.editing_parents).to_canvas(SpreadRef::Doc(0), p);
        Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(c)
    }

    fn preedit(t: &str, c: usize) -> egui::Event {
        egui::Event::Ime(egui::ImeEvent::Preedit { text: t.into(), active_range_chars: Some(c..c) })
    }

    /// Extract `(surrounding text, cursor, anchor)` from an IME output.
    fn ime_payload(ime: &egui::output::IMEOutput) -> Option<(&str, usize, usize)> {
        ime.surrounding_text.available().map(|p| (p.text.as_str(), p.cursor, p.anchor))
    }

    /// Set an engine text selection for a test fixture.
    fn text_selection(app: &mut DesignApp, sid: &Value, anchor: usize, focus: usize) {
        app.session.execute("text.select", &json!({"story": sid, "anchor": anchor, "focus": focus})).unwrap();
    }

    struct NativeClickFixture {
        app: DesignApp,
        ctx: egui::Context,
        target: Value,
        old: Value,
        at: Pos2,
        focus: usize,
    }

    /// Build the old and target stories, measure an interior target caret, and leave the old story selected before the click.
    fn native_gui_click_fixture() -> NativeClickFixture {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let old = typing_app(&mut app, &ctx, "Old story");
        let frame = app.run("frame.create", json!({"rect": [36, 280, 300, 400], "content": "text", "text": "Target story"})).unwrap();
        let target = frame["story"].clone();
        // Measure an interior first-word caret through the engine, rather than guessing a
        // font-dependent screen pixel or clicking the frame's resize border.
        let (point, focus) = (50..180)
            .find_map(|x| {
                app.run("text.placeCaret", json!({"frame": frame["id"], "point": [x, 290]})).unwrap();
                let sel = app.session.doc().unwrap().selection.text.unwrap();
                assert_eq!(sel.story.0, target.as_u64().unwrap());
                (1..5).contains(&sel.focus).then_some((Point::new(f64::from(x), 290.0), sel.focus))
            })
            .expect("an interior point in the target's first word");
        text_selection(&mut app, &old, "Old story".len(), "Old story".len());
        for _ in 0..4 {
            ime_frame(&mut app, &ctx, vec![]);
        }
        let at = on_screen(&app, point);
        assert!(app.canvas_rect.unwrap().contains(at));
        ime_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)]);
        assert_eq!(app.session.doc().unwrap().selection.text.unwrap().story.0, old.as_u64().unwrap());
        NativeClickFixture { app, ctx, target, old, at, focus }
    }

    /// Build a primary pointer press or release event at a position.
    fn native_gui_button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    /// The 37-byte Devanagari line the native run had in the previous story.
    const NATIVE_GUI_OLD_DEVANAGARI: &str = "नमस्ते दुनिया";

    /// The click fixture with the non-empty selection in the previous story that the native
    /// Windows run had (37-byte Devanagari, 0..37). A non-empty selection shows the contextual
    /// task bar; keep it present but clear of the target click.
    fn native_gui_selected_old_click_fixture() -> NativeClickFixture {
        let mut f = native_gui_click_fixture();
        text_selection(&mut f.app, &f.old, 0, "Old story".len());
        f.app.run("text.insert", json!({"text": NATIVE_GUI_OLD_DEVANAGARI})).unwrap();
        text_selection(&mut f.app, &f.old, 0, NATIVE_GUI_OLD_DEVANAGARI.len());
        for _ in 0..4 {
            ime_frame(&mut f.app, &f.ctx, vec![]);
        }
        let sel = f.app.session.doc().unwrap().selection.text.unwrap();
        assert_eq!((sel.story.0, sel.anchor, sel.focus), (f.old.as_u64().unwrap(), 0, NATIVE_GUI_OLD_DEVANAGARI.len()));
        assert_eq!(NATIVE_GUI_OLD_DEVANAGARI.len(), 37, "the native fixture's Devanagari line was 37 UTF-8 bytes");
        assert!(!sel.is_caret(), "the previous story's selection is non-empty");
        assert!(!context_menu_open(&f.ctx), "no context menu owns this frame's keys");
        assert!(f.app.ui.dialog.is_none() && f.app.ui.palette.is_none(), "no modal owns the keyboard");
        assert!(!f.ctx.text_edit_focused(), "no text field owns the keyboard");
        assert_eq!(f.app.pane, f.app.focus_pane, "the canvas pane takes the keyboard");
        let task_bar = egui::Id::unique(("task_bar", f.ctx.viewport_id(), f.app.pane));
        let bar = f.ctx.memory(|m| m.area_rect(task_bar)).expect("the old selection shows the floating task bar");
        assert!(!bar.contains(f.at), "the floating task bar does not cover the target click at {bar:?}");
        f
    }

    /// Run one raw event batch, optionally force a discarded pass, and return the pointer edges observed by each pass.
    fn native_gui_input(f: &mut NativeClickFixture, events: Vec<egui::Event>, discard: bool) -> Vec<(bool, bool, bool)> {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut edges = Vec::new();
        let mut out = f.ctx.run_ui(raw, |ui| {
            edges.push(ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin().is_some())));
            f.app.logic(ui.ctx());
            f.app.ui(ui);
            if discard && edges.len() == 1 {
                ui.ctx().request_discard("native same-batch click exactly-once regression");
            }
        });
        if discard {
            assert_eq!(out.platform_output.num_completed_passes, 2, "the requested discarded pass actually ran");
        }
        out.textures_delta.clear();
        edges
    }

    /// Assert that the primary button, canvas press state and tool gesture are all released.
    fn native_gui_released(f: &NativeClickFixture) {
        assert!(!f.ctx.input(|i| i.pointer.primary_down()), "the native primary button is up");
        let down = f.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("canvas_pointer_down", f.app.pane)))).unwrap_or(false);
        assert!(!down, "the canvas retained a released pointer");
        assert!(!f.app.session.tool_busy(), "a released click must not leave a tool gesture");
    }

    /// Create an independently measured second-story caret farther apart than the double-click distance.
    fn native_gui_second_click_target(f: &mut NativeClickFixture) -> (Value, Pos2, usize) {
        let frame = f.app.run("frame.create", json!({"rect": [330, 280, 575, 400], "content": "text", "text": "Second story"})).unwrap();
        let story = frame["story"].clone();
        let (point, focus) = (345..465)
            .find_map(|x| {
                f.app.run("text.placeCaret", json!({"frame": frame["id"], "point": [x, 290]})).unwrap();
                let sel = f.app.session.doc().unwrap().selection.text.unwrap();
                assert_eq!(sel.story.0, story.as_u64().unwrap());
                (1..5).contains(&sel.focus).then_some((Point::new(f64::from(x), 290.0), sel.focus))
            })
            .expect("an independently measured interior caret in the second story");
        text_selection(&mut f.app, &f.old, "Old story".len(), "Old story".len());
        for _ in 0..4 {
            ime_frame(&mut f.app, &f.ctx, vec![]);
        }
        let at = on_screen(&f.app, point);
        assert!(f.app.canvas_rect.unwrap().contains(f.at));
        assert!(f.app.canvas_rect.unwrap().contains(at));
        let max_click_dist = f.ctx.options(|o| o.input_options.max_click_dist);
        assert!((at - f.at).length() > max_click_dist, "the two target presses must not form a double click");
        (story, at, focus)
    }

    /// Two completed clicks in one batch use the last press once, emit one caret/release pair, add no history, and type at the final caret.
    #[test]
    fn native_gui_two_completed_clicks_use_last_primary_press_once() {
        for reverse in [false, true] {
            for discard in [false, true] {
                let mut f = native_gui_click_fixture();
                let (second, second_at, second_focus) = native_gui_second_click_target(&mut f);
                let (first_at, last_at, final_story, final_focus) =
                    if reverse { (second_at, f.at, f.target.clone(), f.focus) } else { (f.at, second_at, second.clone(), second_focus) };
                let initial = f.app.session.doc().unwrap().selection.text.unwrap();
                assert_eq!((initial.story.0, initial.anchor, initial.focus), (f.old.as_u64().unwrap(), "Old story".len(), "Old story".len()));
                let undo = f.app.session.doc().unwrap().history.undo.len();
                let journal = f.app.session.journal.len();
                let edges = native_gui_input(
                    &mut f,
                    vec![
                        egui::Event::PointerMoved(first_at),
                        native_gui_button(first_at, true),
                        native_gui_button(first_at, false),
                        egui::Event::PointerMoved(last_at),
                        native_gui_button(last_at, true),
                        native_gui_button(last_at, false),
                    ],
                    discard,
                );
                assert_eq!(edges[0], (true, true, false), "both completed clicks leave no held origin");
                assert_eq!(edges.iter().filter(|e| e.0).count(), 1, "discard must not replay either press");
                let sel = f.app.session.doc().unwrap().selection.text.unwrap();
                assert_eq!(
                    (sel.story.0, sel.anchor, sel.focus),
                    (final_story.as_u64().unwrap(), final_focus, final_focus),
                    "reverse={reverse}, discard={discard}"
                );
                let commands = &f.app.session.journal[journal..];
                assert_eq!(
                    commands.iter().filter(|(id, _)| id == "text.placeCaret").count(),
                    1,
                    "one aggregate canvas selection, reverse={reverse}, discard={discard}"
                );
                assert_eq!(
                    commands.iter().filter(|(id, _)| id == "text.release").count(),
                    1,
                    "one corresponding release, reverse={reverse}, discard={discard}"
                );
                let clicks =
                    f.ctx.data(|d| d.get_temp::<Clicks>(egui::Id::unique(("canvas_clicks", f.app.pane)))).expect("one recovered canvas press");
                assert_eq!(clicks.n, 1, "separated clicks and discarded passes must not select a word");
                assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo, "pointer selection adds no document history");
                native_gui_released(&f);
                native_gui_input(&mut f, vec![egui::Event::Text("!".into())], false);
                let mut target_text = "Target story".to_owned();
                let mut second_text = "Second story".to_owned();
                if reverse {
                    target_text.insert(final_focus, '!');
                } else {
                    second_text.insert(final_focus, '!');
                }
                assert_eq!(story_text(&mut f.app, &f.target), target_text);
                assert_eq!(story_text(&mut f.app, &second), second_text);
                assert_eq!(story_text(&mut f.app, &f.old), "Old story");
                assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo + 1, "typing at the final caret is one edit");
                native_gui_released(&f);
            }
        }
    }

    /// A non-finite final press rejects the click instead of reviving an earlier valid press; later motion cannot drag.
    #[test]
    fn native_gui_nonfinite_final_press_cannot_reuse_an_earlier_click() {
        for discard in [false, true] {
            let mut f = native_gui_click_fixture();
            let at = f.at;
            let initial = f.app.session.doc().unwrap().selection.text;
            let undo = f.app.session.doc().unwrap().history.undo.len();
            let journal = f.app.session.journal.len();
            let edges = native_gui_input(
                &mut f,
                vec![
                    egui::Event::PointerMoved(at),
                    native_gui_button(at, true),
                    native_gui_button(at, false),
                    native_gui_button(pos2(f32::NAN, at.y), true),
                    egui::Event::PointerMoved(at),
                    native_gui_button(at, false),
                ],
                discard,
            );
            assert_eq!(edges[0], (true, true, false));
            assert_eq!(edges.iter().filter(|e| e.0).count(), 1, "discard must not replay malformed input");
            assert_eq!(
                f.app.session.doc().unwrap().selection.text,
                initial,
                "discard={discard}: reject the last press instead of reviving an earlier valid one"
            );
            assert!(f.app.session.journal[journal..].iter().all(|(id, _)| id != "text.placeCaret" && id != "text.release"));
            assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo);
            assert_eq!(story_text(&mut f.app, &f.target), "Target story");
            assert_eq!(story_text(&mut f.app, &f.old), "Old story");
            native_gui_released(&f);
            native_gui_input(&mut f, vec![egui::Event::PointerMoved(at + vec2(90.0, 0.0))], false);
            assert_eq!(f.app.session.doc().unwrap().selection.text, initial, "motion after the invalid released press cannot drag");
            native_gui_released(&f);
        }
    }

    /// A same-batch click switches story exactly once; typing, undo and redo affect only the target story, including discarded-pass runs.
    #[test]
    fn native_gui_same_batch_click_switches_story_once_before_typing() {
        for discard in [false, true] {
            let mut f = native_gui_click_fixture();
            let undo = f.app.session.doc().unwrap().history.undo.len();
            let at = f.at;
            let edges =
                native_gui_input(&mut f, vec![egui::Event::PointerMoved(at), native_gui_button(at, true), native_gui_button(at, false)], discard);
            assert_eq!(edges[0], (true, true, false), "the real input consumed both edges and cleared the origin");
            assert_eq!(edges.iter().filter(|e| e.0).count(), 1, "a discarded pass must not replay the press");
            let selection = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!((selection.story.0, selection.anchor, selection.focus), (f.target.as_u64().unwrap(), f.focus, f.focus), "discard={discard}");
            let clicks = f.ctx.data(|d| d.get_temp::<Clicks>(egui::Id::unique(("canvas_clicks", f.app.pane)))).expect("one actual canvas click");
            assert_eq!(clicks.n, 1, "discard={discard}: replay must not turn a caret into word selection");
            native_gui_released(&f);
            assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo, "a click is not a document edit");
            let end = |pressed| egui::Event::Key { key: egui::Key::End, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
            native_gui_input(&mut f, vec![end(true), end(false)], false);
            let caret = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!((caret.story.0, caret.focus), (f.target.as_u64().unwrap(), "Target story".len()));
            native_gui_input(&mut f, vec![egui::Event::Text("!".into())], false);
            assert_eq!(story_text(&mut f.app, &f.target), "Target story!");
            assert_eq!(story_text(&mut f.app, &f.old), "Old story");
            assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo + 1, "one typed edit, including discarded-pass control");
            f.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(story_text(&mut f.app, &f.target), "Target story");
            assert_eq!(story_text(&mut f.app, &f.old), "Old story");
            f.app.run("edit.redo", json!({})).unwrap();
            assert_eq!(story_text(&mut f.app, &f.target), "Target story!");
            assert_eq!(story_text(&mut f.app, &f.old), "Old story");
            native_gui_released(&f);
        }
    }

    /// The native Windows failure: with a non-empty selection in another story, a click that
    /// switches stories followed by End in the same job left the new story's caret at the click
    /// point. End in a later job worked. Drive the actual app UI and actual pointer/key events in
    /// one batch, in split batches, and with End while the press is still held; discard a pass to
    /// cover replay. The End press must reach the type tool once and win over the click, wherever
    /// the pointer release falls, and typing must go there once.
    #[test]
    fn native_gui_click_then_end_in_one_batch_keeps_the_target_caret_at_the_story_end() {
        let names = ["same batch, click events first", "click batch then End batch", "End while the press is held"];
        for (layout, name) in names.into_iter().enumerate() {
            for discard in [false, true] {
                let mut f = native_gui_selected_old_click_fixture();
                let (at, target, old) = (f.at, f.target.clone(), f.old.clone());
                let undo = f.app.session.doc().unwrap().history.undo.len();
                let journal = f.app.session.journal.len();
                let moved = || egui::Event::PointerMoved(at);
                let down = || native_gui_button(at, true);
                let up = || native_gui_button(at, false);
                let end =
                    |pressed| egui::Event::Key { key: egui::Key::End, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
                let batches: Vec<Vec<egui::Event>> = match layout {
                    0 => vec![vec![moved(), down(), up(), end(true), end(false)]],
                    1 => vec![vec![moved(), down(), up()], vec![end(true), end(false)]],
                    _ => vec![vec![moved(), down()], vec![end(true)], vec![up(), end(false)]],
                };
                for events in batches {
                    native_gui_input(&mut f, events, discard);
                }
                let selection = f.app.session.doc().unwrap().selection.text.unwrap();
                assert_eq!(
                    (selection.story.0, selection.anchor, selection.focus),
                    (target.as_u64().unwrap(), "Target story".len(), "Target story".len()),
                    "{name}, discard={discard}: End must win after the click, wherever the release falls"
                );
                let commands = &f.app.session.journal[journal..];
                assert_eq!(
                    commands.iter().filter(|(id, _)| id == "text.move").count(),
                    1,
                    "{name}, discard={discard}: the End press reached the type tool once"
                );
                let clicks = f.ctx.data(|d| d.get_temp::<Clicks>(egui::Id::unique(("canvas_clicks", f.app.pane)))).expect("one actual canvas click");
                assert_eq!(clicks.n, 1, "{name}, discard={discard}: a discarded pass must not replay the press as a word selection");
                native_gui_released(&f);
                native_gui_input(&mut f, vec![egui::Event::Text("!".into())], discard);
                assert_eq!(story_text(&mut f.app, &target), "Target story!", "{name}, discard={discard}: typing lands at the End caret once");
                assert_eq!(story_text(&mut f.app, &old), NATIVE_GUI_OLD_DEVANAGARI);
                assert_eq!(
                    f.app.session.journal[journal..].iter().filter(|(id, _)| id == "text.insert").count(),
                    1,
                    "{name}, discard={discard}: the text event is not replayed"
                );
                assert_eq!(f.app.session.doc().unwrap().history.undo.len(), undo + 1, "{name}, discard={discard}: one edit");
            }
        }
    }

    /// Pointer ownership survives a split press/move/release; a real drag selects, release ends it, and later motion cannot continue it.
    #[test]
    fn native_gui_split_click_and_drag_release_keep_pointer_ownership() {
        for drag in [false, true] {
            let mut f = native_gui_click_fixture();
            let at = f.at;
            native_gui_input(&mut f, vec![native_gui_button(at, true)], false);
            let sel = f.app.session.doc().unwrap().selection.text.unwrap();
            assert_eq!((sel.story.0, sel.focus), (f.target.as_u64().unwrap(), f.focus));
            assert!(f.ctx.input(|i| i.pointer.primary_down()));
            assert!(f.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("canvas_pointer_down", f.app.pane)))).unwrap());
            if drag {
                let moved = at + vec2(60.0, 0.0);
                native_gui_input(&mut f, vec![egui::Event::PointerMoved(moved)], false);
                let sel = f.app.session.doc().unwrap().selection.text.unwrap();
                assert_eq!(sel.story.0, f.target.as_u64().unwrap());
                assert!(!sel.is_caret(), "a real held drag selects text");
            }
            let release = if drag { pos2(-20.0, at.y) } else { at };
            native_gui_input(&mut f, vec![egui::Event::PointerMoved(release), native_gui_button(release, false)], false);
            native_gui_released(&f);
            let selection = f.app.session.doc().unwrap().selection.text;
            native_gui_input(&mut f, vec![egui::Event::PointerMoved(at + vec2(90.0, 0.0))], false);
            assert_eq!(f.app.session.doc().unwrap().selection.text, selection, "motion after release cannot continue the old drag");
            assert_eq!(story_text(&mut f.app, &f.target), "Target story");
            assert_eq!(story_text(&mut f.app, &f.old), "Old story");
        }
    }

    /// An unowned outside press, and a press that dismisses an open menu, cannot move the selection or edit either story.
    #[test]
    fn native_gui_outside_press_and_open_menu_do_not_click_through() {
        for menu in [false, true] {
            let mut f = native_gui_click_fixture();
            let at = f.at;
            let selection = f.app.session.doc().unwrap().selection.text;
            if menu {
                let blank = f.app.canvas_rect.unwrap().max - vec2(30.0, 30.0);
                for pressed in [true, false] {
                    let event =
                        egui::Event::PointerButton { pos: blank, button: egui::PointerButton::Secondary, pressed, modifiers: Default::default() };
                    native_gui_input(&mut f, vec![egui::Event::PointerMoved(blank), event], false);
                }
                assert!(context_menu_open(&f.ctx), "the actual secondary click opened the canvas menu");
                assert_eq!(f.app.session.doc().unwrap().selection.text, selection);
                native_gui_input(&mut f, vec![egui::Event::PointerMoved(at), native_gui_button(at, true), native_gui_button(at, false)], false);
            } else {
                let outside = pos2(-20.0, -20.0);
                assert!(!f.app.canvas_rect.unwrap().contains(outside));
                native_gui_input(
                    &mut f,
                    vec![
                        egui::Event::PointerMoved(outside),
                        native_gui_button(outside, true),
                        egui::Event::PointerMoved(at),
                        native_gui_button(at, false),
                    ],
                    false,
                );
            }
            assert_eq!(f.app.session.doc().unwrap().selection.text, selection, "menu={menu}: an unowned press cannot move the old caret");
            assert_eq!(story_text(&mut f.app, &f.target), "Target story");
            assert_eq!(story_text(&mut f.app, &f.old), "Old story");
            native_gui_released(&f);
        }
    }

    /// Font inventory distinguishes exact bundled faces, missing styles and missing families without accepting installed or injected substitutes.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_gui_font_inventory_keeps_exact_regular_and_missing_style_diagnostics() {
        if designcraft_fonts::CRAFT_FONTS.is_empty() {
            assert!(std::env::var_os("CRAFT_FONTS_REQUIRED").is_none(), "required canonical fonts were not embedded");
            return; // The supported optional-input build has no central document fonts.
        }
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        for (family, text) in [("Noto Sans Hebrew", "שלום"), ("Noto Sans Devanagari", "नमस्ते")] {
            assert!(designcraft_fonts::CRAFT_FONTS.iter().any(|f| f.family == family && f.style == "Regular"), "canonical {family} Regular absent");
            for style in ["Regular", "Unavailable Native GUI Style"] {
                let frame = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": text})).unwrap();
                app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": text.len()})).unwrap();
                app.run("type.char", json!({"fontFamily": family, "fontStyle": style})).unwrap();
            }
        }
        let frame = app.run("frame.create", json!({"rect": [36, 36, 300, 120], "content": "text", "text": "Missing"})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 7})).unwrap();
        app.run("type.char", json!({"fontFamily": "Unavailable Native GUI Family", "fontStyle": "Regular"})).unwrap();
        let inventory = app.run("font.list", json!({})).unwrap();
        let rows = inventory.as_array().unwrap();
        assert_eq!(rows.len(), 5, "{inventory}");
        for family in ["Noto Sans Hebrew", "Noto Sans Devanagari"] {
            for style in ["Regular", "Unavailable Native GUI Style"] {
                let row = rows.iter().find(|r| r["family"] == family && r["style"] == style).unwrap();
                assert_eq!(row["missing"], false, "{row}");
                assert_eq!(row["styleMissing"], style != "Regular", "{row}");
                assert_eq!(row["matchStatus"], if style == "Regular" { "exact" } else { "styleSubstitute" }, "{row}");
                assert_eq!(row["resolvedFamily"], family, "{row}");
                assert_eq!(row["resolvedStyle"], "Regular", "{row}");
                assert_eq!(row["source"], "bundled", "an installed or injected face cannot satisfy this production contract: {row}");
            }
        }
        let missing = rows.iter().find(|r| r["family"] == "Unavailable Native GUI Family").unwrap();
        assert_eq!(missing["missing"], true, "{missing}");
        assert_eq!(missing["matchStatus"], "missing", "{missing}");
        assert!(missing["source"].is_null(), "{missing}");
    }

    #[test]
    #[allow(deprecated)] // `ImeEvent::Enabled` / `Disabled`: egui no longer sends them, other integrations may.
    fn ime_events_compose_in_the_story_with_the_candidate_window_at_the_caret() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "曲:");
        // At a text caret the IME is allowed, its window on the caret as the canvas draws it.
        let ime = ime_frame(&mut app, &ctx, vec![]).expect("IME allowed at a text caret");
        let (top, bottom) = {
            let st = app.session.active().unwrap();
            let cs = app.session.cache.get(&st.doc, designcraft_doc::StoryId(sid.as_u64().unwrap()), None);
            let (fi, x, bl, asc, desc) = compose::caret(&cs, "曲:".len()).unwrap();
            let fid = cs.frames[fi].frame;
            let (a, _) = item_canvas_xf(&st.doc, &CanvasLayout::new(&st.doc, st.editing_parents), fid).unwrap();
            let m = a * st.doc.text_xf(st.doc.item(fid).unwrap());
            let xf = Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
            (xf.to_screen(m * Point::new(x, bl - asc)), xf.to_screen(m * Point::new(x, bl + desc)))
        };
        assert!(ime.rect.expand(1.0).contains(top) && ime.rect.expand(1.0).contains(bottom), "{:?} vs {top:?}–{bottom:?}", ime.rect);
        assert_eq!(ime.cursor_rect, ime.rect);
        assert!(!ime.should_interrupt_composition);
        assert!(app.canvas_ime(), "the web build keeps whole words typed at the caret");
        // "ni" → に → 日, with the events winit sends.
        ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Enabled), preedit("ni", 2)]);
        assert_eq!(story_text(&mut app, &sid), "曲:ni", "marked text shows in place");
        assert!(app.session.tool_composing());
        // Keys, plain text, the clipboard and Undo (the native menu's ⌘Z) wait for the IME.
        let steps = app.session.doc().unwrap().history.undo.len();
        let key = |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        ime_frame(
            &mut app,
            &ctx,
            vec![key(egui::Key::Backspace, true), key(egui::Key::Backspace, false), egui::Event::Text("x".into()), egui::Event::Paste("y".into())],
        );
        crate::menus::activate(&mut app, "edit.undo", &Value::Null);
        assert!(!crate::menus::enabled(&app, "edit.undo"));
        assert_eq!(story_text(&mut app, &sid), "曲:ni");
        let ime2 = ime_frame(&mut app, &ctx, vec![preedit("に", 1)]).expect("IME allowed while composing");
        assert_eq!(story_text(&mut app, &sid), "曲:に");
        assert!(ime2.rect.center().x > ime.rect.center().x, "the window follows the IME's cursor: {:?} {:?}", ime2.rect, ime.rect);
        ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into())), egui::Event::Ime(egui::ImeEvent::Disabled)]);
        assert_eq!(story_text(&mut app, &sid), "曲:日");
        assert!(!app.session.tool_composing());
        assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "the commit is one undo step");
        assert_eq!(app.session.journal.last(), Some(&("text.insert".to_string(), json!({"text": "日"}))));
        // After the commit, typing and Undo work again.
        ime_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]);
        assert_eq!(story_text(&mut app, &sid), "曲:日!");
        crate::menus::activate(&mut app, "edit.undo", &Value::Null);
        crate::menus::activate(&mut app, "edit.undo", &Value::Null);
        assert_eq!(story_text(&mut app, &sid), "曲:");
        // Without a text caret the IME is off: single-key tool shortcuts keep working.
        app.select_tool("selection");
        assert!(ime_frame(&mut app, &ctx, vec![]).is_none());
        assert!(!app.canvas_ime());
    }

    /// Both panes of a split window see the same keyboard events: only the pane last clicked
    /// types them.
    #[test]
    fn a_split_window_types_once() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "ab");
        app.run("window.split", json!({"on": true})).unwrap();
        for _ in 0..3 {
            ime_frame(&mut app, &ctx, vec![]);
        }
        assert!(app.other_pane.as_ref().and_then(|o| o.1).is_some(), "both panes are drawn");
        ime_frame(&mut app, &ctx, vec![egui::Event::Text("c".into())]);
        assert_eq!(story_text(&mut app, &sid), "abc");
        let key = |pressed| egui::Event::Key { key: egui::Key::Backspace, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        ime_frame(&mut app, &ctx, vec![key(true), key(false)]);
        assert_eq!(story_text(&mut app, &sid), "ab");
        ime_frame(&mut app, &ctx, vec![egui::Event::Paste("de".into())]);
        assert_eq!(story_text(&mut app, &sid), "abde");
        ime_frame(&mut app, &ctx, vec![preedit("に", 1)]);
        assert_eq!(story_text(&mut app, &sid), "abdeに");
        ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))]);
        assert_eq!(story_text(&mut app, &sid), "abde日");
    }

    /// Surrounding text is committed text only, preserves reversed selection direction, and a commit keeps owner/generation while reporting `InputMethod`.
    #[test]
    fn ime_reports_the_committed_story_and_excludes_the_pre_edit() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "a😀b");
        let len = "a😀b".len();
        text_selection(&mut app, &sid, 0, len);
        let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        assert_eq!(ime_payload(&ime), Some(("a😀b", len, 0)), "reversed selection keeps its direction");
        let owner = (ime.owner, ime.context_generation);
        // Marked text previews in the document but is not committed surrounding text.
        let ime = ime_frame(&mut app, &ctx, vec![preedit("に", 1)]).expect("a text caret");
        assert_eq!(ime_payload(&ime), Some(("a😀b", len, 0)));
        assert_eq!(ime.owner, owner.0);
        let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))]).expect("a text caret");
        assert_eq!(ime_payload(&ime), Some(("日", 3, 3)));
        assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
        assert_eq!(story_text(&mut app, &sid), "日");
        assert_eq!(ime.owner, owner.0);
        assert_eq!(ime.context_generation, owner.1, "a commit is not a new context");
        let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        assert_eq!(ime_payload(&ime), Some(("日", 3, 3)), "a later pass reports the same committed text");
        assert_eq!((ime.owner, ime.context_generation), owner, "an unchanged pass keeps the context");
    }

    /// An empty story provides `("",0,0)`; long text is bounded and an out-of-window selection is explicitly unavailable.
    #[test]
    fn ime_empty_story_is_available_and_wide_text_is_bounded() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        typing_app(&mut app, &ctx, "");
        let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        assert_eq!(ime_payload(&ime), Some(("", 0, 0)), "an empty document is available, not unavailable");
        assert!(!app.session.tool_composing());

        let long = "a".repeat(5000);
        let sid = typing_app(&mut app, &ctx, &long);
        let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        let (text, _, _) = ime_payload(&ime).expect("a long document still has surrounding text");
        assert!(text.len() <= egui::output::MAX_IME_SURROUNDING_TEXT_BYTES, "{}", text.len());
        text_selection(&mut app, &sid, 0, 5000);
        let ime = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        assert!(ime.surrounding_text.available().is_none(), "a selection beyond the bound is explicitly unavailable");
    }

    /// Caret movement preserves owner; changing story or document changes owner and generation.
    #[test]
    fn ime_owner_follows_the_story_and_the_document() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        typing_app(&mut app, &ctx, "ab");
        let first = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        app.session.execute("text.move", &json!({"dir": "left"})).unwrap();
        let moved = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        assert_eq!((moved.owner, moved.context_generation), (first.owner, first.context_generation), "a caret move keeps the context");

        let other = app.run("frame.create", json!({"rect": [400, 36, 200, 100], "content": "text", "text": "xy"})).unwrap();
        let other_sid = other["story"].clone();
        text_selection(&mut app, &other_sid, 2, 2);
        let second = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        assert_ne!(second.owner, first.owner, "another story is another owner");
        assert_ne!(second.context_generation, first.context_generation);

        app.run("file.new", json!({})).unwrap();
        let doc = app.run("frame.create", json!({"rect": [36, 36, 200, 100], "content": "text", "text": "z"})).unwrap();
        text_selection(&mut app, &doc["story"], 1, 1);
        let replaced = ime_frame(&mut app, &ctx, vec![]).expect("a text caret");
        assert_ne!(replaced.owner, second.owner, "a new document is another owner");
        assert_eq!(ime_payload(&replaced), Some(("z", 1, 1)));
    }

    /// Pure commits report `InputMethod`; typing before or after a commit in one pass makes the cause `Other`.
    #[test]
    fn ime_cause_distinguishes_commits_from_application_edits() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        typing_app(&mut app, &ctx, "a");
        let committed = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))]).expect("a text caret");
        assert_eq!(committed.change_cause, egui::output::ImeChangeCause::InputMethod);
        let typed = ime_frame(&mut app, &ctx, vec![egui::Event::Text("!".into())]).expect("a text caret");
        assert_eq!(typed.change_cause, egui::output::ImeChangeCause::Other, "ordinary typing is an application edit");
        assert_eq!(ime_payload(&typed), Some(("a日!", 5, 5)));

        // An application edit before the commit in one pass breaks the receipt chain.
        let mut before = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        typing_app(&mut before, &ctx, "a");
        let mixed = ime_frame(&mut before, &ctx, vec![egui::Event::Text("x".into()), egui::Event::Ime(egui::ImeEvent::Commit("日".into()))])
            .expect("a text caret");
        assert_eq!(mixed.change_cause, egui::output::ImeChangeCause::Other);
        assert_eq!(ime_payload(&mixed), Some(("ax日", 5, 5)));

        // An application edit after the commit in one pass also dominates.
        let mut after = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        typing_app(&mut after, &ctx, "a");
        let mixed = ime_frame(&mut after, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into())), egui::Event::Text("!".into())])
            .expect("a text caret");
        assert_eq!(mixed.change_cause, egui::output::ImeChangeCause::Other);
        assert_eq!(ime_payload(&mixed), Some(("a日!", 5, 5)));
    }

    /// The settled multipass result rebuilds the current surrounding text while retaining the accepted commit cause.
    #[test]
    fn ime_cause_and_context_survive_a_discarded_pass() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        typing_app(&mut app, &ctx, "a");
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            events: vec![egui::Event::Ime(egui::ImeEvent::Commit("日".into()))],
            ..Default::default()
        };
        let mut passes = 0;
        let mut out = ctx.run_ui(raw, |ui| {
            passes += 1;
            if passes == 1 {
                ui.ctx().request_discard("IME multipass regression");
            }
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        assert_eq!(out.platform_output.num_completed_passes, 2, "the frame really was multipass");
        let ime = out.platform_output.ime.expect("a text caret");
        assert_eq!(ime_payload(&ime), Some(("a日", 4, 4)), "the second pass rebuilds the current text");
        assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod, "the discarded pass keeps the accepted commit's cause");
    }

    /// Autocorrected commit text is an application edit with cause `Other`.
    #[test]
    fn ime_autocorrect_is_not_an_input_method_cause() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        app.session.prefs.autocorrect = true;
        let sid = typing_app(&mut app, &ctx, "teh");
        let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit(" ".into()))]).expect("a text caret");
        assert_eq!(story_text(&mut app, &sid), "the ");
        assert_eq!(ime.change_cause, egui::output::ImeChangeCause::Other);
    }

    /// The selection stays selected and directed; byte partials leave whole characters; the deletion is one undo step.
    #[test]
    fn ime_delete_surrounding_excludes_the_selection_and_rounds_bytes_inward() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "aé😀b");
        // Select é, which keeps its bytes and direction; 'a' before and 😀 after go.
        text_selection(&mut app, &sid, 1, 3);
        ime_frame(&mut app, &ctx, vec![]);
        let steps = app.session.doc().unwrap().history.undo.len();
        let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 2, after_bytes: 4 })])
            .expect("a text caret");
        assert_eq!(story_text(&mut app, &sid), "éb");
        let sel = app.session.doc().unwrap().selection.text.unwrap();
        assert_eq!((sel.anchor, sel.focus), (0, 2), "the selected text stays selected and directed");
        assert_eq!(ime_payload(&ime), Some(("éb", 2, 0)));
        assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "one deletion is one undo step");
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(story_text(&mut app, &sid), "aé😀b");
        ime_frame(&mut app, &ctx, vec![]);
        // The same deletion with a reversed selection keeps anchor > focus.
        text_selection(&mut app, &sid, 3, 1);
        ime_frame(&mut app, &ctx, vec![]);
        let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 2, after_bytes: 4 })])
            .expect("a text caret");
        assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
        let sel = app.session.doc().unwrap().selection.text.unwrap();
        assert_eq!(story_text(&mut app, &sid), "éb");
        assert_eq!((sel.anchor, sel.focus), (2, 0), "reversed selection stays reversed");

        // One byte of a two-byte character leaves the whole character.
        let partial = typing_app(&mut app, &ctx, "aé");
        text_selection(&mut app, &partial, 1, 1);
        ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 0, after_bytes: 1 })]);
        assert_eq!(story_text(&mut app, &partial), "aé");
        // A preceding count that lands inside the four-byte emoji leaves it whole too.
        let partial = typing_app(&mut app, &ctx, "a😀");
        ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 2, after_bytes: 0 })]);
        assert_eq!(story_text(&mut app, &partial), "a😀");
    }

    /// A 200-character request deletes the whole 100-character story.
    #[test]
    fn ime_delete_surrounding_has_no_sixty_four_character_cap() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, &"x".repeat(100));
        ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars: 200, after_chars: 0 })]);
        assert_eq!(story_text(&mut app, &sid), "");
    }

    /// Delete plus commit in one IME batch produces one undoable edit and reports the final surrounding text.
    #[test]
    fn ime_delete_and_commit_are_one_undo_step() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "ab");
        text_selection(&mut app, &sid, 1, 2);
        ime_frame(&mut app, &ctx, vec![]);
        let steps = app.session.doc().unwrap().history.undo.len();
        let ime = ime_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars: 1, after_chars: 0 }),
                egui::Event::Ime(egui::ImeEvent::Commit("X".into())),
            ],
        )
        .expect("a text caret");
        assert_eq!(story_text(&mut app, &sid), "X");
        assert_eq!(ime_payload(&ime), Some(("X", 1, 1)));
        assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "the IME batch is one undo step");
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(story_text(&mut app, &sid), "ab");
    }

    /// The input method's Commit for a composition a click ended earlier in the same event batch
    /// must not type the marked text again at the clicked caret.
    #[test]
    fn a_commit_after_a_click_in_the_same_batch_is_not_typed_twice() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "ab");
        ime_frame(&mut app, &ctx, vec![preedit("い", 1)]);
        assert_eq!(story_text(&mut app, &sid), "abい");
        let p = on_screen(&app, Point::new(150.0, 120.0));
        let press = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        ime_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p), press(true), egui::Event::Ime(egui::ImeEvent::Commit("い".into()))])
            .expect("still at a text caret");
        let typed = story_text(&mut app, &sid);
        assert_eq!(typed.matches('い').count(), 1, "the click typed the marked text once: {typed:?}");
        assert!(typed.starts_with("ab"), "{typed:?}");
        assert!(!app.session.tool_composing());
        assert!(app.take_ime_discard(), "the system IME is told to drop its copy");
        ime_frame(&mut app, &ctx, vec![press(false)]);
    }

    /// A delete-surrounding and its commit from the same ended composition must not edit the
    /// clicked caret's text either.
    #[test]
    fn a_delete_and_commit_after_a_click_in_the_same_batch_are_dropped() {
        for delete in [
            egui::ImeEvent::DeleteSurrounding { before_chars: 1, after_chars: 0 },
            egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 1, after_bytes: 0 },
        ] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            let ctx = egui::Context::default();
            let sid = typing_app(&mut app, &ctx, "ab");
            ime_frame(&mut app, &ctx, vec![preedit("い", 1)]);
            let p = on_screen(&app, Point::new(150.0, 120.0));
            let press = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            ime_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(p), press(true), egui::Event::Ime(delete), egui::Event::Ime(egui::ImeEvent::Commit("い".into()))],
            )
            .expect("still at a text caret");
            let typed = story_text(&mut app, &sid);
            assert_eq!(typed.matches('い').count(), 1, "the late commit is dropped: {typed:?}");
            assert!(typed.contains("ab"), "the click's text was not deleted: {typed:?}");
            assert!(app.take_ime_discard());
            ime_frame(&mut app, &ctx, vec![press(false)]);
        }
    }

    /// A byte-count delete that reaches past the bounded IME window is still exactly the requested
    /// input method mutation: the engine reports the ranges from the whole story.
    #[test]
    fn a_delete_beyond_the_surrounding_window_is_still_an_input_method_cause() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, &"x".repeat(5000));
        let ime = ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::DeleteSurroundingBytes { before_bytes: 4500, after_bytes: 0 })])
            .expect("a text caret");
        assert_eq!(story_text(&mut app, &sid).len(), 500);
        assert_eq!(ime.change_cause, egui::output::ImeChangeCause::InputMethod);
        assert_eq!(ime_payload(&ime).map(|(text, cursor, anchor)| (text.len(), cursor, anchor)), Some((500, 500, 500)));
    }

    /// The pass's IME observation comes after the later UI sections: a click into the Story
    /// Editor's field takes the keyboard from the canvas in the same pass, so the composition
    /// ends there and the canvas reports no IME.
    #[test]
    fn a_later_field_taking_the_keyboard_ends_the_composition_in_that_pass() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "ab"})).unwrap();
        let sid = f["story"].clone();
        app.select_tool("type");
        app.run("text.select", json!({"story": sid, "anchor": 2, "focus": 2})).unwrap();
        app.run("app.storyEditor", json!({"story": sid})).unwrap();
        let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
        h.event(preedit("い", 1));
        h.run_steps(2);
        assert!(h.state().app.session.tool_composing());
        let canvas_owner = h.output().platform_output.ime.as_ref().expect("the canvas owns the composition").owner;
        let field = h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).next().expect("the Story Editor's text field");
        field.click();
        h.run_steps(2);
        assert!(!h.state().app.session.tool_composing(), "the composition ended where the field took the keyboard");
        assert!(h.state_mut().app.take_ime_discard());
        assert!(h.get_all_by_role(egui::accesskit::Role::MultilineTextInput).next().unwrap().is_focused(), "the field has the keyboard");
        let ime = h.output().platform_output.ime.as_ref().expect("the focused Story Editor publishes its IME context");
        assert_ne!(ime.owner, canvas_owner, "the field owns the IME, not the canvas");
        let typed = story_text(&mut h.state_mut().app, &sid);
        assert!(typed.contains('い'), "the marked text stays as typed: {typed:?}");
        h.event(egui::Event::Ime(egui::ImeEvent::Commit("X".into())));
        h.run_steps(2);
        let edited = story_text(&mut h.state_mut().app, &sid);
        assert_eq!(edited.matches('X').count(), 1, "the field's next commit is applied once: {edited:?}");
        assert_eq!(edited.matches('い').count(), 1, "the old canvas composition is not replayed: {edited:?}");
    }

    #[test]
    fn clicking_away_mid_composition_keeps_the_text_and_interrupts_the_ime() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let sid = typing_app(&mut app, &ctx, "");
        ime_frame(&mut app, &ctx, vec![preedit("しょうこ", 4)]);
        assert!(app.session.tool_composing());
        // A click in the frame (macOS and Windows send nothing for it): the marked text stays as
        // typed and the IME is told to drop its composition, once.
        let p = on_screen(&app, Point::new(150.0, 120.0));
        let press = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let ime = ime_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p), press(true)]).expect("still at a text caret");
        assert!(ime.should_interrupt_composition);
        ime_frame(&mut app, &ctx, vec![press(false)]);
        assert!(!app.session.tool_composing());
        assert_eq!(story_text(&mut app, &sid), "しょうこ");
        assert!(app.take_ime_discard(), "the host tells the system IME to drop its marked text");
        assert!(!ime_frame(&mut app, &ctx, vec![]).expect("still at a text caret").should_interrupt_composition);
        assert!(!app.take_ime_discard());
        // A bare line break from the IME confirms a composition: the marked text, no new paragraph.
        ime_frame(&mut app, &ctx, vec![preedit("がく", 2)]);
        ime_frame(&mut app, &ctx, vec![egui::Event::Ime(egui::ImeEvent::Commit("\n".into()))]);
        assert!(!app.session.tool_composing());
        let typed = story_text(&mut app, &sid);
        assert!(typed.contains("がく") && typed.contains("しょうこ") && !typed.contains('\n'), "{typed:?}");
    }

    #[test]
    fn baseline_grid_lines_are_the_ones_on_the_page() {
        // A start far above the page used to be walked line by line, without end.
        for start in [-1e9, -1e17] {
            let mut far = baseline_grid_lines(start, 12.0, 792.0);
            assert_eq!((far.next(), far.count()), (Some(8.0), 65));
        }
        let lines = |start, increment, height| baseline_grid_lines(start, increment, height).collect::<Vec<_>>();
        assert_eq!(lines(36.0, 12.0, 72.0), [36.0, 48.0, 60.0], "the bottom edge is not a line");
        assert_eq!(lines(-30.0, 12.0, 30.0), [6.0, 18.0], "a start above the page keeps its rhythm");
        assert_eq!(lines(f64::NAN, 12.0, 792.0), [0.0; 0]);
        assert_eq!(baseline_grid_lines(0.0, 12.0, f64::INFINITY).count(), MAX_BASELINES);
    }

    #[test]
    fn enter_or_tab_in_a_panel_field_leave_the_selected_text_alone() {
        for key in [egui::Key::Enter, egui::Key::Tab] {
            let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Hello"})).unwrap();
            app.select_tool("type");
            app.run("text.select", json!({"story": f["story"], "anchor": 1, "focus": 4})).unwrap();
            assert!(app.session.wants_text());
            let mut h = harness(app);
            let size = h.get_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.value().as_deref() == Some("12 pt")).unwrap();
            size.focus();
            h.run_steps(1);
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            h.run_steps(1);
            h.event(egui::Event::Text("18".into()));
            h.run_steps(1);
            h.key_press(key);
            h.run_steps(3);
            let text = h.state_mut().session.execute("story.get", &json!({"story": f["story"]})).unwrap()["text"].clone();
            assert_eq!(text, "Hello", "{key:?} confirms the field and leaves the story alone");
            let size = crate::panels::text_attrs(h.state_mut()).unwrap()["chars"]["size"].clone();
            assert_eq!(size, 18.0, "{key:?} applies the typed size");
        }
    }

    #[test]
    fn trailing_spaces_show_dots_before_the_paragraph_mark() {
        let text = "ab  \ncd  ";
        let mut d = Document::new(&designcraft_doc::build::NewDocument::default());
        let lid = d.default_layer();
        let rect = DRect::new(0.0, 0.0, 300.0, 100.0);
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), rect, lid, text, Default::default()).unwrap();
        let cs = compose::compose_story(&d, sid, &compose::ComposeOptions::default());
        let lines: Vec<_> = cs.frames.iter().flat_map(|f| &f.lines).collect();
        assert_eq!(lines.len(), 2);
        for (l, end_mark) in lines.iter().zip(["¶", "#"]) {
            let marks = hidden_marks(text, l);
            let dots: Vec<f64> = marks.iter().filter(|m| m.1 == "·").map(|m| m.0).collect();
            assert_eq!(dots.len(), 2, "a dot for each trailing space: {marks:?}");
            let &(para_x, mark, _) = marks.last().unwrap();
            assert_eq!(mark, end_mark);
            // The dots sit past the last letter, and the mark comes after both spaces.
            let space = l.glyphs.iter().rev().find(|g| g.len > 0).unwrap();
            assert!(dots.iter().all(|&x| x > l.end_x && x < para_x), "{dots:?} then {mark} at {para_x}");
            assert!(para_x >= space.x + space.adv, "{mark} at {para_x} overlaps the last space");
        }
    }
}

#[cfg(test)]
mod pencil_preview_tests {
    use egui::{Pos2, pos2, vec2};

    /// Lines painted in `color` (egui paths) and the points they pass through.
    fn lines_in(h: &egui_kittest::Harness<'static, crate::test_window::Window>, color: egui::Color32) -> Vec<Vec<Pos2>> {
        fn walk(s: &egui::Shape, color: egui::Color32, out: &mut Vec<Vec<Pos2>>) {
            match s {
                egui::Shape::Path(p) if p.stroke.color == egui::epaint::ColorMode::Solid(color) && p.points.len() >= 2 => out.push(p.points.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, color, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        h.output().shapes.iter().for_each(|s| walk(&s.shape, color, &mut out));
        out
    }

    #[test]
    fn the_pencil_stroke_shows_while_drawing() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({})).unwrap();
        app.run("tool.select", serde_json::json!({"tool": "pencil"})).unwrap();
        let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
        let doc = &h.state().app.session.doc().unwrap().doc;
        let layer = doc.layers[0].color;
        let color = egui::Color32::from_rgb(layer[0], layer[1], layer[2]);
        let items = |h: &egui_kittest::Harness<'static, crate::test_window::Window>| h.state().app.session.doc().unwrap().doc.spreads[0].items.len();
        let before = items(&h);
        // Press on the page and drag, without letting go.
        let start = pos2(520.0, 380.0);
        h.hover_at(start);
        h.drag_at(start);
        h.run_steps(2);
        let path: Vec<Pos2> = (1..=12).map(|i| start + vec2(i as f32 * 12.0, (i as f32 * 0.6).sin() * 30.0)).collect();
        for p in &path {
            h.hover_at(*p);
            h.run_steps(1);
        }
        let drawn = lines_in(&h, color);
        let stroke = drawn.iter().max_by_key(|l| l.len()).expect("the stroke is painted while the button is down");
        let near = |a: Pos2, b: Pos2| (a - b).length() < 2.0;
        assert!(near(stroke[0], start), "it starts where the press was: {:?}", stroke[0]);
        assert!(path.iter().all(|p| stroke.iter().any(|q| near(*p, *q))), "it follows the pointer: {stroke:?}");
        assert_eq!(items(&h), before, "nothing is made before the release");
        // Letting go makes the path and ends the preview.
        h.drop_at(*path.last().unwrap());
        h.run_steps(3);
        assert_eq!(items(&h), before + 1);
        let app = &mut h.state_mut().app;
        let view = app.view_info();
        assert!(app.session.overlays(view).is_empty(), "the preview is gone");
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
