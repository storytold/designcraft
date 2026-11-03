//! Snapping and smart guides.
//!
//! [`snap`] tries each pass on a free axis and keeps the first hit inside the snap zone; the
//! baseline grid and the guides (ruler guides, margins, columns) count as one pass where the
//! nearer target wins.
//! Ruler guides, margins, and columns draw nothing. Alignment draws [`Overlay::Guide`] in
//! canvas coordinates. Equal spacing draws [`Overlay::Gap`] the same way, through one
//! spread-to-canvas transform. A rotation match draws [`Overlay::Measure`] at the pointer
//! the same way.

use std::collections::HashSet;

use designcraft_doc::{Document, Item, ItemId, Orientation, Selection, SpreadRef, StrokeAlign};
use designcraft_geom::snap::snap_to_grid;
use designcraft_geom::{Affine, Point, Rect, Vec2, format_measure};

pub use crate::{Gesture, Snap, SnapRequest, SnapView};

use crate::{Overlay, ToolContext};

/// Snap a single point (drawing tools).
///
/// Zero-size rect, and every edge flag on, so the point can meet a guide or an alignment target.
pub fn snap_point(cx: &ToolContext, sr: SpreadRef, p: Point) -> (Point, Vec<Overlay>) {
    let s = snap(
        cx,
        SnapRequest {
            spread: sr,
            gesture: Gesture::Point,
            rect: Rect::from_points(p, p),
            x_edges: [true, true, true],
            y_edges: [true, true, true],
            exclude: &[],
            exclude_guides: &[],
            copying: false,
            lengths: [None, None],
            angle: None,
            radius: 0.0,
            pointer: p,
        },
    );
    (p + s.delta, s.guides)
}

fn rect_finite(r: Rect) -> bool {
    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
}

/// First hit on each free axis.
///
/// A non-finite rect, or no snap category, returns no correction. An axis whose three edge
/// flags are all false stays at 0 and draws nothing. The zone is [`SnapView::zone_px`] screen
/// pixels, converted with [`ToolContext::tol`]. A zone of 0 or less is no hit.
/// Rotate does not run the position passes, so a locked axis still leaves room for [`Snap::angle`].
pub fn snap(cx: &ToolContext, req: SnapRequest<'_>) -> Snap {
    if !rect_finite(req.rect) || !cx.snap.any() {
        return Snap::default();
    }
    let Some(tol) = zone_tol(cx) else {
        return Snap::default();
    };
    // Position passes would ignore a rotate whose edge flags are all false, and a stray
    // edge flag must not move the object. Only the angle match runs.
    if matches!(req.gesture, Gesture::Rotate) {
        return rotation_snap(cx, &req, tol);
    }
    // Three false flags lock the axis: do not move it and do not draw a guide for it.
    let x_free = req.x_edges != [false; 3];
    let y_free = req.y_edges != [false; 3];
    let x_hit = x_free.then(|| first_on_axis(cx, &req, Axis::X, tol)).flatten();
    let y_hit = y_free.then(|| first_on_axis(cx, &req, Axis::Y, tol)).flatten();
    let mut out = Snap::default();
    if let Some(hit) = &x_hit {
        out.delta.x = hit.delta;
        out.length_delta[0] = hit.length_delta;
    }
    if let Some(hit) = &y_hit {
        out.delta.y = hit.delta;
        out.length_delta[1] = hit.length_delta;
    }
    let placed = Rect::new(req.rect.x0 + out.delta.x, req.rect.y0 + out.delta.y, req.rect.x1 + out.delta.x, req.rect.y1 + out.delta.y);
    if let Some(hit) = &x_hit {
        push_hit_guides(&mut out.guides, cx, &req, Axis::X, hit, placed);
    }
    if let Some(hit) = &y_hit {
        push_hit_guides(&mut out.guides, cx, &req, Axis::Y, hit, placed);
    }
    out
}

/// Move the flagged edges by `delta`.
///
/// One edge takes the whole delta. Both edges of an axis split it, low edge one way and high
/// edge the other, so a length match keeps the centre. A non-finite component is ignored.
pub(crate) fn nudge_edges(mut rect: Rect, x_edges: [bool; 3], y_edges: [bool; 3], delta: Vec2) -> Rect {
    nudge_axis(&mut rect.x0, &mut rect.x1, x_edges, delta.x);
    nudge_axis(&mut rect.y0, &mut rect.y1, y_edges, delta.y);
    rect
}

fn nudge_axis(lo: &mut f64, hi: &mut f64, flags: [bool; 3], delta: f64) {
    if !delta.is_finite() {
        return;
    }
    let [low_edge, _, high_edge] = flags;
    if low_edge && high_edge {
        *lo -= delta;
        *hi += delta;
    } else if low_edge {
        *lo += delta;
    } else if high_edge {
        *hi += delta;
    }
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

#[derive(Clone)]
struct AxisHit {
    delta: f64,
    /// Position of the winning line on this axis.
    at: f64,
    /// Target box for an alignment guide. None when the hit draws nothing.
    span: Option<Rect>,
    /// Matched length minus the proposed length, when this hit is a dimension match.
    length_delta: Option<f64>,
    /// Spread-space segment along the matched side, plus the measure text.
    dimension: Option<DimMark>,
    /// Spacing segments in spread space. Empty unless this hit is a spacing match.
    gaps: Vec<GapMark>,
}

/// Spacing segment in spread space. Drawn after one spread-to-canvas transform.
#[derive(Clone)]
struct GapMark {
    a: Point,
    b: Point,
    label: String,
}

#[derive(Clone)]
struct DimMark {
    a: Point,
    b: Point,
    text: String,
}

struct Best {
    dist: f64,
    hit: AxisHit,
}

/// Screen pixels to spread points. A zone of 0 or less, or a non-finite zone, is no hit.
fn zone_tol(cx: &ToolContext) -> Option<f64> {
    let zone = cx.snap.zone_px;
    if !zone.is_finite() || zone <= 0.0 {
        return None;
    }
    let tol = cx.tol(zone);
    if tol.is_finite() && tol > 0.0 { Some(tol) } else { None }
}

/// Matched rotation, in the same degrees `transform.rotate` receives.
///
/// Runs only for [`Gesture::Rotate`], and only when Smart Guides and Smart Dimensions are on.
/// `angle` is compared with `decompose` of the absolute transform in the same sign. That
/// transform is the ancestor chain composed with `item.xf`. A top-level item has no ancestor,
/// so it stays `decompose(item.xf)`. The tolerance is the zone length subtended at `radius`
/// (`atan(zone / radius)` in degrees). A radius below 1 pt, or a non-finite angle, radius, or
/// zone, does not hit. Candidates are visible items on the requested spread, including children
/// of a group, plus shown parent items. A child inside a rotated group uses the composed angle,
/// not its local 0. Hidden items and the shared exclude list are skipped. An unrotated item
/// contributes 0. The closest angle wins, folding whole turns so a few degrees across 0 still
/// match. The measure is the angle in degrees and there is no guide line.
fn rotation_snap(cx: &ToolContext, req: &SnapRequest<'_>, zone_length: f64) -> Snap {
    let Some(angle) = match_rotation(cx, req, zone_length) else {
        return Snap::default();
    };
    let mut guides = Vec::new();
    let p = cx.layout.xf(req.spread) * req.pointer;
    if point_finite(p) {
        guides.push(Overlay::Measure { p, text: format_angle(angle) });
    }
    Snap { angle: Some(angle), guides, ..Snap::default() }
}

fn match_rotation(cx: &ToolContext, req: &SnapRequest<'_>, zone_length: f64) -> Option<f64> {
    if !matches!(req.gesture, Gesture::Rotate) || !cx.snap.smart_guides || !cx.snap.smart_dimensions {
        return None;
    }
    let proposed = req.angle?;
    if !proposed.is_finite() || !req.radius.is_finite() || req.radius < 1.0 || !zone_length.is_finite() || zone_length <= 0.0 {
        return None;
    }
    let tol = (zone_length / req.radius).atan().to_degrees();
    if !tol.is_finite() {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    let mut seen = HashSet::new();
    if let Some(spread) = cx.doc.spread(req.spread) {
        for item in &spread.items {
            push_rotation(&mut best, cx.selection, req, item, Walk::ROOT, &mut seen, (proposed, tol));
        }
    }
    if shows_parent_items(cx, req) {
        push_parent_rotations(&mut best, cx, req, &mut seen, proposed, tol);
    }
    best.map(|(_, angle)| angle)
}

/// Shown parent items. Parent items are only translated onto the page, so the angle is still `item.xf`.
fn push_parent_rotations(
    best: &mut Option<(f64, f64)>,
    cx: &ToolContext,
    req: &SnapRequest<'_>,
    seen_items: &mut HashSet<ItemId>,
    proposed: f64,
    tol: f64,
) {
    for (item, _) in shown_parent_items(cx.doc, req.spread) {
        push_rotation(best, cx.selection, req, item, Walk::ROOT, seen_items, (proposed, tol));
    }
}

/// Where a recursive walk over an item tree stands: the parent's absolute transform, whether an
/// ancestor was skipped, and how deep it is.
#[derive(Clone, Copy)]
struct Walk {
    ancestor: Affine,
    skipped: bool,
    depth: usize,
}

impl Walk {
    const ROOT: Walk = Walk { ancestor: Affine::IDENTITY, skipped: false, depth: 0 };

    fn child(self, ancestor: Affine, skipped: bool) -> Walk {
        Walk { ancestor, skipped, depth: self.depth + 1 }
    }
}

fn push_rotation(
    best: &mut Option<(f64, f64)>,
    selection: &Selection,
    req: &SnapRequest<'_>,
    item: &Item,
    walk: Walk,
    seen: &mut HashSet<ItemId>,
    (proposed, tol): (f64, f64),
) {
    let Walk { ancestor, skipped, depth } = walk;
    if depth >= ALIGN_GROUP_DEPTH || !seen.insert(item.id) || item.hidden {
        return;
    }
    let abs = ancestor * item.xf;
    let skip = skipped || align_skips_item(selection, req, item.id);
    if !skip && let Some(angle) = rotation_of(abs) {
        let dist = angular_distance(proposed, angle);
        if dist.is_finite() && dist <= tol && best.is_none_or(|(have, _)| dist < have) {
            *best = Some((dist, angle));
        }
    }
    if !xf_finite(abs) {
        return;
    }
    for child in item.children() {
        push_rotation(best, selection, req, child, walk.child(abs, skip), seen, (proposed, tol));
    }
}

/// Decompose rotation of an absolute transform. A non-finite matrix contributes nothing.
fn rotation_of(xf: Affine) -> Option<f64> {
    if !xf_finite(xf) {
        return None;
    }
    let rotation = designcraft_geom::decompose::decompose(xf).rotation;
    rotation.is_finite().then_some(rotation)
}

/// Decompose rotation of the first selected item's own `xf`, captured before a preview changes it.
/// None when nothing is selected. The command composes onto that `xf`, so ancestors stay out.
pub(crate) fn reference_rotation(cx: &ToolContext) -> Option<f64> {
    let id = *cx.selection.items.first()?;
    rotation_of(cx.doc.item(id)?.xf)
}

/// Degrees folded into the range `decompose` reports, `(-180, 180]`.
pub(crate) fn folded_rotation(deg: f64) -> Option<f64> {
    if !deg.is_finite() {
        return None;
    }
    let mut d = deg.rem_euclid(360.0);
    if !d.is_finite() {
        return None;
    }
    if d > 180.0 {
        d -= 360.0;
    }
    if d <= -180.0 + 1e-9 {
        d = 180.0;
    }
    d.is_finite().then_some(d)
}

/// Delta `transform.rotate` should apply so the reference lands on `snapped`.
///
/// `transform.rotate` composes, so the command is `snapped - start`, not `snapped`.
/// Whole turns are added so the result stays nearest the raw command delta.
pub(crate) fn rotation_command_delta(start: f64, snapped: f64, raw: f64) -> Option<f64> {
    if !start.is_finite() || !snapped.is_finite() || !raw.is_finite() {
        return None;
    }
    let base = snapped - start;
    if !base.is_finite() {
        return None;
    }
    let turns = ((raw - base) / 360.0).round();
    if !turns.is_finite() {
        return None;
    }
    let delta = base + turns * 360.0;
    delta.is_finite().then_some(delta)
}

/// Smallest absolute difference in degrees, folded into `0..=180`.
fn angular_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if !d.is_finite() {
        return f64::NAN;
    }
    d.min(360.0 - d)
}

/// Whole degrees have no decimal. Anything else prints one decimal place.
fn format_angle(deg: f64) -> String {
    let nearest = deg.round();
    if nearest.is_finite() && (deg - nearest).abs() < 1e-6 { format!("{nearest:.0}") } else { format!("{deg:.1}") }
}

/// The baseline grid comes last: its lines are dense and it snaps while hidden, so ahead of
/// smart guides it would take nearly every vertical move and draw nothing (#188). Only when a
/// guide or margin is in reach does the nearer of it and the baseline grid win.
fn first_on_axis(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    pass_grid(cx, req, axis, tol)
        .or_else(|| pass_guides(cx, req, axis, tol).and_then(|g| nearer(pass_baseline(cx, req, axis, tol), Some(g))))
        .or_else(|| pass_align(cx, req, axis, tol))
        .or_else(|| pass_spacing(cx, req, axis, tol))
        .or_else(|| pass_dimensions(cx, req, axis, tol))
        .or_else(|| pass_baseline(cx, req, axis, tol))
}

/// The baseline grid and the guides (ruler guides, margins, columns) compete by distance, so a
/// margin is reachable between the lines of a fine baseline grid at any zoom. A tie goes to the
/// guide.
fn nearer(baseline: Option<AxisHit>, guide: Option<AxisHit>) -> Option<AxisHit> {
    match (baseline, guide) {
        (Some(b), Some(g)) => Some(if b.delta.abs() < g.delta.abs() { b } else { g }),
        (b, g) => g.or(b),
    }
}

/// Document grid. No overlay.
///
/// The interval is the major spacing divided by the subdivision count. A count below 1 counts
/// as 1. `grid.vertical` spaces the vertical lines (x). `grid.horizontal` spaces the horizontal
/// lines (y). A spacing of 0 or less skips that axis. The origin is the page-bounds origin of
/// the page under the rect center. Whether the grid is shown is not read.
fn pass_grid(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !cx.snap.snap_to_document_grid {
        return None;
    }
    let spacing = grid_interval(cx, axis)?;
    let origin = grid_origin(cx, req, axis)?;
    let mut best: Option<Best> = None;
    offer_enabled(&mut best, req.rect, axis_flags(req, axis), axis, tol, |v| Some(snap_to_grid(v, spacing, origin)));
    best.map(|b| b.hit)
}

/// Major spacing divided by the subdivision count. Non-finite or non-positive spacing is no hit.
fn grid_interval(cx: &ToolContext, axis: Axis) -> Option<f64> {
    let grid = &cx.doc.settings.grid;
    let major = match axis {
        Axis::X => grid.vertical,
        Axis::Y => grid.horizontal,
    };
    if !major.is_finite() || major <= 0.0 {
        return None;
    }
    let interval = major / f64::from(grid.subdivisions.max(1));
    (interval.is_finite() && interval > 0.0).then_some(interval)
}

/// Page-bounds origin on `axis` for the page under the rect center.
fn grid_origin(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis) -> Option<f64> {
    let spread = cx.doc.spread(req.spread)?;
    let bounds = spread.pages.get(spread.page_at_x(req.rect.center().x)?)?.bounds();
    let origin = match axis {
        Axis::X => bounds.x0,
        Axis::Y => bounds.y0,
    };
    origin.is_finite().then_some(origin)
}

/// Baseline grid. No overlay.
///
/// Lines are `page.y0 + origin + n * increment` for n >= 0 while the line is inside the page
/// (the bottom edge is not a line, matching the canvas), `origin` from
/// [`BaselineGrid::origin`](designcraft_doc::BaselineGrid::origin). An increment of 0 or less
/// skips the pass. `view_threshold` is not read, and hiding the baseline grid does not turn this
/// off. Only a page the rect crosses on x contributes. A dragged ruler guide
/// ([`Gesture::Guide`]) doesn't snap to it.
fn pass_baseline(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !matches!(axis, Axis::Y) || !cx.snap.snap_to_guides || matches!(req.gesture, Gesture::Guide) {
        return None;
    }
    let baseline = &cx.doc.settings.baseline_grid;
    let increment = baseline.increment;
    if !increment.is_finite() || increment <= 0.0 || !baseline.start.is_finite() {
        return None;
    }
    let spread = cx.doc.spread(req.spread)?;
    let flags = axis_flags(req, axis);
    let (moving_lo, moving_hi) = perp_ends(req.rect, axis);
    let mut best: Option<Best> = None;
    for page in &spread.pages {
        let bounds = page.bounds();
        let (lo, hi) = perp_ends(bounds, axis);
        if !ranges_overlap(moving_lo, moving_hi, lo, hi) {
            continue;
        }
        let origin = bounds.y0 + baseline.origin(page);
        if !origin.is_finite() {
            continue;
        }
        offer_enabled(&mut best, req.rect, flags, axis, tol, |v| nearest_baseline(v, origin, increment, bounds.y0, bounds.y1));
    }
    best.map(|b| b.hit)
}

/// Nearest baseline at or after `origin`, strictly inside `[y0, y1)`.
fn nearest_baseline(v: f64, origin: f64, increment: f64, y0: f64, y1: f64) -> Option<f64> {
    let (n_min, n_max) = baseline_n_range(origin, increment, y0, y1)?;
    let snapped = snap_to_grid(v, increment, origin);
    if baseline_line_ok(snapped, origin, increment, y0, y1) {
        return Some(snapped);
    }
    let n = ((snapped - origin) / increment).round();
    if !n.is_finite() {
        return None;
    }
    let n = if n < n_min {
        n_min
    } else if n > n_max {
        n_max
    } else {
        n
    };
    let line = origin + n * increment;
    baseline_line_ok(line, origin, increment, y0, y1).then_some(line)
}

fn baseline_n_range(origin: f64, increment: f64, y0: f64, y1: f64) -> Option<(f64, f64)> {
    if !origin.is_finite() || !increment.is_finite() || increment <= 0.0 || !y0.is_finite() || !y1.is_finite() || y0 >= y1 {
        return None;
    }
    let mut n_min = if origin < y0 { ((y0 - origin) / increment).ceil() } else { 0.0 };
    if !n_min.is_finite() || n_min < 0.0 {
        return None;
    }
    if origin + n_min * increment < y0 {
        n_min += 1.0;
    }
    let mut n_max = ((y1 - origin) / increment).floor();
    if !n_max.is_finite() {
        return None;
    }
    if origin + n_max * increment >= y1 {
        n_max -= 1.0;
    }
    (n_max >= n_min).then_some((n_min, n_max))
}

fn baseline_line_ok(line: f64, origin: f64, increment: f64, y0: f64, y1: f64) -> bool {
    if !line.is_finite() || !increment.is_finite() || increment <= 0.0 || line < y0 || line >= y1 {
        return false;
    }
    let n = ((line - origin) / increment).round();
    n.is_finite() && n >= 0.0
}

/// Each edge whose flag is set, against its own target. Nothing is drawn.
fn offer_enabled(best: &mut Option<Best>, moving: Rect, flags: [bool; 3], axis: Axis, tol: f64, mut target_at: impl FnMut(f64) -> Option<f64>) {
    let (left, mid, right) = axis_triple(moving, axis);
    let [use_left, use_mid, use_right] = flags;
    for (on, v) in [(use_left, left), (use_mid, mid), (use_right, right)] {
        if !on {
            continue;
        }
        let Some(target) = target_at(v) else { continue };
        offer(best, v, target, tol, None);
    }
}

/// Equal gaps on a move, when smart guides and smart spacing are on.
///
/// Boxes are the top-level visible bounds on this spread. A group contributes the union of
/// its children. Parent items are not targets. `req.rect` is the moving box and is not also a
/// stationary box. A box counts only when its range overlaps on the other axis. The nearest
/// box on a side that overlaps the mover, or a zero gap, produces no gap, and a farther box
/// is not used instead. A candidate shifts the mover so one of its gaps equals one stationary
/// nearest-neighbor gap, or, with a box on both sides, so its two gaps are equal (centered
/// between them). The shift has to fall inside the zone. The closest shift wins.
/// One gap overlay is drawn for every gap of that length, including the moving gap after the
/// shift. Create, resize, rotate, and point do not match.
fn pass_spacing(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !matches!(req.gesture, Gesture::Move) || !cx.snap.smart_guides || !cx.snap.smart_spacing {
        return None;
    }
    if !tol.is_finite() || tol <= 0.0 {
        return None;
    }
    let mover = interval_on(req.rect, axis)?;
    let boxes = spacing_intervals(cx, req, axis, &mover);
    let stationary = stationary_gaps(&boxes);
    let low = nearest_side(&mover, &boxes, None, true);
    let high = nearest_side(&mover, &boxes, None, false);
    let mut best: Option<SpacingChoice> = None;
    if let Some(n) = &low {
        for g in &stationary {
            offer_spacing(&mut best, g.gap - n.gap, g.gap, tol);
        }
    }
    if let Some(n) = &high {
        for g in &stationary {
            offer_spacing(&mut best, n.gap - g.gap, g.gap, tol);
        }
    }
    if let (Some(l), Some(h)) = (&low, &high) {
        offer_spacing(&mut best, (h.gap - l.gap) * 0.5, (l.gap + h.gap) * 0.5, tol);
    }
    let choice = best?;
    let shifted = Interval { lo: mover.lo + choice.delta, hi: mover.hi + choice.delta, perp_lo: mover.perp_lo, perp_hi: mover.perp_hi };
    if !shifted.lo.is_finite() || !shifted.hi.is_finite() {
        return None;
    }
    let label = format_measure(choice.distance, cx.unit);
    let mut gaps = Vec::new();
    push_moving_gaps(&mut gaps, &shifted, &boxes, axis, choice.distance, &label);
    for g in &stationary {
        if gap_matches(g.gap, choice.distance) {
            push_gap_mark(&mut gaps, axis, g.left, g.right, g.perp_lo, g.perp_hi, &label);
        }
    }
    Some(AxisHit { delta: choice.delta, at: 0.0, span: None, length_delta: None, dimension: None, gaps })
}

/// Top-level visible boxes that overlap `mover` on the other axis.
///
/// The moving rect is not one of them. Parents are not walked.
fn spacing_intervals(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, mover: &Interval) -> Vec<Interval> {
    let mut out = Vec::new();
    let Some(sp) = cx.doc.spread(req.spread) else { return out };
    for item in &sp.items {
        let Some(rect) = item_target_rect(cx.selection, req, item) else { continue };
        let Some(span) = interval_on(rect, axis) else { continue };
        if ranges_overlap(mover.perp_lo, mover.perp_hi, span.perp_lo, span.perp_hi) {
            out.push(span);
        }
    }
    out
}

struct Interval {
    lo: f64,
    hi: f64,
    perp_lo: f64,
    perp_hi: f64,
}

struct SideGap {
    gap: f64,
    edge: f64,
    perp_lo: f64,
    perp_hi: f64,
}

struct StoredGap {
    gap: f64,
    left: f64,
    right: f64,
    perp_lo: f64,
    perp_hi: f64,
}

struct SpacingChoice {
    dist: f64,
    delta: f64,
    distance: f64,
}

fn interval_on(rect: Rect, axis: Axis) -> Option<Interval> {
    if !rect_finite(rect) {
        return None;
    }
    let (a0, a1, p0, p1) = match axis {
        Axis::X => (rect.x0, rect.x1, rect.y0, rect.y1),
        Axis::Y => (rect.y0, rect.y1, rect.x0, rect.x1),
    };
    Some(Interval { lo: a0.min(a1), hi: a0.max(a1), perp_lo: p0.min(p1), perp_hi: p0.max(p1) })
}

/// Nearest positive gap on one side of `mover`.
///
/// `low` looks toward decreasing coordinates. A box that overlaps that edge, or a zero gap,
/// is nearer than any open gap, so the side produces nothing and a farther box is not used.
/// `skip` drops a box that would otherwise be compared with itself.
fn nearest_side(mover: &Interval, boxes: &[Interval], skip: Option<usize>, low: bool) -> Option<SideGap> {
    let mut blocked = false;
    let mut best: Option<SideGap> = None;
    for (i, b) in boxes.iter().enumerate() {
        if skip == Some(i) || !ranges_overlap(mover.perp_lo, mover.perp_hi, b.perp_lo, b.perp_hi) {
            continue;
        }
        match side_gap(mover, b, low) {
            SideKind::Block => blocked = true,
            SideKind::Ignore => {}
            SideKind::Gap { gap, edge } => {
                if best.as_ref().is_none_or(|have| gap < have.gap) {
                    best = Some(SideGap { gap, edge, perp_lo: b.perp_lo, perp_hi: b.perp_hi });
                }
            }
        }
    }
    if blocked { None } else { best }
}

enum SideKind {
    Block,
    Ignore,
    Gap { gap: f64, edge: f64 },
}

fn side_gap(mover: &Interval, b: &Interval, low: bool) -> SideKind {
    let (gap, edge, crosses, past) = if low {
        (mover.lo - b.hi, b.hi, b.lo < mover.lo && b.hi > mover.lo, b.hi > mover.lo)
    } else {
        (b.lo - mover.hi, b.lo, b.lo < mover.hi && b.hi > mover.hi, b.lo < mover.hi)
    };
    if crosses {
        return SideKind::Block;
    }
    if past {
        return SideKind::Ignore;
    }
    if !gap.is_finite() {
        return SideKind::Ignore;
    }
    if gap <= 0.0 { SideKind::Block } else { SideKind::Gap { gap, edge } }
}

/// Nearest-neighbor gaps between stationary boxes that overlap on the other axis.
///
/// Only the high side of each box is recorded, so a pair is one gap. An overlap or a zero
/// gap on that side contributes nothing.
fn stationary_gaps(boxes: &[Interval]) -> Vec<StoredGap> {
    let mut out = Vec::new();
    for (i, a) in boxes.iter().enumerate() {
        let Some(n) = nearest_side(a, boxes, Some(i), false) else { continue };
        let Some((perp_lo, perp_hi)) = perp_span(a.perp_lo, a.perp_hi, n.perp_lo, n.perp_hi) else { continue };
        out.push(StoredGap { gap: n.gap, left: a.hi, right: n.edge, perp_lo, perp_hi });
    }
    out
}

fn perp_span(a0: f64, a1: f64, b0: f64, b1: f64) -> Option<(f64, f64)> {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    (lo.is_finite() && hi.is_finite() && lo <= hi).then_some((lo, hi))
}

fn offer_spacing(best: &mut Option<SpacingChoice>, delta: f64, distance: f64, tol: f64) {
    if !delta.is_finite() || !distance.is_finite() || distance <= 0.0 {
        return;
    }
    let dist = delta.abs();
    if dist <= tol && best.as_ref().is_none_or(|have| dist < have.dist) {
        *best = Some(SpacingChoice { dist, delta, distance });
    }
}

fn gap_matches(gap: f64, distance: f64) -> bool {
    gap.is_finite() && distance.is_finite() && (gap - distance).abs() <= 1e-6
}

/// Moving gaps after the shift. A side is drawn when its new nearest gap equals `distance`.
fn push_moving_gaps(out: &mut Vec<GapMark>, shifted: &Interval, boxes: &[Interval], axis: Axis, distance: f64, label: &str) {
    if let Some(n) = nearest_side(shifted, boxes, None, true)
        && gap_matches(n.gap, distance)
        && let Some((perp_lo, perp_hi)) = perp_span(shifted.perp_lo, shifted.perp_hi, n.perp_lo, n.perp_hi)
    {
        push_gap_mark(out, axis, n.edge, shifted.lo, perp_lo, perp_hi, label);
    }
    if let Some(n) = nearest_side(shifted, boxes, None, false)
        && gap_matches(n.gap, distance)
        && let Some((perp_lo, perp_hi)) = perp_span(shifted.perp_lo, shifted.perp_hi, n.perp_lo, n.perp_hi)
    {
        push_gap_mark(out, axis, shifted.hi, n.edge, perp_lo, perp_hi, label);
    }
}

fn push_gap_mark(out: &mut Vec<GapMark>, axis: Axis, left: f64, right: f64, perp_lo: f64, perp_hi: f64, label: &str) {
    let Some((a, b)) = gap_ends(axis, left, right, perp_lo, perp_hi) else { return };
    if out.iter().any(|g| segment_same(g.a, g.b, a, b)) {
        return;
    }
    out.push(GapMark { a, b, label: label.to_string() });
}

/// Facing edges. The line sits on the midpoint of the perpendicular overlap.
fn gap_ends(axis: Axis, left: f64, right: f64, perp_lo: f64, perp_hi: f64) -> Option<(Point, Point)> {
    if !left.is_finite() || !right.is_finite() || !perp_lo.is_finite() || !perp_hi.is_finite() || right <= left {
        return None;
    }
    let mid = (perp_lo + perp_hi) * 0.5;
    if !mid.is_finite() {
        return None;
    }
    let (a, b) = match axis {
        Axis::X => (Point::new(left, mid), Point::new(right, mid)),
        Axis::Y => (Point::new(mid, left), Point::new(mid, right)),
    };
    (point_finite(a) && point_finite(b)).then_some((a, b))
}

/// Other items' own side lengths, on resize and create.
///
/// Own size is the path box in item space, inflated the way [`Item::visible_bounds`] inflates,
/// then scaled by the absolute transform. That transform is the parent chain composed with the
/// item and decomposed once, so a rotated child inside a non-uniform group keeps the spread-space
/// sides. Rotation is dropped. Width and height are both candidates for whichever length is set.
/// The closest length inside the zone wins. Delta is the length difference applied from the moving
/// edge. A rotated rectangle contributes its side lengths, not the axis-aligned box.
fn pass_dimensions(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !matches!(req.gesture, Gesture::Resize | Gesture::Create) || !cx.snap.smart_guides || !cx.snap.smart_dimensions {
        return None;
    }
    let proposed = match axis {
        Axis::X => req.lengths[0],
        Axis::Y => req.lengths[1],
    }?;
    if !proposed.is_finite() {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    for len in dimension_lengths(cx, req) {
        if !len.is_finite() || len < 0.0 {
            continue;
        }
        let dist = (len - proposed).abs();
        if dist <= tol && best.is_none_or(|(have, _)| dist < have) {
            best = Some((dist, len));
        }
    }
    let (_, length) = best?;
    let diff = length - proposed;
    if !diff.is_finite() {
        return None;
    }
    let flags = axis_flags(req, axis);
    let text = format_measure(length, cx.unit);
    Some(AxisHit {
        delta: length_edge_delta(flags, diff),
        at: 0.0,
        span: None,
        length_delta: Some(diff),
        dimension: Some(dimension_mark(req, axis, diff, text)),
        gaps: Vec::new(),
    })
}

/// Edge shift so adding it to the moving edge grows the length by `diff`.
fn length_edge_delta(flags: [bool; 3], diff: f64) -> f64 {
    let [low_edge, mid, high_edge] = flags;
    if low_edge && high_edge && !mid {
        diff / 2.0
    } else if high_edge && !low_edge {
        diff
    } else if low_edge && !high_edge {
        -diff
    } else {
        diff
    }
}

/// Guide along the side whose length matched, after the edge has moved.
fn dimension_mark(req: &SnapRequest<'_>, axis: Axis, diff: f64, text: String) -> DimMark {
    let (a, b) = dimension_segment(req.rect, axis, axis_flags(req, axis), diff).unwrap_or((Point::ORIGIN, Point::ORIGIN));
    DimMark { a, b, text }
}

/// Spread-space segment along the matched side. `diff` 0 is the side of `rect` itself.
fn dimension_segment(rect: Rect, axis: Axis, flags: [bool; 3], diff: f64) -> Option<(Point, Point)> {
    if !diff.is_finite() || !rect_finite(rect) {
        return None;
    }
    let (a, b) = match axis {
        Axis::X => {
            let (x0, x1) = grown_span(rect.x0, rect.x1, flags, diff);
            let y = rect.y0.max(rect.y1);
            (Point::new(x0, y), Point::new(x1, y))
        }
        Axis::Y => {
            let (y0, y1) = grown_span(rect.y0, rect.y1, flags, diff);
            let x = rect.x0.max(rect.x1);
            (Point::new(x, y0), Point::new(x, y1))
        }
    };
    (point_finite(a) && point_finite(b)).then_some((a, b))
}

fn point_finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Move dimension guides onto the rect the gesture commits.
///
/// Snap draws the guide on `pre` plus the length difference along the flagged edge. Aspect re-lock
/// and a centered scale move that side afterwards. The guide then follows the committed side.
pub(crate) fn lay_dimension_guides(
    guides: &mut Vec<Overlay>,
    xf: Affine,
    pre: Rect,
    committed: Rect,
    x_edges: [bool; 3],
    y_edges: [bool; 3],
    length_delta: [Option<f64>; 2],
) {
    if !rect_finite(committed) {
        return;
    }
    let axes = [(Axis::X, x_edges, length_delta[0]), (Axis::Y, y_edges, length_delta[1])];
    for (axis, edges, diff) in axes {
        let Some(diff) = diff else { continue };
        let Some((old_a, old_b)) = dimension_segment(pre, axis, edges, diff) else { continue };
        let Some((new_a, new_b)) = dimension_segment(committed, axis, edges, 0.0) else { continue };
        let (oa, ob) = (xf * old_a, xf * old_b);
        let (na, nb) = (xf * new_a, xf * new_b);
        if !point_finite(na) || !point_finite(nb) {
            continue;
        }
        let mut found = false;
        for g in guides.iter_mut() {
            if let Overlay::Guide { a, b } = g
                && segment_same(*a, *b, oa, ob)
            {
                *a = na;
                *b = nb;
                found = true;
                break;
            }
        }
        if !found {
            guides.push(Overlay::Guide { a: na, b: nb });
        }
    }
}

/// Move a line's dimension tick onto the committed segment.
///
/// The dimensions pass draws a short horizontal tick at the proposed endpoint because the
/// length is carried on x. The guide that remains is the segment from the anchor to the
/// endpoint the gesture commits.
pub(crate) fn lay_line_dimension(guides: &mut Vec<Overlay>, xf: Affine, anchor: Point, proposed: Point, committed: Point, diff: f64) {
    if !diff.is_finite() {
        return;
    }
    let rect = Rect::from_points(proposed, proposed);
    let Some((old_a, old_b)) = dimension_segment(rect, Axis::X, [true, true, true], diff) else { return };
    let (oa, ob) = (xf * old_a, xf * old_b);
    let (na, nb) = (xf * anchor, xf * committed);
    if !point_finite(na) || !point_finite(nb) {
        return;
    }
    let mut found = false;
    for g in guides.iter_mut() {
        if let Overlay::Guide { a, b } = g
            && segment_same(*a, *b, oa, ob)
        {
            *a = na;
            *b = nb;
            found = true;
            break;
        }
    }
    if !found {
        guides.push(Overlay::Guide { a: na, b: nb });
    }
}

fn segment_same(a0: Point, b0: Point, a1: Point, b1: Point) -> bool {
    let close = |p: Point, q: Point| (p.x - q.x).abs() <= 1e-6 && (p.y - q.y).abs() <= 1e-6;
    (close(a0, a1) && close(b0, b1)) || (close(a0, b1) && close(b0, a1))
}

fn grown_span(a0: f64, a1: f64, flags: [bool; 3], diff: f64) -> (f64, f64) {
    let (lo, hi) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
    let [low_edge, mid, high_edge] = flags;
    if low_edge && high_edge && !mid {
        let mid_v = (lo + hi) / 2.0;
        let half = (hi - lo) / 2.0 + diff / 2.0;
        (mid_v - half, mid_v + half)
    } else if low_edge && !high_edge {
        (lo - diff, hi)
    } else {
        (lo, hi + diff)
    }
}

fn dimension_lengths(cx: &ToolContext, req: &SnapRequest<'_>) -> Vec<f64> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(sp) = cx.doc.spread(req.spread) {
        for item in &sp.items {
            push_own_lengths(&mut out, cx.selection, req, item, Walk::ROOT, &mut seen);
        }
    }
    if shows_parent_items(cx, req) {
        push_parent_lengths(&mut out, cx, req, &mut seen);
    }
    out
}

fn push_own_lengths(out: &mut Vec<f64>, selection: &Selection, req: &SnapRequest<'_>, item: &Item, walk: Walk, seen: &mut HashSet<ItemId>) {
    let Walk { ancestor, skipped, depth } = walk;
    if depth >= ALIGN_GROUP_DEPTH || !seen.insert(item.id) || item.hidden {
        return;
    }
    let abs = ancestor * item.xf;
    if !xf_finite(abs) {
        return;
    }
    let skip = skipped || align_skips_item(selection, req, item.id);
    if item.is_group() && !item.children().is_empty() {
        for child in item.children() {
            push_own_lengths(out, selection, req, child, walk.child(abs, skip), seen);
        }
        return;
    }
    if skip {
        return;
    }
    let Some((w, h)) = leaf_own_size(item, abs) else { return };
    out.push(w);
    out.push(h);
}

fn xf_finite(m: Affine) -> bool {
    m.as_coeffs().iter().all(|c| c.is_finite())
}

/// Path bounds in item space, stroke outset, then one decompose of the absolute transform.
/// Rotation is not applied to the side lengths.
fn leaf_own_size(item: &Item, abs: Affine) -> Option<(f64, f64)> {
    let local = item.path.bounds()?;
    if !rect_finite(local) {
        return None;
    }
    let outset = stroke_outset(item);
    if !outset.is_finite() {
        return None;
    }
    let local = if outset > 0.0 { local.inflate(outset, outset) } else { local };
    if !rect_finite(local) {
        return None;
    }
    let decomposed = designcraft_geom::decompose::decompose(abs);
    let sx = decomposed.scale_x.abs();
    let sy = decomposed.scale_y.abs();
    if !sx.is_finite() || !sy.is_finite() {
        return None;
    }
    let w = (local.x1 - local.x0).abs() * sx;
    let h = (local.y1 - local.y0).abs() * sy;
    (w.is_finite() && h.is_finite()).then_some((w, h))
}

/// Same outset as [`Item::visible_bounds`]: half the weight when centered, the whole weight when
/// outside, nothing when inside or when the stroke is none. Open paths also count arrowheads.
fn stroke_outset(item: &Item) -> f64 {
    if item.stroke.is_none() {
        return 0.0;
    }
    let w = match item.stroke.align {
        StrokeAlign::Center => item.stroke.weight / 2.0,
        StrokeAlign::Inside => 0.0,
        StrokeAlign::Outside => item.stroke.weight,
    };
    if item.path.is_closed() { w } else { w.max(item.stroke.extent() - item.stroke.weight) }
}

fn push_parent_lengths(out: &mut Vec<f64>, cx: &ToolContext, req: &SnapRequest<'_>, seen_items: &mut HashSet<ItemId>) {
    for (item, _) in shown_parent_items(cx.doc, req.spread) {
        push_own_lengths(out, cx.selection, req, item, Walk::ROOT, seen_items);
    }
}

/// Ruler guides, margins, and column sides. No overlay.
///
/// A page guide attracts when the moving rect overlaps that page on the other axis. A spread
/// guide uses the spread bounds. Locked guides are included, and so are the guides a page shows
/// from its parent page. A guide is a target only when `Guide::visible_in` accepts it, and not
/// when the request excludes it (the guides being dragged).
fn pass_guides(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !cx.snap.snap_to_guides || !cx.snap.show_guides {
        return None;
    }
    let sp = cx.doc.spread(req.spread)?;
    let spread_bounds = sp.bounds();
    let flags = axis_flags(req, axis);
    let (moving_lo, moving_hi) = perp_ends(req.rect, axis);
    let mut best: Option<Best> = None;
    for (pi, page) in sp.pages.iter().enumerate() {
        let page_bounds = page.bounds();
        // The page's own guides, less the excluded ones, then those it shows from its parent page.
        let own = page
            .guides
            .iter()
            .enumerate()
            .filter(|&(gi, _)| !req.exclude_guides.contains(&designcraft_doc::GuideSel { spread: req.spread, page: pi, index: gi }));
        for guide in own.map(|(_, g)| g.clone()).chain(cx.doc.parent_guides(page)) {
            if !guide.visible_in(cx.doc) || !guide.position.is_finite() || guide.orientation != axis_orientation(axis) {
                continue;
            }
            let range = if guide.spread { spread_bounds } else { page_bounds };
            let (range_lo, range_hi) = perp_ends(range, axis);
            if !ranges_overlap(moving_lo, moving_hi, range_lo, range_hi) {
                continue;
            }
            offer_line(&mut best, req.rect, flags, axis, guide.position, tol);
        }
        let margin = page.margin_rect();
        let (a, b) = match axis {
            Axis::X => (margin.x0, margin.x1),
            Axis::Y => (margin.y0, margin.y1),
        };
        offer_line(&mut best, req.rect, flags, axis, a, tol);
        offer_line(&mut best, req.rect, flags, axis, b, tol);
        if matches!(axis, Axis::X) {
            for column in page.column_rects() {
                offer_line(&mut best, req.rect, flags, axis, column.x0, tol);
                offer_line(&mut best, req.rect, flags, axis, column.x1, tol);
            }
        }
    }
    // The bleed guide's four sides, like InDesign's bleed guides.
    if let Some(bleed) = sp.bleed_rect(cx.doc.settings.bleed) {
        let (range_lo, range_hi) = perp_ends(bleed, axis);
        if ranges_overlap(moving_lo, moving_hi, range_lo, range_hi) {
            let (a, b) = match axis {
                Axis::X => (bleed.x0, bleed.x1),
                Axis::Y => (bleed.y0, bleed.y1),
            };
            offer_line(&mut best, req.rect, flags, axis, a, tol);
            offer_line(&mut best, req.rect, flags, axis, b, tol);
        }
    }
    best.map(|b| b.hit)
}

/// How deep a group may nest before alignment stops walking it.
const ALIGN_GROUP_DEPTH: usize = 32;

/// Page and item edges and centers. Draws one [`Overlay::Guide`] per winning axis.
///
/// Edges match edges only while `align_edges` is on. Centers match centers only while
/// `align_centers` is on, and only when the request's middle flag is set. Hidden items are
/// skipped. Without copying, excluded items are skipped. With copying, that list is the
/// drag-start set and stays a target. Live selection items outside that set are preview
/// copies and are skipped. A group with children uses the union of each child's visible bounds
/// after that child's transform, then the group's transform. Transform each child, then union.
/// It does not rotate the local union as one rectangle, and it does not use the group's own stroke.
/// Parent items are added for a document page that shows them, in document spread space.
/// A layout built for parent editing already has those items on the spread, so they are not
/// added again.
fn pass_align(cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, tol: f64) -> Option<AxisHit> {
    if !cx.snap.smart_guides || (!cx.snap.align_edges && !cx.snap.align_centers) {
        return None;
    }
    let flags = axis_flags(req, axis);
    let mut best: Option<Best> = None;
    if let Some(sp) = cx.doc.spread(req.spread) {
        for page in &sp.pages {
            offer_box(&mut best, req.rect, flags, axis, page.bounds(), tol, (cx.snap.align_edges, cx.snap.align_centers));
        }
        for item in &sp.items {
            let Some(rect) = item_target_rect(cx.selection, req, item) else { continue };
            offer_box(&mut best, req.rect, flags, axis, rect, tol, (cx.snap.align_edges, cx.snap.align_centers));
        }
    }
    if shows_parent_items(cx, req) {
        offer_parent_items(&mut best, cx, req, flags, axis, tol);
    }
    best.map(|b| b.hit)
}

/// Drag-start ids stay targets while copying. Preview copies are the live selection minus that set.
fn align_skips_item(selection: &Selection, req: &SnapRequest<'_>, id: ItemId) -> bool {
    if req.copying { selection.items.contains(&id) && !req.exclude.contains(&id) } else { req.exclude.contains(&id) }
}

/// Parent spreads are the canvas while a parent is being edited. Their items are ordinary targets.
fn layout_edits_parents(cx: &ToolContext) -> bool {
    cx.layout.slots.iter().any(|slot| matches!(slot.spread, SpreadRef::Parent(_)))
}

/// Whether to offer the parent items drawn on `req.spread`. A parent spread shows its bases' items,
/// which are not on it.
fn shows_parent_items(cx: &ToolContext, req: &SnapRequest<'_>) -> bool {
    matches!(req.spread, SpreadRef::Parent(_)) || !layout_edits_parents(cx)
}

/// Visible box used for alignment. Groups with children contribute the union of the children.
fn item_target_rect(selection: &Selection, req: &SnapRequest<'_>, item: &Item) -> Option<Rect> {
    if item.hidden || align_skips_item(selection, req, item.id) {
        return None;
    }
    if item.is_group() && !item.children().is_empty() {
        let mut seen = HashSet::new();
        return group_target_rect(selection, req, item, &mut seen, 0);
    }
    let bounds = item.visible_bounds();
    rect_finite(bounds).then_some(bounds)
}

/// Union of each child's visible bounds transformed by `item.xf`.
///
/// Transform each child, then union. Rotating the local union as one rectangle is larger when
/// a rotated group's children do not fill that rectangle. Hidden and excluded children stay out.
/// Nested groups use the same rule. A child's stroke is part of that child's visible bounds.
/// The group's own stroke is not added.
fn group_target_rect(selection: &Selection, req: &SnapRequest<'_>, item: &Item, seen: &mut HashSet<ItemId>, depth: usize) -> Option<Rect> {
    group_box(item, seen, depth, Some((selection, req)))
}

/// Visible box of an item being moved. Groups use the same box as alignment, with no exclude list.
pub(crate) fn moving_bounds(item: &Item) -> Option<Rect> {
    if item.is_group() && !item.children().is_empty() {
        let mut seen = HashSet::new();
        return group_box(item, &mut seen, 0, None);
    }
    let bounds = item.visible_bounds();
    rect_finite(bounds).then_some(bounds)
}

fn group_box(item: &Item, seen: &mut HashSet<ItemId>, depth: usize, filter: Option<(&Selection, &SnapRequest<'_>)>) -> Option<Rect> {
    if depth >= ALIGN_GROUP_DEPTH || !seen.insert(item.id) {
        return None;
    }
    let mut acc: Option<Rect> = None;
    for child in item.children() {
        if child.hidden {
            continue;
        }
        if let Some((selection, req)) = filter
            && align_skips_item(selection, req, child.id)
        {
            continue;
        }
        let local = if child.is_group() && !child.children().is_empty() {
            group_box(child, seen, depth + 1, filter)
        } else {
            let bounds = child.visible_bounds();
            rect_finite(bounds).then_some(bounds)
        };
        let Some(local) = local else { continue };
        if !rect_finite(local) {
            continue;
        }
        let placed = item.xf.transform_rect_bbox(local);
        if !rect_finite(placed) {
            continue;
        }
        acc = Some(match acc {
            Some(have) => have.union(placed),
            None => placed,
        });
    }
    acc.filter(|r| rect_finite(*r))
}

/// Parent items shown on the spread, shifted into that spread's space.
fn offer_parent_items(best: &mut Option<Best>, cx: &ToolContext, req: &SnapRequest<'_>, flags: [bool; 3], axis: Axis, tol: f64) {
    for (item, dx) in shown_parent_items(cx.doc, req.spread) {
        let Some(rect) = item_target_rect(cx.selection, req, item) else { continue };
        offer_box(best, req.rect, flags, axis, shift_x(rect, dx), tol, (cx.snap.align_edges, cx.snap.align_centers));
    }
}

/// Parent items drawn on `spread`, back to front, each with the x shift from its parent spread:
/// a document page's parent and its bases, or a parent spread's bases. Same page and override
/// filters as the renderer.
fn shown_parent_items(doc: &Document, spread: SpreadRef) -> Vec<(&Item, f64)> {
    let Some(sp) = doc.spread(spread) else { return Vec::new() };
    let mut out = Vec::new();
    for (pi, page) in sp.pages.iter().enumerate() {
        if !page.show_parent_items {
            continue;
        }
        for (ppi, ppage) in doc.shown_parent_pages(spread, pi) {
            let Some(parent) = doc.parents.get(ppi) else { continue };
            let Some(parent_page) = parent.pages.get(ppage) else { continue };
            let dx = page.x - parent_page.x;
            if !dx.is_finite() {
                continue;
            }
            for item in &parent.items {
                if page.overridden.contains(&item.id) {
                    continue;
                }
                // A facing parent only shows items whose center sits on the parent page this page uses.
                if parent.pages.len() > 1 && parent.page_at_x(item.bounds().center().x) != Some(ppage) {
                    continue;
                }
                out.push((item.as_ref(), dx));
            }
        }
    }
    out
}

fn shift_x(r: Rect, dx: f64) -> Rect {
    Rect::new(r.x0 + dx, r.y0, r.x1 + dx, r.y1)
}

fn axis_flags(req: &SnapRequest<'_>, axis: Axis) -> [bool; 3] {
    match axis {
        Axis::X => req.x_edges,
        Axis::Y => req.y_edges,
    }
}

fn axis_orientation(axis: Axis) -> Orientation {
    match axis {
        Axis::X => Orientation::Vertical,
        Axis::Y => Orientation::Horizontal,
    }
}

/// Left/center/right, or top/center/bottom.
fn axis_triple(r: Rect, axis: Axis) -> (f64, f64, f64) {
    match axis {
        Axis::X => (r.x0, r.center().x, r.x1),
        Axis::Y => (r.y0, r.center().y, r.y1),
    }
}

/// The rect's extent on the axis perpendicular to `axis`.
fn perp_ends(r: Rect, axis: Axis) -> (f64, f64) {
    match axis {
        Axis::X => (r.y0, r.y1),
        Axis::Y => (r.x0, r.x1),
    }
}

fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    if !a0.is_finite() || !a1.is_finite() || !b0.is_finite() || !b1.is_finite() {
        return false;
    }
    let (a_lo, a_hi) = (a0.min(a1), a0.max(a1));
    let (b_lo, b_hi) = (b0.min(b1), b0.max(b1));
    a_lo <= b_hi && a_hi >= b_lo
}

/// Any requested edge, including the center, against one guide line. No span, so nothing is drawn.
fn offer_line(best: &mut Option<Best>, moving: Rect, flags: [bool; 3], axis: Axis, target: f64, tol: f64) {
    let (left, mid, right) = axis_triple(moving, axis);
    let [use_left, use_mid, use_right] = flags;
    if use_left {
        offer(best, left, target, tol, None);
    }
    if use_mid {
        offer(best, mid, target, tol, None);
    }
    if use_right {
        offer(best, right, target, tol, None);
    }
}

fn offer_box(best: &mut Option<Best>, moving: Rect, flags: [bool; 3], axis: Axis, target: Rect, tol: f64, (edges, centers): (bool, bool)) {
    let (m0, m_mid, m1) = axis_triple(moving, axis);
    let (t0, t_mid, t1) = axis_triple(target, axis);
    let [use_left, use_mid, use_right] = flags;
    if edges {
        if use_left {
            offer(best, m0, t0, tol, Some(target));
            offer(best, m0, t1, tol, Some(target));
        }
        if use_right {
            offer(best, m1, t0, tol, Some(target));
            offer(best, m1, t1, tol, Some(target));
        }
    }
    if centers && use_mid {
        offer(best, m_mid, t_mid, tol, Some(target));
    }
}

fn offer(best: &mut Option<Best>, moving: f64, target: f64, tol: f64, span: Option<Rect>) {
    if !moving.is_finite() || !target.is_finite() {
        return;
    }
    let delta = target - moving;
    let dist = delta.abs();
    if dist <= tol && best.as_ref().is_none_or(|b| dist < b.dist) {
        *best = Some(Best { dist, hit: AxisHit { delta, at: target, span, length_delta: None, dimension: None, gaps: Vec::new() } });
    }
}

fn push_hit_guides(out: &mut Vec<Overlay>, cx: &ToolContext, req: &SnapRequest<'_>, axis: Axis, hit: &AxisHit, moving: Rect) {
    push_alignment_guide(out, cx, req.spread, axis, hit, moving);
    push_gap_guides(out, cx, req.spread, hit);
    let Some(mark) = &hit.dimension else { return };
    let xf = cx.layout.xf(req.spread);
    if mark.a.x.is_finite() && mark.a.y.is_finite() && mark.b.x.is_finite() && mark.b.y.is_finite() {
        out.push(Overlay::Guide { a: xf * mark.a, b: xf * mark.b });
    }
    let p = xf * req.pointer;
    if p.x.is_finite() && p.y.is_finite() {
        out.push(Overlay::Measure { p, text: mark.text.clone() });
    }
}

fn push_gap_guides(out: &mut Vec<Overlay>, cx: &ToolContext, spread: SpreadRef, hit: &AxisHit) {
    if hit.gaps.is_empty() {
        return;
    }
    let xf = cx.layout.xf(spread);
    for g in &hit.gaps {
        let a = xf * g.a;
        let b = xf * g.b;
        if point_finite(a) && point_finite(b) {
            out.push(Overlay::Gap { a, b, label: g.label.clone() });
        }
    }
}

fn push_alignment_guide(out: &mut Vec<Overlay>, cx: &ToolContext, spread: SpreadRef, axis: Axis, hit: &AxisHit, moving: Rect) {
    let Some(target) = hit.span else { return };
    let xf = cx.layout.xf(spread);
    let (a, b) = match axis {
        Axis::X => {
            let lo = moving.y0.min(moving.y1).min(target.y0.min(target.y1)) - 6.0;
            let hi = moving.y0.max(moving.y1).max(target.y0.max(target.y1)) + 6.0;
            (xf * Point::new(hit.at, lo), xf * Point::new(hit.at, hi))
        }
        Axis::Y => {
            let lo = moving.x0.min(moving.x1).min(target.x0.min(target.x1)) - 6.0;
            let hi = moving.x0.max(moving.x1).max(target.x0.max(target.x1)) + 6.0;
            (xf * Point::new(lo, hit.at), xf * Point::new(hi, hit.at))
        }
    };
    out.push(Overlay::Guide { a, b });
}

#[cfg(test)]
mod tests;
