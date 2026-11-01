use super::types::{Frame, Rendered};
use super::{Affine, BlendMode, Compose, DcBlend, Document, Item, Mix, Rect, RenderContext, peniko, vello_cpu};

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

/// An image's ink coverage on one process plate, as grey (cached per image and plate).
pub(crate) fn plate_pixmap(pm: &std::sync::Arc<vello_cpu::Pixmap>, plate: u8) -> std::sync::Arc<vello_cpu::Pixmap> {
    // Keep source allocations alive (their addresses are keys), invalidate on colour settings,
    // and cap derived raster memory rather than merely counting differently sized images.
    type Entry = (std::sync::Arc<vello_cpu::Pixmap>, std::sync::Arc<vello_cpu::Pixmap>);
    type Cache = (designcraft_color::cms::ColorSettings, std::collections::HashMap<(usize, u8), Entry>, usize);
    const BUDGET: usize = 64 << 20;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (std::sync::Arc::as_ptr(pm) as usize, plate);
    let settings = designcraft_color::cms::active_settings();
    {
        let mut guard = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((previous, map, _)) = guard.as_ref() {
            if *previous == settings {
                if let Some((_, derived)) = map.get(&key) {
                    return derived.clone();
                }
            } else {
                *guard = None;
            }
        }
    }
    let table = cmyk_table();
    let data: Vec<vello_cpu::color::PremulRgba8> = pm
        .data()
        .iter()
        .map(|p| {
            if p.a == 0 {
                return *p;
            }
            let k = 255.0 / p.a as f32;
            let rgb = [p.r, p.g, p.b].map(|v| (v as f32 * k / 255.0).min(1.0));
            let cmyk = cmyk_in(&table, rgb);
            let ink = |i: usize| ((1.0 - cmyk[i].clamp(0.0, 1.0)) * p.a as f32).round() as u8;
            if plate == 4 {
                return vello_cpu::color::PremulRgba8 { r: ink(0), g: ink(1), b: ink(2), a: p.a };
            }
            let v = ink(plate.min(3) as usize);
            vello_cpu::color::PremulRgba8 { r: v, g: v, b: v, a: p.a }
        })
        .collect();
    let out = std::sync::Arc::new(vello_cpu::Pixmap::from_parts(data, pm.width(), pm.height()));
    let bytes = (pm.width() as usize).saturating_mul(pm.height() as usize).saturating_mul(8);
    if bytes <= BUDGET {
        let mut guard = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (previous, map, used) = guard.get_or_insert_with(|| (settings.clone(), Default::default(), 0));
        if *previous != settings || used.saturating_add(bytes) > BUDGET {
            map.clear();
            *used = 0;
            *previous = settings;
        }
        if map.insert(key, (pm.clone(), out.clone())).is_none() {
            *used += bytes;
        }
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
