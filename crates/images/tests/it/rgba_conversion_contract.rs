//! Original small fixtures pin the straight RGBA8 conversion contract.
use image::{DynamicImage, ImageFormat, RgbaImage};

fn encoded(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut bytes = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut bytes), format).unwrap();
    bytes
}

fn assert_pixels(source: DynamicImage, expected: &[u8]) {
    for format in [ImageFormat::Png, ImageFormat::Tiff] {
        let bytes = encoded(&source, format);
        let original = bytes.clone();
        let result = designcraft_images::decode_rgba(&bytes).unwrap();
        assert_eq!(result.dimensions(), (2, 2));
        assert_eq!(result.as_raw(), expected, "{format:?}");
        assert_eq!(bytes, original);
        // The reference is the existing borrowed-conversion API, not the proposed owned one.
        assert_eq!(result, image::load_from_memory(&bytes).unwrap().to_rgba8());
    }
}

#[test]
fn rgba8_keeps_hidden_rgb_and_straight_partial_alpha() {
    let pixels = [200, 30, 90, 0, 20, 170, 240, 1, 180, 40, 70, 128, 5, 250, 90, 255];
    let source = DynamicImage::ImageRgba8(RgbaImage::from_raw(2, 2, pixels.to_vec()).unwrap());
    assert_pixels(source, &pixels);
}

#[test]
fn rgb8_only_adds_opaque_alpha() {
    let source = DynamicImage::ImageRgb8(image::RgbImage::from_raw(2, 2, vec![200, 30, 90, 20, 170, 240, 180, 40, 70, 5, 250, 90]).unwrap());
    assert_pixels(source, &[200, 30, 90, 255, 20, 170, 240, 255, 180, 40, 70, 255, 5, 250, 90, 255]);
}

#[test]
fn grayscale8_matches_fixed_rgb_values() {
    let source = DynamicImage::ImageLuma8(image::GrayImage::from_raw(2, 2, vec![0, 1, 128, 255]).unwrap());
    assert_pixels(source, &[0, 0, 0, 255, 1, 1, 1, 255, 128, 128, 128, 255, 255, 255, 255, 255]);
}

#[test]
fn grayscale16_uses_existing_eight_bit_scaling() {
    let source = DynamicImage::ImageLuma16(image::ImageBuffer::from_raw(2, 2, vec![0, 257, 32896, 65535]).unwrap());
    assert_pixels(source, &[0, 0, 0, 255, 1, 1, 1, 255, 128, 128, 128, 255, 255, 255, 255, 255]);
}

#[test]
fn grayscale16_rounding_boundaries_match_fixed_values() {
    let source = DynamicImage::ImageLuma16(image::ImageBuffer::from_raw(2, 2, vec![128, 129, 65406, 65407]).unwrap());
    assert_pixels(source, &[0, 0, 0, 255, 1, 1, 1, 255, 254, 254, 254, 255, 255, 255, 255, 255]);
}

#[test]
fn rgba16_preserves_straight_alpha_and_existing_scaling() {
    let source = DynamicImage::ImageRgba16(
        image::ImageBuffer::from_raw(2, 2, vec![65535, 0, 32896, 0, 0, 65535, 257, 257, 32896, 257, 65535, 32896, 257, 32896, 0, 65535]).unwrap(),
    );
    assert_pixels(source, &[255, 0, 128, 0, 0, 255, 1, 1, 128, 1, 255, 128, 1, 128, 0, 255]);
}

#[test]
fn malformed_rasters_remain_unsupported_without_mutating_input() {
    let png = encoded(&DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, image::Rgba([13, 27, 42, 128]))), ImageFormat::Png);
    let tiff = encoded(&DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, image::Rgba([13, 27, 42, 128]))), ImageFormat::Tiff);
    for input in [b"".as_slice(), b"not a raster", &png[..16], &tiff[..6]] {
        let original = input.to_vec();
        assert!(designcraft_images::decode_rgba(input).is_none());
        assert_eq!(input, original);
    }
}

#[test]
fn all_gray_bytes_survive_png_and_tiff_decoding() {
    let bytes: Vec<u8> = (0u8..=255).collect();
    let source = DynamicImage::ImageLuma8(image::GrayImage::from_raw(256, 1, bytes).unwrap());
    for format in [ImageFormat::Png, ImageFormat::Tiff] {
        let encoded = encoded(&source, format);
        let old = image::load_from_memory(&encoded).unwrap().to_rgba8();
        let actual = designcraft_images::decode_rgba(&encoded).unwrap();
        assert_eq!(actual.dimensions(), (256, 1));
        assert_eq!(actual.as_raw(), old.as_raw());
        assert_eq!(actual.color_space(), old.color_space());
        for (value, pixel) in (0u8..=255).zip(actual.pixels()) {
            assert_eq!(pixel.0, [value, value, value, 255]);
        }
    }
}

#[test]
fn grayscale_eps_proxy_keeps_all_values_and_source_bytes() {
    let source = DynamicImage::ImageLuma8(image::GrayImage::from_raw(256, 1, (0u8..=255).collect()).unwrap());
    let tiff = encoded(&source, ImageFormat::Tiff);
    // Original DOS EPS wrapper with this test's generated grayscale TIFF preview.
    let ps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 256 1\n%%EOF\n";
    let mut eps = vec![0xC5, 0xD0, 0xD3, 0xC6];
    for value in [30, ps.len(), 0, 0, 30 + ps.len(), tiff.len()] {
        eps.extend_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
    }
    eps.extend_from_slice(&[0xFF, 0xFF]);
    eps.extend_from_slice(ps);
    eps.extend_from_slice(&tiff);
    let original = eps.clone();
    let old = image::load_from_memory(&tiff).unwrap().to_rgba8();
    // Current main exposes EPS proxies separately; automatic EPS raster routing is PR150.
    let (proxy, _) = designcraft_images::eps_proxy(&eps).unwrap();
    let actual = designcraft_images::decode_rgba(&proxy).unwrap();
    assert_eq!(actual.dimensions(), (256, 1));
    assert_eq!(actual.as_raw(), old.as_raw());
    assert_eq!(actual.color_space(), old.color_space());
    assert_eq!(eps, original);
}
