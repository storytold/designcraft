//! Soft object effects (Effects panel): drop shadow, outer glow, inner shadow and basic feather,
//! drawn with vello filter layers.
//!
//! vello_cpu only supports filter layers in single-threaded contexts, so on the multithreaded
//! pipeline each effect is rendered into an offscreen single-threaded context cropped to its
//! reach and composited back as an image ([`Renderer::with_filters`]); single-threaded contexts
//! draw directly. A very wide blur goes offscreen in both, at a lower resolution; the object
//! itself and the clip to its shape stay at full resolution.
//!
//! - Drop shadow / outer glow: the object's silhouette (everything it paints: fill, image, text,
//!   stroke; a group's objects together) in a `DropShadowOnly` layer under the object. `size`
//!   sets the blur (σ = size / 2). `spread` (percent) hardens it: the blurred coverage is raised
//!   by 1 / (1 − spread), so the silhouette of what is painted grows solid, never the frame of a
//!   transparent image. The shadow's opacity rides on the filter colour's alpha.
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

use crate::{Frame, Renderer, color_of};
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
    /// Drop shadow and outer glow, below the object (or the group: its objects together). Spread
    /// grows the silhouette of what the object paints, so a transparent part of an image or an
    /// empty frame casts nothing along its frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_below(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine, page: ShownOn, reach: Rect) {
        let doc = f.doc;
        let e = &it.effects;
        let mut below = vec![];
        if e.outer_glow.on {
            let g = &e.outer_glow;
            below.push((Vec2::ZERO, g.size, g.spread, g.color.clone(), g.opacity));
        }
        if e.drop_shadow.on {
            let d = &e.drop_shadow;
            below.push((offset(doc.light_angle(d.angle, d.global_light), d.distance), d.size, d.spread, d.color.clone(), d.opacity));
        }
        for (off, size, pct, color, opacity) in below {
            let Some(c) = doc.resolve_color(&color, 1.0) else { continue };
            let (sigma, grow) = split(size, pct);
            let filter = shadow(sigma, color_of(&c, opacity.clamp(0.0, 1.0)));
            // The offset moves the geometry, not the filter: vello drops layer content that lies
            // entirely outside the viewport before filtering.
            let r = (reach + off).inflate(grow, grow);
            let grown = if grow > 0.0 { self.grown_silhouette(ctx, f, it, bp, xf, page, reach, off, grow) } else { None };
            if grow > 0.0 && grown.is_none() {
                continue;
            }
            self.with_filters(ctx, f, r, sigma, None, |me, c, fr| {
                let moved = moved_frame(fr, off, sigma);
                c.set_transform(moved.view);
                c.push_layer(None, None, None, None, Some(filter));
                if let Some((mask, at)) = &grown {
                    // `at` places the mask on the device; this frame's view replaces `f.view`.
                    let to_frame = if f.view.determinant().abs() > 1e-12 { fr.view * f.view.inverse() } else { Affine::IDENTITY };
                    c.set_transform(to_frame * *at);
                    c.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(mask.clone()), sampler: peniko::ImageSampler::default() });
                    c.fill_rect(&Rect::new(0.0, 0.0, f64::from(mask.width()), f64::from(mask.height())));
                } else {
                    me.draw_silhouette(c, &moved, it, bp, xf, page);
                }
                c.pop_layer();
            });
        }
    }

    /// What casts the object's shadow: its objects for a group, else the object itself.
    #[allow(clippy::too_many_arguments)]
    fn draw_silhouette(&mut self, c: &mut RenderContext, fr: &Frame, it: &Item, bp: &BezPath, xf: Affine, page: ShownOn) {
        if it.is_group() {
            for child in it.shown_children() {
                self.draw_item(c, fr, child, xf, page);
            }
        } else {
            self.draw_body(c, fr, it, bp, xf, page);
        }
    }

    /// The silhouette moved by `off` and grown by `grow` (spread space) around what it paints, as
    /// black coverage on the device grid: a transparent part of an image or an empty frame grows
    /// nothing. Returns the image and where it goes on the device, or `None` when none of it shows.
    #[allow(clippy::too_many_arguments)]
    fn grown_silhouette(
        &mut self,
        ctx: &RenderContext,
        f: &Frame,
        it: &Item,
        bp: &BezPath,
        xf: Affine,
        page: ShownOn,
        reach: Rect,
        off: Vec2,
        grow: f64,
    ) -> Option<(std::sync::Arc<vello_cpu::Pixmap>, Affine)> {
        let o = offscreen(f.view.transform_rect_bbox(reach + off), grow / f.px + 2.0, ctx.width(), ctx.height())?;
        let mut c = RenderContext::new_with(o.width, o.height, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
        let view = Affine::scale(1.0 / o.scale) * Affine::translate((-o.x0, -o.y0)) * f.view;
        let shifted = Frame { mt: false, view, px: f.px * o.scale, ..*f };
        let moved = moved_frame(&shifted, off, grow);
        self.draw_silhouette(&mut c, &moved, it, bp, xf, page);
        c.flush();
        let mut pm = vello_cpu::Pixmap::new(o.width, o.height);
        c.render(&mut pm, &mut self.resources);
        let coverage: Vec<u8> = pm.data().iter().map(|p| p.a).collect();
        let grown = dilate(&coverage, usize::from(o.width), usize::from(o.height), grow / shifted.px);
        for (p, a) in pm.data_mut().iter_mut().zip(grown) {
            *p = vello_cpu::color::PremulRgba8 { r: 0, g: 0, b: 0, a };
        }
        Some((std::sync::Arc::new(pm), Affine::translate((o.x0, o.y0)) * Affine::scale(o.scale)))
    }

    /// Draw an item with soft effects: below-effects, the (feathered) body, inner shadow.
    pub(crate) fn draw_item_fx(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine, page: ShownOn) {
        let e = &it.effects;
        let doc = f.doc;
        let sw = if it.stroke.is_none() { 0.0 } else { it.stroke.extent() };
        // Spread-space reach of what the object paints.
        let reach = xf.transform_rect_bbox(bp.bounding_box()).inflate(sw + 1.0, sw + 1.0);
        self.draw_below(ctx, f, it, bp, xf, page, reach);
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
            self.with_filters(ctx, f, reach, sigma, None, |_, c, fr| {
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
            self.inner_edge(ctx, f, reach, bp, xf, off, sigma, choke, color_of(&c, s.opacity.clamp(0.0, 1.0)), Mix::Normal);
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
                self.with_blended_filters(ctx, f, reach, sigma, mix, (bp, xf), |_, c, fr| {
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
                self.inner_edge(ctx, f, reach, bp, xf, Vec2::ZERO, sigma, choke, paint, mix);
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
            self.with_blended_filters(ctx, f, reach, sigma, Mix::Multiply, (bp, xf), |_, c, fr| {
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
                self.inner_edge(ctx, f, reach, bp, xf, off, sigma, 0.0, color_of(&c, b.highlight_opacity.clamp(0.0, 1.0)), Mix::Screen);
            }
            if let Some(c) = doc.resolve_color(&b.shadow, 1.0) {
                self.inner_edge(ctx, f, reach, bp, xf, -off, sigma, 0.0, color_of(&c, b.shadow_opacity.clamp(0.0, 1.0)), Mix::Multiply);
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
        reach: Rect,
        bp: &BezPath,
        xf: Affine,
        off: Vec2,
        sigma: f64,
        choke: f64,
        paint: peniko::Color,
        mix: Mix,
    ) {
        self.with_blended_filters(ctx, f, reach, sigma, mix, (bp, xf), |_, c, fr| {
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

    /// [`Renderer::with_filters`] clipped to `clip` (a path and its spread transform), blended with
    /// `mix`. The effect's blend must see the object's backdrop, outside its clipping and offscreen
    /// filter layers. Putting it inside the clip blends with transparent pixels instead.
    #[allow(clippy::too_many_arguments)]
    fn with_blended_filters(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        reach: Rect,
        sigma: f64,
        mix: Mix,
        clip: (&BezPath, Affine),
        draw: impl FnOnce(&mut Self, &mut RenderContext, &Frame),
    ) {
        if mix != Mix::Normal {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(BlendMode::new(mix, Compose::SrcOver)), None, None, None);
        }
        self.with_filters(ctx, f, reach, sigma, Some(clip), draw);
        if mix != Mix::Normal {
            ctx.pop_layer();
        }
    }

    /// Run `draw`, which pushes filter layers. Single-threaded contexts draw directly. On a
    /// multithreaded context, or for a blur wider than [`MAX_BLUR_SPREAD`], the layer is drawn into
    /// an offscreen single-threaded context covering `reach` (spread space) plus the blur's spread,
    /// then composited back. `clip` (a path and its spread transform) clips what `draw` paints;
    /// at a lower resolution it clips the composited image on `ctx`, so its edge stays sharp.
    pub(crate) fn with_filters(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        reach: Rect,
        sigma: f64,
        clip: Option<(&BezPath, Affine)>,
        draw: impl FnOnce(&mut Self, &mut RenderContext, &Frame),
    ) {
        // 3σ of the Gaussian in pixels, plus a pixel of antialiasing.
        let spread = sigma.max(0.0) * 3.0 / f.px + 2.0;
        if !f.mt && spread <= MAX_BLUR_SPREAD {
            push_clip(ctx, f, clip);
            draw(self, ctx, f);
            pop_clip(ctx, clip);
            return;
        }
        let Some(o) = offscreen(f.view.transform_rect_bbox(reach), spread, ctx.width(), ctx.height()) else { return };
        let mut off = RenderContext::new_with(o.width, o.height, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
        let view = Affine::scale(1.0 / o.scale) * Affine::translate((-o.x0, -o.y0)) * f.view;
        let shifted = Frame { mt: false, view, px: f.px * o.scale, ..*f };
        // Clip where the result is at full resolution: offscreen unless that is reduced.
        let (inner, outer) = if o.scale > 1.0 { (None, clip) } else { (clip, None) };
        push_clip(&mut off, &shifted, inner);
        draw(self, &mut off, &shifted);
        pop_clip(&mut off, inner);
        off.flush();
        let mut pm = vello_cpu::Pixmap::new(o.width, o.height);
        off.render(&mut pm, &mut self.resources);
        push_clip(ctx, f, outer);
        ctx.set_transform(Affine::translate((o.x0, o.y0)) * Affine::scale(o.scale));
        ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(std::sync::Arc::new(pm)), sampler: peniko::ImageSampler::default() });
        ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(o.width), f64::from(o.height)));
        pop_clip(ctx, outer);
        ctx.set_transform(Affine::IDENTITY);
    }
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

/// An offscreen image for [`Renderer::with_filters`]: where it goes on the device, how many device
/// pixels one of its pixels covers, and its size.
#[derive(Debug, PartialEq)]
struct Offscreen {
    x0: f64,
    y0: f64,
    scale: f64,
    width: u16,
    height: u16,
}

/// The offscreen image an effect is drawn in: `reach` (device pixels) widened by the blur's
/// `spread`, within the window widened the same way, at a resolution that keeps the blur within
/// [`MAX_BLUR_SPREAD`], at most `MAX_SIDE` a side. `None` when none of it shows.
fn offscreen(reach: Rect, spread: f64, width: u16, height: u16) -> Option<Offscreen> {
    let screen = Rect::new(0.0, 0.0, f64::from(width), f64::from(height)).inflate(spread, spread);
    let r = reach.inflate(spread, spread).intersect(screen);
    let (x0, y0) = (r.x0.floor(), r.y0.floor());
    let scale = (spread / MAX_BLUR_SPREAD).max(1.0);
    let side = |len: f64| (len / scale).ceil().min(crate::MAX_SIDE as f64);
    let (w, h) = (side(r.x1.ceil() - x0), side(r.y1.ceil() - y0));
    (w >= 1.0 && h >= 1.0).then_some(Offscreen { x0, y0, scale, width: w as u16, height: h as u16 })
}

/// `fr` with its view moved by `off` (spread space), and its visible area widened to take in
/// what a blur of `sigma` brings into view from there.
fn moved_frame<'a>(fr: &Frame<'a>, off: Vec2, sigma: f64) -> Frame<'a> {
    let pad = sigma.max(0.0) * 3.0;
    Frame { view: fr.view * Affine::translate(off), visible: (fr.visible - off).inflate(pad, pad), ..*fr }
}

/// `coverage` (`w` × `h`, row-major) grown by `radius` pixels: each pixel covers as much as its
/// distance to the nearest edge allows (antialiased over one pixel), and never less than it did. Exact Euclidean distances (Felzenszwalb and Huttenlocher's transform).
fn dilate(coverage: &[u8], w: usize, h: usize, radius: f64) -> Vec<u8> {
    const FAR: f64 = 1e20;
    // A pixel's centre lies (0.5 − coverage) outside the edge when less than half covered.
    let seed = |a: u8| match a {
        0 => FAR,
        1..128 => (0.5 - f64::from(a) / 255.0).powi(2),
        _ => 0.0,
    };
    let mut d: Vec<f64> = coverage.iter().map(|&a| seed(a)).collect();
    let mut line = vec![0.0; w.max(h)];
    let mut out = vec![0.0; w.max(h)];
    let (mut v, mut z) = (vec![0usize; w.max(h)], vec![0.0; w.max(h) + 1]);
    for x in 0..w {
        for y in 0..h {
            line[y] = d[y * w + x];
        }
        distance_1d(&line[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            d[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        line[..w].copy_from_slice(&d[y * w..(y + 1) * w]);
        distance_1d(&line[..w], &mut out[..w], &mut v, &mut z);
        d[y * w..(y + 1) * w].copy_from_slice(&out[..w]);
    }
    coverage
        .iter()
        .zip(d)
        .map(|(&a, d2)| {
            let grown = (radius + 0.5 - d2.sqrt()).clamp(0.0, 1.0);
            a.max((grown * 255.0).round() as u8)
        })
        .collect()
}

/// Squared distance transform of one line (`f`: 0 at sources, huge elsewhere) into `out`.
fn distance_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let sq = |q: usize| (q * q) as f64;
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let meet = |q: usize, p: usize| ((f[q] + sq(q)) - (f[p] + sq(p))) / (2.0 * (q as f64 - p as f64));
    for q in 1..n {
        // z[0] is -∞, so this stops at k = 0 at the latest.
        let mut s = meet(q, v[k]);
        while k > 0 && s <= z[k] {
            k -= 1;
            s = meet(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f64 - p as f64;
        *o = dq * dq + f[p];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_blur_is_drawn_offscreen_at_a_lower_resolution() {
        let object = Rect::new(150.0, 150.0, 350.0, 350.0);
        let small = Offscreen { x0: 140.0, y0: 140.0, scale: 1.0, width: 220, height: 220 };
        assert_eq!(offscreen(object, 10.0, 1440, 900), Some(small));
        // A 10,000 pt drop shadow at 100%: 3σ is 15,000 px, which asked for 30,204 px a side and as
        // much padding again inside vello's filter layer. Drawn at 1/14.65 it is 1,024 px wide.
        let wide = Offscreen { x0: -14852.0, y0: -14852.0, scale: 15_002.0 / 1024.0, width: 2062, height: 2062 };
        assert_eq!(offscreen(object, 15_002.0, 1440, 900), Some(wide));
    }
}
