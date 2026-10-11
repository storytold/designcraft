//! Composed text → PDF text objects with embedded (subsetted) fonts.
//!
//! Each line is split into runs of glyphs sharing face, style, scale and baseline offset. A run is
//! drawn with one `draw_glyphs` call: the run origin, horizontal scale and skew go into the
//! transform, glyph advances are taken from the composed x positions (so justification, tracking
//! and kerning are exact), and every glyph carries the story text it came from so viewers can
//! select, search and copy it.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use designcraft_compose::{ComposedStory, FrameText, PlacedGlyph};
use designcraft_fonts::{FontDb, FontFace};
use designcraft_geom::kurbo::Shape as _;
use designcraft_geom::{Affine, BezPath, Rect};
use krilla::geom::Point;
use krilla::paint::Stroke;
use krilla::surface::Surface;
use krilla::tagging::{Artifact, ArtifactType, ContentTag, SpanTag};
use krilla::text::{Font, GlyphId, KrillaGlyph};

use crate::export::{Exporter, solid_fill, tf, to_path};

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
        && (a.y - b.y).abs() < 1e-6
}

/// Merge bars of one paint layer; underlines and strikes must never be merged together.
fn text_rules(cs: &ComposedStory, ft: &FrameText, underline: bool) -> Vec<(designcraft_compose::Rule, Rect)> {
    let mut bars: Vec<(designcraft_compose::Rule, Rect)> = Vec::new();
    for line in &ft.lines {
        for g in line.glyphs.iter().filter(|g| g.visible) {
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
                if face.is_variable() {
                    // A named instance: krilla embeds the font at these axis settings.
                    let coords: Vec<(krilla::text::Tag, f32)> = face.coords.iter().map(|(t, v)| (krilla::text::Tag::new(t), *v)).collect();
                    Font::new_variable(face.data().to_vec().into(), face.index(), &coords)
                } else {
                    Font::new(face.data().to_vec().into(), face.index())
                }
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
            for d in &ft.decos {
                if let Some(c) = me.swatch_color(&d.color, d.tint) {
                    me.fill_rect(s, d.rect, c);
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
                if ft.vertical
                    && let Some(turn) = g.vertical_xf(l.baseline)
                {
                    s.push_transform(&crate::export::tf(turn));
                    self.tagged_run(s, cs, &gs[i..i + 1], l.baseline, story);
                    s.pop();
                    i += 1;
                    continue;
                }
                let mut j = i + 1;
                while j < gs.len() && same_run(g, &gs[j]) && !(ft.vertical && (gs[j].upright || gs[j].tcy.is_some())) {
                    j += 1;
                }
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
                    me.frame_text(s, &n.text, nft, &n.source);
                    s.pop();
                }
            }
        });
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
                    self.frame_text(s, &c.text, cft, &c.source);
                    s.pop();
                }
            }
            for seg in &t.strokes {
                let st = &seg.stroke;
                let Some(col) = self.swatch_color(&st.color, st.tint) else { continue };
                let (cap, dash) = match &st.kind {
                    designcraft_doc::StrokeType::Dashed { pattern } if pattern.iter().any(|v| *v > 0.0) => {
                        let mut pat: Vec<f32> = pattern.iter().map(|v| v.max(0.0) as f32).collect();
                        if pat.len() % 2 == 1 {
                            pat.extend(pat.clone());
                        }
                        (krilla::paint::LineCap::Butt, Some(krilla::paint::StrokeDash { array: pat, offset: 0.0 }))
                    }
                    designcraft_doc::StrokeType::Dotted => {
                        (krilla::paint::LineCap::Round, Some(krilla::paint::StrokeDash { array: vec![0.0, (st.weight * 2.0) as f32], offset: 0.0 }))
                    }
                    _ => (krilla::paint::LineCap::Square, None),
                };
                let mut bp = BezPath::new();
                bp.move_to(seg.a);
                bp.line_to(seg.b);
                let Some(p) = to_path(&bp) else { continue };
                s.set_fill(None);
                s.set_stroke(Some(Stroke { paint: col.into(), width: st.weight as f32, line_cap: cap, dash, ..Default::default() }));
                s.draw_path(&p);
                s.set_stroke(None);
            }
        }
    }

    fn fill_rect(&self, s: &mut Surface, r: Rect, c: krilla::color::Color) {
        if let Some(p) = to_path(&r.to_path(0.1)) {
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
        let fill = self.swatch_color(&st.fill, st.fill_tint);
        let stroke =
            if st.stroke != designcraft_color::swatch::NONE && st.stroke_weight > 0.0 { self.swatch_color(&st.stroke, st.stroke_tint) } else { None };
        if fill.is_none() && stroke.is_none() {
            return;
        }
        let face = g0.face;
        let size = g0.sy * face.upem;
        let hs = g0.sx / g0.sy;
        let skew = if st.skew != 0.0 { Affine::new([1.0, 0.0, -st.skew.to_radians().tan(), 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
        let origin = Affine::translate((g0.x, baseline + g0.y)) * skew * Affine::scale_non_uniform(hs, 1.0);
        // A missing glyph is drawn as its box: PDF/A and PDF/UA forbid showing .notdef as text.
        if g0.gid == 0 {
            self.outline_run(s, glyphs, origin, size, fill, stroke, st.stroke_weight);
            return;
        }
        let Some(font) = self.font(&face) else {
            self.warn(format!("font `{} {}` could not be embedded; its text was drawn as outlines", face.family, face.style));
            self.outline_run(s, glyphs, origin, size, fill, stroke, st.stroke_weight);
            return;
        };
        let (text, ranges) = self.run_text(glyphs, story);
        let k = 1.0 / (hs * size);
        let kg: Vec<KrillaGlyph> = glyphs
            .iter()
            .zip(ranges)
            .enumerate()
            .map(|(i, (g, r))| {
                let next = glyphs.get(i + 1).map_or(g.x + g.adv, |n| n.x);
                KrillaGlyph::new(GlyphId::new(g.gid), ((next - g.x) * k) as f32, 0.0, 0.0, 0.0, r, None)
            })
            .collect();
        s.push_transform(&tf(origin));
        s.set_fill(fill.map(|c| solid_fill(c, 1.0)));
        s.set_stroke(stroke.map(|c| Stroke { paint: c.into(), width: st.stroke_weight as f32, ..Default::default() }));
        s.draw_glyphs(Point::from_xy(0.0, 0.0), &kg, font, &text, size as f32, false);
        s.set_fill(None);
        s.set_stroke(None);
        s.pop();
    }

    /// Fallback for faces krilla can't embed: glyph outlines as paths.
    #[allow(clippy::too_many_arguments)]
    fn outline_run(
        &mut self,
        s: &mut Surface,
        glyphs: &[PlacedGlyph],
        origin: Affine,
        size: f64,
        fill: Option<krilla::color::Color>,
        stroke: Option<krilla::color::Color>,
        stroke_weight: f64,
    ) {
        let db = FontDb::global();
        let g0 = &glyphs[0];
        let k = size / g0.face.upem;
        let mut bp = BezPath::new();
        for g in glyphs {
            let a = Affine::translate(((g.x - g0.x) / (g0.sx / g0.sy), 0.0)) * Affine::scale(k);
            let mut o = (*db.outline(&g.face, g.gid)).clone();
            o.apply_affine(a);
            bp.extend(o.iter());
        }
        let Some(p) = to_path(&bp) else { return };
        s.push_transform(&tf(origin));
        s.set_fill(fill.map(|c| solid_fill(c, 1.0)));
        s.set_stroke(stroke.map(|c| Stroke { paint: c.into(), width: stroke_weight as f32, ..Default::default() }));
        s.draw_path(&p);
        s.set_fill(None);
        s.set_stroke(None);
        s.pop();
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
