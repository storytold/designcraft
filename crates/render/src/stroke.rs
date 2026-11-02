use super::color::{HIDDEN_LAYERS, PLATE, color_of, overprints, plate_pixmap, screen_color, set_fill_paint};
use super::pdf_raster::{is_pdf, shown_box};
use super::types::{DisplayQuality, Frame};
use super::{
    Affine, BezPath, BlendMode, Compose, Item, Mix, Rect, RenderContext, Renderer, Shape, StrokeAlign, StrokeType, colorize, images, kurbo,
    pdf_layers, peniko, vello_cpu,
};

impl Renderer {
    pub(crate) fn draw_stroke(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine) {
        self.draw_stroke_painted(ctx, f, it, bp, xf);
        ctx.reset_paint_transform();
    }

    /// The stroke, in its colour or gradient (a gradient runs across the path's box, as a fill's).
    fn draw_stroke_painted(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine) {
        let st = &it.stroke;
        let (paint, bounds) = (st.as_fill(), bp.bounding_box());
        if f.doc.resolve_color(&st.swatch, st.tint).is_none() && designcraft_color::swatch::resolve_gradient(&f.doc.swatches, &st.swatch).is_none() {
            return;
        }
        let mut stroke = kurbo::Stroke::new(st.weight)
            .with_caps(match st.cap {
                designcraft_doc::Cap::Butt => kurbo::Cap::Butt,
                designcraft_doc::Cap::Round => kurbo::Cap::Round,
                designcraft_doc::Cap::Projecting => kurbo::Cap::Square,
            })
            .with_join(match st.join {
                designcraft_doc::Join::Miter => kurbo::Join::Miter,
                designcraft_doc::Join::Round => kurbo::Join::Round,
                designcraft_doc::Join::Bevel => kurbo::Join::Bevel,
            })
            .with_miter_limit(st.miter_limit);
        let kind = f.doc.stroke_kind(&st.kind);
        let length = designcraft_doc::path_length_bound(bp);
        match &kind {
            StrokeType::Dashed { pattern } if StrokeType::expandable_dashes(pattern, length) => {
                stroke = stroke.with_dashes(0.0, pattern.iter().copied())
            }
            StrokeType::Dotted if StrokeType::expandable_dashes(&[0.0, st.weight * 2.0], length) => {
                stroke = stroke.with_dashes(0.0, [0.0, st.weight * 2.0]).with_caps(kurbo::Cap::Round)
            }
            _ => {}
        }
        ctx.set_transform(f.view * xf);
        if !set_fill_paint(ctx, f.doc, &paint, bounds) {
            return;
        }
        let closed = it.path.is_closed();
        // Arrowheads: the path is shortened under them; they're drawn after it.
        let arrows = designcraft_doc::arrow::apply(bp, st, closed);
        let bp = arrows.as_ref().map_or(bp, |a| &a.0);
        // Gap colour under dashes and dots: the whole path, undashed.
        let gap = if matches!(kind, StrokeType::Solid) { None } else { f.doc.resolve_color(&st.gap_swatch, st.gap_tint) };
        if let Some(g) = gap
            && !matches!(st.align, StrokeAlign::Inside | StrokeAlign::Outside if closed)
        {
            let op = overprints(f, &st.gap_swatch, st.gap_tint, st.gap_overprint);
            if op {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(BlendMode::new(Mix::Multiply, Compose::SrcOver)), None, None, None);
                ctx.set_transform(f.view * xf);
            }
            ctx.set_paint(color_of(&g, 1.0));
            ctx.set_stroke(kurbo::Stroke {
                dash_pattern: Default::default(),
                start_cap: kurbo::Cap::Butt,
                end_cap: kurbo::Cap::Butt,
                ..stroke.clone()
            });
            ctx.stroke_path(bp);
            if op {
                ctx.pop_layer();
            }
            set_fill_paint(ctx, f.doc, &paint, bounds);
        }
        // Stripes, wavy and hash strokes are fills (along the path's centre line).
        if let Some(o) = kind.outline(bp, st.weight, 0.05 * f.px) {
            ctx.fill_path(&o);
            for h in arrows.iter().flat_map(|a| &a.1) {
                match h.outline {
                    Some(w) => {
                        ctx.set_stroke(kurbo::Stroke::new(w).with_join(kurbo::Join::Miter));
                        ctx.stroke_path(&h.path);
                    }
                    None => ctx.fill_path(&h.path),
                }
            }
            return;
        }
        match st.align {
            StrokeAlign::Inside if closed => {
                // Clip a double-width stroke to the path.
                ctx.push_clip_layer(bp);
                ctx.set_stroke(kurbo::Stroke { width: st.weight * 2.0, ..stroke });
                ctx.stroke_path(bp);
                ctx.pop_layer();
            }
            StrokeAlign::Outside if closed => {
                // Subtract the filled path from a double-width stroke. Combining a stroked
                // outline (already hollow) with the path using even-odd would paint its centre.
                // Isolate the stroke so the subtraction leaves the item's fill/content and
                // backdrop intact; the same nonzero fill rule also preserves compound holes.
                ctx.push_layer(None, None, None, None, None);
                ctx.set_stroke(kurbo::Stroke { width: st.weight * 2.0, ..stroke });
                ctx.stroke_path(bp);
                ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                ctx.fill_path(bp);
                ctx.pop_layer();
                ctx.pop_layer();
            }
            _ => {
                ctx.set_stroke(stroke);
                ctx.stroke_path(bp);
            }
        }
        for h in arrows.iter().flat_map(|a| &a.1) {
            match h.outline {
                Some(w) => {
                    ctx.set_stroke(kurbo::Stroke::new(w).with_join(kurbo::Join::Miter));
                    ctx.stroke_path(&h.path);
                }
                None => ctx.fill_path(&h.path),
            }
        }
    }

    /// Draw a placed graphic, inside its clipping path when it has one.
    pub(crate) fn draw_graphic(&mut self, ctx: &mut RenderContext, f: &Frame, g: &designcraft_doc::Graphic, xf: Affine) {
        let Some((region, even_odd)) = g.clip_region() else {
            self.draw_graphic_unclipped(ctx, f, g, xf);
            return;
        };
        ctx.set_transform(f.view * xf);
        if even_odd {
            ctx.set_fill_rule(peniko::Fill::EvenOdd);
        }
        ctx.push_clip_layer(&region);
        ctx.set_fill_rule(peniko::Fill::NonZero);
        self.draw_graphic_unclipped(ctx, f, g, xf);
        ctx.pop_layer();
    }

    fn draw_graphic_unclipped(&mut self, ctx: &mut RenderContext, f: &Frame, g: &designcraft_doc::Graphic, xf: Affine) {
        let Some(asset) = f.doc.assets.get(&g.asset) else { return };
        if designcraft_doc::media_kind(&asset.mime).is_some() {
            // Video / sound: a dark frame with a play mark (the poster, when set, is drawn by the item).
            ctx.set_transform(f.view * xf * g.xf);
            ctx.set_paint(screen_color([48, 48, 52], 255));
            ctx.fill_rect(&Rect::new(0.0, 0.0, g.size.0, g.size.1));
            let (cx, cy, r) = (g.size.0 / 2.0, g.size.1 / 2.0, g.size.0.min(g.size.1) * 0.18);
            let mut tri = BezPath::new();
            tri.move_to((cx - r * 0.6, cy - r));
            tri.line_to((cx + r, cy));
            tri.line_to((cx - r * 0.6, cy + r));
            tri.close_path();
            ctx.set_paint(screen_color([235, 235, 235], 255));
            ctx.fill_path(&tri);
            return;
        }
        if f.opts.quality == DisplayQuality::Fast {
            ctx.set_transform(f.view * xf * g.xf);
            ctx.set_paint(screen_color([178, 178, 178], 255));
            ctx.fill_rect(&Rect::new(0.0, 0.0, g.size.0, g.size.1));
            return;
        }
        // Pick a mip level close to the on-screen size (Typical: a 72 ppi proxy).
        let on_screen = (f.view * xf * g.xf).determinant().abs().sqrt();
        let on_screen = if f.opts.quality == DisplayQuality::Typical { on_screen.min(1.0) } else { on_screen };
        let hidden = HIDDEN_LAYERS.with(|h| h.borrow().clone());
        let data = match hidden.as_deref().filter(|_| is_pdf(&asset.data)) {
            // An explicit choice (including all-on) that cannot be rewritten fails closed:
            // stale persisted/relinked choices must not reveal hidden source content.
            Some(hidden) => {
                let Some(variant) = pdf_layers::layered(&asset.data, hidden) else { return };
                variant
            }
            None => asset.data.clone(),
        };
        // A placed PDF cropped to a box: the whole page, placed so that the box spans graphic space.
        let [l, t, r, b] = shown_box(asset).unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let (page_w, page_h) = (g.size.0 / (r - l), g.size.1 / (b - t));
        let page = Rect::new(-l * page_w, -t * page_h, (1.0 - l) * page_w, (1.0 - t) * page_h);
        let Some(pm) = images::mip(&data, asset.page, page_w, on_screen) else { return };
        // A coloured image is already in the plate's inks (its colours come from `color_of`).
        let pm = match (colorize::colorized(f.doc, &g.fill, &asset.data, &pm), PLATE.with(std::cell::Cell::get)) {
            (Some(coloured), _) => coloured,
            (None, Some(plate)) => plate_pixmap(&pm, plate),
            (None, None) => pm,
        };
        // Only where the page is: a box reaching past it (a media box larger than the crop box)
        // shows nothing there.
        let rect = Rect::new(0.0, 0.0, g.size.0, g.size.1).intersect(page);
        ctx.set_transform(f.view * xf * g.xf);
        let sx = page_w / pm.width().max(1) as f64;
        let sy = page_h / pm.height().max(1) as f64;
        ctx.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(pm),
            sampler: peniko::ImageSampler::default().with_quality(peniko::ImageQuality::Medium),
        });
        ctx.set_paint_transform(Affine::translate((page.x0, page.y0)) * Affine::scale_non_uniform(sx, sy));
        ctx.fill_rect(&rect);
        ctx.reset_paint_transform();
    }
}
