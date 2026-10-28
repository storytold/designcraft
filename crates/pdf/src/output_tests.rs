//! Export PDF › Compression (resampling, cropping and encoding placed images per placement) and
//! Output › Color Conversion, checked on exported files.

use designcraft_color::cms::{COATED_FOGRA39, GENERIC_CMYK, SRGB};
use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, Fill, Item, ItemId, SpreadRef};
use designcraft_geom::{Affine, Rect};

use crate::*;

/// An image XObject: colour space, filter, pixel size and decoded samples.
#[derive(Debug)]
struct ImageObject {
    space: String,
    filter: String,
    size: (u32, u32),
    samples: Vec<u8>,
}

fn images(bytes: &[u8]) -> Vec<ImageObject> {
    use hayro_syntax::object::Name;
    let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).expect("parse");
    pdf.objects()
        .into_iter()
        .filter_map(|o| o.into_stream())
        .filter(|s| s.dict().get::<Name>("Subtype").is_some_and(|n| n.as_str() == "Image"))
        // Soft masks are images too; only colour images here.
        .filter(|s| s.dict().get::<Name>("ColorSpace").is_some() || s.dict().get::<hayro_syntax::object::Array>("ColorSpace").is_some())
        .map(|s| {
            let d = s.dict();
            let space = d.get::<Name>("ColorSpace").map_or("ICCBased".to_string(), |n| n.as_str().to_string());
            let filter = d.get::<Name>("Filter").map_or(String::new(), |n| n.as_str().to_string());
            let size = (d.get::<u32>("Width").unwrap_or(0), d.get::<u32>("Height").unwrap_or(0));
            ImageObject { space, filter, size, samples: s.decoded().map(|c| c.into_owned()).unwrap_or_default() }
        })
        .collect()
}

/// The fill colours set in the (uncompressed) content: `rg` (3 values) and `k` (4 values).
fn fills(bytes: &[u8]) -> Vec<(&'static str, Vec<f32>)> {
    let s = String::from_utf8_lossy(bytes);
    let tokens: Vec<&str> = s.split_whitespace().collect();
    let mut out = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let (op, n) = match *t {
            "rg" => ("rg", 3),
            "k" => ("k", 4),
            _ => continue,
        };
        let args: Option<Vec<f32>> = tokens.get(i.saturating_sub(n)..i).map(|a| a.iter().filter_map(|v| v.parse().ok()).collect());
        if let Some(a) = args.filter(|a| a.len() == n) {
            out.push((op, a));
        }
    }
    out
}

fn doc() -> Document {
    Document::new(&NewDocument { pages: 1, ..NewDocument::default() })
}

/// Place `data` (`px` pixels) in a frame `frame` (points), the image scaled to `width` points
/// wide from the frame's top left.
fn place(d: &mut Document, data: Vec<u8>, px: (u32, u32), frame: Rect, width: f64) {
    place_xf(d, data, px, frame, Affine::translate((frame.x0, frame.y0)) * Affine::scale(width / f64::from(px.0)));
}

/// Place `data` in `frame` with the graphic transform `xf`.
fn place_xf(d: &mut Document, data: Vec<u8>, px: (u32, u32), frame: Rect, xf: Affine) {
    let asset = designcraft_doc::AssetId(d.alloc());
    let mime = designcraft_images::mime(&data).into();
    d.assets.insert(
        asset,
        std::sync::Arc::new(designcraft_doc::Asset {
            id: asset,
            name: "photo".into(),
            mime,
            data: data.into(),
            pixels: Some(px),
            ..Default::default()
        }),
    );
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut it = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(frame));
    it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
        layer_options: None,
        asset,
        size: (f64::from(px.0), f64::from(px.1)),
        xf,
        clip: None,
        auto_fit: designcraft_doc::Fitting::None,
        fit_align: 4,
        crop: [0.0; 4],
        wrap: Default::default(),
        fill: Default::default(),
    });
    d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
}

fn encode(img: image::DynamicImage, fmt: image::ImageFormat) -> Vec<u8> {
    let mut b = std::io::Cursor::new(Vec::new());
    img.write_to(&mut b, fmt).unwrap();
    b.into_inner()
}

/// A photo-like RGB image (many colours, so Automatic picks JPEG).
fn photo(w: u32, h: u32, fmt: image::ImageFormat) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7 + y) as u8, (y * 5 + x * 3) as u8, (x * y / 3) as u8]));
    encode(image::DynamicImage::ImageRgb8(img), fmt)
}

fn color(downsample: Downsample, ppi: f64, above: f64, compression: ImageCompression) -> ImageOptions {
    ImageOptions { color: ImageSampling::resample(downsample, ppi, above, compression, JpegQuality::High), ..ImageOptions::default() }
}

fn export(d: &Document, opts: PdfOptions) -> ExportReport {
    export_pdf_with_report(d, &Cache::new(), &PdfOptions { compress: false, created: Some(0), ..opts }).expect("export")
}

#[test]
fn images_above_the_threshold_are_downsampled_at_their_placed_size() {
    // 600 px across one inch: 600 ppi.
    let mut d = doc();
    place(&mut d, photo(600, 300, image::ImageFormat::Png), (600, 300), Rect::new(72.0, 72.0, 144.0, 108.0), 72.0);
    let opts = |io: Option<ImageOptions>| PdfOptions { images: io, ..Default::default() };
    assert_eq!(images(&export(&d, opts(None)).bytes)[0].size, (600, 300), "no options: full resolution");
    let r = export(&d, opts(Some(color(Downsample::Bicubic, 300.0, 450.0, ImageCompression::Zip))));
    let img = &images(&r.bytes)[0];
    assert_eq!(img.size, (300, 150));
    assert_eq!(img.filter, "FlateDecode");
    // Below the threshold: kept.
    let r = export(&d, opts(Some(color(Downsample::Average, 300.0, 700.0, ImageCompression::Zip))));
    assert_eq!(images(&r.bytes)[0].size, (600, 300));
    // Do Not Downsample.
    let r = export(&d, opts(Some(color(Downsample::None, 72.0, 72.0, ImageCompression::Zip))));
    assert_eq!(images(&r.bytes)[0].size, (600, 300));

    // The same file placed at two inches is 300 ppi: nothing to do.
    let mut d = doc();
    place(&mut d, photo(600, 300, image::ImageFormat::Png), (600, 300), Rect::new(72.0, 72.0, 216.0, 144.0), 144.0);
    let r = export(&d, opts(Some(color(Downsample::Subsample, 300.0, 450.0, ImageCompression::Zip))));
    assert_eq!(images(&r.bytes)[0].size, (600, 300));
}

#[test]
fn compression_writes_jpeg_or_lossless() {
    let mut d = doc();
    place(&mut d, photo(64, 64, image::ImageFormat::Png), (64, 64), Rect::new(72.0, 72.0, 136.0, 136.0), 64.0);
    let opts = |c| PdfOptions { images: Some(color(Downsample::None, 300.0, 450.0, c)), ..Default::default() };
    assert_eq!(images(&export(&d, opts(ImageCompression::Jpeg)).bytes)[0].filter, "DCTDecode");
    assert_eq!(images(&export(&d, opts(ImageCompression::Automatic)).bytes)[0].filter, "DCTDecode", "a photo: JPEG");
    assert_eq!(images(&export(&d, opts(ImageCompression::Zip)).bytes)[0].filter, "FlateDecode");

    // Flat art stays lossless under Automatic.
    let flat = encode(
        image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(64, 64, |x, _| image::Rgb(if x < 32 { [200, 0, 0] } else { [0, 0, 200] }))),
        image::ImageFormat::Png,
    );
    let mut d = doc();
    place(&mut d, flat, (64, 64), Rect::new(72.0, 72.0, 136.0, 136.0), 64.0);
    assert_eq!(images(&export(&d, opts(ImageCompression::Automatic)).bytes)[0].filter, "FlateDecode");

    // A JPEG that needs nothing is passed through; ZIP decodes it.
    let jpeg = photo(64, 64, image::ImageFormat::Jpeg);
    let mut d = doc();
    place(&mut d, jpeg.clone(), (64, 64), Rect::new(72.0, 72.0, 136.0, 136.0), 64.0);
    let r = export(&d, opts(ImageCompression::Automatic));
    assert!(r.bytes.windows(jpeg.len().min(200)).any(|w| w == &jpeg[..jpeg.len().min(200)]), "the original JPEG data");
    assert_eq!(images(&export(&d, opts(ImageCompression::Zip)).bytes)[0].filter, "FlateDecode");
}

#[test]
fn image_data_outside_the_frame_is_left_out() {
    // A 200×100 px image 2 inches wide, in a frame showing its left inch.
    let mut d = doc();
    place(&mut d, photo(200, 100, image::ImageFormat::Png), (200, 100), Rect::new(72.0, 72.0, 144.0, 144.0), 144.0);
    let io = ImageOptions { crop_to_frames: true, ..color(Downsample::None, 300.0, 450.0, ImageCompression::Zip) };
    let r = export(&d, PdfOptions { images: Some(io), ..Default::default() });
    let (w, h) = images(&r.bytes)[0].size;
    // Half the width, plus a pixel of margin.
    assert_eq!((w, h), (101, 100));
    // Without cropping, every pixel.
    let r = export(&d, PdfOptions { images: Some(ImageOptions { crop_to_frames: false, ..io }), ..Default::default() });
    assert_eq!(images(&r.bytes)[0].size, (200, 100));
    // A frame that shows nothing of the image writes no image.
    let mut d = doc();
    place_xf(
        &mut d,
        photo(100, 100, image::ImageFormat::Png),
        (100, 100),
        Rect::new(72.0, 72.0, 144.0, 144.0),
        Affine::translate((400.0, 400.0)) * Affine::scale(0.72),
    );
    let r = export(&d, PdfOptions { images: Some(io), ..Default::default() });
    assert!(images(&r.bytes).is_empty());
}

#[test]
fn black_and_white_images_stay_black_and_white() {
    let bw = image::GrayImage::from_fn(240, 240, |x, y| image::Luma([if (x / 3 + y / 3) % 2 == 0 { 0 } else { 255 }]));
    let mut d = doc();
    // 240 px in a fifth of an inch: 1200 ppi.
    place(&mut d, encode(image::DynamicImage::ImageLuma8(bw), image::ImageFormat::Png), (240, 240), Rect::new(72.0, 72.0, 86.4, 86.4), 14.4);
    let io = ImageOptions {
        mono: ImageSampling::resample(Downsample::Bicubic, 600.0, 900.0, ImageCompression::Zip, JpegQuality::Low),
        // The grey settings must not apply to it.
        gray: ImageSampling::resample(Downsample::Bicubic, 72.0, 72.0, ImageCompression::Jpeg, JpegQuality::Low),
        ..ImageOptions::default()
    };
    let r = export(&d, PdfOptions { images: Some(io), ..Default::default() });
    let img = &images(&r.bytes)[0];
    assert_eq!(img.space, "DeviceGray");
    assert_eq!(img.size, (120, 120));
    assert_eq!(img.filter, "FlateDecode");
    assert!(img.samples.iter().all(|v| *v == 0 || *v == 255), "only black and white");
}

#[test]
fn convert_to_a_cmyk_destination() {
    let mut d = doc();
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(300.0, 300.0, 400.0, 400.0)));
    d.swatches.push(designcraft_color::Swatch::color("Web Red", designcraft_color::Color::rgb(1.0, 0.0, 0.0)));
    b.fill = Fill::swatch("Web Red");
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    let red = encode(image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([255, 0, 0]))), image::ImageFormat::Png);
    place(&mut d, red, (8, 8), Rect::new(72.0, 72.0, 80.0, 80.0), 8.0);
    let fills = |bytes: &[u8]| {
        let f = fills(bytes);
        (f.iter().any(|(op, _)| *op == "rg"), f.iter().any(|(op, _)| *op == "k"))
    };

    // Without conversion the red stays RGB.
    let plain = export(&d, PdfOptions::default());
    assert!(fills(&plain.bytes).0);
    assert_eq!(images(&plain.bytes)[0].space, "DeviceRGB");

    let output =
        OutputColor { conversion: ColorConversion::ToDestination, destination: Some(GENERIC_CMYK.into()), include_profile: true, intent: None };
    let r = export(&d, PdfOptions { output: output.clone(), ..Default::default() });
    let (rgb, cmyk) = fills(&r.bytes);
    assert!(!rgb && cmyk, "the fill is CMYK");
    let img = &images(&r.bytes)[0];
    assert_eq!(img.space, "DeviceCMYK");
    // Red: magenta and yellow, little cyan.
    assert!(img.samples[1] > 150 && img.samples[2] > 150 && img.samples[0] < 60, "{:?}", &img.samples[..4]);
    // The destination is the output intent.
    let s = String::from_utf8_lossy(&r.bytes);
    assert!(s.contains("OutputIntent") && s.contains("Generic CMYK"), "output intent names the destination");

    // PDF/X-4 to the destination passes its checks.
    let r = export(&d, PdfOptions { standard: Standard::PdfX4, output, ..Default::default() });
    assert!(check_pdfx4(&r.bytes).is_empty(), "{:?}", check_pdfx4(&r.bytes));
    assert!(r.warnings.iter().all(|w| !w.contains("PDF/X-4")), "{:?}", r.warnings);
    assert!(String::from_utf8_lossy(&r.bytes).contains("Generic CMYK"));
}

#[test]
fn convert_to_an_rgb_destination() {
    let mut d = doc();
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(300.0, 300.0, 400.0, 400.0)));
    d.swatches.push(designcraft_color::Swatch::color("Process Cyan", designcraft_color::Color::cmyk(1.0, 0.0, 0.0, 0.0)));
    b.fill = Fill::swatch("Process Cyan");
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    let output = OutputColor { conversion: ColorConversion::ToDestination, destination: Some(SRGB.into()), include_profile: false, intent: None };
    let r = export(&d, PdfOptions { output, ..Default::default() });
    let f = fills(&r.bytes);
    assert!(!f.iter().any(|(op, _)| *op == "k"), "no CMYK fill left: {f:?}");
    assert!(f.iter().any(|(op, _)| *op == "rg"), "{f:?}");
    // PDF/X can't take an RGB destination: not converted, with a warning.
    let output = OutputColor { conversion: ColorConversion::ToDestination, destination: Some(SRGB.into()), include_profile: false, intent: None };
    let r = export(&d, PdfOptions { standard: Standard::PdfX4, output, ..Default::default() });
    assert!(r.warnings.iter().any(|w| w.contains("PDF/X needs a CMYK destination")), "{:?}", r.warnings);
}

#[test]
fn preserve_numbers_keeps_cmyk_in_another_cmyk_destination() {
    let mut d = doc();
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(300.0, 300.0, 400.0, 400.0)));
    d.swatches.push(designcraft_color::Swatch::color("Teal", designcraft_color::Color::cmyk(0.6, 0.0, 0.2, 0.0)));
    b.fill = Fill::swatch("Teal");
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    let teal = |conversion| {
        let output = OutputColor { conversion, destination: Some(GENERIC_CMYK.into()), include_profile: false, intent: None };
        fills(&export(&d, PdfOptions { output, ..Default::default() }).bytes)
            .into_iter()
            .find(|(op, _)| *op == "k")
            .map(|(_, v)| v)
            .unwrap_or_default()
    };
    let kept = teal(ColorConversion::PreserveNumbers);
    // krilla writes 8-bit values.
    assert!(kept.len() == 4 && (kept[0] - 0.6).abs() < 0.01 && kept[1] == 0.0 && (kept[2] - 0.2).abs() < 0.01 && kept[3] == 0.0, "{kept:?}");
    assert_ne!(teal(ColorConversion::ToDestination), kept, "re-separated for the destination");
    // An unknown destination fails the export with its name.
    let output = OutputColor { conversion: ColorConversion::ToDestination, destination: Some("Nope".into()), include_profile: false, intent: None };
    let e = export_pdf(&d, &Cache::new(), &PdfOptions { output, ..Default::default() }).unwrap_err().to_string();
    assert!(e.contains("Nope"), "{e}");
    let _ = COATED_FOGRA39;
}

fn text(d: &Document, opts: PdfOptions) -> String {
    String::from_utf8_lossy(&export(d, opts).bytes).into_owned()
}

#[test]
fn export_bleed_overrides_the_documents() {
    let mut d = doc();
    d.settings.bleed = [9.0; 4];
    let s = text(&d, PdfOptions { bleed: true, bleed_values: Some([18.0, 0.0, 9.0, 9.0]), ..Default::default() });
    assert!(s.contains("/BleedBox[0 0 630 810]"), "top 18, bottom 0, sides 9");
    assert!(s.contains("/TrimBox[9 0 621 792]"));
    // Bleed off: the override doesn't apply.
    let s = text(&d, PdfOptions { bleed: false, bleed_values: Some([18.0; 4]), ..Default::default() });
    assert!(s.contains("/MediaBox[0 0 612 792]"));
    // Hostile values are clamped, not trusted.
    let s = text(&d, PdfOptions { bleed: true, bleed_values: Some([f64::NAN, -5.0, 0.0, 0.0]), ..Default::default() });
    assert!(s.contains("/MediaBox[0 0 612 792]"));
}

#[test]
fn the_slug_area_prints_when_included() {
    let mut d = doc();
    d.settings.bleed = [9.0; 4];
    d.settings.slug = [0.0, 36.0, 0.0, 0.0];
    // A box in the slug, below the page.
    let lid = d.default_layer();
    let id = ItemId(d.alloc());
    let mut b = Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 806.0, 200.0, 822.0)));
    d.swatches.push(designcraft_color::Swatch::color("Slug Note", designcraft_color::Color::cmyk(0.0, 0.0, 0.0, 0.7)));
    b.fill = Fill::swatch("Slug Note");
    d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    let note = |opts| fills(&export(&d, opts).bytes).iter().any(|(op, v)| *op == "k" && (v[3] - 0.7).abs() < 0.01);
    let s = text(&d, PdfOptions { bleed: true, slug: true, ..Default::default() });
    // 792 + 36 below, bleed 9 above.
    assert!(s.contains("/MediaBox[0 0 630 837]"), "{}", s.lines().find(|l| l.contains("MediaBox")).unwrap_or(""));
    // PDF space is y-up: the bleed sits above the 27 pt of slug that pass beyond it.
    assert!(s.contains("/BleedBox[0 27 630 837]"), "the bleed box stays the bleed");
    assert!(note(PdfOptions { bleed: true, slug: true, ..Default::default() }), "the slug item is drawn");
    assert!(!note(PdfOptions { bleed: true, slug: false, ..Default::default() }), "outside the bleed it is not");
}

#[test]
fn registration_marks_and_colour_bars() {
    let d = doc();
    let marks = Marks { registration: true, color_bars: true, ..Marks::NONE };
    let r = export(&d, PdfOptions { marks, ..Default::default() });
    let f = fills(&r.bytes);
    // Solid cyan and every black tint.
    assert!(f.iter().any(|(op, v)| *op == "k" && v == &vec![1.0, 0.0, 0.0, 0.0]), "{f:?}");
    for i in 1..=10 {
        assert!(f.iter().any(|(op, v)| *op == "k" && v[..3] == [0.0; 3] && (v[3] - i as f32 / 10.0).abs() < 0.01), "{i}0% black");
    }
    let s = String::from_utf8_lossy(&r.bytes);
    assert!(s.contains("/All"), "registration targets on every plate");
    assert!(s.contains("/MediaBox[0 0 672 852]"), "room for the marks");
    // In PDF/A the bars are RGB, like everything else.
    let r = export(&d, PdfOptions { marks, standard: Standard::PdfA2b, ..Default::default() });
    assert!(!fills(&r.bytes).iter().any(|(op, _)| *op == "k"));
}

/// An image placed several times is decoded once per variant written, not once per placement
/// (four placements at two sizes: two decodes).
#[test]
fn an_image_placed_many_times_is_decoded_once_per_variant() {
    let png = photo(600, 300, image::ImageFormat::Png);
    let mut d = doc();
    for (i, width) in [72.0, 72.0, 72.0, 144.0].into_iter().enumerate() {
        let y = 72.0 + 100.0 * i as f64;
        place(&mut d, png.clone(), (600, 300), Rect::new(72.0, y, 72.0 + width, y + width / 2.0), width);
    }
    // The four placements share one asset.
    let first = d.assets.keys().next().copied().unwrap();
    let assets: Vec<_> = d.assets.keys().copied().collect();
    for id in assets.into_iter().filter(|id| *id != first) {
        d.assets.remove(&id);
    }
    for sp in &mut d.spreads {
        for it in &mut std::sync::Arc::make_mut(sp).items {
            if let designcraft_doc::Content::Graphic(g) = &mut std::sync::Arc::make_mut(it).content {
                g.asset = first;
            }
        }
    }
    crate::export::DECODES.with(|c| c.set(0));
    let r = export(&d, PdfOptions { images: Some(color(Downsample::Bicubic, 150.0, 200.0, ImageCompression::Zip)), ..Default::default() });
    assert_eq!(crate::export::DECODES.with(|c| c.get()), 2, "one decode per size written");
    let sizes: std::collections::BTreeSet<_> = images(&r.bytes).into_iter().map(|i| i.size).collect();
    assert_eq!(sizes, [(150, 75), (300, 150)].into_iter().collect(), "600 ppi → 150 ppi, 300 ppi → 150 ppi");
}

/// PDF/X writes the chosen CMYK destination as its output intent even without colour
/// conversion.
#[test]
fn pdf_x_carries_the_chosen_destination_without_conversion() {
    let d = doc();
    let output = OutputColor { conversion: ColorConversion::None, destination: Some(GENERIC_CMYK.into()), include_profile: false, intent: None };
    let r = export(&d, PdfOptions { standard: Standard::PdfX4, output, ..Default::default() });
    assert!(String::from_utf8_lossy(&r.bytes).contains("Generic CMYK"), "the destination is the output intent");
    assert!(check_pdfx4(&r.bytes).is_empty(), "{:?}", check_pdfx4(&r.bytes));
}

/// Converting colours without image options (the legacy `compressImages` path) passes an RGB
/// JPEG that needs no conversion through instead of re-encoding it losslessly.
#[test]
fn legacy_image_settings_pass_jpegs_through_when_converting() {
    let jpeg = photo(64, 64, image::ImageFormat::Jpeg);
    let mut d = doc();
    place(&mut d, jpeg.clone(), (64, 64), Rect::new(72.0, 72.0, 136.0, 136.0), 64.0);
    // To sRGB keeping numbers: an untagged RGB JPEG is unchanged.
    let output = OutputColor { conversion: ColorConversion::PreserveNumbers, destination: Some(SRGB.into()), include_profile: false, intent: None };
    let r = export(&d, PdfOptions { output, ..Default::default() });
    let head = &jpeg[..jpeg.len().min(200)];
    assert!(r.bytes.windows(head.len()).any(|w| w == head), "the original JPEG data");
    assert_eq!(images(&r.bytes)[0].filter, "DCTDecode");
}
