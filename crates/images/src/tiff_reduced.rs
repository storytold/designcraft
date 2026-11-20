//! Reduced-size TIFF decoding (1/2, 1/4, 1/8) via banded streaming for screen and link proxies.

use std::collections::HashMap;

use craft_image_codecs::{ChannelLayout, CodecError, InkSet, Limits, SampleType, TiffBands};
use image::RgbaImage;

use crate::CmykRaster;
use crate::colour::{DisplayColour, Pixels, to_display_rgba};

/// Largest `shrink` [`decode_reduced`] accepts: 3, an eighth of each side.
pub(crate) const MAX_SHRINK: u8 = crate::jpeg::MAX_SHRINK;

/// Most bytes one strip or row of tiles may take while streaming: stops single-strip files
/// larger than this before any pixel buffer is allocated.
const MAX_BAND_BYTES: u64 = 512 << 20;

/// Classic (`II*\0`, `MM\0*`) or BigTIFF (`II+\0`, `MM\0+`) signature.
pub(crate) fn is_tiff(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4), Some(b"II*\0" | b"MM\0*" | b"II+\0" | b"MM\0+"))
}

fn limits() -> Limits {
    Limits { max_alloc: MAX_BAND_BYTES, ..Limits::banded() }
}

/// False when a Separated TIFF uses a non-CMYK ink set (`InkSet` != 1), matching
/// [`crate::decode_cmyk_tiff`].
fn supported_ink_set(bytes: &[u8]) -> bool {
    !crate::default_tiff_page(bytes).is_some_and(|page| page.photometric == Some(5) && page.ink_set.is_some_and(|set| set != InkSet::Cmyk))
}

/// Opens the default TIFF page for banded decoding at `1 / 2^shrink`, enforcing that both the
/// stream bands and the reduced RGBA output stay within budget.
fn open_checked(bytes: &[u8], shrink: u8) -> Option<(TiffBands<'_>, u32, u32)> {
    if !is_tiff(bytes) || !(1..=MAX_SHRINK).contains(&shrink) || !supported_ink_set(bytes) {
        return None;
    }
    let (pw, ph) = crate::pixel_size(bytes)?;
    let bands = TiffBands::open(bytes, None, &limits()).ok()?.upright();
    if bands.dimensions() != (pw, ph) {
        return None;
    }
    let step = 1u32 << shrink;
    let (rw, rh) = (pw.div_ceil(step), ph.div_ceil(step));
    if rw == 0 || rh == 0 || crate::over_rgba_budget(rw, rh) {
        return None;
    }
    Some((bands, rw, rh))
}

/// Reduced `(width, height, estimated_peak_bytes)` for a TIFF at `1 / 2^shrink` without decoding
/// any pixels. `None` when [`decode_reduced`] cannot handle the file.
pub(crate) fn reduced_estimate(bytes: &[u8], shrink: u8) -> Option<(u32, u32, usize)> {
    let (bands, rw, rh) = open_checked(bytes, shrink)?;
    let group = bands.group_bytes().ok()?;
    let scratch = usize::try_from(bands.decode_scratch_bytes()).ok()?;
    let is_cmyk = bands.layout().is_cmyk();
    let target_layout = if is_cmyk { ChannelLayout::CmykA } else { ChannelLayout::Rgba };
    let out_ch = target_layout.channels();
    let src_ch = bands.layout().channels().max(1);
    let src_bpp = src_ch.saturating_mul(bands.sample_type().bytes()).max(1);
    let group_px = group / src_bpp;
    // Turning bands upright (`Orientation != 1`) copies the whole group inside `TiffBands::decode`.
    let orient_copy = if (2..=8).contains(&bands.orientation()) { group } else { 0 };
    // Float and Lab bands convert through normalized 32-bit float intermediates before narrowing to U8.
    let is_lab = matches!(bands.layout(), ChannelLayout::Lab | ChannelLayout::LabA);
    let is_float = bands.sample_type().is_float();
    let conv_per_px = if bands.layout() == target_layout && bands.sample_type() == SampleType::U8 {
        0
    } else if !is_lab && !is_float {
        out_ch
    } else if bands.layout() == target_layout {
        4 * src_ch + out_ch
    } else {
        4 * src_ch + 4 * out_ch + out_ch
    };
    let conv_bytes = group_px.saturating_mul(conv_per_px);
    let group_extra = scratch.max(orient_copy).max(conv_bytes);
    let band_peak = group.saturating_add(group_extra);
    let out_px = usize::try_from(rw).ok()?.checked_mul(usize::try_from(rh).ok()?)?;
    // Reduced output buffers (CMYK inks + alpha + RGBA display, or RGBA + premultiplied copy).
    let out_bytes = out_px.saturating_mul(if is_cmyk { 9 } else { 8 });
    let peak = band_peak.saturating_add(out_bytes).max(8);
    Some((rw, rh, peak))
}

struct AccumTarget<'a> {
    w: u32,
    h: u32,
    rw: u32,
    shrink: u8,
    step: u32,
    channels: usize,
    pixels_out: &'a mut [u8],
    alpha_out: &'a mut [u8],
    partial: &'a mut HashMap<(u32, u32), ([u32; 4], u32, u16)>,
}

fn write_cell(target: &mut AccumTarget<'_>, rx: u32, ry: u32, expected: u16, sums: [u32; 4], sum_a: u32) -> Result<(), CodecError> {
    let out_idx = usize::try_from(ry)
        .ok()
        .and_then(|y| y.checked_mul(usize::try_from(target.rw).ok()?))
        .and_then(|row| row.checked_add(usize::try_from(rx).ok()?))
        .ok_or_else(|| CodecError::LimitExceeded("reduced pixel index out of range".into()))?;
    let c_off = out_idx.checked_mul(4).ok_or_else(|| CodecError::LimitExceeded("reduced buffer offset out of range".into()))?;
    let dst = target
        .pixels_out
        .get_mut(c_off..c_off.saturating_add(4))
        .ok_or_else(|| CodecError::InvalidImage("reduced output buffer too small".into()))?;
    let exp = u32::from(expected.max(1));
    let out_a = ((sum_a + exp / 2) / exp).min(255) as u8;
    for (d, &s) in dst[..target.channels].iter_mut().zip(&sums[..target.channels]) {
        *d = (s + sum_a / 2).checked_div(sum_a).map_or(0, |v| v.min(255) as u8);
    }
    if target.channels == 3 {
        dst[3] = out_a;
    } else if let Some(a_slot) = target.alpha_out.get_mut(out_idx) {
        *a_slot = out_a;
    }
    Ok(())
}

fn accumulate_group(target: &mut AccumTarget<'_>, gx: u32, gy: u32, gw: u32, gh: u32, src: &[u8]) -> Result<(), CodecError> {
    if gw == 0 || gh == 0 || gx >= target.w || gy >= target.h {
        return Ok(());
    }
    let max_x = gx.saturating_add(gw).min(target.w);
    let max_y = gy.saturating_add(gh).min(target.h);
    let rx_start = gx >> target.shrink;
    let rx_end = (max_x - 1) >> target.shrink;
    let ry_start = gy >> target.shrink;
    let ry_end = (max_y - 1) >> target.shrink;
    let gw_usize = usize::try_from(gw).map_err(|_| CodecError::LimitExceeded("group width out of range".into()))?;
    let px_stride = target.channels + 1;

    for ry in ry_start..=ry_end {
        let uy0 = ry << target.shrink;
        let uy1 = uy0.saturating_add(target.step).min(target.h);
        let iy0 = uy0.max(gy);
        let iy1 = uy1.min(max_y);
        for rx in rx_start..=rx_end {
            let ux0 = rx << target.shrink;
            let ux1 = ux0.saturating_add(target.step).min(target.w);
            let ix0 = ux0.max(gx);
            let ix1 = ux1.min(max_x);
            let expected =
                u16::try_from((ux1 - ux0).saturating_mul(uy1 - uy0)).map_err(|_| CodecError::InvalidImage("reduced cell area overflow".into()))?;
            let seen =
                u16::try_from((ix1 - ix0).saturating_mul(iy1 - iy0)).map_err(|_| CodecError::InvalidImage("reduced overlap area overflow".into()))?;

            let mut sums = [0u32; 4];
            let mut sum_a = 0u32;
            let span_px = (ix1 - ix0) as usize;
            let x_off = (ix0 - gx) as usize;
            for y in iy0..iy1 {
                let row_start = ((y - gy) as usize).saturating_mul(gw_usize).saturating_add(x_off).saturating_mul(px_stride);
                let span_bytes = span_px.saturating_mul(px_stride);
                let row = src
                    .get(row_start..row_start.saturating_add(span_bytes))
                    .ok_or_else(|| CodecError::InvalidImage("TIFF band row truncated".into()))?;
                if target.channels == 3 {
                    for &[r, g, b, a] in row.as_chunks::<4>().0 {
                        let a = u32::from(a);
                        sum_a += a;
                        sums[0] += u32::from(r) * a;
                        sums[1] += u32::from(g) * a;
                        sums[2] += u32::from(b) * a;
                    }
                } else {
                    for &[c, m, y_ink, k, a] in row.as_chunks::<5>().0 {
                        let a = u32::from(a);
                        sum_a += a;
                        sums[0] += u32::from(c) * a;
                        sums[1] += u32::from(m) * a;
                        sums[2] += u32::from(y_ink) * a;
                        sums[3] += u32::from(k) * a;
                    }
                }
            }

            if seen == expected {
                write_cell(target, rx, ry, expected, sums, sum_a)?;
                continue;
            }
            let entry = target.partial.entry((rx, ry)).or_insert(([0u32; 4], 0u32, 0u16));
            for (dst_s, s) in entry.0.iter_mut().zip(&sums) {
                *dst_s += *s;
            }
            entry.1 += sum_a;
            entry.2 += seen;
            if entry.2 >= expected {
                let Some((done_sums, done_a, _)) = target.partial.remove(&(rx, ry)) else {
                    return Err(CodecError::InvalidImage("missing completed reduced cell".into()));
                };
                write_cell(target, rx, ry, expected, done_sums, done_a)?;
            }
        }
    }
    Ok(())
}

/// Decodes a TIFF at `1 / 2^shrink` (`1..=[MAX_SHRINK]`, each side rounded up) by streaming
/// strip/tile groups through [`TiffBands`] and averaging directly into the reduced display buffer,
/// applying the embedded ICC profile or active working CMYK profile just like [`crate::decode_rgba`].
pub(crate) fn decode_reduced(bytes: &[u8], shrink: u8) -> Option<RgbaImage> {
    let (mut bands, rw, rh) = open_checked(bytes, shrink)?;
    let (w, h) = bands.dimensions();
    let step = 1u32 << shrink;
    let is_cmyk = bands.layout().is_cmyk();
    let is_gray = bands.layout().is_gray();

    let out_px = usize::try_from(rw).ok()?.checked_mul(usize::try_from(rh).ok()?)?;
    let rgba_len = out_px.checked_mul(4)?;
    let mut pixels_out = Vec::new();
    pixels_out.try_reserve_exact(rgba_len).ok()?;
    pixels_out.resize(rgba_len, 0);

    let mut alpha_out = Vec::new();
    if is_cmyk {
        alpha_out.try_reserve_exact(out_px).ok()?;
        alpha_out.resize(out_px, 255);
    }

    let mut partial = HashMap::new();
    let target_layout = if is_cmyk { ChannelLayout::CmykA } else { ChannelLayout::Rgba };
    let mut target = AccumTarget {
        w,
        h,
        rw,
        shrink,
        step,
        channels: if is_cmyk { 4 } else { 3 },
        pixels_out: &mut pixels_out,
        alpha_out: &mut alpha_out,
        partial: &mut partial,
    };

    bands
        .decode::<CodecError>(|gx, gy, group| {
            let (gw, gh) = group.dimensions();
            let converted = group.converted(target_layout, SampleType::U8);
            let src = converted.as_u8().ok_or_else(|| CodecError::InvalidImage("converted TIFF band is not 8-bit".into()))?;
            accumulate_group(&mut target, gx, gy, gw, gh, src)
        })
        .ok()?;

    if !partial.is_empty() {
        return None;
    }

    let icc = bands.icc.take();
    if is_cmyk {
        let alpha = alpha_out.iter().any(|&a| a < 255).then_some(alpha_out);
        let raster = CmykRaster { width: rw, height: rh, cmyk: pixels_out, alpha };
        return to_display_rgba(Pixels::Cmyk(raster), icc.as_deref(), &DisplayColour::active());
    }
    let rgba = RgbaImage::from_raw(rw, rh, pixels_out)?;
    let Some(icc) = icc.as_deref() else { return Some(rgba) };
    let px = if is_gray { Pixels::Gray(rgba) } else { Pixels::Rgb(rgba) };
    to_display_rgba(px, Some(icc), &DisplayColour::active())
}
