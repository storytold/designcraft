//! Reduced-size JPEG decoding (1/2, 1/4, 1/8 through the DCT) for screen proxies.

/// The largest reduction [`decode_reduced`] reads straight from a JPEG's 8×8 blocks: 1/8.
pub(crate) const MAX_SHRINK: u8 = 3;

/// The largest decoded image, in bytes (the `image` crate's default allocation limit).
const MAX_BYTES: usize = 512 << 20;

/// Bytes a pixel a full decode of a progressive JPEG holds for its coefficients until the last
/// scan: two a component, up to the four components a JPEG decoder takes. 0 for a baseline JPEG
/// and for anything that isn't a JPEG.
pub(crate) fn progressive_coefficient_bytes(bytes: &[u8]) -> usize {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return 0;
    }
    let mut i = 2usize;
    while let Some(&[0xFF, marker, hi, lo]) = bytes.get(i..i.saturating_add(4)).and_then(|s| <&[u8; 4]>::try_from(s).ok()) {
        // Start of scan or end of image: no more frame headers.
        if marker == 0xDA || marker == 0xD9 {
            return 0;
        }
        // SOF2, SOF6, SOF10 and SOF14: progressive frames. The component count follows the
        // length, precision, height and width.
        if matches!(marker, 0xC2 | 0xC6 | 0xCA | 0xCE) {
            let components = bytes.get(i.saturating_add(9)).map_or(4, |&n| usize::from(n).clamp(1, 4));
            return 2 * components;
        }
        i = i.saturating_add(2).saturating_add(usize::from(u16::from_be_bytes([hi, lo])));
    }
    0
}

fn limits() -> craft_jpeg::Limits {
    let mut limits = craft_jpeg::Limits::default();
    limits.max_alloc = MAX_BYTES as u64;
    limits
}

/// Output size of a JPEG reduction without decoding pixels. `None` when the reduced decoder
/// cannot handle the JPEG, including its full-size resource limits.
pub(crate) fn reduced_estimate(bytes: &[u8], shrink: u8) -> Option<(u32, u32, usize)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) || shrink == 0 || shrink > MAX_SHRINK {
        return None;
    }
    craft_jpeg::reduced_estimate(bytes, &limits(), shrink).ok()
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
    let decoded = craft_jpeg::decode_reduced(bytes, &limits(), None, shrink).ok()?;
    let colour = match decoded.components {
        3 => true,
        1 => false,
        _ => return None,
    };
    let craft_jpeg::Samples::U8(px) = decoded.samples else {
        return None;
    };
    let pixels = usize::try_from(decoded.width).ok()?.checked_mul(usize::try_from(decoded.height).ok()?)?;
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
    Some((image::RgbaImage::from_raw(decoded.width, decoded.height, rgba)?, colour))
}
