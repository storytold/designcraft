//! Placed graphics: format sniffing, sizes and decoding for the renderer and PDF export.
//! Rasters go through `image` (PNG, JPEG, GIF, WebP, TIFF, BMP) or [`psd`] (Photoshop's merged
//! composite); SVG is parsed with usvg (text set in the bundled fonts, then the shared ones) and
//! rasterised with resvg for the screen — PDF export draws the same tree as vectors.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::{Arc, OnceLock};

pub use resvg::usvg;

pub mod colour;
mod eps;
mod psd;
mod tone;
pub use craft_eps::is_eps;
pub use eps::{bounding_box as eps_bounding_box, eps_pixel_size, eps_png, eps_proxy, eps_size};
pub use tone::{Tone, tone};

/// CSS pixels (SVG user units) → points.
pub const PT_PER_PX: f64 = 0.75;

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

/// SVG options with DesignCraft's font database and family/PostScript fallback policy.
/// The PDF adapter uses these same font choices while owning its separate parser.
pub fn svg_options_for_pdf() -> usvg::Options<'static> {
    usvg::Options {
        fontdb: svg_fontdb(),
        font_family: "Source Sans 3".into(),
        font_resolver: usvg::FontResolver { select_font: select_font(), select_fallback: select_fallback() },
        ..Default::default()
    }
}

/// Initialize the process-wide usvg options with preflighted data URLs; file-backed image URLs are never read.
fn svg_options() -> &'static usvg::Options<'static> {
    static OPTS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTS.get_or_init(|| {
        let mut options = svg_options_for_pdf();
        // Images inside an SVG come only from `data:` URLs: an `href` naming a file is not read
        // (it is left out, as usvg leaves out an image it can't load).
        let default_data = usvg::ImageHrefResolver::default_data_resolver();
        options.image_href_resolver = usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, options| {
                let raster = matches!(
                    craft_image_codecs::detect(&data),
                    Some(
                        craft_image_codecs::Format::Png
                            | craft_image_codecs::Format::Jpeg
                            | craft_image_codecs::Format::Gif
                            | craft_image_codecs::Format::WebP
                    )
                );
                if mime == "image/svg+xml" || (mime == "text/plain" && !raster) {
                    let xml = svg_text(&data, MAX_NESTED_SVG_BYTES.saturating_sub(NESTED_SVG_BYTES.get()).min(MAX_SVG_BYTES))?;
                    charge_nested_svg(xml.len())?;
                    default_data(mime, Arc::new(xml.into_owned().into_bytes()), options)
                } else {
                    default_data(mime, data, options)
                }
            }),
            resolve_string: Box::new(|_, _| None),
        };
        options
    })
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

/// Limits apply before usvg's recursive XML/SVG parsing, including SVGZ output.
/// Ordinary SVG doctypes are accepted; internal DTD subsets are not, since entity
/// expansion could introduce markup that the streaming depth check never sees.
const MAX_SVG_BYTES: usize = 16 * 1024 * 1024;
const MAX_SVG_EVENTS: usize = 100_000;
const MAX_SVG_DEPTH: usize = 64;

/// SVG bytes the data images of one document may add up to, all levels together: each nested
/// document gets the per-document limits, so without this one outer file could hold many of them.
const MAX_NESTED_SVG_BYTES: usize = 32 * 1024 * 1024;

thread_local! {
    /// Nested SVG bytes taken so far by the parse running on this thread (see [`svg_tree`]).
    static NESTED_SVG_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Take `n` bytes of the running parse's nested-SVG budget; `None` when they don't fit.
fn charge_nested_svg(n: usize) -> Option<()> {
    let used = NESTED_SVG_BYTES.get().checked_add(n).filter(|u| *u <= MAX_NESTED_SVG_BYTES)?;
    NESTED_SVG_BYTES.set(used);
    Some(())
}

/// Decode and structurally check SVG/SVGZ through the shared bounded parser.
fn svg_text(bytes: &[u8], limit: usize) -> Option<std::borrow::Cow<'_, str>> {
    let limits = craft_svg::tree::Limits {
        bytes: limit.min(MAX_SVG_BYTES),
        depth: MAX_SVG_DEPTH,
        xml_nodes: u32::try_from(MAX_SVG_EVENTS).ok()?,
        refuse_entity_markup: true,
        ..Default::default()
    };
    craft_svg::tree::text(bytes, &limits).ok()
}

/// Parse an SVG with at most 16 MiB encoded/decoded data, 100,000 XML events and
/// 64 nested elements. Reject internal DTD subsets before recursive conversion.
pub fn svg_tree(bytes: &[u8]) -> Option<usvg::Tree> {
    let xml = svg_text(bytes, MAX_SVG_BYTES)?;
    NESTED_SVG_BYTES.set(0);
    usvg::Tree::from_data(xml.as_bytes(), svg_options()).ok()
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
    // An EPS is drawn as its proxy (preview, simple PostScript or placeholder); the document
    // keeps the EPS itself.
    if is_eps(bytes) {
        return decode_rgba(&eps_proxy(bytes)?.0);
    }
    let decoded = craft_image_codecs::decode_with(bytes, &decode_options()).ok()?;
    if decoded.layout().is_cmyk() {
        let raster = cmyk_raster(&decoded)?;
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

fn cmyk_raster(image: &craft_image_codecs::Image) -> Option<CmykRaster> {
    let converted = image.convert(craft_image_codecs::ChannelLayout::CmykA, craft_image_codecs::SampleType::U8);
    let pixels = usize::try_from(converted.width()).ok()?.checked_mul(usize::try_from(converted.height()).ok()?)?;
    let data = converted.as_u8()?;
    if data.len() != pixels.checked_mul(5)? {
        return None;
    }
    let mut cmyk = Vec::with_capacity(pixels.checked_mul(4)?);
    let mut alpha = Vec::with_capacity(pixels);
    for &[c, m, y, k, a] in data.as_chunks::<5>().0 {
        cmyk.extend([c, m, y, k]);
        alpha.push(a);
    }
    let alpha = alpha.iter().any(|&a| a < 255).then_some(alpha);
    Some(CmykRaster { width: converted.width(), height: converted.height(), cmyk, alpha })
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

/// A TIFF whose pixels are CMYK (PhotometricInterpretation Separated, four inks).
pub fn is_cmyk_tiff(bytes: &[u8]) -> bool {
    craft_image_codecs::tiff_info(bytes)
        .ok()
        .and_then(|info| info.default_page().and_then(|i| info.pages.get(i).map(|page| (page.photometric, page.samples_per_pixel))))
        .is_some_and(|(photometric, samples)| photometric == Some(5) && matches!(samples, 4 | 5))
}

/// The ink values of a CMYK TIFF, which [`decode_rgba`] converts to RGB. 8- or 16-bit CMYK,
/// pixels interleaved, opaque or with unassociated alpha. `None` for anything else — another
/// format, another ink set (InkSet 2), planar storage or premultiplied alpha — and for files
/// over the decoder's 256 MiB limit; [`is_cmyk_tiff`] tells those apart from RGB files.
pub fn decode_cmyk_tiff(bytes: &[u8]) -> Option<CmykRaster> {
    if too_large_to_decode(bytes) {
        return None;
    }
    let image = craft_image_codecs::decode_with(bytes, &decode_options()).ok()?;
    image.layout().is_cmyk().then(|| cmyk_raster(&image)).flatten()
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
