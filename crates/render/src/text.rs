//! Composed text → pixels.
//!
//! Glyph outlines of a frame are transformed into frame space and batched per line and run style
//! once, then cached per (composed story allocation, frame id): a composed story is immutable, so
//! the cache entry stays valid for as long as the composition cache hands out the same `Arc`.
//! Each render only fills the lines that intersect the visible rectangle, and lines smaller than
//! the greeking threshold become grey bars without touching any outline.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use designcraft_compose::{ComposedStory, FrameText, Line, ShownOn};
use designcraft_doc::ItemId;
use designcraft_fonts::FontDb;
use designcraft_geom::{Affine, BezPath, Rect, Shape};
use vello_cpu::RenderContext;
use vello_cpu::kurbo;
use vello_cpu::peniko;

use crate::{Frame, Renderer, color_of, rect_overlaps, screen_color};

/// A text colour: solid, or a gradient swatch laid across the frame (InDesign spans a gradient on
/// text across its frame's box).
#[derive(Clone)]
enum TextPaint {
    Solid(peniko::Color),
    Gradient(designcraft_doc::Fill),
}

impl TextPaint {
    fn of(doc: &designcraft_doc::Document, swatch: &str, tint: f32) -> Option<Self> {
        if designcraft_color::swatch::resolve_gradient(&doc.swatches, swatch).is_some() {
            return Some(TextPaint::Gradient(designcraft_doc::Fill { swatch: swatch.into(), tint, ..designcraft_doc::Fill::none() }));
        }
        doc.resolve_color(swatch, tint).map(|c| TextPaint::Solid(color_of(&c, 1.0)))
    }

    /// Make this the context's paint; a gradient runs across `area` (text space).
    fn set(&self, ctx: &mut RenderContext, doc: &designcraft_doc::Document, area: Rect) {
        match self {
            TextPaint::Solid(c) => ctx.set_paint(*c),
            TextPaint::Gradient(fill) => {
                crate::set_fill_paint(ctx, doc, fill, area);
            }
        }
    }
}

/// The box a gradient on a frame's text spans: the frame's, in text space (the union of its lines
/// when the frame is gone).
fn text_area(doc: &designcraft_doc::Document, ft: &FrameText) -> Rect {
    if let Some(it) = doc.item(ft.frame) {
        let local = doc.text_local(it);
        if local.determinant().abs() > 1e-12 {
            return local.inverse().transform_rect_bbox(it.inner_bounds());
        }
    }
    ft.lines.iter().filter_map(line_box).reduce(|a, b| a.union(b)).unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0))
}

/// Cached outlines of one composed line.
pub(crate) struct LineGlyphs {
    /// Frame-space bounds of everything drawn for the line (empty lines: `None`).
    pub bounds: Option<Rect>,
    /// Glyph outlines batched per run style.
    pub runs: Vec<(u32, BezPath)>,
    /// Bars below/above the glyph paint pass: colour swatch, tint, rectangle.
    pub underlines: Vec<(String, f32, Rect)>,
    pub strikes: Vec<(String, f32, Rect)>,
    pub glyphs: usize,
}

pub(crate) struct FrameGlyphs {
    pub lines: Vec<LineGlyphs>,
    /// Path elements held (cache budget).
    pub elements: usize,
}

struct Entry {
    /// Pins only the allocation identity used by the key. The cached paths own their data;
    /// old story layout buffers can drop once composition/render users release them. A Weak
    /// still prevents this allocation address from being reused by another story.
    _story: Weak<ComposedStory>,
    glyphs: Arc<FrameGlyphs>,
    stamp: u64,
}

/// Path cache keyed by (composed story address, frame id).
#[derive(Default)]
pub(crate) struct GlyphCache {
    map: HashMap<(usize, ItemId), Entry>,
    elements: usize,
    clock: u64,
}

/// Roughly 56 bytes per element: ~110 MB of cached outlines.
const MAX_ELEMENTS: usize = 2_000_000;

impl GlyphCache {
    pub fn tick(&mut self) {
        self.clock += 1;
    }

    fn get(&mut self, cs: &Arc<ComposedStory>, ft: &FrameText) -> Arc<FrameGlyphs> {
        let key = (Arc::as_ptr(cs) as usize, ft.frame);
        let stamp = self.clock;
        if let Some(e) = self.map.get_mut(&key) {
            e.stamp = stamp;
            return e.glyphs.clone();
        }
        let g = Arc::new(build(cs, ft));
        self.elements += g.elements;
        self.map.insert(key, Entry { _story: Arc::downgrade(cs), glyphs: g.clone(), stamp });
        if self.elements > MAX_ELEMENTS {
            self.evict();
        }
        g
    }

    /// Drop least recently used entries (never the ones used by the current render) until the
    /// cache is at half its budget.
    fn evict(&mut self) {
        let mut stamps: Vec<(u64, usize, (usize, ItemId))> = self.map.iter().map(|(k, e)| (e.stamp, e.glyphs.elements, *k)).collect();
        stamps.sort_unstable_by_key(|s| s.0);
        for (stamp, n, k) in stamps {
            if self.elements <= MAX_ELEMENTS / 2 || stamp >= self.clock {
                break;
            }
            self.map.remove(&k);
            self.elements -= n;
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.elements = 0;
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

fn transform_el(a: Affine, el: kurbo::PathEl) -> kurbo::PathEl {
    use kurbo::PathEl::*;
    match el {
        MoveTo(p) => MoveTo(a * p),
        LineTo(p) => LineTo(a * p),
        QuadTo(p1, p2) => QuadTo(a * p1, a * p2),
        CurveTo(p1, p2, p3) => CurveTo(a * p1, a * p2, a * p3),
        ClosePath => ClosePath,
    }
}

fn union(r: Option<Rect>, b: Rect) -> Option<Rect> {
    Some(r.map_or(b, |r| r.union(b)))
}

/// Add a rule bar, joined onto the previous one when it continues the same stretch.
fn push_bar(bars: &mut Vec<(String, f32, Rect)>, color: &str, tint: f32, r: Rect) {
    match bars.last_mut() {
        Some((c, t, last))
            if c.as_str() == color
                && *t == tint
                && (last.y0 - r.y0).abs() < 1e-6
                && (last.y1 - r.y1).abs() < 1e-6
                && (r.x0 - last.x1).abs() < 0.5 =>
        {
            last.x1 = last.x1.max(r.x1);
        }
        _ => bars.push((color.to_owned(), tint, r)),
    }
}

fn build_line(db: &FontDb, cs: &ComposedStory, l: &Line, vertical: bool) -> LineGlyphs {
    let mut runs: Vec<(u32, BezPath)> = Vec::new();
    let mut underlines: Vec<(String, f32, Rect)> = Vec::new();
    let mut strikes: Vec<(String, f32, Rect)> = Vec::new();
    let mut glyphs = 0;
    for g in &l.glyphs {
        if !g.spans_rules() {
            continue;
        }
        let style = &cs.styles[g.style as usize];
        for (on, rule, bars) in [(style.underline, &style.underline_rule, &mut underlines), (style.strikethrough, &style.strike_rule, &mut strikes)] {
            if on {
                let r = rule.rect(g.x, g.x + g.adv, designcraft_compose::rule_baseline(style, l, g));
                // One bar per stretch, as the PDF export draws them: a bar per glyph left
                // anti-aliased seams between the letters.
                push_bar(bars, &rule.color, rule.tint, r);
            }
        }
        // A tab carries the rule across its width but draws nothing itself.
        if !g.visible {
            continue;
        }
        let outline = db.outline(&g.face, g.gid);
        if outline.elements().is_empty() {
            continue;
        }
        let skew = if style.skew != 0.0 { Affine::new([1.0, 0.0, -style.skew.to_radians().tan(), 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
        let mut a = Affine::translate((g.x, l.baseline + g.y)) * skew * Affine::scale_non_uniform(g.sx, g.sy);
        if let Some(shatai) = style.shatai_xf(g, l.baseline) {
            a = shatai * a;
        }
        if vertical && let Some(turn) = g.vertical_xf(l.baseline) {
            // Upright in vertical text.
            a = turn * a;
        }
        let k = match runs.iter().position(|r| r.0 == g.style) {
            Some(k) => k,
            None => {
                runs.push((g.style, BezPath::new()));
                runs.len() - 1
            }
        };
        let bp = &mut runs[k].1;
        for el in outline.elements() {
            bp.push(transform_el(a, *el));
        }
        glyphs += 1;
    }
    let mut bounds = None;
    for (si, bp) in &runs {
        let st = &cs.styles[*si as usize];
        let sw = if st.stroke != designcraft_color::swatch::NONE { st.stroke_weight } else { 0.0 };
        bounds = union(bounds, bp.bounding_box().inflate(sw, sw));
    }
    for (_, _, r) in underlines.iter().chain(&strikes) {
        bounds = union(bounds, *r);
    }
    LineGlyphs { bounds, runs, underlines, strikes, glyphs }
}

fn build(cs: &ComposedStory, ft: &FrameText) -> FrameGlyphs {
    let db = FontDb::global();
    let lines: Vec<LineGlyphs> = ft.lines.iter().map(|l| build_line(db, cs, l, ft.vertical)).collect();
    let elements = lines.iter().flat_map(|l| &l.runs).map(|(_, bp)| bp.elements().len()).sum();
    FrameGlyphs { lines, elements }
}

/// Conservative frame-space box of a line from its metrics (no outlines needed).
fn line_box(l: &Line) -> Option<Rect> {
    let first = l.glyphs.first()?;
    let x0 = l.glyphs.iter().map(|g| g.x).fold(first.x, f64::min);
    let x1 = l.glyphs.iter().map(|g| g.x + g.adv.max(0.0)).fold(l.end_x, f64::max);
    let pad = l.ascent.max(1.0);
    // A drop cap reaches down past its line.
    let bottom = l.drop_cap.map_or(l.baseline + l.descent.max(0.0), |dc| dc.rect.y1.max(l.baseline + l.descent.max(0.0)));
    Some(Rect::new(x0 - pad, l.baseline - l.ascent - pad, x1 + pad, bottom + pad))
}

impl Renderer {
    pub(crate) fn draw_text(&mut self, ctx: &mut RenderContext, f: &Frame, cs: &Arc<ComposedStory>, ft: &FrameText, xf: Affine) {
        let doc = f.doc;
        let m = f.view * xf;
        // Visible area in frame space.
        if xf.determinant().abs() < 1e-12 {
            return;
        }
        let vis = xf.inverse().transform_rect_bbox(f.visible);
        // Decorations under text (shading) and rules.
        for d in &ft.decos {
            if !rect_overlaps(d.rect, vis) || d.nonprinting && f.opts.printing_only {
                continue;
            }
            if let Some(c) = doc.resolve_color(&d.color, d.tint) {
                ctx.set_transform(m);
                ctx.set_paint(color_of(&c, 1.0));
                match &d.path {
                    Some(p) => ctx.fill_path(p),
                    None => ctx.fill_rect(&d.rect),
                }
            }
        }
        if !ft.tables.is_empty() {
            self.draw_tables(ctx, f, ft, xf);
        }
        // Text in fonts that aren't installed: pink behind the glyphs (screen only).
        if f.opts.highlight_missing_fonts && cs.styles.iter().any(|s| s.missing_font) {
            ctx.set_transform(m);
            ctx.set_paint(screen_color([255, 166, 214], 255));
            for l in &ft.lines {
                for g in l.glyphs.iter().filter(|g| g.adv > 0.0 && cs.styles.get(g.style as usize).is_some_and(|s| s.missing_font)) {
                    ctx.fill_rect(&kurbo::Rect::new(g.x, l.baseline - l.ascent, g.x + g.adv, l.baseline + l.descent));
                }
            }
        }
        if f.opts.highlight_keeps && ft.lines.iter().any(|l| l.keep_violation) {
            ctx.set_transform(m);
            ctx.set_paint(screen_color([255, 236, 120], 255));
            for l in ft.lines.iter().filter(|l| l.keep_violation) {
                ctx.fill_rect(&kurbo::Rect::new(l.x0, l.baseline - l.ascent, l.x1, l.baseline + l.descent));
            }
        }
        if f.opts.highlight_hj && ft.lines.iter().any(|l| l.hj > 0) {
            ctx.set_transform(m);
            for l in ft.lines.iter().filter(|l| l.hj > 0) {
                let shade = [0u8, 0xF6, 0xEA, 0xD8][l.hj.min(3) as usize];
                ctx.set_paint(screen_color([255, shade, [0u8, 0xB0, 0x70, 0x30][l.hj.min(3) as usize]], 255));
                ctx.fill_rect(&kurbo::Rect::new(l.x0, l.baseline - l.ascent, l.end_x.max(l.x0 + 1.0), l.baseline + l.descent));
            }
        }
        if f.opts.highlight_custom_tracking && cs.styles.iter().any(|s| s.custom_tracking) {
            ctx.set_transform(m);
            ctx.set_paint(screen_color([176, 236, 176], 255));
            for l in &ft.lines {
                for g in l.glyphs.iter().filter(|g| g.adv > 0.0 && cs.styles.get(g.style as usize).is_some_and(|s| s.custom_tracking)) {
                    ctx.fill_rect(&kurbo::Rect::new(g.x, l.baseline - l.ascent, g.x + g.adv, l.baseline + l.descent));
                }
            }
        }
        if f.opts.tag_markers && cs.styles.iter().any(|s| s.xml_tag.is_some()) {
            ctx.set_transform(m);
            let px = 1.0 / m.determinant().abs().sqrt().max(1e-9);
            for l in &ft.lines {
                let tag_of = |g: &designcraft_compose::PlacedGlyph| cs.styles.get(g.style as usize).and_then(|s| s.xml_tag.clone());
                let gs: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0).collect();
                let mut i = 0;
                while i < gs.len() {
                    let Some(tag) = tag_of(gs[i]) else {
                        i += 1;
                        continue;
                    };
                    let mut j = i + 1;
                    while j < gs.len() && tag_of(gs[j]).as_deref() == Some(tag.as_str()) {
                        j += 1;
                    }
                    let c = f.doc.xml.tags.iter().find(|t| t.name == tag).map_or([100, 100, 220], |t| t.color);
                    ctx.set_paint(screen_color([c[0], c[1], c[2]], 255));
                    let (x0, x1) = (gs[i].x, gs[j - 1].x + gs[j - 1].adv);
                    let (top, bot) = (l.baseline - l.ascent, l.baseline + l.descent);
                    let (w, t) = (2.5 * px, px.max(0.3));
                    // [ before the element, ] after it.
                    for (x, dir) in [(x0, 1.0), (x1, -1.0)] {
                        ctx.fill_rect(&kurbo::Rect::new(x - t / 2.0, top, x + t / 2.0, bot));
                        for y in [top, bot - t] {
                            let (a, b) = if dir > 0.0 { (x, x + w) } else { (x - w, x) };
                            ctx.fill_rect(&kurbo::Rect::new(a, y, b, y + t));
                        }
                    }
                    i = j;
                }
            }
        }
        if f.opts.change_markup && cs.styles.iter().any(|s| s.inserted) {
            ctx.set_transform(m);
            for l in &ft.lines {
                for g in l.glyphs.iter().filter(|g| g.adv > 0.0 && cs.styles.get(g.style as usize).is_some_and(|s| s.inserted)) {
                    ctx.set_paint(screen_color([120, 190, 255], 110));
                    ctx.fill_rect(&kurbo::Rect::new(g.x, l.baseline - l.ascent, g.x + g.adv, l.baseline + l.descent));
                }
            }
        }
        if f.opts.note_indicators
            && let Some(st) = f.doc.story(cs.story).filter(|st| !st.editorial.is_empty())
        {
            // A small flag at each note anchor, in the notes' amber.
            ctx.set_transform(m);
            ctx.set_paint(screen_color([232, 160, 32], 255));
            for l in &ft.lines {
                for g in l.glyphs.iter().filter(|g| st.text.get(g.byte..).is_some_and(|t| t.starts_with(designcraft_doc::NOTE_MARK))) {
                    let h = l.ascent.max(4.0);
                    let mut p = BezPath::new();
                    p.move_to((g.x - h * 0.25, l.baseline - h));
                    p.line_to((g.x + h * 0.25, l.baseline - h));
                    p.line_to((g.x, l.baseline - h * 0.55));
                    p.close_path();
                    ctx.fill_path(&p);
                    ctx.fill_rect(&kurbo::Rect::new(g.x - h * 0.03, l.baseline - h * 0.6, g.x + h * 0.03, l.baseline + l.descent * 0.5));
                }
            }
        }
        if f.opts.condition_indicators && !f.doc.conditions.is_empty() && cs.styles.iter().any(|s| s.condition.is_some()) {
            ctx.set_transform(m);
            let px = 1.0 / m.determinant().abs().sqrt().max(1e-9);
            for l in &ft.lines {
                for g in l.glyphs.iter().filter(|g| g.visible && g.adv > 0.0) {
                    let Some(name) = cs.styles.get(g.style as usize).and_then(|s| s.condition.as_deref()) else { continue };
                    let Some(c) = f.doc.conditions.iter().find(|c| c.name == name) else { continue };
                    ctx.set_paint(screen_color([c.color[0], c.color[1], c.color[2]], 255));
                    let y = l.baseline + l.descent * 0.6;
                    ctx.fill_rect(&kurbo::Rect::new(g.x, y, g.x + g.adv, y + 2.0 * px));
                }
            }
        }
        let scale = m.determinant().abs().sqrt();
        let greek_px = f.opts.greek_below_px;
        let mut greek = BezPath::new();
        let mut shown: Vec<usize> = Vec::new();
        for (i, l) in ft.lines.iter().enumerate() {
            let Some(b) = line_box(l) else { continue };
            if !rect_overlaps(b, vis) {
                continue;
            }
            if greek_px > 0.0 && l.ascent * scale < greek_px {
                if let Some(a) = l.glyphs.first() {
                    let r = kurbo::Rect::new(a.x, l.baseline - l.ascent * 0.5, l.end_x, l.baseline);
                    if r.width() > 0.0 {
                        greek.extend(r.path_elements(0.1));
                    }
                }
                continue;
            }
            shown.push(i);
        }
        ctx.set_transform(m);
        if !shown.is_empty() {
            let fg = self.glyphs.get(cs, ft);
            // Resolve each style's paints once per frame; gradients span the frame.
            let area = text_area(doc, ft);
            let mut fills: Vec<Option<Option<TextPaint>>> = vec![None; cs.styles.len()];
            let mut fill_of = |si: u32| -> Option<TextPaint> {
                fills[si as usize]
                    .get_or_insert_with(|| {
                        let st = &cs.styles[si as usize];
                        TextPaint::of(doc, &st.fill, st.fill_tint)
                    })
                    .clone()
            };
            shown.retain(|&i| fg.lines[i].bounds.is_some_and(|b| rect_overlaps(b, vis)));
            // Paint all underlines before any glyphs: skewed runs and tightly spaced lines
            // can overlap a later run's rule. A per-run paint pass would still cover ink.
            for &i in &shown {
                for (sw, tint, r) in &fg.lines[i].underlines {
                    if let Some(c) = doc.resolve_color(sw, *tint) {
                        ctx.set_paint(color_of(&c, 1.0));
                        ctx.fill_rect(r);
                    }
                }
            }
            for &i in &shown {
                let lg = &fg.lines[i];
                self.stats.glyphs += lg.glyphs;
                for (si, bp) in &lg.runs {
                    let st = &cs.styles[*si as usize];
                    let fill = fill_of(*si).map(|c| (c, crate::overprints(f, &st.fill, st.fill_tint, st.overprint_fill)));
                    let stroke = stroke_of(doc, st).map(|c| (c, crate::overprints(f, &st.stroke, st.stroke_tint, st.overprint_stroke)));
                    paint_glyphs(ctx, doc, area, bp, fill, stroke, st.stroke_weight, st.stroke_behind_fill);
                }
            }
            for &i in &shown {
                for (sw, tint, r) in &fg.lines[i].strikes {
                    if let Some(c) = doc.resolve_color(sw, *tint) {
                        ctx.set_paint(color_of(&c, 1.0));
                        ctx.fill_rect(r);
                    }
                }
            }
        }
        if !greek.elements().is_empty() {
            ctx.set_paint(color_of(&designcraft_color::Color::gray(0.35), 0.5));
            ctx.fill_path(&greek);
        }
        for n in &ft.notes {
            if let Some(nft) = n.text.frames.first() {
                self.draw_text(ctx, f, &n.text, nft, xf * Affine::translate(n.origin.to_vec2()));
            }
        }
        // Anchored objects ride on their lines.
        for o in &ft.objects {
            self.draw_item(ctx, f, &o.object.item, xf * Affine::translate(o.origin.to_vec2()), ShownOn::Own);
        }
    }
}

impl Renderer {
    /// Table fragments: cell fills, cell text, then cell edges and the border.
    fn draw_tables(&mut self, ctx: &mut RenderContext, f: &Frame, ft: &FrameText, xf: Affine) {
        let doc = f.doc;
        for t in &ft.tables {
            for c in &t.cells {
                if let Some((sw, tint)) = &c.fill {
                    ctx.set_transform(f.view * xf);
                    let fill = designcraft_doc::Fill { swatch: sw.clone(), tint: *tint, ..designcraft_doc::Fill::none() };
                    // Overlap neighbours by half a device pixel so adjacent fills show no seams.
                    let h = 0.5 * f.px;
                    // All cells share the table's gradient space; it must not restart at a cell edge.
                    if crate::set_fill_paint(ctx, doc, &fill, t.rect) {
                        ctx.fill_rect(&c.rect.inflate(h, h));
                    }
                    ctx.reset_paint_transform();
                }
            }
            for c in &t.cells {
                if let Some(g) = &c.graphic {
                    ctx.set_transform(f.view * xf);
                    ctx.push_clip_layer(&c.clip.to_path(0.1));
                    self.draw_graphic(ctx, f, g, xf * Affine::translate((c.clip.x0, c.clip.y0)));
                    ctx.set_transform(f.view * xf);
                    ctx.pop_layer();
                } else if let Some(cft) = c.text.frames.first() {
                    self.draw_text(ctx, f, &c.text, cft, xf * Affine::translate(c.origin.to_vec2()));
                }
            }
            ctx.set_transform(f.view * xf);
            for s in &t.strokes {
                let Some(col) = doc.resolve_color(&s.stroke.color, s.stroke.tint) else { continue };
                let kind = doc.stroke_kind(&s.stroke.kind);
                let line = kurbo::Line::new(s.a, s.b).to_path(0.1);
                // Butt-ended fills close the table's corners the way square caps do.
                let (ca, cb) = t.corner_capped(s);
                let capped = kurbo::Line::new(ca, cb).to_path(0.1);
                // Gap colour under dashes, dots and stripes: the whole edge, solid.
                if !matches!(kind, designcraft_doc::StrokeType::Solid)
                    && let Some(gap) = doc.resolve_color(&s.stroke.gap_color, s.stroke.gap_tint)
                {
                    ctx.set_paint(color_of(&gap, 1.0));
                    ctx.set_stroke(kurbo::Stroke::new(s.stroke.weight).with_caps(kurbo::Cap::Butt));
                    ctx.stroke_path(&capped);
                }
                ctx.set_paint(color_of(&col, 1.0));
                // Stripes, wavy and hash types are fills along the edge.
                if let Some(o) = kind.outline(&capped, s.stroke.weight, 0.05 * f.px) {
                    ctx.fill_path(&o);
                    continue;
                }
                ctx.set_stroke(cell_stroke(&kind, s.stroke.weight, s.a.distance(s.b)));
                ctx.stroke_path(&line);
            }
        }
    }
}

/// A run's stroke colour, if it has one.
fn stroke_of(doc: &designcraft_doc::Document, st: &designcraft_compose::RunStyle) -> Option<TextPaint> {
    if st.stroke == designcraft_color::swatch::NONE {
        return None;
    }
    TextPaint::of(doc, &st.stroke, st.stroke_tint)
}

/// Run `paint` in a Multiply layer when `overprint` is set.
fn overprinting(ctx: &mut RenderContext, overprint: bool, paint: impl FnOnce(&mut RenderContext)) {
    if overprint {
        ctx.push_layer(None, Some(peniko::BlendMode::new(peniko::Mix::Multiply, peniko::Compose::SrcOver)), None, None, None);
    }
    paint(ctx);
    if overprint {
        ctx.pop_layer();
    }
}

/// Paint glyph outlines: the fill, then the stroke over it, each multiplied when it overprints
/// (`(colour, overprint)`). With stroke behind fill a double-width stroke goes first and the fill
/// covers its inner half; with no fill to cover it, the glyph shapes are cut out of the stroke
/// instead.
#[allow(clippy::too_many_arguments)]
fn paint_glyphs(
    ctx: &mut RenderContext,
    doc: &designcraft_doc::Document,
    area: Rect,
    bp: &BezPath,
    fill: Option<(TextPaint, bool)>,
    stroke: Option<(TextPaint, bool)>,
    weight: f64,
    behind: bool,
) {
    // A gradient paint runs across `area`; reset its transform after each use.
    let paint_fill = |ctx: &mut RenderContext| {
        let Some((c, overprint)) = &fill else { return };
        overprinting(ctx, *overprint, |ctx| {
            c.set(ctx, doc, area);
            ctx.fill_path(bp);
            ctx.reset_paint_transform();
        });
    };
    let Some((sc, stroke_overprint)) = stroke else {
        paint_fill(ctx);
        return;
    };
    let paint_stroke = |ctx: &mut RenderContext, width: f64| {
        overprinting(ctx, stroke_overprint, |ctx| {
            sc.set(ctx, doc, area);
            ctx.set_stroke(kurbo::Stroke::new(width));
            ctx.stroke_path(bp);
            ctx.reset_paint_transform();
        });
    };
    if !behind {
        paint_fill(ctx);
        paint_stroke(ctx, weight);
        return;
    }
    if fill.is_some() {
        paint_stroke(ctx, weight * 2.0);
        paint_fill(ctx);
    } else {
        overprinting(ctx, stroke_overprint, |ctx| {
            ctx.push_layer(None, None, None, None, None);
            sc.set(ctx, doc, area);
            ctx.set_stroke(kurbo::Stroke::new(weight * 2.0));
            ctx.stroke_path(bp);
            ctx.push_layer(None, Some(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::DestOut)), None, None, None);
            ctx.fill_path(bp);
            ctx.pop_layer();
            ctx.pop_layer();
            ctx.reset_paint_transform();
        });
    }
}

/// Kurbo stroke for a table edge `length` points long of a resolved stroke type.
pub(crate) fn cell_stroke(kind: &designcraft_doc::StrokeType, weight: f64, length: f64) -> kurbo::Stroke {
    use designcraft_doc::StrokeType;
    let st = kurbo::Stroke::new(weight).with_caps(kurbo::Cap::Square);
    match kind {
        StrokeType::Dashed { pattern } if StrokeType::expandable_dashes(pattern, length) => {
            st.with_caps(kurbo::Cap::Butt).with_dashes(0.0, pattern.iter().copied())
        }
        StrokeType::Dotted if StrokeType::expandable_dashes(&[0.0, weight * 2.0], length) => {
            st.with_dashes(0.0, [0.0, weight * 2.0]).with_caps(kurbo::Cap::Round)
        }
        _ => st,
    }
}

impl Renderer {
    /// Type on a path: glyphs follow the frame's path.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_path_text(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        cs: &Arc<ComposedStory>,
        ft: &FrameText,
        xf: Affine,
        path: &BezPath,
        pt: &designcraft_doc::PathType,
    ) {
        let doc = f.doc;
        ctx.set_transform(f.view * xf);
        // A gradient on type on a path spans the path's box.
        let area = path.bounding_box();
        for (si, bp) in designcraft_compose::path_glyphs(cs, ft, path, pt) {
            let st = &cs.styles[si as usize];
            let fill = TextPaint::of(doc, &st.fill, st.fill_tint).map(|c| (c, false));
            paint_glyphs(ctx, doc, area, &bp, fill, stroke_of(doc, st).map(|c| (c, false)), st.stroke_weight, st.stroke_behind_fill);
            self.stats.glyphs += 1;
        }
    }
}

#[cfg(test)]
mod cache_identity_tests {
    use super::*;

    fn composition() -> Arc<ComposedStory> {
        Arc::new(ComposedStory { frames: vec![FrameText { frame: ItemId(1), ..Default::default() }], ..Default::default() })
    }

    fn rich_composition() -> (crate::Cache, Arc<ComposedStory>) {
        let mut doc = designcraft_doc::Document::new(&Default::default());
        for style in &mut doc.styles_mut().paragraph {
            style.chars.underline = Some(true);
        }
        let layer = doc.default_layer();
        let (_, sid) = doc
            .add_text_frame(
                designcraft_doc::SpreadRef::Doc(0),
                Rect::new(36.0, 36.0, 300.0, 300.0),
                layer,
                "Visible decorated glyphs.",
                designcraft_doc::ParaFormat::default(),
            )
            .unwrap();
        let owner = crate::Cache::new();
        let cs = owner.get(&doc, sid, None);
        (owner, cs)
    }

    #[test]
    fn cache_owns_weak_identity_and_reuses_live_glyphs() {
        let cs = composition();
        let mut cache = GlyphCache::default();
        cache.tick();
        let first = cache.get(&cs, &cs.frames[0]);
        assert_eq!(Arc::strong_count(&cs), 1, "the glyph cache must not own the whole layout");
        assert_eq!(Arc::weak_count(&cs), 1, "the glyph cache must still pin the allocation identity");
        assert!(Arc::ptr_eq(&first, &cache.get(&cs, &cs.frames[0])));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn make_mut_moves_to_a_new_cache_identity_without_reusing_old_paths() {
        let mut cs = composition();
        let old_address = Arc::as_ptr(&cs) as usize;
        let mut cache = GlyphCache::default();
        cache.tick();
        let original_paths = cache.get(&cs, &cs.frames[0]);
        Arc::make_mut(&mut cs).text_len = 5;
        assert_ne!(Arc::as_ptr(&cs) as usize, old_address);
        let new_paths = cache.get(&cs, &cs.frames[0]);
        assert!(!Arc::ptr_eq(&original_paths, &new_paths));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn in_flight_owner_keeps_layout_valid_then_releases_it_without_losing_outlines() {
        let (origin, cs) = rich_composition();
        let in_flight = cs.clone();
        let lifetime = Arc::downgrade(&cs);
        let mut cache = GlyphCache::default();
        cache.tick();
        let glyphs = cache.get(&cs, &cs.frames[0]);
        assert!(glyphs.elements > 0);
        let runs = glyphs.lines[0].runs.clone();
        let underlines = glyphs.lines[0].underlines.clone();
        assert!(!underlines.is_empty());
        drop(cs);
        origin.clear();
        assert!(lifetime.upgrade().is_some());
        assert!(!in_flight.styles.is_empty());
        assert!(Arc::ptr_eq(&glyphs, &cache.get(&in_flight, &in_flight.frames[0])));
        drop(in_flight);
        assert!(lifetime.upgrade().is_none(), "only the real in-flight owner should keep the layout alive");
        assert_eq!(glyphs.lines[0].runs, runs);
        assert_eq!(glyphs.lines[0].underlines, underlines);
        assert_eq!(cache.len(), 1, "no path eviction was needed");
    }

    #[test]
    fn clearing_cache_releases_weak_identity_but_not_independently_owned_paths() {
        let (origin, cs) = rich_composition();
        let mut cache = GlyphCache::default();
        cache.tick();
        let glyphs = cache.get(&cs, &cs.frames[0]);
        let runs = glyphs.lines[0].runs.clone();
        assert!(glyphs.elements > 0);
        assert_eq!(Arc::weak_count(&cs), 1);
        cache.clear();
        assert_eq!(Arc::weak_count(&cs), 0);
        assert_eq!(cache.len(), 0);
        origin.clear();
        drop(cs);
        assert_eq!(glyphs.lines[0].runs, runs);
    }
}

#[cfg(test)]
mod rule_tests {
    use super::*;

    /// A tab is not drawn, but it is a character with an advance, and InDesign underlines it:
    /// forms rule their fill-in lines with underlined tabs (`Name ⇥⇥⇥⇥, city ⇥⇥⇥⇥`). The bars
    /// stopped at the last letter before each tab, so an imported form lost every writing line
    /// (#1466).
    #[test]
    fn an_underline_runs_across_a_tab() {
        let mut doc = designcraft_doc::Document::new(&Default::default());
        for style in &mut doc.styles_mut().paragraph {
            style.chars.underline = Some(true);
        }
        let layer = doc.default_layer();
        let (_, sid) = doc
            .add_text_frame(
                designcraft_doc::SpreadRef::Doc(0),
                Rect::new(36.0, 36.0, 400.0, 300.0),
                layer,
                "Name\tCity",
                designcraft_doc::ParaFormat::default(),
            )
            .unwrap();
        let owner = crate::Cache::new();
        let cs = owner.get(&doc, sid, None);
        let line = &cs.frames[0].lines[0];
        let tab = line.glyphs.iter().find(|g| g.rendered_char == '\t').expect("the tab is placed on the line");
        assert!(!tab.visible, "a tab draws nothing itself");
        assert!(tab.adv > 1.0, "it advances to its stop: {}", tab.adv);

        let mut cache = GlyphCache::default();
        cache.tick();
        let glyphs = cache.get(&cs, &cs.frames[0]);
        let bars = &glyphs.lines[0].underlines;
        assert_eq!(bars.len(), 1, "one bar across the line, not one before the tab and one after: {bars:?}");
        let bar = bars[0].2;
        assert!(bar.x0 <= tab.x + 0.5, "the bar starts at the text, not at the tab: {bar:?}");
        assert!(bar.x1 >= tab.x + tab.adv - 0.5, "the bar covers the tab's width: {bar:?}, tab {} + {}", tab.x, tab.adv);
    }
}
