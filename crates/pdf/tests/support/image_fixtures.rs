//! Original procedural CMYK TIFFs and JPEGs shared by conversion and public-export contract tests.

pub const INKS: [u8; 8] = [0, 0, 0, 255, 0, 255, 255, 0];
pub const ALPHA: [u8; 2] = [255, 128];

pub fn cmyk_tiff(alpha: Option<u16>, other_inks: bool) -> Vec<u8> {
    use tiff::encoder::colortype::{CMYK8, CMYKA8};
    use tiff::tags::Tag;
    let mut bytes = Vec::new();
    let mut encoder = tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut bytes)).unwrap();
    if let Some(kind) = alpha {
        let mut image = encoder.new_image::<CMYKA8>(2, 1).unwrap();
        image.encoder().write_tag(Tag::ExtraSamples, &[kind][..]).unwrap();
        image.write_data(&[0, 0, 0, 255, ALPHA[0], 0, 255, 255, 0, ALPHA[1]]).unwrap();
    } else {
        let mut image = encoder.new_image::<CMYK8>(2, 1).unwrap();
        if other_inks {
            image.encoder().write_tag(Tag::Unknown(332), 2u16).unwrap();
        }
        image.write_data(&INKS).unwrap();
    }
    bytes
}

/// An 8×8 baseline JPEG of four channels, each one flat `values` (as stored), with Adobe's
/// APP14 marker when `adobe` is a transform (0 = CMYK stored inverted).
#[allow(dead_code, reason = "the unit tests use it; the integration tests share this file")]
pub fn cmyk_jpeg(values: [u8; 4], adobe: Option<u8>) -> Vec<u8> {
    fn seg(j: &mut Vec<u8>, marker: u8, body: &[u8]) {
        j.extend([0xFF, marker]);
        j.extend(u16::try_from(body.len() + 2).unwrap().to_be_bytes());
        j.extend(body);
    }
    const DC_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
    let mut j = vec![0xFF, 0xD8];
    if let Some(t) = adobe {
        seg(&mut j, 0xEE, &[b"Adobe".as_slice(), &[0, 100, 0, 0, 0, 0, t]].concat());
    }
    seg(&mut j, 0xDB, &[[0u8].as_slice(), &[1u8; 64]].concat());
    seg(&mut j, 0xC0, &[8, 0, 8, 0, 8, 4, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 4, 0x11, 0]);
    seg(&mut j, 0xC4, &[[0x00u8].as_slice(), &DC_BITS, &(0u8..12).collect::<Vec<_>>()].concat());
    let mut ac_bits = [0u8; 16];
    ac_bits[0] = 1; // one code, "0": end of block
    seg(&mut j, 0xC4, &[[0x10u8].as_slice(), &ac_bits, &[0x00]].concat());
    seg(&mut j, 0xDA, &[4, 1, 0, 2, 0, 3, 0, 4, 0, 0, 63, 0]);
    // Canonical DC codes for categories 0..11.
    let mut codes = Vec::new();
    let mut code = 0u32;
    for (len, &n) in (1..=16u32).zip(&DC_BITS) {
        for _ in 0..n {
            codes.push((code, len));
            code += 1;
        }
        code <<= 1;
    }
    let mut bits: Vec<bool> = Vec::new();
    let mut put = |v: u32, n: u32| bits.extend((0..n).rev().map(|i| v >> i & 1 == 1));
    for v in values {
        // DC of a flat block is 8 × the level-shifted sample (quantiser 1).
        let diff = (i32::from(v) - 128) * 8;
        let cat = 32 - diff.unsigned_abs().leading_zeros();
        let (c, len) = codes[cat as usize];
        put(c, len);
        let extra = if diff >= 0 { diff } else { diff - 1 } & ((1 << cat) - 1);
        put(extra as u32, cat);
        put(0, 1);
    }
    while !bits.len().is_multiple_of(8) {
        bits.push(true);
    }
    for byte in bits.chunks(8).map(|b| b.iter().fold(0u8, |acc, &x| acc << 1 | u8::from(x))) {
        j.push(byte);
        if byte == 0xFF {
            j.push(0);
        }
    }
    j.extend([0xFF, 0xD9]);
    j
}
