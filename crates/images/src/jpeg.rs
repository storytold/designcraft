//! Reduced-size JPEG decoding (1/2, 1/4, 1/8 through the DCT) for screen proxies.

use jpeg_decoder::{CodingProcess, PixelFormat};

/// The largest reduction [`decode_reduced`] reads straight from a JPEG's 8×8 blocks: 1/8.
pub(crate) const MAX_SHRINK: u8 = 3;

/// The largest decoded image, in bytes (the `image` crate's default allocation limit).
const MAX_BYTES: usize = 512 << 20;

/// Output size of a JPEG reduction without decoding pixels. `None` when the reduced decoder
/// cannot handle the JPEG, including its full-size resource limits.
pub(crate) fn reduced_estimate(bytes: &[u8], shrink: u8) -> Option<(u32, u32, usize)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) || shrink == 0 || shrink > MAX_SHRINK {
        return None;
    }
    let mut d = jpeg_decoder::Decoder::new(bytes);
    d.set_max_decoding_buffer_size(MAX_BYTES);
    d.read_info().ok()?;
    let info = d.info()?;
    let colour = match info.pixel_format {
        PixelFormat::RGB24 => true,
        PixelFormat::L8 => false,
        PixelFormat::L16 | PixelFormat::CMYK32 => return None,
    };
    if crate::over_rgba_budget(u32::from(info.width), u32::from(info.height)) {
        return None;
    }
    let progressive_bytes = if info.coding_process == CodingProcess::DctProgressive {
        let side = |n: u16| u64::from(n).next_multiple_of(32);
        let components: u64 = if colour { 3 } else { 1 };
        let bytes = side(info.width) * side(info.height) * components * 2;
        if bytes > MAX_BYTES as u64 {
            return None;
        }
        usize::try_from(bytes).ok()?
    } else {
        0
    };
    let reduced = |n: u16| u32::from(n).div_ceil(1 << shrink);
    let (width, height) = (reduced(info.width), reduced(info.height));
    let output_bytes = width as usize * height as usize * 8;
    Some((width, height, progressive_bytes.saturating_add(output_bytes)))
}

/// A YCbCr, RGB or grayscale JPEG decoded at 1/2^`shrink` of its size, each side rounded up, for
/// `shrink` 1 to [`MAX_SHRINK`]: straight RGBA and whether the image has colour. `None` for CMYK,
/// 12-bit and lossless JPEGs, other formats and anything that doesn't decode. `None` as well for a
/// JPEG whose full size is over the full decode's limit, and for a progressive JPEG whose
/// full-size coefficients take more than [`MAX_BYTES`]; the full decode applies its own limits to
/// those.
pub(crate) fn decode_reduced(bytes: &[u8], shrink: u8) -> Option<(image::RgbaImage, bool)> {
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
