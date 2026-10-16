//! The document canvas: pasteboard, spreads (rendered by `designcraft-render`), guides, frame edges,
//! selection, text ports and threads, the text caret, rulers, and pointer/keyboard input.

use designcraft_compose as compose;
use designcraft_doc::{Content, Document, Item, PageSide, SpreadRef};
use designcraft_engine::doc::Selection;
use designcraft_geom::{Affine, Point, Rect as DRect, Vec2 as DVec2};
use designcraft_tools::{CanvasLayout, Cursor, Mods, Overlay, PointerEvent, PointerKind, ToolKey};
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
    let pad = 40.0;
    let zoom = ((rect.width() as f64 - 2.0 * pad) / b.width()).min((rect.height() as f64 - 2.0 * pad) / b.height()).clamp(0.05, 40.0);
    let origin = Point::new(b.center().x - rect.width() as f64 / 2.0 / zoom, b.center().y - rect.height() as f64 / 2.0 / zoom);
    if let Some(v) = app.view_mut() {
        *v = View { zoom, origin, fitted: true, rotation: v.rotation };
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

/// Scroll so absolute page `abs` is centred (keeps zoom).
pub fn go_to_page(app: &mut DesignApp, abs: usize) {
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, false);
    let Some(pr) = layout.page_rect(&st.doc, abs) else { return };
    let Some(rect) = app.canvas_rect else { return };
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(pr.center().x - rect.width() as f64 / 2.0 / v.zoom, pr.center().y - rect.height() as f64 / 2.0 / v.zoom);
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

/// Whether the wheel motion now arriving was made with Alt down. As egui does for Ctrl/Cmd, a
/// motion keeps its key to its end: the wheel is smoothed over a few frames, and a trackpad swipe
/// coasts on after the key is let go.
fn wheel_alt(ui: &egui::Ui) -> bool {
    let id = egui::Id::new("canvas_wheel_alt");
    // Alt, and whether a trackpad swipe is under way.
    let (mut alt, mut swipe): (bool, bool) = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.input(|i| {
        for e in &i.events {
            if let egui::Event::MouseWheel { phase, modifiers, .. } = e {
                (alt, swipe) = match phase {
                    egui::TouchPhase::Start => (modifiers.alt, true),
                    egui::TouchPhase::Move => (modifiers.alt || (swipe && alt), swipe),
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => (false, false),
                };
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, (alt, swipe)));
    alt
}

pub fn apply_view_request(app: &mut DesignApp, p: &Value) {
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
    if !app.view().is_some_and(|v| v.fitted) {
        fit(app, rect, "spread");
    }
    handle_input(app, ui, &resp, rect);
    if bars {
        scrollbars(app, ui, outer, screen, rect, &t);
    }
    let Some(st) = app.session.active() else { return };
    let Some(&v) = app.view() else { return };
    let xf = Xf::new(rect, &v);
    let doc = st.doc.clone();
    let layout = CanvasLayout::new(&doc, st.editing_parents);
    let painter = ui.painter_at(screen);
    let preview = matches!(app.ui.screen_mode, ScreenMode::Preview | ScreenMode::Presentation);
    let bg = if app.ui.screen_mode == ScreenMode::Presentation { Color32::BLACK } else { t.pasteboard };
    painter.rect_filled(screen, 0.0, bg);
    // Page shadow: a hard 1.5 pt black offset on the right and bottom (InDesign 2026).
    for slot in &layout.slots {
        let r = xf.rect(slot.bounds);
        painter.rect_filled(r.translate(vec2(1.5, 1.5)), 0.0, Color32::BLACK);
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
            // Preview: only page areas (trim) show content.
            for slot in &layout.slots {
                let r = xf.rect(slot.bounds);
                painter.with_clip_rect(r.intersect(screen)).add(egui::Shape::mesh(mesh.clone()));
            }
        } else {
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    let hair = 1.0 / ui.ctx().pixels_per_point();
    for slot in &layout.slots {
        painter.rect_stroke(xf.rect(slot.bounds), 0.0, Stroke::new(hair, Color32::from_rgb(0x4a, 0x4a, 0x4a)), StrokeKind::Outside);
    }
    if !preview {
        draw_guides(app, &painter, &xf, &doc, &layout, &t);
        if let Some(g) = ui.data(|d| d.get_temp::<GuideDrag>(egui::Id::new(GUIDE_DRAG)))
            && let Some(p) = g.at
        {
            let col = Color32::from_rgb(74, 227, 255);
            let (a, b) = match g.orientation {
                designcraft_doc::Orientation::Horizontal => (pos2(rect.min.x, p.y), pos2(rect.max.x, p.y)),
                designcraft_doc::Orientation::Vertical => (pos2(p.x, rect.min.y), pos2(p.x, rect.max.y)),
            };
            // A new guide from the rulers with the Page tool is a liquid guide: dashed.
            if g.from.is_none() && app.session.tool_id() == "page" {
                dashed(&painter, a, b, Stroke::new(1.0, col), 6.0, 3.0);
            } else {
                painter.line_segment([a, b], Stroke::new(1.0, col));
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
    let sel_rect = draw_selection(app, &painter, &xf, &doc, &layout);
    draw_tool_overlays(app, &painter, &xf);
    if rulers {
        draw_rulers(app, ui, full, rect, &xf, &doc, &layout, &t);
        ruler_units_menus(app, &resp, full, rect, &doc);
    }
    if let Some(r) = sel_rect
        && !preview
        && r.intersects(screen)
    {
        crate::taskbar::show(app, &ui.ctx().clone(), r, screen);
    }
    // Cursor.
    if let Some(p) = resp.hover_pos().filter(|p| screen.contains(*p)) {
        let wants_text = app.session.wants_text();
        let space = ui.input(|i| i.key_down(egui::Key::Space)) && !wants_text;
        let middle = ui.input(|i| i.pointer.middle_down());
        let alt_hand: bool = ui.data(|d| d.get_temp(egui::Id::new(("canvas_alt_hand", app.pane)))).unwrap_or(false);
        let tool_drag: bool = ui.data(|d| d.get_temp(egui::Id::new(("canvas_pointer_down", app.pane)))).unwrap_or(false);
        let alt_text = wants_text && !tool_drag && ui.input(|i| i.modifiers.alt);
        let c = if middle || alt_hand {
            Cursor::HandGrab
        } else if space || alt_text {
            Cursor::Hand
        } else {
            app.session.cursor(xf.to_canvas(p), ui.input(|i| mods(i, false)), app.view_info())
        };
        let icon = crate::cursors::show(ui.ctx(), &mut app.cursors, c, p);
        ui.ctx().set_cursor_icon(icon);
        draw_cursor_badge(&painter, p, c, &t);
    }
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

/// Keep the canvas texture current. Panning inside the overscanned region just shifts the
/// texture; zooming shows the old texture scaled until the background render arrives; document
/// edits render synchronously so editing never lags.
fn render_texture(app: &mut DesignApp, ctx: &egui::Context, rect: Rect, xf: &Xf, layout: &CanvasLayout, preview: bool) {
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
        let opts = designcraft_render::RenderOptions {
            printing_only: preview,
            greek_below_px: 3.0 * ppp,
            highlight_missing_fonts: !preview && app.session.prefs.highlight_substituted_fonts,
            highlight_hj: !preview && app.session.prefs.highlight_hj,
            condition_indicators: !preview,
            note_indicators: !preview && app.session.prefs.show_note_anchors,
            change_markup: !preview && app.session.prefs.show_added_text,
            tag_markers: !preview && app.ui.tag_markers,
            plate: app.ui.separation,
            highlight_keeps: !preview && app.session.prefs.highlight_keeps,
            highlight_custom_tracking: !preview && app.session.prefs.highlight_custom_tracking,
            quality: app.ui.display_quality,
            rich_black: app.ui.rich_black,
            overprint_preview: app.ui.overprint_preview,
            blend_space_view: !app.ui.proof_colors,
            ..Default::default()
        };
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
    if sh.doc.0 != doc_key.0 || sh.doc.3 != doc_key.3 || sh.editing_parents != st.editing_parents || app.canvas.texture.is_none() {
        return false;
    }
    let new = st.doc.clone();
    let Some(regions) = designcraft_render::damage::damage_with(&old, &new, Some(&app.session.cache)) else { return false };
    let t0 = crate::now_ms();
    let (tw, th) = ((sh.size.0 as f64 * ppp).round() as i64, (sh.size.1 as f64 * ppp).round() as i64);
    // A texture `upload` sampled down to the GPU limit has other pixels than the view: a region
    // computed in view pixels would be written outside it. Render it whole instead.
    if app.canvas.texture.as_ref().is_none_or(|t| t.size() != [tw.max(0) as usize, th.max(0) as usize]) {
        return false;
    }
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
    let opts = designcraft_render::RenderOptions {
        printing_only: preview,
        greek_below_px: 3.0 * ppp,
        highlight_missing_fonts: !preview && app.session.prefs.highlight_substituted_fonts,
        highlight_hj: !preview && app.session.prefs.highlight_hj,
        condition_indicators: !preview,
        note_indicators: !preview && app.session.prefs.show_note_anchors,
        change_markup: !preview && app.session.prefs.show_added_text,
        tag_markers: !preview && app.ui.tag_markers,
        plate: app.ui.separation,
        highlight_keeps: !preview && app.session.prefs.highlight_keeps,
        highlight_custom_tracking: !preview && app.session.prefs.highlight_custom_tracking,
        quality: app.ui.display_quality,
        rich_black: app.ui.rich_black,
        overprint_preview: app.ui.overprint_preview,
        blend_space_view: !app.ui.proof_colors,
        ..Default::default()
    };
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
    // Plates render from source colours; the ink limit is estimated from the composite.
    if app.ui.separation.is_none() && app.ui.ink_limit.is_some() {
        designcraft_render::separation_view(&mut img, None, app.ui.ink_limit);
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
/// edge is not a line). Lines above the page are skipped without walking them, however far up the start is.
fn baseline_grid_lines(start: f64, increment: f64, height: f64) -> impl Iterator<Item = f64> {
    let step = increment.max(1.0);
    let first = if start < 0.0 { start.rem_euclid(step) } else { start };
    (0..MAX_BASELINES).map(move |n| first + n as f64 * step).take_while(move |y| *y < height)
}

fn draw_guides(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout, t: &Tokens) {
    let s = &doc.settings;
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        // Everything below is in spread coordinates, placed through the slot (which may be turned).
        let m = slot.xf;
        let pt = |p: Point| xf.to_screen(m * p);
        let rect = |r: DRect| xf.rect(m.transform_rect_bbox(r));
        // Bleed: one rectangle around the spread (inside bleed only applies at the spread's outer edges).
        let b = s.bleed;
        if app.ui.guides && b.iter().any(|v| *v > 0.0) && !sp.pages.is_empty() {
            let first = &sp.pages[0];
            let last = &sp.pages[sp.pages.len() - 1];
            let l = if first.side == PageSide::Left { b[3] } else { b[2] };
            let r = if last.side == PageSide::Right || last.side == PageSide::Single { b[3] } else { b[2] };
            let sb = sp.bounds();
            let br = DRect::new(sb.x0 - l, sb.y0 - b[0], sb.x1 + r, sb.y1 + b[1]);
            painter.rect_stroke(rect(br), 0.0, Stroke::new(hair(painter), c32(s.bleed_color)), StrokeKind::Middle);
        }
        let pasteboard = m.inverse().transform_rect_bbox(layout.pasteboard);
        let first_abs = match slot.spread {
            SpreadRef::Doc(si) => Some(doc.first_page_of_spread(si)),
            _ => None,
        };
        for (pi, p) in sp.pages.iter().enumerate() {
            let pr = p.bounds();
            // Page tool › Show Parent Page Overlay: the parent page's size over the page.
            if p.show_parent_overlay
                && let SpreadRef::Doc(si) = slot.spread
                && let Some((ppi, ppg)) = doc.parent_page_for(doc.first_page_of_spread(si) + pi)
                && let Some(pp) = doc.parents.get(ppi).and_then(|ps| ps.pages.get(ppg))
            {
                let r = rect(DRect::new(pr.x0, pr.y0, pr.x0 + pp.width, pr.y0 + pp.height));
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(110, 110, 230, 28));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(110, 110, 230)), StrokeKind::Middle);
            }
            if !app.ui.guides {
                continue;
            }
            // Guides set on the applied parent page show on this page too.
            let inherited = first_abs.map(|f| doc.inherited_guides(f + pi)).unwrap_or_default();
            // Baseline grid.
            if app.ui.baseline_grid && xf.zoom >= s.baseline_grid.view_threshold {
                let g = &s.baseline_grid;
                for y in baseline_grid_lines(g.start, g.increment, p.height) {
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
            for g in p.guides.iter().chain(inherited.iter()).filter(|g| g.visible_in(doc)) {
                // Spread guides cross the pasteboard; page guides their page.
                let span = if g.spread { pasteboard } else { pr };
                let (a, bb) = match g.orientation {
                    designcraft_doc::Orientation::Horizontal => (Point::new(span.x0, g.position), Point::new(span.x1, g.position)),
                    designcraft_doc::Orientation::Vertical => {
                        let span = if g.spread { sp.bounds().inflate(0.0, 36.0) } else { pr };
                        (Point::new(g.position, span.y0), Point::new(g.position, span.y1))
                    }
                };
                let stroke = Stroke::new(hair(painter), Color32::from_rgb(74, 227, 255));
                if g.liquid {
                    // Liquid guides are dashed.
                    painter.extend(egui::Shape::dashed_line(&[pt(a), pt(bb)], stroke, 6.0, 3.0));
                } else {
                    painter.line_segment([pt(a), pt(bb)], stroke);
                }
            }
        }
    }
    let _ = t;
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
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        let a = slot.xf;
        // Parent items on document pages: dotted edges.
        if let SpreadRef::Doc(si) = slot.spread {
            let first = doc.first_page_of_spread(si);
            for (pi, page) in sp.pages.iter().enumerate() {
                let Some((ppi, ppg)) = doc.parent_page_for(first + pi) else { continue };
                let parent = &doc.parents[ppi];
                let dx = page.x - parent.pages[ppg].x;
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
        }
        let b = xf.rect(a.transform_rect_bbox(it.bounds()));
        union = Some(union.map_or(b, |u| u.union(b)));
        // Text frame ports.
        if let Content::Text(tf) = &it.content {
            draw_ports(app, painter, xf, doc, layout, it, tf.story, color);
        }
        if let Content::Graphic(g) = &it.content
            && sel.content
        {
            let gb = xf.rect((a * it.xf).transform_rect_bbox(g.xf.transform_rect_bbox(DRect::new(0.0, 0.0, g.size.0, g.size.1))));
            painter.rect_stroke(gb, 0.0, Stroke::new(1.0, Color32::from_rgb(196, 111, 43)), StrokeKind::Middle);
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
        // Centre point and the live-corner widget.
        painter.rect_filled(Rect::from_center_size(u.center(), vec2(3.5, 3.5)), 0.0, color);
        if sel.items.len() == 1
            && doc.item(sel.items[0]).is_some_and(|i| matches!(i.shape, designcraft_doc::Shape::Rectangle) && i.children().is_empty())
        {
            let lc = Rect::from_center_size(pos2(u.max.x, u.min.y + 11.5), vec2(6.0, 6.0));
            painter.rect_filled(lc, 0.0, Color32::from_rgb(0xff, 0xe5, 0x00));
            painter.rect_stroke(lc, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
        }
        for h in designcraft_tools::select::handles(DRect::new(u.min.x as f64, u.min.y as f64, u.max.x as f64, u.max.y as f64)) {
            let r = Rect::from_center_size(pos2(h.x as f32, h.y as f32), vec2(hs, hs));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(r, 0.0, Stroke::new(hair(painter), color), StrokeKind::Inside);
        }
        // Content grabber on graphic frames.
        if sel.items.len() == 1 && doc.item(sel.items[0]).is_some_and(|i| matches!(i.content, Content::Graphic(_))) {
            painter.circle_stroke(u.center(), 8.0, Stroke::new(1.5, Color32::from_white_alpha(170)));
            painter.circle_stroke(u.center(), 4.0, Stroke::new(1.5, Color32::from_white_alpha(170)));
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
    // Text an input method is composing at the caret is marked like it is in every Mac text
    // field: a yellow box behind the glyphs.
    let composing = app.session.composition().filter(|c| c.0 == ts.story && c.1 == ts.cell && ts.is_caret()).map(|c| c.2);
    let range = composing.clone().unwrap_or_else(|| ts.range());
    let tint = if composing.is_some() { COMPOSING_FILL.gamma_multiply(0.7) } else { Color32::from_rgba_unmultiplied(80, 140, 255, 110) };
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
        draw_inverse_highlight(app, painter, xf, doc, &cs, ts.story, &runs, composing.is_some());
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
        remember_ime_caret(painter, p0, p1);
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
    }
}

/// Where the text caret stood on screen last frame: the OS puts its input-method window there.
fn ime_caret_id() -> egui::Id {
    egui::Id::new("canvas_ime_caret")
}

/// The input method is composing text (sent to `text.compose`) that it hasn't committed.
fn ime_composing_id() -> egui::Id {
    egui::Id::new("canvas_ime_composing")
}

/// Remembers the caret at `p0`–`p1` for the input method's window.
fn remember_ime_caret(painter: &egui::Painter, p0: egui::Pos2, p1: egui::Pos2) {
    painter.ctx().data_mut(|d| d.insert_temp(ime_caret_id(), egui::Rect::from_two_pos(p0, p1)));
}

/// Caret / text highlight inside a table cell (the cell's lines are offset by the cell origin).
#[allow(clippy::too_many_arguments)]
fn draw_cell_selection(
    app: &DesignApp,
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
    let composing = app.session.composition().filter(|c| c.0 == ts.story && c.1 == ts.cell && ts.is_caret()).map(|c| c.2);
    let range = composing.clone().unwrap_or_else(|| ts.range());
    let tint = if composing.is_some() { COMPOSING_FILL.gamma_multiply(0.6) } else { Color32::from_rgba_unmultiplied(80, 140, 255, 110) };
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
                painter.add(egui::Shape::convex_polygon(poly.to_vec(), tint, Stroke::NONE));
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
        remember_ime_caret(painter, p0, p1);
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
    }
}

/// The box behind text an input method is composing.
const COMPOSING_FILL: Color32 = Color32::from_rgb(255, 247, 153);

/// InDesign draws selected text inverted: a black highlight with the selected glyphs in the
/// inverse of their colour (white for black text). The highlight and glyph outlines are
/// rasterised with `vello_cpu` into a small screen-space texture over the selection.
/// `composing`: the text is an input method's, still being composed: a yellow box with the
/// glyphs in their own colour.
#[allow(clippy::too_many_arguments)]
fn draw_inverse_highlight(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    cs: &compose::ComposedStory,
    story: designcraft_doc::StoryId,
    runs: &[(usize, Affine, Vec<[Point; 4]>, Vec<(usize, usize)>)],
    composing: bool,
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
    let key = egui::Id::new(("inverse_sel", st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize, story.0))
        .with((runs.iter().map(|r| r.2.len() + r.3.len() * 7919).sum::<usize>(), w, h))
        .with((bbox.min.x.to_bits(), bbox.min.y.to_bits(), xf.zoom.to_bits(), st.selection.text.map(|t| (t.anchor, t.focus)), composing));
    let cache_id = egui::Id::new("inverse_sel_tex");
    let cached: Option<(egui::Id, egui::TextureHandle)> = ctx.data(|d| d.get_temp(cache_id));
    let tex = match cached {
        Some((k, t)) if k == key => t,
        _ => {
            let view = xf.affine(ppp as f64, bbox.min);
            let mut rc = vello_cpu::RenderContext::new_with(w as u16, h as u16, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
            let [br, bg, bb, _] = if composing { COMPOSING_FILL.to_array() } else { [0, 0, 0, 255] };
            rc.set_paint(peniko::Color::from_rgba8(br, bg, bb, 255));
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
                    let [r, gg, b, a] = c.to_rgba8(1.0);
                    if composing {
                        rc.set_paint(peniko::Color::from_rgba8(r, gg, b, a));
                    } else {
                        rc.set_paint(peniko::Color::from_rgba8(255 - r, 255 - gg, 255 - b, 255));
                    }
                    let outline = db.outline(&g.face, g.gid);
                    let skew =
                        if style.skew != 0.0 { Affine::new([1.0, 0.0, -style.skew.to_radians().tan(), 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
                    let ga = Affine::translate((g.x, l.baseline + g.y)) * skew * Affine::scale_non_uniform(g.sx, g.sy);
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
            Overlay::Path { .. } => {}
        }
    }
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
    let zc = Rect::from_min_max(full.min, rect.min).center();
    painter.line_segment([zc - vec2(4.0, 0.0), zc + vec2(4.0, 0.0)], Stroke::new(1.0, t.ruler_tick));
    painter.line_segment([zc - vec2(0.0, 4.0), zc + vec2(0.0, 4.0)], Stroke::new(1.0, t.ruler_tick));
    // Origin: top-left of the current spread's first page.
    let Some(i) = current_slot(app, layout) else { return };
    let slot = &layout.slots[i];
    let origin = Point::new(slot.bounds.x0, slot.bounds.y0);
    let font = egui::FontId::proportional(9.0);
    let tick = Stroke::new(1.0, t.ruler_tick);
    // Horizontal.
    let unit = doc.settings.horizontal_units;
    let (major, sub) = unit.ruler_ticks(xf.zoom);
    let c0 = xf.to_canvas(top.min).x - origin.x;
    let c1 = xf.to_canvas(pos2(top.max.x, 0.0)).x - origin.x;
    let mut k = (c0 / major).floor() as i64;
    while (k as f64) * major <= c1 {
        let x0 = k as f64 * major;
        for s in 0..sub {
            let x = x0 + major * s as f64 / sub as f64;
            let sx = xf.to_screen(Point::new(origin.x + x, 0.0)).x;
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
    if let Some(p) = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p)) {
        let m = Stroke::new(1.0, t.text_dim);
        dashed(&painter, pos2(p.x, top.min.y), pos2(p.x, top.max.y), m, 1.5, 1.5);
        dashed(&painter, pos2(left.min.x, p.y), pos2(left.max.x, p.y), m, 1.5, 1.5);
    }
}

/// The horizontal (top) and vertical (left) rulers around the view `rect` in `full`.
fn ruler_rects(full: Rect, rect: Rect) -> (Rect, Rect) {
    (
        Rect::from_min_max(pos2(rect.min.x, full.min.y), pos2(full.max.x, rect.min.y)),
        Rect::from_min_max(pos2(full.min.x, rect.min.y), pos2(rect.min.x, full.max.y)),
    )
}

/// Right-clicking a ruler opens a menu of units for that ruler.
fn ruler_units_menus(app: &mut DesignApp, resp: &egui::Response, full: Rect, rect: Rect, doc: &Document) {
    let (top, left) = ruler_rects(full, rect);
    let at = resp.interact_pointer_pos();
    for (ruler, key, cur) in [(top, "horizontalUnits", doc.settings.horizontal_units), (left, "verticalUnits", doc.settings.vertical_units)] {
        let open = if resp.secondary_clicked() {
            Some(egui::SetOpenCommand::Bool(at.is_some_and(|p| ruler.contains(p))))
        } else if resp.clicked() {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        egui::Popup::context_menu(resp).id(resp.id.with(("ruler_units", key))).open_memory(open).show(|ui| ruler_units_menu(app, ui, key, cur));
    }
}

/// Every unit, the current one checked; picking one sets `key` (`horizontalUnits` or
/// `verticalUnits`) through `document.preferences`.
fn ruler_units_menu(app: &mut DesignApp, ui: &mut egui::Ui, key: &str, cur: designcraft_geom::Unit) {
    for u in designcraft_geom::Unit::ALL {
        let label = crate::i18n::tr(&app.ui.language, u.label());
        let text = if u == cur { format!("✓ {label}") } else { format!("   {label}") };
        if ui.button(crate::rtl::widget(ui, text)).clicked() {
            let _ = app.run("document.preferences", json!({ key: u }));
            ui.close();
        }
    }
}

fn fmt_tick(v: f64) -> String {
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
        let id = egui::Id::new(("canvas_scrollbar", app.pane, vertical));
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
        let along = |p: egui::Vec2| if vertical { p.y } else { p.x };
        let page = along(screen.size());
        let mut moved = 0.0;
        if grabbed {
            moved = -along(resp.drag_delta()) * k;
        } else if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let (p, a, b) = if vertical { (p.y, thumb.min.y, thumb.max.y) } else { (p.x, thumb.min.x, thumb.max.x) };
            if p < a {
                moved = page;
            } else if p > b {
                moved = -page;
            }
        }
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

fn handle_input(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect) {
    let Some(v) = app.view().copied() else { return };
    let xf = Xf::new(rect, &v);
    let wants_text = app.session.wants_text();
    let space = ui.input(|i| i.key_down(egui::Key::Space)) && !wants_text && !ui.ctx().text_edit_focused();
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
    // Space-drag = hand (unless a tool drag is under way: then Space is a modifier, e.g. Live
    // Distribute while resizing). Middle-drag pans too, with any tool; tools only
    // see the primary button.
    let tool_drag: bool = ui.data(|d| d.get_temp(egui::Id::new(("canvas_pointer_down", app.pane)))).unwrap_or(false);
    if !tool_drag && ((space && resp.dragged()) || resp.dragged_by(egui::PointerButton::Middle)) {
        let d = xf.unrotate_delta(resp.drag_delta());
        if let Some(v) = app.view_mut() {
            v.origin = Point::new(v.origin.x - d.x as f64 / v.zoom, v.origin.y - d.y as f64 / v.zoom);
        }
        return;
    }
    // Editing text (Space types a space): Alt held at the press gives the Hand tool until the
    // button is released, and the caret stays. Alt pressed later in a text drag copies the text.
    let alt_hand_id = egui::Id::new(("canvas_alt_hand", app.pane));
    let mut alt_hand: bool = ui.data(|d| d.get_temp(alt_hand_id)).unwrap_or(false);
    let (press, alt, primary_down) = ui.input(|i| (i.pointer.primary_pressed(), i.modifiers.alt, i.pointer.primary_down()));
    if press && alt && wants_text && !tool_drag && resp.hovered() {
        alt_hand = true;
    }
    if alt_hand {
        if resp.dragged() {
            pan(app, &xf, resp.drag_delta());
        }
        ui.data_mut(|d| d.insert_temp(alt_hand_id, primary_down));
        return;
    }
    // Ruler guides need the rulers (hidden while the view is rotated).
    if !space && xf.rot == 0 && guide_drag(app, ui, resp, rect, &xf) {
        return;
    }
    let m = ui.input(|i| mods(i, space));
    let vi = app.view_info();
    let pos = |p: Pos2| xf.to_canvas(p);
    let mut events: Vec<PointerEvent> = Vec::new();
    let down_id = egui::Id::new(("canvas_pointer_down", app.pane));
    let mut down: bool = ui.data(|d| d.get_temp(down_id)).unwrap_or(false);
    let clicks_id = egui::Id::new(("canvas_clicks", app.pane));
    let (pressed, released, origin, latest, now) =
        ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin(), i.pointer.latest_pos(), i.time));
    if pressed
        && !space
        && let Some(o) = origin.filter(|o| (if xf.rot == 0 { rect.contains(*o) } else { resp.rect.contains(*o) }) && resp.hovered())
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
    let hold_id = egui::Id::new(("canvas_hold", app.pane));
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
    if resp.clicked() || resp.drag_started() {
        resp.request_focus();
    }
    // Both split panes show the same document, but only the focused pane dispatches its keys.
    if app.pane != app.focus_pane || (ui.ctx().text_edit_focused() && !resp.has_focus()) {
        return;
    }
    // The engine drops composed text when something else edits or the caret moves; the input
    // method must forget it too, or the next key would finish an accent that is gone.
    let composing = ui.data(|d| d.get_temp::<bool>(ime_composing_id())).unwrap_or(false);
    let interrupt = composing && app.session.composition().is_none();
    if interrupt || !wants_text {
        ui.data_mut(|d| d.remove::<bool>(ime_composing_id()));
    }
    if wants_text {
        // Turn the OS input method on: macOS composes dead keys (⌥E then I types í) and
        // Japanese/Chinese input only for a window that asks for it. Without it ⌥E typed "e".
        let cursor_rect = ui.data(|d| d.get_temp(ime_caret_id())).unwrap_or(egui::Rect::from_min_size(rect.min, egui::Vec2::ZERO));
        ui.ctx().output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput { purpose: egui::IMEPurpose::Normal, rect, cursor_rect, should_interrupt_composition: interrupt });
        });
    }
    let evs = ui.input(|i| i.events.clone());
    for e in evs {
        match e {
            egui::Event::Text(t) if wants_text => {
                let _ = app.run("text.insert", json!({"text": t}));
            }
            // What is composed shows in the story, in the text's own format, until it commits.
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) if wants_text => {
                let on = !text.is_empty();
                if app.run("text.compose", json!({"text": text})).is_ok() {
                    ui.data_mut(|d| d.insert_temp(ime_composing_id(), on));
                }
            }
            egui::Event::Ime(egui::ImeEvent::Commit(t)) if wants_text => {
                ui.data_mut(|d| d.remove::<bool>(ime_composing_id()));
                let _ = app.run("text.compose", json!({"text": ""}));
                if !t.is_empty() {
                    let _ = app.run("text.insert", json!({"text": t}));
                }
            }
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
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let k = match key {
                    egui::Key::ArrowLeft => Some(ToolKey::Left),
                    egui::Key::ArrowRight => Some(ToolKey::Right),
                    egui::Key::ArrowUp => Some(ToolKey::Up),
                    egui::Key::ArrowDown => Some(ToolKey::Down),
                    egui::Key::Enter => Some(ToolKey::Enter),
                    egui::Key::Escape => Some(ToolKey::Escape),
                    egui::Key::Backspace => Some(ToolKey::Backspace),
                    egui::Key::Delete => Some(ToolKey::Delete),
                    egui::Key::Tab => Some(ToolKey::Tab),
                    egui::Key::Home => Some(ToolKey::Home),
                    egui::Key::End => Some(ToolKey::End),
                    _ => None,
                };
                if let Some(k) = k {
                    let m = Mods { shift: modifiers.shift, alt: modifiers.alt, cmd: modifiers.command, ctrl: modifiers.ctrl, space: false };
                    let handled = app.session.tool_key(k, m, vi).unwrap_or(false);
                    if !handled && k == ToolKey::Escape {
                        let _ = app.run("edit.deselectAll", json!({}));
                    }
                    app.after_engine();
                }
            }
            _ => {}
        }
    }
    let _ = DVec2::ZERO;
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
                for g in &l.glyphs {
                    if g.len == 0 {
                        continue;
                    }
                    let ch = story.text[g.byte.min(story.text.len())..].chars().next().unwrap_or(' ');
                    let mark = match ch {
                        ' ' => "·",
                        '\t' => "»",
                        '\u{2028}' => "¬",
                        '\u{A0}' => "°",
                        _ => continue,
                    };
                    let p = xf.to_screen(m * Point::new(g.x + if ch == ' ' { g.adv / 2.0 } else { 0.0 }, l.baseline));
                    let align = if ch == ' ' { egui::Align2::CENTER_BOTTOM } else { egui::Align2::LEFT_BOTTOM };
                    painter.text(p, align, mark, font.clone(), col);
                }
                if l.last_in_para {
                    let end = l.range.end;
                    let mark = if end >= story.text.len() { "#" } else { "¶" };
                    let p = xf.to_screen(m * Point::new(l.end_x + 1.0, l.baseline));
                    painter.text(p, egui::Align2::LEFT_BOTTOM, mark, font.clone(), col);
                }
            }
        }
    }
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
        let key = egui::Id::new(("dyn_spell", std::sync::Arc::as_ptr(story) as usize, doc.user_words.len()));
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

const GUIDE_DRAG: &str = "canvas_guide_drag";

/// A ruler guide being dragged: out of a ruler (`from` None) or an existing one.
#[derive(Clone, Debug)]
struct GuideDrag {
    orientation: designcraft_doc::Orientation,
    from: Option<(designcraft_doc::SpreadRef, usize, usize)>,
    /// Pointer position (screen).
    at: Option<Pos2>,
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

/// Ruler guides: drag one out of a ruler, drag an existing one (Selection tools, not over an
/// object) to move it, drop it back on a ruler to delete it. Returns true while it owns the pointer.
fn guide_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
    let id = egui::Id::new(GUIDE_DRAG);
    let (pressed, released, origin, latest) =
        ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.press_origin(), i.pointer.latest_pos()));
    let mut drag: Option<GuideDrag> = ui.data(|d| d.get_temp(id));
    if drag.is_none() && pressed && resp.hovered() && app.ui.guides && !app.ui.guides_locked {
        let Some(o) = origin else { return false };
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
                drag = Some(GuideDrag { orientation, from: None, at: Some(o) });
            }
        } else if rect.contains(o)
            && matches!(app.session.tool_id(), "selection" | "directSelection")
            && let Some((r, pi, gi, orientation)) = guide_at(app, xf, o)
        {
            // Objects take precedence over the guides behind them.
            let over_item = app.session.active().is_some_and(|st| {
                let layout = CanvasLayout::new(&st.doc, st.editing_parents);
                layout.spread_at(xf.to_canvas(o)).is_some_and(|(sr, p)| match sr {
                    designcraft_doc::SpreadRef::Doc(si) => st.doc.hit_item(si, p, 3.0 / xf.zoom).is_some(),
                    _ => false,
                })
            });
            if !over_item {
                drag = Some(GuideDrag { orientation, from: Some((r, pi, gi)), at: Some(o) });
            }
        }
    }
    let Some(mut g) = drag else { return false };
    if let Some(p) = latest {
        g.at = Some(p);
    }
    if !released {
        ui.ctx().set_cursor_icon(match g.orientation {
            designcraft_doc::Orientation::Horizontal => egui::CursorIcon::ResizeVertical,
            designcraft_doc::Orientation::Vertical => egui::CursorIcon::ResizeHorizontal,
        });
        ui.data_mut(|d| d.insert_temp(id, g));
        return true;
    }
    ui.data_mut(|d| d.remove::<GuideDrag>(id));
    let Some(p) = g.at else { return true };
    let on_canvas = rect.contains(p);
    let target = app.session.active().and_then(|st| {
        let layout = CanvasLayout::new(&st.doc, st.editing_parents);
        let (r, sp) = layout.spread_at(xf.to_canvas(p))?;
        let spread = st.doc.spread(r)?;
        let on_page = spread.pages.iter().position(|pg| pg.bounds().contains(sp));
        Some((r, sp, on_page))
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
        (Some((r, pi, gi)), false, _) => {
            let _ = app.run("guide.delete", json!({"spread": r, "page": pi, "index": gi}));
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

    fn harness(app: DesignApp) -> Harness<'static, DesignApp> {
        let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).build_ui_state(
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

    fn held(modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::ModifiersChanged(modifiers)
    }

    fn wheel(phase: egui::TouchPhase, up: f32, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, up), phase, modifiers }
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
            let (mut with_alt, mut with_command) = (new_document(), new_document());
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
        // The page and the ruler corner open no units menu.
        for p in [rect.center(), rect.min - vec2(RULER / 2.0, RULER / 2.0)] {
            right_click(&mut h, p);
            assert!(h.query_by_label("   Points").is_none());
        }
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

    fn selected(h: &Harness<'_, DesignApp>) -> std::ops::Range<usize> {
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
            let r = selected(&h);
            got.push(text[r].to_string());
        }
        assert_eq!(got[1..], ["Three", "One two. Three four! Five six.", "One two. Three four! Five six.\n", text], "{got:?}");
        assert!(got[0].is_empty(), "one click places the caret: {got:?}");
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
        assert!(selected(&h).is_empty(), "three clicks half a second apart leave a caret: {:?}", selected(&h));
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
        assert_eq!(text(&h), "T´", "the accent being composed shows in the story, in its format");
        assert!(h.state().session.composition().is_some(), "and is marked as composed, not typed");
        h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None }));
        h.event(egui::Event::Ime(egui::ImeEvent::Commit("í".into())));
        h.run_steps(2);
        assert_eq!(text(&h), "Tí");
        assert!(h.state().session.composition().is_none());
        // Moving the caret mid-composition drops the accent and tells the input method so.
        h.event(egui::Event::Ime(egui::ImeEvent::Preedit { text: "˜".into(), active_range_chars: None }));
        h.run_steps(2);
        h.state_mut().run("text.move", json!({"dir": "left"})).unwrap();
        h.step();
        assert_eq!(text(&h), "Tí");
        assert!(h.output().platform_output.ime.as_ref().is_some_and(|i| i.should_interrupt_composition));
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
        let (pen, direct, text) = (shown("pen"), shown("directSelection"), shown("type"));
        let ppp = h.ctx.pixels_per_point();
        let images = &mut h.state_mut().cursors;
        match crate::cursors::MODE {
            crate::cursors::Mode::Os => {
                assert_eq!(pen.1, images.get(Cursor::Pen, ppp), "Pen");
                assert_eq!(direct.1, images.get(Cursor::ArrowHollow, ppp), "Direct Selection");
                assert_eq!(text, (egui::CursorIcon::Text, None), "the I-beam stays a system cursor");
            }
            crate::cursors::Mode::Painted => assert_eq!((pen.0, direct.0), (egui::CursorIcon::None, egui::CursorIcon::None)),
            crate::cursors::Mode::System => assert_eq!(pen, (egui::CursorIcon::Crosshair, None)),
        }
    }

    #[test]
    fn rulers_open_no_units_menu_without_a_document() {
        let mut h = harness(DesignApp::new(designcraft_engine::Session::new(), crate::Services::default()));
        let Some(rect) = h.state().canvas_rect else { return };
        right_click(&mut h, pos2(rect.center().x, rect.min.y - RULER / 2.0));
        assert!(h.query_by_label("   Points").is_none());
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

    #[test]
    fn document_views_have_scrollbars_beside_the_view() {
        let h = window();
        let rect = h.state().app.canvas_rect.unwrap();
        for vertical in [true, false] {
            let r = h.ctx.read_response(egui::Id::new(("canvas_scrollbar", 0u8, vertical))).expect("scrollbar").rect;
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
        let r = h.ctx.read_response(egui::Id::new(("canvas_scrollbar", 0u8, true))).expect("scrollbar");
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
