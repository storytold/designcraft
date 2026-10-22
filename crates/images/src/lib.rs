//! Placed graphics: format sniffing, sizes and decoding for the renderer and PDF export.
//! Rasters go through `image` (PNG, JPEG, GIF, WebP, TIFF, BMP) or [`psd`] (Photoshop's merged
//! composite); SVG is parsed with usvg (text set in the bundled fonts, then the shared ones) and
//! rasterised with resvg for the screen — PDF export draws the same tree as vectors.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::{Arc, OnceLock};

pub use resvg::usvg;

pub mod colour;
mod eps;
mod jpeg;
mod psd;
mod tone;
pub use eps::{bounding_box as eps_bounding_box, eps_pixel_size, eps_png, eps_proxy, eps_size, is_eps};
pub use tone::{Tone, tone};

/// CSS pixels (SVG user units) → points.
pub const PT_PER_PX: f64 = 0.75;

pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

pub fn is_psd(bytes: &[u8]) -> bool {
    bytes.starts_with(b"8BPS")
}

/// An SVG document (optionally after an XML declaration, comments or a doctype).
pub fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let s = String::from_utf8_lossy(head);
    let t = s.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('<') && t.contains("<svg")
}

/// MIME type of a placed graphic.
pub fn mime(bytes: &[u8]) -> &'static str {
    if is_eps(bytes) {
        return "application/postscript";
    }
    if is_pdf(bytes) {
        return "application/pdf";
    }
    if is_psd(bytes) {
        return "image/vnd.adobe.photoshop";
    }
    if is_svg(bytes) {
        return "image/svg+xml";
    }
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => "image/png",
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::Gif) => "image/gif",
        Ok(image::ImageFormat::WebP) => "image/webp",
        Ok(image::ImageFormat::Tiff) => "image/tiff",
        Ok(image::ImageFormat::Bmp) => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn svg_options() -> &'static usvg::Options<'static> {
    static OPTS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for f in designcraft_fonts::bundled() {
            db.load_font_data(f.to_vec());
        }
        // Japanese text in SVGs: the craft-fonts faces (none without CRAFT_FONTS_DIR).
        for f in designcraft_fonts::japanese_document_fonts() {
            db.load_font_data(f.bytes.to_vec());
        }
        db.set_serif_family("Source Serif 4");
        db.set_sans_serif_family("Source Sans 3");
        db.set_monospace_family("JetBrains Mono");
        let font_resolver = usvg::FontResolver { select_font: select_font(), select_fallback: select_fallback() };
        // Images inside an SVG come only from `data:` URLs: an `href` naming a file is not read
        // (it is left out, as usvg leaves out an image it can't load).
        let image_href_resolver =
            usvg::ImageHrefResolver { resolve_data: usvg::ImageHrefResolver::default_data_resolver(), resolve_string: Box::new(|_, _| None) };
        usvg::Options { fontdb: Arc::new(db), font_family: "Source Sans 3".into(), font_resolver, image_href_resolver, ..Default::default() }
    })
}

/// A family the SVG names that isn't bundled comes from the shared fonts (installed ones too),
/// as it would in a text frame, and matches in any case, as it does there. A name that is no
/// family's but a face's PostScript name (as Illustrator's SVG export writes them:
/// `font-family="'MyriadPro-Regular'"`) sets the text in that face, whatever font-weight and
/// font-style say: the name already picks the weight and style, and a family's other faces would
/// contradict it. Families listed before it come first.
fn select_font() -> usvg::FontSelectionFn<'static> {
    use usvg::fontdb::{Family, Query, Stretch, Style, Weight};
    Box::new(move |font, db| {
        let fonts = designcraft_fonts::FontDb::global();
        let spelled = |db: &usvg::fontdb::Database, name: &str| {
            db.faces().flat_map(|f| &f.families).find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(n, _)| n.clone())
        };
        for family in font.families() {
            if let usvg::FontFamily::Named(name) = family
                && spelled(db, name).is_none()
            {
                if fonts.has_family(name) {
                    for style in fonts.styles(name) {
                        add_face(db, fonts.face(name, &style));
                    }
                } else if postscript_face(db, name).is_none()
                    && let Some((family, style)) = fonts.postscript_face(name)
                {
                    add_face(db, fonts.face(&family, &style));
                }
            }
        }
        for family in font.families() {
            match family {
                usvg::FontFamily::Named(name) if spelled(db, name).is_none() => {
                    if let Some(id) = postscript_face(db, name) {
                        return Some(id);
                    }
                }
                // A family (or generic family) that matches: usvg's own selection.
                _ => break,
            }
        }
        // usvg's default query, but with each named family spelled as the database spells it:
        // fontdb compares names exactly, so "DeJaVu SaNs" would quietly fall to the serif.
        let names: Vec<Option<String>> = font
            .families()
            .iter()
            .map(|f| match f {
                usvg::FontFamily::Named(name) => Some(spelled(db, name).unwrap_or_else(|| name.clone())),
                _ => None,
            })
            .collect();
        let mut families: Vec<Family> = font
            .families()
            .iter()
            .zip(&names)
            .map(|(f, name)| match f {
                usvg::FontFamily::Serif => Family::Serif,
                usvg::FontFamily::SansSerif => Family::SansSerif,
                usvg::FontFamily::Cursive => Family::Cursive,
                usvg::FontFamily::Fantasy => Family::Fantasy,
                usvg::FontFamily::Monospace => Family::Monospace,
                usvg::FontFamily::Named(_) => Family::Name(name.as_deref().unwrap_or_default()),
            })
            .collect();
        families.push(Family::Serif);
        let stretch = match font.stretch() {
            usvg::FontStretch::UltraCondensed => Stretch::UltraCondensed,
            usvg::FontStretch::ExtraCondensed => Stretch::ExtraCondensed,
            usvg::FontStretch::Condensed => Stretch::Condensed,
            usvg::FontStretch::SemiCondensed => Stretch::SemiCondensed,
            usvg::FontStretch::Normal => Stretch::Normal,
            usvg::FontStretch::SemiExpanded => Stretch::SemiExpanded,
            usvg::FontStretch::Expanded => Stretch::Expanded,
            usvg::FontStretch::ExtraExpanded => Stretch::ExtraExpanded,
            usvg::FontStretch::UltraExpanded => Stretch::UltraExpanded,
        };
        let style = match font.style() {
            usvg::FontStyle::Normal => Style::Normal,
            usvg::FontStyle::Italic => Style::Italic,
            usvg::FontStyle::Oblique => Style::Oblique,
        };
        db.query(&Query { families: &families, weight: Weight(font.weight()), stretch, style })
    })
}

/// The face in the SVG's font database with PostScript name `name` (ignoring case).
fn postscript_face(db: &usvg::fontdb::Database, name: &str) -> Option<usvg::fontdb::ID> {
    db.faces().find(|f| f.post_script_name.eq_ignore_ascii_case(name)).map(|f| f.id)
}

/// Characters the chosen fonts lack (Korean, Arabic…) fall back to a shared face that has them.
fn select_fallback() -> usvg::FallbackSelectionFn<'static> {
    let loaded = usvg::FontResolver::default_fallback_selector();
    Box::new(move |c, exclude, db| {
        loaded(c, exclude, db).or_else(|| {
            let id = add_face(db, designcraft_fonts::FontDb::global().fallback_for(c, 0, None)?)?;
            // usvg asks again until we run out: a face it already tried ends the search.
            (!exclude.contains(&id)).then_some(id)
        })
    })
}

/// A shared face's bytes, handed to usvg without a copy.
struct FaceBytes(Arc<designcraft_fonts::FontFace>);

impl AsRef<[u8]> for FaceBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.data()
    }
}

/// Add `face` to the SVG's font database (once) and return its id there.
fn add_face(db: &mut Arc<usvg::fontdb::Database>, face: Arc<designcraft_fonts::FontFace>) -> Option<usvg::fontdb::ID> {
    let same = |f: &usvg::fontdb::FaceInfo| {
        f.index == face.index()
            && matches!(&f.source, usvg::fontdb::Source::Binary(b) if std::ptr::eq(b.as_ref().as_ref().as_ptr(), face.data().as_ptr()))
    };
    if let Some(f) = db.faces().find(|f| same(f)) {
        return Some(f.id);
    }
    let index = face.index();
    let ids = Arc::make_mut(db).load_font_source(usvg::fontdb::Source::Binary(Arc::new(FaceBytes(face))));
    // A collection loads all its faces (skipping any it can't read): pick ours by its index.
    ids.into_iter().find(|id| db.face(*id).is_some_and(|f| f.index == index))
}

/// The parsed SVG.
pub fn svg_tree(bytes: &[u8]) -> Option<usvg::Tree> {
    usvg::Tree::from_data(bytes, svg_options()).ok()
}

/// Natural size of a placed graphic in points: rasters at 72 ppi (one pixel per point), PSDs
/// likewise, SVGs from their width/height (CSS pixels), PDFs are the caller's.
pub fn natural_size(bytes: &[u8]) -> Option<(f64, f64)> {
    if is_svg(bytes) {
        let s = svg_tree(bytes)?.size();
        return Some((s.width() as f64 * PT_PER_PX, s.height() as f64 * PT_PER_PX));
    }
    if is_eps(bytes) {
        let bb = eps::bounding_box(bytes)?;
        return Some((bb[2] - bb[0], bb[3] - bb[1])).filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0);
    }
    let (w, h) = pixel_size(bytes)?;
    Some((w as f64, h as f64))
}

/// Pixel dimensions of a raster.
pub fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if let Some(p) = palette_tiff(bytes) {
        return Some((p.width, p.height));
    }
    if is_psd(bytes) && bytes.len() >= 26 {
        // Header: signature, version, 6 reserved, channels, height, width (big-endian).
        let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        return Some((be(18), be(14)));
    }
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
        // `image` doesn't read five-sample CMYK with alpha.
        .or_else(|| tiff_decoder(bytes)?.dimensions().ok())
}

/// Straight (non-premultiplied) RGBA8 of a raster in display sRGB: an embedded ICC profile is
/// applied, and CMYK goes through its own profile or the working CMYK space ([`colour`]).
pub fn decode_rgba(bytes: &[u8]) -> Option<image::RgbaImage> {
    use colour::{DisplayColour, Pixels, to_display_rgba};
    if is_psd(bytes) {
        return psd::decode(bytes);
    }
    // `image` rejects indexed-colour TIFFs (an old GIF logo saved from Photoshop).
    if let Some(img) = decode_palette_tiff(bytes) {
        return Some(img);
    }
    // An EPS is drawn as its proxy (preview, simple PostScript or placeholder); the document
    // keeps the EPS itself.
    if is_eps(bytes) {
        return decode_rgba(&eps_proxy(bytes)?.0);
    }
    if let Some((raster, icc)) = jpeg::decode_cmyk(bytes) {
        return to_display_rgba(Pixels::Cmyk(raster), icc.as_deref(), &DisplayColour::active());
    }
    if let Some(raster) = decode_cmyk_tiff(bytes) {
        return to_display_rgba(Pixels::Cmyk(raster), embedded_icc(bytes).as_deref(), &DisplayColour::active());
    }
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = decode_to_rgba8(&img);
    let Some(icc) = embedded_icc(bytes) else { return Some(rgba) };
    let px = if img.color().has_color() { Pixels::Rgb(rgba) } else { Pixels::Gray(rgba) };
    to_display_rgba(px, Some(&icc), &DisplayColour::active())
}

/// The ICC profile embedded in a PNG, JPEG, TIFF or WebP.
fn embedded_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    use image::ImageDecoder;
    let mut d = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_decoder().ok()?;
    d.icc_profile().ok().flatten()
}

/// Fresh raster decodes are sRGB in the pinned image decoder. Expanding an L8 value to
/// [gray, gray, gray, 255] is exact and avoids its generic floating-point conversion.
fn decode_to_rgba8(img: &image::DynamicImage) -> image::RgbaImage {
    let prepared = match img {
        image::DynamicImage::ImageLuma8(gray) => prepare_luma8_rgba(gray),
        _ => None,
    };
    rgba_or_original(img, prepared)
}

// Keep preparation failure on the existing conversion/error path. This small boundary also
// lets tests force failure deterministically without exhausting memory or changing an allocator.
fn rgba_or_original(img: &image::DynamicImage, prepared: Option<image::RgbaImage>) -> image::RgbaImage {
    prepared.unwrap_or_else(|| img.to_rgba8())
}

fn prepare_luma8_rgba(gray: &image::GrayImage) -> Option<image::RgbaImage> {
    let (width, height) = gray.dimensions();
    let pixels = usize::try_from(width).ok()?.checked_mul(usize::try_from(height).ok()?)?;
    let length = pixels.checked_mul(4)?;
    let mut bytes = Vec::new();
    // Unlike the generic ConvertBuffer API's infallible allocation, preserve a route back to
    // the original to_rgba8 behavior when the requested buffer cannot be reserved.
    bytes.try_reserve_exact(length).ok()?;
    bytes.resize(length, 0);
    for (output, input) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(gray.pixels()) {
        let value = input.0[0];
        output.copy_from_slice(&[value, value, value, 255]);
    }
    let mut rgba = image::RgbaImage::from_raw(width, height, bytes)?;
    rgba.set_color_space(gray.color_space()).ok()?;
    Some(rgba)
}

#[cfg(test)]
mod luma8_tests;

/// A CMYK raster's ink values, 8 bits per ink (0 = no ink, 255 = solid), as PDF's DeviceCMYK
/// reads them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CmykRaster {
    pub width: u32,
    pub height: u32,
    /// C, M, Y, K for each pixel, row by row: `width * height * 4` bytes.
    pub cmyk: Vec<u8>,
    /// Straight alpha, `width * height` bytes; `None` when the image is opaque.
    pub alpha: Option<Vec<u8>>,
}

fn tiff_decoder(bytes: &[u8]) -> Option<tiff::decoder::Decoder<std::io::Cursor<&[u8]>>> {
    let tiff = [b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"].iter().any(|m| bytes.starts_with(*m));
    if !tiff {
        return None;
    }
    tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).ok()
}

/// An indexed-colour TIFF (PhotometricInterpretation 3): its size, bits per index and ColorMap
/// in 8 bits per channel.
struct PaletteTiff {
    width: u32,
    height: u32,
    bits: u8,
    colours: Vec<[u8; 3]>,
}

/// The header of a palette TIFF with 1-, 2-, 4- or 8-bit indices and a full ColorMap, which
/// neither `image` nor `tiff` decodes. `None` for anything else.
fn palette_tiff(bytes: &[u8]) -> Option<PaletteTiff> {
    use tiff::tags::Tag;
    let mut d = tiff_decoder(bytes)?;
    if d.find_tag_unsigned::<u16>(Tag::PhotometricInterpretation).ok()?? != 3 {
        return None;
    }
    if d.find_tag_unsigned::<u16>(Tag::SamplesPerPixel).ok()?.unwrap_or(1) != 1 {
        return None;
    }
    let bits = d.find_tag_unsigned::<u8>(Tag::BitsPerSample).ok()?.unwrap_or(1);
    if !matches!(bits, 1 | 2 | 4 | 8) {
        return None;
    }
    // 2^bits reds, then greens, then blues; 16-bit entries whose high byte is the 8-bit value.
    let map = d.find_tag_unsigned_vec::<u16>(Tag::ColorMap).ok()??;
    let n = 1usize << bits;
    let (r, gb) = map.split_at_checked(n)?;
    let (g, b) = gb.split_at_checked(n)?;
    if b.len() != n {
        return None;
    }
    let colours = r.iter().zip(g).zip(b).map(|((&r, &g), &b)| [r, g, b].map(|v| (v >> 8) as u8)).collect();
    let (width, height) = d.dimensions().ok()?;
    Some(PaletteTiff { width, height, bits, colours })
}

/// A palette TIFF as RGBA, each index looked up in the ColorMap.
fn decode_palette_tiff(bytes: &[u8]) -> Option<image::RgbaImage> {
    let p = palette_tiff(bytes)?;
    // Read as gray, `tiff` hands back the indices themselves, whatever the compression or layout.
    let gray = with_photometric(bytes, 1)?;
    let tiff::decoder::DecodingResult::U8(px) = tiff_decoder(&gray)?.read_image().ok()? else { return None };
    let (w, h) = (usize::try_from(p.width).ok()?, usize::try_from(p.height).ok()?);
    let bits = usize::from(p.bits);
    // Indices below 8 bits are packed high bits first, each row starting on a byte.
    let row = w.checked_mul(bits)?.div_ceil(8);
    if row == 0 || px.len() < row.checked_mul(h)? {
        return None;
    }
    let per_byte = 8 / bits;
    let mask = ((1u16 << bits) - 1) as u8;
    let mut out = Vec::new();
    out.try_reserve_exact(w.checked_mul(h)?.checked_mul(4)?).ok()?;
    for line in px.chunks_exact(row).take(h) {
        for x in 0..w {
            let shift = 8 - bits * (x % per_byte + 1);
            let i = (line.get(x / per_byte)? >> shift) & mask;
            let [r, g, b] = *p.colours.get(usize::from(i))?;
            out.extend([r, g, b, 255]);
        }
    }
    image::RgbaImage::from_raw(p.width, p.height, out)
}

/// A copy of a TIFF with its first image's PhotometricInterpretation (tag 262, one SHORT) set
/// to `value`. Classic and BigTIFF, either byte order.
fn with_photometric(bytes: &[u8], value: u16) -> Option<Vec<u8>> {
    let le = bytes.starts_with(b"II");
    let int = |at: usize, len: usize| -> Option<usize> {
        let b = bytes.get(at..at.checked_add(len)?)?;
        let v = |acc: u64, &x: &u8| acc << 8 | u64::from(x);
        usize::try_from(if le { b.iter().rev().fold(0, v) } else { b.iter().fold(0, v) }).ok()
    };
    // Classic: 4-byte offsets and value counts, 2-byte entry count, 12-byte entries. BigTIFF:
    // 8, 8, 8 and 20.
    let (ifd, word, count_len, entry_len) = if int(2, 2)? == 43 { (int(8, 8)?, 8, 8, 20) } else { (int(4, 4)?, 4, 2, 12) };
    let count = int(ifd, count_len)?;
    for k in 0..count {
        let at = ifd.checked_add(count_len)?.checked_add(k.checked_mul(entry_len)?)?;
        // Tag, type (3 = SHORT), value count, then the value itself.
        if int(at, 2)? == 262 && int(at + 2, 2)? == 3 && int(at + 4, word)? == 1 {
            let mut out = bytes.to_vec();
            let v = if le { value.to_le_bytes() } else { value.to_be_bytes() };
            let value_at = at + 4 + word;
            out.get_mut(value_at..value_at + 2)?.copy_from_slice(&v);
            return Some(out);
        }
    }
    None
}

/// A TIFF whose pixels are CMYK (PhotometricInterpretation Separated, four inks).
pub fn is_cmyk_tiff(bytes: &[u8]) -> bool {
    tiff_decoder(bytes).and_then(|mut d| d.colortype().ok()).is_some_and(|c| matches!(c, tiff::ColorType::CMYK(_) | tiff::ColorType::CMYKA(_)))
}

/// The ink values of a CMYK TIFF, which [`decode_rgba`] converts to RGB. 8- or 16-bit CMYK,
/// pixels interleaved, opaque or with unassociated alpha. `None` for anything else — another
/// format, another ink set (InkSet 2), planar storage or premultiplied alpha — and for files
/// over the decoder's 256 MiB limit; [`is_cmyk_tiff`] tells those apart from RGB files.
pub fn decode_cmyk_tiff(bytes: &[u8]) -> Option<CmykRaster> {
    use tiff::tags::Tag;
    let mut d = tiff_decoder(bytes)?;
    let has_alpha = match d.colortype().ok()? {
        tiff::ColorType::CMYK(8 | 16) => false,
        tiff::ColorType::CMYKA(8 | 16) => true,
        _ => return None,
    };
    // InkSet (tag 332): 1 or absent = CMYK; 2 = other inks, named in InkNames.
    if d.find_tag_unsigned::<u16>(Tag::Unknown(332)).ok()?.is_some_and(|s| s != 1) {
        return None;
    }
    // ExtraSamples 2 = unassociated alpha. Premultiplied CMYK has no meaning as ink values.
    if has_alpha && d.find_tag_unsigned_vec::<u16>(Tag::ExtraSamples).ok()?.and_then(|v| v.first().copied()) != Some(2) {
        return None;
    }
    let (w, h) = d.dimensions().ok()?;
    let pixels = usize::try_from(w).ok()?.checked_mul(usize::try_from(h).ok()?)?;
    let samples = if has_alpha { 5 } else { 4 };
    let px = match d.read_image().ok()? {
        tiff::decoder::DecodingResult::U8(v) => v,
        tiff::decoder::DecodingResult::U16(v) => v.iter().map(|&s| ((u32::from(s) * 255 + 32_767) / 65_535) as u8).collect(),
        _ => return None,
    };
    // A planar file reads back as its first plane only, and extra non-alpha samples make the
    // pixels wider: either way the length is not `pixels * samples`.
    if px.len() != pixels.checked_mul(samples)? {
        return None;
    }
    if !has_alpha {
        return Some(CmykRaster { width: w, height: h, cmyk: px, alpha: None });
    }
    let mut cmyk = Vec::with_capacity(pixels.checked_mul(4)?);
    let mut alpha = Vec::with_capacity(pixels);
    for &[c, m, y, k, a] in px.as_chunks::<5>().0 {
        cmyk.extend([c, m, y, k]);
        alpha.push(a);
    }
    let alpha = alpha.iter().any(|&a| a < 255).then_some(alpha);
    Some(CmykRaster { width: w, height: h, cmyk, alpha })
}

/// An SVG rasterised so its longer side is `max_side` pixels: premultiplied RGBA8.
pub fn render_svg(bytes: &[u8], max_side: u32) -> Option<(Vec<u8>, u32, u32)> {
    let tree = svg_tree(bytes)?;
    let s = tree.size();
    let k = max_side as f32 / s.width().max(s.height()).max(1e-3);
    let (w, h) = ((s.width() * k).round().max(1.0) as u32, (s.height() * k).round().max(1.0) as u32);
    let mut pm = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pm.as_mut());
    Some((pm.take(), w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
  <rect x="0" y="0" width="100" height="100" fill="#ff0000"/>
  <text x="110" y="60" font-size="20">Hi</text>
</svg>"##;

    #[test]
    fn svg_sniff_size_and_render() {
        let b = SVG.as_bytes();
        assert!(is_svg(b));
        assert_eq!(mime(b), "image/svg+xml");
        assert_eq!(natural_size(b), Some((150.0, 75.0)));
        let (px, w, h) = render_svg(b, 400).unwrap();
        assert_eq!((w, h), (400, 200));
        // Red on the left; the text drew some ink on the right.
        assert_eq!(&px[(100 * 400 + 100) * 4..][..4], &[255, 0, 0, 255]);
        assert!(px.chunks(4).skip(220).step_by(7).any(|p| p[3] > 128 && p[0] < 100), "text pixels");
        assert!(!is_svg(b"\x89PNG"));
    }

    #[test]
    fn svg_images_come_only_from_data_urls() {
        let dir = std::env::temp_dir().join(format!("dc-svg-href-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("red.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])).save(&file).unwrap();
        let blue = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10'%3E%3Crect width='10' height='10' fill='blue'/%3E%3C/svg%3E";
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="10">
<image x="0" y="0" width="10" height="10" href="{}"/>
<image x="10" y="0" width="10" height="10" href="{blue}"/>
<image x="20" y="0" width="10" height="10" href="/etc/hostname"/>
</svg>"#,
            file.display()
        );
        // The image nodes usvg kept: only the data URL's (PDF export embeds every one).
        fn images(g: &usvg::Group) -> usize {
            g.children()
                .iter()
                .map(|n| match n {
                    usvg::Node::Group(g) => images(g),
                    usvg::Node::Image(_) => 1,
                    _ => 0,
                })
                .sum()
        }
        assert_eq!(images(svg_tree(svg.as_bytes()).unwrap().root()), 1, "files the SVG names are not read");
        let (px, w, _) = render_svg(svg.as_bytes(), 30).unwrap();
        let at = |x: usize, y: usize| px[(y * w as usize + x) * 4..][..4].to_vec();
        assert_eq!(at(15, 5), [0, 0, 255, 255], "a data URL is drawn");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The family of the font that sets each text glyph of `svg`, and the glyph's id.
    fn glyph_families(svg: &str) -> Vec<(String, u16)> {
        fn walk(g: &usvg::Group, tree: &usvg::Tree, out: &mut Vec<(String, u16)>) {
            for n in g.children() {
                match n {
                    usvg::Node::Group(g) => walk(g, tree, out),
                    usvg::Node::Text(t) => {
                        for g in t.layouted().iter().flat_map(|s| &s.positioned_glyphs) {
                            out.extend(tree.fontdb().face(g.font).map(|f| (f.families[0].0.clone(), g.id.0)));
                        }
                    }
                    _ => {}
                }
            }
        }
        let tree = svg_tree(svg.as_bytes()).unwrap();
        let mut out = Vec::new();
        walk(tree.root(), &tree, &mut out);
        out
    }

    #[test]
    fn svg_text_uses_the_shared_fonts() {
        let font = designcraft_fonts::testing::font_with("DC Test SVG Hangul", &['한', '글']).unwrap();
        designcraft_fonts::FontDb::global().add_font(font);
        let text = |attrs: &str, body: &str| {
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text y="30" font-size="20"{attrs}>{body}</text></svg>"#)
        };
        let named = r#" font-family="DC Test SVG Hangul""#;
        // A family named in the SVG.
        let set = glyph_families(&text(named, "한글"));
        assert!(set.len() == 2 && set.iter().all(|(f, gid)| f == "DC Test SVG Hangul" && *gid != 0), "{set:?}");
        // Characters the bundled default lacks: a shared face that has them, not the default.
        let set = glyph_families(&text("", "한글"));
        assert!(set.len() == 2 && set.iter().all(|(f, gid)| f != "Source Sans 3" && *gid != 0), "{set:?}");
    }

    #[test]
    fn svg_family_names_match_in_any_case() {
        let font = designcraft_fonts::testing::font_with("DC Test SVG Casing", &['A', 'b']).unwrap();
        designcraft_fonts::FontDb::global().add_font(font);
        let text = |family: &str, attrs: &str| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text y="30" font-size="20" font-family="{family}"{attrs}>Ab</text></svg>"#
            )
        };
        for family in ["DC Test SVG Casing", "dc test svg casing", "DC TEST svg Casing", "Nope, dc test SVG casing, serif"] {
            let set = glyph_families(&text(family, ""));
            assert!(set.len() == 2 && set.iter().all(|(f, gid)| f == "DC Test SVG Casing" && *gid != 0), "{family}: {set:?}");
        }
        // Weight and style still reach the query: there's only a regular face, so it sets the text.
        let set = glyph_families(&text("dc test svg casing", r#" font-weight="700" font-style="italic""#));
        assert!(set.len() == 2 && set.iter().all(|(f, _)| f == "DC Test SVG Casing"), "{set:?}");
        // A family nobody has still falls to the bundled serif.
        let set = glyph_families(&text("DC Test SVG Nobody", ""));
        assert!(set.len() == 2 && set.iter().all(|(f, _)| f == "Source Serif 4"), "{set:?}");
    }

    /// The PostScript name of the face that sets the text of `svg` (one face for all of it).
    fn text_face(svg_attrs: &str) -> String {
        let svg =
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="40"><text y="30" font-size="20"{svg_attrs}>ab</text></svg>"#);
        let tree = svg_tree(svg.as_bytes()).unwrap();
        let mut faces = Vec::new();
        for n in tree.root().children() {
            if let usvg::Node::Text(t) = n {
                for g in t.layouted().iter().flat_map(|s| &s.positioned_glyphs) {
                    faces.extend(tree.fontdb().face(g.font).map(|f| f.post_script_name.clone()));
                }
            }
        }
        faces.dedup();
        assert_eq!(faces.len(), 1, "{faces:?}");
        faces.remove(0)
    }

    #[test]
    fn svg_text_finds_fonts_by_postscript_name() {
        // Bundled, as Illustrator's SVG export names it; any case.
        assert_eq!(text_face(r#" font-family="'SourceSerif4-Semibold'""#), "SourceSerif4-Semibold");
        assert_eq!(text_face(r#" font-family="sourceserif4-semibold""#), "SourceSerif4-Semibold");
        // A shared font (as an installed one would be).
        let font = designcraft_fonts::testing::font_with("DC Test SVG PostScript", &['a', 'b']).unwrap();
        designcraft_fonts::FontDb::global().add_font(font);
        assert_eq!(text_face(r#" font-family="DCTestSVGPostScript-Regular""#), "DCTestSVGPostScript-Regular");
        // The name picks that face: font-weight and font-style don't change it.
        assert_eq!(text_face(r#" font-family="SourceSerif4-Semibold" font-weight="bold" font-style="italic""#), "SourceSerif4-Semibold");
        // A family listed before it still comes first; one after it is a fallback.
        assert_eq!(text_face(r#" font-family="'Source Sans 3', SourceSerif4-Bold""#), "SourceSans3-Regular");
        assert_eq!(text_face(r#" font-family="NoSuch-Font, SourceSerif4-Bold, 'Source Sans 3'""#), "SourceSerif4-Bold");
        // Family names work as before.
        assert_eq!(text_face(r#" font-family="'Source Serif 4'" font-weight="bold""#), "SourceSerif4-Bold");
        // An unknown name falls back as before (the serif default).
        assert_eq!(text_face(r#" font-family="NoSuch-Font""#), text_face(r#" font-family="No Such Font""#));
        assert_eq!(text_face(r#" font-family="NoSuch-Font""#), "SourceSerif4-Regular");
    }

    /// A minimal Photoshop file: 2×1 RGB, no layers, raw merged image.
    fn tiny_psd() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(b"8BPS");
        b.extend(1u16.to_be_bytes());
        b.extend([0u8; 6]);
        b.extend(3u16.to_be_bytes()); // channels
        b.extend(1u32.to_be_bytes()); // height
        b.extend(2u32.to_be_bytes()); // width
        b.extend(8u16.to_be_bytes()); // depth
        b.extend(3u16.to_be_bytes()); // RGB
        b.extend(0u32.to_be_bytes()); // colour mode data
        b.extend(0u32.to_be_bytes()); // image resources
        b.extend(0u32.to_be_bytes()); // layer and mask info
        b.extend(0u16.to_be_bytes()); // raw
        b.extend([255, 0, 0, 255, 0, 0]); // R, G, B planes: a red pixel, then a green one
        b
    }

    #[test]
    fn psd_composite_decodes() {
        let b = tiny_psd();
        assert_eq!(mime(&b), "image/vnd.adobe.photoshop");
        assert_eq!(pixel_size(&b), Some((2, 1)));
        let img = decode_rgba(&b).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(1, 0).0, [0, 255, 0, 255]);
    }

    fn tiff_of<C: tiff::encoder::colortype::ColorType>(w: u32, h: u32, px: &[C::Inner], alpha: bool) -> Vec<u8>
    where
        [C::Inner]: tiff::encoder::TiffValue,
    {
        let mut b = Vec::new();
        let mut enc = tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut b)).unwrap();
        let mut img = enc.new_image::<C>(w, h).unwrap();
        if alpha {
            // CMYKA8 already has five samples: only the tag that says the fifth is alpha.
            img.encoder().write_tag(tiff::tags::Tag::ExtraSamples, &[2u16][..]).unwrap();
        }
        img.write_data(px).unwrap();
        b
    }

    /// A placed EPS is measured by its bounding box in points and drawn through its proxy; the
    /// bytes stay the EPS.
    #[test]
    fn eps_draws_through_its_proxy_at_its_bounding_box_size() {
        let eps =
            b"%!PS-Adobe-3.1 EPSF-3.0\n%%BoundingBox: 0 0 4 2\n%%EndComments\n/rf { rectfill } bind def\n0 0 0 1 setcmykcolor\n0 0 2 2 rf\n%%EOF\n";
        assert_eq!(natural_size(eps), Some((4.0, 2.0)));
        let img = decode_rgba(eps).expect("drawn");
        assert_eq!(img.dimensions(), (32, 16));
        assert_eq!(img.get_pixel(1, 1).0, [0, 0, 0, 255], "the rectangle");
        assert_eq!(img.get_pixel(30, 1).0[3], 0, "outside it, transparent");
    }

    /// CMYK TIFFs read back as their ink values; RGB ones aren't CMYK.
    #[test]
    fn cmyk_tiff_keeps_inks() {
        use tiff::encoder::colortype::{CMYK8, CMYK16, CMYKA8, RGB8};
        // 2×1: 100% K, then C0 M100 Y100 K0.
        let px = [0, 0, 0, 255, 0, 255, 255, 0];
        let b = tiff_of::<CMYK8>(2, 1, &px, false);
        assert_eq!(mime(&b), "image/tiff");
        assert!(is_cmyk_tiff(&b));
        assert_eq!(decode_cmyk_tiff(&b), Some(CmykRaster { width: 2, height: 1, cmyk: px.to_vec(), alpha: None }));
        // The screen gets RGB through the working CMYK space.
        let raster = CmykRaster { width: 2, height: 1, cmyk: px.to_vec(), alpha: None };
        let want = colour::to_display_rgba(colour::Pixels::Cmyk(raster), None, &colour::DisplayColour::active()).unwrap();
        assert_eq!(decode_rgba(&b).unwrap(), want);

        let b = tiff_of::<CMYK16>(2, 1, &[0, 0, 0, 65_535, 0, 32_896, 65_535, 0], false);
        assert_eq!(decode_cmyk_tiff(&b).unwrap().cmyk, [0, 0, 0, 255, 0, 128, 255, 0]);

        let b = tiff_of::<CMYKA8>(2, 1, &[0, 0, 0, 255, 255, 0, 255, 255, 0, 0], true);
        let r = decode_cmyk_tiff(&b).unwrap();
        assert_eq!((r.cmyk, r.alpha), (px.to_vec(), Some(vec![255, 0])));

        let rgb = tiff_of::<RGB8>(1, 1, &[255, 0, 0], false);
        assert!(!is_cmyk_tiff(&rgb));
        assert_eq!(decode_cmyk_tiff(&rgb), None);
        assert!(!is_cmyk_tiff(b"II*\0"), "truncated");
        assert_eq!(decode_cmyk_tiff(&b[..b.len() / 2]), None, "truncated");
    }

    /// Indexed colour (PhotometricInterpretation 3), which the `tiff` encoder has no type for.
    struct Palette<const BITS: u16>;
    impl<const BITS: u16> tiff::encoder::colortype::ColorType for Palette<BITS> {
        type Inner = u8;
        const TIFF_VALUE: tiff::tags::PhotometricInterpretation = tiff::tags::PhotometricInterpretation::RGBPalette;
        const BITS_PER_SAMPLE: &'static [u16] = &[BITS];
        const SAMPLE_FORMAT: &'static [tiff::tags::SampleFormat] = &[tiff::tags::SampleFormat::Uint];
        fn horizontal_predict(row: &[u8], result: &mut Vec<u8>) {
            result.extend_from_slice(row);
        }
    }

    /// A palette TIFF as Photoshop and Pillow write it: 16-bit ColorMap entries (reds, then
    /// greens, then blues), each 8-bit value times 256.
    fn palette_tiff<const BITS: u16>(w: u32, h: u32, px: &[u8], colours: &[[u8; 3]], lzw: bool) -> Vec<u8> {
        let n = 1usize << BITS;
        let mut map = vec![0u16; 3 * n];
        for (i, c) in colours.iter().enumerate() {
            for (ch, &v) in c.iter().enumerate() {
                map[ch * n + i] = u16::from(v) << 8;
            }
        }
        let mut b = Vec::new();
        let mut enc = tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut b)).unwrap();
        if lzw {
            enc = enc.with_compression(tiff::encoder::Compression::Lzw);
        }
        let mut img = enc.new_image::<Palette<BITS>>(w, h).unwrap();
        img.encoder().write_tag(tiff::tags::Tag::ColorMap, &map[..]).unwrap();
        img.write_data(px).unwrap();
        b
    }

    /// Indexed-colour TIFFs (an old GIF logo saved from Photoshop) are sized and drawn through
    /// their palette, uncompressed or LZW, 8- or 4-bit.
    #[test]
    fn palette_tiff_decodes_through_its_colormap() {
        let colours = [[0, 128, 255], [255, 0, 0], [10, 20, 30]];
        for lzw in [false, true] {
            let b = palette_tiff::<8>(3, 1, &[2, 0, 1], &colours, lzw);
            assert_eq!(pixel_size(&b), Some((3, 1)), "lzw {lzw}");
            let img = decode_rgba(&b).expect("decodes");
            let px: Vec<_> = img.pixels().map(|p| p.0).collect();
            assert_eq!(px, [[10, 20, 30, 255], [0, 128, 255, 255], [255, 0, 0, 255]], "lzw {lzw}");
        }
        // Two indices per byte, high nibble first: 2, 1 | 0, 2 (the encoder counts samples, not
        // bytes, so the strip carries two spare bytes the decoder doesn't read).
        let b = palette_tiff::<4>(2, 2, &[0x21, 0x02, 0, 0], &colours, false);
        assert_eq!(pixel_size(&b), Some((2, 2)));
        let px: Vec<_> = decode_rgba(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px, [[10, 20, 30, 255], [255, 0, 0, 255], [0, 128, 255, 255], [10, 20, 30, 255]]);
    }

    #[test]
    fn cmyk_tiff_with_alpha_reports_its_pixel_size() {
        use tiff::encoder::colortype::CMYKA8;
        let b = tiff_of::<CMYKA8>(2, 1, &[0, 0, 0, 255, 255, 0, 0, 0, 0, 0], true);
        assert_eq!(pixel_size(&b), Some((2, 1)));
    }

    #[test]
    fn bmp_decodes() {
        let mut bmp = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([0, 0, 255, 255]))
            .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
            .unwrap();
        assert_eq!(mime(&bmp), "image/bmp");
        assert_eq!(pixel_size(&bmp), Some((3, 2)));
        assert_eq!(decode_rgba(&bmp).unwrap().get_pixel(1, 1).0, [0, 0, 255, 255]);
    }
}
