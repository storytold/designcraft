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
//!   stroke) in a `DropShadowOnly` layer under the object. `size` sets the blur (σ = size / 2),
//!   `spread`/`choke` (percent of size) grow the silhouette instead of blurring it. The shadow's
//!   opacity rides on the filter colour's alpha.
//! - Inner shadow: the inverse of the offset shape, blurred, clipped to the shape, on top.
//! - Inner glow, bevel and emboss, satin: built like the inner shadow (an offset inverse of the
//!   shape, blurred and clipped) with screen / multiply blending; satin XORs two offset copies.
//! - Feather: the object is masked by its shape inset by half the width and blurred, so the
//!   edge fades from ~0 at the path to full strength `width` inside. The basic, directional and
//!   gradient feathers fade the inner effects with the object.

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
    let distance = bound(distance);
    Vec2::new(-a.cos() * distance, a.sin() * distance)
}

/// How far (points) the effects of `it` reach outside its geometry.
pub(crate) fn outset(it: &Item) -> f64 {
    let e = &it.effects;
    let mut o: f64 = 0.0;
    if e.drop_shadow.on {
        o = o.max(bound(e.drop_shadow.distance).abs() + bound(e.drop_shadow.size).abs() * 1.6 + 1.0);
    }
    if e.outer_glow.on {
        o = o.max(bound(e.outer_glow.size).abs() * 1.6 + 1.0);
    }
    o
}

/// Whether `it` has a basic feather: a width on a closed shape.
fn basic_feather(it: &Item) -> bool {
    it.effects.feather > 0.0 && it.path.is_closed()
}

/// Whether any of `it`'s feathers (basic, directional, gradient) fades it.
fn fades(it: &Item) -> bool {
    let e = &it.effects;
    basic_feather(it) || e.directional_feather.on && e.directional_feather.widths.iter().any(|w| *w > 0.0) || e.gradient_feather.on
}

/// (blur σ, growth) in points for an effect `size` with `pct` percent hardened.
fn split(size: f64, pct: f64) -> (f64, f64) {
    let size = bound(size).max(0.0);
    let p = (pct / 100.0).clamp(0.0, 1.0);
    (size * (1.0 - p) / 2.0, size * p)
}

/// Largest blur standard deviation handed to the filter planner, in points: far wider than any
/// page, so a larger one looks the same. Its planner squares the value in f32, so an unbounded
/// finite size would overflow to infinity there and never finish.
const MAX_SIGMA: f64 = 1.0e6;

/// Largest effect extent (size, distance, feather width) used for geometry, in points: twice the
/// largest blur, so a size of any finite value, or infinity, cannot overflow later arithmetic
/// (`sigma * 3.0`, padding, reach). NaN is 0.
fn bound(v: f64) -> f64 {
    if v.is_nan() { 0.0 } else { v.clamp(-2.0 * MAX_SIGMA, 2.0 * MAX_SIGMA) }
}

fn std_dev(sigma: f64) -> f32 {
    if sigma.is_nan() { 0.0 } else { sigma.clamp(0.0, MAX_SIGMA) as f32 }
}

fn blur(sigma: f64) -> Filter {
    Filter::from_primitive(FilterPrimitive::GaussianBlur { std_deviation: std_dev(sigma), edge_mode: EdgeMode::None })
}

fn shadow(sigma: f64, color: peniko::Color) -> Filter {
    Filter::from_primitive(FilterPrimitive::DropShadowOnly { dx: 0.0, dy: 0.0, std_deviation: std_dev(sigma), color, edge_mode: EdgeMode::None })
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
            below.push((Vec2::ZERO, g.size, g.spread, g.color.clone(), g.opacity, g.mode));
        }
        if e.drop_shadow.on {
            let d = &e.drop_shadow;
            below.push((offset(doc.light_angle(d.angle, d.global_light), d.distance), d.size, d.spread, d.color.clone(), d.opacity, d.mode));
        }
        for (off, size, pct, color, opacity, mode) in below {
            let Some(c) = doc.resolve_color(&color, 1.0) else { continue };
            let (sigma, grow) = split(size, pct);
            let filter = shadow(sigma, color_of(&c, opacity.clamp(0.0, 1.0)));
            // The offset moves the geometry, not the filter: vello drops layer content that lies
            // entirely outside the viewport before filtering.
            let r = (reach + off).inflate(grow, grow);
            // The effect's blend sees what's behind the object: a layer around the filtered one.
            let mix = crate::blend_mode(mode).mix;
            if mix != Mix::Normal {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(BlendMode::new(mix, Compose::SrcOver)), None, None, None);
            }
            self.with_filters(ctx, f, r, sigma, None, |me, c, fr| {
                let moved = Frame { view: fr.view * Affine::translate(off), ..*fr };
                c.set_transform(moved.view);
                c.push_layer(None, None, None, None, Some(filter));
                me.draw_body(c, &moved, it, bp, xf, page);
                if grow > 0.0 && solid {
                    c.set_transform(moved.view * xf);
                    c.set_paint(peniko::Color::BLACK);
                    c.set_stroke(kurbo::Stroke::new(grow * 2.0).with_join(kurbo::Join::Round));
                    c.stroke_path(bp);
                }
                c.pop_layer();
            });
            if mix != Mix::Normal {
                ctx.pop_layer();
            }
        }
        // The object, feathered or not, drawn in its own layer and faded there. A basic feather
        // clips the body to the shape at device resolution; only its blurred mask may be drawn
        // offscreen at a lower one.
        let fading = fades(it);
        let basic = basic_feather(it);
        if fading {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, None, None, None);
        }
        if basic {
            ctx.set_transform(f.view * xf);
            ctx.push_clip_layer(bp);
        }
        self.draw_body(ctx, f, it, bp, xf, page);
        if basic {
            ctx.pop_layer();
        }
        if fading {
            self.fade(ctx, f, it, bp, xf, reach);
            ctx.pop_layer();
        }
        if !it.path.is_closed() {
            return;
        }
        // The feathers fade the inner effects with the object: each one is faded in its own
        // layer before it blends with what is behind.
        let fade = fading.then_some(it);
        // An image's edge is where it is opaque, not the frame: inner shadows, glows and bevels
        // follow what is painted when the frame holds a graphic.
        let painted = matches!(it.content, designcraft_doc::Content::Graphic(_)).then_some((it, page));
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
                reach,
                bp,
                xf,
                off,
                sigma,
                choke,
                color_of(&c, s.opacity.clamp(0.0, 1.0)),
                crate::blend_mode(s.mode).mix,
                painted,
                fade,
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
                self.with_blended_filters(ctx, f, reach, sigma, mix, (bp, xf), fade, |_, c, fr| {
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
                    c.set_stroke(kurbo::Stroke::new((bound(g.size) - choke).max(0.5) * 2.0).with_join(kurbo::Join::Round));
                    c.stroke_path(bp);
                    c.pop_layer();
                    c.pop_layer();
                });
            } else {
                self.inner_edge(ctx, f, reach, bp, xf, Vec2::ZERO, sigma, choke, paint, mix, painted, fade);
            }
        }
        // Satin: the shape shifted both ways along the angle, blurred, where exactly one covers.
        if e.satin.on
            && let Some(c) = doc.resolve_color(&e.satin.color, 1.0)
        {
            let st = &e.satin;
            let off = offset(st.angle, st.distance / 2.0);
            let sigma = bound(st.size).max(0.0) / 2.0;
            let paint = color_of(&c, st.opacity.clamp(0.0, 1.0));
            let invert = st.invert;
            self.with_blended_filters(ctx, f, reach, sigma, Mix::Multiply, (bp, xf), fade, |_, c, fr| {
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
            let off = offset(doc.light_angle(b.angle, b.global_light), bound(b.size) * (b.depth / 100.0).clamp(0.01, 10.0) * 0.5);
            let sigma = bound(b.size).max(0.0) / 2.0;
            if let Some(c) = doc.resolve_color(&b.highlight, 1.0) {
                // The inverse shape moved away from the light lights the edges facing it.
                let paint = color_of(&c, b.highlight_opacity.clamp(0.0, 1.0));
                self.inner_edge(ctx, f, reach, bp, xf, off, sigma, 0.0, paint, Mix::Screen, painted, fade);
            }
            if let Some(c) = doc.resolve_color(&b.shadow, 1.0) {
                let paint = color_of(&c, b.shadow_opacity.clamp(0.0, 1.0));
                self.inner_edge(ctx, f, reach, bp, xf, -off, sigma, 0.0, paint, Mix::Multiply, painted, fade);
            }
        }
    }

    /// An inner shadow-like edge: everything outside the shape moved by `off`, blurred by
    /// `sigma`, clipped to the shape and blended with `mix`. With `painted` (an object and the
    /// page it shows on), the shape is what the object paints, so a transparent part of an image
    /// is outside it: the edge runs along the image's opaque pixels, not the frame. With `fade`,
    /// the edge fades with that object's feathers.
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
        painted: Option<(&Item, ShownOn)>,
        fade: Option<&Item>,
    ) {
        let sigma = bound(sigma).max(0.0);
        let choke = bound(choke);
        if let Some((it, page)) = painted {
            self.with_blended_filters(ctx, f, reach, sigma, mix, (bp, xf), fade, |me, c, fr| {
                let pad = sigma * 4.0 + choke + off.hypot() + 4.0 * fr.px;
                let area = reach.inflate(pad, pad);
                // The colour everywhere but under the painted pixels moved by `off`, blurred, ...
                c.set_transform(fr.view);
                c.push_layer(None, None, None, None, None);
                c.push_layer(None, None, None, None, Some(blur(sigma)));
                c.set_paint(paint);
                c.fill_rect(&area);
                c.set_transform(Affine::IDENTITY);
                c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                let moved = Frame { view: fr.view * Affine::translate(off), ..*fr };
                me.draw_body(c, &moved, it, bp, xf, page);
                c.pop_layer();
                c.pop_layer();
                // ... kept only where the object paints.
                c.set_transform(Affine::IDENTITY);
                c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestIn)), None, None, None);
                me.draw_body(c, fr, it, bp, xf, page);
                c.pop_layer();
                c.pop_layer();
            });
            return;
        }
        self.with_blended_filters(ctx, f, reach, sigma, mix, (bp, xf), fade, |_, c, fr| {
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
    /// filter layers. Putting it inside the clip blends with transparent pixels instead. With
    /// `fade`, what `draw` paints is faded by that object's feathers before it blends.
    #[allow(clippy::too_many_arguments)]
    fn with_blended_filters(
        &mut self,
        ctx: &mut RenderContext,
        f: &Frame,
        reach: Rect,
        sigma: f64,
        mix: Mix,
        clip: (&BezPath, Affine),
        fade: Option<&Item>,
        draw: impl FnOnce(&mut Self, &mut RenderContext, &Frame),
    ) {
        let layered = mix != Mix::Normal || fade.is_some();
        if layered {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(BlendMode::new(mix, Compose::SrcOver)), None, None, None);
        }
        self.with_filters(ctx, f, reach, sigma, Some(clip), draw);
        if let Some(it) = fade {
            self.fade(ctx, f, it, clip.0, clip.1, reach);
        }
        if layered {
            ctx.pop_layer();
        }
    }

    /// Fade what was drawn in the current layer by `it`'s feathers: the basic feather's blurred
    /// inset shape, a ramp for each directional feather side and the gradient feather. `bp` and
    /// `xf` are the object's shape and spread transform, `reach` (spread space) what it paints.
    fn fade(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine, reach: Rect) {
        let e = &it.effects;
        // Choke keeps that much of the width opaque (a shorter fade); Sharp and Rounded corners
        // follow the inset shape more tightly than Diffused.
        if basic_feather(it) {
            let fo = &e.feather_options;
            let w = bound(e.feather) * (1.0 - fo.choke.clamp(0.0, 100.0) / 100.0).max(0.01);
            let (sigma, join) = match fo.corners {
                designcraft_doc::FeatherCorners::Diffused => (w / 4.0, kurbo::Join::Round),
                designcraft_doc::FeatherCorners::Rounded => (w / 3.0, kurbo::Join::Round),
                designcraft_doc::FeatherCorners::Sharp => (w / 6.0, kurbo::Join::Miter),
            };
            // Keep what was drawn where the blurred inset silhouette is.
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
        if e.gradient_feather.on {
            gradient_feather_mask(ctx, f, it, xf);
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
        let sigma = bound(sigma).max(0.0);
        // 3σ of the Gaussian in pixels, plus a pixel of antialiasing.
        let spread = sigma * 3.0 / f.px + 2.0;
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_huge_blur_size_reaches_the_planner_finite_and_squarable() {
        use super::{MAX_SIGMA, bound, std_dev};
        for v in [f64::MAX, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let b = bound(v);
            assert!(b.is_finite() && (b * 3.0 + 4.0 * b).is_finite(), "{v}");
        }
        assert_eq!(bound(7.5), 7.5);
        assert_eq!(super::split(f64::MAX, 50.0), (MAX_SIGMA / 2.0 * 2.0 * 0.5, 2.0 * MAX_SIGMA * 0.5));
        for s in [1.0e300, f64::MAX, f64::INFINITY] {
            let d = std_dev(s);
            assert_eq!(f64::from(d), MAX_SIGMA);
            assert!((d * d).is_finite());
        }
        assert_eq!(std_dev(-3.0), 0.0);
        assert_eq!(std_dev(f64::NAN), 0.0);
        assert_eq!(std_dev(12.5), 12.5);
    }

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

    /// An inner shadow on a frame holding an image whose left half is transparent: the shadow
    /// lines the image's opaque part (its edge in the middle of the frame), not only the frame.
    #[test]
    fn an_inner_shadow_follows_a_transparent_images_edge() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Document, SpreadRef};
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        // 20 × 20: transparent left half, opaque white right half.
        let img = image::RgbaImage::from_fn(20, 20, |x, _| if x < 10 { image::Rgba([0, 0, 0, 0]) } else { image::Rgba([255, 255, 255, 255]) });
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let aid = designcraft_doc::AssetId(d.alloc());
        d.assets.insert(
            aid,
            std::sync::Arc::new(designcraft_doc::Asset {
                id: aid,
                name: "half.png".into(),
                mime: "image/png".into(),
                data: std::sync::Arc::new(png),
                pixels: Some((20, 20)),
                ..Default::default()
            }),
        );
        let mut it = Item::new(
            designcraft_doc::ItemId(d.alloc()),
            lid,
            designcraft_doc::Shape::Rectangle,
            designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 300.0, 300.0)),
        );
        it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
            layer_options: None,
            asset: aid,
            size: (200.0, 200.0),
            xf: Affine::translate((100.0, 100.0)),
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        let s = &mut it.effects.inner_shadow;
        s.on = true;
        s.color = "[Black]".into();
        s.opacity = 1.0;
        s.distance = 0.0;
        s.size = 20.0;
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let cache = crate::Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let page = r.render_page(&d, &cache, 0, 1.0, false, &crate::RenderOptions::default()).unwrap();
        let lum = |x: u32| {
            let p = page.pixel(x, 200);
            u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])
        };
        // The image's opaque part starts at x = 200: just inside it the shadow darkens it; in its
        // middle (x = 250, 50 pt from every edge but the frame's right one) it stays light.
        assert!(lum(204) + 60 < lum(250), "shadow at the image's own edge: {} vs {}", lum(204), lum(250));
        // The transparent half shows the page: no shadow painted there.
        assert!(lum(150) > 700, "nothing over the transparent part: {}", lum(150));
    }

    /// A feathered object fades to transparent at its edge, its bevel and inner glow included: where
    /// the feather shows what is behind the object, the highlight and shadow leave it as it is.
    #[test]
    fn feathers_fade_the_inner_effects_with_the_object() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Document, Fill, SpreadRef};
        let doc = |image: bool, feather: u8, inner: u8| {
            let mut d = Document::new(&NewDocument::default());
            let lid = d.default_layer();
            let mut back = Item::new(
                designcraft_doc::ItemId(d.alloc()),
                lid,
                designcraft_doc::Shape::Rectangle,
                designcraft_geom::shapes::rectangle(Rect::new(50.0, 50.0, 350.0, 350.0)),
            );
            back.fill = Fill::swatch("C=0 M=100 Y=0 K=0");
            d.insert_item(SpreadRef::Doc(0), back, None).unwrap();
            let mut it = Item::new(
                designcraft_doc::ItemId(d.alloc()),
                lid,
                designcraft_doc::Shape::Rectangle,
                designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 300.0, 300.0)),
            );
            if image {
                let img = image::RgbaImage::from_pixel(20, 20, image::Rgba([40, 160, 90, 255]));
                let mut png = Vec::new();
                img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
                let aid = designcraft_doc::AssetId(d.alloc());
                d.assets.insert(
                    aid,
                    std::sync::Arc::new(designcraft_doc::Asset {
                        id: aid,
                        name: "green.png".into(),
                        mime: "image/png".into(),
                        data: std::sync::Arc::new(png),
                        pixels: Some((20, 20)),
                        ..Default::default()
                    }),
                );
                it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
                    layer_options: None,
                    asset: aid,
                    size: (200.0, 200.0),
                    xf: Affine::translate((100.0, 100.0)),
                    clip: None,
                    auto_fit: Default::default(),
                    fit_align: 4,
                    crop: [0.0; 4],
                    wrap: Default::default(),
                    fill: Default::default(),
                });
            } else {
                it.fill = Fill::swatch("C=100 M=0 Y=0 K=0");
            }
            let e = &mut it.effects;
            match feather {
                0 => e.feather = 30.0,
                1 => (e.directional_feather.on, e.directional_feather.widths) = (true, [30.0; 4]),
                _ => e.gradient_feather.on = true,
            }
            match inner {
                0 => {}
                1 => (e.bevel.on, e.bevel.size) = (true, 12.0),
                _ => (e.inner_glow.on, e.inner_glow.color, e.inner_glow.opacity, e.inner_glow.size) = (true, "[Paper]".into(), 1.0, 20.0),
            }
            d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
            d
        };
        let px = |d: &Document, threads: u16, x: u32| {
            let mut r = Renderer::new();
            r.threads = threads;
            r.render_page(d, &crate::Cache::new(), 0, 1.0, false, &crate::RenderOptions::default()).unwrap().pixel(x, 200)
        };
        let diff = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).map(|(p, q)| u32::from(p.abs_diff(q))).sum::<u32>();
        for image in [false, true] {
            for feather in 0..3 {
                for threads in [0, 4] {
                    let plain = doc(image, feather, 0);
                    // The object's faded edges: the left and right ones for the basic and directional
                    // feathers, the right one for the gradient feather (opaque on the left).
                    let edges: &[u32] = if feather == 2 { &[299] } else { &[100, 299] };
                    for &x in edges {
                        let behind = px(&plain, threads, 70);
                        let faded = px(&plain, threads, x);
                        assert!(diff(faded, behind) < 40, "image {image} feather {feather}: the edge at {x} shows what is behind");
                        for inner in [1, 2] {
                            let with = px(&doc(image, feather, inner), threads, x);
                            assert!(
                                diff(with, faded) < 30,
                                "image {image} feather {feather} inner {inner} threads {threads}: {with:?} over the faded edge at {x}, {faded:?} without"
                            );
                        }
                    }
                    // Inside the bevel and glow bands the effect still shows, faded with the object
                    // where the feather reaches.
                    let inside = px(&plain, threads, 110);
                    for inner in [1, 2] {
                        let with = px(&doc(image, feather, inner), threads, 110);
                        assert!(
                            diff(with, inside) > 0,
                            "image {image} feather {feather} inner {inner} threads {threads}: {with:?} at 110, {inside:?} without the effect"
                        );
                    }
                }
            }
        }
    }
}
