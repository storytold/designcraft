//! Soft object effects (Effects panel): drop shadow, outer glow, inner shadow and basic feather,
//! drawn with vello filter layers.
//!
//! vello_cpu only supports filter layers in single-threaded contexts, so on the multithreaded
//! pipeline each effect is recorded into an offscreen single-threaded context cropped to its
//! reach and placed as an image ([`Renderer::with_filters`]). The images are drawn together on
//! the cores before the canvas is rasterized ([`Renderer::raster_fx`]) and kept between renders
//! ([`crate::fx_cache`]); single-threaded contexts draw directly. A very wide blur goes offscreen in both, at a lower resolution; the object
//! itself and the clip to its shape stay at full resolution.
//!
//! - Drop shadow / outer glow: the object's silhouette (everything it paints: fill, image, text,
//!   stroke) in a `DropShadowOnly` layer under the object. `size` sets the blur (σ = size / 2),
//!   `spread`/`choke` (percent of size) grow the silhouette instead of blurring it. The shadow's
//!   opacity rides on the filter colour's alpha.
//! - Inner shadow: the inverse of the offset shape, blurred, clipped to the shape, on top.
//! - Inner glow, bevel and emboss, satin: built like the inner shadow (an offset inverse of the
//!   shape, blurred and clipped) with screen / multiply blending; satin XORs two offset copies.
//! - Feather: the object is masked by its shape inset by half the width and blurred, so the
//!   edge fades from ~0 at the path to full strength `width` inside.

use designcraft_doc::Item;
use designcraft_geom::{Affine, BezPath, Rect, Shape, Vec2};
use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::peniko::{self, BlendMode, Compose, Mix};
use vello_cpu::{RenderContext, kurbo};

use crate::fx_cache::Slot;
use crate::{Frame, RenderOptions, Renderer, color_of};
use designcraft_compose::ShownOn;

/// Shadow offset (spread units) for an InDesign angle (degrees, light direction) and distance.
pub(crate) fn offset(angle: f64, distance: f64) -> Vec2 {
    let a = angle.to_radians();
    Vec2::new(-a.cos() * distance, a.sin() * distance)
}

/// How far (points) the effects of `it` reach outside its geometry.
pub(crate) fn outset(it: &Item) -> f64 {
    let e = &it.effects;
    let mut o: f64 = 0.0;
    if e.drop_shadow.on {
        o = o.max(e.drop_shadow.distance.abs() + e.drop_shadow.size.abs() * 1.6 + 1.0);
    }
    if e.outer_glow.on {
        o = o.max(e.outer_glow.size.abs() * 1.6 + 1.0);
    }
    o
}

/// (blur σ, growth) in points for an effect `size` with `pct` percent hardened.
fn split(size: f64, pct: f64) -> (f64, f64) {
    let size = size.max(0.0);
    let p = (pct / 100.0).clamp(0.0, 1.0);
    (size * (1.0 - p) / 2.0, size * p)
}

fn blur(sigma: f64) -> Filter {
    Filter::from_primitive(FilterPrimitive::GaussianBlur { std_deviation: sigma.max(0.0) as f32, edge_mode: EdgeMode::None })
}

fn shadow(sigma: f64, color: peniko::Color) -> Filter {
    Filter::from_primitive(FilterPrimitive::DropShadowOnly {
        dx: 0.0,
        dy: 0.0,
        std_deviation: sigma.max(0.0) as f32,
        color,
        edge_mode: EdgeMode::None,
    })
}

/// Keep what was drawn in the current layer as much as `it`'s gradient feather says. Cover the
/// whole viewport: strokes, text and child effects can extend arbitrarily past the geometry,
/// and past the gradient's ends its end opacities carry on.
pub(crate) fn gradient_feather_mask(ctx: &mut RenderContext, f: &Frame, it: &Item, xf: Affine) {
    let gf = &it.effects.gradient_feather;
    let (p0, p1) = gf.points(it.feather_bounds());
    let stop = |o: f32, a: f32| peniko::ColorStop::from((o, peniko::Color::from_rgba8(0, 0, 0, (a.clamp(0.0, 1.0) * 255.0).round() as u8)));
    let stops = [stop(0.0, gf.start), stop(1.0, gf.end)];
    let grad = if gf.radial {
        peniko::Gradient::new_radial(p0, (p1 - p0).hypot().max(1e-3) as f32).with_stops(stops.as_slice())
    } else {
        peniko::Gradient::new_linear(p0, p1).with_stops(stops.as_slice())
    };
    ctx.set_transform(Affine::IDENTITY);
    ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestIn)), None, None, None);
    ctx.set_paint(grad);
    ctx.set_paint_transform(f.view * xf);
    ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(ctx.width()), f64::from(ctx.height())));
    ctx.reset_paint_transform();
    ctx.pop_layer();
}

impl Renderer {
    /// Draw an item with soft effects: below-effects, the (feathered) body, inner shadow.
    pub(crate) fn draw_item_fx(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine, page: ShownOn) {
        let doc = f.doc;
        let e = &it.effects;
        let sw = if it.stroke.is_none() { 0.0 } else { it.stroke.extent() };
        // Spread-space reach of what the object paints.
        let reach = xf.transform_rect_bbox(bp.bounding_box()).inflate(sw + 1.0, sw + 1.0);
        // The silhouette is the shape when the object paints its area (fill / image); text frames
        // without a fill cast the shadow of their text only, which can't be grown.
        let solid = !it.fill.is_none() || matches!(it.content, designcraft_doc::Content::Graphic(_)) || it.has_nested_items();
        let mut below = vec![];
        if e.outer_glow.on {
            let g = &e.outer_glow;
            below.push((Slot::OuterGlow, Vec2::ZERO, g.size, g.spread, g.color.clone(), g.opacity));
        }
        if e.drop_shadow.on {
            let d = &e.drop_shadow;
            below.push((
                Slot::DropShadow,
                offset(doc.light_angle(d.angle, d.global_light), d.distance),
                d.size,
                d.spread,
                d.color.clone(),
                d.opacity,
            ));
        }
        // A text frame's text casts them, not its frame: they need not cover the frame.
        let cast = if solid || below.is_empty() { reach } else { self.text_ink(f, it, xf, page).unwrap_or(reach) };
        for (slot, off, size, pct, color, opacity) in below {
            let Some(c) = doc.resolve_color(&color, 1.0) else { continue };
            let (sigma, grow) = split(size, pct);
            let filter = shadow(sigma, color_of(&c, opacity.clamp(0.0, 1.0)));
            // The offset moves the geometry, not the filter: vello drops layer content that lies
            // entirely outside the viewport before filtering.
            let r = (cast + off).inflate(grow, grow);
            let tint = color_of(&c, opacity.clamp(0.0, 1.0));
            self.with_filters(ctx, f, Some((it, xf, page, slot)), r, sigma, Some(tint), None, |me, c, fr, filtered| {
                let moved = Frame { view: fr.view * Affine::translate(off), ..*fr };
                c.set_transform(moved.view);
                if filtered {
                    c.push_layer(None, None, None, None, Some(filter.clone()));
                }
                me.draw_body(c, &moved, it, bp, xf, page);
                if grow > 0.0 && solid {
                    c.set_transform(moved.view * xf);
                    c.set_paint(peniko::Color::BLACK);
                    c.set_stroke(kurbo::Stroke::new(grow * 2.0).with_join(kurbo::Join::Round));
                    c.stroke_path(bp);
                }
                if filtered {
                    c.pop_layer();
                }
            });
        }
        // Gradient feather: the object (feathered or not) drawn in its own layer, then kept as
        // much as the opacity gradient says.
        let gf = &e.gradient_feather;
        let directional = e.directional_feather.on && e.directional_feather.widths.iter().any(|w| *w > 0.0);
        if gf.on || directional {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, None, None, None);
        }
        // The object, feathered or not. Choke keeps that much of the width opaque (a shorter
        // fade); Sharp and Rounded corners follow the inset shape more tightly than Diffused.
        if e.feather > 0.0 && it.path.is_closed() {
            let fo = &e.feather_options;
            let w = e.feather * (1.0 - fo.choke.clamp(0.0, 100.0) / 100.0).max(0.01);
            let (sigma, join) = match fo.corners {
                designcraft_doc::FeatherCorners::Diffused => (w / 4.0, kurbo::Join::Round),
                designcraft_doc::FeatherCorners::Rounded => (w / 3.0, kurbo::Join::Round),
                designcraft_doc::FeatherCorners::Sharp => (w / 6.0, kurbo::Join::Miter),
            };
            // The body and its clip stay at device resolution; only the blurred mask may be drawn
            // offscreen at a lower one.
            ctx.set_transform(f.view * xf);
            ctx.push_clip_layer(bp);
            self.draw_body(ctx, f, it, bp, xf, page);
            // Keep the body where the blurred inset silhouette is.
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestIn)), None, None, None);
            self.with_filters(ctx, f, Some((it, xf, page, Slot::Feather)), reach, sigma, None, None, |_, c, fr, _| {
                c.set_transform(fr.view);
                c.push_layer(None, None, None, None, Some(blur(sigma)));
                c.set_transform(fr.view * xf);
                c.set_paint(peniko::Color::BLACK);
                c.fill_path(bp);
                c.set_transform(Affine::IDENTITY);
                c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                c.set_transform(fr.view * xf);
                c.set_stroke(kurbo::Stroke::new(w).with_join(join));
                c.stroke_path(bp);
                c.pop_layer();
                c.pop_layer();
            });
            ctx.pop_layer();
            ctx.pop_layer();
        } else {
            self.draw_body(ctx, f, it, bp, xf, page);
        }
        // Directional feather: one fading ramp per side, multiplied in.
        let df = &e.directional_feather;
        if df.on && df.widths.iter().any(|w| *w > 0.0) {
            let b = it.inner_bounds();
            let area = if xf.determinant().abs() > 1e-12 { xf.inverse().transform_rect_bbox(reach) } else { b };
            let ramps = [
                (df.widths[0], kurbo::Point::new(b.x0, b.y0), kurbo::Point::new(b.x0, b.y0 + df.widths[0])),
                (df.widths[1], kurbo::Point::new(b.x0, b.y0), kurbo::Point::new(b.x0 + df.widths[1], b.y0)),
                (df.widths[2], kurbo::Point::new(b.x0, b.y1), kurbo::Point::new(b.x0, b.y1 - df.widths[2])),
                (df.widths[3], kurbo::Point::new(b.x1, b.y0), kurbo::Point::new(b.x1 - df.widths[3], b.y0)),
            ];
            for (w, p0, p1) in ramps {
                if w <= 0.0 {
                    continue;
                }
                let stops = [
                    peniko::ColorStop::from((0.0, peniko::Color::from_rgba8(0, 0, 0, 0))),
                    peniko::ColorStop::from((1.0, peniko::Color::from_rgba8(0, 0, 0, 255))),
                ];
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestIn)), None, None, None);
                ctx.set_transform(f.view * xf);
                ctx.set_paint(peniko::Gradient::new_linear(p0, p1).with_stops(stops.as_slice()));
                ctx.fill_rect(&area.inflate(4.0, 4.0));
                ctx.reset_paint_transform();
                ctx.pop_layer();
            }
        }
        if gf.on {
            gradient_feather_mask(ctx, f, it, xf);
        }
        if gf.on || directional {
            ctx.pop_layer();
        }
        if !it.path.is_closed() {
            return;
        }
        // Inner shadow, clipped to the shape.
        if e.inner_shadow.on
            && let Some(c) = doc.resolve_color(&e.inner_shadow.color, 1.0)
        {
            let s = &e.inner_shadow;
            let (sigma, choke) = split(s.size, s.choke);
            let off = offset(doc.light_angle(s.angle, s.global_light), s.distance);
            self.inner_edge(
                ctx,
                f,
                (it, xf, page, Slot::InnerShadow),
                reach,
                bp,
                off,
                sigma,
                choke,
                color_of(&c, s.opacity.clamp(0.0, 1.0)),
                Mix::Normal,
            );
        }
        // Inner glow: from the edges inwards (or out from the centre), screened over the object.
        if e.inner_glow.on
            && let Some(c) = doc.resolve_color(&e.inner_glow.color, 1.0)
        {
            let g = &e.inner_glow;
            let (sigma, choke) = split(g.size, g.choke);
            // Precise keeps the glow close to the edge: a tighter blur.
            let sigma = if g.technique == designcraft_doc::GlowTechnique::Precise { sigma * 0.4 } else { sigma };
            let mix = crate::blend_mode(g.mode).mix;
            let paint = color_of(&c, g.opacity.clamp(0.0, 1.0));
            if g.center {
                self.with_blended_filters(ctx, f, (it, xf, page, Slot::InnerGlow), reach, sigma, mix, bp, |_, c, fr, _| {
                    c.set_transform(fr.view);
                    c.push_layer(None, None, None, None, Some(blur(sigma)));
                    c.set_transform(fr.view * xf);
                    c.set_paint(paint);
                    c.fill_path(bp);
                    // Fade towards the edge: cut a band of the glow's size away.
                    c.set_transform(Affine::IDENTITY);
                    c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                    c.set_transform(fr.view * xf);
                    c.set_paint(peniko::Color::BLACK);
                    c.set_stroke(kurbo::Stroke::new((g.size - choke).max(0.5) * 2.0).with_join(kurbo::Join::Round));
                    c.stroke_path(bp);
                    c.pop_layer();
                    c.pop_layer();
                });
            } else {
                self.inner_edge(ctx, f, (it, xf, page, Slot::InnerGlow), reach, bp, Vec2::ZERO, sigma, choke, paint, mix);
            }
        }
        // Satin: the shape shifted both ways along the angle, blurred, where exactly one covers.
        if e.satin.on
            && let Some(c) = doc.resolve_color(&e.satin.color, 1.0)
        {
            let st = &e.satin;
            let off = offset(st.angle, st.distance / 2.0);
            let sigma = st.size.max(0.0) / 2.0;
            let paint = color_of(&c, st.opacity.clamp(0.0, 1.0));
            let invert = st.invert;
            self.with_blended_filters(ctx, f, (it, xf, page, Slot::Satin), reach, sigma, Mix::Multiply, bp, |_, c, fr, _| {
                c.set_transform(fr.view);
                c.push_layer(None, None, None, None, Some(blur(sigma)));
                if invert {
                    c.set_transform(fr.view * xf);
                    c.set_paint(paint);
                    c.fill_path(bp);
                }
                c.set_transform(Affine::IDENTITY);
                c.push_layer(None, Some(BlendMode::new(Mix::Normal, if invert { Compose::DestOut } else { Compose::SrcOver })), None, None, None);
                c.set_transform(fr.view * Affine::translate(off) * xf);
                c.set_paint(paint);
                c.fill_path(bp);
                c.set_transform(Affine::IDENTITY);
                c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::Xor)), None, None, None);
                c.set_transform(fr.view * Affine::translate(-off) * xf);
                c.fill_path(bp);
                c.pop_layer();
                c.pop_layer();
                c.pop_layer();
            });
        }
        // Bevel and Emboss (inner bevel): highlight from the lit side, shadow from the other.
        if e.bevel.on {
            let b = &e.bevel;
            let off = offset(doc.light_angle(b.angle, b.global_light), b.size * (b.depth / 100.0).clamp(0.01, 10.0) * 0.5);
            let sigma = b.size.max(0.0) / 2.0;
            if let Some(c) = doc.resolve_color(&b.highlight, 1.0) {
                // The inverse shape moved away from the light lights the edges facing it.
                self.inner_edge(
                    ctx,
                    f,
                    (it, xf, page, Slot::BevelHighlight),
                    reach,
                    bp,
                    off,
                    sigma,
                    0.0,
                    color_of(&c, b.highlight_opacity.clamp(0.0, 1.0)),
                    Mix::Screen,
                );
            }
            if let Some(c) = doc.resolve_color(&b.shadow, 1.0) {
                self.inner_edge(
                    ctx,
                    f,
                    (it, xf, page, Slot::BevelShadow),
                    reach,
                    bp,
                    -off,
                    sigma,
                    0.0,
                    color_of(&c, b.shadow_opacity.clamp(0.0, 1.0)),
                    Mix::Multiply,
                );
            }
        }
    }

    /// An inner shadow-like edge: everything outside the shape moved by `off`, blurred by
    /// `sigma`, clipped to the shape and blended with `mix`.
    #[allow(clippy::too_many_arguments)]
    fn inner_edge(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        keep: Keep,
        reach: Rect,
        bp: &BezPath,
        off: Vec2,
        sigma: f64,
        choke: f64,
        paint: peniko::Color,
        mix: Mix,
    ) {
        let xf = keep.1;
        self.with_blended_filters(ctx, f, keep, reach, sigma, mix, bp, |_, c, fr, _| {
            c.set_transform(fr.view);
            c.push_layer(None, None, None, None, Some(blur(sigma)));
            // Everything outside the offset shape, so the colour bleeds in from the edges.
            let m = Affine::translate(off) * xf;
            let pad = sigma * 4.0 + choke + off.hypot() + 4.0 * fr.px;
            let mut inv = m.transform_rect_bbox(bp.bounding_box()).inflate(pad, pad).to_path(0.1);
            let mut shape = bp.clone();
            shape.apply_affine(m);
            inv.extend(shape.iter());
            c.set_paint(paint);
            c.set_fill_rule(peniko::Fill::EvenOdd);
            c.fill_path(&inv);
            c.set_fill_rule(peniko::Fill::NonZero);
            if choke > 0.0 {
                c.set_stroke(kurbo::Stroke::new(choke * 2.0).with_join(kurbo::Join::Round));
                c.stroke_path(&shape);
            }
            c.pop_layer();
        });
    }

    /// [`Renderer::with_filters`] clipped to `clip` (with the object's transform in `keep`), blended
    /// with `mix`. The effect's blend must see the object's backdrop, outside its clipping and offscreen
    /// filter layers. Putting it inside the clip blends with transparent pixels instead.
    #[allow(clippy::too_many_arguments)]
    fn with_blended_filters(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        keep: Keep,
        reach: Rect,
        sigma: f64,
        mix: Mix,
        clip: &BezPath,
        draw: impl Fn(&mut Self, &mut RenderContext, &Frame, bool),
    ) {
        if mix != Mix::Normal {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(BlendMode::new(mix, Compose::SrcOver)), None, None, None);
        }
        self.with_filters(ctx, f, Some(keep), reach, sigma, None, Some((clip, keep.1)), draw);
        if mix != Mix::Normal {
            ctx.pop_layer();
        }
    }

    /// Run `draw`, which pushes filter layers. Single-threaded contexts draw directly. On a
    /// multithreaded context, or for a blur wider than [`MAX_BLUR_SPREAD`], the layer is drawn into
    /// an offscreen single-threaded context covering `reach` (spread space) plus the blur's spread,
    /// then composited back. `clip` (a path and its spread transform) clips what `draw` paints;
    /// at a lower resolution it clips the composited image on `ctx`, so its edge stays sharp.
    ///
    /// With `keep` (the object, its parent transform, where it shows and which of its effects)
    /// the offscreen image covers what is on screen and a margin, and is placed again from
    /// [`crate::fx_cache`] while nothing it depends on changes. On a multithreaded context the
    /// image is only recorded here: [`Renderer::raster_fx`] draws all of them together on the
    /// cores before the context is rasterized.
    ///
    /// `draw` pushes its filter layer when told to (its last argument). A drop shadow or glow
    /// (`shadow`, its colour) drawn offscreen is drawn without it; its coverage is blurred
    /// afterwards ([`shadow_coverage`]) and placed in its colour: the same, at a fraction of the
    /// filter's cost, and kept once for every plate.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_filters(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        keep: Option<Keep>,
        reach: Rect,
        sigma: f64,
        shadow: Option<peniko::Color>,
        clip: Option<(&BezPath, Affine)>,
        draw: impl Fn(&mut Self, &mut RenderContext, &Frame, bool),
    ) {
        use crate::fx_cache::{Band, FxCache, FxImage, MAX_ENTRY_PIXELS, Source};
        // 3σ of the Gaussian in pixels, plus a pixel of antialiasing.
        let spread = sigma.max(0.0) * 3.0 / f.px + 2.0;
        if !f.mt && spread <= MAX_BLUR_SPREAD {
            push_clip(ctx, f, clip);
            draw(self, ctx, f, true);
            pop_clip(ctx, clip);
            return;
        }
        let all = f.view.transform_rect_bbox(reach);
        let screen = (ctx.width(), ctx.height());
        // A blur has no detail finer than itself: drawn at a lower resolution where it is
        // [`BLUR_SIGMA_PX`] wide, it looks the same and costs a fraction.
        let coarse = sigma.max(0.0) / f.px / BLUR_SIGMA_PX;
        // Kept: what is on screen and a margin around it, so panning within that only moves the
        // image. Too large to keep: what is on screen, drawn each time.
        let kept = keep.and_then(|k| {
            offscreen(all, spread, screen, KEEP_MARGIN, coarse)
                .filter(|o| f64::from(o.width) * f64::from(o.height) <= MAX_ENTRY_PIXELS)
                .map(|o| (k, o))
        });
        let o = match &kept {
            Some((_, o)) => *o,
            None => match offscreen(all, spread, screen, 0.0, coarse) {
                Some(o) => o,
                None => return,
            },
        };
        // Clip where the result is at full resolution: offscreen unless that is reduced.
        let (inner, outer) = if o.scale > 1.0 { (None, clip) } else { (clip, None) };
        let mut prepared = match kept {
            Some(((it, xf, page, slot), _)) => {
                let mut stories = Vec::new();
                it.walk(&mut |i: &Item| {
                    if let designcraft_doc::Content::Text(tf) = &i.content {
                        stories.push(tf.story);
                    }
                });
                let stories = stories.into_iter().map(|sid| self.story(f, sid, page)).collect();
                // What must be in the image now: what shows, and what a blur brings onto the screen.
                let sw = f64::from(screen.0);
                let sh = f64::from(screen.1);
                let needed = all.inflate(spread, spread).intersect(Rect::new(0.0, 0.0, sw, sh).inflate(spread, spread));
                let src = Source { item: it, xf, view: f.view, px: f.px, stories, needed };
                // A shadow's coverage is the same on every plate (it is coloured as it is placed).
                let plain;
                let (plate, opts) = match shadow {
                    Some(_) => {
                        plain = RenderOptions { plate: None, ..f.opts.clone() };
                        (None, &plain)
                    }
                    None => (f.opts.plate, f.opts),
                };
                let key = (it.id, page, slot, plate);
                if let Some(p) = self.fx_cache.get(key, &src, f.doc, opts) {
                    push_clip(ctx, f, outer);
                    for b in p.image.bands {
                        place(ctx, vello_cpu::ImageSource::Pixmap(b.image), (p.x0, p.y0, p.scale), p.image.width, b.top, b.core, shadow);
                    }
                    pop_clip(ctx, outer);
                    return;
                }
                Some(FxCache::prepare(key, src, opts, o.x0, o.y0, o.scale))
            }
            None => None,
        };
        let at = (o.x0, o.y0, o.scale);
        // Drawn with the others on the cores, a large image is drawn in bands of rows, each
        // reaching past its rows by the blur's spread.
        let deferred = f.mt && self.fx_deferred();
        // A shadow's blur (σ, offscreen pixels), put on its coverage after it is drawn.
        let post = shadow.map(|_| (sigma.max(0.0) / (f.px * o.scale)) as f32);
        let pad = (spread / o.scale).ceil().clamp(0.0, f64::from(u16::MAX)) as u32 + 2;
        let mut bands = Vec::new();
        push_clip(ctx, f, outer);
        for core in bands_of(o.width, o.height, pad, deferred) {
            let top = core.0.saturating_sub(pad);
            let bottom = core.1.saturating_add(pad).min(u32::from(o.height));
            let rows = u16::try_from(bottom.saturating_sub(top)).unwrap_or(o.height).max(1);
            let mut off = RenderContext::new_with(o.width, rows, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
            let band_y0 = o.y0 + f64::from(top) * o.scale;
            let view = Affine::scale(1.0 / o.scale) * Affine::translate((-o.x0, -band_y0)) * f.view;
            // Culling inside sees the whole band as visible.
            let area = Rect::new(o.x0, band_y0, o.x0 + f64::from(o.width) * o.scale, band_y0 + f64::from(rows) * o.scale);
            let visible = if f.view.determinant().abs() > 1e-12 { f.view.inverse().transform_rect_bbox(area) } else { f.visible };
            let shifted = Frame { mt: false, view, px: f.px * o.scale, visible, ..*f };
            push_clip(&mut off, &shifted, inner);
            draw(self, &mut off, &shifted, post.is_none());
            pop_clip(&mut off, inner);
            off.flush();
            if deferred {
                let id = self.defer_fx(off, post);
                place(ctx, vello_cpu::ImageSource::OpaqueId { id, may_have_transparency: true }, at, o.width, top, core, shadow);
                bands.push((id, top, core));
            } else {
                let mut pm = vello_cpu::Pixmap::new(o.width, rows);
                off.render(&mut pm, &mut self.resources);
                if let Some(sigma) = post {
                    shadow_coverage(&mut pm, sigma);
                }
                let image = std::sync::Arc::new(pm);
                place(ctx, vello_cpu::ImageSource::Pixmap(image.clone()), at, o.width, top, core, shadow);
                if let Some(p) = prepared.take() {
                    self.fx_cache.put(p, FxImage { width: o.width, height: u32::from(o.height), bands: vec![Band { image, top, core }] });
                }
            }
        }
        if deferred {
            if let Some(p) = prepared {
                self.keep_fx(p, o.width, u32::from(o.height), bands);
            }
            self.fx_flush_if_full();
        }
        pop_clip(ctx, outer);
    }
}

/// An object whose effect image may be kept: the object, its parent transform, where it shows and
/// which of its effects.
pub(crate) type Keep<'a> = (&'a Item, Affine, ShownOn, Slot);

/// Device pixels kept around the screen of an effect image, for panning.
const KEEP_MARGIN: f64 = 256.0;

/// Rows `core` of an effect image `width` pixels wide whose top left is at (`x0`, `y0`) on the
/// device and each of whose pixels is `scale` device pixels wide, from `image`, which holds its
/// rows from `top` on; a shadow's coverage in its colour (`tint`).
#[allow(clippy::too_many_arguments)]
fn place(
    ctx: &mut RenderContext,
    image: vello_cpu::ImageSource,
    (x0, y0, scale): (f64, f64, f64),
    width: u16,
    top: u32,
    core: (u32, u32),
    tint: Option<peniko::Color>,
) {
    ctx.set_transform(Affine::translate((x0, y0)) * Affine::scale(scale) * Affine::translate((0.0, f64::from(top))));
    ctx.set_paint(vello_cpu::Image { image, sampler: peniko::ImageSampler::default() });
    if let Some(color) = tint {
        ctx.set_tint(Some(vello_common::paint::Tint { color, mode: vello_common::paint::TintMode::AlphaMask }));
    }
    let rows = |r: u32| f64::from(r.saturating_sub(top));
    ctx.fill_rect(&Rect::new(0.0, rows(core.0), f64::from(width), rows(core.1)));
    if tint.is_some() {
        ctx.set_tint(None);
    }
    ctx.set_transform(Affine::IDENTITY);
}

/// Pixels of an effect image drawn as one band at most when it is drawn with the others on the
/// cores: a large one is shared out among them.
const BAND_PIXELS: u64 = 1 << 19;

/// The bands of rows an effect image `width` × `height` pixels large is drawn in (one unless
/// `split`): none thinner than twice the rows each reaches past its own (`pad`).
fn bands_of(width: u16, height: u16, pad: u32, split: bool) -> Vec<(u32, u32)> {
    let h = u32::from(height);
    let n = if split { (u64::from(width) * u64::from(h)).div_ceil(BAND_PIXELS).clamp(1, 64) as u32 } else { 1 };
    let rows = h.div_ceil(n.max(1)).max(pad.saturating_mul(2)).max(1);
    let mut out = Vec::new();
    let mut y = 0;
    while y < h {
        let end = y.saturating_add(rows).min(h);
        out.push((y, end));
        y = end;
    }
    out
}

fn push_clip(ctx: &mut RenderContext, f: &Frame, clip: Option<(&BezPath, Affine)>) {
    if let Some((bp, xf)) = clip {
        ctx.set_transform(f.view * xf);
        ctx.push_clip_layer(bp);
    }
}

fn pop_clip(ctx: &mut RenderContext, clip: Option<(&BezPath, Affine)>) {
    if clip.is_some() {
        ctx.pop_layer();
    }
}

/// The widest blur (3σ, device pixels) drawn at full resolution. vello pads a filter layer by the
/// blur's width on every side, so a wide blur needs gigabytes; it is drawn at a lower resolution
/// instead, where it is this wide, and has no detail to lose there.
const MAX_BLUR_SPREAD: f64 = 1024.0;

/// The blur (σ, pixels) an offscreen effect image is drawn with at most: a wider one is drawn at
/// a lower resolution, where it is this wide, and scaled up. Smoothed by the blur, a silhouette
/// drawn so differs from one drawn at full resolution by a level or two; at 400 % a heading's
/// shadow takes a hundredth of the pixels.
const BLUR_SIGMA_PX: f64 = 3.0;

/// An offscreen image for [`Renderer::with_filters`]: where it goes on the device, how many device
/// pixels one of its pixels covers, and its size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Offscreen {
    x0: f64,
    y0: f64,
    scale: f64,
    width: u16,
    height: u16,
}

/// The offscreen image an effect is drawn in: `reach` (device pixels) widened by the blur's
/// `spread`, within the `screen` widened the same way and by `margin`, at a resolution that keeps
/// the blur within [`MAX_BLUR_SPREAD`] and each of its pixels at least `coarse` device pixels
/// wide, at most `MAX_SIDE` a side. `None` when none of it shows.
fn offscreen(reach: Rect, spread: f64, (width, height): (u16, u16), margin: f64, coarse: f64) -> Option<Offscreen> {
    let screen = Rect::new(0.0, 0.0, f64::from(width), f64::from(height)).inflate(spread + margin, spread + margin);
    let r = reach.inflate(spread, spread).intersect(screen);
    if !(r.width() > 0.0 && r.height() > 0.0) {
        return None;
    }
    let (x0, y0) = (r.x0.floor(), r.y0.floor());
    let scale = (spread / MAX_BLUR_SPREAD).max(coarse).max(1.0);
    let side = |len: f64| (len / scale).ceil().min(crate::MAX_SIDE as f64);
    let (w, h) = (side(r.x1.ceil() - x0), side(r.y1.ceil() - y0));
    (w >= 1.0 && h >= 1.0).then_some(Offscreen { x0, y0, scale, width: w as u16, height: h as u16 })
}

/// What vello's drop-shadow filter (no offset) blurs of `pm`, without it: its coverage blurred by
/// `sigma` pixels (a Gaussian 3σ wide, nothing beyond the edges), as white of that coverage. It
/// is coloured as it is placed (an alpha-mask tint), so one image serves every colour and plate.
/// One channel where the filter blurs four, and nothing blurred where nothing is.
pub(crate) fn shadow_coverage(pm: &mut vello_cpu::Pixmap, sigma: f32) {
    let w = usize::from(pm.width());
    let mut cov: Vec<f32> = pm.data().iter().map(|p| f32::from(p.a)).collect();
    if sigma > 0.0 && w > 0 {
        blur_coverage(&mut cov, w, sigma);
    }
    for (p, v) in pm.data_mut().iter_mut().zip(&cov) {
        let a = v.round().clamp(0.0, 255.0) as u8;
        *p = vello_cpu::color::PremulRgba8 { r: a, g: a, b: a, a };
    }
}

/// The weights of a Gaussian of `sigma` pixels, 3σ either way (as vello's).
fn gaussian(sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil().clamp(0.0, 255.0) as usize;
    let k: Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let x = i as f32 - radius as f32;
            (-(x * x) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f32 = k.iter().sum();
    k.into_iter().map(|v| if sum > 0.0 { v / sum } else { v }).collect()
}

/// `cov` (rows `w` wide) blurred by a Gaussian of `sigma` pixels, along the rows then the columns;
/// rows with nothing in them are passed over.
fn blur_coverage(cov: &mut [f32], w: usize, sigma: f32) {
    let k = gaussian(sigma);
    let r = k.len() / 2;
    let mut tmp = vec![0.0f32; cov.len()];
    let mut inked = Vec::with_capacity(cov.len() / w);
    for (src, dst) in cov.chunks(w).zip(tmp.chunks_mut(w)) {
        let any = src.iter().any(|v| *v > 0.0);
        inked.push(any);
        if !any {
            continue;
        }
        for (x, d) in dst.iter_mut().enumerate() {
            let lo = x.saturating_sub(r);
            let taps = k.iter().skip(lo + r - x);
            *d = src.iter().skip(lo).zip(taps).map(|(s, t)| s * t).sum();
        }
    }
    for (y, out) in cov.chunks_mut(w).enumerate() {
        out.fill(0.0);
        let lo = y.saturating_sub(r);
        for (j, (row, t)) in tmp.chunks(w).zip(&inked).enumerate().skip(lo).take(y + r + 1 - lo) {
            if !*t {
                continue;
            }
            let Some(weight) = k.get(j + r - y) else { continue };
            for (o, s) in out.iter_mut().zip(row) {
                *o += s * weight;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_blur_is_drawn_offscreen_at_a_lower_resolution() {
        let object = Rect::new(150.0, 150.0, 350.0, 350.0);
        let small = Offscreen { x0: 140.0, y0: 140.0, scale: 1.0, width: 220, height: 220 };
        assert_eq!(offscreen(object, 10.0, (1440, 900), 0.0, 1.0), Some(small));
        // A 10,000 pt drop shadow at 100%: 3σ is 15,000 px, which asked for 30,204 px a side and as
        // much padding again inside vello's filter layer. Drawn at 1/14.65 it is 1,024 px wide.
        let wide = Offscreen { x0: -14852.0, y0: -14852.0, scale: 15_002.0 / 1024.0, width: 2062, height: 2062 };
        assert_eq!(offscreen(object, 15_002.0, (1440, 900), 0.0, 1.0), Some(wide));
        // A 40 px σ drawn at a 3 px one: a pixel for every 13⅓ on the device.
        let coarse = offscreen(object, 122.0, (1440, 900), 0.0, 40.0 / BLUR_SIGMA_PX).unwrap();
        assert_eq!((coarse.scale, coarse.width, coarse.height), (40.0 / 3.0, 34, 34));
    }
}
