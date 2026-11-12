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
//!   shape, blurred and clipped) with screen / multiply blending; satin shows where two offset
//!   copies' coverages differ.
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
            let w = bound(e.feather) * (1.0 - fo.choke.clamp(0.0, 100.0) / 100.0).max(0.01);
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
            if g.center
                && let Some((it, page)) = painted
            {
                // With a graphic, the glow fills what the object paints, shrunk by the band: the
                // object's coverage is drawn once offscreen and eroded by the band there.
                let band = (bound(g.size) - choke).max(0.5);
                self.with_blended_filters(ctx, f, reach, sigma, mix, (bp, xf), |me, c, fr| {
                    let spread = (sigma * 3.0 + band) / fr.px + 2.0;
                    let Some(o) = mask_offscreen(fr.view.transform_rect_bbox(reach), spread, c.width(), c.height()) else { return };
                    let mut off = RenderContext::new_with(o.width, o.height, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
                    let view = Affine::scale(1.0 / o.scale) * Affine::translate((-o.x0, -o.y0)) * fr.view;
                    me.draw_body(&mut off, &Frame { mt: false, view, px: fr.px * o.scale, ..*fr }, it, bp, xf, page);
                    off.flush();
                    let mut pm = vello_cpu::Pixmap::new(o.width, o.height);
                    off.render(&mut pm, &mut me.resources);
                    let mask = erode(&pm, band / (fr.px * o.scale));
                    // The eroded coverage in the glow color, ...
                    c.set_transform(fr.view);
                    c.push_layer(None, None, None, None, None);
                    c.push_layer(None, None, None, None, Some(blur(sigma)));
                    c.set_transform(Affine::translate((o.x0, o.y0)) * Affine::scale(o.scale));
                    c.set_paint(vello_cpu::Image {
                        image: vello_cpu::ImageSource::Pixmap(std::sync::Arc::new(mask)),
                        sampler: peniko::ImageSampler::default(),
                    });
                    c.fill_rect(&Rect::new(0.0, 0.0, f64::from(o.width), f64::from(o.height)));
                    let pad = sigma * 4.0 + band + 4.0 * fr.px;
                    c.set_transform(Affine::IDENTITY);
                    c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::SrcIn)), None, None, None);
                    c.set_transform(fr.view);
                    c.set_paint(paint);
                    c.fill_rect(&reach.inflate(pad, pad));
                    c.pop_layer();
                    c.pop_layer();
                    // ... blurred and kept only where the object paints.
                    c.set_transform(Affine::IDENTITY);
                    c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestIn)), None, None, None);
                    me.draw_body(c, fr, it, bp, xf, page);
                    c.pop_layer();
                    c.pop_layer();
                });
            } else if g.center {
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
                    c.set_stroke(kurbo::Stroke::new((bound(g.size) - choke).max(0.5) * 2.0).with_join(kurbo::Join::Round));
                    c.stroke_path(bp);
                    c.pop_layer();
                    c.pop_layer();
                });
            } else {
                self.inner_edge(ctx, f, reach, bp, xf, Vec2::ZERO, sigma, choke, paint, mix, painted);
            }
        }
        // Satin: the shape shifted both ways along the angle, blurred, where exactly one covers.
        // With a graphic, the shape is what the object paints, as for the inner shadow. Plain
        // satin on a graphic with no distance compares two equal copies and is not drawn.
        if e.satin.on
            && !(painted.is_some() && !e.satin.invert && e.satin.distance == 0.0)
            && let Some(c) = doc.resolve_color(&e.satin.color, 1.0)
        {
            let st = &e.satin;
            let off = offset(st.angle, st.distance / 2.0);
            let sigma = bound(st.size).max(0.0) / 2.0;
            let paint = color_of(&c, st.opacity.clamp(0.0, 1.0));
            let invert = st.invert;
            if let Some((it, page)) = painted {
                // With a graphic, the object's coverage is drawn once offscreen and the two shifted
                // copies are compared there, pixel by pixel, so a partly transparent pixel that
                // both copies cover equally gets no satin.
                self.with_blended_filters(ctx, f, reach, sigma, Mix::Multiply, (bp, xf), |me, c, fr| {
                    // The copies' shift on the device.
                    let shift = fr.view * off.to_point() - fr.view * kurbo::Point::ZERO;
                    let spread = (sigma * 3.0) / fr.px + shift.hypot() + 2.0;
                    let Some(o) = mask_offscreen(fr.view.transform_rect_bbox(reach), spread, c.width(), c.height()) else { return };
                    let mut off_ctx = RenderContext::new_with(o.width, o.height, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
                    let view = Affine::scale(1.0 / o.scale) * Affine::translate((-o.x0, -o.y0)) * fr.view;
                    me.draw_body(&mut off_ctx, &Frame { mt: false, view, px: fr.px * o.scale, ..*fr }, it, bp, xf, page);
                    off_ctx.flush();
                    let mut pm = vello_cpu::Pixmap::new(o.width, o.height);
                    off_ctx.render(&mut pm, &mut me.resources);
                    let mask = satin_mask(&pm, shift / o.scale, invert);
                    // The satin colour where the copies differ, ...
                    c.set_transform(fr.view);
                    c.push_layer(None, None, None, None, None);
                    c.push_layer(None, None, None, None, Some(blur(sigma)));
                    c.set_transform(Affine::translate((o.x0, o.y0)) * Affine::scale(o.scale));
                    c.set_paint(vello_cpu::Image {
                        image: vello_cpu::ImageSource::Pixmap(std::sync::Arc::new(mask)),
                        sampler: peniko::ImageSampler::default(),
                    });
                    c.fill_rect(&Rect::new(0.0, 0.0, f64::from(o.width), f64::from(o.height)));
                    let pad = sigma * 4.0 + off.hypot() + 4.0 * fr.px;
                    c.set_transform(Affine::IDENTITY);
                    c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::SrcIn)), None, None, None);
                    c.set_transform(fr.view);
                    c.set_paint(paint);
                    c.fill_rect(&reach.inflate(pad, pad));
                    c.pop_layer();
                    c.pop_layer();
                    // ... blurred and kept only where the object paints.
                    c.set_transform(Affine::IDENTITY);
                    c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestIn)), None, None, None);
                    me.draw_body(c, fr, it, bp, xf, page);
                    c.pop_layer();
                    c.pop_layer();
                });
            } else {
                self.with_blended_filters(ctx, f, reach, sigma, Mix::Multiply, (bp, xf), |_, c, fr| {
                    // One copy of the shape in the satin colour, moved by `by`.
                    let copy = |c: &mut RenderContext, by: Vec2| {
                        c.set_transform(fr.view * Affine::translate(by) * xf);
                        c.set_paint(paint);
                        c.fill_path(bp);
                    };
                    c.set_transform(fr.view);
                    c.push_layer(None, None, None, None, Some(blur(sigma)));
                    if invert {
                        copy(c, Vec2::ZERO);
                    }
                    c.set_transform(Affine::IDENTITY);
                    c.push_layer(None, Some(BlendMode::new(Mix::Normal, if invert { Compose::DestOut } else { Compose::SrcOver })), None, None, None);
                    copy(c, off);
                    c.set_transform(Affine::IDENTITY);
                    c.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::Xor)), None, None, None);
                    copy(c, -off);
                    c.pop_layer();
                    c.pop_layer();
                    c.pop_layer();
                });
            }
        }
        // Bevel and Emboss (inner bevel): highlight from the lit side, shadow from the other.
        if e.bevel.on {
            let b = &e.bevel;
            let off = offset(doc.light_angle(b.angle, b.global_light), bound(b.size) * (b.depth / 100.0).clamp(0.01, 10.0) * 0.5);
            let sigma = bound(b.size).max(0.0) / 2.0;
            if let Some(c) = doc.resolve_color(&b.highlight, 1.0) {
                // The inverse shape moved away from the light lights the edges facing it.
                self.inner_edge(ctx, f, reach, bp, xf, off, sigma, 0.0, color_of(&c, b.highlight_opacity.clamp(0.0, 1.0)), Mix::Screen, painted);
            }
            if let Some(c) = doc.resolve_color(&b.shadow, 1.0) {
                self.inner_edge(ctx, f, reach, bp, xf, -off, sigma, 0.0, color_of(&c, b.shadow_opacity.clamp(0.0, 1.0)), Mix::Multiply, painted);
            }
        }
    }

    /// An inner shadow-like edge: everything outside the shape moved by `off`, blurred by
    /// `sigma`, clipped to the shape and blended with `mix`. With `painted` (an object and the
    /// page it shows on), the shape is what the object paints, so a transparent part of an image
    /// is outside it: the edge runs along the image's opaque pixels, not the frame.
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
    ) {
        let sigma = bound(sigma).max(0.0);
        let choke = bound(choke);
        if let Some((it, page)) = painted {
            self.with_blended_filters(ctx, f, reach, sigma, mix, (bp, xf), |me, c, fr| {
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

/// The most pixels a coverage mask drawn offscreen ([`mask_offscreen`]) holds: 4096 × 4096.
const MAX_MASK_PIXELS: f64 = 4096.0 * 4096.0;

/// The offscreen image an object's coverage is drawn in to build an effect's mask: `reach` (device
/// pixels) widened by `spread`, within the window widened the same way, as [`offscreen`] finds it,
/// at a resolution low enough that the whole area fits within [`crate::MAX_SIDE`] a side and
/// [`MAX_MASK_PIXELS`] in all. `None` when none of it shows.
fn mask_offscreen(reach: Rect, spread: f64, width: u16, height: u16) -> Option<Offscreen> {
    let screen = Rect::new(0.0, 0.0, f64::from(width), f64::from(height)).inflate(spread, spread);
    let r = reach.inflate(spread, spread).intersect(screen);
    let (x0, y0) = (r.x0.floor(), r.y0.floor());
    let (w, h) = (r.x1.ceil() - x0, r.y1.ceil() - y0);
    if !(w >= 1.0 && h >= 1.0) {
        return None;
    }
    let scale = (spread / MAX_BLUR_SPREAD).max(1.0).max(w.max(h) / crate::MAX_SIDE as f64).max((w * h / MAX_MASK_PIXELS).sqrt());
    let side = |len: f64| (len / scale).ceil().clamp(1.0, crate::MAX_SIDE as f64);
    Some(Offscreen { x0, y0, scale, width: side(w) as u16, height: side(h) as u16 })
}

/// `pm`'s coverage shrunk by `r` pixels: each pixel takes the smallest alpha among itself and the
/// points on circles of `r` and `r / 2` around it, as white premultiplied by that alpha. Pixels
/// outside `pm` count as transparent.
fn erode(pm: &vello_cpu::Pixmap, r: f64) -> vello_cpu::Pixmap {
    let (w, h) = (i64::from(pm.width()), i64::from(pm.height()));
    let src = pm.data();
    let mut taps = vec![(0_i64, 0_i64)];
    for k in 0..16_u8 {
        let d = if k % 2 == 0 { r } else { r / 2.0 };
        let a = f64::from(k) * std::f64::consts::PI / 8.0;
        taps.push(((a.cos() * d).round() as i64, (a.sin() * d).round() as i64));
    }
    taps.dedup();
    let alpha = |x: i64, y: i64| if x < 0 || y < 0 || x >= w || y >= h { 0 } else { src[(y * w + x) as usize].a };
    let mut data = Vec::with_capacity(src.len());
    for y in 0..h {
        for x in 0..w {
            let a = taps.iter().map(|&(dx, dy)| alpha(x.saturating_add(dx), y.saturating_add(dy))).min().unwrap_or(0);
            data.push(vello_cpu::color::PremulRgba8 { r: a, g: a, b: a, a });
        }
    }
    vello_cpu::Pixmap::from_parts(data, pm.width(), pm.height())
}

/// The satin coverage of `pm`'s two copies moved by `shift` and `-shift` pixels: at each pixel,
/// how much the copies' coverages differ, or with `invert`, `pm`'s own coverage where they
/// agree, as white premultiplied by that amount. Copies are sampled bilinearly; pixels outside
/// `pm` count as transparent.
fn satin_mask(pm: &vello_cpu::Pixmap, shift: Vec2, invert: bool) -> vello_cpu::Pixmap {
    let (w, h) = (i64::from(pm.width()), i64::from(pm.height()));
    let src = pm.data();
    let alpha = |x: i64, y: i64| if x < 0 || y < 0 || x >= w || y >= h { 0.0 } else { f64::from(src[(y * w + x) as usize].a) / 255.0 };
    let sample = |x: f64, y: f64| {
        let (fx, fy) = (x.floor(), y.floor());
        let (tx, ty) = (x - fx, y - fy);
        let (ix, iy) = (fx as i64, fy as i64);
        let top = alpha(ix, iy) * (1.0 - tx) + alpha(ix.saturating_add(1), iy) * tx;
        let bottom = alpha(ix, iy.saturating_add(1)) * (1.0 - tx) + alpha(ix.saturating_add(1), iy.saturating_add(1)) * tx;
        top * (1.0 - ty) + bottom * ty
    };
    let mut data = Vec::with_capacity(src.len());
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f64, y as f64);
            let diff = (sample(px - shift.x, py - shift.y) - sample(px + shift.x, py + shift.y)).abs();
            let v = if invert { alpha(x, y) * (1.0 - diff) } else { diff };
            let a = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            data.push(vello_cpu::color::PremulRgba8 { r: a, g: a, b: a, a });
        }
    }
    vello_cpu::Pixmap::from_parts(data, pm.width(), pm.height())
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

    /// A coverage mask covers the whole widened area at a lower resolution when it is wider than
    /// `MAX_SIDE` or holds more than `MAX_MASK_PIXELS`; a small one stays at full resolution.
    #[test]
    fn a_coverage_mask_covers_its_whole_area_within_its_pixel_budget() {
        let small = Offscreen { x0: 140.0, y0: 140.0, scale: 1.0, width: 220, height: 220 };
        assert_eq!(mask_offscreen(Rect::new(150.0, 150.0, 350.0, 350.0), 10.0, 1440, 900), Some(small));
        for (reach, w, h) in [(Rect::new(0.0, 0.0, 65_535.0, 100.0), u16::MAX, 100), (Rect::new(0.0, 0.0, 20_000.0, 20_000.0), 20_000, 20_000)] {
            let o = mask_offscreen(reach, 10.0, w, h).unwrap();
            assert_eq!((o.x0, o.y0), (-10.0, -10.0));
            assert!(o.x0 + f64::from(o.width) * o.scale >= reach.x1 + 10.0, "{o:?}");
            assert!(o.y0 + f64::from(o.height) * o.scale >= reach.y1 + 10.0, "{o:?}");
            assert!(f64::from(o.width) * f64::from(o.height) <= MAX_MASK_PIXELS + f64::from(o.width) + f64::from(o.height), "{o:?}");
        }
    }

    /// An inner shadow on a frame holding an image whose left half is transparent: the shadow
    /// lines the image's opaque part (its edge in the middle of the frame), not only the frame.
    #[test]
    fn an_inner_shadow_follows_a_transparent_images_edge() {
        let (d, mut it) = half_image_frame();
        let s = &mut it.effects.inner_shadow;
        s.on = true;
        s.color = "[Black]".into();
        s.opacity = 1.0;
        s.distance = 0.0;
        s.size = 20.0;
        let lum = render_row(d, it);
        // The image's opaque part starts at x = 200: just inside it the shadow darkens it; in its
        // middle (x = 250, 50 pt from every edge but the frame's right one) it stays light.
        assert!(lum(204) + 60 < lum(250), "shadow at the image's own edge: {} vs {}", lum(204), lum(250));
        // The transparent half shows the page: no shadow painted there.
        assert!(lum(150) > 700, "nothing over the transparent part: {}", lum(150));
    }

    /// Satin on a frame holding an image whose left half is transparent multiplies only over the
    /// image's opaque part, plain or inverted: the page shows through the transparent half.
    #[test]
    fn satin_follows_a_transparent_images_edge() {
        for invert in [false, true] {
            let (d, mut it) = half_image_frame();
            let s = &mut it.effects.satin;
            s.on = true;
            s.color = "[Black]".into();
            s.opacity = 1.0;
            s.angle = 0.0;
            s.distance = 100.0;
            s.size = 4.0;
            s.invert = invert;
            let lum = render_row(d, it);
            // The copies are 50 pt to each side. Over the frame they leave the satin on 100 to 150
            // (plain) or 150 to 250 (inverted), over the transparent half of the image.
            let clear = if invert { 175 } else { 125 };
            assert!(lum(clear) > 700, "invert {invert}: satin over the transparent part at x = {clear}: {}", lum(clear));
            // Over the image's opaque part the copies (150 to 250 and 250 to 350) leave plain satin on
            // all of it and inverted satin on none of it.
            let shaded = lum(225) < 100;
            assert_eq!(shaded, !invert, "invert {invert}: lum at x = 225 is {}", lum(225));
        }
    }

    /// Satin on a frame holding a uniformly half-transparent image: where both copies cover the
    /// same partial coverage (all of the image with a distance of 0, its middle with a short one),
    /// plain satin leaves the image as it is; inverted satin shades it.
    #[test]
    fn satin_leaves_a_uniform_half_transparent_image_plain() {
        for (distance, invert) in [(0.0, false), (40.0, false), (0.0, true)] {
            let (d, mut it) = image_frame(|_| image::Rgba([255, 255, 255, 128]));
            let s = &mut it.effects.satin;
            s.on = true;
            s.color = "[Black]".into();
            s.opacity = 1.0;
            s.angle = 0.0;
            s.distance = distance;
            s.size = 4.0;
            s.invert = invert;
            let lum = render_row(d, it);
            // The image is white over a white page; satin multiplies black over it.
            let shaded = lum(200) < 700;
            assert_eq!(shaded, invert, "distance {distance}, invert {invert}: lum at x = 200 is {}", lum(200));
        }
    }

    /// The satin mask is the difference of the shifted coverages: none where both copies cover the
    /// same half coverage, half along an edge only one copy reaches, and the inverse keeps the
    /// pixmap's own coverage where they agree.
    #[test]
    fn the_satin_mask_compares_partial_coverage() {
        let half = vello_cpu::color::PremulRgba8 { r: 128, g: 128, b: 128, a: 128 };
        let pm = vello_cpu::Pixmap::from_parts(vec![half; 40 * 40], 40, 40);
        let plain = satin_mask(&pm, Vec2::new(3.0, 0.0), false);
        assert_eq!(plain.sample(20, 20).a, 0);
        assert_eq!(plain.sample(1, 20).a, 128);
        let inverted = satin_mask(&pm, Vec2::new(3.0, 0.0), true);
        assert_eq!(inverted.sample(20, 20).a, 128);
        assert_eq!(inverted.sample(1, 20).a, 64);
    }

    /// An inner glow from the center on a frame holding an image whose left half is transparent
    /// fills only the image's opaque part, fading toward that part's own edge.
    #[test]
    fn a_center_inner_glow_follows_a_transparent_images_edge() {
        let (d, mut it) = half_image_frame();
        let g = &mut it.effects.inner_glow;
        g.on = true;
        g.center = true;
        g.color = "[Black]".into();
        g.opacity = 1.0;
        g.size = 20.0;
        g.mode = Default::default();
        let lum = render_row(d, it);
        // The transparent half shows the page: no glow painted there.
        assert!(lum(150) > 700, "nothing over the transparent part: {}", lum(150));
        // The opaque part runs from x = 200 to 300: dark in its middle, light at its own left edge.
        assert!(lum(250) + 60 < lum(204), "glow fades toward the image's own edge: {} vs {}", lum(250), lum(204));
    }

    /// Eroding keeps a half-transparent area at half coverage away from its edge and clears the
    /// band along the edge.
    #[test]
    fn erosion_keeps_partial_coverage_and_clears_the_edge_band() {
        let half = vello_cpu::color::PremulRgba8 { r: 128, g: 128, b: 128, a: 128 };
        let pm = vello_cpu::Pixmap::from_parts(vec![half; 40 * 40], 40, 40);
        let out = erode(&pm, 5.0);
        assert_eq!(out.sample(20, 20).a, 128);
        assert_eq!(out.sample(2, 20).a, 0);
        assert_eq!(out.sample(20, 37).a, 0);
        assert_eq!(out.sample(6, 20).a, 128);
    }

    /// A 200 pt frame at (100, 100) holding a 20 × 20 image with a transparent left half and an
    /// opaque white right half, in a new document.
    fn half_image_frame() -> (designcraft_doc::Document, Item) {
        image_frame(|x| if x < 10 { image::Rgba([0, 0, 0, 0]) } else { image::Rgba([255, 255, 255, 255]) })
    }

    /// A 200 pt frame at (100, 100) holding a 20 × 20 image whose pixels in column x are
    /// `pixel(x)`, in a new document.
    fn image_frame(pixel: impl Fn(u32) -> image::Rgba<u8>) -> (designcraft_doc::Document, Item) {
        use designcraft_doc::Document;
        use designcraft_doc::build::NewDocument;
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let img = image::RgbaImage::from_fn(20, 20, |x, _| pixel(x));
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
        (d, it)
    }

    /// The first page with `it` added, rendered at 100%: the sum of R, G and B at x on row 200.
    fn render_row(mut d: designcraft_doc::Document, it: Item) -> impl Fn(u32) -> u32 {
        d.insert_item(designcraft_doc::SpreadRef::Doc(0), it, None).unwrap();
        let cache = crate::Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let page = r.render_page(&d, &cache, 0, 1.0, false, &crate::RenderOptions::default()).unwrap();
        move |x: u32| {
            let p = page.pixel(x, 200);
            u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])
        }
    }
}
