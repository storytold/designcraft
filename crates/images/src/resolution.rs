//! Embedded raster resolution: the density a placed graphic was saved with, so placing honors
//! it instead of assuming 72 ppi (#1991: a 600 px photo saved at 600 dpi places one inch wide,
//! as in InDesign). Reads JFIF/EXIF from JPEG (and EXIF from WebP), pHYs from PNG, the resolution
//! tags from TIFF, and the header density from BMP. GIF, PSD and files without (usable) metadata
//! report nothing and keep the pixel-is-a-point size.

/// Dots per inch in x and y, when the file carries usable resolution metadata.
pub fn raster_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    if bytes.len() < 8 {
        return None;
    }
    if bytes.starts_with(b"\xFF\xD8\xFF") {
        return jpeg_resolution(bytes);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png_resolution(bytes);
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        return webp_resolution(bytes);
    }
    if bytes.starts_with(b"BM") {
        return bmp_resolution(bytes);
    }
    if is_classic_tiff(bytes) {
        return tiff_resolution(bytes);
    }
    None
}

/// A parsed density is usable when both axes are finite, positive and below the absurdity cap
/// (a corrupt or hostile value falls back to 72 ppi at the call site).
pub fn sane_resolution(dpi: (f64, f64)) -> bool {
    let (dx, dy) = dpi;
    dx.is_finite() && dy.is_finite() && dx > 0.0 && dy > 0.0 && dx <= 100_000.0 && dy <= 100_000.0
}

fn is_classic_tiff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*")
}

// ---------- JPEG ----------

/// Scan JPEG markers for a JFIF density, preferring an EXIF resolution when both are present.
/// Stops at the start of scan (entropy data can contain marker-like bytes).
fn jpeg_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    let mut pos = 2;
    let mut jfif: Option<(f64, f64)> = None;
    for _ in 0..512 {
        // Markers are FF-prefixed; skip fill bytes. Running out of bytes ends the scan
        // (a truncated file keeps a JFIF density found before the truncation).
        let Some(mut m) = bytes.get(pos).copied() else { break };
        while m == 0xFF {
            pos += 1;
            let Some(b) = bytes.get(pos).copied() else { break };
            m = b;
            if m != 0xFF {
                break;
            }
        }
        if m == 0xFF {
            break;
        }
        pos += 1;
        // Standalone markers carry no payload; SOS starts entropy-coded data.
        if m == 0xD8 || m == 0xD9 || (0xD0..=0xD7).contains(&m) || m == 0x01 {
            continue;
        }
        if m == 0xDA {
            break;
        }
        let Some(&a) = bytes.get(pos) else { break };
        let Some(&b) = bytes.get(pos + 1) else { break };
        let len = u16::from_be_bytes([a, b]) as usize;
        if len < 2 || pos + len > bytes.len() {
            break;
        }
        let payload = &bytes[pos + 2..pos + len];
        if m == 0xE0 && jfif.is_none() {
            jfif = jfif_density(payload);
        } else if m == 0xE1 && let Some(dpi) = exif_resolution(payload) {
            // A non-EXIF APP1 (XMP, ICC, ...) parses to nothing: keep scanning, since a
            // later APP1 can still carry the EXIF resolution.
            return Some(dpi);
        }
        pos += len;
    }
    jfif
}

/// JFIF APP0 density: version, units (0 aspect, 1 dpi, 2 dots/cm), x/y density.
fn jfif_density(payload: &[u8]) -> Option<(f64, f64)> {
    if !payload.starts_with(b"JFIF\x00") || payload.len() < 14 {
        return None;
    }
    let units = payload[7];
    let (x, y) = (u16::from_be_bytes([payload[8], payload[9]]), u16::from_be_bytes([payload[10], payload[11]]));
    match (units, x, y) {
        (1, 1.., 1..) => Some((x as f64, y as f64)),
        (2, 1.., 1..) => Some((x as f64 * 2.54, y as f64 * 2.54)),
        _ => None,
    }
}

/// EXIF APP1 payload ("Exif\0\0" + a TIFF header): the resolution tags of IFD0.
fn exif_resolution(payload: &[u8]) -> Option<(f64, f64)> {
    let tiff = payload.strip_prefix(b"Exif\x00\x00")?;
    if !is_classic_tiff(tiff) {
        return None;
    }
    tiff_resolution(tiff)
}

// ---------- TIFF (also the body of EXIF) ----------

/// Classic TIFF resolution tags of the first IFD: XResolution/YResolution rationals scaled by
/// ResolutionUnit (2 = inch, 3 = cm; a missing unit defaults to inch per the spec).
pub fn tiff_resolution(tiff: &[u8]) -> Option<(f64, f64)> {
    let le = match &tiff[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16at = |i: usize| -> Option<u16> {
        let b: [u8; 2] = tiff.get(i..i + 2)?.try_into().ok()?;
        Some(if le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    };
    let u32at = |i: usize| -> Option<u32> {
        let b: [u8; 4] = tiff.get(i..i + 4)?.try_into().ok()?;
        Some(if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    };
    let ifd = u32at(4)? as usize;
    let count = u16at(ifd)? as usize;
    if count > 512 {
        return None;
    }
    let mut xres: Option<(u32, u32)> = None;
    let mut yres: Option<(u32, u32)> = None;
    let mut unit: Option<u16> = None;
    for n in 0..count {
        let e = ifd.checked_add(2)?.checked_add(n.checked_mul(12)?)?;
        let (tag, typ, cnt) = (u16at(e)?, u16at(e + 2)?, u32at(e + 4)?);
        // SHORT values of count 1 live inline in the value field (low bytes LE, high bytes BE).
        let short_inline = || -> Option<u16> {
            let raw = u32at(e + 8)?;
            Some(if le { (raw & 0xFFFF) as u16 } else { (raw >> 16) as u16 })
        };
        // RATIONAL values of count 1 live at the offset (relative to the TIFF header start).
        let rational_at = || -> Option<(u32, u32)> {
            if cnt != 1 {
                return None;
            }
            let off = u32at(e + 8)? as usize;
            if off.checked_add(8)? > tiff.len() {
                return None;
            }
            Some((u32at(off)?, u32at(off + 4)?))
        };
        match (tag, typ) {
            (0x011A, 5) if xres.is_none() => xres = rational_at(),
            (0x011B, 5) if yres.is_none() => yres = rational_at(),
            (0x0128, 3) if unit.is_none() => unit = short_inline(),
            _ => {}
        }
    }
    let num = |(n, d): (u32, u32)| (d != 0).then(|| n as f64 / d as f64);
    let (x, y) = (num(xres?)?, num(yres?)?);
    match unit.unwrap_or(2) {
        2 => Some((x, y)),
        3 => Some((x * 2.54, y * 2.54)),
        _ => None,
    }
}

// ---------- PNG ----------

/// The pHYs chunk: pixels-per-unit in x and y plus the unit (1 = meter → dpi).
fn png_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    let mut pos = 8;
    for _ in 0..64 {
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
        if kind == b"IEND" {
            return None;
        }
        pos = pos.checked_add(8)?.checked_add(len)?.checked_add(4)?;
        if pos >= bytes.len() {
            return None;
        }
    }
    None
}

// ---------- WebP ----------

/// A WebP EXIF chunk carries a TIFF header: reuse the TIFF resolution tags.
fn webp_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    let riff = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let end = 12usize.checked_add(riff)?.min(bytes.len());
    let mut pos = 12;
    for _ in 0..64 {
        let kind = bytes.get(pos..pos + 4)?;
        let size = u32::from_le_bytes(bytes.get(pos + 4..pos + 8)?.try_into().ok()?) as usize;
        let data = pos.checked_add(8)?;
        if data.checked_add(size)? > end {
            return None;
        }
        if kind == b"EXIF" && is_classic_tiff(bytes.get(data..data + size.min(4)).unwrap_or_default()) {
            return tiff_resolution(&bytes[data..(data + size).min(bytes.len())]);
        }
        pos = data.checked_add(size.checked_add(size & 1)?)?;
    }
    None
}

// ---------- BMP ----------

/// BITMAPINFOHEADER pixels-per-meter (offset 38/42): meters → dpi.
fn bmp_resolution(bytes: &[u8]) -> Option<(f64, f64)> {
    if bytes.len() < 46 || u32::from_le_bytes(bytes.get(14..18)?.try_into().ok()?) < 40 {
        return None;
    }
    let ppm = |i: usize| -> Option<i32> { Some(i32::from_le_bytes(bytes.get(i..i + 4)?.try_into().ok()?)) };
    let (x, y) = (ppm(38)?, ppm(42)?);
    if x > 0 && y > 0 {
        return Some((x as f64 * 0.0254, y as f64 * 0.0254));
    }
    None
}
