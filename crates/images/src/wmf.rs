//! The bitmap inside a Windows Metafile (WMF), from the public [MS-WMF] specification.
//!
//! A screenshot pasted from the Windows clipboard is often a WMF that only draws one bitmap.
//! Only the bitmap records are read; vector records are skipped.

/// The key at the start of a placeable WMF (its 22-byte header comes before the metafile header).
const PLACEABLE_KEY: [u8; 4] = [0xD7, 0xCD, 0xC6, 0x9A];
const PLACEABLE_HEADER_BYTES: usize = 22;

const META_EOF: u16 = 0x0000;
const META_DIBBITBLT: u16 = 0x0940;
const META_DIBSTRETCHBLT: u16 = 0x0B41;
const META_STRETCHDIB: u16 = 0x0F43;
/// `ColorUsage` value: the colour table holds palette indices, not RGB colours.
const DIB_PAL_COLORS: u16 = 1;

/// The largest number of colour table entries a DIB can have (8 bits per pixel).
const MAX_PALETTE: u32 = 256;
const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;
const BI_JPEG: u32 = 4;
const BI_PNG: u32 = 5;
const BITMAPFILEHEADER_BYTES: usize = 14;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// Where the records start, when `b` is a WMF (placeable or not).
fn records_start(b: &[u8]) -> Option<usize> {
    let header = if b.starts_with(&PLACEABLE_KEY) { PLACEABLE_HEADER_BYTES } else { 0 };
    // META_HEADER: Type (1 memory, 2 disk), HeaderSize (9 words), Version (0x0100 or 0x0300).
    let kind = u16_at(b, header)?;
    let words = u16_at(b, header + 2)?;
    let version = u16_at(b, header + 4)?;
    if !matches!(kind, 1 | 2) || words != 9 || !matches!(version, 0x0100 | 0x0300) {
        return None;
    }
    Some(header + usize::from(words) * 2)
}

/// Whether `b` is a Windows Metafile.
pub fn is_wmf(b: &[u8]) -> bool {
    records_start(b).is_some()
}

/// The largest bitmap a WMF draws, as a file the image decoders read: a BMP, or the JPEG or PNG
/// a DIB can hold. `None` when `b` is not a WMF or draws no bitmap that can be read.
pub fn wmf_bitmap(b: &[u8]) -> Option<Vec<u8>> {
    let mut at = records_start(b)?;
    let mut best: Option<(u64, Vec<u8>)> = None;
    // Every record is at least 3 words long, so this ends after at most `b.len() / 6` records.
    while let (Some(words), Some(function)) = (u32_at(b, at), u16_at(b, at.saturating_add(4))) {
        if function == META_EOF || words < 3 {
            break;
        }
        let len = usize::try_from(words).ok().and_then(|w| w.checked_mul(2)).unwrap_or(usize::MAX);
        let end = at.saturating_add(len).min(b.len());
        let record = b.get(at..end).unwrap_or_default();
        // Parameter bytes before the DIB, after the 6-byte record header (RecordSize, RecordFunction).
        let dib_at = match function {
            META_DIBBITBLT => Some(6 + 16),
            META_DIBSTRETCHBLT => Some(6 + 20),
            META_STRETCHDIB if u16_at(record, 10) != Some(DIB_PAL_COLORS) => Some(6 + 22),
            _ => None,
        };
        if let Some(found) = dib_at.and_then(|d| record.get(d..)).and_then(dib_file)
            && best.as_ref().is_none_or(|(area, _)| found.0 > *area)
        {
            best = Some(found);
        }
        at = at.saturating_add(len);
    }
    best.map(|(_, file)| file)
}

/// A device-independent bitmap (`BITMAPINFOHEADER` or `BITMAPCOREHEADER`, colours, pixels) as a
/// file, with its pixel count: for a compressed payload, the size of the image it decodes to, not
/// the size its header claims. `None` when the header doesn't describe a bitmap, when the bytes it
/// describes are not all there, or when it is too large to decode: a bitmap that can't be drawn
/// must not beat a smaller one that can.
fn dib_file(dib: &[u8]) -> Option<(u64, Vec<u8>)> {
    let header = u32_at(dib, 0)?;
    let (width, height, bits, compression, used, entry) = if header == 12 {
        // BITMAPCOREHEADER: 16-bit sizes, 3-byte colours.
        (i32::from(u16_at(dib, 4)?), i32::from(u16_at(dib, 6)?), u16_at(dib, 10)?, BI_RGB, 0, 3)
    } else if (40..=124).contains(&header) {
        (i32_at(dib, 4)?, i32_at(dib, 8)?, u16_at(dib, 14)?, u32_at(dib, 16)?, u32_at(dib, 32)?, 4)
    } else {
        return None;
    };
    if width <= 0 || height == 0 || height == i32::MIN {
        return None;
    }
    let (wide, tall) = (width.unsigned_abs(), height.unsigned_abs());
    // A bitmap too big to decode cannot be drawn, so it must not beat a smaller one that can.
    if crate::over_rgba_budget(wide, tall) {
        return None;
    }
    let area = u64::from(wide) * u64::from(tall);
    let header_len = usize::try_from(header).ok()?;
    if matches!(compression, BI_JPEG | BI_PNG) {
        // The pixels are the whole image file: it must be there, be the kind it claims, and decode.
        let payload = dib.get(header_len..)?;
        let magic: &[u8] = if compression == BI_JPEG { &[0xFF, 0xD8, 0xFF] } else { b"\x89PNG" };
        if !payload.starts_with(magic) {
            return None;
        }
        // The header's width and height are only a recommendation for a compressed payload, so
        // rank by the image that actually decoded: a small picture in a large header is small.
        let (wide, tall) = crate::decode_rgba(payload)?.dimensions();
        return Some((u64::from(wide) * u64::from(tall), payload.to_vec()));
    }
    if !matches!(compression, BI_RGB | BI_RLE8 | BI_RLE4 | BI_BITFIELDS) {
        return None;
    }
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) {
        return None;
    }
    let colours = match (used, bits) {
        (0, 1 | 4 | 8) => 1u32 << bits,
        (n, _) if n <= MAX_PALETTE => n,
        _ => return None,
    };
    let masks = if header == 40 && compression == BI_BITFIELDS { 12 } else { 0 };
    let offset = BITMAPFILEHEADER_BYTES + header_len + masks + usize::try_from(colours).ok()? * entry;
    let pixels = dib.get(offset - BITMAPFILEHEADER_BYTES..)?;
    if matches!(compression, BI_RLE8 | BI_RLE4) {
        // A run-length bitmap is shorter than its pixel count, so only its start can be checked.
        if pixels.is_empty() {
            return None;
        }
    } else {
        // Every row is padded to 4 bytes, and a negative height is a top-down bitmap of the same size.
        let stride = u64::from(wide).checked_mul(u64::from(bits))?.checked_add(31)? / 32 * 4;
        if (pixels.len() as u64) < stride.checked_mul(u64::from(tall))? {
            return None;
        }
    }
    let size = u32::try_from(BITMAPFILEHEADER_BYTES + dib.len()).ok()?;
    let mut file = Vec::with_capacity(BITMAPFILEHEADER_BYTES + dib.len());
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&size.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
    file.extend_from_slice(dib);
    Some((area, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×1 24-bit DIB: a red pixel then a blue one.
    fn dib() -> Vec<u8> {
        let mut d = Vec::new();
        for v in [40u32, 2, 1] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0; 24]);
        // BGR rows padded to 4 bytes.
        d.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
        d
    }

    /// A placeable WMF holding one META_STRETCHDIB of `dib`, then META_EOF.
    fn wmf(dib: &[u8]) -> Vec<u8> {
        wmf_records(&[dib])
    }

    /// A placeable WMF holding one META_STRETCHDIB of each of `dibs`, then META_EOF.
    fn wmf_records(dibs: &[&[u8]]) -> Vec<u8> {
        let mut w = PLACEABLE_KEY.to_vec();
        w.extend_from_slice(&[0; 18]);
        for v in [1u16, 9, 0x0300] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        w.extend_from_slice(&[0; 12]);
        for dib in dibs {
            let params = 22 + dib.len() + dib.len() % 2;
            w.extend_from_slice(&u32::try_from((6 + params) / 2).unwrap().to_le_bytes());
            w.extend_from_slice(&META_STRETCHDIB.to_le_bytes());
            w.extend_from_slice(&0x00CC_0020u32.to_le_bytes());
            w.extend_from_slice(&0u16.to_le_bytes());
            w.extend_from_slice(&[0; 16]);
            w.extend_from_slice(dib);
            w.resize(w.len() + dib.len() % 2, 0);
        }
        w.extend_from_slice(&3u32.to_le_bytes());
        w.extend_from_slice(&META_EOF.to_le_bytes());
        w
    }

    /// A 100×100 24-bit DIB that claims its full 30,000-byte pixel buffer but holds two bytes of it.
    fn truncated_dib() -> Vec<u8> {
        let mut d = Vec::new();
        for v in [40u32, 100, 100] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0; 24]);
        d.extend_from_slice(&[0, 0]);
        d
    }

    #[test]
    fn the_bitmap_of_a_stretchdib_record_decodes() {
        let file = wmf_bitmap(&wmf(&dib())).unwrap();
        let rgba = crate::decode_rgba(&file).unwrap();
        assert_eq!((rgba.width(), rgba.height()), (2, 1));
        assert_eq!(rgba.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(rgba.get_pixel(1, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn a_truncated_bitmap_does_not_beat_a_complete_smaller_one() {
        let big = truncated_dib();
        assert!(wmf_bitmap(&wmf(&big)).is_none(), "a bitmap missing its pixel buffer is not readable");
        // The complete 2×1 bitmap is drawn after the truncated one, and still wins.
        let file = wmf_bitmap(&wmf_records(&[&big, &dib()])).unwrap();
        let rgba = crate::decode_rgba(&file).unwrap();
        assert_eq!((rgba.width(), rgba.height()), (2, 1));
    }

    /// A 2×1 PNG: a red pixel then a blue one.
    fn png_file() -> Vec<u8> {
        crate::png_from_rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap()
    }

    #[test]
    fn a_compressed_bitmap_must_hold_a_readable_image() {
        let mut jpeg = dib()[..40].to_vec();
        jpeg[16..20].copy_from_slice(&BI_JPEG.to_le_bytes());
        let mut png = jpeg.clone();
        png[16..20].copy_from_slice(&BI_PNG.to_le_bytes());
        assert!(wmf_bitmap(&wmf(&jpeg)).is_none(), "a header with no image is not readable");
        assert!(wmf_bitmap(&wmf(&png)).is_none());
        // The image must be the kind the header claims.
        jpeg.extend_from_slice(&png_file());
        assert!(wmf_bitmap(&wmf(&jpeg)).is_none());
        png.extend_from_slice(&png_file());
        assert!(wmf_bitmap(&wmf(&png)).is_some());
        // A real JPEG is readable too.
        use image::ImageEncoder;
        let mut jpeg = dib()[..40].to_vec();
        jpeg[16..20].copy_from_slice(&BI_JPEG.to_le_bytes());
        let mut file = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut file).write_image(&[255, 0, 0, 0, 0, 255], 2, 1, image::ExtendedColorType::Rgb8).unwrap();
        jpeg.extend_from_slice(&file);
        assert!(wmf_bitmap(&wmf(&jpeg)).is_some());
    }

    #[test]
    fn a_signature_is_not_an_image() {
        // A 100×100 header claiming a PNG whose payload is only the PNG signature: larger in area
        // than the complete 2×1 bitmap, but it cannot be drawn.
        let mut fake = dib()[..40].to_vec();
        fake[4..8].copy_from_slice(&100i32.to_le_bytes());
        fake[8..12].copy_from_slice(&100i32.to_le_bytes());
        fake[16..20].copy_from_slice(&BI_PNG.to_le_bytes());
        fake.extend_from_slice(b"\x89PNG");
        assert!(wmf_bitmap(&wmf(&fake)).is_none());
        let file = wmf_bitmap(&wmf_records(&[&fake, &dib()])).unwrap();
        let rgba = crate::decode_rgba(&file).unwrap();
        assert_eq!((rgba.width(), rgba.height()), (2, 1));
    }

    /// A `width`×1 24-bit DIB of black pixels.
    fn dib_row(width: u32) -> Vec<u8> {
        let mut d = Vec::new();
        for v in [40u32, width, 1] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0; 24]);
        d.resize(d.len() + (width as usize * 3).div_ceil(4) * 4, 0);
        d
    }

    #[test]
    fn a_compressed_bitmap_is_ranked_by_the_image_it_holds() {
        // A header claiming 100×100 around a readable 2×1 PNG is a 2×1 bitmap, so the complete
        // 3×1 bitmap beside it is the larger one.
        let mut fake = dib()[..40].to_vec();
        fake[4..8].copy_from_slice(&100i32.to_le_bytes());
        fake[8..12].copy_from_slice(&100i32.to_le_bytes());
        fake[16..20].copy_from_slice(&BI_PNG.to_le_bytes());
        fake.extend_from_slice(&png_file());
        let file = wmf_bitmap(&wmf_records(&[&fake, &dib_row(3)])).unwrap();
        let rgba = crate::decode_rgba(&file).unwrap();
        assert_eq!((rgba.width(), rgba.height()), (3, 1));
    }

    /// A 20,000 × 20,000 run-length DIB: a few bytes whose RGBA is 1.6 GB, over the decode budget.
    /// Run-length keeps the fixture small; the budget is checked from the header, before the
    /// compression branch.
    fn over_budget_dib() -> Vec<u8> {
        let mut d = Vec::new();
        for v in [40u32, 20_000, 20_000] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&8u16.to_le_bytes());
        d.extend_from_slice(&BI_RLE8.to_le_bytes());
        d.extend_from_slice(&[0; 20]);
        // The 256-entry colour table, then the start of the runs.
        d.extend_from_slice(&[0; 256 * 4]);
        d.extend_from_slice(&[0, 1]);
        d
    }

    #[test]
    fn a_bitmap_too_large_to_decode_does_not_beat_a_readable_one() {
        assert!(wmf_bitmap(&wmf(&over_budget_dib())).is_none(), "a bitmap that cannot be decoded is not readable");
        let file = wmf_bitmap(&wmf_records(&[&over_budget_dib(), &dib()])).unwrap();
        let rgba = crate::decode_rgba(&file).unwrap();
        assert_eq!((rgba.width(), rgba.height()), (2, 1));
    }

    #[test]
    fn hostile_metafiles_find_no_bitmap() {
        assert!(wmf_bitmap(b"").is_none());
        assert!(wmf_bitmap(&PLACEABLE_KEY).is_none());
        let mut huge = dib();
        huge[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(wmf_bitmap(&wmf(&huge)).is_none());
        let mut truncated = wmf(&dib());
        truncated.truncate(truncated.len() - 20);
        let _ = wmf_bitmap(&truncated);
        for cut in 0..wmf(&dib()).len() {
            let _ = wmf_bitmap(&wmf(&dib())[..cut]);
        }
    }
}
