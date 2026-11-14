use super::color::{HIDDEN_LAYERS, PLATE, color_of, overprints, plate_pixmap, screen_color, set_fill_paint};
use super::pdf_raster::{is_pdf, shown_box_now};
use super::prepare::Prepared;
use super::types::{DisplayQuality, Frame};
use super::{
    Affine, BezPath, BlendMode, Compose, Item, Mix, Rect, RenderContext, Renderer, Shape, StrokeAlign, StrokeType, colorize, images, kurbo,
    pdf_layers, peniko, vello_cpu,
};

/// What a placed graphic takes from the image cache.
pub(crate) enum Source {
    /// The pixels of these bytes.
    Want(images::Want),
    /// The bytes to draw aren't known yet: a PDF layer variant is being made, or the crop of a
    /// placed PDF measured, in the background. What is cached under `cached` (an earlier copy of
    /// the variant) is drawn meanwhile, else a light box.
    Waiting { cached: Option<designcraft_doc::SourceKey>, page: u32, width_pt: f64, on_screen: f64 },
}

/// What drawing placed graphic `g` takes from the image cache, the page rectangle it covers in
/// graphic space and that page's height (points). `None` for video and sound, Fast display and a
/// PDF whose hidden layers can't be applied (none of these draw image pixels). A frame that
/// doesn't wait for images reads no file and rewrites none here.
pub(crate) fn image_want(f: &Frame, asset: &designcraft_doc::Asset, g: &designcraft_doc::Graphic, xf: Affine) -> Option<(Source, Rect, f64)> {
    if designcraft_doc::media_kind(&asset.mime).is_some() || f.opts.quality == DisplayQuality::Fast {
        return None;
    }
    let wait = !f.opts.defer_images;
    // Pick a mip level close to the on-screen size (Typical: a 72 ppi proxy).
    let on_screen = (f.view * xf * g.xf).determinant().abs().sqrt();
    let on_screen = if f.opts.quality == DisplayQuality::Typical { on_screen.min(1.0) } else { on_screen };
    // A placed PDF cropped to a box: the whole page, placed so that the box spans graphic space.
    let shown = match shown_box_now(asset, wait) {
        Prepared::Ready(b) => b,
        Prepared::Failed => None,
        Prepared::Pending => {
            let waiting = Source::Waiting { cached: None, page: asset.page, width_pt: g.size.0, on_screen };
            return Some((waiting, Rect::new(0.0, 0.0, g.size.0, g.size.1), g.size.1));
        }
    };
    let [l, t, r, b] = shown.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let (page_w, page_h) = (g.size.0 / (r - l), g.size.1 / (b - t));
    let page = Rect::new(-l * page_w, -t * page_h, (1.0 - l) * page_w, (1.0 - t) * page_h);
    let hidden = HIDDEN_LAYERS.with(|h| h.borrow().clone());
    // Linked bytes not in memory aren't loaded to ask: their type says.
    let pdf = asset.data.resident().map_or(asset.mime == "application/pdf", |b| is_pdf(&b));
    let want = |bytes| Source::Want(images::Want { bytes, page: asset.page, width_pt: page_w, on_screen });
    let source = match hidden.as_deref().filter(|_| pdf) {
        // An explicit choice (including all-on) that cannot be rewritten fails closed:
        // stale persisted/relinked choices must not reveal hidden source content.
        Some(hidden) => {
            let covered = |id| images::cached(id, asset.page, page_w, on_screen).is_some_and(|(_, exact)| exact);
            match pdf_layers::layered(&asset.data, hidden, wait, covered) {
                pdf_layers::Layered::Ready(v) => want(v),
                pdf_layers::Layered::Making(cached) => Source::Waiting { cached, page: asset.page, width_pt: page_w, on_screen },
                pdf_layers::Layered::Failed => return None,
            }
        }
        None => want(asset.data.clone()),
    };
    Some((source, page, page_h))
}

/// What a placed graphic draws.
enum Drawn<'a> {
    /// Pixels decoded from these bytes (the graphic's, or its proxy's).
    Pixels(std::sync::Arc<vello_cpu::Pixmap>, &'a designcraft_doc::AssetData),
    /// Nothing decoded yet: a light box where it goes.
    Loading,
    Nothing,
}

/// The pixels to draw for `want`: its own, else (still loading or decoding, or its linked file
/// can't be had) those of `proxy`, the asset's preview, which spans the same page.
fn pixels<'a>(f: &Frame, want: &'a images::Want, proxy: Option<&'a designcraft_doc::AssetData>) -> Drawn<'a> {
    let (width_pt, on_screen) = (want.width_pt, want.on_screen);
    if !f.opts.defer_images {
        if let Some(pm) = images::mip(&want.bytes, want.page, width_pt, on_screen) {
            return Drawn::Pixels(pm, &want.bytes);
        }
        return proxy.and_then(|p| Some(Drawn::Pixels(images::mip(p, 0, width_pt, on_screen)?, p))).unwrap_or(Drawn::Nothing);
    }
    let loading = match images::mip_now(&want.bytes, want.page, width_pt, on_screen) {
        images::Mip::Ready(pm) | images::Mip::Pending(Some(pm)) => return Drawn::Pixels(pm, &want.bytes),
        images::Mip::Pending(None) => true,
        images::Mip::Failed => false,
    };
    match proxy.map(|p| (p, images::mip_now(p, 0, width_pt, on_screen))) {
        Some((p, images::Mip::Ready(pm) | images::Mip::Pending(Some(pm)))) => Drawn::Pixels(pm, p),
        Some((_, images::Mip::Pending(None))) => Drawn::Loading,
        _ if loading => Drawn::Loading,
        _ => Drawn::Nothing,
    }
}

/// `stroke` as round dots `weight` wide, one every `2 × weight`. Kurbo's stroker drops
/// zero-length dashes, so each dot is a dash a hair long, centred where the zero-length dash
/// would be (so the path's ends get dots too).
pub(crate) fn dotted(stroke: kurbo::Stroke, weight: f64) -> kurbo::Stroke {
    let dot = weight * 1e-3;
    stroke.with_dashes(dot / 2.0, [dot, weight * 2.0 - dot]).with_caps(kurbo::Cap::Round)
}

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
            StrokeType::Dotted if StrokeType::expandable_dashes(&[0.0, st.weight * 2.0], length) => stroke = dotted(stroke, st.weight),
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
        let Some((source, page, page_h)) = image_want(f, asset, g, xf) else { return };
        let loading = |ctx: &mut RenderContext| {
            ctx.set_transform(f.view * xf * g.xf);
            ctx.set_paint(screen_color([222, 222, 222], 255));
            ctx.fill_rect(&Rect::new(0.0, 0.0, g.size.0, g.size.1).intersect(page));
        };
        let want = match source {
            Source::Want(want) => want,
            Source::Waiting { cached, page: pdf_page, width_pt, on_screen } => {
                // A layer variant: a PDF, which takes no swatch.
                match cached.and_then(|id| images::cached(id, pdf_page, width_pt, on_screen)) {
                    Some((pm, _)) => {
                        let pm = match PLATE.with(std::cell::Cell::get) {
                            Some(plate) => plate_pixmap(&pm, plate),
                            None => pm,
                        };
                        paint_pixels(ctx, f, g, xf, page, width_pt, page_h, pm);
                    }
                    None => loading(ctx),
                }
                return;
            }
        };
        let page_w = want.width_pt;
        // A layer variant has no proxy: the asset's shows the file's default layers.
        let proxy = asset.proxy.as_ref().map(|p| &p.data).filter(|_| want.bytes.key() == asset.data.key());
        let (pm, data) = match pixels(f, &want, proxy) {
            Drawn::Pixels(pm, data) => (pm, data),
            Drawn::Loading => return loading(ctx),
            Drawn::Nothing => return,
        };
        // A coloured image is already in the plate's inks (its colours come from `color_of`).
        let coloured = match colorize::colorized(f.doc, &g.fill, data, &pm, !f.opts.defer_images) {
            Prepared::Ready(coloured) => coloured,
            Prepared::Pending | Prepared::Failed => return loading(ctx),
        };
        let pm = match (coloured, PLATE.with(std::cell::Cell::get)) {
            (Some(coloured), _) => coloured,
            (None, Some(plate)) => plate_pixmap(&pm, plate),
            (None, None) => pm,
        };
        paint_pixels(ctx, f, g, xf, page, page_w, page_h, pm);
    }
}

/// Draw `pm`, the pixels of a page `page_w` × `page_h` points at `page` in graphic `g`'s space.
#[allow(clippy::too_many_arguments)]
fn paint_pixels(
    ctx: &mut RenderContext,
    f: &Frame,
    g: &designcraft_doc::Graphic,
    xf: Affine,
    page: Rect,
    page_w: f64,
    page_h: f64,
    pm: std::sync::Arc<vello_cpu::Pixmap>,
) {
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
