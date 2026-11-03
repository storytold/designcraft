//! Table fragments as native PowerPoint tables: the grid from the cells' edges, merged cells as
//! spans, fills, edges as cell borders, and each cell's composed text.

use std::fmt::Write as _;

use designcraft_compose::{FrameText, PlacedCell, StrokeSeg, TableFrag};
use designcraft_doc::StrokeType;
use designcraft_geom::{Affine, Rect};

use crate::slide::{Exporter, box_of};
use crate::text::{paragraphs, seg_rect, segments};
use crate::xml::{emu, emu_len};

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
    /// (x of the first end, index), for segments with both ends finite; sorted by x.
    by_x: Vec<(f64, usize)>,
    by_y: Vec<(f64, usize)>,
}

impl<'a> EdgeIndex<'a> {
    fn new(strokes: &'a [StrokeSeg]) -> Self {
        let finite = |s: &StrokeSeg| [s.a.x, s.a.y, s.b.x, s.b.y].iter().all(|v| v.is_finite());
        let mut by_x: Vec<(f64, usize)> = strokes.iter().enumerate().filter(|(_, s)| finite(s)).map(|(i, s)| (s.a.x, i)).collect();
        let mut by_y: Vec<(f64, usize)> = strokes.iter().enumerate().filter(|(_, s)| finite(s)).map(|(i, s)| (s.a.y, i)).collect();
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
        let from = list.partition_point(|(v, _)| *v <= line - 0.75);
        let to = list.partition_point(|(v, _)| *v < line + 0.75);
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

fn border(ex: &mut Exporter, tag: &str, s: Option<&StrokeSeg>, k: f64, opacity: f32) -> String {
    let Some(s) = s.filter(|s| s.stroke.weight > 0.0) else { return format!("<a:{tag} w=\"0\"><a:noFill/></a:{tag}>") };
    let st = &s.stroke;
    let Some(c) = ex.color(&st.color, st.tint, opacity) else { return format!("<a:{tag} w=\"0\"><a:noFill/></a:{tag}>") };
    let dash = match st.kind {
        StrokeType::Dashed { .. } => "dash",
        StrokeType::Dotted => "sysDot",
        _ => "solid",
    };
    format!(
        "<a:{tag} w=\"{}\" cap=\"flat\" cmpd=\"sng\"><a:solidFill>{c}</a:solidFill><a:prstDash val=\"{dash}\"/></a:{tag}>",
        emu_len(st.weight * k)
    )
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
    let mut s = format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>\
<p:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr/><a:tblGrid>",
        emu(xfrm.rect.x0),
        emu(xfrm.rect.y0),
        emu_len(xfrm.rect.width()),
        emu_len(xfrm.rect.height())
    );
    for w in xs.windows(2) {
        let _ = write!(s, "<a:gridCol w=\"{}\"/>", emu_len((w[1] - w[0]) * k));
    }
    s.push_str("</a:tblGrid>");
    for r in 0..nr {
        let h = ys.get(r + 1).zip(ys.get(r)).map_or(0.0, |(b, a)| b - a);
        let _ = write!(s, "<a:tr h=\"{}\">", emu_len(h * k));
        for col in 0..nc {
            let at = r * nc + col;
            let (hm, vm) = covered.get(at).copied().unwrap_or((false, false));
            if hm || vm {
                let _ = write!(
                    s,
                    "<a:tc{}{}><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\" dirty=\"0\"/></a:p></a:txBody><a:tcPr/></a:tc>",
                    if hm { " hMerge=\"1\"" } else { "" },
                    if vm { " vMerge=\"1\"" } else { "" }
                );
                continue;
            }
            let (gs, rs) = span.get(at).copied().unwrap_or((1, 1));
            let mut attrs = String::new();
            if gs > 1 {
                let _ = write!(attrs, " gridSpan=\"{gs}\"");
            }
            if rs > 1 {
                let _ = write!(attrs, " rowSpan=\"{rs}\"");
            }
            let Some(c) = origin.get(at).copied().flatten() else {
                let _ = write!(
                    s,
                    "<a:tc{attrs}><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\" dirty=\"0\"/></a:p></a:txBody><a:tcPr marL=\"0\" marR=\"0\" marT=\"0\" marB=\"0\"><a:noFill/></a:tcPr></a:tc>"
                );
                continue;
            };
            s.push_str(&cell(ex, c, &attrs, &edges_of, k, opacity));
        }
        s.push_str("</a:tr>");
    }
    s.push_str("</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
    ex.out.push_str(&s);
}

fn cell(ex: &mut Exporter, c: &PlacedCell, attrs: &str, edges: &EdgeIndex, k: f64, opacity: f32) -> String {
    let rc = c.rect;
    let mut body = String::new();
    let mut margins = [0.0f64; 4];
    if let Some(cft) = c.text.frames.first() {
        let segs = segments(ex, &c.text, cft, &c.source, &[]);
        // A cell holds one box: its lines, from the first segment's slot.
        let mut all = segs.first().cloned().unwrap_or_default();
        for sg in segs.iter().skip(1) {
            all.lines.extend(sg.lines.iter().cloned());
            all.x1 = all.x1.max(sg.x1);
        }
        if !all.lines.is_empty() {
            let r = seg_rect(&all);
            let r = Rect::new(r.x0 + c.origin.x, r.y0 + c.origin.y, r.x1 + c.origin.x, r.y1 + c.origin.y);
            margins = [(r.x0 - rc.x0).max(0.0), (r.y0 - rc.y0).max(0.0), (rc.x1 - r.x1).max(0.0), (rc.y1 - r.y1).max(0.0)];
            let local = Rect::new(r.x0 - c.origin.x, r.y0 - c.origin.y, r.x1 - c.origin.x, r.y1 - c.origin.y);
            body = paragraphs(ex, &all, local, k, opacity);
        }
    }
    if body.is_empty() {
        body = "<a:p><a:endParaRPr lang=\"en-US\" dirty=\"0\"/></a:p>".into();
    }
    let [l, tp, r, b] = margins.map(|v| emu_len(v * k));
    let ln = |side: char| -> ((f64, f64), (f64, f64)) {
        match side {
            'L' => ((rc.x0, rc.y0), (rc.x0, rc.y1)),
            'R' => ((rc.x1, rc.y0), (rc.x1, rc.y1)),
            'T' => ((rc.x0, rc.y0), (rc.x1, rc.y0)),
            _ => ((rc.x0, rc.y1), (rc.x1, rc.y1)),
        }
    };
    let mut pr = format!("<a:tcPr marL=\"{l}\" marR=\"{r}\" marT=\"{tp}\" marB=\"{b}\">");
    for (tag, side) in [("lnL", 'L'), ("lnR", 'R'), ("lnT", 'T'), ("lnB", 'B')] {
        let (a, bb) = ln(side);
        let seg = edges.stroke(a, bb);
        pr.push_str(&border(ex, tag, seg, k, opacity));
    }
    match c.fill.as_ref().and_then(|(sw, tint)| ex.color(sw, *tint, opacity)) {
        Some(col) => {
            let _ = write!(pr, "<a:solidFill>{col}</a:solidFill>");
        }
        None => pr.push_str("<a:noFill/>"),
    }
    pr.push_str("</a:tcPr>");
    format!("<a:tc{attrs}><a:txBody><a:bodyPr/><a:lstStyle/>{body}</a:txBody>{pr}</a:tc>")
}
