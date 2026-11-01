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

mod column_guides;
mod frames;
mod guides;
mod ime;
mod input;
mod page_decor;
mod render;
mod rulers;
mod scroll;
mod text_overlay;
mod tool_overlays;
mod view;

use column_guides::{COLUMN_DRAG, ColumnDrag, column_guide_at, spread_x_across};
use frames::{draw_frames, draw_selection};
#[cfg(test)]
use frames::{item_canvas_xf, overset_frames, overset_marks, ported_frames, text_frames_in};
use guides::{GUIDE_DRAG, GUIDE_PRESS, GuideDrag, GuidePress, guide_readout, guide_spot, moving_guides, readout_at};
#[cfg(test)]
use guides::{guide_at, guide_drag_delta, guide_measure};
use ime::ime_canvas_pass;
pub(crate) use ime::{CanvasIme, ime_observe};
pub(crate) use input::forwarded_keys;
pub use input::{begin_frame, context_menu_open};
use input::{context_menu, draw_cursor_badge, handle_input, pointer_elsewhere};
#[cfg(test)]
use input::{press_count, release_clicks};
#[cfg(test)]
use page_decor::{MAX_BASELINES, baseline_grid_lines};
use page_decor::{dashed, draw_flattener_preview, draw_guides, shown_area};
use render::render_texture;
pub use render::{DisplayKey, color_at};
#[cfg(test)]
use render::{c32, invalidate_on_display_change, screen_options};
pub(crate) use rulers::fmt_tick;
#[cfg(test)]
use rulers::ruler_pieces;
use rulers::{draw_rulers, ruler_units_menus};
use scroll::{mods, navigation_space, scrollbars, spring_zoom_mode};
pub use text_overlay::selected_text;
use text_overlay::{draw_dynamic_spelling, draw_hidden_characters};
#[cfg(test)]
use text_overlay::{draw_text_selection, frame_hidden_marks, hidden_marks};
use tool_overlays::draw_tool_overlays;
pub(crate) use view::page_in_view;
use view::present_fit;
pub use view::{
    PageBox, apply_view_request, can_go_through_history, current_page, current_parent_page, current_slot, current_spread, current_spread_page,
    drop_point, fit, go_through_history, go_to_page, page_box, page_box_go, set_zoom, show_spread, step_spread, view_centre, zoom_at,
};

pub const RULER: f32 = 15.0;
/// Width of the view's scrollbars.
pub const SCROLLBAR: f32 = 10.0;

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

/// A guide being dragged out or moved: its line, what caught it, and where it is beside the pointer (#1282).
fn draw_guide_drag(app: &DesignApp, painter: &egui::Painter, xf: &Xf, g: &GuideDrag, p: Pos2, rect: Rect, t: &Tokens) {
    let col = Color32::from_rgb(74, 227, 255);
    let spot = guide_spot(app, xf, g);
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
        dashed(painter, a, b, Stroke::new(1.0, col), 6.0, 3.0);
    } else {
        painter.line_segment([a, b], Stroke::new(1.0, col));
    }
    // Where it is, beside the pointer (#1282).
    if let Some(text) = guide_readout(app, xf, g) {
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), t.text);
        let at = readout_at(p, galley.size(), rect);
        let bg = Rect::from_min_size(at, galley.size()).expand(3.0);
        painter.rect_filled(bg, 2.0, t.panel);
        painter.rect_stroke(bg, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
        painter.galley(at, galley, t.text);
    }
}

/// The rendered texture, placed where its region is in the current view.
fn paint_render(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout, screen: Rect, preview: bool) {
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
                let r = shown_area(app, doc, xf, slot);
                painter.with_clip_rect(r.intersect(screen)).add(egui::Shape::mesh(mesh.clone()));
            }
        } else {
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

/// Presentation: a click shows the next spread, Shift-click the previous one.
fn present_click(app: &mut DesignApp, ui: &egui::Ui, resp: &egui::Response, rect: Rect) {
    if resp.clicked() {
        // The release that made the click says whether Shift was down.
        let shift = ui.input(|i| {
            i.modifiers.shift || i.events.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: false, modifiers, .. } if modifiers.shift))
        });
        step_spread(app, if shift { -1 } else { 1 });
        present_fit(app, rect);
    }
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
        present_click(app, ui, &resp, rect);
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
    paint_render(app, &painter, &xf, &doc, &layout, screen, preview);
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
            draw_guide_drag(app, &painter, &xf, &g, p, rect, &t);
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
        let c = pointer_cursor(app, ui, &xf, p);
        let icon = crate::cursors::show(ui.ctx(), &mut app.cursors, c, p);
        ui.ctx().set_cursor_icon(icon);
        draw_cursor_badge(&painter, p, c, &t);
        if c == Cursor::LoadedGraphic {
            draw_loaded_thumbnail(app, &painter, p, &t);
        }
    }
    // A second viewport has no panels, dialogs or Quick Apply after its canvas: observe here,
    // once, as the end of its pass. The root viewport observes at the end of `DesignApp::ui`.
    if ui.ctx().viewport_id() != egui::ViewportId::ROOT {
        ime_observe(app, ui.ctx());
    }
}

/// The cursor under the pointer at `p` over the canvas: the navigation hands, then the guide
/// resize cursors, then the active tool's.
fn pointer_cursor(app: &mut DesignApp, ui: &egui::Ui, xf: &Xf, p: Pos2) -> Cursor {
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
            .then(|| column_guide_at(app, xf, p).map(|h| h.0))
            .flatten()
    });
    let c = match column {
        Some(r) if spread_x_across(app, r) => Cursor::ResizeH,
        Some(_) => Cursor::ResizeV,
        None => c,
    };
    // A ruler guide being dragged keeps the cursor across it until it is dropped, over the
    // canvas too (the tool's cursor took over as soon as the pointer left the ruler). Only
    // while the button is down: a drag state left behind by a skipped release doesn't stick.
    let dragging = ui.input(|i| i.pointer.primary_down());
    match ui.data(|d| d.get_temp::<GuideDrag>(egui::Id::unique(GUIDE_DRAG))).filter(|_| dragging).map(|g| g.orientation) {
        Some(designcraft_doc::Orientation::Horizontal) => Cursor::ResizeV,
        Some(designcraft_doc::Orientation::Vertical) => Cursor::ResizeH,
        None => c,
    }
}

/// Longest side of the loaded place cursor's thumbnail, in points.
const LOADED_THUMB: f32 = 128.0;

/// The loaded place cursor's thumbnail, as InDesign shows it: the graphic, at most 128 pt on its
/// longer side, hanging from the pointer, with an image badge in its corner.
fn draw_loaded_thumbnail(app: &DesignApp, painter: &egui::Painter, p: Pos2, t: &Tokens) {
    let (Some(l), Ok(st)) = (app.session.loaded.as_ref(), app.session.doc()) else { return };
    let (w, h) = (l.size.0 as f32, l.size.1 as f32);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return;
    }
    let k = LOADED_THUMB / w.max(h);
    let r = Rect::from_min_size(p + vec2(2.0, 2.0), vec2(w * k, h * k));
    if let Some(tex) = loaded_thumb_texture(painter.ctx(), &st.doc, l.asset, l.size) {
        painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha(225));
    } else {
        // Not drawable here (an unreadable file): its frame only.
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, t.measure_text), StrokeKind::Inside);
    }
    let badge = Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(16.0, 12.0));
    painter.rect_filled(badge, 2.0, t.measure_bg);
    let ink = Stroke::new(1.2, t.measure_text);
    let inner = badge.shrink2(vec2(3.0, 2.5));
    painter.rect_stroke(inner, 0.5, ink, StrokeKind::Middle);
    painter.add(egui::Shape::line(
        vec![pos2(inner.min.x, inner.max.y), pos2(inner.min.x + 3.5, inner.min.y + 2.5), pos2(inner.max.x, inner.max.y)],
        ink,
    ));
}

/// The loaded graphic's thumbnail texture, cached per image data.
fn loaded_thumb_texture(ctx: &egui::Context, doc: &Document, asset: designcraft_doc::AssetId, size: (f64, f64)) -> Option<egui::TextureHandle> {
    let a = doc.assets.get(&asset)?;
    let key = egui::Id::unique(("loaded_thumb", std::sync::Arc::as_ptr(&a.data) as usize, a.data.len(), a.page));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(t);
    }
    // Twice the thumbnail's size in pixels, for high-density screens.
    let scale = (2.0 * f64::from(LOADED_THUMB) / size.0.max(size.1)).min(1.0);
    let pm = designcraft_render::images::mip(&a.data, a.page, size.0, scale)?;
    let (pw, ph) = (pm.width() as usize, pm.height() as usize);
    if pw == 0 || ph == 0 {
        return None;
    }
    let ci = egui::ColorImage::from_rgba_premultiplied([pw, ph], pm.data_as_u8_slice());
    let max = ctx.input(|i| i.max_texture_side).min(1024);
    let t = ctx.load_texture("loaded_thumb", crate::widgets::fit_texture(ci, max), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, t.clone()));
    Some(t)
}

/// Presses in a row on the canvas: the last press's count, and where and when it was released
/// (`at` is −∞ while the button is down).
#[derive(Clone, Copy, Debug)]
struct Clicks {
    at: f64,
    pos: Pos2,
    n: u8,
}

#[cfg(test)]
#[path = "parent_canvas_tests.rs"]
mod parent_canvas_tests;

#[cfg(test)]
#[path = "guide_selection_tests.rs"]
mod guide_selection_tests;

#[cfg(test)]
mod tests;
