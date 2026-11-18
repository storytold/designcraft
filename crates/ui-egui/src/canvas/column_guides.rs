use super::guides::{guide_at, over_item};
use super::rulers::ruler_pieces;
use super::{CanvasLayout, DRect, DesignApp, Document, Point, Pos2, Rect, ScreenMode, SpreadRef, Xf, json};

pub(super) const COLUMN_DRAG: &str = "canvas_column_drag";

/// A column guide being dragged with a Selection tool: its gutter follows the pointer.
#[derive(Clone, Debug)]
pub(super) struct ColumnDrag {
    /// The document the drag's interaction belongs to.
    doc: u64,
    pub(super) spread: SpreadRef,
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
pub(super) fn column_guide_at(app: &DesignApp, xf: &Xf, p: Pos2) -> Option<(SpreadRef, usize, usize, f64)> {
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
pub(super) fn spread_x_across(app: &DesignApp, r: SpreadRef) -> bool {
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
        loaded: None,
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
pub(super) fn column_guide_drag(app: &mut DesignApp, ui: &mut egui::Ui, resp: &egui::Response, rect: Rect, xf: &Xf) -> bool {
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
