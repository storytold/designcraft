//! Embedded raster resolution: the density a placed graphic was saved with, so placing honors
//! it instead of assuming 72 ppi (#1991: a 600 px photo saved at 600 dpi places one inch wide).
//! Delegates container resolution probes to `craft-image-codecs` (`file_resolution`, which reads
//! JPEG Photoshop ResolutionInfo -> XMP -> EXIF -> JFIF, PNG eXIf, WebP EXIF, and classic TIFF;
//! plus `exif_resolution` and `bmp_probe::pixels_per_meter`) and PSD `ResolutionInfo` to
//! [`crate::psd`]. PNG `pHYs` is read locally in `f64` before falling back to `file_resolution`
//! for `eXIf`. GIF and files without (usable) metadata report nothing and keep the
//! pixel-is-a-point size.

/// Dots per inch in x and y, when the file carries usable resolution metadata.
pub fn raster_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    if bytes.len() < 8 {
        return None;
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        && let Some(dpi) = png_resolution(bytes)
    {
        return Some(dpi);
    }
    if bytes.starts_with(b"BM") {
        return bmp_resolution(bytes);
    }
    if crate::is_psd(bytes) {
        return crate::psd::resolution(bytes);
    }
    craft_image_codecs::resolution_probe::file_resolution(bytes).map(|(x, y)| (f64::from(x), f64::from(y)))
}

/// The pHYs chunk: pixels-per-unit in x and y plus the unit (1 = meter -> dpi, in f64).
fn png_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    let mut pos = 8;
    loop {
        let len = u32::from_be_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?) as usize;
        let kind = bytes.get(pos + 4..pos + 8)?;
        if kind == b"pHYs" {
            let data = bytes.get(pos + 8..pos + 8 + 9)?;
            let (x, y) = (u32::from_be_bytes(data[0..4].try_into().ok()?), u32::from_be_bytes(data[4..8].try_into().ok()?));
            if data[8] == 1 && x > 0 && y > 0 {
                return Some((x as f64 * 0.0254, y as f64 * 0.0254));
            }
            return None;
        }
        if kind == b"IEND" || kind == b"IDAT" {
            return None;
        }
        pos = pos.checked_add(8)?.checked_add(len)?.checked_add(4)?;
        if pos >= bytes.len() {
            return None;
        }
    }
}

/// A parsed density is usable when both axes are finite, positive and below the absurdity cap
/// (a corrupt or hostile value falls back to 72 ppi at the call site).
pub fn sane_resolution(dpi: (f64, f64)) -> bool {
    let (dx, dy) = dpi;
    dx.is_finite() && dy.is_finite() && dx >= 1.0 && dy >= 1.0 && dx <= 100_000.0 && dy <= 100_000.0
}

/// Classic TIFF resolution tags of the first IFD: XResolution/YResolution rationals scaled by
/// ResolutionUnit (2 = inch, 3 = cm; a missing unit defaults to inch per the spec).
#[cfg(test)]
pub fn tiff_resolution(tiff: &[u8]) -> Option<(f64, f64)> {
    craft_image_codecs::resolution::exif_resolution(tiff).map(|(x, y)| (f64::from(x), f64::from(y)))
}

// ---------- BMP ----------

/// BITMAPINFOHEADER pixels-per-meter (offset 38/42): meters → dpi.
fn bmp_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    let (x, y) = craft_image_codecs::bmp_probe::pixels_per_meter(bytes)?;
    Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254))
}
