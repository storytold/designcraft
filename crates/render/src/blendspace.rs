//! CMYK transparency using two passes of the ordinary scene renderer.
//!
//! Separable PDF blend functions operate on the complements of the four inks. Render C/M/Y
//! complements into RGB and K into a second raster, keeping the existing transparency groups,
//! masks, parent-page placement and effects. Convert the final inks to sRGB once, then put the
//! transparent result over paper. Work is independent of the number of blended objects.
//!
//! Nonseparable modes need a four-channel implementation: a spread containing one keeps the
//! existing RGB/proof approximation instead of applying RGB Hue/Color formulas to CMY channels.

use crate::{Frame, Placed, RenderOptions, Rendered, Renderer};
use designcraft_color::BlendMode as DcBlend;
use designcraft_doc::{Document, Item, SpreadRef};
use designcraft_geom::{Affine, Rect, Shape};
use vello_cpu::{RenderContext, peniko};

/// At most one viewport, capped at 64 MiB. The snapshot shares the document's large Arcs;
/// comparisons use their identity without scanning text or image bytes. Keeping it prevents address reuse.
/// All document fields, colour settings and options participate, including parent pages,
/// stories, assets, layer visibility and font scope. `clear_caches` releases it too.
pub(crate) struct CachedView {
    doc: Document,
    settings: designcraft_color::cms::ColorSettings,
    opts: RenderOptions,
    spreads: Vec<Placed>,
    view: Affine,
    image: Rendered,
    stats: crate::FrameStats,
    threads: u16,
}
impl CachedView {
    pub(crate) const MAX_BYTES: usize = 64 << 20;
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn matches(
        &self,
        doc: &Document,
        spreads: &[Placed],
        width: u32,
        height: u32,
        view: Affine,
        opts: &RenderOptions,
        threads: u16,
    ) -> bool {
        self.image.width == width.clamp(1, crate::MAX_SIDE)
            && self.image.height == height.clamp(1, crate::MAX_SIDE)
            && self.view == view
            && self.opts == *opts
            && self.spreads == spreads
            && self.threads == threads
            && self.settings == designcraft_color::cms::active_settings()
            && same_document(&self.doc, doc)
    }
    pub(crate) fn image(&self) -> Rendered {
        self.image.clone()
    }
    pub(crate) fn stats(&self) -> crate::FrameStats {
        self.stats
    }
    pub(crate) fn new(
        doc: &Document,
        spreads: &[Placed],
        view: Affine,
        opts: &RenderOptions,
        image: &Rendered,
        stats: crate::FrameStats,
        threads: u16,
    ) -> Self {
        Self {
            doc: doc.clone(),
            settings: designcraft_color::cms::active_settings(),
            opts: opts.clone(),
            spreads: spreads.to_vec(),
            view,
            image: image.clone(),
            stats,
            threads,
        }
    }
}
/// Compare every field, but keep large immutable data on an identity-only path. Exhaustive
/// destructuring makes a new document field require an explicit cache invalidation decision.
fn same_document(old: &Document, new: &Document) -> bool {
    let Document {
        spreads,
        parents,
        stories,
        styles,
        assets,
        title,
        settings,
        layers,
        swatches,
        color_groups,
        xml,
        articles,
        stroke_styles,
        inks,
        endnote_options,
        endnote_story,
        conditions,
        sections,
        hyperlinks,
        data_merge,
        bookmarks,
        user_words,
        hyphenation_exceptions,
        toc,
        text_variables,
        footnote_options,
        xref_formats,
        index,
        created,
        modified,
        next_id,
        font_scope,
    } = new;
    let same_spreads = |a: &[std::sync::Arc<designcraft_doc::Spread>], b: &[std::sync::Arc<designcraft_doc::Spread>]| {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| std::sync::Arc::ptr_eq(a, b))
    };
    same_spreads(&old.spreads, spreads)
        && same_spreads(&old.parents, parents)
        && std::sync::Arc::ptr_eq(&old.styles, styles)
        && old.stories.len() == stories.len()
        && old.stories.iter().zip(stories).all(|((a, x), (b, y))| a == b && std::sync::Arc::ptr_eq(x, y))
        && old.assets.len() == assets.len()
        && old.assets.iter().zip(assets).all(|((a, x), (b, y))| a == b && std::sync::Arc::ptr_eq(x, y))
        && old.title == *title
        && old.settings == *settings
        && old.layers == *layers
        && old.swatches == *swatches
        && old.color_groups == *color_groups
        && old.xml == *xml
        && old.articles == *articles
        && old.stroke_styles == *stroke_styles
        && old.inks == *inks
        && old.endnote_options == *endnote_options
        && old.endnote_story == *endnote_story
        && old.conditions == *conditions
        && old.sections == *sections
        && old.hyperlinks == *hyperlinks
        && old.data_merge == *data_merge
        && old.bookmarks == *bookmarks
        && old.user_words == *user_words
        && old.hyphenation_exceptions == *hyphenation_exceptions
        && old.toc == *toc
        && old.text_variables == *text_variables
        && old.footnote_options == *footnote_options
        && old.xref_formats == *xref_formats
        && old.index == *index
        && old.created == *created
        && old.modified == *modified
        && old.next_id == *next_id
        && old.font_scope == *font_scope
}

fn supported(mode: DcBlend) -> bool {
    !matches!(mode, DcBlend::Hue | DcBlend::Saturation | DcBlend::Color | DcBlend::Luminosity)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pipeline {
    Rgb,
    Cmyk,
    Proof,
}

/// Whether a visible spread needs, and can use, the ink-channel pipeline.
pub(crate) fn uses_cmyk(doc: &Document, sr: SpreadRef, opts: &RenderOptions) -> bool {
    pipeline(doc, sr, opts) == Pipeline::Cmyk
}
/// The legacy approximation is retained only for unsupported nonseparable blends.
pub(crate) fn uses_rgb_proof(doc: &Document, sr: SpreadRef, opts: &RenderOptions) -> bool {
    pipeline(doc, sr, opts) == Pipeline::Proof
}

fn pipeline(doc: &Document, sr: SpreadRef, opts: &RenderOptions) -> Pipeline {
    if !opts.blend_space_view || opts.plate.is_some() || doc.settings.blend_space != designcraft_doc::BlendSpace::Cmyk {
        return Pipeline::Rgb;
    }
    let Some(sp) = doc.spread(sr) else { return Pipeline::Rgb };
    let mut transparent = false;
    let mut inspect = |item: &Item| {
        if !doc.layer(item.layer).is_some_and(|l| l.visible && (!opts.printing_only || l.printable)) {
            return true;
        }
        inspect_item(doc, item, opts, 0, &mut transparent)
    };
    for item in &sp.items {
        if !inspect(item) {
            return Pipeline::Proof;
        }
    }
    if let SpreadRef::Doc(si) = sr {
        let first = doc.first_page_of_spread(si);
        for (pi, page) in sp.pages.iter().enumerate() {
            if !page.show_parent_items {
                continue;
            }
            let Some((ppi, ppage)) = doc.parent_page_for(first.saturating_add(pi)) else { continue };
            let Some(parent) = doc.parents.get(ppi) else { continue };
            for item in &parent.items {
                if page.overridden.contains(&item.id) || parent.pages.len() > 1 && parent.page_at_x(item.bounds().center().x) != Some(ppage) {
                    continue;
                }
                if !inspect(item) {
                    return Pipeline::Proof;
                }
            }
        }
    }
    if transparent { Pipeline::Cmyk } else { Pipeline::Rgb }
}

/// Bounded traversal with no item-count-sized temporary allocation, including anchored items.
fn inspect_item(doc: &Document, it: &Item, opts: &RenderOptions, depth: usize, transparent: &mut bool) -> bool {
    if it.hidden || opts.hidden.contains(&it.id) || opts.printing_only && it.nonprinting {
        return true;
    }
    if depth >= 64 || !supported(it.blend) {
        return false;
    }
    *transparent |= it.opacity < 1.0 || it.blend != DcBlend::Normal || it.effects.any() || it.knockout;
    if let Some(text) = it.text_frame()
        && let Some(story) = doc.story(text.story)
    {
        for object in &story.objects {
            if !inspect_item(doc, &object.item, opts, depth + 1, transparent) {
                return false;
            }
        }
    }
    it.shown_children().all(|child| inspect_item(doc, child, opts, depth + 1, transparent))
}

const RGB_N: usize = 9;

/// The working CMYK → sRGB table for the current colour settings (9⁴, built once per settings);
/// sample it with [`srgb_in`].
fn srgb_table() -> std::sync::Arc<Vec<[f32; 3]>> {
    const N: usize = RGB_N;
    type Lut = (designcraft_color::cms::ColorSettings, std::sync::Arc<Vec<[f32; 3]>>);
    static LUT: std::sync::Mutex<Option<Lut>> = std::sync::Mutex::new(None);
    let settings = designcraft_color::cms::active_settings();
    let mut g = LUT.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some((s, t)) if *s == settings => t.clone(),
        _ => {
            let cms = designcraft_color::cms::active();
            let step = |i: usize| i as f32 / (N - 1) as f32;
            let t: Vec<[f32; 3]> = (0..N * N * N * N)
                .map(|i| cms.cmyk_to_srgb([step(i / (N * N * N)), step(i / (N * N) % N), step(i / N % N), step(i % N)], false))
                .collect();
            let t = std::sync::Arc::new(t);
            *g = Some((settings, t.clone()));
            t
        }
    }
}

/// Quadrilinear lookup of `cmyk` in an [`srgb_table`].
fn srgb_in(table: &[[f32; 3]], cmyk: [f32; 4]) -> [f32; 3] {
    const N: usize = RGB_N;
    let f = cmyk.map(|v| v.clamp(0.0, 1.0) * (N - 1) as f32);
    let i = f.map(|v| (v as usize).min(N - 2));
    let d = [f[0] - i[0] as f32, f[1] - i[1] as f32, f[2] - i[2] as f32, f[3] - i[3] as f32];
    let mut out = [0.0f32; 3];
    for corner in 0..16usize {
        let bit = |n: usize| corner >> (3 - n) & 1;
        let w: f32 = (0..4).map(|n| if bit(n) == 1 { d[n] } else { 1.0 - d[n] }).product();
        if w == 0.0 {
            continue;
        }
        let idx = (i[0] + bit(0)) * N * N * N + (i[1] + bit(1)) * N * N + (i[2] + bit(2)) * N + i[3] + bit(3);
        if let Some(rgb) = table.get(idx) {
            for k in 0..3 {
                out[k] += rgb[k] * w;
            }
        }
    }
    out
}

impl Renderer {
    /// The view pixels the spread's transparency can change: the union of its transparent
    /// objects (opacity, blend, effects, knockout, also nested or anchored) with their effects'
    /// reach. None: the whole view (a transparent parent-page item, placed through the parent
    /// page). Empty: nothing transparent is visible.
    fn transparent_area(&self, f: &Frame, sr: SpreadRef) -> Option<Rect> {
        let doc = f.doc;
        let sp = doc.spread(sr)?;
        let visible = |item: &Item| doc.layer(item.layer).is_some_and(|l| l.visible && (!f.opts.printing_only || l.printable));
        let transparent = |item: &Item| {
            let mut t = false;
            inspect_item(doc, item, f.opts, 0, &mut t);
            t
        };
        if let SpreadRef::Doc(si) = sr {
            let first = doc.first_page_of_spread(si);
            for (pi, page) in sp.pages.iter().enumerate() {
                if !page.show_parent_items {
                    continue;
                }
                let Some((ppi, _)) = doc.parent_page_for(first.saturating_add(pi)) else { continue };
                if doc
                    .parents
                    .get(ppi)
                    .is_some_and(|parent| parent.items.iter().any(|it| visible(it) && !page.overridden.contains(&it.id) && transparent(it)))
                {
                    return None;
                }
            }
        }
        // What an object can paint on the spread: its bounds with its (and its children's)
        // strokes and effects.
        let reach = |it: &Item| {
            let mut o: f64 = 0.0;
            it.walk(&mut |c| o = o.max(crate::fx::outset(c) + c.stroke.extent()));
            it.bounds().inflate(o + 2.0, o + 2.0)
        };
        let items: Vec<(Rect, bool)> = sp.items.iter().filter(|it| visible(it)).map(|it| (reach(it), transparent(it))).collect();
        let mut area: Option<Rect> = None;
        for (r, _) in items.iter().filter(|(_, t)| *t) {
            area = Some(area.map_or(*r, |a| a.union(*r)));
        }
        // Objects the area touches join it whole, so none is drawn partly through the ink passes
        // and partly through the ordinary drawing (their colour conversions differ slightly).
        let Some(mut area) = area else { return Some(Rect::ZERO) };
        for _ in 0..items.len() {
            let grown = items.iter().filter(|(r, _)| r.intersect(area).area() > 0.0).fold(area, |a, (r, _)| a.union(*r));
            if grown == area {
                break;
            }
            area = grown;
        }
        Some(f.view.transform_rect_bbox(area))
    }

    /// The spread with its transparency blended in CMYK. Paper has already been painted in `ctx`;
    /// it must never be a blend backdrop. Only the area transparent objects can change takes the
    /// two ink passes (C/M/Y complements, then K); the rest of the view is drawn once, as usual.
    pub(crate) fn draw_cmyk_spread(&mut self, ctx: &mut RenderContext, f: &Frame, sr: SpreadRef) {
        let (w, h) = (ctx.width(), ctx.height());
        let view = Rect::new(0.0, 0.0, w as f64, h as f64);
        let area = self.transparent_area(f, sr).unwrap_or(view);
        let area = Rect::new(area.x0.floor() - 1.0, area.y0.floor() - 1.0, area.x1.ceil() + 1.0, area.y1.ceil() + 1.0).intersect(view);
        // The document grid is drawn once, by the caller, behind and in front of all this.
        let rgb = RenderOptions {
            paper: false,
            background: None,
            page_shadow: false,
            blend_space_view: false,
            document_grid: Vec::new(),
            ..f.opts.clone()
        };
        if area.width() < 1.0 || area.height() < 1.0 {
            // Nothing transparent in view: the ordinary drawing is exact.
            self.draw_spread(ctx, &Frame { opts: &rgb, ..*f }, sr);
            return;
        }
        // Outside the area: the ordinary drawing (an even-odd clip of the view minus the area).
        if area != view {
            let mut outside = view.to_path(0.1);
            outside.extend(area.to_path(0.1).iter());
            ctx.set_transform(Affine::IDENTITY);
            ctx.set_fill_rule(peniko::Fill::EvenOdd);
            ctx.push_clip_layer(&outside);
            ctx.set_fill_rule(peniko::Fill::NonZero);
            self.draw_spread(ctx, &Frame { opts: &rgb, ..*f }, sr);
            ctx.pop_layer();
        }
        // Inside it: the two ink passes, with the render threads, over just the area.
        let (aw, ah) = (area.width() as u16, area.height() as u16);
        let shifted = Affine::translate((-area.x0, -area.y0)) * f.view;
        // The passes draw only what reaches the area: items elsewhere are culled.
        let visible = f.view.inverse().transform_rect_bbox(area);
        let stats = self.stats;
        let threads = self.threads;
        let mut pass = |plate| {
            let opts = RenderOptions { plate: Some(plate), ..rgb.clone() };
            crate::PLATE.with(|p| p.set(opts.plate));
            let mut off = RenderContext::new_with(aw, ah, vello_cpu::RenderSettings { num_threads: threads, ..Default::default() });
            #[cfg(test)]
            let items = self.stats.items;
            self.draw_spread(&mut off, &Frame { opts: &opts, view: shifted, visible, mt: threads > 0, ..*f }, sr);
            #[cfg(test)]
            {
                self.cmyk_pass_items = self.stats.items - items;
            }
            off.flush();
            let mut pm = vello_cpu::Pixmap::new(aw, ah);
            off.render(&mut pm, &mut self.resources);
            #[cfg(test)]
            {
                self.cmyk_passes += 1;
                self.cmyk_pass_pixels = aw as usize * ah as usize;
            }
            pm
        };
        let mut cmy = pass(4);
        let black = pass(3);
        self.cmyk_ran = true;
        crate::PLATE.with(|p| p.set(f.opts.plate));
        self.stats.items = (self.stats.items - stats.items) / 2 + stats.items;
        self.stats.glyphs = (self.stats.glyphs - stats.glyphs) / 2 + stats.glyphs;
        let table = srgb_table();
        for (c, k) in cmy.data_mut().iter_mut().zip(black.data()) {
            if c.a == 0 {
                continue;
            }
            let a = c.a as f32;
            let inks = [1.0 - c.r as f32 / a, 1.0 - c.g as f32 / a, 1.0 - c.b as f32 / a, 1.0 - k.r as f32 / a];
            let rgb = if f.opts.rich_black && inks[0..3].iter().all(|v| *v <= 1e-3) && inks[3] >= 0.999 { [0.0; 3] } else { srgb_in(&table, inks) };
            let [r, g, b] = rgb.map(|v| (v.clamp(0.0, 1.0) * a).round() as u8);
            (c.r, c.g, c.b) = (r, g, b);
        }
        ctx.set_transform(Affine::translate((area.x0, area.y0)));
        ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(std::sync::Arc::new(cmy)), sampler: peniko::ImageSampler::default() });
        ctx.fill_rect(&Rect::new(0.0, 0.0, aw as f64, ah as f64));
        ctx.set_transform(Affine::IDENTITY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_compose::Cache;
    use designcraft_doc::{Content, Fill, ItemId, Shape, build::NewDocument};
    use std::sync::Arc;

    fn document() -> Document {
        let mut doc = Document::new(&NewDocument { width: 160.0, height: 120.0, facing_pages: false, ..Default::default() });
        doc.swatches.push(designcraft_color::swatch::Swatch::color("Lilac", designcraft_color::Color::rgb8(171, 154, 227)));
        doc
    }
    fn rectangle(doc: &mut Document, sr: SpreadRef, rect: Rect, fill: &str, blend: DcBlend) -> ItemId {
        let id = ItemId(doc.alloc());
        let mut item = Item::new(id, doc.default_layer(), Shape::Rectangle, designcraft_geom::shapes::rectangle(rect));
        item.fill = Fill::swatch(fill);
        item.blend = blend;
        doc.insert_item(sr, item, None).unwrap();
        id
    }
    fn draw(doc: &Document, renderer: &mut Renderer) -> Rendered {
        renderer.render(
            doc,
            &Cache::new(),
            &[Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }],
            200,
            140,
            Affine::translate((30.0, 10.0)),
            &RenderOptions { blend_space_view: true, background: Some([180, 180, 180, 255]), ..Default::default() },
        )
    }
    fn dodge(doc: &mut Document, sr: SpreadRef) -> ItemId {
        rectangle(doc, sr, Rect::new(-25.0, 10.0, 130.0, 70.0), "Lilac", DcBlend::Normal);
        rectangle(doc, sr, Rect::new(-20.0, 25.0, 100.0, 100.0), designcraft_color::swatch::BLACK, DcBlend::ColorDodge)
    }
    fn white(pixel: [u8; 4]) {
        assert!(pixel[..3].iter().all(|v| *v > 235), "{pixel:?}");
    }

    #[test]
    fn cmyk_nested_blends_match_top_level_blends() {
        let mut doc = document();
        let black = dodge(&mut doc, SpreadRef::Doc(0));
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let reference = draw(&doc, &mut renderer);
        let child = doc.remove_item(black).unwrap();
        let mut group = Item::new(ItemId(doc.alloc()), doc.default_layer(), Shape::Group, Default::default());
        group.content = Content::Group { items: vec![child] };
        // A second group ensures the preflight and the rendering both follow nesting.
        let mut outer = Item::new(ItemId(doc.alloc()), doc.default_layer(), Shape::Group, Default::default());
        outer.content = Content::Group { items: vec![Arc::new(group)] };
        doc.insert_item(SpreadRef::Doc(0), outer, None).unwrap();
        let nested = draw(&doc, &mut renderer);
        white(nested.pixel(70, 50));
        assert_eq!(nested.pixels, reference.pixels);
    }

    #[test]
    fn cmyk_parent_blends_and_overrides_follow_parent_visibility() {
        let mut doc = document();
        let black = dodge(&mut doc, SpreadRef::Parent(0));
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let shown = draw(&doc, &mut renderer);
        white(shown.pixel(70, 50));
        Arc::make_mut(&mut doc.spreads[0]).pages[0].overridden.push(black);
        let overridden = draw(&doc, &mut renderer);
        assert_ne!(shown.pixel(70, 50), overridden.pixel(70, 50));
        Arc::make_mut(&mut doc.spreads[0]).pages[0].show_parent_items = false;
        let hidden = draw(&doc, &mut renderer);
        assert_eq!(hidden.pixel(70, 50), [255; 4]);
    }

    #[test]
    fn cmyk_blending_continues_across_trim_and_keeps_paper_out_of_backdrop() {
        let mut doc = document();
        dodge(&mut doc, SpreadRef::Doc(0));
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let image = draw(&doc, &mut renderer);
        // x=30 is the left trim. Identical overlap on either side must have identical inks.
        white(image.pixel(20, 50));
        assert_eq!(image.pixel(20, 50), image.pixel(40, 50));
        assert!(image.pixel(70, 95)[..3].iter().all(|v| *v < 90));
        assert_eq!(image.pixel(5, 130), [180, 180, 180, 255]);
        // Optional synthetic preview for manual visual QA, never a checked-in fixture.
        if let Ok(path) = std::env::var("DESIGNCRAFT_CMYK_PREVIEW") {
            std::fs::write(path, image.to_png()).unwrap();
        }
    }

    #[test]
    fn cmyk_effects_blend_with_the_ink_backdrop_even_with_render_threads() {
        let mut doc = document();
        let item = rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(20.0, 20.0, 120.0, 100.0), "C=100 M=0 Y=0 K=0", DcBlend::Normal);
        let satin = &mut doc.item_mut(item).unwrap().effects.satin;
        satin.on = true;
        satin.color = "C=0 M=100 Y=0 K=0".into();
        satin.opacity = 1.0;
        satin.size = 0.0;
        satin.angle = 0.0;
        satin.distance = 40.0;
        let mut renderer = Renderer::new();
        renderer.threads = 2;
        let image = draw(&doc, &mut renderer);
        let expected = srgb_in(&srgb_table(), [1.0, 1.0, 0.0, 0.0]).map(|v| (v * 255.0).round() as u8);
        let actual = image.pixel(60, 60);
        assert!(actual[..3].iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3), "Multiply satin: {actual:?}, expected {expected:?}");
        renderer.threads = 0;
        assert_eq!(draw(&doc, &mut renderer).pixels, image.pixels);
        // The effect's blend must also see the object's backdrop in the existing RGB path,
        // including the multithreaded offscreen-filter implementation.
        doc.settings.blend_space = designcraft_doc::BlendSpace::Rgb;
        let rgb = draw(&doc, &mut renderer);
        let mixed = rgb.pixel(60, 60);
        assert!(mixed[0] < 100 && mixed[1] < 100 && mixed[2] > 100, "cyan times magenta is blue, not magenta: {mixed:?}");
        renderer.threads = 2;
        assert_eq!(draw(&doc, &mut renderer).pixels, rgb.pixels);
    }

    #[test]
    fn cmyk_pass_count_is_constant_and_unchanged_views_are_cached() {
        let mut doc = document();
        let mut ids = Vec::new();
        for _ in 0..10 {
            ids.push(rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(20.0, 20.0, 100.0, 100.0), "C=100 M=0 Y=0 K=0", DcBlend::Multiply));
        }
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let before = draw(&doc, &mut renderer);
        assert_eq!(renderer.cmyk_passes, 2, "ten Multiply objects still need only two ink passes");
        assert_eq!(draw(&doc, &mut renderer).pixels, before.pixels);
        assert_eq!(renderer.cmyk_passes, 2, "an unchanged frame performs no ink passes");
        doc.item_mut(ids[9]).unwrap().fill = Fill::swatch(designcraft_color::swatch::BLACK);
        let after = draw(&doc, &mut renderer);
        assert_ne!(after.pixel(70, 50), before.pixel(70, 50));
        assert_eq!(renderer.cmyk_passes, 4);
        renderer.clear_caches();
        assert!(renderer.cmyk_view.is_none());
        assert_eq!(draw(&doc, &mut renderer).pixels, after.pixels);
        assert_eq!(renderer.cmyk_passes, 6);
        let (_, story) =
            doc.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 160.0, 30.0), doc.default_layer(), "Hello", Default::default()).unwrap();
        let text_before = draw(&doc, &mut renderer);
        doc.story_mut(story).unwrap().insert(5, " world");
        let text_after = draw(&doc, &mut renderer);
        assert_ne!(text_before.pixels, text_after.pixels, "story edits invalidate the viewport");
        assert_eq!(renderer.cmyk_passes, 10);
    }

    #[test]
    fn cmyk_passes_cover_only_what_transparency_can_change() {
        // A 200×140 view; one small Multiply object in a corner, a solid one far from it.
        let mut doc = document();
        rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(100.0, 70.0, 150.0, 110.0), "C=100 M=0 Y=0 K=0", DcBlend::Normal);
        rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(5.0, 5.0, 25.0, 25.0), "C=0 M=100 Y=0 K=0", DcBlend::Multiply);
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let image = draw(&doc, &mut renderer);
        assert_eq!(renderer.cmyk_passes, 2);
        assert!(renderer.cmyk_pass_pixels < 200 * 140 / 8, "{} px", renderer.cmyk_pass_pixels);
        // The solid object is drawn as before, its colour exact.
        let cyan = RenderOptions { blend_space_view: false, background: Some([180, 180, 180, 255]), ..Default::default() };
        let plain = renderer.render(
            &doc,
            &Cache::new(),
            &[Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }],
            200,
            140,
            Affine::translate((30.0, 10.0)),
            &cyan,
        );
        assert_eq!(image.pixel(150, 100), plain.pixel(150, 100));
        // An object the area touches is drawn whole in the ink passes (no seam across it).
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(15.0, 15.0, 90.0, 60.0), "C=0 M=0 Y=100 K=0", DcBlend::Normal);
        draw(&doc, &mut renderer);
        assert!(renderer.cmyk_pass_pixels >= 75 * 45, "{} px", renderer.cmyk_pass_pixels);
    }

    #[test]
    fn ink_passes_draw_only_the_items_that_reach_their_area() {
        // A small Multiply object in a corner and a solid one far from it: the ink passes cover the
        // corner and draw just the Multiply object.
        let mut doc = document();
        rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(100.0, 70.0, 150.0, 110.0), "C=100 M=0 Y=0 K=0", DcBlend::Normal);
        rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(5.0, 5.0, 25.0, 25.0), "C=0 M=100 Y=0 K=0", DcBlend::Multiply);
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        draw(&doc, &mut renderer);
        assert_eq!(renderer.cmyk_passes, 2);
        assert_eq!(renderer.cmyk_pass_items, 1);
    }

    #[test]
    fn the_view_is_kept_only_when_the_ink_passes_ran() {
        let mut doc = document();
        doc.settings.blend_space = designcraft_doc::BlendSpace::Cmyk;
        let id = rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(20.0, 20.0, 100.0, 100.0), "C=100 M=0 Y=0 K=0", DcBlend::Normal);
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        draw(&doc, &mut renderer);
        assert!(renderer.cmyk_view.is_none(), "nothing transparent: no ink passes, nothing kept");
        doc.item_mut(id).unwrap().blend = DcBlend::Multiply;
        draw(&doc, &mut renderer);
        assert!(renderer.cmyk_view.is_some());
    }

    #[test]
    fn cmyk_channels_keep_the_same_alpha_through_transparency() {
        let mut doc = document();
        let a = rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(20.0, 20.0, 100.0, 100.0), "C=100 M=0 Y=0 K=0", DcBlend::Normal);
        let b = rectangle(&mut doc, SpreadRef::Doc(0), Rect::new(40.0, 20.0, 120.0, 100.0), "C=0 M=100 Y=0 K=0", DcBlend::Multiply);
        doc.item_mut(a).unwrap().opacity = 0.5;
        doc.item_mut(b).unwrap().opacity = 0.5;
        doc.item_mut(b).unwrap().effects.gradient_feather.on = true;
        let children = vec![doc.remove_item(a).unwrap(), doc.remove_item(b).unwrap()];
        let mut group = Item::new(ItemId(doc.alloc()), doc.default_layer(), Shape::Group, Default::default());
        group.content = Content::Group { items: children };
        group.opacity = 0.5;
        group.isolate = true;
        doc.insert_item(SpreadRef::Doc(0), group, None).unwrap();
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let placements = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
        let options = RenderOptions { paper: false, blend_space_view: true, ..Default::default() };
        let out = renderer.render(&doc, &Cache::new(), &placements, 160, 120, Affine::IDENTITY, &options);
        assert!(out.pixel(30, 40)[3].abs_diff(64) <= 1, "half-opacity object inside a half-opacity group is quarter opaque");
        let cmy = renderer.render(&doc, &Cache::new(), &placements, 160, 120, Affine::IDENTITY, &RenderOptions { plate: Some(4), ..options.clone() });
        let k = renderer.render(&doc, &Cache::new(), &placements, 160, 120, Affine::IDENTITY, &RenderOptions { plate: Some(3), ..options });
        for ((out, cmy), k) in out.pixels.as_chunks::<4>().0.iter().zip(cmy.pixels.as_chunks::<4>().0).zip(k.pixels.as_chunks::<4>().0) {
            assert_eq!(cmy[3], k[3], "all ink passes have the same coverage");
            assert_eq!(out[3], cmy[3], "conversion preserves coverage");
            assert!(out[..3].iter().all(|v| *v <= out[3]), "output remains premultiplied");
        }
    }

    #[test]
    fn cmyk_nonseparable_modes_use_the_existing_rgb_approximation() {
        let mut doc = document();
        let id = dodge(&mut doc, SpreadRef::Doc(0));
        doc.item_mut(id).unwrap().blend = DcBlend::Hue;
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        draw(&doc, &mut renderer);
        assert_eq!(renderer.cmyk_passes, 0, "RGB Hue must not be evaluated on CMY channels");
        doc.item_mut(id).unwrap().blend = DcBlend::ColorDodge;
        draw(&doc, &mut renderer);
        assert_eq!(renderer.cmyk_passes, 2);
    }
}
