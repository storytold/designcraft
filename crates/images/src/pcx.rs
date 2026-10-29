//! PCX (ZSoft Paintbrush) rasters: 1, 2, 4 and 8 bits per plane, one plane (paletted),
//! three planes (RGB) or four (EGA 1-bit, or 8-bit with alpha), compressed run-length encoded.
//! Decoding follows the public PCX file format description; damaged files return `None`.
//!
//! A single-plane 1-bit file is a *bitmap*: InDesign treats its white pixels as transparent
//! (paper) and its black pixels as ink, so a placed line drawing shows the page behind it.
//! Other paletted forms keep every pixel opaque.

/// The 128-byte PCX header.
const HEADER_LEN: usize = 128;

/// The largest decoded image, in RGBA bytes (the `image` crate's default allocation limit).
const MAX_RGBA_BYTES: usize = 512 << 20;

/// The 256-colour palette at the end of an 8-bit file: a 0x0C marker and 768 RGB bytes.
const PALETTE_TRAILER: usize = 769;

struct Header {
    bits: u8,
    planes: u8,
    width: u32,
    height: u32,
    bytes_per_line: usize,
    /// The 16-colour palette of the header (1-, 2-, 4-bit and EGA files).
    palette: [[u8; 3]; 16],
}

impl Header {
    /// The header of a supported PCX, if `bytes` is one: ZSoft maker, RLE encoding, a known
    /// bits/planes combination, sane dimensions and a line length that fits `width`.
    fn parse(bytes: &[u8]) -> Option<Self> {
        let h = bytes.get(..HEADER_LEN)?;
        if h.first().copied()? != 0x0A {
            return None;
        }
        // Versions 0–5; anything newer is not a PCX we know how to read.
        if *h.get(1)? > 5 {
            return None;
        }
        // 1 = run-length encoded. Uncompressed (version 2) files are not seen in practice.
        if *h.get(2)? != 1 {
            return None;
        }
        let bits = *h.get(3)?;
        let le16 = |at: usize| -> Option<u16> { Some(u16::from_le_bytes(h.get(at..at.checked_add(2)?)?.try_into().ok()?)) };
        let (xmin, ymin, xmax, ymax) = (le16(4)?, le16(6)?, le16(8)?, le16(10)?);
        let planes = *h.get(65)?;
        let bytes_per_line = usize::from(le16(66)?);
        if !matches!((planes, bits), (1, 1 | 2 | 4 | 8) | (3, 8) | (4, 1 | 8)) {
            return None;
        }
        if xmax < xmin || ymax < ymin {
            return None;
        }
        let width = u32::from(xmax - xmin) + 1;
        let height = u32::from(ymax - ymin) + 1;
        let (wu, hu) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
        // Each plane of each scanline holds this many bytes of pixels, padding to `bytes_per_line`.
        let needed = wu.checked_mul(usize::from(bits))?.div_ceil(8);
        let row = bytes_per_line.checked_mul(usize::from(planes))?;
        if bytes_per_line < needed || row.checked_mul(hu)? > MAX_RGBA_BYTES {
            return None;
        }
        let pixels = wu.checked_mul(hu)?;
        if pixels.checked_mul(4)? > MAX_RGBA_BYTES {
            return None;
        }
        let mut palette = [[0u8; 3]; 16];
        for (i, entry) in palette.iter_mut().enumerate() {
            let at = 16 + i * 3;
            let c = h.get(at..at + 3)?;
            entry.copy_from_slice(c);
        }
        Some(Self { bits, planes, width, height, bytes_per_line, palette })
    }

    /// The colour table the indices stand for. 8-bit files carry their 256 colours after the
    /// pixel data; without one (a few old grey files) the indices are grey levels.
    fn colours(&self, bytes: &[u8]) -> Vec<[u8; 3]> {
        if self.planes == 1 && self.bits == 8 {
            let table = bytes.len().checked_sub(PALETTE_TRAILER).and_then(|at| bytes.get(at..)).and_then(|t| t.strip_prefix(&[0x0C]));
            if let Some(table) = table {
                return table.as_chunks::<3>().0.to_vec();
            }
            return (0..=255u8).map(|v| [v; 3]).collect();
        }
        self.palette.to_vec()
    }
}

/// Is this a PCX file?
pub fn is_pcx(bytes: &[u8]) -> bool {
    Header::parse(bytes).is_some()
}

/// Pixel size from the header.
pub(crate) fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let h = Header::parse(bytes)?;
    Some((h.width, h.height))
}

/// The image as straight (non-premultiplied) RGBA8, `None` when the file is damaged or unsupported.
pub(crate) fn decode(bytes: &[u8]) -> Option<image::RgbaImage> {
    let h = Header::parse(bytes)?;
    let (w, height) = (usize::try_from(h.width).ok()?, usize::try_from(h.height).ok()?);
    let planes = usize::from(h.planes);
    let bpl = h.bytes_per_line;
    let row = bpl.checked_mul(planes)?;
    let data = rle(bytes.get(HEADER_LEN..)?, row.checked_mul(height)?)?;
    let colours = h.colours(bytes);
    let mut out = Vec::new();
    out.try_reserve_exact(w.checked_mul(height)?.checked_mul(4)?).ok()?;
    for line in data.chunks_exact(row).take(height) {
        match (planes, h.bits) {
            (1, 8) => {
                for &i in line.get(..w)? {
                    let [r, g, b] = *colours.get(usize::from(i))?;
                    out.extend([r, g, b, 255]);
                }
            }
            (3, 8) => {
                let (red, rest) = line.split_at_checked(bpl)?;
                let (green, blue) = rest.split_at_checked(bpl)?;
                for x in 0..w {
                    out.extend([*red.get(x)?, *green.get(x)?, *blue.get(x)?, 255]);
                }
            }
            (4, 8) => {
                let (red, rest) = line.split_at_checked(bpl)?;
                let (green, rest) = rest.split_at_checked(bpl)?;
                let (blue, alpha) = rest.split_at_checked(bpl)?;
                for x in 0..w {
                    out.extend([*red.get(x)?, *green.get(x)?, *blue.get(x)?, *alpha.get(x)?]);
                }
            }
            (1, bits @ (1 | 2 | 4)) => {
                let bits = usize::from(bits);
                let per_byte = 8 / bits;
                let mask = (1u8 << bits) - 1;
                for x in 0..w {
                    // Pixels are packed high bits first.
                    let byte = *line.get(x / per_byte)?;
                    let shift = 8 - bits * (x % per_byte + 1);
                    let [r, g, b] = *colours.get(usize::from((byte >> shift) & mask))?;
                    // A bitmap's white is the paper: transparent, as InDesign places it.
                    let alpha = if bits == 1 && is_white([r, g, b]) { 0 } else { 255 };
                    out.extend([r, g, b, alpha]);
                }
            }
            (4, 1) => {
                // EGA: one bit per plane, plane 0 the least significant bit of the index.
                let (p0, rest) = line.split_at_checked(bpl)?;
                let (p1, rest) = rest.split_at_checked(bpl)?;
                let (p2, p3) = rest.split_at_checked(bpl)?;
                for x in 0..w {
                    let (byte, bit) = (x / 8, 7 - x % 8);
                    let index = usize::from((*p0.get(byte)? >> bit) & 1)
                        | usize::from((*p1.get(byte)? >> bit) & 1) << 1
                        | usize::from((*p2.get(byte)? >> bit) & 1) << 2
                        | usize::from((*p3.get(byte)? >> bit) & 1) << 3;
                    let [r, g, b] = *colours.get(index)?;
                    out.extend([r, g, b, 255]);
                }
            }
            _ => return None,
        }
    }
    image::RgbaImage::from_raw(h.width, h.height, out)
}

/// A palette entry a bitmap treats as paper: white, allowing the tiny rounding some writers
/// leave in a "white" entry.
fn is_white([r, g, b]: [u8; 3]) -> bool {
    r >= 250 && g >= 250 && b >= 250
}

/// Run-length decode exactly `len` bytes: 0xC0-set bytes carry a run length in their low six
/// bits and the value in the next byte; any other byte is a literal.
fn rle(src: &[u8], len: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.try_reserve_exact(len).ok()?;
    let mut at = 0usize;
    while out.len() < len {
        let b = *src.get(at)?;
        at += 1;
        if b & 0xC0 == 0xC0 {
            let run = usize::from(b & 0x3F);
            let value = *src.get(at)?;
            at += 1;
            if run == 0 {
                return None;
            }
            // The last run may pad past the logical end; keep only what the line holds.
            let take = run.min(len - out.len());
            out.extend(std::iter::repeat_n(value, take));
        } else {
            out.push(b);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PCX header for `w`×`h` at `bits` per plane, `planes` planes and `bpl` bytes per line.
    fn header(bits: u8, planes: u8, w: u16, h: u16, bpl: u16, palette: &[[u8; 3]]) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN];
        b[0] = 0x0A;
        b[1] = 5;
        b[2] = 1;
        b[3] = bits;
        b[6..8].copy_from_slice(&0u16.to_le_bytes());
        b[8..10].copy_from_slice(&(w - 1).to_le_bytes());
        b[10..12].copy_from_slice(&(h - 1).to_le_bytes());
        b[12..14].copy_from_slice(&72u16.to_le_bytes());
        b[14..16].copy_from_slice(&72u16.to_le_bytes());
        for (i, colour) in palette.iter().enumerate() {
            b[16 + i * 3..16 + i * 3 + 3].copy_from_slice(colour);
        }
        b[65] = planes;
        b[66..68].copy_from_slice(&bpl.to_le_bytes());
        b[68..70].copy_from_slice(&1u16.to_le_bytes());
        b
    }

    /// RLE-encode one plane's scanline.
    fn rle_encode(src: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < src.len() {
            let value = src[at];
            let mut run = 1;
            while at + run < src.len() && src[at + run] == value && run < 0x3F {
                run += 1;
            }
            if run > 1 || value & 0xC0 == 0xC0 {
                out.push(0xC0 | run as u8);
                out.push(value);
            } else {
                out.push(value);
            }
            at += run;
        }
        out
    }

    fn append_palette(bytes: &mut Vec<u8>, colours: &[[u8; 3]]) {
        // The 256-colour palette is the last 769 bytes: the 0x0C marker, then 768 RGB bytes.
        bytes.push(0x0C);
        let mut table = [[0u8; 3]; 256];
        for (slot, colour) in table.iter_mut().zip(colours) {
            *slot = *colour;
        }
        for colour in table {
            bytes.extend(colour);
        }
    }

    #[test]
    fn palette8_pixels_come_from_the_trailing_palette() {
        let colours = [[10, 20, 30], [255, 0, 0], [0, 128, 255], [255, 255, 255]];
        let mut b = header(8, 1, 3, 2, 4, &[]);
        // Row 0: 0 1 2 0; row 1: 3 1 0 0 (the last byte is line padding).
        b.extend(rle_encode(&[0, 1, 2, 0, 3, 1, 0, 0]));
        append_palette(&mut b, &colours);
        assert!(is_pcx(&b));
        assert_eq!(pixel_size(&b), Some((3, 2)));
        let img = decode(&b).expect("decodes");
        let px: Vec<_> = img.pixels().map(|p| p.0).collect();
        assert_eq!(px, [[10, 20, 30, 255], [255, 0, 0, 255], [0, 128, 255, 255], [255, 255, 255, 255], [255, 0, 0, 255], [10, 20, 30, 255]]);
    }

    #[test]
    fn palette8_without_a_trailer_reads_as_grey() {
        let mut b = header(8, 1, 2, 1, 2, &[]);
        b.extend(rle_encode(&[0, 200]));
        assert!(is_pcx(&b));
        let img = decode(&b).expect("decodes");
        let px: Vec<_> = img.pixels().map(|p| p.0).collect();
        assert_eq!(px, [[0, 0, 0, 255], [200, 200, 200, 255]]);
    }

    #[test]
    fn three_plane_rgb_rows_interleave_planes() {
        let mut b = header(8, 3, 2, 1, 2, &[]);
        // Red plane: 255 0 · green: 0 128 · blue: 0 255 (each padded to two bytes).
        b.extend(rle_encode(&[255, 0, 0, 128, 0, 255]));
        assert!(is_pcx(&b));
        assert_eq!(pixel_size(&b), Some((2, 1)));
        let px: Vec<_> = decode(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px, [[255, 0, 0, 255], [0, 128, 255, 255]]);
    }

    #[test]
    fn four_plane_rgb_keeps_alpha() {
        let mut b = header(8, 4, 1, 1, 2, &[]);
        b.extend(rle_encode(&[10, 0, 20, 0, 30, 0, 40, 0]));
        assert!(is_pcx(&b));
        let px: Vec<_> = decode(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px, [[10, 20, 30, 40]]);
    }

    #[test]
    fn one_bit_pixels_use_the_header_palette() {
        let mut palette = [[0u8; 3]; 16];
        palette[0] = [255, 255, 255];
        palette[1] = [0, 0, 0];
        let mut b = header(1, 1, 10, 1, 2, &palette);
        // 1 0 1 0 1 0 1 0 | 1 0, padded to two bytes.
        b.extend(rle_encode(&[0xAA, 0x80]));
        assert!(is_pcx(&b));
        let px: Vec<_> = decode(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px[0], [0, 0, 0, 255]);
        assert_eq!(px[1], [255, 255, 255, 0], "white is the bitmap's paper");
        assert_eq!(px[8], [0, 0, 0, 255], "the ninth pixel is the first bit of the last byte");
    }

    #[test]
    fn one_bit_black_on_white_is_ink_on_transparent() {
        // The common clip-art palette: index 0 black ink, index 1 white paper.
        let mut palette = [[0u8; 3]; 16];
        palette[1] = [255, 255, 255];
        let mut b = header(1, 1, 2, 1, 1, &palette);
        b.extend(rle_encode(&[0x40]));
        assert!(is_pcx(&b));
        let px: Vec<_> = decode(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px, [[0, 0, 0, 255], [255, 255, 255, 0]]);
    }

    #[test]
    fn four_bit_pixels_are_high_nibble_first() {
        let mut palette = [[0u8; 3]; 16];
        palette[1] = [10, 20, 30];
        palette[2] = [40, 50, 60];
        let mut b = header(4, 1, 3, 1, 2, &palette);
        b.extend(rle_encode(&[0x21, 0x00]));
        assert!(is_pcx(&b));
        let px: Vec<_> = decode(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px, [[40, 50, 60, 255], [10, 20, 30, 255], [0, 0, 0, 255]]);
    }

    #[test]
    fn ega_planes_form_the_index_bit_by_bit() {
        let mut palette = [[0u8; 3]; 16];
        palette[5] = [1, 2, 3];
        let mut b = header(1, 4, 2, 1, 1, &palette);
        // Index 5 at x0 (bits 0 and 2), index 0 at x1: plane 0 = 10, plane 2 = 10.
        b.extend(rle_encode(&[0x80, 0x00, 0x80, 0x00]));
        assert!(is_pcx(&b));
        let px: Vec<_> = decode(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px, [[1, 2, 3, 255], [0, 0, 0, 255]]);
    }

    #[test]
    fn damaged_files_are_rejected() {
        let good = {
            let mut b = header(8, 1, 2, 2, 2, &[]);
            b.extend(rle_encode(&[1, 2, 3, 4]));
            append_palette(&mut b, &[[0, 0, 0]; 4]);
            b
        };
        assert!(is_pcx(&good));
        assert_eq!(decode(&good).map(|i| i.dimensions()), Some((2, 2)));

        // Truncated header, wrong maker, an unknown encoding, a bad bits/planes pair.
        assert!(!is_pcx(&good[..100]));
        let mut b = good.clone();
        b[0] = 0x0B;
        assert!(!is_pcx(&b));
        let mut b = good.clone();
        b[2] = 0;
        assert!(!is_pcx(&b));
        let mut b = good.clone();
        b[3] = 3;
        assert!(!is_pcx(&b));
        let mut b = good.clone();
        b[65] = 2;
        assert!(!is_pcx(&b));

        // A line shorter than the width needs, and dimensions beyond what a decode may allocate.
        let mut b = good.clone();
        b[66..68].copy_from_slice(&1u16.to_le_bytes());
        assert!(!is_pcx(&b));
        let mut b = good.clone();
        b[8..10].copy_from_slice(&65535u16.to_le_bytes());
        b[10..12].copy_from_slice(&65535u16.to_le_bytes());
        b[66..68].copy_from_slice(&8192u16.to_le_bytes());
        assert!(!is_pcx(&b));

        // A truncated pixel stream cannot be decoded, even when the header parses.
        let cut = good.len().min(HEADER_LEN + 2);
        assert!(!is_pcx(&good[..cut]) || decode(&good[..cut]).is_none());
    }

    #[test]
    fn runs_may_pad_the_last_line_but_never_go_past_it() {
        // A 4-pixel line in two bytes, encoded as one run of 4 then an extra value.
        let mut b = header(8, 1, 4, 1, 4, &[]);
        b.push(0xC4);
        b.push(7);
        b.extend([9, 9]);
        let px: Vec<_> = decode(&b).expect("decodes").pixels().map(|p| p.0).collect();
        assert_eq!(px, [[7, 7, 7, 255]; 4]);
        // A zero-length run is invalid.
        let mut b = header(8, 1, 4, 1, 4, &[]);
        b.extend([0xC0, 1]);
        assert!(decode(&b).is_none());
    }
}
