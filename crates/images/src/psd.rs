//! Photoshop documents (.psd, and .psb large documents): the merged composite in the image data
//! section, decoded by its colour mode per the public file format specification. Extra alpha and
//! spot channels don't change the placed image; when the layer count is negative, the first extra
//! channel is the composite's merged transparency.

use designcraft_color::cms::{Lab, lab::lab_to_srgb};

use crate::CmykRaster;
use crate::colour::{DisplayColour, Pixels, to_display_rgba};

/// The largest decoded image, in RGBA bytes (the `image` crate's default allocation limit).
const MAX_RGBA_BYTES: usize = 512 << 20;

/// Tagged blocks whose length is 8 bytes in a PSB.
const LONG_KEYS: [&[u8; 4]; 13] =
    [b"LMsk", b"Lr16", b"Lr32", b"Layr", b"Mt16", b"Mt32", b"Mtrn", b"Alph", b"FMsk", b"lnk2", b"FEid", b"FXid", b"PxSD"];

/// Big-endian reads; each one fails at the end of the data.
struct Reader<'a> {
    b: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.b.split_at_checked(n)?;
        self.b = rest;
        Some(head)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
    /// A length-prefixed block: 4-byte length, or 8 bytes when `long`.
    fn block(&mut self, long: bool) -> Option<&'a [u8]> {
        let n = if long { u64::from_be_bytes(self.take(8)?.try_into().ok()?) } else { u64::from(self.u32()?) };
        self.take(usize::try_from(n).ok()?)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Bitmap,
    /// Grayscale, and Duotone (its data is the gray channel; the inks are in the colour mode data).
    Gray,
    Indexed,
    Rgb,
    Cmyk,
    Lab,
}

impl Mode {
    fn parse(v: u16) -> Option<Mode> {
        Some(match v {
            0 => Mode::Bitmap,
            1 | 8 => Mode::Gray,
            2 => Mode::Indexed,
            3 => Mode::Rgb,
            4 => Mode::Cmyk,
            9 => Mode::Lab,
            _ => return None,
        })
    }
    fn colour_channels(self) -> usize {
        match self {
            Mode::Bitmap | Mode::Gray | Mode::Indexed => 1,
            Mode::Rgb | Mode::Lab => 3,
            Mode::Cmyk => 4,
        }
    }
    /// The stored channel values of white: a composite with transparency is matted against it.
    fn white(self) -> Option<[f32; 4]> {
        match self {
            Mode::Gray | Mode::Rgb | Mode::Cmyk => Some([255.0; 4]),
            Mode::Lab => Some([255.0, 128.0, 128.0, 0.0]),
            Mode::Bitmap | Mode::Indexed => None,
        }
    }
}

/// The merged composite as straight RGBA8, `None` when the file is damaged or unsupported (32-bit,
/// Multichannel, ZIP-compressed composite).
/// Whether the header's depth, channel count and size are all within what the decoder supports.
fn header_in_range(mode: Mode, depth: u16, channels: usize, (w, h): (u32, u32), max_side: u32) -> bool {
    let depth_ok = match mode {
        Mode::Bitmap => depth == 1,
        Mode::Indexed => depth == 8,
        _ => depth == 8 || depth == 16,
    };
    (1..=56).contains(&channels) && channels >= mode.colour_channels() && (1..=max_side).contains(&w) && (1..=max_side).contains(&h) && depth_ok
}

pub(crate) fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    let mut r = Reader { b: bytes };
    if r.take(4)? != b"8BPS" {
        return None;
    }
    let psb = match r.u16()? {
        1 => false,
        2 => true,
        _ => return None,
    };
    r.take(6)?;
    let channels = usize::from(r.u16()?);
    let (h, w) = (r.u32()?, r.u32()?);
    let depth = r.u16()?;
    let mode = Mode::parse(r.u16()?)?;
    let max_side = if psb { 300_000 } else { 30_000 };
    let colours = mode.colour_channels();
    if !header_in_range(mode, depth, channels, (w, h), max_side) {
        return None;
    }
    let (wu, hu) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    let pixels = wu.checked_mul(hu)?;
    let (stride, len) = (wu.checked_mul(4)?, pixels.checked_mul(4)?);
    if len > MAX_RGBA_BYTES {
        return None;
    }

    let colour_mode_data = r.block(false)?;
    let icc = icc_profile(r.block(false)?);
    let layers = r.block(psb)?;
    let transparency = channels > colours && layer_count(layers, psb).is_some_and(|n| n < 0);
    let row_bytes = wu.checked_mul(usize::from(depth))?.div_ceil(8);
    let data = ImageData::parse(r, psb, channels, row_bytes, hu)?;
    let palette = if mode == Mode::Indexed { Some(palette(colour_mode_data)?) } else { None };
    // Header sizes are untrusted: allocate only once the data to fill the image is present.
    data.holds(colours + usize::from(transparency))?;

    let mut rgba = vec![0u8; len];
    for c in 0..colours {
        let mut rows = rgba.chunks_exact_mut(stride);
        data.channel(c, |src| {
            if let Some(dst) = rows.next() {
                fill(dst, c, 4, src, depth);
            }
        })?;
    }
    let mut alpha = Vec::new();
    if transparency {
        alpha = vec![0u8; pixels];
        let mut rows = alpha.chunks_exact_mut(wu);
        data.channel(colours, |src| {
            if let Some(dst) = rows.next() {
                fill(dst, 0, 1, src, depth);
            }
        })?;
    }

    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let byte = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    let white = mode.white();
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let a = alpha.get(i).copied().unwrap_or(255);
        let mut v = px.map(f32::from);
        // The composite is matted against white where it's partly transparent: un-matte it.
        if let Some(white) = white
            && a > 0
            && a < 255
        {
            for (v, w) in v.iter_mut().zip(white).take(colours) {
                *v = (w + (*v - w) * 255.0 / f32::from(a)).round().clamp(0.0, 255.0);
            }
        }
        let [v0, v1, v2, v3] = v;
        *px = match mode {
            Mode::Bitmap | Mode::Gray => [byte(v0), byte(v0), byte(v0), a],
            Mode::Indexed => {
                let Some([pr, pg, pb]) = &palette else { return None };
                let i = usize::from(px[0]);
                [pr[i], pg[i], pb[i], a]
            }
            Mode::Rgb => [byte(v0), byte(v1), byte(v2), a],
            // Stored inverted (255 is no ink): ink values, converted below.
            Mode::Cmyk => [v0, v1, v2, v3].map(|v| 255 - byte(v)),
            Mode::Lab => {
                let [r, g, b] = lab_to_srgb(Lab::new(v0 * 100.0 / 255.0, v1 - 128.0, v2 - 128.0));
                [q(r), q(g), q(b), a]
            }
        };
    }
    let icc = icc.filter(|_| matches!(mode, Mode::Gray | Mode::Rgb | Mode::Cmyk));
    if mode == Mode::Cmyk {
        let alpha = transparency.then_some(alpha);
        let raster = CmykRaster { width: w, height: h, cmyk: rgba, alpha };
        return to_display_rgba(Pixels::Cmyk(raster), icc, &DisplayColour::active());
    }
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    match icc {
        Some(icc) if mode == Mode::Gray => to_display_rgba(Pixels::Gray(img), Some(icc), &DisplayColour::active()),
        Some(icc) => to_display_rgba(Pixels::Rgb(img), Some(icc), &DisplayColour::active()),
        None => Some(img),
    }
}

/// The ICC profile in the image resources section (resource 1039), if any.
fn icc_profile(resources: &[u8]) -> Option<&[u8]> {
    let mut r = Reader { b: resources };
    while r.take(4)? == b"8BIM" {
        let id = r.u16()?;
        // Pascal name, padded to an even length including its length byte.
        let name_len = usize::from(*r.b.first()?);
        r.take((name_len + 2) & !1)?;
        let data = r.block(false)?;
        if data.len() % 2 == 1 {
            r.take(1)?;
        }
        if id == 1039 {
            return Some(data);
        }
    }
    None
}

/// The layer count from the layer and mask section. 16- and 32-bit documents keep their layer
/// info in an `Lr16` / `Lr32` tagged block after the global layer mask info.
fn layer_count(section: &[u8], psb: bool) -> Option<i16> {
    let count = |info: &[u8]| Some(i16::from_be_bytes(info.get(..2)?.try_into().ok()?));
    let mut r = Reader { b: section };
    let info = r.block(psb)?;
    if !info.is_empty() {
        return count(info);
    }
    r.block(false)?; // global layer mask info
    while !r.b.is_empty() {
        let sig = r.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            return None;
        }
        let key = r.take(4)?;
        let data = r.block(psb && LONG_KEYS.iter().any(|k| k.as_slice() == key))?;
        if matches!(key, b"Lr16" | b"Lr32" | b"Layr") {
            return count(data);
        }
        // Padding to 2 or 4 bytes (a signature never starts with 0).
        while r.b.first() == Some(&0) {
            r.take(1)?;
        }
    }
    None
}

/// An indexed-colour palette: 256 reds, then greens, then blues.
fn palette(data: &[u8]) -> Option<[[u8; 256]; 3]> {
    let part = |i: usize| data.get(i * 256..(i + 1) * 256)?.try_into().ok();
    Some([part(0)?, part(1)?, part(2)?])
}

/// The composite's channels: raw planes, or PackBits-compressed rows.
struct ImageData<'a> {
    /// Each row's compressed length, channel after channel (2 bytes each, 4 in a PSB); empty for raw.
    counts: &'a [u8],
    count_size: usize,
    data: &'a [u8],
    row_bytes: usize,
    height: usize,
}

impl<'a> ImageData<'a> {
    fn parse(mut r: Reader<'a>, psb: bool, channels: usize, row_bytes: usize, height: usize) -> Option<Self> {
        let count_size = if psb { 4 } else { 2 };
        let counts = match r.u16()? {
            0 => &[][..],
            1 => r.take(channels.checked_mul(height)?.checked_mul(count_size)?)?,
            // ZIP appears only in layers, never in the composite.
            _ => return None,
        };
        Some(ImageData { counts, count_size, data: r.b, row_bytes, height })
    }

    /// `Some` when the data can hold the first `channels` channels: whole raw planes, or row
    /// counts that fit in the data and are each long enough to unpack to a full row (PackBits
    /// writes at most 128 bytes per 2 input bytes).
    fn holds(&self, channels: usize) -> Option<()> {
        let rows = channels.checked_mul(self.height)?;
        if self.counts.is_empty() {
            let need = self.row_bytes.checked_mul(rows)?;
            return (need <= self.data.len()).then_some(());
        }
        let min_row = self.row_bytes.div_ceil(128).checked_mul(2)?;
        let mut total = 0usize;
        for c in self.counts.chunks_exact(self.count_size).take(rows) {
            let n = c.iter().fold(0usize, |n, &b| n << 8 | usize::from(b));
            if n < min_row {
                return None;
            }
            total = total.checked_add(n)?;
        }
        (total <= self.data.len()).then_some(())
    }

    /// Channel `ch`'s rows, uncompressed, to `row` from top to bottom.
    fn channel(&self, ch: usize, mut row: impl FnMut(&[u8])) -> Option<()> {
        if self.row_bytes == 0 {
            return None;
        }
        if self.counts.is_empty() {
            let plane = self.row_bytes.checked_mul(self.height)?;
            let start = ch.checked_mul(plane)?;
            self.data.get(start..start.checked_add(plane)?)?.chunks_exact(self.row_bytes).for_each(row);
            return Some(());
        }
        let mut counts = self.counts.chunks_exact(self.count_size).map(|c| c.iter().fold(0usize, |n, &b| n << 8 | usize::from(b)));
        // This channel starts after every earlier channel's rows.
        let mut at = counts.by_ref().take(ch.checked_mul(self.height)?).try_fold(0usize, usize::checked_add)?;
        for _ in 0..self.height {
            let n = counts.next()?;
            let end = at.checked_add(n)?;
            row(&unpack_bits(self.data.get(at..end)?, self.row_bytes)?);
            at = end;
        }
        Some(())
    }
}

/// One PackBits row; `None` unless it unpacks to exactly `len` bytes.
fn unpack_bits(mut src: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(len);
    while let Some((&head, rest)) = src.split_first() {
        src = rest;
        match head as i8 {
            -128 => {}
            n @ 0.. => {
                let (literal, rest) = src.split_at_checked(n as usize + 1)?;
                out.extend_from_slice(literal);
                src = rest;
            }
            n => {
                let (&v, rest) = src.split_first()?;
                out.resize(out.len() + (1 - n as isize) as usize, v);
                src = rest;
            }
        }
        if out.len() > len {
            return None;
        }
    }
    (out.len() == len).then_some(out)
}

/// A row of `depth`-bit samples as 8-bit values, into every `step`-th byte of `dst` from `slot`.
/// Bitmap mode stores black as 1.
fn fill(dst: &mut [u8], slot: usize, step: usize, src: &[u8], depth: u16) {
    let dst = dst.iter_mut().skip(slot).step_by(step);
    match depth {
        1 => dst.zip(src.iter().flat_map(|&b| (0..8).rev().map(move |i| if (b >> i) & 1 == 1 { 0 } else { 255 }))).for_each(|(d, s)| *d = s),
        16 => dst
            .zip(src.as_chunks::<2>().0.iter().map(|c| c.iter().fold(0u32, |n, &b| n << 8 | u32::from(b))))
            .for_each(|(d, s)| *d = ((s * 255 + 32_767) / 65_535) as u8),
        _ => dst.zip(src).for_each(|(d, &s)| *d = s),
    }
}

#[cfg(test)]
mod tests;
