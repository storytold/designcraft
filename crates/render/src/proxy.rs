//! Small preview images ("proxies") of linked files: at most [`MAX_SIDE_PX`] pixels on the longer
//! side and [`MAX_BYTES`] bytes, so a document can keep a stand-in when the file is missing.

use crate::Pixmap;
use crate::decode::{decode_pixmap_page, decode_pixmap_reduced, image_size};
use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image};

/// Longest side of a proxy, in pixels.
pub const MAX_SIDE_PX: u32 = 1024;
/// Largest encoded proxy, in bytes.
pub const MAX_BYTES: usize = 512 * 1024;
const JPEG_QUALITY: u8 = 80;
const MIN_QUALITY: u8 = 40;
const MAX_TRIES: usize = 8;

/// An encoded proxy preview.
pub struct ProxyImage {
    /// `image/jpeg` for an opaque image, `image/png` for one with alpha.
    pub mime: &'static str,
    /// Size in pixels.
    pub pixels: (u32, u32),
    pub data: Vec<u8>,
}

/// A proxy of page `page` of the image or PDF `bytes`, or `None` when it can't be decoded or no
/// encoding within [`MAX_BYTES`] was found.
pub fn make_proxy(bytes: &[u8], page: u32) -> Option<ProxyImage> {
    let pm = decode_for_proxy(bytes, page)?;
    let (w, h) = (u32::from(pm.width()), u32::from(pm.height()));
    if w == 0 || h == 0 {
        return None;
    }
    let (tw, th) = fit(w, h, MAX_SIDE_PX);
    let mut cur = if (tw, th) == (w, h) { pm } else { resample(&pm, tw, th)? };
    let mut quality = JPEG_QUALITY;
    for _ in 0..MAX_TRIES {
        let (cw, ch) = (u32::from(cur.width()), u32::from(cur.height()));
        let (rgba, opaque) = unpremultiplied(&cur);
        let data = if opaque { encode_jpeg(cw, ch, &rgba, quality) } else { encode_png(cw, ch, rgba) };
        if let Some(data) = data.filter(|d| d.len() <= MAX_BYTES) {
            return Some(ProxyImage { mime: if opaque { "image/jpeg" } else { "image/png" }, pixels: (cw, ch), data });
        }
        // Lower the JPEG quality first, then halve the size (PNG has no quality).
        if opaque && quality > MIN_QUALITY {
            quality = quality.saturating_sub(15).max(MIN_QUALITY);
        } else {
            let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
            if (nw, nh) == (cw, ch) {
                return None;
            }
            cur = resample(&cur, nw, nh)?;
        }
    }
    None
}

/// Decode `bytes`, through the reduced JPEG decode when it still leaves at least [`MAX_SIDE_PX`].
fn decode_for_proxy(bytes: &[u8], page: u32) -> Option<Pixmap> {
    if page == 0 && bytes.starts_with(&[0xFF, 0xD8]) {
        let (w, h) = image_size(bytes)?;
        let long = w.max(h);
        let mut shrink = 0u8;
        while shrink < designcraft_images::MAX_REDUCED_SHRINK && (long >> (shrink + 1)) >= MAX_SIDE_PX {
            shrink += 1;
        }
        if shrink > 0
            && let Some(pm) = decode_pixmap_reduced(bytes, shrink)
        {
            return Some(pm);
        }
    }
    decode_pixmap_page(bytes, page)
}

/// `(w, h)` scaled down so the longer side is at most `max`; never up.
fn fit(w: u32, h: u32, max: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max {
        return (w, h);
    }
    let scale = |v: u32| ((u64::from(v) * u64::from(max) + u64::from(long) / 2) / u64::from(long)).max(1) as u32;
    (scale(w), scale(h))
}

/// Box-filter resample of premultiplied pixels to `tw` x `th`.
fn resample(pm: &Pixmap, tw: u32, th: u32) -> Option<Pixmap> {
    let (w, h) = (usize::from(pm.width()), usize::from(pm.height()));
    let (tw, th) = (usize::try_from(tw).ok()?, usize::try_from(th).ok()?);
    if w == 0 || h == 0 || tw == 0 || th == 0 || tw > w || th > h || tw > usize::from(u16::MAX) || th > usize::from(u16::MAX) {
        return None;
    }
    let src = pm.data();
    let mut out = Vec::with_capacity(tw.checked_mul(th)?);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h).div_ceil(th).min(h));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w).div_ceil(tw).min(w));
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for y in y0..y1 {
                let row = src.get(y * w + x0..y * w + x1)?;
                for p in row {
                    acc[0] += u64::from(p.r);
                    acc[1] += u64::from(p.g);
                    acc[2] += u64::from(p.b);
                    acc[3] += u64::from(p.a);
                }
                n += (x1 - x0) as u64;
            }
            let n = n.max(1);
            let avg = |v: u64| ((v + n / 2) / n) as u8;
            out.push(vello_cpu::color::PremulRgba8 { r: avg(acc[0]), g: avg(acc[1]), b: avg(acc[2]), a: avg(acc[3]) });
        }
    }
    Some(Pixmap::from_parts(out, tw as u16, th as u16))
}

/// Straight RGBA bytes of a premultiplied pixmap, and whether every pixel is opaque.
fn unpremultiplied(pm: &Pixmap) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(pm.data().len().saturating_mul(4));
    let mut opaque = true;
    for p in pm.data() {
        if p.a == 255 {
            out.extend_from_slice(&[p.r, p.g, p.b, 255]);
        } else if p.a == 0 {
            opaque = false;
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            opaque = false;
            let un = |c: u8| ((u32::from(c) * 255 + u32::from(p.a) / 2) / u32::from(p.a)).min(255) as u8;
            out.extend_from_slice(&[un(p.r), un(p.g), un(p.b), p.a]);
        }
    }
    (out, opaque)
}

fn encode_jpeg(w: u32, h: u32, rgba: &[u8], quality: u8) -> Option<Vec<u8>> {
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    craft_image_codecs::web::encode_jpeg_rgb8(w, h, &rgb, quality, false, true, None, None, None, None).ok()
}

fn encode_png(w: u32, h: u32, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = Image::from_u8(w, h, ChannelLayout::Rgba, rgba).ok()?;
    craft_image_codecs::encode(&img, Format::Png, &EncodeOptions::default()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
        // Smooth gradient plus mild noise so JPEG output is realistic.
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let n = (x.wrapping_mul(7919) ^ y.wrapping_mul(104729)) % 16;
                px.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, n as u8 * 8, if alpha { ((x + y) % 200 + 55) as u8 } else { 255 }]);
            }
        }
        let img = Image::from_u8(w, h, ChannelLayout::Rgba, px).unwrap();
        craft_image_codecs::encode(&img, Format::Png, &EncodeOptions::default()).unwrap()
    }

    #[test]
    fn large_opaque_becomes_small_jpeg() {
        let p = make_proxy(&png(3000, 2000, false), 0).unwrap();
        assert_eq!(p.mime, "image/jpeg");
        assert_eq!(p.pixels, (1024, 683));
        assert!(p.data.len() <= MAX_BYTES);
        assert!(p.data.starts_with(&[0xFF, 0xD8]));
        eprintln!("proxy jpeg {:?} {} bytes", p.pixels, p.data.len());
    }

    #[test]
    fn alpha_becomes_png_and_small_is_not_upscaled() {
        let p = make_proxy(&png(300, 200, true), 0).unwrap();
        assert_eq!(p.mime, "image/png");
        assert_eq!(p.pixels, (300, 200));
        assert!(p.data.starts_with(&[0x89, b'P', b'N', b'G']));
        let q = make_proxy(&png(300, 200, false), 0).unwrap();
        assert_eq!(q.pixels, (300, 200));
        eprintln!("proxy png {} bytes, jpeg {} bytes", p.data.len(), q.data.len());
    }

    #[test]
    fn garbage_is_none() {
        assert!(make_proxy(&[], 0).is_none());
        assert!(make_proxy(b"not an image at all", 0).is_none());
        assert!(make_proxy(&[0xFF, 0xD8, 0xFF], 0).is_none());
    }

    #[test]
    fn pdf_page_is_rasterised() {
        let content = "1 0 0 rg 100 100 300 300 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
        let p = make_proxy(&out, 0).unwrap();
        assert!(p.pixels.0.max(p.pixels.1) <= MAX_SIDE_PX && p.pixels.1 > p.pixels.0);
        assert!(p.data.len() <= MAX_BYTES);
    }
}
