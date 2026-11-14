use super::pdf_raster::{is_pdf, pdf_page_size, render_pdf_page};
use super::{Pixmap, vello_cpu};

/// Decode encoded image bytes into a premultiplied pixmap.
pub fn decode_pixmap(bytes: &[u8]) -> Option<Pixmap> {
    decode_pixmap_page(bytes, 0)
}

/// Longer side (pixels) PDF pages and SVGs are drawn at by [`decode_pixmap_page`].
const VECTOR_SIDE: u32 = 3000;

/// About how many bytes [`decode_pixmap_page`] of `bytes` takes, with its smaller levels (a third
/// more): PDF pages and SVGs at [`VECTOR_SIDE`] pixels on the longer side, an EPS without a
/// preview at up to 8 pixels per point and 4000 pixels on the longer side (as its PostScript is
/// drawn) or at its placeholder's size when that is larger, other images and previews at their
/// pixel size. 0 when the size can't be read.
#[cfg(test)]
pub(crate) fn decoded_bytes(bytes: &[u8], page: u32) -> usize {
    decode_sizes(bytes, page).0
}

/// [`decoded_bytes`] of `bytes`, and about how many bytes [`decode_pixmap_page`] of it holds at
/// once while it runs: a raster as decoded and its RGBA copy, or that copy and the premultiplied
/// pixels, whichever is more ([`designcraft_images::decode_bytes_per_pixel`]: 8 bytes a pixel
/// for 8-bit RGB, 16 for 16-bit RGBA, 14 for 8-bit CMYK, an EPS as its TIFF preview). PDF pages, SVGs and EPS
/// files without a preview count 8 bytes a pixel. Both are 0 when the size can't be read.
pub(crate) fn decode_sizes(bytes: &[u8], page: u32) -> (usize, usize) {
    let Some((w, h)) = drawn_size(bytes, page) else { return (0, 0) };
    let raster = !is_pdf(bytes) && !designcraft_images::is_svg(bytes);
    let per_pixel = if raster { designcraft_images::decode_bytes_per_pixel(bytes) } else { 8 } as f64;
    ((w * h * 16.0 / 3.0) as usize, (w * h * per_pixel) as usize)
}

/// [`drawn_size`] in whole pixels, from the header (no decode).
pub(crate) fn drawn_pixels(bytes: &[u8], page: u32) -> Option<(u32, u32)> {
    drawn_size(bytes, page).map(|(w, h)| (w.clamp(1.0, f64::from(u32::MAX)) as u32, h.clamp(1.0, f64::from(u32::MAX)) as u32))
}

/// The pixel size [`decode_pixmap_page`] of `bytes` draws at, as [`decoded_bytes`] describes it.
fn drawn_size(bytes: &[u8], page: u32) -> Option<(f64, f64)> {
    let drawn = |(w, h): (f64, f64), side: f64, most: f64| {
        let k = (side / w.max(h).max(1e-3)).min(most);
        ((w * k).round().max(1.0), (h * k).round().max(1.0))
    };
    if designcraft_images::is_eps(bytes) {
        image_size(bytes).zip(designcraft_images::eps_size(bytes)).map(|((pw, ph), (w, h))| {
            // Without a preview, the pixel size is the 2 pixels per point placeholder's. The
            // PostScript may be drawn instead, at another size: count the larger of the two.
            let placeholder = ((w * 2.0).round().clamp(2.0, 4000.0), (h * 2.0).round().clamp(2.0, 4000.0));
            let px = (pw as f64, ph as f64);
            let rendered = drawn((w, h), 4000.0, 8.0);
            if px == placeholder && rendered.0 * rendered.1 > px.0 * px.1 { rendered } else { px }
        })
    } else if is_pdf(bytes) {
        // As render_pdf_page scales: at most 8 times the page size.
        pdf_page_size(bytes, page as usize).map(|s| drawn(s, VECTOR_SIDE as f64, 8.0))
    } else if designcraft_images::is_svg(bytes) {
        designcraft_images::natural_size(bytes).map(|s| drawn(s, VECTOR_SIDE as f64, f64::INFINITY))
    } else {
        image_size(bytes).map(|(w, h)| (w as f64, h as f64))
    }
}

/// [`decode_pixmap`] for page `page` of a PDF (other formats have one page).
pub fn decode_pixmap_page(bytes: &[u8], page: u32) -> Option<Pixmap> {
    // Documents retain the EPS source; only display its TIFF preview or placeholder.
    let proxy = if designcraft_images::is_eps(bytes) { Some(designcraft_images::eps_proxy(bytes)?.0) } else { None };
    let bytes = proxy.as_deref().unwrap_or(bytes);
    if is_pdf(bytes) {
        return render_pdf_page(bytes, page as usize, VECTOR_SIDE);
    }
    if designcraft_images::is_svg(bytes) {
        let (px, w, h) = designcraft_images::render_svg(bytes, VECTOR_SIDE)?;
        return Some(Pixmap::from_parts(premul_pixels(px), w.min(u16::MAX as u32) as u16, h.min(u16::MAX as u32) as u16));
    }
    let img = designcraft_images::decode_rgba(bytes)?;
    premultiplied(img.width(), img.height(), img.into_raw())
}

/// [`decode_pixmap`] of a JPEG at 1/2^`shrink` of its size, `shrink` 1 to
/// [`designcraft_images::MAX_REDUCED_SHRINK`], without a full-size decode. `None` where only a
/// full decode applies (other formats, CMYK JPEGs).
pub(crate) fn decode_pixmap_reduced(bytes: &[u8], shrink: u8) -> Option<Pixmap> {
    let img = designcraft_images::decode_rgba_reduced(bytes, shrink)?;
    premultiplied(img.width(), img.height(), img.into_raw())
}

/// Straight RGBA pixels, `w` × `h`, as a premultiplied pixmap.
fn premultiplied(w: u32, h: u32, mut px: Vec<u8>) -> Option<Pixmap> {
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
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
