use super::rulers::ruler_pieces_of;
use super::view::current_slot;
use super::{CanvasLayout, DRect, DesignApp, Document, GuideSel, Overlay, Point, Pos2, Rect, SpreadRef, SpreadSlot, Xf, json, pos2};

pub(super) const GUIDE_DRAG: &str = "canvas_guide_drag";

/// A ruler guide being dragged: out of a ruler (`from` None) or an existing one.
#[derive(Clone, Debug)]
pub(super) struct GuideDrag {
    pub(super) orientation: designcraft_doc::Orientation,
    pub(super) from: Option<(designcraft_doc::SpreadRef, usize, usize)>,
    /// Pointer position (screen).
    pub(super) at: Option<Pos2>,
    /// Modifiers held at the pointer's last move: Shift snaps to the ruler's ticks, Command
    /// suspends snapping.
    pub(super) mods: egui::Modifiers,
}

pub(super) const GUIDE_PRESS: &str = "canvas_guide_press";

/// A press on an existing guide: where it was (screen) and whether Shift was down. Released
/// where it was pressed, it is a click, which selects the guide.
#[derive(Clone, Copy, Debug)]
pub(super) struct GuidePress {
    pub(super) origin: Pos2,
    pub(super) shift: bool,
}

/// How far (screen pixels) the pointer may move between press and release for a click.
const GUIDE_CLICK_SLOP: f32 = 3.0;

/// Where a dragged guide is, as the rulers measure it (#1282): `X: …` for a vertical guide, `Y: …`
/// for a horizontal one, from the zero point (and the page origin, when rulers restart per page)
/// in the document's ruler units. `None` with no document or rulers.
pub(super) fn guide_readout(app: &DesignApp, xf: &Xf, g: &GuideDrag) -> Option<String> {
    let (label, value, unit) = guide_measure(app, xf, g)?;
    Some(format!("{label}: {}", designcraft_geom::units::format_measure(value, unit)))
}

/// Where a pointer readout of `size` goes for a pointer at `p`: below right of it, on the other
/// side where that would leave the canvas `area` (near its right or bottom edge), and kept inside
/// it (with the label's 3-point frame) however small the canvas is.
pub(super) fn readout_at(p: Pos2, size: egui::Vec2, area: Rect) -> Pos2 {
    const GAP: f32 = 12.0;
    const PAD: f32 = 3.0;
    let x = if p.x + GAP + size.x + PAD > area.max.x { p.x - GAP - size.x } else { p.x + GAP };
    let y = if p.y + GAP + size.y + PAD > area.max.y { p.y - GAP - size.y } else { p.y + GAP };
    let clamp = |v: f32, lo: f32, hi: f32| if hi < lo { lo } else { v.clamp(lo, hi) };
    pos2(clamp(x, area.min.x + PAD, area.max.x - PAD - size.x), clamp(y, area.min.y + PAD, area.max.y - PAD - size.y))
}

/// [`guide_readout`]'s parts: the axis label, the value (points) and its unit.
pub(super) fn guide_measure(app: &DesignApp, xf: &Xf, g: &GuideDrag) -> Option<(&'static str, f64, designcraft_geom::Unit)> {
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
pub(super) struct GuideSpot {
    /// The spread under the pointer.
    spread: SpreadRef,
    /// The pointer in that spread's coordinates.
    pointer: Point,
    /// [`Self::pointer`] with the guide's axis snapped.
    pub(super) sp: Point,
    /// [`Self::sp`] on screen, where the preview line goes.
    pub(super) screen: Pos2,
    /// Alignment marks (canvas coordinates) when an object or page edge or center caught it.
    pub(super) marks: Vec<Overlay>,
}

/// Where the guide of drag `g` is: its axis (x for a vertical guide, y for a horizontal one)
/// snapped as a moved object's is, to ruler guides, margins, columns, bleed, the document grid and
/// smart-guide targets (not the baseline grid, which would catch a horizontal guide almost
/// anywhere). Shift snaps to the ruler's ticks instead and Command suspends snapping. The guides
/// being dragged are not targets. An existing guide over another spread, where it
/// can't be dropped, stays at the pointer. The preview, the readout and the drop all read it.
/// `None` with no spread under the pointer.
pub(super) fn guide_spot(app: &DesignApp, xf: &Xf, g: &GuideDrag) -> Option<GuideSpot> {
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
        loaded: None,
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
pub(super) fn guide_at(app: &DesignApp, xf: &Xf, p: Pos2) -> Option<(designcraft_doc::SpreadRef, usize, usize, designcraft_doc::Orientation)> {
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
pub(super) fn guide_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
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
pub(super) fn guide_drag_delta(app: &DesignApp, xf: &Xf, g: &GuideDrag, origin: Pos2) -> Option<(f64, f64)> {
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
pub(super) struct MovingGuides {
    pub(super) guides: std::collections::HashSet<GuideSel>,
    pub(super) dx: f64,
    pub(super) dy: f64,
}

/// What a drag of an existing guide moves, for the preview: the dragged guide, or every selected
/// guide when it is one of several selected. `None` before the drag leaves the click distance,
/// over a ruler (a drop there deletes) and off the guide's spread.
pub(super) fn moving_guides(app: &DesignApp, xf: &Xf, rect: Rect, g: &GuideDrag, press: GuidePress) -> Option<MovingGuides> {
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
pub(super) fn over_item(app: &DesignApp, xf: &Xf, p: Pos2) -> bool {
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
        loaded: None,
    };
    cx.hit(xf.to_canvas(p)).is_some()
}
