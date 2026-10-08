//! Palette TIFF (as in EPS previews) → PNG, because the image decoder only reads RGB/grey TIFFs.

use crate::bytes::{clamp, slice};

const MAX_PIXELS: usize = 64 << 20;

struct Ifd<'a> {
    data: &'a [u8],
    le: bool,
    entries: Vec<(u16, u16, u32, u32)>,
}

impl<'a> Ifd<'a> {
    fn u16(&self, off: usize) -> Option<u16> {
        let b = slice(self.data, off, 2)?;
        Some(if self.le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    }

    fn u32(&self, off: usize) -> Option<u32> {
        let b = slice(self.data, off, 4)?;
        let a = [b[0], b[1], b[2], b[3]];
        Some(if self.le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }

    fn parse(data: &'a [u8]) -> Option<Self> {
        let le = match data.get(..4)? {
            b"II*\0" => true,
            b"MM\0*" => false,
            _ => return None,
        };
        let mut ifd = Ifd { data, le, entries: Vec::new() };
        let off = ifd.u32(4)? as usize;
        let n = ifd.u16(off)? as usize;
        for i in 0..n.min(512) {
            // `off` comes from the file: on 32-bit targets these sums can overflow.
            let e = off.checked_add(2 + 12 * i)?;
            let (tag, typ, count) = (ifd.u16(e)?, ifd.u16(e.checked_add(2)?)?, ifd.u32(e.checked_add(4)?)?);
            // A single SHORT value sits in the first two bytes of the value field.
            let value = if typ == 3 && count == 1 { u32::from(ifd.u16(e.checked_add(8)?)?) } else { ifd.u32(e.checked_add(8)?)? };
            ifd.entries.push((tag, typ, count, value));
        }
        Some(ifd)
    }

    fn get(&self, tag: u16) -> Option<(u16, u32, u32)> {
        self.entries.iter().find(|e| e.0 == tag).map(|e| (e.1, e.2, e.3))
    }

    fn value(&self, tag: u16) -> Option<u32> {
        self.get(tag).map(|(_, _, v)| v)
    }

    /// All values of a SHORT/LONG array tag.
    fn values(&self, tag: u16) -> Option<Vec<u32>> {
        let (typ, count, value) = self.get(tag)?;
        let size = if typ == 3 { 2 } else { 4 };
        let count = count as usize;
        if count.checked_mul(size)? <= 4 {
            return Some(if typ == 3 && count == 2 { vec![value & 0xFFFF, value >> 16] } else { vec![value] });
        }
        (0..count.min(1 << 20))
            .map(|i| {
                let off = (value as usize).checked_add(i.checked_mul(size)?)?;
                if typ == 3 { self.u16(off).map(u32::from) } else { self.u32(off) }
            })
            .collect()
    }
}

fn packbits(src: &[u8], want: usize) -> Vec<u8> {
    // `want` comes from the header; the data decides how much is really produced.
    let mut out = Vec::with_capacity(want.min(src.len()));
    let mut i = 0;
    while out.len() < want {
        let Some(&b) = src.get(i) else { break };
        let n = b as i8;
        i += 1;
        if n >= 0 {
            let len = n as usize + 1;
            out.extend_from_slice(clamp(src, i, len));
            i = i.saturating_add(len);
        } else if n != -128 {
            let Some(&b) = src.get(i) else { break };
            out.extend(std::iter::repeat_n(b, (1 - i32::from(n)) as usize));
            i += 1;
        }
    }
    out
}

/// PNG bytes for an 8-bit palette TIFF (optionally with an alpha extra sample); None for any other
/// TIFF, which is passed through unchanged.
pub(crate) fn palette_to_png(data: &[u8]) -> Option<Vec<u8>> {
    let ifd = Ifd::parse(data)?;
    if ifd.value(262)? != 3 || ifd.value(258).unwrap_or(1) != 8 || ifd.value(284).unwrap_or(1) != 1 {
        return None;
    }
    let (w, h) = (ifd.value(256)? as usize, ifd.value(257)? as usize);
    let spp = ifd.value(277).unwrap_or(1) as usize;
    if w == 0 || h == 0 || !(1..=2).contains(&spp) || w.checked_mul(h)? > MAX_PIXELS {
        return None;
    }
    let compression = ifd.value(259).unwrap_or(1);
    let map = ifd.values(320)?;
    if map.len() < 3 * 256 {
        return None;
    }
    let offsets = ifd.values(273)?;
    let counts = ifd.values(279)?;
    let want = w.checked_mul(h)?.checked_mul(spp)?;
    // The header sizes are not trusted for allocations: the strips decide how much is read.
    let mut raw = Vec::with_capacity(want.min(data.len()));
    for (o, c) in offsets.iter().zip(&counts) {
        // Strips may repeat; stop once the image is complete.
        if raw.len() >= want {
            break;
        }
        let strip = slice(data, *o as usize, *c as usize)?;
        match compression {
            1 => raw.extend_from_slice(clamp(strip, 0, want.saturating_sub(raw.len()))),
            32773 => raw.extend(packbits(strip, want.saturating_sub(raw.len()))),
            _ => return None,
        }
    }
    // Too few pixels for the declared size: reject before allocating the RGBA image.
    if raw.len() < want {
        return None;
    }
    let mut rgba = Vec::with_capacity(w.checked_mul(h)?.checked_mul(4)?);
    for px in raw.chunks_exact(spp).take(w * h) {
        let idx = px[0] as usize;
        let c = |k: usize| (map.get(k * 256 + idx).copied().unwrap_or(0) >> 8) as u8;
        rgba.extend_from_slice(&[c(0), c(1), c(2), if spp == 2 { px[1] } else { 255 }]);
    }
    if rgba.len() != w * h * 4 {
        return None;
    }
    let mut png = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut png);
    image::ImageEncoder::write_image(enc, &rgba, w as u32, h as u32, image::ExtendedColorType::Rgba8).ok()?;
    Some(png)
}
