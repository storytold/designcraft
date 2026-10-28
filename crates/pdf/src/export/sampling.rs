//! Export PDF › Compression: placed raster images resampled to the resolution they are printed
//! at, cropped to their frames and encoded per image class, and Output › Color Conversion
//! applied to their pixels.
//!
//! Each placement is handled on its own, because its resolution depends on how large it is
//! placed: the effective resolution is the image's pixels over the inches its placement covers
//! on the page. A converted image is cached by asset, crop, size and conversion, so an image
//! placed several times at the same size is written once, and its pixels are decoded and scanned
//! only when a placement needs a variant not made yet.

use super::images::{RasterMode, embedded_icc, is_grey, is_grey_icc, jpeg};
use super::*;
use crate::{Downsample, ImageCompression, ImageOptions, ImageSampling};

/// Pixels decoded for resampling.
#[derive(Clone)]
pub(crate) enum Raster {
    /// RGBA with the file's own RGB profile, if any.
    Rgb { rgba: image::RgbaImage, icc: Option<Arc<Vec<u8>>> },
    /// Grey with alpha, with the file's own grey profile, if any.
    Grey { la: image::GrayAlphaImage, icc: Option<Arc<Vec<u8>>> },
    /// Ink values, written unchanged as DeviceCMYK.
    Cmyk(designcraft_images::CmykRaster),
}

/// The image classes Compression sets options for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Color,
    Gray,
    Mono,
}

impl Class {
    pub(crate) fn sampling(self, io: &ImageOptions) -> ImageSampling {
        match self {
            Class::Color => io.color,
            Class::Gray => io.gray,
            Class::Mono => io.mono,
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Images decoded for resampling on this thread (tests count them).
    pub(crate) static DECODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Largest number of pixels decoded for resampling (as the renderer's caps, an image this large
/// would already have failed to place): 400 megapixels.
const MAX_PIXELS: u64 = 400_000_000;

impl Raster {
    /// Decode `data`. A CMYK TIFF keeps its inks unless `rgb_only` (PDF/A); a grey file stays
    /// grey. `None` when it can't be decoded or is too large.
    pub(crate) fn decode(data: &[u8], rgb_only: bool) -> Option<Raster> {
        #[cfg(test)]
        DECODES.with(|d| d.set(d.get() + 1));
        let (w, h) = designcraft_images::pixel_size(data)?;
        if u64::from(w).saturating_mul(u64::from(h)) > MAX_PIXELS || w == 0 || h == 0 {
            return None;
        }
        if !rgb_only
            && designcraft_images::is_cmyk_tiff(data)
            && let Some(r) = designcraft_images::decode_cmyk_tiff(data)
        {
            return Some(Raster::Cmyk(r));
        }
        if is_grey(data) {
            let la = image::load_from_memory(data).ok()?.to_luma_alpha8();
            let icc = embedded_icc(data).filter(|p| is_grey_icc(p)).map(Arc::new);
            return Some(Raster::Grey { la, icc });
        }
        let rgba = designcraft_images::decode_rgba(data)?;
        let icc = embedded_icc(data).filter(|p| p.get(16..20) == Some(b"RGB ".as_slice())).map(Arc::new);
        Some(Raster::Rgb { rgba, icc })
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        match self {
            Raster::Rgb { rgba, .. } => rgba.dimensions(),
            Raster::Grey { la, .. } => la.dimensions(),
            Raster::Cmyk(r) => (r.width, r.height),
        }
    }

    /// Colour, grayscale, or black and white (a grey image whose pixels are all fully black or
    /// fully white).
    pub(crate) fn class(&self) -> Class {
        match self {
            Raster::Grey { la, .. } if la.pixels().all(|p| p.0[0] == 0 || p.0[0] == 255) => Class::Mono,
            Raster::Grey { .. } => Class::Gray,
            _ => Class::Color,
        }
    }

    fn has_alpha(&self) -> bool {
        match self {
            Raster::Rgb { rgba, .. } => rgba.pixels().any(|p| p.0[3] < 255),
            Raster::Grey { la, .. } => la.pixels().any(|p| p.0[1] < 255),
            Raster::Cmyk(r) => r.alpha.as_ref().is_some_and(|a| a.iter().any(|v| *v < 255)),
        }
    }

    /// The pixels inside `[x0, y0, x1, y1)` (clamped to the image).
    pub(crate) fn crop(self, r: [u32; 4]) -> Option<Raster> {
        let (w, h) = self.size();
        let [x0, y0, x1, y1] = [r[0].min(w), r[1].min(h), r[2].min(w), r[3].min(h)];
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        if [x0, y0, x1, y1] == [0, 0, w, h] {
            return Some(self);
        }
        let (cw, ch) = (x1 - x0, y1 - y0);
        Some(match self {
            Raster::Rgb { rgba, icc } => Raster::Rgb { rgba: image::imageops::crop_imm(&rgba, x0, y0, cw, ch).to_image(), icc },
            Raster::Grey { la, icc } => Raster::Grey { la: image::imageops::crop_imm(&la, x0, y0, cw, ch).to_image(), icc },
            Raster::Cmyk(r) => {
                let row = |plane: &[u8], n: usize| -> Option<Vec<u8>> {
                    let mut out = Vec::with_capacity((cw as usize).checked_mul(ch as usize)?.checked_mul(n)?);
                    for y in y0..y1 {
                        let start = (y as usize).checked_mul(w as usize)?.checked_add(x0 as usize)?.checked_mul(n)?;
                        out.extend_from_slice(plane.get(start..start.checked_add((cw as usize).checked_mul(n)?)?)?);
                    }
                    Some(out)
                };
                let cmyk = row(&r.cmyk, 4)?;
                let alpha = match &r.alpha {
                    Some(a) => Some(row(a, 1)?),
                    None => None,
                };
                Raster::Cmyk(designcraft_images::CmykRaster { width: cw, height: ch, cmyk, alpha })
            }
        })
    }

    /// Resampled to `w` × `h` with `method`. Colour is filtered premultiplied by alpha, so
    /// transparent pixels don't bleed their colour into the edges.
    pub(crate) fn resample(self, w: u32, h: u32, method: Downsample) -> Option<Raster> {
        let (sw, sh) = self.size();
        if (w, h) == (sw, sh) || method == Downsample::None {
            return Some(self);
        }
        let (w, h) = (w.max(1), h.max(1));
        Some(match self {
            Raster::Rgb { rgba, icc } => {
                let px = resample(rgba.as_raw(), sw, sh, 4, w, h, method, true)?;
                Raster::Rgb { rgba: image::RgbaImage::from_raw(w, h, px)?, icc }
            }
            Raster::Grey { la, icc } => {
                let px = resample(la.as_raw(), sw, sh, 2, w, h, method, true)?;
                Raster::Grey { la: image::GrayAlphaImage::from_raw(w, h, px)?, icc }
            }
            Raster::Cmyk(r) => {
                let cmyk = resample(&r.cmyk, sw, sh, 4, w, h, method, false)?;
                let alpha = match &r.alpha {
                    Some(a) => Some(resample(a, sw, sh, 1, w, h, method, false)?),
                    None => None,
                };
                Raster::Cmyk(designcraft_images::CmykRaster { width: w, height: h, cmyk, alpha })
            }
        })
    }

    /// Black-and-white again after resampling softened its edges.
    fn threshold(self) -> Raster {
        match self {
            Raster::Grey { mut la, icc } => {
                for p in la.pixels_mut() {
                    p.0[0] = if p.0[0] >= 128 { 255 } else { 0 };
                }
                Raster::Grey { la, icc }
            }
            other => other,
        }
    }
}

/// Whether an image has at most 256 distinct colours (flat art JPEG would smear): checks every
/// pixel, stopping at the 257th colour.
fn few_colors(r: &Raster) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut add = |v: u32| {
        seen.insert(v);
        seen.len() <= 256
    };
    match r {
        Raster::Rgb { rgba, .. } => rgba.pixels().all(|p| add(u32::from_le_bytes([p.0[0], p.0[1], p.0[2], 0]))),
        Raster::Grey { la, .. } => la.pixels().all(|p| add(u32::from(p.0[0]))),
        Raster::Cmyk(r) => r.cmyk.as_chunks::<4>().0.iter().all(|c| add(u32::from_le_bytes(*c))),
    }
}

/// The encoding a raster gets: JPEG at `quality` or lossless. Black-and-white, CMYK (the JPEG
/// encoder writes no CMYK) and images with transparency (JPEG has no alpha) are always lossless.
pub(crate) fn encode_jpeg(r: &Raster, class: Class, s: &ImageSampling) -> Option<u8> {
    if class == Class::Mono || matches!(r, Raster::Cmyk(_)) || r.has_alpha() {
        return None;
    }
    match s.compression {
        ImageCompression::Zip => None,
        ImageCompression::Jpeg => Some(s.quality.level()),
        ImageCompression::Automatic => (!few_colors(r)).then(|| s.quality.level()),
    }
}

/// The raster as a PDF image: JPEG at `jpeg_quality`, else lossless. RGB keeps its own profile;
/// with `mode.tag_rgb` (PDF/X) RGB without one gets sRGB.
pub(crate) fn to_image(r: Raster, jpeg_quality: Option<u8>, mode: RasterMode) -> Option<Image> {
    let (w, h) = r.size();
    if let Some(q) = jpeg_quality {
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, q.clamp(1, 100));
        let icc = match &r {
            Raster::Rgb { rgba, icc } => {
                let rgb: Vec<u8> = rgba.pixels().flat_map(|p| [p.0[0], p.0[1], p.0[2]]).collect();
                image::ImageEncoder::write_image(enc, &rgb, w, h, image::ExtendedColorType::Rgb8).ok()?;
                icc.clone()
            }
            Raster::Grey { la, icc } => {
                let l: Vec<u8> = la.pixels().map(|p| p.0[0]).collect();
                image::ImageEncoder::write_image(enc, &l, w, h, image::ExtendedColorType::L8).ok()?;
                icc.clone()
            }
            Raster::Cmyk(_) => return to_image(r, None, mode),
        };
        // The encoder writes no profile: an image with its own keeps it in the PDF, as the
        // lossless path does; one without follows `jpeg` (sRGB in PDF/X, untagged otherwise).
        return match icc {
            Some(own) => Image::from_jpeg_with_icc(buf.into(), Some(own.to_vec().into()), mode.interpolate).ok(),
            None => jpeg(buf.into(), || None, mode),
        };
    }
    let i = mode.interpolate;
    match r {
        Raster::Rgb { rgba, icc } => {
            let icc = icc.or_else(|| mode.tag_rgb.then(|| Arc::new(super::SRGB_ICC.to_vec())));
            Image::from_custom(TaggedRgb::new(&rgba, icc)?, i).ok()
        }
        Raster::Grey { la, icc } => {
            let pixels = (w as usize).checked_mul(h as usize)?;
            let (mut grey, mut alpha) = (Vec::with_capacity(pixels), Vec::with_capacity(pixels));
            for p in la.pixels() {
                grey.push(p.0[0]);
                alpha.push(p.0[1]);
            }
            let alpha = alpha.iter().any(|a| *a < 255).then(|| Arc::new(alpha));
            Image::from_custom(GreyImage { grey: Arc::new(grey), alpha, size: (w, h), icc }, i).ok()
        }
        Raster::Cmyk(c) => {
            // krilla panics when the channel lengths don't match the size.
            let pixels = (c.width as usize).checked_mul(c.height as usize)?;
            if c.cmyk.len() != pixels.checked_mul(4)? || c.alpha.as_ref().is_some_and(|a| a.len() != pixels) {
                return None;
            }
            Image::from_custom(CmykImage(Arc::new(c)), i).ok()
        }
    }
}

/// An RGB raster with its own profile (or none), written losslessly.
#[derive(Clone, Hash)]
pub(crate) struct TaggedRgb {
    rgb: Arc<Vec<u8>>,
    alpha: Option<Arc<Vec<u8>>>,
    size: (u32, u32),
    icc: Option<Arc<Vec<u8>>>,
}

impl TaggedRgb {
    fn new(rgba: &image::RgbaImage, icc: Option<Arc<Vec<u8>>>) -> Option<Self> {
        let size = rgba.dimensions();
        let pixels = (size.0 as usize).checked_mul(size.1 as usize)?;
        if pixels == 0 {
            return None;
        }
        let mut rgb = Vec::with_capacity(pixels.checked_mul(3)?);
        let mut alpha = Vec::with_capacity(pixels);
        for p in rgba.pixels() {
            rgb.extend_from_slice(&p.0[..3]);
            alpha.push(p.0[3]);
        }
        let alpha = alpha.iter().any(|a| *a < 255).then(|| Arc::new(alpha));
        Some(Self { rgb: Arc::new(rgb), alpha, size, icc })
    }
}

impl krilla::image::CustomImage for TaggedRgb {
    fn color_channel(&self) -> &[u8] {
        &self.rgb
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        self.alpha.as_deref().map(Vec::as_slice)
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        self.icc.as_deref().map(Vec::as_slice)
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Rgb
    }
}

/// The resolution of a placement: pixels per inch along the image's x and y axes, given the
/// image's pixel size, the graphic's size in graphic space and the transform from graphic space
/// to the page (in points). `None` for a degenerate placement.
pub(crate) fn effective_ppi(pixels: (u32, u32), size: (f64, f64), to_page: Affine) -> Option<(f64, f64)> {
    let c = to_page.as_coeffs();
    let (pw, ph) = (f64::from(pixels.0), f64::from(pixels.1));
    if pw <= 0.0 || ph <= 0.0 || !(size.0 > 0.0 && size.1 > 0.0) {
        return None;
    }
    // One pixel's step along each image axis, on the page.
    let step_x = (size.0 / pw) * c[0].hypot(c[1]);
    let step_y = (size.1 / ph) * c[2].hypot(c[3]);
    let ppi = |step: f64| (step.is_finite() && step > 1e-9).then(|| 72.0 / step);
    Some((ppi(step_x)?, ppi(step_y)?))
}

/// The pixel size an image of `pixels` at `ppi` gets with `s`: each axis above the threshold is
/// brought down to the target resolution; nothing is ever enlarged.
pub(crate) fn target_size(pixels: (u32, u32), ppi: (f64, f64), s: &ImageSampling) -> (u32, u32) {
    if s.downsample == Downsample::None || s.ppi.is_nan() || s.ppi <= 0.0 {
        return pixels;
    }
    let above = s.above_ppi.max(s.ppi);
    let axis = |n: u32, eff: f64| -> u32 {
        // NaN counts as not above the threshold.
        if eff.is_nan() || eff <= above {
            return n;
        }
        let scaled = (f64::from(n) * s.ppi / eff).round();
        if scaled.is_finite() { (scaled as u32).clamp(1, n) } else { n }
    };
    (axis(pixels.0, ppi.0), axis(pixels.1, ppi.1))
}

/// The pixel rectangle `[x0, y0, x1, y1)` of a `pixels`-sized image of graphic size `size` that
/// covers `visible` (graphic space), padded by a pixel so resampling has its neighbours.
/// `None` when nothing of the image is visible.
pub(crate) fn crop_rect(pixels: (u32, u32), size: (f64, f64), visible: Rect) -> Option<[u32; 4]> {
    let (pw, ph) = (f64::from(pixels.0), f64::from(pixels.1));
    if !(size.0 > 0.0 && size.1 > 0.0) {
        return None;
    }
    let sx = pw / size.0;
    let sy = ph / size.1;
    let px = |v: f64, max: f64| if v.is_finite() { v.clamp(0.0, max) } else { 0.0 };
    let x0 = px((visible.x0 * sx).floor() - 1.0, pw);
    let y0 = px((visible.y0 * sy).floor() - 1.0, ph);
    let x1 = px((visible.x1 * sx).ceil() + 1.0, pw);
    let y1 = px((visible.y1 * sy).ceil() + 1.0, ph);
    (x1 > x0 && y1 > y0).then_some([x0 as u32, y0 as u32, x1 as u32, y1 as u32])
}

/// Number of colour components in a JPEG's frame header (3 RGB/YCbCr, 4 CMYK, 1 grey), read
/// from its first SOF marker. `None` when the file isn't a JPEG this can read.
pub(crate) fn jpeg_components(data: &[u8]) -> Option<u8> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2usize;
    // Markers until the frame header; each segment gives its own length.
    for _ in 0..1024 {
        while *data.get(i)? != 0xFF {
            i = i.checked_add(1)?;
        }
        while *data.get(i)? == 0xFF {
            i = i.checked_add(1)?;
        }
        let marker = *data.get(i)?;
        i = i.checked_add(1)?;
        if matches!(marker, 0xD8 | 0x01 | 0xD0..=0xD7) {
            continue;
        }
        let len = usize::from(u16::from_be_bytes([*data.get(i)?, *data.get(i + 1)?]));
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            return data.get(i.checked_add(7)?).copied();
        }
        if marker == 0xD9 || marker == 0xDA || len < 2 {
            return None;
        }
        i = i.checked_add(len)?;
    }
    None
}

/// Resample interleaved 8-bit pixels (`ch` channels, the last one alpha when `alpha`) from
/// `sw` × `sh` to `dw` × `dh`, separably. Average weighs source pixels by the area each new pixel
/// covers, Bicubic uses a Catmull-Rom cubic widened by the reduction, Subsample takes the centre
/// pixel.
#[allow(clippy::too_many_arguments)]
fn resample(src: &[u8], sw: u32, sh: u32, ch: usize, dw: u32, dh: u32, method: Downsample, alpha: bool) -> Option<Vec<u8>> {
    let (sw, sh, dw, dh) = (sw as usize, sh as usize, dw as usize, dh as usize);
    if src.len() != sw.checked_mul(sh)?.checked_mul(ch)? || dw == 0 || dh == 0 || ch == 0 {
        return None;
    }
    let premul = alpha && ch > 1;
    // Working copy in f32, premultiplied.
    let mut work: Vec<f32> = Vec::with_capacity(src.len());
    for px in src.chunks_exact(ch) {
        let a = if premul { f32::from(px[ch - 1]) / 255.0 } else { 1.0 };
        for (k, v) in px.iter().enumerate() {
            let v = f32::from(*v);
            work.push(if premul && k < ch - 1 { v * a } else { v });
        }
    }
    let horizontal = resample_axis(&work, sw, sh, ch, dw, true, method)?;
    let both = resample_axis(&horizontal, dw, sh, ch, dh, false, method)?;
    let mut out = Vec::with_capacity(both.len());
    for px in both.chunks_exact(ch) {
        let a = if premul { px[ch - 1].clamp(0.0, 255.0) / 255.0 } else { 1.0 };
        for (k, v) in px.iter().enumerate() {
            let v = if premul && k < ch - 1 { if a > 0.0 { v / a } else { 0.0 } } else { *v };
            out.push(v.round().clamp(0.0, 255.0) as u8);
        }
    }
    Some(out)
}

/// One pass of [`resample`]: rows (`horizontal`) or columns from `n` to `m` samples.
fn resample_axis(src: &[f32], w: usize, h: usize, ch: usize, m: usize, horizontal: bool, method: Downsample) -> Option<Vec<f32>> {
    let n = if horizontal { w } else { h };
    let lines = if horizontal { h } else { w };
    let (ow, oh) = if horizontal { (m, h) } else { (w, m) };
    let mut out = vec![0f32; ow.checked_mul(oh)?.checked_mul(ch)?];
    let scale = n as f64 / m as f64;
    // Weights for each output sample (source index, weight), shared by every line.
    let mut taps: Vec<Vec<(usize, f32)>> = Vec::with_capacity(m);
    for o in 0..m {
        let centre = (o as f64 + 0.5) * scale;
        let mut t: Vec<(usize, f32)> = Vec::new();
        match method {
            Downsample::Subsample | Downsample::None => {
                let i = (centre.floor() as usize).min(n - 1);
                t.push((i, 1.0));
            }
            Downsample::Average => {
                let (a, b) = (o as f64 * scale, (o as f64 + 1.0) * scale);
                let first = a.floor() as usize;
                let last = (b.ceil() as usize).min(n);
                for i in first..last {
                    let cover = (b.min(i as f64 + 1.0) - a.max(i as f64)).max(0.0);
                    if cover > 0.0 {
                        t.push((i, cover as f32));
                    }
                }
            }
            Downsample::Bicubic => {
                let support = 2.0 * scale.max(1.0);
                let first = ((centre - support).floor().max(0.0)) as usize;
                let last = ((centre + support).ceil() as usize).min(n);
                for i in first..last {
                    let x = ((i as f64 + 0.5) - centre) / scale.max(1.0);
                    let wgt = catmull_rom(x);
                    if wgt != 0.0 {
                        t.push((i, wgt as f32));
                    }
                }
            }
        }
        let sum: f32 = t.iter().map(|(_, w)| *w).sum();
        if sum.abs() > 1e-6 {
            for tap in &mut t {
                tap.1 /= sum;
            }
        } else {
            t = vec![(((centre.floor()) as usize).min(n - 1), 1.0)];
        }
        taps.push(t);
    }
    for line in 0..lines {
        for (o, t) in taps.iter().enumerate() {
            let dst = if horizontal { (line * ow + o) * ch } else { (o * ow + line) * ch };
            for k in 0..ch {
                let mut acc = 0f32;
                for (i, wgt) in t {
                    let s = if horizontal { (line * w + i) * ch + k } else { (i * w + line) * ch + k };
                    acc += src.get(s).copied().unwrap_or(0.0) * wgt;
                }
                if let Some(d) = out.get_mut(dst + k) {
                    *d = acc;
                }
            }
        }
    }
    Some(out)
}

/// What an image asset's pixels decide for the whole export: its class, the JPEG quality it is
/// written at (`None`: lossless) and whether Output converts it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ImageFacts {
    class: Class,
    jpeg: Option<u8>,
    converts: bool,
}

/// A resampled image: its asset, the pixels kept, the pixel size written and the encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    asset: AssetId,
    crop: [u32; 4],
    size: (u32, u32),
    /// JPEG quality, or 0 for lossless.
    jpeg: u8,
}

/// The legacy Compress Images setting as image options: everything at full resolution, JPEG
/// (quality 90) for opaque non-JPEG images when on, else lossless.
pub(crate) fn legacy_options(compress: bool) -> ImageOptions {
    let compression = if compress { ImageCompression::Jpeg } else { ImageCompression::Zip };
    let mut s = ImageSampling::keep(compression);
    s.quality = crate::JpegQuality::Maximum;
    ImageOptions { color: s, gray: s, mono: ImageSampling::keep(ImageCompression::Zip), crop_to_frames: false }
}

impl Exporter<'_> {
    /// The bytes an image asset is drawn from: its file, or an EPS file's preview. `None` (with
    /// the warning) when there is nothing to draw.
    fn image_bytes(&mut self, id: AssetId) -> Option<Arc<Vec<u8>>> {
        let asset = self.doc.assets.get(&id)?;
        if designcraft_images::is_eps(&asset.data) {
            return designcraft_images::eps_proxy(&asset.data).map(|(proxy, _)| Arc::new(proxy));
        }
        (!asset.data.is_empty()).then(|| asset.data.clone())
    }

    /// Draw a placed raster image resampled, cropped, re-encoded or colour-converted for this
    /// placement, as Compression and Output ask. `false` when the image goes in unchanged (or
    /// can't be decoded), for [`Exporter::load_image`] to write it as before.
    pub(crate) fn sampled_graphic(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic) -> bool {
        let converting = self.convert.is_some();
        if self.opts.images.is_none() && !converting {
            return false;
        }
        let io = self.opts.images.unwrap_or_else(|| legacy_options(self.opts.compress_images));
        let Some(data) = self.image_bytes(g.asset) else { return false };
        let Some(pixels) = designcraft_images::pixel_size(&data) else { return false };
        let (gw, gh) = g.size;
        // Where it sits on the page decides its resolution and what of it shows.
        let to_page = self.placement.map(|(xf, _)| xf * g.xf);
        let ppi = to_page.and_then(|t| effective_ppi(pixels, (gw, gh), t));
        let visible = match self.placement {
            Some((_, frame)) if io.crop_to_frames && g.xf.determinant().abs() > 1e-12 => {
                let r = g.xf.inverse().transform_rect_bbox(frame).intersect(Rect::new(0.0, 0.0, gw, gh));
                if r.width() <= 0.0 || r.height() <= 0.0 {
                    // Nothing of the image is inside its frame.
                    return true;
                }
                Some(r)
            }
            _ => None,
        };
        let full = [0, 0, pixels.0, pixels.1];
        let crop = match visible {
            Some(v) => match crop_rect(pixels, (gw, gh), v) {
                Some(c) => c,
                None => return true,
            },
            None => full,
        };
        let jpeg_parts = jpeg_components(&data);
        // A JPEG that needs no change goes in as it is (CMYK JPEGs always do: they can't be
        // decoded as inks here).
        if let Some(n) = jpeg_parts {
            let class = if n == 1 { Class::Gray } else { Class::Color };
            let sampling = class.sampling(&io);
            let resize =
                ppi.is_some_and(|p| target_size((crop[2] - crop[0], crop[3] - crop[1]), p, &sampling) != (crop[2] - crop[0], crop[3] - crop[1]));
            // A JPEG tagged with an RGB profile of its own is converted from it.
            let own_rgb = n == 3 && embedded_icc(&data).is_some_and(|p| p.get(16..20) == Some(b"RGB ".as_slice()));
            let changes = crop != full
                || resize
                // ZIP decodes a JPEG only when Compression asked for it, not under the legacy
                // Compress Images setting, which passes JPEGs through.
                || (self.opts.images.is_some() && sampling.compression == ImageCompression::Zip)
                || self.convert.as_ref().is_some_and(|c| c.changes_jpeg(n, own_rgb));
            if n == 4 {
                if changes {
                    let name = self.doc.assets.get(&g.asset).map(|a| a.name.clone()).unwrap_or_default();
                    self.warn(format!("{name}: a CMYK JPEG is written as placed (not resampled, cropped or converted)"));
                }
                return false;
            }
            if !changes {
                return false;
            }
        }
        // What decides how the image is written doesn't change between its placements: decode it
        // and scan its pixels once per export, and again only to make a new variant.
        let mode = RasterMode::for_standard(self.opts.standard);
        let mut decoded: Option<Raster> = None;
        let facts = match self.image_facts.get(&g.asset) {
            Some(f) => *f,
            None => {
                decoded = Raster::decode(&data, self.rgb_only);
                let f = decoded.as_ref().map(|r| {
                    let class = r.class();
                    ImageFacts {
                        class,
                        jpeg: encode_jpeg(r, class, &class.sampling(&io)),
                        converts: self.convert.as_ref().is_some_and(|c| c.changes(r)),
                    }
                });
                self.image_facts.insert(g.asset, f);
                f
            }
        };
        let Some(ImageFacts { class, jpeg, converts }) = facts else { return false };
        let sampling = class.sampling(&io);
        // The pixel size this placement writes, from the crop's resolution.
        let crop_size = (crop[2].saturating_sub(crop[0]), crop[3].saturating_sub(crop[1]));
        let size = ppi.map_or(crop_size, |p| target_size(crop_size, p, &sampling));
        let unchanged_pixels = crop == full && size == pixels && !converts;
        // Lossless and untouched: the usual path writes it (and shares it between assets).
        if unchanged_pixels && jpeg.is_none() && jpeg_parts.is_none() {
            return false;
        }
        let key = Key { asset: g.asset, crop, size, jpeg: jpeg.unwrap_or(0) };
        let img = match self.sampled.get(&key) {
            Some(img) => img.clone(),
            None => {
                let Some(raster) = decoded.or_else(|| Raster::decode(&data, self.rgb_only)) else { return false };
                let mut r = raster.crop(crop).and_then(|r| r.resample(size.0, size.1, sampling.downsample));
                if let Some(c) = &self.convert {
                    r = r.and_then(|r| c.raster(r));
                }
                if class == Class::Mono {
                    r = r.map(Raster::threshold);
                }
                let img = r.and_then(|r| to_image(r, jpeg, mode));
                self.sampled.insert(key, img.clone());
                img
            }
        };
        let Some(img) = img else { return false };
        // The kept pixels span their part of the graphic.
        let (sx, sy) = (gw / f64::from(pixels.0.max(1)), gh / f64::from(pixels.1.max(1)));
        let (x0, y0) = (f64::from(crop[0]) * sx, f64::from(crop[1]) * sy);
        let (w, h) = (f64::from(crop_size.0) * sx, f64::from(crop_size.1) * sy);
        let Some(drawn) = Size::from_wh(w.max(1e-3) as f32, h.max(1e-3) as f32) else { return true };
        s.push_transform(&tf(g.xf * Affine::translate((x0, y0))));
        s.draw_image(img, drawn);
        s.pop();
        true
    }
}

/// The Catmull-Rom cubic (B = 0, C = 0.5).
fn catmull_rom(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        1.5 * x * x * x - 2.5 * x * x + 1.0
    } else if x < 2.0 {
        -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JpegQuality;

    fn sampling(downsample: Downsample, ppi: f64, above: f64) -> ImageSampling {
        ImageSampling::resample(downsample, ppi, above, ImageCompression::Zip, JpegQuality::Maximum)
    }

    #[test]
    fn effective_ppi_follows_the_placed_size() {
        // 600 px across 72 pt (one inch) = 600 ppi; scaled to half size = 1200 ppi.
        assert_eq!(effective_ppi((600, 300), (72.0, 36.0), Affine::IDENTITY), Some((600.0, 600.0)));
        let (x, y) = effective_ppi((600, 300), (72.0, 36.0), Affine::scale(0.5)).unwrap();
        assert!((x - 1200.0).abs() < 1e-9 && (y - 1200.0).abs() < 1e-9);
        // Rotation doesn't change the resolution.
        let (x, _) = effective_ppi((600, 300), (72.0, 36.0), Affine::rotate(0.7)).unwrap();
        assert!((x - 600.0).abs() < 1e-6);
        assert_eq!(effective_ppi((600, 300), (72.0, 36.0), Affine::scale(0.0)), None);
        assert_eq!(effective_ppi((0, 300), (72.0, 36.0), Affine::IDENTITY), None);
    }

    #[test]
    fn only_images_above_the_threshold_are_reduced() {
        let s = sampling(Downsample::Bicubic, 300.0, 450.0);
        assert_eq!(target_size((600, 600), (400.0, 400.0), &s), (600, 600));
        assert_eq!(target_size((600, 600), (600.0, 600.0), &s), (300, 300));
        // Never enlarged; an axis below the threshold is kept.
        assert_eq!(target_size((600, 600), (600.0, 100.0), &s), (300, 600));
        assert_eq!(target_size((600, 600), (600.0, 600.0), &sampling(Downsample::None, 300.0, 450.0)), (600, 600));
        // A threshold below the target means the target.
        assert_eq!(target_size((600, 600), (310.0, 310.0), &sampling(Downsample::Average, 300.0, 100.0)), (581, 581));
        assert_eq!(target_size((600, 600), (f64::INFINITY, f64::NAN), &s), (1, 600));
    }

    #[test]
    fn crop_covers_the_visible_part_with_a_pixel_margin() {
        let r = crop_rect((100, 100), (50.0, 50.0), Rect::new(10.0, 10.0, 20.0, 30.0)).unwrap();
        assert_eq!(r, [19, 19, 41, 61]);
        assert_eq!(crop_rect((100, 100), (50.0, 50.0), Rect::new(-10.0, -10.0, 100.0, 100.0)).unwrap(), [0, 0, 100, 100]);
        assert_eq!(crop_rect((100, 100), (50.0, 50.0), Rect::new(60.0, 60.0, 80.0, 80.0)), None);
        assert_eq!(crop_rect((100, 100), (0.0, 50.0), Rect::new(0.0, 0.0, 1.0, 1.0)), None);
    }

    #[test]
    fn average_keeps_the_mean_and_subsample_picks_pixels() {
        // A 4×1 grey ramp averaged to 2×1.
        let src = [0u8, 255, 100, 200];
        assert_eq!(resample(&src, 4, 1, 1, 2, 1, Downsample::Average, false).unwrap(), vec![128, 150]);
        assert_eq!(resample(&src, 4, 1, 1, 2, 1, Downsample::Subsample, false).unwrap(), vec![255, 200]);
        // A flat image stays flat with every filter.
        let flat = vec![77u8; 10 * 10 * 4];
        for m in [Downsample::Average, Downsample::Bicubic, Downsample::Subsample] {
            assert!(resample(&flat, 10, 10, 4, 3, 7, m, false).unwrap().iter().all(|v| *v == 77), "{m:?}");
        }
        // A wrong buffer length is refused, not read past.
        assert_eq!(resample(&[1, 2, 3], 4, 1, 1, 2, 1, Downsample::Average, false), None);
    }

    #[test]
    fn transparent_pixels_do_not_bleed_into_colour() {
        // Red opaque next to transparent green: the average is red at half alpha.
        let src = [255u8, 0, 0, 255, 0, 255, 0, 0];
        let out = resample(&src, 2, 1, 4, 1, 1, Downsample::Average, true).unwrap();
        assert_eq!(out, vec![255, 0, 0, 128]);
    }

    #[test]
    fn cmyk_crop_and_resample_keep_ink_planes() {
        let r = Raster::Cmyk(designcraft_images::CmykRaster { width: 4, height: 2, cmyk: (0..32).map(|v| v as u8 * 8).collect(), alpha: None });
        let c = r.crop([1, 0, 3, 2]).unwrap();
        let Raster::Cmyk(c) = &c else { panic!("cmyk") };
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(&c.cmyk[..4], &[32, 40, 48, 56]);
        let small = Raster::Cmyk(c.clone()).resample(1, 1, Downsample::Average).unwrap();
        assert_eq!(small.size(), (1, 1));
    }

    #[test]
    fn classes_and_encoding_choices() {
        let bw = Raster::Grey { la: image::GrayAlphaImage::from_fn(4, 4, |x, _| image::LumaA([if x < 2 { 0 } else { 255 }, 255])), icc: None };
        assert_eq!(bw.class(), Class::Mono);
        let grey = Raster::Grey { la: image::GrayAlphaImage::from_fn(4, 4, |x, _| image::LumaA([x as u8 * 40, 255])), icc: None };
        assert_eq!(grey.class(), Class::Gray);
        let auto = ImageSampling::keep(ImageCompression::Automatic);
        // Few colours stay lossless under Automatic; a photo-like ramp becomes JPEG.
        assert_eq!(encode_jpeg(&grey, Class::Gray, &auto), None);
        let photo =
            Raster::Rgb { rgba: image::RgbaImage::from_fn(40, 40, |x, y| image::Rgba([x as u8 * 6, y as u8 * 6, (x * y) as u8, 255])), icc: None };
        assert_eq!(encode_jpeg(&photo, Class::Color, &auto), Some(JpegQuality::Maximum.level()));
        assert_eq!(encode_jpeg(&bw, Class::Mono, &ImageSampling::keep(ImageCompression::Jpeg)), None);
        assert_eq!(encode_jpeg(&photo, Class::Color, &ImageSampling::keep(ImageCompression::Zip)), None);
        let soft = Raster::Rgb { rgba: image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 100])), icc: None };
        assert_eq!(encode_jpeg(&soft, Class::Color, &ImageSampling::keep(ImageCompression::Jpeg)), None);
        assert!(to_image(photo, Some(60), RasterMode { interpolate: false, tag_rgb: true }).is_some());
        assert!(to_image(bw.threshold(), None, RasterMode { interpolate: false, tag_rgb: false }).is_some());
    }

    #[test]
    fn jpeg_component_count_from_the_frame_header() {
        let img = image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30]));
        let mut buf = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 80).encode_image(&img).unwrap();
        assert_eq!(jpeg_components(&buf), Some(3));
        assert_eq!(jpeg_components(b"not a jpeg"), None);
        // Truncated anywhere before the component count: no panic, no answer.
        let sof = buf.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        for end in 0..sof + 9 {
            assert_eq!(jpeg_components(&buf[..end]), None, "{end}");
        }
        assert_eq!(jpeg_components(&[0xFF, 0xD8, 0xFF]), None);
    }
}
