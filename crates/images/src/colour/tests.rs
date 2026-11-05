use designcraft_color::cms::{self, Cms, ColorSettings};
use image::{ImageEncoder, Rgba};

use super::*;
use crate::decode_rgba;

/// Generic CMYK as the working space.
fn generic(bpc: bool) -> Cms {
    Cms::new(&ColorSettings { cmyk: cms::GENERIC_CMYK.into(), bpc, ..ColorSettings::default() }).unwrap()
}

/// sRGB 0..255 of CMYK inks through the Generic CMYK model (relative colorimetric, no BPC).
fn generic_rgb(cmyk: [f32; 4]) -> [f32; 3] {
    generic(false).cmyk_to_srgb(cmyk, false).map(|v| v * 255.0)
}

/// The default colour settings without black point compensation.
fn no_bpc() -> DisplayColour {
    DisplayColour::of(&Cms::new(&ColorSettings { bpc: false, ..ColorSettings::default() }).unwrap())
}

fn icc(b: Builtin) -> Vec<u8> {
    b.profile().to_bytes().to_vec()
}

/// One CMYK pixel (inks 0..255) as shown with `display`.
fn shown(cmyk: [u8; 4], display: &DisplayColour) -> [u8; 4] {
    let raster = CmykRaster { width: 1, height: 1, cmyk: cmyk.to_vec(), alpha: None };
    to_display_rgba(Pixels::Cmyk(raster), None, display).unwrap().get_pixel(0, 0).0
}

fn near(p: [u8; 4], want: [f32; 3], tol: f32) -> bool {
    p.iter().zip(want).all(|(&a, b)| (f32::from(a) - b).abs() <= tol)
}

/// A baseline JPEG, one 8×8 block per component of four, every sample of component `i` equal to
/// `values[i]`; with an Adobe APP14 marker carrying `adobe` (its colour transform) when given.
fn solid_jpeg(values: [u8; 4], adobe: Option<u8>) -> Vec<u8> {
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

/// A PNG of `px` (RGBA) with `icc` embedded.
fn png_with_icc(w: u32, h: u32, px: &[u8], icc: Option<Vec<u8>>) -> Vec<u8> {
    let mut b = Vec::new();
    let mut enc = image::codecs::png::PngEncoder::new(&mut b);
    if let Some(icc) = icc {
        enc.set_icc_profile(icc).unwrap();
    }
    enc.write_image(px, w, h, image::ExtendedColorType::Rgba8).unwrap();
    b
}

fn adobe_rgb_icc() -> Vec<u8> {
    icc(Builtin::AdobeRgbCompat)
}

/// sRGB 0..255 of an Adobe RGB colour through the colour crate's float transform.
fn adobe_to_srgb(rgb: [u8; 3]) -> [f32; 3] {
    let cms = Cms::new(&ColorSettings { rgb: cms::ADOBE_RGB.into(), ..ColorSettings::default() }).unwrap();
    cms.rgb_to_srgb(rgb.map(|v| f32::from(v) / 255.0)).map(|v| v * 255.0)
}

#[test]
fn cmyk_jpeg_goes_through_the_working_cmyk_space() {
    // Adobe CMYK is stored inverted: cyan 100 % is C 0, M Y K 255.
    let jpeg = solid_jpeg([0, 255, 255, 255], Some(0));
    let p = decode_rgba(&jpeg).unwrap().get_pixel(3, 3).0;
    let cyan = shown([255, 0, 0, 0], &DisplayColour::active());
    assert_eq!(p, cyan);
    // Not the profile-free (0, 255, 255): press cyan is a duller, darker blue.
    assert!(p[1] < 200 && p[2] < 250 && p[3] == 255, "{p:?}");

    // YCCK (Adobe transform 2) of the same inks: YCbCr of (255, 0, 0), K stored 255.
    let ycck = solid_jpeg([76, 85, 255, 255], Some(2));
    let q = decode_rgba(&ycck).unwrap().get_pixel(0, 0).0;
    let want = cyan.map(f32::from);
    assert!(near(q, [want[0], want[1], want[2]], 6.0), "{q:?} vs {cyan:?}");

    // Without the Adobe marker the values are ink amounts as stored.
    let plain = solid_jpeg([255, 0, 0, 0], None);
    assert_eq!(decode_rgba(&plain).unwrap().get_pixel(7, 7).0, cyan);
}

#[test]
fn cmyk_without_a_profile_shows_as_the_default_swatches_do() {
    // Default settings: Coated FOGRA39, relative colorimetric with black point compensation.
    let display = DisplayColour::of(&Cms::default());
    for (inks, want) in [
        ([0, 50, 100, 0], [243.0, 146.0, 0.0]),
        ([0, 60, 100, 0], [239.0, 123.0, 0.0]),
        ([0, 0, 0, 100], [28.0, 28.0, 27.0]),
        ([100, 100, 0, 0], [48.0, 38.0, 131.0]),
    ] {
        let inks = inks.map(|v: u16| u8::try_from((v * 255 + 50) / 100).unwrap());
        let p = shown(inks, &display);
        let swatch = Cms::default().display_rgb(&Color::cmyk(
            f32::from(inks[0]) / 255.0,
            f32::from(inks[1]) / 255.0,
            f32::from(inks[2]) / 255.0,
            f32::from(inks[3]) / 255.0,
        ));
        assert!(near(p, want, 2.0), "{inks:?}: {p:?} vs {want:?}");
        assert!(near(p, swatch.map(|v| v * 255.0), 2.0), "{inks:?}: {p:?} vs swatch {swatch:?}");
        // A CMYK JPEG without a profile (and without an Adobe marker: inks as stored) too.
        assert_eq!(decode_rgba(&solid_jpeg(inks, None)).unwrap().get_pixel(0, 0).0, shown(inks, &DisplayColour::active()));
    }
}

#[test]
fn working_cmyk_follows_the_model_and_black_point_compensation() {
    // Generic CMYK's ICC table agrees with the model itself.
    for inks in [[255, 0, 0, 0], [0, 255, 0, 0], [0, 0, 255, 0], [0, 0, 0, 255], [64, 32, 160, 20]] {
        let p = shown(inks, &DisplayColour::of(&generic(false)));
        let want = generic_rgb(inks.map(|v| f32::from(v) / 255.0));
        // Within the 9-point table and 8-bit rounding.
        assert!(near(p, want, 6.0), "{inks:?}: {p:?} vs {want:?}");
    }
    // With black point compensation the press's darkest black is display black, paper stays
    // white, and solid K is darker than without it.
    let with = DisplayColour::active();
    let rich = shown([255, 255, 255, 255], &with);
    assert!(rich[..3].iter().all(|&v| v <= 3), "{rich:?}");
    assert_eq!(shown([0, 0, 0, 0], &with), shown([0, 0, 0, 0], &no_bpc()));
    let (k, k_plain) = (shown([0, 0, 0, 255], &with), shown([0, 0, 0, 255], &no_bpc()));
    assert!(k[1] < k_plain[1], "{k:?} vs {k_plain:?}");
}

#[test]
fn cmyk_uses_its_own_profile_before_the_working_space() {
    let raster = || CmykRaster { width: 2, height: 1, cmyk: vec![255, 0, 0, 0, 0, 0, 0, 255], alpha: Some(vec![255, 128]) };
    let device = DisplayColour::of(&Cms::new(&ColorSettings { cmyk: cms::DEVICE_CMYK.into(), ..ColorSettings::default() }).unwrap());
    // Device CMYK working space: the profile-free formula.
    let img = to_display_rgba(Pixels::Cmyk(raster()), None, &device).unwrap();
    assert_eq!(img.get_pixel(0, 0).0, [0, 255, 255, 255]);
    assert_eq!(img.get_pixel(1, 0).0, [0, 0, 0, 128]);
    // An embedded profile wins over it.
    let icc = cms::icc_out::generic_cmyk_icc().unwrap();
    let img = to_display_rgba(Pixels::Cmyk(raster()), Some(icc), &device).unwrap();
    assert_eq!(img.get_pixel(0, 0).0, shown([255, 0, 0, 0], &DisplayColour::of(&generic(true))));
    assert_eq!(img.get_pixel(1, 0).0[3], 128, "alpha kept");
    // A damaged one falls back to the working space.
    let img = to_display_rgba(Pixels::Cmyk(raster()), Some(&icc[..200]), &device).unwrap();
    assert_eq!(img.get_pixel(0, 0).0, [0, 255, 255, 255]);
    // Buffers that don't match the size are refused.
    let bad = CmykRaster { width: 3, height: 1, cmyk: vec![0; 8], alpha: None };
    assert!(to_display_rgba(Pixels::Cmyk(bad), None, &device).is_none());
}

#[test]
fn rgb_with_an_embedded_profile_is_converted_to_srgb() {
    let px = [64, 192, 64, 255, 200, 40, 40, 128];
    let png = png_with_icc(2, 1, &px, Some(adobe_rgb_icc()));
    let img = decode_rgba(&png).unwrap();
    let green = img.get_pixel(0, 0).0;
    let want = adobe_to_srgb([64, 192, 64]);
    assert!(near(green, want, 2.0), "{green:?} vs {want:?}");
    // Adobe RGB's green is greener than sRGB's: shown as sRGB, the colour is more saturated.
    assert!(green[0] < 50 && green[3] == 255, "{green:?}");
    assert_eq!(img.get_pixel(1, 0).0[3], 128, "alpha kept");

    // An sRGB profile, or none, leaves the pixels exactly as stored.
    let srgb = png_with_icc(2, 1, &px, Some(icc(Builtin::Srgb)));
    assert_eq!(decode_rgba(&srgb).unwrap().as_raw(), &px);
    assert_eq!(decode_rgba(&png_with_icc(2, 1, &px, None)).unwrap().as_raw(), &px);
}

#[test]
fn gray_with_an_embedded_profile_is_converted() {
    let img = image::RgbaImage::from_pixel(1, 1, Rgba([128, 128, 128, 255]));
    let icc = craft_cms::builtin::linear_gray().to_bytes().to_vec();
    let p = to_display_rgba(Pixels::Gray(img), Some(&icc), &DisplayColour::active()).unwrap().get_pixel(0, 0).0;
    // Linear 50 % grey is sRGB 188.
    assert!(p[0].abs_diff(188) <= 2 && p[0] == p[1] && p[1] == p[2], "{p:?}");
}

#[test]
fn unusable_profiles_leave_the_pixels_alone() {
    let px = [64, 192, 64, 255];
    let adobe = adobe_rgb_icc();
    let cmyk = cms::icc_out::generic_cmyk_icc().unwrap().to_vec();
    let mut huge = adobe.clone();
    huge.resize(MAX_ICC_BYTES + 1, 0);
    let mut lying = adobe.clone();
    lying[128..132].copy_from_slice(&u32::MAX.to_be_bytes()); // tag count far beyond the data
    for icc in [b"not a profile".to_vec(), adobe[..adobe.len() / 2].to_vec(), lying, cmyk, huge, Vec::new()] {
        let img = image::RgbaImage::from_raw(1, 1, px.to_vec()).unwrap();
        let out = to_display_rgba(Pixels::Rgb(img), Some(&icc), &DisplayColour::active()).unwrap();
        assert_eq!(out.as_raw(), &px);
    }
    // Through the decoder too: a PNG whose profile is garbage still decodes.
    let png = png_with_icc(1, 1, &px, Some(vec![0xAB; 300]));
    assert_eq!(decode_rgba(&png).unwrap().as_raw(), &px);
}
