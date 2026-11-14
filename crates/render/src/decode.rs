use super::pdf_raster::{is_pdf, pdf_page_size, render_pdf_page};
use super::{Pixmap, vello_cpu};

/// Decode encoded image bytes into a premultiplied pixmap.
pub fn decode_pixmap(bytes: &[u8]) -> Option<Pixmap> {
    decode_pixmap_page(bytes, 0)
}

/// [`decode_pixmap`] for page `page` of a PDF (other formats have one page).
pub fn decode_pixmap_page(bytes: &[u8], page: u32) -> Option<Pixmap> {
    // Documents retain the EPS source; only display its TIFF preview or placeholder.
    let proxy = if designcraft_images::is_eps(bytes) { Some(designcraft_images::eps_proxy(bytes)?.0) } else { None };
    let bytes = proxy.as_deref().unwrap_or(bytes);
    if is_pdf(bytes) {
        return render_pdf_page(bytes, page as usize, 3000);
    }
    if designcraft_images::is_svg(bytes) {
        let (px, w, h) = designcraft_images::render_svg(bytes, 3000)?;
        return Some(Pixmap::from_parts(premul_pixels(px), w.min(u16::MAX as u32) as u16, h.min(u16::MAX as u32) as u16));
    }
    premultiplied(designcraft_images::decode_rgba(bytes)?)
}

/// [`decode_pixmap`] of a JPEG at 1/2^`shrink` of its size, `shrink` 1 to
/// [`designcraft_images::MAX_REDUCED_SHRINK`], without a full-size decode. `None` where only a
/// full decode applies (other formats, CMYK JPEGs).
pub(crate) fn decode_pixmap_reduced(bytes: &[u8], shrink: u8) -> Option<Pixmap> {
    premultiplied(designcraft_images::decode_rgba_reduced(bytes, shrink)?)
}

fn premultiplied(img: designcraft_images::RgbaImage) -> Option<Pixmap> {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    let mut px = img.into_raw();
    px.truncate(w as usize * h as usize * 4);
    Some(Pixmap::from_parts(premultiply(px), w as u16, h as u16))
}

/// Premultiply straight-alpha RGBA bytes in place and return them as pixmap pixels. Opaque pixels
/// are left as they are.
pub(crate) fn premultiply(mut px: Vec<u8>) -> Vec<vello_cpu::color::PremulRgba8> {
    for p in px.as_chunks_mut::<4>().0 {
        let a = p[3];
        if a != 255 {
            let m = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
            p[0] = m(p[0]);
            p[1] = m(p[1]);
            p[2] = m(p[2]);
        }
    }
    premul_pixels(px)
}

/// Premultiplied RGBA bytes as pixmap pixels, in the same allocation when its capacity is a whole
/// number of pixels and in a copy otherwise.
fn premul_pixels(px: Vec<u8>) -> Vec<vello_cpu::color::PremulRgba8> {
    bytemuck::allocation::try_cast_vec(px).unwrap_or_else(|(_, px)| {
        px.as_chunks::<4>().0.iter().map(|p| vello_cpu::color::PremulRgba8 { r: p[0], g: p[1], b: p[2], a: p[3] }).collect()
    })
}

/// Box-filter downsample by 2.
pub(crate) fn halve(pm: &Pixmap) -> Pixmap {
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
pub(crate) fn now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn now() -> u64 {
    0
}

/// Pixel size of an encoded image without decoding it fully.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if designcraft_images::is_eps(bytes) {
        return designcraft_images::eps_pixel_size(bytes);
    }
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

/// Natural size of a placed graphic, unrounded: a PDF page (crop box) and an SVG in points, a raster
/// at its stored resolution (72 ppi without one), an EPS its bounding box independently of preview pixels. Placed graphics are drawn at
/// this size; `image_size` gives a raster's pixel size and rounds a PDF page or SVG to whole points.
pub fn natural_size(bytes: &[u8], page: usize) -> Option<(f64, f64)> {
    if is_pdf(bytes) {
        return pdf_page_size(bytes, page).map(|(w, h)| (w.max(1e-3), h.max(1e-3)));
    }
    designcraft_images::natural_size(bytes)
}

pub fn image_mime(bytes: &[u8]) -> &'static str {
    designcraft_images::mime(bytes)
}
