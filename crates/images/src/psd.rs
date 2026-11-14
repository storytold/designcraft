//! Photoshop's merged composite, decoded by the shared PSD implementation and colour managed by
//! DesignCraft for its active display profile.

use craft_psd::header::ColorMode;

use crate::CmykRaster;
use crate::colour::{DisplayColour, Pixels, to_display_rgba};

/// The largest decoded raster in RGBA bytes.
const MAX_RGBA_BYTES: u64 = 512 << 20;

pub(crate) fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    let (file, _) = craft_psd::PsdFile::from_bytes_salvaging(bytes).ok()?;
    let header = &file.header;
    let pixels = usize::try_from(header.width).ok()?.checked_mul(usize::try_from(header.height).ok()?)?;
    if u64::try_from(pixels).ok()?.checked_mul(4)? > MAX_RGBA_BYTES {
        return None;
    }
    if header.color_mode == ColorMode::Cmyk {
        let raster = cmyk(&file, pixels)?;
        return to_display_rgba(Pixels::Cmyk(raster), file.icc_profile(), &DisplayColour::active());
    }
    let composite = file.composite_rgba8().ok()?;
    let rgba = image::RgbaImage::from_raw(composite.width, composite.height, composite.data)?;
    match (header.color_mode, file.icc_profile()) {
        (ColorMode::Grayscale | ColorMode::Duotone, Some(icc)) => to_display_rgba(Pixels::Gray(rgba), Some(icc), &DisplayColour::active()),
        (ColorMode::Rgb, Some(icc)) => to_display_rgba(Pixels::Rgb(rgba), Some(icc), &DisplayColour::active()),
        _ => Some(rgba),
    }
}

fn cmyk(file: &craft_psd::PsdFile, pixels: usize) -> Option<CmykRaster> {
    let header = &file.header;
    if !matches!(header.depth, 8 | 16) || header.channels < 4 {
        return None;
    }
    let plane = header.row_bytes().checked_mul(usize::try_from(header.height).ok()?)?;
    let decoded = file.decode_merged().ok()?;
    let needed = plane.checked_mul(4)?;
    if decoded.len() < needed {
        return None;
    }
    let alpha_plane = if file.merged_has_alpha() { Some(decoded.get(needed..needed.checked_add(plane)?)?) } else { None };
    let sample = |channel: usize, pixel: usize| -> Option<u8> {
        let bytes_per_sample = usize::from(header.depth / 8);
        let at = channel.checked_mul(plane)?.checked_add(pixel.checked_mul(bytes_per_sample)?)?;
        let value = match header.depth {
            8 => *decoded.get(at)?,
            16 => {
                let v = u16::from_be_bytes(decoded.get(at..at.checked_add(2)?)?.try_into().ok()?);
                ((u32::from(v) * 255 + 32_767) / 65_535) as u8
            }
            _ => return None,
        };
        Some(255 - value)
    };
    let mut inks = Vec::new();
    inks.try_reserve_exact(pixels.checked_mul(4)?).ok()?;
    let mut alpha = Vec::new();
    if alpha_plane.is_some() {
        alpha.try_reserve_exact(pixels).ok()?;
    }
    for p in 0..pixels {
        for channel in 0..4 {
            inks.push(sample(channel, p)?);
        }
        if let Some(plane) = alpha_plane {
            let bytes_per_sample = usize::from(header.depth / 8);
            let at = p.checked_mul(bytes_per_sample)?;
            let a = match header.depth {
                8 => *plane.get(at)?,
                16 => {
                    let v = u16::from_be_bytes(plane.get(at..at.checked_add(2)?)?.try_into().ok()?);
                    ((u32::from(v) * 255 + 32_767) / 65_535) as u8
                }
                _ => return None,
            };
            alpha.push(a);
        }
    }
    let alpha = alpha.iter().any(|&a| a < 255).then_some(alpha);
    Some(CmykRaster { width: header.width, height: header.height, cmyk: inks, alpha })
}
