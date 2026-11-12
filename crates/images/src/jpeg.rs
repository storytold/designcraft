//! CMYK and YCCK JPEGs, decoded to their ink values (the `image` crate converts them to RGB with
//! a profile-free formula).

use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::CmykRaster;

/// The largest decoded image, in bytes (the `image` crate's default allocation limit).
const MAX_BYTES: usize = 512 << 20;

fn decoder(bytes: &[u8], out: ColorSpace) -> JpegDecoder<ZCursor<&[u8]>> {
    let options = DecoderOptions::default().set_strict_mode(false).set_max_width(65_535).set_max_height(65_535).jpeg_set_out_colorspace(out);
    JpegDecoder::new_with_options(ZCursor::new(bytes), options)
}

/// A four-channel JPEG's inks (0 = no ink) and its embedded ICC profile. `None` for other JPEGs
/// and for anything that doesn't decode.
pub(crate) fn decode_cmyk(bytes: &[u8]) -> Option<(CmykRaster, Option<Vec<u8>>)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut d = decoder(bytes, ColorSpace::RGB);
    d.decode_headers().ok()?;
    let space = d.input_colorspace()?;
    if !matches!(space, ColorSpace::CMYK | ColorSpace::YCCK) {
        return None;
    }
    let info = d.info()?;
    let (w, h) = (usize::from(info.width), usize::from(info.height));
    let pixels = w.checked_mul(h)?;
    if pixels.checked_mul(4)? > MAX_BYTES {
        return None;
    }
    // Decoding into the input colour space copies the channels as stored.
    let mut d = decoder(bytes, space);
    let mut px = d.decode().ok()?;
    if px.len() != pixels.checked_mul(4)? {
        return None;
    }
    let icc = d.icc_profile();
    if space == ColorSpace::YCCK {
        // YCbCr encodes 255 − the first three stored values (JFIF conversion); K is as stored.
        for p in px.as_chunks_mut::<4>().0 {
            let (y, cb, cr) = (f32::from(p[0]), f32::from(p[1]) - 128.0, f32::from(p[2]) - 128.0);
            let byte = |v: f32| 255 - v.round().clamp(0.0, 255.0) as u8;
            p[0] = byte(y + 1.402 * cr);
            p[1] = byte(y - 0.344_136 * cb - 0.714_136 * cr);
            p[2] = byte(y + 1.772 * cb);
        }
    }
    // Adobe's APP14 marker (always there for YCCK) means the values are stored inverted:
    // 255 = no ink.
    if space == ColorSpace::YCCK || adobe_marker(bytes) {
        for v in &mut px {
            *v = 255 - *v;
        }
    }
    let raster = CmykRaster { width: u32::try_from(w).ok()?, height: u32::try_from(h).ok()?, cmyk: px, alpha: None };
    Some((raster, icc))
}

/// The largest reduction [`decode_reduced`] reads straight from a JPEG's 8×8 blocks: 1/8.
pub(crate) const MAX_SHRINK: u8 = 3;

/// A YCbCr, RGB or grayscale JPEG decoded at 1/2^`shrink` of its size, each side rounded up, for
/// `shrink` 1 to [`MAX_SHRINK`]: straight RGBA and whether the image has colour. `None` for CMYK,
/// 12-bit and lossless JPEGs, other formats and anything that doesn't decode. `None` as well for a
/// JPEG whose full size is over the full decode's limit, and for a progressive JPEG whose
/// full-size coefficients take more than [`MAX_BYTES`]; the full decode applies its own limits to
/// those.
pub(crate) fn decode_reduced(bytes: &[u8], shrink: u8) -> Option<(image::RgbaImage, bool)> {
    use jpeg_decoder::{CodingProcess, PixelFormat};
    if !bytes.starts_with(&[0xFF, 0xD8]) || shrink == 0 || shrink > MAX_SHRINK {
        return None;
    }
    let mut d = jpeg_decoder::Decoder::new(bytes);
    // This limits the output only, which is the reduced size.
    d.set_max_decoding_buffer_size(MAX_BYTES);
    d.read_info().ok()?;
    let info = d.info()?;
    if crate::over_rgba_budget(u32::from(info.width), u32::from(info.height)) {
        return None;
    }
    let colour = match info.pixel_format {
        PixelFormat::RGB24 => true,
        PixelFormat::L8 => false,
        PixelFormat::L16 | PixelFormat::CMYK32 => return None,
    };
    if info.coding_process == CodingProcess::DctProgressive {
        // A progressive JPEG holds every coefficient of the full-size image (two bytes each) until
        // its last scan, whatever the output size. Each component is counted at the full size and
        // each side is rounded up to the largest MCU (32 pixels): an upper bound.
        let side = |n: u16| u64::from(n).next_multiple_of(32);
        let components: u64 = if colour { 3 } else { 1 };
        if side(info.width) * side(info.height) * components * 2 > MAX_BYTES as u64 {
            return None;
        }
    }
    let reduced = |n: u16| u16::try_from(u32::from(n).div_ceil(1 << shrink)).ok();
    let expected = (reduced(info.width)?, reduced(info.height)?);
    // The decoder takes the smallest scale that meets either requested side, and a short side
    // rounds up to the same size at several scales, so only the long side is requested.
    let request = if info.width >= info.height { (expected.0, u16::MAX) } else { (u16::MAX, expected.1) };
    let (w, h) = d.scale(request.0, request.1).ok()?;
    if (w, h) != expected {
        return None;
    }
    let px = d.decode().ok()?;
    let pixels = usize::from(w).checked_mul(usize::from(h))?;
    let rgba: Vec<u8> = if colour {
        if px.len() != pixels.checked_mul(3)? {
            return None;
        }
        px.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect()
    } else {
        if px.len() != pixels {
            return None;
        }
        px.iter().flat_map(|&v| [v, v, v, 255]).collect()
    };
    Some((image::RgbaImage::from_raw(u32::from(w), u32::from(h), rgba)?, colour))
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
