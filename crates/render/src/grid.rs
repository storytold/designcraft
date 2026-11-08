//! The document grid on screen (View › Grids & Guides › Show/Hide Document Grid).
//!
//! The lines are the positions Snap to Document Grid snaps to: every `vertical / subdivisions`
//! on x and `horizontal / subdivisions` on y, counted from the top-left corner of the page that
//! owns that x (`Spread::page_at_x`: a page owns its own width, the outer pages also own the
//! pasteboard beside them).

use designcraft_doc::{BaselineGrid, DocumentGrid, Spread};
use designcraft_geom::Rect;
use vello_cpu::{RenderContext, peniko};

use crate::Frame;

/// Lines closer together than this many device pixels are left out: subdivisions first, then
/// major lines.
const MIN_GAP_PX: f64 = 6.0;
/// Most lines drawn on one axis of one page column. A denser grid draws nothing on that axis.
const MAX_LINES: f64 = 4096.0;

/// The step between drawn lines on one axis and how many steps make a major line, or `None`
/// when the spacing is unusable or every line would be closer than [`MIN_GAP_PX`].
fn axis_step(major: f64, subdivisions: u32, px: f64) -> Option<(f64, i64)> {
    if !major.is_finite() || major <= 0.0 || !px.is_finite() || px <= 0.0 {
        return None;
    }
    let sub = subdivisions.max(1);
    let minor = major / f64::from(sub);
    if minor.is_finite() && minor / px >= MIN_GAP_PX {
        return Some((minor, i64::from(sub)));
    }
    (major / px >= MIN_GAP_PX).then_some((major, 1))
}

/// Lines `origin + n × step` in `[lo, hi)`, each with whether it is a major line.
fn lines(origin: f64, (step, every): (f64, i64), lo: f64, hi: f64) -> Vec<(f64, bool)> {
    let n0 = ((lo - origin) / step).ceil();
    let n1 = ((hi - origin) / step).ceil() - 1.0;
    if !n0.is_finite() || !n1.is_finite() || n1 < n0 || n1 - n0 >= MAX_LINES {
        return vec![];
    }
    let (n0, n1) = (n0 as i64, n1 as i64);
    (n0..=n1).map(|n| (n as f64 * step + origin, n.rem_euclid(every.max(1)) == 0)).collect()
}

/// Page columns of `sp` across `[x0, x1)`: each x range with the bounds origin of its page.
fn columns(sp: &Spread, x0: f64, x1: f64) -> Vec<(f64, f64, f64, f64)> {
    let mut cuts: Vec<f64> = vec![x0, x1];
    for p in &sp.pages {
        for x in [p.x, p.x + p.width] {
            if x > x0 && x < x1 {
                cuts.push(x);
            }
        }
    }
    cuts.retain(|x| x.is_finite());
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    cuts.windows(2)
        .filter_map(|w| {
            let (a, b) = (*w.first()?, *w.get(1)?);
            let page = sp.pages.get(sp.page_at_x((a + b) / 2.0)?)?;
            let o = page.bounds();
            Some((a, b, o.x0, o.y0))
        })
        .collect()
}

/// Draws the grid over `area` (spread coordinates) of `sp`, clipped to the visible area.
pub(crate) fn draw(ctx: &mut RenderContext, f: &Frame, sp: &Spread, grid: &DocumentGrid, area: Rect) {
    let area = area.intersect(f.visible);
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return;
    }
    if let Some(xs) = axis_step(grid.vertical, grid.subdivisions, f.px) {
        draw_columns(ctx, f, sp, grid, area, xs);
    }
    if let Some(ys) = axis_step(grid.horizontal, grid.subdivisions, f.px) {
        draw_rows(ctx, f, sp, grid, area, ys);
    }
}

/// Draws the baseline grid of `sp` over `area` (spread coordinates) — one rule per increment across
/// each page, counted from where that page's grid starts — clipped to what is visible.
pub(crate) fn draw_baseline(ctx: &mut RenderContext, f: &Frame, sp: &Spread, grid: &BaselineGrid, area: Rect) {
    let area = area.intersect(f.visible);
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return;
    }
    let half = f.px / 2.0;
    let [r, g, b] = grid.color;
    ctx.set_transform(f.view);
    ctx.set_paint(crate::screen_color([r, g, b], 210));
    for p in &sp.pages {
        let pb = p.bounds();
        let (x0, x1) = (pb.x0.max(area.x0), pb.x1.min(area.x1));
        if x1 <= x0 {
            continue;
        }
        for y in baseline_lines(grid.origin(p), grid.increment, p.height) {
            let y = pb.y0 + y;
            if y >= area.y0 && y <= area.y1 {
                ctx.fill_rect(&Rect::new(x0, y - half, x1, y + half));
            }
        }
    }
}

/// The most baseline grid lines drawn on a page: one per point of the tallest page (216 in).
const MAX_BASELINES: usize = 216 * 72;

/// Baseline grid lines inside a page, measured from its top: `start + n × increment` for n >= 0 (the
/// bottom edge is not a line), `start` from [`BaselineGrid::origin`](designcraft_doc::BaselineGrid::origin).
/// Lines above the page are skipped without walking them, however far up the start is.
fn baseline_lines(start: f64, increment: f64, height: f64) -> impl Iterator<Item = f64> {
    let step = increment.max(1.0);
    let first = if start < 0.0 { start.rem_euclid(step) } else { start };
    (0..MAX_BASELINES).map(move |n| first + n as f64 * step).take_while(move |y| *y < height)
}

fn paints(grid: &DocumentGrid) -> [peniko::Color; 2] {
    let [r, g, b] = grid.color;
    [crate::screen_color([r, g, b], 110), crate::screen_color([r, g, b], 255)]
}

/// Vertical lines.
fn draw_columns(ctx: &mut RenderContext, f: &Frame, sp: &Spread, grid: &DocumentGrid, area: Rect, xs: (f64, i64)) {
    let half = f.px / 2.0;
    ctx.set_transform(f.view);
    let cols = columns(sp, area.x0, area.x1);
    for (major, paint) in [false, true].into_iter().zip(paints(grid)) {
        ctx.set_paint(paint);
        for &(a, b, ox, _) in &cols {
            for (x, m) in lines(ox, xs, a, b) {
                if m == major {
                    ctx.fill_rect(&Rect::new(x - half, area.y0, x + half, area.y1));
                }
            }
        }
    }
}

/// Horizontal lines, each page column from its own page's top.
fn draw_rows(ctx: &mut RenderContext, f: &Frame, sp: &Spread, grid: &DocumentGrid, area: Rect, ys: (f64, i64)) {
    let half = f.px / 2.0;
    ctx.set_transform(f.view);
    let cols = columns(sp, area.x0, area.x1);
    for (major, paint) in [false, true].into_iter().zip(paints(grid)) {
        ctx.set_paint(paint);
        for &(a, b, _, oy) in &cols {
            for (y, m) in lines(oy, ys, area.y0, area.y1) {
                if m == major {
                    ctx.fill_rect(&Rect::new(a, y - half, b, y + half));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unusable_spacing_draws_nothing_and_dense_grids_thin_out() {
        for bad in [0.0, -9.0, f64::NAN, f64::INFINITY] {
            assert_eq!(axis_step(bad, 8, 1.0), None);
        }
        assert_eq!(axis_step(72.0, 8, 1.0), Some((9.0, 8)));
        // 9 pt subdivisions at 2 pt per pixel are 4.5 px apart: only major lines.
        assert_eq!(axis_step(72.0, 8, 2.0), Some((72.0, 1)));
        assert_eq!(axis_step(72.0, 0, 1.0), Some((72.0, 1)));
        assert_eq!(axis_step(72.0, 8, 100.0), None);
        // A span too wide for the cap draws no lines instead of looping.
        assert!(lines(0.0, (1e-9, 1), 0.0, 1e9).is_empty());
        assert_eq!(lines(10.0, (9.0, 8), 0.0, 30.0), vec![(1.0, false), (10.0, true), (19.0, false), (28.0, false)]);
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_BASELINES, baseline_lines};

    #[test]
    fn the_baseline_rules_are_the_ones_on_the_page() {
        // A start far above the page used to be walked line by line, without end.
        for start in [-1e9, -1e17] {
            let mut far = baseline_lines(start, 12.0, 792.0);
            assert_eq!((far.next(), far.count()), (Some(8.0), 65));
        }
        let lines = |start, increment, height| baseline_lines(start, increment, height).collect::<Vec<_>>();
        assert_eq!(lines(36.0, 12.0, 72.0), [36.0, 48.0, 60.0], "the bottom edge is not a line");
        assert_eq!(lines(-30.0, 12.0, 30.0), [6.0, 18.0], "a start above the page keeps its rhythm");
        assert_eq!(lines(f64::NAN, 12.0, 792.0), [0.0; 0]);
        assert_eq!(baseline_lines(0.0, 12.0, f64::INFINITY).count(), MAX_BASELINES);
    }
}
