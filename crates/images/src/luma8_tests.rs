use super::{decode_to_rgba8, prepare_luma8_rgba, rgba_or_original};
use image::metadata::{CicpColorPrimaries as Primaries, CicpTransferCharacteristics as Transfer};
use image::{DynamicImage, GrayImage, RgbaImage};

const PRIMARIES: [Primaries; 12] = [
    Primaries::SRgb,
    Primaries::Unspecified,
    Primaries::RgbM,
    Primaries::RgbB,
    Primaries::Bt601,
    Primaries::Rgb240m,
    Primaries::GenericFilm,
    Primaries::Rgb2020,
    Primaries::Xyz,
    Primaries::SmpteRp431,
    Primaries::SmpteRp432,
    Primaries::Industry22,
];
const TRANSFERS: [Transfer; 17] = [
    Transfer::Bt709,
    Transfer::Unspecified,
    Transfer::Bt470M,
    Transfer::Bt470BG,
    Transfer::Bt601,
    Transfer::Smpte240m,
    Transfer::Linear,
    Transfer::Log100,
    Transfer::LogSqrt,
    Transfer::Iec61966_2_4,
    Transfer::Bt1361,
    Transfer::SRgb,
    Transfer::Bt2020_10bit,
    Transfer::Bt2020_12bit,
    Transfer::Smpte2084,
    Transfer::Smpte428,
    Transfer::Bt2100Hlg,
];

#[test]
fn every_luma8_value_and_representable_cicp_match_old_and_independent_oracles() {
    for primaries in PRIMARIES {
        for transfer in TRANSFERS {
            let mut gray = GrayImage::from_raw(256, 1, (0u8..=255).collect()).unwrap();
            gray.set_rgb_primaries(primaries);
            gray.set_transfer_function(transfer);
            let original_color = gray.color_space();
            let source = DynamicImage::ImageLuma8(gray);
            let old = source.to_rgba8();
            let new = decode_to_rgba8(&source);
            assert_eq!(new.dimensions(), (256, 1));
            assert_eq!(new.as_raw(), old.as_raw(), "{primaries:?} {transfer:?}");
            assert_eq!(new.color_space(), original_color);
            assert_eq!(new.color_space(), old.color_space());
            for (value, pixel) in (0u8..=255).zip(new.pixels()) {
                assert_eq!(pixel.0, [value, value, value, 255]);
            }
        }
    }
}

#[test]
fn zero_non_square_and_chunk_boundary_dimensions_match() {
    for (width, height) in [(0, 0), (0, 7), (7, 0), (3, 7), (255, 1), (256, 1), (257, 1)] {
        let length = width as usize * height as usize;
        let bytes = (0..length).map(|n| (n % 256) as u8).collect();
        let source = DynamicImage::ImageLuma8(GrayImage::from_raw(width, height, bytes).unwrap());
        let old = source.to_rgba8();
        let new = decode_to_rgba8(&source);
        assert_eq!(new.dimensions(), old.dimensions());
        assert_eq!(new.color_space(), old.color_space());
        assert_eq!(new.as_raw(), old.as_raw());
    }
}

#[test]
fn unused_backing_elements_do_not_enter_the_output() {
    let gray = GrayImage::from_raw(2, 1, vec![13, 27, 222, 223, 224]).unwrap();
    let prepared = prepare_luma8_rgba(&gray).unwrap();
    assert_eq!(prepared.as_raw(), &[13, 13, 13, 255, 27, 27, 27, 255]);
    assert_eq!(prepared, DynamicImage::ImageLuma8(gray).to_rgba8());
}

#[test]
fn forced_preparation_failure_uses_original_conversion_and_metadata() {
    let mut gray = GrayImage::from_raw(3, 1, vec![0, 129, 255]).unwrap();
    gray.set_rgb_primaries(Primaries::Rgb2020);
    gray.set_transfer_function(Transfer::Smpte2084);
    let source = DynamicImage::ImageLuma8(gray);
    let old = source.to_rgba8();
    let fallback = rgba_or_original(&source, None);
    assert_eq!(fallback.as_raw(), old.as_raw());
    assert_eq!(fallback.dimensions(), old.dimensions());
    assert_eq!(fallback.color_space(), old.color_space());
}

#[test]
fn non_luma8_variants_keep_original_conversion() {
    let variants = [
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 2, image::Rgba([200, 30, 90, 0]))),
        DynamicImage::ImageRgb8(image::RgbImage::from_pixel(3, 2, image::Rgb([17, 29, 43]))),
        DynamicImage::ImageLumaA8(image::GrayAlphaImage::from_pixel(3, 2, image::LumaA([129, 67]))),
        DynamicImage::ImageLuma16(image::ImageBuffer::from_pixel(3, 2, image::Luma([129u16]))),
        DynamicImage::ImageLumaA16(image::ImageBuffer::from_pixel(3, 2, image::LumaA([65406u16, 32896]))),
        DynamicImage::ImageRgb16(image::ImageBuffer::from_pixel(3, 2, image::Rgb([128u16, 129, 65535]))),
        DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(3, 2, image::Rgba([128u16, 129, 65535, 32896]))),
        DynamicImage::ImageRgb32F(image::ImageBuffer::from_pixel(3, 2, image::Rgb([0.1f32, 0.5, 0.9]))),
        DynamicImage::ImageRgba32F(image::ImageBuffer::from_pixel(3, 2, image::Rgba([0.1f32, 0.5, 0.9, 0.25]))),
    ];
    for source in variants {
        let old = source.to_rgba8();
        let new = decode_to_rgba8(&source);
        assert_eq!(new.as_raw(), old.as_raw());
        assert_eq!(new.dimensions(), old.dimensions());
        assert_eq!(new.color_space(), old.color_space());
    }
}
