//! Which placed rasters a swatch can colour. InDesign colours a greyscale or a bitmap (1-bit)
//! image with the swatch applied to it; colour images, and images with alpha or spot channels,
//! keep their own colours.

/// The kind of image a swatch colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    /// Each pixel is a tint of the swatch (its darkness); white is 0%, opaque.
    Gray,
    /// Black pixels take the swatch, white ones are transparent.
    Bitmap,
}

/// The [`Tone`] of an image file, `None` when a swatch doesn't colour it.
pub fn tone(bytes: &[u8]) -> Option<Tone> {
    use image::{ExtendedColorType as E, ImageDecoder, ImageFormat as F};
    if crate::is_psd(bytes) {
        // Header: channels at 12, colour mode at 24 (big-endian). A second channel is alpha or spot.
        let be = |i: usize| Some(u16::from_be_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]));
        return match (be(12)?, be(24)?) {
            (1, 0) => Some(Tone::Bitmap),
            (1, 1) => Some(Tone::Gray),
            _ => None,
        };
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    match reader.format()? {
        F::Png => {
            // `image` expands 1-bit greys to 8 bits: the depth is IHDR's (signature, length, type,
            // width, height, then the bit depth).
            let gray = matches!(reader.into_decoder().ok()?.color_type(), image::ColorType::L8 | image::ColorType::L16);
            let one_bit = *bytes.get(24)? == 1;
            gray.then_some(if one_bit { Tone::Bitmap } else { Tone::Gray })
        }
        F::Tiff | F::Jpeg | F::Bmp => match reader.into_decoder().ok()?.original_color_type() {
            E::L1 => Some(Tone::Bitmap),
            E::L2 | E::L4 | E::L8 | E::L16 => Some(Tone::Gray),
            _ => None,
        },
        // Indexed: what the palette holds decides. Transparent pixels are left out of a bitmap.
        F::Gif => {
            if crate::too_large_to_decode(bytes) {
                return None;
            }
            let img = image::load_from_memory_with_format(bytes, F::Gif).ok()?.into_rgba8();
            let mut opaque = true;
            let mut two_level = true;
            for p in img.pixels() {
                let [r, g, b, a] = p.0;
                match a {
                    0 => opaque = false,
                    255 if r == g && g == b => two_level &= r == 0 || r == 255,
                    _ => return None,
                }
            }
            if two_level { Some(Tone::Bitmap) } else { opaque.then_some(Tone::Gray) }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    fn encode(px: &[u8], w: u32, h: u32, ct: ExtendedColorType, f: image::ImageFormat) -> Vec<u8> {
        let mut b = Vec::new();
        let c = std::io::Cursor::new(&mut b);
        match f {
            image::ImageFormat::Png => image::codecs::png::PngEncoder::new(c).write_image(px, w, h, ct).unwrap(),
            image::ImageFormat::Tiff => image::codecs::tiff::TiffEncoder::new(c).write_image(px, w, h, ct).unwrap(),
            image::ImageFormat::Jpeg => image::codecs::jpeg::JpegEncoder::new(c).write_image(px, w, h, ct).unwrap(),
            image::ImageFormat::Gif => image::codecs::gif::GifEncoder::new(c).encode(px, w, h, ct).unwrap(),
            f => panic!("no encoder for {f:?} here"),
        }
        b
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    /// A 1-bit greyscale PNG, one row of up to 8 pixels (`bits`, MSB first, 1 = white), written
    /// by hand: `image` only writes 8- and 16-bit PNGs.
    fn png_1bit(w: u32, bits: u8) -> Vec<u8> {
        let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]| {
            out.extend(u32::try_from(body.len()).unwrap().to_be_bytes());
            let start = out.len();
            out.extend(kind);
            out.extend(body);
            let crc = crc32(&out[start..]);
            out.extend(crc.to_be_bytes());
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut png, b"IHDR", &[&w.to_be_bytes()[..], &1u32.to_be_bytes(), &[1, 0, 0, 0, 0]].concat());
        // zlib, one stored deflate block holding the filter byte and the row.
        let raw = [0u8, bits];
        let (mut a, mut b) = (1u32, 0u32);
        for &v in &raw {
            a = (a + u32::from(v)) % 65_521;
            b = (b + a) % 65_521;
        }
        let z = [&[0x78, 0x01, 0x01, 2, 0, 0xFD, 0xFF][..], &raw, &((b << 16) | a).to_be_bytes()].concat();
        chunk(&mut png, b"IDAT", &z);
        chunk(&mut png, b"IEND", &[]);
        png
    }

    /// A 1-bit TIFF (BlackIsZero), one row of 8 pixels, uncompressed.
    fn tiff_1bit(bits: u8) -> Vec<u8> {
        let entries: [(u16, u16, u32); 8] = [
            (256, 3, 8), // width
            (257, 3, 1), // height
            (258, 3, 1), // bits per sample
            (259, 3, 1), // no compression
            (262, 3, 1), // BlackIsZero
            (273, 4, 8), // strip offset
            (278, 3, 1), // rows per strip
            (279, 4, 1), // strip byte count
        ];
        let mut t = b"II*\0".to_vec();
        t.extend(10u32.to_le_bytes());
        t.extend([bits, 0]);
        t.extend(u16::try_from(entries.len()).unwrap().to_le_bytes());
        for (tag, kind, value) in entries {
            t.extend(tag.to_le_bytes());
            t.extend(kind.to_le_bytes());
            t.extend(1u32.to_le_bytes());
            if kind == 3 {
                t.extend(u16::try_from(value).unwrap().to_le_bytes());
                t.extend([0, 0]);
            } else {
                t.extend(value.to_le_bytes());
            }
        }
        t.extend(0u32.to_le_bytes());
        t
    }

    /// A PSD header: `channels` channels in colour `mode`, 8 bits deep (1 for bitmap).
    fn psd_header(channels: u16, mode: u16) -> Vec<u8> {
        let mut b = b"8BPS".to_vec();
        b.extend(1u16.to_be_bytes());
        b.extend([0; 6]);
        b.extend(channels.to_be_bytes());
        b.extend(2u32.to_be_bytes());
        b.extend(2u32.to_be_bytes());
        b.extend((if mode == 0 { 1u16 } else { 8 }).to_be_bytes());
        b.extend(mode.to_be_bytes());
        b
    }

    #[test]
    fn greyscale_and_bitmap_files_take_a_swatch() {
        use image::ImageFormat::{Gif, Jpeg, Png, Tiff};
        let grey = [0u8, 128, 255, 64];
        assert_eq!(tone(&encode(&grey, 2, 2, ExtendedColorType::L8, Png)), Some(Tone::Gray));
        assert_eq!(tone(&encode(&grey, 2, 2, ExtendedColorType::L8, Tiff)), Some(Tone::Gray));
        assert_eq!(tone(&encode(&grey, 2, 2, ExtendedColorType::L8, Jpeg)), Some(Tone::Gray));
        let wide: Vec<u8> = grey.iter().flat_map(|&v| (u16::from(v) * 257).to_ne_bytes()).collect();
        assert_eq!(tone(&encode(&wide, 2, 2, ExtendedColorType::L16, Png)), Some(Tone::Gray));
        assert_eq!(tone(&png_1bit(8, 0b1010_0000)), Some(Tone::Bitmap));
        assert_eq!(tone(&tiff_1bit(0b1100_0000)), Some(Tone::Bitmap));
        assert_eq!(tone(&psd_header(1, 1)), Some(Tone::Gray));
        assert_eq!(tone(&psd_header(1, 0)), Some(Tone::Bitmap));

        // GIFs are indexed: black and white only is a bitmap, neutral greys a greyscale image.
        let rgba = |vals: &[[u8; 4]]| vals.concat();
        let bw = rgba(&[[0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 255], [255, 255, 255, 0]]);
        assert_eq!(tone(&encode(&bw, 2, 2, ExtendedColorType::Rgba8, Gif)), Some(Tone::Bitmap));
        let greys = rgba(&[[0, 0, 0, 255], [128, 128, 128, 255], [200, 200, 200, 255], [255, 255, 255, 255]]);
        assert_eq!(tone(&encode(&greys, 2, 2, ExtendedColorType::Rgba8, Gif)), Some(Tone::Gray));
    }

    #[test]
    fn colour_and_alpha_files_keep_their_colours() {
        use image::ImageFormat::{Gif, Png};
        // Neutral pixels in an RGB file: still a colour image.
        let rgb = [0u8, 0, 0, 255, 255, 255, 128, 128, 128, 9, 9, 9];
        assert_eq!(tone(&encode(&rgb, 2, 2, ExtendedColorType::Rgb8, Png)), None);
        // Greyscale with an alpha channel.
        let la = [0u8, 255, 128, 128, 255, 255, 64, 0];
        assert_eq!(tone(&encode(&la, 2, 2, ExtendedColorType::La8, Png)), None);
        assert_eq!(tone(&psd_header(2, 1)), None);
        assert_eq!(tone(&psd_header(3, 3)), None);
        // A GIF with a colour in its palette, and grey levels with transparency.
        let red = [[255u8, 0, 0, 255], [0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 0, 255]].concat();
        assert_eq!(tone(&encode(&red, 2, 2, ExtendedColorType::Rgba8, Gif)), None);
        let see_through = [[128u8, 128, 128, 255], [0, 0, 0, 0], [255, 255, 255, 255], [0, 0, 0, 255]].concat();
        assert_eq!(tone(&encode(&see_through, 2, 2, ExtendedColorType::Rgba8, Gif)), None);
        // Not an image, a truncated header, an SVG.
        for junk in [&b""[..], b"8BPS\0\x01", b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR", b"<svg xmlns='http://www.w3.org/2000/svg'/>"] {
            assert_eq!(tone(junk), None);
        }
    }
}
