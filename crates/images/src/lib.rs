//! Placed graphics: format-agnostic pixel operations and shared file-format delegation.
//! Raster formats are decoded by `craft-image-codecs`; SVG parsing and bounded screen rendering
//! are provided by `craft-svg`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::{Arc, OnceLock};

pub use craft_svg::tree::usvg;

pub mod colour;
mod eps;
mod jpeg;
mod psd;
mod resolution;
mod tone;
pub use craft_eps::is_eps;
pub use eps::{bounding_box as eps_bounding_box, eps_pixel_size, eps_png, eps_proxy, eps_size};
pub use tone::{Tone, tone};

/// CSS pixels (SVG user units) → points.
pub const PT_PER_PX: f64 = 0.75;

const MAX_SVG_BYTES: usize = 16 * 1024 * 1024;
const MAX_SVG_EVENTS: usize = 100_000;
const MAX_SVG_DEPTH: usize = 64;

pub fn is_pdf(bytes: &[u8]) -> bool {
    file_format(bytes) == Some(craft_file_formats::Format::Pdf)
}

pub fn is_psd(bytes: &[u8]) -> bool {
    file_format(bytes) == Some(craft_file_formats::Format::Psd)
}

/// An SVG document (optionally after an XML declaration, comments or a doctype).
pub fn is_svg(bytes: &[u8]) -> bool {
    file_format(bytes) == Some(craft_file_formats::Format::Svg)
}

pub fn file_format(bytes: &[u8]) -> Option<craft_file_formats::Format> {
    craft_file_formats::detect("", bytes)
}

/// MIME type of a placed graphic.
pub fn mime(bytes: &[u8]) -> &'static str {
    match file_format(bytes) {
        Some(craft_file_formats::Format::Eps | craft_file_formats::Format::PostScript | craft_file_formats::Format::Illustrator) => {
            "application/postscript"
        }
        Some(craft_file_formats::Format::Pdf) => "application/pdf",
        Some(craft_file_formats::Format::Psd) => "image/vnd.adobe.photoshop",
        Some(craft_file_formats::Format::Svg) => "image/svg+xml",
        Some(craft_file_formats::Format::Png) => "image/png",
        Some(craft_file_formats::Format::Jpeg) => "image/jpeg",
        Some(craft_file_formats::Format::Gif) => "image/gif",
        Some(craft_file_formats::Format::WebP) => "image/webp",
        Some(craft_file_formats::Format::Tiff) => "image/tiff",
        Some(craft_file_formats::Format::Bmp) => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn svg_fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
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
        Arc::new(db)
    })
    .clone()
}

/// usvg options with DesignCraft's font database and family/PostScript fallback policy.
fn svg_options() -> usvg::Options<'static> {
    usvg::Options {
        fontdb: svg_fontdb(),
        font_family: "Source Sans 3".into(),
        font_resolver: usvg::FontResolver { select_font: select_font(), select_fallback: select_fallback() },
        ..Default::default()
    }
}

/// A family the SVG names that isn't bundled comes from the shared fonts (installed ones too),
/// as it would in a text frame, and matches in any case, as it does there. A name that is no
/// family's but a face's PostScript name (as Illustrator's SVG export writes them:
/// `font-family="'MyriadPro-Regular'"`) sets the text in that face, whatever font-weight and
/// font-style say: the name already picks the weight and style, and a family's other faces would
/// contradict it. A name that is a face's legacy family name (name ID 1) or full name (ID 4), as
/// Windows names faces outside the regular/bold/italic quartet (`'Segoe UI Semibold'`, whose
/// typographic family is `Segoe UI`), sets the text in that face (the closest to font-weight and
/// font-style when the legacy family has several). Families listed before it come first.
fn select_font() -> usvg::FontSelectionFn<'static> {
    use usvg::fontdb::{Family, Query, Stretch, Style, Weight};
    Box::new(move |font, db| {
        let fonts = designcraft_fonts::FontDb::global();
        let spelled = |db: &usvg::fontdb::Database, name: &str| {
            db.faces().flat_map(|f| &f.families).find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(n, _)| n.clone())
        };
        // (index in the font-family list, face) of the faces a name names by its legacy names.
        let mut legacy: Vec<(usize, usvg::fontdb::ID)> = Vec::new();
        for (i, family) in font.families().iter().enumerate() {
            if let usvg::FontFamily::Named(name) = family
                && spelled(db, name).is_none()
            {
                if fonts.has_family(name) {
                    for style in fonts.styles(name) {
                        let face = fonts.face(name, &style);
                        let named = has_legacy_name(&face, name);
                        if let Some(id) = add_face(db, face)
                            && named
                        {
                            legacy.push((i, id));
                        }
                    }
                } else if postscript_face(db, name).is_none() {
                    if let Some((family, style)) = fonts.postscript_face(name) {
                        add_face(db, fonts.face(&family, &style));
                    } else {
                        // A full name: the family is the name without its last words ("Bold").
                        let mut base = name.as_str();
                        for _ in 0..3 {
                            let Some((head, _)) = base.trim_end().rsplit_once(' ') else { break };
                            base = head;
                            if fonts.has_family(base) {
                                for style in fonts.styles(base) {
                                    let face = fonts.face(base, &style);
                                    if has_legacy_name(&face, name)
                                        && let Some(id) = add_face(db, face)
                                    {
                                        legacy.push((i, id));
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
        let want_style = match font.style() {
            usvg::FontStyle::Normal => Style::Normal,
            usvg::FontStyle::Italic => Style::Italic,
            usvg::FontStyle::Oblique => Style::Oblique,
        };
        for (i, family) in font.families().iter().enumerate() {
            match family {
                usvg::FontFamily::Named(name) if spelled(db, name).is_none() => {
                    if let Some(id) = postscript_face(db, name) {
                        return Some(id);
                    }
                    let closest = legacy
                        .iter()
                        .filter(|(at, _)| *at == i)
                        .filter_map(|(_, id)| db.face(*id))
                        .min_by_key(|f| (f.style != want_style, f.weight.0.abs_diff(font.weight())))
                        .map(|f| f.id);
                    if closest.is_some() {
                        return closest;
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
        db.query(&Query { families: &families, weight: Weight(font.weight()), stretch, style: want_style })
    })
}

/// Is `name` (ignoring case) a legacy family name (name ID 1) or full name (ID 4) of `face`?
fn has_legacy_name(face: &designcraft_fonts::FontFace, name: &str) -> bool {
    use skrifa::MetadataProvider;
    use skrifa::string::StringId;
    let Some(f) = face.skrifa() else { return false };
    [StringId::FAMILY_NAME, StringId::FULL_NAME]
        .into_iter()
        .flat_map(|id| f.localized_strings(id))
        .take(256)
        .any(|s| s.chars().map(|c| c.to_ascii_lowercase()).eq(name.chars().map(|c| c.to_ascii_lowercase())))
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
                for &byte in &doctype[..] {
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

/// Parse SVG/SVGZ through craft-svg's bounded parser and data-only nested-image resolver.
pub fn svg_tree(bytes: &[u8]) -> Option<usvg::Tree> {
    let xml = craft_svg::svgz::inflate(bytes, MAX_SVG_BYTES).ok()?;
    svg_preflight(&xml)?;
    let limits = craft_svg::tree::Limits {
        bytes: MAX_SVG_BYTES,
        depth: MAX_SVG_DEPTH,
        refuse_entity_markup: true,
        xml_nodes: MAX_SVG_EVENTS as u32,
        embedded_bytes: 32 * 1024 * 1024,
        embedded_depth: 4,
        ..Default::default()
    };
    let mut options = svg_options();
    options.image_href_resolver = craft_svg::tree::image_resolver(limits);
    craft_svg::tree::parse(&xml, &limits, &options).ok()
}

pub use resolution::raster_resolution;

/// Natural size of a placed graphic in points: rasters at their embedded resolution when sane
/// (one pixel per point, 72 ppi, without usable metadata), PSDs likewise, SVGs from their
/// width/height (CSS pixels), PDFs are the caller's.
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
    // Honor the file's density (#1991): a 600 px photo saved at 600 dpi places one inch wide.
    // Absurd metadata falls back to a pixel per point.
    match raster_resolution(bytes) {
        Some(dpi) if resolution::sane_resolution(dpi) => Some((w as f64 * 72.0 / dpi.0, h as f64 * 72.0 / dpi.1)),
        _ => Some((w as f64, h as f64)),
    }
}

/// Unpremultiplied RGBA pixels, rows top to bottom, as a PNG file with no resolution stored.
/// `None` when `rgba` doesn't hold exactly `width × height` pixels.
pub fn png_from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// Pixel dimensions of a raster.
pub fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        return Some((w, h));
    }
    if is_psd(bytes) {
        let header = craft_psd::header::Header::from_bytes(bytes).ok()?;
        return Some((header.width, header.height));
    }
    craft_image_codecs::inspect_raster(bytes)
        .ok()
        .flatten()
        .map(|image| (image.width, image.height))
        .or_else(|| craft_image_codecs::probe_size(bytes).ok())
        .or_else(|| {
            let image = craft_image_codecs::decode_with(bytes, &decode_options()).ok()?;
            Some(image.dimensions())
        })
}

/// The largest decoded raster, in RGBA bytes: 256 Mpx, a 300 ppi scan of an A0 sheet fits.
const MAX_RGBA_BYTES: u64 = 1 << 30;

/// Whether a raster's header gives it more pixels than [`decode_rgba`] decodes.
pub fn too_large_to_decode(bytes: &[u8]) -> bool {
    // `image` limits only its own output, which can be a quarter of the RGBA copy or less.
    pixel_size(bytes).is_some_and(|(w, h)| over_rgba_budget(w, h))
}

/// Whether a `w` × `h` raster takes more than [`MAX_RGBA_BYTES`] as RGBA.
fn over_rgba_budget(w: u32, h: u32) -> bool {
    u64::from(w).saturating_mul(u64::from(h)).saturating_mul(4) > MAX_RGBA_BYTES
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
    // An EPS is drawn as its proxy (preview, simple PostScript or placeholder); the document
    // keeps the EPS itself.
    if is_eps(bytes) {
        return decode_rgba(&eps_proxy(bytes)?.0);
    }
    let decoded = craft_image_codecs::decode_with(bytes, &decode_options()).ok()?;
    if decoded.layout().is_cmyk() {
        let raster = cmyk_raster(&decoded, bytes)?;
        return to_display_rgba(Pixels::Cmyk(raster), decoded.icc.as_deref(), &DisplayColour::active());
    }
    let rgba = image::RgbaImage::from_raw(decoded.width(), decoded.height(), decoded.to_rgba8())?;
    let Some(icc) = decoded.icc.as_deref() else { return Some(rgba) };
    let px = if decoded.layout().is_gray() { Pixels::Gray(rgba) } else { Pixels::Rgb(rgba) };
    to_display_rgba(px, Some(icc), &DisplayColour::active())
}

fn decode_options() -> craft_image_codecs::DecodeOptions {
    craft_image_codecs::DecodeOptions { limits: craft_image_codecs::Limits { max_alloc: MAX_RGBA_BYTES, ..Default::default() }, ..Default::default() }
}

/// Whether an APP14 "Adobe" segment comes before the image data.
fn adobe_marker(bytes: &[u8]) -> bool {
    let mut i = 2;
    while let Some(&[0xFF, marker, hi, lo]) = bytes.get(i..i + 4).and_then(|s| <&[u8; 4]>::try_from(s).ok()) {
        // Start of scan or end of image: no more header segments.
        if marker == 0xDA || marker == 0xD9 {
            return false;
        }
        let len = usize::from(u16::from_be_bytes([hi, lo]));
        if marker == 0xEE && bytes.get(i + 4..i + 9) == Some(b"Adobe".as_slice()) {
            return true;
        }
        i += 2 + len;
    }
    false
}

fn cmyk_raster(image: &craft_image_codecs::Image, bytes: &[u8]) -> Option<CmykRaster> {
    let converted = image.convert(craft_image_codecs::ChannelLayout::CmykA, craft_image_codecs::SampleType::U8);
    let pixels = usize::try_from(converted.width()).ok()?.checked_mul(usize::try_from(converted.height()).ok()?)?;
    let data = converted.as_u8()?;
    if data.len() != pixels.checked_mul(5)? {
        return None;
    }
    let invert = bytes.starts_with(&[0xFF, 0xD8]) && !adobe_marker(bytes);
    let mut cmyk = Vec::with_capacity(pixels.checked_mul(4)?);
    let mut alpha = Vec::with_capacity(pixels);
    for &[c, m, y, k, a] in data.as_chunks::<5>().0 {
        if invert {
            cmyk.extend([255 - c, 255 - m, 255 - y, 255 - k]);
        } else {
            cmyk.extend([c, m, y, k]);
        }
        alpha.push(a);
    }
    let alpha = alpha.iter().any(|&a| a < 255).then_some(alpha);
    Some(CmykRaster { width: converted.width(), height: converted.height(), cmyk, alpha })
}

/// False when `bytes` is a WebP whose `ICCP` chunk declares more bytes than the file holds or than
/// [`colour::MAX_ICC_BYTES`]. The decoder sizes its buffer from the declared length before it reads
/// anything, so such a file must not reach `icc_profile()`; its pixels decode without it.
pub fn webp_icc_is_sane(bytes: &[u8]) -> bool {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return true;
    }
    let mut pos = 12usize;
    while let Some(head) = bytes.get(pos..pos.saturating_add(8)) {
        let size = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let body = pos + 8;
        if &head[..4] == b"ICCP" {
            return size <= colour::MAX_ICC_BYTES && body.saturating_add(size) <= bytes.len();
        }
        // Chunks are padded to an even size.
        pos = body.saturating_add(size).saturating_add(size & 1);
    }
    true
}

/// The ICC profile embedded in a PNG, JPEG, TIFF or WebP.
pub(crate) fn embedded_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    craft_image_codecs::embedded_icc(bytes).ok().flatten()
}

/// The largest `shrink` [`decode_rgba_reduced`] takes: 3, an eighth of each side.
pub const MAX_REDUCED_SHRINK: u8 = jpeg::MAX_SHRINK;

/// [`decode_rgba`] of a JPEG at 1/2^`shrink` of its size (`shrink` 1 to [`MAX_REDUCED_SHRINK`], each
/// side rounded up), read from its compressed blocks without a full-size decode. The embedded ICC
/// profile is applied as [`decode_rgba`] applies it. `None` where only a full decode applies: other
/// formats, CMYK, 12-bit and lossless JPEGs, files whose size [`pixel_size`] cannot read, and files
/// [`decode_rgba`] refuses as too large.
pub fn decode_rgba_reduced(bytes: &[u8], shrink: u8) -> Option<image::RgbaImage> {
    use colour::{DisplayColour, Pixels, to_display_rgba};
    // The size check must have a size to check: a header only the JPEG decoder reads is refused.
    pixel_size(bytes)?;
    if too_large_to_decode(bytes) {
        return None;
    }
    let (rgba, has_colour) = jpeg::decode_reduced(bytes, shrink)?;
    let Some(icc) = embedded_icc(bytes) else { return Some(rgba) };
    let px = if has_colour { Pixels::Rgb(rgba) } else { Pixels::Gray(rgba) };
    to_display_rgba(px, Some(&icc), &DisplayColour::active())
}

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

/// The TIFF page that decoding opens; `None` for other formats.
fn default_tiff_page(bytes: &[u8]) -> Option<craft_image_codecs::TiffPage> {
    let mut info = craft_image_codecs::tiff_info(bytes).ok()?;
    let page = info.default_page()?;
    (page < info.pages.len()).then(|| info.pages.swap_remove(page))
}

/// A TIFF whose pixels are CMYK (PhotometricInterpretation Separated, four inks).
pub fn is_cmyk_tiff(bytes: &[u8]) -> bool {
    default_tiff_page(bytes).is_some_and(|page| page.photometric == Some(5) && matches!(page.samples_per_pixel, 4 | 5))
}

/// The ink values of a CMYK TIFF, which [`decode_rgba`] converts to RGB. 8- or 16-bit CMYK,
/// opaque or with alpha (associated alpha is made straight). `None` for anything else —
/// another format or another ink set (InkSet 2) — and for files over the decoder's 256 MiB
/// limit; [`is_cmyk_tiff`] tells those apart from RGB files.
pub fn decode_cmyk_tiff(bytes: &[u8]) -> Option<CmykRaster> {
    if too_large_to_decode(bytes) {
        return None;
    }
    // InkSet (tag 332): 1 or absent = CMYK; 2 = other inks, named in InkNames. Their samples
    // are not C, M, Y and K, so reading them as such would print the wrong colours.
    if default_tiff_page(bytes).is_some_and(|page| page.ink_set.is_some_and(|set| set != 1)) {
        return None;
    }
    let image = craft_image_codecs::decode_with(bytes, &decode_options()).ok()?;
    image.layout().is_cmyk().then(|| cmyk_raster(&image, bytes)).flatten()
}

/// An SVG rasterised so its longer side is `max_side` pixels: premultiplied RGBA8.
pub fn render_svg(bytes: &[u8], max_side: u32) -> Option<(Vec<u8>, u32, u32)> {
    let tree = svg_tree(bytes)?;
    let s = tree.size();
    let k = max_side as f32 / s.width().max(s.height()).max(1e-3);
    let (w, h) = ((s.width() * k).round().max(1.0) as u32, (s.height() * k).round().max(1.0) as u32);
    let pm = craft_svg::tree::draw::render(&tree, craft_svg::tree::draw::tiny_skia::Transform::from_scale(k, k), w, h, 0, MAX_RGBA_BYTES).ok()?;
    Some((pm.take(), w, h))
}

#[cfg(test)]
mod tests;
