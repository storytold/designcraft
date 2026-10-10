//! DesignCraft renderer: spreads → premultiplied RGBA pixels with `vello_cpu`.
//!
//! The renderer draws *document content only* (paper, page items, parent items, composed text,
//! placed images). Non-printing chrome — guides, frame edges, ports, selection — is drawn by the
//! UI as vector overlays so it stays crisp at any zoom.
//!
//! Callers pass a list of spreads with their placement on a shared canvas and a canvas → pixel
//! view transform, so the UI can render exactly its viewport with every visible spread.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_color::BlendMode as DcBlend;
use designcraft_compose::{Cache, ComposedStory};
use designcraft_doc::{Content, Document, Item, PageSide, SpreadRef, StoryId, StrokeAlign, StrokeType};
use designcraft_geom::corners;
use designcraft_geom::{Affine, BezPath, Rect, Shape, Vec2};
use vello_cpu::kurbo;
use vello_cpu::peniko::{self, BlendMode, Compose, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

pub use vello_cpu;

pub mod damage;
mod fx;

/// How far (points) an object's soft effects reach outside its geometry.
pub fn effect_outset(it: &Item) -> f64 {
    fx::outset(it)
}
pub mod glyphs;
mod pdf_layers;
pub use pdf_layers::{pdf_hide_layers, pdf_layers};
pub mod images;
mod text;

/// Largest side of a raster the renderer draws. vello_cpu keeps sizes in `u16` and rounds them
/// up to its tiles (4 px) and depth buckets (128 px), so a side near `u16::MAX` overflows inside
/// it: stay a few buckets below.
pub const MAX_SIDE: u32 = u16::MAX as u32 + 1 - 512;

/// Largest page raster [`Renderer::render_page`] makes (2 GiB of RGBA).
pub const MAX_PAGE_PIXELS: u64 = 1 << 29;

/// A rendered image (premultiplied RGBA8, row-major).
#[derive(Clone)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rendered {
    pub fn to_straight(&self) -> Vec<u8> {
        let mut out = self.pixels.clone();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a != 0 && a != 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
    /// PNG bytes (empty, with the error logged, if the image can't be encoded).
    pub fn to_png(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut px = self.to_straight();
        // `pixels` is public: a mismatched buffer is padded or cut to the stated size.
        px.resize(self.width as usize * self.height as usize * 4, 0);
        let Some(img) = image::RgbaImage::from_raw(self.width, self.height, px) else {
            log::error!("PNG encode: {}×{} image is too large", self.width, self.height);
            return Vec::new();
        };
        if let Err(e) = img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png) {
            log::error!("PNG encode: {e}");
            return Vec::new();
        }
        buf
    }
    pub fn to_jpeg(&self, quality: u8) -> Vec<u8> {
        let rgba = self.to_straight();
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = p[3] as u32;
                let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                [mix(p[0]), mix(p[1]), mix(p[2])]
            })
            .collect();
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
        let _ = image::ImageEncoder::write_image(enc, &rgb, self.width, self.height, image::ExtendedColorType::Rgb8);
        buf
    }
    /// Straight-alpha RGBA at (x, y).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        let p = &self.pixels[i..i + 4];
        let a = p[3] as u32;
        if a == 0 {
            return [0, 0, 0, 0];
        }
        let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
        [un(p[0]), un(p[1]), un(p[2]), p[3]]
    }
}

/// What to draw.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Canvas background (premultiplied RGBA8); `None` = transparent.
    pub background: Option<[u8; 4]>,
    /// Fill pages with paper colour.
    pub paper: bool,
    /// Skip non-printing items and layers (Preview mode, export).
    pub printing_only: bool,
    /// Items not to draw (live drag previews).
    pub hidden: Vec<designcraft_doc::ItemId>,
    /// Draw text smaller than this many pixels as grey bars ("greeking"); 0 = never.
    pub greek_below_px: f64,
    /// Drop shadow on pages (screen view).
    pub page_shadow: bool,
    /// Pink highlight behind text set in fonts that aren't installed (screen view).
    pub highlight_missing_fonts: bool,
    /// Preferences › Composition: yellow behind lines whose word spacing breaks the paragraph's
    /// limits (darker = worse), green behind text with custom tracking or kerning (screen view).
    pub highlight_hj: bool,
    pub highlight_keeps: bool,
    pub highlight_custom_tracking: bool,
    /// Conditional text indicators: an underline in each condition's colour (screen view).
    pub condition_indicators: bool,
    /// Editorial note anchors (screen view).
    pub note_indicators: bool,
    /// Track Changes: added text highlighted (screen view).
    pub change_markup: bool,
    /// View › Structure › Show Tag Markers: brackets around inline-tagged text (screen view).
    pub tag_markers: bool,
    /// Separations Preview: draw one process plate (0 = cyan … 3 = black) as grey ink coverage,
    /// from each object's own colour (CMYK as authored, others through the colour settings).
    pub plate: Option<u8>,
    /// View › Display Performance.
    pub quality: DisplayQuality,
    /// Preferences › Appearance of Black: show 100% K as rich (pure) black instead of the
    /// accurate dark grey.
    pub rich_black: bool,
    /// View › Overprint Preview: overprinting inks mix with what's beneath.
    pub overprint_preview: bool,
    /// Screen: spreads with transparency are shown as blended in the document's blend space (a
    /// CMYK blend space keeps their colours in the working CMYK gamut).
    pub blend_space_view: bool,
}

/// View › Display Performance: how placed graphics and effects are drawn on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayQuality {
    /// Graphics as grey boxes, no transparency effects.
    Fast,
    /// Low-resolution proxies (72 ppi at 100%).
    Typical,
    /// Full resolution.
    #[default]
    High,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            background: None,
            paper: true,
            printing_only: false,
            hidden: vec![],
            greek_below_px: 0.0,
            page_shadow: false,
            highlight_missing_fonts: false,
            highlight_hj: false,
            highlight_keeps: false,
            highlight_custom_tracking: false,
            condition_indicators: false,
            note_indicators: false,
            change_markup: false,
            tag_markers: false,
            plate: None,
            quality: DisplayQuality::High,
            rich_black: false,
            overprint_preview: false,
            blend_space_view: false,
        }
    }
}

/// A spread placed on the canvas: canvas = xf × spread (a translation, turned for a rotated spread view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub spread: SpreadRef,
    pub xf: Affine,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub items: usize,
    pub glyphs: usize,
    pub micros: u64,
}

/// Reusable renderer: keeps the render context, the frame-space glyph paths of composed text
/// and (process-wide) decoded images between renders.
pub struct Renderer {
    ctx: Option<RenderContext>,
    resources: Resources,
    glyphs: text::GlyphCache,
    /// Composed stories looked up during the current render.
    stories: HashMap<(StoryId, Option<String>), Arc<ComposedStory>>,
    pub threads: u16,
    pub stats: FrameStats,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Frame<'a> {
    doc: &'a Document,
    cache: &'a Cache,
    view: Affine,
    /// Visible area in spread space.
    visible: Rect,
    /// Size of a device pixel in points.
    px: f64,
    opts: &'a RenderOptions,
    /// Drawing into a multithreaded context (no filter layers).
    mt: bool,
}

pub fn default_threads() -> u16 {
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(n) = std::env::var("DESIGNCRAFT_RENDER_THREADS").ok().and_then(|v| v.parse().ok()) {
            return n;
        }
        std::thread::available_parallelism().map(|n| (n.get().saturating_sub(1)).clamp(0, 8) as u16).unwrap_or(0)
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            ctx: None,
            resources: Resources::new(),
            glyphs: text::GlyphCache::default(),
            stories: HashMap::new(),
            threads: default_threads(),
            stats: FrameStats::default(),
        }
    }

    /// Drop cached glyph paths (they are rebuilt on demand).
    pub fn clear_caches(&mut self) {
        self.glyphs.clear();
        self.stories.clear();
    }

    /// Number of text frames whose glyph paths are cached.
    pub fn cached_frames(&self) -> usize {
        self.glyphs.len()
    }

    /// Render the given spreads into a `width`×`height` image with `view` (canvas → pixels).
    pub fn render(
        &mut self,
        doc: &Document,
        cache: &Cache,
        spreads: &[Placed],
        width: u32,
        height: u32,
        view: Affine,
        opts: &RenderOptions,
    ) -> Rendered {
        let start = now();
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
                if sp.items.iter().any(|it| it.involves_transparency() && !it.hidden) {
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
        Some(self.render(doc, cache, &[Placed { spread: SpreadRef::Doc(si), xf: Affine::translate(Vec2::ZERO) }], w, h, view, &o))
    }

    fn draw_spread(&mut self, ctx: &mut RenderContext, f: &Frame, sr: SpreadRef) {
        let doc = f.doc;
        let Some(sp) = doc.spread(sr) else { return };
        // Paper.
        if f.opts.paper {
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
        // Layers back to front; within each layer parent items first (document spreads only).
        for layer in doc.layers.iter().rev() {
            if !layer.visible || f.opts.printing_only && !layer.printable {
                continue;
            }
            if let SpreadRef::Doc(si) = sr {
                let first = doc.first_page_of_spread(si);
                for (pi, page) in sp.pages.iter().enumerate() {
                    if !page.show_parent_items {
                        continue;
                    }
                    let Some((ppi, ppage)) = doc.parent_page_for(first + pi) else { continue };
                    let parent = &doc.parents[ppi];
                    let dx = page.x - parent.pages[ppage].x;
                    let page_name = doc.page_name(first + pi);
                    let pr = parent.pages[ppage].bounds();
                    for it in &parent.items {
                        if it.layer != layer.id || page.overridden.contains(&it.id) {
                            continue;
                        }
                        // Parent items belong to the parent page they sit on.
                        if parent.page_at_x(it.bounds().center().x) != Some(ppage) && parent.pages.len() > 1 {
                            continue;
                        }
                        let _ = pr;
                        self.draw_item(ctx, f, it, Affine::translate((dx, 0.0)), Some(&page_name));
                    }
                }
            }
            for it in &sp.items {
                if it.layer == layer.id {
                    self.draw_item(ctx, f, it, Affine::IDENTITY, None);
                }
            }
        }
    }

    fn draw_item(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, parent: Affine, page_name: Option<&str>) {
        if it.hidden || f.opts.hidden.contains(&it.id) || f.opts.printing_only && it.nonprinting {
            return;
        }
        let xf = parent * it.xf;
        let vb = xf.transform_rect_bbox(it.inner_bounds()).inflate(it.stroke.extent() + 2.0, it.stroke.extent() + 2.0);
        if it.has_nested_items() {
            // A frame with items pasted into it: fill, the items clipped to the frame, stroke.
            if !rect_overlaps(vb, f.visible) {
                return;
            }
            let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
            let layered = it.opacity < 0.999 || it.blend != DcBlend::Normal;
            if layered {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(blend_mode(it.blend)), Some(it.opacity), None, None);
            }
            if !it.fill.is_none() {
                ctx.set_transform(f.view * xf);
                if set_fill_paint(ctx, f.doc, &it.fill, bp.bounding_box()) {
                    ctx.fill_path(&bp);
                }
                ctx.reset_paint_transform();
            }
            ctx.set_transform(f.view * xf);
            ctx.push_clip_layer(&bp);
            for c in it.children() {
                self.draw_item(ctx, f, c, xf, page_name);
            }
            ctx.pop_layer();
            if !it.stroke.is_none() {
                self.draw_stroke(ctx, f, it, &bp, xf);
            }
            if layered {
                ctx.pop_layer();
            }
            return;
        }
        if !it.children().is_empty() {
            // Groups: children carry their own transforms relative to the group.
            // Isolate Blending / Knockout: the group is its own layer (blend modes stop at it).
            let layered = it.opacity < 0.999 || it.blend != DcBlend::Normal || it.isolate || it.knockout;
            if layered {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_layer(None, Some(blend_mode(it.blend)), Some(it.opacity), None, None);
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
                    self.draw_item(ctx, f, c, xf, page_name);
                }
            } else {
                for c in it.shown_children() {
                    self.draw_item(ctx, f, c, xf, page_name);
                }
            }
            if layered {
                ctx.pop_layer();
            }
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
            self.draw_item_fx(ctx, f, it, &bp, xf, page_name);
        } else {
            self.draw_body(ctx, f, it, &bp, xf, page_name);
        }
        if layered {
            ctx.pop_layer();
        }
    }

    /// The composed story for a frame (memoised for the current render: the cache lookup
    /// validates the whole thread each time).
    fn story(&mut self, f: &Frame, sid: StoryId, page_name: Option<&str>) -> Arc<ComposedStory> {
        let key = (sid, page_name.map(str::to_string));
        if let Some(cs) = self.stories.get(&key) {
            return cs.clone();
        }
        let cs = f.cache.get(f.doc, sid, page_name);
        self.stories.insert(key, cs.clone());
        cs
    }

    /// Fill, content and stroke of a leaf item (no opacity, blend or effects).
    pub(crate) fn draw_body(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine, page_name: Option<&str>) {
        let doc = f.doc;
        // Fill.
        if !it.fill.is_none() {
            ctx.set_transform(f.view * xf);
            let op = overprints(f, &it.fill.swatch, it.fill.tint, it.fill.overprint);
            if op {
                ctx.push_layer(None, Some(BlendMode::new(Mix::Multiply, Compose::SrcOver)), None, None, None);
            }
            if set_fill_paint(ctx, doc, &it.fill, bp.bounding_box()) && it.path.is_closed() {
                ctx.fill_path(bp);
            }
            ctx.reset_paint_transform();
            if op {
                ctx.pop_layer();
            }
        }
        // Content.
        match &it.content {
            Content::Graphic(g) => {
                let g = &if it.media.is_some() { it.drawn_graphic(&f.doc.assets).unwrap_or_else(|| g.clone()) } else { g.clone() };
                ctx.set_transform(f.view * xf);
                ctx.push_clip_layer(bp);
                if !it.pdf_hidden_layers.is_empty() {
                    HIDDEN_LAYERS.with(|h| *h.borrow_mut() = it.pdf_hidden_layers.clone());
                }
                self.draw_graphic(ctx, f, g, xf);
                if !it.pdf_hidden_layers.is_empty() {
                    HIDDEN_LAYERS.with(|h| h.borrow_mut().clear());
                }
                ctx.pop_layer();
            }
            Content::Text(tf) => {
                let cs = self.story(f, tf.story, page_name);
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
            ctx.set_paint(peniko::Color::from_rgba8(80, 140, 255, 40));
            ctx.fill_rect(&r);
            ctx.set_paint(peniko::Color::from_rgba8(60, 110, 220, 200));
            ctx.set_stroke(kurbo::Stroke::new(0.75));
            ctx.stroke_path(&r.to_path(0.1));
            if ff.kind == designcraft_doc::FieldKind::CheckBox && !ff.value.is_empty() && ff.value != "Off" {
                let (w, h) = (r.width(), r.height());
                let mut c = BezPath::new();
                c.move_to((r.x0 + w * 0.22, r.y0 + h * 0.48));
                c.line_to((r.x0 + w * 0.42, r.y0 + h * 0.72));
                c.line_to((r.x0 + w * 0.78, r.y0 + h * 0.24));
                ctx.set_paint(peniko::Color::BLACK);
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

    fn draw_stroke(&mut self, ctx: &mut RenderContext, f: &Frame, it: &Item, bp: &BezPath, xf: Affine) {
        let st = &it.stroke;
        let Some(c) = f.doc.resolve_color(&st.swatch, st.tint) else { return };
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
        match &kind {
            StrokeType::Dashed { pattern } if !pattern.is_empty() => stroke = stroke.with_dashes(0.0, pattern.iter().copied()),
            StrokeType::Dotted => stroke = stroke.with_dashes(0.0, [0.0, st.weight * 2.0]).with_caps(kurbo::Cap::Round),
            _ => {}
        }
        ctx.set_transform(f.view * xf);
        ctx.set_paint(color_of(&c, 1.0));
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
            ctx.set_paint(color_of(&c, 1.0));
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

    pub(crate) fn draw_graphic(&mut self, ctx: &mut RenderContext, f: &Frame, g: &designcraft_doc::Graphic, xf: Affine) {
        let Some(asset) = f.doc.assets.get(&g.asset) else { return };
        if designcraft_doc::media_kind(&asset.mime).is_some() {
            // Video / sound: a dark frame with a play mark (the poster, when set, is drawn by the item).
            ctx.set_transform(f.view * xf * g.xf);
            ctx.set_paint(peniko::Color::from_rgba8(48, 48, 52, 255));
            ctx.fill_rect(&Rect::new(0.0, 0.0, g.size.0, g.size.1));
            let (cx, cy, r) = (g.size.0 / 2.0, g.size.1 / 2.0, g.size.0.min(g.size.1) * 0.18);
            let mut tri = BezPath::new();
            tri.move_to((cx - r * 0.6, cy - r));
            tri.line_to((cx + r, cy));
            tri.line_to((cx - r * 0.6, cy + r));
            tri.close_path();
            ctx.set_paint(peniko::Color::from_rgba8(235, 235, 235, 255));
            ctx.fill_path(&tri);
            return;
        }
        if f.opts.quality == DisplayQuality::Fast {
            ctx.set_transform(f.view * xf * g.xf);
            ctx.set_paint(peniko::Color::from_rgba8(178, 178, 178, 255));
            ctx.fill_rect(&Rect::new(0.0, 0.0, g.size.0, g.size.1));
            return;
        }
        // Pick a mip level close to the on-screen size (Typical: a 72 ppi proxy).
        let on_screen = (f.view * xf * g.xf).determinant().abs().sqrt();
        let on_screen = if f.opts.quality == DisplayQuality::Typical { on_screen.min(1.0) } else { on_screen };
        let hidden = HIDDEN_LAYERS.with(|h| h.borrow().clone());
        let data = if !hidden.is_empty() && is_pdf(&asset.data) { pdf_layers::layered(&asset.data, &hidden) } else { asset.data.clone() };
        let Some(pm) = images::mip(&data, asset.page, g.size.0, on_screen) else { return };
        let pm = match PLATE.with(|p| p.get()) {
            Some(plate) => plate_pixmap(&pm, plate),
            None => pm,
        };
        let rect = Rect::new(0.0, 0.0, g.size.0, g.size.1);
        ctx.set_transform(f.view * xf * g.xf);
        let sx = g.size.0 / pm.width().max(1) as f64;
        let sy = g.size.1 / pm.height().max(1) as f64;
        ctx.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(pm),
            sampler: peniko::ImageSampler::default().with_quality(peniko::ImageQuality::Medium),
        });
        ctx.set_paint_transform(Affine::scale_non_uniform(sx, sy));
        ctx.fill_rect(&rect);
        ctx.reset_paint_transform();
    }
}

/// sRGB → working CMYK through a 17³ table built once per colour settings (trilinear).
fn cmyk_lut(rgb: [f32; 3]) -> [f32; 4] {
    const N: usize = 17;
    type Lut = (designcraft_color::cms::ColorSettings, std::sync::Arc<Vec<[f32; 4]>>);
    static LUT: std::sync::Mutex<Option<Lut>> = std::sync::Mutex::new(None);
    let settings = designcraft_color::cms::active_settings();
    let table = {
        let mut g = LUT.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_ref() {
            Some((s, t)) if *s == settings => t.clone(),
            _ => {
                let cms = designcraft_color::cms::active();
                let step = |i: usize| i as f32 / (N - 1) as f32;
                let t: Vec<[f32; 4]> = (0..N * N * N)
                    .map(|i| {
                        cms.srgb_to_cmyk([step(i / (N * N)), step(i / N % N), step(i % N)], designcraft_color::cms::Intent::RelativeColorimetric)
                    })
                    .collect();
                let t = std::sync::Arc::new(t);
                *g = Some((settings, t.clone()));
                t
            }
        }
    };
    let f = rgb.map(|v| v.clamp(0.0, 1.0) * (N - 1) as f32);
    let i = f.map(|v| (v as usize).min(N - 2));
    let d = [f[0] - i[0] as f32, f[1] - i[1] as f32, f[2] - i[2] as f32];
    let mut out = [0.0f32; 4];
    for (corner, w) in (0..8).map(|c| {
        let (a, b, cc) = (c >> 2 & 1, c >> 1 & 1, c & 1);
        let w = (if a == 1 { d[0] } else { 1.0 - d[0] }) * (if b == 1 { d[1] } else { 1.0 - d[1] }) * (if cc == 1 { d[2] } else { 1.0 - d[2] });
        ((i[0] + a) * N * N + (i[1] + b) * N + i[2] + cc, w)
    }) {
        for k in 0..4 {
            out[k] += table[corner][k] * w;
        }
    }
    out
}

/// An image's ink coverage on one process plate, as grey (cached per image and plate).
fn plate_pixmap(pm: &std::sync::Arc<vello_cpu::Pixmap>, plate: u8) -> std::sync::Arc<vello_cpu::Pixmap> {
    type Cache = std::collections::HashMap<(usize, u8), std::sync::Arc<vello_cpu::Pixmap>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (std::sync::Arc::as_ptr(pm) as usize, plate);
    if let Some(v) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.get(&key).cloned()) {
        return v;
    }
    let data: Vec<vello_cpu::color::PremulRgba8> = pm
        .data()
        .iter()
        .map(|p| {
            if p.a == 0 {
                return *p;
            }
            let k = 255.0 / p.a as f32;
            let rgb = [p.r, p.g, p.b].map(|v| (v as f32 * k / 255.0).min(1.0));
            let cmyk = cmyk_lut(rgb);
            let v = ((1.0 - cmyk[plate.min(3) as usize].clamp(0.0, 1.0)) * p.a as f32).round() as u8;
            vello_cpu::color::PremulRgba8 { r: v, g: v, b: v, a: p.a }
        })
        .collect();
    let out = std::sync::Arc::new(vello_cpu::Pixmap::from_parts(data, pm.width(), pm.height()));
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = c.get_or_insert_with(Default::default);
    if map.len() > 32 {
        map.clear();
    }
    map.insert(key, out.clone());
    out
}

fn rect_overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

thread_local! {
    /// Object Layer Options of the placed PDF being drawn.
    static HIDDEN_LAYERS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Appearance of Black for the render on this thread: 100% K as pure black.
    static RICH_BLACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Separations Preview plate for the render on this thread.
    static PLATE: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

pub fn color_of(c: &designcraft_color::Color, alpha: f32) -> peniko::Color {
    if let Some(p) = PLATE.with(|p| p.get()) {
        let cmyk = match *c {
            designcraft_color::Color::Cmyk { c, m, y, k } => [c, m, y, k],
            _ => c.to_cmyk_managed(designcraft_color::cms::Intent::RelativeColorimetric),
        };
        let v = ((1.0 - cmyk[p.min(3) as usize].clamp(0.0, 1.0)) * 255.0).round() as u8;
        return peniko::Color::from_rgba8(v, v, v, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    if RICH_BLACK.with(|r| r.get())
        && let designcraft_color::Color::Cmyk { c: cc, m, y, k } = *c
        && cc <= 1e-3
        && m <= 1e-3
        && y <= 1e-3
        && k >= 0.999
    {
        return peniko::Color::from_rgba8(0, 0, 0, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    let [r, g, b, a] = c.to_rgba8(alpha);
    peniko::Color::from_rgba8(r, g, b, a)
}

/// Set a swatch fill (solid or gradient) as the paint. Returns false for [None]/unknown.
fn set_fill_paint(ctx: &mut RenderContext, doc: &Document, fill: &designcraft_doc::Fill, bounds: Rect) -> bool {
    let (swatch, tint, angle) = (fill.swatch.as_str(), fill.tint, fill.gradient_angle);
    if let Some(g) = designcraft_color::swatch::resolve_gradient(&doc.swatches, swatch) {
        let stops: Vec<peniko::ColorStop> = g.expanded_stops().iter().map(|(o, c, a)| peniko::ColorStop::from((*o, color_of(c, *a)))).collect();
        if stops.is_empty() {
            return false;
        }
        let c = bounds.center();
        let grad = match (g.kind, fill.gradient_vector) {
            (designcraft_color::GradientKind::Radial, Some([x0, y0, x1, y1])) => {
                let r = Vec2::new(x1 - x0, y1 - y0).hypot().max(1e-3);
                peniko::Gradient::new_radial((x0, y0), r as f32).with_stops(stops.as_slice())
            }
            (_, Some([x0, y0, x1, y1])) => peniko::Gradient::new_linear((x0, y0), (x1, y1)).with_stops(stops.as_slice()),
            (designcraft_color::GradientKind::Radial, None) => {
                let r = bounds.width().max(bounds.height()) / 2.0;
                peniko::Gradient::new_radial(c, r as f32).with_stops(stops.as_slice())
            }
            _ => {
                let a = angle.unwrap_or(0.0).to_radians();
                let half = (bounds.width() * a.cos().abs() + bounds.height() * a.sin().abs()) / 2.0;
                let d = Vec2::new(a.cos(), -a.sin()) * half;
                peniko::Gradient::new_linear(c - d, c + d).with_stops(stops.as_slice())
            }
        };
        ctx.set_paint(grad);
        return true;
    }
    match doc.resolve_color(swatch, tint) {
        Some(c) => {
            ctx.set_paint(color_of(&c, 1.0));
            true
        }
        None => false,
    }
}

/// Overprint Preview: does this paint overprint (its own flag, or 100% [Black], which
/// overprints by default)? It's then multiplied over what's beneath.
pub(crate) fn overprints(f: &Frame, swatch: &str, tint: f32, flag: bool) -> bool {
    f.opts.overprint_preview && (flag || (f.doc.settings.overprint_black && swatch == designcraft_color::swatch::BLACK && tint >= 0.999))
}

pub fn blend_mode(b: DcBlend) -> BlendMode {
    use DcBlend as B;
    let mix = match b {
        B::Normal => Mix::Normal,
        B::Darken => Mix::Darken,
        B::Multiply => Mix::Multiply,
        B::ColorBurn => Mix::ColorBurn,
        B::Lighten => Mix::Lighten,
        B::Screen => Mix::Screen,
        B::ColorDodge => Mix::ColorDodge,
        B::Overlay => Mix::Overlay,
        B::SoftLight => Mix::SoftLight,
        B::HardLight => Mix::HardLight,
        B::Difference => Mix::Difference,
        B::Exclusion => Mix::Exclusion,
        B::Hue => Mix::Hue,
        B::Saturation => Mix::Saturation,
        B::Color => Mix::Color,
        B::Luminosity => Mix::Luminosity,
    };
    BlendMode::new(mix, Compose::SrcOver)
}

/// Decode encoded image bytes into a premultiplied pixmap.
pub fn decode_pixmap(bytes: &[u8]) -> Option<Pixmap> {
    decode_pixmap_page(bytes, 0)
}

/// [`decode_pixmap`] for page `page` of a PDF (other formats have one page).
pub fn decode_pixmap_page(bytes: &[u8], page: u32) -> Option<Pixmap> {
    if is_pdf(bytes) {
        return render_pdf_page(bytes, page as usize, 3000);
    }
    if designcraft_images::is_svg(bytes) {
        let (px, w, h) = designcraft_images::render_svg(bytes, 3000)?;
        let data = px.as_chunks::<4>().0.iter().map(|p| vello_cpu::color::PremulRgba8 { r: p[0], g: p[1], b: p[2], a: p[3] }).collect();
        return Some(Pixmap::from_parts(data, w.min(u16::MAX as u32) as u16, h.min(u16::MAX as u32) as u16));
    }
    let img = designcraft_images::decode_rgba(bytes)?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    let data: Vec<vello_cpu::color::PremulRgba8> = img
        .pixels()
        .map(|p| {
            let a = p[3] as u16;
            let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
            vello_cpu::color::PremulRgba8 { r: m(p[0]), g: m(p[1]), b: m(p[2]), a: p[3] }
        })
        .collect();
    Some(Pixmap::from_parts(data, w as u16, h as u16))
}

/// Box-filter downsample by 2.
fn halve(pm: &Pixmap) -> Pixmap {
    let (w, h) = (pm.width() as usize, pm.height() as usize);
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let src = pm.data();
    let mut out = Vec::with_capacity(nw * nh);
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (2 * x + dx).min(w - 1);
                let sy = (2 * y + dy).min(h - 1);
                let p = src[sy * w + sx];
                acc[0] += p.r as u32;
                acc[1] += p.g as u32;
                acc[2] += p.b as u32;
                acc[3] += p.a as u32;
            }
            out.push(vello_cpu::color::PremulRgba8 { r: (acc[0] / 4) as u8, g: (acc[1] / 4) as u8, b: (acc[2] / 4) as u8, a: (acc[3] / 4) as u8 });
        }
    }
    Pixmap::from_parts(out, nw as u16, nh as u16)
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}
#[cfg(target_arch = "wasm32")]
fn now() -> u64 {
    0
}

/// Pixel size of an encoded image without decoding it fully.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if is_pdf(bytes) {
        // Placed PDFs are sized by their page (crop box) in points.
        return pdf_page_size(bytes, 0).map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32));
    }
    if designcraft_images::is_svg(bytes) {
        // SVGs by their width and height (CSS pixels → points).
        return designcraft_images::natural_size(bytes).map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32));
    }
    designcraft_images::pixel_size(bytes)
}

/// MIME type guess for encoded image bytes.
/// Is this a PDF file (placed PDFs are graphics)?
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

/// Page count of a PDF.
pub fn pdf_page_count(bytes: &[u8]) -> Option<usize> {
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    Some(pdf.pages().len())
}

/// Size of PDF page `page` (0-based), in points, as it shows (crop box, rotation applied).
pub fn pdf_page_size(bytes: &[u8], page: usize) -> Option<(f64, f64)> {
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    let p = pdf.pages().get(page)?;
    let (w, h) = p.render_dimensions();
    Some((w as f64, h as f64))
}

/// PDF page `page` as PNG, its longer side about `max_side` pixels, with the `hidden` layers off.
pub fn pdf_page_png(bytes: &[u8], page: usize, max_side: u32, hidden: &[String]) -> Option<Vec<u8>> {
    let data = if hidden.is_empty() { bytes.to_vec() } else { pdf_hide_layers(bytes, hidden).unwrap_or_else(|| bytes.to_vec()) };
    let pm = render_pdf_page(&data, page, max_side)?;
    let pixels: Vec<u8> = pm.data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    Some(Rendered { width: pm.width() as u32, height: pm.height() as u32, pixels }.to_png())
}

/// Rasterize PDF page `page` so its longer side is about `max_side` pixels (screen display; PDF
/// export embeds the page as vectors).
pub fn render_pdf_page(bytes: &[u8], page: usize, max_side: u32) -> Option<Pixmap> {
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    let p = pdf.pages().get(page)?;
    let (w, h) = p.render_dimensions();
    let scale = (max_side as f32 / w.max(h).max(1.0)).min(8.0);
    let rs = hayro::RenderSettings { x_scale: scale, y_scale: scale, ..Default::default() };
    let pm = hayro::render(p, &hayro::RenderCache::new(), &hayro::hayro_interpret::InterpreterSettings::default(), &rs);
    let (pw, ph) = (pm.width(), pm.height());
    let data: Vec<vello_cpu::color::PremulRgba8> =
        pm.data_as_u8_slice().as_chunks::<4>().0.iter().map(|c| vello_cpu::color::PremulRgba8 { r: c[0], g: c[1], b: c[2], a: c[3] }).collect();
    Some(Pixmap::from_parts(data, pw, ph))
}

/// A page box of a PDF page (`crop`, `trim`, `bleed`, `art`, `media`) as (x, y, w, h) in points
/// from the top-left of the page as rendered (its visible crop box); missing boxes fall back to the
/// crop box, as PDF readers do.
pub fn pdf_page_box(bytes: &[u8], page: usize, kind: &str) -> Option<(f64, f64, f64, f64)> {
    use hayro::hayro_syntax::object::{Rect as PRect, dict::keys};
    let pdf = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.to_vec())).ok()?;
    let p = pdf.pages().get(page)?;
    let visible = p.intersected_crop_box();
    let key: &[u8] = match kind {
        "trim" => keys::TRIM_BOX,
        "bleed" => keys::BLEED_BOX,
        "art" => keys::ART_BOX,
        "media" => return Some(rel(visible, p.media_box())),
        _ => return Some(rel(visible, visible)),
    };
    let b = p.raw().get::<PRect>(key).map(|b| b.intersect(p.media_box())).unwrap_or(visible);
    fn rel(v: PRect, b: PRect) -> (f64, f64, f64, f64) {
        (b.x0 - v.x0, v.y1 - b.y1, b.width(), b.height())
    }
    Some(rel(visible, b))
}

/// Separations Preview on a rendered image: `plate` 0–3 shows that process plate (C, M, Y, K) as
/// ink density in grey; with an `ink_limit` (0–4, total ink) areas over it are shown in red.
/// Pixels are separated with a plain GCR (black = 1 − max(r, g, b)), so it's a preview.
/// View › Proof Colors: simulate the proof target (a press, another RGB space, colour-vision
/// deficiencies) through the active colour settings' proof table.
pub fn proof_view(img: &mut Rendered, setup: &designcraft_color::cms::ProofSetup) {
    let lut = designcraft_color::cms::active().proof_lut(setup);
    for px in img.pixels.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a == 0 {
            continue;
        }
        // Premultiplied → straight, proof, back.
        let k = 255.0 / a as f32;
        let rgb = [px[0], px[1], px[2]].map(|v| ((v as f32 * k).round()).min(255.0) as u8);
        let out = lut.apply8(rgb);
        for i in 0..3 {
            px[i] = ((out[i] as u32 * a as u32 + 127) / 255) as u8;
        }
    }
}

/// A CMYK transparency blend space on screen: the pixels of `area` (device space) through the
/// working CMYK profile and back, as a spread composited in that space looks.
fn blend_space_view(img: &mut Rendered, area: Rect) {
    use designcraft_color::cms::{ProofSetup, ProofTarget};
    let lut = designcraft_color::cms::active().proof_lut(&ProofSetup { target: ProofTarget::WorkingCmyk, ..Default::default() });
    let (w, h) = (img.width as i64, img.height as i64);
    let (x0, y0) = ((area.x0.floor() as i64).clamp(0, w), (area.y0.floor() as i64).clamp(0, h));
    let (x1, y1) = ((area.x1.ceil() as i64).clamp(0, w), (area.y1.ceil() as i64).clamp(0, h));
    for y in y0..y1 {
        let row = &mut img.pixels[((y * w + x0) * 4) as usize..((y * w + x1) * 4) as usize];
        for px in row.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a == 0 {
                continue;
            }
            let k = 255.0 / a as f32;
            let rgb = [px[0], px[1], px[2]].map(|v| ((v as f32 * k).round()).min(255.0) as u8);
            let out = lut.apply8(rgb);
            for i in 0..3 {
                px[i] = ((out[i] as u32 * a as u32 + 127) / 255) as u8;
            }
        }
    }
}

pub fn separation_view(img: &mut Rendered, plate: Option<u8>, ink_limit: Option<f32>) {
    for px in img.pixels.as_chunks_mut::<4>().0 {
        let a = px[3] as f32 / 255.0;
        // Composite over paper white, then separate.
        let ch = |v: u8| (v as f32 / 255.0) + (1.0 - a);
        let (r, g, b) = (ch(px[0]).min(1.0), ch(px[1]).min(1.0), ch(px[2]).min(1.0));
        let k = 1.0 - r.max(g).max(b);
        let inks =
            if k >= 0.999 { [0.0, 0.0, 0.0, 1.0] } else { [(1.0 - r - k) / (1.0 - k), (1.0 - g - k) / (1.0 - k), (1.0 - b - k) / (1.0 - k), k] };
        let mut out = match plate {
            Some(p) => {
                let v = ((1.0 - inks[p.min(3) as usize].clamp(0.0, 1.0)) * 255.0).round() as u8;
                [v, v, v, 255]
            }
            None => [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255],
        };
        if let Some(limit) = ink_limit
            && inks.iter().sum::<f32>() > limit
        {
            out = [230, 40, 40, 255];
        }
        px.copy_from_slice(&out);
    }
}

pub fn image_mime(bytes: &[u8]) -> &'static str {
    designcraft_images::mime(bytes)
}

#[cfg(test)]
mod tests_previous_layout_reuse;

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::{Fill, ParaFormat};

    /// `Rendered` has public fields: a buffer that doesn't match the stated size used to panic
    /// in `to_png`.
    /// A canvas 65535 pixels wide (any render wider than that was clamped to it) panicked inside
    /// vello_cpu when it snapped the width up to its tiles; a huge page export now declines.
    #[test]
    fn very_wide_renders_do_not_panic() {
        let doc = Document::new(&NewDocument::default());
        let cache = Cache::new();
        let mut r = Renderer::new();
        let img = r.render(&doc, &cache, &[], 70_000, 4, Affine::IDENTITY, &RenderOptions::default());
        assert_eq!(img.width, MAX_SIDE);
        let page = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
        let opts = RenderOptions { background: Some([255, 255, 255, 255]), ..Default::default() };
        let img = r.render(&doc, &cache, &page, 70_000, 64, Affine::scale(120.0), &opts);
        assert_eq!(img.width, MAX_SIDE);
        assert!(r.render_page(&doc, &cache, 0, 500.0, true, &RenderOptions::default()).is_none());
    }

    #[test]
    fn to_png_survives_a_mismatched_buffer() {
        let short = Rendered { width: 4, height: 4, pixels: vec![255; 10] };
        let png = short.to_png();
        assert_eq!(image::load_from_memory(&png).map(|i| (i.width(), i.height())).ok(), Some((4, 4)));
        assert!(Rendered { width: 0, height: 0, pixels: vec![] }.to_png().len() < 1024);
    }

    #[test]
    fn proof_view_simulates_the_target() {
        use designcraft_color::cms::{ProofSetup, ProofTarget};
        let red = |a: u8| Rendered { width: 1, height: 1, pixels: vec![a, 0, 0, a] };
        let mut img = red(255);
        proof_view(&mut img, &ProofSetup { target: ProofTarget::Protanopia, ..Default::default() });
        assert!(img.pixels[1] > 40, "protanopia turns pure red olive: {:?}", img.pixels);
        let mut img = red(255);
        proof_view(&mut img, &ProofSetup { target: ProofTarget::MonitorRgb, ..Default::default() });
        assert!(img.pixels[0] > 245 && img.pixels[1] < 10, "monitor RGB is no simulation: {:?}", img.pixels);
        // Transparent pixels stay transparent; premultiplication is kept.
        let mut img = Rendered { width: 2, height: 1, pixels: vec![0, 0, 0, 0, 100, 0, 0, 128] };
        proof_view(&mut img, &ProofSetup { target: ProofTarget::WorkingCmyk, ..Default::default() });
        assert_eq!(&img.pixels[..4], &[0, 0, 0, 0]);
        assert!(img.pixels[4] <= 128 && img.pixels[7] == 128);
    }

    #[test]
    fn renders_page_with_text_and_fill() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (fid, _) =
            d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 576.0, 300.0), lid, "Hello DesignCraft", ParaFormat::default()).unwrap();
        let id = designcraft_doc::ItemId(d.alloc());
        let mut box_ =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(36.0, 400.0, 136.0, 500.0)));
        box_.fill = Fill::swatch("C=100 M=0 Y=0 K=0");
        d.insert_item(SpreadRef::Doc(0), box_, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        assert_eq!((img.width, img.height), (612, 792));
        // Paper is white, the box is cyan-ish, text drew some dark pixels.
        assert_eq!(img.pixel(300, 700), [255, 255, 255, 255]);
        let c = img.pixel(80, 450);
        assert!(c[0] < 80 && c[2] > 150, "{c:?}");
        let dark = (36..300).flat_map(|x| (36..60).map(move |y| (x, y))).filter(|&(x, y)| img.pixel(x, y)[0] < 100).count();
        assert!(dark > 50, "text pixels: {dark}");
        let _ = fid;
        assert!(r.stats.glyphs >= 15);
    }

    #[test]
    fn arrowheads_draw_at_line_ends() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let id = designcraft_doc::ItemId(d.alloc());
        let path = designcraft_geom::shapes::line(designcraft_geom::Point::new(100.0, 400.0), designcraft_geom::Point::new(400.0, 400.0));
        let mut line = Item::new(id, lid, designcraft_doc::Shape::GraphicLine, path);
        line.stroke = designcraft_doc::Stroke {
            swatch: designcraft_color::swatch::BLACK.into(),
            weight: 2.0,
            end: designcraft_doc::Arrowhead::TriangleWide,
            ..Default::default()
        };
        d.insert_item(SpreadRef::Doc(0), line, None).unwrap();
        // A dashed line with a gap colour: the gaps are painted.
        let id = designcraft_doc::ItemId(d.alloc());
        let path = designcraft_geom::shapes::line(designcraft_geom::Point::new(100.0, 500.0), designcraft_geom::Point::new(400.0, 500.0));
        let mut dashed = Item::new(id, lid, designcraft_doc::Shape::GraphicLine, path);
        dashed.stroke = designcraft_doc::Stroke {
            swatch: designcraft_color::swatch::BLACK.into(),
            weight: 4.0,
            kind: designcraft_doc::StrokeType::Dashed { pattern: vec![12.0, 4.0] },
            gap_swatch: "C=100 M=0 Y=0 K=0".into(),
            ..Default::default()
        };
        d.insert_item(SpreadRef::Doc(0), dashed, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        // The head is 6.4 × weight wide: ink 4 pt above the line near the tip, none at the start.
        assert!(img.pixel(393, 396)[0] < 128, "{:?}", img.pixel(393, 396));
        assert_eq!(img.pixel(105, 396), [255, 255, 255, 255]);
        // Nothing past the tip.
        assert_eq!(img.pixel(403, 400), [255, 255, 255, 255]);
        let (dash, gap) = (img.pixel(106, 500), img.pixel(114, 500));
        assert!(dash[0] < 80 && dash[2] < 80, "dash {dash:?}");
        assert!(gap[0] < 80 && gap[2] > 150, "gap {gap:?}");
    }

    #[test]
    fn overprint_preview_mixes_inks() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for (sw, op) in [("C=0 M=0 Y=100 K=0", false), ("C=0 M=100 Y=0 K=0", true)] {
            let id = designcraft_doc::ItemId(d.alloc());
            let mut it =
                Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
            it.fill = Fill { overprint: op, ..Fill::swatch(sw) };
            d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        }
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |on: bool, r: &mut Renderer| {
            r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { overprint_preview: on, ..Default::default() }).unwrap().pixel(150, 150)
        };
        let off = px(false, &mut r);
        let on = px(true, &mut r);
        assert!(off[2] > 100, "magenta knocks out: {off:?}");
        assert!(on[2] < 60 && on[0] > 180, "magenta over yellow → red: {on:?}");
    }

    #[test]
    fn cmyk_blend_space_shows_transparent_spreads_in_cmyk() {
        let mut d = Document::new(&NewDocument::default());
        d.swatches.push(designcraft_color::swatch::Swatch::color("Green", designcraft_color::Color::rgb8(0, 255, 0)));
        let lid = d.default_layer();
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        it.fill = Fill::swatch("Green");
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |d: &Document, r: &mut Renderer| {
            r.render_page(d, &cache, 0, 1.0, false, &RenderOptions { blend_space_view: true, ..Default::default() }).unwrap().pixel(150, 150)
        };
        // No transparency on the spread: shown as is.
        assert_eq!(px(&d, &mut r)[1], 255);
        // With transparency in a CMYK blend space the bright green leaves the RGB gamut's edge.
        d.item_mut(id).unwrap().opacity = 0.99;
        let cmyk = px(&d, &mut r);
        assert!(cmyk[0] > 20 || cmyk[1] < 240, "{cmyk:?}");
        d.settings.blend_space = designcraft_doc::BlendSpace::Rgb;
        let rgb = px(&d, &mut r);
        assert!(rgb[1] > 245 && rgb[0] < 10, "{rgb:?}");
    }

    #[test]
    fn knockout_group_hides_its_own_objects() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let mut kids = Vec::new();
        for (sw, x) in [("C=0 M=0 Y=100 K=0", 100.0), ("C=0 M=100 Y=0 K=0", 150.0)] {
            let id = designcraft_doc::ItemId(d.alloc());
            let mut it =
                Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(x, 100.0, x + 100.0, 200.0)));
            it.fill = Fill::swatch(sw);
            it.opacity = 0.5;
            kids.push(std::sync::Arc::new(it));
        }
        let gid = designcraft_doc::ItemId(d.alloc());
        let mut g = Item::new(gid, lid, designcraft_doc::Shape::Group, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 250.0, 200.0)));
        g.content = designcraft_doc::Content::Group { items: kids };
        d.insert_item(SpreadRef::Doc(0), g, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let overlap = |d: &Document, r: &mut Renderer| r.render_page(d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap().pixel(175, 150);
        let plain = overlap(&d, &mut r);
        d.item_mut(gid).unwrap().knockout = true;
        let ko = overlap(&d, &mut r);
        let magenta_only = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap().pixel(225, 150);
        assert_ne!(plain, ko, "{plain:?} {ko:?}");
        for c in 0..3 {
            assert!((ko[c] as i32 - magenta_only[c] as i32).abs() <= 3, "the overlap shows magenta alone: {ko:?} vs {magenta_only:?}");
        }
    }

    #[test]
    fn separations_split_plates_and_flag_ink_limits() {
        // Pure cyan (0, 255, 255) and rich black-ish (20, 20, 20).
        let mut img = Rendered { width: 2, height: 1, pixels: vec![0, 255, 255, 255, 20, 20, 20, 255] };
        let mut c = img.clone();
        separation_view(&mut c, Some(0), None);
        assert_eq!(c.pixel(0, 0)[0], 0, "cyan plate solid under cyan");
        let mut m = img.clone();
        separation_view(&mut m, Some(1), None);
        assert_eq!(m.pixel(0, 0)[0], 255, "no magenta");
        separation_view(&mut img, None, Some(0.5));
        assert_eq!(img.pixel(0, 0), [230, 40, 40, 255], "100% over a 50% limit");
    }

    #[test]
    fn black_overprints_only_with_the_preference() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for (sw, r) in
            [("C=100 M=0 Y=0 K=0", Rect::new(100.0, 100.0, 200.0, 200.0)), (designcraft_color::swatch::BLACK, Rect::new(150.0, 100.0, 250.0, 200.0))]
        {
            let id = designcraft_doc::ItemId(d.alloc());
            let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
            it.fill = Fill::swatch(sw);
            d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        }
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let o = RenderOptions { overprint_preview: true, ..Default::default() };
        let on = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(175, 150);
        let alone = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(225, 150);
        d.settings.overprint_black = false;
        let off = r.render_page(&d, &cache, 0, 1.0, false, &o).unwrap().pixel(175, 150);
        assert_eq!(off, alone, "knocks out: black alone");
        assert_ne!(on, alone, "overprints: the cyan shows through");
    }

    #[test]
    fn appearance_of_black_on_screen() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        it.fill = Fill::swatch(designcraft_color::swatch::BLACK);
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |rich: bool, r: &mut Renderer| {
            r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { rich_black: rich, ..Default::default() }).unwrap().pixel(150, 150)
        };
        assert!(px(false, &mut r)[0] > 20, "accurate: a dark grey");
        assert_eq!(px(true, &mut r)[..3], [0, 0, 0]);
    }

    #[test]
    fn display_performance_fast_draws_grey_boxes() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        // A 4×4 red PNG placed at 100,100 (100×100 pt).
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let aid = designcraft_doc::AssetId(d.alloc());
        d.assets.insert(
            aid,
            Arc::new(designcraft_doc::Asset {
                page: 0,
                id: aid,
                name: "red.png".into(),
                mime: "image/png".into(),
                link: None,
                data: Arc::new(png),
                pixels: Some((4, 4)),
            }),
        );
        let id = designcraft_doc::ItemId(d.alloc());
        let mut it =
            Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
        it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
            asset: aid,
            size: (100.0, 100.0),
            xf: Affine::translate((100.0, 100.0)),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
        });
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        let cache = Cache::new();
        let mut r = Renderer::new();
        r.threads = 0;
        let px = |q: DisplayQuality, r: &mut Renderer| {
            r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { quality: q, ..Default::default() }).unwrap().pixel(150, 150)
        };
        assert_eq!(px(DisplayQuality::High, &mut r)[..3], [255, 0, 0]);
        assert_eq!(px(DisplayQuality::Typical, &mut r)[..3], [255, 0, 0]);
        assert_eq!(px(DisplayQuality::Fast, &mut r)[..3], [178, 178, 178]);
    }

    #[test]
    fn missing_fonts_are_highlighted_on_screen_only() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 100.0), lid, "Missing font", ParaFormat::default()).unwrap();
        d.story_mut(sid).unwrap().format_chars(0..12, |f| f.over.font_family = Some("No Such Family".into()));
        let cache = Cache::new();
        let cs = cache.get(&d, sid, None);
        assert!(cs.styles.iter().any(|s| s.missing_font));
        let l = &cs.frames[0].lines[0];
        // A point inside the line box but between glyph strokes: the space after "Missing".
        let g = l.glyphs.iter().find(|g| g.byte == 7).unwrap();
        let (x, y) = ((g.x + g.adv / 2.0) as u32, (l.baseline - l.ascent * 0.5) as u32);
        let mut r = Renderer::new();
        r.threads = 0;
        let plain = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        let shown = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions { highlight_missing_fonts: true, ..Default::default() }).unwrap();
        assert_eq!(plain.pixel(x, y)[1], 255, "no highlight in output");
        let p = shown.pixel(x, y);
        assert!(p[0] > 240 && p[1] < 200, "pink on screen: {p:?}");
    }

    #[test]
    fn renders_tables() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 336.0, 400.0), lid, "", ParaFormat::default()).unwrap();
        let mut t = designcraft_doc::Table::new(5, 3, 3, 1, 0, 300.0);
        for c in 0..3 {
            let cell = t.cell_mut(0, c).unwrap();
            cell.fill = "C=100 M=0 Y=0 K=0".into();
            cell.text.insert(0, "Head");
            t.cell_mut(2, c).unwrap().text.insert(0, "WWWWWWWW");
        }
        d.story_mut(sid).unwrap().insert_table(0, t);
        let cache = Cache::new();
        let cs = cache.get(&d, sid, None);
        let tf = &cs.frames[0].tables[0];
        let head = tf.cell(0, 1).unwrap().rect;
        let body = tf.cell(2, 0).unwrap().rect;
        let mut r = Renderer::new();
        r.threads = 0;
        let img = r.render_page(&d, &cache, 0, 1.0, false, &RenderOptions::default()).unwrap();
        // Header fill (cyan) near the right edge of the middle header cell.
        let c = img.pixel((head.x1 - 3.0) as u32, (head.y0 + 3.0) as u32);
        assert!(c[0] < 80 && c[2] > 150, "{c:?}");
        // The table border (black) on the left edge.
        let e = img.pixel(body.x0.round() as u32, body.center().y as u32);
        assert!(e[0] < 200, "{e:?}");
        // Cell text drew dark pixels inside the body cell.
        let dark = ((body.x0 + 5.0) as u32..(body.x1 - 2.0) as u32)
            .flat_map(|x| ((body.y0 + 2.0) as u32..(body.y1 - 2.0) as u32).map(move |y| (x, y)))
            .filter(|&(x, y)| img.pixel(x, y)[0] < 100)
            .count();
        assert!(dark > 30, "cell text pixels: {dark}");
    }
}
