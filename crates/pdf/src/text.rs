//! Composed text → PDF text objects with embedded (subsetted) fonts.
//!
//! Each line is split into runs of glyphs sharing face, style and scale. A run is drawn with one
//! `draw_glyphs` call: the run origin, horizontal scale and skew go into the transform, glyph
//! advances and offsets are taken from the composed positions (so justification, tracking, kerning
//! and mark positioning are exact), and every glyph carries the story text it came from so viewers
//! can select, search and copy it.
//!
//! Composed lines hold their glyphs in text order, so content streams list text in reading order
//! even where right-to-left text is drawn leftwards. Right-to-left lines and runs carry no
//! ActualText of their own: poppler orders right-to-left glyphs by itself, but it positions an
//! ActualText with the transform in force where its span ends (losing a span over several runs)
//! and reverses the characters of one in a right-to-left line.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use craft_pdf::write::backend as krilla;
use designcraft_compose::{ComposedStory, FrameText, PlacedGlyph};
use designcraft_fonts::{FontDb, FontFace};
use designcraft_geom::kurbo::Shape as _;
use designcraft_geom::{Affine, BezPath, Rect};
use krilla::geom::Point;
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, FillRule, Paint, Stroke};
use krilla::surface::Surface;
use krilla::tagging::{Artifact, ArtifactType, ContentTag, SpanTag};
use krilla::text::{Font, GlyphId, KrillaGlyph};

use crate::export::{Exporter, solid_fill, tf, to_path};

impl Exporter<'_> {
    /// A text run's paint: solid, or a gradient spanning the frame's box (`area`, text space),
    /// for glyphs drawn under `origin` (text space ← run space).
    fn text_paint(&mut self, swatch: &str, tint: f32, origin: Affine, area: Rect) -> Option<Paint> {
        if designcraft_color::swatch::resolve_gradient(&self.doc.swatches, swatch).is_some() && origin.determinant().abs() > 1e-12 {
            let fill = designcraft_doc::Fill { swatch: swatch.into(), tint, ..designcraft_doc::Fill::none() };
            return self.fill_paint_in(&fill, self.text_area.unwrap_or(area), origin.inverse());
        }
        self.swatch_color(swatch, tint).map(Into::into)
    }
}

/// The box a gradient on a cell's or a note's text spans: its lines, each padded by its ascent as
/// the screen renderer pads them, so both lay the gradient out alike.
fn lines_area(ft: &FrameText) -> Option<Rect> {
    ft.lines
        .iter()
        .filter_map(|l| {
            let first = l.glyphs.first()?;
            let x0 = l.glyphs.iter().map(|g| g.x).fold(first.x, f64::min);
            let x1 = l.glyphs.iter().map(|g| g.x + g.adv.max(0.0)).fold(l.end_x, f64::max);
            let pad = l.ascent.max(1.0);
            let bottom = l.drop_cap.map_or(l.baseline + l.descent.max(0.0), |dc| dc.rect.y1.max(l.baseline + l.descent.max(0.0)));
            Some(Rect::new(x0 - pad, l.baseline - l.ascent - pad, x1 + pad, bottom + pad))
        })
        .reduce(|a, b| a.union(b))
}

/// An opaque nonzero fill with `paint`.
fn paint_fill(paint: Paint) -> Fill {
    Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }
}

/// Private-use characters DesignCraft stores for markers (page numbers, section markers, breaks).
fn is_marker(c: char) -> bool {
    ('\u{E000}'..='\u{E1FF}').contains(&c)
}

/// Can `b` join the run `a` is in? A missing glyph (.notdef) is a run of its own.
fn same_run(a: &PlacedGlyph, b: &PlacedGlyph) -> bool {
    b.visible
        && a.gid != 0
        && b.gid != 0
        && a.face.id() == b.face.id()
        && a.style == b.style
        && (a.sx - b.sx).abs() < 1e-9
        && (a.sy - b.sy).abs() < 1e-9
}

/// End of the run of `gs` that starts at `i`.
fn run_end(ft: &FrameText, gs: &[PlacedGlyph], i: usize) -> usize {
    let mut j = i + 1;
    while j < gs.len() && same_run(&gs[i], &gs[j]) && !(ft.vertical && (gs[j].upright || gs[j].tcy.is_some())) {
        j += 1;
    }
    j
}

/// Clusters of several glyphs (a base and its marks, as in pointed Hebrew) map glyph by glyph
/// when every glyph stands for one of the cluster's characters, and are drawn in text order:
/// viewers that only read ToUnicode then get each character once, marks after their base. A
/// glyph stands for a character when it is the font's glyph for it, or when both are marks (a
/// mark's glyph may be a positional variant). Other clusters (ligatures, contextual forms) keep
/// one shared range, which krilla writes as ActualText. Returns the order to draw the glyphs in.
fn split_clusters(glyphs: &[PlacedGlyph], text: &str, ranges: &mut [Range<usize>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..glyphs.len()).collect();
    let mut i = 0;
    while i < ranges.len() {
        let r = ranges[i].clone();
        let mut j = i + 1;
        while ranges.get(j).is_some_and(|n| *n == r) {
            j += 1;
        }
        if let (Some(cluster), Some(members)) = (text.get(r.clone()).filter(|_| j - i > 1), glyphs.get(i..j))
            && let Some(own) = cluster_chars(members, cluster)
        {
            for (m, (at, c)) in own.into_iter().enumerate() {
                if let Some(range) = ranges.get_mut(i + m) {
                    *range = r.start + at..r.start + at + c.len_utf8();
                }
            }
            if let Some(o) = order.get_mut(i..j) {
                o.sort_by_key(|&m| ranges.get(m).map_or(0, |r| r.start));
            }
        }
        i = j;
    }
    order
}

/// The character (byte offset in `cluster`, char) each glyph of a cluster stands for, if each
/// stands for a different one and all are covered.
fn cluster_chars(members: &[PlacedGlyph], cluster: &str) -> Option<Vec<(usize, char)>> {
    let chars: Vec<(usize, char)> = cluster.char_indices().collect();
    if chars.len() != members.len() {
        return None;
    }
    let mut own: Vec<Option<usize>> = vec![None; members.len()];
    let mut used = vec![false; chars.len()];
    for (m, g) in members.iter().enumerate() {
        if let Some(k) = (0..chars.len()).find(|&k| !used[k] && g.face.glyph_for(chars[k].1) == g.gid) {
            used[k] = true;
            own[m] = Some(k);
        }
    }
    // The remaining glyphs and characters must all be marks; they pair up in text order (a
    // right-to-left cluster lists its glyphs right to left).
    let mut rest: Vec<usize> = (0..members.len()).filter(|&m| own[m].is_none()).collect();
    if members.first().is_some_and(|g| g.rtl) {
        rest.reverse();
    }
    let marks: Vec<usize> = (0..chars.len()).filter(|&k| !used[k]).collect();
    if rest.len() != marks.len() {
        return None;
    }
    for (&m, &k) in rest.iter().zip(&marks) {
        let g = members.get(m)?;
        let c = chars.get(k)?.1;
        if !g.face.glyph_is_mark(g.gid) || !g.face.glyph_is_mark(g.face.glyph_for(c)) {
            return None;
        }
        own[m] = Some(k);
    }
    own.into_iter().map(|k| chars.get(k?).copied()).collect()
}

/// Merge bars of one paint layer; underlines and strikes must never be merged together.
fn text_rules(cs: &ComposedStory, ft: &FrameText, underline: bool) -> Vec<(designcraft_compose::Rule, Rect)> {
    let mut bars: Vec<(designcraft_compose::Rule, Rect)> = Vec::new();
    for line in &ft.lines {
        for g in line.glyphs.iter().filter(|g| g.spans_rules()) {
            let style = &cs.styles[g.style as usize];
            let (on, rule) = if underline { (style.underline, &style.underline_rule) } else { (style.strikethrough, &style.strike_rule) };
            if !on {
                continue;
            }
            let rect = rule.rect(g.x, g.x + g.adv, designcraft_compose::rule_baseline(style, line, g));
            match bars.last_mut() {
                Some((last_rule, last)) if last_rule == rule && (last.y0 - rect.y0).abs() < 1e-6 && (rect.x0 - last.x1).abs() < 0.5 => {
                    last.x1 = rect.x1;
                }
                _ => bars.push((rule.clone(), rect)),
            }
        }
    }
    bars
}

impl Exporter<'_> {
    pub(crate) fn font(&mut self, face: &FontFace) -> Option<Font> {
        self.fonts
            .entry(face.id())
            .or_insert_with(|| {
                // Keep the existing outline fallback and warning when embedding fails.
                let variations = face.is_variable().then_some(face.coords.as_slice());
                craft_pdf::write::font(face.data(), face.index(), variations)
                    .inspect_err(|error| log::warn!("PDF font `{} {}` could not be embedded: {error}", face.family, face.style))
                    .ok()
            })
            .clone()
    }

    fn reverse_cmap(&mut self, face: &FontFace) -> Arc<HashMap<u32, char>> {
        self.reverse_cmaps
            .entry(face.id())
            .or_insert_with(|| {
                let mut m = HashMap::new();
                // Sorted by code point: the lowest code point wins (`-` before U+2010).
                for (c, g) in face.chars() {
                    m.entry(g).or_insert(c);
                }
                Arc::new(m)
            })
            .clone()
    }

    /// Draw `f` as an artifact while a story is being tagged (nested text isn't tagged).
    fn as_artifact(&mut self, s: &mut Surface, f: impl FnOnce(&mut Self, &mut Surface)) {
        match self.tag_story.take() {
            Some(sid) => {
                s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Other, None)));
                f(self, s);
                s.end_tagged();
                self.tag_story = Some(sid);
            }
            None => f(self, s),
        }
    }

    /// A glyph run, tagged under its paragraph while a story is being tagged.
    fn tagged_run(&mut self, s: &mut Surface, cs: &ComposedStory, gs: &[PlacedGlyph], baseline: f64, story: &str) {
        match self.tag_story {
            Some(sid) => {
                let pi = self.doc.story(sid).map_or(0, |st| st.para_at(gs[0].byte.min(st.len())));
                let id = s.start_tagged(ContentTag::Span(SpanTag::empty()));
                self.run(s, cs, gs, baseline, story);
                s.end_tagged();
                self.para_tags.entry((sid, pi)).or_default().push(id);
            }
            None => self.run(s, cs, gs, baseline, story),
        }
    }

    pub(crate) fn frame_text(&mut self, s: &mut Surface, cs: &ComposedStory, ft: &FrameText, story: &str) {
        // Paragraph shading and rules under the text.
        self.as_artifact(s, |me, s| {
            for d in ft.decos.iter().filter(|d| !d.nonprinting) {
                if let Some(c) = me.swatch_color(&d.color, d.tint) {
                    match &d.path {
                        Some(p) => me.fill_path(s, p, c),
                        None => me.fill_rect(s, d.rect, c),
                    }
                }
            }
            if !ft.tables.is_empty() {
                me.tables(s, ft);
            }
        });
        // All underlines precede all glyph runs, including ink overlapping from another run.
        let underlines = text_rules(cs, ft, true);
        if !underlines.is_empty() {
            self.as_artifact(s, |me, s| {
                for (rule, r) in underlines {
                    if let Some(c) = me.swatch_color(&rule.color, rule.tint) {
                        me.fill_rect(s, r, c);
                    }
                }
            });
        }
        for l in &ft.lines {
            let gs = &l.glyphs;
            let mut i = 0;
            while i < gs.len() {
                let g = &gs[i];
                if !g.visible || g.sx <= 0.0 || g.sy <= 0.0 {
                    i += 1;
                    continue;
                }
                // Vertical type: an upright glyph turns back a quarter about its em box centre.
                // Shatai slants each glyph about its own em box centre first.
                let turn = if ft.vertical { g.vertical_xf(l.baseline) } else { None };
                let shatai = cs.styles.get(g.style as usize).and_then(|st| st.shatai_xf(g, l.baseline));
                if turn.is_some() || shatai.is_some() {
                    let m = turn.unwrap_or(Affine::IDENTITY) * shatai.unwrap_or(Affine::IDENTITY);
                    s.push_transform(&crate::export::tf(m));
                    self.tagged_run(s, cs, &gs[i..i + 1], l.baseline, story);
                    s.pop();
                    i += 1;
                    continue;
                }
                let j = run_end(ft, gs, i);
                self.tagged_run(s, cs, &gs[i..j], l.baseline, story);
                i = j;
            }
        }
        self.as_artifact(s, |me, s| {
            for (rule, r) in text_rules(cs, ft, false) {
                if let Some(c) = me.swatch_color(&rule.color, rule.tint) {
                    me.fill_rect(s, r, c);
                }
            }
            for n in &ft.notes {
                if let Some(nft) = n.text.frames.first() {
                    s.push_transform(&tf(Affine::translate(n.origin.to_vec2())));
                    let area = std::mem::replace(&mut me.text_area, lines_area(nft));
                    me.frame_text(s, &n.text, nft, &n.source);
                    me.text_area = area;
                    s.pop();
                }
            }
        });
    }

    /// One table cell's graphic, or its text composed in the cell's own area.
    fn table_cell(&mut self, s: &mut Surface, c: &designcraft_compose::PlacedCell) {
        if let Some(g) = &c.graphic {
            if let Some(clip) = to_path(&c.clip.to_path(0.1)) {
                s.push_clip_path(&clip, &krilla::paint::FillRule::NonZero);
                s.push_transform(&tf(Affine::translate((c.clip.x0, c.clip.y0))));
                self.graphic(s, g);
                s.pop();
                s.pop();
            }
        } else if let Some(cft) = c.text.frames.first() {
            s.push_transform(&tf(Affine::translate(c.origin.to_vec2())));
            let area = std::mem::replace(&mut self.text_area, lines_area(cft));
            self.frame_text(s, &c.text, cft, &c.source);
            self.text_area = area;
            s.pop();
        }
    }

    /// Table fragments: cell fills, cell text (real text), edges and the border.
    fn tables(&mut self, s: &mut Surface, ft: &FrameText) {
        for t in &ft.tables {
            for c in &t.cells {
                if let Some((sw, tint)) = &c.fill
                    && let Some(col) = self.swatch_color(sw, *tint)
                {
                    self.fill_rect(s, c.rect, col);
                }
            }
            for c in &t.cells {
                self.table_cell(s, c);
            }
            for seg in &t.strokes {
                let st = &seg.stroke;
                let Some(col) = self.swatch_color(&st.color, st.tint) else { continue };
                let kind = self.doc.stroke_kind(&st.kind);
                let mut bp = BezPath::new();
                bp.move_to(seg.a);
                bp.line_to(seg.b);
                let Some(p) = to_path(&bp) else { continue };
                // Butt-ended fills close the table's corners the way square caps do.
                let (ca, cb) = t.corner_capped(seg);
                let mut capped = BezPath::new();
                capped.move_to(ca);
                capped.line_to(cb);
                let Some(cp) = to_path(&capped) else { continue };
                // Gap colour under dashes, dots and stripes: the whole edge, solid.
                if !matches!(kind, designcraft_doc::StrokeType::Solid)
                    && st.gap_color != designcraft_color::swatch::NONE
                    && let Some(gap) = self.swatch_color(&st.gap_color, st.gap_tint)
                {
                    s.set_fill(None);
                    s.set_stroke(Some(Stroke {
                        paint: gap.into(),
                        width: st.weight as f32,
                        line_cap: krilla::paint::LineCap::Butt,
                        ..Default::default()
                    }));
                    s.draw_path(&cp);
                    s.set_stroke(None);
                }
                // Stripes, wavy and hash types are fills along the edge.
                if let Some(o) = kind.outline(&capped, st.weight, 0.05)
                    && let Some(op) = to_path(&o)
                {
                    s.set_stroke(None);
                    s.set_fill(Some(solid_fill(col, 1.0)));
                    s.draw_path(&op);
                    s.set_fill(None);
                    continue;
                }
                let (cap, dash) = match &kind {
                    designcraft_doc::StrokeType::Dashed { pattern }
                        if designcraft_doc::StrokeType::expandable_dashes(pattern, seg.a.distance(seg.b)) =>
                    {
                        let mut pat: Vec<f32> = pattern.iter().map(|v| v.max(0.0) as f32).collect();
                        if pat.len() % 2 == 1 {
                            pat.extend(pat.clone());
                        }
                        (krilla::paint::LineCap::Butt, Some(krilla::paint::StrokeDash { array: pat, offset: 0.0 }))
                    }
                    designcraft_doc::StrokeType::Dotted
                        if designcraft_doc::StrokeType::expandable_dashes(&[0.0, st.weight * 2.0], seg.a.distance(seg.b)) =>
                    {
                        (krilla::paint::LineCap::Round, Some(krilla::paint::StrokeDash { array: vec![0.0, (st.weight * 2.0) as f32], offset: 0.0 }))
                    }
                    _ => (krilla::paint::LineCap::Square, None),
                };
                s.set_fill(None);
                s.set_stroke(Some(Stroke { paint: col.into(), width: st.weight as f32, line_cap: cap, dash, ..Default::default() }));
                s.draw_path(&p);
                s.set_stroke(None);
            }
        }
    }

    fn fill_rect(&self, s: &mut Surface, r: Rect, c: krilla::color::Color) {
        self.fill_path(s, &r.to_path(0.1), c);
    }

    fn fill_path(&self, s: &mut Surface, bp: &designcraft_geom::BezPath, c: krilla::color::Color) {
        if let Some(p) = to_path(bp) {
            s.set_stroke(None);
            s.set_fill(Some(solid_fill(c, 1.0)));
            s.draw_path(&p);
            s.set_fill(None);
        }
    }

    /// Unicode text for a run's glyphs: (text, per-glyph byte range into it).
    fn run_text(&mut self, glyphs: &[PlacedGlyph], story: &str) -> (String, Vec<Range<usize>>) {
        let cmap = self.reverse_cmap(&glyphs[0].face);
        let mut text = String::new();
        let mut ranges: Vec<Range<usize>> = Vec::with_capacity(glyphs.len());
        // Byte of the previous glyph when it came straight from the story (cluster continuation).
        let mut prev_src: Option<usize> = None;
        for g in glyphs {
            let src = story.get(g.byte..g.byte + g.len).filter(|t| !t.is_empty() && !t.chars().any(is_marker));
            let range = if let Some(t) = src {
                prev_src = Some(g.byte);
                let a = text.len();
                text.push_str(t);
                a..text.len()
            } else if g.len == 0 && prev_src == Some(g.byte) && !ranges.is_empty() {
                // Second glyph of a multi-glyph cluster: share the cluster's text.
                ranges[ranges.len() - 1].clone()
            } else if let Some(t) = g.generated_text.as_deref().filter(|t| !t.is_empty() && !t.chars().any(is_marker)) {
                // Generated text (page numbers, list labels): its own characters, which a glyph
                // the font substituted (small-cap or old-style figures) can't be mapped back to.
                prev_src = None;
                let a = text.len();
                text.push_str(t);
                a..text.len()
            } else if g.generated_text.as_deref() == Some("") && !ranges.is_empty() {
                // Later glyph of a generated cluster.
                prev_src = None;
                ranges[ranges.len() - 1].clone()
            } else if g.len == 0 && cmap.get(&g.gid) == Some(&'\u{0640}') {
                // A kashida that justification inserted: drawn, but no text of its own.
                prev_src = None;
                text.len()..text.len()
            } else {
                // Other inserted glyphs (hyphens, tab leaders, ruby): map back through the font.
                prev_src = None;
                match cmap.get(&g.gid) {
                    Some(c) => {
                        let a = text.len();
                        text.push(*c);
                        a..text.len()
                    }
                    None => match ranges.last() {
                        Some(r) => r.clone(),
                        None => {
                            text.push('\u{FFFD}');
                            0..text.len()
                        }
                    },
                }
            };
            ranges.push(range);
        }
        (text, ranges)
    }

    fn run(&mut self, s: &mut Surface, cs: &ComposedStory, glyphs: &[PlacedGlyph], baseline: f64, story: &str) {
        let g0 = &glyphs[0];
        let st = &cs.styles[g0.style as usize];
        let face = g0.face;
        let size = g0.sy * face.upem;
        let hs = g0.sx / g0.sy;
        let tan = if st.skew != 0.0 { st.skew.to_radians().tan() } else { 0.0 };
        let skew = if st.skew != 0.0 { Affine::new([1.0, 0.0, -tan, 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
        let origin = Affine::translate((g0.x, baseline + g0.y)) * skew * Affine::scale_non_uniform(hs, 1.0);
        // Outside a frame (no text area), a gradient spans the run.
        let end = glyphs.last().map_or(g0.x, |g| g.x + g.adv);
        let run_area = Rect::new(g0.x, baseline - size, end.max(g0.x + 1.0), baseline + size * 0.25);
        let fill = self.text_paint(&st.fill, st.fill_tint, origin, run_area);
        let stroke = if st.stroke != designcraft_color::swatch::NONE && st.stroke_weight > 0.0 {
            self.text_paint(&st.stroke, st.stroke_tint, origin, run_area)
        } else {
            None
        };
        if fill.is_none() && stroke.is_none() {
            return;
        }
        if st.overprint_fill || st.overprint_stroke {
            self.warn("overprint is not exported yet");
        }
        // Stroke behind fill: a double-width stroke first, as outlines so the text is not
        // duplicated, then the fill over its inner half. With no fill, the stroke is clipped
        // to the outside of the glyphs instead.
        let behind = st.stroke_behind_fill && stroke.is_some();
        let weight = if behind { st.stroke_weight * 2.0 } else { st.stroke_weight };
        let mut clipped = false;
        let stroke = if behind {
            let outlines = glyph_outlines(glyphs, size, tan);
            if fill.is_some() {
                self.paint_outlines(s, &outlines, origin, None, stroke, weight);
                None
            } else {
                clipped = self.push_outside_clip(s, &outlines, origin, weight);
                stroke
            }
        } else {
            stroke
        };
        // A missing glyph is drawn as its box: PDF/A and PDF/UA forbid showing .notdef as text.
        if g0.gid == 0 {
            self.paint_outlines(s, &glyph_outlines(glyphs, size, tan), origin, fill, stroke, weight);
        } else if let Some(font) = self.font(&face) {
            self.text_run(s, glyphs, story, font, origin, size, hs, tan, fill, stroke, weight);
        } else {
            self.warn(format!("font `{} {}` could not be embedded; its text was drawn as outlines", face.family, face.style));
            self.paint_outlines(s, &glyph_outlines(glyphs, size, tan), origin, fill, stroke, weight);
        }
        if clipped {
            s.pop();
        }
    }

    /// A run as real text.
    #[allow(clippy::too_many_arguments)]
    fn text_run(
        &mut self,
        s: &mut Surface,
        glyphs: &[PlacedGlyph],
        story: &str,
        font: Font,
        origin: Affine,
        size: f64,
        hs: f64,
        tan: f64,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_weight: f64,
    ) {
        let Some(g0) = glyphs.first() else { return };
        let (text, mut ranges) = self.run_text(glyphs, story);
        let order = split_clusters(glyphs, &text, &mut ranges);
        let k = 1.0 / (hs * size);
        // Glyphs are placed absolutely: each advance reaches the next glyph drawn, a vertical offset
        // (a mark above or below its base) is undone by the skew so it lands where it was composed.
        let kg: Vec<KrillaGlyph> = order
            .iter()
            .enumerate()
            .filter_map(|(n, &i)| {
                let g = glyphs.get(i)?;
                let next = order.get(n + 1).and_then(|&m| glyphs.get(m)).map_or(g.x + g.adv, |m| m.x);
                let dy = g.y - g0.y;
                let r = ranges.get(i)?.clone();
                Some(KrillaGlyph::new(GlyphId::new(g.gid), ((next - g.x) * k) as f32, (tan * dy * k) as f32, (-dy / size) as f32, 0.0, r, None))
            })
            .collect();
        let start = order.first().and_then(|&i| glyphs.get(i)).map_or(0.0, |g| (g.x - g0.x) / hs);
        s.push_transform(&tf(origin));
        s.set_fill(fill.map(paint_fill));
        s.set_stroke(stroke.map(|paint| Stroke { paint, width: stroke_weight as f32, ..Default::default() }));
        s.draw_glyphs(Point::from_xy(start as f32, 0.0), &kg, font, &text, size as f32, false);
        s.set_fill(None);
        s.set_stroke(None);
        s.pop();
    }

    /// Glyph outlines as paths (faces krilla can't embed, missing glyphs, strokes behind fill).
    fn paint_outlines(
        &mut self,
        s: &mut Surface,
        outlines: &BezPath,
        origin: Affine,
        fill: Option<Paint>,
        stroke: Option<Paint>,
        stroke_weight: f64,
    ) {
        let Some(p) = to_path(outlines) else { return };
        s.push_transform(&tf(origin));
        s.set_fill(fill.map(paint_fill));
        s.set_stroke(stroke.map(|paint| Stroke { paint, width: stroke_weight as f32, ..Default::default() }));
        s.draw_path(&p);
        s.set_fill(None);
        s.set_stroke(None);
        s.pop();
    }

    /// Clip to everything outside the glyphs (a frame around them plus their outlines,
    /// even-odd). False when there was nothing to clip with.
    fn push_outside_clip(&mut self, s: &mut Surface, outlines: &BezPath, origin: Affine, weight: f64) -> bool {
        let m = weight * 2.0 + 10.0;
        let mut outside = outlines.bounding_box().inflate(m, m).to_path(0.1);
        outside.extend(outlines.iter());
        let Some(p) = to_path(&(origin * outside)) else { return false };
        s.push_clip_path(&p, &krilla::paint::FillRule::EvenOdd);
        true
    }

    /// One line of plain text in the bundled UI face (page information mark).
    pub(crate) fn plain_text(&mut self, s: &mut Surface, text: &str, at: (f64, f64), size: f64, color: krilla::color::Color) {
        let face = FontDb::global().face(designcraft_fonts::FALLBACK_FAMILY, "Regular");
        let Some(font) = self.font(&face) else { return };
        let shaped = designcraft_fonts::shape(&face, text, &[], |c| c);
        let upem = face.upem as f32;
        let glyphs: Vec<KrillaGlyph> = shaped
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let end = shaped.iter().skip(i + 1).map(|n| n.cluster).find(|c| *c != g.cluster).unwrap_or(text.len());
                let end = if end > g.cluster { end } else { text.len() };
                KrillaGlyph::new(
                    GlyphId::new(g.gid),
                    g.x_advance as f32 / upem,
                    g.x_offset as f32 / upem,
                    g.y_offset as f32 / upem,
                    0.0,
                    g.cluster..end,
                    None,
                )
            })
            .collect();
        s.set_stroke(None);
        s.set_fill(Some(solid_fill(color, 1.0)));
        s.draw_glyphs(Point::from_xy(at.0 as f32, at.1 as f32), &glyphs, font, text, size as f32, false);
        s.set_fill(None);
    }
}

/// A run's glyph outlines at `size`, relative to its first glyph's origin. A vertical offset (a
/// mark above or below its base) is undone by the skew `tan` so it lands where it was composed.
fn glyph_outlines(glyphs: &[PlacedGlyph], size: f64, tan: f64) -> BezPath {
    let db = FontDb::global();
    let mut bp = BezPath::new();
    let Some(g0) = glyphs.first() else { return bp };
    let k = size / g0.face.upem;
    for g in glyphs {
        let dy = g.y - g0.y;
        let a = Affine::translate(((g.x - g0.x + tan * dy) / (g0.sx / g0.sy), dy)) * Affine::scale(k);
        let mut o = (*db.outline(&g.face, g.gid)).clone();
        o.apply_affine(a);
        bp.extend(o.iter());
    }
    bp
}

#[cfg(test)]
mod rule_tests {
    use super::*;
    use designcraft_compose::Cache;
    use designcraft_doc::{Document, ParaFormat, SpreadRef};
    use designcraft_geom::Rect;

    /// A tab draws nothing but advances to its stop, and the underline runs across it, as it does
    /// on screen: a form's fill-in lines are underlined tabs (#1466).
    #[test]
    fn an_underline_runs_across_a_tab() {
        let mut doc = Document::new(&Default::default());
        for style in &mut doc.styles_mut().paragraph {
            style.chars.underline = Some(true);
        }
        let layer = doc.default_layer();
        let (_, sid) =
            doc.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 400.0, 300.0), layer, "Name\tCity", ParaFormat::default()).unwrap();
        let cs = Cache::new().get(&doc, sid, None);
        let line = &cs.frames[0].lines[0];
        let tab = line.glyphs.iter().find(|g| g.rendered_char == '\t').expect("the tab is placed on the line");
        let bars = text_rules(&cs, &cs.frames[0], true);
        assert_eq!(bars.len(), 1, "one bar across the line, not one before the tab and one after: {bars:?}");
        let bar = bars[0].1;
        assert!(bar.x0 <= tab.x + 0.5 && bar.x1 >= tab.x + tab.adv - 0.5, "the bar covers the tab: {bar:?}, tab {} + {}", tab.x, tab.adv);
        // The strikethrough of the same text behaves the same way.
        for style in &mut doc.styles_mut().paragraph {
            style.chars.underline = None;
            style.chars.strikethrough = Some(true);
        }
        let cs = Cache::new().get(&doc, sid, None);
        assert!(text_rules(&cs, &cs.frames[0], true).is_empty(), "no underline now");
        assert_eq!(text_rules(&cs, &cs.frames[0], false).len(), 1, "one strike across the line");
    }
}
