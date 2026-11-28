use super::common::{device, norm, q, tf, to_path};
use super::*;

impl Exporter<'_> {
    /// Fill `p` with an item's fill paint, overprinting when the fill does.
    pub(super) fn fill_item_path(&mut self, s: &mut Surface, fill: &designcraft_doc::Fill, paint: krilla::paint::Paint, p: &Path) {
        let op = self.overprints(&fill.swatch, fill.tint, fill.overprint);
        let pushed = self.push_overprint(s, op, false);
        s.set_stroke(None);
        s.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
        s.draw_path(p);
        s.set_fill(None);
        if pushed {
            s.pop();
        }
    }

    /// A swatch fill (solid or gradient) in the item's inner space. `None` for [None]/unknown.
    pub(crate) fn fill_paint(&mut self, fill: &designcraft_doc::Fill, bounds: Rect) -> Option<krilla::paint::Paint> {
        self.fill_paint_in(fill, bounds, Affine::IDENTITY)
    }

    /// [`Self::fill_paint`] for a gradient laid out in a space that `xf` maps to the current one
    /// (text runs: the frame's text space under the run's origin).
    pub(crate) fn fill_paint_in(&mut self, fill: &designcraft_doc::Fill, bounds: Rect, xf: Affine) -> Option<krilla::paint::Paint> {
        let (swatch, tint) = (fill.swatch.as_str(), fill.tint);
        if let Some(g) = designcraft_color::swatch::resolve_gradient(&self.doc.swatches, swatch) {
            let kind = g.kind;
            let overprint = self.overprints(swatch, tint, fill.overprint);
            let stops = match self.ink_gradient_stops(g, swatch, overprint) {
                Some(stops) => stops,
                None => self.device_gradient_stops(g, swatch),
            };
            if stops.is_empty() {
                return None;
            }
            return Some(match kind {
                GradientKind::Radial => {
                    // A circle stretched into the object's ellipse, as on screen.
                    let (c, rx, ry) = fill.radial_ellipse(bounds);
                    let rx = rx.max(1e-3);
                    let stretch = Affine::translate(c.to_vec2()) * Affine::scale_non_uniform(1.0, ry / rx) * Affine::translate(-c.to_vec2());
                    let (cx, cy) = (c.x as f32, c.y as f32);
                    RadialGradient {
                        fx: cx,
                        fy: cy,
                        fr: 0.0,
                        cx,
                        cy,
                        cr: rx as f32,
                        transform: tf(xf * stretch),
                        spread_method: SpreadMethod::Pad,
                        stops,
                        anti_alias: false,
                    }
                    .into()
                }
                _ => {
                    let (p0, p1) = fill.linear_axis(bounds);
                    LinearGradient {
                        x1: p0.x as f32,
                        y1: p0.y as f32,
                        x2: p1.x as f32,
                        y2: p1.y as f32,
                        transform: tf(xf),
                        spread_method: SpreadMethod::Pad,
                        stops,
                        anti_alias: false,
                    }
                    .into()
                }
            });
        }
        self.swatch_color(swatch, tint).map(Into::into)
    }

    /// A gradient's stops in device colors: CMYK, RGB or Gray, as one space.
    fn device_gradient_stops(&mut self, g: &designcraft_color::Gradient, swatch: &str) -> Vec<Stop> {
        let mut expanded = g.expanded_stops();
        // Gray and CMYK stops blend as inks, so a gray stop beside a CMYK one is the K ink.
        if !self.rgb_only && expanded.iter().any(|(_, c, _)| matches!(c, Color::Cmyk { .. })) {
            for (_, c, _) in &mut expanded {
                if let Color::Gray { k } = *c {
                    *c = Color::cmyk(0.0, 0.0, 0.0, k);
                }
            }
        }
        // A PDF shading needs one color space. Midpoint expansion can introduce RGB even
        // when every authored stop is CMYK/Gray; PDF/A can also change the output space.
        let components = |c: &Color| match c {
            Color::Cmyk { .. } if !self.rgb_only => 4,
            Color::Gray { .. } => 1,
            _ => 3,
        };
        let mixed = expanded.first().is_some_and(|(_, first, _)| expanded.iter().any(|(_, c, _)| components(c) != components(first)));
        if mixed {
            self.warn(format!("gradient '{swatch}': mixed color spaces converted to RGB (color appearance and separations may change)"));
        }
        let mut stops: Vec<Stop> = Vec::new();
        let mut last = 0.0f32;
        for (o, c, a) in expanded {
            let o = o.clamp(last, 1.0);
            last = o;
            let color = if mixed && !matches!(c, Color::Rgb { .. }) {
                let [r, g, b] = c.to_rgb();
                rgb::Color::new(q(r), q(g), q(b)).into()
            } else {
                // Preserve RGB values, including already-interpolated display RGB, and
                // leave every homogeneous gradient on its existing device-space path.
                device(&c, self.rgb_only)
            };
            stops.push(Stop { offset: norm(o), color, opacity: norm(a) });
        }
        stops
    }

    pub(super) fn stroke(&mut self, s: &mut Surface, it: &Item, bp: &BezPath, path: &Path) {
        let st = &it.stroke;
        // A gradient swatch runs across the path's box, as on a fill.
        let paint: krilla::paint::Paint = if designcraft_color::swatch::resolve_gradient(&self.doc.swatches, &st.swatch).is_some() {
            let Some(p) = self.fill_paint(&st.as_fill(), bp.bounding_box()) else { return };
            p
        } else {
            let Some(c) = self.swatch_color(&st.swatch, st.tint) else { return };
            c.into()
        };
        let closed = it.path.is_closed();
        // Overprint Stroke covers the stroke and its arrowheads, Overprint Gap the gap color;
        // each is pushed around what it paints (a filled outline or a stroked path).
        let stroke_op = self.overprints(&st.swatch, st.tint, st.overprint);
        let gap_op = self.overprints(&st.gap_swatch, st.gap_tint, st.gap_overprint);
        let mut cap = match st.cap {
            designcraft_doc::Cap::Butt => krilla::paint::LineCap::Butt,
            designcraft_doc::Cap::Round => krilla::paint::LineCap::Round,
            designcraft_doc::Cap::Projecting => krilla::paint::LineCap::Square,
        };
        let kind = self.doc.stroke_kind(&st.kind);
        // Stripes, wavy and hash strokes: a filled outline (gap colour under it).
        if let Some(o) = kind.outline(bp, st.weight, 0.05)
            && let Some(op) = to_path(&o)
        {
            s.set_stroke(None);
            if let Some(g) = self.swatch_color(&st.gap_swatch, st.gap_tint)
                && st.gap_swatch != designcraft_color::swatch::NONE
            {
                let pushed = self.push_overprint(s, gap_op, gap_op);
                s.set_stroke(Some(Stroke { paint: g.into(), width: st.weight as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
                s.set_fill(None);
                s.draw_path(path);
                s.set_stroke(None);
                if pushed {
                    s.pop();
                }
            }
            let pushed = self.push_overprint(s, stroke_op, stroke_op);
            s.set_fill(Some(Fill { paint: paint.clone(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            s.draw_path(&op);
            s.set_fill(None);
            if pushed {
                s.pop();
            }
            return;
        }
        // Viewers expand dashes too: a degenerate pattern draws solid, as on screen.
        let length = designcraft_doc::path_length_bound(bp);
        let dash = match &kind {
            StrokeType::Dashed { .. } => kind.dashes(st.weight).filter(|p| StrokeType::expandable_dashes(p, length)).map(|pattern| {
                let mut pat: Vec<f32> = pattern.iter().map(|v| v.max(0.0) as f32).collect();
                if pat.len() % 2 == 1 {
                    pat.extend(pat.clone());
                }
                StrokeDash { array: pat, offset: 0.0 }
            }),
            StrokeType::Dotted if StrokeType::expandable_dashes(&[0.0, st.weight * 2.0], length) => {
                cap = krilla::paint::LineCap::Round;
                Some(StrokeDash { array: vec![0.0, (st.weight * 2.0) as f32], offset: 0.0 })
            }
            _ => None,
        };
        let arrows = designcraft_doc::arrow::apply(bp, st, closed);
        let trimmed = arrows.as_ref().and_then(|a| to_path(&a.0));
        let path = trimmed.as_ref().unwrap_or(path);
        let aligned = closed && st.align != StrokeAlign::Center;
        let width = if aligned { st.weight * 2.0 } else { st.weight };
        let mut pushes = 0;
        match st.align {
            StrokeAlign::Inside if closed => {
                s.push_clip_path(path, &FillRule::NonZero);
                pushes += 1;
            }
            StrokeAlign::Outside if closed => {
                // Clip to everything outside the path: a big frame plus the path, even-odd.
                let b = bp.bounding_box().inflate(width * 2.0 + 10.0, width * 2.0 + 10.0);
                let mut outside = b.to_path(0.1);
                outside.extend(bp.iter());
                if let Some(p) = to_path(&outside) {
                    s.push_clip_path(&p, &FillRule::EvenOdd);
                    pushes += 1;
                }
            }
            _ => {}
        }
        s.set_fill(None);
        if dash.is_some()
            && pushes == 0
            && let Some(g) = self.swatch_color(&st.gap_swatch, st.gap_tint)
        {
            // Gap colour under dashes and dots.
            let pushed = self.push_overprint(s, gap_op, gap_op);
            s.set_stroke(Some(Stroke { paint: g.into(), width: width as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
            s.draw_path(path);
            if pushed {
                s.pop();
            }
        }
        let pushed = self.push_overprint(s, stroke_op, stroke_op);
        s.set_stroke(Some(Stroke {
            paint: paint.clone(),
            width: width as f32,
            miter_limit: st.miter_limit.max(1.0) as f32,
            line_cap: cap,
            line_join: match st.join {
                designcraft_doc::Join::Miter => krilla::paint::LineJoin::Miter,
                designcraft_doc::Join::Round => krilla::paint::LineJoin::Round,
                designcraft_doc::Join::Bevel => krilla::paint::LineJoin::Bevel,
            },
            opacity: NormalizedF32::ONE,
            dash,
        }));
        s.draw_path(path);
        s.set_stroke(None);
        if pushed {
            s.pop();
        }
        for _ in 0..pushes {
            s.pop();
        }
        let pushed = self.push_overprint(s, stroke_op, stroke_op);
        for h in arrows.iter().flat_map(|a| &a.1) {
            let Some(p) = to_path(&h.path) else { continue };
            match h.outline {
                Some(w) => {
                    s.set_stroke(Some(Stroke { paint: paint.clone(), width: w as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
                    s.draw_path(&p);
                    s.set_stroke(None);
                }
                None => {
                    s.set_fill(Some(krilla::paint::Fill { paint: paint.clone(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                    s.draw_path(&p);
                    s.set_fill(None);
                }
            }
        }
        if pushed {
            s.pop();
        }
    }
}
