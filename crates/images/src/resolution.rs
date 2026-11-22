//! Embedded raster resolution: the density a placed graphic was saved with, so placing honors
//! it instead of assuming 72 ppi (#1991: a 600 px photo saved at 600 dpi places one inch wide).
//! Delegates container resolution probes to `craft-image-codecs` (`file_resolution`, which reads
//! JPEG Photoshop ResolutionInfo -> XMP -> EXIF -> JFIF, PNG eXIf, WebP EXIF, and classic TIFF;
//! plus `exif_resolution`, `png_probe::pixels_per_meter`, and `bmp_probe::pixels_per_meter`) and
//! PSD `ResolutionInfo` to [`crate::psd`]. JPEG EXIF orientation uses the shared strict APP1
//! reader (`jpeg_probe::strict_exif_orientation`: `"Exif\0\0"` with complete segment bounds,
//! before the first scan). GIF and files without (usable) metadata report nothing and keep the
//! pixel-is-a-point size.

/// Dots per inch in x and y, when the file carries usable resolution metadata.
pub fn raster_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    if bytes.len() < 8 {
        return None;
    }
    let dpi = if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        && let Some(dpi) = png_resolution(bytes)
    {
        Some(dpi)
    } else if bytes.starts_with(b"BM") {
        bmp_resolution(bytes)
    } else if crate::is_psd(bytes) {
        crate::psd::resolution(bytes)
    } else {
        craft_image_codecs::resolution_probe::file_resolution(bytes).map(|(x, y)| (f64::from(x), f64::from(y)))
    }?;
    Some(if (5..=8).contains(&orientation(bytes)) { (dpi.1, dpi.0) } else { dpi })
}

/// The EXIF/TIFF orientation (1–8) of `bytes`, or 1 when absent or unreadable.
fn orientation(bytes: &[u8]) -> u16 {
    if bytes.starts_with(b"\xFF\xD8\xFF") {
        return craft_image_codecs::jpeg_probe::strict_exif_orientation(bytes);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return craft_image_codecs::png_probe::header(bytes).map_or(1, |(_, _, o)| o);
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return craft_image_codecs::webp_probe::header(bytes).map_or(1, |(_, _, o)| o);
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return craft_image_codecs::tiff_orientation(bytes);
    }
    1
}

/// PNG `pHYs` pixels-per-metre via the shared probe: meters -> dpi in `f64` (preserving exact
/// placement DPI without `dpi_of_ppm` snapping).
fn png_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    let (x, y) = craft_image_codecs::png_probe::pixels_per_meter(bytes)?;
    Some((f64::from(x) * 0.0254, f64::from(y) * 0.0254))
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
