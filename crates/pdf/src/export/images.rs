#[cfg(test)]
use super::metadata::civil;
#[cfg(test)]
use super::pipeline::export_pdf_with_report;
#[cfg(test)]
use super::{
    Affine, AssetId, Cache, Content, Document, Exporter, Item, PageSettings, PdfOptions, PdfVersion, Rect, Size, Standard, Writer, registration_tests,
};
use super::{Image, krilla};
use std::collections::HashMap;
use std::sync::Arc;

/// A successful conversion and its name-independent diagnostic. Replayed once for each asset.
#[derive(Clone)]
pub(super) struct ConvertedImage {
    pub(super) image: Image,
    pub(super) warning: Option<&'static str>,
}

pub(super) const CMYK_FALLBACK_WARNING: &str =
    "converted to RGB (a CMYK TIFF that is planar, has premultiplied alpha, uses other inks or is over 256 MiB)";

/// An image Output colour conversion couldn't decode (or one past the decoding budget) goes in
/// as it is.
pub(super) const UNCONVERTED_WARNING: &str = "not converted to the Output destination (it couldn't be decoded for it), so it keeps its own colours";

#[cfg(not(test))]
pub(super) type ImageContentCache = HashMap<Arc<Vec<u8>>, ConvertedImage>;

// Unit tests force collisions through the real cache path; production keeps RandomState.
#[cfg(test)]
pub(super) type ImageContentCache = HashMap<Arc<Vec<u8>>, ConvertedImage, std::hash::BuildHasherDefault<ConstantImageHasher>>;

#[cfg(test)]
#[derive(Default)]
pub(super) struct ConstantImageHasher;

/// The sRGB profile (ICC v2.1) that untagged RGB images carry in PDF/X-4.
pub(crate) static SRGB_ICC: &[u8] = include_bytes!("../../../../assets/icc/sRGB-v2-magic.icc");

/// How raster images are written for the export's standard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RasterMode {
    /// `/Interpolate true`: smoother upscaling in viewers. PDF/X-4 and PDF/A don't allow it.
    pub(super) interpolate: bool,
    /// Give RGB images with no ICC profile of their own the sRGB profile: under PDF/X-4's CMYK
    /// output intent, RGB must be colour-managed.
    pub(super) tag_rgb: bool,
}

/// The ICC profile embedded in an image file, if it has one.
pub(super) fn embedded_icc(data: &[u8]) -> Option<Vec<u8>> {
    use image::ImageDecoder;
    if !designcraft_images::webp_icc_is_sane(data) {
        return None;
    }
    let mut dec = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?.into_decoder().ok()?;
    dec.icc_profile().ok().flatten()
}

/// Whether an ICC profile describes RGB (its colour space signature, header bytes 16 to 20).
fn is_rgb_icc(icc: &[u8]) -> bool {
    icc.get(16..20) == Some(b"RGB ".as_slice())
}

/// Whether an ICC profile describes grey.
pub(super) fn is_grey_icc(icc: &[u8]) -> bool {
    icc.get(16..20) == Some(b"GRAY".as_slice())
}

/// Whether `data` decodes to grey (with or without alpha), read from its header.
pub(super) fn is_grey(data: &[u8]) -> bool {
    use image::ImageDecoder;
    image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_decoder().ok())
        .is_some_and(|dec| !dec.color_type().has_color())
}

/// A JPEG, passed through. With `tag_rgb`, a JPEG keeps its own profile, and one without gets
/// sRGB. krilla uses a profile only when its channels match the JPEG's, so a grey or CMYK JPEG
/// without a profile stays DeviceGray or DeviceCMYK.
pub(super) fn jpeg(data: krilla::Data, icc: impl FnOnce() -> Option<Vec<u8>>, mode: RasterMode) -> Option<Image> {
    if !mode.tag_rgb {
        return Image::from_jpeg(data, mode.interpolate).ok();
    }
    let icc: krilla::Data = match icc() {
        Some(own) => own.into(),
        None => SRGB_ICC.into(),
    };
    Image::from_jpeg_with_icc(data, Some(icc), mode.interpolate).ok()
}

/// Whether krilla would write this PNG, GIF or WebP as untagged RGB: not grey, and no profile of
/// its own.
fn untagged_rgb(data: &[u8]) -> bool {
    use image::ImageDecoder;
    if !designcraft_images::webp_icc_is_sane(data) {
        return false;
    }
    let Some(mut dec) = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok().and_then(|r| r.into_decoder().ok()) else {
        return false;
    };
    let grey = matches!(dec.color_type(), image::ColorType::L8 | image::ColorType::La8 | image::ColorType::L16 | image::ColorType::La16);
    !grey && dec.icc_profile().ok().flatten().is_none()
}

pub(super) fn lossless(data: &Arc<Vec<u8>>, fmt: Option<image::ImageFormat>, mode: RasterMode) -> Option<Image> {
    // krilla reads PNG, GIF and WebP lazily: a damaged file is accepted here and fails the whole
    // export when the PDF is written. Decoding it first means it is skipped, like any other
    // image that can't be decoded.
    let rgba = designcraft_images::decode_rgba(data)?;
    let i = mode.interpolate;
    let direct = match fmt {
        Some(image::ImageFormat::Png | image::ImageFormat::Gif | image::ImageFormat::WebP) if mode.tag_rgb && untagged_rgb(data) => None,
        Some(image::ImageFormat::Png) => Image::from_png(data.clone().into(), i).ok(),
        Some(image::ImageFormat::Gif) => Image::from_gif(data.clone().into(), i).ok(),
        Some(image::ImageFormat::WebP) => Image::from_webp(data.clone().into(), i).ok(),
        _ => None,
    };
    direct.or_else(|| {
        // A grey TIFF, BMP… stays DeviceGray, as a grey PNG or JPEG does (#1426).
        if let Some(grey) = GreyImage::new(data) {
            return Image::from_custom(grey, i).ok();
        }
        // TIFF, BMP, PSD composites… and, with `tag_rgb`, untagged RGB.
        if mode.tag_rgb {
            return SrgbImage::new(rgba).and_then(|img| Image::from_custom(img, i).ok());
        }
        let (w, h) = rgba.dimensions();
        Some(Image::from_rgba8(rgba.into_raw(), w, h))
    })
}

/// An RGB raster tagged with the sRGB profile, for PDF/X-4.
#[derive(Clone, Hash)]
pub(super) struct SrgbImage {
    pub(super) rgb: Arc<Vec<u8>>,
    /// Only when some pixel is not opaque.
    pub(super) alpha: Option<Arc<Vec<u8>>>,
    pub(super) size: (u32, u32),
}

/// A grey raster as a DeviceGray image (ICCBased when the file has a grey profile of its own), so
/// a page that only holds black prints as black, not as RGB.
#[derive(Clone, Hash)]
pub(super) struct GreyImage {
    pub(super) grey: Arc<Vec<u8>>,
    /// Only when some pixel is not opaque.
    pub(super) alpha: Option<Arc<Vec<u8>>>,
    pub(super) size: (u32, u32),
    pub(super) icc: Option<Arc<Vec<u8>>>,
}

/// A CMYK raster as a krilla image: its samples are written unchanged as DeviceCMYK, like a
/// CMYK JPEG's. An embedded ICC profile is left out, as it is for JPEGs: the inks are meant for
/// the output intent's press, and tagging them with another CMYK profile would let a RIP
/// convert 100% K to four-colour black.
#[derive(Clone, Hash)]
pub(super) struct CmykImage(pub(super) Arc<designcraft_images::CmykRaster>);

/// A CMYK TIFF as a DeviceCMYK image (see [`cmyk_inks`]). `None` when it can't be read as CMYK.
pub(super) fn cmyk_tiff(data: &[u8], mode: RasterMode, c: &Compression) -> Option<Image> {
    cmyk_inks(designcraft_images::decode_cmyk_tiff(data)?, mode, c)
}

/// A CMYK Photoshop file as a DeviceCMYK image of its composite's inks (see [`cmyk_inks`]).
/// `None` when it can't be read as CMYK.
pub(super) fn cmyk_psd(data: &[u8], mode: RasterMode, c: &Compression) -> Option<Image> {
    cmyk_inks(designcraft_images::decode_cmyk_psd(data)?, mode, c)
}

/// A placed raster's inks as a DeviceCMYK image: downsampled when the colour class asks for it,
/// into the Output destination when it converts CMYK, then a CMYK JPEG at the class's own
/// quality or lossless. The legacy `compress_images` switch never makes these JPEG: it kept
/// their inks exact, and callers rely on that.
fn cmyk_inks(r: designcraft_images::CmykRaster, mode: RasterMode, c: &Compression) -> Option<Image> {
    let plan = c.plan(c.classes.color, true, (r.width, r.height));
    let quality = plan.quality.filter(|_| c.classes.color.quality.is_some());
    let conv = c.to_cmyk.as_deref().filter(|v| v.converts_cmyk());
    cmyk_image(r, plan.size, quality, conv, mode)
}

/// A CMYK JPEG: passed through (its inks as they are) unless it is downsampled or converted into
/// another CMYK destination; then decoded and written again, at the colour class's quality or
/// lossless.
fn cmyk_jpeg(data: &Arc<Vec<u8>>, mode: RasterMode, c: &Compression) -> Option<Image> {
    let pass = || jpeg(data.clone().into(), || embedded_icc(data), mode);
    let Some(px) = designcraft_images::pixel_size(data) else { return pass() };
    let size = c.plan(c.classes.color, true, px).size;
    let conv = c.to_cmyk.as_deref().filter(|v| v.converts_cmyk());
    if size.is_none() && conv.is_none() {
        return pass();
    }
    let decoded = if designcraft_images::too_large_to_decode(data) { None } else { designcraft_images::decode_cmyk_jpeg(data) };
    let Some(r) = decoded else {
        c.unconverted.set(conv.is_some());
        return pass();
    };
    cmyk_image(r, size, c.classes.color.quality, conv, mode)
}

/// Ink values (working CMYK) as a DeviceCMYK image: resized to `size`, through `conv`, then a
/// CMYK JPEG at `quality` when opaque, else lossless with its alpha. `None` when the channels
/// don't match the size.
fn cmyk_image(
    mut r: designcraft_images::CmykRaster,
    size: Option<(u32, u32)>,
    quality: Option<u8>,
    conv: Option<&crate::color_out::CmykConverter>,
    mode: RasterMode,
) -> Option<Image> {
    // krilla panics when the channel lengths don't match the size.
    let pixels = usize::try_from(r.width).ok()?.checked_mul(usize::try_from(r.height).ok()?)?;
    if r.cmyk.len() != pixels.checked_mul(4)? || r.alpha.as_ref().is_some_and(|a| a.len() != pixels) {
        return None;
    }
    if let Some((w, h)) = size {
        // Four 8-bit inks resample like RGBA; alpha like a grey image.
        let cmyk = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(r.width, r.height, std::mem::take(&mut r.cmyk))?;
        let alpha = match r.alpha.take() {
            Some(a) => {
                let a = image::ImageBuffer::<image::Luma<u8>, Vec<u8>>::from_raw(r.width, r.height, a)?;
                Some(image::imageops::resize(&a, w, h, DOWNSAMPLE).into_raw())
            }
            None => None,
        };
        r = designcraft_images::CmykRaster { width: w, height: h, cmyk: image::imageops::resize(&cmyk, w, h, DOWNSAMPLE).into_raw(), alpha };
    }
    if let Some(conv) = conv {
        r = conv.cmyk_raster(r);
    }
    if let Some(q) = quality
        && r.alpha.is_none()
        && let Some(buf) = encode_cmyk_jpeg(&r, q)
        && let Some(img) = jpeg(buf.into(), || None, mode)
    {
        return Some(img);
    }
    Image::from_custom(CmykImage(Arc::new(r)), mode.interpolate).ok()
}

/// Inks as a CMYK JPEG at `quality`. The encoder writes Adobe-inverted samples with Adobe's
/// marker, so the PDF's Decode array turns them back. `None` past JPEG's 65 535-pixel sides, or
/// when the inks don't fill the size.
fn encode_cmyk_jpeg(r: &designcraft_images::CmykRaster, quality: u8) -> Option<Vec<u8>> {
    let (w, h) = (u16::try_from(r.width).ok().filter(|w| *w > 0)?, u16::try_from(r.height).ok().filter(|h| *h > 0)?);
    if r.cmyk.len() != usize::from(w).checked_mul(usize::from(h))?.checked_mul(4)? {
        return None;
    }
    let mut buf = Vec::new();
    jpeg_encoder::Encoder::new(&mut buf, quality.clamp(1, 100)).encode(&r.cmyk, w, h, jpeg_encoder::ColorType::Cmyk).ok()?;
    Some(buf)
}

#[cfg(test)]
thread_local! {
    /// Rasters `raster` has decoded on this thread.
    static RASTER_DECODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The reference app's "Bicubic Downsampling".
const DOWNSAMPLE: image::imageops::FilterType = image::imageops::FilterType::CatmullRom;

/// Export PDF › Compression as it applies to one image.
pub(super) struct Compression {
    pub(super) classes: crate::ImageCompression,
    /// `PdfOptions::compress_images`: JPEG quality 90 for classes without a quality of their own.
    pub(super) legacy_jpeg: bool,
    /// The image's narrowest placed width in points (`None`: not placed, so never downsampled).
    pub(super) placed: Option<f64>,
    /// Output: colour images go into this destination CMYK.
    pub(super) to_cmyk: Option<Arc<crate::color_out::CmykConverter>>,
    /// PDF/A: no CMYK raster goes in, a CMYK JPEG included.
    pub(super) rgb_only: bool,
    /// Set when an image to convert into `to_cmyk` went in unconverted (it couldn't be decoded,
    /// or is past the decoding budget), for the export's warning.
    pub(super) unconverted: std::cell::Cell<bool>,
}

/// How a raster goes into the PDF: resized to `size` when downsampling applies, and as JPEG at
/// `quality` (`None` = lossless).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct RasterPlan {
    pub(super) size: Option<(u32, u32)>,
    pub(super) quality: Option<u8>,
}

impl Compression {
    /// The plan for a raster of `px` pixels in `class`; `lossy` allows JPEG (not for monochrome).
    pub(super) fn plan(&self, class: crate::ImageClass, lossy: bool, px: (u32, u32)) -> RasterPlan {
        let quality = if lossy { class.quality.or(self.legacy_jpeg.then_some(90)).map(|q| q.clamp(1, 100)) } else { None };
        let size = (|| {
            let to = class.downsample_to.filter(|_| class.downsamples())?;
            let placed = self.placed.filter(|w| w.is_finite() && *w > 1e-6)?;
            let ppi = f64::from(px.0) / (placed / 72.0);
            if !ppi.is_finite() || ppi <= class.above.max(to) {
                return None;
            }
            let k = to / ppi;
            let side = |n: u32| (f64::from(n) * k).round().clamp(1.0, f64::from(n)) as u32;
            let (w, h) = (side(px.0), side(px.1));
            (w < px.0 || h < px.1).then_some((w, h))
        })();
        RasterPlan { size, quality }
    }
}

/// Whether every pixel is black or white: a monochrome image (decoded to grey).
fn is_mono(img: &image::DynamicImage) -> bool {
    !img.color().has_alpha() && img.to_luma8().pixels().all(|p| p[0] == 0 || p[0] == 255)
}

/// `img` as JPEG at `quality`: grey stays grey.
fn encode_jpeg(img: &image::DynamicImage, grey: bool, quality: u8) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    if grey {
        let l = img.to_luma8();
        image::ImageEncoder::write_image(enc, l.as_raw(), l.width(), l.height(), image::ExtendedColorType::L8).ok()?;
    } else {
        let rgb = img.to_rgb8();
        image::ImageEncoder::write_image(enc, rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).ok()?;
    }
    Some(buf)
}

/// `img` as PNG with `icc`: a downsampled image kept lossless goes in through [`lossless`] as a
/// PNG of the source's channels would.
fn encode_png(img: &image::DynamicImage, grey: bool, icc: Option<Vec<u8>>) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let mut buf = Vec::new();
    let mut enc = image::codecs::png::PngEncoder::new(&mut buf);
    if let Some(icc) = icc {
        // A profile the encoder can't take: no PNG, so the pixels aren't left untagged.
        enc.set_icc_profile(icc).ok()?;
    }
    if grey {
        let la = img.to_luma_alpha8();
        enc.write_image(la.as_raw(), la.width(), la.height(), image::ExtendedColorType::La8).ok()?;
    } else {
        let rgba = img.to_rgba8();
        enc.write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8).ok()?;
    }
    Some(buf)
}

/// A raster other than a CMYK TIFF per Compression and Output: downsampled when its class asks
/// for it, converted into the Output destination (colour images), then JPEG at the class's quality
/// (opaque colour and grey images; CMYK JPEG when converted) or lossless. A JPEG is passed through
/// unless it is downsampled or converted; a CMYK JPEG keeps its inks (see `cmyk_jpeg`), except in
/// PDF/A, which takes it as RGB. `None` when the image can't be decoded at all.
pub(super) fn raster(data: &Arc<Vec<u8>>, fmt: Option<image::ImageFormat>, mode: RasterMode, c: &Compression) -> Option<Image> {
    use image::ImageDecoder;
    // PDF/A takes no CMYK: a CMYK JPEG is decoded to display sRGB and written again as RGB.
    let cmyk_to_rgb = fmt == Some(image::ImageFormat::Jpeg) && designcraft_images::is_cmyk_jpeg(data);
    if cmyk_to_rgb && !c.rgb_only {
        return cmyk_jpeg(data, mode, c);
    }
    let conv = c.to_cmyk.as_deref();
    // Formats `image` reads are probed from their header. A PSD, which only `designcraft_images`
    // reads, counts as colour that may be transparent.
    let probe = image::ImageReader::new(std::io::Cursor::new(&data[..])).with_guessed_format().ok().and_then(|r| r.into_decoder().ok()).map(|d| {
        let ct = d.color_type();
        let grey = matches!(ct, image::ColorType::L8 | image::ColorType::La8 | image::ColorType::L16 | image::ColorType::La16);
        (d.dimensions(), grey, ct.has_alpha())
    });
    let readable = probe.is_some();
    let Some((px, grey, typed_alpha)) = probe.or_else(|| designcraft_images::pixel_size(data).map(|px| (px, false, true))) else {
        return lossless(data, fmt, mode);
    };
    let classes = &c.classes;
    let plan = c.plan(if grey { classes.gray } else { classes.color }, true, px);
    // Monochrome is told from the pixels, so a grey image starts in the grey class; it may
    // downsample as monochrome when the grey class doesn't.
    let mono_may_shrink = grey && c.plan(classes.mono, false, px).size.is_some();
    // Output conversion: a colour image goes into the destination CMYK; grey stays grey.
    let converting = conv.is_some() && !grey;
    if plan.size.is_none() && !mono_may_shrink && !converting && !cmyk_to_rgb {
        // A JPEG is never re-encoded (a second lossy generation) unless it is downsampled or
        // converted.
        if fmt == Some(image::ImageFormat::Jpeg) {
            return jpeg(data.clone().into(), || embedded_icc(data), mode);
        }
        if plan.quality.is_none() {
            return lossless(data, fmt, mode);
        }
    }
    // Past the RGBA budget nothing is decoded here; `lossless` refuses it too (#1586).
    if designcraft_images::too_large_to_decode(data) {
        c.unconverted.set(converting);
        return lossless(data, fmt, mode);
    }
    #[cfg(test)]
    {
        RASTER_DECODES.with(|n| n.set(n.get() + 1));
    }
    // Files `image` reads keep their own samples (their profile goes along); the others, and an
    // image to convert, are decoded to display sRGB (a converted image starts from display sRGB,
    // its own profile applied).
    let decoded = if readable && !converting && !cmyk_to_rgb {
        image::load_from_memory(data).ok()
    } else {
        designcraft_images::decode_rgba(data).map(image::DynamicImage::ImageRgba8)
    };
    // What can't be decoded here goes in as it is, as it would without Compression.
    let Some(img) = decoded else {
        c.unconverted.set(converting);
        return lossless(data, fmt, mode);
    };
    let plan = if grey && is_mono(&img) { c.plan(classes.mono, false, px) } else { plan };
    if plan.size.is_none() && !converting && !cmyk_to_rgb {
        // Decoded only because it might have been monochrome: a JPEG still isn't re-encoded.
        if fmt == Some(image::ImageFormat::Jpeg) {
            return jpeg(data.clone().into(), || embedded_icc(data), mode);
        }
        if plan.quality.is_none() {
            return lossless(data, fmt, mode);
        }
    }
    let img = match plan.size {
        Some((w, h)) => img.resize_exact(w, h, DOWNSAMPLE),
        None => img,
    };
    if let Some(conv) = conv.filter(|_| converting) {
        // A converted raster is CMYK JPEG only at its class's own Image Quality: the legacy
        // `compressImages` alone never makes a CMYK raster JPEG.
        let quality = plan.quality.filter(|_| classes.color.quality.is_some());
        return cmyk_image(conv.rgb_raster(&img.to_rgba8()), None, quality, None, mode);
    }
    let icc = || if readable { embedded_icc(data).filter(|p| if grey { is_grey_icc(p) } else { is_rgb_icc(p) }) } else { None };
    let transparent = typed_alpha && has_transparency(&img);
    if let Some(q) = plan.quality.filter(|_| !transparent)
        && let Some(buf) = encode_jpeg(&img, grey, q)
    {
        return jpeg(buf.into(), icc, mode);
    }
    // Not downsampled (transparent, or lossless by choice): the file goes in as it is, unless it
    // is a CMYK JPEG on its way to RGB.
    if plan.size.is_none() && !cmyk_to_rgb {
        return lossless(data, fmt, mode);
    }
    match encode_png(&img, grey, icc()) {
        Some(png) => lossless(&Arc::new(png), Some(image::ImageFormat::Png), mode),
        // A profile the encoder refuses: the file as it is rather than untagged pixels.
        None => lossless(data, fmt, mode),
    }
}

/// Whether a pixel of `img` is less than opaque, read from its own alpha channel (no copy for
/// the 8-bit layouts).
fn has_transparency(img: &image::DynamicImage) -> bool {
    use image::DynamicImage as D;
    match img {
        D::ImageRgba8(b) => b.pixels().any(|p| p[3] < 255),
        D::ImageLumaA8(b) => b.pixels().any(|p| p[1] < 255),
        i if i.color().has_alpha() => i.to_rgba16().pixels().any(|p| p[3] < u16::MAX),
        _ => false,
    }
}

/// The narrowest placed width (points) of each raster asset, over spreads, parent pages and
/// objects anchored in text, parent transforms included: with its pixel width it gives the
/// effective resolution Compression compares. The width is the length of the image's transformed
/// x axis, so a graphic scaled unevenly counts its horizontal scale. Assets holding the same file
/// share the narrowest of their widths, as they share one embedded image. Empty, without walking
/// the document, when no class of `images` downsamples.
pub(super) fn placed_widths(doc: &designcraft_doc::Document, images: &crate::ImageCompression) -> HashMap<designcraft_doc::AssetId, f64> {
    use designcraft_doc::{Content, Item};
    use designcraft_geom::Affine;
    const MAX_DEPTH: usize = 64;
    fn visit(it: &Item, xf: Affine, out: &mut HashMap<designcraft_doc::AssetId, f64>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let m = xf * it.xf;
        match &it.content {
            Content::Graphic(g) => {
                let [a, b, ..] = (m * g.xf).as_coeffs();
                let w = g.size.0 * a.hypot(b);
                if w.is_finite() && w > 0.0 {
                    out.entry(g.asset).and_modify(|v| *v = v.min(w)).or_insert(w);
                }
            }
            Content::Group { items } => {
                for child in items {
                    visit(child, m, out, depth + 1);
                }
            }
            _ => {}
        }
    }
    let mut out = HashMap::new();
    if !images.downsamples() {
        return out;
    }
    for it in doc.spreads.iter().chain(doc.parents.iter()).flat_map(|sp| &sp.items) {
        visit(it, Affine::IDENTITY, &mut out, 0);
    }
    // Anchored objects: their own transform carries their scale.
    let mut stories: Vec<&designcraft_doc::Story> = doc.stories.values().map(AsRef::as_ref).collect();
    while let Some(story) = stories.pop() {
        stories.extend(story.notes.iter().chain(&story.endnotes).map(|n| &n.text));
        stories.extend(story.tables.values().flat_map(|t| t.cells.iter().map(|c| &c.text)));
        for o in &story.objects {
            visit(&o.item, Affine::IDENTITY, &mut out, 0);
        }
    }
    // One file, one embedded image: the narrowest placement of any asset holding it. Only assets
    // of the same length can hold the same file, so only those are compared.
    let mut by_len: HashMap<usize, Vec<designcraft_doc::AssetId>> = HashMap::new();
    for id in out.keys() {
        if let Some(a) = doc.assets.get(id) {
            by_len.entry(a.data.len()).or_default().push(*id);
        }
    }
    for ids in by_len.into_values().filter(|ids| ids.len() > 1) {
        let mut by_content: HashMap<&[u8], f64> = HashMap::new();
        for id in &ids {
            if let (Some(a), Some(w)) = (doc.assets.get(id), out.get(id)) {
                by_content.entry(a.data.as_slice()).and_modify(|v| *v = v.min(*w)).or_insert(*w);
            }
        }
        for id in &ids {
            if let (Some(a), Some(w)) = (doc.assets.get(id), out.get_mut(id))
                && let Some(narrowest) = by_content.get(a.data.as_slice())
            {
                *w = *narrowest;
            }
        }
    }
    out
}

#[cfg(test)]
mod image_content_cache_tests {
    use super::*;
    use designcraft_doc::Asset;

    fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba(color));
        let mut bytes = Vec::new();
        image.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        bytes
    }
    fn asset(doc: &mut Document, bytes: Vec<u8>, name: &str) -> AssetId {
        let id = AssetId(doc.alloc());
        doc.assets.insert(id, Arc::new(Asset { id, name: name.into(), mime: "image/png".into(), data: Arc::new(bytes), ..Default::default() }));
        id
    }
    fn exporter<'a>(doc: &'a Document, cache: &'a Cache, opts: &'a PdfOptions) -> Exporter<'a> {
        Exporter {
            drawing_items: Vec::new(),
            inks: crate::inks::ink_set(doc),
            doc,
            cache,
            composed: HashMap::new(),
            opts,
            clip: Rect::ZERO,
            warnings: Vec::new(),
            images: HashMap::new(),
            images_by_content: ImageContentCache::default(),
            placed_widths: placed_widths(doc, &opts.images),
            image_conversions: 0,
            pdfs: HashMap::new(),
            variants: HashMap::new(),
            has_optional_content: false,
            pdf_preparations: 0,
            layer_error: None,
            version: PdfVersion::Pdf17,
            too_new: Vec::new(),
            svgs: HashMap::new(),
            fonts: HashMap::new(),
            reverse_cmaps: HashMap::new(),
            rgb_only: false,
            to_cmyk: None,
            tags: Vec::new(),
            story_tags: HashMap::new(),
            tag_story: None,
            text_area: None,
            para_tags: HashMap::new(),
        }
    }
    #[test]
    fn prepared_pdf_variants_reuse_parsing_by_asset_and_hidden_names() {
        let mut doc = Document::new(&Default::default());
        let cache = Cache::new();
        let opts = PdfOptions::default();
        // One layer, "Hidden", shown by default: hiding it needs a prepared copy, showing it doesn't.
        let content = "/OC /a BDC 0 0 1 rg 0 0 100 100 re f EMC";
        let source = registration_tests::assemble(&[
            "<</Type/Catalog/Pages 2 0 R/OCProperties<</OCGs[5 0 R]>>>>".into(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".into(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</Properties<</a 5 0 R>>>>/Contents 4 0 R>>".into(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
            "<</Type/OCG/Name(Hidden)>>".into(),
        ]);
        let a = asset(&mut doc, source.clone(), "a.pdf");
        let b = asset(&mut doc, source, "b.pdf");
        let mut ex = exporter(&doc, &cache, &opts);
        let mut writer = Writer::new(Default::default(), Default::default());
        let mut page = writer.document_mut().start_page_with(PageSettings::new(Size::from_wh(100.0, 100.0).unwrap()));
        let mut surface = page.surface();
        for (id, hidden, count) in [(a, None, 1), (a, None, 1), (a, Some(vec!["Hidden".into()]), 2), (b, None, 3), (a, None, 3)] {
            let graphic = designcraft_doc::Graphic {
                asset: id,
                size: (100.0, 100.0),
                xf: Affine::IDENTITY,
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
                layer_options: None,
            };
            ex.graphic_unclipped(&mut surface, &graphic, hidden.as_deref());
            assert!(ex.layer_error.is_none());
            assert_eq!(ex.pdf_preparations, count, "count actual preparations, not writer resource deduplication");
        }
        assert_eq!(ex.variants.len(), 3);
        // a, b, and a with its layer hidden (a copy of its own).
        assert_eq!(ex.pdfs.len(), 3);
        let variant = ex.variants.get(&(a, None)).unwrap();
        assert!(Arc::ptr_eq(&variant.data, &doc.assets[&a].data));
        surface.finish();
        page.finish();
        assert!(writer.finish().is_ok());
    }
    #[test]
    fn exact_bytes_reuse_conversion_under_real_hash_collisions() {
        use std::hash::{BuildHasher, Hasher};
        let mut d = Document::new(&Default::default());
        let red = png(2, 1, [255, 0, 0, 255]);
        let blue = png(1, 2, [0, 0, 255, 128]);
        let a1 = asset(&mut d, red.clone(), "a1");
        let b1 = asset(&mut d, blue.clone(), "b1");
        let a2 = asset(&mut d, red, "a2");
        let b2 = asset(&mut d, blue, "b2");
        assert!(!Arc::ptr_eq(&d.assets[&a1].data, &d.assets[&a2].data));
        let cache = Cache::new();
        let opts = PdfOptions::default();
        let mut ex = exporter(&d, &cache, &opts);
        let mut h1 = ex.images_by_content.hasher().build_hasher();
        h1.write(&d.assets[&a1].data);
        let mut h2 = ex.images_by_content.hasher().build_hasher();
        h2.write(&d.assets[&b1].data);
        assert_eq!(h1.finish(), h2.finish());
        assert_ne!(d.assets[&a1].data, d.assets[&b1].data);
        for (id, count, size) in [(a1, 1, (2, 1)), (b1, 2, (1, 2)), (a2, 2, (2, 1)), (a1, 2, (2, 1)), (b2, 2, (1, 2))] {
            assert_eq!(ex.load_image(id).unwrap().size(), size);
            assert_eq!(ex.image_conversions, count, "count actual converter executions, not krilla resource equality");
        }
        assert_eq!(ex.images.len(), 4);
        assert_eq!(ex.images_by_content.len(), 2);
        let first_key = ex.images_by_content.keys().find(|key| key.as_slice() == d.assets[&a1].data.as_slice()).unwrap();
        assert!(Arc::ptr_eq(first_key, &d.assets[&a1].data), "cache keys share the original encoded payload");
        ex.images_by_content.clear();
        assert_eq!(ex.load_image(a1).unwrap().size(), (2, 1));
        assert_eq!(ex.image_conversions, 2, "the AssetId cache remains the first lookup");
        assert!(ex.images_by_content.is_empty());
    }
    #[test]
    fn successful_content_cache_does_not_suppress_failure_names() {
        let mut d = Document::new(&Default::default());
        let a = asset(&mut d, vec![0, 1, 2, 3], "broken-a.png");
        let b = asset(&mut d, vec![0, 1, 2, 3], "broken-b.png");
        let cache = Cache::new();
        let opts = PdfOptions::default();
        let mut ex = exporter(&d, &cache, &opts);
        assert!(ex.load_image(a).is_none());
        assert!(ex.load_image(b).is_none());
        assert!(ex.load_image(a).is_none());
        assert_eq!(ex.image_conversions, 2);
        assert!(ex.images_by_content.is_empty());
        assert_eq!(ex.warnings.len(), 2);
        assert!(ex.warnings[0].contains("broken-a.png"));
        assert!(ex.warnings[1].contains("broken-b.png"));
    }
    #[test]
    fn content_cache_is_scoped_to_each_immutable_options_context() {
        let mut d = Document::new(&Default::default());
        let data = png(2, 2, [240, 20, 30, 255]);
        let a = asset(&mut d, data.clone(), "a");
        let b = asset(&mut d, data, "b");
        let cache = Cache::new();
        for compress_images in [false, true, false] {
            let opts = PdfOptions { compress_images, ..Default::default() };
            let mut ex = exporter(&d, &cache, &opts);
            assert!(ex.load_image(a).is_some());
            assert!(ex.load_image(b).is_some());
            assert_eq!(ex.image_conversions, 1);
            assert_eq!(ex.images_by_content.len(), 1);
        }
    }

    #[test]
    fn successful_cmyk_fallback_replays_each_asset_name_once_and_converts_once() {
        let bytes = crate::image_fixtures::cmyk_tiff(None, true);
        assert!(designcraft_images::is_cmyk_tiff(&bytes));
        assert!(designcraft_images::decode_cmyk_tiff(&bytes).is_none());
        assert!(designcraft_images::decode_rgba(&bytes).is_some());
        let mut d = Document::new(&Default::default());
        let a = asset(&mut d, bytes.clone(), "ink-a.tif");
        let b = asset(&mut d, bytes, "ink-b.tif");
        let cache = Cache::new();
        for compress_images in [false, true] {
            let opts = PdfOptions { compress_images, ..Default::default() };
            let mut ex = exporter(&d, &cache, &opts);
            for id in [a, b, a, b] {
                assert_eq!(ex.load_image(id).unwrap().size(), (2, 1));
            }
            assert_eq!(ex.image_conversions, 1);
            assert_eq!(ex.images_by_content.len(), 1);
            assert_eq!(ex.warnings, [format!("ink-a.tif: {CMYK_FALLBACK_WARNING}"), format!("ink-b.tif: {CMYK_FALLBACK_WARNING}")]);
        }
    }

    #[test]
    fn cmyk_alpha_and_rgb_only_context_keep_conversion_counts_and_failures() {
        let cache = Cache::new();
        for alpha in [None, Some(2), Some(1)] {
            let mut d = Document::new(&Default::default());
            let bytes = crate::image_fixtures::cmyk_tiff(alpha, false);
            // RGB-only exports get the RGB the screen shows.
            let screen_rgb: Vec<u8> =
                designcraft_images::decode_rgba(&bytes).map_or_else(Vec::new, |img| img.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect());
            let a = asset(&mut d, bytes.clone(), "a.tif");
            let b = asset(&mut d, bytes, "b.tif");
            for standard in [Standard::None, Standard::PdfA2b, Standard::PdfX4, Standard::PdfA2b, Standard::None] {
                let opts = PdfOptions { standard, ..Default::default() };
                let mut ex = exporter(&d, &cache, &opts);
                ex.rgb_only = standard == Standard::PdfA2b;
                // Unassociated alpha (2) decodes as inks, and as RGB for RGB-only exports.
                // Associated alpha is rejected by both paths: retain both failure names.
                let succeeds = alpha.is_none() || alpha == Some(2);
                let first = ex.load_image(a);
                assert_eq!(first.is_some(), succeeds, "{alpha:?} {standard:?}");
                if alpha.is_none() {
                    let (space, samples) = image_color_samples(first.unwrap());
                    assert_eq!(space, if ex.rgb_only { "DeviceRGB" } else { "DeviceCMYK" });
                    assert_eq!(samples, if ex.rgb_only { screen_rgb.clone() } else { crate::image_fixtures::INKS.to_vec() });
                }
                assert_eq!(ex.load_image(b).is_some(), succeeds, "{alpha:?} {standard:?}");
                assert_eq!(ex.load_image(a).is_some(), succeeds);
                assert_eq!(ex.image_conversions, if succeeds { 1 } else { 2 });
                assert_eq!(ex.images_by_content.len(), usize::from(succeeds));
                if succeeds {
                    assert!(ex.warnings.is_empty());
                } else {
                    for name in ["a.tif", "b.tif"] {
                        assert!(ex.warnings.contains(&format!("image `{name}` could not be decoded and was skipped")));
                        assert_eq!(ex.warnings.contains(&format!("{name}: {CMYK_FALLBACK_WARNING}")), !ex.rgb_only);
                    }
                }
            }
        }
    }

    // Serialize the converted raster without an archival validator so the RGB-only pixel
    // contract stays independent of main's existing PDF/A interpolation validation failure.
    fn image_color_samples(image: Image) -> (String, Vec<u8>) {
        use hayro_syntax::object::Name;
        let mut pdf = krilla::Document::new();
        let size = Size::from_wh(20.0, 20.0).unwrap();
        let mut page = pdf.start_page_with(PageSettings::new(size));
        let mut surface = page.surface();
        surface.draw_image(image, size);
        surface.finish();
        page.finish();
        let bytes = pdf.finish().unwrap();
        let pdf = hayro_syntax::Pdf::new(bytes).unwrap();
        let stream = pdf
            .objects()
            .into_iter()
            .filter_map(|o| o.into_stream())
            .find(|s| s.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
            .unwrap();
        (stream.dict().get::<Name>("ColorSpace").unwrap().as_str().to_owned(), stream.decoded().unwrap().into_owned())
    }

    #[test]
    fn content_cache_owns_only_existing_payloads_and_drops_with_exporter() {
        let mut d = Document::new(&Default::default());
        let bytes = png(2, 1, [20, 30, 40, 255]);
        let a = asset(&mut d, bytes.clone(), "a.png");
        let b = asset(&mut d, bytes, "b.png");
        let payload = Arc::downgrade(&d.assets[&a].data);
        let duplicate = Arc::downgrade(&d.assets[&b].data);
        let cache = Cache::new();
        let opts = PdfOptions::default();
        let mut ex = exporter(&d, &cache, &opts);
        assert!(ex.load_image(a).is_some());
        assert!(ex.load_image(b).is_some());
        assert_eq!(duplicate.strong_count(), 1, "a content hit must not retain the second encoded allocation");
        let key = ex.images_by_content.keys().next().unwrap();
        assert!(Arc::ptr_eq(key, &d.assets[&a].data));
        drop(ex);
        drop(d);
        assert!(payload.upgrade().is_none(), "no converted image or encoded key outlives this exporter");
        assert!(duplicate.upgrade().is_none());
    }
}

#[cfg(test)]
mod eps_tests {
    use super::*;
    use hayro_syntax::object::Name;

    #[test]
    fn eps_tiff_preview_keeps_cmyk_inks_in_pdf() {
        let pixels = vec![0u8, 0, 0, 255, 0, 255, 255, 0];
        let mut tiff = Vec::new();
        tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut tiff))
            .unwrap()
            .write_image::<tiff::encoder::colortype::CMYK8>(2, 1, &pixels)
            .unwrap();
        let ps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 2 1\n%%EOF\n";
        let mut eps = vec![0xC5, 0xD0, 0xD3, 0xC6];
        for value in [30usize, ps.len(), 0, 0, 30 + ps.len(), tiff.len()] {
            eps.extend((value as u32).to_le_bytes());
        }
        eps.extend([0xFF, 0xFF]);
        eps.extend_from_slice(ps);
        eps.extend_from_slice(&tiff);
        assert_eq!(designcraft_images::eps_proxy(&eps).unwrap().0, tiff);
        let mut doc = Document::new(&designcraft_doc::build::NewDocument::default());
        let aid = AssetId(doc.alloc());
        doc.assets.insert(
            aid,
            Arc::new(designcraft_doc::Asset {
                id: aid,
                name: "cmyk.eps".into(),
                mime: "application/postscript".into(),
                link: None,
                data: Arc::new(eps),
                pixels: Some((2, 1)),
                page: 0,
                ..Default::default()
            }),
        );
        let mut item = Item::new(
            designcraft_doc::ItemId(doc.alloc()),
            doc.default_layer(),
            designcraft_doc::Shape::Rectangle,
            designcraft_geom::shapes::rectangle(Rect::new(72.0, 72.0, 272.0, 172.0)),
        );
        item.content = Content::Graphic(designcraft_doc::Graphic {
            layer_options: None,
            asset: aid,
            size: (2.0, 1.0),
            xf: Affine::translate((72.0, 72.0)) * Affine::scale(100.0),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            clip: None,
            wrap: Default::default(),
            fill: Default::default(),
        });
        doc.insert_item(designcraft_doc::SpreadRef::Doc(0), item, None).unwrap();
        for standard in [Standard::None, Standard::PdfX4] {
            let report = export_pdf_with_report(&doc, &Cache::new(), &PdfOptions { standard, ..Default::default() }).unwrap();
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
            let pdf = hayro_syntax::Pdf::new(report.bytes).unwrap();
            let images: Vec<_> = pdf
                .objects()
                .into_iter()
                .filter_map(|o| o.into_stream())
                .filter(|s| s.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
                .map(|s| (s.dict().get::<Name>("ColorSpace").unwrap().as_str().to_string(), s.decoded().unwrap().into_owned()))
                .collect();
            assert_eq!(images, vec![("DeviceCMYK".to_string(), pixels.clone())], "{standard:?}");
        }
    }

    #[test]
    fn civil_dates_match_the_old_cycle_conversion_including_beyond_chrono() {
        use super::civil;
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(civil(1_791_504_000 + 3_723), (2026, 10, 9, 1, 2, 3));
        // Beyond chrono's year range the old `civil` still converted; pin that output, never the
        // epoch. `date_time` is what clamps the year to the PDF `0..=9999` range afterwards.
        assert_eq!(civil(253_402_300_800), (10000, 1, 1, 0, 0, 0));
        assert_eq!(civil(10_000_000_000_000), (318857, 5, 20, 17, 46, 40));
        assert_eq!(civil(-10_000_000_000_000), (-314918, 8, 13, 6, 13, 20));
        assert_eq!(civil(i64::MAX), (292277026596, 12, 4, 15, 30, 7));
        assert_eq!(civil(i64::MIN), (-292277022657, 1, 27, 8, 29, 52));
    }
}

#[cfg(test)]
mod recompress_tests {
    use super::*;

    /// The CMYK JPEG encoder's samples read back as the same inks (Adobe's convention, which the
    /// PDF's Decode array follows).
    #[test]
    fn cmyk_jpeg_round_trips_its_inks() {
        let inks = [255u8, 0, 0, 0, 0, 0, 0, 255, 0, 128, 255, 0, 40, 40, 40, 40];
        let cmyk: Vec<u8> = (0..64).flat_map(|i| inks.chunks(4).nth(i / 16).map(<[u8]>::to_vec).unwrap_or_default()).collect();
        let r = designcraft_images::CmykRaster { width: 8, height: 8, cmyk: cmyk.clone(), alpha: None };
        let jpeg = encode_cmyk_jpeg(&r, 100).unwrap();
        assert!(designcraft_images::is_cmyk_jpeg(&jpeg));
        let back = designcraft_images::decode_cmyk_jpeg(&jpeg).unwrap();
        assert_eq!((back.width, back.height), (8, 8));
        for (a, b) in back.cmyk.iter().zip(&cmyk) {
            assert!(a.abs_diff(*b) <= 3, "{:?} vs {:?}", &back.cmyk[..16], &cmyk[..16]);
        }
        // Past JPEG's size limit: no JPEG (the caller keeps it lossless).
        let wide = designcraft_images::CmykRaster { width: 70_000, height: 1, cmyk: Vec::new(), alpha: None };
        assert!(encode_cmyk_jpeg(&wide, 80).is_none());
    }

    /// A raster past the RGBA budget is refused before `raster` decodes it to recompress (#1586).
    #[test]
    fn a_raster_past_the_budget_is_not_decoded() {
        let mode = RasterMode { interpolate: false, tag_rgb: false };
        let jpeg = Compression {
            classes: crate::ImageCompression::NONE,
            legacy_jpeg: true,
            placed: None,
            to_cmyk: None,
            rgb_only: false,
            unconverted: Default::default(),
        };
        let decodes = || RASTER_DECODES.with(std::cell::Cell::get);
        let start = decodes();
        let fmt = Some(image::ImageFormat::Png);
        assert!(raster(&Arc::new(crate::tests::blank_png(16_385)), fmt, mode, &jpeg).is_none());
        assert_eq!(decodes(), start);
        assert!(raster(&Arc::new(crate::tests::blank_png(9)), fmt, mode, &jpeg).is_some());
        assert_eq!(decodes(), start + 1);
    }
}
