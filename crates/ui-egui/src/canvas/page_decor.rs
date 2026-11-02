use super::guides::MovingGuides;
use super::render::c32;
use super::rulers::hair;
use super::{
    CanvasLayout, Color32, DRect, DesignApp, Document, GuideSel, Item, Point, Pos2, Rect, ScreenMode, SpreadRef, SpreadSlot, Stroke, StrokeKind,
    Tokens, Xf,
};

pub(super) fn dashed(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke, dash: f32, gap: f32) {
    painter.extend(egui::Shape::dashed_line(&[a, b], stroke, dash, gap));
}

/// The most baseline grid lines drawn on a page: one per point of the tallest page (216 in).
pub(super) const MAX_BASELINES: usize = 216 * 72;

/// Baseline grid lines inside a page, measured from its top: `start + n * increment` for n >= 0 (the bottom
/// edge is not a line), `start` from [`BaselineGrid::origin`](designcraft_doc::BaselineGrid::origin). Lines
/// above the page are skipped without walking them, however far up the start is.
pub(super) fn baseline_grid_lines(start: f64, increment: f64, height: f64) -> impl Iterator<Item = f64> {
    let step = increment.max(1.0);
    let first = if start < 0.0 { start.rem_euclid(step) } else { start };
    (0..MAX_BASELINES).map(move |n| first + n as f64 * step).take_while(move |y| *y < height)
}

/// What a screen mode shows of a spread, in screen coordinates: the trim, or out to the bleed or
/// the slug box. Slug shows every printing element, so the bleed as well — and a slug narrower
/// than the bleed does not pull the edge back in.
pub(super) fn shown_area(app: &DesignApp, doc: &Document, xf: &Xf, slot: &SpreadSlot) -> Rect {
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
pub(super) fn draw_guides(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    t: &Tokens,
    moving: Option<&MovingGuides>,
) {
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
            let cx = GuideCtx { painter, xf, doc, slot, sp, pasteboard };
            draw_page_guides(&cx, (pi, p, pr), moving, &selection);
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

/// What one page's guides are drawn against.
struct GuideCtx<'a> {
    painter: &'a egui::Painter,
    xf: &'a Xf,
    doc: &'a Document,
    slot: &'a SpreadSlot,
    sp: &'a designcraft_doc::Spread,
    pasteboard: DRect,
}

/// The page's own guides, then those it shows from its parent page. `moving` draws dragged
/// guides where they would be dropped. `page`: its index in the spread, the page, and its bounds.
fn draw_page_guides(
    cx: &GuideCtx,
    page: (usize, &designcraft_doc::Page, DRect),
    moving: Option<&MovingGuides>,
    selection: &std::collections::HashSet<GuideSel>,
) {
    let GuideCtx { doc, slot, .. } = *cx;
    let (pi, p, pr) = page;
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
        draw_guide(cx, pr, &g, selected);
    }
}

/// One guide as a line: across the pasteboard for a spread guide, across its page otherwise.
fn draw_guide(cx: &GuideCtx, pr: DRect, g: &designcraft_doc::Guide, selected: bool) {
    let GuideCtx { painter, xf, doc, slot, sp, pasteboard } = *cx;
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

/// View › Flattener Preview: red over every object that involves transparency (opacity, blend
/// modes, effects, knockout) — what flattening or export rasterising affects.
pub(super) fn draw_flattener_preview(painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
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
