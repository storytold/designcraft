use super::color::{PLATE, RICH_BLACK, blend_space_view, rect_overlaps};
use super::decode::now;
use super::types::{Frame, FrameStats, MAX_PAGE_PIXELS, MAX_SIDE, Placed, RenderOptions, Rendered, default_threads};
use super::{
    Affine, Cache, Document, HashMap, PageSide, Rect, RenderContext, Renderer, Resources, SpreadRef, Vec2, blendspace, kurbo, peniko, text, vello_cpu,
};

impl Renderer {
    pub fn new() -> Self {
        Self {
            ctx: None,
            resources: Resources::new(),
            glyphs: text::GlyphCache::default(),
            stories: HashMap::new(),
            drawing_items: Vec::new(),
            cmyk_view: None,
            cmyk_ran: false,
            #[cfg(test)]
            cmyk_passes: 0,
            #[cfg(test)]
            cmyk_pass_pixels: 0,
            #[cfg(test)]
            cmyk_pass_items: 0,
            threads: default_threads(),
            stats: FrameStats::default(),
        }
    }

    /// Drop cached glyph paths and the CMYK viewport (rebuilt on demand).
    pub fn clear_caches(&mut self) {
        self.cmyk_view = None;
        self.glyphs.clear();
        self.stories.clear();
    }

    /// Number of text frames whose glyph paths are cached.
    pub fn cached_frames(&self) -> usize {
        self.glyphs.len()
    }

    /// Render the given spreads into a `width`×`height` image with `view` (canvas → pixels).
    pub fn render(&mut self, doc: &Document, cache: &Cache, spreads: &[Placed], size: (u32, u32), view: Affine, opts: &RenderOptions) -> Rendered {
        let start = now();
        let (width, height) = size;
        RICH_BLACK.with(|r| r.set(opts.rich_black));
        PLATE.with(|p| p.set(opts.plate));
        if let Some(hit) = self.cmyk_view.as_ref().filter(|entry| entry.matches(doc, spreads, width, height, view, opts, self.threads)) {
            let image = hit.image();
            self.stats = hit.stats();
            self.stats.micros = now().saturating_sub(start);
            return image;
        }
        // Release the old snapshot before rendering an edited document or resized viewport.
        self.cmyk_view = None;
        self.cmyk_ran = false;
        let image = self.render_uncached(doc, cache, spreads, size, view, opts);
        // Kept only when the ink passes ran: other views render at their usual cost anyway.
        if self.cmyk_ran && image.pixels.len() <= blendspace::CachedView::MAX_BYTES {
            self.cmyk_view = Some(blendspace::CachedView::new(doc, spreads, view, opts, &image, self.stats, self.threads));
        }
        self.stats.micros = now().saturating_sub(start);
        image
    }

    fn render_uncached(
        &mut self,
        doc: &Document,
        cache: &Cache,
        spreads: &[Placed],
        size: (u32, u32),
        view: Affine,
        opts: &RenderOptions,
    ) -> Rendered {
        let start = now();
        let (width, height) = size;
        let w = width.clamp(1, MAX_SIDE) as u16;
        let h = height.clamp(1, MAX_SIDE) as u16;
        let mut ctx = match self.ctx.take() {
            Some(mut c) if c.width() == w && c.height() == h && c.render_settings().num_threads == self.threads => {
                c.reset();
                c
            }
            _ => RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: self.threads, ..Default::default() }),
        };
        let mt = ctx.render_settings().num_threads > 0;
        RICH_BLACK.with(|r| r.set(opts.rich_black));
        PLATE.with(|p| p.set(opts.plate));
        self.stats = FrameStats::default();
        self.stories.clear();
        self.glyphs.tick();
        if let Some(bg) = opts.background {
            ctx.set_transform(Affine::IDENTITY);
            ctx.set_paint(peniko::Color::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
            ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
        }
        let px = 1.0 / view.determinant().abs().sqrt().max(1e-12);
        let canvas_visible = view.inverse().transform_rect_bbox(Rect::new(0.0, 0.0, w as f64, h as f64));
        for pl in spreads {
            let Some(sp) = doc.spread(pl.spread) else { continue };
            let sview = view * pl.xf;
            let visible = pl.xf.inverse().transform_rect_bbox(canvas_visible);
            let bounds = sp.bounds();
            // Cull spreads far outside the viewport (pasteboard items may extend; allow a margin).
            if !rect_overlaps(bounds.inflate(2000.0, 2000.0), visible) {
                continue;
            }
            let f = Frame { doc, cache, view: sview, visible, px, opts, mt };
            self.draw_spread(&mut ctx, &f, pl.spread);
        }
        ctx.flush();
        // Render straight into the returned buffer (no extra copy of the frame).
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        if let Some(pm) = vello_cpu::PixmapMut::new(w, h, &mut pixels) {
            ctx.render(pm, &mut self.resources);
        }
        if opts.blend_space_view && doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk && opts.plate.is_none() {
            let mut img = Rendered { width: w as u32, height: h as u32, pixels };
            for pl in spreads {
                let Some(sp) = doc.spread(pl.spread) else { continue };
                if blendspace::uses_rgb_proof(doc, pl.spread, opts) {
                    let r = (view * pl.xf).transform_rect_bbox(sp.bounds());
                    blend_space_view(&mut img, r);
                }
            }
            pixels = img.pixels;
        }
        self.ctx = Some(ctx);
        self.stories.clear();
        self.stats.micros = now().saturating_sub(start);
        Rendered { width: w as u32, height: h as u32, pixels }
    }

    /// Render one document page (absolute index) at `scale` px/pt, optionally including bleed.
    pub fn render_page(&mut self, doc: &Document, cache: &Cache, abs: usize, scale: f64, bleed: bool, opts: &RenderOptions) -> Option<Rendered> {
        let (si, pi) = doc.page_loc(abs)?;
        let page = &doc.spreads[si].pages[pi];
        let mut r = page.bounds();
        if bleed {
            let b = doc.settings.bleed;
            let (l, rr) = if page.side == PageSide::Left { (b[3], b[2]) } else { (b[2], b[3]) };
            r = Rect::new(r.x0 - l, r.y0 - b[0], r.x1 + rr, r.y1 + b[1]);
        }
        let w = (r.width() * scale).round().max(1.0) as u32;
        let h = (r.height() * scale).round().max(1.0) as u32;
        if w > MAX_SIDE || h > MAX_SIDE || w as u64 * h as u64 > MAX_PAGE_PIXELS {
            log::warn!("page {abs} at {scale} px/pt would be {w}×{h} pixels: too large to render");
            return None;
        }
        let view = Affine::scale(scale) * Affine::translate(-r.origin().to_vec2());
        let mut o = opts.clone();
        o.background = Some([255, 255, 255, 255]);
        Some(self.render(doc, cache, &[Placed { spread: SpreadRef::Doc(si), xf: Affine::translate(Vec2::ZERO) }], (w, h), view, &o))
    }
}
