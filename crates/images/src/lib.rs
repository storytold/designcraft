//! Placed graphics: format sniffing, sizes and decoding for the renderer and PDF export.
//! Rasters go through `image` (PNG, JPEG, GIF, WebP, TIFF, BMP) or [`psd`] (Photoshop's merged
//! composite); SVG is parsed with usvg (text set in the bundled fonts, then the shared ones) and
//! rasterised with resvg for the screen — PDF export draws the same tree as vectors.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::{Arc, OnceLock};

pub use resvg::usvg;

mod eps;
mod psd;
pub use eps::{bounding_box as eps_bounding_box, eps_proxy, is_eps};

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
        usvg::Options { fontdb: Arc::new(db), font_family: "Source Sans 3".into(), font_resolver, ..Default::default() }
    })
}

/// A family the SVG names that isn't bundled comes from the shared fonts (installed ones too),
/// as it would in a text frame.
fn select_font() -> usvg::FontSelectionFn<'static> {
    let bundled = usvg::FontResolver::default_font_selector();
    Box::new(move |font, db| {
        let fonts = designcraft_fonts::FontDb::global();
        for family in font.families() {
            if let usvg::FontFamily::Named(name) = family
                && !db.faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
                && fonts.has_family(name)
            {
                for style in fonts.styles(name) {
                    add_face(db, fonts.face(name, &style));
                }
            }
        }
        bundled(font, db)
    })
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
    let (w, h) = pixel_size(bytes)?;
    Some((w as f64, h as f64))
}

/// Pixel dimensions of a raster.
pub fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if is_psd(bytes) && bytes.len() >= 26 {
        // Header: signature, version, 6 reserved, channels, height, width (big-endian).
        let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        return Some((be(18), be(14)));
    }
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// Straight (non-premultiplied) RGBA8 of a raster.
pub fn decode_rgba(bytes: &[u8]) -> Option<image::RgbaImage> {
    if is_psd(bytes) {
        return psd::decode(bytes);
    }
    Some(image::load_from_memory(bytes).ok()?.to_rgba8())
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

fn tiff_decoder(bytes: &[u8]) -> Option<tiff::decoder::Decoder<std::io::Cursor<&[u8]>>> {
    let tiff = [b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"].iter().any(|m| bytes.starts_with(*m));
    if !tiff {
        return None;
    }
    tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).ok()
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
        // The screen still gets RGB.
        assert_eq!(decode_rgba(&b).unwrap().get_pixel(0, 0).0, [0, 0, 0, 255]);

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
