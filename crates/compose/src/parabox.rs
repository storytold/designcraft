//! Paragraph border and shading: one box per column a paragraph (or a run of consecutive
//! paragraphs with the same settings) sits in, drawn once the lines are in place.

use designcraft_doc::{BoxBottom, BoxTop, BoxWidth, Cap, Document, Join, ParaProps, StrokeType};
use designcraft_geom::corners::{Corner, box_outline};
use designcraft_geom::{BezPath, Rect, Shape, kurbo};

use crate::{ComposedStory, Deco, FrameSpec, Line};

/// Flattening tolerance of stroked outlines (points).
const TOL: f64 = 0.05;
/// Most dashes drawn along one border (more falls back to a solid stroke).
const MAX_DASHES: f64 = 20_000.0;

/// The border and shading settings of a paragraph (the stroke type resolved against the document's
/// stroke styles). Consecutive paragraphs with equal settings share a box when they merge.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParaBox {
    shading: Option<Shading>,
    border: Option<Border>,
    merge: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Shading {
    color: String,
    tint: f32,
    offsets: [f64; 4],
    corners: [Corner; 4],
    width: BoxWidth,
    top: BoxTop,
    bottom: BoxBottom,
    clip: bool,
    nonprinting: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Border {
    color: String,
    tint: f32,
    /// Top, left, bottom, right.
    weights: [f64; 4],
    offsets: [f64; 4],
    kind: StrokeType,
    cap: Cap,
    join: Join,
    corners: [Corner; 4],
    width: BoxWidth,
    top: BoxTop,
    bottom: BoxBottom,
    display_if_splits: bool,
    gap_color: String,
    gap_tint: f32,
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

impl ParaBox {
    pub(crate) fn of(doc: &Document, pp: &ParaProps) -> Option<ParaBox> {
        if !pp.shading_on && !pp.border_on {
            return None;
        }
        let shading = pp.shading_on.then(|| Shading {
            color: pp.shading_color.clone(),
            tint: pp.shading_tint,
            offsets: pp.shading_offsets.map(finite),
            corners: pp.shading_corners,
            width: pp.shading_width,
            top: pp.shading_top,
            bottom: pp.shading_bottom,
            clip: pp.shading_clip,
            nonprinting: pp.shading_nonprinting,
        });
        let border = pp.border_on.then(|| Border {
            color: pp.border_color.clone(),
            tint: pp.border_tint,
            weights: pp.border_weights.map(|w| finite(w).clamp(0.0, 1000.0)),
            offsets: pp.border_offsets.map(finite),
            kind: doc.stroke_kind(&pp.border_type),
            cap: pp.border_cap,
            join: pp.border_join,
            corners: pp.border_corners,
            width: pp.border_width,
            top: pp.border_top,
            bottom: pp.border_bottom,
            display_if_splits: pp.border_display_if_splits,
            gap_color: pp.border_gap_color.clone(),
            gap_tint: pp.border_gap_tint,
        });
        Some(ParaBox { shading, border, merge: pp.border_merge })
    }
}

/// Lines of one box in one frame: a part ends where the next line moves to another column or
/// sub-column.
struct Part<'a> {
    fi: usize,
    lines: Vec<&'a Line>,
}

/// Draw the boxes of `boxes` (one entry per paragraph) into `out`.
pub(crate) fn draw(out: &mut ComposedStory, boxes: &[Option<ParaBox>], frames: &[FrameSpec]) {
    let mut pi = 0;
    while pi < boxes.len() {
        let Some(b) = boxes.get(pi).and_then(Option::as_ref) else {
            pi += 1;
            continue;
        };
        let mut end = pi;
        while b.merge && boxes.get(end + 1).and_then(Option::as_ref).is_some_and(|n| n == b) {
            end += 1;
        }
        let decos = group_decos(out, pi..=end, b, frames);
        for (fi, under, d) in decos {
            if let Some(ft) = out.frames.get_mut(fi) {
                if under { ft.decos.insert(0, d) } else { ft.decos.push(d) }
            }
        }
        pi = end + 1;
    }
}

/// The decorations of paragraphs `paras` sharing box `b`: (frame, drawn under the rest, deco).
fn group_decos(out: &ComposedStory, paras: std::ops::RangeInclusive<usize>, b: &ParaBox, frames: &[FrameSpec]) -> Vec<(usize, bool, Deco)> {
    let mut parts: Vec<Part<'_>> = Vec::new();
    for (fi, ft) in out.frames.iter().enumerate() {
        let lines: Vec<&Line> = ft.lines.iter().filter(|l| paras.contains(&l.para)).collect();
        for part in lines.chunk_by(|a, b| a.column == b.column && b.baseline > a.baseline) {
            parts.push(Part { fi, lines: part.to_vec() });
        }
    }
    let n = parts.len();
    let mut decos = Vec::new();
    for (k, part) in parts.iter().enumerate() {
        let (first_part, last_part) = (k == 0, k + 1 == n);
        if let Some(s) = &b.shading
            && let Some(r) = inner_rect(&part.lines, s.width, s.top, s.bottom)
        {
            let [t, l, bo, rr] = s.offsets;
            let mut rect = Rect::new(r.x0 - l, r.y0 - t, r.x1 + rr, r.y1 + bo);
            if s.clip
                && let Some(f) = frames.get(part.fi)
            {
                rect = rect.intersect(frame_rect(f));
            }
            if rect.width() > 0.0 && rect.height() > 0.0 {
                // Corners stay square where the box is cut by a split.
                let corners = cut_corners(s.corners, !first_part, !last_part);
                let path = (!corners.iter().all(Corner::is_none)).then(|| box_outline(rect, &corners, [true; 4]));
                decos.push((part.fi, true, Deco { rect, color: s.color.clone(), tint: s.tint, path, nonprinting: s.nonprinting }));
            }
        }
        if let Some(bd) = &b.border
            && let Some(r) = inner_rect(&part.lines, bd.width, bd.top, bd.bottom)
        {
            let [t, l, bo, rr] = bd.offsets;
            let o = Rect::new(r.x0 - l, r.y0 - t, r.x1 + rr, r.y1 + bo);
            let open_top = !first_part && !bd.display_if_splits;
            let open_bottom = !last_part && !bd.display_if_splits;
            for (path, gap) in border_paths(bd, o, open_top, open_bottom) {
                let (color, tint) = if gap { (bd.gap_color.clone(), bd.gap_tint) } else { (bd.color.clone(), bd.tint) };
                decos.push((part.fi, false, Deco { rect: path.bounding_box(), color, tint, path: Some(path), nonprinting: false }));
            }
            if is_plain(bd, open_top, open_bottom) {
                decos.extend(
                    plain_edges(bd, o, open_top, open_bottom)
                        .into_iter()
                        .map(|rect| (part.fi, false, Deco { rect, color: bd.color.clone(), tint: bd.tint, path: None, nonprinting: false })),
                );
            }
        }
    }
    decos
}

/// The frame's edges in its inner space (the text area out to the insets).
fn frame_rect(f: &FrameSpec) -> Rect {
    let a = f.area;
    if f.vertical {
        return a;
    }
    let [t, l, b, r] = f.opts.inset.map(finite);
    Rect::new(a.x0 - l, a.y0 - t, a.x1 + r, a.y1 + b)
}

/// `corners` with the top pair square when the box is cut at the top, the bottom pair when cut at
/// the bottom (top left, top right, bottom right, bottom left).
fn cut_corners(mut corners: [Corner; 4], top: bool, bottom: bool) -> [Corner; 4] {
    for (i, c) in corners.iter_mut().enumerate() {
        if (top && i < 2) || (bottom && i >= 2) {
            *c = Corner::default();
        }
    }
    corners
}

/// The em box of a line's tallest glyph: how far its top is above the baseline and its bottom below.
fn em_box(l: &Line) -> Option<(f64, f64)> {
    l.glyphs.iter().filter(|g| g.visible && g.sy.is_finite() && g.sy > 0.0).fold(None, |acc: Option<(f64, f64)>, g| {
        let (top, bottom) = g.face.em_box();
        let (t, b) = (top * g.sy, -bottom * g.sy);
        Some(acc.map_or((t, b), |(at, ab)| (at.max(t), ab.max(b))))
    })
}

/// The box around `lines` before offsets: the column or the text across, from the top edge of the
/// first line to the bottom edge of the last.
fn inner_rect(lines: &[&Line], width: BoxWidth, top: BoxTop, bottom: BoxBottom) -> Option<Rect> {
    let (a, z) = (lines.first()?, lines.last()?);
    let y0 = match top {
        BoxTop::Ascent => a.baseline - a.ascent,
        BoxTop::Baseline => a.baseline,
        BoxTop::EmBox => a.baseline - em_box(a).map_or(a.ascent, |e| e.0),
        BoxTop::Leading => a.baseline - a.leading.max(a.ascent),
    };
    let y1 = match bottom {
        BoxBottom::Descent => z.baseline + z.descent,
        BoxBottom::Baseline => z.baseline,
        BoxBottom::EmBox => z.baseline + em_box(z).map_or(z.descent, |e| e.1),
    };
    let column = || (lines.iter().map(|l| l.x0).fold(f64::INFINITY, f64::min), lines.iter().map(|l| l.x1).fold(f64::NEG_INFINITY, f64::max));
    let (x0, x1) = match width {
        BoxWidth::Column => column(),
        BoxWidth::Text => {
            let start = lines.iter().flat_map(|l| l.glyphs.iter().filter(|g| g.visible).map(|g| g.x.min(g.x + g.adv))).fold(f64::INFINITY, f64::min);
            let end = lines.iter().filter(|l| l.glyphs.iter().any(|g| g.visible)).map(|l| l.end_x).fold(f64::NEG_INFINITY, f64::max);
            if start.is_finite() && end.is_finite() && end > start { (start, end) } else { column() }
        }
    };
    (x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()).then(|| Rect::new(x0, y0, x1, y1))
}

/// The edges drawn: top, right, bottom, left (a cut end, or a side of no weight, isn't).
fn edges(bd: &Border, open_top: bool, open_bottom: bool) -> [bool; 4] {
    let [wt, wl, wb, wr] = bd.weights;
    [wt > 0.0 && !open_top, wr > 0.0, wb > 0.0 && !open_bottom, wl > 0.0]
}

/// The common weight of the drawn edges, when they all have the same one.
fn equal_weight(bd: &Border, drawn: [bool; 4]) -> Option<f64> {
    let [wt, wl, wb, wr] = bd.weights;
    let w: Vec<f64> = [wt, wr, wb, wl].into_iter().zip(drawn).filter(|(_, d)| *d).map(|(w, _)| w).collect();
    let first = *w.first()?;
    w.iter().all(|x| (x - first).abs() < 1e-9).then_some(first)
}

/// A solid border with no corner shapes (or different weights per side) is filled rectangles, one
/// per edge.
fn is_plain(bd: &Border, open_top: bool, open_bottom: bool) -> bool {
    let drawn = edges(bd, open_top, open_bottom);
    let corners = cut_corners(bd.corners, open_top, open_bottom);
    bd.kind == StrokeType::Solid && (corners.iter().all(Corner::is_none) || equal_weight(bd, drawn).is_none())
}

/// The edge rectangles of a plain border, outside the offset box `o`.
fn plain_edges(bd: &Border, o: Rect, open_top: bool, open_bottom: bool) -> Vec<Rect> {
    let [wt, wl, wb, wr] = bd.weights;
    let [top, right, bottom, left] = edges(bd, open_top, open_bottom);
    let mut v = Vec::new();
    if top {
        v.push(Rect::new(o.x0 - wl, o.y0 - wt, o.x1 + wr, o.y0));
    }
    if bottom {
        v.push(Rect::new(o.x0 - wl, o.y1, o.x1 + wr, o.y1 + wb));
    }
    if left {
        v.push(Rect::new(o.x0 - wl, o.y0, o.x0, o.y1));
    }
    if right {
        v.push(Rect::new(o.x1, o.y0, o.x1 + wr, o.y1));
    }
    v
}

/// The stroked outlines of a border that isn't plain, each with whether it is the gap colour
/// (drawn first, under the dashes). The stroke lies outside the offset box `o`.
fn border_paths(bd: &Border, o: Rect, open_top: bool, open_bottom: bool) -> Vec<(BezPath, bool)> {
    if is_plain(bd, open_top, open_bottom) {
        return Vec::new();
    }
    let drawn = edges(bd, open_top, open_bottom);
    let mut out = Vec::new();
    let mut add = |path: &BezPath, w: f64, perimeter: f64| {
        if bd.kind != StrokeType::Solid {
            let gap = stroke_outline(path, w, &StrokeType::Solid, Cap::Butt, bd.join, perimeter);
            out.push((gap, true));
        }
        out.push((stroke_outline(path, w, &bd.kind, bd.cap, bd.join, perimeter), false));
    };
    match equal_weight(bd, drawn) {
        Some(w) => {
            // One outline along the stroke's centre, corners shaped where two edges meet.
            let c = Rect::new(o.x0 - w / 2.0, o.y0 - w / 2.0, o.x1 + w / 2.0, o.y1 + w / 2.0);
            let corners = cut_corners(bd.corners, open_top, open_bottom);
            add(&box_outline(c, &corners, drawn), w, 2.0 * (c.width() + c.height()));
        }
        None => {
            // Different weights: each edge on its own.
            let [wt, wl, wb, wr] = bd.weights;
            let lines = [
                (drawn[0], wt, (o.x0 - wl, o.y0 - wt / 2.0), (o.x1 + wr, o.y0 - wt / 2.0)),
                (drawn[1], wr, (o.x1 + wr / 2.0, o.y0), (o.x1 + wr / 2.0, o.y1)),
                (drawn[2], wb, (o.x0 - wl, o.y1 + wb / 2.0), (o.x1 + wr, o.y1 + wb / 2.0)),
                (drawn[3], wl, (o.x0 - wl / 2.0, o.y0), (o.x0 - wl / 2.0, o.y1)),
            ];
            for (on, w, a, b) in lines {
                if on {
                    let mut p = BezPath::new();
                    p.move_to(a);
                    p.line_to(b);
                    add(&p, w, kurbo::Point::from(a).distance(b.into()));
                }
            }
        }
    }
    out
}

/// The filled outline of stroking `path` `w` wide in stroke type `kind`.
fn stroke_outline(path: &BezPath, w: f64, kind: &StrokeType, cap: Cap, join: Join, length: f64) -> BezPath {
    if !(w.is_finite() && w > 0.0) {
        return BezPath::new();
    }
    if let Some(o) = kind.outline(path, w, TOL) {
        return o;
    }
    let mut st = kurbo::Stroke::new(w)
        .with_caps(match cap {
            Cap::Butt => kurbo::Cap::Butt,
            Cap::Round => kurbo::Cap::Round,
            Cap::Projecting => kurbo::Cap::Square,
        })
        .with_join(match join {
            Join::Miter => kurbo::Join::Miter,
            Join::Round => kurbo::Join::Round,
            Join::Bevel => kurbo::Join::Bevel,
        });
    let dashes: Option<Vec<f64>> = match kind {
        StrokeType::Dashed { pattern } => {
            let p: Vec<f64> = pattern.iter().copied().filter(|v| v.is_finite() && *v >= 0.0).collect();
            Some(if p.iter().sum::<f64>() > 1e-3 { p } else { vec![3.0 * w, 2.0 * w] })
        }
        StrokeType::Dotted => {
            st = st.with_caps(kurbo::Cap::Round);
            Some(vec![0.0, 2.0 * w])
        }
        _ => None,
    };
    if let Some(d) = dashes {
        let period: f64 = d.iter().sum();
        if period > 1e-3 && length / period < MAX_DASHES {
            st = st.with_dashes(0.0, d);
        }
    }
    kurbo::stroke(path.iter(), &st, &kurbo::StrokeOpts::default(), TOL)
}
