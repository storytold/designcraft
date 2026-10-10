//! Photoshop documents (.psd, and .psb large documents): the merged composite in the image data
//! section, decoded by its colour mode per the public file format specification. Extra alpha and
//! spot channels don't change the placed image; when the layer count is negative, the first extra
//! channel is the composite's merged transparency.

use designcraft_color::Color;
use designcraft_color::cms::{Lab, lab::lab_to_srgb};

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
    let depth_ok = match mode {
        Mode::Bitmap => depth == 1,
        Mode::Indexed => depth == 8,
        _ => depth == 8 || depth == 16,
    };
    let colours = mode.colour_channels();
    if !(1..=56).contains(&channels) || channels < colours || !(1..=max_side).contains(&w) || !(1..=max_side).contains(&h) || !depth_ok {
        return None;
    }
    let (wu, hu) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    let pixels = wu.checked_mul(hu)?;
    let (stride, len) = (wu.checked_mul(4)?, pixels.checked_mul(4)?);
    if len > MAX_RGBA_BYTES {
        return None;
    }

    let colour_mode_data = r.block(false)?;
    r.block(false)?; // image resources
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
            Mode::Cmyk => {
                // Stored inverted: 255 is no ink.
                let ink = |v: f32| 1.0 - v / 255.0;
                let [r, g, b] = Color::cmyk(ink(v0), ink(v1), ink(v2), ink(v3)).to_rgb_uncalibrated();
                [q(r), q(g), q(b), a]
            }
            Mode::Lab => {
                let [r, g, b] = lab_to_srgb(Lab::new(v0 * 100.0 / 255.0, v1 - 128.0, v2 - 128.0));
                [q(r), q(g), q(b), a]
            }
        };
    }
    image::RgbaImage::from_raw(w, h, rgba)
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
        let mut at = counts.by_ref().take(ch.checked_mul(self.height)?).try_fold(0usize, |at, n| at.checked_add(n))?;
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
mod tests {
    use crate::decode_rgba;

    const W: u32 = 12;
    const H: u32 = 8;

    /// A `W`×`H` 8-bit plane: `bg` everywhere, `val` in the square `[x0, x1)×[y0, y1)`.
    fn plane(bg: u8, square: Option<(u32, u32, u32, u32, u8)>) -> Vec<u8> {
        let mut p = vec![bg; (W * H) as usize];
        if let Some((x0, y0, x1, y1, val)) = square {
            for y in y0..y1 {
                for x in x0..x1 {
                    p[(y * W + x) as usize] = val;
                }
            }
        }
        p
    }
    /// Mid grey with a dark square in the middle.
    fn gray() -> Vec<u8> {
        plane(150, Some((4, 2, 8, 6, 60)))
    }
    /// An alpha channel (a saved selection): a square of 255 top-left.
    fn sel() -> Vec<u8> {
        plane(0, Some((1, 1, 4, 4, 255)))
    }
    /// A spot channel: a square of 255 bottom-right.
    fn spot() -> Vec<u8> {
        plane(0, Some((8, 5, 11, 7, 255)))
    }

    enum Layers {
        /// An empty layer and mask section.
        None,
        /// Layer info with this layer count and one layer record.
        Count(i16),
        /// Empty layer info; the layers in an `Lr16` tagged block (16-bit documents).
        Lr16(i16),
    }

    /// A Photoshop file for the tests, written per the public file format spec.
    struct Psd {
        /// 1 = PSD, 2 = PSB.
        version: u16,
        mode: u16,
        depth: u16,
        w: u32,
        h: u32,
        /// Composite channels: rows of big-endian samples.
        channels: Vec<Vec<u8>>,
        palette: Vec<u8>,
        layers: Layers,
        rle: bool,
    }

    impl Psd {
        fn new(mode: u16, channels: Vec<Vec<u8>>) -> Self {
            Psd { version: 1, mode, depth: 8, w: W, h: H, channels, palette: Vec::new(), layers: Layers::None, rle: false }
        }
        fn rle(self) -> Self {
            Psd { rle: true, ..self }
        }
        fn layers(self, layers: Layers) -> Self {
            Psd { layers, ..self }
        }
        fn psb(&self) -> bool {
            self.version == 2
        }
        /// A section length: 4 bytes, 8 in PSB.
        fn len(&self, n: usize) -> Vec<u8> {
            if self.psb() { (n as u64).to_be_bytes().to_vec() } else { (n as u32).to_be_bytes().to_vec() }
        }
        fn row_bytes(&self) -> usize {
            (self.w as usize * self.depth as usize).div_ceil(8)
        }
        /// Compression + channel data, as in the image data section.
        fn channel_data(&self, planes: &[Vec<u8>], rle: bool) -> Vec<u8> {
            let mut b = Vec::new();
            if !rle {
                b.extend(0u16.to_be_bytes());
                planes.iter().for_each(|p| b.extend(p));
                return b;
            }
            b.extend(1u16.to_be_bytes());
            let rows: Vec<Vec<u8>> = planes.iter().flat_map(|p| p.chunks(self.row_bytes()).map(packbits)).collect();
            for r in &rows {
                if self.psb() { b.extend((r.len() as u32).to_be_bytes()) } else { b.extend((r.len() as u16).to_be_bytes()) }
            }
            rows.iter().for_each(|r| b.extend(r));
            b
        }
        /// Layer info: the count, one full-canvas layer (transparency + first channel), its data.
        fn layer_info(&self, count: i16) -> Vec<u8> {
            let plane_len = self.row_bytes() * self.h as usize;
            let mut b = Vec::new();
            b.extend(count.to_be_bytes());
            for v in [0, 0, self.h as i32, self.w as i32] {
                b.extend(v.to_be_bytes());
            }
            b.extend(2u16.to_be_bytes());
            for id in [-1i16, 0] {
                b.extend(id.to_be_bytes());
                b.extend(self.len(2 + plane_len));
            }
            b.extend(b"8BIMnorm");
            b.extend([255, 0, 0, 0]);
            let mut extra = Vec::new();
            extra.extend(0u32.to_be_bytes()); // layer mask data
            extra.extend(0u32.to_be_bytes()); // blending ranges
            extra.extend([7, b'L', b'a', b'y', b'e', b'r', b' ', b'1']); // name, padded to 4
            b.extend((extra.len() as u32).to_be_bytes());
            b.extend(extra);
            let first = self.channels.first().cloned().unwrap_or_default();
            b.extend(self.channel_data(&[vec![255; plane_len]], false));
            b.extend(self.channel_data(&[first], false));
            if b.len() % 2 == 1 {
                b.push(0);
            }
            b
        }
        fn layer_and_mask(&self) -> Vec<u8> {
            let mut s = Vec::new();
            match self.layers {
                Layers::None => return s,
                Layers::Count(n) => {
                    let info = self.layer_info(n);
                    s.extend(self.len(info.len()));
                    s.extend(info);
                    s.extend(0u32.to_be_bytes()); // global layer mask info
                }
                Layers::Lr16(n) => {
                    s.extend(self.len(0));
                    s.extend(0u32.to_be_bytes());
                    // An unrelated tagged block first (4-byte length, padded to 4).
                    s.extend(b"8BIMPatt");
                    s.extend(3u32.to_be_bytes());
                    s.extend([1, 2, 3, 0]);
                    let mut info = self.layer_info(n);
                    while !info.len().is_multiple_of(4) {
                        info.push(0);
                    }
                    s.extend(b"8BIMLr16");
                    s.extend(self.len(info.len()));
                    s.extend(info);
                }
            }
            s
        }
        fn write(&self) -> Vec<u8> {
            let mut b = Vec::new();
            b.extend(b"8BPS");
            b.extend(self.version.to_be_bytes());
            b.extend([0u8; 6]);
            b.extend((self.channels.len() as u16).to_be_bytes());
            b.extend(self.h.to_be_bytes());
            b.extend(self.w.to_be_bytes());
            b.extend(self.depth.to_be_bytes());
            b.extend(self.mode.to_be_bytes());
            b.extend((self.palette.len() as u32).to_be_bytes());
            b.extend(&self.palette);
            // Image resources: ResolutionInfo (1005), 72 ppi.
            let mut res = Vec::new();
            res.extend(b"8BIM");
            res.extend(1005u16.to_be_bytes());
            res.extend([0, 0]);
            res.extend(16u32.to_be_bytes());
            for v in [72u32 << 16, 0x0001_0001, 72 << 16, 0x0001_0001] {
                res.extend(v.to_be_bytes());
            }
            b.extend((res.len() as u32).to_be_bytes());
            b.extend(res);
            let lm = self.layer_and_mask();
            b.extend(self.len(lm.len()));
            b.extend(lm);
            b.extend(self.channel_data(&self.channels, self.rle));
            b
        }
    }

    /// PackBits: repeat runs of 2+ bytes, literals otherwise.
    fn packbits(row: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < row.len() {
            let run = row[i..].iter().take(128).take_while(|&&v| v == row[i]).count();
            if run >= 2 {
                out.push((1 - run as i16) as i8 as u8);
                out.push(row[i]);
                i += run;
                continue;
            }
            let start = i;
            while i < row.len() && i - start < 128 && !(i + 1 < row.len() && row[i + 1] == row[i]) {
                i += 1;
            }
            out.push((i - start - 1) as u8);
            out.extend(&row[start..i]);
        }
        out
    }

    fn pixels(b: &[u8]) -> Vec<[u8; 4]> {
        let img = decode_rgba(b).expect("decodes");
        assert_eq!(img.dimensions(), (W, H));
        img.pixels().map(|p| p.0).collect()
    }

    fn at(x: u32, y: u32) -> usize {
        (y * W + x) as usize
    }

    #[test]
    fn grayscale_extra_channels_leave_the_gray_composite() {
        let extras = [sel(), spot(), sel(), plane(255, None)];
        for n in 0..=extras.len() {
            for rle in [false, true] {
                for layered in [false, true] {
                    let mut ch = vec![gray()];
                    ch.extend(extras[..n].iter().cloned());
                    let mut psd = Psd::new(1, ch);
                    psd.rle = rle;
                    if layered {
                        psd.layers = Layers::Count(1);
                    }
                    let want: Vec<[u8; 4]> = gray().iter().map(|&g| [g, g, g, 255]).collect();
                    assert_eq!(pixels(&psd.write()), want, "gray + {n} extra channels, rle {rle}, layered {layered}");
                }
            }
        }
    }

    #[test]
    fn negative_layer_count_makes_the_first_extra_channel_the_transparency() {
        let hole = plane(255, Some((2, 3, 5, 6, 0)));
        for rle in [false, true] {
            for extras in [vec![], vec![spot()], vec![spot(), sel(), sel()]] {
                let mut ch = vec![gray(), hole.clone()];
                ch.extend(extras.iter().cloned());
                let mut psd = Psd::new(1, ch).layers(Layers::Count(-1));
                psd.rle = rle;
                let px = pixels(&psd.write());
                for (i, p) in px.iter().enumerate() {
                    if hole[i] == 0 {
                        assert_eq!(p[3], 0, "pixel {i} is in the hole");
                    } else {
                        let g = gray()[i];
                        assert_eq!(*p, [g, g, g, 255], "pixel {i}, rle {rle}, {} extras", extras.len());
                    }
                }
            }
        }
    }

    #[test]
    fn rgb_with_a_spot_channel_is_opaque_rgb() {
        let (r, g, b) = (plane(200, None), plane(10, Some((0, 0, 6, 8, 90))), plane(30, None));
        for rle in [false, true] {
            let mut psd = Psd::new(3, vec![r.clone(), g.clone(), b.clone(), spot()]);
            psd.rle = rle;
            let px = pixels(&psd.write());
            assert_eq!(px[at(0, 0)], [200, 90, 30, 255]);
            assert_eq!(px[at(9, 6)], [200, 10, 30, 255], "the spot square doesn't hide the image");
            assert!(px.iter().all(|p| p[3] == 255));
        }
    }

    #[test]
    fn rgb_with_merged_transparency_is_rgba_unmatted_from_white() {
        // Straight red at alpha 128, stored matted over white: 0 → 255 − 128 = 127.
        let r = plane(255, None);
        let gb = plane(0, Some((0, 0, 1, 1, 127)));
        let a = plane(255, Some((0, 0, 1, 1, 128)));
        let psd = Psd::new(3, vec![r, gb.clone(), gb, a, sel()]).layers(Layers::Count(-1)).rle();
        let px = pixels(&psd.write());
        assert_eq!(px[at(0, 0)], [255, 0, 0, 128]);
        assert_eq!(px[at(5, 5)], [255, 0, 0, 255]);
        assert_eq!(px[at(2, 2)][3], 255, "the alpha selection after the transparency is ignored");
    }

    #[test]
    fn cmyk_converts_like_other_cmyk_images() {
        // Stored inverted: 255 = no ink. Cyan 100 % on the left, 50 % black on the right.
        let c = plane(255, Some((0, 0, 6, 8, 0)));
        let m = plane(255, None);
        let y = plane(255, None);
        let k = plane(255, Some((6, 0, 12, 8, 127)));
        let rgb8 = |c: f32, m: f32, y: f32, k: f32| {
            let [r, g, b] = designcraft_color::Color::cmyk(c, m, y, k).to_rgb_uncalibrated();
            [r, g, b].map(|v| (v * 255.0).round() as u8)
        };
        for rle in [false, true] {
            // An extra alpha channel (5 channels) doesn't become the alpha.
            let mut psd = Psd::new(4, vec![c.clone(), m.clone(), y.clone(), k.clone(), sel()]);
            psd.rle = rle;
            let px = pixels(&psd.write());
            let [r, g, b] = rgb8(1.0, 0.0, 0.0, 0.0);
            assert_eq!(px[at(0, 0)], [r, g, b, 255]);
            let [r, g, b] = rgb8(0.0, 0.0, 0.0, 128.0 / 255.0);
            assert_eq!(px[at(11, 7)], [r, g, b, 255]);
            assert!(px.iter().all(|p| p[3] == 255));
        }
    }

    /// 8-bit plane → 16-bit big-endian samples (v × 257).
    fn wide(p: &[u8]) -> Vec<u8> {
        p.iter().flat_map(|&v| (v as u16 * 257).to_be_bytes()).collect()
    }

    #[test]
    fn sixteen_bit_grayscale() {
        for rle in [false, true] {
            let mut psd = Psd::new(1, vec![wide(&gray()), wide(&sel()), wide(&spot())]);
            psd.depth = 16;
            psd.rle = rle;
            let want: Vec<[u8; 4]> = gray().iter().map(|&g| [g, g, g, 255]).collect();
            assert_eq!(pixels(&psd.write()), want);
        }
    }

    #[test]
    fn sixteen_bit_transparency_from_the_lr16_block() {
        let hole = plane(255, Some((2, 3, 5, 6, 0)));
        let mut psd = Psd::new(1, vec![wide(&gray()), wide(&hole), wide(&spot())]).layers(Layers::Lr16(-1));
        psd.depth = 16;
        let px = pixels(&psd.write());
        assert_eq!(px[at(3, 4)][3], 0);
        assert_eq!(px[at(9, 6)], [150, 150, 150, 255]);
    }

    #[test]
    fn psb_with_rle_and_transparency() {
        let hole = plane(255, Some((2, 3, 5, 6, 0)));
        let mut psd = Psd::new(1, vec![gray(), hole, sel()]).layers(Layers::Count(-1)).rle();
        psd.version = 2;
        let px = pixels(&psd.write());
        assert_eq!(px[at(3, 4)][3], 0);
        assert_eq!(px[at(2, 2)], [150, 150, 150, 255]);
        assert_eq!(px[at(5, 3)], [60, 60, 60, 255]);
    }

    #[test]
    fn indexed_uses_the_palette() {
        let mut palette = vec![0u8; 768];
        palette[3] = 10; // index 3: R
        palette[256 + 3] = 20; // G
        palette[512 + 3] = 30; // B
        palette[7] = 255; // index 7: red
        let mut psd = Psd::new(2, vec![plane(3, Some((0, 0, 1, 1, 7)))]);
        psd.palette = palette;
        let px = pixels(&psd.write());
        assert_eq!(px[at(0, 0)], [255, 0, 0, 255]);
        assert_eq!(px[at(1, 0)], [10, 20, 30, 255]);
    }

    #[test]
    fn bitmap_set_bits_are_black() {
        // 12 px wide: 2 bytes per row. The first pixel of each row is black.
        let mut psd = Psd::new(0, vec![[0x80u8, 0x00].repeat(H as usize)]);
        psd.depth = 1;
        let px = pixels(&psd.write());
        assert_eq!(px[at(0, 3)], [0, 0, 0, 255]);
        assert_eq!(px[at(1, 3)], [255, 255, 255, 255]);
        assert_eq!(px[at(11, 3)], [255, 255, 255, 255]);
    }

    #[test]
    fn lab_white_and_black() {
        let l = plane(255, Some((0, 0, 1, 1, 0)));
        let ab = plane(128, None);
        let psd = Psd::new(9, vec![l, ab.clone(), ab, sel()]);
        let px = pixels(&psd.write());
        assert_eq!(px[at(0, 0)], [0, 0, 0, 255]);
        for v in &px[at(5, 5)][..3] {
            assert!(*v >= 254, "L 100 is white: {:?}", px[at(5, 5)]);
        }
    }

    fn valid_files() -> Vec<Vec<u8>> {
        let hole = plane(255, Some((2, 3, 5, 6, 0)));
        let mut wide16 = Psd::new(1, vec![wide(&gray()), wide(&hole)]).layers(Layers::Lr16(-1)).rle();
        wide16.depth = 16;
        let mut psb = Psd::new(3, vec![gray(), gray(), gray(), hole.clone()]).layers(Layers::Count(-1)).rle();
        psb.version = 2;
        vec![
            Psd::new(1, vec![gray(), sel(), spot()]).write(),
            Psd::new(1, vec![gray(), hole.clone(), spot()]).layers(Layers::Count(-1)).rle().write(),
            Psd::new(4, vec![gray(), gray(), gray(), gray()]).rle().write(),
            wide16.write(),
            psb.write(),
        ]
    }

    #[test]
    fn truncated_files_never_panic() {
        for file in valid_files() {
            let full = decode_rgba(&file).expect("the full file decodes");
            for n in 0..file.len() {
                if let Some(img) = decode_rgba(&file[..n]) {
                    assert_eq!(img, full, "a truncated file decodes only when the composite survived");
                }
            }
        }
    }

    #[test]
    fn corrupted_files_never_panic() {
        // A small LCG: deterministic byte flips across every valid file.
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 8
        };
        for file in valid_files() {
            for _ in 0..2000 {
                let mut b = file.clone();
                for _ in 0..1 + next() % 4 {
                    let i = 4 + next() as usize % (b.len() - 4);
                    b[i] = next() as u8;
                }
                let _ = decode_rgba(&b);
            }
        }
    }

    #[test]
    fn hostile_headers_are_rejected() {
        let ok = Psd::new(1, vec![gray()]).write();
        let set = |at: usize, v: &[u8]| {
            let mut b = ok.clone();
            b[at..at + v.len()].copy_from_slice(v);
            b
        };
        assert!(decode_rgba(&ok).is_some());
        assert!(decode_rgba(&set(4, &3u16.to_be_bytes())).is_none(), "version 3");
        assert!(decode_rgba(&set(12, &0u16.to_be_bytes())).is_none(), "no channels");
        assert!(decode_rgba(&set(12, &57u16.to_be_bytes())).is_none(), "57 channels");
        assert!(decode_rgba(&set(14, &0u32.to_be_bytes())).is_none(), "zero height");
        assert!(decode_rgba(&set(18, &0u32.to_be_bytes())).is_none(), "zero width");
        assert!(decode_rgba(&set(18, &30_001u32.to_be_bytes())).is_none(), "wider than a PSD can be");
        assert!(decode_rgba(&set(14, &u32::MAX.to_be_bytes())).is_none(), "absurd height");
        assert!(decode_rgba(&set(22, &32u16.to_be_bytes())).is_none(), "32-bit");
        assert!(decode_rgba(&set(22, &7u16.to_be_bytes())).is_none(), "7-bit");
        assert!(decode_rgba(&set(24, &7u16.to_be_bytes())).is_none(), "multichannel");
        // Huge but within the side limit: refused before allocating.
        let mut big = set(14, &30_000u32.to_be_bytes());
        big[18..22].copy_from_slice(&30_000u32.to_be_bytes());
        assert!(decode_rgba(&big).is_none(), "too many pixels");
        // RGB with only 2 channels; indexed without a palette.
        assert!(decode_rgba(&Psd::new(3, vec![gray(), gray()]).write()).is_none());
        assert!(decode_rgba(&Psd::new(2, vec![gray()]).write()).is_none());
    }

    #[test]
    fn bad_compression_is_rejected() {
        let ok = Psd::new(1, vec![gray()]).rle().write();
        assert!(decode_rgba(&ok).is_some());
        let comp = ok.len() - Psd::new(1, vec![gray()]).rle().channel_data(&[gray()], true).len();
        for method in [2u16, 3, 4] {
            let mut b = ok.clone();
            b[comp..comp + 2].copy_from_slice(&method.to_be_bytes());
            assert!(decode_rgba(&b).is_none(), "compression {method}");
        }
        // A row count past the end of the data.
        let mut b = ok.clone();
        b[comp + 2..comp + 4].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(decode_rgba(&b).is_none());
        // A row that unpacks to the wrong width: a literal of 1 byte instead of a run of 12.
        let mut rows = Vec::new();
        for _ in 0..H {
            rows.push(vec![0u8, 150]);
        }
        let mut b = ok[..comp].to_vec();
        b.extend(1u16.to_be_bytes());
        rows.iter().for_each(|r| b.extend((r.len() as u16).to_be_bytes()));
        rows.iter().for_each(|r| b.extend(r));
        assert!(decode_rgba(&b).is_none());
    }

    #[test]
    fn packbits_round_trips() {
        let row = [1u8, 1, 1, 2, 3, 4, 4, 5, 6, 6, 6, 6, 7];
        assert_eq!(super::unpack_bits(&packbits(&row), row.len()), Some(row.to_vec()));
        // -128 is a no-op.
        assert_eq!(super::unpack_bits(&[0x80, 0x01, 9, 8], 2), Some(vec![9, 8]));
        assert_eq!(super::unpack_bits(&[0x05, 1, 2], 6), None, "literal past the end of the data");
        assert_eq!(super::unpack_bits(&[0xFE, 7], 2), None, "run longer than the row");
    }

    #[test]
    fn huge_header_without_data_is_rejected_before_allocating() {
        // A gray 11000×11000 image (≈ 460 MiB as RGBA, under the cap) with almost no image data.
        let (w, h) = (11_000u32, 11_000u32);
        let mut head = Vec::new();
        head.extend(b"8BPS");
        head.extend(1u16.to_be_bytes());
        head.extend([0u8; 6]);
        head.extend(1u16.to_be_bytes()); // channels
        head.extend(h.to_be_bytes());
        head.extend(w.to_be_bytes());
        head.extend(8u16.to_be_bytes());
        head.extend(1u16.to_be_bytes()); // grayscale
        head.extend([0u8; 12]); // empty colour mode data, image resources, layers
        let holds = |b: &[u8]| {
            let r = super::Reader { b: &b[head.len()..] };
            super::ImageData::parse(r, false, 1, w as usize, h as usize).expect("parses").holds(1)
        };
        // Raw: a few bytes instead of 11000 × 11000.
        let mut raw = head.clone();
        raw.extend(0u16.to_be_bytes());
        raw.extend([0u8; 16]);
        assert!(holds(&raw).is_none());
        assert!(super::decode(&raw).is_none());
        // RLE: every row count present and plausible, but the rows themselves missing.
        let mut rle = head.clone();
        rle.extend(1u16.to_be_bytes());
        for _ in 0..h {
            rle.extend(174u16.to_be_bytes()); // 2 × ⌈11000 / 128⌉: the shortest full row
        }
        assert!(holds(&rle).is_none());
        assert!(super::decode(&rle).is_none());
        // RLE row counts too short to unpack to a full row, even with the data present.
        let mut short = head.clone();
        short.extend(1u16.to_be_bytes());
        for _ in 0..h {
            short.extend(2u16.to_be_bytes());
        }
        short.extend(vec![0u8; 2 * h as usize]);
        assert!(holds(&short).is_none());
        assert!(super::decode(&short).is_none());
    }
}
