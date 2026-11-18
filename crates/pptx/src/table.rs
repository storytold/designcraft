//! Table fragments as native PowerPoint tables: the grid from the cells' edges, merged cells as
//! spans, fills, edges as cell borders, and each cell's composed text.

use designcraft_compose::{FrameText, PlacedCell, StrokeSeg, TableFrag};
use designcraft_doc::StrokeType;
use designcraft_geom::{Affine, Rect};

use crate::slide::{Exporter, box_of};

/// Sorted distinct values (within 0.25 pt).
fn edges(mut v: Vec<f64>) -> Vec<f64> {
    v.retain(|x| x.is_finite());
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 0.25);
    v
}

/// The grid line nearest `x` (the first of two equally near).
fn index(edges: &[f64], x: f64) -> usize {
    if !x.is_finite() {
        return edges.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i);
    }
    // `edges` is sorted: the nearest is one of the two around `x`.
    let at = edges.partition_point(|e| *e < x);
    match (at.checked_sub(1).and_then(|i| edges.get(i)), edges.get(at)) {
        (Some(lo), Some(hi)) if x - lo > hi - x => at,
        (Some(_), _) => at - 1,
        _ => at.min(edges.len().saturating_sub(1)),
    }
}

pub fn tables(ex: &mut Exporter, ft: &FrameText, m: Affine, k: f64, opacity: f32) {
    for t in &ft.tables {
        table(ex, t, m, k, opacity);
    }
}

/// A table's stroke segments by where they lie, so the sides of each cell don't search them all.
struct EdgeIndex<'a> {
    strokes: &'a [StrokeSeg],
    /// (x of the first end, index), for segments whose first end is finite; sorted by x.
    by_x: Vec<(f64, usize)>,
    by_y: Vec<(f64, usize)>,
}

impl<'a> EdgeIndex<'a> {
    fn new(strokes: &'a [StrokeSeg]) -> Self {
        let mut by_x: Vec<(f64, usize)> = strokes.iter().enumerate().filter(|(_, s)| s.a.x.is_finite()).map(|(i, s)| (s.a.x, i)).collect();
        let mut by_y: Vec<(f64, usize)> = strokes.iter().enumerate().filter(|(_, s)| s.a.y.is_finite()).map(|(i, s)| (s.a.y, i)).collect();
        by_x.sort_by(|p, q| p.0.total_cmp(&q.0));
        by_y.sort_by(|p, q| p.0.total_cmp(&q.0));
        EdgeIndex { strokes, by_x, by_y }
    }

    /// The stroke along one side of a cell (`a`-`b`), if the segments on that line cover most of it
    /// (a merged cell's side is drawn in pieces, one per grid cell).
    fn stroke(&self, a: (f64, f64), b: (f64, f64)) -> Option<&'a StrokeSeg> {
        let vertical = (a.0 - b.0).abs() < 0.25;
        let (lo, hi) = if vertical { (a.1, b.1) } else { (a.0, b.0) };
        let (list, line) = if vertical { (&self.by_x, a.0) } else { (&self.by_y, a.1) };
        // Segments whose first end is within 0.75 of the line: the only ones that can be on it.
        let from = list.partition_point(|(v, _)| line - *v >= 0.75);
        let to = list.partition_point(|(v, _)| *v - line < 0.75);
        let mut near: Vec<usize> = list.get(from..to).unwrap_or_default().iter().map(|(_, i)| *i).collect();
        // Back in the segments' own order: the first one on the line is the one that shows.
        near.sort_unstable();
        let mut covered = 0.0;
        let mut first = None;
        for s in near.into_iter().filter_map(|i| self.strokes.get(i)) {
            let (on, s0, s1) = if vertical {
                ((s.a.x - a.0).abs() < 0.75 && (s.b.x - a.0).abs() < 0.75, s.a.y.min(s.b.y), s.a.y.max(s.b.y))
            } else {
                ((s.a.y - a.1).abs() < 0.75 && (s.b.y - a.1).abs() < 0.75, s.a.x.min(s.b.x), s.a.x.max(s.b.x))
            };
            let overlap = s1.min(hi) - s0.max(lo);
            if on && overlap > 0.0 {
                covered += overlap;
                first.get_or_insert(s);
            }
        }
        first.filter(|_| covered > 0.5 * (hi - lo))
    }
}

fn model_border(ex: &Exporter, s: Option<&StrokeSeg>, k: f64, opacity: f32) -> craft_pptx::Line {
    let Some(stroke) = s.filter(|s| s.stroke.weight > 0.0) else { return craft_pptx::Line::none() };
    let Some(color) = ex.model_color(&stroke.stroke.color, stroke.stroke.tint, opacity) else { return craft_pptx::Line::none() };
    let mut line = craft_pptx::Line::solid(color, stroke.stroke.weight * k);
    line.cap = Some(craft_pptx::style::LineCap::Flat);
    line.dash = match stroke.stroke.kind {
        StrokeType::Dashed { .. } => Some(craft_pptx::style::Dash::Preset { name: "dash".into() }),
        StrokeType::Dotted => Some(craft_pptx::style::Dash::Preset { name: "sysDot".into() }),
        _ => None,
    };
    line
}

fn model_cell_text(ex: &mut Exporter, c: &PlacedCell, k: f64, opacity: f32) -> (craft_pptx::TextBody, [f64; 4]) {
    let Some(frame) = c.text.frames.first() else {
        let mut text = craft_pptx::TextBody::from_text("");
        if let Some(paragraph) = text.paragraphs.first_mut() {
            paragraph.end_props.lang = Some("en-US".into());
        }
        return (text, [0.0; 4]);
    };
    let segments = crate::text::segments(ex, &c.text, frame, &c.source, &[]);
    let mut all = segments.first().cloned().unwrap_or_default();
    for segment in segments.iter().skip(1) {
        all.lines.extend(segment.lines.iter().cloned());
        all.x1 = all.x1.max(segment.x1);
    }
    if all.lines.is_empty() {
        let mut text = craft_pptx::TextBody::from_text("");
        if let Some(paragraph) = text.paragraphs.first_mut() {
            paragraph.end_props.lang = Some("en-US".into());
        }
        return (text, [0.0; 4]);
    }
    let bounds = crate::text::seg_rect(&all);
    let bounds = Rect::new(bounds.x0 + c.origin.x, bounds.y0 + c.origin.y, bounds.x1 + c.origin.x, bounds.y1 + c.origin.y);
    let margins =
        [(bounds.x0 - c.rect.x0).max(0.0), (bounds.y0 - c.rect.y0).max(0.0), (c.rect.x1 - bounds.x1).max(0.0), (c.rect.y1 - bounds.y1).max(0.0)];
    let mut body = crate::text::model_body(ex, &all, k, opacity, [0.0; 4]);
    body.body = Default::default();
    (body, margins)
}

fn table(ex: &mut Exporter, t: &TableFrag, m: Affine, k: f64, opacity: f32) {
    if t.cells.is_empty() {
        return;
    }
    let Some((xfrm, dm)) = box_of(t.rect, m) else { return };
    if dm.rot.abs() > 0.01 || xfrm.flip_v {
        ex.warn("tables in rotated or flipped frames are exported upright");
    }
    let xs = edges(t.cells.iter().flat_map(|c| [c.rect.x0, c.rect.x1]).collect());
    let ys = edges(t.cells.iter().flat_map(|c| [c.rect.y0, c.rect.y1]).collect());
    if xs.len() < 2 || ys.len() < 2 {
        return;
    }
    let (nc, nr) = (xs.len() - 1, ys.len() - 1);
    // Which placed cell starts at each grid position, and which positions merged cells cover.
    let mut origin: Vec<Option<&PlacedCell>> = vec![None; nc * nr];
    let mut covered: Vec<(bool, bool)> = vec![(false, false); nc * nr];
    let mut span: Vec<(usize, usize)> = vec![(1, 1); nc * nr];
    for c in &t.cells {
        let (c0, c1) = (index(&xs, c.rect.x0), index(&xs, c.rect.x1));
        let (r0, r1) = (index(&ys, c.rect.y0), index(&ys, c.rect.y1));
        if c1 <= c0 || r1 <= r0 {
            continue;
        }
        let Some(slot) = origin.get_mut(r0 * nc + c0) else { continue };
        *slot = Some(c);
        if let Some(sp) = span.get_mut(r0 * nc + c0) {
            *sp = (c1 - c0, r1 - r0);
        }
        for r in r0..r1 {
            for col in c0..c1 {
                if (r, col) != (r0, c0)
                    && let Some(cv) = covered.get_mut(r * nc + col)
                {
                    *cv = (col > c0, r > r0);
                }
            }
        }
    }
    if t.cells.iter().any(|c| c.graphic.is_some()) {
        ex.warn("graphic cells are exported empty");
    }
    let edges_of = EdgeIndex::new(&t.strokes);
    let id = ex.id();
    let mut model = craft_pptx::table::Table {
        cols: xs.windows(2).map(|w| (w[1] - w[0]) * k).collect(),
        rows: ys
            .windows(2)
            .map(|w| craft_pptx::table::Row { height: (w[1] - w[0]) * k, cells: vec![craft_pptx::table::Cell::default(); nc] })
            .collect(),
        style: String::new(),
        first_row: false,
        first_col: false,
        last_row: false,
        last_col: false,
        band_row: false,
        band_col: false,
        ..Default::default()
    };
    for row in 0..nr {
        for col in 0..nc {
            let at = row * nc + col;
            let Some(cell) = model.rows.get_mut(row).and_then(|r| r.cells.get_mut(col)) else { continue };
            let (horizontal, vertical) = covered.get(at).copied().unwrap_or((false, false));
            if horizontal || vertical {
                cell.h_merge = horizontal;
                cell.v_merge = vertical;
                continue;
            }
            let (grid_span, row_span) = span.get(at).copied().unwrap_or((1, 1));
            cell.grid_span = u32::try_from(grid_span).unwrap_or(u32::MAX);
            cell.row_span = u32::try_from(row_span).unwrap_or(u32::MAX);
            let Some(placed) = origin.get(at).copied().flatten() else {
                cell.fill = Some(craft_pptx::Fill::None);
                cell.margins = Some([0.0; 4]);
                continue;
            };
            let (text, [left, top, right, bottom]) = model_cell_text(ex, placed, k, opacity);
            cell.text = text;
            cell.margins = Some([left * k, right * k, top * k, bottom * k]);
            cell.fill = Some(
                placed
                    .fill
                    .as_ref()
                    .and_then(|(swatch, tint)| ex.model_color(swatch, *tint, opacity))
                    .map_or(craft_pptx::Fill::None, craft_pptx::Fill::solid),
            );
            let bounds = placed.rect;
            let edge = |side: char| {
                let (a, b) = match side {
                    'L' => ((bounds.x0, bounds.y0), (bounds.x0, bounds.y1)),
                    'R' => ((bounds.x1, bounds.y0), (bounds.x1, bounds.y1)),
                    'T' => ((bounds.x0, bounds.y0), (bounds.x1, bounds.y0)),
                    _ => ((bounds.x0, bounds.y1), (bounds.x1, bounds.y1)),
                };
                edges_of.stroke(a, b)
            };
            cell.borders = ['L', 'R', 'T', 'B'].map(|side| Some(model_border(ex, edge(side), k, opacity)));
        }
    }
    ex.native_shapes.push(craft_pptx::Shape {
        id: craft_pptx::ShapeId(id),
        name: format!("Table {id}"),
        xfrm: Some(craft_pptx::Xfrm::new(xfrm.rect.x0, xfrm.rect.y0, xfrm.rect.width(), xfrm.rect.height())),
        kind: craft_pptx::ShapeKind::Table(model),
        ..Default::default()
    });
}
