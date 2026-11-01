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

/// Initialize the process-wide usvg database from the bundled and Japanese fonts, with case-aware family/PostScript resolution, fallback selection and preflighted SVG data URLs; file-backed image URLs are never read.
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
        let default_data = usvg::ImageHrefResolver::default_data_resolver();
        let image_href_resolver = usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, options| {
                let raster = matches!(
                    image::guess_format(&data),
                    Ok(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Gif | image::ImageFormat::WebP)
                );
                if mime == "image/svg+xml" || (mime == "text/plain" && !raster) {
                    let xml = svg_data(&data)?;
                    svg_preflight(&xml)?;
                    default_data(mime, Arc::new(xml.into_owned()), options)
                } else {
                    default_data(mime, data, options)
                }
            }),
            resolve_string: Box::new(|_, _| None),
        };
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
                // A family (or generic family) that matches: the query below.
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

/// Limits apply before usvg's recursive XML/SVG parsing, including SVGZ output.
/// Ordinary SVG doctypes are accepted; internal DTD subsets are not, since entity
/// expansion could introduce markup that the streaming depth check never sees.
const MAX_SVG_BYTES: usize = 16 * 1024 * 1024;
const MAX_SVG_EVENTS: usize = 100_000;
const MAX_SVG_DEPTH: usize = 64;

/// Enforce the encoded-size cap, gunzip SVGZ data with that bound, and return the decoded content within the cap as an owned or borrowed `Cow`.
fn svg_data(bytes: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
    use std::io::Read;

    if bytes.len() > MAX_SVG_BYTES {
        return None;
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut xml = Vec::new();
        flate2::read::GzDecoder::new(bytes).take(MAX_SVG_BYTES as u64 + 1).read_to_end(&mut xml).ok()?;
        (xml.len() <= MAX_SVG_BYTES).then_some(std::borrow::Cow::Owned(xml))
    } else {
        Some(std::borrow::Cow::Borrowed(bytes))
    }
}

/// Require UTF-8, bounded XML events, balanced tags and depth at most 64; reject internal DTD subsets while accepting ordinary external doctypes.
fn svg_preflight(bytes: &[u8]) -> Option<()> {
    use quick_xml::events::Event;

    let text = std::str::from_utf8(bytes).ok()?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut depth = 0usize;
    for _ in 0..MAX_SVG_EVENTS {
        match reader.read_event().ok()? {
            Event::Start(_) => {
                depth += 1;
                if depth > MAX_SVG_DEPTH {
                    return None;
                }
            }
            Event::Empty(_) if depth >= MAX_SVG_DEPTH => return None,
            Event::End(_) => depth = depth.checked_sub(1)?,
            Event::DocType(doctype) => {
                // A '[' inside a quoted external identifier is not an internal subset.
                let mut quote = None;
                for &byte in doctype.as_ref() {
                    match (quote, byte) {
                        (Some(q), b) if b == q => quote = None,
                        (None, b'\'' | b'"') => quote = Some(byte),
                        (None, b'[') => return None,
                        _ => {}
                    }
                }
            }
            Event::Eof => return (depth == 0).then_some(()),
            _ => {}
        }
    }
    None
}

/// Parse an SVG with at most 16 MiB encoded/decoded data, 100,000 XML events and
/// 64 nested elements. Reject internal DTD subsets before recursive conversion.
pub fn svg_tree(bytes: &[u8]) -> Option<usvg::Tree> {
    let xml = svg_data(bytes)?;
    svg_preflight(&xml)?;
    usvg::Tree::from_data(&xml, svg_options()).ok()
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

/// The largest decoded raster, in RGBA bytes: 256 Mpx, a 300 ppi scan of an A0 sheet fits.
const MAX_RGBA_BYTES: u64 = 1 << 30;

/// Whether a raster's header gives it more pixels than [`decode_rgba`] decodes.
pub fn too_large_to_decode(bytes: &[u8]) -> bool {
    // `image` limits only its own output, which can be a quarter of the RGBA copy or less.
    pixel_size(bytes).is_some_and(|(w, h)| u64::from(w).saturating_mul(u64::from(h)).saturating_mul(4) > MAX_RGBA_BYTES)
}

/// Straight (non-premultiplied) RGBA8 of a raster in display sRGB: an embedded ICC profile is
/// applied, and CMYK goes through its own profile or the working CMYK space ([`colour`]).
pub fn decode_rgba(bytes: &[u8]) -> Option<image::RgbaImage> {
    use colour::{DisplayColour, Pixels, to_display_rgba};
    if too_large_to_decode(bytes) {
        return None;
    }
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
    if too_large_to_decode(bytes) {
        return None;
    }
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
mod tests;
