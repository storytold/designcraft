use super::types::{Frame, Rendered};
use super::{Affine, BlendMode, Compose, DcBlend, Document, Item, Mix, Rect, RenderContext, fx, peniko, vello_cpu};

const CMYK_N: usize = 17;

/// The sRGB → working CMYK table for the current colour settings (17³, built once per settings,
/// trilinear);
/// sample it with [`cmyk_in`]. Fetch it once for many pixels.
pub(crate) fn cmyk_table() -> std::sync::Arc<Vec<[f32; 4]>> {
    const N: usize = CMYK_N;
    type Lut = (designcraft_color::cms::ColorSettings, std::sync::Arc<Vec<[f32; 4]>>);
    static LUT: std::sync::Mutex<Option<Lut>> = std::sync::Mutex::new(None);
    let settings = designcraft_color::cms::active_settings();
    let mut g = LUT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match g.as_ref() {
        Some((s, t)) if *s == settings => t.clone(),
        _ => {
            let cms = designcraft_color::cms::active();
            let step = |i: usize| i as f32 / (N - 1) as f32;
            let t: Vec<[f32; 4]> = (0..N * N * N)
                .map(|i| cms.srgb_to_cmyk([step(i / (N * N)), step(i / N % N), step(i % N)], designcraft_color::cms::Intent::RelativeColorimetric))
                .collect();
            let t = std::sync::Arc::new(t);
            *g = Some((settings, t.clone()));
            t
        }
    }
}

/// Trilinear lookup of `rgb` in a [`cmyk_table`].
pub(crate) fn cmyk_in(table: &[[f32; 4]], rgb: [f32; 3]) -> [f32; 4] {
    const N: usize = CMYK_N;
    let f = rgb.map(|v| v.clamp(0.0, 1.0) * (N - 1) as f32);
    let i = f.map(|v| (v as usize).min(N - 2));
    let d = [f[0] - i[0] as f32, f[1] - i[1] as f32, f[2] - i[2] as f32];
    let mut out = [0.0f32; 4];
    for (corner, w) in (0..8).map(|c| {
        let (a, b, cc) = (c >> 2 & 1, c >> 1 & 1, c & 1);
        let w = (if a == 1 { d[0] } else { 1.0 - d[0] }) * (if b == 1 { d[1] } else { 1.0 - d[1] }) * (if cc == 1 { d[2] } else { 1.0 - d[2] });
        ((i[0] + a) * N * N + (i[1] + b) * N + i[2] + cc, w)
    }) {
        if let Some(t) = table.get(corner) {
            for k in 0..4 {
                out[k] += t[k] * w;
            }
        }
    }
    out
}

/// How far (points) any effect in `doc` can carry an image's pixels: an image is converted to a
/// plate only where the view can show it, widened by this so shadows and glows near the edge of
/// the view still find their pixels. Generous on purpose: it only makes the converted part bigger.
pub(crate) fn effect_reach(doc: &Document) -> f64 {
    let mut reach = 0.0;
    let mut add = |it: &Item| {
        if it.effects.any() {
            reach += fx::outset(it) + 144.0;
        }
    };
    for it in doc.spreads.iter().flat_map(|sp| &sp.items).chain(doc.parents.iter().flat_map(|p| &p.items)) {
        it.walk(&mut add);
    }
    reach
}

/// How far the effects of the object showing graphic `g` and of the groups around it can carry
/// its pixels: only those move or spread this image. None when the graphic isn't found on a page
/// (an anchored object in a story): then [`effect_reach`] covers it.
pub(crate) fn image_effect_reach(doc: &Document, g: &designcraft_doc::Graphic) -> Option<f64> {
    fn find(it: &Item, g: &designcraft_doc::Graphic, around: f64, depth: usize) -> Option<f64> {
        let reach = around + if it.effects.any() { fx::outset(it) + 144.0 } else { 0.0 };
        if it.graphic().is_some_and(|own| std::ptr::eq(own, g)) {
            return Some(reach);
        }
        if depth >= 64 {
            return None;
        }
        it.children().iter().find_map(|c| find(c, g, reach, depth + 1))
    }
    doc.spreads.iter().flat_map(|sp| &sp.items).chain(doc.parents.iter().flat_map(|p| &p.items)).find_map(|it| find(it, g, 0.0, 0))
}

/// An image's plate (see [`plate_pixmap`]) where `area` (spread space; `to_pixels` maps it to the
/// image's pixels) can show it, and where that part starts in the image. Small images are
/// converted whole and cached. A large one (a 3840 × 5760 photo at full resolution, zoomed in)
/// would take seconds to convert and crowd the other images out of the cache, so only the part in
/// view is converted. None: none of it is in view.
pub(crate) fn plate_part(
    pm: &std::sync::Arc<vello_cpu::Pixmap>,
    plate: u8,
    to_pixels: Affine,
    area: impl FnOnce() -> Rect,
) -> Option<(std::sync::Arc<vello_cpu::Pixmap>, (u16, u16))> {
    let (w, h) = (pm.width(), pm.height());
    if plate_bytes(w, h) <= PLATE_CACHE_BYTES / 8 {
        return Some((plate_pixmap(pm, plate), (0, 0)));
    }
    // Bilinear and bicubic sampling read a few pixels around each one shown.
    let r = to_pixels.transform_rect_bbox(area()).inflate(4.0, 4.0);
    if !(r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()) {
        return Some((plate_pixmap(pm, plate), (0, 0)));
    }
    let clamp = |v: f64, max: u16| v.clamp(0.0, f64::from(max)) as u16;
    let (x0, y0, x1, y1) = (clamp(r.x0.floor(), w), clamp(r.y0.floor(), h), clamp(r.x1.ceil(), w), clamp(r.y1.ceil(), h));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    // All or nearly all of it in view (three quarters or more): convert the whole plate once and
    // keep it, as long as it takes at most half the cache (kept whole, a bigger one would push
    // every other image out). Converting it in parts would redo nearly all of it on every render.
    let part_area = u64::from(x1 - x0) * u64::from(y1 - y0);
    let whole_area = u64::from(w) * u64::from(h);
    if part_area.saturating_mul(4) >= whole_area.saturating_mul(3) && plate_bytes(w, h) <= PLATE_CACHE_BYTES / 2 {
        return Some((plate_pixmap(pm, plate), (0, 0)));
    }
    let row = usize::from(w);
    let data = pm.data();
    let mut part = Vec::with_capacity(usize::from(x1 - x0) * usize::from(y1 - y0));
    for y in usize::from(y0)..usize::from(y1) {
        let start = y * row;
        part.extend_from_slice(data.get(start + usize::from(x0)..start + usize::from(x1))?);
    }
    let table = cmyk_table();
    let part = plate_pixels(&part, &table, plate);
    Some((std::sync::Arc::new(vello_cpu::Pixmap::from_parts(part, x1 - x0, y1 - y0)), (x0, y0)))
}

/// Plate conversions cached per image and plate: at most this much derived raster memory.
pub(crate) const PLATE_CACHE_BYTES: usize = 192 << 20;

/// What one plate of a `w` × `h` image costs the cache (premultiplied RGBA8).
fn plate_bytes(w: u16, h: u16) -> usize {
    usize::from(w).saturating_mul(usize::from(h)).saturating_mul(4)
}

/// Premultiplied pixels' ink coverage on `plate` (4: C, M and Y complements as R, G and B).
fn plate_pixels(pixels: &[vello_cpu::color::PremulRgba8], table: &[[f32; 4]], plate: u8) -> Vec<vello_cpu::color::PremulRgba8> {
    pixels
        .iter()
        .map(|p| {
            if p.a == 0 {
                return *p;
            }
            let k = 255.0 / p.a as f32;
            let rgb = [p.r, p.g, p.b].map(|v| (v as f32 * k / 255.0).min(1.0));
            let cmyk = cmyk_in(table, rgb);
            let ink = |i: usize| ((1.0 - cmyk[i].clamp(0.0, 1.0)) * p.a as f32).round() as u8;
            if plate == 4 {
                return vello_cpu::color::PremulRgba8 { r: ink(0), g: ink(1), b: ink(2), a: p.a };
            }
            let v = ink(plate.min(3) as usize);
            vello_cpu::color::PremulRgba8 { r: v, g: v, b: v, a: p.a }
        })
        .collect()
}

/// An image's ink coverage on one process plate, as grey (cached per image and plate).
pub(crate) fn plate_pixmap(pm: &std::sync::Arc<vello_cpu::Pixmap>, plate: u8) -> std::sync::Arc<vello_cpu::Pixmap> {
    // Keep source allocations alive (their addresses are keys), invalidate on colour settings,
    // and cap derived raster memory. A full cache lets go of the plates used longest ago: every
    // CMYK view render draws each image twice (two ink passes), so dropping the whole cache when
    // it filled made a few placed images miss on every render and converted them again each time.
    type Entry = (std::sync::Arc<vello_cpu::Pixmap>, std::sync::Arc<vello_cpu::Pixmap>, u64);
    type Cache = (designcraft_color::cms::ColorSettings, std::collections::HashMap<(usize, u8), Entry>, usize, u64);
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (std::sync::Arc::as_ptr(pm) as usize, plate);
    let settings = designcraft_color::cms::active_settings();
    {
        let mut guard = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((previous, map, _, clock)) = guard.as_mut() {
            if *previous == settings {
                *clock += 1;
                if let Some((_, derived, used_at)) = map.get_mut(&key) {
                    *used_at = *clock;
                    return derived.clone();
                }
            } else {
                *guard = None;
            }
        }
    }
    let table = cmyk_table();
    #[cfg(test)]
    PLATE_CONVERSIONS.with(|n| n.set(n.get() + 1));
    let data = plate_pixels(pm.data(), &table, plate);
    let out = std::sync::Arc::new(vello_cpu::Pixmap::from_parts(data, pm.width(), pm.height()));
    let bytes = plate_bytes(pm.width(), pm.height());
    if bytes <= PLATE_CACHE_BYTES {
        let mut guard = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (previous, map, used, clock) = guard.get_or_insert_with(|| (settings.clone(), Default::default(), 0, 0));
        if *previous != settings {
            map.clear();
            *used = 0;
            *previous = settings;
        }
        while used.saturating_add(bytes) > PLATE_CACHE_BYTES {
            let Some(oldest) = map.iter().min_by_key(|(_, e)| e.2).map(|(k, _)| *k) else { break };
            if let Some((_, gone, _)) = map.remove(&oldest) {
                *used = used.saturating_sub(plate_bytes(gone.width(), gone.height()));
            }
        }
        *clock += 1;
        if let Some((_, replaced, _)) = map.insert(key, (pm.clone(), out.clone(), *clock)) {
            *used = used.saturating_sub(plate_bytes(replaced.width(), replaced.height()));
        }
        *used = used.saturating_add(bytes);
    }
    out
}

pub(crate) fn rect_overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

thread_local! {
    /// Object Layer Options of the placed PDF being drawn.
    pub(crate) static HIDDEN_LAYERS: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    /// Appearance of Black for the render on this thread: 100% K as pure black.
    pub(crate) static RICH_BLACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The CMYK blend space's ink pass for the render on this thread (`RenderOptions::plate`).
    pub(crate) static PLATE: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

/// Screen indicators and placeholders are display sRGB, independent of the document's RGB
/// working profile. Encode their inks only while drawing a process plate or CMYK blend pass.
pub(crate) fn screen_color(rgb: [u8; 3], alpha: u8) -> peniko::Color {
    if PLATE.with(std::cell::Cell::get).is_none() {
        return peniko::Color::from_rgba8(rgb[0], rgb[1], rgb[2], alpha);
    }
    let [c, m, y, k] = cmyk_in(&cmyk_table(), rgb.map(|v| v as f32 / 255.0));
    color_of(&designcraft_color::Color::Cmyk { c, m, y, k }, alpha as f32 / 255.0)
}

pub fn color_of(c: &designcraft_color::Color, alpha: f32) -> peniko::Color {
    if let Some(p) = PLATE.with(std::cell::Cell::get) {
        let cmyk = match *c {
            designcraft_color::Color::Cmyk { c, m, y, k } => [c, m, y, k],
            _ => c.to_cmyk_managed(designcraft_color::cms::Intent::RelativeColorimetric),
        };
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        let ink = |i: usize| ((1.0 - cmyk[i].clamp(0.0, 1.0)) * 255.0).round() as u8;
        if p == 4 {
            return peniko::Color::from_rgba8(ink(0), ink(1), ink(2), a);
        }
        let v = ink(p.min(3) as usize);
        return peniko::Color::from_rgba8(v, v, v, a);
    }
    if RICH_BLACK.with(std::cell::Cell::get)
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
pub(crate) fn set_fill_paint(ctx: &mut RenderContext, doc: &Document, fill: &designcraft_doc::Fill, bounds: Rect) -> bool {
    let (swatch, tint) = (fill.swatch.as_str(), fill.tint);
    if let Some(g) = designcraft_color::swatch::resolve_gradient(&doc.swatches, swatch) {
        // Sampled so process colours blend as inks, as InDesign shows them (and as the PDF's CMYK
        // shading prints).
        let stops: Vec<peniko::ColorStop> = g.display_stops().iter().map(|(o, c, a)| peniko::ColorStop::from((*o, color_of(c, *a)))).collect();
        if stops.is_empty() {
            return false;
        }
        let grad = match g.kind {
            // Radial: a circle stretched into the object's ellipse (the paint transform).
            designcraft_color::GradientKind::Radial => {
                let (c, rx, ry) = fill.radial_ellipse(bounds);
                let rx = rx.max(1e-3);
                ctx.set_paint_transform(Affine::translate(c.to_vec2()) * Affine::scale_non_uniform(1.0, ry / rx) * Affine::translate(-c.to_vec2()));
                peniko::Gradient::new_radial(c, rx as f32).with_stops(stops.as_slice())
            }
            _ => {
                let (a, b) = fill.linear_axis(bounds);
                peniko::Gradient::new_linear(a, b).with_stops(stops.as_slice())
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

/// Does any item on the spread, or on the parent pages it shows, use a blend mode other than
/// Normal (groups and pasted-in items included)?
pub(crate) fn spread_blends(doc: &Document, sp: &designcraft_doc::Spread) -> bool {
    let blends = |it: &Item| {
        let mut any = false;
        // A screened glow or a shadow with a blend mode blends with the objects below too, not the paper.
        let fx_blends = |e: &designcraft_doc::Effects| {
            (e.outer_glow.on && e.outer_glow.mode != Default::default()) || (e.drop_shadow.on && e.drop_shadow.mode != Default::default())
        };
        it.walk(&mut |i| any |= i.blend != Default::default() || fx_blends(&i.effects));
        any
    };
    sp.items.iter().any(|i| blends(i)) || doc.parents.iter().any(|p| p.items.iter().any(|i| blends(i)))
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
pub(crate) fn blend_space_view(img: &mut Rendered, area: Rect) {
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

#[cfg(test)]
thread_local! {
    /// How many whole images [`plate_pixmap`] converted to a plate on this thread.
    pub(crate) static PLATE_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod plate_tests {
    use designcraft_doc::SpreadRef;
    use designcraft_doc::build::NewDocument;

    use super::*;

    /// A `w` × `h` image with a different colour in every pixel.
    fn ramp(w: u16, h: u16) -> std::sync::Arc<vello_cpu::Pixmap> {
        let data = (0..usize::from(w) * usize::from(h))
            .map(|i| vello_cpu::color::PremulRgba8 { r: (i % 251) as u8, g: (i / 7 % 241) as u8, b: (i / 13 % 239) as u8, a: 255 })
            .collect();
        std::sync::Arc::new(vello_cpu::Pixmap::from_parts(data, w, h))
    }

    /// A CMYK view draws every image twice per render (two ink passes). The plate cache used to
    /// count each plate twice and empty itself when full, so three placed photos missed on every
    /// render and an edit took half a second; now they all stay cached.
    #[test]
    fn plates_of_the_images_in_view_stay_cached_across_renders() {
        let images = [ramp(1600, 1200), ramp(1500, 1300), ramp(1400, 1400)];
        let before = PLATE_CONVERSIONS.with(std::cell::Cell::get);
        for _ in 0..3 {
            for plate in [4, 3] {
                for pm in &images {
                    plate_pixmap(pm, plate);
                }
            }
        }
        assert_eq!(PLATE_CONVERSIONS.with(std::cell::Cell::get) - before, 6, "each image and plate is converted once");
    }

    /// A large image is converted only where the view shows it, with the same inks as converting
    /// it whole.
    #[test]
    fn a_large_image_converts_only_the_part_in_view() {
        let pm = ramp(4000, 2500);
        assert!(plate_bytes(4000, 2500) > PLATE_CACHE_BYTES / 8, "large enough to be converted in parts");
        let before = PLATE_CONVERSIONS.with(std::cell::Cell::get);
        let (part, at) = plate_part(&pm, 4, Affine::IDENTITY, || Rect::new(1000.0, 500.0, 1100.0, 560.0)).unwrap();
        assert_eq!(PLATE_CONVERSIONS.with(std::cell::Cell::get), before, "the whole image was not converted");
        assert_eq!(at, (996, 496));
        assert_eq!((part.width(), part.height()), (108, 68));
        let table = cmyk_table();
        let row = usize::from(pm.width());
        for (y, x) in [(0usize, 0usize), (30, 50), (67, 107)] {
            let source = pm.data()[(usize::from(at.1) + y) * row + usize::from(at.0) + x];
            assert_eq!(part.data()[y * 108 + x], plate_pixels(&[source], &table, 4)[0]);
        }
        assert!(plate_part(&pm, 4, Affine::IDENTITY, || Rect::new(5000.0, 0.0, 5100.0, 50.0)).is_none(), "out of view: nothing");
        // All of it in view: converted whole once, then taken from the cache.
        let before = PLATE_CONVERSIONS.with(std::cell::Cell::get);
        for _ in 0..3 {
            let (whole, at) = plate_part(&pm, 4, Affine::IDENTITY, || Rect::new(-10.0, -10.0, 4010.0, 2510.0)).unwrap();
            assert_eq!(((whole.width(), whole.height()), at), ((4000, 2500), (0, 0)));
        }
        assert_eq!(PLATE_CONVERSIONS.with(std::cell::Cell::get) - before, 1, "the whole plate is converted once and cached");
        // Nearly all of it in view (90 % of the width): the cached whole plate, not a new part.
        let before = PLATE_CONVERSIONS.with(std::cell::Cell::get);
        let (whole, at) = plate_part(&pm, 4, Affine::IDENTITY, || Rect::new(200.0, -10.0, 3800.0, 2510.0)).unwrap();
        assert_eq!(((whole.width(), whole.height()), at), ((4000, 2500), (0, 0)));
        assert_eq!(PLATE_CONVERSIONS.with(std::cell::Cell::get), before, "taken from the cache");
        let small = ramp(64, 64);
        let (whole, at) = plate_part(&small, 3, Affine::IDENTITY, || Rect::new(0.0, 0.0, 10.0, 10.0)).unwrap();
        assert_eq!(((whole.width(), whole.height()), at), ((64, 64), (0, 0)), "a small image is converted whole and cached");
    }

    /// A large image is widened only by the effects that move its own pixels (its object's and
    /// the groups' around it), not by a big shadow elsewhere in the document.
    #[test]
    fn an_images_effect_reach_is_its_own() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let graphic = |aid| designcraft_doc::Graphic {
            layer_options: None,
            asset: aid,
            size: (100.0, 100.0),
            xf: Affine::IDENTITY,
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        };
        let item = |d: &mut Document, shadow: f64, with_graphic: bool| {
            let mut it = Item::new(
                designcraft_doc::ItemId(d.alloc()),
                lid,
                designcraft_doc::Shape::Rectangle,
                designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0)),
            );
            if with_graphic {
                it.content = designcraft_doc::Content::Graphic(graphic(designcraft_doc::AssetId(1)));
            }
            if shadow > 0.0 {
                it.effects.drop_shadow.on = true;
                it.effects.drop_shadow.distance = shadow;
            }
            d.insert_item(SpreadRef::Doc(0), it.clone(), None).unwrap();
            it.id
        };
        let image = item(&mut d, 0.0, true);
        item(&mut d, 500.0, false);
        let g = d.item(image).unwrap().graphic().unwrap().clone();
        let own = d.item(image).unwrap().graphic().unwrap();
        assert_eq!(image_effect_reach(&d, own), Some(0.0), "no effect on the image's own object");
        assert!(effect_reach(&d) > 500.0, "the document-wide reach counts the other shadow");
        assert_eq!(image_effect_reach(&d, &g), None, "a graphic not in the document: the document-wide reach");
    }
}
