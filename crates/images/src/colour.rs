//! Display colour of placed rasters. Pixels with an embedded ICC profile are converted to sRGB;
//! CMYK pixels go through their own profile, else the working CMYK space. Every raster decode
//! path (JPEG, PNG, TIFF, WebP, PSD) converts here, once, when the image is decoded.
//!
//! Colour management is craft-cms's ([`Profile`], [`Transform`], [`TransformOptions`],
//! [`Intent`], [`Builtin`]); black point compensation is its, as set in the colour settings.
//!
//! Profiles are untrusted input: one that is too large, can't be parsed, doesn't match the
//! pixels or yields no transform is ignored with a log line, and the pixels are shown as before
//! (RGB and grey as sRGB, CMYK through the working space).

use std::sync::Arc;

use designcraft_color::{Color, cms::Cms};
use image::RgbaImage;

pub use craft_cms::{Builtin, ColorSpace, Intent, Profile, Transform, TransformOptions};

use crate::CmykRaster;

/// Embedded profiles larger than this are ignored.
pub const MAX_ICC_BYTES: usize = 16 << 20;

/// Pixels per conversion call when converting in place (each chunk is copied once).
const CHUNK: usize = 1 << 16;

/// Decoded 8-bit pixels and what their values mean.
pub enum Pixels {
    /// RGB with straight alpha; converted in place.
    Rgb(RgbaImage),
    /// Grey, each pixel `[g, g, g, a]`; converted in place.
    Gray(RgbaImage),
    /// Ink values (0 = no ink).
    Cmyk(CmykRaster),
}

/// How images are shown: the CMYK profile for CMYK pixels without a usable profile of their own
/// (`None`: the profile-free formula, for Device CMYK), and the transform options.
#[derive(Clone, Debug)]
pub struct DisplayColour {
    working_cmyk: Option<Arc<Profile>>,
    options: TransformOptions,
}

impl DisplayColour {
    pub fn new(working_cmyk: Option<Arc<Profile>>, options: TransformOptions) -> Self {
        Self { working_cmyk, options }
    }

    /// The active colour settings (Edit → Color Settings): working CMYK space, intent and black
    /// point compensation.
    pub fn active() -> Self {
        Self::of(&designcraft_color::cms::active())
    }

    pub fn of(cms: &Cms) -> Self {
        let s = cms.settings();
        Self::new(cms.cmyk_icc_profile(), TransformOptions { intent: s.intent.into(), bpc: s.bpc, ..TransformOptions::default() })
    }
}

/// Straight RGBA8 in display sRGB. `None` only when a CMYK raster's buffers don't match its size
/// or the output can't be allocated.
pub fn to_display_rgba(px: Pixels, embedded_icc: Option<&[u8]>, display: &DisplayColour) -> Option<RgbaImage> {
    let own = |space| embedded_icc.and_then(|icc| to_srgb(&parse(icc, space)?, display.options));
    match px {
        Pixels::Rgb(mut img) => {
            if let Some(x) = own(ColorSpace::Rgb)
                && !keeps_rgb(&x)
            {
                in_place(&mut img, &x);
            }
            Some(img)
        }
        Pixels::Gray(mut img) => {
            if let Some(x) = own(ColorSpace::Gray)
                && !keeps_gray(&x)
            {
                in_place(&mut img, &x);
            }
            Some(img)
        }
        Pixels::Cmyk(raster) => {
            let xf = own(ColorSpace::Cmyk).or_else(|| to_srgb(display.working_cmyk.as_deref()?, display.options));
            cmyk(&raster, xf.as_deref())
        }
    }
}

fn parse(icc: &[u8], pixels: ColorSpace) -> Option<Profile> {
    if icc.len() > MAX_ICC_BYTES {
        log::warn!("embedded ICC profile of {} bytes is too large: ignored", icc.len());
        return None;
    }
    match Profile::parse(icc) {
        Ok(p) if p.color_space == pixels => Some(p),
        Ok(p) => {
            log::warn!("embedded ICC profile is for {:?}, the pixels are {pixels:?}: ignored", p.color_space);
            None
        }
        Err(e) => {
            log::warn!("embedded ICC profile can't be read ({e}): ignored");
            None
        }
    }
}

/// A transform from `src` to display sRGB, reusing one built earlier for the same profiles and
/// options (CMYK transforms build a large table).
fn to_srgb(src: &Profile, options: TransformOptions) -> Option<Arc<Transform>> {
    match craft_cms::cached(src, Builtin::Srgb.profile(), options) {
        Ok(x) if x.inputs() == src.channels() && x.outputs() == 3 => Some(x),
        Ok(_) => {
            log::warn!("image ICC profile to sRGB: unexpected channel counts; profile ignored");
            None
        }
        Err(e) => {
            log::warn!("image ICC profile to sRGB: {e}; profile ignored");
            None
        }
    }
}

/// Convert interleaved pixels (`src_stride` / `dst_stride` values per pixel, colour channels
/// first; the rest of `dst` is left alone). `false` when the buffers don't fit the transform.
fn convert(x: &Transform, src: &[u8], src_stride: usize, dst: &mut [u8], dst_stride: usize) -> bool {
    match x.convert_u8(src, src_stride, dst, dst_stride, false) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("image ICC conversion: {e}; profile ignored");
            false
        }
    }
}

/// Whether an RGB transform leaves every value within 1 of where it was (an sRGB profile): the
/// pixels are then kept exactly as decoded.
fn keeps_rgb(x: &Transform) -> bool {
    const V: [u8; 9] = [0, 32, 64, 96, 128, 160, 192, 224, 255];
    let src: Vec<u8> = V.iter().flat_map(|&r| V.iter().flat_map(move |&g| V.iter().flat_map(move |&b| [r, g, b]))).collect();
    let mut dst = vec![0u8; src.len()];
    convert(x, &src, 3, &mut dst, 3) && src.iter().zip(&dst).all(|(a, b)| a.abs_diff(*b) <= 1)
}

fn keeps_gray(x: &Transform) -> bool {
    let src: Vec<u8> = (0..=255).collect();
    let mut dst = vec![0u8; src.len() * 3];
    convert(x, &src, 1, &mut dst, 3) && src.iter().zip(dst.as_chunks::<3>().0).all(|(g, rgb)| rgb.iter().all(|v| v.abs_diff(*g) <= 1))
}

/// Convert RGBA pixels whose first channels (RGB, or grey for a grey transform) hold the source
/// values; alpha is kept.
fn in_place(img: &mut RgbaImage, x: &Transform) {
    let mut src = Vec::with_capacity(CHUNK * 4);
    for chunk in img.chunks_mut(CHUNK * 4) {
        src.clear();
        src.extend_from_slice(chunk);
        if !convert(x, &src, 4, chunk, 4) {
            return;
        }
    }
}

/// CMYK inks → RGBA through `x`, or the profile-free formula without one.
fn cmyk(r: &CmykRaster, x: Option<&Transform>) -> Option<RgbaImage> {
    let pixels = usize::try_from(r.width).ok()?.checked_mul(usize::try_from(r.height).ok()?)?;
    let len = pixels.checked_mul(4)?;
    if r.cmyk.len() != len || r.alpha.as_ref().is_some_and(|a| a.len() != pixels) {
        return None;
    }
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(len).ok()?;
    rgba.resize(len, 255);

    let converted = x.is_some_and(|x| convert(x, &r.cmyk, 4, &mut rgba, 4));
    if !converted {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let ink = |v: u8| f32::from(v) / 255.0;
        for (p, &[c, m, y, k]) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(r.cmyk.as_chunks::<4>().0) {
            let [rr, g, b] = Color::cmyk(ink(c), ink(m), ink(y), ink(k)).to_rgb_uncalibrated();
            p[..3].copy_from_slice(&[q(rr), q(g), q(b)]);
        }
    }
    if let Some(alpha) = &r.alpha {
        for (p, &a) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(alpha) {
            p[3] = a;
        }
    }
    RgbaImage::from_raw(r.width, r.height, rgba)
}

#[cfg(test)]
mod tests;
