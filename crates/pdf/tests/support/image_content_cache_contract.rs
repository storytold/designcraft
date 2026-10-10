//! Public-export controls run against the production RandomState cache, independent of the
//! unit-only forced-collision hasher. Also included by the library unit-test target.
//! All images and placed PDFs are tiny procedural fixtures.
use std::sync::Arc;

use designcraft_compose::Cache;
use designcraft_doc::{Asset, AssetId, Content, Document, Graphic, Item, ItemId, Shape, SpreadRef};
use designcraft_geom::{Affine, Rect, shapes};
use designcraft_pdf::{BookletOptions, PdfOptions, Standard, export_booklet, export_pdf, export_pdf_with_report};

use crate::image_fixtures as fixtures;

fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(color));
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}

fn place(d: &mut Document, data: Vec<u8>, x: f64) {
    place_named(d, data, x, &format!("original-{x}.png"), "image/png", 0);
}

fn place_named(d: &mut Document, data: Vec<u8>, x: f64, name: &str, mime: &str, page: u32) {
    let aid = AssetId(d.alloc());
    d.assets.insert(aid, Arc::new(Asset { id: aid, name: name.into(), mime: mime.into(), link: None, data: Arc::new(data), pixels: None, page }));
    let id = ItemId(d.alloc());
    let mut item = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(x, 36.0, x + 20.0, 56.0)));
    item.stroke.weight = 0.0;
    item.content = Content::Graphic(Graphic {
        asset: aid,
        size: (20.0, 20.0),
        xf: Affine::translate((x, 36.0)),
        auto_fit: Default::default(),
        fit_align: 4,
        crop: [0.0; 4],
    });
    d.insert_item(SpreadRef::Doc(0), item, None).unwrap();
}

#[derive(Debug)]
struct DrawnImage {
    size: (u32, u32),
    rgb: [u8; 3],
    alpha: u8,
    x: f64,
}

fn drawn_images(bytes: Vec<u8>) -> Vec<DrawnImage> {
    drawn_content(bytes).0
}

fn drawn_content(bytes: Vec<u8>) -> (Vec<DrawnImage>, Vec<([u8; 4], kurbo::Rect)>) {
    use hayro_interpret::font::Glyph;
    use hayro_interpret::{
        BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, ImageData, InterpreterCache, InterpreterSettings, Paint, PathDrawMode, SoftMask,
        interpret_page,
    };
    use kurbo::Shape as _;
    struct Sink(Vec<DrawnImage>, Vec<([u8; 4], kurbo::Rect)>);
    impl Device<'_> for Sink {
        fn set_soft_mask(&mut self, _: Option<SoftMask<'_>>) {}
        fn set_blend_mode(&mut self, _: BlendMode) {}
        fn draw_path(&mut self, path: &kurbo::BezPath, transform: kurbo::Affine, paint: &Paint<'_>, mode: &PathDrawMode) {
            if let (Paint::Color(color), PathDrawMode::Fill(_)) = (paint, mode) {
                self.1.push((color.to_rgba().to_rgba8(), (transform * path.clone()).bounding_box()));
            }
        }
        fn push_clip_path(&mut self, _: &ClipPath) {}
        fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'_>>, _: BlendMode) {}
        fn draw_glyph(&mut self, _: &Glyph<'_>, _: kurbo::Affine, _: kurbo::Affine, _: &Paint<'_>, _: &GlyphDrawMode) {}
        fn draw_image(&mut self, image: Image<'_, '_>, transform: kurbo::Affine) {
            if let Image::Raster(image) = image {
                image.with_rgba(
                    |pixels, alpha| {
                        let size = (pixels.width(), pixels.height());
                        let rgb = match pixels {
                            ImageData::Rgb(p) => [p.data[0], p.data[1], p.data[2]],
                            ImageData::Luma(p) => [p.data[0]; 3],
                        };
                        self.0.push(DrawnImage { size, rgb, alpha: alpha.map_or(255, |p| p.data[0]), x: transform.as_coeffs()[4] });
                    },
                    None,
                );
            }
        }
        fn pop_clip_path(&mut self) {}
        fn pop_transparency_group(&mut self) {}
    }
    let pdf = hayro_syntax::Pdf::new(bytes).unwrap();
    let cache = InterpreterCache::new();
    let mut sink = Sink(Vec::new(), Vec::new());
    for page in pdf.pages().iter() {
        let mut context =
            Context::new(kurbo::Affine::IDENTITY, kurbo::Rect::new(0.0, 0.0, 1000.0, 1000.0), &cache, pdf.xref(), InterpreterSettings::default());
        interpret_page(page, &mut context, &mut sink);
    }
    (sink.0, sink.1)
}

#[test]
fn repeated_svg_assets_keep_vector_paths_and_placement() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20"><rect x="2" y="3" width="5" height="7" fill="red"/></svg>"#;
    let mut d = Document::new(&Default::default());
    for (name, x) in [("vector-a.svg", 36.0), ("vector-b.svg", 66.0)] {
        place_named(&mut d, svg.to_vec(), x, name, "image/svg+xml", 0);
    }
    let report = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(image_objects(&report.bytes).is_empty(), "placed SVGs must stay vector artwork");
    let (images, paths) = drawn_content(report.bytes);
    assert!(images.is_empty());
    let red: Vec<_> = paths.into_iter().filter(|(color, _)| *color == [255, 0, 0, 255]).collect();
    assert_eq!(red.len(), 2, "both SVG assets must paint the original rectangle");
    for ((_, bounds), x) in red.iter().zip([38.0, 68.0]) {
        assert!((bounds.x0 - x).abs() < 1e-6, "{bounds:?}");
        assert!((bounds.width() - 5.0).abs() < 1e-6, "{bounds:?}");
        assert!((bounds.height() - 7.0).abs() < 1e-6, "{bounds:?}");
    }
}

#[test]
fn repeated_encoded_assets_preserve_colors_alpha_dimensions_and_placement() {
    let mut d = Document::new(&Default::default());
    let red = png(2, 1, [255, 0, 0, 255]);
    let blue = png(1, 2, [0, 0, 255, 128]);
    for (data, x) in [(red.clone(), 36.0), (blue.clone(), 66.0), (red, 96.0), (blue, 126.0)] {
        place(&mut d, data, x);
    }
    d.check().unwrap();
    let got = drawn_images(export_pdf(&d, &Cache::new(), &PdfOptions::default()).unwrap());
    assert_eq!(got.len(), 4);
    for (index, item) in got.iter().enumerate() {
        assert_eq!(item.size, if index % 2 == 0 { (2, 1) } else { (1, 2) });
        assert_eq!(item.rgb, if index % 2 == 0 { [255, 0, 0] } else { [0, 0, 255] });
        assert_eq!(item.alpha, if index % 2 == 0 { 255 } else { 128 });
        assert!((item.x - (36.0 + index as f64 * 30.0)).abs() < 1e-6, "{got:?}");
    }
}

#[test]
fn compress_images_context_does_not_leak_between_exports() {
    let mut d = Document::new(&Default::default());
    let data = png(4, 4, [220, 30, 40, 255]);
    place(&mut d, data.clone(), 36.0);
    place(&mut d, data, 66.0);
    for compress_images in [false, true, false] {
        let bytes = export_pdf(&d, &Cache::new(), &PdfOptions { compress_images, ..Default::default() }).unwrap();
        let has_dct = bytes.windows(b"/DCTDecode".len()).any(|w| w == b"/DCTDecode");
        assert_eq!(has_dct, compress_images, "export options must remain independent");
    }
}

#[test]
fn repeated_png_with_deferred_decode_error_is_skipped() {
    let mut bad = png(2, 2, [255, 0, 0, 255]);
    let idat = bad.windows(4).position(|w| w == b"IDAT").unwrap();
    bad[idat + 4] ^= 255;
    // Metadata succeeds, while corrupted IDAT decoding would fail later in the backend: the
    // image is decoded first, so both copies are skipped with a warning, not a failed export.
    assert!(krilla::image::Image::from_png(Arc::new(bad.clone()).into(), true).is_ok());
    let mut d = Document::new(&Default::default());
    place(&mut d, bad.clone(), 36.0);
    place(&mut d, bad, 66.0);
    let report = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("could not be decoded")), "{:?}", report.warnings);
    assert!(image_objects(&report.bytes).is_empty());
}

#[derive(Debug, PartialEq)]
struct ImageObject {
    space: String,
    size: (i32, i32),
    samples: Vec<u8>,
    alpha: Option<Vec<u8>>,
}

fn image_objects(bytes: &[u8]) -> Vec<ImageObject> {
    use hayro_syntax::object::{Name, Stream};
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
    pdf.objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
        .map(|s| ImageObject {
            space: s.dict().get::<Name>("ColorSpace").map_or("ICC".into(), |n| n.as_str().to_owned()),
            size: (s.dict().get::<i32>("Width").unwrap(), s.dict().get::<i32>("Height").unwrap()),
            samples: s.decoded().unwrap().into_owned(),
            alpha: s.dict().get::<Stream<'_>>("SMask").map(|a| a.decoded().unwrap().into_owned()),
        })
        .collect()
}

#[test]
fn repeated_cmyk_tiffs_keep_exact_inks_and_unassociated_alpha() {
    for alpha in [None, Some(2)] {
        let bytes = fixtures::cmyk_tiff(alpha, false);
        let mut d = Document::new(&Default::default());
        for (name, x) in [("a.tif", 36.0), ("b.tif", 66.0)] {
            place_named(&mut d, bytes.clone(), x, name, "image/tiff", 0);
        }
        for standard in [Standard::None, Standard::PdfX4] {
            for compress_images in [false, true] {
                let report = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard, compress_images, ..Default::default() }).unwrap();
                assert!(report.warnings.is_empty(), "{:?}", report.warnings);
                let images = image_objects(&report.bytes);
                let cmyk: Vec<_> = images.iter().filter(|i| i.space == "DeviceCMYK").collect();
                assert_eq!(cmyk.len(), 1, "{images:?}");
                assert_eq!(cmyk[0].size, (2, 1));
                assert_eq!(cmyk[0].samples, fixtures::INKS);
                assert_eq!(cmyk[0].alpha, alpha.map(|_| fixtures::ALPHA.to_vec()));
                let drawn = drawn_images(report.bytes);
                assert_eq!(drawn.len(), 2);
                assert_eq!(drawn.iter().map(|i| i.x).collect::<Vec<_>>(), [36.0, 66.0]);
            }
        }
    }
}

#[test]
fn successful_fallback_warns_for_both_names_and_preserves_rgb_samples() {
    let bytes = fixtures::cmyk_tiff(None, true);
    let mut d = Document::new(&Default::default());
    for (name, x) in [("other-inks-a.tif", 36.0), ("other-inks-b.tif", 66.0)] {
        place_named(&mut d, bytes.clone(), x, name, "image/tiff", 0);
    }
    let reason = "converted to RGB (a CMYK TIFF that is planar, has premultiplied alpha, uses other inks or is over 256 MiB)";
    let expected_warnings = [format!("other-inks-a.tif: {reason}"), format!("other-inks-b.tif: {reason}")];
    for compress_images in [false, true] {
        let report = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { compress_images, ..Default::default() }).unwrap();
        assert_eq!(report.warnings, expected_warnings);
        let images = image_objects(&report.bytes);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].space, "DeviceRGB");
        assert_eq!(images[0].samples, [0, 0, 0, 255, 0, 0]);
        assert_eq!(images[0].size, (2, 1));
    }
    let booklet = export_booklet(&d, &Cache::new(), &BookletOptions::default()).unwrap();
    assert_eq!(booklet.warnings, expected_warnings);
    assert_eq!(image_objects(&booklet.bytes)[0].samples, [0, 0, 0, 255, 0, 0]);
}

#[test]
fn pdfa_rgb_conversion_does_not_poison_later_cmyk_exports() {
    let bytes = fixtures::cmyk_tiff(None, false);
    let mut d = Document::new(&Default::default());
    place_named(&mut d, bytes.clone(), 36.0, "a.tif", "image/tiff", 0);
    place_named(&mut d, bytes, 66.0, "b.tif", "image/tiff", 0);
    for standard in [Standard::PdfA2b, Standard::None, Standard::PdfX4, Standard::PdfA2b, Standard::None] {
        let result = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard, created: Some(1_700_000_000), ..Default::default() });
        let report = result.unwrap();
        let images = image_objects(&report.bytes);
        assert_eq!(images.len(), 1);
        if standard == Standard::PdfA2b {
            assert_eq!(report.warnings, ["PDF/A: CMYK colours were converted to RGB (no CMYK output intent profile is available yet)"]);
            assert_eq!(images[0].space, "ICC");
            assert_eq!(images[0].samples, [0, 0, 0, 255, 0, 0]);
        } else {
            assert!(report.warnings.is_empty());
            assert_eq!(images[0].space, "DeviceCMYK");
            assert_eq!(images[0].samples, fixtures::INKS);
        }
    }
}

#[test]
fn unsupported_associated_alpha_keeps_each_asset_failure_warning() {
    let bytes = fixtures::cmyk_tiff(Some(1), false);
    let mut d = Document::new(&Default::default());
    for (name, x) in [("alpha-a.tif", 36.0), ("alpha-b.tif", 66.0)] {
        place_named(&mut d, bytes.clone(), x, name, "image/tiff", 0);
    }
    let report = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(image_objects(&report.bytes).is_empty());
    assert_eq!(report.warnings.len(), 4);
    for name in ["alpha-a.tif", "alpha-b.tif"] {
        assert!(report.warnings.iter().any(|w| w.starts_with(&format!("{name}: converted to RGB"))));
        assert!(report.warnings.contains(&format!("image `{name}` could not be decoded and was skipped")));
    }
}

fn two_page_pdf() -> Vec<u8> {
    let mut pdf = krilla::Document::new();
    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let mut page = pdf.start_page_with(krilla::page::PageSettings::new(krilla::geom::Size::from_wh(20.0, 20.0).unwrap()));
        let mut surface = page.surface();
        let image = krilla::image::Image::from_png(Arc::new(png(1, 1, color)).into(), true).unwrap();
        surface.draw_image(image, krilla::geom::Size::from_wh(20.0, 20.0).unwrap());
        surface.finish();
        page.finish();
    }
    pdf.finish().unwrap()
}

#[test]
fn placed_pdf_selection_and_media_routing_precede_content_cache() {
    let mut d = Document::new(&Default::default());
    let pdf = two_page_pdf();
    place_named(&mut d, pdf.clone(), 36.0, "page-two.pdf", "application/pdf", 1);
    place_named(&mut d, pdf, 66.0, "page-one.pdf", "application/pdf", 0);
    let red = png(2, 1, [255, 0, 0, 255]);
    place_named(&mut d, red.clone(), 96.0, "red.png", "image/png", 0);
    place_named(&mut d, red, 126.0, "movie.mp4", "video/mp4", 0);
    let report = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let drawn = drawn_images(report.bytes);
    assert_eq!(drawn.len(), 3, "media bytes must draw the existing placeholder, not the matching raster: {drawn:?}");
    assert_eq!(drawn.iter().map(|i| (i.rgb, i.size)).collect::<Vec<_>>(), [([0, 0, 255], (1, 1)), ([255, 0, 0], (1, 1)), ([255, 0, 0], (2, 1))]);
    assert_eq!(drawn.iter().map(|i| i.x).collect::<Vec<_>>(), [36.0, 66.0, 96.0]);
}

#[test]
fn repeated_direct_pngs_export_as_pdfa_without_interpolation() {
    let data = png(2, 1, [220, 30, 40, 255]);
    let mut d = Document::new(&Default::default());
    place(&mut d, data.clone(), 36.0);
    place(&mut d, data, 66.0);
    // PDF/A turns image interpolation off (#128), and the cached conversion keeps that.
    let report =
        export_pdf_with_report(&d, &Cache::new(), &PdfOptions { standard: Standard::PdfA2b, created: Some(1_700_000_000), ..Default::default() })
            .unwrap();
    assert_eq!(image_objects(&report.bytes).len(), 1, "both placements share one image");
    let needle = b"/Interpolate true";
    assert!(!report.bytes.windows(needle.len()).any(|w| w == needle));
}
