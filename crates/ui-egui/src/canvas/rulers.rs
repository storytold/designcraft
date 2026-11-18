use super::input::pointer_elsewhere;
use super::page_decor::dashed;
use super::view::current_slot;
use super::{CanvasLayout, DesignApp, Document, Point, Pos2, RULER, Rect, SpreadSlot, Stroke, Tokens, Value, Xf, json, pos2, vec2};

/// Paint ruler bands and ticks, the zero-point crosshair, guide previews, and ticks restarted at each page origin.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_rulers(app: &DesignApp, ui: &egui::Ui, full: Rect, rect: Rect, xf: &Xf, doc: &Document, layout: &CanvasLayout, t: &Tokens) {
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
pub(super) fn ruler_pieces(app: &DesignApp, doc: &Document, layout: &CanvasLayout, zp: [f64; 2]) -> Vec<(f64, f64, Point)> {
    let Some(slot) = current_slot(app, layout).and_then(|i| layout.slots.get(i)) else { return Vec::new() };
    ruler_pieces_of(doc, slot, zp)
}

/// [`ruler_pieces`] for the spread in `slot`.
pub(super) fn ruler_pieces_of(doc: &Document, slot: &SpreadSlot, zp: [f64; 2]) -> Vec<(f64, f64, Point)> {
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
pub(super) fn ruler_units_menus(app: &mut DesignApp, resp: &egui::Response, full: Rect, rect: Rect, doc: &Document) {
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

pub(crate) fn fmt_tick(v: f64) -> String {
    let r = v.round();
    if (v - r).abs() < 1e-6 { format!("{}", r as i64) } else { format!("{v:.1}") }
}

/// One device pixel, the width InDesign uses for guides and frame edges.
pub(super) fn hair(painter: &egui::Painter) -> f32 {
    1.0 / painter.ctx().pixels_per_point()
}

const ZERO_POINT_DRAG: &str = "canvas_zero_point_drag";

/// The ruler corner: drag from it and the zero point lands where the pointer is released; a click
/// that stays in the corner leaves it; a double-click resets it. Locked, the corner ignores both.
/// Cmd-dragged (Ctrl elsewhere), it makes a horizontal and a vertical guide instead.
/// Holds the pointer position while dragging; returns true while it owns the pointer.
pub(super) fn zero_point_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
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
