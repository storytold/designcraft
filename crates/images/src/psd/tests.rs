use crate::CmykRaster;
use crate::colour::{DisplayColour, Pixels, to_display_rgba};
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
    /// An ICC profile resource (1039) when not empty.
    icc: Vec<u8>,
}

impl Psd {
    fn new(mode: u16, channels: Vec<Vec<u8>>) -> Self {
        Psd { version: 1, mode, depth: 8, w: W, h: H, channels, palette: Vec::new(), layers: Layers::None, rle: false, icc: Vec::new() }
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
        if !self.icc.is_empty() {
            res.extend(b"8BIM");
            res.extend(1039u16.to_be_bytes());
            res.extend([0, 0]);
            res.extend((self.icc.len() as u32).to_be_bytes());
            res.extend(&self.icc);
            if self.icc.len() % 2 == 1 {
                res.push(0);
            }
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
    // Through the working CMYK space, like CMYK JPEGs and TIFFs.
    let expect = |cmyk: [u8; 4]| {
        let raster = CmykRaster { width: 1, height: 1, cmyk: cmyk.to_vec(), alpha: None };
        to_display_rgba(Pixels::Cmyk(raster), None, &DisplayColour::active()).unwrap().get_pixel(0, 0).0
    };
    for rle in [false, true] {
        // An extra alpha channel (5 channels) doesn't become the alpha.
        let mut psd = Psd::new(4, vec![c.clone(), m.clone(), y.clone(), k.clone(), sel()]);
        psd.rle = rle;
        let px = pixels(&psd.write());
        let cyan = px[at(0, 0)];
        assert_eq!(cyan, expect([255, 0, 0, 0]));
        assert!(cyan[1] < 200 && cyan[2] < 250, "press cyan, not (0, 255, 255): {cyan:?}");
        assert_eq!(px[at(11, 7)], expect([0, 0, 0, 128]));
        assert!(px.iter().all(|p| p[3] == 255));
    }
}

#[test]
fn embedded_profile_converts_rgb_to_srgb() {
    let r = plane(64, None);
    let g = plane(192, None);
    let b = plane(64, None);
    let plain = pixels(&Psd::new(3, vec![r.clone(), g.clone(), b.clone()]).write());
    assert_eq!(plain[0], [64, 192, 64, 255]);
    let mut psd = Psd::new(3, vec![r, g, b]);
    psd.icc = craft_cms::Builtin::AdobeRgbCompat.profile().to_bytes().to_vec();
    let px = pixels(&psd.write());
    // Adobe RGB green is greener than sRGB's: less red once shown as sRGB.
    assert!(px[0][0] < 50 && px[0][1] > 180 && px[0][3] == 255, "{:?}", px[0]);
    // A damaged profile is ignored.
    psd.icc.truncate(100);
    assert_eq!(pixels(&psd.write())[0], [64, 192, 64, 255]);
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
