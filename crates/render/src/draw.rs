use super::color::{HIDDEN_LAYERS, blend_mode, color_of, overprints, rect_overlaps, screen_color, set_fill_paint, spread_blends};
use super::types::{DisplayQuality, Frame};
use super::*;

/// An item's fill under its content; an overprinting fill multiplies into what is beneath.
fn draw_body_fill(ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine) {
    if it.fill.is_none() {
        return;
    }
    let doc = f.doc;
    ctx.set_transform(f.view * xf);
    let op = overprints(f, &it.fill.swatch, it.fill.tint, it.fill.overprint);
    if op {
        ctx.push_layer(None, Some(BlendMode::new(Mix::Multiply, Compose::SrcOver)), None, None, None);
    }
    if set_fill_paint(ctx, doc, &it.fill, bp.bounding_box()) {
        ctx.fill_path(bp);
    }
    ctx.reset_paint_transform();
    if op {
        ctx.pop_layer();
    }
}

impl Renderer {
    pub(crate) fn draw_spread(&mut self, ctx: &mut RenderContext, f: &Frame, sr: SpreadRef) {
        let doc = f.doc;
        let Some(sp) = doc.spread(sr) else { return };
        // Paper.
        if f.opts.paper && self.slice_includes_surrounding(f.opts) {
            let paper = doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).unwrap_or(designcraft_color::Color::WHITE);
            if f.opts.page_shadow {
                ctx.set_transform(f.view);
                ctx.set_paint(peniko::Color::from_rgba8(0, 0, 0, 90));
                for p in &sp.pages {
                    let s = 3.0 * f.px;
                    ctx.fill_rect(&(p.bounds() + Vec2::new(s, s)));
                }
            }
            ctx.set_transform(f.view);
            ctx.set_paint(color_of(&paper, 1.0));
            // One rect for equal-height spreads avoids an anti-aliasing seam at the spine.
            if sp.pages.windows(2).all(|w| w[0].height == w[1].height) {
                ctx.fill_rect(&sp.bounds());
            } else {
                for p in &sp.pages {
                    ctx.fill_rect(&p.bounds());
                }
            }
        }
        let grid_area = f.opts.document_grid.iter().find(|(r, _)| *r == sr).map(|(_, a)| *a);
        let grid = &doc.settings.grid;
        if let Some(area) = grid_area.filter(|_| grid.in_back && self.slice_includes_surrounding(f.opts)) {
            grid::draw(ctx, f, sp, grid, area);
        }
        if blendspace::uses_cmyk(doc, sr, f.opts) {
            self.draw_cmyk_spread(ctx, f, sr);
            if let Some(area) = grid_area.filter(|_| !grid.in_back && self.slice_includes_surrounding(f.opts)) {
                grid::draw(ctx, f, sp, grid, area);
            }
            return;
        }
        // Blend modes blend with the objects below, not with the paper (as in InDesign and in PDF,
        // where the page's group starts transparent): a Color Dodge or Screen object over blank paper
        // shows as it is. So the items are drawn in their own group, laid over the paper once.
        let isolate = f.opts.paper && spread_blends(doc, sp);
        if isolate {
            ctx.push_layer(None, None, None, None, None);
        }
        // Layers back to front; within each layer parent items first.
        for layer in doc.layers.iter().rev() {
            if !layer.visible || f.opts.printing_only && !layer.printable {
                continue;
            }
            // Document pages show their parent and its bases; a parent spread shows its bases.
            let first = match sr {
                SpreadRef::Doc(si) => Some(doc.first_page_of_spread(si)),
                SpreadRef::Parent(_) => None,
            };
            // Base items compose for where they show: the document page, or this parent spread.
            let shown_on = |pi: usize| match (sr, first) {
                (SpreadRef::Parent(p), _) => ShownOn::Parent(p),
                (_, Some(a)) => ShownOn::Page(a + pi),
                (_, None) => ShownOn::Own,
            };
            for (pi, page) in sp.pages.iter().enumerate() {
                if !page.show_parent_items {
                    continue;
                }
                for (ppi, ppage) in doc.shown_parent_pages(sr, pi) {
                    let Some(parent) = doc.parents.get(ppi) else { continue };
                    let Some(ppg) = parent.pages.get(ppage) else { continue };
                    let dx = page.x - ppg.x;
                    for it in &parent.items {
                        if it.layer != layer.id || page.overridden.contains(&it.id) {
                            continue;
                        }
                        // Parent items belong to the parent page they sit on.
                        if parent.page_at_x(it.bounds().center().x) != Some(ppage) && parent.pages.len() > 1 {
                            continue;
                        }
                        self.draw_root(ctx, f, it, Affine::translate((dx, 0.0)), shown_on(pi));
                    }
                }
            }
            for it in &sp.items {
                if it.layer == layer.id {
                    self.draw_root(ctx, f, it, Affine::IDENTITY, ShownOn::Own);
                }
            }
        }
        if isolate {
            ctx.pop_layer();
        }
        if let Some(area) = grid_area.filter(|_| !grid.in_back && self.slice_includes_surrounding(f.opts)) {
            grid::draw(ctx, f, sp, grid, area);
        }
    }

    /// Paper and grids stay in the surrounding slice at their position in paint order.
    /// They never move with the selected item.
    fn slice_includes_surrounding(&self, opts: &RenderOptions) -> bool {
        opts.slice.is_none_or(|slice| match slice.part {
            SlicePart::Before => !self.slice_seen,
            SlicePart::Item => false,
            SlicePart::After => self.slice_seen,
        })
    }

    fn draw_root(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, parent: Affine, page: ShownOn) {
        let draw = f.opts.slice.is_none_or(|s| {
            let hit = s.item == it.id;
            let was_seen = self.slice_seen;
            self.slice_seen |= hit;
            match s.part {
                SlicePart::Before => !was_seen && !hit,
                SlicePart::Item => hit,
                SlicePart::After => was_seen && !hit,
            }
        });
        if draw && !f.opts.cancelled() {
            self.draw_item(ctx, f, it, parent, page);
        }
    }

    pub(crate) fn draw_item(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, parent: Affine, page: ShownOn) {
        if self.drawing_items.len() >= 64 || self.drawing_items.contains(&it.id) {
            return;
        }
        self.drawing_items.push(it.id);
        self.draw_item_inner(ctx, f, it, parent, page);
        self.drawing_items.pop();
    }

    /// A group: children carry their own transforms relative to the group.
    fn draw_group(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, xf: Affine, page: ShownOn) {
        // Isolate Blending / Knockout: the group is its own layer (blend modes stop at it).
        let layered = it.opacity < 0.999 || it.blend != DcBlend::Normal || it.isolate || it.knockout;
        if layered {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(blend_mode(it.blend)), Some(it.opacity), None, None);
        }
        // Gradient feather: the group's objects drawn together, then faded as one.
        let feather = it.effects.gradient_feather.on && f.opts.quality != DisplayQuality::Fast;
        if feather {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, None, None, None, None);
        }
        if it.knockout {
            // Knockout: each object first clears its area of those behind it, so it composites
            // with the group's backdrop rather than with its siblings.
            for c in it.shown_children() {
                if !c.hidden {
                    ctx.set_transform(Affine::IDENTITY);
                    ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                    ctx.set_transform(f.view * xf * c.xf);
                    ctx.set_paint(peniko::Color::BLACK);
                    ctx.fill_path(&c.path.to_bezpath());
                    ctx.pop_layer();
                }
                self.draw_item(ctx, f, c, xf, page);
            }
        } else {
            for c in it.shown_children() {
                self.draw_item(ctx, f, c, xf, page);
            }
        }
        if feather {
            fx::gradient_feather_mask(ctx, f, it, xf);
            ctx.pop_layer();
        }
        if layered {
            ctx.pop_layer();
        }
    }

    fn draw_item_inner(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, parent: Affine, page: ShownOn) {
        if f.opts.cancelled() || it.hidden || f.opts.hidden.contains(&it.id) || f.opts.printing_only && it.nonprinting {
            return;
        }
        let xf = parent * it.xf;
        let vb = xf.transform_rect_bbox(it.inner_bounds()).inflate(it.stroke.extent() + 2.0, it.stroke.extent() + 2.0);
        // A frame with items pasted into it draws like any other object (draw_body: fill, the items
        // clipped to the frame, stroke), so its effects apply to all of it.
        if !it.children().is_empty() && !it.has_nested_items() {
            self.draw_group(ctx, f, it, xf, page);
            return;
        }
        // Composed text stays inside its frame apart from glyph overhang (italics, swashes, big
        // initials), so text frames get a margin rather than being exempt from culling.
        let margin = match it.content {
            Content::Text(_) => 36.0 + 0.25 * vb.width().max(vb.height()),
            _ => 0.0,
        };
        let effects = it.effects.any() && f.opts.quality != DisplayQuality::Fast;
        let fx_outset = if effects { fx::outset(it) } else { 0.0 };
        if !rect_overlaps(vb.inflate(margin + fx_outset, margin + fx_outset), f.visible) {
            return;
        }
        self.stats.items += 1;
        let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
        let layered = it.opacity < 0.999 || it.blend != DcBlend::Normal;
        if layered {
            ctx.set_transform(Affine::IDENTITY);
            ctx.push_layer(None, Some(blend_mode(it.blend)), Some(it.opacity), None, None);
        }
        if effects {
            self.draw_item_fx(ctx, f, it, &bp, xf, page);
        } else {
            self.draw_body(ctx, f, it, &bp, xf, page);
        }
        if layered {
            ctx.pop_layer();
        }
    }

    /// The composed story for a frame (memoised for the current render: the cache lookup
    /// validates the whole thread each time).
    fn story(&mut self, f: &Frame, sid: StoryId, page: ShownOn) -> Arc<ComposedStory> {
        let key = (sid, page);
        if let Some(cs) = self.stories.get(&key) {
            return cs.clone();
        }
        let cs = f.cache.get_on(f.doc, sid, page);
        self.stories.insert(key, cs.clone());
        cs
    }

    /// Fill, content and stroke of a leaf item (no opacity, blend or effects).
    pub(crate) fn draw_body(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine, page: ShownOn) {
        draw_body_fill(ctx, f, it, bp, xf);
        // Content.
        match &it.content {
            Content::Graphic(g) => {
                let g = &if it.media.is_some() { it.drawn_graphic(&f.doc.assets).unwrap_or_else(|| g.clone()) } else { g.clone() };
                ctx.set_transform(f.view * xf);
                ctx.push_clip_layer(bp);
                let hidden = it.has_pdf_layer_override().then(|| it.pdf_hidden_layers.clone());
                let previous = HIDDEN_LAYERS.with(|h| h.replace(hidden));
                self.draw_graphic(ctx, f, g, xf);
                HIDDEN_LAYERS.with(|h| h.replace(previous));
                ctx.pop_layer();
            }
            // Items pasted into the frame, clipped by it.
            Content::Group { items } if it.has_nested_items() => {
                ctx.set_transform(f.view * xf);
                ctx.push_clip_layer(bp);
                for c in items {
                    self.draw_item(ctx, f, c, xf, page);
                }
                ctx.pop_layer();
            }
            Content::Text(tf) => {
                let cs = self.story(f, tf.story, page);
                if let Some(ft) = cs.frame(it.id) {
                    match &tf.options.path {
                        Some(pt) => self.draw_path_text(ctx, f, &cs, ft, xf, bp, pt),
                        None => self.draw_text(ctx, f, &cs, ft, xf * f.doc.text_local(it)),
                    }
                }
            }
            _ => {}
        }
        // Form fields (screen only; PDF viewers draw their own): a tint, a check mark.
        if let Some(ff) = &it.form_field
            && !f.opts.printing_only
        {
            let r = it.inner_bounds();
            ctx.set_transform(f.view * xf);
            ctx.set_paint(screen_color([80, 140, 255], 40));
            ctx.fill_rect(&r);
            ctx.set_paint(screen_color([60, 110, 220], 200));
            ctx.set_stroke(kurbo::Stroke::new(0.75));
            ctx.stroke_path(&r.to_path(0.1));
            if ff.kind == designcraft_doc::FieldKind::CheckBox && !ff.value.is_empty() && ff.value != "Off" {
                let (w, h) = (r.width(), r.height());
                let mut c = BezPath::new();
                c.move_to((r.x0 + w * 0.22, r.y0 + h * 0.48));
                c.line_to((r.x0 + w * 0.42, r.y0 + h * 0.72));
                c.line_to((r.x0 + w * 0.78, r.y0 + h * 0.24));
                ctx.set_paint(screen_color([0, 0, 0], 255));
                ctx.set_stroke(kurbo::Stroke::new((w.min(h) * 0.12).max(0.8)).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
                ctx.stroke_path(&c);
            }
        }
        // Stroke.
        if !it.stroke.is_none() {
            let op = overprints(f, &it.stroke.swatch, it.stroke.tint, it.stroke.overprint);
            if op {
                ctx.set_transform(f.view * xf);
                ctx.push_layer(None, Some(BlendMode::new(Mix::Multiply, Compose::SrcOver)), None, None, None);
            }
            self.draw_stroke(ctx, f, it, bp, xf);
            if op {
                ctx.pop_layer();
            }
        }
    }
}
